use gpui::{
    AnyElement, App, ClickEvent, Context, IntoElement, ParentElement, Styled, Window, anchored, deferred,
    div, prelude::*, px,
};
use model::plugin::view::{Action, ButtonStyle};

use super::{Frame, FrameEvent, Placement, Shown};
use crate::theme::Theme;
use crate::{brand, shell};

impl Frame {
    fn back(cx: &mut Context<Self>) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(FrameEvent::Run(vec![Action::Back])))
    }

    fn icon_button(id: &'static str, icon: &'static str) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .size(px(36.))
            .flex_none()
            .rounded(px(18.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|button| button.bg(Theme::raised()))
            .child(brand::icon(brand::symbol(Some(icon)), 18., Theme::ink()))
    }

    pub(super) fn body(&self, window: &Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let level = u8::from(self.placement == Placement::Dialog);
        let mut body: Vec<AnyElement> = self
            .problem
            .iter()
            .map(|problem| {
                div()
                    .text_xs()
                    .text_color(Theme::danger())
                    .child(problem.clone())
                    .into_any_element()
            })
            .collect();
        match &self.shown {
            Shown::Waiting => body.push(
                div()
                    .text_sm()
                    .text_color(Theme::ink_dim())
                    .child("Loading…")
                    .into_any_element(),
            ),
            Shown::Failed(refusal) => body.push(
                div()
                    .p_4()
                    .rounded(px(16.))
                    .bg(Theme::danger_container())
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_color(Theme::danger()).child(refusal.said.clone()))
                    .children(refusal.detail.clone().map(|detail| {
                        div()
                            .font_family(Theme::MONO)
                            .text_xs()
                            .text_color(Theme::ink())
                            .child(detail)
                    }))
                    .child(
                        div().flex().child(
                            div()
                                .id("frame-retry")
                                .h(px(30.))
                                .px_4()
                                .rounded(px(15.))
                                .border_1()
                                .border_color(Theme::line())
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .text_sm()
                                .hover(|button| button.bg(Theme::raised()))
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.load(cx)))
                                .child("Try again"),
                        ),
                    )
                    .into_any_element(),
            ),
            Shown::Ready(view) => body.extend(self.blocks(&view.blocks, "", level, window, cx)),
        }
        body
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        match &self.shown {
            Shown::Ready(view) => view
                .toolbar
                .iter()
                .enumerate()
                .map(|(index, button)| self.button(button, format!("toolbar-{index}").into(), None, cx))
                .collect(),
            Shown::Waiting | Shown::Failed(_) => Vec::new(),
        }
    }

    fn heading(&self) -> (String, Option<String>) {
        match &self.shown {
            Shown::Ready(view) => (view.title.clone(), view.subtitle.clone()),
            Shown::Waiting | Shown::Failed(_) => (self.named.clone(), None),
        }
    }

    pub(super) fn page(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let (title, subtitle) = self.heading();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                shell::header()
                    .when(self.depth > 0, |header| {
                        header.child(Self::icon_button("frame-back", "arrow_back").on_click(Self::back(cx)))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(shell::title(title).truncate())
                            .children(subtitle.map(|subtitle| {
                                div()
                                    .text_xs()
                                    .text_color(Theme::ink_dim())
                                    .truncate()
                                    .child(subtitle)
                            })),
                    )
                    .child(
                        div()
                            .flex_none()
                            .px_2()
                            .rounded(px(8.))
                            .bg(Theme::card())
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .child(self.plugin.node.clone()),
                    )
                    .child(div().flex_1())
                    .when(self.loading && matches!(self.shown, Shown::Ready(_)), |header| {
                        header.child(div().text_xs().text_color(Theme::ink_dim()).child("Updating…"))
                    })
                    .children(self.toolbar(cx)),
            )
            .child(
                shell::tile().child(
                    div()
                        .id("plugin-page")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .child(
                            div()
                                .p_6()
                                .flex()
                                .flex_col()
                                .gap_4()
                                .children(self.body(window, cx)),
                        ),
                ),
            )
            .into_any_element()
    }

    pub(super) fn dialog(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let (title, subtitle) = self.heading();
        div()
            .w(px(560.))
            .max_h(window.viewport_size().height * 0.85)
            .rounded(px(28.))
            .bg(Theme::card())
            .border_1()
            .border_color(Theme::line())
            .flex()
            .flex_col()
            .occlude()
            .child(
                div()
                    .px_6()
                    .pt_5()
                    .pb_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(shell::title(title))
                            .children(subtitle.map(|subtitle| {
                                div().text_xs().text_color(Theme::ink_dim()).child(subtitle)
                            })),
                    )
                    .children(self.toolbar(cx))
                    .child(Self::icon_button("dialog-close", "close").on_click(Self::back(cx))),
            )
            .child(
                div()
                    .id("plugin-dialog")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .px_6()
                    .pb_6()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .children(self.body(window, cx)),
            )
            .into_any_element()
    }

    pub(super) fn menu_overlay(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let (at, buttons) = self.menu.as_ref()?;
        Some(deferred(
            anchored().position(*at).child(
                div()
                    .min_w(px(200.))
                    .p_1()
                    .rounded(px(16.))
                    .bg(Theme::card())
                    .border_1()
                    .border_color(Theme::line())
                    .text_color(Theme::ink())
                    .occlude()
                    .on_mouse_down_out(cx.listener(|this, _: &gpui::MouseDownEvent, _, cx| {
                        this.menu = None;
                        cx.notify();
                    }))
                    .children(buttons.iter().enumerate().map(|(index, button)| {
                        let pressed = button.clone();
                        let ink = if button.style == ButtonStyle::Danger {
                            Theme::danger()
                        } else {
                            Theme::ink()
                        };
                        div()
                            .id(("row-menu", index))
                            .px_3()
                            .py_2()
                            .rounded(px(10.))
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_color(ink)
                            .map(|item| {
                                if button.disabled {
                                    item.opacity(0.5)
                                } else {
                                    item.cursor_pointer().hover(|item| item.bg(Theme::selected()))
                                }
                            })
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.press(&pressed, None, cx);
                            }))
                            .children(
                                button
                                    .icon
                                    .as_deref()
                                    .map(|icon| brand::icon(brand::symbol(Some(icon)), 16., ink)),
                            )
                            .child(button.label.clone())
                    })),
            ),
        ))
    }
}
