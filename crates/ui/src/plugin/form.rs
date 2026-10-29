use std::collections::BTreeMap;

use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, Focusable, FontWeight, IntoElement,
    MouseButton, ParentElement, PathPromptOptions, Render, SharedString, Styled, Subscription, Window, div,
    prelude::*, px,
};
use model::plugin::view::{Choice, Field, FileValue, Input, Value};
use model::plugin::{accepts, media_type};

use crate::brand;
use crate::input::{Submitted, TextInput};
use crate::text::bytes;
use crate::theme::Theme;

/// The most a form's files may add up to: rh takes 16 MiB a request, and base64 grows them by a third.
const FILES_LIMIT: usize = 12 * 1024 * 1024;

/// Enter in one of the form's one-line fields.
pub struct FormSubmitted;

struct Entry {
    field: Field,
    /// The text of a text, secret or number field.
    typed: Option<Entity<TextInput>>,
    /// The value of a toggle, select or file field.
    chosen: Option<Value>,
}

/// A plug-in's form, or its settings, drawn as the app's own fields.
pub struct FormView {
    given: Vec<Field>,
    entries: Vec<Entry>,
    problems: BTreeMap<String, String>,
    /// The single-choice field whose options are open.
    choosing: Option<usize>,
    _submitted: Vec<Subscription>,
}

impl EventEmitter<FormSubmitted> for FormView {}

fn shown_number(number: f64) -> String {
    if number.fract() == 0. && number.abs() < 1e15 {
        format!("{number:.0}")
    } else {
        number.to_string()
    }
}

impl FormView {
    /// `secrets_set` names the secrets that hold a value, which is never sent back to be shown.
    pub fn new(fields: Vec<Field>, secrets_set: &[String], cx: &mut Context<Self>) -> Self {
        let mut submitted = Vec::new();
        let entries = fields
            .iter()
            .map(|field| {
                let text = |placeholder: &str, multiline: bool, secret: bool, cx: &mut Context<Self>| {
                    let value = match &field.value {
                        Some(Value::Text(text)) => Some(text.clone()),
                        Some(Value::Number(number)) => Some(shown_number(*number)),
                        _ => None,
                    };
                    let placeholder = placeholder.to_owned();
                    cx.new(move |cx| {
                        let mut input = TextInput::new(placeholder, cx).keeping();
                        if multiline {
                            input = input.multiline();
                        }
                        if secret {
                            input = input.secret();
                        }
                        match value {
                            Some(value) => input.with_text(&value),
                            None => input,
                        }
                    })
                };
                let placeholder = field.placeholder.clone().unwrap_or_default();
                let (typed, chosen) = match &field.input {
                    Input::Text { multiline } => (Some(text(&placeholder, *multiline, false, cx)), None),
                    Input::Secret => {
                        let hint = if secrets_set.contains(&field.key) {
                            "Set · type to replace it".to_owned()
                        } else {
                            placeholder
                        };
                        (Some(text(&hint, false, true, cx)), None)
                    }
                    Input::Number { .. } => (Some(text(&placeholder, false, false, cx)), None),
                    Input::Toggle => (None, Some(field.value.clone().unwrap_or(Value::Bool(false)))),
                    Input::Select { .. } | Input::File { .. } => (None, field.value.clone()),
                };
                let one_line = !matches!(field.input, Input::Text { multiline: true });
                if let Some(input) = typed.as_ref().filter(|_| one_line) {
                    submitted.push(cx.subscribe(input, |_, _, _: &Submitted, cx| cx.emit(FormSubmitted)));
                }
                Entry {
                    field: field.clone(),
                    typed,
                    chosen,
                }
            })
            .collect();
        Self {
            given: fields,
            entries,
            problems: BTreeMap::new(),
            choosing: None,
            _submitted: submitted,
        }
    }

    /// The fields as they were given, to tell whether a fetch changed the form.
    pub fn given(&self) -> &[Field] {
        &self.given
    }

    fn value(entry: &Entry, cx: &App) -> Option<Value> {
        match (&entry.field.input, &entry.typed) {
            (Input::Number { .. }, Some(input)) => {
                let text = input.read(cx).text();
                (!text.is_empty()).then(|| text.parse().ok().map(Value::Number))?
            }
            (_, Some(input)) => {
                let text = input.read(cx).text();
                (!text.is_empty()).then(|| Value::Text(text.to_owned()))
            }
            (_, None) => entry.chosen.clone().filter(|value| !value.is_empty()),
        }
    }

    /// What the person filled in, by key.
    pub fn values(&self, cx: &App) -> BTreeMap<String, Value> {
        self.entries
            .iter()
            .filter_map(|entry| Some((entry.field.key.clone(), Self::value(entry, cx)?)))
            .collect()
    }

    /// The values to keep as a plug-in's settings: an emptied field clears its value, and a secret
    /// left blank keeps the one it has.
    pub fn settings(&self, cx: &App) -> BTreeMap<String, Option<Value>> {
        self.entries
            .iter()
            .filter_map(|entry| {
                let value = Self::value(entry, cx);
                let kept_secret = matches!(entry.field.input, Input::Secret) && value.is_none();
                (!kept_secret).then(|| (entry.field.key.clone(), value))
            })
            .collect()
    }

    /// Marks what's wrong before anything is sent; whether every field is fine.
    pub fn check(&mut self, cx: &mut Context<Self>) -> bool {
        self.problems.clear();
        for entry in &self.entries {
            let value = Self::value(entry, cx);
            let unreadable = matches!(entry.field.input, Input::Number { .. })
                && value.is_none()
                && entry
                    .typed
                    .as_ref()
                    .is_some_and(|input| !input.read(cx).is_empty());
            let problem = if unreadable {
                Some("Type a number.".to_owned())
            } else {
                entry.field.problem(value.as_ref())
            };
            if let Some(problem) = problem {
                self.problems.insert(entry.field.key.clone(), problem);
            }
        }
        cx.notify();
        self.problems.is_empty()
    }

    /// Shows what the plug-in said is wrong with which fields.
    pub fn refused(&mut self, problems: BTreeMap<String, String>, cx: &mut Context<Self>) {
        self.problems = problems;
        cx.notify();
    }

    fn choose(&mut self, index: usize, value: &str, cx: &mut Context<Self>) {
        let Some(entry) = self.entries.get_mut(index) else {
            return;
        };
        let Input::Select { multiple, .. } = entry.field.input else {
            return;
        };
        if multiple {
            let mut chosen = match entry.chosen.take() {
                Some(Value::List(items)) => items,
                _ => Vec::new(),
            };
            if let Some(at) = chosen.iter().position(|item| item == value) {
                chosen.remove(at);
            } else {
                chosen.push(value.to_owned());
            }
            entry.chosen = Some(Value::List(chosen));
        } else {
            entry.chosen = Some(Value::Text(value.to_owned()));
            self.choosing = None;
        }
        self.problems.remove(&entry.field.key);
        cx.notify();
    }

    fn flip(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(entry) = self.entries.get_mut(index) {
            let on = matches!(entry.chosen, Some(Value::Bool(true)));
            entry.chosen = Some(Value::Bool(!on));
            cx.notify();
        }
    }

    fn pick_files(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(Input::File { multiple, accept }) =
            self.entries.get(index).map(|entry| entry.field.input.clone())
        else {
            return;
        };
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let read = cx
                .background_executor()
                .spawn(async move {
                    paths
                        .into_iter()
                        .map(|path| {
                            let name = path
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let bytes =
                                std::fs::read(&path).map_err(|err| format!("reading {name}: {err}"))?;
                            Ok(FileValue {
                                media_type: media_type(&name).to_owned(),
                                name,
                                bytes,
                            })
                        })
                        .collect::<Result<Vec<_>, String>>()
                })
                .await;
            this.update(cx, |this, cx| {
                let Some(entry) = this.entries.get_mut(index) else {
                    return;
                };
                let key = entry.field.key.clone();
                let problem = match read {
                    Err(problem) => Some(problem),
                    Ok(files) => {
                        let refused = files.iter().find(|file| !accepts(&accept, &file.media_type));
                        if let Some(file) = refused {
                            Some(format!("{} isn't a kind of file this takes.", file.name))
                        } else if files.iter().map(|file| file.bytes.len()).sum::<usize>() > FILES_LIMIT {
                            Some(format!(
                                "Files can add up to {} at most.",
                                bytes(FILES_LIMIT as u64)
                            ))
                        } else {
                            entry.chosen = Some(Value::Files(files));
                            None
                        }
                    }
                };
                match problem {
                    Some(problem) => this.problems.insert(key, problem),
                    None => this.problems.remove(&key),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn unpick(&mut self, index: usize, file: usize, cx: &mut Context<Self>) {
        if let Some(Entry {
            chosen: Some(Value::Files(files)),
            ..
        }) = self.entries.get_mut(index)
            && file < files.len()
        {
            files.remove(file);
            cx.notify();
        }
    }
}

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

    fn entry(&self, index: usize, window: &Window, cx: &mut Context<Self>) -> AnyElement {
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

impl Render for FormView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .children((0..self.entries.len()).map(|index| self.entry(index, window, cx)))
    }
}
