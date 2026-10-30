use gpui::{AnyElement, ClickEvent, Context, IntoElement, ParentElement, Rgba, Styled, div, prelude::*, px};
use model::plan::{Plan, StepStatus};
use model::session::SessionView;

use super::SessionPane;
use crate::theme::Theme;
use crate::transcript::Fold;

pub(crate) fn mark(status: StepStatus) -> (&'static str, Rgba) {
    match status {
        StepStatus::Completed => ("✓", Theme::ink_dim()),
        StepStatus::InProgress => ("●", Theme::accent()),
        StepStatus::Pending => ("○", Theme::ink_dim()),
    }
}

impl SessionPane {
    /// The agent's to-do list above the composer: how far along it is and the step it's on, opening
    /// to every step. It shows while a step is left or a turn runs.
    pub(super) fn plan_panel(&self, view: &SessionView, cx: &mut Context<Self>) -> Option<AnyElement> {
        let plan = view.plan();
        if plan.steps.is_empty() || (plan.finished() && !view.busy()) {
            return None;
        }
        let open = self.folds.open(Fold::Plan);
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
                .child(Self::plan_head(plan, open, cx))
                .when(open, |panel| {
                    panel.children(plan.steps.iter().map(|step| {
                        let (glyph, ink) = mark(step.status);
                        div()
                            .flex()
                            .items_start()
                            .gap_2()
                            .text_sm()
                            .child(div().flex_none().w(px(14.)).text_color(ink).child(glyph))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_color(match step.status {
                                        StepStatus::Completed => Theme::ink_dim(),
                                        StepStatus::InProgress | StepStatus::Pending => Theme::ink(),
                                    })
                                    .child(step.content.clone()),
                            )
                    }))
                })
                .into_any_element(),
        )
    }

    fn plan_head(plan: &Plan, open: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let current = plan.current().map(|step| step.content.clone());
        div()
            .id("plan-head")
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .text_xs()
            .text_color(Theme::ink_dim())
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_fold(Fold::Plan, cx)))
            .child(format!("Plan · {} of {} done", plan.done(), plan.steps.len()))
            .when_some(current.filter(|_| !open), |head, current| {
                head.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .text_color(Theme::ink())
                        .child(current),
                )
            })
            .child(div().flex_1())
            .child(if open { "▾" } else { "▸" })
    }
}
