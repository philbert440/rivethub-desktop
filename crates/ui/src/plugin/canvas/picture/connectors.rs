use gpui::{BorderStyle, Bounds, Hsla, PathBuilder, Pixels, TextAlign, Window, point, px, quad, size};
use model::plugin::canvas::{Connector, Line, Piece, Point, Role, Weight};

use super::text::{Face, shape_text};
use super::{Picture, hsla, role};
use crate::theme::Theme;

impl Picture {
    pub(super) fn stroke_line(
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

    pub(super) fn paint_connector(
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
}
