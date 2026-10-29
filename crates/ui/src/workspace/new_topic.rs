use gpui::{
    AnyElement, ClickEvent, Context, Entity, Focusable, IntoElement, ParentElement, Styled, Subscription,
    Window, div, prelude::*, px,
};

use super::Workspace;
use super::everything::TILE_WIDTH;
use crate::brand;
use crate::input::{Submitted, TextInput};
use crate::theme::Theme;

/// A topic being named in the New topic card.
pub(super) struct Naming {
    input: Entity<TextInput>,
    busy: bool,
    problem: Option<String>,
    _submitted: Subscription,
}

impl Workspace {
    fn begin_naming(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextInput::new("Topic name", cx));
        let submitted = cx.subscribe(&input, |this, _, Submitted(name), cx| {
            this.create_topic(name.trim().to_owned(), cx);
        });
        window.focus(&input.focus_handle(cx), cx);
        self.naming = Some(Naming {
            input,
            busy: false,
            problem: None,
            _submitted: submitted,
        });
        cx.notify();
    }

    fn create_topic(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(naming) = self
            .naming
            .as_mut()
            .filter(|naming| !naming.busy && !name.is_empty())
        else {
            return;
        };
        naming.busy = true;
        naming.problem = None;
        cx.notify();
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let created = hub.create_topic(name).await;
            this.update(cx, |this, cx| {
                match created {
                    Ok(_) => {
                        this.naming = None;
                        this.load(cx);
                    }
                    Err(err) => {
                        if let Some(naming) = this.naming.as_mut() {
                            naming.busy = false;
                            naming.problem = Some(format!("couldn't create it: {err}"));
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The last card in TOPICS: it names a new, empty topic to drag sessions into.
    pub(super) fn new_topic_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let card = div()
            .id("new-topic")
            .w(px(TILE_WIDTH))
            .p_4()
            .rounded(px(16.))
            .border_1()
            .border_dashed()
            .border_color(Theme::line())
            .flex()
            .flex_col()
            .gap_2();
        let Some(naming) = &self.naming else {
            return card
                .cursor_pointer()
                .hover(|card| card.bg(Theme::card()))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.begin_naming(window, cx)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_color(Theme::ink_dim())
                        .child(brand::icon("icons/plus.svg", 16., Theme::ink_dim()))
                        .child("New topic"),
                )
                .into_any_element();
        };
        card.border_color(Theme::accent())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(naming.input.clone()))
                    .child(
                        div()
                            .id("new-topic-cancel")
                            .cursor_pointer()
                            .text_color(Theme::ink_dim())
                            .hover(|cancel| cancel.text_color(Theme::ink()))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.naming = None;
                                cx.notify();
                            }))
                            .child("×"),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(if naming.busy {
                        "Creating…"
                    } else {
                        "Enter creates it; then drag sessions onto it."
                    }),
            )
            .children(
                naming
                    .problem
                    .clone()
                    .map(|problem| div().text_xs().text_color(Theme::warn()).child(problem)),
            )
            .into_any_element()
    }
}
