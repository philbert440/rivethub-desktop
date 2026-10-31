use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, ClickEvent, Context, FontWeight, IntoElement, ParentElement,
    SharedString, Styled, Window, div, prelude::*, px, relative,
};
use model::plugin::view::{self, Button, Property, Tone};

use super::{inline, symbol};
use crate::markdown_view;
use crate::plugin::frame::{Frame, Pressed};
use crate::text::now_ms;
use crate::theme::Theme;

impl Frame {
    pub(super) fn markdown(text: &str, key: &str, window: &Window) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .text_sm()
            .children(markdown_view::blocks(
                &model::markdown::parse(text),
                key,
                Theme::ink().into(),
                window,
            ))
            .into_any_element()
    }

    pub(super) fn code(&self, text: &str, key: &str) -> AnyElement {
        let scroll = self.logs.get(key).cloned().unwrap_or_default();
        div()
            .id(SharedString::from(format!("code-{key}")))
            .max_h(px(360.))
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .p_3()
            .rounded(px(12.))
            .bg(Theme::code())
            .font_family(Theme::MONO)
            .text_xs()
            .text_color(Theme::ink())
            .child(text.trim_end().to_owned())
            .into_any_element()
    }

    pub(super) fn placeholder(alt: &str, state: &str) -> AnyElement {
        div()
            .h(px(160.))
            .rounded(px(12.))
            .bg(Theme::code())
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_1()
            .text_xs()
            .text_color(Theme::ink_dim())
            .child(symbol(Some("image"), 24., Theme::ink_dim()))
            .child(alt.to_owned())
            .child(state.to_owned())
            .into_any_element()
    }

    pub(super) fn properties(items: &[Property]) -> AnyElement {
        let now = now_ms();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .children(items.iter().map(|item| {
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .text_sm()
                    .child(
                        div()
                            .w(px(140.))
                            .flex_none()
                            .text_color(Theme::ink_dim())
                            .child(item.label.clone()),
                    )
                    .child(div().flex().min_w_0().child(inline(&item.value, now)))
            }))
            .into_any_element()
    }

    pub(super) fn progress(value: Option<f32>, label: Option<&str>, key: &str) -> AnyElement {
        let track = div()
            .relative()
            .h(px(6.))
            .w_full()
            .rounded_full()
            .bg(Theme::line())
            .overflow_hidden();
        let track = match value {
            Some(value) => track
                .child(
                    div()
                        .h_full()
                        .rounded_full()
                        .bg(Theme::accent())
                        .w(relative(value.clamp(0., 1.))),
                )
                .into_any_element(),
            None => track
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .h_full()
                        .w(relative(0.3))
                        .rounded_full()
                        .bg(Theme::accent())
                        .with_animation(
                            SharedString::from(format!("progress-{key}")),
                            Animation::new(Duration::from_millis(1400)).repeat(),
                            |bar, moved| bar.left(relative(moved * 1.3 - 0.3)),
                        ),
                )
                .into_any_element(),
        };
        div()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(track)
            .children(label.map(|label| {
                div()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(label.to_owned())
            }))
            .into_any_element()
    }

    pub(super) fn notice(
        &self,
        tone: Tone,
        title: Option<&str>,
        text: &str,
        actions: &[Button],
        key: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (ink, fill) = Theme::tone(tone);
        let icon = match tone {
            Tone::Neutral | Tone::Info => "info",
            Tone::Success => "check_circle",
            Tone::Warning => "warning",
            Tone::Danger => "error",
        };
        div()
            .p_4()
            .rounded(px(16.))
            .bg(fill)
            .flex()
            .gap_3()
            .child(symbol(Some(icon), 20., ink))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(title.map(|title| {
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(ink)
                            .child(title.to_owned())
                    }))
                    .child(div().text_sm().text_color(Theme::ink()).child(text.to_owned()))
                    .when(!actions.is_empty(), |body| {
                        body.child(self.buttons(actions, key, None, cx).pt_2())
                    }),
            )
            .into_any_element()
    }

    pub(super) fn breadcrumbs(crumbs: &[view::Crumb], key: &str, cx: &mut Context<Self>) -> AnyElement {
        let last = crumbs.len().saturating_sub(1);
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .text_sm()
            .children(crumbs.iter().enumerate().map(|(index, crumb)| {
                let label = div()
                    .id(SharedString::from(format!("{key}-crumb-{index}")))
                    .px_1()
                    .rounded(px(6.))
                    .text_color(if index == last {
                        Theme::ink()
                    } else {
                        Theme::ink_dim()
                    })
                    .when_some(crumb.action.clone(), |label, action| {
                        label
                            .cursor_pointer()
                            .hover(|label| label.text_color(Theme::accent()))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                let pressed = Pressed {
                                    action: action.clone(),
                                    form: None,
                                };
                                this.run_pressed(pressed, cx);
                            }))
                    })
                    .child(crumb.label.clone());
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .when(index > 0, |crumb| {
                        crumb.child(symbol(Some("chevron_right"), 16., Theme::ink_dim()))
                    })
                    .child(label)
            }))
            .into_any_element()
    }

    pub(super) fn form(&self, form: &view::Form, key: &str, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.forms.get(&form.id) else {
            return div().into_any_element();
        };
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(state.view.clone())
            .children(
                state
                    .refusal
                    .clone()
                    .map(|refusal| div().text_sm().text_color(Theme::danger()).child(refusal)),
            )
            .when(!form.buttons.is_empty(), |body| {
                body.child(self.buttons(&form.buttons, key, Some(&form.id), cx).justify_end())
            })
            .into_any_element()
    }
}
