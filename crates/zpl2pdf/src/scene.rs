//! Backend independent drawing list.
//!
//! The interpreter turns every label into a [`Label`]: a list of items in
//! printer dots, with the origin in the top left corner and y pointing down.
//! The PDF and raster backends only need to understand these few items.

use std::sync::Arc;

use crate::font::{PlacedGlyph, ResolvedFont};
use crate::graphic::Bitmap;
use crate::types::{Rotation, Transform};

/// How an item is painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Paint {
    /// Prints black dots.
    Black,
    /// Clears dots to white.
    White,
    /// Inverts the dots below (`^FR`, `^LR`).
    Reverse,
}

/// A path segment, coordinates in dots.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Segment {
    /// Starts a new subpath.
    MoveTo(f64, f64),
    /// Straight line.
    LineTo(f64, f64),
    /// Cubic Bezier curve `(c1, c2, end)`.
    CubicTo(f64, f64, f64, f64, f64, f64),
    /// Closes the subpath.
    Close,
}

/// A filled path. Subpaths are combined with the even-odd rule, which is how
/// rings (box borders, circles) are expressed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Path {
    /// Segments.
    pub segments: Vec<Segment>,
}

const KAPPA: f64 = 0.552_284_749_830_793_4;

impl Path {
    /// Empty path.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a closed polygon.
    pub fn polygon(&mut self, points: &[(f64, f64)]) -> &mut Self {
        if let Some((&first, rest)) = points.split_first() {
            self.segments.push(Segment::MoveTo(first.0, first.1));
            for p in rest {
                self.segments.push(Segment::LineTo(p.0, p.1));
            }
            self.segments.push(Segment::Close);
        }
        self
    }

    /// Adds a rectangle.
    pub fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) -> &mut Self {
        self.polygon(&[(x, y), (x + w, y), (x + w, y + h), (x, y + h)])
    }

    /// Adds a rectangle with rounded corners of radius `r`.
    pub fn rounded_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) -> &mut Self {
        let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
        if r <= 0.0 {
            return self.rect(x, y, w, h);
        }
        let k = r * KAPPA;
        let (x1, y1) = (x + w, y + h);
        let s = &mut self.segments;
        s.push(Segment::MoveTo(x + r, y));
        s.push(Segment::LineTo(x1 - r, y));
        s.push(Segment::CubicTo(x1 - r + k, y, x1, y + r - k, x1, y + r));
        s.push(Segment::LineTo(x1, y1 - r));
        s.push(Segment::CubicTo(x1, y1 - r + k, x1 - r + k, y1, x1 - r, y1));
        s.push(Segment::LineTo(x + r, y1));
        s.push(Segment::CubicTo(x + r - k, y1, x, y1 - r + k, x, y1 - r));
        s.push(Segment::LineTo(x, y + r));
        s.push(Segment::CubicTo(x, y + r - k, x + r - k, y, x + r, y));
        s.push(Segment::Close);
        self
    }

    /// Adds an axis aligned ellipse centered at `(cx, cy)`.
    pub fn ellipse(&mut self, cx: f64, cy: f64, rx: f64, ry: f64) -> &mut Self {
        if rx <= 0.0 || ry <= 0.0 {
            return self;
        }
        let (kx, ky) = (rx * KAPPA, ry * KAPPA);
        let s = &mut self.segments;
        s.push(Segment::MoveTo(cx + rx, cy));
        s.push(Segment::CubicTo(
            cx + rx,
            cy + ky,
            cx + kx,
            cy + ry,
            cx,
            cy + ry,
        ));
        s.push(Segment::CubicTo(
            cx - kx,
            cy + ry,
            cx - rx,
            cy + ky,
            cx - rx,
            cy,
        ));
        s.push(Segment::CubicTo(
            cx - rx,
            cy - ky,
            cx - kx,
            cy - ry,
            cx,
            cy - ry,
        ));
        s.push(Segment::CubicTo(
            cx + kx,
            cy - ry,
            cx + rx,
            cy - ky,
            cx + rx,
            cy,
        ));
        s.push(Segment::Close);
        self
    }

    /// Applies a transform to every point.
    pub fn transform(&self, t: &Transform) -> Path {
        let segments = self
            .segments
            .iter()
            .map(|s| match *s {
                Segment::MoveTo(x, y) => {
                    let (x, y) = t.apply(x, y);
                    Segment::MoveTo(x, y)
                }
                Segment::LineTo(x, y) => {
                    let (x, y) = t.apply(x, y);
                    Segment::LineTo(x, y)
                }
                Segment::CubicTo(a, b, c, d, e, f) => {
                    let (a, b) = t.apply(a, b);
                    let (c, d) = t.apply(c, d);
                    let (e, f) = t.apply(e, f);
                    Segment::CubicTo(a, b, c, d, e, f)
                }
                Segment::Close => Segment::Close,
            })
            .collect();
        Path { segments }
    }

    /// Bounding box of all points `(x0, y0, x1, y1)`.
    pub fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        let mut b: Option<(f64, f64, f64, f64)> = None;
        let mut add = |x: f64, y: f64| {
            b = Some(match b {
                None => (x, y, x, y),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            });
        };
        for s in &self.segments {
            match *s {
                Segment::MoveTo(x, y) | Segment::LineTo(x, y) => add(x, y),
                Segment::CubicTo(a, b2, c, d, e, f) => {
                    add(a, b2);
                    add(c, d);
                    add(e, f);
                }
                Segment::Close => {}
            }
        }
        b
    }
}

/// A run of text on one baseline.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    /// Font at its resolved size.
    pub font: ResolvedFont,
    /// Glyphs positioned along the baseline.
    pub glyphs: Vec<PlacedGlyph>,
    /// Maps run coordinates (x along the baseline, y down from the
    /// baseline, origin at the start of the baseline) to label dots.
    pub transform: Transform,
    /// Paint.
    pub paint: Paint,
}

/// A drawing operation.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// Axis aligned rectangle.
    Rect {
        /// Left edge.
        x: f64,
        /// Top edge.
        y: f64,
        /// Width.
        w: f64,
        /// Height.
        h: f64,
        /// Paint.
        paint: Paint,
    },
    /// Filled path (even-odd rule).
    Path {
        /// Path in label dots.
        path: Path,
        /// Paint.
        paint: Paint,
    },
    /// Text.
    Text(TextRun),
    /// Monochrome image; only its black dots are painted.
    Bitmap {
        /// Left edge.
        x: f64,
        /// Top edge.
        y: f64,
        /// The image.
        bitmap: Arc<Bitmap>,
        /// Paint.
        paint: Paint,
    },
}

/// One rendered label.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    /// Page width in dots.
    pub width: f64,
    /// Page height in dots.
    pub height: f64,
    /// Printer resolution in dots per millimeter.
    pub dpmm: u8,
    /// Dots per inch derived from `dpmm`.
    pub dpi: f64,
    /// Drawing operations in painting order.
    pub items: Vec<Item>,
    /// Printable area `(x, y, width, height)` when `^PW` is narrower than the
    /// page. The printable area is centered, like on a printer.
    pub clip: Option<(f64, f64, f64, f64)>,
    /// `^PO I`: the label is rotated by 180 degrees.
    pub inverted: bool,
    /// `^PM Y`: the label is mirrored horizontally.
    pub mirrored: bool,
    /// Rotation of the output page, applied after `^PO` and `^PM`.
    /// `width` and `height` stay those of the unrotated label; see
    /// [`Label::page_size`].
    pub rotation: Rotation,
}

impl Item {
    /// Moves the item by `(dx, dy)` dots.
    pub fn translate(&mut self, dx: f64, dy: f64) {
        match self {
            Item::Rect { x, y, .. } | Item::Bitmap { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
            Item::Path { path, .. } => *path = path.transform(&Transform::translate(dx, dy)),
            Item::Text(run) => run.transform = run.transform.then(&Transform::translate(dx, dy)),
        }
    }
}

impl Label {
    /// Output page size in dots, after [`Label::rotation`].
    pub fn page_size(&self) -> (f64, f64) {
        if self.rotation.swaps_axes() {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }

    /// Transform from label dots to output page dots (print orientation,
    /// mirror and rotation).
    pub fn page_transform(&self) -> Transform {
        let mut t = Transform::IDENTITY;
        if self.mirrored {
            t = t.then(&Transform {
                a: -1.0,
                e: self.width,
                ..Transform::IDENTITY
            });
        }
        if self.inverted {
            t = t.then(&Transform {
                a: -1.0,
                d: -1.0,
                e: self.width,
                f: self.height,
                ..Transform::IDENTITY
            });
        }
        t.then(&self.rotation.transform(self.width, self.height))
    }
}

/// Converts dots per millimeter to the dots per inch Zebra uses.
pub fn dpmm_to_dpi(dpmm: u8) -> f64 {
    match dpmm {
        6 => 152.0,
        8 => 203.0,
        12 => 300.0,
        24 => 600.0,
        other => (other as f64 * 25.4).round(),
    }
}
