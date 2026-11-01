pub mod board;
pub mod canvas;
pub mod panel;
pub mod view;

use std::collections::BTreeMap;

use crate::{SessionId, TopicId};

/// A plug-in as one node runs it: a plug-in is installed on one node and reached through it, so
/// the same id on two nodes is two plug-ins.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PluginRef {
    pub node: String,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginState {
    Disabled,
    AwaitingApproval,
    Starting,
    Running,
    Backoff,
    /// The manifest it was installed from is gone or no longer valid.
    Invalid,
}

impl PluginState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Off",
            Self::AwaitingApproval => "Needs approval",
            Self::Starting => "Starting",
            Self::Running => "Running",
            Self::Backoff => "Restarting",
            Self::Invalid => "Broken",
        }
    }

    /// Whether its node means to run it, so what it adds to the app belongs on screen.
    pub fn is_live(self) -> bool {
        matches!(self, Self::Starting | Self::Running | Self::Backoff)
    }
}

/// What a plug-in may do through rh. The `Host*` ones record what the person agreed to; rh can't
/// hold a plug-in to them, since it runs as the node's user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Permission {
    MemoryRead,
    MemoryWrite,
    SessionsRead,
    SessionsControl,
    TopicsRead,
    TopicsWrite,
    TasksRead,
    TasksDelegate,
    AgentsRead,
    NotificationsSend,
    HostFilesRead,
    HostFilesWrite,
    HostNetwork,
    HostExec,
}

impl Permission {
    pub fn describe(self) -> &'static str {
        match self {
            Self::MemoryRead => "Search and read memory and the wiki",
            Self::MemoryWrite => "Write to memory",
            Self::SessionsRead => "See sessions and their transcripts",
            Self::SessionsControl => "Start, steer and stop sessions",
            Self::TopicsRead => "See topics and where sessions are filed",
            Self::TopicsWrite => "Change topics and file sessions",
            Self::TasksRead => "See tasks",
            Self::TasksDelegate => "Delegate tasks",
            Self::AgentsRead => "See agent presets",
            Self::NotificationsSend => "Send you notifications",
            Self::HostFilesRead => "Read files on the node",
            Self::HostFilesWrite => "Change files on the node",
            Self::HostNetwork => "Use the network",
            Self::HostExec => "Run other programs",
        }
    }

    /// What a host permission means for the sandbox rh runs the plug-in in; the others are
    /// checked on every call it makes to rh.
    pub fn sandbox(self) -> Option<&'static str> {
        match self {
            Self::HostFilesRead | Self::HostFilesWrite | Self::HostNetwork => {
                Some("Widens the sandbox it runs in, which otherwise holds it to its own folder.")
            }
            Self::HostExec => Some("What it runs stays in its sandbox."),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plugin {
    pub at: PluginRef,
    pub name: String,
    pub version: String,
    pub description: String,
    pub icon: Option<String>,
    pub state: PluginState,
    pub error: Option<String>,
    pub granted: Vec<Permission>,
    pub pending: Vec<Permission>,
    pub settings: Vec<view::Field>,
    /// The agent tools it gives every harness on its node, by title.
    pub tools: Vec<String>,
    pub pages: Vec<PageEntry>,
    pub topic_cards: Vec<TopicCard>,
    pub session_actions: Vec<SessionAction>,
    /// The jobs rh runs for it, by title.
    pub jobs: Vec<String>,
    /// How many kinds of event it follows.
    pub events: usize,
}

/// What one node says it has installed.
#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    pub node: String,
    pub plugins: Vec<Plugin>,
}

/// A plug-in's settings as its node keeps them; a secret's value is never sent back, only that
/// it is set.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settings {
    pub values: BTreeMap<String, view::Value>,
    pub secrets_set: Vec<String>,
}

/// Which of a plug-in's streams a log line came from; `Host` is rh's own word about it: starts,
/// exits and answers it refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
    Host,
}

/// One line of a plug-in's log, as its node kept it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub at_ms: i64,
    pub stream: LogStream,
    pub text: String,
}

/// A sidebar entry opening `view` as a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageEntry {
    pub id: String,
    pub title: String,
    pub icon: Option<String>,
    pub view: String,
}

/// A view drawn as a card in the topic rail, about the topic shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicCard {
    pub id: String,
    pub title: String,
    pub view: String,
}

/// A session-menu entry running `action` on the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionAction {
    pub id: String,
    pub title: String,
    pub icon: Option<String>,
    pub action: view::Action,
}

/// A page of a plug-in: one of its sidebar entries, or `None` for the page a topic card or a
/// session action opens when the plug-in has no entry of its own.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PageKey {
    pub plugin: PluginRef,
    pub entry: Option<String>,
}

/// What a view is about, from where it is shown.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Context {
    pub topic: Option<TopicId>,
    pub session: Option<SessionId>,
}

/// A view's or an action's parameters: a JSON object the plug-in wrote, which the app hands back
/// without reading.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Params(String);

impl Params {
    pub fn from_json(json: String) -> Self {
        Self(json)
    }

    pub fn as_json(&self) -> &str {
        &self.0
    }
}

/// What a node says about its plug-ins on its event stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// A plug-in was installed, changed state, or removed.
    Changed(String),
    /// These views of a plug-in have something new to show.
    Views { plugin: String, views: Vec<String> },
}

/// A media type for a file by its name, for files handed to a plug-in.
pub fn media_type(name: &str) -> &'static str {
    let extension = name.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase());
    match extension.as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("pdf") => "application/pdf",
        Some("json") => "application/json",
        Some("zip") => "application/zip",
        Some("md") => "text/markdown",
        Some("csv") => "text/csv",
        Some("html" | "htm") => "text/html",
        Some("txt" | "log" | "rs" | "ts" | "js" | "py" | "toml" | "yaml" | "yml") => "text/plain",
        _ => "application/octet-stream",
    }
}

/// Whether a file of `media_type` is one a field that `accept`s these types takes: any file when
/// the list is empty, and `image/*` for every image.
pub fn accepts(accept: &[String], media_type: &str) -> bool {
    accept.is_empty()
        || accept.iter().any(|wanted| {
            wanted == media_type
                || wanted
                    .strip_suffix("/*")
                    .is_some_and(|family| media_type.split('/').next() == Some(family))
        })
}
