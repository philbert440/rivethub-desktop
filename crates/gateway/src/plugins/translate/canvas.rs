use model::plugin::board;
use model::plugin::canvas::{
    Anchor, Arrow, Connector, Direction, End, Event, Grid, Item, MenuItem, Node, Op, Placed, Reply, Role,
    Route, Scene, Shape, Side, Viewport, Weight,
};
use wire::plugin::canvas::{
    CanvasAnchor, CanvasArrow, CanvasDirection, CanvasEnd, CanvasEvent, CanvasItem, CanvasMenuItem, CanvasOp,
    CanvasPosition, CanvasResponse, CanvasRoute, CanvasScene, CanvasShape, CanvasSide, CanvasWeight,
    ColorRole,
};

use super::view::{action, align};
use super::{optional_units, units};

pub(super) fn scene(scene: CanvasScene) -> Scene {
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

pub(super) fn menu_item(item: CanvasMenuItem) -> MenuItem {
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
