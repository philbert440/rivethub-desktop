use std::path::Path;

use rustls::{ClientConfig, RootCertStore};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use x509_parser::prelude::{FromDer, X509Certificate, X509Name};

use crate::{GatewayError, Identity};

pub fn client_config(ca_pem: &Path, identity: Option<&Identity>) -> Result<ClientConfig, GatewayError> {
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
    let builder = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots);
    let Some(identity) = identity else {
        return Ok(builder.with_no_client_auth());
    };
    let read = |path: &Path| {
        std::fs::read(path).map_err(|source| GatewayError::Ca {
            path: path.display().to_string(),
            source,
        })
    };
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(&read(&identity.cert_pem)?)
        .flatten()
        .collect();
    let key = PrivateKeyDer::from_pem_slice(&read(&identity.key_pem)?)
        .map_err(|_| GatewayError::Key(identity.key_pem.display().to_string()))?;
    Ok(builder.with_client_auth_cert(chain, key)?)
}

/// The certificates in a PEM file, leaf first.
pub fn certificates(path: &Path) -> Result<Vec<model::trust::Certificate>, GatewayError> {
    let shown = path.display().to_string();
    let bytes = std::fs::read(path).map_err(|source| GatewayError::Ca {
        path: shown.clone(),
        source,
    })?;
    CertificateDer::pem_slice_iter(&bytes)
        .map(|der| {
            let der = der.map_err(|_| GatewayError::Certificate(shown.clone()))?;
            let (_, cert) =
                X509Certificate::from_der(&der).map_err(|_| GatewayError::Certificate(shown.clone()))?;
            Ok(model::trust::Certificate {
                subject: common_name(cert.subject()),
                issuer: common_name(cert.issuer()),
                not_after_ms: cert.validity().not_after.timestamp().saturating_mul(1000),
            })
        })
        .collect()
}

fn common_name(name: &X509Name<'_>) -> String {
    name.iter_common_name()
        .next()
        .and_then(|cn| cn.as_str().ok())
        .map_or_else(|| name.to_string(), str::to_owned)
}
