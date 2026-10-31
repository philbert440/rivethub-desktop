mod fields;

use std::collections::BTreeMap;

use gpui::{
    App, Context, Entity, EventEmitter, IntoElement, ParentElement, PathPromptOptions, Render, Styled,
    Subscription, Window, div, prelude::*,
};
use model::plugin::view::{Field, FileValue, Input, Value};
use model::plugin::{accepts, media_type};

use crate::input::{Submitted, TextInput};
use crate::text::bytes;

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

/// The text a text, secret or number field starts with.
fn given_text(field: &Field) -> Option<String> {
    match &field.value {
        Some(Value::Text(text)) => Some(text.clone()),
        Some(Value::Number(number)) => Some(shown_number(*number)),
        _ => None,
    }
}

/// The value a toggle, select or file field starts with; a toggle starts off unless it says.
fn given_choice(field: &Field) -> Option<Value> {
    match field.input {
        Input::Toggle => Some(field.value.clone().unwrap_or(Value::Bool(false))),
        _ => field.value.clone(),
    }
}

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
                    let value = given_text(field);
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
                    Input::Toggle | Input::Select { .. } | Input::File { .. } => (None, given_choice(field)),
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

    /// Puts every field back as the view gave it, once the plug-in took what was sent.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        for entry in &mut self.entries {
            if let Some(input) = &entry.typed {
                let given = given_text(&entry.field).unwrap_or_default();
                input.update(cx, |input, cx| input.set_text(&given, cx));
            } else {
                entry.chosen = given_choice(&entry.field);
            }
        }
        self.problems.clear();
        self.choosing = None;
        cx.notify();
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

impl Render for FormView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .children((0..self.entries.len()).map(|index| self.entry(index, window, cx)))
    }
}
