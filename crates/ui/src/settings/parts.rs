use gpui::{
    AnyElement, Div, IntoElement, ParentElement, SharedString, Stateful, Styled, Window, div, prelude::*, px,
};

use crate::theme::Theme;

/// Something Settings reads from a node, while it's being read and once it has been.
pub enum Load<T> {
    Waiting,
    Ready(T),
    Failed(SharedString),
}

impl<T> Load<T> {
    pub fn from(result: Result<T, gateway::GatewayError>) -> Self {
        result.map_or_else(|err| Self::Failed(err.to_string().into()), Self::Ready)
    }
}

/// A titled card of rows, ruled between them.
pub fn section(title: &'static str, rows: Vec<AnyElement>) -> Div {
    section_with(title, None, rows)
}

/// A section with a control beside its title, such as a button that adds a row.
pub fn section_with(title: &'static str, action: Option<AnyElement>, rows: Vec<AnyElement>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .min_h(px(28.))
                .px_1()
                .flex()
                .items_center()
                .justify_between()
                .child(div().text_color(Theme::ink_dim()).child(title))
                .children(action),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .rounded(px(20.))
                .bg(Theme::card())
                .overflow_hidden()
                .children(rows.into_iter().enumerate().map(|(index, row)| {
                    div()
                        .when(index > 0, |line| line.border_t_1().border_color(Theme::line()))
                        .child(row)
                })),
        )
}

pub fn row() -> Div {
    div()
        .min_h(px(52.))
        .px_5()
        .py_3()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
}

/// A label on the left and its value on the right.
pub fn field(label: &'static str, value: Div) -> AnyElement {
    row()
        .child(div().flex_none().text_color(Theme::ink_dim()).child(label))
        .child(value.min_w_0())
        .into_any_element()
}

/// What a row is, with a line under it saying what it does.
pub fn labelled(label: impl Into<SharedString>, detail: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .min_w_0()
        .child(label.into())
        .child(dim(detail).text_xs())
}

/// A row of prose explaining something the node does on its own.
pub fn note(title: &'static str, text: &'static str) -> AnyElement {
    div()
        .px_5()
        .py_3()
        .flex()
        .flex_col()
        .gap_1()
        .child(title)
        .child(dim(text).text_xs())
        .into_any_element()
}

/// A row that flips a setting when clicked anywhere, with a switch showing its state.
pub fn toggle(
    id: &'static str,
    label: &'static str,
    detail: &'static str,
    on: bool,
    flip: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    toggle_if(true, id, label, detail, on, flip)
}

/// A toggle that only flips while `enabled`, shown faded otherwise, for a switch that a switch
/// above it has turned off.
pub fn toggle_if(
    enabled: bool,
    id: &'static str,
    label: &'static str,
    detail: &'static str,
    on: bool,
    flip: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let track = div()
        .w(px(36.))
        .h(px(20.))
        .flex_none()
        .flex()
        .items_center()
        .px(px(3.))
        .rounded_full()
        .bg(if on { Theme::accent() } else { Theme::line() })
        .when(on, Styled::justify_end)
        .child(
            div()
                .size(px(14.))
                .rounded_full()
                .bg(if on { Theme::on_accent() } else { Theme::ink() }),
        );
    row()
        .id(id)
        .map(|row| {
            if enabled {
                row.cursor_pointer()
                    .hover(|row| row.bg(Theme::raised()))
                    .on_click(flip)
            } else {
                row.opacity(0.45)
            }
        })
        .child(labelled(label, detail))
        .child(track)
        .into_any_element()
}

/// The strip that holds one choice of a few, as `segment`s.
pub fn segments() -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .p_1()
        .rounded(px(18.))
        .bg(Theme::panel())
}

/// One choice in `segments`, filled while it's the chosen one.
pub fn segment(id: &'static str, label: &'static str, here: bool) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(30.))
        .px_4()
        .rounded(px(15.))
        .flex()
        .items_center()
        .cursor_pointer()
        .map(|chip| {
            if here {
                chip.bg(Theme::selected()).text_color(Theme::on_selected())
            } else {
                chip.text_color(Theme::ink_dim())
                    .hover(|chip| chip.bg(Theme::raised()))
            }
        })
        .child(label)
}

/// An outlined pill button.
pub fn button(id: impl Into<gpui::ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .h(px(28.))
        .px_3()
        .rounded(px(14.))
        .border_1()
        .border_color(Theme::line())
        .flex()
        .items_center()
        .cursor_pointer()
        .text_xs()
        .text_color(Theme::ink())
        .hover(|button| button.bg(Theme::raised()))
        .child(label.into())
}

/// The filled pill for the one thing a row is for.
pub fn primary(id: impl Into<gpui::ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .h(px(32.))
        .px_4()
        .rounded(px(16.))
        .flex()
        .items_center()
        .cursor_pointer()
        .bg(Theme::accent())
        .text_color(Theme::on_accent())
        .child(label.into())
}

pub fn pending<T>(load: &Load<T>) -> Vec<AnyElement> {
    let line = match load {
        Load::Failed(problem) => div().text_color(Theme::warn()).child(problem.clone()),
        _ => dim("Checking…"),
    };
    vec![row().child(line).into_any_element()]
}

pub fn dim(text: impl Into<SharedString>) -> Div {
    div().text_color(Theme::ink_dim()).child(text.into())
}

pub fn mono(text: impl Into<SharedString>) -> Div {
    div()
        .font_family(Theme::MONO)
        .text_xs()
        .text_color(Theme::ink_dim())
        .truncate()
        .child(text.into())
}

/// A path, cut at its start when it doesn't fit, since its end says the most.
pub fn path(text: impl Into<SharedString>) -> Div {
    div()
        .font_family(Theme::MONO)
        .text_xs()
        .text_color(Theme::ink_dim())
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis_start()
        .child(text.into())
}

pub fn dot(color: gpui::Rgba) -> Div {
    div().size(px(8.)).flex_none().rounded_full().bg(color)
}

pub fn chip(text: impl Into<SharedString>) -> Div {
    div()
        .flex_none()
        .px_2()
        .rounded(px(8.))
        .text_xs()
        .bg(Theme::selected())
        .text_color(Theme::on_selected())
        .child(text.into())
}

pub fn warn_chip(text: &'static str) -> Div {
    div()
        .flex_none()
        .px_2()
        .rounded(px(8.))
        .text_xs()
        .bg(Theme::warn_container())
        .text_color(Theme::warn())
        .child(text)
}

pub fn compact(count: u64) -> String {
    match count {
        0..1_000 => count.to_string(),
        1_000..10_000 => format!("{}.{}k", count / 1_000, count % 1_000 / 100),
        10_000..1_000_000 => format!("{}k", count / 1_000),
        _ => format!("{}.{}M", count / 1_000_000, count % 1_000_000 / 100_000),
    }
}
