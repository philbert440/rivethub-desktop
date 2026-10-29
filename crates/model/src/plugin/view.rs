use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::plugin::Params;
use crate::plugin::canvas::{MenuItem, Scene};
use crate::{SessionId, TopicId};

/// A screen a plug-in answers, which the app draws with its own widgets.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub title: String,
    pub subtitle: Option<String>,
    pub toolbar: Vec<Button>,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Section {
        title: Option<String>,
        subtitle: Option<String>,
        actions: Vec<Button>,
        blocks: Vec<Block>,
        /// `None` for a section that doesn't fold; else whether it starts folded.
        collapsed: Option<bool>,
    },
    /// Side by side on a wide window, stacked on a narrow one.
    Split {
        primary: Vec<Block>,
        secondary: Vec<Block>,
        ratio: Option<f32>,
    },
    Tabs {
        tabs: Vec<Tab>,
        selected: Option<String>,
    },
    Text {
        text: String,
        tone: Option<Tone>,
        monospace: bool,
    },
    Markdown(String),
    Code {
        text: String,
        language: Option<String>,
        /// Keeps the end in view, as a log does.
        follow: bool,
    },
    /// An image from the plug-in's own files.
    Image {
        path: String,
        alt: String,
    },
    Properties(Vec<Property>),
    Progress {
        /// From 0 to 1, or `None` while it can't say how far along it is.
        value: Option<f32>,
        label: Option<String>,
    },
    Notice {
        tone: Tone,
        title: Option<String>,
        text: String,
        actions: Vec<Button>,
    },
    List {
        items: Vec<ListItem>,
        empty: Option<Empty>,
    },
    Table {
        columns: Vec<Column>,
        rows: Vec<Row>,
        empty: Option<Empty>,
    },
    Breadcrumbs(Vec<Crumb>),
    Actions(Vec<Button>),
    Form(Form),
    Canvas(CanvasBlock),
    /// The plug-in's own HTML, which this app doesn't show yet.
    WebPanel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tab {
    pub id: String,
    pub title: String,
    pub badge: Option<String>,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    pub label: String,
    pub value: Inline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub icon: Option<String>,
    pub trailing: Vec<Inline>,
    pub action: Option<Action>,
    pub menu: Vec<Button>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub title: String,
    pub align: Align,
    pub sortable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub cells: Vec<Inline>,
    pub action: Option<Action>,
    pub menu: Vec<Button>,
}

/// What a list or table shows when it has nothing in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Empty {
    pub title: String,
    pub text: Option<String>,
    pub button: Option<Button>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crumb {
    pub label: String,
    pub action: Option<Action>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Form {
    pub id: String,
    pub fields: Vec<Field>,
    pub buttons: Vec<Button>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CanvasBlock {
    pub id: String,
    pub scene: Scene,
    pub height: Option<f32>,
    pub read_only: bool,
    /// The menu on empty space.
    pub menu: Vec<MenuItem>,
}

/// A value inside a line of text, formatted by the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline {
    Text {
        text: String,
        tone: Option<Tone>,
        icon: Option<String>,
        monospace: bool,
    },
    Badge {
        text: String,
        tone: Tone,
    },
    Time {
        at_ms: i64,
        style: Option<TimeStyle>,
    },
    Duration {
        ms: u64,
    },
    Bytes(u64),
}

impl Inline {
    /// How a sortable column orders its cells: times, durations and sizes by their value, words
    /// without regard to case, and a word after any number.
    pub fn order(&self, other: &Self) -> Ordering {
        match (self.number(), other.number()) {
            (Some(left), Some(right)) => left.cmp(&right),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => self.words().to_lowercase().cmp(&other.words().to_lowercase()),
        }
    }

    fn number(&self) -> Option<i128> {
        match self {
            Self::Time { at_ms, .. } => Some(i128::from(*at_ms)),
            Self::Duration { ms } => Some(i128::from(*ms)),
            Self::Bytes(value) => Some(i128::from(*value)),
            Self::Text { .. } | Self::Badge { .. } => None,
        }
    }

    fn words(&self) -> &str {
        match self {
            Self::Text { text, .. } | Self::Badge { text, .. } => text,
            Self::Time { .. } | Self::Duration { .. } | Self::Bytes(_) => "",
        }
    }
}

/// The rows of a table in the order a sortable column puts them, by index.
pub fn sorted(rows: &[Row], column: usize, descending: bool) -> Vec<usize> {
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by(|&left, &right| {
        let ordering = match (rows[left].cells.get(column), rows[right].cells.get(column)) {
            (Some(left), Some(right)) => left.order(right),
            (left, right) => left.is_some().cmp(&right.is_some()).reverse(),
        };
        if descending { ordering.reverse() } else { ordering }
    });
    order
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tone {
    #[default]
    Neutral,
    Info,
    Success,
    Warning,
    Danger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeStyle {
    Relative,
    Absolute,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    pub label: String,
    pub icon: Option<String>,
    pub style: ButtonStyle,
    pub action: Action,
    pub confirm: Option<Confirm>,
    pub disabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonStyle {
    Primary,
    #[default]
    Secondary,
    Danger,
}

/// Asked before a button's action runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub title: String,
    pub text: String,
    pub confirm_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub key: String,
    pub label: String,
    pub input: Input,
    pub value: Option<Value>,
    pub required: bool,
    pub help: Option<String>,
    pub placeholder: Option<String>,
}

impl Field {
    /// What's wrong with `value` for this field before it is sent, if anything.
    pub fn problem(&self, value: Option<&Value>) -> Option<String> {
        let Some(value) = value.filter(|value| !value.is_empty()) else {
            return self.required.then(|| format!("{} is needed.", self.label));
        };
        let (Input::Number { min, max, .. }, Value::Number(number)) = (&self.input, value) else {
            return None;
        };
        match (min, max) {
            (Some(min), _) if number < min => Some(format!("At least {min}.")),
            (_, Some(max)) if number > max => Some(format!("At most {max}.")),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    Text {
        multiline: bool,
    },
    Secret,
    Number {
        min: Option<f64>,
        max: Option<f64>,
        step: Option<f64>,
    },
    Toggle,
    Select {
        options: Vec<Choice>,
        multiple: bool,
    },
    File {
        multiple: bool,
        /// Media types, `image/*` for a family; empty takes any file.
        accept: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Number(f64),
    Text(String),
    List(Vec<String>),
    Files(Vec<FileValue>),
}

impl Value {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Bool(_) | Self::Number(_) => false,
            Self::Text(text) => text.trim().is_empty(),
            Self::List(items) => items.is_empty(),
            Self::Files(files) => files.is_empty(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileValue {
    pub name: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

/// What a button, a row or a plug-in's answer asks the app to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Runs one of the plug-in's actions, with the form's values when the button is a form's.
    Call {
        action: String,
        params: Option<Params>,
    },
    /// Opens a view as a page, or in place of the page shown when `replace`.
    Navigate {
        view: String,
        params: Option<Params>,
        replace: bool,
    },
    Dialog {
        view: String,
        params: Option<Params>,
    },
    /// Closes the dialog, else goes back a page.
    Back,
    /// Fetches every view of the plug-in on screen again.
    Refresh,
    Toast {
        text: String,
        tone: Tone,
    },
    Copy(String),
    OpenSession(SessionId),
    OpenTopic(TopicId),
    OpenUrl(String),
    /// Saves one of the plug-in's files.
    Download {
        path: String,
        filename: Option<String>,
    },
}

/// How a plug-in answered one of its actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// What to do next, in order.
    Done(Vec<Action>),
    /// Why it didn't, and what's wrong with which of the form's fields.
    Refused {
        message: String,
        fields: BTreeMap<String, String>,
    },
}

#[cfg(test)]
mod tests {
    use super::{Inline, Row, sorted};

    fn row(id: &str, cell: Inline) -> Row {
        Row {
            id: id.to_owned(),
            cells: vec![cell],
            action: None,
            menu: Vec::new(),
        }
    }

    #[test]
    fn a_sortable_column_orders_by_value_not_by_how_it_reads() {
        let rows = [
            row("big", Inline::Bytes(18_233)),
            row("small", Inline::Bytes(4_210)),
            row(
                "folder",
                Inline::Text {
                    text: String::new(),
                    tone: None,
                    icon: None,
                    monospace: false,
                },
            ),
        ];
        assert_eq!(sorted(&rows, 0, false), [1, 0, 2]);
        assert_eq!(sorted(&rows, 0, true), [2, 0, 1]);
    }
}
