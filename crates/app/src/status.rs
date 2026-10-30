use gateway::GatewayError;
use model::health::MemoryHealth;
use model::mesh::Node;
use model::pairing::{PairingAvailability, PairingCode, PairingState};
use model::trust::Trust;

use crate::Hub;

impl Hub {
    pub async fn nodes(&self) -> Result<Vec<Node>, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.mesh().await }).await
    }

    /// This client's own node, once the gateway has named it.
    pub fn local_node(&self) -> Option<String> {
        self.inner.local_node.read().clone()
    }

    pub async fn memory_health(&self) -> Result<MemoryHealth, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.memory_health().await })
            .await
    }

    pub async fn trust(&self) -> Result<Trust, GatewayError> {
        let ca_pem = self.inner.ca_pem.clone();
        let identity = self.inner.identity.clone();
        self.on_runtime(async move {
            let authority = gateway::certificates(&ca_pem)?
                .into_iter()
                .next()
                .ok_or_else(|| GatewayError::EmptyCa(ca_pem.display().to_string()))?;
            let device = match identity {
                Some(identity) => gateway::certificates(&identity.cert_pem)?.into_iter().next(),
                None => None,
            };
            Ok(Trust { device, authority })
        })
        .await
    }

    /// Phone pairing happens on this client's own node, which gives the phone its certificate.
    pub async fn phone_pairing(&self) -> Result<PairingAvailability, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.phone_pairing().await })
            .await
    }

    pub async fn start_pairing(&self, name: String) -> Result<PairingCode, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.start_pairing(&name).await })
            .await
    }

    pub async fn pairing_state(&self, device_id: String) -> Result<PairingState, GatewayError> {
        let gateway = self.inner.local.clone();
        self.on_runtime(async move { gateway.pairing_state(&device_id).await })
            .await
    }
}
