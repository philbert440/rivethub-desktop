use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use model::TopicId;
use model::plugin::board;
use model::plugin::canvas::{
    Anchor, Arrow, Connector, Direction, End, Event, Grid, Item, MenuItem, Node, Op, Placed, Reply, Role,
    Route, Scene, Shape, Side, Viewport, Weight,
};
use model::plugin::view::{
    Action, Align, Block, Button, ButtonStyle, CanvasBlock, Choice, Column, Confirm, Crumb, Empty, Field,
    FileValue, Form, Inline, Input, ListItem, Property, Row, Tab, TimeStyle, Tone, Value, View,
};
use model::plugin::{
    Context, PageEntry, Params, Permission, Plugin, PluginRef, PluginState, SessionAction, TopicCard,
};
use wire::JsonObject;
use wire::plugin::action::ViewAction;
use wire::plugin::canvas::{
    CanvasAnchor, CanvasArrow, CanvasDirection, CanvasEnd, CanvasEvent, CanvasItem, CanvasMenuItem, CanvasOp,
    CanvasPosition, CanvasResponse, CanvasRoute, CanvasScene, CanvasShape, CanvasSide, CanvasWeight,
    ColorRole,
};
use wire::plugin::host::{PluginRecord, PluginState as WireState};
use wire::plugin::manifest::Permission as WirePermission;
use wire::plugin::view::{
    ViewAlign, ViewBlock, ViewButton, ViewButtonStyle, ViewContext, ViewField, ViewFile, ViewInline,
    ViewInput, ViewTimeStyle, ViewTone, ViewValue,
};

use crate::{GatewayError, translate};

/// A length or position in canvas units; scenes are logical pixels, far inside f32's range.
fn units(value: f64) -> f32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "canvas units are logical pixels, far inside f32's range"
    )]
    let units = value as f32;
    units
}

fn optional_units(value: Option<f64>) -> Option<f32> {
    value.map(units)
}

pub fn plugin(record: PluginRecord) -> Plugin {
    let manifest = record.manifest;
    let contributes = manifest.contributes;
    Plugin {
        at: PluginRef {
            node: record.node,
            id: record.id.into(),
        },
        name: manifest.name,
        version: manifest.version,
        description: manifest.description,
        icon: manifest.icon,
        state: match record.state {
            WireState::Disabled => PluginState::Disabled,
            WireState::AwaitingApproval => PluginState::AwaitingApproval,
            WireState::Starting => PluginState::Starting,
            WireState::Running => PluginState::Running,
            WireState::Backoff => PluginState::Backoff,
            WireState::Invalid => PluginState::Invalid,
        },
        error: record.error,
        granted: record.granted.into_iter().map(permission).collect(),
        pending: record.pending.into_iter().map(permission).collect(),
        settings: manifest.settings.into_iter().map(field).collect(),
        tools: manifest
            .tools
            .into_iter()
            .map(|tool| tool.title.unwrap_or_else(|| tool.name.into()))
            .collect(),
        pages: contributes
            .pages
            .into_iter()
            .map(|page| PageEntry {
                id: page.id.into(),
                title: page.title,
                icon: page.icon,
                view: page.view.into(),
            })
            .collect(),
        topic_cards: contributes
            .topic_cards
            .into_iter()
            .map(|card| TopicCard {
                id: card.id.into(),
                title: card.title,
                view: card.view.into(),
            })
            .collect(),
        session_actions: contributes
            .session_actions
            .into_iter()
            .map(|entry| SessionAction {
                id: entry.id.into(),
                title: entry.title,
                icon: entry.icon,
                action: action(entry.action),
            })
            .collect(),
    }
}

fn permission(permission: WirePermission) -> Permission {
    match permission {
        WirePermission::MemoryRead => Permission::MemoryRead,
        WirePermission::MemoryWrite => Permission::MemoryWrite,
        WirePermission::SessionsRead => Permission::SessionsRead,
        WirePermission::SessionsControl => Permission::SessionsControl,
        WirePermission::TopicsRead => Permission::TopicsRead,
        WirePermission::TopicsWrite => Permission::TopicsWrite,
        WirePermission::TasksRead => Permission::TasksRead,
        WirePermission::TasksDelegate => Permission::TasksDelegate,
        WirePermission::AgentsRead => Permission::AgentsRead,
        WirePermission::NotificationsSend => Permission::NotificationsSend,
        WirePermission::HostFilesRead => Permission::HostFilesRead,
        WirePermission::HostFilesWrite => Permission::HostFilesWrite,
        WirePermission::HostNetwork => Permission::HostNetwork,
        WirePermission::HostExec => Permission::HostExec,
    }
}

pub fn wire_permission(permission: Permission) -> WirePermission {
    match permission {
        Permission::MemoryRead => WirePermission::MemoryRead,
        Permission::MemoryWrite => WirePermission::MemoryWrite,
        Permission::SessionsRead => WirePermission::SessionsRead,
        Permission::SessionsControl => WirePermission::SessionsControl,
        Permission::TopicsRead => WirePermission::TopicsRead,
        Permission::TopicsWrite => WirePermission::TopicsWrite,
        Permission::TasksRead => WirePermission::TasksRead,
        Permission::TasksDelegate => WirePermission::TasksDelegate,
        Permission::AgentsRead => WirePermission::AgentsRead,
        Permission::NotificationsSend => WirePermission::NotificationsSend,
        Permission::HostFilesRead => WirePermission::HostFilesRead,
        Permission::HostFilesWrite => WirePermission::HostFilesWrite,
        Permission::HostNetwork => WirePermission::HostNetwork,
        Permission::HostExec => WirePermission::HostExec,
    }
}

fn params(params: Option<JsonObject>) -> Option<Params> {
    params.and_then(|params| serde_json::to_string(&params).ok().map(Params::from_json))
}

pub fn wire_params(params: Option<&Params>) -> Result<Option<JsonObject>, GatewayError> {
    params
        .map(|params| {
            serde_json::from_str(params.as_json())
                .map_err(|err| GatewayError::Refused(format!("a plug-in's params: {err}")))
        })
        .transpose()
}

pub fn wire_context(context: &Context) -> Result<ViewContext, GatewayError> {
    Ok(ViewContext {
        topic_id: context.topic.as_ref().map(|topic| topic.as_str().to_owned()),
        session_id: context
            .session
            .as_ref()
            .map(translate::wire_session)
            .transpose()?,
    })
}

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
        ViewBlock::WebPanel { .. } => Block::WebPanel,
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

fn align(align: ViewAlign) -> Align {
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

fn field(field: ViewField) -> Field {
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

fn scene(scene: CanvasScene) -> Scene {
    Scene {
        items: scene.items.into_iter().map(item).collect(),
        grid: scene.grid.map(|grid| Grid {
            size: units(grid.size),
            snap: grid.snap,
        }),
        viewport: scene.viewport.map(|view| Viewport {
            x: units(view.x),
            y: units(view.y),
            zoom: units(view.zoom).clamp(board::MIN_ZOOM, board::MAX_ZOOM),
        }),
    }
}

fn item(item: CanvasItem) -> Item {
    match item {
        CanvasItem::Node {
            id,
            x,
            y,
            width,
            height,
            shapes,
            anchors,
            tooltip,
            menu,
            locked,
            decorative,
        } => Item::Node(Node {
            id,
            x: units(x),
            y: units(y),
            width: units(width),
            height: units(height),
            shapes: shapes.into_iter().map(shape).collect(),
            anchors: anchors.into_iter().map(anchor).collect(),
            tooltip,
            menu: menu.into_iter().map(menu_item).collect(),
            locked,
            decorative,
        }),
        CanvasItem::Connector {
            id,
            from,
            to,
            route,
            stroke,
            stroke_width,
            dashed,
            arrow,
            label,
            tooltip,
            menu,
            decorative,
        } => Item::Connector(Connector {
            id,
            from: end(from),
            to: end(to),
            route: match route {
                CanvasRoute::Straight => Route::Straight,
                CanvasRoute::Curved => Route::Curved,
                CanvasRoute::Orthogonal => Route::Orthogonal,
            },
            stroke: stroke.map(role),
            stroke_width: optional_units(stroke_width),
            dashed,
            arrow: match arrow {
                CanvasArrow::None => Arrow::None,
                CanvasArrow::End => Arrow::End,
                CanvasArrow::Start => Arrow::Start,
                CanvasArrow::Both => Arrow::Both,
            },
            label,
            tooltip,
            menu: menu.into_iter().map(menu_item).collect(),
            decorative,
        }),
    }
}

fn shape(shape: CanvasShape) -> Shape {
    match shape {
        CanvasShape::Rect {
            x,
            y,
            width,
            height,
            radius,
            fill,
            stroke,
            stroke_width,
        } => Shape::Rect {
            x: units(x),
            y: units(y),
            width: units(width),
            height: units(height),
            radius: optional_units(radius),
            fill: fill.map(role),
            stroke: stroke.map(role),
            stroke_width: optional_units(stroke_width),
        },
        CanvasShape::Ellipse {
            x,
            y,
            width,
            height,
            fill,
            stroke,
            stroke_width,
        } => Shape::Ellipse {
            x: units(x),
            y: units(y),
            width: units(width),
            height: units(height),
            fill: fill.map(role),
            stroke: stroke.map(role),
            stroke_width: optional_units(stroke_width),
        },
        CanvasShape::Text {
            x,
            y,
            text,
            size,
            weight,
            color,
            align: placed,
            max_width,
            monospace,
        } => Shape::Text {
            x: units(x),
            y: units(y),
            text,
            size: optional_units(size),
            weight: match weight {
                Some(CanvasWeight::Medium) => Weight::Medium,
                Some(CanvasWeight::Bold) => Weight::Bold,
                Some(CanvasWeight::Regular) | None => Weight::Regular,
            },
            color: color.map(role),
            align: placed.map(align).unwrap_or_default(),
            max_width: optional_units(max_width),
            monospace,
        },
        CanvasShape::Path {
            d,
            fill,
            stroke,
            stroke_width,
            dashed,
        } => Shape::Path {
            d,
            fill: fill.map(role),
            stroke: stroke.map(role),
            stroke_width: optional_units(stroke_width),
            dashed,
        },
        CanvasShape::Icon {
            x,
            y,
            name,
            size,
            color,
        } => Shape::Icon {
            x: units(x),
            y: units(y),
            name,
            size: optional_units(size),
            color: color.map(role),
        },
    }
}

fn anchor(anchor: CanvasAnchor) -> Anchor {
    Anchor {
        id: anchor.id,
        x: units(anchor.x),
        y: units(anchor.y),
        side: match anchor.side {
            CanvasSide::Top => Side::Top,
            CanvasSide::Right => Side::Right,
            CanvasSide::Bottom => Side::Bottom,
            CanvasSide::Left => Side::Left,
        },
        direction: match anchor.direction {
            CanvasDirection::In => Direction::In,
            CanvasDirection::Out => Direction::Out,
            CanvasDirection::Both => Direction::Both,
        },
    }
}

fn end(end: CanvasEnd) -> End {
    End {
        node: end.node,
        anchor: end.anchor,
    }
}

fn wire_end(end: &End) -> CanvasEnd {
    CanvasEnd {
        node: end.node.clone(),
        anchor: end.anchor.clone(),
    }
}

fn menu_item(item: CanvasMenuItem) -> MenuItem {
    MenuItem {
        id: item.id,
        label: item.label,
        icon: item.icon,
        danger: item.danger,
    }
}

fn role(role: ColorRole) -> Role {
    match role {
        ColorRole::Surface => Role::Surface,
        ColorRole::SurfaceContainer => Role::SurfaceContainer,
        ColorRole::SurfaceContainerHigh => Role::SurfaceContainerHigh,
        ColorRole::Primary => Role::Primary,
        ColorRole::PrimaryContainer => Role::PrimaryContainer,
        ColorRole::OnPrimary => Role::OnPrimary,
        ColorRole::OnPrimaryContainer => Role::OnPrimaryContainer,
        ColorRole::SecondaryContainer => Role::SecondaryContainer,
        ColorRole::OnSurface => Role::OnSurface,
        ColorRole::OnSurfaceVariant => Role::OnSurfaceVariant,
        ColorRole::Outline => Role::Outline,
        ColorRole::OutlineVariant => Role::OutlineVariant,
        ColorRole::Info => Role::Info,
        ColorRole::Success => Role::Success,
        ColorRole::Warning => Role::Warning,
        ColorRole::Danger => Role::Danger,
        ColorRole::Category1 => Role::Category1,
        ColorRole::Category2 => Role::Category2,
        ColorRole::Category3 => Role::Category3,
        ColorRole::Category4 => Role::Category4,
        ColorRole::Category5 => Role::Category5,
        ColorRole::Category6 => Role::Category6,
    }
}

pub fn wire_event(event: &Event) -> CanvasEvent {
    match event {
        Event::Selected(items) => CanvasEvent::Selected { items: items.clone() },
        Event::Moved(placed) => CanvasEvent::Moved {
            items: placed.iter().map(position).collect(),
        },
        Event::Connected { from, to } => CanvasEvent::Connected {
            from: wire_end(from),
            to: wire_end(to),
        },
        Event::Deleted(items) => CanvasEvent::Deleted { items: items.clone() },
        Event::Activated(item) => CanvasEvent::Activated { item: item.clone() },
        Event::Menu { item, choice, at } => CanvasEvent::Menu {
            item: item.clone(),
            choice: choice.clone(),
            x: f64::from(at.x),
            y: f64::from(at.y),
        },
    }
}

fn position(placed: &Placed) -> CanvasPosition {
    CanvasPosition {
        id: placed.id.clone(),
        x: f64::from(placed.at.x),
        y: f64::from(placed.at.y),
    }
}

pub fn reply(response: CanvasResponse) -> Reply {
    Reply {
        ops: response
            .ops
            .into_iter()
            .map(|op| match op {
                CanvasOp::Upsert { item: changed } => Op::Upsert(item(changed)),
                CanvasOp::Remove { ids } => Op::Remove(ids),
            })
            .collect(),
        actions: response.actions.into_iter().map(action).collect(),
    }
}
