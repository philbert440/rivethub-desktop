use gpui::{
    ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement, Render,
    Styled, Subscription, Window, actions, div, prelude::*, px,
};
use std::collections::{HashMap, HashSet};

use model::TopicId;
use model::topics::{Topic, outline, slug, suggest};

use crate::input::{Submitted, TextInput};
use crate::text::middle;
use crate::theme::Theme;

actions!(topic_picker, [Dismiss]);

const SHOWN: usize = 5;
/// How many matches a tree picker lists while you type, each with its path.
const MATCHED: usize = 8;
const INDENT: f32 = 20.;
/// How long a name above a match may run in its path before its middle goes.
const PATH_NAME: usize = 24;

#[derive(Debug, Clone)]
pub enum PickerEvent {
    Chose(TopicId),
    Create(String),
    NoTopic,
    Dismissed,
}

pub struct TopicPicker {
    input: Entity<TextInput>,
    topics: Vec<Topic>,
    /// Topics that exist but aren't offered, each with why: their names are taken, so typing one
    /// says why it isn't there rather than offering to create it.
    withheld: Vec<(Topic, String)>,
    no_topic: Option<&'static str>,
    creates: bool,
    heading: Option<String>,
    noun: &'static str,
    /// For a picker shown as the topic tree: the topics opened to show their subtopics.
    tree: Option<HashSet<TopicId>>,
    _input: [Subscription; 2],
}

impl EventEmitter<PickerEvent> for TopicPicker {}

impl TopicPicker {
    pub fn new(topics: Vec<Topic>, no_topic: Option<&'static str>, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new("Topic name", cx));
        let submitted = cx.subscribe(&input, |this, _, Submitted(text), cx| this.submit(text, cx));
        let typed = cx.observe(&input, |_, _, cx| cx.notify());
        Self {
            input,
            topics,
            withheld: Vec::new(),
            no_topic,
            creates: true,
            heading: None,
            noun: "topic",
            tree: None,
            _input: [submitted, typed],
        }
    }

    /// A line above the field saying what the pick is for.
    #[must_use]
    pub fn headed(mut self, heading: String) -> Self {
        self.heading = Some(heading);
        self
    }

    /// Names what is picked, such as "type", in the field and on the Create row.
    #[must_use]
    pub fn naming(mut self, noun: &'static str, cx: &mut Context<Self>) -> Self {
        self.noun = noun;
        let mut field = format!("{noun} name");
        if let Some(first) = field.get_mut(..1) {
            first.make_ascii_uppercase();
        }
        self.input
            .update(cx, |input, cx| input.set_placeholder(field, cx));
        self
    }

    /// Shows the topics as their tree, each root closed until opened, rather than the most recent
    /// few; typing lists the matches with their paths.
    #[must_use]
    pub fn outlined(mut self) -> Self {
        self.tree = Some(HashSet::new());
        self
    }

    /// Topics left out of the offer, each with the sentence saying why.
    #[must_use]
    pub fn withholding(mut self, withheld: Vec<(Topic, String)>) -> Self {
        self.withheld = withheld;
        self
    }

    /// Offers only the topics given, with no Create row: for picking an existing topic.
    #[must_use]
    pub fn without_create(mut self) -> Self {
        self.creates = false;
        self
    }

    /// Enter picks the first topic shown; with nothing shown it creates the typed name, and a
    /// blank one means no topic where that's offered, since rh refuses an empty topic name.
    fn submit(&self, text: &str, cx: &mut Context<Self>) {
        let event = match suggest(&self.topics, text, 1).first() {
            Some(topic) => PickerEvent::Chose(topic.id.clone()),
            None if text.trim().is_empty() => match self.no_topic {
                Some(_) => PickerEvent::NoTopic,
                None => return,
            },
            None if self.can_create(text) => PickerEvent::Create(text.to_owned()),
            None => return,
        };
        cx.emit(event);
    }

    /// Whether a topic named `typed` could be made: rh refuses a name with no letters or digits,
    /// and one whose slug a topic already has.
    fn can_create(&self, typed: &str) -> bool {
        let wanted = slug(typed);
        self.creates
            && !wanted.is_empty()
            && !self.topics.iter().any(|topic| Self::named(topic, &wanted))
            && self.withheld_for(typed).is_none()
    }

    /// Why the topic named `typed` isn't offered, when it's one of the withheld.
    fn withheld_for(&self, typed: &str) -> Option<&str> {
        let wanted = slug(typed);
        self.withheld
            .iter()
            .find(|(topic, _)| !wanted.is_empty() && Self::named(topic, &wanted))
            .map(|(_, why)| why.as_str())
    }

    fn named(topic: &Topic, wanted: &str) -> bool {
        topic.slug == wanted || slug(&topic.name) == wanted
    }

    /// The topic tree's rows: each topic whose parents are all open, with its depth and whether it
    /// has subtopics to open.
    fn tree_rows(&self, open: &HashSet<TopicId>, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let parents: HashSet<&TopicId> = self
            .topics
            .iter()
            .filter_map(|topic| topic.parent.as_ref())
            .collect();
        let mut shown_depth: Option<usize> = None;
        let mut rows = Vec::new();
        for (index, (depth, topic)) in outline(&self.topics).into_iter().enumerate() {
            if shown_depth.is_some_and(|hidden_below| depth > hidden_below) {
                continue;
            }
            shown_depth = None;
            let has_children = parents.contains(&topic.id);
            let opened = open.contains(&topic.id);
            if has_children && !opened {
                shown_depth = Some(depth);
            }
            let toggled = topic.id.clone();
            let toggle = div()
                .id(("open", index))
                .w(px(INDENT))
                .flex_none()
                .text_color(Theme::ink_dim())
                .text_lg()
                .when(has_children, |toggle| {
                    toggle
                        .cursor_pointer()
                        .child(if opened { "▾" } else { "▸" })
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            if let Some(open) = this.tree.as_mut()
                                && !open.remove(&toggled)
                            {
                                open.insert(toggled.clone());
                            }
                            cx.notify();
                        }))
                });
            let indent = px(INDENT * f32::from(u8::try_from(depth).unwrap_or(u8::MAX)));
            let chosen = topic.id.clone();
            rows.push(
                div()
                    .id(("pick", index))
                    .pl(indent + px(6.))
                    .pr_3()
                    .py(px(6.))
                    .rounded(px(10.))
                    .flex()
                    .items_center()
                    .gap_1()
                    .cursor_pointer()
                    .hover(|row| row.bg(Theme::raised()))
                    .text_color(Theme::ink())
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        cx.emit(PickerEvent::Chose(chosen.clone()));
                    }))
                    .child(toggle)
                    .child(topic.name.clone())
                    .into_any_element(),
            );
        }
        rows
    }

    /// What a match is filed under, as the names above it.
    fn path_above(&self, topic: &Topic) -> Option<String> {
        let known: HashMap<&TopicId, &Topic> = self.topics.iter().map(|topic| (&topic.id, topic)).collect();
        let mut names = Vec::new();
        let mut parent = topic.parent.as_ref();
        while let Some(above) = parent.and_then(|id| known.get(id)) {
            names.push(middle(&above.name, PATH_NAME));
            parent = above
                .parent
                .as_ref()
                .filter(|_| names.len() < model::topics::MAX_LEVELS);
        }
        names.reverse();
        (!names.is_empty()).then(|| format!("in {}", names.join(" › ")))
    }

    fn row(
        id: (&'static str, usize),
        label: String,
        cx: &mut Context<Self>,
        event: PickerEvent,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .px_3()
            .py(px(6.))
            .rounded(px(10.))
            .cursor_pointer()
            .hover(|row| row.bg(Theme::raised()))
            .text_color(Theme::ink())
            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                cx.emit(event.clone());
            }))
            .child(label)
    }
}

impl Focusable for TopicPicker {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl Render for TopicPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let typed = self.input.read(cx).text().to_owned();
        let limit = if self.tree.is_some() { MATCHED } else { SHOWN };
        let shown: Vec<(TopicId, String, Option<String>)> = suggest(&self.topics, &typed, limit)
            .into_iter()
            .map(|topic| {
                let above = self.tree.as_ref().and_then(|_| self.path_above(topic));
                (topic.id.clone(), topic.name.clone(), above)
            })
            .collect();
        let creatable = self.can_create(&typed);
        let refusal = self.withheld_for(&typed).map(str::to_owned);
        let picks: Vec<_> = match self.tree.clone() {
            Some(open) if typed.trim().is_empty() => self.tree_rows(&open, cx),
            _ => shown
                .into_iter()
                .enumerate()
                .map(|(index, (id, name, above))| {
                    Self::row(("pick", index), name, cx, PickerEvent::Chose(id))
                        .when_some(above, |row, above| {
                            row.child(
                                div()
                                    .text_xs()
                                    .text_color(Theme::ink_dim())
                                    .truncate()
                                    .child(above),
                            )
                        })
                        .into_any_element()
                })
                .collect(),
        };
        let picks = div()
            .id("picks")
            .flex()
            .flex_col()
            .gap_1()
            .max_h(px(320.))
            .overflow_y_scroll()
            .children(picks);
        div()
            .key_context("TopicPicker")
            .on_action(cx.listener(|_, _: &Dismiss, _, cx| cx.emit(PickerEvent::Dismissed)))
            .mt_2()
            .p_2()
            .rounded(px(14.))
            .bg(Theme::surface())
            .border_1()
            .border_color(Theme::line())
            .flex()
            .flex_col()
            .gap_1()
            .children(self.heading.clone().map(|heading| {
                div()
                    .px_2()
                    .pb_1()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(heading)
            }))
            .child(
                div()
                    .px_3()
                    .py_2()
                    .rounded(px(10.))
                    .bg(Theme::card())
                    .child(self.input.clone()),
            )
            .child(picks)
            .when_some(self.no_topic.filter(|_| typed.is_empty()), |list, label| {
                list.child(Self::row(("none", 0), label.to_owned(), cx, PickerEvent::NoTopic))
            })
            .when_some(refusal, |list, why| {
                list.child(
                    div()
                        .px_3()
                        .py(px(6.))
                        .text_sm()
                        .text_color(Theme::ink_dim())
                        .child(why),
                )
            })
            .when(creatable, |list| {
                list.child(Self::row(
                    ("create", 0),
                    format!("Create {} “{typed}”", self.noun),
                    cx,
                    PickerEvent::Create(typed.clone()),
                ))
            })
    }
}
