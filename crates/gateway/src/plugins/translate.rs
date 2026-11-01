mod canvas;
mod view;

pub use canvas::{reply, wire_event};
pub use view::{action, value, view, wire_value, wire_values};

use model::plugin::{
    Context, PageEntry, Params, Permission, Plugin, PluginRef, PluginState, SessionAction, TopicCard,
};
use wire::JsonObject;
use wire::plugin::host::{PluginRecord, PluginState as WireState};
use wire::plugin::manifest::Permission as WirePermission;
use wire::plugin::view::ViewContext;

use crate::{GatewayError, translate};
use view::field;

/// A length or position in canvas units; scenes are logical pixels, far inside f32's range.
fn units(value: f64) -> f32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "canvas units are logical pixels, far inside f32's range"
    )]
    let units = value as f32;
    units
}

fn optional_units(value: Option<f64>) -> Option<f32> {
    value.map(units)
}

pub fn plugin(record: PluginRecord) -> Plugin {
    let manifest = record.manifest;
    let contributes = manifest.contributes;
    let events = manifest.events.len();
    Plugin {
        at: PluginRef {
            node: record.node,
            id: record.id.into(),
        },
        name: manifest.name,
        version: manifest.version,
        description: manifest.description,
        icon: manifest.icon,
        state: match record.state {
            WireState::Disabled => PluginState::Disabled,
            WireState::AwaitingApproval => PluginState::AwaitingApproval,
            WireState::Starting => PluginState::Starting,
            WireState::Running => PluginState::Running,
            WireState::Backoff => PluginState::Backoff,
            WireState::Invalid => PluginState::Invalid,
        },
        error: record.error,
        granted: record.granted.into_iter().map(permission).collect(),
        pending: record.pending.into_iter().map(permission).collect(),
        settings: manifest.settings.into_iter().map(field).collect(),
        tools: manifest
            .tools
            .into_iter()
            .map(|tool| tool.title.unwrap_or_else(|| tool.name.into()))
            .collect(),
        pages: contributes
            .pages
            .into_iter()
            .map(|page| PageEntry {
                id: page.id.into(),
                title: page.title,
                icon: page.icon,
                view: page.view.into(),
            })
            .collect(),
        topic_cards: contributes
            .topic_cards
            .into_iter()
            .map(|card| TopicCard {
                id: card.id.into(),
                title: card.title,
                view: card.view.into(),
            })
            .collect(),
        session_actions: contributes
            .session_actions
            .into_iter()
            .map(|entry| SessionAction {
                id: entry.id.into(),
                title: entry.title,
                icon: entry.icon,
                action: action(entry.action),
            })
            .collect(),
        jobs: manifest.jobs.into_iter().map(|job| job.title).collect(),
        events,
    }
}

fn permission(permission: WirePermission) -> Permission {
    match permission {
        WirePermission::MemoryRead => Permission::MemoryRead,
        WirePermission::MemoryWrite => Permission::MemoryWrite,
        WirePermission::SessionsRead => Permission::SessionsRead,
        WirePermission::SessionsControl => Permission::SessionsControl,
        WirePermission::TopicsRead => Permission::TopicsRead,
        WirePermission::TopicsWrite => Permission::TopicsWrite,
        WirePermission::TasksRead => Permission::TasksRead,
        WirePermission::TasksDelegate => Permission::TasksDelegate,
        WirePermission::AgentsRead => Permission::AgentsRead,
        WirePermission::NotificationsSend => Permission::NotificationsSend,
        WirePermission::HostFilesRead => Permission::HostFilesRead,
        WirePermission::HostFilesWrite => Permission::HostFilesWrite,
        WirePermission::HostNetwork => Permission::HostNetwork,
        WirePermission::HostExec => Permission::HostExec,
    }
}

pub fn wire_permission(permission: Permission) -> WirePermission {
    match permission {
        Permission::MemoryRead => WirePermission::MemoryRead,
        Permission::MemoryWrite => WirePermission::MemoryWrite,
        Permission::SessionsRead => WirePermission::SessionsRead,
        Permission::SessionsControl => WirePermission::SessionsControl,
        Permission::TopicsRead => WirePermission::TopicsRead,
        Permission::TopicsWrite => WirePermission::TopicsWrite,
        Permission::TasksRead => WirePermission::TasksRead,
        Permission::TasksDelegate => WirePermission::TasksDelegate,
        Permission::AgentsRead => WirePermission::AgentsRead,
        Permission::NotificationsSend => WirePermission::NotificationsSend,
        Permission::HostFilesRead => WirePermission::HostFilesRead,
        Permission::HostFilesWrite => WirePermission::HostFilesWrite,
        Permission::HostNetwork => WirePermission::HostNetwork,
        Permission::HostExec => WirePermission::HostExec,
    }
}

fn params(params: Option<JsonObject>) -> Option<Params> {
    params.and_then(|params| serde_json::to_string(&params).ok().map(Params::from_json))
}

pub fn wire_params(params: Option<&Params>) -> Result<Option<JsonObject>, GatewayError> {
    params
        .map(|params| {
            serde_json::from_str(params.as_json())
                .map_err(|err| GatewayError::Refused(format!("a plug-in's params: {err}")))
        })
        .transpose()
}

pub fn wire_context(context: &Context) -> Result<ViewContext, GatewayError> {
    Ok(ViewContext {
        topic_id: context.topic.as_ref().map(|topic| topic.as_str().to_owned()),
        session_id: context
            .session
            .as_ref()
            .map(translate::wire_session)
            .transpose()?,
    })
}
