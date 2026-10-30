use model::pairing::{PairingAvailability, PairingCode, PairingState};
use serde::Deserialize;
use wire::gateway::devices::{
    PhonePairingAvailability, PhonePairingCode, PhonePairingRequest, PhonePairingState, PhonePairingStatus,
};

use crate::{Gateway, GatewayError};

/// rh's error body.
#[derive(Deserialize)]
struct Refusal {
    error: String,
}

impl Gateway {
    /// Whether this node can pair a phone. A node that won't, because pairing is off, the caller
    /// isn't its owner or its rh predates pairing, says why rather than failing.
    pub async fn phone_pairing(&self) -> Result<PairingAvailability, GatewayError> {
        let asked: Result<PhonePairingAvailability, GatewayError> = self
            .get_json(&format!("{}/api/phone-pairing", self.base_url))
            .await;
        match asked {
            Ok(PhonePairingAvailability {
                available: true,
                gateway: Some(gateway),
                ..
            }) => Ok(PairingAvailability::Available { gateway }),
            Ok(other) => Ok(PairingAvailability::Unavailable {
                reason: other.reason.unwrap_or_else(|| "this node gave no reason".into()),
            }),
            Err(GatewayError::Status { status: 404, .. }) => Ok(PairingAvailability::Unavailable {
                reason: "this node's rh can't pair phones".into(),
            }),
            Err(GatewayError::Status {
                status: 403 | 503,
                detail,
            }) => Ok(PairingAvailability::Unavailable {
                reason: refusal(detail),
            }),
            Err(err) => Err(err),
        }
    }

    /// Mints a one-time code for a phone named `name`.
    pub async fn start_pairing(&self, name: &str) -> Result<PairingCode, GatewayError> {
        let url = format!("{}/api/phone-pairing", self.base_url);
        let body = PhonePairingRequest {
            name: name.to_owned(),
            host: None,
        };
        let code: Result<PhonePairingCode, GatewayError> =
            Self::read_json(self.http.post(url).json(&body).send().await)?.await;
        let code = code.map_err(|err| match err {
            GatewayError::Status { detail, .. } => GatewayError::Refused(refusal(detail)),
            other => other,
        })?;
        Ok(PairingCode {
            name: name.to_owned(),
            device_id: code.device_id,
            gateway: code.gateway,
            expires_at_ms: code.expires_at.timestamp_millis(),
            qr_text: code.qr_text,
        })
    }

    /// Whether the phone has used the code for `device_id` yet.
    pub async fn pairing_state(&self, device_id: &str) -> Result<PairingState, GatewayError> {
        let url = self.api_url(&["api", "phone-pairing", device_id])?;
        let status: PhonePairingStatus = Self::read_json(self.http.get(url).send().await)?.await?;
        Ok(match status.state {
            PhonePairingState::Pending => PairingState::Pending,
            PhonePairingState::Paired => PairingState::Paired,
            PhonePairingState::Expired => PairingState::Expired,
        })
    }
}

/// rh's reason from an error body, or the body itself when it isn't one.
fn refusal(detail: String) -> String {
    serde_json::from_str::<Refusal>(&detail).map_or(detail, |refusal| refusal.error)
}
