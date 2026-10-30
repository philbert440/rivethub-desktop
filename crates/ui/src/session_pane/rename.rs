use gpui::{
    ClickEvent, Context, Entity, Focusable, IntoElement, ParentElement, Styled, Subscription, Window, div,
    prelude::*, px,
};

use super::{PaneEvent, SessionPane};
use crate::input::{Submitted, TextInput};
use crate::theme::Theme;

pub(super) struct Renaming {
    input: Entity<TextInput>,
    problem: Option<String>,
    _submitted: Subscription,
}

impl SessionPane {
    fn begin_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.summary.title.clone().unwrap_or_default();
        let input = cx.new(|cx| {
            let mut input = TextInput::new("Session title", cx);
            input.set_text(current.trim(), cx);
            input
        });
        let submitted = cx.subscribe_in(&input, window, |this, _, Submitted(title), window, cx| {
            this.commit_rename(title.clone(), window, cx);
        });
        window.focus(&input.focus_handle(cx), cx);
        self.renaming = Some(Renaming {
            input,
            problem: None,
            _submitted: submitted,
        });
        cx.notify();
    }

    /// A finished rename hands the keyboard back to the composer, as cancelling does.
    fn commit_rename(&mut self, title: String, window: &mut Window, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        let summary = self.summary.clone();
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let renamed = hub.rename(&summary, title).await;
            cx.update_window(window_handle, |_, window, cx| {
                this.update(cx, |this, cx| {
                    match renamed {
                        Ok(renamed) => {
                            this.summary.title = renamed.title;
                            this.renaming = None;
                            window.focus(&this.composer.focus_handle(cx), cx);
                            cx.emit(PaneEvent::Renamed(Box::new(this.summary.clone())));
                        }
                        Err(err) => {
                            if let Some(renaming) = this.renaming.as_mut() {
                                renaming.problem = Some(format!("rename failed: {err}"));
                            }
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn title(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(renaming) = &self.renaming else {
            return div()
                .id("session-title")
                .flex_shrink(1.)
                .min_w_0()
                .truncate()
                .text_color(Theme::ink())
                .cursor_text()
                .hover(|title| title.text_color(Theme::accent()))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.begin_rename(window, cx)))
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .truncate()
                        .child(crate::text::one_line(self.summary.title.as_deref())),
                )
                .child(crate::brand::icon("icons/rename.svg", 14., Theme::ink_dim()))
                .into_any_element();
        };
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .w(px(360.))
                    .px_3()
                    .py_1()
                    .rounded(px(10.))
                    .bg(Theme::card())
                    .border_1()
                    .border_color(Theme::accent())
                    .child(renaming.input.clone()),
            )
            .child(
                div()
                    .id("cancel-rename")
                    .cursor_pointer()
                    .text_color(Theme::ink_dim())
                    .hover(|cancel| cancel.text_color(Theme::ink()))
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.renaming = None;
                        window.focus(&this.composer.focus_handle(cx), cx);
                        cx.notify();
                    }))
                    .child("×"),
            )
            .children(
                renaming
                    .problem
                    .clone()
                    .map(|problem| div().text_xs().text_color(Theme::warn()).child(problem)),
            )
            .into_any_element()
    }
}
