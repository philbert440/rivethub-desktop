use app::Hub;
use gateway::GatewayError;
use gpui::{
    AnyElement, ClickEvent, Context, Entity, EventEmitter, Focusable, FontWeight, IntoElement, ParentElement,
    Render, Styled, Subscription, Window, actions, div, prelude::*, px,
};
use model::decisions::{Decision, text};
use model::preview::StartupPreview;
use model::session::SessionSummary;
use model::{SessionId, TopicId};

use crate::input::{Submitted, TextInput};
use crate::memory::Focus;
use crate::text::{ago, now_ms, one_line};
use crate::theme::Theme;

actions!(topic_rail, [CancelDecision]);

/// How many of the newest sessions catch-up reads, and how many of their summaries it shows.
const CAUGHT_SESSIONS: usize = 6;
const CAUGHT_SHOWN: usize = 4;

pub enum RailEvent {
    OpenSession(Box<SessionSummary>),
    ViewInMemory(Focus),
}

/// One catch-up line: the latest summary of one of the topic's sessions.
struct Caught {
    session: SessionSummary,
    text: String,
    at_ms: i64,
}

/// A decision being typed into the Decisions card.
struct Adding {
    input: Entity<TextInput>,
    busy: bool,
    problem: Option<String>,
    _submitted: Subscription,
}

/// The topic screen's rail: the shown topic's decisions, and what its newest sessions last did.
pub struct TopicRail {
    hub: Hub,
    topic: Option<TopicId>,
    decisions: Option<Vec<Decision>>,
    problem: Option<String>,
    adding: Option<Adding>,
    watched: Vec<(SessionId, i64)>,
    caught: Vec<Caught>,
    startup: Option<StartupPreview>,
    startup_open: bool,
}

impl EventEmitter<RailEvent> for TopicRail {}

impl TopicRail {
    pub fn new(hub: Hub, _: &mut Context<Self>) -> Self {
        Self {
            hub,
            topic: None,
            decisions: None,
            problem: None,
            adding: None,
            watched: Vec::new(),
            caught: Vec::new(),
            startup: None,
            startup_open: false,
        }
    }

    /// Shows `topic`, whose subtree holds `sessions`, newest first. Decisions are read again when
    /// the topic changes and catch-up when one of its newest sessions does.
    pub fn show(&mut self, topic: TopicId, sessions: Vec<SessionSummary>, cx: &mut Context<Self>) {
        if self.topic.as_ref() != Some(&topic) {
            self.topic = Some(topic.clone());
            self.decisions = None;
            self.problem = None;
            self.adding = None;
            self.caught.clear();
            self.watched.clear();
            self.startup = None;
            self.startup_open = false;
            self.load_startup(topic.clone(), cx);
            self.load_decisions(topic, cx);
        }
        let newest: Vec<SessionSummary> = sessions.into_iter().take(CAUGHT_SESSIONS).collect();
        let watched: Vec<(SessionId, i64)> = newest
            .iter()
            .map(|session| (session.id.clone(), session.updated_at_ms))
            .collect();
        if watched != self.watched {
            self.watched = watched;
            self.load_catch_up(newest, cx);
        }
    }

    /// What a session started in the topic would be told. A node without startup memory, or with
    /// an older rh, has nothing to preview, and the card stays away.
    fn load_startup(&self, topic: TopicId, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let loaded = hub.topic_startup(topic.clone()).await;
            this.update(cx, |this, cx| {
                if this.topic.as_ref() == Some(&topic) {
                    this.startup = loaded.ok().filter(|startup| !startup.sections.is_empty());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn load_decisions(&self, topic: TopicId, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let loaded = hub.decisions(topic.clone()).await;
            this.update(cx, |this, cx| {
                if this.topic.as_ref() != Some(&topic) {
                    return;
                }
                match loaded {
                    Ok(decisions) => {
                        this.decisions = Some(decisions.into_iter().filter(Decision::is_active).collect());
                    }
                    Err(err) if err.is_missing_route() => {
                        this.problem = Some("Decisions need a newer rh on this node.".into());
                    }
                    Err(err) => this.problem = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn load_catch_up(&self, sessions: Vec<SessionSummary>, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        let watched = self.watched.clone();
        cx.spawn(async move |this, cx| {
            let ids = sessions.iter().map(|session| session.id.clone()).collect();
            let read = hub.session_memories(ids).await.unwrap_or_default();
            let mut caught: Vec<Caught> = sessions
                .into_iter()
                .zip(read)
                .filter_map(|(session, memory)| {
                    let latest = memory.as_ref()?.latest()?;
                    Some(Caught {
                        at_ms: latest.to_ms.unwrap_or(session.updated_at_ms),
                        text: latest.text.clone(),
                        session,
                    })
                })
                .collect();
            caught.sort_by_key(|line| std::cmp::Reverse(line.at_ms));
            caught.truncate(CAUGHT_SHOWN);
            this.update(cx, |this, cx| {
                if this.watched == watched {
                    this.caught = caught;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn begin_adding(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextInput::new("What was decided", cx));
        let submitted = cx.subscribe(&input, |this, _, Submitted(typed), cx| {
            this.add(typed, cx);
        });
        window.focus(&input.focus_handle(cx), cx);
        self.adding = Some(Adding {
            input,
            busy: false,
            problem: None,
            _submitted: submitted,
        });
        cx.notify();
    }

    fn cancel_adding(&mut self, _: &CancelDecision, _: &mut Window, cx: &mut Context<Self>) {
        self.adding = None;
        cx.notify();
    }

    fn add(&mut self, typed: &str, cx: &mut Context<Self>) {
        let Some(topic) = self.topic.clone() else {
            return;
        };
        let Some(adding) = self.adding.as_mut().filter(|adding| !adding.busy) else {
            return;
        };
        let Some(typed) = text(typed) else {
            adding.problem = Some("A decision needs some text.".into());
            cx.notify();
            return;
        };
        adding.busy = true;
        adding.problem = None;
        cx.notify();
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let added = hub.add_decision(topic.clone(), typed).await;
            this.update(cx, |this, cx| {
                if this.topic.as_ref() != Some(&topic) {
                    return;
                }
                match added {
                    Ok(decision) => {
                        this.adding = None;
                        this.decisions.get_or_insert_with(Vec::new).push(decision);
                    }
                    Err(err) => {
                        if let Some(adding) = this.adding.as_mut() {
                            adding.busy = false;
                            adding.problem = Some(match err {
                                GatewayError::Conflict(message) => message,
                                other => other.to_string(),
                            });
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn card() -> gpui::Div {
        div()
            .p(px(18.))
            .rounded(px(20.))
            .bg(Theme::panel())
            .flex()
            .flex_col()
            .gap_2()
    }

    fn label(text: String) -> impl IntoElement {
        div()
            .flex_1()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(Theme::ink_dim())
            .child(text)
    }

    fn pill(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .px_3()
            .py_1()
            .rounded(px(15.))
            .border_1()
            .border_color(Theme::line())
            .cursor_pointer()
            .text_xs()
            .text_color(Theme::ink())
            .hover(|pill| pill.bg(Theme::card()))
            .child(label)
    }

    fn adding_field(adding: &Adding, cx: &mut Context<Self>) -> AnyElement {
        div()
            .key_context("RailDecision")
            .on_action(cx.listener(Self::cancel_adding))
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .px_3()
                    .py_1()
                    .rounded(px(12.))
                    .bg(Theme::card())
                    .border_1()
                    .border_color(Theme::accent())
                    .when(adding.busy, |field| field.opacity(0.6))
                    .child(adding.input.clone()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(
                        adding
                            .problem
                            .as_ref()
                            .map_or_else(Theme::ink_dim, |_| Theme::warn()),
                    )
                    .child(
                        adding
                            .problem
                            .clone()
                            .unwrap_or_else(|| "Enter adds · Esc cancels".into()),
                    ),
            )
            .into_any_element()
    }

    fn decisions_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let count = self.decisions.as_ref().map_or(0, Vec::len);
        let topic = self.topic.clone();
        Self::card()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Self::label(format!("DECISIONS · {count}")))
                    .when_some(topic, |row, topic| {
                        row.child(
                            Self::pill("rail-decisions-memory", "In Memory").on_click(cx.listener(
                                move |_, _: &ClickEvent, _, cx| {
                                    cx.emit(RailEvent::ViewInMemory(Focus::Topic(topic.clone())));
                                },
                            )),
                        )
                    })
                    .when(self.adding.is_none(), |row| {
                        row.child(Self::pill("rail-decision-add", "+ Add").on_click(
                            cx.listener(|this, _: &ClickEvent, window, cx| this.begin_adding(window, cx)),
                        ))
                    }),
            )
            .children(self.decisions.iter().flatten().map(|decision| {
                div()
                    .flex()
                    .items_baseline()
                    .gap_2()
                    .child(div().flex_none().text_color(Theme::accent()).child("•"))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_color(Theme::ink())
                            .child(decision.text.clone()),
                    )
            }))
            .children(
                self.problem
                    .clone()
                    .map(|problem| div().text_xs().text_color(Theme::warn()).child(problem)),
            )
            .children(self.adding.as_ref().map(|adding| Self::adding_field(adding, cx)))
            .into_any_element()
    }

    fn catch_up_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.caught.is_empty() {
            return None;
        }
        let now = now_ms();
        Some(
            Self::card()
                .gap_3()
                .child(Self::label("CATCH-UP".into()))
                .children(self.caught.iter().enumerate().map(|(index, line)| {
                    let session = line.session.clone();
                    div()
                        .id(("caught", index))
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .p_1()
                        .rounded(px(10.))
                        .cursor_pointer()
                        .hover(|line| line.bg(Theme::card()))
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                            cx.emit(RailEvent::OpenSession(Box::new(session.clone())));
                        }))
                        .child(
                            div()
                                .text_xs()
                                .text_color(Theme::ink_dim())
                                .truncate()
                                .child(format!(
                                    "{} · {}",
                                    ago(now, line.at_ms),
                                    one_line(line.session.title.as_deref())
                                )),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(Theme::ink())
                                .line_clamp(2)
                                .child(one_line(Some(&line.text))),
                        )
                }))
                .into_any_element(),
        )
    }
}

impl TopicRail {
    /// NEW SESSIONS HERE GET: the sections startup memory would hand a session started in the
    /// topic now, and on request the text itself.
    fn startup_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let startup = self.startup.as_ref()?;
        let open = self.startup_open;
        Some(
            Self::card()
                .bg(Theme::surface())
                .border_1()
                .border_color(Theme::line())
                .child(Self::label("NEW SESSIONS HERE GET".into()))
                .children(startup.sections.iter().map(|section| {
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .text_color(Theme::ink())
                                .child(section.title.clone()),
                        )
                        .when(open, |part| {
                            part.child(
                                div()
                                    .text_xs()
                                    .text_color(Theme::ink_dim())
                                    .child(section.text.clone()),
                            )
                        })
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .text_xs()
                                .text_color(Theme::ink_dim())
                                .child(format!("about {} tokens", startup.tokens)),
                        )
                        .child(
                            Self::pill(
                                "startup-open",
                                if open {
                                    "Hide what's sent"
                                } else {
                                    "Show what's sent"
                                },
                            )
                            .on_click(cx.listener(
                                |this, _: &ClickEvent, _, cx| {
                                    this.startup_open = !this.startup_open;
                                    cx.notify();
                                },
                            )),
                        ),
                )
                .into_any_element(),
        )
    }
}

impl Render for TopicRail {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_4()
            .child(self.decisions_card(cx))
            .children(self.catch_up_card(cx))
            .children(self.startup_card(cx))
    }
}
