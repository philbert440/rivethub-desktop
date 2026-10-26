mod everything;
mod topic_view;

use std::collections::{HashMap, HashSet};

use app::Hub;
use gateway::GatewayError;
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, IntoElement, ParentElement, Render, Rgba, SharedString,
    Styled, Subscription, Window, actions, div, img, prelude::*, px,
};
use model::session::{SessionStatus, SessionSummary};
use model::topics::{Atlas, Board, Placements, Suggestion, Topic, Windows};
use model::{Altitude, HarnessId, SessionId, TopicId};

use crate::session_pane::{PaneEvent, SessionPane};
use crate::side_panel::{DraggedAgent, DraggedSession, PanelEvent, PastTopic, SidePanel};
use crate::text::now_ms;
use crate::topic_picker::{PickerEvent, TopicPicker};
use crate::{brand, theme::Theme};

actions!(workspace, [ZoomOut, NewSession, ToggleSidePanel, NextNeedingYou]);

const CLAUDE: &str = "claude-code";
const HOUR_MS: i64 = 60 * 60 * 1000;
const FORMING_MS: i64 = 24 * HOUR_MS;
const RECENT_MS: i64 = 14 * 24 * HOUR_MS;

type Loaded = (
    Result<Vec<SessionSummary>, GatewayError>,
    Result<(Vec<Topic>, Placements), GatewayError>,
);

enum Launch {
    Forming,
    In(TopicId),
    New(String),
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
    launching: Option<(Entity<TopicPicker>, Subscription)>,
    feed: app::WorkspaceFeed,
    _panel_events: Subscription,
    _feed: [gpui::Task<()>; 2],
}

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
            let loaded: Loaded = (
                hub.recent_sessions(HarnessId::new(CLAUDE)).await,
                hub.topics().await,
            );
            this.update(cx, |this, cx| this.loaded(loaded, cx)).ok();
        })
        .detach();
    }

    fn loaded(&mut self, (sessions, topics): Loaded, cx: &mut Context<Self>) {
        let mut problems = Vec::new();
        match sessions {
            Ok(sessions) => self.sessions = sessions,
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
            PaneEvent::Renamed(renamed) => {
                if let Some(known) = this.sessions.iter_mut().find(|known| known.id == renamed.id) {
                    known.title.clone_from(&renamed.title);
                }
                this.sync_panel(cx);
                cx.notify();
            }
        }));
        self.pane = Some(pane);
        self.placing = None;
        self.altitude = Altitude::Session { topic, session };
        cx.notify();
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

    fn new_session(&mut self, _: &NewSession, window: &mut Window, cx: &mut Context<Self>) {
        self.launch(window, cx);
    }

    fn start(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.launch(window, cx);
    }

    fn launch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Altitude::Topic(topic) = &self.altitude {
            let launch = Launch::In(topic.clone());
            self.start_session(launch, window, cx);
            return;
        }
        let topics = self.topics.clone();
        let picker = cx.new(|cx| TopicPicker::new(topics, Some("No topic yet · start in Forming"), cx));
        let events = cx.subscribe_in(&picker, window, |this, _, event: &PickerEvent, window, cx| {
            this.launching = None;
            let launch = match event {
                PickerEvent::Chose(topic) => Launch::In(topic.clone()),
                PickerEvent::Create(name) => Launch::New(name.clone()),
                PickerEvent::NoTopic => Launch::Forming,
                PickerEvent::Dismissed => {
                    window.focus(&this.focus, cx);
                    cx.notify();
                    return;
                }
            };
            this.start_session(launch, window, cx);
        });
        window.focus(&picker.focus_handle(cx), cx);
        self.launching = Some((picker, events));
        cx.notify();
    }

    fn start_session(&mut self, launch: Launch, window: &mut Window, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let started = hub.start_session(HarnessId::new(CLAUDE)).await;
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
        let hub = self.hub.clone();
        let launch = topic.map_or(Launch::Forming, Launch::In);
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
        self.altitude = self.altitude.zoom_out();
        if !matches!(self.altitude, Altitude::Session { .. }) {
            self.pane = None;
            self.load(cx);
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn breadcrumb(&self) -> SharedString {
        let named = |topic: &TopicId| {
            self.topic(topic)
                .map_or_else(|| topic.as_str().to_owned(), |t| t.name.clone())
        };
        match &self.altitude {
            Altitude::Everything => "Everything".into(),
            Altitude::Topic(topic)
            | Altitude::Session {
                topic: Some(topic), ..
            } => format!("Everything › {}", named(topic)).into(),
            Altitude::Session { topic: None, .. } => "Everything › Forming".into(),
        }
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

    fn rail() -> impl IntoElement {
        div()
            .w(px(56.))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .pt_3()
            .gap_4()
            .bg(Theme::window())
            .child(img(brand::rh()).size(px(32.)))
            .child(
                div()
                    .size(px(44.))
                    .rounded(px(22.))
                    .bg(Theme::selected())
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(brand::icon("icons/workspace.svg", 20., Theme::on_selected())),
            )
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let in_session = matches!(self.altitude, Altitude::Session { .. });
        let back = match self.altitude.zoom_out() {
            Altitude::Topic(_) => "Ctrl ↑ Topic",
            _ => "Ctrl ↑ Everything",
        };
        div()
            .h(px(52.))
            .flex_none()
            .flex()
            .items_center()
            .gap_4()
            .px_5()
            .child(div().text_color(Theme::ink()).child(self.breadcrumb()))
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
        let tile = div()
            .id("everything-drop")
            .flex_1()
            .mr_3()
            .mb_3()
            .rounded(px(28.))
            .bg(Theme::surface())
            .overflow_hidden()
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
        let launcher = self.launching.as_ref().map(|(picker, _)| {
            div()
                .absolute()
                .top(px(52.))
                .right(px(20.))
                .w(px(360.))
                .p_2()
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
                        .child("Start a session in…"),
                )
                .child(picker.clone())
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
            .bg(Theme::window())
            .font_family(Theme::SANS)
            .text_color(Theme::ink())
            .text_sm()
            .child(Self::rail())
            .child(
                div().flex_1().flex().flex_col().child(self.header(cx)).child(
                    div()
                        .flex_1()
                        .flex()
                        .child(tile)
                        .when(!in_session && self.panel_open, |row| {
                            row.child(
                                div()
                                    .mb_3()
                                    .mr_3()
                                    .rounded(px(20.))
                                    .overflow_hidden()
                                    .child(self.panel.clone()),
                            )
                        }),
                ),
            )
            .children(launcher)
    }
}
