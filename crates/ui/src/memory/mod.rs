mod ask;
mod conversation;
mod decisions;
mod entities;
mod entity_edit;
mod flagged;
mod links;
mod members;
mod mentions;
mod proposals;
mod recalled;
mod session;
mod sessions;
mod stats;
mod tidy;
mod topics;
mod trash;

use std::sync::Arc;

use app::{Hub, Library, Tidy};
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement, Render,
    ScrollHandle, Styled, Subscription, Window, div, prelude::*, px,
};
use model::decisions::Group;
use model::entities::Catalog;
use model::markdown::Block;
use model::memory::{Found, Period, Stats};
use model::session::SessionSummary;
use model::tidy::Upkeep;
use model::topics::FiledConversation;
use model::wiki::{Listing, Page};
use model::{SessionId, TopicId};

use crate::brand;
use crate::input::{Submitted, TextInput};
use crate::menu::OpenMenu;
use crate::shell;
use crate::theme::Theme;
use crate::workspace::Listed;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Ask,
    Topics,
    Entities,
    Stats,
}

impl Tab {
    const ALL: [Self; 4] = [Self::Ask, Self::Topics, Self::Entities, Self::Stats];

    fn label(self) -> &'static str {
        match self {
            Self::Ask => "Ask",
            Self::Topics => "Topics",
            Self::Entities => "Entities",
            Self::Stats => "Stats",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Ask => "icons/search.svg",
            Self::Topics => "icons/book-open.svg",
            Self::Entities => "icons/library.svg",
            Self::Stats => "icons/tags.svg",
        }
    }
}

/// What the workspace asks Memory to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Focus {
    Topic(TopicId),
    Session(Box<SessionSummary>),
    /// A filed conversation that is no harness session.
    Conversation(Box<FiledConversation>),
}

pub enum MemoryEvent {
    OpenSession(SessionId),
    OpenTopic(TopicId),
    TopicDeleted(TopicId),
    /// Memory filed or unfiled something, so the workspace lists topics again.
    Refiled,
    /// The trash review deleted these sessions, which leave the workspace's lists too.
    SessionsDeleted(Vec<SessionId>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Opened {
    Topic(TopicId),
    Page(String),
}

struct Reading {
    page: Page,
    blocks: Vec<Block>,
}

/// The Memory page: ask the mesh's memory, read each topic's wiki page, and tidy what needs it.
pub struct MemoryPage {
    hub: Hub,
    tab: Tab,
    problem: Option<String>,
    query: Entity<TextInput>,
    period: Period,
    asked: Option<String>,
    found: Option<Vec<Found>>,
    library: Option<Library>,
    find: Entity<TextInput>,
    finding: Option<String>,
    loose: Vec<Listing>,
    opened: Option<Opened>,
    reading: Option<Reading>,
    decisions: Option<Vec<Group>>,
    deciding: Option<decisions::Deciding>,
    page_scroll: ScrollHandle,
    tidy: Option<Tidy>,
    review: proposals::Review,
    trash: trash::Sweep,
    stats: Option<Stats>,
    upkeep: Option<Upkeep>,
    flagged: flagged::Flagged,
    catalog: Option<Catalog>,
    browse: entities::Browse,
    links: links::Links,
    entity_find: Entity<TextInput>,
    viewing: Option<session::Viewing>,
    remembering: Option<conversation::Remembered>,
    listed: Arc<Listed>,
    all_sessions: bool,
    shown: bool,
    menu: Option<OpenMenu>,
    _submitted: [Subscription; 3],
}

impl EventEmitter<MemoryEvent> for MemoryPage {}

impl Focusable for MemoryPage {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.tab {
            Tab::Topics => self.find.focus_handle(cx),
            Tab::Entities => self.entity_find.focus_handle(cx),
            Tab::Ask | Tab::Stats => self.query.focus_handle(cx),
        }
    }
}

impl MemoryPage {
    pub fn new(hub: Hub, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| TextInput::new("Search what your sessions said", cx));
        let find = cx.new(|cx| TextInput::new("Find a topic or page", cx));
        let asked = cx.subscribe(&query, |this, _, Submitted(text), cx| {
            this.search(text.clone(), cx);
        });
        let found = cx.subscribe(&find, |this, _, Submitted(text), cx| {
            this.finding = Some(text.clone());
            this.load_pages(cx);
        });
        let entity_find = cx.new(|cx| TextInput::new("Find an entity by name or alias", cx));
        let sought = cx.subscribe(&entity_find, |this, _, Submitted(text), cx| {
            this.find_entities(text, cx);
        });
        let mut page = Self {
            hub,
            tab: Tab::Ask,
            problem: None,
            query,
            period: Period::AnyTime,
            asked: None,
            found: None,
            library: None,
            find,
            finding: None,
            loose: Vec::new(),
            opened: None,
            reading: None,
            decisions: None,
            deciding: None,
            page_scroll: ScrollHandle::new(),
            tidy: None,
            review: proposals::Review::default(),
            trash: trash::Sweep::default(),
            stats: None,
            upkeep: None,
            flagged: flagged::Flagged::default(),
            catalog: None,
            browse: entities::Browse::default(),
            links: links::Links::default(),
            entity_find,
            viewing: None,
            remembering: None,
            listed: Arc::default(),
            all_sessions: false,
            shown: false,
            menu: None,
            _submitted: [asked, found, sought],
        };
        page.load_tidy(cx);
        page.load_trash(cx);
        page
    }

    pub fn focus(&mut self, focus: Focus, cx: &mut Context<Self>) {
        match focus {
            Focus::Topic(topic) => {
                self.select(Tab::Topics, cx);
                self.open_topic(topic, cx);
            }
            Focus::Session(session) => self.open_session(*session, cx),
            Focus::Conversation(conversation) => self.open_conversation(*conversation, true, cx),
        }
    }

    /// The workspace's latest listing, which keeps the topic index and an open session's name current.
    pub fn listed(&mut self, listed: Arc<Listed>, cx: &mut Context<Self>) {
        if let Some(library) = self.library.as_mut() {
            library.topics.clone_from(&listed.topics);
        }
        if let Some(viewing) = self.viewing.as_mut() {
            viewing.relist(&listed.sessions);
        }
        self.listed = listed;
        cx.notify();
    }

    /// Whether the shell is showing Memory; coming back to it refreshes the open session's page.
    pub fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        let back = shown && !self.shown;
        self.shown = shown;
        if back {
            self.refresh_session(cx);
        }
    }

    /// The session a search hit belongs to isn't listed on any reachable node.
    pub fn unlisted(&mut self, cx: &mut Context<Self>) {
        self.problem = Some("That session isn't listed on any node this app can reach.".into());
        cx.notify();
    }

    fn select(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.viewing = None;
        self.remembering = None;
        self.problem = None;
        match tab {
            Tab::Ask => {}
            Tab::Topics => {
                self.load_library(cx);
                if self.loose.is_empty() {
                    self.load_pages(cx);
                }
            }
            Tab::Entities => self.load_catalog(cx),
            Tab::Stats => {
                self.load_tidy(cx);
                self.load_trash(cx);
                self.load_stats(cx);
                self.load_flagged(cx);
                self.load_catalog(cx);
            }
        }
        cx.notify();
    }

    fn failed(&mut self, err: &gateway::GatewayError, cx: &mut Context<Self>) {
        self.problem = Some(err.to_string());
        cx.notify();
    }

    fn tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let waiting = self.tidy.as_ref().map_or(0, app::Tidy::waiting) + self.trash.waiting();
        div()
            .flex()
            .items_center()
            .gap_1()
            .p_1()
            .rounded(px(18.))
            .bg(Theme::panel())
            .children(Tab::ALL.map(|tab| {
                let here = tab == self.tab && self.viewing.is_none() && self.remembering.is_none();
                let ink = if here {
                    Theme::on_selected()
                } else {
                    Theme::ink_dim()
                };
                div()
                    .id(tab.label())
                    .h(px(30.))
                    .px_4()
                    .rounded(px(15.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .map(|chip| {
                        if here {
                            chip.bg(Theme::selected()).text_color(Theme::on_selected())
                        } else {
                            chip.text_color(Theme::ink_dim())
                                .hover(|chip| chip.bg(Theme::card()))
                        }
                    })
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.select(tab, cx);
                    }))
                    .child(brand::icon(tab.icon(), 14., ink))
                    .child(tab.label())
                    .when(tab == Tab::Stats && waiting > 0, |chip| {
                        chip.child(
                            div()
                                .px_2()
                                .rounded(px(9.))
                                .bg(Theme::warn_container())
                                .text_xs()
                                .text_color(Theme::warn())
                                .child(waiting.to_string()),
                        )
                    })
            }))
    }
}

impl Render for MemoryPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.tab {
            _ if self.viewing.is_some() => self.session_page(cx),
            _ if self.remembering.is_some() => self.conversation_page(cx),
            Tab::Ask => self.ask(cx),
            Tab::Topics => self.topics(window, cx),
            Tab::Entities => self.entities(window, cx),
            Tab::Stats => self.stats(cx),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(shell::header().child(shell::title("Memory")).child(self.tabs(cx)))
            .child(shell::tile().flex().flex_col().child(content))
            .children(self.menu.as_ref().map(OpenMenu::overlay))
            .children(self.choosing_overlay(cx))
    }
}
