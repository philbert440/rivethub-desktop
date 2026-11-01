mod board;
mod context_menu;
mod everything;
mod header;
mod launch;
mod listing;
mod navigate;
mod new_topic;
mod placing;
mod sharing;
mod topic_directory;
mod topic_edit;
mod topic_path;
mod topic_rows;
mod topic_view;

pub use topic_edit::CancelRetitle;

use std::collections::HashMap;
use std::sync::Arc;

use app::Hub;
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement, Render, Rgba,
    Styled, Subscription, Window, actions, div, prelude::*, px,
};
use model::notice::{Notice, NoticeKind};
use model::preview::Previews;
use model::session::SessionSummary;
use model::topics::{FiledBy, FiledConversation, HistoryMarks, Placements, Suggestion, Topic};
use model::visibility::SessionVisibility;
use model::{Altitude, SessionId, TopicId};

use crate::agent_editor::AgentEditor;
use crate::launcher::Launcher;
use crate::memory::Focus;
use crate::menu::OpenMenu;
use crate::plugin::Plugins;
use crate::session_pane::SessionPane;
use crate::side_panel::{DraggedAgent, DraggedSession, PanelEvent, SidePanel};
use crate::topic_rail::{RailEvent, TopicRail};
use crate::{shell, theme::Theme};
use listing::Listing;
use placing::Placing;

actions!(workspace, [ZoomOut, NewSession, ToggleSidePanel, NextNeedingYou]);

const HOUR_MS: i64 = 60 * 60 * 1000;
/// How long a quiet session stays Active.
const ACTIVE_MS: i64 = 24 * HOUR_MS;
const RECENT_MS: i64 = 14 * 24 * HOUR_MS;

/// Asks the shell to show Memory focused on a topic or session.
pub struct ViewInMemory(pub Focus);

/// Tells the shell a topic was deleted here, so other pages can let go of it.
pub struct TopicDeleted(pub TopicId);

/// What the workspace listed last, so Memory can name sessions and topics without listing them again.
#[derive(Debug, Default)]
pub struct Listed {
    pub sessions: Vec<SessionSummary>,
    pub topics: Vec<Topic>,
    pub placements: Placements,
    pub filed_by: HashMap<SessionId, FiledBy>,
    pub conversations: Vec<FiledConversation>,
}

/// Whether the sessions have been listed yet: until then an empty page is still loading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Arrival {
    Awaited,
    Arrived,
}

/// Where the topic screen stands in the open topic.
#[derive(Debug, Default)]
struct TopicScreen {
    /// The subtopic stepped into, if any.
    step: Option<TopicId>,
    /// Whether History lists all its sessions rather than its newest few.
    all_history: bool,
    /// Where all of a long History stands in its own box.
    history_scroll: gpui::UniformListScrollHandle,
    /// The pill last stepped into and when: a second click soon after, which lands on the row of
    /// its subtopics, is the pill's double-click and renames it.
    stepped: Option<(TopicId, std::time::Instant)>,
}

pub struct Workspace {
    focus: FocusHandle,
    hub: Hub,
    altitude: Altitude,
    sessions: Vec<SessionSummary>,
    arrival: Arrival,
    topics: Vec<Topic>,
    placements: Placements,
    marks: HistoryMarks,
    filed_by: HashMap<SessionId, FiledBy>,
    visibility: HashMap<SessionId, SessionVisibility>,
    /// Placed sessions as rh remembers them, so a topic's History reaches past what nodes list.
    remembered: Vec<SessionSummary>,
    /// The listed sessions, then the remembered ones no node lists, for the board and topic screen.
    filed_sessions: Vec<SessionSummary>,
    conversations: Vec<FiledConversation>,
    screen: TopicScreen,
    naming: Option<new_topic::Naming>,
    retitling: Option<topic_edit::Retitling>,
    rooting: Option<topic_directory::Rooting>,
    choosing: Option<topic_edit::Choosing>,
    suggestions: HashMap<SessionId, Suggestion>,
    subagents: HashMap<SessionId, usize>,
    uncategorized_open: bool,
    uncategorized_folded: bool,
    /// What the last listing couldn't show, and why.
    unlisted: Option<String>,
    topics_problem: Option<String>,
    listing: Listing,
    /// Why the last thing the person asked for failed; reloads keep it, their next request clears it.
    failed: Option<String>,
    pane: Option<Entity<SessionPane>>,
    pane_events: Option<Subscription>,
    panel: Entity<SidePanel>,
    panel_open: bool,
    placing: Option<Placing>,
    launching: Option<(Entity<Launcher>, Subscription)>,
    editing_agent: Option<(Entity<AgentEditor>, Subscription)>,
    menu: Option<OpenMenu>,
    plugins: Entity<Plugins>,
    feed: app::WorkspaceFeed,
    _panel_events: Subscription,
    rail: Entity<TopicRail>,
    previews: Previews,
    _rail_events: Subscription,
    _feed: [gpui::Task<()>; 3],
}

/// A session worth a desktop notification, with its title when this node's listing has it.
pub struct Attention {
    pub notice: Notice,
    pub title: Option<String>,
}

impl EventEmitter<ViewInMemory> for Workspace {}
impl EventEmitter<Attention> for Workspace {}
impl EventEmitter<TopicDeleted> for Workspace {}
impl EventEmitter<Arc<Listed>> for Workspace {}

impl Workspace {
    pub fn new(hub: Hub, plugins: Entity<Plugins>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let panel = cx.new(|cx| SidePanel::new(hub.clone(), cx));
        let rail = cx.new(|cx| TopicRail::new(hub.clone(), plugins.clone(), cx));
        let rail_events = cx.subscribe_in(
            &rail,
            window,
            |this, _, event: &RailEvent, window, cx| match event {
                RailEvent::OpenSession(summary) => this.open((**summary).clone(), window, cx),
                RailEvent::ViewInMemory(focus) => cx.emit(ViewInMemory(focus.clone())),
            },
        );
        let panel_events =
            cx.subscribe_in(
                &panel,
                window,
                |this, _, event: &PanelEvent, window, cx| match event {
                    PanelEvent::Open(summary) => this.open(summary.clone(), window, cx),
                    PanelEvent::OpenTopic(topic) => this.open_topic(topic.clone(), window, cx),
                    PanelEvent::Start(agent) => this.start_with(agent.clone(), None, window, cx),
                    PanelEvent::NewAgent => this.edit_agent(None, window, cx),
                    PanelEvent::EditAgent(agent) => this.edit_agent(Some(agent), window, cx),
                    PanelEvent::Renamed(renamed) => this.renamed(renamed, cx),
                    PanelEvent::TopicMenu(topic, at) => {
                        this.open_menu(Focus::Topic(topic.clone()), *at, window, cx);
                    }
                    PanelEvent::SessionMenu(session, at) => {
                        this.open_menu(Focus::Session(Box::new(session.clone())), *at, window, cx);
                    }
                },
            );
        let feed = hub.watch_workspace();
        let followed = Self::follow(&feed, cx);
        let mut workspace = Self {
            focus: cx.focus_handle(),
            hub,
            altitude: Altitude::Everything,
            sessions: Vec::new(),
            arrival: Arrival::Awaited,
            topics: Vec::new(),
            placements: Placements::new(),
            marks: HistoryMarks::new(),
            filed_by: HashMap::new(),
            visibility: HashMap::new(),
            remembered: Vec::new(),
            filed_sessions: Vec::new(),
            conversations: Vec::new(),
            screen: TopicScreen::default(),
            naming: None,
            retitling: None,
            rooting: None,
            choosing: None,
            suggestions: HashMap::new(),
            subagents: HashMap::new(),
            uncategorized_open: false,
            uncategorized_folded: false,
            unlisted: None,
            topics_problem: None,
            listing: Listing::Idle,
            failed: None,
            pane: None,
            pane_events: None,
            panel,
            panel_open: true,
            placing: None,
            launching: None,
            editing_agent: None,
            menu: None,
            plugins,
            feed,
            _panel_events: panel_events,
            rail,
            previews: Previews::new(),
            _rail_events: rail_events,
            _feed: followed,
        };
        workspace.load(cx);
        workspace
    }

    /// Follows the feed: a session change lists again, live state repaints, and a topic change
    /// reads the topics.
    fn follow(feed: &app::WorkspaceFeed, cx: &mut Context<Self>) -> [gpui::Task<()>; 3] {
        let mut changes = feed.changes();
        let refresh = cx.spawn(async move |this, cx| {
            while changes.changed().await.is_ok() {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(750))
                    .await;
                changes.borrow_and_update();
                if this.update(cx, Workspace::load).is_err() {
                    break;
                }
            }
        });
        let mut activity = feed.activity();
        let repaint = cx.spawn(async move |this, cx| {
            while activity.changed().await.is_ok() {
                // Live waits and finishes don't reload the listing, so the History chips
                // catch up here.
                let synced = this.update(cx, |this, cx| {
                    this.take_notices(cx);
                    this.sync_panel(cx);
                });
                if synced.is_err() {
                    break;
                }
            }
        });
        let mut topic_changes = feed.topic_changes();
        let retopic = cx.spawn(async move |this, cx| {
            while topic_changes.changed().await.is_ok() {
                if this.update(cx, Workspace::topics_changed).is_err() {
                    break;
                }
            }
        });
        [refresh, repaint, retopic]
    }

    fn take_notices(&mut self, cx: &mut Context<Self>) {
        for notice in self.feed.take_notices() {
            let title = self
                .sessions
                .iter()
                .find(|session| session.id == notice.session)
                .map(SessionSummary::name);
            cx.emit(Attention { notice, title });
        }
        cx.notify();
    }

    /// A terminal's bell asks for the person the way a blocked session does.
    fn rang(&self, session: SessionId, cx: &mut Context<Self>) {
        let title = self
            .sessions
            .iter()
            .find(|known| known.id == session)
            .map(SessionSummary::name);
        let notice = Notice {
            kind: NoticeKind::NeedsYou,
            session,
        };
        cx.emit(Attention { notice, title });
    }

    /// A node said topics changed, such as a decision added from somewhere else: the topics are
    /// read again, and the rail its decisions when it shows one of them.
    fn topics_changed(&mut self, cx: &mut Context<Self>) {
        let changed = self.feed.take_changed_topics();
        if changed.is_empty() {
            return;
        }
        self.rail.update(cx, |rail, cx| rail.topics_changed(&changed, cx));
        self.refile(cx);
    }

    /// Settings added, changed or removed an agent, so the agent list reads them again.
    pub fn agents_changed(&mut self, cx: &mut Context<Self>) {
        self.panel.update(cx, |panel, cx| panel.load_agents(cx));
    }

    /// The session open in the pane, if the workspace is showing one.
    pub fn on_screen(&self) -> Option<&SessionId> {
        match &self.altitude {
            Altitude::Session { session, .. } => Some(session),
            Altitude::Everything | Altitude::Topic(_) => None,
        }
    }

    /// What a session runs, by the label its node offers for the model, else the model's id, else
    /// the harness.
    fn model_name(&self, session: &SessionSummary) -> String {
        match session.model.as_deref() {
            Some(model) => self
                .hub
                .model_label(session.node.as_deref(), session.id.harness(), model)
                .unwrap_or_else(|| model.to_owned()),
            None => session.id.harness().display_name().to_owned(),
        }
    }

    fn topic(&self, id: &TopicId) -> Option<&Topic> {
        self.topics.iter().find(|topic| &topic.id == id)
    }

    fn dot(&self, session: &SessionSummary) -> Rgba {
        if self.feed.needs_you(session) {
            Theme::warn()
        } else if self.feed.is_active(session) {
            Theme::accent()
        } else {
            Theme::ink_dim()
        }
    }

    fn needs_you_chip() -> impl IntoElement {
        div()
            .flex_none()
            .px_2()
            .rounded(px(9.))
            .bg(Theme::warn_container())
            .text_xs()
            .text_color(Theme::warn())
            .child("Needs you")
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let in_session = matches!(self.altitude, Altitude::Session { .. });
        let content = match (&self.altitude, &self.pane) {
            (Altitude::Session { .. }, Some(pane)) => pane.clone().into_any_element(),
            (Altitude::Topic(topic), _) => self.topic_view(&topic.clone(), cx),
            _ => self.everything(cx),
        };
        let tile = shell::tile()
            .id("everything-drop")
            .when(self.altitude == Altitude::Everything, |tile| {
                tile.drag_over::<DraggedAgent>(|style, _, _, _| style.bg(Theme::selected()))
                    .drag_over::<DraggedSession>(|style, _, _, _| style.bg(Theme::selected()))
                    .on_drop(cx.listener(|this, dragged: &DraggedAgent, window, cx| {
                        this.start_with(dragged.0.clone(), None, window, cx);
                    }))
                    .on_drop(cx.listener(|this, dragged: &DraggedSession, window, cx| {
                        this.open(dragged.0.clone(), window, cx);
                    }))
            })
            .child(content);
        div()
            .relative()
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::new_session))
            .on_action(cx.listener(Self::toggle_panel))
            .on_action(cx.listener(Self::next_needing_you))
            .size_full()
            .flex()
            .flex_col()
            .child(self.header(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(tile)
                    .when(!in_session && self.panel_open, |row| {
                        row.child(
                            div()
                                .mb_3()
                                .mr_3()
                                .min_h_0()
                                .rounded(px(20.))
                                .overflow_hidden()
                                .child(self.panel.clone()),
                        )
                    }),
            )
            .children(self.launcher(cx))
            .children(self.editing_agent.as_ref().map(|(editor, _)| {
                div()
                    .absolute()
                    .inset_0()
                    .occlude()
                    .bg(Theme::scrim())
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(editor.clone())
            }))
            .children(self.menu.as_ref().map(OpenMenu::overlay))
            .children(self.choosing_overlay(cx))
    }
}
