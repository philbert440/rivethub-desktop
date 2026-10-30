use std::sync::Arc;
use std::time::Duration;

use rustls::ClientConfig;
use tokio::net::TcpStream;
use tokio::time::{Instant, Interval, MissedTickBehavior};
use tokio_tungstenite::{
    Connector, MaybeTlsStream, WebSocketStream, connect_async_tls_with_config, tungstenite::Message,
};

use crate::GatewayError;

pub type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// How long opening a socket may take before the node counts as out of reach.
const HANDSHAKE: Duration = Duration::from_secs(10);
/// How often an open socket pings, so a node that is gone shows as gone.
const PING_EVERY: Duration = Duration::from_secs(15);
/// How long a socket may hear nothing, not even a pong, before it counts as dropped. A node that
/// falls off the network without closing its connections leaves them open but silent.
const SILENT_FOR: Duration = Duration::from_secs(35);

pub async fn connect(url: String, tls: Arc<ClientConfig>) -> Result<Socket, GatewayError> {
    let host = url
        .split_once("://")
        .map_or(url.as_str(), |(_, rest)| rest)
        .split('/')
        .next()
        .unwrap_or_default()
        .to_owned();
    let handshake = connect_async_tls_with_config(url, None, false, Some(Connector::Rustls(tls)));
    match tokio::time::timeout(HANDSHAKE, handshake).await {
        Ok(connected) => Ok(connected?.0),
        Err(_) => Err(GatewayError::TimedOut(host)),
    }
}

/// When an open socket last heard from its node, and the pings that keep asking.
#[derive(Debug)]
pub struct Liveness {
    heard: Instant,
    pings: Interval,
}

impl Liveness {
    pub fn new() -> Self {
        let mut pings = tokio::time::interval_at(Instant::now() + PING_EVERY, PING_EVERY);
        pings.set_missed_tick_behavior(MissedTickBehavior::Delay);
        Self {
            heard: Instant::now(),
            pings,
        }
    }

    pub fn heard(&mut self) {
        self.heard = Instant::now();
    }

    /// Waits until the next ping is due: true to send it, false once the node has been silent
    /// too long to count as there.
    pub async fn due(&mut self) -> bool {
        self.pings.tick().await;
        self.heard.elapsed() < SILENT_FOR
    }
}

pub fn ping() -> Message {
    Message::Ping(Vec::new().into())
}
