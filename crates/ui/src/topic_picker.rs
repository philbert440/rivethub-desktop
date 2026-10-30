use gpui::{
    ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement, Render,
    Styled, Subscription, Window, actions, div, prelude::*, px,
};
use model::TopicId;
use model::topics::{Topic, suggest};

use crate::input::{Submitted, TextInput};
use crate::theme::Theme;

actions!(topic_picker, [Dismiss]);

const SHOWN: usize = 5;

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
    no_topic: Option<&'static str>,
    creates: bool,
    heading: Option<String>,
    noun: &'static str,
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
            no_topic,
            creates: true,
            heading: None,
            noun: "topic",
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
            None if self.creates => PickerEvent::Create(text.to_owned()),
            None => return,
        };
        cx.emit(event);
    }

    fn row(
        id: (&'static str, usize),
        label: String,
        cx: &mut Context<Self>,
        event: PickerEvent,
    ) -> impl IntoElement {
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
        let shown: Vec<(TopicId, String)> = suggest(&self.topics, &typed, SHOWN)
            .into_iter()
            .map(|topic| (topic.id.clone(), topic.name.clone()))
            .collect();
        let exact = shown.iter().any(|(_, name)| name.eq_ignore_ascii_case(&typed));
        let picks: Vec<_> = shown
            .into_iter()
            .enumerate()
            .map(|(index, (id, name))| {
                Self::row(("pick", index), name, cx, PickerEvent::Chose(id)).into_any_element()
            })
            .collect();
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
            .children(picks)
            .when_some(self.no_topic.filter(|_| typed.is_empty()), |list, label| {
                list.child(Self::row(("none", 0), label.to_owned(), cx, PickerEvent::NoTopic))
            })
            .when(self.creates && !typed.trim().is_empty() && !exact, |list| {
                list.child(Self::row(
                    ("create", 0),
                    format!("Create {} “{typed}”", self.noun),
                    cx,
                    PickerEvent::Create(typed.clone()),
                ))
            })
    }
}
