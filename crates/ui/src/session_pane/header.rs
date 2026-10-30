use gpui::{ClickEvent, Context, IntoElement, ParentElement, Styled, div, prelude::*, px, relative};
use model::session::{Driver, SessionView};
use model::usage::tokens;

use super::{Mode, SessionPane};
use crate::theme::Theme;

impl SessionPane {
    /// One view of the session; a dot marks the view that holds it now.
    fn tab(
        &self,
        mode: Mode,
        label: &'static str,
        driving: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = self.mode == mode;
        div()
            .id(label)
            .h(px(30.))
            .px_4()
            .rounded(px(15.))
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .when(active, |tab| {
                tab.bg(Theme::selected()).text_color(Theme::on_selected())
            })
            .when(!active, |tab| tab.text_color(Theme::ink_dim()))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.show(mode, window, cx)))
            .child(label)
            .when(driving, |tab| {
                tab.child(div().size(px(6.)).rounded(px(3.)).bg(Theme::accent()))
            })
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
        let driver = view.driving().driver;
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
                    .child(self.tab(Mode::Chat, "Chat", driver == Some(Driver::Chat), cx))
                    .child(self.tab(Mode::Terminal, "Terminal", driver == Some(Driver::Terminal), cx)),
            )
            .when(
                self.mode == Mode::Terminal && self.live_terminal(cx).is_some(),
                |header| {
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
                },
            )
    }

    fn context_meter(view: &SessionView) -> Option<gpui::Div> {
        let usage = view.context()?;
        let meter = div()
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .text_xs()
            .text_color(Theme::ink_dim());
        let (Some(window), Some(fraction), Some(percent)) = (usage.window, usage.fraction(), usage.percent())
        else {
            return Some(meter.child(format!("{} in context", tokens(usage.used))));
        };
        let fill = if usage.nearly_full() {
            Theme::warn()
        } else {
            Theme::accent()
        };
        Some(
            meter
                .child(
                    div()
                        .w(px(64.))
                        .h(px(6.))
                        .rounded(px(3.))
                        .bg(Theme::card())
                        .child(div().h_full().w(relative(fraction)).rounded(px(3.)).bg(fill)),
                )
                .child(format!("{} / {}", tokens(usage.used), tokens(window)))
                .child(
                    div()
                        .when(usage.nearly_full(), |percent| percent.text_color(Theme::warn()))
                        .child(format!("{percent}%")),
                ),
        )
    }
}
