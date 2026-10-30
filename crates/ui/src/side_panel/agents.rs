use gpui::{
    AnyElement, ClickEvent, Context, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div,
    prelude::*, px, rgb,
};
use model::agents::Agent;

use super::drag::{DragGhost, DraggedAgent};
use super::{PanelEvent, SidePanel};
use crate::theme::Theme;

impl SidePanel {
    pub(super) fn add_agent(cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("add-agent")
            .size(px(28.))
            .rounded(px(14.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|button| button.bg(Theme::card()))
            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                cx.emit(PanelEvent::NewAgent);
            }))
            .child(crate::brand::icon("icons/plus.svg", 16., Theme::ink_dim()))
            .into_any_element()
    }

    pub(super) fn agents(agents: &[&Agent], cx: &mut Context<Self>) -> AnyElement {
        div()
            .pb_2()
            .flex()
            .flex_col()
            .children(agents.iter().enumerate().map(|(index, agent)| {
                let dot = agent.color.map_or(Theme::ink_dim(), rgb);
                let edit = (*agent).clone();
                let row = div()
                    .id(("agent", index))
                    .px_2()
                    .py(px(7.))
                    .rounded(px(12.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .hover(|row| row.bg(Theme::card()))
                    .child(div().flex_none().size(px(8.)).rounded(px(4.)).bg(dot))
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_color(Theme::ink())
                            .child(agent.name.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .child(agent.node.clone().unwrap_or_default()),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |_, _: &MouseDownEvent, _, cx| {
                            cx.emit(PanelEvent::EditAgent(edit.clone()));
                        }),
                    );
                if agent.launchable() {
                    let start = (*agent).clone();
                    row.cursor_grab()
                        .on_drag(DraggedAgent((*agent).clone()), |dragged, _, _, cx| {
                            cx.new(|_| DragGhost(format!("Start a session with {}", dragged.0.name).into()))
                        })
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                            cx.emit(PanelEvent::Start(start.clone()));
                        }))
                        .into_any_element()
                } else {
                    row.opacity(0.55).into_any_element()
                }
            }))
            .into_any_element()
    }
}
