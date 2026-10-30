use gpui::{Context, IntoElement, ParentElement, Render, SharedString, Styled, Window, div, px};
use model::agents::Agent;
use model::session::SessionSummary;

use crate::theme::Theme;

#[derive(Debug, Clone)]
pub struct DraggedAgent(pub Agent);

#[derive(Debug, Clone)]
pub struct DraggedSession(pub SessionSummary);

pub struct DragGhost(pub SharedString);

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_2()
            .rounded(px(12.))
            .bg(Theme::raised())
            .border_1()
            .border_color(Theme::accent())
            .text_color(Theme::ink())
            .child(self.0.clone())
    }
}
