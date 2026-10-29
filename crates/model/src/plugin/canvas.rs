use std::ops::{Add, Mul, Sub};

use crate::plugin::view::{Action, Align};

/// A point in canvas units: logical pixels at zoom 1, y downwards.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn length(self) -> f32 {
        self.x.hypot(self.y)
    }

    pub fn distance(self, other: Self) -> f32 {
        (self - other).length()
    }

    /// The same direction at length 1, or nothing for no direction at all.
    pub fn unit(self) -> Option<Self> {
        let length = self.length();
        (length > f32::EPSILON).then(|| self * (1. / length))
    }
}

impl Add for Point {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }
}

impl Sub for Point {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y)
    }
}

impl Mul<f32> for Point {
    type Output = Self;

    fn mul(self, by: f32) -> Self {
        Self::new(self.x * by, self.y * by)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub origin: Point,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn between(a: Point, b: Point) -> Self {
        Self {
            origin: Point::new(a.x.min(b.x), a.y.min(b.y)),
            width: (a.x - b.x).abs(),
            height: (a.y - b.y).abs(),
        }
    }

    pub fn far(self) -> Point {
        Point::new(self.origin.x + self.width, self.origin.y + self.height)
    }

    pub fn center(self) -> Point {
        Point::new(self.origin.x + self.width / 2., self.origin.y + self.height / 2.)
    }

    pub fn contains(self, point: Point) -> bool {
        let far = self.far();
        (self.origin.x..=far.x).contains(&point.x) && (self.origin.y..=far.y).contains(&point.y)
    }

    pub fn intersects(self, other: Self) -> bool {
        let (far, other_far) = (self.far(), other.far());
        self.origin.x <= other_far.x
            && other.origin.x <= far.x
            && self.origin.y <= other_far.y
            && other.origin.y <= far.y
    }

    #[must_use]
    pub fn union(self, other: Self) -> Self {
        let origin = Point::new(
            self.origin.x.min(other.origin.x),
            self.origin.y.min(other.origin.y),
        );
        let (far, other_far) = (self.far(), other.far());
        Self::between(origin, Point::new(far.x.max(other_far.x), far.y.max(other_far.y)))
    }
}

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

/// The way a connector runs: where it starts, and each piece after.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub start: Point,
    pub pieces: Vec<Piece>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Piece {
    To(Point),
    Curve {
        control_a: Point,
        control_b: Point,
        to: Point,
    },
}

/// How finely a curve is cut into straight pieces to measure and hit it.
const CURVE_STEPS: u16 = 24;
/// How far an orthogonal connector runs straight out of an anchor before it turns.
const STUB: f32 = 16.;

impl Line {
    /// Runs from `start` leaving along `out` to `end` arriving against `into`, as `route` says.
    pub fn routed(route: Route, (start, out): (Point, Point), (end, into): (Point, Point)) -> Self {
        let pieces = match route {
            Route::Straight => vec![Piece::To(end)],
            Route::Curved => {
                let reach = (start.distance(end) * 0.4).max(30.);
                vec![Piece::Curve {
                    control_a: start + out * reach,
                    control_b: end + into * reach,
                    to: end,
                }]
            }
            Route::Orthogonal => {
                let (leave, arrive) = (start + out * STUB, end + into * STUB);
                let bend = if out.y.abs() < f32::EPSILON {
                    let middle = f32::midpoint(leave.x, arrive.x);
                    [Point::new(middle, leave.y), Point::new(middle, arrive.y)]
                } else {
                    let middle = f32::midpoint(leave.y, arrive.y);
                    [Point::new(leave.x, middle), Point::new(arrive.x, middle)]
                };
                [leave, bend[0], bend[1], arrive, end]
                    .into_iter()
                    .map(Piece::To)
                    .collect()
            }
        };
        Self { start, pieces }
    }

    /// The line as straight pieces.
    pub fn points(&self) -> Vec<Point> {
        let mut points = vec![self.start];
        for piece in &self.pieces {
            match *piece {
                Piece::To(to) => points.push(to),
                Piece::Curve {
                    control_a,
                    control_b,
                    to,
                } => {
                    let from = points.last().copied().unwrap_or(self.start);
                    points.extend((1..=CURVE_STEPS).map(|step| {
                        cubic(
                            from,
                            control_a,
                            control_b,
                            to,
                            f32::from(step) / f32::from(CURVE_STEPS),
                        )
                    }));
                }
            }
        }
        points.dedup();
        points
    }

    /// The point halfway along it, where its label goes.
    pub fn middle(&self) -> Point {
        let points = self.points();
        let length: f32 = points.windows(2).map(|pair| pair[0].distance(pair[1])).sum();
        let mut left = length / 2.;
        for pair in points.windows(2) {
            let piece = pair[0].distance(pair[1]);
            if piece >= left && piece > 0. {
                return pair[0] + (pair[1] - pair[0]) * (left / piece);
            }
            left -= piece;
        }
        self.start
    }

    /// The way it heads as it arrives at its end, and as it leaves its start, backwards.
    pub fn heads(&self) -> (Point, Point) {
        let points = self.points();
        let heading = |from: Option<&Point>, to: Option<&Point>| match (from, to) {
            (Some(&from), Some(&to)) => (to - from).unit().unwrap_or(Point::new(1., 0.)),
            _ => Point::new(1., 0.),
        };
        let count = points.len();
        let end = heading(points.get(count.wrapping_sub(2)), points.last());
        let start = heading(points.get(1), points.first());
        (end, start)
    }

    pub fn distance_to(&self, point: Point) -> f32 {
        self.points()
            .windows(2)
            .map(|pair| segment_distance(point, pair[0], pair[1]))
            .fold(f32::INFINITY, f32::min)
    }
}

fn cubic(from: Point, control_a: Point, control_b: Point, to: Point, t: f32) -> Point {
    let rest = 1. - t;
    from * (rest * rest * rest)
        + control_a * (3. * rest * rest * t)
        + control_b * (3. * rest * t * t)
        + to * (t * t * t)
}

fn segment_distance(point: Point, a: Point, b: Point) -> f32 {
    let along = b - a;
    let span = along.x * along.x + along.y * along.y;
    if span <= f32::EPSILON {
        return point.distance(a);
    }
    let t = (((point - a).x * along.x + (point - a).y * along.y) / span).clamp(0., 1.);
    point.distance(a + along * t)
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
