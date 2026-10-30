use gpui::{ClickEvent, Context, FontWeight, IntoElement, ParentElement, Styled, div, prelude::*, px};
use model::plugin::view::Tone;

use super::{Confirming, Plugins};
use crate::brand;
use crate::theme::Theme;

impl Plugins {
    fn confirmed(&mut self, cx: &mut Context<Self>) {
        if let Some(confirming) = self.confirming.take()
            && let Some(frame) = confirming.frame.upgrade()
        {
            frame.update(cx, |frame, cx| frame.run_pressed(confirming.pressed, cx));
        }
        cx.notify();
    }

    pub(super) fn confirm_card(confirming: &Confirming, cx: &mut Context<Self>) -> impl IntoElement {
        let pill = |id: &'static str| {
            div()
                .id(id)
                .h(px(34.))
                .px_4()
                .rounded(px(17.))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_sm()
        };
        div()
            .absolute()
            .inset_0()
            .bg(Theme::scrim())
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(400.))
                    .p_6()
                    .rounded(px(28.))
                    .bg(Theme::card())
                    .border_1()
                    .border_color(Theme::line())
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::MEDIUM)
                            .child(confirming.confirm.title.clone()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(Theme::ink_dim())
                            .child(confirming.confirm.text.clone()),
                    )
                    .child(
                        div()
                            .pt_2()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                pill("confirm-cancel")
                                    .text_color(Theme::ink_dim())
                                    .hover(|pill| pill.text_color(Theme::ink()))
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.confirming = None;
                                        cx.notify();
                                    }))
                                    .child("Cancel"),
                            )
                            .child(
                                pill("confirm-go")
                                    .bg(Theme::danger_container())
                                    .text_color(Theme::danger())
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.confirmed(cx)))
                                    .child(
                                        confirming
                                            .confirm
                                            .confirm_label
                                            .clone()
                                            .unwrap_or_else(|| "Confirm".to_owned()),
                                    ),
                            ),
                    ),
            )
    }

    pub(super) fn toasts(&self) -> impl IntoElement {
        div()
            .absolute()
            .bottom(px(24.))
            .left_0()
            .right_0()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .children(self.toasts.iter().map(|toast| {
                let (ink, _) = Theme::tone(toast.tone);
                let icon = match toast.tone {
                    Tone::Success => "check_circle",
                    Tone::Warning => "warning",
                    Tone::Danger => "error",
                    Tone::Neutral | Tone::Info => "info",
                };
                div()
                    .max_w(px(520.))
                    .px_4()
                    .py_2()
                    .rounded(px(18.))
                    .bg(Theme::raised())
                    .border_1()
                    .border_color(Theme::line())
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .text_color(Theme::ink())
                    .child(brand::icon(brand::symbol(Some(icon)), 18., ink))
                    .child(div().min_w_0().child(toast.text.clone()))
            }))
    }
}
