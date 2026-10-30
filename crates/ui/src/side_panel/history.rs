use gpui::{
    AnyElement, ClickEvent, Context, IntoElement, MouseButton, MouseDownEvent, ParentElement, SharedString,
    Styled, div, prelude::*, px,
};
use model::history::{ChipCounts, Phase};
use model::lineage;
use model::session::SessionSummary;

use super::drag::{DragGhost, DraggedSession};
use super::{HISTORY_PAGE, PanelEvent, SidePanel};
use crate::theme::Theme;

const HISTORY_ROW: &str = "history-row";

impl SidePanel {
    /// Where a History session stands, by the live feed the workspace hands over, else by its listing.
    pub(super) fn phase(&self, session: &SessionSummary) -> Phase {
        self.phases.get(&session.id).copied().unwrap_or_else(|| {
            Phase::of(
                session.blocked,
                session.status == model::session::SessionStatus::Active,
                session.status.is_finished(),
            )
        })
    }

    fn chip(
        id: impl Into<gpui::ElementId>,
        label: &str,
        count: usize,
        on: bool,
        toggle: impl Fn(&mut Self) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .h(px(22.))
            .px_2()
            .rounded(px(11.))
            .flex()
            .items_center()
            .gap_1()
            .cursor_pointer()
            .text_xs()
            .map(|chip| {
                if on {
                    chip.bg(Theme::selected()).text_color(Theme::on_selected())
                } else {
                    chip.border_1()
                        .border_color(Theme::line())
                        .text_color(if count == 0 {
                            Theme::ink_dim()
                        } else {
                            Theme::ink()
                        })
                        .hover(|chip| chip.bg(Theme::card()))
                }
            })
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                toggle(this);
                cx.notify();
            }))
            .child(label.to_owned())
            .child(div().opacity(0.7).child(count.to_string()))
    }

    /// Status chips, then harness chips when the list holds more than one harness or one is picked.
    pub(super) fn history_chips(&self, counts: ChipCounts, cx: &mut Context<Self>) -> AnyElement {
        let phases: Vec<AnyElement> = counts
            .phases
            .into_iter()
            .enumerate()
            .map(|(index, (phase, count))| {
                Self::chip(
                    ("phase-chip", index),
                    phase.label(),
                    count,
                    self.chips.has_phase(phase),
                    move |this| this.chips.toggle_phase(phase),
                    cx,
                )
                .into_any_element()
            })
            .collect();
        let picked = counts
            .harnesses
            .keys()
            .any(|harness| self.chips.has_harness(harness));
        let harnesses: Vec<AnyElement> = if picked || counts.harnesses.len() > 1 {
            counts
                .harnesses
                .into_iter()
                .map(|(harness, count)| {
                    let on = self.chips.has_harness(&harness);
                    let label = harness.display_name().to_owned();
                    Self::chip(
                        SharedString::from(format!("harness-chip-{}", harness.as_str())),
                        &label,
                        count,
                        on,
                        move |this| this.chips.toggle_harness(harness.clone()),
                        cx,
                    )
                    .into_any_element()
                })
                .collect()
        } else {
            Vec::new()
        };
        div()
            .px_2()
            .pb_2()
            .flex()
            .flex_wrap()
            .gap_1()
            .children(phases)
            .children(harnesses)
            .into_any_element()
    }

    fn harness_badge(session: &SessionSummary) -> impl IntoElement {
        div()
            .flex_none()
            .px(px(6.))
            .rounded(px(6.))
            .bg(Theme::card())
            .text_size(px(10.))
            .text_color(Theme::ink_dim())
            .child(session.id.harness().display_name().to_owned())
    }

    pub(super) fn history(&self, history: &[SessionSummary], cx: &mut Context<Self>) -> AnyElement {
        let rows = lineage::nest(history, &self.collapsed);
        let hidden = rows.len().saturating_sub(self.history_shown);
        div()
            .flex()
            .flex_col()
            .children(
                rows.into_iter()
                    .take(self.history_shown)
                    .enumerate()
                    .map(|(index, row)| self.history_row(index, &row, cx)),
            )
            .when(hidden > 0, |list| {
                list.child(
                    div()
                        .id("history-more")
                        .px_2()
                        .py(px(7.))
                        .cursor_pointer()
                        .text_xs()
                        .text_color(Theme::ink_dim())
                        .hover(|more| more.text_color(Theme::ink()))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.history_shown += HISTORY_PAGE;
                            cx.notify();
                        }))
                        .child(format!("Show {} more of {hidden}", hidden.min(HISTORY_PAGE))),
                )
            })
            .into_any_element()
    }

    fn history_row(&self, index: usize, row: &lineage::Row, cx: &mut Context<Self>) -> AnyElement {
        let indent = px(16. * f32::from(u8::try_from(row.depth.min(4)).unwrap_or(4)));
        if let Some(renaming) = self
            .renaming
            .as_ref()
            .filter(|renaming| renaming.session.id == row.session.id)
        {
            return Self::rename_row(renaming, indent, cx);
        }
        let open = row.session.clone();
        let menu = row.session.clone();
        let rename = row.session.clone();
        let fold = row.session.id.clone();
        let lead = if row.children > 0 {
            div()
                .id(("fold", index))
                .cursor_pointer()
                .hover(|fold| fold.text_color(Theme::ink()))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if !this.collapsed.remove(&fold) {
                        this.collapsed.insert(fold.clone());
                    }
                    cx.stop_propagation();
                    cx.notify();
                }))
                .child(if row.collapsed { "▸" } else { "▾" })
                .into_any_element()
        } else {
            div()
                .child(if row.depth > 0 { "↳" } else { "" })
                .into_any_element()
        };
        div()
            .id(("history", index))
            .group(HISTORY_ROW)
            .pl(px(8.) + indent)
            .pr_2()
            .py(px(7.))
            .rounded(px(12.))
            .cursor_pointer()
            .hover(|line| line.bg(Theme::card()))
            .flex()
            .items_center()
            .gap(px(6.))
            .on_drag(DraggedSession(row.session.clone()), |dragged, _, _, cx| {
                cx.new(|_| DragGhost(crate::text::one_line(dragged.0.title.as_deref())))
            })
            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                cx.emit(PanelEvent::Open(open.clone()));
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |_, event: &MouseDownEvent, _, cx| {
                    cx.emit(PanelEvent::SessionMenu(menu.clone(), event.position));
                    cx.stop_propagation();
                }),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(12.))
                    .text_color(Theme::ink_dim())
                    .child(lead),
            )
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .text_color(Theme::ink())
                    .child(crate::text::one_line(row.session.title.as_deref())),
            )
            .child(
                div()
                    .id(("rename", index))
                    .flex_none()
                    .invisible()
                    .group_hover(HISTORY_ROW, gpui::StyleRefinement::visible)
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        cx.stop_propagation();
                        this.begin_rename(rename.clone(), window, cx);
                    }))
                    .child(crate::brand::icon("icons/rename.svg", 13., Theme::ink_dim())),
            )
            .child(Self::harness_badge(&row.session))
            .when(row.children > 0, |line| {
                line.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(Theme::ink_dim())
                        .child(row.children.to_string()),
                )
            })
            .into_any_element()
    }
}
