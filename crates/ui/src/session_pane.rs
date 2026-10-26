use std::sync::Arc;

use app::SessionHandle;
use gpui::{
    AnyElement, App, Context, Entity, IntoElement, ListAlignment, ListState, ParentElement, Render, Styled,
    Subscription, Task, Window, div, list, prelude::*, px, relative,
};
use model::session::{Role, SessionView, ToolState, Turn};
use parking_lot::RwLock;

use crate::input::{Submitted, TextInput};
use crate::theme::Theme;

pub struct SessionPane {
    handle: SessionHandle,
    turns: ListState,
    composer: Entity<TextInput>,
    _revisions: Task<()>,
    _submitted: Subscription,
}

impl SessionPane {
    pub fn new(handle: SessionHandle, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| TextInput::new("Message claude…", cx));
        let submitted = cx.subscribe(&composer, |this, _, Submitted(text), _| {
            this.handle.send(text.clone());
        });
        let mut revisions = handle.revisions();
        let watcher = cx.spawn(async move |this, cx| {
            while revisions.changed().await.is_ok() {
                if this.update(cx, SessionPane::refresh).is_err() {
                    break;
                }
            }
        });
        Self {
            handle,
            turns: ListState::new(0, ListAlignment::Bottom, px(600.)),
            composer,
            _revisions: watcher,
            _submitted: submitted,
        }
    }

    pub fn composer(&self) -> &Entity<TextInput> {
        &self.composer
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let count = {
            let view = self.handle.view();
            let view = view.read();
            view.turns().len() + usize::from(view.live().is_some())
        };
        let known = self.turns.item_count();
        if count < known {
            self.turns.reset(count);
        } else if count > known {
            self.turns.splice(known..known, count - known);
        } else if count > 0 {
            self.turns.splice(count - 1..count, 1);
        }
        cx.notify();
    }

    fn context_bar(view: &SessionView) -> Option<impl IntoElement> {
        let usage = view.context()?;
        let fraction = usage.fraction();
        let fill = if fraction >= 0.8 {
            Theme::warn()
        } else {
            Theme::accent()
        };
        Some(
            div()
                .flex()
                .items_center()
                .gap_3()
                .text_xs()
                .text_color(Theme::ink_dim())
                .child("Context")
                .child(
                    div()
                        .flex_1()
                        .h(px(6.))
                        .rounded(px(3.))
                        .bg(Theme::card())
                        .child(div().h_full().w(relative(fraction)).rounded(px(3.)).bg(fill)),
                )
                .child(format!("{}k / {}k", usage.used / 1000, usage.window / 1000)),
        )
    }

    fn needs_you(view: &SessionView) -> Option<impl IntoElement> {
        let approval = view.approvals().first()?;
        let reason = approval.reason.clone().unwrap_or_default();
        Some(
            div()
                .p_4()
                .rounded(px(18.))
                .bg(Theme::warn_container())
                .border_1()
                .border_color(Theme::warn_line())
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_color(Theme::warn())
                        .child(format!("Approval: {}", approval.tool)),
                )
                .child(div().text_color(Theme::ink()).child(reason)),
        )
    }
}

fn render_turn(view: &Arc<RwLock<SessionView>>, index: usize) -> AnyElement {
    let view = view.read();
    let turn = view
        .turns()
        .get(index)
        .or_else(|| view.live().filter(|_| index == view.turns().len()));
    let Some(turn) = turn else {
        return div().into_any_element();
    };
    turn_element(turn).into_any_element()
}

fn turn_element(turn: &Turn) -> impl IntoElement {
    let row = div().w_full().flex().py_2();
    match turn.role {
        Role::User => row.justify_end().child(
            div()
                .max_w(px(600.))
                .px_4()
                .py_3()
                .rounded(px(18.))
                .bg(Theme::raised())
                .child(turn.text.clone()),
        ),
        Role::Assistant => row.child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .max_w(px(820.))
                .children(turn.tools.iter().map(|tool| {
                    let (label, color) = match tool.state {
                        ToolState::Running => ("running", Theme::ink_dim()),
                        ToolState::Done => ("done", Theme::accent()),
                        ToolState::Failed => ("failed", Theme::warn()),
                    };
                    div()
                        .flex()
                        .gap_3()
                        .px_3()
                        .py_2()
                        .rounded(px(14.))
                        .bg(Theme::card())
                        .child(tool.name.clone())
                        .child(div().text_color(color).child(label))
                }))
                .child(div().text_color(Theme::ink()).child(turn.text.clone())),
        ),
    }
}

impl Render for SessionPane {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let shared = self.handle.view();
        let view = shared.read();
        let error = view.error().map(str::to_owned);
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .child(
                div().flex_1().w(px(820.)).pt_6().child(
                    list(self.turns.clone(), {
                        let shared = Arc::clone(&shared);
                        move |index, _: &mut Window, _: &mut App| render_turn(&shared, index)
                    })
                    .size_full(),
                ),
            )
            .child(
                div()
                    .w(px(820.))
                    .pb_5()
                    .pt_2()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .children(Self::needs_you(&view))
                    .children(error.map(|message| div().text_color(Theme::warn()).child(message)))
                    .children(Self::context_bar(&view))
                    .child(
                        div()
                            .px_5()
                            .py_3()
                            .rounded(px(24.))
                            .bg(Theme::card())
                            .child(self.composer.clone()),
                    ),
            )
    }
}
