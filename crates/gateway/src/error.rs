use thiserror::Error;

#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("reading the CA bundle at {path}: {source}")]
    Ca { path: String, source: std::io::Error },
    #[error("the CA bundle at {0} holds no certificates")]
    EmptyCa(String),
    #[error("the device key at {0} holds no private key")]
    Key(String),
    #[error("no reachable gateway for node {0}")]
    UnknownNode(String),
    #[error("tls: {0}")]
    Tls(#[from] rustls::Error),
    #[error("http: {0}")]
    Http(reqwest::Error),
    #[error("gateway answered {status}: {detail}")]
    Status { status: u16, detail: String },
    #[error("websocket: {0}")]
    Socket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("refused: {0}")]
    Refused(String),
    #[error("not a session id: {0}")]
    SessionId(String),
}

impl GatewayError {
    pub fn is_turn_in_flight(&self) -> bool {
        matches!(self, Self::Status { status: 409, detail } if detail.contains("turn_in_flight"))
    }

    pub fn is_missing_route(&self) -> bool {
        matches!(self, Self::Status { status: 404, .. })
    }
}
