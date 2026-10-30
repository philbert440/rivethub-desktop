mod everything;
mod new_topic;
mod topic_edit;
mod topic_path;
mod topic_rows;
mod topic_view;

pub use topic_edit::CancelRetitle;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use app::Hub;
use gateway::GatewayError;
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Pixels, Point, Render, Rgba, SharedString, Styled, Subscription, Window, actions, div,
    prelude::*, px,
};
use model::history::Phase;
use model::notice::{Notice, NoticeKind};
use model::prefs::SessionView;
use model::preview::Previews;
use model::session::SessionSummary;
use model::shelf::Shelf;
use model::topics::{
    Activity, Atlas, Board, Directory, Filing, HistoryMarks, Placements, Suggestion, Topic, Windows,
    in_history,
};
use model::{Altitude, HarnessId, SessionId, TopicId};

use crate::agent_editor::{AgentEditor, AgentEditorEvent};
use crate::launch_choice::Choice;
use crate::launcher::{Launcher, LauncherEvent, Target};
use crate::memory::Focus;
use crate::menu::{Lift, Menu, MenuEvent, OpenMenu};
use crate::session_pane::{PaneEvent, SessionPane};
use crate::side_panel::{DraggedAgent, DraggedSession, PanelEvent, PastTopic, SidePanel};
use crate::text::{now_ms, one_line};
use crate::topic_picker::{PickerEvent, TopicPicker};
use crate::topic_rail::{RailEvent, TopicRail};
use crate::{brand, shell, theme::Theme};
use topic_edit::Pick;

actions!(workspace, [ZoomOut, NewSession, ToggleSidePanel, NextNeedingYou]);

const HOUR_MS: i64 = 60 * 60 * 1000;
/// How long a quiet session stays Active.
const ACTIVE_MS: i64 = 24 * HOUR_MS;
const RECENT_MS: i64 = 14 * 24 * HOUR_MS;

type Loaded = (
    Result<app::AllSessions, GatewayError>,
    Result<Filing, GatewayError>,
);

enum Launch {
    Uncategorized,
    In(TopicId),
    New(String),
}

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
}

/// Where the topic screen stands in the open topic.
#[derive(Debug, Default)]
struct TopicScreen {
    /// The subtopic stepped into, if any.
    step: Option<TopicId>,
    /// Whether History lists all its sessions rather than its newest few.
    all_history: bool,
}

struct Placing {
    session: SessionId,
    picker: Entity<TopicPicker>,
    _events: Subscription,
}

pub struct Workspace {
    focus: FocusHandle,
    hub: Hub,
    altitude: Altitude,
    sessions: Vec<SessionSummary>,
    topics: Vec<Topic>,
    placements: Placements,
    marks: HistoryMarks,
    screen: TopicScreen,
    naming: Option<new_topic::Naming>,
    retitling: Option<topic_edit::Retitling>,
    choosing: Option<topic_edit::Choosing>,
    suggestions: HashMap<SessionId, Suggestion>,
    subagents: HashMap<SessionId, usize>,
    uncategorized_open: bool,
    uncategorized_folded: bool,
    problem: Option<String>,
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
    feed: app::WorkspaceFeed,
    _panel_events: Subscription,
    rail: Entity<TopicRail>,
    previews: Previews,
    _rail_events: Subscription,
    _feed: [gpui::Task<()>; 2],
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
    pub fn new(hub: Hub, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let panel = cx.new(|cx| SidePanel::new(hub.clone(), cx));
        let rail = cx.new(|cx| TopicRail::new(hub.clone(), cx));
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
        let mut changes = feed.changes();
        let refresh = cx.spawn(async move |this, cx| {
            while changes.changed().await.is_ok() {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(750))
                    .await;
                changes.borrow_and_update();
                if this.update(cx, |this, cx| this.load(cx)).is_err() {
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
        let workspace = Self {
            focus: cx.focus_handle(),
            hub,
            altitude: Altitude::Everything,
            sessions: Vec::new(),
            topics: Vec::new(),
            placements: Placements::new(),
            marks: HistoryMarks::new(),
            screen: TopicScreen::default(),
            naming: None,
            retitling: None,
            choosing: None,
            suggestions: HashMap::new(),
            subagents: HashMap::new(),
            uncategorized_open: false,
            uncategorized_folded: false,
            problem: None,
            failed: None,
            pane: None,
            pane_events: None,
            panel,
            panel_open: true,
            placing: None,
            launching: None,
            editing_agent: None,
            menu: None,
            feed,
            _panel_events: panel_events,
            rail,
            previews: Previews::new(),
            _rail_events: rail_events,
            _feed: [refresh, repaint],
        };
        workspace.load(cx);
        workspace
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

    /// The session open in the pane, if the workspace is showing one.
    /// Settings added, changed or removed an agent, so the agent list reads them again.
    pub fn agents_changed(&mut self, cx: &mut Context<Self>) {
        self.panel.update(cx, |panel, cx| panel.load_agents(cx));
    }

    pub fn on_screen(&self) -> Option<&SessionId> {
        match &self.altitude {
            Altitude::Session { session, .. } => Some(session),
            Altitude::Everything | Altitude::Topic(_) => None,
        }
    }

    fn load(&self, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let loaded: Loaded = (hub.all_sessions().await, hub.topics().await);
            this.update(cx, |this, cx| this.loaded(loaded, cx)).ok();
        })
        .detach();
    }

    fn loaded(&mut self, (sessions, topics): Loaded, cx: &mut Context<Self>) {
        let mut problems = Vec::new();
        match sessions {
            Ok(all) => {
                if let Some(nodes) = &all.other_nodes {
                    self.feed.follow_nodes(nodes);
                }
                self.sessions = all.sessions;
                if !all.unreachable.is_empty() {
                    problems.push(format!("couldn't reach {}", all.unreachable.join(", ")));
                }
            }
            Err(err) => problems.push(err.to_string()),
        }
        match topics {
            Ok(filing) => {
                self.topics = filing.topics;
                self.placements = filing.placements;
                self.marks = filing.marks;
            }
            Err(err) if err.is_missing_route() => {
                problems.push("Topics need a newer rh on this node.".to_owned());
            }
            Err(err) => problems.push(format!("topics: {err}")),
        }
        self.problem = (!problems.is_empty()).then(|| problems.join(" · "));
        self.publish(cx);
        if let Some(pane) = &self.pane {
            let open = pane.read(cx).session_id().clone();
            if let Some(latest) = self.sessions.iter().find(|session| session.id == open).cloned() {
                pane.update(cx, |pane, cx| pane.take_listing(&latest, cx));
            }
        }
        self.subagents.clear();
        for parent in self.sessions.iter().filter_map(|session| session.parent.clone()) {
            *self.subagents.entry(parent).or_default() += 1;
        }
        self.sync_panel(cx);
        self.sync_step(cx);
        self.load_suggestions(cx);
        cx.notify();
    }

    /// The listing's problems and the last failed request, for the page to show.
    fn problems(&self) -> Option<String> {
        let parts: Vec<&str> = self
            .failed
            .iter()
            .chain(&self.problem)
            .map(String::as_str)
            .collect();
        (!parts.is_empty()).then(|| parts.join(" · "))
    }

    fn load_suggestions(&self, cx: &mut Context<Self>) {
        let uncategorized: Vec<SessionId> = self
            .board()
            .uncategorized
            .iter()
            .map(|session| session.id.clone())
            .collect();
        if uncategorized.is_empty() {
            return;
        }
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let found = hub.suggestions(uncategorized).await;
            this.update(cx, |this, cx| {
                if let Ok(found) = found {
                    this.suggestions = found;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn accept_suggestion(&mut self, session: SessionId, cx: &mut Context<Self>) {
        let Some(suggestion) = self.suggestions.remove(&session) else {
            return;
        };
        match suggestion.topic.filter(|topic| self.topic(topic).is_some()) {
            Some(topic) => self.place(session, topic, cx),
            None => self.place_in_new(session, suggestion.name, cx),
        }
    }

    fn board(&self) -> Board<'_> {
        let windows = Windows {
            now_ms: now_ms(),
            active_ms: ACTIVE_MS,
            recent_ms: RECENT_MS,
        };
        model::topics::board(
            &self.topics,
            &self.sessions,
            &self.placements,
            &self.marks,
            &self.live(),
            windows,
        )
    }

    /// The sessions that are live: running, waiting on you, or open in the pane.
    fn live(&self) -> HashSet<SessionId> {
        self.sessions
            .iter()
            .filter(|session| {
                self.feed.is_active(session)
                    || self.feed.needs_you(session)
                    || self.on_screen() == Some(&session.id)
            })
            .map(|session| session.id.clone())
            .collect()
    }

    /// The open topic's sessions at `step`, Active then History.
    fn shelf(&self, step: &TopicId) -> Shelf<'_> {
        let live = self.live();
        let activity = Activity {
            now_ms: now_ms(),
            window_ms: ACTIVE_MS,
            live: &live,
            marks: &self.marks,
        };
        Shelf::of(
            &Atlas::new(&self.topics, &self.sessions, &self.placements),
            step,
            &activity,
        )
    }

    /// The part of the open topic the topic screen shows: the subtopic stepped into, else the topic.
    fn step_in(&self, root: &TopicId) -> TopicId {
        let atlas = Atlas::new(&self.topics, &self.sessions, &self.placements);
        self.screen
            .step
            .as_ref()
            .filter(|step| atlas.lineage(step).contains(&root))
            .map_or_else(|| root.clone(), Clone::clone)
    }

    /// Points the rail at the open topic's current step and its sessions, newest first, and asks
    /// what the step's Active sessions are doing now.
    pub(super) fn sync_step(&self, cx: &mut Context<Self>) {
        let Altitude::Topic(root) = &self.altitude else {
            return;
        };
        let step = self.step_in(root);
        let inside = self.shelf(&step);
        let active: Vec<SessionSummary> = inside.active.iter().map(|session| (*session).clone()).collect();
        let mut sessions: Vec<SessionSummary> = active
            .iter()
            .cloned()
            .chain(inside.history.iter().map(|session| (*session).clone()))
            .collect();
        sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at_ms));
        self.rail
            .update(cx, |rail, cx| rail.show(step.clone(), sessions, cx));
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let previews = hub.previews(active).await;
            this.update(cx, |this, cx| {
                let current = match &this.altitude {
                    Altitude::Topic(root) => this.step_in(root) == step,
                    _ => false,
                };
                if current {
                    this.previews = previews;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn sync_panel(&self, cx: &mut Context<Self>) {
        let board = self.board();
        let history: Vec<SessionSummary> = board.history.iter().map(|session| (*session).clone()).collect();
        let phases: HashMap<SessionId, Phase> = history
            .iter()
            .map(|session| {
                let phase = Phase::of(
                    self.feed.needs_you(session),
                    self.feed.is_active(session),
                    session.status.is_finished(),
                );
                (session.id.clone(), phase)
            })
            .collect();
        let past: Vec<PastTopic> = board
            .past
            .iter()
            .map(|card| PastTopic {
                id: card.topic.id.clone(),
                name: card.topic.name.clone(),
                sessions: card.sessions.len() + card.history,
                active_ms: card.active_ms,
            })
            .collect();
        let topics = self.topics.clone();
        let known = self.sessions.clone();
        self.panel.update(cx, |panel, cx| {
            panel.set_sessions(history, phases, known, cx);
            panel.set_topics(topics, past, cx);
        });
    }

    fn home_of(&self, session: &SessionSummary) -> Option<TopicId> {
        let atlas = Atlas::new(&self.topics, &self.sessions, &self.placements);
        atlas.topic_of(session).map(|topic| atlas.root(topic).clone())
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

    fn open(&mut self, summary: SessionSummary, window: &mut Window, cx: &mut Context<Self>) {
        // Rebuilding the pane on show would drop its draft and terminal.
        if self
            .pane
            .as_ref()
            .is_some_and(|pane| pane.read(cx).session_id() == &summary.id)
        {
            return;
        }
        let topic = self.home_of(&summary);
        let session = summary.id.clone();
        // A child session is a transcript inside its parent's, with no terminal of its own.
        let terminal = self.hub.preferences().opens_in == SessionView::Terminal && summary.parent.is_none();
        let pane = cx.new(|cx| SessionPane::new(self.hub.clone(), summary, cx));
        if terminal {
            pane.update(cx, |pane, cx| pane.show_terminal(window, cx));
        } else {
            window.focus(&pane.read(cx).composer().focus_handle(cx), cx);
        }
        self.pane_events = Some(
            cx.subscribe(&pane, |this, pane, event: &PaneEvent, cx| match event {
                PaneEvent::Renamed(renamed) => this.renamed(renamed, cx),
                PaneEvent::Bell => {
                    let session = pane.read(cx).session_id().clone();
                    this.rang(session, cx);
                }
            }),
        );
        self.pane = Some(pane);
        self.placing = None;
        self.altitude = Altitude::Session { topic, session };
        cx.notify();
    }

    /// Tells whoever follows the workspace, Memory among them, what it lists now.
    fn publish(&self, cx: &mut Context<Self>) {
        cx.emit(Arc::new(Listed {
            sessions: self.sessions.clone(),
            topics: self.topics.clone(),
            placements: self.placements.clone(),
        }));
    }

    fn renamed(&mut self, renamed: &SessionSummary, cx: &mut Context<Self>) {
        if let Some(known) = self.sessions.iter_mut().find(|known| known.id == renamed.id) {
            known.title.clone_from(&renamed.title);
        }
        self.publish(cx);
        if let Some(pane) = self
            .pane
            .clone()
            .filter(|pane| pane.read(cx).session_id() == &renamed.id)
        {
            pane.update(cx, |pane, cx| pane.take_listing(renamed, cx));
        }
        self.sync_panel(cx);
        cx.notify();
    }

    /// Opens a session by id when some reachable node lists it.
    pub fn open_listed(&mut self, id: &SessionId, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(summary) = self.sessions.iter().find(|session| &session.id == id).cloned() else {
            return false;
        };
        self.open(summary, window, cx);
        true
    }

    /// Right-clicking opens a menu that can show this topic or session in Memory, or delete the topic.
    fn on_right_click(
        focus: Focus,
        cx: &mut Context<Self>,
    ) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
            this.open_menu(focus.clone(), event.position, window, cx);
            cx.stop_propagation();
        })
    }

    fn open_menu(&mut self, focus: Focus, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.choosing = None;
        let topic = match &focus {
            Focus::Topic(topic) => self.topic(topic).map(|known| {
                (
                    topic.clone(),
                    known.name.clone(),
                    known.parent.clone(),
                    self.closed(known),
                )
            }),
            Focus::Session(_) => None,
        };
        let session = match &focus {
            Focus::Session(session) => {
                let latest = self
                    .sessions
                    .iter()
                    .find(|known| known.id == session.id)
                    .unwrap_or(session.as_ref());
                Some((
                    latest.clone(),
                    self.placements.contains_key(&latest.id),
                    in_history(latest, &self.marks),
                ))
            }
            Focus::Topic(_) => None,
        };
        let delete = topic.as_ref().map(|(id, name, ..)| (id.clone(), name.clone()));
        let mut menu = Menu::new(self.hub.clone(), Some(focus), delete);
        if let Some((id, _, parent, closed)) = topic {
            let atlas = Atlas::new(&self.topics, &self.sessions, &self.placements);
            let movable = self
                .topics
                .iter()
                .any(|under| Some(&under.id) != parent.as_ref() && atlas.can_nest(&id, &under.id));
            let up = parent.map(|parent| Lift(self.topic(&parent).and_then(|parent| parent.parent.clone())));
            menu = menu.editing(id.clone(), up, movable).closing(id, closed);
        }
        if let Some((latest, placed, history)) = session {
            let id = latest.id.clone();
            menu = menu.moving(latest, placed).filing(id, history);
        }
        let menu = cx.new(|_| menu);
        let events = cx.subscribe_in(&menu, window, move |this, menu, event: &MenuEvent, window, cx| {
            if this.menu.as_ref().is_none_or(|open| &open.menu != menu) {
                return;
            }
            this.menu = None;
            match event {
                MenuEvent::ViewInMemory(focus) => cx.emit(ViewInMemory(focus.clone())),
                MenuEvent::Deleted(topic) => {
                    this.topic_deleted(topic, window, cx);
                    cx.emit(TopicDeleted(topic.clone()));
                }
                MenuEvent::Filed => this.load(cx),
                MenuEvent::Rename(topic) => this.begin_retitle(topic.clone(), window, cx),
                MenuEvent::MoveInto(topic) => this.choose(Pick::MoveInto(topic.clone()), at, window, cx),
                MenuEvent::MergeInto(topic) => this.choose(Pick::MergeInto(topic.clone()), at, window, cx),
                MenuEvent::Open(session) => this.open((**session).clone(), window, cx),
                MenuEvent::MoveSession(session) => this.choose(Pick::File(session.clone()), at, window, cx),
                MenuEvent::Unplace(session) => this.unplace(session.clone(), cx),
                MenuEvent::Closed => {}
            }
            cx.notify();
        });
        self.menu = Some(OpenMenu {
            menu,
            at,
            _events: events,
        });
        cx.notify();
    }

    /// Closed, with none of its sessions active since.
    fn closed(&self, topic: &Topic) -> bool {
        let atlas = Atlas::new(&self.topics, &self.sessions, &self.placements);
        let newest = self
            .sessions
            .iter()
            .filter(|session| atlas.within(session, &topic.id))
            .map(|session| session.updated_at_ms)
            .max()
            .unwrap_or(0);
        topic.is_closed(newest)
    }

    /// A deleted topic's sessions are back in Uncategorized; leave the topic if it was open.
    pub fn topic_deleted(&mut self, topic: &TopicId, window: &mut Window, cx: &mut Context<Self>) {
        match &mut self.altitude {
            Altitude::Topic(open) if open == topic => {
                self.leave_to(Altitude::Everything, window, cx);
                return;
            }
            Altitude::Session { topic: home, .. } if home.as_ref() == Some(topic) => *home = None,
            _ => {}
        }
        self.load(cx);
    }

    /// The New session picker over a backdrop; clicking anywhere outside the picker closes it.
    fn launcher(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let (launcher, _) = self.launching.as_ref()?;
        let picker = div()
            .absolute()
            .top(px(52.))
            .right(px(20.))
            .w(px(460.))
            .p_3()
            .rounded(px(18.))
            .bg(Theme::raised())
            .border_1()
            .border_color(Theme::line())
            .flex()
            .flex_col()
            .gap_1()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px_2()
                    .pt_1()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child("Start a session"),
            )
            .child(launcher.clone());
        Some(
            div()
                .id("launcher-backdrop")
                .absolute()
                .inset_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, window, cx| {
                        this.launching = None;
                        window.focus(&this.focus, cx);
                        cx.notify();
                    }),
                )
                .child(picker),
        )
    }

    pub fn open_topic(&mut self, topic: TopicId, window: &mut Window, cx: &mut Context<Self>) {
        self.pane = None;
        self.placing = None;
        self.screen = TopicScreen::default();
        self.retitling = None;
        self.altitude = Altitude::Topic(topic);
        self.sync_step(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn next_needing_you(&mut self, _: &NextNeedingYou, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.on_screen().cloned();
        let feed = &self.feed;
        let next = model::attention::next_needing_you(&self.sessions, current.as_ref(), |session| {
            feed.needs_you(session)
        });
        if let Some(next) = next.filter(|next| current.as_ref() != Some(&next.id)).cloned() {
            self.open(next, window, cx);
        }
    }

    fn begin_placing(&mut self, session: SessionId, window: &mut Window, cx: &mut Context<Self>) {
        let topics = self.topics.clone();
        let picker = cx.new(|cx| TopicPicker::new(topics, None, cx));
        let target = session.clone();
        let events = cx.subscribe(&picker, move |this, _, event: &PickerEvent, cx| {
            this.placing = None;
            match event {
                PickerEvent::Chose(topic) => this.place(target.clone(), topic.clone(), cx),
                PickerEvent::Create(name) => this.place_in_new(target.clone(), name.clone(), cx),
                PickerEvent::NoTopic | PickerEvent::Dismissed => {}
            }
            cx.notify();
        });
        window.focus(&picker.focus_handle(cx), cx);
        self.placing = Some(Placing {
            session,
            picker,
            _events: events,
        });
        cx.notify();
    }

    fn place(&mut self, session: SessionId, topic: TopicId, cx: &mut Context<Self>) {
        self.failed = None;
        self.placements.insert(session.clone(), topic.clone());
        self.sync_panel(cx);
        cx.notify();
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let placed = hub.place(session, topic).await;
            this.update(cx, |this, cx| {
                if let Err(err) = placed {
                    this.failed = Some(format!("placing the session failed: {err}"));
                }
                this.load(cx);
            })
            .ok();
        })
        .detach();
    }

    fn unplace(&mut self, session: SessionId, cx: &mut Context<Self>) {
        self.failed = None;
        self.placements.remove(&session);
        self.sync_panel(cx);
        cx.notify();
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let unplaced = hub.unplace(session).await;
            this.update(cx, |this, cx| {
                if let Err(err) = unplaced {
                    this.failed = Some(format!("taking the session out of its topic failed: {err}"));
                }
                this.load(cx);
            })
            .ok();
        })
        .detach();
    }

    fn place_in_new(&mut self, session: SessionId, name: String, cx: &mut Context<Self>) {
        self.failed = None;
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let created = hub.place_in_new(session, name).await;
            this.update(cx, |this, cx| {
                if let Err(err) = created {
                    this.failed = Some(format!("creating the topic failed: {err}"));
                }
                this.load(cx);
            })
            .ok();
        })
        .detach();
    }

    fn edit_agent(
        &mut self,
        agent: Option<&model::agents::Agent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let hub = self.hub.clone();
        let editor = cx.new(|cx| AgentEditor::new(hub, agent, cx));
        let events = cx.subscribe_in(
            &editor,
            window,
            |this, _, event: &AgentEditorEvent, window, cx| {
                this.editing_agent = None;
                if matches!(event, AgentEditorEvent::Saved | AgentEditorEvent::Deleted) {
                    this.panel.update(cx, |panel, cx| panel.load_agents(cx));
                }
                window.focus(&this.focus, cx);
                cx.notify();
            },
        );
        window.focus(&editor.focus_handle(cx), cx);
        self.editing_agent = Some((editor, events));
        cx.notify();
    }

    fn new_session(&mut self, _: &NewSession, window: &mut Window, cx: &mut Context<Self>) {
        self.launch(window, cx);
    }

    fn start(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.launch(window, cx);
    }

    fn launch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let preset = match &self.altitude {
            Altitude::Topic(root) => self
                .topic(&self.step_in(root))
                .map(|known| (known.id.clone(), known.name.clone())),
            _ => None,
        };
        let topics = self.topics.clone();
        let hub = self.hub.clone();
        let launcher = cx.new(|cx| Launcher::new(&hub, topics, preset, cx));
        let events = cx.subscribe_in(&launcher, window, |this, _, event: &LauncherEvent, window, cx| {
            this.launching = None;
            let LauncherEvent::Start { choice, target } = event else {
                window.focus(&this.focus, cx);
                cx.notify();
                return;
            };
            let launch = match target {
                Target::Uncategorized => Launch::Uncategorized,
                Target::In(topic) => Launch::In(topic.clone()),
                Target::New(name) => Launch::New(name.clone()),
            };
            match (**choice).clone() {
                Choice::Agent(agent) => this.start_agent(agent, launch, window, cx),
                Choice::Harness { node, harness, model } => {
                    this.start_session(launch, node, harness, model, window, cx);
                }
            }
        });
        window.focus(&launcher.focus_handle(cx), cx);
        self.launching = Some((launcher, events));
        cx.notify();
    }

    fn start_session(
        &mut self,
        launch: Launch,
        node: Option<String>,
        harness: HarnessId,
        model: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.failed = None;
        let (topic, node, cwd) = self.start_place(&launch, node);
        let hub = self.hub.clone();
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let started = hub.start_session(node, harness, model, topic, cwd).await;
            cx.update_window(window_handle, |_, window, cx| {
                this.update(cx, |this, cx| this.started(started, launch, window, cx))
                    .ok();
            })
            .ok();
        })
        .detach();
    }

    /// Where a session launched into a topic starts: filed in the topic, so rh hands it the topic's
    /// startup memory, and in the topic's working directory (its own or the nearest above it), on
    /// that directory's node when the directory is local to one.
    fn start_place(
        &self,
        launch: &Launch,
        node: Option<String>,
    ) -> (Option<TopicId>, Option<String>, Option<String>) {
        let Launch::In(topic) = launch else {
            return (None, node, None);
        };
        let atlas = Atlas::new(&self.topics, &self.sessions, &self.placements);
        match atlas.directory(topic) {
            Some(Directory::Local { node, path }) => {
                (Some(topic.clone()), Some(node.clone()), Some(path.clone()))
            }
            Some(Directory::Shared { path }) => (Some(topic.clone()), node, Some(path.clone())),
            None => (Some(topic.clone()), node, None),
        }
    }

    fn start_with(
        &mut self,
        agent: model::agents::Agent,
        topic: Option<TopicId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_agent(agent, topic.map_or(Launch::Uncategorized, Launch::In), window, cx);
    }

    fn start_agent(
        &mut self,
        agent: model::agents::Agent,
        launch: Launch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.failed = None;
        let hub = self.hub.clone();
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let started = hub.start_with(agent).await;
            cx.update_window(window_handle, |_, window, cx| {
                this.update(cx, |this, cx| this.started(started, launch, window, cx))
                    .ok();
            })
            .ok();
        })
        .detach();
    }

    fn started(
        &mut self,
        started: Result<SessionSummary, GatewayError>,
        launch: Launch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match started {
            Ok(summary) => {
                match launch {
                    Launch::In(topic) => self.place(summary.id.clone(), topic, cx),
                    Launch::New(name) => self.place_in_new(summary.id.clone(), name, cx),
                    Launch::Uncategorized => {}
                }
                self.sessions.push(summary.clone());
                self.open(summary, window, cx);
            }
            Err(err) => {
                self.failed = Some(err.to_string());
                cx.notify();
            }
        }
    }

    fn toggle_panel(&mut self, _: &ToggleSidePanel, _: &mut Window, cx: &mut Context<Self>) {
        self.panel_open = !self.panel_open;
        cx.notify();
    }

    fn reveal_panel(&mut self, search: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.panel_open = true;
        if search {
            let query = self.panel.read(cx).query().clone();
            window.focus(&query.focus_handle(cx), cx);
        }
        cx.notify();
    }

    fn zoom_out(&mut self, _: &ZoomOut, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_to(self.altitude.zoom_out(), window, cx);
    }

    fn leave_to(&mut self, altitude: Altitude, window: &mut Window, cx: &mut Context<Self>) {
        if altitude == self.altitude {
            return;
        }
        let back_to_its_topic = matches!(
            (&self.altitude, &altitude),
            (Altitude::Session { topic: Some(from), .. }, Altitude::Topic(to)) if from == to
        );
        if !matches!(altitude, Altitude::Session { .. }) && !back_to_its_topic {
            self.screen = TopicScreen::default();
        }
        self.altitude = altitude;
        self.retitling = None;
        if !matches!(self.altitude, Altitude::Session { .. }) {
            self.pane = None;
            self.placing = None;
            self.load(cx);
        }
        self.sync_step(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The location bar, one chip per level; any step above the current one zooms out to it.
    fn crumbs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let separator = || div().flex_none().child("›");
        let mut bar = div()
            .flex()
            .flex_nowrap()
            .items_center()
            .gap_1()
            .min_w_0()
            .max_w(gpui::relative(0.6))
            .p_1()
            .rounded(px(16.))
            .border_1()
            .border_color(Theme::line())
            .bg(Theme::panel())
            .shadow_md()
            .text_sm()
            .text_color(Theme::ink_dim())
            .child(
                Self::crumb(
                    "crumb-everything",
                    "Everything".into(),
                    self.altitude == Altitude::Everything,
                )
                .flex_none()
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                    this.leave_to(Altitude::Everything, window, cx);
                })),
            );
        let (topic, session) = match &self.altitude {
            Altitude::Everything => return bar,
            Altitude::Topic(topic) => (Some(topic.clone()), None),
            Altitude::Session { topic, session } => (topic.clone(), Some(session.clone())),
        };
        let topic_crumb = match topic {
            Some(topic) => {
                let name = self
                    .topic(&topic)
                    .map_or_else(|| topic.as_str().to_owned(), |known| known.name.clone());
                let here = session.is_none();
                match self.retitle_field(&topic, cx) {
                    Some(field) => div().id("crumb-topic").child(field),
                    None => Self::crumb("crumb-topic", name.into(), here)
                        .on_mouse_down(
                            MouseButton::Right,
                            Self::on_right_click(Focus::Topic(topic.clone()), cx),
                        )
                        .on_click(cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                            if here && event.click_count() == 2 {
                                this.begin_retitle(topic.clone(), window, cx);
                            } else {
                                this.leave_to(Altitude::Topic(topic.clone()), window, cx);
                            }
                        })),
                }
            }
            None => Self::crumb("crumb-uncategorized", "Uncategorized".into(), false).on_click(cx.listener(
                |this, _: &gpui::ClickEvent, window, cx| {
                    this.uncategorized_open = true;
                    this.uncategorized_folded = false;
                    this.leave_to(Altitude::Everything, window, cx);
                },
            )),
        };
        bar = bar.child(separator()).child(topic_crumb);
        if let Some(session) = session {
            let known = self.sessions.iter().find(|known| known.id == session);
            let title = known.map_or_else(|| "Untitled".into(), |known| one_line(known.title.as_deref()));
            bar = bar
                .child(separator())
                .child(Self::crumb("crumb-session", title, true).when_some(
                    known.cloned(),
                    |crumb, known| {
                        crumb.on_mouse_down(
                            MouseButton::Right,
                            Self::on_right_click(Focus::Session(Box::new(known)), cx),
                        )
                    },
                ));
        }
        bar
    }

    fn crumb(id: &'static str, label: SharedString, here: bool) -> gpui::Stateful<gpui::Div> {
        let ink = if here { Theme::on_selected() } else { Theme::ink() };
        div()
            .id(id)
            .flex()
            .items_center()
            .gap_1p5()
            .min_w_0()
            .px_3()
            .py_1()
            .rounded(px(999.))
            .cursor_pointer()
            .map(|chip| {
                if here {
                    chip.bg(Theme::selected())
                        .text_color(Theme::on_selected())
                        .font_weight(gpui::FontWeight::MEDIUM)
                } else {
                    chip.bg(Theme::card())
                        .text_color(Theme::ink())
                        .hover(|chip| chip.bg(Theme::raised()))
                }
            })
            .when(id == "crumb-everything", |chip| {
                chip.child(brand::icon("icons/workspace.svg", 14., ink))
            })
            .child(div().min_w_0().truncate().child(label))
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

    fn icon_button(
        id: &'static str,
        icon: &'static str,
        listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        div()
            .id(id)
            .size(px(36.))
            .rounded(px(18.))
            .bg(Theme::card())
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|button| button.bg(Theme::raised()))
            .on_click(listener)
            .child(brand::icon(icon, 18., Theme::ink()))
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let in_session = matches!(self.altitude, Altitude::Session { .. });
        let back = match self.altitude.zoom_out() {
            Altitude::Topic(_) => "Ctrl ↑ Topic",
            _ => "Ctrl ↑ Everything",
        };
        shell::header()
            .child(self.crumbs(cx))
            // Everything shows problems on the page; elsewhere they'd have nowhere else to go.
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(Theme::warn())
                    .children(self.problems().filter(|_| self.altitude != Altitude::Everything)),
            )
            .when(self.altitude != Altitude::Everything, |header| {
                header.child(div().text_xs().text_color(Theme::ink_dim()).child(back))
            })
            .when(!in_session && !self.panel_open, |header| {
                header
                    .child(Self::icon_button(
                        "open-search",
                        "icons/search.svg",
                        cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                            this.reveal_panel(true, window, cx);
                        }),
                    ))
                    .child(Self::icon_button(
                        "open-history",
                        "icons/history.svg",
                        cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                            this.reveal_panel(false, window, cx);
                        }),
                    ))
            })
            .when(!in_session, |header| {
                header.child(Self::icon_button(
                    "toggle-panel",
                    "icons/side-panel.svg",
                    cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                        this.toggle_panel(&ToggleSidePanel, window, cx);
                    }),
                ))
            })
            .child(
                div()
                    .id("new-session")
                    .h(px(34.))
                    .px_4()
                    .rounded(px(17.))
                    .bg(Theme::accent())
                    .text_color(Theme::on_accent())
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .on_click(cx.listener(Self::start))
                    .child(brand::icon("icons/plus.svg", 16., Theme::on_accent()))
                    .child("New session"),
            )
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
