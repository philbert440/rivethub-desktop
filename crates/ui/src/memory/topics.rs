use gpui::{
    AnyElement, ClickEvent, Context, IntoElement, ParentElement, Point, Styled, Window, div, prelude::*, px,
};
use model::TopicId;
use model::topics::{Atlas, Placements, Topic, outline, suggest};
use model::wiki::{Page, loose, readable, section};

use super::{MemoryPage, Opened, Reading};
use crate::markdown_view;
use crate::text::{ago, now_ms};
use crate::theme::Theme;

const LIST_WIDTH: f32 = 300.;
const PAGE_COLUMN: f32 = 820.;

fn reading(page: Page) -> Reading {
    let blocks = model::markdown::parse(&readable(&page.markdown));
    Reading { page, blocks }
}

impl MemoryPage {
    pub(super) fn load_library(&self, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let loaded = hub.library().await;
            this.update(cx, |this, cx| match loaded {
                Ok(library) => {
                    this.library = Some(library);
                    if let Some(Opened::Topic(topic)) = this.opened.clone() {
                        this.open_topic(topic, cx);
                    }
                    cx.notify();
                }
                Err(err) if err.is_missing_route() => {
                    this.problem = Some("Topic pages need a newer rh on this node.".into());
                    cx.notify();
                }
                Err(err) => this.failed(&err, cx),
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn load_pages(&self, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        let query = self.finding.clone().unwrap_or_default();
        cx.spawn(async move |this, cx| {
            let listed = hub.wiki_pages(query.clone()).await;
            this.update(cx, |this, cx| {
                if this.finding.clone().unwrap_or_default() != query {
                    return;
                }
                match listed {
                    Ok(listed) => {
                        this.loose = listed;
                        cx.notify();
                    }
                    Err(err) => this.failed(&err, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Shows the page that holds this topic: its own, or for a subtopic the root topic's page,
    /// scrolled to the subtopic's section.
    pub(super) fn open_topic(&mut self, topic: TopicId, cx: &mut Context<Self>) {
        let mut at = None;
        self.reading = self.library.as_ref().and_then(|library| {
            let placements = Placements::new();
            let atlas = Atlas::new(&library.topics, &[], &placements);
            let root = atlas.root(&topic);
            let read = library
                .pages
                .iter()
                .find(|page| page.topic.as_ref() == Some(root))
                .cloned()
                .map(reading)?;
            if root != &topic {
                at = library
                    .topics
                    .iter()
                    .find(|known| known.id == topic)
                    .and_then(|sub| section(&read.blocks, &sub.name));
            }
            Some(read)
        });
        match at {
            Some(block) => self.page_scroll.scroll_to_top_of_item(block + 1),
            None => self.page_scroll.set_offset(Point::default()),
        }
        self.opened = Some(Opened::Topic(topic));
        cx.notify();
    }

    fn open_page(&mut self, slug: String, cx: &mut Context<Self>) {
        self.opened = Some(Opened::Page(slug.clone()));
        self.reading = None;
        self.page_scroll.set_offset(Point::default());
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let fetched = hub.wiki_page(slug.clone()).await;
            this.update(cx, |this, cx| {
                if this.opened != Some(Opened::Page(slug)) {
                    return;
                }
                match fetched {
                    Ok(page) => {
                        this.reading = Some(reading(page));
                        cx.notify();
                    }
                    Err(err) => this.failed(&err, cx),
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn clear_find(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.finding = None;
        self.load_pages(cx);
        cx.notify();
    }

    pub(super) fn topics(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        div()
            .size_full()
            .flex()
            .child(self.index(cx))
            .child(div().w(px(1.)).h_full().bg(Theme::line()))
            .child(self.page(window))
            .into_any_element()
    }

    fn row(
        id: (&'static str, usize),
        label: String,
        depth: usize,
        note: String,
        here: bool,
    ) -> gpui::Stateful<gpui::Div> {
        let indent = px(12. + 16. * f32::from(u8::try_from(depth.min(4)).unwrap_or(4)));
        div()
            .id(id)
            .pl(indent)
            .pr_3()
            .py_2()
            .rounded(px(14.))
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .map(|row| {
                if here {
                    row.bg(Theme::selected()).text_color(Theme::on_selected())
                } else {
                    row.text_color(Theme::ink()).hover(|row| row.bg(Theme::card()))
                }
            })
            .child(div().flex_1().min_w_0().truncate().child(label))
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(note),
            )
    }

    fn index(&self, cx: &mut Context<Self>) -> AnyElement {
        let now = now_ms();
        let finding = self.finding.clone();
        let topics: Vec<(usize, &Topic)> = match (&self.library, &finding) {
            (None, _) => Vec::new(),
            (Some(library), None) => outline(&library.topics),
            (Some(library), Some(query)) => suggest(&library.topics, query, usize::MAX)
                .into_iter()
                .map(|topic| (0, topic))
                .collect(),
        };
        let pages = self
            .library
            .as_ref()
            .map(|library| loose(&self.loose, &library.pages))
            .unwrap_or_default();
        let find = div()
            .mx_1()
            .mb_2()
            .px_4()
            .py_2()
            .rounded(px(18.))
            .bg(Theme::card())
            .flex()
            .items_center()
            .gap_2()
            .child(crate::brand::icon("icons/search.svg", 14., Theme::ink_dim()))
            .child(div().flex_1().child(self.find.clone()))
            .when(finding.is_some(), |bar| {
                bar.child(
                    div()
                        .id("clear-find")
                        .cursor_pointer()
                        .text_color(Theme::ink_dim())
                        .on_click(cx.listener(Self::clear_find))
                        .child("×"),
                )
            });
        let heading = |text: String| {
            div()
                .px_3()
                .pt_4()
                .pb_1()
                .text_xs()
                .text_color(Theme::ink_dim())
                .child(text)
        };
        div()
            .id("memory-index")
            .w(px(LIST_WIDTH))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .p_3()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(find)
            .child(heading(match (&finding, &self.library) {
                (Some(query), _) => format!("MATCHING “{query}”"),
                (None, Some(_)) => "TOPICS".to_owned(),
                (None, None) => "Loading topics…".to_owned(),
            }))
            .children(topics.into_iter().enumerate().map(|(index, (depth, topic))| {
                let here = self.opened == Some(Opened::Topic(topic.id.clone()));
                let open = topic.id.clone();
                let active = topic.last_active_ms.map(|at| ago(now, at)).unwrap_or_default();
                Self::row(("memory-topic", index), topic.name.clone(), depth, active, here).on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.open_topic(open.clone(), cx);
                    }),
                )
            }))
            .child(heading("PAGES WITHOUT A TOPIC".into()))
            .children(pages.into_iter().enumerate().map(|(index, listing)| {
                let here = self.opened == Some(Opened::Page(listing.slug.clone()));
                let open = listing.slug.clone();
                Self::row(
                    ("memory-page", index),
                    listing.title.clone(),
                    0,
                    ago(now, listing.updated_ms),
                    here,
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.open_page(open.clone(), cx);
                }))
            }))
            .into_any_element()
    }

    /// The open page. The head and each markdown block are the scroll container's own children,
    /// so a subtopic's section heading is child `block + 1` for the page's scroll handle.
    fn page(&self, window: &Window) -> AnyElement {
        let head = div().flex().flex_col().gap_3().children(
            self.problem
                .clone()
                .map(|problem| div().text_xs().text_color(Theme::warn()).child(problem)),
        );
        let mut blocks = Vec::new();
        let head = match (&self.opened, &self.reading) {
            (_, Some(read)) => {
                blocks = markdown_view::blocks(
                    &read.blocks,
                    &format!("wiki-{}", read.page.slug),
                    Theme::ink().into(),
                    window,
                );
                head.child(
                    div()
                        .text_2xl()
                        .text_color(Theme::ink())
                        .child(read.page.title.clone()),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .children(read.page.tags.iter().map(|tag| {
                            div()
                                .px_2()
                                .py(px(2.))
                                .rounded(px(10.))
                                .bg(Theme::card())
                                .font_family(Theme::MONO)
                                .text_color(Theme::ink_dim())
                                .child(tag.clone())
                        }))
                        .child(
                            div()
                                .text_color(Theme::ink_dim())
                                .child(format!("updated {} ago", ago(now_ms(), read.page.updated_ms))),
                        ),
                )
                .when(read.blocks.is_empty(), |head| {
                    head.child(
                        div()
                            .pt_2()
                            .text_color(Theme::ink_dim())
                            .child("Memory hasn't written anything on this page yet."),
                    )
                })
            }
            (Some(Opened::Topic(_)), None) if self.library.is_some() => head.child(
                div()
                    .text_color(Theme::ink_dim())
                    .child("Memory hasn't started this topic's page yet."),
            ),
            (Some(_), None) => head.child(div().text_color(Theme::ink_dim()).child("Loading…")),
            (None, None) => head.child(
                div()
                    .text_color(Theme::ink_dim())
                    .child("Pick a topic to read its page."),
            ),
        };
        div()
            .id("memory-page")
            .track_scroll(&self.page_scroll)
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .py_8()
            .flex()
            .flex_col()
            .items_center()
            .gap_3()
            .children(
                std::iter::once(head.into_any_element())
                    .chain(blocks)
                    .map(|child| div().w_full().max_w(px(PAGE_COLUMN)).px_10().child(child)),
            )
            .into_any_element()
    }
}
