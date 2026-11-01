use std::future::Future;

use gateway::GatewayError;
use gpui::{
    AnyElement, Context, Entity, Focusable, IntoElement, MouseDownEvent, ParentElement, Pixels, Point,
    SharedString, Styled, Subscription, Window, actions, anchored, deferred, div, prelude::*, px,
};
use model::topics::{Atlas, MAX_LEVELS, Topic, Unnestable};
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
    /// A new subtopic of this topic, or an existing topic moved under it.
    Child(TopicId),
}

/// The topics a picker offers, and the ones it leaves out with why.
type Offered = (Vec<Topic>, Vec<(Topic, String)>);

/// The floating topic picker, opened where the menu was.
pub(super) struct Choosing {
    picker: Entity<TopicPicker>,
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
    /// Starts renaming the topic in place: on its card, on the topic screen's path or pills, or in
    /// the topic screen's heading. A topic shown in none of those is opened first.
    pub(super) fn begin_retitle(&mut self, topic: TopicId, window: &mut Window, cx: &mut Context<Self>) {
        let Some((name, parent)) = self
            .topic(&topic)
            .map(|known| (known.name.clone(), known.parent.clone()))
        else {
            return;
        };
        let atlas = Atlas::new(&self.topics, &self.sessions, &self.placements);
        let shown = match &self.altitude {
            Altitude::Everything => self.board().recent.iter().any(|card| card.topic.id == topic),
            Altitude::Topic(root) => {
                let step = self.step_in(root);
                atlas.lineage(&step).contains(&&topic) || parent.as_ref() == Some(&step)
            }
            Altitude::Session { .. } => false,
        };
        if !shown {
            let root = atlas.root(&topic).clone();
            let step = (root != topic).then(|| topic.clone());
            self.open_topic(root, window, cx);
            self.screen.step = step;
            self.sync_step(cx);
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
            "Renaming it failed",
            |_| {},
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
        let (heading, (topics, withheld)) = self.offer_for(&pick);
        let merging = matches!(pick, Pick::MergeInto(_));
        let picker = cx.new(|cx| {
            let picker = TopicPicker::new(topics, None, cx)
                .headed(heading)
                .withholding(withheld);
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
                (PickerEvent::Chose(child), Pick::Child(parent)) => {
                    this.nest(child.clone(), Some(parent.clone()), cx);
                }
                (PickerEvent::Create(name), Pick::Child(parent)) => {
                    this.create_child(name.trim().to_owned(), parent.clone(), cx);
                }
                (PickerEvent::Create(_), Pick::MergeInto(_))
                | (PickerEvent::NoTopic | PickerEvent::Dismissed, _) => {}
            }
            cx.notify();
        });
        window.focus(&picker.focus_handle(cx), cx);
        self.choosing = Some(Choosing {
            picker,
            at,
            _events: events,
        });
        cx.notify();
    }

    /// What the picker for `pick` says it is for, the topics it offers, and the rest with why each
    /// is left out.
    fn offer_for(&self, pick: &Pick) -> (String, Offered) {
        let name = |topic: &TopicId| {
            self.topic(topic)
                .map_or_else(String::new, |known| known.name.clone())
        };
        let atlas = Atlas::new(&self.topics, &self.sessions, &self.placements);
        match pick {
            Pick::MoveInto(topic) => (
                format!("Move “{}” into", name(topic)),
                self.offer(|under| self.topic(topic).and_then(|moved| refusal(&atlas, moved, under))),
            ),
            Pick::MergeInto(topic) => (
                format!("Merge “{}” into", name(topic)),
                self.offer(|known| {
                    (&known.id == topic).then(|| "A topic can't merge into itself.".to_owned())
                }),
            ),
            Pick::File(_) => ("Move to topic".to_owned(), self.offer(|_| None)),
            Pick::Child(parent) => (
                format!("New subtopic of “{}”, or move one here", name(parent)),
                self.offer(|child| self.topic(parent).and_then(|under| refusal(&atlas, child, under))),
            ),
        }
    }

    /// The topics a pick offers, and the rest with why `refused` leaves each out.
    fn offer(&self, refused: impl Fn(&Topic) -> Option<String>) -> Offered {
        let mut offered = Vec::new();
        let mut withheld = Vec::new();
        for topic in &self.topics {
            match refused(topic) {
                Some(why) => withheld.push((topic.clone(), why)),
                None => offered.push(topic.clone()),
            }
        }
        (offered, withheld)
    }

    pub(super) fn choosing_overlay(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let choosing = self.choosing.as_ref()?;
        Some(deferred(
            anchored().position(choosing.at).child(
                div()
                    .w(px(300.))
                    .occlude()
                    .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.choosing = None;
                        cx.notify();
                    }))
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
            "Moving it failed",
            |_| {},
            cx,
        );
    }

    fn create_child(&mut self, name: String, parent: TopicId, cx: &mut Context<Self>) {
        if name.is_empty() {
            return;
        }
        let hub = self.hub.clone();
        self.edit_topics(
            async move { hub.create_topic(name, Some(parent)).await.map(drop) },
            "Adding it failed",
            |_| {},
            cx,
        );
    }

    fn nest_in_new(&mut self, topic: TopicId, name: String, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        self.edit_topics(
            async move {
                let parent = hub.create_topic(name, None).await?;
                hub.move_topic(topic, Some(parent.id)).await
            },
            "Moving it failed",
            |_| {},
            cx,
        );
    }

    /// Merges the topic into another, and follows it there if it was open.
    fn merge(&mut self, topic: TopicId, into: TopicId, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        let (merged, target) = (topic.clone(), into.clone());
        self.edit_topics(
            async move { hub.merge_topic(topic, into).await },
            "Merging it failed",
            move |this| {
                if this.altitude == Altitude::Topic(merged) {
                    this.altitude = Altitude::Topic(target);
                }
            },
            cx,
        );
    }

    /// Runs a topic change, then `then` once rh has taken it; a refusal shows rh's own words.
    fn edit_topics(
        &mut self,
        work: impl Future<Output = Result<(), GatewayError>> + 'static,
        failure: &'static str,
        then: impl FnOnce(&mut Self) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.failed = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let done = work.await;
            this.update(cx, |this, cx| {
                match done {
                    Ok(()) => then(this),
                    Err(GatewayError::Conflict(message)) => this.failed = Some(message),
                    Err(err) => this.failed = Some(format!("{failure}: {}", err.reason())),
                }
                this.refile(cx);
            })
            .ok();
        })
        .detach();
    }
}

/// Why `moved` can't go under `under`, as a sentence, or `None` when it can.
fn refusal(atlas: &Atlas<'_>, moved: &Topic, under: &Topic) -> Option<String> {
    if moved.parent.as_ref() == Some(&under.id) {
        return Some(format!("“{}” is already in “{}”.", moved.name, under.name));
    }
    match atlas.nesting(&moved.id, &under.id) {
        Ok(()) => None,
        Err(Unnestable::OwnSubtree) if moved.id == under.id => {
            Some(format!("“{}” can't move into itself.", moved.name))
        }
        Err(Unnestable::OwnSubtree) => Some(format!(
            "“{}” is inside “{}”, so “{}” can't move into it.",
            under.name, moved.name, moved.name
        )),
        Err(Unnestable::TooDeep) => Some(format!(
            "“{}” can't move into “{}”: topics nest at most {MAX_LEVELS} levels deep.",
            moved.name, under.name
        )),
    }
}
