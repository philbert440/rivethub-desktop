use gpui::{ClickEvent, Context, IntoElement, ParentElement, Styled, div, prelude::*, px, relative};
use model::session::SessionView;

use super::{Mode, SessionPane};
use crate::theme::Theme;

impl SessionPane {
    fn tab(&self, mode: Mode, label: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.mode == mode;
        div()
            .id(label)
            .h(px(30.))
            .px_4()
            .rounded(px(15.))
            .flex()
            .items_center()
            .cursor_pointer()
            .when(active, |tab| {
                tab.bg(Theme::selected()).text_color(Theme::on_selected())
            })
            .when(!active, |tab| tab.text_color(Theme::ink_dim()))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.show(mode, window, cx)))
            .child(label)
    }

    pub(super) fn header(&self, view: &SessionView, cx: &mut Context<Self>) -> gpui::Div {
        let meta = self
            .summary
            .model
            .as_deref()
            .map(|model| self.model_label(model))
            .into_iter()
            .chain(self.summary.cwd.clone())
            .collect::<Vec<_>>()
            .join(" · ");
        div()
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_6()
            .pt_4()
            .pb_2()
            .child(self.title(cx))
            .child(
                div()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .truncate()
                    .child(meta),
            )
            .child(div().flex_1())
            .children(Self::context_meter(view))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .p(px(3.))
                    .rounded(px(18.))
                    .bg(Theme::card())
                    .child(self.tab(Mode::Chat, "Chat", cx))
                    .child(self.tab(Mode::Terminal, "Terminal", cx)),
            )
            .when(self.mode == Mode::Terminal && self.terminal.is_some(), |header| {
                header.child(
                    div()
                        .id("end-terminal")
                        .h(px(30.))
                        .px_3()
                        .rounded(px(15.))
                        .border_1()
                        .border_color(Theme::line())
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .text_color(Theme::ink_dim())
                        .hover(|button| button.text_color(Theme::ink()))
                        .on_click(
                            cx.listener(|this, _: &ClickEvent, window, cx| this.end_terminal(window, cx)),
                        )
                        .child("End"),
                )
            })
    }

    fn context_meter(view: &SessionView) -> Option<gpui::Div> {
        let usage = view.context()?;
        let fill = if usage.nearly_full() {
            Theme::warn()
        } else {
            Theme::accent()
        };
        Some(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .text_color(Theme::ink_dim())
                .child(
                    div()
                        .w(px(64.))
                        .h(px(6.))
                        .rounded(px(3.))
                        .bg(Theme::card())
                        .child(
                            div()
                                .h_full()
                                .w(relative(usage.fraction()))
                                .rounded(px(3.))
                                .bg(fill),
                        ),
                )
                .child(format!("{}k / {}k", usage.used / 1000, usage.window / 1000)),
        )
    }
}
