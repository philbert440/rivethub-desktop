use app::Hub;
use gpui::{
    AnyElement, Context, Div, IntoElement, ParentElement, Render, SharedString, Styled, Window, div,
    prelude::*, px,
};
use model::health::{MemoryHealth, Status};
use model::mesh::Node;
use model::trust::{Certificate, Trust, Validity};

use crate::shell;
use crate::text::now_ms;
use crate::theme::Theme;

enum Load<T> {
    Waiting,
    Ready(T),
    Failed(SharedString),
}

impl<T> Load<T> {
    fn from(result: Result<T, gateway::GatewayError>) -> Self {
        result.map_or_else(|err| Self::Failed(err.to_string().into()), Self::Ready)
    }
}

/// The Settings page: the mesh's nodes, this device's certificate and how memory is doing.
pub struct SettingsPage {
    hub: Hub,
    nodes: Load<Vec<Node>>,
    trust: Load<Trust>,
    memory: Load<MemoryHealth>,
}

impl SettingsPage {
    pub fn new(hub: Hub, cx: &mut Context<Self>) -> Self {
        let mut page = Self {
            hub,
            nodes: Load::Waiting,
            trust: Load::Waiting,
            memory: Load::Waiting,
        };
        page.refresh(cx);
        page
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.nodes = Load::Waiting;
        self.trust = Load::Waiting;
        self.memory = Load::Waiting;
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

    fn nodes(&self) -> Vec<AnyElement> {
        let nodes = match &self.nodes {
            Load::Ready(nodes) if nodes.is_empty() => {
                return vec![row().child(dim("The mesh lists no nodes.")).into_any_element()];
            }
            Load::Ready(nodes) => nodes,
            other => return pending(other),
        };
        let local = self.hub.local_node();
        nodes
            .iter()
            .map(|node| {
                let here = local.as_deref().is_some_and(|name| node.answers_to(name));
                let status = match (node.online, node.sessions) {
                    (false, _) => "offline".to_owned(),
                    (true, Some(1)) => "online · 1 session".to_owned(),
                    (true, Some(count)) => format!("online · {count} sessions"),
                    (true, None) => "online".to_owned(),
                };
                row()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .min_w_0()
                            .child(dot(if node.online {
                                Theme::accent()
                            } else {
                                Theme::line()
                            }))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(node.name.clone())
                                            .when(here, |name| name.child(chip("this node"))),
                                    )
                                    .child(mono(node.url.clone())),
                            ),
                    )
                    .child(dim(status))
                    .into_any_element()
            })
            .collect()
    }

    fn device(&self) -> Vec<AnyElement> {
        let trust = match &self.trust {
            Load::Ready(trust) => trust,
            other => return pending(other),
        };
        let Some(cert) = &trust.device else {
            return vec![
                row()
                    .child("No device certificate")
                    .child(dim("This app connects without one."))
                    .into_any_element(),
            ];
        };
        let holder = cert.holder().into_iter().flat_map(|holder| {
            [
                field("Device", div().child(holder.device.to_owned())),
                field("User", div().child(holder.user.to_owned())),
            ]
        });
        holder
            .chain([
                field("Identity", mono(cert.subject.clone())),
                field("Issued by", div().child(cert.issuer.clone())),
                field("Expires", expiry(cert)),
            ])
            .collect()
    }

    fn authority(&self) -> Vec<AnyElement> {
        match &self.trust {
            Load::Ready(trust) => vec![
                field("Authority", div().child(trust.authority.subject.clone())),
                field("Expires", expiry(&trust.authority)),
            ],
            other => pending(other),
        }
    }

    fn notifications(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let prefs = self.hub.preferences();
        vec![
            toggle(
                "notify-finished",
                "An agent finished a reply",
                "When you're in another window or another session.",
                prefs.notify_finished,
                cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                    let mut prefs = this.hub.preferences();
                    prefs.notify_finished = !prefs.notify_finished;
                    this.hub.set_preferences(prefs);
                    cx.notify();
                }),
            ),
            toggle(
                "notify-needs-you",
                "A session needs you",
                "When an agent waits on an approval or an answer.",
                prefs.notify_needs_you,
                cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                    let mut prefs = this.hub.preferences();
                    prefs.notify_needs_you = !prefs.notify_needs_you;
                    this.hub.set_preferences(prefs);
                    cx.notify();
                }),
            ),
        ]
    }

    fn memory(&self) -> Vec<AnyElement> {
        let health = match &self.memory {
            Load::Ready(health) => health,
            other => return pending(other),
        };
        let (tone, label) = match health.status {
            Status::Ok => (Theme::accent(), "Memory checks OK"),
            Status::Degraded | Status::Error => (Theme::warn(), "Memory needs attention"),
        };
        vec![
            row()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(dot(tone))
                        .child(label)
                        .when(!health.semantic, |line| line.child(warn_chip("keywords only"))),
                )
                .child(dim(format!(
                    "{} sessions · {} messages",
                    compact(health.conversations),
                    compact(health.messages)
                )))
                .into_any_element(),
        ]
    }
}

impl Render for SettingsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let refresh = div()
            .id("refresh-settings")
            .h(px(34.))
            .px_4()
            .rounded(px(17.))
            .border_1()
            .border_color(Theme::line())
            .flex()
            .items_center()
            .cursor_pointer()
            .text_color(Theme::ink_dim())
            .hover(|button| button.bg(Theme::card()))
            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| this.refresh(cx)))
            .child("Refresh");
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                shell::header()
                    .child(shell::title("Settings"))
                    .child(div().flex_1())
                    .child(refresh),
            )
            .child(
                shell::tile().child(
                    div().id("settings-scroll").size_full().overflow_y_scroll().child(
                        div()
                            .max_w(px(720.))
                            .px_8()
                            .py_6()
                            .flex()
                            .flex_col()
                            .gap_8()
                            .child(section("Nodes", self.nodes()))
                            .child(section("Notifications", self.notifications(cx)))
                            .child(section("This device", self.device()))
                            .child(section("Certificate authority", self.authority()))
                            .child(section("Memory", self.memory())),
                    ),
                ),
            )
    }
}

fn section(title: &'static str, rows: Vec<AnyElement>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(div().px_1().text_color(Theme::ink_dim()).child(title))
        .child(
            div()
                .flex()
                .flex_col()
                .rounded(px(20.))
                .bg(Theme::card())
                .overflow_hidden()
                .children(rows.into_iter().enumerate().map(|(index, row)| {
                    div()
                        .when(index > 0, |line| line.border_t_1().border_color(Theme::line()))
                        .child(row)
                })),
        )
}

fn row() -> Div {
    div()
        .min_h(px(52.))
        .px_5()
        .py_3()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
}

fn field(label: &'static str, value: Div) -> AnyElement {
    row()
        .child(div().text_color(Theme::ink_dim()).child(label))
        .child(value)
        .into_any_element()
}

/// A row that flips a setting when clicked anywhere, with a switch showing its state.
fn toggle(
    id: &'static str,
    label: &'static str,
    detail: &'static str,
    on: bool,
    flip: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let track = div()
        .w(px(36.))
        .h(px(20.))
        .flex_none()
        .flex()
        .items_center()
        .px(px(3.))
        .rounded_full()
        .bg(if on { Theme::accent() } else { Theme::line() })
        .when(on, Styled::justify_end)
        .child(div().size(px(14.)).rounded_full().bg(Theme::ink()));
    row()
        .id(id)
        .cursor_pointer()
        .hover(|row| row.bg(Theme::raised()))
        .on_click(flip)
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .child(label)
                .child(dim(detail).text_xs()),
        )
        .child(track)
        .into_any_element()
}

fn pending<T>(load: &Load<T>) -> Vec<AnyElement> {
    let line = match load {
        Load::Failed(problem) => div().text_color(Theme::warn()).child(problem.clone()),
        _ => dim("Checking…"),
    };
    vec![row().child(line).into_any_element()]
}

fn expiry(cert: &Certificate) -> Div {
    match cert.validity(now_ms()) {
        Validity::Valid { days_left: 0 } => div().child("today"),
        Validity::Valid { days_left: 1 } => div().child("in 1 day"),
        Validity::Valid { days_left } if days_left < 730 => div().child(format!("in {days_left} days")),
        Validity::Valid { days_left } => div().child(format!("in {} years", days_left / 365)),
        Validity::Expired { days_ago: 0 } => div().text_color(Theme::removed()).child("expired today"),
        Validity::Expired { days_ago } => div()
            .text_color(Theme::removed())
            .child(format!("expired {days_ago} days ago")),
    }
}

fn dim(text: impl Into<SharedString>) -> Div {
    div().text_color(Theme::ink_dim()).child(text.into())
}

fn mono(text: impl Into<SharedString>) -> Div {
    div()
        .font_family(Theme::MONO)
        .text_xs()
        .text_color(Theme::ink_dim())
        .truncate()
        .child(text.into())
}

fn dot(color: gpui::Rgba) -> Div {
    div().size(px(8.)).flex_none().rounded_full().bg(color)
}

fn chip(text: &'static str) -> Div {
    div()
        .px_2()
        .rounded(px(8.))
        .text_xs()
        .bg(Theme::selected())
        .text_color(Theme::on_selected())
        .child(text)
}

fn warn_chip(text: &'static str) -> Div {
    div()
        .px_2()
        .rounded(px(8.))
        .text_xs()
        .bg(Theme::warn_container())
        .text_color(Theme::warn())
        .child(text)
}

fn compact(count: u64) -> String {
    match count {
        0..1_000 => count.to_string(),
        1_000..10_000 => format!("{}.{}k", count / 1_000, count % 1_000 / 100),
        10_000..1_000_000 => format!("{}k", count / 1_000),
        _ => format!("{}.{}M", count / 1_000_000, count % 1_000_000 / 100_000),
    }
}
