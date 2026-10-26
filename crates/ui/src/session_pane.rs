use std::sync::Arc;

use app::{Hub, SessionHandle};
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, Focusable, IntoElement, ListAlignment, ListState,
    ParentElement, Pixels, Render, SharedString, Styled, Subscription, Task, Window, div, list, prelude::*,
    px, relative,
};
use model::chat::exchanges;
use model::session::{SessionSummary, SessionView};
use term::GridSize;

use crate::input::{Submitted, TextInput};
use crate::terminal_view::TerminalView;
use crate::theme::Theme;
use crate::transcript::{self, ExchangeView, Fold, Folds};

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
    exchanges: Arc<Vec<ExchangeView>>,
    folds: Arc<Folds>,
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
            exchanges: Arc::new(Vec::new()),
            folds: Arc::new(Folds::default()),
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
        let folded = {
            let view = self.handle.view();
            let view = view.read();
            exchanges(view.turns(), view.live())
        };
        let previous = Arc::clone(&self.exchanges);
        let views: Vec<ExchangeView> = folded
            .into_iter()
            .enumerate()
            .map(|(index, exchange)| match previous.get(index) {
                Some(old) if old.exchange == exchange => old.clone(),
                _ => ExchangeView::new(exchange),
            })
            .collect();
        let count = views.len();
        let known = self.turns.item_count();
        let first_changed = views
            .iter()
            .zip(previous.iter())
            .position(|(new, old)| new != old)
            .unwrap_or(known.min(count));
        if count < known {
            self.turns.reset(count);
        } else {
            let stale = first_changed.min(known);
            self.turns.splice(stale..known, count - stale);
        }
        self.exchanges = Arc::new(views);
        cx.notify();
    }

    pub fn toggle_fold(&mut self, fold: Fold, cx: &mut Context<Self>) {
        Arc::make_mut(&mut self.folds).toggle(fold);
        let index = match fold {
            Fold::Exchange(index) | Fold::Group(index, _) | Fold::Thought(index, _) => index,
        };
        if index < self.turns.item_count() {
            self.turns.splice(index..index + 1, 1);
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

fn column_width(window: &Window) -> Pixels {
    let body = window.viewport_size().width - px(68.);
    (body * 0.64).clamp(px(680.), px(920.)).min(body - px(48.))
}

impl Render for SessionPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let column = column_width(window);
        let shared = self.handle.view();
        let view = shared.read();
        let error = view.error().map(str::to_owned);
        let behind = self.turns.is_scrolled_to_end() == Some(false);
        let footer = div()
            .w(column)
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
            _ => {
                let exchanges = Arc::clone(&self.exchanges);
                let folds = Arc::clone(&self.folds);
                let pane = cx.entity().downgrade();
                let transcript = list(
                    self.turns.clone(),
                    move |index, window: &mut Window, _: &mut App| {
                        exchanges.get(index).map_or_else(
                            || div().into_any_element(),
                            |exchange| {
                                let context = transcript::Context {
                                    index,
                                    folds: &folds,
                                    column,
                                    pane: pane.clone(),
                                };
                                transcript::exchange(exchange, &context, window)
                            },
                        )
                    },
                )
                .size_full();
                let back = behind.then(|| {
                    div().w(column).flex().justify_end().child(
                        div()
                            .id("back-to-bottom")
                            .size(px(34.))
                            .rounded(px(17.))
                            .bg(Theme::raised())
                            .text_color(Theme::ink())
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.turns.scroll_to_end();
                                cx.notify();
                            }))
                            .child("↓"),
                    )
                });
                div()
                    .flex_1()
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(div().flex_1().w_full().pt_2().child(transcript))
                    .children(back)
                    .child(
                        footer.child(
                            div()
                                .px_5()
                                .py_3()
                                .rounded(px(28.))
                                .bg(Theme::card())
                                .child(self.composer.clone()),
                        ),
                    )
            }
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
