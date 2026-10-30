use std::rc::Rc;

use model::plugin::canvas::Point;

/// SVG path data, absolute and with arcs as curves, in canvas units from the node's top-left.
#[derive(Debug, Clone, Copy)]
pub(super) enum Segment {
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

pub(super) fn parse_path(data: &str) -> Rc<[Segment]> {
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
