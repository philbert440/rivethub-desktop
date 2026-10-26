use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, IntoElement, ParentElement, Rgba, SharedString, Styled, div,
    prelude::*, pulsating_between, px,
};
use model::chat::Category;
use model::session::{ToolCall, ToolState};
use model::tools::{Change, DiffLine, ToolInput, counts, diff, label};

use crate::theme::Theme;
use crate::transcript::{Context, Fold, chevron, toggle_on};

const BODY_LINES: usize = 40;

fn glyph(name: &str) -> &'static str {
    match Category::of(name) {
        Category::Read => "▤",
        Category::Search => "⌕",
        Category::Edit => "✎",
        Category::Command => "❯",
        Category::Web => "◎",
        Category::Subagent => "⑂",
        Category::Tool => "◇",
    }
}

fn expandable(tool: &ToolCall) -> bool {
    !matches!(tool.input, ToolInput::Unknown)
        || tool.output.as_ref().is_some_and(|output| !output.text.is_empty())
        || fetchable(tool)
}

pub fn fetchable(tool: &ToolCall) -> bool {
    tool.output.is_none() && tool.id.is_some() && tool.state != ToolState::Running
}

fn suffix(tool: &ToolCall) -> Option<AnyElement> {
    let diffed = match &tool.input {
        ToolInput::Edit { before, after, .. } => Some(counts(&diff(before, after))),
        ToolInput::Write { content, .. } => Some((content.lines().count(), 0)),
        _ => None,
    };
    if tool.state != ToolState::Failed
        && let Some((added, removed)) = diffed
    {
        return Some(
            div()
                .flex_none()
                .flex()
                .gap_1()
                .text_xs()
                .child(div().text_color(Theme::accent()).child(format!("+{added}")))
                .when(removed > 0, |row| {
                    row.child(div().text_color(Theme::removed()).child(format!("−{removed}")))
                })
                .into_any_element(),
        );
    }
    if let Some(code) = tool.output.as_ref().and_then(|output| output.exit_code) {
        return Some(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .px_2()
                .rounded(px(9.))
                .bg(Theme::card())
                .text_xs()
                .child(div().size(px(6.)).rounded(px(3.)).bg(Theme::removed()))
                .child(format!("exit {code}"))
                .into_any_element(),
        );
    }
    (tool.state == ToolState::Failed).then(|| {
        div()
            .flex_none()
            .text_xs()
            .text_color(Theme::warn())
            .child("failed")
            .into_any_element()
    })
}

fn mono_line(text: impl Into<SharedString>, color: Rgba, background: Option<Rgba>) -> AnyElement {
    div()
        .px_3()
        .min_h(px(18.))
        .text_color(color)
        .when_some(background, gpui::Styled::bg)
        .child(text.into())
        .into_any_element()
}

fn tail_lines(text: &str) -> Vec<AnyElement> {
    let lines: Vec<&str> = text.lines().collect();
    let skipped = lines.len().saturating_sub(BODY_LINES);
    let mut out = Vec::with_capacity(BODY_LINES + 1);
    if skipped > 0 {
        out.push(mono_line(
            format!("… {skipped} earlier lines"),
            Theme::ink_dim(),
            None,
        ));
    }
    out.extend(
        lines[skipped..]
            .iter()
            .map(|line| mono_line((*line).to_owned(), Theme::ink_dim(), None)),
    );
    out
}

fn diff_lines(lines: &[DiffLine]) -> Vec<AnyElement> {
    lines
        .iter()
        .take(BODY_LINES * 2)
        .map(|line| match line {
            DiffLine::Gap(hidden) => mono_line(format!("⋯ {hidden} unchanged lines"), Theme::ink_dim(), None),
            DiffLine::Line { change, text } => {
                let (sign, color, background) = match change {
                    Change::Same => (" ", Theme::ink_dim(), None),
                    Change::Added => ("+", Theme::ink(), Some(Theme::added_bg())),
                    Change::Removed => ("−", Theme::ink(), Some(Theme::removed_bg())),
                };
                mono_line(format!("{sign} {text}"), color, background)
            }
        })
        .collect()
}

fn body(tool: &ToolCall) -> Vec<AnyElement> {
    let output = tool
        .output
        .as_ref()
        .map(|output| output.text.as_str())
        .unwrap_or_default();
    match &tool.input {
        ToolInput::Command { command, .. } => {
            let mut lines: Vec<AnyElement> = command
                .lines()
                .map(|line| mono_line(format!("$ {line}"), Theme::ink(), None))
                .collect();
            lines.extend(tail_lines(output));
            lines
        }
        ToolInput::Edit { before, after, .. } => diff_lines(&diff(before, after)),
        ToolInput::Write { content, .. } => diff_lines(&diff("", content)),
        ToolInput::Fields(fields) => {
            let mut lines: Vec<AnyElement> = fields
                .iter()
                .map(|(key, value)| mono_line(format!("{key}: {value}"), Theme::ink(), None))
                .collect();
            lines.extend(tail_lines(output));
            lines
        }
        _ => tail_lines(output),
    }
}

pub fn row(tool: &ToolCall, fold: Fold, cx: &Context<'_>) -> AnyElement {
    let fetched = tool
        .output
        .is_none()
        .then(|| tool.id.as_ref().and_then(|id| cx.fetched.get(id)))
        .flatten()
        .map(|output| ToolCall {
            output: Some(output.clone()),
            ..tool.clone()
        });
    let tool = fetched.as_ref().unwrap_or(tool);
    let open = cx.folds.open(fold);
    let can_open = expandable(tool);
    let running = tool.state == ToolState::Running;
    let id = SharedString::from(format!("tool-{fold:?}"));
    let text = label(&tool.name, &tool.input, cx.cwd, cx.home);
    let mut header = toggle_on(id.clone(), fold, cx.pane.clone())
        .h(px(24.))
        .pl(px(6.))
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(13.))
        .text_color(Theme::ink_dim())
        .hover(|row| row.text_color(Theme::ink()))
        .child(
            div()
                .flex_none()
                .w(px(12.))
                .child(if can_open { chevron(open) } else { "" }),
        )
        .child(div().flex_none().w(px(14.)).child(glyph(&tool.name)))
        .child(div().flex_none().child(tool.name.clone()))
        .when(text != tool.name, |row| {
            row.child(div().flex_1().truncate().child(text))
        })
        .children(suffix(tool));
    if !can_open {
        header = header.cursor_default();
    }
    let header = if running {
        header
            .with_animation(
                id,
                Animation::new(Duration::from_millis(1400))
                    .repeat()
                    .with_easing(pulsating_between(0.5, 1.)),
                gpui::Styled::opacity,
            )
            .into_any_element()
    } else {
        header.into_any_element()
    };
    div()
        .pl(px(16.))
        .flex()
        .flex_col()
        .child(header)
        .when(open && can_open, |row| {
            row.child(
                div()
                    .ml(px(32.))
                    .my_1()
                    .py_2()
                    .rounded(px(10.))
                    .bg(Theme::code())
                    .overflow_hidden()
                    .font_family(Theme::MONO)
                    .text_size(px(12.))
                    .flex()
                    .flex_col()
                    .children(body(tool)),
            )
        })
        .into_any_element()
}
