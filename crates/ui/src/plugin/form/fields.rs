use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, Focusable, FontWeight, IntoElement, MouseButton,
    ParentElement, SharedString, Styled, Window, div, prelude::*, px,
};
use model::plugin::view::{Choice, Input, Value};

use super::FormView;
use crate::brand;
use crate::input::TextInput;
use crate::text::bytes;
use crate::theme::Theme;

impl FormView {
    fn text_box(
        input: &Entity<TextInput>,
        tall: bool,
        problem: bool,
        window: &Window,
        cx: &App,
    ) -> AnyElement {
        let handle = input.focus_handle(cx);
        let focused = handle.is_focused(window);
        div()
            .px_3()
            .py_2()
            .rounded(px(12.))
            .bg(Theme::panel())
            .border_1()
            .border_color(if focused {
                Theme::accent()
            } else if problem {
                Theme::danger()
            } else {
                Theme::line()
            })
            .cursor_text()
            .when(tall, |field| field.min_h(px(88.)))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| window.focus(&handle, cx))
            .child(input.clone())
            .into_any_element()
    }

    fn switch(on: bool) -> impl IntoElement {
        div()
            .w(px(36.))
            .h(px(20.))
            .flex_none()
            .flex()
            .items_center()
            .px(px(3.))
            .rounded_full()
            .bg(if on { Theme::accent() } else { Theme::line() })
            .when(on, Styled::justify_end)
            .child(div().size(px(14.)).rounded_full().bg(Theme::ink()))
    }

    fn chip(id: SharedString, label: &str, on: bool) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .h(px(30.))
            .px_3()
            .rounded(px(15.))
            .flex()
            .items_center()
            .gap_1()
            .cursor_pointer()
            .text_sm()
            .map(|chip| {
                if on {
                    chip.bg(Theme::selected()).text_color(Theme::on_selected())
                } else {
                    chip.border_1()
                        .border_color(Theme::line())
                        .text_color(Theme::ink())
                        .hover(|chip| chip.bg(Theme::raised()))
                }
            })
            .when(on, |chip| {
                chip.child(brand::icon(
                    brand::symbol(Some("check")),
                    14.,
                    Theme::on_selected(),
                ))
            })
            .child(label.to_owned())
    }

    fn select(&self, index: usize, options: &[Choice], multiple: bool, cx: &mut Context<Self>) -> AnyElement {
        let chosen = self.entries.get(index).and_then(|entry| entry.chosen.clone());
        let picked = |value: &str| match &chosen {
            Some(Value::Text(text)) => text == value,
            Some(Value::List(items)) => items.iter().any(|item| item == value),
            _ => false,
        };
        if multiple {
            return div()
                .flex()
                .flex_wrap()
                .gap_2()
                .children(options.iter().enumerate().map(|(at, option)| {
                    let value = option.value.clone();
                    Self::chip(
                        format!("choice-{index}-{at}").into(),
                        &option.label,
                        picked(&option.value),
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.choose(index, &value, cx)))
                }))
                .into_any_element();
        }
        let open = self.choosing == Some(index);
        let label = options
            .iter()
            .find(|option| picked(&option.value))
            .map_or_else(|| "Choose…".to_owned(), |option| option.label.clone());
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .id(("select", index))
                    .h(px(38.))
                    .px_3()
                    .rounded(px(12.))
                    .bg(Theme::panel())
                    .border_1()
                    .border_color(if open { Theme::accent() } else { Theme::line() })
                    .flex()
                    .items_center()
                    .justify_between()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.choosing = if open { None } else { Some(index) };
                        cx.notify();
                    }))
                    .child(label)
                    .child(brand::icon(
                        brand::symbol(Some(if open { "expand_less" } else { "expand_more" })),
                        18.,
                        Theme::ink_dim(),
                    )),
            )
            .when(open, |select| {
                select.child(
                    div()
                        .p_1()
                        .rounded(px(12.))
                        .bg(Theme::raised())
                        .border_1()
                        .border_color(Theme::line())
                        .flex()
                        .flex_col()
                        .children(options.iter().enumerate().map(|(at, option)| {
                            let value = option.value.clone();
                            let here = picked(&option.value);
                            div()
                                .id(("option", at))
                                .px_3()
                                .py_2()
                                .rounded(px(10.))
                                .flex()
                                .items_center()
                                .justify_between()
                                .cursor_pointer()
                                .hover(|row| row.bg(Theme::selected()).text_color(Theme::on_selected()))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.choose(index, &value, cx);
                                }))
                                .child(option.label.clone())
                                .when(here, |row| {
                                    row.child(brand::icon(brand::symbol(Some("check")), 16., Theme::accent()))
                                })
                        })),
                )
            })
            .into_any_element()
    }

    fn files(&self, index: usize, multiple: bool, cx: &mut Context<Self>) -> AnyElement {
        let files = match self.entries.get(index).and_then(|entry| entry.chosen.as_ref()) {
            Some(Value::Files(files)) => files.as_slice(),
            _ => &[],
        };
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div().flex().child(
                    Self::chip(
                        format!("pick-{index}").into(),
                        if multiple {
                            "Choose files…"
                        } else {
                            "Choose a file…"
                        },
                        false,
                    )
                    .child(brand::icon(
                        brand::symbol(Some("attach_file")),
                        14.,
                        Theme::ink_dim(),
                    ))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.pick_files(index, cx))),
                ),
            )
            .children(files.iter().enumerate().map(|(at, file)| {
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .child(brand::icon(
                        brand::symbol(Some("description")),
                        16.,
                        Theme::ink_dim(),
                    ))
                    .child(div().min_w_0().truncate().child(file.name.clone()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .child(bytes(file.bytes.len() as u64)),
                    )
                    .child(
                        div()
                            .id(("unpick", at))
                            .cursor_pointer()
                            .child(brand::icon(brand::symbol(Some("close")), 14., Theme::ink_dim()))
                            .on_click(
                                cx.listener(move |this, _: &ClickEvent, _, cx| this.unpick(index, at, cx)),
                            ),
                    )
            }))
            .into_any_element()
    }

    pub(super) fn entry(&self, index: usize, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(entry) = self.entries.get(index) else {
            return div().into_any_element();
        };
        let field = &entry.field;
        let problem = self.problems.get(&field.key).cloned();
        let label = div()
            .flex()
            .gap_1()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .text_color(Theme::ink())
            .child(field.label.clone())
            .when(field.required, |label| {
                label.child(div().text_color(Theme::ink_dim()).child("*"))
            });
        let accept = match &field.input {
            Input::File { accept, .. } if !accept.is_empty() => Some(format!("Takes {}", accept.join(", "))),
            _ => None,
        };
        let notes = field.help.iter().cloned().chain(accept).map(|note| {
            div()
                .text_xs()
                .text_color(Theme::ink_dim())
                .child(note)
                .into_any_element()
        });
        let problem_line = problem
            .clone()
            .map(|problem| div().text_xs().text_color(Theme::danger()).child(problem));
        if let Input::Toggle = field.input {
            let on = matches!(entry.chosen, Some(Value::Bool(true)));
            return div()
                .id(("toggle", index))
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .cursor_pointer()
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.flip(index, cx)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .min_w_0()
                        .child(label)
                        .children(notes),
                )
                .child(Self::switch(on))
                .into_any_element();
        }
        let control = match (&field.input, &entry.typed) {
            (Input::Text { multiline }, Some(input)) => {
                Self::text_box(input, *multiline, problem.is_some(), window, cx)
            }
            (Input::Secret | Input::Number { .. }, Some(input)) => {
                Self::text_box(input, false, problem.is_some(), window, cx)
            }
            (Input::Select { options, multiple }, _) => self.select(index, options, *multiple, cx),
            (Input::File { multiple, .. }, _) => self.files(index, *multiple, cx),
            _ => div().into_any_element(),
        };
        div()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(label)
            .child(control)
            .children(notes)
            .children(problem_line)
            .into_any_element()
    }
}
