use thiserror::Error;

#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("reading the CA bundle at {path}: {source}")]
    Ca { path: String, source: std::io::Error },
    #[error("the CA bundle at {0} holds no certificates")]
    EmptyCa(String),
    #[error("the device key at {0} holds no private key")]
    Key(String),
    #[error("the certificate at {0} cannot be read")]
    Certificate(String),
    #[error("no reachable gateway for node {0}")]
    UnknownNode(String),
    #[error("{0} didn't answer in time")]
    TimedOut(String),
    #[error("tls: {0}")]
    Tls(#[from] rustls::Error),
    #[error("http: {0}")]
    Http(reqwest::Error),
    #[error("gateway answered {status}: {detail}")]
    Status { status: u16, detail: String },
    #[error("websocket: {0}")]
    Socket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("{0}")]
    Conflict(String),
    #[error("refused: {0}")]
    Refused(String),
    /// A plug-in couldn't answer, or rh refused what it answered; `detail` is rh's word on it,
    /// such as the field it refused.
    #[error("the plug-in {why}{}", .detail.as_ref().map_or_else(String::new, |detail| format!(": {detail}")))]
    Plugin {
        why: &'static str,
        detail: Option<String>,
    },
    #[error("not a session id: {0}")]
    SessionId(String),
}

impl GatewayError {
    /// What went wrong, in rh's own words when it answered with one, rather than its whole reply.
    pub fn reason(&self) -> String {
        match self {
            Self::Status { detail, .. } => crate::topics::rh_message(detail.clone()),
            other => other.to_string(),
        }
    }

    pub fn is_turn_in_flight(&self) -> bool {
        matches!(self, Self::Status { status: 409, detail } if detail.contains("turn_in_flight"))
    }

    pub fn is_missing_route(&self) -> bool {
        matches!(self, Self::Status { status: 404, .. })
    }

    /// The node answered, but won't take requests from this device, which isn't paired with it.
    pub fn is_refused_device(&self) -> bool {
        matches!(
            self,
            Self::Status {
                status: 401 | 403,
                ..
            }
        )
    }

    /// Nothing answered: the node is down, gone from the network, or never listened there.
    pub fn is_unreachable(&self) -> bool {
        match self {
            Self::TimedOut(_)
            | Self::UnknownNode(_)
            | Self::Socket(tokio_tungstenite::tungstenite::Error::Io(_)) => true,
            Self::Http(err) => err.is_connect() || err.is_timeout(),
            _ => false,
        }
    }
}
