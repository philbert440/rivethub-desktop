use gpui::{
    AnyElement, App, ClickEvent, Context, IntoElement, ParentElement, Styled, WeakEntity, div, prelude::*, px,
};

use super::SessionPane;
use crate::theme::Theme;

/// How close to the top of a long transcript a scroll comes before its earlier page is read back,
/// so it is there by the time the reader gets to it.
const NEAR_TOP: usize = 3;

impl SessionPane {
    /// Reads back the earlier page of a long transcript once a scroll nears the top of the turns
    /// held. A scroll handler runs while the list is borrowed, so it only asks the session.
    pub(super) fn watch_top(&self, cx: &Context<Self>) {
        let pane = cx.weak_entity();
        self.turns.set_scroll_handler(move |event, _, cx| {
            if event.visible_range.start < NEAR_TOP {
                pane.update(cx, SessionPane::read_earlier).ok();
            }
        });
    }

    pub(super) fn read_earlier(&mut self, cx: &mut Context<Self>) {
        let wanted = {
            let view = self.handle.view();
            let view = view.read();
            view.earlier().is_some() && !view.loading_earlier()
        };
        if wanted {
            self.handle.read_earlier();
            cx.notify();
        }
    }
}

/// The exchange that opens the turns held, under the line saying earlier ones are still to read
/// back when they are.
pub(super) fn opening(
    shown: AnyElement,
    earlier: Option<bool>,
    pane: &WeakEntity<SessionPane>,
) -> AnyElement {
    match earlier {
        Some(loading) => div()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .child(marker(loading, pane.clone()))
            .child(shown)
            .into_any_element(),
        None => shown,
    }
}

fn marker(loading: bool, pane: WeakEntity<SessionPane>) -> AnyElement {
    let row = div()
        .id("earlier-messages")
        .h(px(32.))
        .px_3()
        .flex()
        .items_center()
        .gap_2()
        .text_xs()
        .text_color(Theme::ink_dim());
    if loading {
        return row.child("Loading earlier messages…").into_any_element();
    }
    row.cursor_pointer()
        .hover(|row| row.text_color(Theme::ink()))
        .on_click(move |_: &ClickEvent, _, cx: &mut App| {
            pane.update(cx, SessionPane::read_earlier).ok();
        })
        .child("↑")
        .child("Earlier messages")
        .into_any_element()
}
