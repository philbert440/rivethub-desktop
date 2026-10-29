use std::sync::Arc;

use futures::StreamExt;
use model::SessionId;
use model::plugin::Notice;
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

/// What a node's event stream says about its plug-ins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginEvent {
    Notice(Notice),
    Closed,
}

/// A plug-in notice, told apart by its `type`: rh's untagged `SessionBroadcast` reads a
/// `plugin.views` frame as `plugin.changed`, since neither notice checks its tag when read.
fn plugin_notice(text: &str) -> Option<PluginEvent> {
    use wire::plugin::host::{PluginChangedNotice, PluginViewsNotice};
    if !text.contains("\"plugin.") {
        return None;
    }
    let frame: serde_json::Value = serde_json::from_str(text).ok()?;
    let notice = match frame.get("type")?.as_str()? {
        "plugin.changed" => Notice::Changed(
            serde_json::from_value::<PluginChangedNotice>(frame)
                .ok()?
                .plugin
                .into(),
        ),
        "plugin.views" => {
            let views = serde_json::from_value::<PluginViewsNotice>(frame).ok()?;
            Notice::Views {
                plugin: views.plugin.into(),
                views: views.views.into_iter().map(Into::into).collect(),
            }
        }
        _ => return None,
    };
    Some(PluginEvent::Notice(notice))
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
        SessionBroadcast::HubState(_)
        | SessionBroadcast::PluginChanged(_)
        | SessionBroadcast::PluginViews(_) => None,
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
    follow(base_url, "/ws", tls, broadcast, WorkspaceEvent::Closed).await
}

pub async fn watch_registry(
    base_url: &str,
    tls: Arc<ClientConfig>,
) -> Result<mpsc::Receiver<WorkspaceEvent>, GatewayError> {
    follow(
        base_url,
        "/api/harnesses/ws",
        tls,
        registry,
        WorkspaceEvent::Closed,
    )
    .await
}

pub async fn watch_plugins(
    base_url: &str,
    tls: Arc<ClientConfig>,
) -> Result<mpsc::Receiver<PluginEvent>, GatewayError> {
    follow(
        base_url,
        "/api/events/ws",
        tls,
        plugin_notice,
        PluginEvent::Closed,
    )
    .await
}

async fn follow<T: Send + 'static>(
    base_url: &str,
    path: &str,
    tls: Arc<ClientConfig>,
    decode: fn(&str) -> Option<T>,
    closed: T,
) -> Result<mpsc::Receiver<T>, GatewayError> {
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
        let _ = events.send(closed).await;
    });
    Ok(receiver)
}

#[cfg(test)]
mod tests {
    use model::plugin::Notice;

    use super::{PluginEvent, plugin_notice};

    #[test]
    fn plugin_notices_are_told_apart_by_their_type() {
        assert_eq!(
            plugin_notice(r#"{"type":"plugin.views","v":1,"plugin":"workflows","views":["editor"]}"#),
            Some(PluginEvent::Notice(Notice::Views {
                plugin: "workflows".into(),
                views: vec!["editor".into()],
            }))
        );
        assert_eq!(
            plugin_notice(r#"{"type":"plugin.changed","v":1,"plugin":"workflows"}"#),
            Some(PluginEvent::Notice(Notice::Changed("workflows".into())))
        );
    }
}
