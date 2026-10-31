use app::Hub;
use gpui::{
    ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, Window, div, prelude::*, px,
};
use model::TopicId;
use model::topics::Topic;

use crate::launch_choice::{Choice, LaunchChoices, chip, label};
use crate::theme::Theme;
use crate::topic_picker::{PickerEvent, TopicPicker};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Uncategorized,
    In(TopicId),
    New(String),
}

#[derive(Debug, Clone)]
pub enum LauncherEvent {
    Start { choice: Box<Choice>, target: Target },
    Dismissed,
}

pub struct Launcher {
    hub: Hub,
    choices: Entity<LaunchChoices>,
    preset: Option<(TopicId, String)>,
    picker: Entity<TopicPicker>,
    _choose_topic: Subscription,
}

impl EventEmitter<LauncherEvent> for Launcher {}

impl Launcher {
    pub fn new(
        hub: &Hub,
        topics: Vec<Topic>,
        preset: Option<(TopicId, String)>,
        cx: &mut Context<Self>,
    ) -> Self {
        let picker = cx.new(|cx| TopicPicker::new(topics, Some("Start without a topic"), cx).outlined());
        let choose_topic = cx.subscribe(&picker, |this, _, event: &PickerEvent, cx| {
            let target = match event {
                PickerEvent::Chose(topic) => Target::In(topic.clone()),
                PickerEvent::Create(name) => Target::New(name.clone()),
                PickerEvent::NoTopic => Target::Uncategorized,
                PickerEvent::Dismissed => {
                    cx.emit(LauncherEvent::Dismissed);
                    return;
                }
            };
            this.start(target, cx);
        });
        Self {
            hub: hub.clone(),
            choices: cx.new(|cx| LaunchChoices::new(hub, cx)),
            preset,
            picker,
            _choose_topic: choose_topic,
        }
    }

    /// Starts the chosen session and remembers the choice for the next one.
    fn start(&self, target: Target, cx: &mut Context<Self>) {
        if let Some(choice) = self.choices.read(cx).choice().cloned() {
            choice.remember(&self.hub);
            cx.emit(LauncherEvent::Start {
                choice: Box::new(choice),
                target,
            });
        }
    }

    fn topic(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some((topic, name)) = self.preset.clone() else {
            return div()
                .flex()
                .flex_col()
                .gap_1()
                .child(label("Topic · optional"))
                .child(self.picker.clone())
                .into_any_element();
        };
        let name: SharedString = format!("In {name}").into();
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(chip(("launch-topic", 0), true).child(name))
            .child(
                div()
                    .id("launch-clear-topic")
                    .cursor_pointer()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .hover(|clear| clear.text_color(Theme::ink()))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.preset = None;
                        cx.notify();
                    }))
                    .child("No topic"),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("launch-start")
                    .h(px(32.))
                    .px_4()
                    .rounded(px(16.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .bg(Theme::accent())
                    .text_color(Theme::on_accent())
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.start(Target::In(topic.clone()), cx);
                    }))
                    .child("Start"),
            )
            .into_any_element()
    }
}

impl Focusable for Launcher {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.picker.focus_handle(cx)
    }
}

impl Render for Launcher {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(self.choices.clone())
            .child(self.topic(cx))
    }
}
