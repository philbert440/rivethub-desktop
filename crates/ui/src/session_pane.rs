use std::sync::Arc;

use app::{Hub, SessionHandle};
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, Focusable, IntoElement, ListAlignment, ListState,
    ParentElement, Render, SharedString, Styled, Subscription, Task, Window, div, list, prelude::*, px,
    relative,
};
use model::session::{Role, SessionSummary, SessionView, ToolState, Turn};
use parking_lot::RwLock;
use term::GridSize;

use crate::input::{Submitted, TextInput};
use crate::terminal_view::TerminalView;
use crate::theme::Theme;

gpui::actions!(session, [ToggleTerminal]);

const FIRST_TERMINAL: GridSize = GridSize {
    columns: 120,
    rows: 40,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Chat,
    Terminal,
}

pub struct SessionPane {
    hub: Hub,
    summary: SessionSummary,
    mode: Mode,
    terminal: Option<Entity<TerminalView>>,
    confirm_spawn: bool,
    handle: SessionHandle,
    turns: ListState,
    composer: Entity<TextInput>,
    _revisions: Task<()>,
    _submitted: Subscription,
}

impl SessionPane {
    pub fn new(hub: Hub, summary: SessionSummary, cx: &mut Context<Self>) -> Self {
        let handle = hub.open_session(summary.id.clone());
        let composer = cx.new(|cx| TextInput::new("Message claude…", cx));
        let submitted = cx.subscribe(&composer, |this, _, Submitted(text), _| {
            this.handle.send(text.clone());
        });
        let mut revisions = handle.revisions();
        let watcher = cx.spawn(async move |this, cx| {
            while revisions.changed().await.is_ok() {
                if this.update(cx, SessionPane::refresh).is_err() {
                    break;
                }
            }
        });
        Self {
            hub,
            summary,
            mode: Mode::Chat,
            terminal: None,
            confirm_spawn: false,
            handle,
            turns: ListState::new(0, ListAlignment::Bottom, px(600.)),
            composer,
            _revisions: watcher,
            _submitted: submitted,
        }
    }

    pub fn composer(&self) -> &Entity<TextInput> {
        &self.composer
    }

    fn show(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
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
        let id = self.summary.id.clone();
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let running = hub.has_terminal(id).await.unwrap_or(false);
            cx.update_window(window_handle, |_, window, cx| {
                this.update(cx, |this, cx| {
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
            let handle = self.hub.open_terminal(self.summary.id.clone(), FIRST_TERMINAL);
            cx.new(|cx| TerminalView::new(handle, cx))
        });
        window.focus(&terminal.focus_handle(cx), cx);
        cx.notify();
    }

    fn spawn_prompt(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .when(self.confirm_spawn, |prompt| {
                prompt
                    .child(div().text_color(Theme::ink()).child("No terminal is running for this session."))
                    .child(div().max_w(px(520.)).text_color(Theme::ink_dim()).child(
                        "Opening one starts the harness for this session in a terminal. If the session is \
                         running anywhere else, including this chat, both copies write to the same transcript.",
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
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.attach_terminal(window, cx)))
                            .child("Open terminal"),
                    )
            })
            .into_any_element()
    }

    fn toggle(&mut self, _: &ToggleTerminal, window: &mut Window, cx: &mut Context<Self>) {
        let next = if self.mode == Mode::Chat {
            Mode::Terminal
        } else {
            Mode::Chat
        };
        self.show(next, window, cx);
    }

    fn tab(&self, mode: Mode, label: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.mode == mode;
        div()
            .id(label)
            .h(px(30.))
            .px_4()
            .rounded(px(15.))
            .flex()
            .items_center()
            .cursor_pointer()
            .when(active, |tab| {
                tab.bg(Theme::selected()).text_color(Theme::on_selected())
            })
            .when(!active, |tab| tab.text_color(Theme::ink_dim()))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.show(mode, window, cx)))
            .child(label)
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let title: SharedString = crate::text::one_line(self.summary.title.as_deref());
        let meta = [self.summary.model.as_deref(), self.summary.cwd.as_deref()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
        div()
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_6()
            .pt_4()
            .pb_2()
            .child(div().text_color(Theme::ink()).truncate().child(title))
            .child(
                div()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .truncate()
                    .child(meta),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex_none()
                    .flex()
                    .p(px(3.))
                    .rounded(px(18.))
                    .bg(Theme::card())
                    .child(self.tab(Mode::Chat, "Chat", cx))
                    .child(self.tab(Mode::Terminal, "Terminal", cx)),
            )
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let count = {
            let view = self.handle.view();
            let view = view.read();
            view.turns().len() + usize::from(view.live().is_some())
        };
        let known = self.turns.item_count();
        if count < known {
            self.turns.reset(count);
        } else if count > known {
            self.turns.splice(known..known, count - known);
        } else if count > 0 {
            self.turns.splice(count - 1..count, 1);
        }
        cx.notify();
    }

    fn context_bar(view: &SessionView) -> Option<impl IntoElement> {
        let usage = view.context()?;
        let fraction = usage.fraction();
        let fill = if fraction >= 0.8 {
            Theme::warn()
        } else {
            Theme::accent()
        };
        Some(
            div()
                .flex()
                .items_center()
                .gap_3()
                .text_xs()
                .text_color(Theme::ink_dim())
                .child("Context")
                .child(
                    div()
                        .flex_1()
                        .h(px(6.))
                        .rounded(px(3.))
                        .bg(Theme::card())
                        .child(div().h_full().w(relative(fraction)).rounded(px(3.)).bg(fill)),
                )
                .child(format!("{}k / {}k", usage.used / 1000, usage.window / 1000)),
        )
    }

    fn needs_you(view: &SessionView) -> Option<impl IntoElement> {
        let approval = view.approvals().first()?;
        let reason = approval.reason.clone().unwrap_or_default();
        Some(
            div()
                .p_4()
                .rounded(px(18.))
                .bg(Theme::warn_container())
                .border_1()
                .border_color(Theme::warn_line())
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_color(Theme::warn())
                        .child(format!("Approval: {}", approval.tool)),
                )
                .child(div().text_color(Theme::ink()).child(reason)),
        )
    }
}

fn render_turn(view: &Arc<RwLock<SessionView>>, index: usize) -> AnyElement {
    let view = view.read();
    let turn = view
        .turns()
        .get(index)
        .or_else(|| view.live().filter(|_| index == view.turns().len()));
    let Some(turn) = turn else {
        return div().into_any_element();
    };
    turn_element(turn).into_any_element()
}

fn turn_element(turn: &Turn) -> impl IntoElement {
    let row = div().w_full().flex().py_2();
    match turn.role {
        Role::User => row.justify_end().child(
            div()
                .max_w(px(600.))
                .px_4()
                .py_3()
                .rounded(px(18.))
                .bg(Theme::raised())
                .child(turn.text.clone()),
        ),
        Role::Assistant => row.child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .max_w(px(820.))
                .children(turn.tools.iter().map(|tool| {
                    let (label, color) = match tool.state {
                        ToolState::Running => ("running", Theme::ink_dim()),
                        ToolState::Done => ("done", Theme::accent()),
                        ToolState::Failed => ("failed", Theme::warn()),
                    };
                    div()
                        .flex()
                        .gap_3()
                        .px_3()
                        .py_2()
                        .rounded(px(14.))
                        .bg(Theme::card())
                        .child(tool.name.clone())
                        .child(div().text_color(color).child(label))
                }))
                .child(div().text_color(Theme::ink()).child(turn.text.clone())),
        ),
    }
}

impl Render for SessionPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shared = self.handle.view();
        let view = shared.read();
        let error = view.error().map(str::to_owned);
        let footer = div()
            .w(px(820.))
            .pb_5()
            .pt_2()
            .flex()
            .flex_col()
            .gap_3()
            .children(Self::needs_you(&view))
            .children(error.map(|message| div().text_color(Theme::warn()).child(message)))
            .children(Self::context_bar(&view));
        let prompt = self.spawn_prompt(cx);
        let body = match (self.mode, &self.terminal) {
            (Mode::Terminal, None) => div().flex_1().w_full().child(prompt),
            (Mode::Terminal, Some(terminal)) => div()
                .flex_1()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .px_4()
                .child(div().flex_1().w_full().child(terminal.clone()))
                .child(footer),
            _ => div()
                .flex_1()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .child(
                    div().flex_1().w(px(820.)).pt_2().child(
                        list(self.turns.clone(), {
                            let shared = Arc::clone(&shared);
                            move |index, _: &mut Window, _: &mut App| render_turn(&shared, index)
                        })
                        .size_full(),
                    ),
                )
                .child(
                    footer.child(
                        div()
                            .px_5()
                            .py_3()
                            .rounded(px(24.))
                            .bg(Theme::card())
                            .child(self.composer.clone()),
                    ),
                ),
        };
        div()
            .key_context("Session")
            .on_action(cx.listener(Self::toggle))
            .size_full()
            .flex()
            .flex_col()
            .child(self.header(cx))
            .child(body)
    }
}
