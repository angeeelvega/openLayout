use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }

    pub fn min_x(self) -> f64 {
        self.x
    }

    pub fn min_y(self) -> f64 {
        self.y
    }

    pub fn max_x(self) -> f64 {
        self.x + self.width
    }

    pub fn max_y(self) -> f64 {
        self.y + self.height
    }

    pub fn mid_x(self) -> f64 {
        self.x + self.width / 2.0
    }

    pub fn mid_y(self) -> f64 {
        self.y + self.height / 2.0
    }

    pub fn intersects(self, other: Rect) -> bool {
        self.min_x() < other.max_x()
            && self.max_x() > other.min_x()
            && self.min_y() < other.max_y()
            && self.max_y() > other.min_y()
    }

    pub fn intersection(self, other: Rect) -> Option<Rect> {
        let left = self.min_x().max(other.min_x());
        let bottom = self.min_y().max(other.min_y());
        let right = self.max_x().min(other.max_x());
        let top = self.max_y().min(other.max_y());
        if right - left > 0.0 && top - bottom > 0.0 {
            Some(Rect::new(left, bottom, right - left, top - bottom))
        } else {
            None
        }
    }

    pub fn contains(self, px: f64, py: f64) -> bool {
        px >= self.min_x() && px < self.max_x() && py >= self.min_y() && py < self.max_y()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WindowId {
    pub pid: i32,
    pub number: u32,
}

impl WindowId {
    pub fn new(pid: i32, number: u32) -> Self {
        Self { pid, number }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisplaySlice {
    pub id: u32,
    pub frame: Rect,
    pub usable: Rect,
    pub name: String,
}

/// Left to right, then bottom to top when displays share an x range.
pub fn order_displays(mut displays: Vec<DisplaySlice>) -> Vec<DisplaySlice> {
    displays.sort_by(|lhs, rhs| {
        if (lhs.frame.min_x() - rhs.frame.min_x()).abs() > 48.0 {
            lhs.frame
                .min_x()
                .partial_cmp(&rhs.frame.min_x())
                .unwrap_or(std::cmp::Ordering::Equal)
        } else {
            lhs.frame
                .min_y()
                .partial_cmp(&rhs.frame.min_y())
                .unwrap_or(std::cmp::Ordering::Equal)
        }
    });
    displays
}
