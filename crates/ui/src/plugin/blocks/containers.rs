use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, IntoElement, ParentElement, SharedString, Styled, Window,
    div, prelude::*, px, relative,
};
use model::plugin::view::{Block, Button, Tab};

use super::{container, symbol};
use crate::plugin::frame::{Frame, Placement};
use crate::theme::Theme;

/// The narrowest a page is for a split to sit side by side.
const SIDE_BY_SIDE: f32 = 1100.;

impl Frame {
    #[expect(
        clippy::too_many_arguments,
        reason = "a split's two sides and where it is drawn"
    )]
    pub(super) fn split(
        &self,
        primary: &[Block],
        secondary: &[Block],
        ratio: Option<f32>,
        key: &str,
        level: u8,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let wide = self.placement == Placement::Page && window.viewport_size().width >= px(SIDE_BY_SIDE);
        let primary = self.stack(primary, &format!("{key}p"), level, window, cx);
        let secondary = self.stack(secondary, &format!("{key}s"), level, window, cx);
        if wide {
            div()
                .flex()
                .items_start()
                .gap_4()
                .child(
                    primary
                        .w(relative(ratio.unwrap_or(0.5).clamp(0.2, 0.8)))
                        .flex_none(),
                )
                .child(secondary.flex_1())
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap_4()
                .child(primary)
                .child(secondary)
                .into_any_element()
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "a section's parts, as the contract lists them"
    )]
    pub(super) fn section(
        &self,
        (title, subtitle): (Option<&str>, Option<&str>),
        actions: &[Button],
        blocks: &[Block],
        collapsed: Option<bool>,
        key: &str,
        level: u8,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let folded = collapsed.map(|start| self.folded.get(key).copied().unwrap_or(start));
        let owned = key.to_owned();
        let header = div()
            .id(SharedString::from(format!("section-{key}")))
            .flex()
            .items_center()
            .gap_3()
            .when_some(folded, |header, folded| {
                header
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.folded.insert(owned.clone(), !folded);
                        cx.notify();
                    }))
                    .child(symbol(
                        Some(if folded { "chevron_right" } else { "expand_more" }),
                        18.,
                        Theme::ink_dim(),
                    ))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .children(title.map(|title| {
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(Theme::ink())
                            .child(title.to_owned())
                    }))
                    .children(subtitle.map(|subtitle| {
                        div()
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .child(subtitle.to_owned())
                    })),
            )
            .child(self.buttons(actions, key, None, cx));
        let open = folded != Some(true);
        div()
            .p_4()
            .rounded(px(20.))
            .bg(container(level))
            .flex()
            .flex_col()
            .gap_3()
            .when(
                title.is_some() || subtitle.is_some() || !actions.is_empty() || folded.is_some(),
                |section| section.child(header),
            )
            .when(open, |section| {
                section.child(self.stack(blocks, key, level + 1, window, cx))
            })
            .into_any_element()
    }

    pub(super) fn tabs(
        &self,
        tabs: &[Tab],
        selected: Option<&str>,
        key: &str,
        level: u8,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let set = tabs
            .iter()
            .map(|tab| tab.id.as_str())
            .collect::<Vec<_>>()
            .join("|");
        let current = self
            .tabs
            .get(&set)
            .map(String::as_str)
            .or(selected)
            .filter(|id| tabs.iter().any(|tab| tab.id == *id))
            .or_else(|| tabs.first().map(|tab| tab.id.as_str()))
            .unwrap_or_default()
            .to_owned();
        let bar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .p_1()
            .rounded(px(18.))
            .bg(Theme::panel())
            .children(tabs.iter().enumerate().map(|(index, tab)| {
                let here = tab.id == current;
                let (set, id) = (set.clone(), tab.id.clone());
                div()
                    .id(SharedString::from(format!("{key}-tab-{index}")))
                    .h(px(30.))
                    .px_4()
                    .rounded(px(15.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .map(|chip| {
                        if here {
                            chip.bg(Theme::selected()).text_color(Theme::on_selected())
                        } else {
                            chip.text_color(Theme::ink_dim())
                                .hover(|chip| chip.bg(Theme::card()))
                        }
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.tabs.insert(set.clone(), id.clone());
                        cx.notify();
                    }))
                    .child(tab.title.clone())
                    .children(tab.badge.clone().map(|badge| {
                        div()
                            .px_2()
                            .rounded(px(9.))
                            .bg(Theme::raised())
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .child(badge)
                    }))
            }));
        let body = tabs
            .iter()
            .find(|tab| tab.id == current)
            .map(|tab| self.stack(&tab.blocks, &format!("{key}:{}", tab.id), level, window, cx));
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().flex().child(bar))
            .children(body)
            .into_any_element()
    }
}
