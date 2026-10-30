use gpui::{BorderStyle, Bounds, PathBuilder, Pixels, TransformationMatrix, Window, point, px, quad};
use model::plugin::canvas::{Point, Rect, Role, Shape};

use super::text::{Face, paint_text};
use super::{Picture, role};
use crate::brand;
use crate::plugin::canvas::path::Segment;

impl Picture {
    pub(super) fn paint_shape(
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
}
