use app::Hub;
use gateway::GatewayError;
use gpui::{
    AnyElement, ClickEvent, Context, Entity, EventEmitter, IntoElement, ParentElement, Render, Styled,
    Subscription, Window, div, prelude::*, px,
};
use model::plugin::view::Field;
use model::plugin::{Plugin, PluginRef, PluginState};

use super::form::FormView;
use super::{Plugins, PluginsEvent};
use crate::brand;
use crate::settings::parts::{dim, row, section};
use crate::theme::Theme;

pub enum SettingsEvent {
    Back,
}

/// A plug-in's entry in Settings: what it is, what it may do, turning it on or off, and its
/// settings as a form.
pub struct PluginSettings {
    hub: Hub,
    plugins: Entity<Plugins>,
    plugin: PluginRef,
    form: Option<Entity<FormView>>,
    problem: Option<String>,
    busy: bool,
    saved: bool,
    _listed: Subscription,
}

impl EventEmitter<SettingsEvent> for PluginSettings {}

fn failure(err: GatewayError) -> String {
    match err {
        GatewayError::Conflict(message) => message,
        other => other.to_string(),
    }
}

impl PluginSettings {
    pub fn new(hub: Hub, plugins: Entity<Plugins>, plugin: PluginRef, cx: &mut Context<Self>) -> Self {
        let listed = cx.subscribe(&plugins, |_, _, event: &PluginsEvent, cx| {
            if matches!(event, PluginsEvent::Listed) {
                cx.notify();
            }
        });
        let mut page = Self {
            hub,
            plugins,
            plugin,
            form: None,
            problem: None,
            busy: false,
            saved: false,
            _listed: listed,
        };
        page.load(cx);
        page
    }

    fn known<'a>(&self, cx: &'a gpui::App) -> Option<&'a Plugin> {
        self.plugins.read(cx).find(&self.plugin)
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(fields) = self
            .known(cx)
            .map(|plugin| plugin.settings.clone())
            .filter(|fields| !fields.is_empty())
        else {
            return;
        };
        let hub = self.hub.clone();
        let plugin = self.plugin.clone();
        cx.spawn(async move |this, cx| {
            let kept = hub.plugin_settings(plugin).await;
            this.update(cx, |this, cx| {
                match kept {
                    Ok(kept) => {
                        let fields: Vec<Field> = fields
                            .into_iter()
                            .map(|mut field| {
                                if let Some(value) = kept.values.get(&field.key) {
                                    field.value = Some(value.clone());
                                }
                                field
                            })
                            .collect();
                        this.form = Some(cx.new(|cx| FormView::new(fields, &kept.secrets_set, cx)));
                    }
                    Err(err) if err.is_missing_route() => {
                        this.problem = Some("Its settings need a newer rh on its node.".to_owned());
                    }
                    Err(err) => this.problem = Some(format!("Couldn't read its settings: {}", failure(err))),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.form.clone() else {
            return;
        };
        if self.busy || !form.update(cx, FormView::check) {
            return;
        }
        let values = form.read(cx).settings(cx);
        self.busy = true;
        self.saved = false;
        self.problem = None;
        cx.notify();
        let hub = self.hub.clone();
        let plugin = self.plugin.clone();
        cx.spawn(async move |this, cx| {
            let saved = hub.save_plugin_settings(plugin, values).await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match saved {
                    Ok(()) => this.saved = true,
                    Err(err) => this.problem = Some(failure(err)),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Turns the plug-in on, approving exactly the permissions it waits on, or off.
    fn switch(&mut self, on: bool, cx: &mut Context<Self>) {
        let pending = self
            .known(cx)
            .map(|plugin| plugin.pending.clone())
            .unwrap_or_default();
        self.busy = true;
        self.problem = None;
        cx.notify();
        let hub = self.hub.clone();
        let plugin = self.plugin.clone();
        cx.spawn(async move |this, cx| {
            let switched = if on {
                hub.enable_plugin(plugin, pending).await
            } else {
                hub.disable_plugin(plugin).await
            };
            this.update(cx, |this, cx| {
                this.busy = false;
                if let Err(err) = switched {
                    this.problem = Some(failure(err));
                }
                this.plugins.update(cx, |plugins, cx| plugins.load(cx));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn pill(id: &'static str, icon: Option<&str>, label: String, primary: bool) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .h(px(34.))
            .px_4()
            .rounded(px(17.))
            .flex()
            .items_center()
            .gap_2()
            .flex_none()
            .cursor_pointer()
            .text_sm()
            .map(|pill| {
                if primary {
                    pill.bg(Theme::accent()).text_color(Theme::on_accent())
                } else {
                    pill.border_1()
                        .border_color(Theme::line())
                        .text_color(Theme::ink())
                        .hover(|pill| pill.bg(Theme::raised()))
                }
            })
            .children(icon.map(|icon| brand::icon(brand::symbol(Some(icon)), 16., Theme::ink_dim())))
            .child(label)
    }

    fn header(plugin: &Plugin) -> AnyElement {
        let (ink, fill) = match plugin.state {
            PluginState::Running => (Theme::on_selected(), Theme::selected()),
            PluginState::AwaitingApproval | PluginState::Backoff | PluginState::Starting => {
                (Theme::warn(), Theme::warn_container())
            }
            PluginState::Invalid => (Theme::danger(), Theme::danger_container()),
            PluginState::Disabled => (Theme::ink_dim(), Theme::raised()),
        };
        div()
            .flex()
            .items_center()
            .gap_4()
            .child(
                div()
                    .size(px(48.))
                    .flex_none()
                    .rounded(px(16.))
                    .bg(Theme::card())
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(brand::icon(
                        brand::symbol(plugin.icon.as_deref()),
                        26.,
                        Theme::accent(),
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().text_lg().child(plugin.name.clone()))
                            .child(
                                div()
                                    .px_2()
                                    .rounded(px(8.))
                                    .bg(fill)
                                    .text_xs()
                                    .text_color(ink)
                                    .child(plugin.state.label()),
                            ),
                    )
                    .child(dim(format!("{} · on {}", plugin.version, plugin.at.node)).text_xs())
                    .child(
                        div()
                            .text_sm()
                            .text_color(Theme::ink())
                            .child(plugin.description.clone()),
                    ),
            )
            .into_any_element()
    }

    fn permissions(plugin: &Plugin) -> Vec<AnyElement> {
        let mut rows: Vec<AnyElement> = plugin
            .pending
            .iter()
            .map(|permission| (permission, true))
            .chain(plugin.granted.iter().map(|permission| (permission, false)))
            .map(|(permission, pending)| {
                row()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .min_w_0()
                            .child(brand::icon(
                                brand::symbol(Some(if pending { "hourglass_empty" } else { "check" })),
                                16.,
                                if pending { Theme::warn() } else { Theme::accent() },
                            ))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(permission.describe())
                                    .children(permission.sandbox().map(|note| dim(note).text_xs())),
                            ),
                    )
                    .child(dim(if pending { "Waiting for you" } else { "Allowed" }).text_xs())
                    .into_any_element()
            })
            .collect();
        if rows.is_empty() {
            rows.push(row().child(dim("It asks for no permissions.")).into_any_element());
        }
        let adds: Vec<String> = plugin
            .pages
            .iter()
            .map(|page| format!("the {} page", page.title))
            .chain(
                plugin
                    .topic_cards
                    .iter()
                    .map(|card| format!("a {} topic card", card.title)),
            )
            .chain(
                plugin
                    .session_actions
                    .iter()
                    .map(|entry| format!("“{}” in the session menu", entry.title)),
            )
            .chain(plugin.tools.iter().map(|tool| format!("the agent tool “{tool}”")))
            .collect();
        if !adds.is_empty() {
            rows.push(
                row()
                    .items_start()
                    .child(dim("Adds").flex_none())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .text_color(Theme::ink())
                            .child(adds.join(", ")),
                    )
                    .into_any_element(),
            );
        }
        rows
    }
}

impl Render for PluginSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let back = div().flex().child(
            Self::pill(
                "plugin-settings-back",
                Some("arrow_back"),
                "Settings".to_owned(),
                false,
            )
            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(SettingsEvent::Back))),
        );
        let Some(plugin) = self.known(cx).cloned() else {
            return div()
                .flex()
                .flex_col()
                .gap_6()
                .child(back)
                .child(dim("This plug-in is no longer installed."));
        };
        let switch = match plugin.state {
            PluginState::Disabled | PluginState::AwaitingApproval | PluginState::Invalid => {
                let label = if plugin.pending.is_empty() {
                    "Turn on"
                } else {
                    "Approve and turn on"
                };
                Self::pill("plugin-switch", None, label.to_owned(), true)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.switch(true, cx)))
            }
            PluginState::Starting | PluginState::Running | PluginState::Backoff => {
                Self::pill("plugin-switch", None, "Turn off".to_owned(), false)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.switch(false, cx)))
            }
        };
        let settings = self.form.clone().map(|form| {
            vec![
                div().px_5().py_4().child(form).into_any_element(),
                row()
                    .child(dim(if self.saved { "Saved." } else { "" }))
                    .child(
                        Self::pill("plugin-settings-save", None, "Save".to_owned(), true)
                            .when(self.busy, |pill| pill.opacity(0.5))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.save(cx))),
                    )
                    .into_any_element(),
            ]
        });
        div()
            .flex()
            .flex_col()
            .gap_8()
            .child(back)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(div().flex_1().min_w_0().child(Self::header(&plugin)))
                    .child(switch.when(self.busy, |pill| pill.opacity(0.5))),
            )
            .children(plugin.error.clone().map(|error| {
                div()
                    .px_4()
                    .py_3()
                    .rounded(px(16.))
                    .bg(Theme::danger_container())
                    .text_sm()
                    .text_color(Theme::danger())
                    .child(error)
            }))
            .children(
                self.problem
                    .clone()
                    .map(|problem| div().text_sm().text_color(Theme::danger()).child(problem)),
            )
            .child(section("What it may do", Self::permissions(&plugin)))
            .children(settings.map(|rows| section("Its settings", rows)))
            .child(
                div()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .font_family(Theme::MONO)
                    .child(plugin.at.id.clone()),
            )
    }
}
