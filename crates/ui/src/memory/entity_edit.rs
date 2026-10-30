use std::future::Future;

use gateway::GatewayError;
use gpui::{
    AnyElement, ClickEvent, Context, Entity, Focusable, IntoElement, MouseDownEvent, ParentElement, Pixels,
    Point, Styled, Subscription, Window, anchored, deferred, div, prelude::*, px,
};
use model::TopicId;
use model::entities::{Catalog, Entity as Known};
use model::topics::Topic;
use model::wiki::{Owned, PageEdit};

use super::MemoryPage;
use super::entities::EntityReading;
use super::sessions::link_button;
use crate::input::{Submitted, TextInput};
use crate::menu::{Lift, Menu, MenuEvent, Noun, OpenMenu};
use crate::theme::Theme;
use crate::topic_picker::{PickerEvent, TopicPicker};
use crate::workspace::CancelRetitle;

/// A one-line field editing something in place: a name, or a new alias.
pub(super) struct Field {
    target: TopicId,
    input: Entity<TextInput>,
    _submitted: Subscription,
}

/// Which part of an entity's page is being edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Part {
    Now,
    Article,
}

/// An entity page's text being edited in place.
pub(super) struct Editing {
    part: Part,
    input: Entity<TextInput>,
    _submitted: Subscription,
}

/// The floating picker for a new type, or for what to merge into.
pub(super) struct Choosing {
    picker: Entity<TopicPicker>,
    at: Point<Pixels>,
    _events: Subscription,
}

/// What a pick in the floating picker does.
#[derive(Debug, Clone)]
enum Pick {
    Under(TopicId),
    Merge(TopicId),
}

impl MemoryPage {
    /// The right-click or ⋯ menu on a type or an entity.
    pub(super) fn entity_menu(
        &mut self,
        id: TopicId,
        noun: Noun,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(catalog) = self.catalog.as_ref() else {
            return;
        };
        let (name, parent) = match noun {
            Noun::Entity => match catalog.entity(&id) {
                Some(entity) => (entity.topic.name.clone(), entity.topic.parent.clone()),
                None => return,
            },
            Noun::Type | Noun::Topic => match catalog.kind(&id) {
                Some(kind) => (kind.name.clone(), kind.parent.clone()),
                None => return,
            },
        };
        let up = match noun {
            Noun::Type => parent
                .as_ref()
                .map(|parent| Lift(catalog.kind(parent).and_then(|above| above.parent.clone()))),
            Noun::Entity | Noun::Topic => None,
        };
        let movable = match noun {
            Noun::Entity => catalog.types.iter().any(|kind| Some(&kind.id) != parent.as_ref()),
            Noun::Type | Noun::Topic => catalog
                .types
                .iter()
                .any(|kind| Some(&kind.id) != parent.as_ref() && catalog.can_nest(&id, &kind.id)),
        };
        let hub = self.hub.clone();
        let target = id.clone();
        let menu = cx.new(|_| {
            Menu::new(hub, None, Some((id, name)))
                .editing(target.clone(), up, movable)
                .naming(noun)
        });
        let events = cx.subscribe_in(&menu, window, move |this, menu, event: &MenuEvent, window, cx| {
            if this.menu.as_ref().is_none_or(|open| &open.menu != menu) {
                return;
            }
            this.menu = None;
            match event {
                MenuEvent::Rename(topic) => this.begin_rename(topic.clone(), window, cx),
                MenuEvent::MoveInto(topic) => this.choose(Pick::Under(topic.clone()), at, window, cx),
                MenuEvent::MergeInto(topic) => this.choose(Pick::Merge(topic.clone()), at, window, cx),
                MenuEvent::Deleted(_) | MenuEvent::Filed => this.load_catalog(cx),
                MenuEvent::ViewInMemory(_)
                | MenuEvent::Open(_)
                | MenuEvent::MoveSession(_)
                | MenuEvent::Unplace(_)
                | MenuEvent::SessionDeleted(_)
                | MenuEvent::TaskStopped
                | MenuEvent::PluginAction { .. }
                | MenuEvent::Closed => {}
            }
            cx.notify();
        });
        self.menu = Some(OpenMenu {
            menu,
            at,
            _events: events,
        });
        cx.notify();
    }

    fn begin_rename(&mut self, target: TopicId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(catalog) = self.catalog.as_ref() else {
            return;
        };
        let name = catalog
            .entity(&target)
            .map(|entity| entity.topic.name.clone())
            .or_else(|| catalog.kind(&target).map(|kind| kind.name.clone()))
            .unwrap_or_default();
        let (renamed, shown) = (target.clone(), name.clone());
        let field = Self::field(&shown, "Name", window, cx, move |this, text, cx| {
            this.browse.renaming = None;
            if text.is_empty() || text == name {
                return;
            }
            let (hub, topic) = (this.hub.clone(), renamed.clone());
            this.change(
                async move { hub.rename_topic(topic, text).await },
                "renaming it failed",
                cx,
            );
        });
        self.browse.renaming = Some(Field { target, ..field });
        cx.notify();
    }

    pub(super) fn begin_alias(&mut self, entity: &Known, window: &mut Window, cx: &mut Context<Self>) {
        let (target, mut aliases) = (entity.topic.id.clone(), entity.aliases.clone());
        let topic = target.clone();
        let field = Self::field("", "Another name", window, cx, move |this, text, cx| {
            this.browse.aliasing = None;
            if text.is_empty() || aliases.contains(&text) {
                return;
            }
            aliases.push(text);
            let (hub, topic, aliases) = (this.hub.clone(), topic.clone(), aliases.clone());
            this.change(
                async move { hub.set_aliases(topic, aliases).await },
                "adding it failed",
                cx,
            );
        });
        self.browse.aliasing = Some(Field { target, ..field });
        cx.notify();
    }

    fn drop_alias(&mut self, entity: &Known, alias: &str, cx: &mut Context<Self>) {
        let aliases = entity
            .aliases
            .iter()
            .filter(|known| known.as_str() != alias)
            .cloned()
            .collect();
        let (hub, topic) = (self.hub.clone(), entity.topic.id.clone());
        self.change(
            async move { hub.set_aliases(topic, aliases).await },
            "removing it failed",
            cx,
        );
    }

    fn begin_edit(&mut self, part: Part, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            let mut input = TextInput::new("What the wiki should say", cx).multiline();
            input.set_text(text, cx);
            input
        });
        let submitted = cx.subscribe(&input, move |this, _, Submitted(text), cx| {
            this.save_text(part, text.clone(), cx);
        });
        window.focus(&input.focus_handle(cx), cx);
        self.browse.editing = Some(Editing {
            part,
            input,
            _submitted: submitted,
        });
        cx.notify();
    }

    fn save_text(&mut self, part: Part, text: String, cx: &mut Context<Self>) {
        self.browse.editing = None;
        cx.notify();
        let Some(entity) = self.browse.entity.clone() else {
            return;
        };
        let edit = match part {
            Part::Now => PageEdit {
                current_state: Some(text),
                ..PageEdit::default()
            },
            Part::Article => PageEdit {
                article: Some(text),
                ..PageEdit::default()
            },
        };
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let saved = hub.edit_page(entity.clone(), edit).await;
            this.update(cx, |this, cx| {
                if this.browse.entity.as_ref() != Some(&entity) {
                    return;
                }
                match saved {
                    Ok(Owned { page: Some(page), .. }) => {
                        this.browse.reading = Some(EntityReading::new(page));
                        this.browse.unwritten = false;
                    }
                    Ok(Owned { page: None, .. }) => this.problem = Some("rh kept no page for it.".into()),
                    Err(err) => this.problem = Some(format!("saving it failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn cancel_edits(&mut self, _: &CancelRetitle, _: &mut Window, cx: &mut Context<Self>) {
        self.browse.renaming = None;
        self.browse.aliasing = None;
        self.browse.editing = None;
        cx.notify();
    }

    /// A focused one-line field that hands its trimmed text to `done` on Enter; Esc drops it.
    fn field(
        text: &str,
        placeholder: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
        mut done: impl FnMut(&mut Self, String, &mut Context<Self>) + 'static,
    ) -> Field {
        let input = cx.new(|cx| {
            let mut input = TextInput::new(placeholder, cx);
            input.set_text(text, cx);
            input
        });
        let submitted = cx.subscribe(&input, move |this, _, Submitted(text), cx| {
            done(this, text.trim().to_owned(), cx);
            cx.notify();
        });
        window.focus(&input.focus_handle(cx), cx);
        Field {
            target: TopicId::new(String::new()),
            input,
            _submitted: submitted,
        }
    }

    /// The field editing `target` in place, if that's the one being edited.
    pub(super) fn field_for(
        field: Option<&Field>,
        target: &TopicId,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let field = field.filter(|field| &field.target == target)?;
        Some(Self::framed(field.input.clone(), cx))
    }

    fn framed(input: Entity<TextInput>, cx: &mut Context<Self>) -> AnyElement {
        div()
            .key_context("TopicRetitle")
            .on_action(cx.listener(Self::cancel_edits))
            .min_w(px(180.))
            .px_2()
            .py_1()
            .rounded(px(10.))
            .bg(Theme::panel())
            .border_1()
            .border_color(Theme::accent())
            .text_color(Theme::ink())
            .child(input)
            .into_any_element()
    }

    /// The page text being edited, with what Enter and Esc do under it.
    pub(super) fn edit_field(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let editing = self.browse.editing.as_ref()?;
        let what = match editing.part {
            Part::Now => "What it is now",
            Part::Article => "Article",
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().text_xs().text_color(Theme::ink_dim()).child(what))
                .child(Self::framed(editing.input.clone(), cx))
                .child(
                    div()
                        .text_xs()
                        .text_color(Theme::ink_dim())
                        .child("Enter saves · Shift+Enter starts a new line · Esc cancels"),
                )
                .into_any_element(),
        )
    }

    pub(super) fn edit_buttons(now: String, article: String, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .gap_2()
            .child(
                link_button("entity-edit-now", "Edit what it is now").on_click(cx.listener(
                    move |this, _: &ClickEvent, window, cx| this.begin_edit(Part::Now, &now, window, cx),
                )),
            )
            .child(
                link_button(
                    "entity-edit-article",
                    if article.is_empty() {
                        "Add an article"
                    } else {
                        "Edit article"
                    },
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.begin_edit(Part::Article, &article, window, cx);
                })),
            )
    }

    /// The entity's other names, each removable, and a field to add one.
    pub(super) fn aliases(&self, entity: &Known, cx: &mut Context<Self>) -> impl IntoElement {
        let adding = Self::field_for(self.browse.aliasing.as_ref(), &entity.topic.id, cx);
        let known = entity.clone();
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .text_xs()
            .children(entity.aliases.iter().enumerate().map(|(index, alias)| {
                let (from, alias) = (entity.clone(), alias.clone());
                let shown = alias.clone();
                div()
                    .px_2()
                    .py(px(2.))
                    .rounded(px(10.))
                    .bg(Theme::card())
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_color(Theme::ink_dim())
                    .child(shown)
                    .child(
                        div()
                            .id(("drop-alias", index))
                            .cursor_pointer()
                            .hover(|cross| cross.text_color(Theme::ink()))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.drop_alias(&from, &alias, cx);
                            }))
                            .child("×"),
                    )
            }))
            .child(adding.unwrap_or_else(|| {
                div()
                    .id("add-alias")
                    .px_2()
                    .py(px(2.))
                    .rounded(px(10.))
                    .cursor_pointer()
                    .text_color(Theme::ink_dim())
                    .hover(|chip| chip.bg(Theme::card()).text_color(Theme::ink()))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.begin_alias(&known, window, cx);
                    }))
                    .child("+ Another name")
                    .into_any_element()
            }))
    }

    /// Opens the picker for a type change or a merge, where the menu was.
    fn choose(&mut self, pick: Pick, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(catalog) = self.catalog.as_ref() else {
            return;
        };
        let (heading, choices, creates) = choices(catalog, &pick);
        let noun = match &pick {
            Pick::Merge(id) if catalog.entity(id).is_some() => "entity",
            Pick::Under(_) | Pick::Merge(_) => "type",
        };
        let picker = cx.new(|cx| {
            let picker = TopicPicker::new(choices, None, cx)
                .headed(heading)
                .naming(noun, cx);
            if creates { picker } else { picker.without_create() }
        });
        let events = cx.subscribe(&picker, move |this, _, event: &PickerEvent, cx| {
            this.browse.choosing = None;
            let hub = this.hub.clone();
            match (event, &pick) {
                (PickerEvent::Chose(into), Pick::Under(id)) => {
                    let (id, into) = (id.clone(), into.clone());
                    this.change(
                        async move { hub.move_topic(id, Some(into)).await },
                        "moving it failed",
                        cx,
                    );
                }
                (PickerEvent::Create(name), Pick::Under(id)) => {
                    let (id, name) = (id.clone(), name.trim().to_owned());
                    this.change(
                        async move {
                            let kind = hub.create_type(name, None).await?;
                            hub.move_topic(id, Some(kind.id)).await
                        },
                        "moving it failed",
                        cx,
                    );
                }
                (PickerEvent::Chose(into), Pick::Merge(id)) => {
                    let (id, into) = (id.clone(), into.clone());
                    this.change(
                        async move { hub.merge_topic(id, into).await },
                        "merging it failed",
                        cx,
                    );
                }
                (PickerEvent::Create(_), Pick::Merge(_))
                | (PickerEvent::NoTopic | PickerEvent::Dismissed, _) => {}
            }
            cx.notify();
        });
        window.focus(&picker.focus_handle(cx), cx);
        self.browse.choosing = Some(Choosing {
            picker,
            at,
            _events: events,
        });
        cx.notify();
    }

    pub(super) fn choosing_overlay(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let choosing = self.browse.choosing.as_ref()?;
        Some(deferred(
            anchored().position(choosing.at).child(
                div()
                    .w(px(300.))
                    .occlude()
                    .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.browse.choosing = None;
                        cx.notify();
                    }))
                    .child(choosing.picker.clone()),
            ),
        ))
    }

    /// Runs a change to a type or entity, then lists them again; a refusal shows rh's own words.
    fn change(
        &mut self,
        work: impl Future<Output = Result<(), GatewayError>> + 'static,
        failure: &'static str,
        cx: &mut Context<Self>,
    ) {
        self.problem = None;
        cx.spawn(async move |this, cx| {
            let done = work.await;
            this.update(cx, |this, cx| {
                match done {
                    Ok(()) => {}
                    Err(GatewayError::Conflict(message)) => this.problem = Some(message),
                    Err(err) => this.problem = Some(format!("{failure}: {err}")),
                }
                this.load_catalog(cx);
            })
            .ok();
        })
        .detach();
    }
}

/// The picker's heading, what it offers, and whether it can make a new type.
fn choices(catalog: &Catalog, pick: &Pick) -> (String, Vec<Topic>, bool) {
    let entity = |id: &TopicId| catalog.entity(id);
    match pick {
        Pick::Under(id) => match entity(id) {
            Some(known) => (
                format!("Change “{}” to", known.topic.name),
                catalog
                    .types
                    .iter()
                    .filter(|kind| Some(&kind.id) != known.topic.parent.as_ref())
                    .cloned()
                    .collect(),
                true,
            ),
            None => (
                format!(
                    "Move “{}” into",
                    catalog.kind(id).map_or("", |kind| kind.name.as_str())
                ),
                catalog
                    .types
                    .iter()
                    .filter(|kind| {
                        catalog.kind(id).and_then(|known| known.parent.as_ref()) != Some(&kind.id)
                            && catalog.can_nest(id, &kind.id)
                    })
                    .cloned()
                    .collect(),
                true,
            ),
        },
        Pick::Merge(id) => match entity(id) {
            Some(known) => (
                format!("Merge “{}” into", known.topic.name),
                catalog
                    .entities
                    .iter()
                    .filter(|other| &other.topic.id != id)
                    .map(|other| other.topic.clone())
                    .collect(),
                false,
            ),
            None => (
                format!(
                    "Merge “{}” into",
                    catalog.kind(id).map_or("", |kind| kind.name.as_str())
                ),
                catalog
                    .types
                    .iter()
                    .filter(|kind| &kind.id != id)
                    .cloned()
                    .collect(),
                false,
            ),
        },
    }
}
