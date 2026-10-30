use gpui::{
    AnyElement, ClickEvent, Context, Focusable, IntoElement, ParentElement, Styled, Window, div, prelude::*,
    px,
};
use term::GridSize;

use super::{Mode, PaneEvent, SessionPane, ToggleTerminal};
use crate::terminal_view::{Bell, TerminalView};
use crate::theme::Theme;

const FIRST_TERMINAL: GridSize = GridSize {
    columns: 120,
    rows: 40,
};

impl SessionPane {
    /// Opens on the session's terminal: attached when one is running, else offered.
    pub fn show_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show(Mode::Terminal, window, cx);
    }

    pub(super) fn show(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode;
        match (mode, &self.terminal) {
            (Mode::Chat, _) => window.focus(&self.composer.focus_handle(cx), cx),
            (Mode::Terminal, Some(terminal)) => window.focus(&terminal.focus_handle(cx), cx),
            (Mode::Terminal, None) => self.check_terminal(window, cx),
        }
        cx.notify();
    }

    fn check_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        let summary = self.summary.clone();
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let running = hub.has_terminal(&summary).await.unwrap_or(false);
            cx.update_window(window_handle, |_, window, cx| {
                this.update(cx, |this, cx| {
                    if this.mode != Mode::Terminal {
                        return;
                    }
                    if running {
                        this.attach_terminal(window, cx);
                    } else {
                        this.confirm_spawn = true;
                        cx.notify();
                    }
                })
                .ok();
            })
            .ok();
        })
        .detach();
    }

    fn attach_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_spawn = false;
        let terminal = self.terminal.get_or_insert_with(|| {
            let handle = self.hub.open_terminal(&self.summary, FIRST_TERMINAL);
            let terminal = cx.new(|cx| TerminalView::new(handle, window, cx));
            cx.subscribe(&terminal, |_, _, Bell, cx| cx.emit(PaneEvent::Bell))
                .detach();
            terminal
        });
        window.focus(&terminal.focus_handle(cx), cx);
        cx.notify();
    }

    pub(super) fn spawn_prompt(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .when(self.confirm_spawn, |prompt| {
                prompt
                    .child(
                        div()
                            .text_color(Theme::ink())
                            .child("No terminal is running for this session."),
                    )
                    .child(div().max_w(px(520.)).text_color(Theme::ink_dim()).child(
                        "Opening one resumes the session in a terminal on its node. rh runs one copy: a chat \
                         it's running hands over when its turn ends. A copy running outside rh, such as in a \
                         terminal of your own, would write to the same transcript.",
                    ))
                    .child(
                        div()
                            .id("open-terminal")
                            .h(px(34.))
                            .px_4()
                            .rounded(px(17.))
                            .bg(Theme::accent())
                            .text_color(Theme::on_accent())
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.attach_terminal(window, cx);
                            }))
                            .child("Open terminal"),
                    )
            })
            .into_any_element()
    }

    pub(super) fn end_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(terminal) = self.terminal.take() {
            terminal.read(cx).end();
        }
        self.show(Mode::Chat, window, cx);
    }

    pub(super) fn toggle(&mut self, _: &ToggleTerminal, window: &mut Window, cx: &mut Context<Self>) {
        let next = if self.mode == Mode::Chat {
            Mode::Terminal
        } else {
            Mode::Chat
        };
        self.show(next, window, cx);
    }
}
