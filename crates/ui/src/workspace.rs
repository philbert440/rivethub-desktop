mod everything;
mod topic_view;

use std::collections::{HashMap, HashSet};

use app::Hub;
use gateway::GatewayError;
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, MouseDownEvent, ParentElement,
    Pixels, Point, Render, Rgba, SharedString, Styled, Subscription, Window, actions, anchored, deferred,
    div, prelude::*, px,
};
use model::session::{SessionStatus, SessionSummary};
use model::topics::{Atlas, Board, Placements, Suggestion, Topic, Windows};
use model::{Altitude, HarnessId, SessionId, TopicId};

use crate::agent_editor::{AgentEditor, AgentEditorEvent};
use crate::launcher::{Choice, Launcher, LauncherEvent, Target};
use crate::memory::Focus;
use crate::session_pane::{PaneEvent, SessionPane};
use crate::side_panel::{DraggedAgent, DraggedSession, PanelEvent, PastTopic, SidePanel};
use crate::text::{now_ms, one_line};
use crate::topic_picker::{PickerEvent, TopicPicker};
use crate::{brand, shell, theme::Theme};

actions!(workspace, [ZoomOut, NewSession, ToggleSidePanel, NextNeedingYou]);

const HOUR_MS: i64 = 60 * 60 * 1000;
const FORMING_MS: i64 = 24 * HOUR_MS;
const RECENT_MS: i64 = 14 * 24 * HOUR_MS;

type Loaded = (
    Result<app::AllSessions, GatewayError>,
    Result<(Vec<Topic>, Placements), GatewayError>,
);

enum Launch {
    Forming,
    In(TopicId),
    New(String),
}

/// Asks the shell to show Memory focused on a topic or session.
pub struct ViewInMemory(pub Focus);

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
    suggestions: HashMap<SessionId, Suggestion>,
    subagents: HashMap<SessionId, usize>,
    folded: HashSet<SessionId>,
    forming_open: bool,
    problem: Option<String>,
    pane: Option<Entity<SessionPane>>,
    pane_events: Option<Subscription>,
    panel: Entity<SidePanel>,
    panel_open: bool,
    placing: Option<Placing>,
    launching: Option<(Entity<Launcher>, Subscription)>,
    editing_agent: Option<(Entity<AgentEditor>, Subscription)>,
    menu: Option<(Focus, Point<Pixels>)>,
    feed: app::WorkspaceFeed,
    _panel_events: Subscription,
    _feed: [gpui::Task<()>; 2],
}

impl EventEmitter<ViewInMemory> for Workspace {}

impl Workspace {
    pub fn new(hub: Hub, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let panel = cx.new(|cx| SidePanel::new(hub.clone(), cx));
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
                if this.update(cx, |_, cx| cx.notify()).is_err() {
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
            suggestions: HashMap::new(),
            subagents: HashMap::new(),
            folded: HashSet::new(),
            forming_open: false,
            problem: None,
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
            _feed: [refresh, repaint],
        };
        workspace.load(cx);
        workspace
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
                self.sessions = all.sessions;
                if !all.unreachable.is_empty() {
                    problems.push(format!("couldn't reach {}", all.unreachable.join(", ")));
                }
            }
            Err(err) => problems.push(err.to_string()),
        }
        match topics {
            Ok((topics, placements)) => {
                self.topics = topics;
                self.placements = placements;
            }
            Err(err) if err.is_missing_route() => {
                problems.push("Topics need a newer rh on this node.".to_owned());
            }
            Err(err) => problems.push(format!("topics: {err}")),
        }
        self.problem = (!problems.is_empty()).then(|| problems.join(" · "));
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
        self.load_suggestions(cx);
        cx.notify();
    }

    fn load_suggestions(&self, cx: &mut Context<Self>) {
        let forming: Vec<SessionId> = self
            .board()
            .forming
            .iter()
            .map(|session| session.id.clone())
            .collect();
        if forming.is_empty() {
            return;
        }
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let found = hub.suggestions(forming).await;
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
            forming_ms: FORMING_MS,
            recent_ms: RECENT_MS,
        };
        model::topics::board(&self.topics, &self.sessions, &self.placements, windows)
    }

    fn sync_panel(&self, cx: &mut Context<Self>) {
        let board = self.board();
        let history: Vec<SessionSummary> = board.history.iter().map(|session| (*session).clone()).collect();
        let past: Vec<PastTopic> = board
            .past
            .iter()
            .map(|card| PastTopic {
                id: card.topic.id.clone(),
                name: card.topic.name.clone(),
                sessions: card.sessions.len(),
                active_ms: card.active_ms,
            })
            .collect();
        let topics = self.topics.clone();
        let known = self.sessions.clone();
        self.panel.update(cx, |panel, cx| {
            panel.set_sessions(history, known, cx);
            panel.set_topics(topics, past, cx);
        });
    }

    fn home_of(&self, session: &SessionSummary) -> Option<TopicId> {
        let atlas = Atlas::new(&self.topics, &self.sessions, &self.placements);
        atlas.topic_of(session).map(|topic| atlas.root(topic).clone())
    }

    fn topic(&self, id: &TopicId) -> Option<&Topic> {
        self.topics.iter().find(|topic| &topic.id == id)
    }

    fn open(&mut self, summary: SessionSummary, window: &mut Window, cx: &mut Context<Self>) {
        let topic = self.home_of(&summary);
        let session = summary.id.clone();
        let pane = cx.new(|cx| SessionPane::new(self.hub.clone(), summary, cx));
        window.focus(&pane.read(cx).composer().focus_handle(cx), cx);
        self.pane_events = Some(cx.subscribe(&pane, |this, _, event: &PaneEvent, cx| match event {
            PaneEvent::Renamed(renamed) => this.renamed(renamed, cx),
        }));
        self.pane = Some(pane);
        self.placing = None;
        self.altitude = Altitude::Session { topic, session };
        cx.notify();
    }

    fn renamed(&mut self, renamed: &SessionSummary, cx: &mut Context<Self>) {
        if let Some(known) = self.sessions.iter_mut().find(|known| known.id == renamed.id) {
            known.title.clone_from(&renamed.title);
        }
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

    /// Right-clicking opens a menu that can show this topic or session in Memory.
    fn on_right_click(
        focus: Focus,
        cx: &mut Context<Self>,
    ) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
            this.menu = Some((focus.clone(), event.position));
            cx.stop_propagation();
            cx.notify();
        })
    }

    /// A session shows in Memory as a search for its title, so only titled sessions can.
    fn session_focus(session: &SessionSummary) -> Option<Focus> {
        session
            .title
            .as_deref()
            .map(|title| Focus::Search(one_line(Some(title)).to_string()))
    }

    fn menu(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let (focus, at) = self.menu.clone()?;
        let item = div()
            .id("view-in-memory")
            .px_3()
            .py_2()
            .rounded(px(10.))
            .cursor_pointer()
            .hover(|item| item.bg(Theme::selected()).text_color(Theme::on_selected()))
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                this.menu = None;
                cx.emit(ViewInMemory(focus.clone()));
                cx.notify();
            }))
            .child("View in Memory");
        Some(deferred(
            anchored().position(at).child(
                div()
                    .min_w(px(200.))
                    .p_1()
                    .rounded(px(16.))
                    .bg(Theme::card())
                    .border_1()
                    .border_color(Theme::line())
                    .text_color(Theme::ink())
                    .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.menu = None;
                        cx.notify();
                    }))
                    .child(item),
            ),
        ))
    }

    fn open_topic(&mut self, topic: TopicId, window: &mut Window, cx: &mut Context<Self>) {
        self.pane = None;
        self.placing = None;
        self.altitude = Altitude::Topic(topic);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn next_needing_you(&mut self, _: &NextNeedingYou, window: &mut Window, cx: &mut Context<Self>) {
        let current = match &self.altitude {
            Altitude::Session { session, .. } => Some(session.clone()),
            Altitude::Everything | Altitude::Topic(_) => None,
        };
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
        self.placements.insert(session.clone(), topic.clone());
        self.sync_panel(cx);
        cx.notify();
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let placed = hub.place(session, topic).await;
            this.update(cx, |this, cx| {
                if let Err(err) = placed {
                    this.problem = Some(format!("placing the session failed: {err}"));
                }
                this.load(cx);
            })
            .ok();
        })
        .detach();
    }

    fn unplace(&mut self, session: SessionId, cx: &mut Context<Self>) {
        self.placements.remove(&session);
        self.sync_panel(cx);
        cx.notify();
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let unplaced = hub.unplace(session).await;
            this.update(cx, |this, cx| {
                if let Err(err) = unplaced {
                    this.problem = Some(format!("taking the session out of its topic failed: {err}"));
                }
                this.load(cx);
            })
            .ok();
        })
        .detach();
    }

    fn place_in_new(&mut self, session: SessionId, name: String, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let created = hub.place_in_new(session, name).await;
            this.update(cx, |this, cx| {
                if let Err(err) = created {
                    this.problem = Some(format!("creating the topic failed: {err}"));
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
            Altitude::Topic(topic) => self
                .topic(topic)
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
                Target::Forming => Launch::Forming,
                Target::In(topic) => Launch::In(topic.clone()),
                Target::New(name) => Launch::New(name.clone()),
            };
            match (**choice).clone() {
                Choice::Agent(agent) => this.start_agent(agent, launch, window, cx),
                Choice::Harness { harness, model } => this.start_session(launch, harness, model, window, cx),
            }
        });
        window.focus(&launcher.focus_handle(cx), cx);
        self.launching = Some((launcher, events));
        cx.notify();
    }

    fn start_session(
        &mut self,
        launch: Launch,
        harness: HarnessId,
        model: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let hub = self.hub.clone();
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let started = hub.start_session(harness, model).await;
            cx.update_window(window_handle, |_, window, cx| {
                this.update(cx, |this, cx| this.started(started, launch, window, cx))
                    .ok();
            })
            .ok();
        })
        .detach();
    }

    fn start_with(
        &mut self,
        agent: model::agents::Agent,
        topic: Option<TopicId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_agent(agent, topic.map_or(Launch::Forming, Launch::In), window, cx);
    }

    fn start_agent(
        &mut self,
        agent: model::agents::Agent,
        launch: Launch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
                    Launch::Forming => {}
                }
                self.sessions.push(summary.clone());
                self.open(summary, window, cx);
            }
            Err(err) => {
                self.problem = Some(err.to_string());
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
        self.altitude = altitude;
        if !matches!(self.altitude, Altitude::Session { .. }) {
            self.pane = None;
            self.placing = None;
            self.load(cx);
        }
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
                Self::crumb("crumb-topic", name.into(), here).on_click(cx.listener(
                    move |this, _: &gpui::ClickEvent, window, cx| {
                        this.leave_to(Altitude::Topic(topic.clone()), window, cx);
                    },
                ))
            }
            None => Self::crumb("crumb-forming", "Forming".into(), false).on_click(cx.listener(
                |this, _: &gpui::ClickEvent, window, cx| {
                    this.forming_open = true;
                    this.leave_to(Altitude::Everything, window, cx);
                },
            )),
        };
        bar = bar.child(separator()).child(topic_crumb);
        if let Some(session) = session {
            let title = self
                .sessions
                .iter()
                .find(|known| known.id == session)
                .map_or_else(|| "Untitled".into(), |known| one_line(known.title.as_deref()));
            bar = bar
                .child(separator())
                .child(Self::crumb("crumb-session", title, true));
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
        } else if self.feed.is_working(&session.id) || session.status == SessionStatus::Active {
            Theme::accent()
        } else {
            Theme::ink_dim()
        }
    }

    /// "2 active · 1 waiting on you · 3 idle", leaving out zero counts; `None` when all are zero.
    fn counts<'a>(&self, sessions: impl IntoIterator<Item = &'a SessionSummary>) -> Option<gpui::Div> {
        let (mut active, mut waiting, mut idle) = (0_usize, 0_usize, 0_usize);
        for session in sessions {
            if self.feed.needs_you(session) {
                waiting += 1;
            } else if self.feed.is_working(&session.id) || session.status == SessionStatus::Active {
                active += 1;
            } else {
                idle += 1;
            }
        }
        let parts = [
            (active, format!("{active} active"), Theme::ink_dim()),
            (waiting, format!("{waiting} waiting on you"), Theme::warn()),
            (idle, format!("{idle} idle"), Theme::ink_dim()),
        ];
        let mut line = div().flex().text_xs().text_color(Theme::ink_dim());
        let mut any = false;
        for (count, label, color) in parts {
            if count == 0 {
                continue;
            }
            if any {
                line = line.child("\u{a0}·\u{a0}");
            }
            line = line.child(div().text_color(color).child(label));
            any = true;
        }
        any.then_some(line)
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
            .child(div().flex_1())
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
        let launcher = self.launching.as_ref().map(|(launcher, _)| {
            div()
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
                .child(
                    div()
                        .px_2()
                        .pt_1()
                        .text_xs()
                        .text_color(Theme::ink_dim())
                        .child("Start a session"),
                )
                .child(launcher.clone())
        });
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
            .children(launcher)
            .children(self.editing_agent.as_ref().map(|(editor, _)| {
                div()
                    .absolute()
                    .inset_0()
                    .bg(Theme::scrim())
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(editor.clone())
            }))
            .children(self.menu(cx))
    }
}
