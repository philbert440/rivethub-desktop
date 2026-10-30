use gpui::{
    AnyElement, ClickEvent, Context, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div,
    prelude::*, px,
};

use super::{PanelEvent, PastTopic, SidePanel};
use crate::theme::Theme;

impl SidePanel {
    pub(super) fn past_topics(past: &[&PastTopic], cx: &mut Context<Self>) -> AnyElement {
        let now = crate::text::now_ms();
        div()
            .flex()
            .flex_col()
            .pb_2()
            .children(past.iter().enumerate().map(|(index, past)| {
                let open = past.id.clone();
                let menu = past.id.clone();
                div()
                    .id(("past", index))
                    .px_2()
                    .py(px(7.))
                    .rounded(px(12.))
                    .cursor_pointer()
                    .hover(|line| line.bg(Theme::card()))
                    .flex()
                    .items_center()
                    .gap_2()
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                        cx.emit(PanelEvent::OpenTopic(open.clone()));
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |_, event: &MouseDownEvent, _, cx| {
                            cx.emit(PanelEvent::TopicMenu(menu.clone(), event.position));
                            cx.stop_propagation();
                        }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_color(Theme::ink())
                            .child(past.name.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .child(past.sessions.to_string()),
                    )
                    // A topic with no activity has no age to show, not one since 1970.
                    .when(past.active_ms > 0, |row| {
                        row.child(
                            div()
                                .text_xs()
                                .text_color(Theme::ink_dim())
                                .child(crate::text::ago(now, past.active_ms)),
                        )
                    })
            }))
            .into_any_element()
    }
}
