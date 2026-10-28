use app::Hub;
use gpui::{
    ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, Window, div, prelude::*, px, rgb,
};
use model::agents::Agent;
use model::harness::HarnessOffer;
use model::topics::Topic;
use model::{HarnessId, TopicId};

use crate::theme::Theme;
use crate::topic_picker::{PickerEvent, TopicPicker};

const DEFAULT_HARNESS: &str = "claude-code";

/// What a new session runs: one of your agents, or a harness on this node with a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Agent(Agent),
    Harness {
        harness: HarnessId,
        model: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Forming,
    In(TopicId),
    New(String),
}

#[derive(Debug, Clone)]
pub enum LauncherEvent {
    Start { choice: Box<Choice>, target: Target },
    Dismissed,
}

pub struct Launcher {
    agents: Vec<Agent>,
    offers: Vec<HarnessOffer>,
    choice: Option<Choice>,
    preset: Option<(TopicId, String)>,
    picker: Entity<TopicPicker>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LauncherEvent> for Launcher {}

impl Launcher {
    pub fn new(
        hub: &Hub,
        topics: Vec<Topic>,
        preset: Option<(TopicId, String)>,
        cx: &mut Context<Self>,
    ) -> Self {
        let picker = cx.new(|cx| TopicPicker::new(topics, Some("Start without a topic"), cx));
        let choose_topic = cx.subscribe(&picker, |this, _, event: &PickerEvent, cx| {
            let target = match event {
                PickerEvent::Chose(topic) => Target::In(topic.clone()),
                PickerEvent::Create(name) => Target::New(name.clone()),
                PickerEvent::NoTopic => Target::Forming,
                PickerEvent::Dismissed => {
                    cx.emit(LauncherEvent::Dismissed);
                    return;
                }
            };
            this.start(target, cx);
        });
        let hub = hub.clone();
        cx.spawn(async move |this, cx| {
            let agents = hub.agents().await.map(|roster| roster.agents).unwrap_or_default();
            let offers = hub.harnesses().await.unwrap_or_default();
            this.update(cx, |this, cx| this.loaded(agents, offers, cx)).ok();
        })
        .detach();
        Self {
            agents: Vec::new(),
            offers: Vec::new(),
            choice: None,
            preset,
            picker,
            _subscriptions: vec![choose_topic],
        }
    }

    fn loaded(&mut self, agents: Vec<Agent>, offers: Vec<HarnessOffer>, cx: &mut Context<Self>) {
        self.agents = agents.into_iter().filter(Agent::launchable).collect();
        self.offers = offers.into_iter().filter(|offer| offer.chat).collect();
        if self.choice.is_none() {
            self.choice = self
                .offers
                .iter()
                .find(|offer| offer.id.as_str() == DEFAULT_HARNESS)
                .or_else(|| self.offers.first())
                .map(Self::harness_choice)
                .or_else(|| self.agents.first().cloned().map(Choice::Agent));
        }
        cx.notify();
    }

    fn harness_choice(offer: &HarnessOffer) -> Choice {
        Choice::Harness {
            harness: offer.id.clone(),
            model: offer
                .models
                .iter()
                .find(|model| model.default)
                .map(|model| model.id.clone()),
        }
    }

    fn start(&self, target: Target, cx: &mut Context<Self>) {
        if let Some(choice) = self.choice.clone() {
            cx.emit(LauncherEvent::Start {
                choice: Box::new(choice),
                target,
            });
        }
    }

    fn chip(id: (&'static str, usize), selected: bool) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .h(px(30.))
            .px_3()
            .rounded(px(15.))
            .flex()
            .items_center()
            .gap(px(6.))
            .cursor_pointer()
            .border_1()
            .map(|chip| {
                if selected {
                    chip.bg(Theme::selected())
                        .border_color(Theme::accent())
                        .text_color(Theme::on_selected())
                } else {
                    chip.bg(Theme::card())
                        .border_color(Theme::card())
                        .text_color(Theme::ink())
                        .hover(|chip| chip.bg(Theme::raised()))
                }
            })
    }

    fn label(text: &'static str) -> impl IntoElement {
        div().px_1().text_xs().text_color(Theme::ink_dim()).child(text)
    }

    fn choices(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let agents = self.agents.iter().enumerate().map(|(index, agent)| {
            let pick = Choice::Agent(agent.clone());
            let selected = self.choice.as_ref() == Some(&pick);
            Self::chip(("launch-agent", index), selected)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.choice = Some(pick.clone());
                    cx.notify();
                }))
                .child(
                    div()
                        .size(px(8.))
                        .rounded(px(4.))
                        .bg(agent.color.map_or(Theme::ink_dim(), rgb)),
                )
                .child(agent.name.clone())
                .child(
                    div()
                        .text_xs()
                        .text_color(Theme::ink_dim())
                        .child(agent.node.clone().unwrap_or_default()),
                )
                .into_any_element()
        });
        let harnesses = self.offers.iter().enumerate().map(|(index, offer)| {
            let selected =
                matches!(&self.choice, Some(Choice::Harness { harness, .. }) if *harness == offer.id);
            let pick = Self::harness_choice(offer);
            Self::chip(("launch-harness", index), selected)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.choice = Some(pick.clone());
                    cx.notify();
                }))
                .child(offer.id.as_str().to_owned())
                .into_any_element()
        });
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(Self::label("Agents"))
            .child(div().flex().flex_wrap().gap(px(6.)).children(agents))
            .child(Self::label("Or a harness on this node"))
            .child(div().flex().flex_wrap().gap(px(6.)).children(harnesses))
    }

    fn models(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let Some(Choice::Harness { harness, model }) = &self.choice else {
            return None;
        };
        let offer = self.offers.iter().find(|offer| offer.id == *harness)?;
        if offer.models.is_empty() {
            return None;
        }
        let chips = offer.models.iter().enumerate().map(|(index, choice)| {
            let selected = model.as_deref() == Some(choice.id.as_str());
            let harness = harness.clone();
            let id = choice.id.clone();
            Self::chip(("launch-model", index), selected)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.choice = Some(Choice::Harness {
                        harness: harness.clone(),
                        model: Some(id.clone()),
                    });
                    cx.notify();
                }))
                .child(choice.label.clone())
        });
        Some(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(Self::label("Model"))
                .child(div().flex().flex_wrap().gap(px(6.)).children(chips)),
        )
    }

    fn topic(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some((topic, name)) = self.preset.clone() else {
            return div()
                .flex()
                .flex_col()
                .gap_1()
                .child(Self::label("Topic · optional"))
                .child(self.picker.clone())
                .into_any_element();
        };
        let name: SharedString = format!("In {name}").into();
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(Self::chip(("launch-topic", 0), true).child(name))
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
        let loading = self.agents.is_empty() && self.offers.is_empty();
        div()
            .flex()
            .flex_col()
            .gap_3()
            .when(loading, |list| {
                list.child(Self::label("Loading agents and harnesses…"))
            })
            .when(!loading, |list| list.child(self.choices(cx)))
            .children(self.models(cx))
            .child(self.topic(cx))
    }
}
