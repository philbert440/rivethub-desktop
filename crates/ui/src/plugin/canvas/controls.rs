use gpui::{
    ClickEvent, Context, IntoElement, MouseDownEvent, ParentElement, Styled, anchored, deferred, div,
    prelude::*, px,
};

use super::{CanvasView, ZOOM_STEP};
use crate::brand;
use crate::theme::Theme;

impl CanvasView {
    fn control(id: &'static str, icon: &'static str) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .size(px(28.))
            .rounded(px(14.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|button| button.bg(Theme::raised()))
            .child(brand::icon(brand::symbol(Some(icon)), 16., Theme::ink_dim()))
    }

    pub(super) fn controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let zoom = self.zoom();
        div()
            .absolute()
            .right(px(12.))
            .bottom(px(12.))
            .p(px(2.))
            .rounded(px(16.))
            .bg(Theme::card())
            .border_1()
            .border_color(Theme::line())
            .flex()
            .items_center()
            .occlude()
            .child(
                Self::control("canvas-zoom-out", "remove")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.zoom_by(1. / ZOOM_STEP, cx))),
            )
            .child(
                div()
                    .w(px(44.))
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .flex()
                    .justify_center()
                    .child(format!("{:.0}%", zoom * 100.)),
            )
            .child(
                Self::control("canvas-zoom-in", "add")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.zoom_by(ZOOM_STEP, cx))),
            )
            .child(Self::control("canvas-fit", "fit_screen").on_click(cx.listener(
                |this, _: &ClickEvent, _, cx| {
                    let size = this.size();
                    this.board.fit(size);
                    cx.notify();
                },
            )))
    }

    pub(super) fn tooltip(&self) -> Option<impl IntoElement> {
        if !self.tooltip || self.gesture.is_some() || self.open_menu.is_some() {
            return None;
        }
        let item = self.board.scene().item(self.hover.as_ref()?.item())?;
        let text = item.tooltip()?.to_owned();
        let at = self.local(self.pointer?);
        Some(
            div()
                .absolute()
                .left(px(at.x + 14.))
                .top(px(at.y + 18.))
                .max_w(px(280.))
                .px_3()
                .py_1()
                .rounded(px(10.))
                .bg(Theme::raised())
                .border_1()
                .border_color(Theme::line())
                .text_xs()
                .text_color(Theme::ink())
                .child(text),
        )
    }

    pub(super) fn menu_overlay(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let menu = self.open_menu.as_ref()?;
        Some(deferred(
            anchored().position(menu.at).child(
                div()
                    .min_w(px(200.))
                    .p_1()
                    .rounded(px(16.))
                    .bg(Theme::card())
                    .border_1()
                    .border_color(Theme::line())
                    .text_color(Theme::ink())
                    .occlude()
                    .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.open_menu = None;
                        cx.notify();
                    }))
                    .children(menu.entries.iter().enumerate().map(|(index, entry)| {
                        let choice = entry.id.clone();
                        let ink = if entry.danger {
                            Theme::danger()
                        } else {
                            Theme::ink()
                        };
                        div()
                            .id(("canvas-menu", index))
                            .px_3()
                            .py_2()
                            .rounded(px(10.))
                            .flex()
                            .items_center()
                            .gap_2()
                            .cursor_pointer()
                            .text_color(ink)
                            .hover(|item| item.bg(Theme::selected()))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.choose(choice.clone(), cx);
                            }))
                            .children(
                                entry
                                    .icon
                                    .as_deref()
                                    .map(|icon| brand::icon(brand::symbol(Some(icon)), 16., ink)),
                            )
                            .child(entry.label.clone())
                    })),
            ),
        ))
    }
}
