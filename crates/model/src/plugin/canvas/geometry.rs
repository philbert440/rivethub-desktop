use std::ops::{Add, Mul, Sub};

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
