mod connectors;
mod shapes;
mod text;

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    BorderStyle, Bounds, ContentMask, CursorStyle, Hitbox, Hsla, Pixels, Window, point, px, quad, size,
};
use model::plugin::board::Hit;
use model::plugin::canvas::{End, Item, Line, Node, Point, Rect, Role, Route, Scene, Shape, Viewport};

use super::path::{Segment, parse_path};
use super::{ANCHOR_RADIUS, CanvasView, Gesture};
use crate::theme::Theme;

impl CanvasView {
    /// What one frame draws, taken out of the view so painting can use the app.
    pub(super) fn picture(&mut self, bounds: Bounds<Pixels>, focused: bool) -> Picture {
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
        let scene = self.board.scene();
        Picture {
            scene: Scene {
                items: self.board.shown().cloned().collect(),
                grid: scene.grid,
                viewport: scene.viewport,
            },
            selection: self.board.selection().to_vec(),
            view,
            provisional: self.board.provisional().to_vec(),
            hover: self.hover.clone(),
            overlay,
            anchors: !self.read_only,
            grid: self.board.scene().grid.map(|grid| grid.size),
            paths,
            cursor,
            focused,
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

pub(super) struct Picture {
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
    /// Whether the canvas has the keyboard, which its outline shows.
    focused: bool,
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

    pub(super) fn paint(
        &self,
        bounds: Bounds<Pixels>,
        hitbox: &Hitbox,
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        let edge = if self.focused {
            hsla(Theme::accent()).opacity(0.45)
        } else {
            hsla(Theme::line())
        };
        window.paint_quad(quad(
            bounds,
            px(20.),
            hsla(Theme::panel()),
            px(1.),
            edge,
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
