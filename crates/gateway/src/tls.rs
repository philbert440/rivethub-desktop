use std::path::Path;

use rustls::{ClientConfig, RootCertStore};
use rustls_pki_types::{CertificateDer, pem::PemObject};

use crate::GatewayError;

pub fn client_config(ca_pem: &Path) -> Result<ClientConfig, GatewayError> {
    let shown = ca_pem.display().to_string();
    let bytes = std::fs::read(ca_pem).map_err(|source| GatewayError::Ca {
        path: shown.clone(),
        source,
    })?;
    let mut roots = RootCertStore::empty();
    for cert in CertificateDer::pem_slice_iter(&bytes).flatten() {
        roots.add(cert)?;
    }
    if roots.is_empty() {
        return Err(GatewayError::EmptyCa(shown));
    }
    let provider = std::sync::Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    Ok(ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth())
}
