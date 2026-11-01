use gpui::{AnyElement, ClickEvent, Context, IntoElement, ParentElement, Styled, div, prelude::*, px};
use model::session::{Driver, Driving, SessionView};

use super::{Mode, SessionPane};
use crate::theme::Theme;

impl SessionPane {
    /// Above the composer while the other view holds the session: who has it, the messages the
    /// node holds for it, and a question or permission prompt only the terminal can answer.
    pub(super) fn driving_dock(&self, view: &SessionView, cx: &mut Context<Self>) -> Option<AnyElement> {
        let driving = view.driving();
        if !driving.by_terminal() && driving.queued.is_empty() {
            return None;
        }
        Some(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_2()
                .children(self.terminal_ask(view, cx))
                .child(Self::held(driving))
                .into_any_element(),
        )
    }

    fn held(driving: &Driving) -> impl IntoElement {
        let said = match (driving.driver, driving.busy, driving.queued.len()) {
            (Some(Driver::Terminal), true, 0) => {
                "The terminal has this session. A message sent now waits for its turn to end.".to_owned()
            }
            (Some(Driver::Terminal), false, 0) => {
                "The terminal has this session. A message sent now brings it back to Chat.".to_owned()
            }
            (Some(Driver::Terminal), _, 1) => "1 message waits for the terminal's turn to end".to_owned(),
            (Some(Driver::Terminal), _, held) => {
                format!("{held} messages wait for the terminal's turn to end")
            }
            (_, _, 1) => "1 message waits for the turn running now".to_owned(),
            (_, _, held) => format!("{held} messages wait for the turn running now"),
        };
        div()
            .w_full()
            .px_4()
            .py_2()
            .rounded(px(18.))
            .bg(Theme::panel())
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .child(div().size(px(6.)).rounded(px(3.)).bg(Theme::accent()))
                    .child(said),
            )
            .children(driving.queued.iter().map(|text| {
                div()
                    .truncate()
                    .text_color(Theme::ink())
                    .child(crate::text::one_line(Some(text)))
            }))
    }

    fn terminal_ask(&self, view: &SessionView, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let driving = view.driving();
        if !driving.by_terminal() {
            return None;
        }
        let (heading, asked) = match &driving.approval {
            Some(approval) => (
                "Waiting for approval in the terminal",
                model::tools::label(
                    &approval.tool,
                    &approval.input,
                    self.summary.cwd.as_deref(),
                    std::env::var("HOME").ok().as_deref(),
                ),
            ),
            None if view.needs_you() => (
                "Waiting for your answer in the terminal",
                "A question there waits for you.".to_owned(),
            ),
            None => return None,
        };
        Some(
            div()
                .w_full()
                .p_5()
                .rounded(px(28.))
                .bg(Theme::warn_container())
                .border_1()
                .border_color(Theme::warn_line())
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().text_xs().text_color(Theme::warn()).child(heading))
                        .child(div().text_size(px(15.)).text_color(Theme::ink()).child(asked)),
                )
                .child(
                    div()
                        .id("switch-to-terminal")
                        .flex_none()
                        .h(px(34.))
                        .px_4()
                        .rounded(px(17.))
                        .bg(Theme::accent())
                        .text_color(Theme::on_accent())
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.show(Mode::Terminal, window, cx);
                        }))
                        .child("Switch to Terminal"),
                ),
        )
    }
}
