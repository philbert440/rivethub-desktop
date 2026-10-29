use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use app::Hub;
use gpui::{
    BorderStyle, Bounds, ClickEvent, ContentMask, Context, CursorStyle, DispatchPhase, EventEmitter,
    FontWeight, Hitbox, HitboxBehavior, Hsla, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, PathBuilder, PinchEvent, Pixels, Render, ScrollWheelEvent, SharedString,
    Styled, Task, TextAlign, TextRun, TransformationMatrix, Window, anchored, canvas, deferred, div, font,
    point, prelude::*, px, quad, size,
};
use model::plugin::board::{Board, Hit, Outbox};
use model::plugin::canvas::{
    Connector, End, Event, Item, Line, MenuItem, Node, Op, Piece, Placed, Point, Rect, Role, Route, Scene,
    Shape, Viewport, Weight,
};
use model::plugin::view::{Action, Align, CanvasBlock};
use model::plugin::{Context as About, Params, PluginRef};

use crate::brand;
use crate::theme::Theme;

/// A canvas's height when its block doesn't say.
const HEIGHT: f32 = 420.;
/// How near the pointer counts as on an item or an anchor, in screen pixels.
const SLACK: f32 = 6.;
const ANCHOR_SLACK: f32 = 9.;
const ANCHOR_RADIUS: f32 = 5.;
/// How far a press moves before it is a drag, in screen pixels.
const DRAG_START: f32 = 3.;
/// How long the pointer rests on an item before its tooltip shows.
const TOOLTIP_DELAY: Duration = Duration::from_millis(500);
const ZOOM_STEP: f32 = 1.25;

/// Where a canvas's events go: its plug-in, its id, and the params and context of its view.
#[derive(Debug, Clone)]
pub struct CanvasTarget {
    pub plugin: PluginRef,
    pub canvas: String,
    pub params: Option<Params>,
    pub context: About,
}

/// The plug-in answered an event with what to do next.
pub struct Replied(pub Vec<Action>);

enum Gesture {
    /// Dragging empty space; a press that never moves clears the selection.
    Pan {
        last: gpui::Point<Pixels>,
        moved: bool,
    },
    Drag {
        start: gpui::Point<Pixels>,
        from: Vec<Placed>,
        now: Vec<Placed>,
        moved: bool,
    },
    Box {
        start: Point,
        now: Point,
    },
    Connect {
        node: String,
        anchor: String,
        now: Point,
        target: Option<Hit>,
    },
}

struct OpenMenu {
    at: gpui::Point<Pixels>,
    on: Point,
    item: Option<String>,
    entries: Vec<MenuItem>,
}

/// SVG path data, absolute and with arcs as curves, in canvas units from the node's top-left.
#[derive(Debug, Clone, Copy)]
enum Segment {
    Move(Point),
    Line(Point),
    Cubic(Point, Point, Point),
    Quad(Point, Point),
    Close,
}

fn units(value: f64) -> f32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "path data is in canvas units, far inside f32's range"
    )]
    let units = value as f32;
    units
}

fn parse_path(data: &str) -> Rc<[Segment]> {
    use svgtypes::SimplePathSegment as Simple;
    let at = |x: f64, y: f64| Point::new(units(x), units(y));
    svgtypes::SimplifyingPathParser::from(data)
        .map_while(Result::ok)
        .map(|segment| match segment {
            Simple::MoveTo { x, y } => Segment::Move(at(x, y)),
            Simple::LineTo { x, y } => Segment::Line(at(x, y)),
            Simple::CurveTo { x1, y1, x2, y2, x, y } => Segment::Cubic(at(x1, y1), at(x2, y2), at(x, y)),
            Simple::Quadratic { x1, y1, x, y } => Segment::Quad(at(x1, y1), at(x, y)),
            Simple::ClosePath => Segment::Close,
        })
        .collect()
}

/// A plug-in's canvas, drawn by the app: pan, zoom, selection, dragging and connecting happen
/// here, and the plug-in hears only their results.
pub struct CanvasView {
    hub: Hub,
    target: CanvasTarget,
    board: Board,
    read_only: bool,
    menu: Vec<MenuItem>,
    height: f32,
    outbox: Outbox,
    gesture: Option<Gesture>,
    hover: Option<Hit>,
    pointer: Option<gpui::Point<Pixels>>,
    tooltip: bool,
    resting: Option<Task<()>>,
    open_menu: Option<OpenMenu>,
    bounds: Option<Bounds<Pixels>>,
    paths: HashMap<String, Rc<[Segment]>>,
    /// Where the person dropped nodes, and when, so a fetch asked for before a drop doesn't put
    /// them back.
    dropped: Vec<(Placed, Instant)>,
    problem: Option<String>,
}

impl EventEmitter<Replied> for CanvasView {}

impl CanvasView {
    pub fn new(hub: Hub, block: &CanvasBlock, target: CanvasTarget) -> Self {
        Self {
            hub,
            target,
            board: Board::new(block.scene.clone()),
            read_only: block.read_only,
            menu: block.menu.clone(),
            height: block.height.unwrap_or(HEIGHT),
            outbox: Outbox::default(),
            gesture: None,
            hover: None,
            pointer: None,
            tooltip: false,
            resting: None,
            open_menu: None,
            bounds: None,
            paths: HashMap::new(),
            dropped: Vec::new(),
            problem: None,
        }
    }

    /// Takes the canvas a fetch of its view, asked for at `asked`, brought, keeping where the view
    /// is and what's selected. Nodes dropped since it was asked for, or being dragged, stay where
    /// the person put them.
    pub fn renew(
        &mut self,
        block: &CanvasBlock,
        target: CanvasTarget,
        asked: Instant,
        cx: &mut Context<Self>,
    ) {
        self.board.renew(block.scene.clone());
        self.dropped.retain(|(_, at)| *at > asked);
        let kept: Vec<Placed> = self.dropped.iter().map(|(spot, _)| spot.clone()).collect();
        self.board.place(&kept);
        if let Some(Gesture::Drag { now, .. }) = &self.gesture {
            self.board.place(now);
        }
        self.read_only = block.read_only;
        self.menu.clone_from(&block.menu);
        self.height = block.height.unwrap_or(HEIGHT);
        self.target = target;
        cx.notify();
    }

    fn size(&self) -> Point {
        self.bounds.map_or(Point::new(800., self.height), |bounds| {
            Point::new(f32::from(bounds.size.width), f32::from(bounds.size.height))
        })
    }

    fn zoom(&self) -> f32 {
        self.board.viewport().map_or(1., |view| view.zoom)
    }

    /// The pointer's place in screen pixels from the canvas's top-left.
    fn local(&self, at: gpui::Point<Pixels>) -> Point {
        let origin = self.bounds.map_or(at, |bounds| bounds.origin);
        Point::new(f32::from(at.x - origin.x), f32::from(at.y - origin.y))
    }

    fn tell(&mut self, event: Event, cx: &mut Context<Self>) {
        self.outbox.push(event);
        self.send(cx);
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let Some(event) = self.outbox.take() else {
            return;
        };
        let hub = self.hub.clone();
        let target = self.target.clone();
        cx.spawn(async move |this, cx| {
            let replied = hub
                .canvas_event(
                    target.plugin,
                    target.canvas,
                    event.clone(),
                    target.params,
                    target.context,
                )
                .await;
            this.update(cx, |this, cx| {
                this.outbox.answered();
                this.board.settle(&event);
                match replied {
                    Ok(reply) => {
                        this.problem = None;
                        for op in &reply.ops {
                            if let Op::Upsert(item) = op {
                                this.dropped.retain(|(spot, _)| spot.id != item.id());
                            }
                        }
                        this.board.apply(reply.ops);
                        if !reply.actions.is_empty() {
                            cx.emit(Replied(reply.actions));
                        }
                    }
                    Err(err) => this.problem = Some(err.to_string()),
                }
                this.send(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn down(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        if self.open_menu.take().is_some() {
            cx.notify();
            return;
        }
        let local = self.local(event.position);
        let at = self.board.to_canvas(local);
        let zoom = self.zoom();
        let hit = self
            .board
            .hit(at, ANCHOR_SLACK / zoom, !self.read_only)
            .filter(|hit| matches!(hit, Hit::Anchor { .. }))
            .or_else(|| self.board.hit(at, SLACK / zoom, false));
        if event.button == MouseButton::Right {
            self.open_menu_at(event.position, at, hit.as_ref(), cx);
            return;
        }
        if event.button != MouseButton::Left {
            return;
        }
        let toggle = event.modifiers.shift || event.modifiers.platform;
        match hit {
            Some(Hit::Anchor { node, anchor }) if self.board.starts_connectors(&node, &anchor) => {
                self.gesture = Some(Gesture::Connect {
                    node,
                    anchor,
                    now: at,
                    target: None,
                });
            }
            Some(hit) if event.click_count >= 2 => self.tell(Event::Activated(hit.item().to_owned()), cx),
            Some(hit) => {
                let id = hit.item().to_owned();
                if toggle || !self.board.is_selected(&id) {
                    self.board.select(&id, toggle);
                    self.tell(Event::Selected(self.board.selection().to_vec()), cx);
                }
                if !toggle && !self.read_only && matches!(hit, Hit::Node(_) | Hit::Anchor { .. }) {
                    let from = self.board.draggable();
                    self.gesture = Some(Gesture::Drag {
                        start: event.position,
                        now: from.clone(),
                        from,
                        moved: false,
                    });
                }
            }
            None if event.modifiers.shift => self.gesture = Some(Gesture::Box { start: at, now: at }),
            None => {
                self.gesture = Some(Gesture::Pan {
                    last: event.position,
                    moved: false,
                });
            }
        }
        cx.notify();
    }

    fn open_menu_at(
        &mut self,
        at: gpui::Point<Pixels>,
        on: Point,
        hit: Option<&Hit>,
        cx: &mut Context<Self>,
    ) {
        let item = hit.map(|hit| hit.item().to_owned());
        let entries = match &item {
            Some(id) => self
                .board
                .scene()
                .item(id)
                .map(|item| item.menu().to_vec())
                .unwrap_or_default(),
            None => self.menu.clone(),
        };
        if entries.is_empty() {
            return;
        }
        if let Some(id) = item.as_deref()
            && !self.board.is_selected(id)
        {
            self.board.select(id, false);
            self.tell(Event::Selected(self.board.selection().to_vec()), cx);
        }
        self.open_menu = Some(OpenMenu {
            at,
            on,
            item,
            entries,
        });
        cx.notify();
    }

    fn moved(&mut self, event: &MouseMoveEvent, over: bool, cx: &mut Context<Self>) {
        let local = self.local(event.position);
        let at = self.board.to_canvas(local);
        let zoom = self.zoom();
        match self.gesture.as_mut() {
            Some(Gesture::Pan { last, moved }) => {
                let by = event.position - *last;
                *last = event.position;
                *moved = true;
                self.board.pan(Point::new(f32::from(by.x), f32::from(by.y)));
            }
            Some(Gesture::Drag {
                start,
                from,
                now,
                moved,
            }) => {
                let by = event.position - *start;
                let by = Point::new(f32::from(by.x), f32::from(by.y));
                if !*moved && by.length() < DRAG_START {
                    return;
                }
                *moved = true;
                *now = self.board.dragged(from, by * (1. / zoom));
                self.board.place(now);
            }
            Some(Gesture::Box { now, .. }) => *now = at,
            Some(Gesture::Connect { now, target, .. }) => {
                *now = at;
                *target = self
                    .board
                    .hit(at, ANCHOR_SLACK / zoom, true)
                    .filter(|hit| matches!(hit, Hit::Anchor { .. }))
                    .or_else(|| self.board.hit(at, 0., false));
            }
            None => {
                let pointer = over.then_some(event.position);
                let hover = over
                    .then(|| {
                        self.board
                            .hit(at, ANCHOR_SLACK / zoom, !self.read_only)
                            .filter(|hit| matches!(hit, Hit::Anchor { .. }))
                            .or_else(|| self.board.hit(at, SLACK / zoom, false))
                    })
                    .flatten();
                let tooltip_moves = self.tooltip && pointer != self.pointer;
                self.pointer = pointer;
                if hover != self.hover {
                    self.hover = hover;
                    self.rest(cx);
                    cx.notify();
                } else if tooltip_moves {
                    cx.notify();
                }
                return;
            }
        }
        cx.notify();
    }

    /// Shows the hovered item's tooltip once the pointer has rested on it.
    fn rest(&mut self, cx: &mut Context<Self>) {
        self.tooltip = false;
        let resting = self.hover.clone();
        self.resting = resting.is_some().then(|| {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(TOOLTIP_DELAY).await;
                this.update(cx, |this, cx| {
                    if this.hover == resting {
                        this.tooltip = true;
                        cx.notify();
                    }
                })
                .ok();
            })
        });
    }

    fn up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        if event.button != MouseButton::Left {
            return;
        }
        let Some(gesture) = self.gesture.take() else {
            return;
        };
        match gesture {
            Gesture::Pan { moved: false, .. } => {
                if self.board.clear_selection() {
                    self.tell(Event::Selected(Vec::new()), cx);
                }
            }
            Gesture::Drag {
                from,
                now,
                moved: true,
                ..
            } => {
                let dropped: Vec<Placed> = now.into_iter().filter(|spot| !from.contains(spot)).collect();
                if !dropped.is_empty() {
                    let at = Instant::now();
                    self.dropped
                        .retain(|(spot, _)| !dropped.iter().any(|drop| drop.id == spot.id));
                    self.dropped.extend(dropped.iter().map(|spot| (spot.clone(), at)));
                    self.tell(Event::Moved(dropped), cx);
                }
            }
            Gesture::Pan { .. } | Gesture::Drag { .. } => {}
            Gesture::Box { start, now } => {
                if self.board.select_within(Rect::between(start, now), false) {
                    self.tell(Event::Selected(self.board.selection().to_vec()), cx);
                }
            }
            Gesture::Connect {
                node, anchor, target, ..
            } => {
                if let Some(to) = self.board.connection((&node, &anchor), target.as_ref()) {
                    let from = End {
                        node,
                        anchor: Some(anchor),
                    };
                    self.board.propose(from.clone(), to.clone());
                    self.tell(Event::Connected { from, to }, cx);
                }
            }
        }
        cx.notify();
    }

    fn scrolled(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(px(20.));
        let by = Point::new(f32::from(delta.x), f32::from(delta.y));
        if event.modifiers.control || event.modifiers.platform {
            self.board
                .zoom_at(self.local(event.position), (by.y * 0.002).exp());
        } else {
            self.board.pan(by);
        }
        cx.notify();
    }

    fn pinched(&mut self, event: &PinchEvent, cx: &mut Context<Self>) {
        self.board.zoom_at(self.local(event.position), 1. + event.delta);
        cx.notify();
    }

    fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        let center = self.size() * 0.5;
        self.board.zoom_at(center, factor);
        cx.notify();
    }

    fn choose(&mut self, choice: String, cx: &mut Context<Self>) {
        let Some(menu) = self.open_menu.take() else {
            return;
        };
        self.tell(
            Event::Menu {
                item: menu.item,
                choice,
                at: menu.on,
            },
            cx,
        );
        cx.notify();
    }

    /// What one frame draws, taken out of the view so painting can use the app.
    fn picture(&mut self, bounds: Bounds<Pixels>) -> Picture {
        self.bounds = Some(bounds);
        let view = self.board.view_in(self.size());
        let mut paths = HashMap::new();
        for item in &self.board.scene().items {
            if let Item::Node(node) = item {
                for shape in &node.shapes {
                    if let Shape::Path { d, .. } = shape {
                        let parsed = self.paths.entry(d.clone()).or_insert_with(|| parse_path(d));
                        paths.insert(d.clone(), Rc::clone(parsed));
                    }
                }
            }
        }
        self.paths.retain(|d, _| paths.contains_key(d));
        let overlay = match &self.gesture {
            Some(Gesture::Box { start, now }) => Overlay::Box(Rect::between(*start, *now)),
            Some(Gesture::Connect {
                node,
                anchor,
                now,
                target,
            }) => Overlay::Connecting {
                from: End {
                    node: node.clone(),
                    anchor: Some(anchor.clone()),
                },
                to: *now,
                target: self.board.connection((node, anchor), target.as_ref()),
            },
            _ => Overlay::None,
        };
        let cursor = match &self.gesture {
            Some(Gesture::Pan { moved: true, .. } | Gesture::Drag { moved: true, .. }) => {
                CursorStyle::ClosedHand
            }
            Some(Gesture::Connect { .. }) => CursorStyle::Crosshair,
            _ => match &self.hover {
                Some(Hit::Anchor { .. }) if !self.read_only => CursorStyle::Crosshair,
                Some(Hit::Node(id))
                    if !self.read_only && self.board.scene().node(id).is_some_and(|node| !node.locked) =>
                {
                    CursorStyle::OpenHand
                }
                Some(_) => CursorStyle::PointingHand,
                None => CursorStyle::Arrow,
            },
        };
        Picture {
            scene: self.board.scene().clone(),
            selection: self.board.selection().to_vec(),
            view,
            provisional: self.board.provisional().to_vec(),
            hover: self.hover.clone(),
            overlay,
            anchors: !self.read_only,
            grid: self.board.scene().grid.map(|grid| grid.size),
            paths,
            cursor,
        }
    }
}

enum Overlay {
    None,
    Box(Rect),
    /// A connector being drawn, and the end it would have if dropped now.
    Connecting {
        from: End,
        to: Point,
        target: Option<End>,
    },
}

struct Picture {
    scene: Scene,
    selection: Vec<String>,
    view: Viewport,
    provisional: Vec<(End, End)>,
    hover: Option<Hit>,
    overlay: Overlay,
    anchors: bool,
    grid: Option<f32>,
    paths: HashMap<String, Rc<[Segment]>>,
    cursor: CursorStyle,
}

fn hsla(color: gpui::Rgba) -> Hsla {
    color.into()
}

fn role(role: Option<Role>, unset: Role) -> Hsla {
    hsla(Theme::role(role.unwrap_or(unset)))
}

impl Picture {
    fn screen(&self, bounds: Bounds<Pixels>, at: Point) -> gpui::Point<Pixels> {
        point(
            bounds.origin.x + px((at.x - self.view.x) * self.view.zoom),
            bounds.origin.y + px((at.y - self.view.y) * self.view.zoom),
        )
    }

    fn screen_rect(&self, bounds: Bounds<Pixels>, rect: Rect) -> Bounds<Pixels> {
        Bounds {
            origin: self.screen(bounds, rect.origin),
            size: size(px(rect.width * self.view.zoom), px(rect.height * self.view.zoom)),
        }
    }

    fn scaled(&self, units: f32) -> Pixels {
        px(units * self.view.zoom)
    }

    fn paint(&self, bounds: Bounds<Pixels>, hitbox: &Hitbox, window: &mut Window, cx: &mut gpui::App) {
        window.paint_quad(quad(
            bounds,
            px(20.),
            hsla(Theme::panel()),
            px(1.),
            hsla(Theme::line()),
            BorderStyle::Solid,
        ));
        window.set_cursor_style(self.cursor, hitbox);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            self.paint_grid(bounds, window);
            for item in &self.scene.items {
                match item {
                    Item::Node(node) => self.paint_node(bounds, node, window, cx),
                    Item::Connector(connector) => self.paint_connector(bounds, connector, window, cx),
                }
            }
            for (from, to) in &self.provisional {
                if let Some(line) = self.scene.line_between(Route::Curved, from, to) {
                    self.stroke_line(bounds, &line, hsla(Theme::accent()), px(2.), true, window);
                }
            }
            self.paint_anchors(bounds, window);
            self.paint_overlay(bounds, window);
        });
    }

    fn paint_grid(&self, bounds: Bounds<Pixels>, window: &mut Window) {
        let Some(spacing) = self.grid.filter(|spacing| *spacing > 0.) else {
            return;
        };
        let mut step = spacing * self.view.zoom;
        while step < 12. {
            step *= 2.;
        }
        let first = Point::new(
            (self.view.x * self.view.zoom).rem_euclid(step),
            (self.view.y * self.view.zoom).rem_euclid(step),
        );
        let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        let dot = hsla(Theme::line());
        let mut y = step - first.y;
        while y < height {
            let mut x = step - first.x;
            while x < width {
                window.paint_quad(gpui::fill(
                    Bounds {
                        origin: point(bounds.origin.x + px(x - 1.), bounds.origin.y + px(y - 1.)),
                        size: size(px(2.), px(2.)),
                    },
                    dot,
                ));
                x += step;
            }
            y += step;
        }
    }

    fn paint_node(&self, bounds: Bounds<Pixels>, node: &Node, window: &mut Window, cx: &mut gpui::App) {
        let origin = Point::new(node.x, node.y);
        for shape in &node.shapes {
            self.paint_shape(bounds, origin, shape, window, cx);
        }
        let selected = self.selection.contains(&node.id);
        let hovered = self.hover.as_ref().is_some_and(|hit| hit.item() == node.id);
        if !node.decorative && (selected || hovered) {
            let radius = node
                .shapes
                .iter()
                .find_map(|shape| match shape {
                    Shape::Rect { radius, .. } => *radius,
                    _ => None,
                })
                .unwrap_or(8.);
            let outline = self.screen_rect(bounds, node.bounds()).dilate(px(4.));
            let (width, color) = if selected {
                (px(2.), hsla(Theme::accent()))
            } else {
                (px(1.5), hsla(Theme::accent()).opacity(0.5))
            };
            window.paint_quad(quad(
                outline,
                self.scaled(radius) + px(4.),
                gpui::transparent_black(),
                width,
                color,
                BorderStyle::Solid,
            ));
        }
    }

    fn paint_shape(
        &self,
        bounds: Bounds<Pixels>,
        origin: Point,
        shape: &Shape,
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        match shape {
            Shape::Rect { .. } => self.paint_rect(bounds, origin, shape, window),
            Shape::Ellipse {
                x,
                y,
                width,
                height,
                fill,
                stroke,
                stroke_width,
            } => {
                let rect = self.screen_rect(
                    bounds,
                    Rect {
                        origin: origin + Point::new(*x, *y),
                        width: *width,
                        height: *height,
                    },
                );
                self.paint_ellipse(rect, (*fill, *stroke, *stroke_width), window);
            }
            Shape::Text {
                x,
                y,
                text,
                size,
                weight,
                color,
                align,
                max_width,
                monospace,
            } => {
                let at = self.screen(bounds, origin + Point::new(*x, *y));
                let face = Face {
                    size: self.scaled(size.unwrap_or(13.)),
                    weight: *weight,
                    monospace: *monospace,
                    color: role(*color, Role::OnSurface),
                };
                paint_text(
                    text,
                    at,
                    &face,
                    *align,
                    max_width.map(|max| self.scaled(max)),
                    window,
                    cx,
                );
            }
            Shape::Path {
                d,
                fill,
                stroke,
                stroke_width,
                dashed,
            } => {
                if let Some(segments) = self.paths.get(d) {
                    self.paint_path(
                        bounds,
                        origin,
                        segments,
                        (*fill, *stroke, *stroke_width),
                        *dashed,
                        window,
                    );
                }
            }
            Shape::Icon {
                x,
                y,
                name,
                size,
                color,
            } => self.paint_icon(
                bounds,
                origin + Point::new(*x, *y),
                name,
                (*size, *color),
                window,
                cx,
            ),
        }
    }

    fn paint_rect(&self, bounds: Bounds<Pixels>, origin: Point, shape: &Shape, window: &mut Window) {
        let Shape::Rect {
            x,
            y,
            width,
            height,
            radius,
            fill,
            stroke,
            stroke_width,
        } = shape
        else {
            return;
        };
        let rect = Rect {
            origin: origin + Point::new(*x, *y),
            width: *width,
            height: *height,
        };
        window.paint_quad(quad(
            self.screen_rect(bounds, rect),
            self.scaled(radius.unwrap_or(0.)),
            role(*fill, Role::SurfaceContainer),
            self.scaled(stroke_width.unwrap_or(1.)),
            role(*stroke, Role::OutlineVariant),
            BorderStyle::Solid,
        ));
    }

    fn paint_icon(
        &self,
        bounds: Bounds<Pixels>,
        at: Point,
        name: &str,
        (size, color): (Option<f32>, Option<Role>),
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        let edge = self.scaled(size.unwrap_or(20.));
        let square = Bounds {
            origin: self.screen(bounds, at),
            size: gpui::size(edge, edge),
        };
        let color = role(color, Role::OnSurfaceVariant);
        window
            .paint_svg(
                square,
                brand::symbol(Some(name)),
                None,
                TransformationMatrix::unit(),
                color,
                cx,
            )
            .ok();
    }

    fn paint_ellipse(
        &self,
        rect: Bounds<Pixels>,
        (fill, stroke, stroke_width): (Option<Role>, Option<Role>, Option<f32>),
        window: &mut Window,
    ) {
        let ellipse = |builder: &mut PathBuilder| {
            let radii = point(rect.size.width / 2., rect.size.height / 2.);
            let (left, right) = (
                point(rect.left(), rect.center().y),
                point(rect.right(), rect.center().y),
            );
            builder.move_to(right);
            builder.arc_to(radii, px(0.), false, true, left);
            builder.arc_to(radii, px(0.), false, true, right);
            builder.close();
        };
        let mut filled = PathBuilder::fill();
        ellipse(&mut filled);
        if let Ok(path) = filled.build() {
            window.paint_path(path, role(fill, Role::SurfaceContainer));
        }
        let mut stroked = PathBuilder::stroke(self.scaled(stroke_width.unwrap_or(1.)));
        ellipse(&mut stroked);
        if let Ok(path) = stroked.build() {
            window.paint_path(path, role(stroke, Role::OutlineVariant));
        }
    }

    fn paint_path(
        &self,
        bounds: Bounds<Pixels>,
        origin: Point,
        segments: &[Segment],
        (fill, stroke, stroke_width): (Option<Role>, Option<Role>, Option<f32>),
        dashed: bool,
        window: &mut Window,
    ) {
        let at = |to: Point| self.screen(bounds, origin + to);
        let trace = |builder: &mut PathBuilder| {
            for segment in segments {
                match *segment {
                    Segment::Move(to) => builder.move_to(at(to)),
                    Segment::Line(to) => builder.line_to(at(to)),
                    Segment::Cubic(a, b, to) => builder.cubic_bezier_to(at(to), at(a), at(b)),
                    Segment::Quad(control, to) => builder.curve_to(at(to), at(control)),
                    Segment::Close => builder.close(),
                }
            }
        };
        // An unset fill fills only a closed outline; an open line stays a line.
        let closed = segments.iter().any(|segment| matches!(segment, Segment::Close));
        if fill.is_some() || closed {
            let mut filled = PathBuilder::fill();
            trace(&mut filled);
            if let Ok(path) = filled.build() {
                window.paint_path(path, role(fill, Role::SurfaceContainer));
            }
        }
        let mut stroked = PathBuilder::stroke(self.scaled(stroke_width.unwrap_or(1.)));
        if dashed {
            stroked = stroked.dash_array(&[self.scaled(6.), self.scaled(4.)]);
        }
        trace(&mut stroked);
        if let Ok(path) = stroked.build() {
            window.paint_path(path, role(stroke, Role::OutlineVariant));
        }
    }

    fn stroke_line(
        &self,
        bounds: Bounds<Pixels>,
        line: &Line,
        color: Hsla,
        width: Pixels,
        dashed: bool,
        window: &mut Window,
    ) {
        let mut builder = PathBuilder::stroke(width);
        if dashed {
            builder = builder.dash_array(&[px(6.), px(4.)]);
        }
        builder.move_to(self.screen(bounds, line.start));
        for piece in &line.pieces {
            match *piece {
                Piece::To(to) => builder.line_to(self.screen(bounds, to)),
                Piece::Curve {
                    control_a,
                    control_b,
                    to,
                } => builder.cubic_bezier_to(
                    self.screen(bounds, to),
                    self.screen(bounds, control_a),
                    self.screen(bounds, control_b),
                ),
            }
        }
        if let Ok(path) = builder.build() {
            window.paint_path(path, color);
        }
    }

    fn arrow_head(
        &self,
        bounds: Bounds<Pixels>,
        tip: Point,
        heading: Point,
        reach: f32,
        color: Hsla,
        window: &mut Window,
    ) {
        let across = Point::new(-heading.y, heading.x);
        let back = tip - heading * reach;
        let mut builder = PathBuilder::fill();
        builder.move_to(self.screen(bounds, tip));
        builder.line_to(self.screen(bounds, back + across * (reach * 0.5)));
        builder.line_to(self.screen(bounds, back - across * (reach * 0.5)));
        builder.close();
        if let Ok(path) = builder.build() {
            window.paint_path(path, color);
        }
    }

    fn paint_connector(
        &self,
        bounds: Bounds<Pixels>,
        connector: &Connector,
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        let Some(line) = self.scene.line(connector) else {
            return;
        };
        let selected = self.selection.contains(&connector.id);
        let hovered = self.hover.as_ref().is_some_and(|hit| hit.item() == connector.id);
        let units = connector.stroke_width.unwrap_or(1.5);
        let color = if selected || (hovered && !connector.decorative) {
            hsla(Theme::accent())
        } else {
            role(connector.stroke, Role::Outline)
        };
        let width = (self.scaled(units) + if selected { px(1.) } else { px(0.) }).max(px(1.));
        self.stroke_line(bounds, &line, color, width, connector.dashed, window);
        let (end, start) = line.heads();
        let reach = 7. + units * 2.;
        if connector.arrow.at_end()
            && let Some(Piece::To(tip) | Piece::Curve { to: tip, .. }) = line.pieces.last()
        {
            self.arrow_head(bounds, *tip, end, reach, color, window);
        }
        if connector.arrow.at_start() {
            self.arrow_head(bounds, line.start, start, reach, color, window);
        }
        if let Some(label) = &connector.label {
            self.paint_label(bounds, line.middle(), label, window, cx);
        }
    }

    fn paint_label(
        &self,
        bounds: Bounds<Pixels>,
        at: Point,
        label: &str,
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        let face = Face {
            size: self.scaled(11.),
            weight: Weight::Regular,
            monospace: false,
            color: hsla(Theme::ink_dim()),
        };
        let Some(line) = shape_text(label, &face, None, window) else {
            return;
        };
        let center = self.screen(bounds, at);
        let pad = self.scaled(6.);
        let height = face.size * 1.6;
        let pill = Bounds {
            origin: point(center.x - line.width / 2. - pad, center.y - height / 2.),
            size: size(line.width + pad * 2., height),
        };
        window.paint_quad(quad(
            pill,
            height / 2.,
            hsla(Theme::panel()),
            px(1.),
            hsla(Theme::line()),
            BorderStyle::Solid,
        ));
        line.paint(
            point(
                pill.origin.x + pad,
                pill.origin.y + (height - face.size * 1.3) / 2.,
            ),
            face.size * 1.3,
            TextAlign::Left,
            None,
            window,
            cx,
        )
        .ok();
    }

    fn paint_anchors(&self, bounds: Bounds<Pixels>, window: &mut Window) {
        if !self.anchors {
            return;
        }
        let connecting = match &self.overlay {
            Overlay::Connecting { from, target, .. } => Some((from, target)),
            _ => None,
        };
        let hovered = self.hover.as_ref().map(Hit::item);
        for item in &self.scene.items {
            let Item::Node(node) = item else { continue };
            if node.decorative {
                continue;
            }
            let shown = connecting.is_some()
                || hovered == Some(node.id.as_str())
                || self.selection.contains(&node.id);
            if !shown {
                continue;
            }
            for anchor in &node.anchors {
                let center = self.screen(bounds, node.anchor_point(anchor));
                let (ring, filled) = if let Some((from, target)) = connecting {
                    let open = node.id != from.node && anchor.direction.ends();
                    let aimed = target.as_ref().is_some_and(|end| {
                        end.node == node.id && end.anchor.as_deref() == Some(anchor.id.as_str())
                    });
                    (if open { Theme::accent() } else { Theme::line() }, aimed)
                } else {
                    let on = matches!(&self.hover, Some(Hit::Anchor { node: hit, anchor: id }) if *hit == node.id && *id == anchor.id);
                    (Theme::outline(), on)
                };
                let radius = px(ANCHOR_RADIUS);
                window.paint_quad(quad(
                    Bounds {
                        origin: point(center.x - radius, center.y - radius),
                        size: size(radius * 2., radius * 2.),
                    },
                    radius,
                    hsla(if filled { Theme::accent() } else { Theme::surface() }),
                    px(1.5),
                    hsla(ring),
                    BorderStyle::Solid,
                ));
            }
        }
    }

    fn paint_overlay(&self, bounds: Bounds<Pixels>, window: &mut Window) {
        match &self.overlay {
            Overlay::None => {}
            Overlay::Box(rect) => window.paint_quad(quad(
                self.screen_rect(bounds, *rect),
                px(4.),
                hsla(Theme::accent()).opacity(0.08),
                px(1.),
                hsla(Theme::accent()),
                BorderStyle::Solid,
            )),
            Overlay::Connecting { from, to, target } => {
                let line = if let Some(end) = target {
                    self.scene.line_between(Route::Curved, from, end)
                } else {
                    let start = self.scene.end_point(from, *to);
                    start.map(|(point, out)| Line::routed(Route::Curved, (point, out), (*to, out * -1.)))
                };
                if let Some(line) = line {
                    self.stroke_line(bounds, &line, hsla(Theme::accent()), px(2.), true, window);
                }
            }
        }
    }
}

struct Face {
    size: Pixels,
    weight: Weight,
    monospace: bool,
    color: Hsla,
}

/// One line of text, ending in an ellipsis past `max_width`.
fn shape_text(
    text: &str,
    face: &Face,
    max_width: Option<Pixels>,
    window: &Window,
) -> Option<gpui::ShapedLine> {
    if face.size < px(4.) || text.is_empty() {
        return None;
    }
    let mut family = font(if face.monospace { Theme::MONO } else { Theme::SANS });
    family.weight = match face.weight {
        Weight::Regular => FontWeight::NORMAL,
        Weight::Medium => FontWeight::MEDIUM,
        Weight::Bold => FontWeight::BOLD,
    };
    let shape = |text: SharedString| {
        let run = TextRun {
            len: text.len(),
            font: family.clone(),
            color: face.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        window.text_system().shape_line(text, face.size, &[run], None)
    };
    let first_line: SharedString = text.lines().next().unwrap_or_default().to_owned().into();
    let line = shape(first_line.clone());
    let Some(max) = max_width.filter(|max| line.width > *max) else {
        return Some(line);
    };
    let ends: Vec<usize> = first_line.char_indices().map(|(at, _)| at).collect();
    let (mut fits, mut over) = (0, ends.len());
    while fits + 1 < over {
        let middle = usize::midpoint(fits, over);
        let candidate = format!("{}…", first_line[..ends[middle]].trim_end());
        if shape(candidate.into()).width <= max {
            fits = middle;
        } else {
            over = middle;
        }
    }
    Some(shape(format!("{}…", first_line[..ends[fits]].trim_end()).into()))
}

fn paint_text(
    text: &str,
    at: gpui::Point<Pixels>,
    face: &Face,
    align: Align,
    max_width: Option<Pixels>,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let Some(line) = shape_text(text, face, max_width, window) else {
        return;
    };
    let x = match align {
        Align::Start => at.x,
        Align::Center => at.x - line.width / 2.,
        Align::End => at.x - line.width,
    };
    line.paint(point(x, at.y), face.size * 1.3, TextAlign::Left, None, window, cx)
        .ok();
}

impl CanvasView {
    fn control(id: &'static str, icon: &'static str) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .size(px(28.))
            .rounded(px(14.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|button| button.bg(Theme::raised()))
            .child(brand::icon(brand::symbol(Some(icon)), 16., Theme::ink_dim()))
    }

    fn controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let zoom = self.zoom();
        div()
            .absolute()
            .right(px(12.))
            .bottom(px(12.))
            .p(px(2.))
            .rounded(px(16.))
            .bg(Theme::card())
            .border_1()
            .border_color(Theme::line())
            .flex()
            .items_center()
            .occlude()
            .child(
                Self::control("canvas-zoom-out", "remove")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.zoom_by(1. / ZOOM_STEP, cx))),
            )
            .child(
                div()
                    .w(px(44.))
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .flex()
                    .justify_center()
                    .child(format!("{:.0}%", zoom * 100.)),
            )
            .child(
                Self::control("canvas-zoom-in", "add")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.zoom_by(ZOOM_STEP, cx))),
            )
            .child(Self::control("canvas-fit", "fit_screen").on_click(cx.listener(
                |this, _: &ClickEvent, _, cx| {
                    let size = this.size();
                    this.board.fit(size);
                    cx.notify();
                },
            )))
    }

    fn tooltip(&self) -> Option<impl IntoElement> {
        if !self.tooltip || self.gesture.is_some() || self.open_menu.is_some() {
            return None;
        }
        let item = self.board.scene().item(self.hover.as_ref()?.item())?;
        let text = item.tooltip()?.to_owned();
        let at = self.local(self.pointer?);
        Some(
            div()
                .absolute()
                .left(px(at.x + 14.))
                .top(px(at.y + 18.))
                .max_w(px(280.))
                .px_3()
                .py_1()
                .rounded(px(10.))
                .bg(Theme::raised())
                .border_1()
                .border_color(Theme::line())
                .text_xs()
                .text_color(Theme::ink())
                .child(text),
        )
    }

    fn menu_overlay(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let menu = self.open_menu.as_ref()?;
        Some(deferred(
            anchored().position(menu.at).child(
                div()
                    .min_w(px(200.))
                    .p_1()
                    .rounded(px(16.))
                    .bg(Theme::card())
                    .border_1()
                    .border_color(Theme::line())
                    .text_color(Theme::ink())
                    .occlude()
                    .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.open_menu = None;
                        cx.notify();
                    }))
                    .children(menu.entries.iter().enumerate().map(|(index, entry)| {
                        let choice = entry.id.clone();
                        let ink = if entry.danger {
                            Theme::danger()
                        } else {
                            Theme::ink()
                        };
                        div()
                            .id(("canvas-menu", index))
                            .px_3()
                            .py_2()
                            .rounded(px(10.))
                            .flex()
                            .items_center()
                            .gap_2()
                            .cursor_pointer()
                            .text_color(ink)
                            .hover(|item| item.bg(Theme::selected()))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.choose(choice.clone(), cx);
                            }))
                            .children(
                                entry
                                    .icon
                                    .as_deref()
                                    .map(|icon| brand::icon(brand::symbol(Some(icon)), 16., ink)),
                            )
                            .child(entry.label.clone())
                    })),
            ),
        ))
    }
}

impl Render for CanvasView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let drawing = canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |bounds, hitbox, window, cx| {
                let picture = view.update(cx, |this, _| this.picture(bounds));
                picture.paint(bounds, &hitbox, window, cx);
                window.on_mouse_event({
                    let (view, hitbox) = (view.clone(), hitbox.clone());
                    move |event: &MouseDownEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                            view.update(cx, |this, cx| this.down(event, cx));
                            cx.stop_propagation();
                        }
                    }
                });
                window.on_mouse_event({
                    let (view, hitbox) = (view.clone(), hitbox.clone());
                    move |event: &MouseMoveEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble {
                            let over = hitbox.is_hovered(window);
                            view.update(cx, |this, cx| this.moved(event, over, cx));
                        }
                    }
                });
                window.on_mouse_event({
                    let view = view.clone();
                    move |event: &MouseUpEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble {
                            view.update(cx, |this, cx| this.up(event, cx));
                        }
                    }
                });
                window.on_mouse_event({
                    let (view, hitbox) = (view.clone(), hitbox.clone());
                    move |event: &ScrollWheelEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble && hitbox.should_handle_scroll(window) {
                            view.update(cx, |this, cx| this.scrolled(event, cx));
                            cx.stop_propagation();
                        }
                    }
                });
                window.on_mouse_event({
                    let view = view.clone();
                    move |event: &PinchEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                            view.update(cx, |this, cx| this.pinched(event, cx));
                            cx.stop_propagation();
                        }
                    }
                });
            },
        )
        .size_full();
        div()
            .relative()
            .w_full()
            .h(px(self.height))
            .flex_none()
            .child(drawing)
            .child(self.controls(cx))
            .children(self.problem.clone().map(|problem| {
                div()
                    .absolute()
                    .left(px(12.))
                    .top(px(12.))
                    .max_w(px(420.))
                    .px_3()
                    .py_1()
                    .rounded(px(12.))
                    .bg(Theme::danger_container())
                    .text_xs()
                    .text_color(Theme::danger())
                    .child(problem)
            }))
            .children(self.tooltip())
            .children(self.menu_overlay(cx))
    }
}
