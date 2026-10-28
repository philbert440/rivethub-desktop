use app::Hub;
use gpui::{
    ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, Window, div, prelude::*, px, rgb,
};
use model::agents::Agent;
use model::harness::HarnessOffer;
use model::prefs::LastLaunch;
use model::topics::Topic;
use model::{HarnessId, TopicId};

use crate::theme::Theme;
use crate::topic_picker::{PickerEvent, TopicPicker};

const DEFAULT_HARNESS: &str = "claude-code";

/// What a new session runs: one of your agents on its own node, or a harness on a chosen node
/// (`None` is this client's node) with a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Agent(Agent),
    Harness {
        node: Option<String>,
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

/// The harness list of the node the launcher is showing.
enum Offers {
    Reading,
    Read(Vec<HarnessOffer>),
    Failed(String),
}

pub struct Launcher {
    hub: Hub,
    loaded: bool,
    agents: Vec<Agent>,
    /// This node first, then every other online mesh node.
    nodes: Vec<String>,
    node: Option<String>,
    offers: Offers,
    choice: Option<Choice>,
    /// What the last session was started with, preselected when it is still on offer.
    last: Option<LastLaunch>,
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
        let last = hub.preferences().last_launch;
        let away = match &last {
            Some(LastLaunch::Harness { node: Some(node), .. }) => Some(node.clone()),
            _ => None,
        };
        let loading = hub.clone();
        cx.spawn(async move |this, cx| {
            let agents = loading
                .agents()
                .await
                .map(|roster| roster.agents)
                .unwrap_or_default();
            let nodes = loading.agent_hosts().await.unwrap_or_default();
            // The remembered node opens when it's online and answers; otherwise this one does.
            let away = match away.filter(|node| nodes.iter().skip(1).any(|name| name == node)) {
                Some(node) => loading
                    .harnesses(Some(node.clone()))
                    .await
                    .ok()
                    .map(|offers| (node, offers)),
                None => None,
            };
            let (node, offers) = match away {
                Some((node, offers)) => (Some(node), Ok(offers)),
                None => (None, loading.harnesses(None).await),
            };
            this.update(cx, |this, cx| this.loaded(agents, nodes, node, offers, cx))
                .ok();
        })
        .detach();
        Self {
            hub: hub.clone(),
            loaded: false,
            agents: Vec::new(),
            nodes: Vec::new(),
            node: None,
            offers: Offers::Reading,
            choice: None,
            last,
            preset,
            picker,
            _subscriptions: vec![choose_topic],
        }
    }

    fn loaded(
        &mut self,
        agents: Vec<Agent>,
        nodes: Vec<String>,
        node: Option<String>,
        offers: Result<Vec<HarnessOffer>, gateway::GatewayError>,
        cx: &mut Context<Self>,
    ) {
        self.loaded = true;
        self.agents = agents.into_iter().filter(Agent::launchable).collect();
        self.nodes = nodes;
        self.node = node;
        self.read(offers);
        if self.choice.is_none() {
            self.choice = self
                .remembered_agent()
                .or_else(|| self.preferred_harness())
                .or_else(|| self.agents.first().cloned().map(Choice::Agent));
        }
        cx.notify();
    }

    fn remembered_agent(&self) -> Option<Choice> {
        let Some(LastLaunch::Agent { id, .. }) = &self.last else {
            return None;
        };
        self.agents
            .iter()
            .find(|agent| agent.id == *id)
            .cloned()
            .map(Choice::Agent)
    }

    fn read(&mut self, offers: Result<Vec<HarnessOffer>, gateway::GatewayError>) {
        self.offers = match offers {
            Ok(offers) => Offers::Read(offers.into_iter().filter(|offer| offer.chat).collect()),
            Err(err) => Offers::Failed(err.to_string()),
        };
    }

    fn offers(&self) -> &[HarnessOffer] {
        match &self.offers {
            Offers::Read(offers) => offers,
            Offers::Reading | Offers::Failed(_) => &[],
        }
    }

    /// The remembered harness when the node on show still offers it, otherwise the default one.
    fn preferred_harness(&self) -> Option<Choice> {
        let Some(LastLaunch::Harness { node, harness, model }) = &self.last else {
            return self.default_harness();
        };
        if *node != self.node {
            return self.default_harness();
        }
        self.offers()
            .iter()
            .find(|offer| offer.id == *harness)
            .map(|offer| self.harness_choice(offer, model.as_deref()))
            .or_else(|| self.default_harness())
    }

    fn default_harness(&self) -> Option<Choice> {
        let offers = self.offers();
        offers
            .iter()
            .find(|offer| offer.id.as_str() == DEFAULT_HARNESS)
            .or_else(|| offers.first())
            .map(|offer| self.harness_choice(offer, None))
    }

    /// Shows `node`'s harnesses, reading them again if that failed; picking a node means starting a
    /// harness there, so the remembered one is chosen when it was on this node, else its default.
    fn show_node(&mut self, node: Option<String>, cx: &mut Context<Self>) {
        if self.node == node && !matches!(self.offers, Offers::Failed(_)) {
            return;
        }
        self.node.clone_from(&node);
        self.offers = Offers::Reading;
        self.choice = None;
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let offers = hub.harnesses(node.clone()).await;
            this.update(cx, |this, cx| {
                if this.node != node {
                    return;
                }
                this.read(offers);
                if this.choice.is_none() {
                    this.choice = this.preferred_harness();
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// `offer` on the node on show, with `model` when the harness still offers it and its default
    /// model otherwise.
    fn harness_choice(&self, offer: &HarnessOffer, model: Option<&str>) -> Choice {
        let offered = model.and_then(|wanted| offer.models.iter().find(|choice| choice.id == wanted));
        Choice::Harness {
            node: self.node.clone(),
            harness: offer.id.clone(),
            model: offered
                .or_else(|| offer.models.iter().find(|choice| choice.default))
                .map(|choice| choice.id.clone()),
        }
    }

    /// Starts the chosen session and remembers the choice for the next one.
    fn start(&self, target: Target, cx: &mut Context<Self>) {
        if let Some(choice) = self.choice.clone() {
            let mut prefs = self.hub.preferences();
            prefs.last_launch = Some(match &choice {
                Choice::Agent(agent) => LastLaunch::Agent {
                    id: agent.id.clone(),
                    name: agent.name.clone(),
                },
                Choice::Harness { node, harness, model } => LastLaunch::Harness {
                    node: node.clone(),
                    harness: harness.clone(),
                    model: model.clone(),
                },
            });
            self.hub.set_preferences(prefs);
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
        let nodes = (self.nodes.len() > 1).then(|| self.node_chips(cx));
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
        let harnesses = self.offers().iter().enumerate().map(|(index, offer)| {
            let selected =
                matches!(&self.choice, Some(Choice::Harness { harness, .. }) if *harness == offer.id);
            let pick = self.harness_choice(offer, None);
            Self::chip(("launch-harness", index), selected)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.choice = Some(pick.clone());
                    cx.notify();
                }))
                .child(offer.id.display_name().to_owned())
                .into_any_element()
        });
        let here = self.node.as_deref().unwrap_or("this node");
        let status = match &self.offers {
            Offers::Reading => Some(format!("Reading {here}'s harnesses…")),
            Offers::Failed(err) => Some(format!("Couldn't read {here}'s harnesses: {err}")),
            Offers::Read(offers) if offers.is_empty() => Some(format!("No chat harnesses on {here}")),
            Offers::Read(_) => None,
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(Self::label("Agents"))
            .child(div().flex().flex_wrap().gap(px(6.)).children(agents))
            .map(|list| match nodes {
                Some(nodes) => list.child(Self::label("Or a harness on")).child(nodes),
                None => list.child(Self::label("Or a harness on this node")),
            })
            .children(status.map(|status| div().px_1().text_xs().text_color(Theme::ink_dim()).child(status)))
            .child(div().flex().flex_wrap().gap(px(6.)).children(harnesses))
    }

    fn node_chips(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let chips = self.nodes.iter().enumerate().map(|(index, name)| {
            let node = (index > 0).then(|| name.clone());
            let selected = self.node == node;
            Self::chip(("launch-node", index), selected)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.show_node(node.clone(), cx);
                }))
                .child(name.clone())
                .when(index == 0, |chip| {
                    chip.child(div().text_xs().text_color(Theme::ink_dim()).child("this node"))
                })
        });
        div()
            .flex()
            .flex_wrap()
            .gap(px(6.))
            .children(chips)
            .into_any_element()
    }

    fn models(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let Some(Choice::Harness { node, harness, model }) = &self.choice else {
            return None;
        };
        let offer = self.offers().iter().find(|offer| offer.id == *harness)?;
        if offer.models.is_empty() {
            return None;
        }
        let chips = offer.models.iter().enumerate().map(|(index, choice)| {
            let selected = model.as_deref() == Some(choice.id.as_str());
            let node = node.clone();
            let harness = harness.clone();
            let id = choice.id.clone();
            Self::chip(("launch-model", index), selected)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.choice = Some(Choice::Harness {
                        node: node.clone(),
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
        div()
            .flex()
            .flex_col()
            .gap_3()
            .when(!self.loaded, |list| {
                list.child(Self::label("Loading agents and harnesses…"))
            })
            .when(self.loaded, |list| list.child(self.choices(cx)))
            .children(self.models(cx))
            .child(self.topic(cx))
    }
}
