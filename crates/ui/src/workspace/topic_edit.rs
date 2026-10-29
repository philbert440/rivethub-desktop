use std::future::Future;

use gateway::GatewayError;
use gpui::{
    AnyElement, Context, Entity, Focusable, IntoElement, MouseDownEvent, ParentElement, Pixels, Point,
    SharedString, Styled, Subscription, Window, actions, anchored, deferred, div, prelude::*, px,
};
use model::topics::{Atlas, Topic};
use model::{Altitude, SessionId, TopicId};

use super::Workspace;
use crate::input::{Submitted, TextInput};
use crate::theme::Theme;
use crate::topic_picker::{PickerEvent, TopicPicker};

actions!(topic_edit, [CancelRetitle]);

/// A topic being renamed where its name is shown: its card, its crumb or its subtopic heading.
pub(super) struct Retitling {
    pub(super) topic: TopicId,
    input: Entity<TextInput>,
    _submitted: Subscription,
}

/// What a topic picked in the floating picker is for.
#[derive(Debug, Clone)]
pub(super) enum Pick {
    MoveInto(TopicId),
    MergeInto(TopicId),
    File(SessionId),
}

/// The floating topic picker, opened where the menu was.
pub(super) struct Choosing {
    picker: Entity<TopicPicker>,
    heading: String,
    at: Point<Pixels>,
    _events: Subscription,
}

/// A topic card being dragged: dropped on another topic, it moves under it.
#[derive(Debug, Clone)]
pub struct DraggedTopic {
    pub topic: TopicId,
    pub name: SharedString,
}

impl Workspace {
    /// Starts renaming the topic in place. A topic with no card on the board is opened first, so
    /// its name is on screen in the crumb or as a subtopic heading.
    pub(super) fn begin_retitle(&mut self, topic: TopicId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.topic(&topic).map(|known| known.name.clone()) else {
            return;
        };
        let on_board = self.altitude == Altitude::Everything
            && self.board().recent.iter().any(|card| card.topic.id == topic);
        if !on_board {
            let root = Atlas::new(&self.topics, &self.sessions, &self.placements)
                .root(&topic)
                .clone();
            if self.altitude != Altitude::Topic(root.clone()) {
                self.open_topic(root, window, cx);
            }
        }
        let input = cx.new(|cx| {
            let mut input = TextInput::new("Topic name", cx);
            input.set_text(&name, cx);
            input
        });
        let target = topic.clone();
        let submitted = cx.subscribe(&input, move |this, _, Submitted(name), cx| {
            this.retitle(target.clone(), name.trim().to_owned(), cx);
        });
        window.focus(&input.focus_handle(cx), cx);
        self.retitling = Some(Retitling {
            topic,
            input,
            _submitted: submitted,
        });
        cx.notify();
    }

    fn retitle(&mut self, topic: TopicId, name: String, cx: &mut Context<Self>) {
        self.retitling = None;
        cx.notify();
        let Some(known) = self.topics.iter_mut().find(|known| known.id == topic) else {
            return;
        };
        if name.is_empty() || name == known.name {
            return;
        }
        known.name.clone_from(&name);
        let hub = self.hub.clone();
        self.edit_topics(
            async move { hub.rename_topic(topic, name).await },
            "renaming it failed",
            cx,
        );
    }

    pub(super) fn cancel_retitle(&mut self, _: &CancelRetitle, _: &mut Window, cx: &mut Context<Self>) {
        self.retitling = None;
        cx.notify();
    }

    /// The name field for a topic being renamed, if `topic` is the one.
    pub(super) fn retitle_field(&self, topic: &TopicId, cx: &mut Context<Self>) -> Option<AnyElement> {
        let retitling = self
            .retitling
            .as_ref()
            .filter(|retitling| &retitling.topic == topic)?;
        Some(
            div()
                .key_context("TopicRetitle")
                .on_action(cx.listener(Self::cancel_retitle))
                .min_w(px(180.))
                .px_2()
                .py_1()
                .rounded(px(10.))
                .bg(Theme::panel())
                .border_1()
                .border_color(Theme::accent())
                .text_color(Theme::ink())
                .child(retitling.input.clone())
                .into_any_element(),
        )
    }

    /// Opens the topic picker at `at` for moving or merging a topic, or filing a session.
    pub(super) fn choose(
        &mut self,
        pick: Pick,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = |topic: &TopicId| {
            self.topic(topic)
                .map_or_else(String::new, |known| known.name.clone())
        };
        let (heading, topics): (String, Vec<Topic>) = match &pick {
            Pick::MoveInto(topic) => (
                format!("Move “{}” into", name(topic)),
                self.topics
                    .iter()
                    .filter(|known| known.parent.is_none() && &known.id != topic)
                    .cloned()
                    .collect(),
            ),
            Pick::MergeInto(topic) => (
                format!("Merge “{}” into", name(topic)),
                self.topics
                    .iter()
                    .filter(|known| &known.id != topic)
                    .cloned()
                    .collect(),
            ),
            Pick::File(_) => ("Move to topic".to_owned(), self.topics.clone()),
        };
        let merging = matches!(pick, Pick::MergeInto(_));
        let picker = cx.new(|cx| {
            let picker = TopicPicker::new(topics, None, cx);
            if merging { picker.without_create() } else { picker }
        });
        let events = cx.subscribe(&picker, move |this, _, event: &PickerEvent, cx| {
            this.choosing = None;
            match (event, &pick) {
                (PickerEvent::Chose(into), Pick::MoveInto(topic)) => {
                    this.nest(topic.clone(), Some(into.clone()), cx);
                }
                (PickerEvent::Create(name), Pick::MoveInto(topic)) => {
                    this.nest_in_new(topic.clone(), name.clone(), cx);
                }
                (PickerEvent::Chose(into), Pick::MergeInto(topic)) => {
                    this.merge(topic.clone(), into.clone(), cx);
                }
                (PickerEvent::Chose(into), Pick::File(session)) => {
                    this.place(session.clone(), into.clone(), cx);
                }
                (PickerEvent::Create(name), Pick::File(session)) => {
                    this.place_in_new(session.clone(), name.clone(), cx);
                }
                (PickerEvent::Create(_), Pick::MergeInto(_))
                | (PickerEvent::NoTopic | PickerEvent::Dismissed, _) => {}
            }
            cx.notify();
        });
        window.focus(&picker.focus_handle(cx), cx);
        self.choosing = Some(Choosing {
            picker,
            heading,
            at,
            _events: events,
        });
        cx.notify();
    }

    pub(super) fn choosing_overlay(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let choosing = self.choosing.as_ref()?;
        Some(deferred(
            anchored().position(choosing.at).child(
                div()
                    .w(px(300.))
                    .p_2()
                    .rounded(px(16.))
                    .bg(Theme::surface())
                    .border_1()
                    .border_color(Theme::line())
                    .occlude()
                    .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.choosing = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .px_2()
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .child(choosing.heading.clone()),
                    )
                    .child(choosing.picker.clone()),
            ),
        ))
    }

    /// Moves the topic under `parent`, or to the top level.
    pub(super) fn nest(&mut self, topic: TopicId, parent: Option<TopicId>, cx: &mut Context<Self>) {
        if parent.as_ref() == Some(&topic) {
            return;
        }
        let hub = self.hub.clone();
        self.edit_topics(
            async move { hub.move_topic(topic, parent).await },
            "moving it failed",
            cx,
        );
    }

    fn nest_in_new(&mut self, topic: TopicId, name: String, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        self.edit_topics(
            async move {
                let parent = hub.create_topic(name).await?;
                hub.move_topic(topic, Some(parent.id)).await
            },
            "moving it failed",
            cx,
        );
    }

    /// Merges the topic into another, and follows it there if it was open.
    fn merge(&mut self, topic: TopicId, into: TopicId, cx: &mut Context<Self>) {
        if self.altitude == Altitude::Topic(topic.clone()) {
            self.altitude = Altitude::Topic(into.clone());
        }
        let hub = self.hub.clone();
        self.edit_topics(
            async move { hub.merge_topic(topic, into).await },
            "merging it failed",
            cx,
        );
    }

    fn edit_topics(
        &mut self,
        work: impl Future<Output = Result<(), GatewayError>> + 'static,
        failure: &'static str,
        cx: &mut Context<Self>,
    ) {
        self.failed = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let done = work.await;
            this.update(cx, |this, cx| {
                if let Err(err) = done {
                    this.failed = Some(match err {
                        GatewayError::Conflict(message) => message,
                        err => format!("{failure}: {err}"),
                    });
                }
                this.load(cx);
            })
            .ok();
        })
        .detach();
    }
}
