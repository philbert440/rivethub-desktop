use gpui::{
    AnyElement, ClickEvent, Context, FocusHandle, Focusable, IntoElement, ParentElement, Styled, Window, div,
    prelude::*, px,
};
use model::chat::totals;
use model::prompt::Prompt;
use model::session::{Decision, SessionView};
use model::usage::tokens;

use super::SessionPane;
use crate::input::TextInput;
use crate::theme::Theme;
use crate::tip::Tip;

impl SessionPane {
    /// One answer to an approval; answering hands the keyboard back to the composer the card stood
    /// in for.
    fn decision_button(
        &self,
        label: &'static str,
        decision: Decision,
        primary: bool,
        request_id: &str,
        composer: &FocusHandle,
    ) -> impl IntoElement {
        let handle = self.handle.clone();
        let request_id = request_id.to_owned();
        let composer = composer.clone();
        div()
            .id(label)
            .h(px(34.))
            .px_4()
            .rounded(px(17.))
            .flex()
            .items_center()
            .cursor_pointer()
            .when(primary, |button| {
                button.bg(Theme::accent()).text_color(Theme::on_accent())
            })
            .when(!primary, |button| {
                button
                    .border_1()
                    .border_color(Theme::line())
                    .text_color(Theme::ink())
            })
            .on_click(move |_, window, cx| {
                handle.decide(request_id.clone(), decision);
                window.focus(&composer, cx);
            })
            .child(label)
    }

    pub(super) fn approval_card(&self, view: &SessionView, composer: &FocusHandle) -> Option<AnyElement> {
        let approval = view.approvals().first()?;
        let request_id = approval.request_id.clone();
        Some(
            div()
                .w_full()
                .p_5()
                .rounded(px(28.))
                .bg(Theme::warn_container())
                .border_1()
                .border_color(Theme::warn_line())
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .text_xs()
                        .text_color(Theme::warn())
                        .child("Waiting for approval"),
                )
                .child(
                    div()
                        .text_size(px(15.))
                        .text_color(Theme::ink())
                        .child(approval.tool.clone()),
                )
                .children(approval.reason.clone().map(|reason| {
                    div()
                        .font_family(Theme::MONO)
                        .text_size(px(12.))
                        .text_color(Theme::ink_dim())
                        .child(reason)
                }))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .justify_end()
                        .child(self.decision_button("Reject", Decision::Deny, false, &request_id, composer))
                        .child(self.decision_button(
                            "Allow for session",
                            Decision::AllowForSession,
                            false,
                            &request_id,
                            composer,
                        ))
                        .child(self.decision_button(
                            "Allow once",
                            Decision::Allow,
                            true,
                            &request_id,
                            composer,
                        )),
                )
                .into_any_element(),
        )
    }

    /// Puts a queued prompt back in the composer, after any draft, with its images.
    fn edit_queued(&mut self, queued: &Prompt, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prompt) = self.handle.take_queued(queued) else {
            return;
        };
        self.composer.update(cx, |composer, cx| {
            let draft = composer.text().trim_end().to_owned();
            let text = if draft.is_empty() {
                prompt.text.clone()
            } else {
                format!("{draft}\n{}", prompt.text)
            };
            composer.set_text(&text, cx);
        });
        self.attachments.extend(prompt.attachments);
        window.focus(&self.composer.focus_handle(cx), cx);
        cx.notify();
    }

    pub(super) fn queue_dock(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let queued = self.handle.queued();
        if queued.is_empty() {
            return None;
        }
        Some(
            div()
                .w_full()
                .px_4()
                .py_2()
                .rounded(px(18.))
                .bg(Theme::panel())
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_xs().text_color(Theme::ink_dim()).child(format!(
                    "{} queued {}",
                    queued.len(),
                    if queued.len() == 1 { "message" } else { "messages" }
                )))
                .children(queued.into_iter().enumerate().map(|(index, prompt)| {
                    let (steering, unqueuing) = (self.handle.clone(), self.handle.clone());
                    let (steered, dropped, queued) = (prompt.clone(), prompt.clone(), prompt.clone());
                    let images = match prompt.attachments.len() {
                        0 => None,
                        1 => Some("1 image".to_owned()),
                        count => Some(format!("{count} images")),
                    };
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .text_color(Theme::ink())
                                .child(crate::text::one_line(Some(&prompt.text))),
                        )
                        .children(
                            images.map(|images| div().text_xs().text_color(Theme::ink_dim()).child(images)),
                        )
                        .when(self.steers(), |row| {
                            row.child(
                                div()
                                    .id(("steer-queued", index))
                                    .cursor_pointer()
                                    .text_xs()
                                    .text_color(Theme::accent())
                                    .on_click(move |_, _, _| steering.steer(steered.clone()))
                                    .child("Steer"),
                            )
                        })
                        .child(
                            div()
                                .id(("edit-queued", index))
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.edit_queued(&queued, window, cx);
                                }))
                                .child(crate::brand::icon("icons/rename.svg", 13., Theme::ink_dim())),
                        )
                        .child(
                            div()
                                .id(("unqueue", index))
                                .cursor_pointer()
                                .text_color(Theme::ink_dim())
                                .on_click(move |_, _, _| unqueuing.unqueue(dropped.clone()))
                                .child("×"),
                        )
                }))
                .into_any_element(),
        )
    }

    /// The session so far, beside Send: how fast the replies streamed here came, what the session
    /// wrote, and how much of what it read came from cache.
    fn session_stats(&self, view: &SessionView) -> Option<impl IntoElement> {
        let usage = totals(self.exchanges.iter().map(|view| &view.exchange));
        let parts: Vec<String> = view
            .pace()
            .tokens_per_second()
            .map(|rate| format!("{rate} tok/s"))
            .into_iter()
            .chain((usage.output > 0).then(|| format!("{} out", tokens(usage.output))))
            .chain(
                usage
                    .cached_percent()
                    .filter(|share| *share > 0)
                    .map(|share| format!("{share}% cached")),
            )
            .collect();
        (!parts.is_empty()).then(|| {
            div()
                .flex_none()
                .px_3()
                .text_xs()
                .text_color(Theme::ink_dim())
                .child(parts.join(" · "))
        })
    }

    fn send_button(&self, busy: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let draft = !self.composer.read(cx).is_empty();
        let stop = busy && !draft;
        let enabled = stop || (draft && self.uploading == 0);
        let button = div()
            .id("send")
            .h(px(34.))
            .min_w(px(34.))
            .rounded(px(17.))
            .flex()
            .items_center()
            .justify_center()
            .bg(Theme::accent())
            .text_color(Theme::on_accent())
            .when(!enabled, |button| button.opacity(0.4))
            .when(enabled, gpui::Styled::cursor_pointer)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if stop {
                    this.handle.stop();
                } else if let Some(text) = this.composer.update(cx, TextInput::take) {
                    this.submit(text, cx);
                }
            }));
        match (stop, busy) {
            (true, _) => button
                .tooltip(Tip::text("Stop · Esc twice"))
                .child(crate::brand::icon("icons/square.svg", 14., Theme::on_accent())),
            (false, true) => button
                .px_3()
                .when(self.steers(), |button| {
                    button.tooltip(Tip::text("Queue · Ctrl+Shift+Enter steers instead"))
                })
                .child("Queue"),
            (false, false) => button.child(crate::brand::icon("icons/arrow-up.svg", 18., Theme::on_accent())),
        }
    }

    /// After a first Esc during a turn, what a second one does.
    fn escape_hint(&self, busy: bool) -> Option<impl IntoElement> {
        (busy && self.escaped.is_some()).then(|| {
            div()
                .flex_none()
                .px_3()
                .text_xs()
                .text_color(Theme::ink_dim())
                .child("Esc again stops the turn")
        })
    }

    pub(super) fn composer_card(&self, view: &SessionView, cx: &mut Context<Self>) -> AnyElement {
        div()
            .key_context("Composer")
            .on_action(cx.listener(Self::escape_composer))
            .on_action(cx.listener(Self::steer_draft))
            .w_full()
            .rounded(px(28.))
            .bg(Theme::card())
            .flex()
            .flex_col()
            .children(self.attachment_row(cx))
            .children(self.picker_row(cx))
            .child(
                div()
                    .min_h(px(44.))
                    .pl_5()
                    .pr_4()
                    .pt_3()
                    .child(self.composer.clone()),
            )
            .child(
                div()
                    .h(px(46.))
                    .px_2()
                    .pb_2()
                    .flex()
                    .items_center()
                    .when(self.takes_images(), |row| row.child(Self::attach_button(cx)))
                    .children(self.switch_chips(cx))
                    .child(div().flex_1())
                    .children(self.escape_hint(view.busy()))
                    .children(self.session_stats(view))
                    .child(self.send_button(view.busy(), cx)),
            )
            .into_any_element()
    }
}
