mod about;
mod agents;
mod devices;
mod general;
mod memory;
mod pairing;
mod parts;

use app::Hub;
use gpui::{
    AnyElement, Context, Entity, EventEmitter, FontWeight, IntoElement, ParentElement, Render, Styled,
    Subscription, Window, div, prelude::*, px,
};
use model::agents::Roster;
use model::health::MemoryHealth;
use model::mesh::Node;
use model::trust::Trust;

use crate::GatewayUrl;
use crate::agent_editor::AgentEditor;
use crate::launch_choice::LaunchChoices;
use crate::shell;
use crate::theme::Theme;
use pairing::PhonePairing;
use parts::Load;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    General,
    Agents,
    Nodes,
    Devices,
    Memory,
    About,
}

impl Tab {
    const ALL: [Self; 6] = [
        Self::General,
        Self::Agents,
        Self::Nodes,
        Self::Devices,
        Self::Memory,
        Self::About,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Agents => "Agents",
            Self::Nodes => "Nodes",
            Self::Devices => "Devices",
            Self::Memory => "Memory",
            Self::About => "About",
        }
    }
}

/// What Settings changed that the rest of the window shows too.
#[derive(Debug, Clone, Copy)]
pub enum SettingsEvent {
    AgentsChanged,
}

/// The Settings page: a tab for each kind of setting, read from this app's preferences and the
/// nodes it talks to.
pub struct SettingsPage {
    hub: Hub,
    gateway: GatewayUrl,
    tab: Tab,
    nodes: Load<Vec<Node>>,
    trust: Load<Trust>,
    memory: Load<MemoryHealth>,
    roster: Load<Roster>,
    /// The choices for new sessions, while they're being changed.
    choosing: Option<(Entity<LaunchChoices>, Subscription)>,
    editing: Option<(Entity<AgentEditor>, Subscription)>,
    pairing: Entity<PhonePairing>,
}

impl EventEmitter<SettingsEvent> for SettingsPage {}

impl SettingsPage {
    pub fn new(hub: Hub, cx: &mut Context<Self>) -> Self {
        let pairing = cx.new(|cx| PhonePairing::new(hub.clone(), cx));
        let mut page = Self {
            hub,
            gateway: crate::gateway_url(),
            tab: Tab::default(),
            nodes: Load::Waiting,
            trust: Load::Waiting,
            memory: Load::Waiting,
            roster: Load::Waiting,
            choosing: None,
            editing: None,
            pairing,
        };
        page.refresh(cx);
        page
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.nodes = Load::Waiting;
        self.trust = Load::Waiting;
        self.memory = Load::Waiting;
        self.load_agents(cx);
        self.pairing.update(cx, PhonePairing::check);
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let nodes = hub.nodes().await;
            this.update(cx, |this, cx| {
                this.nodes = Load::from(nodes);
                cx.notify();
            })
            .ok();
        })
        .detach();
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let trust = hub.trust().await;
            this.update(cx, |this, cx| {
                this.trust = Load::from(trust);
                cx.notify();
            })
            .ok();
        })
        .detach();
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let memory = hub.memory_health().await;
            this.update(cx, |this, cx| {
                this.memory = Load::from(memory);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(200.))
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .px_3()
            .py_6()
            .children(Tab::ALL.map(|tab| {
                let here = tab == self.tab;
                div()
                    .id(tab.label())
                    .h(px(36.))
                    .px_4()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .font_weight(FontWeight::MEDIUM)
                    .map(|item| {
                        if here {
                            item.bg(Theme::selected()).text_color(Theme::on_selected())
                        } else {
                            item.text_color(Theme::ink_dim())
                                .hover(|item| item.bg(Theme::card()).text_color(Theme::ink()))
                        }
                    })
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        this.tab = tab;
                        cx.notify();
                    }))
                    .child(tab.label())
            }))
    }

    fn content(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let sections = match self.tab {
            Tab::General => self.general(cx),
            Tab::Agents => self.agents(cx),
            Tab::Nodes => self.nodes_tab(),
            Tab::Devices => self.devices(),
            Tab::Memory => self.memory_tab(),
            Tab::About => self.about(window, cx),
        };
        sections.into_iter().map(IntoElement::into_any_element).collect()
    }
}

impl Render for SettingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let refresh = parts::button("refresh-settings", "Refresh")
            .h(px(34.))
            .px_4()
            .rounded(px(17.))
            .text_sm()
            .text_color(Theme::ink_dim())
            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.refresh(cx)));
        let content = self.content(window, cx);
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .child(
                shell::header()
                    .child(shell::title("Settings"))
                    .child(div().flex_1())
                    .child(refresh),
            )
            .child(
                shell::tile().flex().child(self.tabs(cx)).child(
                    div()
                        .id(("settings-scroll", self.tab as usize))
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .overflow_y_scroll()
                        .child(
                            div()
                                .max_w(px(720.))
                                .px_6()
                                .py_6()
                                .flex()
                                .flex_col()
                                .gap_8()
                                .children(content),
                        ),
                ),
            )
            .children(self.editing.as_ref().map(|(editor, _)| {
                div()
                    .absolute()
                    .inset_0()
                    .occlude()
                    .bg(Theme::scrim())
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(editor.clone())
            }))
    }
}
