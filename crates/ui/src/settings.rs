use gpui::{Context, IntoElement, ParentElement, Render, Styled, Window, div};

use crate::shell;

/// The Settings page.
pub struct SettingsPage;

impl Render for SettingsPage {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(shell::header().child(shell::title("Settings")))
            .child(shell::tile())
    }
}
