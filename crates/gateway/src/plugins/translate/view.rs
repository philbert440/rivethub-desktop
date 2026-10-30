use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use model::TopicId;
use model::plugin::view::{
    Action, Align, Block, Button, ButtonStyle, CanvasBlock, Choice, Column, Confirm, Crumb, Empty, Field,
    FileValue, Form, Inline, Input, ListItem, Property, Row, Tab, TimeStyle, Tone, Value, View,
    WebPanelBlock,
};
use wire::plugin::action::ViewAction;
use wire::plugin::view::{
    ViewAlign, ViewBlock, ViewButton, ViewButtonStyle, ViewField, ViewFile, ViewInline, ViewInput,
    ViewTimeStyle, ViewTone, ViewValue,
};

use super::canvas::{menu_item, scene};
use super::{optional_units, params, units};
use crate::translate;

pub fn view(view: wire::plugin::view::View) -> View {
    View {
        title: view.title,
        subtitle: view.subtitle,
        toolbar: buttons(view.toolbar),
        blocks: blocks(view.blocks),
    }
}

fn blocks(blocks: Vec<ViewBlock>) -> Vec<Block> {
    blocks.into_iter().map(block).collect()
}

#[expect(
    clippy::too_many_lines,
    reason = "one arm for each of the contract's block types"
)]
fn block(block: ViewBlock) -> Block {
    match block {
        ViewBlock::Section {
            title,
            subtitle,
            actions,
            blocks: inner,
            collapsed,
        } => Block::Section {
            title,
            subtitle,
            actions: buttons(actions),
            blocks: blocks(inner),
            collapsed,
        },
        ViewBlock::Split {
            primary,
            secondary,
            ratio,
        } => Block::Split {
            primary: blocks(primary),
            secondary: blocks(secondary),
            ratio: optional_units(ratio),
        },
        ViewBlock::Tabs { tabs, selected } => Block::Tabs {
            tabs: tabs.into_iter().map(tab).collect(),
            selected: selected.map(Into::into),
        },
        ViewBlock::Text {
            text,
            tone: shade,
            monospace,
        } => Block::Text {
            text,
            tone: shade.map(tone),
            monospace,
        },
        ViewBlock::Markdown { text } => Block::Markdown(text),
        ViewBlock::Code {
            text,
            language,
            follow,
        } => Block::Code {
            text,
            language,
            follow,
        },
        ViewBlock::Image { path, alt } => Block::Image { path, alt },
        ViewBlock::Properties { items } => Block::Properties(items.into_iter().map(property).collect()),
        ViewBlock::Progress { value, label } => Block::Progress {
            value: optional_units(value),
            label,
        },
        ViewBlock::Notice {
            tone: shade,
            title,
            text,
            actions,
        } => Block::Notice {
            tone: tone(shade),
            title,
            text,
            actions: buttons(actions),
        },
        ViewBlock::List { items, empty: none } => Block::List {
            items: items.into_iter().map(list_item).collect(),
            empty: none.map(empty),
        },
        ViewBlock::Table {
            columns,
            rows,
            empty: none,
        } => Block::Table {
            columns: columns.into_iter().map(column).collect(),
            rows: rows.into_iter().map(row).collect(),
            empty: none.map(empty),
        },
        ViewBlock::Breadcrumbs { items } => Block::Breadcrumbs(items.into_iter().map(crumb).collect()),
        ViewBlock::Actions { buttons: row } => Block::Actions(buttons(row)),
        ViewBlock::Form {
            id,
            fields,
            buttons: row,
        } => Block::Form(Form {
            id: id.into(),
            fields: fields.into_iter().map(field).collect(),
            buttons: buttons(row),
        }),
        ViewBlock::Canvas {
            id,
            scene: drawn,
            height,
            read_only,
            menu,
        } => Block::Canvas(CanvasBlock {
            id: id.into(),
            scene: scene(drawn),
            height: height.map(|height| units(f64::from(height))),
            read_only,
            menu: menu.into_iter().map(menu_item).collect(),
        }),
        ViewBlock::WebPanel {
            id,
            entry,
            height,
            params: given,
        } => Block::WebPanel(WebPanelBlock {
            id: id.into(),
            entry,
            height: height.map(|height| units(f64::from(height))),
            params: params(given),
        }),
    }
}

fn property(item: wire::plugin::view::ViewProperty) -> Property {
    Property {
        label: item.label,
        value: inline(item.value),
    }
}

fn crumb(crumb: wire::plugin::view::ViewCrumb) -> Crumb {
    Crumb {
        label: crumb.label,
        action: crumb.action.map(action),
    }
}

fn tab(tab: wire::plugin::view::ViewTab) -> Tab {
    Tab {
        id: tab.id.into(),
        title: tab.title,
        badge: tab.badge,
        blocks: blocks(tab.blocks),
    }
}

fn list_item(item: wire::plugin::view::ViewListItem) -> ListItem {
    ListItem {
        id: item.id,
        title: item.title,
        subtitle: item.subtitle,
        icon: item.icon,
        trailing: item.trailing.into_iter().map(inline).collect(),
        action: item.action.map(action),
        menu: buttons(item.menu),
    }
}

fn column(column: wire::plugin::view::ViewColumn) -> Column {
    Column {
        title: column.title,
        align: column.align.map(align).unwrap_or_default(),
        sortable: column.sortable,
    }
}

fn row(row: wire::plugin::view::ViewRow) -> Row {
    Row {
        id: row.id,
        cells: row.cells.into_iter().map(inline).collect(),
        action: row.action.map(action),
        menu: buttons(row.menu),
    }
}

fn empty(empty: wire::plugin::view::ViewEmpty) -> Empty {
    Empty {
        title: empty.title,
        text: empty.text,
        button: empty.button.map(button),
    }
}

fn inline(inline: ViewInline) -> Inline {
    match inline {
        ViewInline::Text {
            text,
            tone: shade,
            icon,
            monospace,
        } => Inline::Text {
            text,
            tone: shade.map(tone),
            icon,
            monospace,
        },
        ViewInline::Badge { text, tone: shade } => Inline::Badge {
            text,
            tone: tone(shade),
        },
        ViewInline::Time { at, style } => Inline::Time {
            at_ms: at.timestamp_millis(),
            style: style.map(|style| match style {
                ViewTimeStyle::Relative => TimeStyle::Relative,
                ViewTimeStyle::Absolute => TimeStyle::Absolute,
            }),
        },
        ViewInline::Duration { ms } => Inline::Duration { ms },
        ViewInline::Bytes { value } => Inline::Bytes(value),
    }
}

fn tone(tone: ViewTone) -> Tone {
    match tone {
        ViewTone::Neutral => Tone::Neutral,
        ViewTone::Info => Tone::Info,
        ViewTone::Success => Tone::Success,
        ViewTone::Warning => Tone::Warning,
        ViewTone::Danger => Tone::Danger,
    }
}

pub(super) fn align(align: ViewAlign) -> Align {
    match align {
        ViewAlign::Start => Align::Start,
        ViewAlign::Center => Align::Center,
        ViewAlign::End => Align::End,
    }
}

fn buttons(buttons: Vec<ViewButton>) -> Vec<Button> {
    buttons.into_iter().map(button).collect()
}

fn button(button: ViewButton) -> Button {
    Button {
        label: button.label,
        icon: button.icon,
        style: match button.style {
            Some(ViewButtonStyle::Primary) => ButtonStyle::Primary,
            Some(ViewButtonStyle::Danger) => ButtonStyle::Danger,
            Some(ViewButtonStyle::Secondary) | None => ButtonStyle::Secondary,
        },
        action: action(button.action),
        confirm: button.confirm.map(|confirm| Confirm {
            title: confirm.title,
            text: confirm.text,
            confirm_label: confirm.confirm_label,
        }),
        disabled: button.disabled,
    }
}

pub fn action(action: ViewAction) -> Action {
    match action {
        ViewAction::Call {
            action,
            params: given,
        } => Action::Call {
            action: action.into(),
            params: params(given),
        },
        ViewAction::Navigate {
            view,
            params: given,
            replace,
        } => Action::Navigate {
            view: view.into(),
            params: params(given),
            replace,
        },
        ViewAction::Dialog { view, params: given } => Action::Dialog {
            view: view.into(),
            params: params(given),
        },
        ViewAction::Back => Action::Back,
        ViewAction::Refresh => Action::Refresh,
        ViewAction::Toast { text, tone: shade } => Action::Toast {
            text,
            tone: shade.map(tone).unwrap_or_default(),
        },
        ViewAction::Copy { text } => Action::Copy(text),
        ViewAction::OpenSession { session_id } => Action::OpenSession(translate::session(&session_id)),
        ViewAction::OpenTopic { topic_id } => Action::OpenTopic(TopicId::new(topic_id)),
        ViewAction::OpenUrl { url } => Action::OpenUrl(url.into()),
        ViewAction::Download { path, filename } => Action::Download { path, filename },
    }
}

pub(super) fn field(field: ViewField) -> Field {
    Field {
        key: field.key,
        label: field.label,
        input: match field.input {
            ViewInput::Text { multiline } => Input::Text { multiline },
            ViewInput::Secret => Input::Secret,
            ViewInput::Number { min, max, step } => Input::Number { min, max, step },
            ViewInput::Toggle => Input::Toggle,
            ViewInput::Select { options, multiple } => Input::Select {
                options: options
                    .into_iter()
                    .map(|option| Choice {
                        value: option.value,
                        label: option.label,
                    })
                    .collect(),
                multiple,
            },
            ViewInput::File { multiple, accept } => Input::File { multiple, accept },
        },
        value: field.value.map(value),
        required: field.required,
        help: field.help,
        placeholder: field.placeholder,
    }
}

pub fn value(value: ViewValue) -> Value {
    match value {
        ViewValue::Bool(on) => Value::Bool(on),
        ViewValue::Number(number) => Value::Number(number),
        ViewValue::Text(text) => Value::Text(text),
        ViewValue::List(items) => Value::List(items),
        ViewValue::Files(files) => Value::Files(
            files
                .into_iter()
                .map(|file| FileValue {
                    bytes: STANDARD.decode(&file.data).unwrap_or_default(),
                    name: file.name,
                    media_type: file.mime_type,
                })
                .collect(),
        ),
    }
}

pub fn wire_value(value: &Value) -> ViewValue {
    match value {
        Value::Bool(on) => ViewValue::Bool(*on),
        Value::Number(number) => ViewValue::Number(*number),
        Value::Text(text) => ViewValue::Text(text.clone()),
        Value::List(items) => ViewValue::List(items.clone()),
        Value::Files(files) => ViewValue::Files(
            files
                .iter()
                .map(|file| ViewFile {
                    name: file.name.clone(),
                    mime_type: file.media_type.clone(),
                    data: STANDARD.encode(&file.bytes),
                })
                .collect(),
        ),
    }
}

pub fn wire_values(values: &BTreeMap<String, Value>) -> BTreeMap<String, ViewValue> {
    values
        .iter()
        .map(|(key, value)| (key.clone(), wire_value(value)))
        .collect()
}
