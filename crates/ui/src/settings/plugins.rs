use gpui::{AnyElement, ClickEvent, Context, Div, IntoElement, ParentElement, Styled, div, prelude::*, px};
use model::plugin::PluginRef;

use super::SettingsPage;
use super::parts::{dim, row, section};
use crate::brand;
use crate::plugin::PluginSettings;
use crate::theme::Theme;

impl SettingsPage {
    /// Every node's plug-ins, or the one whose entry is open.
    pub(super) fn plugins_tab(&self, cx: &mut Context<Self>) -> Vec<Div> {
        match &self.plugin {
            Some((entry, _)) => vec![div().child(entry.clone())],
            None => vec![section("Plug-ins", self.plugin_rows(cx))],
        }
    }

    fn open_plugin(&mut self, plugin: PluginRef, cx: &mut Context<Self>) {
        let (hub, plugins) = (self.hub.clone(), self.plugins.clone());
        let entry = cx.new(|cx| PluginSettings::new(hub, plugins, plugin, cx));
        let back = cx.subscribe(&entry, |this, _, crate::plugin::SettingsEvent::Back, cx| {
            this.plugin = None;
            cx.notify();
        });
        self.plugin = Some((entry, back));
        cx.notify();
    }

    fn plugin_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let (installed, unreachable) = {
            let plugins = self.plugins.read(cx);
            (plugins.installed().to_vec(), plugins.unreachable().join(", "))
        };
        let mut rows: Vec<AnyElement> = installed
            .iter()
            .enumerate()
            .map(|(index, plugin)| {
                let at = plugin.at.clone();
                row()
                    .id(("plugin", index))
                    .cursor_pointer()
                    .hover(|row| row.bg(Theme::raised()))
                    .on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| this.open_plugin(at.clone(), cx)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .min_w_0()
                            .child(brand::icon(
                                brand::symbol(plugin.icon.as_deref()),
                                20.,
                                Theme::ink_dim(),
                            ))
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
                                            .child(plugin.name.clone())
                                            .child(node_chip(plugin.at.node.clone())),
                                    )
                                    .child(dim(plugin.description.clone()).text_xs().truncate())
                                    .child(dim(crate::plugin::adds(plugin)).text_xs().truncate()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .flex_none()
                            .child(dim(plugin.state.label()))
                            .child(brand::icon(
                                brand::symbol(Some("chevron_right")),
                                18.,
                                Theme::ink_dim(),
                            )),
                    )
                    .into_any_element()
            })
            .collect();
        if rows.is_empty() {
            rows.push(
                row()
                    .child(dim("No node has plug-ins installed."))
                    .into_any_element(),
            );
        }
        if !unreachable.is_empty() {
            rows.push(
                row()
                    .child(
                        div()
                            .text_color(Theme::warn())
                            .child(format!("Couldn't reach {unreachable}")),
                    )
                    .into_any_element(),
            );
        }
        rows
    }
}

/// A node's name beside what runs on it.
fn node_chip(text: String) -> Div {
    div()
        .px_2()
        .rounded(px(8.))
        .text_xs()
        .bg(Theme::raised())
        .text_color(Theme::ink_dim())
        .child(text)
}
