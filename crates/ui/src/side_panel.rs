use app::Hub;
use gpui::{
    AnyElement, ClickEvent, Context, Entity, EventEmitter, IntoElement, ParentElement, Render, SharedString,
    Styled, Subscription, Window, div, prelude::*, px, rgb,
};
use model::agents::{Agent, Roster};
use model::search::SessionMatch;
use model::session::SessionSummary;
use model::topics::Topic;
use model::{SessionId, TopicId, lineage};

use crate::input::{Submitted, TextInput};
use crate::theme::Theme;

#[derive(Debug, Clone)]
pub enum PanelEvent {
    Open(SessionSummary),
    OpenTopic(TopicId),
    Start(Agent),
}

#[derive(Debug, Clone)]
pub struct PastTopic {
    pub id: TopicId,
    pub name: String,
    pub sessions: usize,
    pub active_ms: i64,
}

#[derive(Debug, Clone)]
pub struct DraggedAgent(pub Agent);

#[derive(Debug, Clone)]
pub struct DraggedSession(pub SessionSummary);

pub struct DragGhost(pub SharedString);

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_2()
            .rounded(px(12.))
            .bg(Theme::raised())
            .border_1()
            .border_color(Theme::accent())
            .text_color(Theme::ink())
            .child(self.0.clone())
    }
}

pub struct SidePanel {
    hub: Hub,
    query: Entity<TextInput>,
    asked: Option<String>,
    results: Option<Vec<SessionMatch>>,
    problem: Option<String>,
    roster: Roster,
    history: Vec<SessionSummary>,
    known: Vec<SessionSummary>,
    topics: Vec<Topic>,
    past: Vec<PastTopic>,
    agents_open: bool,
    past_open: bool,
    history_open: bool,
    collapsed: std::collections::HashSet<SessionId>,
    _submitted: Subscription,
}

impl EventEmitter<PanelEvent> for SidePanel {}

impl SidePanel {
    pub fn new(hub: Hub, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| TextInput::new("Search sessions", cx));
        let submitted = cx.subscribe(&query, |this, _, Submitted(text), cx| {
            this.search(text.clone(), cx);
        });
        let panel = Self {
            hub,
            query,
            asked: None,
            results: None,
            problem: None,
            roster: Roster::default(),
            history: Vec::new(),
            known: Vec::new(),
            topics: Vec::new(),
            past: Vec::new(),
            agents_open: true,
            past_open: true,
            history_open: true,
            collapsed: std::collections::HashSet::new(),
            _submitted: submitted,
        };
        panel.load_agents(cx);
        panel
    }

    pub fn query(&self) -> &Entity<TextInput> {
        &self.query
    }

    pub fn set_sessions(
        &mut self,
        history: Vec<SessionSummary>,
        known: Vec<SessionSummary>,
        cx: &mut Context<Self>,
    ) {
        self.history = history;
        self.known = known;
        cx.notify();
    }

    pub fn set_topics(&mut self, topics: Vec<Topic>, past: Vec<PastTopic>, cx: &mut Context<Self>) {
        self.topics = topics;
        self.past = past;
        cx.notify();
    }

    fn load_agents(&self, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let loaded = hub.agents().await;
            this.update(cx, |this, cx| {
                match loaded {
                    Ok(roster) => this.roster = roster,
                    Err(err) => this.problem = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn search(&mut self, query: String, cx: &mut Context<Self>) {
        self.asked = Some(query.clone());
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let found = hub.search(query).await;
            this.update(cx, |this, cx| {
                match found {
                    Ok(found) => {
                        this.results = Some(found);
                        this.problem = None;
                    }
                    Err(err) => this.problem = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn clear(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.asked = None;
        self.results = None;
        cx.notify();
    }

    fn summary_for(&self, found: &SessionMatch) -> SessionSummary {
        self.known
            .iter()
            .find(|known| known.id == found.session)
            .cloned()
            .unwrap_or_else(|| SessionSummary {
                id: found.session.clone(),
                title: None,
                status: model::session::SessionStatus::Idle,
                model: None,
                cwd: None,
                updated_at_ms: found.latest_ms,
                parent: None,
                node: None,
                blocked: false,
            })
    }

    fn section(
        id: &'static str,
        label: &'static str,
        note: String,
        open: bool,
        toggle: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .h(px(36.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                toggle(this, cx);
                cx.notify();
            }))
            .child(
                div()
                    .w(px(12.))
                    .text_color(Theme::ink_dim())
                    .child(if open { "▾" } else { "▸" }),
            )
            .child(div().text_size(px(13.)).text_color(Theme::ink()).child(label))
            .child(div().text_xs().text_color(Theme::ink_dim()).child(note))
    }

    fn agents(&self, cx: &mut Context<Self>) -> AnyElement {
        let node = self.roster.node.clone();
        div()
            .px_2()
            .pb_3()
            .flex()
            .flex_wrap()
            .gap(px(6.))
            .children(self.roster.agents.iter().enumerate().map(|(index, agent)| {
                let launchable = agent.launchable_on(&node);
                let dot = agent.color.map_or(Theme::ink_dim(), rgb);
                let chip = div()
                    .id(("agent", index))
                    .h(px(30.))
                    .px_3()
                    .rounded(px(15.))
                    .bg(Theme::card())
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(div().size(px(8.)).rounded(px(4.)).bg(dot))
                    .child(div().text_color(Theme::ink()).child(agent.name.clone()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .child(agent.node.clone().unwrap_or_default()),
                    );
                if launchable {
                    let start = agent.clone();
                    chip.cursor_grab()
                        .on_drag(DraggedAgent(agent.clone()), |dragged, _, _, cx| {
                            cx.new(|_| DragGhost(format!("Start a session with {}", dragged.0.name).into()))
                        })
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                            cx.emit(PanelEvent::Start(start.clone()));
                        }))
                        .into_any_element()
                } else {
                    chip.opacity(0.55).into_any_element()
                }
            }))
            .into_any_element()
    }

    fn history(&self, cx: &mut Context<Self>) -> AnyElement {
        let rows = lineage::nest(&self.history, &self.collapsed);
        div()
            .flex()
            .flex_col()
            .children(rows.into_iter().take(60).enumerate().map(|(index, row)| {
                let open = row.session.clone();
                let fold = row.session.id.clone();
                let indent = px(16. * f32::from(u8::try_from(row.depth.min(4)).unwrap_or(4)));
                div()
                    .id(("history", index))
                    .pl(px(8.) + indent)
                    .pr_2()
                    .py(px(7.))
                    .rounded(px(12.))
                    .cursor_pointer()
                    .hover(|line| line.bg(Theme::card()))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .on_drag(DraggedSession(row.session.clone()), |dragged, _, _, cx| {
                        cx.new(|_| DragGhost(crate::text::one_line(dragged.0.title.as_deref())))
                    })
                    .on_click(
                        cx.listener(move |_, _: &ClickEvent, _, cx| cx.emit(PanelEvent::Open(open.clone()))),
                    )
                    .when(row.depth > 0, |line| {
                        line.child(div().text_color(Theme::ink_dim()).child("↳"))
                    })
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_color(Theme::ink())
                            .child(crate::text::one_line(row.session.title.as_deref())),
                    )
                    .when(row.children > 0, |line| {
                        line.child(
                            div()
                                .id(("fold", index))
                                .text_color(Theme::ink_dim())
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    if !this.collapsed.remove(&fold) {
                                        this.collapsed.insert(fold.clone());
                                    }
                                    cx.stop_propagation();
                                    cx.notify();
                                }))
                                .child(if row.collapsed {
                                    format!("▸ {}", row.children)
                                } else {
                                    "▾".to_owned()
                                }),
                        )
                    })
            }))
            .into_any_element()
    }

    fn past_topics(&self, cx: &mut Context<Self>) -> AnyElement {
        let now = crate::text::now_ms();
        div()
            .flex()
            .flex_col()
            .pb_2()
            .children(self.past.iter().enumerate().map(|(index, past)| {
                let open = past.id.clone();
                div()
                    .id(("past", index))
                    .px_2()
                    .py(px(7.))
                    .rounded(px(12.))
                    .cursor_pointer()
                    .hover(|line| line.bg(Theme::card()))
                    .flex()
                    .items_center()
                    .gap_2()
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                        cx.emit(PanelEvent::OpenTopic(open.clone()));
                    }))
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_color(Theme::ink())
                            .child(past.name.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .child(past.sessions.to_string()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .child(crate::text::ago(now, past.active_ms)),
                    )
            }))
            .into_any_element()
    }

    fn topic_hits(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let asked = self.asked.as_deref()?;
        let hits = model::topics::suggest(&self.topics, asked, 5);
        if hits.is_empty() {
            return None;
        }
        Some(
            div()
                .flex()
                .flex_col()
                .pb_2()
                .child(
                    div()
                        .px_2()
                        .pb_1()
                        .text_xs()
                        .text_color(Theme::ink_dim())
                        .child("Topics"),
                )
                .children(hits.into_iter().enumerate().map(|(index, topic)| {
                    let open = topic.id.clone();
                    div()
                        .id(("topic-hit", index))
                        .px_2()
                        .py(px(7.))
                        .rounded(px(12.))
                        .cursor_pointer()
                        .hover(|line| line.bg(Theme::card()))
                        .text_color(Theme::ink())
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                            cx.emit(PanelEvent::OpenTopic(open.clone()));
                        }))
                        .child(topic.name.clone())
                }))
                .into_any_element(),
        )
    }

    fn results(&self, found: &[SessionMatch], cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .children(self.topic_hits(cx))
            .child(
                div()
                    .px_2()
                    .pb_1()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(match found.len() {
                        0 => "No sessions matched".to_owned(),
                        1 => "1 session".to_owned(),
                        count => format!("{count} sessions"),
                    }),
            )
            .children(found.iter().enumerate().map(|(index, hit)| {
                let summary = self.summary_for(hit);
                let open = summary.clone();
                div()
                    .id(("result", index))
                    .px_2()
                    .py_2()
                    .rounded(px(12.))
                    .cursor_pointer()
                    .hover(|row| row.bg(Theme::card()))
                    .on_drag(DraggedSession(summary.clone()), |dragged, _, _, cx| {
                        cx.new(|_| DragGhost(crate::text::one_line(dragged.0.title.as_deref())))
                    })
                    .on_click(
                        cx.listener(move |_, _: &ClickEvent, _, cx| cx.emit(PanelEvent::Open(open.clone()))),
                    )
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .truncate()
                                    .text_color(Theme::ink())
                                    .child(crate::text::one_line(summary.title.as_deref())),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(Theme::warn())
                                    .child(hit.count.to_string()),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .child(hit.snippet.before.clone())
                            .child(div().text_color(Theme::warn()).child(hit.snippet.matched.clone()))
                            .child(hit.snippet.after.clone()),
                    )
            }))
            .into_any_element()
    }
}

impl Render for SidePanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let searching = self.asked.is_some();
        let body = match (&self.results, searching) {
            (Some(found), true) => self.results(&found.clone(), cx),
            (None, true) => div()
                .px_2()
                .text_xs()
                .text_color(Theme::ink_dim())
                .child("Searching…")
                .into_any_element(),
            _ => {
                let agents_note = self.roster.agents.len().to_string();
                let history_note = format!("{} without a topic", self.history.len());
                div()
                    .flex()
                    .flex_col()
                    .child(Self::section(
                        "agents-header",
                        "Agents",
                        agents_note,
                        self.agents_open,
                        |this, _| this.agents_open = !this.agents_open,
                        cx,
                    ))
                    .when(self.agents_open, |section| section.child(self.agents(cx)))
                    .child(div().h(px(1.)).mx_2().bg(Theme::line()))
                    .when(!self.past.is_empty(), |section| {
                        section
                            .child(Self::section(
                                "past-header",
                                "Past topics",
                                self.past.len().to_string(),
                                self.past_open,
                                |this, _| this.past_open = !this.past_open,
                                cx,
                            ))
                            .when(self.past_open, |section| section.child(self.past_topics(cx)))
                            .child(div().h(px(1.)).mx_2().bg(Theme::line()))
                    })
                    .child(Self::section(
                        "history-header",
                        "History",
                        history_note,
                        self.history_open,
                        |this, _| this.history_open = !this.history_open,
                        cx,
                    ))
                    .when(self.history_open, |section| section.child(self.history(cx)))
                    .into_any_element()
            }
        };
        div()
            .w(px(340.))
            .h_full()
            .flex_none()
            .bg(Theme::panel())
            .px_2()
            .pt_3()
            .flex()
            .flex_col()
            .gap_2()
            .overflow_hidden()
            .child(
                div()
                    .mx_1()
                    .px_4()
                    .py_2()
                    .rounded(px(20.))
                    .bg(Theme::card())
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(crate::brand::icon("icons/search.svg", 16., Theme::ink_dim()))
                    .child(div().flex_1().child(self.query.clone()))
                    .when(searching, |bar| {
                        bar.child(
                            div()
                                .id("clear-search")
                                .cursor_pointer()
                                .text_color(Theme::ink_dim())
                                .on_click(cx.listener(Self::clear))
                                .child("×"),
                        )
                    }),
            )
            .children(
                self.problem
                    .clone()
                    .map(|problem| div().px_2().text_xs().text_color(Theme::warn()).child(problem)),
            )
            .child(div().flex_1().overflow_hidden().child(body))
    }
}
