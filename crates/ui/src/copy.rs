use std::time::Duration;

use gpui::{
    App, ClipboardItem, Entity, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div,
    prelude::*, px,
};

use crate::brand;
use crate::theme::Theme;

/// How long a copy reads as done after a click.
const COPIED_FOR: Duration = Duration::from_millis(1500);

/// Whether something was just copied, kept between frames under the element's id.
#[derive(Debug, Default)]
pub struct Copied(bool);

impl Copied {
    pub fn just(&self) -> bool {
        self.0
    }
}

/// The copied state of the element `id` renders.
pub fn state(id: impl Into<SharedString>, window: &mut Window, cx: &mut App) -> Entity<Copied> {
    window.use_keyed_state(gpui::ElementId::Name(id.into()), cx, |_, _| Copied::default())
}

/// Puts `text` on the clipboard and marks `state` copied for a moment.
pub fn copy(state: &Entity<Copied>, text: &str, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
    state.update(cx, |copied, cx| {
        copied.0 = true;
        cx.notify();
        cx.spawn(async move |copied, cx| {
            cx.background_executor().timer(COPIED_FOR).await;
            copied
                .update(cx, |copied, cx| {
                    copied.0 = false;
                    cx.notify();
                })
                .ok();
        })
        .detach();
    });
}

/// A small copy icon that turns into a check for a moment after a click.
#[derive(IntoElement)]
pub struct CopyIcon {
    id: SharedString,
    text: String,
}

impl CopyIcon {
    pub fn new(id: impl Into<SharedString>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
        }
    }
}

impl RenderOnce for CopyIcon {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let copied = state(self.id.clone(), window, cx);
        let done = copied.read(cx).just();
        let text = self.text;
        div()
            .id(self.id)
            .flex_none()
            .size(px(24.))
            .rounded(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|icon| icon.bg(Theme::raised()))
            .on_click(move |_, _, cx| copy(&copied, &text, cx))
            .child(if done {
                brand::icon("icons/check.svg", 14., Theme::accent())
            } else {
                brand::icon("icons/copy.svg", 14., Theme::ink_dim())
            })
    }
}
