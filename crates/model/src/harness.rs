use crate::HarnessId;

/// A harness a node offers, as `GET /api/harnesses` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessOffer {
    pub id: HarnessId,
    pub chat: bool,
    pub lists_sessions: bool,
    pub models: Vec<ModelChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChoice {
    pub id: String,
    pub label: String,
    pub default: bool,
}
