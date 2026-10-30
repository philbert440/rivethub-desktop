use gpui::{
    AnyElement, ClickEvent, Context, Entity, Focusable, IntoElement, ParentElement, Styled, Subscription,
    Window, div, prelude::*, px,
};
use model::session::SessionSummary;

use super::{PanelEvent, SidePanel};
use crate::input::{Submitted, TextInput};
use crate::theme::Theme;

pub(super) struct Renaming {
    pub(super) session: SessionSummary,
    input: Entity<TextInput>,
    problem: Option<String>,
    _submitted: Subscription,
}

impl SidePanel {
    pub(super) fn begin_rename(
        &mut self,
        session: SessionSummary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| {
            let mut input = TextInput::new("Session title", cx);
            input.set_text(session.title.as_deref().unwrap_or_default().trim(), cx);
            input
        });
        let submitted = cx.subscribe(&input, |this, _, Submitted(title), cx| {
            this.commit_rename(title.clone(), cx);
        });
        window.focus(&input.focus_handle(cx), cx);
        self.renaming = Some(Renaming {
            session,
            input,
            problem: None,
            _submitted: submitted,
        });
        cx.notify();
    }

    fn commit_rename(&mut self, title: String, cx: &mut Context<Self>) {
        let Some(session) = self.renaming.as_ref().map(|renaming| renaming.session.clone()) else {
            return;
        };
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let renamed = hub.rename(&session, title).await;
            this.update(cx, |this, cx| {
                // The person may have moved on to renaming another session meanwhile.
                let still_open = this
                    .renaming
                    .as_mut()
                    .filter(|renaming| renaming.session.id == session.id);
                match renamed {
                    Ok(renamed) => {
                        if still_open.is_some() {
                            this.renaming = None;
                        }
                        cx.emit(PanelEvent::Renamed(renamed));
                    }
                    Err(err) => {
                        if let Some(renaming) = still_open {
                            renaming.problem = Some(format!("rename failed: {err}"));
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn rename_row(
        renaming: &Renaming,
        indent: gpui::Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .pl(px(8.) + indent)
            .pr_2()
            .py(px(4.))
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px_2()
                            .py_1()
                            .rounded(px(10.))
                            .bg(Theme::card())
                            .border_1()
                            .border_color(Theme::accent())
                            .child(renaming.input.clone()),
                    )
                    .child(
                        div()
                            .id("cancel-history-rename")
                            .cursor_pointer()
                            .text_color(Theme::ink_dim())
                            .hover(|cancel| cancel.text_color(Theme::ink()))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.renaming = None;
                                cx.notify();
                            }))
                            .child("×"),
                    ),
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
