use app::Hub;
use gpui::{
    ClickEvent, Context, Entity, EventEmitter, IntoElement, ParentElement, Render, SharedString, Styled,
    Subscription, Window, div, prelude::*, px, rgb,
};
use model::HarnessId;
use model::agents::Agent;
use model::harness::{HarnessOffer, ModelChoice};
use model::prefs::LastLaunch;

use crate::input::{Submitted, TextInput};
use crate::theme::Theme;

const DEFAULT_HARNESS: &str = "claude-code";
/// A harness offering more models than this finds them as you type instead of listing them all.
const LISTED_MODELS: usize = 20;
/// How many models the finder shows for what was typed.
const FOUND_MODELS: usize = 12;

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

impl Choice {
    /// Keeps the choice for the next new session.
    pub fn remember(&self, hub: &Hub) {
        let mut prefs = hub.preferences();
        prefs.last_launch = Some(match self {
            Self::Agent(agent) => LastLaunch::Agent {
                id: agent.id.clone(),
                name: agent.name.clone(),
            },
            Self::Harness { node, harness, model } => LastLaunch::Harness {
                node: node.clone(),
                harness: harness.clone(),
                model: model.clone(),
            },
        });
        hub.set_preferences(prefs);
    }
}

/// The harness list of the node on show.
enum Offers {
    Reading,
    Read(Vec<HarnessOffer>),
    Failed(String),
}

/// The person picked something else.
#[derive(Debug, Clone, Copy)]
pub struct Picked;

/// What a new session could start with: your agents, and the harnesses and models of whichever
/// node is on show, with the remembered choice preselected.
pub struct LaunchChoices {
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
    find_model: Entity<TextInput>,
    _subscriptions: [Subscription; 2],
}

impl EventEmitter<Picked> for LaunchChoices {}

impl LaunchChoices {
    pub fn new(hub: &Hub, cx: &mut Context<Self>) -> Self {
        let find_model = cx.new(|cx| TextInput::new("Type to find a model", cx));
        let typed = cx.observe(&find_model, |_, _, cx| cx.notify());
        let found = cx.subscribe(&find_model, |this, _, Submitted(typed), cx| {
            this.pick_first_model(typed, cx);
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
            find_model,
            _subscriptions: [typed, found],
        }
    }

    pub fn choice(&self) -> Option<&Choice> {
        self.choice.as_ref()
    }

    fn pick(&mut self, choice: Choice, cx: &mut Context<Self>) {
        self.choice = Some(choice);
        cx.emit(Picked);
        cx.notify();
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
            Err(err) => Offers::Failed(err.reason()),
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
                if this.choice.is_none()
                    && let Some(choice) = this.preferred_harness()
                {
                    this.pick(choice, cx);
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

    fn choices(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_agents = !self.agents.is_empty();
        let nodes = (self.nodes.len() > 1).then(|| self.node_chips(cx));
        let agents = self.agents.iter().enumerate().map(|(index, agent)| {
            let pick = Choice::Agent(agent.clone());
            let selected = self.choice.as_ref() == Some(&pick);
            chip(("launch-agent", index), selected)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.pick(pick.clone(), cx);
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
            chip(("launch-harness", index), selected)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.find_model.update(cx, |input, cx| input.set_text("", cx));
                    this.pick(pick.clone(), cx);
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
            .when(has_agents, |list| {
                list.child(label("Agents"))
                    .child(div().flex().flex_wrap().gap(px(6.)).children(agents))
            })
            .map(|list| match (nodes, has_agents) {
                (Some(nodes), true) => list.child(label("Or a harness on")).child(nodes),
                (Some(nodes), false) => list.child(label("A harness on")).child(nodes),
                (None, true) => list.child(label("Or a harness on this node")),
                (None, false) => list.child(label("A harness on this node")),
            })
            .children(status.map(|status| div().px_1().text_xs().text_color(Theme::ink_dim()).child(status)))
            .child(div().flex().flex_wrap().gap(px(6.)).children(harnesses))
    }

    fn node_chips(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let chips = self.nodes.iter().enumerate().map(|(index, name)| {
            let node = (index > 0).then(|| name.clone());
            let selected = self.node == node;
            chip(("launch-node", index), selected)
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

    /// The harness's models as chips. A long list shows the chosen one and finds the rest as you
    /// type, a few at a time.
    fn models(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let Some(Choice::Harness { node, harness, model }) = &self.choice else {
            return None;
        };
        let offer = self.offers().iter().find(|offer| offer.id == *harness)?;
        if offer.models.is_empty() {
            return None;
        }
        let long = offer.models.len() > LISTED_MODELS;
        let typed = self.find_model.read(cx).text().trim().to_owned();
        let chosen = |choice: &&ModelChoice| model.as_deref() == Some(choice.id.as_str());
        let (shown, hint): (Vec<&ModelChoice>, Option<String>) = match (long, typed.is_empty()) {
            (false, _) => (offer.models.iter().collect(), None),
            (true, true) => (offer.models.iter().filter(chosen).collect(), None),
            (true, false) => {
                let (found, more) = offer.suggest_models(&typed, FOUND_MODELS);
                let hint = match (found.is_empty(), more) {
                    (true, _) => Some(format!("No model matches “{typed}”")),
                    (false, 0) => None,
                    (false, more) => Some(format!("{more} more match, keep typing")),
                };
                let others = found.into_iter().filter(|choice| !chosen(choice));
                (offer.models.iter().filter(chosen).chain(others).collect(), hint)
            }
        };
        let chips = shown.into_iter().enumerate().map(|(index, choice)| {
            let selected = chosen(&choice);
            let node = node.clone();
            let harness = harness.clone();
            let id = choice.id.clone();
            chip(("launch-model", index), selected)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.find_model.update(cx, |input, cx| input.set_text("", cx));
                    let picked = Choice::Harness {
                        node: node.clone(),
                        harness: harness.clone(),
                        model: Some(id.clone()),
                    };
                    this.pick(picked, cx);
                }))
                .child(choice.label.clone())
        });
        let heading = if long {
            format!("Model · {} offered", offer.models.len())
        } else {
            "Model".to_owned()
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(label(heading))
                .when(long, |list| {
                    list.child(
                        div()
                            .px_3()
                            .py_2()
                            .rounded(px(10.))
                            .bg(Theme::card())
                            .child(self.find_model.clone()),
                    )
                })
                .child(div().flex().flex_wrap().gap(px(6.)).children(chips))
                .children(hint.map(|hint| div().px_1().text_xs().text_color(Theme::ink_dim()).child(hint))),
        )
    }

    /// Enter in the model finder picks its first match.
    fn pick_first_model(&mut self, typed: &str, cx: &mut Context<Self>) {
        let Some(Choice::Harness { node, harness, .. }) = self.choice.clone() else {
            return;
        };
        let first = self
            .offers()
            .iter()
            .find(|offer| offer.id == harness)
            .and_then(|offer| {
                offer
                    .suggest_models(typed, 1)
                    .0
                    .first()
                    .map(|choice| choice.id.clone())
            });
        if let Some(model) = first {
            let picked = Choice::Harness {
                node,
                harness,
                model: Some(model),
            };
            self.pick(picked, cx);
        }
    }
}

impl Render for LaunchChoices {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .when(!self.loaded, |list| {
                list.child(label("Loading agents and harnesses…"))
            })
            .when(self.loaded, |list| list.child(self.choices(cx)))
            .children(self.models(cx))
    }
}

pub fn chip(id: (&'static str, usize), selected: bool) -> gpui::Stateful<gpui::Div> {
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

pub fn label(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .px_1()
        .text_xs()
        .text_color(Theme::ink_dim())
        .child(text.into())
}
