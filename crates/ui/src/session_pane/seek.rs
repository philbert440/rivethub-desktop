use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, div, px};
use model::chat::holding;

use super::SessionPane;
use crate::theme::Theme;

impl SessionPane {
    /// Opens the transcript at the exchange holding a search hit's message, sent or said at
    /// `at_ms`, and marks it. An older hit reads earlier pages back until one holds it.
    pub fn seek(&mut self, at_ms: i64, cx: &mut Context<Self>) {
        self.seeking = Some(at_ms);
        self.found = None;
        self.seek_step(cx);
    }

    /// One step of a seek, as the transcript arrives: show the exchange once one holds the hit,
    /// else read the earlier page when the hit is older than the held turns, else stop.
    pub(super) fn seek_step(&mut self, cx: &mut Context<Self>) {
        let Some(at_ms) = self.seeking else {
            return;
        };
        if self.exchanges.is_empty() {
            return;
        }
        if let Some(index) = holding(self.exchanges.iter().map(|view| &view.exchange), at_ms) {
            self.seeking = None;
            self.found = Some(index);
            self.turns.scroll_to(gpui::ListOffset {
                item_ix: index,
                offset_in_item: px(0.),
            });
            cx.notify();
            return;
        }
        let (earlier, loading) = {
            let view = self.handle.view();
            let view = view.read();
            (view.earlier().is_some(), view.loading_earlier())
        };
        if loading {
            return;
        }
        if earlier {
            self.read_earlier(cx);
        } else {
            self.seeking = None;
        }
    }
}

/// The exchange a search hit opened at, ringed so the eye finds it.
pub(super) fn marked(shown: AnyElement) -> AnyElement {
    div()
        .rounded(px(18.))
        .border_2()
        .border_color(Theme::accent())
        .child(shown)
        .into_any_element()
}
