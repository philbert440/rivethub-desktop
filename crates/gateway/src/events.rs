use std::sync::Arc;

use futures::StreamExt;
use model::{HarnessId, NativeSessionId, SessionId};
use rustls::ClientConfig;
use tokio::sync::mpsc;
use tokio_tungstenite::{Connector, connect_async_tls_with_config, tungstenite::Message};
use wire::events::{Activity, AgentEventKind, SessionBroadcast};

use crate::GatewayError;

const BUFFER: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceEvent {
    Snapshot(Vec<SessionId>),
    Changed,
    Activity { native: String, working: bool },
    Closed,
}

fn harness_name(harness: Option<&wire::harness::SessionHarness>) -> String {
    harness
        .and_then(|harness| serde_json::to_value(harness).ok())
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn decode(text: &str) -> Option<WorkspaceEvent> {
    match serde_json::from_str::<SessionBroadcast>(text).ok()? {
        SessionBroadcast::Snapshot(snapshot) => Some(WorkspaceEvent::Snapshot(
            snapshot
                .sessions
                .into_iter()
                .map(|live| {
                    SessionId::new(
                        HarnessId::new(harness_name(live.harness.as_ref())),
                        NativeSessionId::new(live.id),
                    )
                })
                .collect(),
        )),
        SessionBroadcast::Removed(_) => Some(WorkspaceEvent::Changed),
        SessionBroadcast::Event(event) => match event.kind {
            AgentEventKind::Activity { activity, .. } => Some(WorkspaceEvent::Activity {
                native: event.session,
                working: !matches!(activity, Activity::Idle | Activity::Sleeping),
            }),
            AgentEventKind::SessionStart { .. }
            | AgentEventKind::SessionEnd { .. }
            | AgentEventKind::TurnEnd { .. } => Some(WorkspaceEvent::Changed),
            _ => None,
        },
    }
}

pub async fn watch(
    base_url: &str,
    tls: Arc<ClientConfig>,
) -> Result<mpsc::Receiver<WorkspaceEvent>, GatewayError> {
    let ws_base = base_url
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    let (socket, _) =
        connect_async_tls_with_config(format!("{ws_base}/ws"), None, false, Some(Connector::Rustls(tls)))
            .await?;
    let (_, mut source) = socket.split();
    let (events, receiver) = mpsc::channel(BUFFER);
    tokio::spawn(async move {
        while let Some(Ok(frame)) = source.next().await {
            let Message::Text(text) = frame else { continue };
            if let Some(event) = decode(&text)
                && events.send(event).await.is_err()
            {
                return;
            }
        }
        let _ = events.send(WorkspaceEvent::Closed).await;
    });
    Ok(receiver)
}
