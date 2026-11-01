mod agents;
mod drag;
mod history;
mod past_topics;
mod rename;
mod search;

pub use drag::{DragGhost, DraggedAgent, DraggedSession};

use std::collections::{HashMap, HashSet};

use app::Hub;
use gpui::{
    AnyElement, ClickEvent, Context, Entity, EventEmitter, IntoElement, ParentElement, Pixels, Point, Render,
    Styled, Subscription, Window, div, prelude::*, px,
};
use model::agents::{Agent, Roster};
use model::history::{HistoryFilter, Phase};
use model::search::SessionMatch;
use model::session::SessionSummary;
use model::topics::Topic;
use model::visibility::Visibility;
use model::{SessionId, TopicId, lineage};

use crate::input::{Submitted, TextInput};
use crate::theme::Theme;
use rename::Renaming;

const HISTORY_PAGE: usize = 100;

#[derive(Debug, Clone)]
pub enum PanelEvent {
    Open(SessionSummary),
    OpenTopic(TopicId),
    Start(Agent),
    NewAgent,
    EditAgent(Agent),
    Renamed(SessionSummary),
    TopicMenu(TopicId, Point<Pixels>),
    SessionMenu(SessionSummary, Point<Pixels>),
}

#[derive(Debug, Clone)]
pub struct PastTopic {
    pub id: TopicId,
    pub name: String,
    pub sessions: usize,
    pub active_ms: i64,
    pub visibility: Option<Visibility>,
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

pub struct SidePanel {
    hub: Hub,
    query: Entity<TextInput>,
    asked: Option<String>,
    results: Option<Vec<SessionMatch>>,
    problem: Option<String>,
    roster: Roster,
    history: Vec<SessionSummary>,
    phases: HashMap<SessionId, Phase>,
    known: Vec<SessionSummary>,
    topics: Vec<Topic>,
    past: Vec<PastTopic>,
    folded_sections: HashSet<Section>,
    history_shown: usize,
    chips: HistoryFilter,
    collapsed: HashSet<SessionId>,
    renaming: Option<Renaming>,
    _query: [Subscription; 2],
}

impl EventEmitter<PanelEvent> for SidePanel {}

impl SidePanel {
    pub fn new(hub: Hub, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| TextInput::new("Filter, or Enter to search transcripts", cx).keeping());
        let submitted = cx.subscribe(&query, |this, _, Submitted(text), cx| {
            if !text.is_empty() {
                this.search(text.clone(), cx);
            }
        });
        // Editing the box away from what was searched drops that search's results, so the list
        // filters by what is typed again until Enter searches it.
        let typed = cx.observe(&query, |this, query, cx| {
            let typed = query.read(cx).text();
            if this.asked.as_deref().is_some_and(|asked| asked != typed) {
                this.asked = None;
                this.results = None;
                this.problem = None;
            }
            cx.notify();
        });
        let panel = Self {
            hub,
            query,
            asked: None,
            results: None,
            problem: None,
            roster: Roster::default(),
            history: Vec::new(),
            phases: HashMap::new(),
            known: Vec::new(),
            topics: Vec::new(),
            past: Vec::new(),
            folded_sections: HashSet::new(),
            history_shown: HISTORY_PAGE,
            chips: HistoryFilter::default(),
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
        phases: HashMap<SessionId, Phase>,
        known: Vec<SessionSummary>,
        cx: &mut Context<Self>,
    ) {
        self.history = history;
        self.phases = phases;
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
                    Ok(roster) => {
                        this.roster = roster;
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

    fn filter(&self, cx: &Context<Self>) -> String {
        self.query.read(cx).text().trim().to_owned()
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
        let history = lineage::keep(&self.history, |session| {
            self.chips.shows(session, self.phase(session), &filter)
        });
        let counts = self
            .chips
            .counts(&self.history, |session| self.phase(session), &filter);
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
                        section.child(self.past_topics(&past, cx))
                    })
                    .child(div().h(px(1.)).mx_2().bg(Theme::line()))
            })
            .child(self.section(Section::History, history_note, None, cx))
            .when(self.is_open(Section::History), |section| {
                section
                    .child(self.history_chips(counts, cx))
                    .child(self.history(&history, cx))
            })
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
