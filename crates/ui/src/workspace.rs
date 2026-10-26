use app::Hub;
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, IntoElement, ParentElement, Render, SharedString, Styled,
    Window, actions, div, img, prelude::*, px,
};
use model::session::{SessionStatus, SessionSummary};
use model::{Altitude, HarnessId};

use crate::session_pane::SessionPane;
use crate::side_panel::{DraggedAgent, DraggedSession, PanelEvent, SidePanel};
use crate::{brand, theme::Theme};

actions!(workspace, [ZoomOut, NewSession, ToggleSidePanel]);

const CLAUDE: &str = "claude-code";
const FORMING_WINDOW_MS: i64 = 24 * 60 * 60 * 1000;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}

fn history(sessions: &[SessionSummary], now_ms: i64) -> Vec<SessionSummary> {
    let young: std::collections::HashSet<&model::SessionId> = sessions
        .iter()
        .filter(|session| session.parent.is_none() && now_ms - session.updated_at_ms <= FORMING_WINDOW_MS)
        .map(|session| &session.id)
        .collect();
    let mut older: Vec<SessionSummary> = sessions
        .iter()
        .filter(|session| !young.contains(&session.id))
        .filter(|session| {
            session
                .parent
                .as_ref()
                .is_none_or(|parent| !young.contains(parent))
        })
        .cloned()
        .collect();
    older.sort_by_key(|session| std::cmp::Reverse(session.updated_at_ms));
    older
}

fn forming(mut sessions: Vec<SessionSummary>, now_ms: i64) -> Vec<SessionSummary> {
    sessions
        .retain(|session| session.parent.is_none() && now_ms - session.updated_at_ms <= FORMING_WINDOW_MS);
    sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at_ms));
    sessions
}

pub struct Workspace {
    focus: FocusHandle,
    hub: Hub,
    altitude: Altitude,
    forming: Vec<SessionSummary>,
    subagents: std::collections::HashMap<model::SessionId, usize>,
    problem: Option<String>,
    pane: Option<Entity<SessionPane>>,
    panel: Entity<SidePanel>,
    panel_open: bool,
    feed: app::WorkspaceFeed,
    _panel_events: gpui::Subscription,
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
                    PanelEvent::Start(agent) => this.start_with(agent.clone(), window, cx),
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
                if this.update(cx, |this, cx| this.load_forming(cx)).is_err() {
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
            forming: Vec::new(),
            subagents: std::collections::HashMap::new(),
            problem: None,
            pane: None,
            panel,
            panel_open: true,
            feed,
            _panel_events: panel_events,
            _feed: [refresh, repaint],
        };
        workspace.load_forming(cx);
        workspace
    }

    fn load_forming(&self, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let loaded = hub.recent_sessions(HarnessId::new(CLAUDE)).await;
            this.update(cx, |this, cx| {
                match loaded {
                    Ok(sessions) => {
                        let now = now_ms();
                        let history = history(&sessions, now);
                        this.panel
                            .update(cx, |panel, cx| panel.set_sessions(history, sessions.clone(), cx));
                        this.forming = forming(sessions, now);
                        this.problem = None;
                    }
                    Err(err) => this.problem = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn open(&mut self, summary: SessionSummary, window: &mut Window, cx: &mut Context<Self>) {
        let id = summary.id.clone();
        let hub = self.hub.clone();
        let pane = cx.new(|cx| SessionPane::new(hub, summary, cx));
        window.focus(&pane.read(cx).composer().focus_handle(cx), cx);
        self.pane = Some(pane);
        self.altitude = Altitude::Session {
            topic: None,
            session: id,
        };
        cx.notify();
    }

    fn new_session(&mut self, _: &NewSession, window: &mut Window, cx: &mut Context<Self>) {
        self.start_session(window, cx);
    }

    fn start(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.start_session(window, cx);
    }

    fn start_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let started = hub.start_session(HarnessId::new(CLAUDE)).await;
            cx.update_window(window_handle, |_, window, cx| {
                this.update(cx, |this, cx| match started {
                    Ok(summary) => this.open(summary, window, cx),
                    Err(err) => {
                        this.problem = Some(err.to_string());
                        cx.notify();
                    }
                })
                .ok();
            })
            .ok();
        })
        .detach();
    }

    fn start_with(&mut self, agent: model::agents::Agent, window: &mut Window, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let started = hub.start_with(agent).await;
            cx.update_window(window_handle, |_, window, cx| {
                this.update(cx, |this, cx| match started {
                    Ok(summary) => this.open(summary, window, cx),
                    Err(err) => {
                        this.problem = Some(err.to_string());
                        cx.notify();
                    }
                })
                .ok();
            })
            .ok();
        })
        .detach();
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
            self.load_forming(cx);
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn breadcrumb(&self) -> SharedString {
        match &self.altitude {
            Altitude::Everything => "Everything".into(),
            Altitude::Topic(topic)
            | Altitude::Session {
                topic: Some(topic), ..
            } => format!("Everything › {}", topic.as_str()).into(),
            Altitude::Session { topic: None, .. } => "Everything › Forming".into(),
        }
    }

    fn icon_button(
        id: &'static str,
        glyph: &'static str,
        listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        div()
            .id(id)
            .size(px(34.))
            .rounded(px(17.))
            .bg(Theme::card())
            .text_color(Theme::ink_dim())
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|button| button.text_color(Theme::ink()))
            .on_click(listener)
            .child(glyph)
    }

    fn rail() -> impl IntoElement {
        div()
            .w(px(56.))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .pt_3()
            .bg(Theme::window())
            .child(img(brand::rh()).size(px(32.)))
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(52.))
            .flex_none()
            .flex()
            .items_center()
            .gap_4()
            .px_5()
            .child(div().text_color(Theme::ink()).child(self.breadcrumb()))
            .child(div().flex_1())
            .children(matches!(self.altitude, Altitude::Session { .. }).then(|| {
                div()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child("Ctrl ↑ Everything")
            }))
            .when(
                !matches!(self.altitude, Altitude::Session { .. }) && !self.panel_open,
                |header| {
                    header
                        .child(Self::icon_button(
                            "open-search",
                            "⌕",
                            cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                                this.reveal_panel(true, window, cx);
                            }),
                        ))
                        .child(Self::icon_button(
                            "open-history",
                            "◷",
                            cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                                this.reveal_panel(false, window, cx);
                            }),
                        ))
                },
            )
            .when(!matches!(self.altitude, Altitude::Session { .. }), |header| {
                header.child(Self::icon_button(
                    "toggle-panel",
                    "▥",
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
                    .cursor_pointer()
                    .on_click(cx.listener(Self::start))
                    .child("+ New session"),
            )
    }

    fn everything(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self.forming.is_empty() {
            let line = self
                .problem
                .clone()
                .unwrap_or_else(|| "Start a session and it will find its topic.".into());
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .child(img(brand::ghost()).size(px(96.)))
                .child(div().text_color(Theme::ink()).child("No topics yet"))
                .child(div().text_color(Theme::ink_dim()).child(line))
                .into_any_element();
        }
        div()
            .size_full()
            .p_6()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .gap_3()
                    .items_baseline()
                    .child(div().text_color(Theme::ink()).child("FORMING"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .child(format!("{} sessions without a topic yet", self.forming.len())),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_3()
                    .children(self.forming.iter().enumerate().map(|(index, session)| {
                        let summary = session.clone();
                        let dot =
                            if self.feed.is_working(&session.id) || session.status == SessionStatus::Active {
                                Theme::accent()
                            } else {
                                Theme::ink_dim()
                            };
                        div()
                            .id(("forming", index))
                            .w(px(320.))
                            .p_4()
                            .rounded(px(16.))
                            .bg(Theme::card())
                            .cursor_pointer()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open(summary.clone(), window, cx);
                            }))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(div().flex_none().size(px(8.)).rounded(px(4.)).bg(dot))
                                    .child(
                                        div()
                                            .text_color(Theme::ink())
                                            .truncate()
                                            .child(crate::text::one_line(session.title.as_deref())),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(Theme::ink_dim())
                                    .child(session.model.clone().unwrap_or_else(|| "claude".into())),
                            )
                            .children(self.subagents.get(&session.id).map(|count| {
                                let noun = if *count == 1 { "subagent" } else { "subagents" };
                                div()
                                    .text_xs()
                                    .text_color(Theme::ink_dim())
                                    .child(format!("↳ {count} {noun}"))
                            }))
                    })),
            )
            .into_any_element()
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let everything = !matches!(self.altitude, Altitude::Session { .. });
        let content = match (&self.altitude, &self.pane) {
            (Altitude::Session { .. }, Some(pane)) => pane.clone().into_any_element(),
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
            .when(everything, |tile| {
                tile.drag_over::<DraggedAgent>(|style, _, _, _| style.bg(Theme::selected()))
                    .drag_over::<DraggedSession>(|style, _, _, _| style.bg(Theme::selected()))
                    .on_drop(cx.listener(|this, dragged: &DraggedAgent, window, cx| {
                        this.start_with(dragged.0.clone(), window, cx);
                    }))
                    .on_drop(cx.listener(|this, dragged: &DraggedSession, window, cx| {
                        this.open(dragged.0.clone(), window, cx);
                    }))
            })
            .child(content);
        div()
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::new_session))
            .on_action(cx.listener(Self::toggle_panel))
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
                        .when(everything && self.panel_open, |row| {
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
    }
}
