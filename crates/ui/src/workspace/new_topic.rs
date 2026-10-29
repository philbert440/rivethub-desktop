use std::future::Future;

use gateway::GatewayError;
use gpui::{
    AnyElement, ClickEvent, Context, Entity, Focusable, IntoElement, ParentElement, Styled, Subscription,
    Window, div, prelude::*, px,
};
use model::TopicId;

use super::Workspace;
use super::everything::TILE_WIDTH;
use crate::brand;
use crate::theme::Theme;
use crate::topic_picker::{PickerEvent, TopicPicker};

/// A topic being named in the New topic card. It suggests the topics that already exist, past
/// ones included, so picking one brings it back instead of making a second.
pub(super) struct Naming {
    picker: Entity<TopicPicker>,
    busy: bool,
    problem: Option<String>,
    _events: Subscription,
}

impl Workspace {
    fn begin_naming(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let topics = self.topics.clone();
        let picker = cx.new(|cx| TopicPicker::new(topics, None, cx));
        let events = cx.subscribe(&picker, |this, _, event: &PickerEvent, cx| match event {
            PickerEvent::Chose(topic) => this.bring_back(topic.clone(), cx),
            PickerEvent::Create(name) => this.create_topic(name.trim().to_owned(), cx),
            PickerEvent::NoTopic | PickerEvent::Dismissed => {
                this.naming = None;
                cx.notify();
            }
        });
        window.focus(&picker.focus_handle(cx), cx);
        self.naming = Some(Naming {
            picker,
            busy: false,
            problem: None,
            _events: events,
        });
        cx.notify();
    }

    /// Reopens a topic picked from the suggestions, which also brings a past one back into TOPICS.
    fn bring_back(&mut self, topic: TopicId, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        self.finish_naming(async move { hub.set_topic_closed(topic, false).await }, cx);
    }

    fn create_topic(&mut self, name: String, cx: &mut Context<Self>) {
        if name.is_empty() {
            return;
        }
        let hub = self.hub.clone();
        self.finish_naming(async move { hub.create_topic(name).await.map(drop) }, cx);
    }

    fn finish_naming(
        &mut self,
        work: impl Future<Output = Result<(), GatewayError>> + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(naming) = self.naming.as_mut().filter(|naming| !naming.busy) else {
            return;
        };
        naming.busy = true;
        naming.problem = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let done = work.await;
            this.update(cx, |this, cx| {
                match done {
                    Ok(()) => {
                        this.naming = None;
                        this.load(cx);
                    }
                    Err(err) => {
                        if let Some(naming) = this.naming.as_mut() {
                            naming.busy = false;
                            naming.problem = Some(err.to_string());
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The last card in TOPICS: it adds a topic to drag sessions onto, or brings an existing one back.
    pub(super) fn new_topic_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(naming) = &self.naming else {
            return div()
                .id("new-topic")
                .w(px(TILE_WIDTH))
                .p_4()
                .rounded(px(16.))
                .border_1()
                .border_dashed()
                .border_color(Theme::line())
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
        div()
            .w(px(TILE_WIDTH))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(div().flex_1().child(if naming.busy {
                        "Adding…"
                    } else {
                        "New topic, or pick one you already have"
                    }))
                    .child(
                        div()
                            .id("new-topic-cancel")
                            .cursor_pointer()
                            .hover(|cancel| cancel.text_color(Theme::ink()))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.naming = None;
                                cx.notify();
                            }))
                            .child("×"),
                    ),
            )
            .child(naming.picker.clone())
            .children(
                naming
                    .problem
                    .clone()
                    .map(|problem| div().pt_1().text_xs().text_color(Theme::warn()).child(problem)),
            )
            .into_any_element()
    }
}
