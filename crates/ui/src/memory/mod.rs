mod ask;
mod tidy;
mod topics;

use app::{Hub, Library, Tidy};
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement, Render,
    ScrollHandle, Styled, Subscription, Window, div, prelude::*, px,
};
use model::markdown::Block;
use model::memory::{Found, Period};
use model::wiki::{Listing, Page};
use model::{SessionId, TopicId};

use crate::brand;
use crate::input::{Submitted, TextInput};
use crate::menu::OpenMenu;
use crate::shell;
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Ask,
    Topics,
    Tidy,
}

impl Tab {
    const ALL: [Self; 3] = [Self::Ask, Self::Topics, Self::Tidy];

    fn label(self) -> &'static str {
        match self {
            Self::Ask => "Ask",
            Self::Topics => "Topics",
            Self::Tidy => "To tidy",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Ask => "icons/search.svg",
            Self::Topics => "icons/book-open.svg",
            Self::Tidy => "icons/tags.svg",
        }
    }
}

/// What the workspace asks Memory to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Focus {
    Topic(TopicId),
    Search(String),
}

pub enum MemoryEvent {
    OpenSession(SessionId),
    TopicDeleted(TopicId),
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
    page_scroll: ScrollHandle,
    tidy: Option<Tidy>,
    menu: Option<OpenMenu>,
    _submitted: [Subscription; 2],
}

impl EventEmitter<MemoryEvent> for MemoryPage {}

impl Focusable for MemoryPage {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.tab {
            Tab::Topics => self.find.focus_handle(cx),
            Tab::Ask | Tab::Tidy => self.query.focus_handle(cx),
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
        let page = Self {
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
            page_scroll: ScrollHandle::new(),
            tidy: None,
            menu: None,
            _submitted: [asked, found],
        };
        page.load_tidy(cx);
        page
    }

    pub fn focus(&mut self, focus: Focus, cx: &mut Context<Self>) {
        match focus {
            Focus::Topic(topic) => {
                self.select(Tab::Topics, cx);
                self.open_topic(topic, cx);
            }
            Focus::Search(text) => {
                self.tab = Tab::Ask;
                self.search(text, cx);
            }
        }
    }

    /// The session a search hit belongs to isn't listed on any reachable node.
    pub fn unlisted(&mut self, cx: &mut Context<Self>) {
        self.problem = Some("That session isn't listed on any node this app can reach.".into());
        cx.notify();
    }

    fn select(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.problem = None;
        match tab {
            Tab::Ask => {}
            Tab::Topics => {
                self.load_library(cx);
                if self.loose.is_empty() {
                    self.load_pages(cx);
                }
            }
            Tab::Tidy => self.load_tidy(cx),
        }
        cx.notify();
    }

    fn failed(&mut self, err: &gateway::GatewayError, cx: &mut Context<Self>) {
        self.problem = Some(err.to_string());
        cx.notify();
    }

    fn tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let waiting = self
            .tidy
            .as_ref()
            .map_or(0, |tidy| tidy.tags.len() + tidy.terms.len());
        div()
            .flex()
            .items_center()
            .gap_1()
            .p_1()
            .rounded(px(18.))
            .bg(Theme::panel())
            .children(Tab::ALL.map(|tab| {
                let here = tab == self.tab;
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
                    .when(tab == Tab::Tidy && waiting > 0, |chip| {
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
            Tab::Ask => self.ask(cx),
            Tab::Topics => self.topics(window, cx),
            Tab::Tidy => self.tidy(cx),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(shell::header().child(shell::title("Memory")).child(self.tabs(cx)))
            .child(shell::tile().flex().flex_col().child(content))
            .children(self.menu.as_ref().map(OpenMenu::overlay))
    }
}
