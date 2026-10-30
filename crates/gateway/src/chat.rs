use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use model::session::SessionEvent;
use rustls::ClientConfig;
use tokio::sync::mpsc;
use tokio_tungstenite::{Connector, connect_async_tls_with_config, tungstenite::Message};
use wire::gateway::sessions::{HarnessSocketMessage, HarnessSocketRefusal};
use wire::harness::HarnessEvent;

use crate::{GatewayError, translate};

const EVENT_BUFFER: usize = 256;

#[derive(Debug)]
pub struct SessionStream {
    pub events: mpsc::Receiver<SessionEvent>,
    resync: mpsc::Sender<()>,
}

impl SessionStream {
    pub fn resync(&self) {
        let _ = self.resync.try_send(());
    }
}

pub async fn open(
    base_url: &str,
    tls: Arc<ClientConfig>,
    session: &wire::session_id::SessionId,
) -> Result<SessionStream, GatewayError> {
    let ws_base = base_url
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    let url = format!(
        "{ws_base}/api/harness-sessions/ws?session={}",
        wire::session_id::encode_segment(session)
    );
    let (socket, _) = connect_async_tls_with_config(url, None, false, Some(Connector::Rustls(tls))).await?;
    let (mut sink, mut source) = socket.split();
    let (events_tx, events) = mpsc::channel(EVENT_BUFFER);
    let (resync, mut resync_rx) = mpsc::channel::<()>(1);
    let sync = serde_json::to_string(&HarnessSocketMessage::Sync).unwrap_or_default();
    tokio::spawn(async move {
        let mut running = translate::Running::default();
        loop {
            tokio::select! {
                frame = source.next() => {
                    let Some(Ok(frame)) = frame else { break };
                    let Message::Text(text) = frame else { continue };
                    let Some(event) = decode(&text, &mut running) else { continue };
                    if events_tx.send(event).await.is_err() {
                        break;
                    }
                }
                wanted = resync_rx.recv() => {
                    if wanted.is_none() || sink.send(Message::Text(sync.clone().into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    });
    Ok(SessionStream { events, resync })
}

fn decode(text: &str, running: &mut translate::Running) -> Option<SessionEvent> {
    if let Ok(event) = serde_json::from_str::<HarnessEvent>(text) {
        return translate::event(event, running);
    }
    match serde_json::from_str::<HarnessSocketRefusal>(text) {
        Ok(HarnessSocketRefusal::Error { message, .. }) => Some(SessionEvent::Error(message)),
        Err(err) => {
            tracing::warn!(%err, "unrecognised session frame");
            None
        }
    }
}
