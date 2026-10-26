use std::collections::HashSet;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, Hsla, IntoElement, ParentElement, Pixels, SharedString, Styled,
    WeakEntity, Window, div, prelude::*, pulsating_between, px,
};
use model::chat::{Exchange, ExchangeState, Step, group_summary, running_label, thought_summary};
use model::markdown::{Block, Parsed, parse};

use crate::markdown_view;
use crate::session_pane::SessionPane;
use crate::theme::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeView {
    pub exchange: Exchange,
    answer: Parsed,
    notes: Vec<Vec<Block>>,
}

fn note_blocks(exchange: &Exchange, previous: Option<&ExchangeView>) -> Vec<Vec<Block>> {
    exchange
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            let Step::Note(text) = step else {
                return Vec::new();
            };
            previous
                .filter(|previous| previous.exchange.steps.get(index) == Some(step))
                .and_then(|previous| previous.notes.get(index).cloned())
                .unwrap_or_else(|| parse(text))
        })
        .collect()
}

impl ExchangeView {
    pub fn new(exchange: Exchange) -> Self {
        Self {
            answer: Parsed::new(&exchange.answer),
            notes: note_blocks(&exchange, None),
            exchange,
        }
    }

    pub fn advance(previous: &Self, exchange: Exchange) -> Self {
        Self {
            answer: previous
                .answer
                .advance(&previous.exchange.answer, &exchange.answer),
            notes: note_blocks(&exchange, Some(previous)),
            exchange,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fold {
    Exchange(usize),
    Group(usize, usize),
    Thought(usize, usize),
    Tool(usize, usize, usize),
}

#[derive(Debug, Clone, Default)]
pub struct Folds(HashSet<Fold>);

impl Folds {
    pub fn toggle(&mut self, fold: Fold) {
        if !self.0.remove(&fold) {
            self.0.insert(fold);
        }
    }

    pub fn open(&self, fold: Fold) -> bool {
        self.0.contains(&fold)
    }
}

pub struct Context<'a> {
    pub index: usize,
    pub folds: &'a Folds,
    pub column: Pixels,
    pub pane: WeakEntity<SessionPane>,
    pub cwd: Option<&'a str>,
    pub home: Option<&'a str>,
    pub fetched: &'a std::collections::HashMap<String, model::tools::ToolOutput>,
}

pub fn toggle_on(id: SharedString, fold: Fold, pane: WeakEntity<SessionPane>) -> gpui::Stateful<gpui::Div> {
    div().id(id).cursor_pointer().on_click(move |_, _, cx| {
        pane.update(cx, |pane, cx| pane.toggle_fold(fold, cx)).ok();
    })
}

pub fn chevron(open: bool) -> &'static str {
    if open { "▾" } else { "▸" }
}

fn step(
    view: &ExchangeView,
    step_index: usize,
    item: &Step,
    working: bool,
    last: bool,
    cx: &Context<'_>,
    window: &Window,
) -> AnyElement {
    let ix = cx.index;
    match item {
        Step::Thought(text) => {
            let fold = Fold::Thought(ix, step_index);
            let open = cx.folds.open(fold);
            let summary = thought_summary(text, working && last);
            div()
                .flex()
                .flex_col()
                .child(
                    toggle_on(
                        SharedString::from(format!("thought-{ix}-{step_index}")),
                        fold,
                        cx.pane.clone(),
                    )
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(13.))
                    .text_color(Theme::ink_dim())
                    .child(div().w(px(16.)).child(chevron(open)))
                    .child("Think")
                    .when(!summary.is_empty(), |row| {
                        row.child("·").child(div().truncate().child(summary))
                    }),
                )
                .when(open, |thought| {
                    thought.child(
                        div()
                            .pl(px(22.))
                            .text_size(px(13.))
                            .text_color(Theme::ink_dim())
                            .child(text.trim().to_owned()),
                    )
                })
                .into_any_element()
        }
        Step::Note(_) => div()
            .pl(px(22.))
            .text_size(px(13.))
            .flex()
            .flex_col()
            .gap_2()
            .children(markdown_view::blocks(
                view.notes.get(step_index).map_or(&[][..], Vec::as_slice),
                &format!("note-{ix}-{step_index}"),
                Theme::ink_dim().into(),
                window,
            ))
            .into_any_element(),
        Step::Tools(tools) => {
            let fold = Fold::Group(ix, step_index);
            let running = running_label(tools);
            let open = cx.folds.open(fold) || (working && last);
            let label = running.map_or_else(|| group_summary(tools), str::to_owned);
            div()
                .flex()
                .flex_col()
                .child(
                    toggle_on(
                        SharedString::from(format!("group-{ix}-{step_index}")),
                        fold,
                        cx.pane.clone(),
                    )
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(13.))
                    .text_color(Theme::ink_dim())
                    .hover(|row| row.text_color(Theme::ink()))
                    .child(div().w(px(16.)).child(chevron(open)))
                    .child(label)
                    .child(div().text_xs().child(format!("{}", tools.len()))),
                )
                .when(open, |group| {
                    group.children(tools.iter().enumerate().map(|(tool, call)| {
                        crate::tool_view::row(call, Fold::Tool(ix, step_index, tool), cx)
                    }))
                })
                .into_any_element()
        }
    }
}

fn status(label: &str) -> AnyElement {
    div()
        .h(px(24.))
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(13.))
        .text_color(Theme::accent())
        .child(div().size(px(8.)).rounded(px(4.)).bg(Theme::accent()))
        .child(format!("Working · {label}"))
        .with_animation(
            "working",
            Animation::new(Duration::from_millis(1500))
                .repeat()
                .with_easing(pulsating_between(0.45, 1.)),
            gpui::Styled::opacity,
        )
        .into_any_element()
}

fn bubble(prompt: &str, column: Pixels) -> impl IntoElement {
    div().w_full().flex().justify_end().child(
        div()
            .max_w(column * 0.702)
            .px_4()
            .py(px(10.))
            .rounded(px(20.))
            .bg(Theme::raised())
            .text_color(Theme::ink())
            .child(prompt.trim().to_owned()),
    )
}

fn fold_header(exchange: &Exchange, ix: usize, open: bool, cx: &Context<'_>) -> impl IntoElement {
    let verdict = if exchange.state == ExchangeState::Failed {
        "Failed"
    } else {
        "Completed"
    };
    let tools = exchange.tool_count();
    let detail = if tools == 1 {
        "1 tool call".to_owned()
    } else {
        format!("{tools} tool calls")
    };
    toggle_on(
        SharedString::from(format!("exchange-{ix}")),
        Fold::Exchange(ix),
        cx.pane.clone(),
    )
    .h(px(33.))
    .flex()
    .items_center()
    .gap_2()
    .border_b_1()
    .border_color(Theme::line())
    .text_size(px(13.))
    .text_color(Theme::ink_dim())
    .hover(|row| row.text_color(Theme::ink()))
    .child(format!("{verdict} · {detail}"))
    .child(div().flex_1())
    .child(chevron(open))
}

pub fn exchange(view: &ExchangeView, cx: &Context<'_>, window: &Window) -> AnyElement {
    let exchange = &view.exchange;
    let ix = cx.index;
    let working = exchange.state == ExchangeState::Working;
    let ink: Hsla = Theme::ink().into();
    let mut column = div().w(cx.column).flex().flex_col().gap_3().py_3();
    if let Some(prompt) = &exchange.prompt {
        column = column.child(bubble(prompt, cx.column));
    }
    if exchange.has_process() {
        let fold = Fold::Exchange(ix);
        let open = working || cx.folds.open(fold);
        if !working {
            column = column.child(fold_header(exchange, ix, open, cx));
        }
        if open {
            let count = exchange.steps.len();
            column =
                column.child(div().flex().flex_col().gap(px(6.)).children(
                    exchange.steps.iter().enumerate().map(|(index, item)| {
                        step(view, index, item, working, index + 1 == count, cx, window)
                    }),
                ));
        }
    }
    if !view.answer.blocks.is_empty() {
        column = column.child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_4()
                .text_color(Theme::ink())
                .line_height(px(24.))
                .children(markdown_view::blocks(
                    &view.answer.blocks,
                    &format!("answer-{ix}"),
                    ink,
                    window,
                )),
        );
    }
    if working {
        let label = exchange
            .steps
            .iter()
            .rev()
            .find_map(|item| {
                if let Step::Tools(tools) = item {
                    running_label(tools)
                } else {
                    None
                }
            })
            .unwrap_or("Thinking");
        column = column.child(status(label));
    }
    div()
        .w_full()
        .flex()
        .justify_center()
        .child(column)
        .into_any_element()
}
