mod geometry;
mod line;

pub use geometry::{Point, Rect};
pub use line::{Line, Piece};

use crate::plugin::view::{Action, Align};

/// What a canvas shows: its items in drawing order, the grid, and where the view starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub items: Vec<Item>,
    pub grid: Option<Grid>,
    pub viewport: Option<Viewport>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub size: f32,
    /// Whether dragged nodes put their top-left on it.
    pub snap: bool,
}

/// The canvas point at the view's top-left, and how far it is zoomed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub zoom: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Node(Node),
    Connector(Connector),
}

impl Item {
    pub fn id(&self) -> &str {
        match self {
            Self::Node(node) => &node.id,
            Self::Connector(connector) => &connector.id,
        }
    }

    pub fn menu(&self) -> &[MenuItem] {
        match self {
            Self::Node(node) => &node.menu,
            Self::Connector(connector) => &connector.menu,
        }
    }

    pub fn tooltip(&self) -> Option<&str> {
        match self {
            Self::Node(node) => node.tooltip.as_deref(),
            Self::Connector(connector) => connector.tooltip.as_deref(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: String,
    pub x: f32,
    pub y: f32,
    /// Its box, which hits, selection and connectors use.
    pub width: f32,
    pub height: f32,
    /// Drawn from the node's top-left.
    pub shapes: Vec<Shape>,
    pub anchors: Vec<Anchor>,
    pub tooltip: Option<String>,
    pub menu: Vec<MenuItem>,
    /// Can't be dragged.
    pub locked: bool,
    pub decorative: bool,
}

impl Node {
    pub fn bounds(&self) -> Rect {
        Rect {
            origin: Point::new(self.x, self.y),
            width: self.width,
            height: self.height,
        }
    }

    pub fn anchor(&self, id: &str) -> Option<&Anchor> {
        self.anchors.iter().find(|anchor| anchor.id == id)
    }

    pub fn anchor_point(&self, anchor: &Anchor) -> Point {
        Point::new(self.x + anchor.x, self.y + anchor.y)
    }

    /// Where a connector without an anchor meets the box, heading for `toward`, and the way out of
    /// the side it leaves by.
    fn edge_toward(&self, toward: Point) -> (Point, Point) {
        let bounds = self.bounds();
        let center = bounds.center();
        let heading = toward - center;
        let half = Point::new(self.width.max(1.) / 2., self.height.max(1.) / 2.);
        let side = if (heading.x / half.x).abs() >= (heading.y / half.y).abs() {
            if heading.x >= 0. { Side::Right } else { Side::Left }
        } else if heading.y >= 0. {
            Side::Bottom
        } else {
            Side::Top
        };
        let point = match side {
            Side::Right => Point::new(bounds.far().x, center.y),
            Side::Left => Point::new(bounds.origin.x, center.y),
            Side::Bottom => Point::new(center.x, bounds.far().y),
            Side::Top => Point::new(center.x, bounds.origin.y),
        };
        (point, side.outward())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Connector {
    pub id: String,
    pub from: End,
    pub to: End,
    pub route: Route,
    pub stroke: Option<Role>,
    pub stroke_width: Option<f32>,
    pub dashed: bool,
    pub arrow: Arrow,
    /// Drawn at its middle.
    pub label: Option<String>,
    pub tooltip: Option<String>,
    pub menu: Vec<MenuItem>,
    pub decorative: bool,
}

/// One end of a connector: a node, and the anchor on it, or its box when there is none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct End {
    pub node: String,
    pub anchor: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: Option<f32>,
        fill: Option<Role>,
        stroke: Option<Role>,
        stroke_width: Option<f32>,
    },
    Ellipse {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        fill: Option<Role>,
        stroke: Option<Role>,
        stroke_width: Option<f32>,
    },
    Text {
        x: f32,
        y: f32,
        text: String,
        size: Option<f32>,
        weight: Weight,
        color: Option<Role>,
        align: Align,
        /// Past this it ends in an ellipsis.
        max_width: Option<f32>,
        monospace: bool,
    },
    /// SVG path data.
    Path {
        d: String,
        fill: Option<Role>,
        stroke: Option<Role>,
        stroke_width: Option<f32>,
        dashed: bool,
    },
    Icon {
        x: f32,
        y: f32,
        name: String,
        size: Option<f32>,
        color: Option<Role>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Anchor {
    pub id: String,
    pub x: f32,
    pub y: f32,
    /// Where a connector leaves it.
    pub side: Side,
    pub direction: Direction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

impl Side {
    fn outward(self) -> Point {
        match self {
            Self::Top => Point::new(0., -1.),
            Self::Right => Point::new(1., 0.),
            Self::Bottom => Point::new(0., 1.),
            Self::Left => Point::new(-1., 0.),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    In,
    Out,
    Both,
}

impl Direction {
    pub fn starts(self) -> bool {
        matches!(self, Self::Out | Self::Both)
    }

    pub fn ends(self) -> bool {
        matches!(self, Self::In | Self::Both)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Straight,
    Curved,
    Orthogonal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrow {
    None,
    End,
    Start,
    Both,
}

impl Arrow {
    pub fn at_end(self) -> bool {
        matches!(self, Self::End | Self::Both)
    }

    pub fn at_start(self) -> bool {
        matches!(self, Self::Start | Self::Both)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Weight {
    #[default]
    Regular,
    Medium,
    Bold,
}

/// A colour by its part in the theme, never a literal, so a canvas follows the app's theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Surface,
    SurfaceContainer,
    SurfaceContainerHigh,
    Primary,
    PrimaryContainer,
    OnPrimary,
    OnPrimaryContainer,
    SecondaryContainer,
    OnSurface,
    OnSurfaceVariant,
    Outline,
    OutlineVariant,
    Info,
    Success,
    Warning,
    Danger,
    Category1,
    Category2,
    Category3,
    Category4,
    Category5,
    Category6,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub id: String,
    pub label: String,
    pub icon: Option<String>,
    pub danger: bool,
}

/// A result of the person's work on a canvas, which the plug-in is told.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Selected(Vec<String>),
    /// Where nodes were dropped, at the end of a drag.
    Moved(Vec<Placed>),
    Connected {
        from: End,
        to: End,
    },
    Deleted(Vec<String>),
    Activated(String),
    /// A menu entry chosen on an item, or on empty space when `item` is `None`, at `at`.
    Menu {
        item: Option<String>,
        choice: String,
        at: Point,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub id: String,
    pub at: Point,
}

/// A change the plug-in makes to the scene.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Adds the item, or replaces the one with its id.
    Upsert(Item),
    Remove(Vec<String>),
}

/// The plug-in's answer to an event: changes to the scene, then what to do next.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Reply {
    pub ops: Vec<Op>,
    pub actions: Vec<Action>,
}

impl Scene {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.items.iter().find_map(|item| match item {
            Item::Node(node) if node.id == id => Some(node),
            _ => None,
        })
    }

    pub fn item(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|item| item.id() == id)
    }

    /// Where `end` meets its node, heading for `toward`, and the way out of it.
    pub fn end_point(&self, end: &End, toward: Point) -> Option<(Point, Point)> {
        let node = self.node(&end.node)?;
        match end.anchor.as_deref().and_then(|anchor| node.anchor(anchor)) {
            Some(anchor) => Some((node.anchor_point(anchor), anchor.side.outward())),
            None => Some(node.edge_toward(toward)),
        }
    }

    /// The line between two ends, when both nodes are in the scene.
    pub fn line_between(&self, route: Route, from: &End, to: &End) -> Option<Line> {
        let from_center = self.node(&from.node)?.bounds().center();
        let to_center = self.node(&to.node)?.bounds().center();
        let start = self.end_point(from, to_center)?;
        let end = self.end_point(to, from_center)?;
        Some(Line::routed(route, start, end))
    }

    pub fn line(&self, connector: &Connector) -> Option<Line> {
        self.line_between(connector.route, &connector.from, &connector.to)
    }

    /// The box around every node, or `None` for a scene with none.
    pub fn extent(&self) -> Option<Rect> {
        self.items
            .iter()
            .filter_map(|item| match item {
                Item::Node(node) => Some(node.bounds()),
                Item::Connector(_) => None,
            })
            .reduce(Rect::union)
    }
}
