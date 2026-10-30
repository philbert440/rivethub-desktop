use super::{Point, Route};

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
