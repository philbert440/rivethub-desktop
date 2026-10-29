use gpui::{
    AnyView, App, Context, IntoElement, ParentElement, Render, SharedString, Styled, Window, div, prelude::*,
    px,
};

use crate::theme::Theme;

/// The short label shown while the pointer rests on a control.
pub struct Tip(SharedString);

impl Tip {
    pub fn text(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
        let text = text.into();
        move |_, cx| cx.new(|_| Self(text.clone())).into()
    }
}

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded(px(8.))
            .bg(Theme::raised())
            .font_family(Theme::SANS)
            .text_xs()
            .text_color(Theme::ink())
            .child(self.0.clone())
    }
}
