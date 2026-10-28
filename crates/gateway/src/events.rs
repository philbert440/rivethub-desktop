use std::sync::Arc;

use futures::StreamExt;
use model::SessionId;
use model::session::SessionStatus;
use rustls::ClientConfig;
use tokio::sync::mpsc;
use tokio_tungstenite::{Connector, connect_async_tls_with_config, tungstenite::Message};
use wire::events::{Activity, AgentEventKind, SessionBroadcast};
use wire::harness::HarnessEvent;

use crate::{GatewayError, translate};

const BUFFER: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceEvent {
    Changed,
    Activity {
        native: String,
        working: bool,
    },
    /// A session's status, and whether it waits on the person when rh says.
    Updated {
        id: SessionId,
        status: SessionStatus,
        blocked: Option<bool>,
    },
    Closed,
}

fn broadcast(text: &str) -> Option<WorkspaceEvent> {
    match serde_json::from_str::<SessionBroadcast>(text).ok()? {
        SessionBroadcast::Snapshot(_) | SessionBroadcast::Removed(_) => Some(WorkspaceEvent::Changed),
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
        SessionBroadcast::HubState(_) => None,
    }
}

fn registry(text: &str) -> Option<WorkspaceEvent> {
    match serde_json::from_str::<HarnessEvent>(text).ok()? {
        HarnessEvent::SessionCreated { .. } => Some(WorkspaceEvent::Changed),
        HarnessEvent::SessionUpdated {
            session_id,
            status,
            blocked,
            ..
        } => Some(WorkspaceEvent::Updated {
            id: translate::session(&session_id),
            status: translate::session_status(status),
            blocked,
        }),
        _ => None,
    }
}

pub async fn watch(
    base_url: &str,
    tls: Arc<ClientConfig>,
) -> Result<mpsc::Receiver<WorkspaceEvent>, GatewayError> {
    follow(base_url, "/ws", tls, broadcast).await
}

pub async fn watch_registry(
    base_url: &str,
    tls: Arc<ClientConfig>,
) -> Result<mpsc::Receiver<WorkspaceEvent>, GatewayError> {
    follow(base_url, "/api/harnesses/ws", tls, registry).await
}

async fn follow(
    base_url: &str,
    path: &str,
    tls: Arc<ClientConfig>,
    decode: fn(&str) -> Option<WorkspaceEvent>,
) -> Result<mpsc::Receiver<WorkspaceEvent>, GatewayError> {
    let ws_base = base_url
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    let (socket, _) = connect_async_tls_with_config(
        format!("{ws_base}{path}"),
        None,
        false,
        Some(Connector::Rustls(tls)),
    )
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
