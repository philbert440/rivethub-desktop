use gpui::{
    AnyElement, ClickEvent, Context, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, Window,
    div, prelude::*, px,
};
use model::TopicId;
use model::entities::{Catalog, Entity};
use model::markdown::Block;
use model::wiki::{Owned, Page, readable};

use super::MemoryPage;
use super::entity_edit::{Choosing, Editing, Field};
use crate::markdown_view;
use crate::menu::Noun;
use crate::text::{ago, now_ms};
use crate::theme::Theme;

const LIST_WIDTH: f32 = 300.;
const PAGE_COLUMN: f32 = 820.;

/// An entity's page with its markdown parsed.
pub(super) struct EntityReading {
    pub(super) page: Page,
    pub(super) blocks: Vec<Block>,
}

impl EntityReading {
    pub(super) fn new(page: Page) -> Self {
        let blocks = model::markdown::parse_page(&readable(&page.markdown));
        Self { page, blocks }
    }
}

/// Where the Entities tab stands: the type picked, the entity open and its page, a search, and
/// whatever is being edited.
#[derive(Default)]
pub(super) struct Browse {
    pub(super) kind: Option<TopicId>,
    pub(super) entity: Option<TopicId>,
    pub(super) reading: Option<EntityReading>,
    pub(super) unwritten: bool,
    pub(super) finding: Option<String>,
    pub(super) renaming: Option<Field>,
    pub(super) aliasing: Option<Field>,
    pub(super) editing: Option<Editing>,
    pub(super) choosing: Option<Choosing>,
    pub(super) all_mentions: bool,
}

impl MemoryPage {
    pub(super) fn load_catalog(&self, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let loaded = hub.catalog().await;
            this.update(cx, |this, cx| match loaded {
                Ok(catalog) => {
                    let gone = this
                        .browse
                        .entity
                        .as_ref()
                        .is_some_and(|entity| catalog.entity(entity).is_none());
                    if gone {
                        this.browse.entity = None;
                        this.browse.reading = None;
                    }
                    if this
                        .browse
                        .kind
                        .as_ref()
                        .is_some_and(|kind| catalog.kind(kind).is_none())
                    {
                        this.browse.kind = None;
                    }
                    this.catalog = Some(catalog);
                    cx.notify();
                }
                Err(err) if err.is_missing_route() => {
                    this.problem = Some("Entities need a newer rh on this node.".into());
                    cx.notify();
                }
                Err(err) => this.failed(&err, cx),
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn find_entities(&mut self, text: &str, cx: &mut Context<Self>) {
        self.browse.finding = Some(text.trim().to_owned()).filter(|text| !text.is_empty());
        cx.notify();
    }

    fn pick_type(&mut self, kind: TopicId, cx: &mut Context<Self>) {
        self.browse.kind = Some(kind);
        self.browse.entity = None;
        self.browse.reading = None;
        self.browse.finding = None;
        self.entity_find.update(cx, |input, cx| input.set_text("", cx));
        cx.notify();
    }

    pub(super) fn open_entity(&mut self, entity: TopicId, cx: &mut Context<Self>) {
        self.browse.entity = Some(entity.clone());
        self.browse.reading = None;
        self.browse.unwritten = false;
        self.browse.editing = None;
        self.browse.aliasing = None;
        self.browse.all_mentions = false;
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let fetched = hub.topic_page(entity.clone()).await;
            this.update(cx, |this, cx| {
                if this.browse.entity.as_ref() != Some(&entity) {
                    return;
                }
                match fetched {
                    Ok(Owned { page: Some(page), .. }) => {
                        this.browse.reading = Some(EntityReading::new(page));
                        this.check_links(cx);
                    }
                    Ok(Owned { page: None, .. }) => this.browse.unwritten = true,
                    Err(err) => this.failed(&err, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(super) fn entities(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        div()
            .size_full()
            .flex()
            .child(self.type_index(cx))
            .child(div().w(px(1.)).h_full().bg(Theme::line()))
            .child(self.entity_side(window, cx))
            .into_any_element()
    }

    fn type_index(&self, cx: &mut Context<Self>) -> AnyElement {
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
            .child(div().flex_1().child(self.entity_find.clone()));
        let rows: Vec<AnyElement> = self.catalog.as_ref().map_or_else(Vec::new, |catalog| {
            catalog
                .outline()
                .into_iter()
                .enumerate()
                .map(|(index, (depth, kind))| self.type_row(catalog, index, depth, kind.id.clone(), cx))
                .collect()
        });
        div()
            .id("entity-index")
            .w(px(LIST_WIDTH))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .p_3()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(find)
            .child(
                div()
                    .px_3()
                    .pt_4()
                    .pb_1()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(if self.catalog.is_some() {
                        "TYPES"
                    } else {
                        "Loading types…"
                    }),
            )
            .children(rows)
            .into_any_element()
    }

    fn type_row(
        &self,
        catalog: &Catalog,
        index: usize,
        depth: usize,
        kind: TopicId,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(field) = Self::field_for(self.browse.renaming.as_ref(), &kind, cx) {
            return div()
                .pl(px(12. + 16. * depth_px(depth)))
                .child(field)
                .into_any_element();
        }
        let name = catalog
            .kind(&kind)
            .map(|known| known.name.clone())
            .unwrap_or_default();
        let here = self.browse.kind.as_ref() == Some(&kind) && self.browse.finding.is_none();
        let count = catalog.within(&kind).len();
        let (open, menu) = (kind.clone(), kind);
        Self::row(("entity-type", index), name, depth, count.to_string(), here)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.pick_type(open.clone(), cx)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.entity_menu(menu.clone(), Noun::Type, event.position, window, cx);
                }),
            )
            .into_any_element()
    }

    fn entity_side(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let body = match (
            &self.catalog,
            &self.browse.finding,
            &self.browse.entity,
            &self.browse.kind,
        ) {
            (None, ..) => Self::quiet("Loading…"),
            (Some(catalog), Some(query), ..) => self.entity_list(
                &format!("MATCHING “{query}”"),
                catalog.find(query),
                "Nothing matches.",
                cx,
            ),
            (Some(_), None, Some(entity), _) => self.entity_page(entity, window, cx),
            (Some(catalog), None, None, Some(kind)) => {
                let path = catalog
                    .path(kind)
                    .iter()
                    .map(|kind| kind.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" › ");
                self.entity_list(
                    &path.to_uppercase(),
                    catalog.within(kind),
                    "The wiki hasn't filed anything here yet.",
                    cx,
                )
            }
            (Some(catalog), None, None, None) => Self::quiet(&format!(
                "{} entities in {} types. Pick a type to see what the wiki knows.",
                catalog.entities.len(),
                catalog.types.len()
            )),
        };
        div()
            .id("entity-side")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .py_8()
            .flex()
            .flex_col()
            .items_center()
            .child(div().w_full().max_w(px(PAGE_COLUMN)).px_10().child(body))
            .into_any_element()
    }

    fn quiet(text: &str) -> AnyElement {
        div()
            .text_color(Theme::ink_dim())
            .child(text.to_owned())
            .into_any_element()
    }

    fn entity_list(
        &self,
        heading: &str,
        entities: Vec<&Entity>,
        empty: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let now = now_ms();
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(
                self.problem
                    .clone()
                    .map(|problem| div().pb_2().text_xs().text_color(Theme::warn()).child(problem)),
            )
            .child(
                div()
                    .px_3()
                    .pb_2()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(heading.to_owned()),
            )
            .when(entities.is_empty(), |list| {
                list.child(div().px_3().text_color(Theme::ink_dim()).child(empty))
            })
            .children(entities.into_iter().enumerate().map(|(index, entity)| {
                let (open, menu) = (entity.topic.id.clone(), entity.topic.id.clone());
                let also = entity
                    .aliases
                    .iter()
                    .take(2)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                Self::row(
                    ("entity", index),
                    entity.topic.name.clone(),
                    0,
                    if also.is_empty() {
                        ago(now, entity.topic.updated_ms)
                    } else {
                        also
                    },
                    false,
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_entity(open.clone(), cx)))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        this.entity_menu(menu.clone(), Noun::Entity, event.position, window, cx);
                    }),
                )
            }))
            .into_any_element()
    }

    fn entity_page(&self, id: &TopicId, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(entity) = self.catalog.as_ref().and_then(|catalog| catalog.entity(id)) else {
            return Self::quiet("This entity is gone.");
        };
        let path = self
            .catalog
            .as_ref()
            .map(|catalog| catalog.path(id))
            .unwrap_or_default();
        let crumbs =
            div()
                .flex()
                .items_center()
                .gap_1()
                .text_xs()
                .children(path.into_iter().enumerate().map(|(index, kind)| {
                    let open = kind.id.clone();
                    div()
                        .id(("entity-crumb", index))
                        .px_2()
                        .rounded(px(9.))
                        .bg(Theme::surface())
                        .cursor_pointer()
                        .text_color(Theme::accent())
                        .hover(|chip| chip.bg(Theme::card()))
                        .on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| this.pick_type(open.clone(), cx)),
                        )
                        .child(kind.name.clone())
                }));
        let menu = id.clone();
        let title = Self::field_for(self.browse.renaming.as_ref(), id, cx).unwrap_or_else(|| {
            div()
                .text_2xl()
                .text_color(Theme::ink())
                .child(entity.topic.name.clone())
                .into_any_element()
        });
        let head = div()
            .flex()
            .items_center()
            .gap_3()
            .child(div().flex_1().min_w_0().child(title))
            .child(
                div()
                    .id("entity-menu")
                    .size(px(28.))
                    .rounded(px(14.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_color(Theme::ink_dim())
                    .hover(|button| button.bg(Theme::raised()).text_color(Theme::ink()))
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        this.entity_menu(menu.clone(), Noun::Entity, event.position(), window, cx);
                    }))
                    .child("⋯"),
            );
        let body = match (&self.browse.reading, self.browse.unwritten) {
            (Some(reading), _) => self.page_body(id, reading, window, cx),
            (None, true) => self.unwritten_body(id, cx),
            (None, false) => Self::quiet("Loading…"),
        };
        let mentioned = self
            .browse
            .reading
            .as_ref()
            .and_then(|reading| self.mentioned_in(&reading.page, cx));
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(crumbs)
            .child(head)
            .child(self.aliases(entity, cx))
            .children(
                self.problem
                    .clone()
                    .map(|problem| div().text_xs().text_color(Theme::warn()).child(problem)),
            )
            .child(body)
            .children(mentioned)
            .into_any_element()
    }

    /// An entity the wiki hasn't written a page for yet; writing its text writes the page.
    fn unwritten_body(&self, id: &TopicId, cx: &mut Context<Self>) -> AnyElement {
        if let Some(field) = self.edit_field(id, cx) {
            return field;
        }
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(Self::edit_buttons(id, String::new(), String::new(), cx))
            .child(
                div()
                    .text_color(Theme::ink_dim())
                    .child("The wiki hasn't written this entity's page yet."),
            )
            .into_any_element()
    }

    fn page_body(
        &self,
        id: &TopicId,
        reading: &EntityReading,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(field) = self.edit_field(id, cx) {
            return field;
        }
        let blocks = markdown_view::page_blocks(
            &reading.blocks,
            &format!("entity-{}", reading.page.slug),
            Theme::ink().into(),
            window,
            &self.page_links(cx),
        );
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(Self::edit_buttons(
                id,
                reading.page.current_state.clone(),
                reading.page.article.clone().unwrap_or_default(),
                cx,
            ))
            .children(blocks)
            .into_any_element()
    }
}

fn depth_px(depth: usize) -> f32 {
    f32::from(u8::try_from(depth.min(4)).unwrap_or(4))
}
