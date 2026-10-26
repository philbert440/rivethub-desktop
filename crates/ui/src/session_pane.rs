use std::sync::Arc;

use app::{Hub, SessionHandle};
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, Focusable, IntoElement, ListAlignment, ListState,
    ParentElement, Pixels, Render, Styled, Subscription, Task, Window, div, list, prelude::*, px, relative,
};
use model::chat::exchanges;
use model::session::{Decision, SessionSummary, SessionView};
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

#[derive(Debug, Clone)]
pub enum PaneEvent {
    Renamed(SessionSummary),
}

struct Renaming {
    input: Entity<TextInput>,
    problem: Option<String>,
    _submitted: Subscription,
}

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
    hovered_turn: Option<usize>,
    composer: Entity<TextInput>,
    renaming: Option<Renaming>,
    _revisions: Task<()>,
    _submitted: Subscription,
    _typed: Subscription,
}

impl SessionPane {
    pub fn new(hub: Hub, summary: SessionSummary, cx: &mut Context<Self>) -> Self {
        let handle = hub.open_session(&summary);
        let composer = cx.new(|cx| TextInput::new("Message claude…", cx));
        let submitted = cx.subscribe(&composer, |this, _, Submitted(text), _| {
            this.handle.send(text.clone());
        });
        let typed = cx.observe(&composer, |_, _, cx| cx.notify());
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
            hovered_turn: None,
            composer,
            renaming: None,
            _revisions: watcher,
            _submitted: submitted,
            _typed: typed,
        }
    }

    fn begin_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.summary.title.clone().unwrap_or_default();
        let input = cx.new(|cx| {
            let mut input = TextInput::new("Session title", cx);
            input.set_text(current.trim(), cx);
            input
        });
        let submitted = cx.subscribe(&input, |this, _, Submitted(title), cx| {
            this.commit_rename(title.clone(), cx);
        });
        window.focus(&input.focus_handle(cx), cx);
        self.renaming = Some(Renaming {
            input,
            problem: None,
            _submitted: submitted,
        });
        cx.notify();
    }

    fn commit_rename(&mut self, title: String, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        let summary = self.summary.clone();
        cx.spawn(async move |this, cx| {
            let renamed = hub.rename(&summary, title).await;
            this.update(cx, |this, cx| {
                match renamed {
                    Ok(renamed) => {
                        this.summary.title = renamed.title;
                        this.renaming = None;
                        cx.emit(PaneEvent::Renamed(this.summary.clone()));
                    }
                    Err(err) => {
                        if let Some(renaming) = this.renaming.as_mut() {
                            renaming.problem = Some(format!("rename failed: {err}"));
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn title(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(renaming) = &self.renaming else {
            return div()
                .id("session-title")
                .flex_shrink(1.)
                .min_w_0()
                .truncate()
                .text_color(Theme::ink())
                .cursor_text()
                .hover(|title| title.text_color(Theme::accent()))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.begin_rename(window, cx)))
                .child(crate::text::one_line(self.summary.title.as_deref()))
                .into_any_element();
        };
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .w(px(360.))
                    .px_3()
                    .py_1()
                    .rounded(px(10.))
                    .bg(Theme::card())
                    .border_1()
                    .border_color(Theme::accent())
                    .child(renaming.input.clone()),
            )
            .child(
                div()
                    .id("cancel-rename")
                    .cursor_pointer()
                    .text_color(Theme::ink_dim())
                    .hover(|cancel| cancel.text_color(Theme::ink()))
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.renaming = None;
                        window.focus(&this.composer.focus_handle(cx), cx);
                        cx.notify();
                    }))
                    .child("×"),
            )
            .children(
                renaming
                    .problem
                    .clone()
                    .map(|problem| div().text_xs().text_color(Theme::warn()).child(problem)),
            )
            .into_any_element()
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
        let summary = self.summary.clone();
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let running = hub.has_terminal(&summary).await.unwrap_or(false);
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
            let handle = self.hub.open_terminal(&self.summary, FIRST_TERMINAL);
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

    fn end_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(terminal) = self.terminal.take() {
            terminal.read(cx).end();
        }
        self.show(Mode::Chat, window, cx);
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
            .child(self.title(cx))
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
            .when(self.mode == Mode::Terminal && self.terminal.is_some(), |header| {
                header.child(
                    div()
                        .id("end-terminal")
                        .h(px(30.))
                        .px_3()
                        .rounded(px(15.))
                        .border_1()
                        .border_color(Theme::line())
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .text_color(Theme::ink_dim())
                        .hover(|button| button.text_color(Theme::ink()))
                        .on_click(
                            cx.listener(|this, _: &ClickEvent, window, cx| this.end_terminal(window, cx)),
                        )
                        .child("End"),
                )
            })
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

    pub fn hover_turn(&mut self, turn: Option<usize>, cx: &mut Context<Self>) {
        if self.hovered_turn != turn {
            self.hovered_turn = turn;
            cx.notify();
        }
    }

    pub fn reveal_turn(&mut self, index: usize, cx: &mut Context<Self>) {
        self.turns.scroll_to_reveal_item(index);
        cx.notify();
    }

    fn active_turn(&self) -> usize {
        let last = self.exchanges.len().saturating_sub(1);
        if self.turns.is_scrolled_to_end() == Some(false) {
            self.turns.logical_scroll_top().item_ix.min(last)
        } else {
            last
        }
    }

    fn chat_body(
        &mut self,
        column: Pixels,
        behind: bool,
        footer: gpui::Div,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let shared = self.handle.view();
        let view = shared.read();
        let exchanges = Arc::clone(&self.exchanges);
        let folds = Arc::clone(&self.folds);
        let pane = cx.entity().downgrade();
        let cwd = self.summary.cwd.clone();
        let home = std::env::var("HOME").ok();
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
                            cwd: cwd.as_deref(),
                            home: home.as_deref(),
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
            .child(
                div()
                    .relative()
                    .flex_1()
                    .w_full()
                    .pt_2()
                    .child(transcript)
                    .children(
                        (window.viewport_size().width > px(900.))
                            .then(|| {
                                let rail = crate::turn_rail::Rail {
                                    exchanges: &self.exchanges,
                                    active: self.active_turn(),
                                    hovered: self.hovered_turn,
                                };
                                crate::turn_rail::rail(&rail, cx)
                            })
                            .flatten(),
                    ),
            )
            .children(back)
            .child(
                footer.children(self.queue_dock()).child(
                    self.approval_card(&view)
                        .unwrap_or_else(|| self.composer_card(&view, cx)),
                ),
            )
    }

    pub fn toggle_fold(&mut self, fold: Fold, cx: &mut Context<Self>) {
        Arc::make_mut(&mut self.folds).toggle(fold);
        let index = match fold {
            Fold::Exchange(index)
            | Fold::Group(index, _)
            | Fold::Thought(index, _)
            | Fold::Tool(index, _, _) => index,
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

    fn decision_button(
        &self,
        label: &'static str,
        decision: Decision,
        primary: bool,
        request_id: &str,
    ) -> impl IntoElement {
        let handle = self.handle.clone();
        let request_id = request_id.to_owned();
        div()
            .id(label)
            .h(px(34.))
            .px_4()
            .rounded(px(17.))
            .flex()
            .items_center()
            .cursor_pointer()
            .when(primary, |button| {
                button.bg(Theme::accent()).text_color(Theme::on_accent())
            })
            .when(!primary, |button| {
                button
                    .border_1()
                    .border_color(Theme::line())
                    .text_color(Theme::ink())
            })
            .on_click(move |_, _, _| handle.decide(request_id.clone(), decision))
            .child(label)
    }

    fn approval_card(&self, view: &SessionView) -> Option<AnyElement> {
        let approval = view.approvals().first()?;
        let request_id = approval.request_id.clone();
        Some(
            div()
                .w_full()
                .p_5()
                .rounded(px(28.))
                .bg(Theme::warn_container())
                .border_1()
                .border_color(Theme::warn_line())
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .text_xs()
                        .text_color(Theme::warn())
                        .child("Waiting for approval"),
                )
                .child(
                    div()
                        .text_size(px(15.))
                        .text_color(Theme::ink())
                        .child(approval.tool.clone()),
                )
                .children(approval.reason.clone().map(|reason| {
                    div()
                        .font_family(Theme::MONO)
                        .text_size(px(12.))
                        .text_color(Theme::ink_dim())
                        .child(reason)
                }))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .justify_end()
                        .child(self.decision_button("Reject", Decision::Deny, false, &request_id))
                        .child(self.decision_button(
                            "Allow for session",
                            Decision::AllowForSession,
                            false,
                            &request_id,
                        ))
                        .child(self.decision_button("Allow once", Decision::Allow, true, &request_id)),
                )
                .into_any_element(),
        )
    }

    fn queue_dock(&self) -> Option<AnyElement> {
        let queued = self.handle.queued();
        if queued.is_empty() {
            return None;
        }
        Some(
            div()
                .w_full()
                .px_4()
                .py_2()
                .rounded(px(18.))
                .bg(Theme::panel())
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_xs().text_color(Theme::ink_dim()).child(format!(
                    "{} queued {}",
                    queued.len(),
                    if queued.len() == 1 { "message" } else { "messages" }
                )))
                .children(queued.into_iter().enumerate().map(|(index, text)| {
                    let handle = self.handle.clone();
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .text_color(Theme::ink())
                                .child(crate::text::one_line(Some(&text))),
                        )
                        .child(
                            div()
                                .id(("unqueue", index))
                                .cursor_pointer()
                                .text_color(Theme::ink_dim())
                                .on_click(move |_, _, _| handle.unqueue(index))
                                .child("×"),
                        )
                }))
                .into_any_element(),
        )
    }

    fn composer_card(&self, view: &SessionView, cx: &mut Context<Self>) -> AnyElement {
        let busy = view.busy();
        let draft = !self.composer.read(cx).is_empty();
        let (label, enabled) = match (busy, draft) {
            (true, false) => ("■", true),
            (true, true) => ("Queue", true),
            (false, draft) => ("↑", draft),
        };
        let button = div()
            .id("send")
            .h(px(34.))
            .min_w(px(34.))
            .px_3()
            .rounded(px(17.))
            .flex()
            .items_center()
            .justify_center()
            .bg(Theme::accent())
            .text_color(Theme::on_accent())
            .when(!enabled, |button| button.opacity(0.4))
            .when(enabled, gpui::Styled::cursor_pointer)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if busy && !draft {
                    this.handle.stop();
                } else if let Some(text) = this.composer.update(cx, super::input::TextInput::take) {
                    this.handle.send(text);
                }
            }))
            .child(label);
        div()
            .w_full()
            .pl_5()
            .pr_2()
            .py_2()
            .rounded(px(28.))
            .bg(Theme::card())
            .flex()
            .items_end()
            .gap_2()
            .child(div().flex_1().py_1().child(self.composer.clone()))
            .child(button)
            .into_any_element()
    }
}

fn column_width(window: &Window) -> Pixels {
    let body = window.viewport_size().width - px(68.);
    (body * 0.64).clamp(px(680.), px(920.)).min(body - px(48.))
}

impl gpui::EventEmitter<PaneEvent> for SessionPane {}

impl Render for SessionPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let column = column_width(window);
        let behind = self.turns.is_scrolled_to_end() == Some(false);
        let footer = {
            let shared = self.handle.view();
            let view = shared.read();
            div()
                .w(column)
                .pb_5()
                .pt_2()
                .flex()
                .flex_col()
                .gap_3()
                .children(
                    view.error()
                        .map(|message| div().text_color(Theme::warn()).child(message.to_owned())),
                )
                .children(Self::context_bar(&view))
        };
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
            _ => self.chat_body(column, behind, footer, window, cx),
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
