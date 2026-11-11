// Points and rects in floating-point logical pixels, top-left origin, with no
// UI toolkit. CGRect's arithmetic where BearDock uses it, and its JSON shape:
// a window's saved frame is [[x, y], [width, height]], as Codable writes one.
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

impl Size {
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }

    pub const ZERO: Rect = Rect::new(0., 0., 0., 0.);

    pub fn min_x(&self) -> f64 {
        self.x
    }
    pub fn min_y(&self) -> f64 {
        self.y
    }
    pub fn max_x(&self) -> f64 {
        self.x + self.width
    }
    pub fn max_y(&self) -> f64 {
        self.y + self.height
    }
    pub fn mid_x(&self) -> f64 {
        self.x + self.width / 2.
    }
    pub fn mid_y(&self) -> f64 {
        self.y + self.height / 2.
    }

    pub fn origin(&self) -> Point {
        Point::new(self.x, self.y)
    }

    pub fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }

    /// As CGRect's: on the minimum edges counts, on the maximum edges doesn't.
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.max_x() && p.y >= self.y && p.y < self.max_y()
    }

    /// The smallest rect holding both. Zero-size rects still count, as
    /// `CGRect.null.union` starts from nothing.
    pub fn union(&self, other: &Rect) -> Rect {
        let min_x = self.x.min(other.x);
        let min_y = self.y.min(other.y);
        let max_x = self.max_x().max(other.max_x());
        let max_y = self.max_y().max(other.max_y());
        Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
    }

    /// `insetBy(dx:dy:)`: positive shrinks, negative grows.
    pub fn inset(&self, dx: f64, dy: f64) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.width - dx * 2., self.height - dy * 2.)
    }

    /// Moved so its origin is at `by` added to where it was.
    pub fn offset(&self, by: Point) -> Rect {
        Rect::new(self.x + by.x, self.y + by.y, self.width, self.height)
    }
}

// Swift's Codable writes a CGRect as [[x, y], [width, height]], and a window's
// frame in a saved layout has that shape.
impl Serialize for Rect {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        [[self.x, self.y], [self.width, self.height]].serialize(s)
    }
}

impl<'de> Deserialize<'de> for Rect {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let [[x, y], [width, height]] = <[[f64; 2]; 2]>::deserialize(d)?;
        Ok(Rect::new(x, y, width, height))
    }
}
