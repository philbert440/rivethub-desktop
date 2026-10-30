use gpui::{
    AnyElement, ClickEvent, Context, Entity, Focusable, IntoElement, ParentElement, Styled, Window, div,
    prelude::*, px,
};
use model::session::Driver;
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

    /// Opens on the session's terminal and starts it without asking first.
    pub fn start_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = Mode::Terminal;
        self.attach_terminal(window, cx);
    }

    pub(super) fn show(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode;
        match (mode, self.live_terminal(cx)) {
            (Mode::Chat, _) => window.focus(&self.composer.focus_handle(cx), cx),
            (Mode::Terminal, Some(terminal)) => window.focus(&terminal.focus_handle(cx), cx),
            (Mode::Terminal, None) => self.check_terminal(window, cx),
        }
        cx.notify();
    }

    /// The terminal this pane shows, unless its process has exited, as when Chat took the session.
    pub(super) fn live_terminal(&self, cx: &Context<Self>) -> Option<Entity<TerminalView>> {
        self.terminal
            .clone()
            .filter(|terminal| !terminal.read(cx).exited())
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
        if self.live_terminal(cx).is_none() {
            self.terminal = None;
        }
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

    /// What the terminal page says while no terminal runs: why, and that opening one carries on.
    fn idle_terminal(&self) -> (&'static str, &'static str) {
        let shared = self.handle.view();
        let chat = shared.read().driving().driver == Some(Driver::Chat);
        let exited = self.terminal.is_some();
        match (chat, exited) {
            (true, _) => (
                "Chat has this session now.",
                "Opening the terminal resumes the session here. If Chat is mid-turn, the terminal starts \
                 when that turn ends, and Chat keeps showing what happens in it.",
            ),
            (false, true) => (
                "The terminal exited.",
                "Opening it again resumes the session in a terminal on its node.",
            ),
            (false, false) => (
                "No terminal is running for this session.",
                "Opening one resumes the session in a terminal on its node. rh runs one copy: a chat \
                 it's running hands over when its turn ends. A copy running outside rh, such as in a \
                 terminal of your own, would write to the same transcript.",
            ),
        }
    }

    pub(super) fn spawn_prompt(&self, cx: &mut Context<Self>) -> AnyElement {
        let (said, why) = self.idle_terminal();
        let offered = self.confirm_spawn || self.terminal.is_some();
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .when(offered, |prompt| {
                prompt
                    .child(div().text_color(Theme::ink()).child(said))
                    .child(div().max_w(px(520.)).text_color(Theme::ink_dim()).child(why))
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
