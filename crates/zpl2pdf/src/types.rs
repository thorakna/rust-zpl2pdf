//! Small value types shared across the crate.

/// Field orientation as used by ZPL (`N`, `R`, `I`, `B`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Orientation {
    /// Normal, no rotation.
    #[default]
    Normal,
    /// Rotated 90 degrees clockwise.
    Rotated,
    /// Inverted, rotated 180 degrees.
    Inverted,
    /// Read from bottom up, rotated 270 degrees clockwise.
    Bottom,
}

impl Orientation {
    /// Parses a ZPL orientation letter.
    pub fn from_byte(b: u8) -> Option<Self> {
        match b.to_ascii_uppercase() {
            b'N' => Some(Self::Normal),
            b'R' => Some(Self::Rotated),
            b'I' => Some(Self::Inverted),
            b'B' => Some(Self::Bottom),
            _ => None,
        }
    }
}

/// Drawing color of a graphic element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum LineColor {
    /// Black (printed dots).
    #[default]
    Black,
    /// White (cleared dots).
    White,
}

impl LineColor {
    /// Parses `B` or `W`.
    pub fn from_byte(b: u8) -> Option<Self> {
        match b.to_ascii_uppercase() {
            b'B' => Some(Self::Black),
            b'W' => Some(Self::White),
            _ => None,
        }
    }
}

/// Text justification inside a field block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Justify {
    /// Left aligned.
    #[default]
    Left,
    /// Centered.
    Center,
    /// Right aligned.
    Right,
    /// Justified (stretched to the block width).
    Justified,
}

impl Justify {
    /// Parses `L`, `C`, `R` or `J`.
    pub fn from_byte(b: u8) -> Option<Self> {
        match b.to_ascii_uppercase() {
            b'L' => Some(Self::Left),
            b'C' => Some(Self::Center),
            b'R' => Some(Self::Right),
            b'J' => Some(Self::Justified),
            _ => None,
        }
    }
}

/// A 2D affine transform `[a b c d e f]` mapping `(x, y)` to
/// `(a*x + c*y + e, b*x + d*y + f)`, the same layout PDF uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    /// The identity transform.
    pub const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// A pure translation.
    pub fn translate(x: f64, y: f64) -> Self {
        Self {
            e: x,
            f: y,
            ..Self::IDENTITY
        }
    }

    /// A rotation for a ZPL orientation in a y-down coordinate system.
    pub fn rotation(o: Orientation) -> Self {
        match o {
            Orientation::Normal => Self::IDENTITY,
            // 90 degrees clockwise on screen: (u, v) -> (-v, u)
            Orientation::Rotated => Self {
                a: 0.0,
                b: 1.0,
                c: -1.0,
                d: 0.0,
                e: 0.0,
                f: 0.0,
            },
            Orientation::Inverted => Self {
                a: -1.0,
                b: 0.0,
                c: 0.0,
                d: -1.0,
                e: 0.0,
                f: 0.0,
            },
            // 270 degrees clockwise: (u, v) -> (v, -u)
            Orientation::Bottom => Self {
                a: 0.0,
                b: -1.0,
                c: 1.0,
                d: 0.0,
                e: 0.0,
                f: 0.0,
            },
        }
    }

    /// Returns a transform that applies `self` first and then `next`.
    pub fn then(&self, next: &Transform) -> Transform {
        // Apply self first, then next.
        Transform {
            a: next.a * self.a + next.c * self.b,
            b: next.b * self.a + next.d * self.b,
            c: next.a * self.c + next.c * self.d,
            d: next.b * self.c + next.d * self.d,
            e: next.a * self.e + next.c * self.f + next.e,
            f: next.b * self.e + next.d * self.f + next.f,
        }
    }

    /// Maps a point.
    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// Whether the transform keeps axis aligned rectangles axis aligned.
    pub fn is_axis_aligned(&self) -> bool {
        (self.b == 0.0 && self.c == 0.0) || (self.a == 0.0 && self.d == 0.0)
    }
}

/// Places a local box of size `w` x `h` (y down, origin top left) at a field
/// origin, rotated by `o`.
///
/// `anchor` is the local point that must land on `(x, y)`. When `anchor` is
/// `None` the rotated bounding box's top left corner lands on `(x, y)`, which
/// is how `^FO` behaves.
pub fn place_box(
    o: Orientation,
    w: f64,
    h: f64,
    x: f64,
    y: f64,
    anchor: Option<(f64, f64)>,
) -> Transform {
    let rot = Transform::rotation(o);
    let (ax, ay) = match anchor {
        Some(p) => p,
        None => match o {
            Orientation::Normal => (0.0, 0.0),
            Orientation::Rotated => (0.0, h),
            Orientation::Inverted => (w, h),
            Orientation::Bottom => (w, 0.0),
        },
    };
    let (rx, ry) = rot.apply(ax, ay);
    Transform {
        e: x - rx,
        f: y - ry,
        ..rot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_box_top_left() {
        for o in [
            Orientation::Normal,
            Orientation::Rotated,
            Orientation::Inverted,
            Orientation::Bottom,
        ] {
            let t = place_box(o, 100.0, 20.0, 10.0, 30.0, None);
            let pts =
                [(0.0, 0.0), (100.0, 0.0), (0.0, 20.0), (100.0, 20.0)].map(|(u, v)| t.apply(u, v));
            let minx = pts.iter().map(|p| p.0).fold(f64::MAX, f64::min);
            let miny = pts.iter().map(|p| p.1).fold(f64::MAX, f64::min);
            assert!(
                (minx - 10.0).abs() < 1e-9 && (miny - 30.0).abs() < 1e-9,
                "{o:?}"
            );
        }
    }

    #[test]
    fn rotated_glyph_top_faces_right() {
        let t = place_box(Orientation::Rotated, 100.0, 20.0, 0.0, 0.0, None);
        // glyph top (v = 0) should be at the right edge (x = 20)
        assert_eq!(t.apply(0.0, 0.0), (20.0, 0.0));
    }
}
