use std::collections::HashSet;

use app::Hub;
use gpui::{
    AnyElement, ClickEvent, Context, Entity, EventEmitter, Focusable, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, Render, SharedString, Styled, Subscription, Window, div, prelude::*, px,
    rgb,
};
use model::agents::{Agent, Roster};
use model::search::SessionMatch;
use model::session::SessionSummary;
use model::topics::Topic;
use model::{SessionId, TopicId, lineage};

use crate::input::{Submitted, TextInput};
use crate::theme::Theme;

const HISTORY_PAGE: usize = 100;
const HISTORY_ROW: &str = "history-row";

#[derive(Debug, Clone)]
pub enum PanelEvent {
    Open(SessionSummary),
    OpenTopic(TopicId),
    Start(Agent),
    NewAgent,
    EditAgent(Agent),
    Renamed(SessionSummary),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Section {
    Agents,
    PastTopics,
    History,
}

impl Section {
    fn label(self) -> &'static str {
        match self {
            Self::Agents => "Agents",
            Self::PastTopics => "Past topics",
            Self::History => "History",
        }
    }
}

struct Renaming {
    session: SessionSummary,
    input: Entity<TextInput>,
    problem: Option<String>,
    _submitted: Subscription,
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
    folded_sections: HashSet<Section>,
    history_shown: usize,
    hide_finished: bool,
    collapsed: HashSet<SessionId>,
    renaming: Option<Renaming>,
    _query: [Subscription; 2],
}

impl EventEmitter<PanelEvent> for SidePanel {}

impl SidePanel {
    pub fn new(hub: Hub, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| TextInput::new("Filter, or Enter to search transcripts", cx));
        let submitted = cx.subscribe(&query, |this, _, Submitted(text), cx| {
            this.search(text.clone(), cx);
        });
        let typed = cx.observe(&query, |_, _, cx| cx.notify());
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
            folded_sections: HashSet::new(),
            history_shown: HISTORY_PAGE,
            hide_finished: false,
            collapsed: HashSet::new(),
            renaming: None,
            _query: [submitted, typed],
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

    pub fn load_agents(&self, cx: &mut Context<Self>) {
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

    fn filter(&self, cx: &Context<Self>) -> String {
        self.query.read(cx).text().trim().to_owned()
    }

    fn begin_rename(&mut self, session: SessionSummary, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            let mut input = TextInput::new("Session title", cx);
            input.set_text(session.title.as_deref().unwrap_or_default().trim(), cx);
            input
        });
        let submitted = cx.subscribe(&input, |this, _, Submitted(title), cx| {
            this.commit_rename(title.clone(), cx);
        });
        window.focus(&input.focus_handle(cx), cx);
        self.renaming = Some(Renaming {
            session,
            input,
            problem: None,
            _submitted: submitted,
        });
        cx.notify();
    }

    fn commit_rename(&mut self, title: String, cx: &mut Context<Self>) {
        let Some(session) = self.renaming.as_ref().map(|renaming| renaming.session.clone()) else {
            return;
        };
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let renamed = hub.rename(&session, title).await;
            this.update(cx, |this, cx| {
                match renamed {
                    Ok(renamed) => {
                        this.renaming = None;
                        cx.emit(PanelEvent::Renamed(renamed));
                    }
                    Err(err) => {
                        if let Some(renaming) = this.renaming.as_mut() {
                            renaming.problem = Some(format!("rename failed: {err}"));
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
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
        &self,
        section: Section,
        note: String,
        action: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let open = self.is_open(section);
        div()
            .id(section.label())
            .h(px(36.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if !this.folded_sections.remove(&section) {
                    this.folded_sections.insert(section);
                }
                cx.notify();
            }))
            .child(
                div()
                    .w(px(12.))
                    .text_color(Theme::ink_dim())
                    .child(if open { "▾" } else { "▸" }),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(Theme::ink())
                    .child(section.label()),
            )
            .child(div().flex_1().text_xs().text_color(Theme::ink_dim()).child(note))
            .children(action)
    }

    fn add_agent(cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("add-agent")
            .size(px(28.))
            .rounded(px(14.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|button| button.bg(Theme::card()))
            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                cx.emit(PanelEvent::NewAgent);
            }))
            .child(crate::brand::icon("icons/plus.svg", 16., Theme::ink_dim()))
            .into_any_element()
    }

    fn hide_finished_toggle(&self, cx: &mut Context<Self>) -> AnyElement {
        let on = self.hide_finished;
        div()
            .id("hide-finished")
            .h(px(24.))
            .px_2()
            .rounded(px(12.))
            .flex()
            .items_center()
            .cursor_pointer()
            .text_xs()
            .map(|chip| {
                if on {
                    chip.bg(Theme::selected()).text_color(Theme::on_selected())
                } else {
                    chip.text_color(Theme::ink_dim())
                        .hover(|chip| chip.bg(Theme::card()).text_color(Theme::ink()))
                }
            })
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.hide_finished = !this.hide_finished;
                cx.stop_propagation();
                cx.notify();
            }))
            .child("Hide finished")
            .into_any_element()
    }

    fn agents(agents: &[&Agent], cx: &mut Context<Self>) -> AnyElement {
        div()
            .pb_2()
            .flex()
            .flex_col()
            .children(agents.iter().enumerate().map(|(index, agent)| {
                let dot = agent.color.map_or(Theme::ink_dim(), rgb);
                let edit = (*agent).clone();
                let row = div()
                    .id(("agent", index))
                    .px_2()
                    .py(px(7.))
                    .rounded(px(12.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .hover(|row| row.bg(Theme::card()))
                    .child(div().flex_none().size(px(8.)).rounded(px(4.)).bg(dot))
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_color(Theme::ink())
                            .child(agent.name.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .child(agent.node.clone().unwrap_or_default()),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                            cx.emit(PanelEvent::EditAgent(edit.clone()));
                        }),
                    );
                if agent.launchable() {
                    let start = (*agent).clone();
                    row.cursor_grab()
                        .on_drag(DraggedAgent((*agent).clone()), |dragged, _, _, cx| {
                            cx.new(|_| DragGhost(format!("Start a session with {}", dragged.0.name).into()))
                        })
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                            cx.emit(PanelEvent::Start(start.clone()));
                        }))
                        .into_any_element()
                } else {
                    row.opacity(0.55).into_any_element()
                }
            }))
            .into_any_element()
    }

    fn is_open(&self, section: Section) -> bool {
        !self.folded_sections.contains(&section)
    }

    /// Agents, past topics and history, narrowed by whatever is typed in the search box.
    fn sections(&self, cx: &mut Context<Self>) -> AnyElement {
        let filter = self.filter(cx);
        let agents: Vec<&Agent> = self
            .roster
            .agents
            .iter()
            .filter(|agent| agent.matches(&filter))
            .collect();
        let past: Vec<&PastTopic> = self
            .past
            .iter()
            .filter(|past| model::search::mentions([past.name.as_str()], &filter))
            .collect();
        let hide_finished = self.hide_finished;
        let history = lineage::keep(&self.history, |session| {
            !(hide_finished && session.status.is_finished()) && session.matches(&filter)
        });
        let history_note = format!("{} without a topic", history.len());
        div()
            .flex()
            .flex_col()
            .child(self.section(
                Section::Agents,
                agents.len().to_string(),
                Some(Self::add_agent(cx)),
                cx,
            ))
            .when(self.is_open(Section::Agents), |section| {
                section.child(Self::agents(&agents, cx))
            })
            .child(div().h(px(1.)).mx_2().bg(Theme::line()))
            .when(!past.is_empty(), |section| {
                section
                    .child(self.section(Section::PastTopics, past.len().to_string(), None, cx))
                    .when(self.is_open(Section::PastTopics), |section| {
                        section.child(Self::past_topics(&past, cx))
                    })
                    .child(div().h(px(1.)).mx_2().bg(Theme::line()))
            })
            .child(self.section(
                Section::History,
                history_note,
                Some(self.hide_finished_toggle(cx)),
                cx,
            ))
            .when(self.is_open(Section::History), |section| {
                section.child(self.history(&history, cx))
            })
            .into_any_element()
    }

    fn harness_badge(session: &SessionSummary) -> impl IntoElement {
        div()
            .flex_none()
            .px(px(6.))
            .rounded(px(6.))
            .bg(Theme::card())
            .text_size(px(10.))
            .text_color(Theme::ink_dim())
            .child(session.id.harness().display_name().to_owned())
    }

    fn rename_row(renaming: &Renaming, indent: gpui::Pixels, cx: &mut Context<Self>) -> AnyElement {
        div()
            .pl(px(8.) + indent)
            .pr_2()
            .py(px(4.))
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px_2()
                            .py_1()
                            .rounded(px(10.))
                            .bg(Theme::card())
                            .border_1()
                            .border_color(Theme::accent())
                            .child(renaming.input.clone()),
                    )
                    .child(
                        div()
                            .id("cancel-history-rename")
                            .cursor_pointer()
                            .text_color(Theme::ink_dim())
                            .hover(|cancel| cancel.text_color(Theme::ink()))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.renaming = None;
                                cx.notify();
                            }))
                            .child("×"),
                    ),
            )
            .children(
                renaming
                    .problem
                    .clone()
                    .map(|problem| div().text_xs().text_color(Theme::warn()).child(problem)),
            )
            .into_any_element()
    }

    fn history(&self, history: &[SessionSummary], cx: &mut Context<Self>) -> AnyElement {
        let rows = lineage::nest(history, &self.collapsed);
        let hidden = rows.len().saturating_sub(self.history_shown);
        div()
            .flex()
            .flex_col()
            .children(
                rows.into_iter()
                    .take(self.history_shown)
                    .enumerate()
                    .map(|(index, row)| self.history_row(index, &row, cx)),
            )
            .when(hidden > 0, |list| {
                list.child(
                    div()
                        .id("history-more")
                        .px_2()
                        .py(px(7.))
                        .cursor_pointer()
                        .text_xs()
                        .text_color(Theme::ink_dim())
                        .hover(|more| more.text_color(Theme::ink()))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.history_shown += HISTORY_PAGE;
                            cx.notify();
                        }))
                        .child(format!("Show {} more of {hidden}", hidden.min(HISTORY_PAGE))),
                )
            })
            .into_any_element()
    }

    fn history_row(&self, index: usize, row: &lineage::Row, cx: &mut Context<Self>) -> AnyElement {
        let indent = px(16. * f32::from(u8::try_from(row.depth.min(4)).unwrap_or(4)));
        if let Some(renaming) = self
            .renaming
            .as_ref()
            .filter(|renaming| renaming.session.id == row.session.id)
        {
            return Self::rename_row(renaming, indent, cx);
        }
        let open = row.session.clone();
        let rename = row.session.clone();
        let fold = row.session.id.clone();
        let lead = if row.children > 0 {
            div()
                .id(("fold", index))
                .cursor_pointer()
                .hover(|fold| fold.text_color(Theme::ink()))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if !this.collapsed.remove(&fold) {
                        this.collapsed.insert(fold.clone());
                    }
                    cx.stop_propagation();
                    cx.notify();
                }))
                .child(if row.collapsed { "▸" } else { "▾" })
                .into_any_element()
        } else {
            div()
                .child(if row.depth > 0 { "↳" } else { "" })
                .into_any_element()
        };
        div()
            .id(("history", index))
            .group(HISTORY_ROW)
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
            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                cx.emit(PanelEvent::Open(open.clone()));
            }))
            .child(
                div()
                    .flex_none()
                    .w(px(12.))
                    .text_color(Theme::ink_dim())
                    .child(lead),
            )
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .text_color(Theme::ink())
                    .child(crate::text::one_line(row.session.title.as_deref())),
            )
            .child(
                div()
                    .id(("rename", index))
                    .flex_none()
                    .invisible()
                    .group_hover(HISTORY_ROW, gpui::StyleRefinement::visible)
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        cx.stop_propagation();
                        this.begin_rename(rename.clone(), window, cx);
                    }))
                    .child(crate::brand::icon("icons/rename.svg", 13., Theme::ink_dim())),
            )
            .child(Self::harness_badge(&row.session))
            .when(row.children > 0, |line| {
                line.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(Theme::ink_dim())
                        .child(row.children.to_string()),
                )
            })
            .into_any_element()
    }

    fn past_topics(past: &[&PastTopic], cx: &mut Context<Self>) -> AnyElement {
        let now = crate::text::now_ms();
        div()
            .flex()
            .flex_col()
            .pb_2()
            .children(past.iter().enumerate().map(|(index, past)| {
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
                    // A topic with no activity has no age to show, not one since 1970.
                    .when(past.active_ms > 0, |row| {
                        row.child(
                            div()
                                .text_xs()
                                .text_color(Theme::ink_dim())
                                .child(crate::text::ago(now, past.active_ms)),
                        )
                    })
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
            _ => self.sections(cx),
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
            .child(
                div()
                    .id("panel-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            )
    }
}
