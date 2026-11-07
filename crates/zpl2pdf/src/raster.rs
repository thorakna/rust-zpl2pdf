//! Raster backend: renders labels to 1 bit images like a thermal printer.
//!
//! Shapes are sampled at pixel centers without anti-aliasing, reverse
//! printing is a real XOR. The output is used for PNG export and for
//! comparing against reference images in tests.

use skrifa::outline::OutlinePen;
use tiny_skia::{FillRule, Mask, PathBuilder};

use crate::Error;
use crate::font;
use crate::scene::{Item, Label, Paint, Path, Segment, TextRun};
use crate::types::{Rotation, Transform};

/// A rendered label: one byte per pixel, `1` = black.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raster {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Row major pixels.
    pub pixels: Vec<u8>,
}

impl Raster {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width as usize) * (height as usize)],
        }
    }

    /// Pixel value at `(x, y)`.
    pub fn get(&self, x: u32, y: u32) -> bool {
        x < self.width && y < self.height && self.pixels[(y * self.width + x) as usize] != 0
    }

    /// Encodes the raster as an 8 bit grayscale PNG (black on white).
    pub fn to_png(&self) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, self.width, self.height);
            enc.set_color(png::ColorType::Grayscale);
            enc.set_depth(png::BitDepth::Eight);
            let mut writer = enc
                .write_header()
                .map_err(|e| Error::Output(e.to_string()))?;
            let data: Vec<u8> = self
                .pixels
                .iter()
                .map(|&p| if p != 0 { 0 } else { 255 })
                .collect();
            writer
                .write_image_data(&data)
                .map_err(|e| Error::Output(e.to_string()))?;
        }
        Ok(out)
    }
}

struct Canvas {
    raster: Raster,
    clip: (i64, i64, i64, i64),
}

impl Canvas {
    fn apply(&mut self, x: i64, y: i64, paint: Paint) {
        let (cx0, cy0, cx1, cy1) = self.clip;
        if x < cx0 || y < cy0 || x >= cx1 || y >= cy1 {
            return;
        }
        let idx = (y as usize) * self.raster.width as usize + x as usize;
        let p = &mut self.raster.pixels[idx];
        *p = match paint {
            Paint::Black => 1,
            Paint::White => 0,
            Paint::Reverse => *p ^ 1,
        };
    }

    fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64, paint: Paint) {
        // Pixels whose centers are inside the rectangle.
        let x0 = (x - 0.5).ceil() as i64;
        let x1 = (x + w - 0.5).ceil() as i64;
        let y0 = (y - 0.5).ceil() as i64;
        let y1 = (y + h - 0.5).ceil() as i64;
        let (cx0, cy0, cx1, cy1) = self.clip;
        for py in y0.max(cy0)..y1.min(cy1) {
            for px in x0.max(cx0)..x1.min(cx1) {
                self.apply(px, py, paint);
            }
        }
    }

    fn fill_mask(
        &mut self,
        builder: PathBuilder,
        bounds: (f64, f64, f64, f64),
        rule: FillRule,
        paint: Paint,
    ) {
        let (cx0, cy0, cx1, cy1) = self.clip;
        let x0 = (bounds.0.floor() as i64 - 1).max(cx0);
        let y0 = (bounds.1.floor() as i64 - 1).max(cy0);
        let x1 = (bounds.2.ceil() as i64 + 1).min(cx1);
        let y1 = (bounds.3.ceil() as i64 + 1).min(cy1);
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let Some(path) = builder.finish() else { return };
        let (w, h) = ((x1 - x0) as u32, (y1 - y0) as u32);
        let Some(mut mask) = Mask::new(w, h) else {
            return;
        };
        mask.fill_path(
            &path,
            rule,
            false,
            tiny_skia::Transform::from_translate(-x0 as f32, -y0 as f32),
        );
        let data = mask.data();
        for my in 0..h as i64 {
            for mx in 0..w as i64 {
                if data[(my * w as i64 + mx) as usize] >= 128 {
                    self.apply(x0 + mx, y0 + my, paint);
                }
            }
        }
    }
}

fn path_builder(path: &Path) -> PathBuilder {
    let mut pb = PathBuilder::new();
    for s in &path.segments {
        match *s {
            Segment::MoveTo(x, y) => pb.move_to(x as f32, y as f32),
            Segment::LineTo(x, y) => pb.line_to(x as f32, y as f32),
            Segment::CubicTo(a, b, c, d, e, f) => {
                pb.cubic_to(a as f32, b as f32, c as f32, d as f32, e as f32, f as f32)
            }
            Segment::Close => pb.close(),
        }
    }
    pb
}

/// Pen that maps glyph outline points into label dots.
struct GlyphPen<'a> {
    builder: &'a mut PathBuilder,
    t: Transform,
    bounds: &'a mut Option<(f64, f64, f64, f64)>,
}

impl GlyphPen<'_> {
    fn map(&mut self, x: f32, y: f32) -> (f32, f32) {
        let (px, py) = self.t.apply(x as f64, y as f64);
        *self.bounds = Some(match *self.bounds {
            None => (px, py, px, py),
            Some((a, b, c, d)) => (a.min(px), b.min(py), c.max(px), d.max(py)),
        });
        (px as f32, py as f32)
    }
}

impl OutlinePen for GlyphPen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.map(x, y);
        self.builder.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.map(x, y);
        self.builder.line_to(x, y);
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let (cx0, cy0) = self.map(cx0, cy0);
        let (x, y) = self.map(x, y);
        self.builder.quad_to(cx0, cy0, x, y);
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let (cx0, cy0) = self.map(cx0, cy0);
        let (cx1, cy1) = self.map(cx1, cy1);
        let (x, y) = self.map(x, y);
        self.builder.cubic_to(cx0, cy0, cx1, cy1, x, y);
    }
    fn close(&mut self) {
        self.builder.close();
    }
}

fn draw_text(canvas: &mut Canvas, run: &TextRun) {
    let face = font::face(run.font.face);
    let unit = run.font.em / face.units_per_em;
    let mut builder = PathBuilder::new();
    let mut bounds = None;
    for g in &run.glyphs {
        // Font units (y up) -> run space (y down) -> label dots.
        let t = Transform {
            a: unit * run.font.scale_x,
            d: -unit,
            e: g.x,
            ..Transform::IDENTITY
        }
        .then(&run.transform);
        let mut pen = GlyphPen {
            builder: &mut builder,
            t,
            bounds: &mut bounds,
        };
        face.outline(g.gid, &mut pen);
    }
    if let Some(b) = bounds {
        canvas.fill_mask(builder, b, FillRule::Winding, run.paint);
    }
}

/// Renders a label to a raster.
pub fn render(label: &Label) -> Raster {
    let (w, h) = (label.width.max(1.0) as u32, label.height.max(1.0) as u32);
    let clip = match label.clip {
        Some((x, y, cw, ch)) => (
            x.round() as i64,
            y.round() as i64,
            (x + cw).round() as i64,
            (y + ch).round() as i64,
        ),
        None => (0, 0, w as i64, h as i64),
    };
    let clip = (
        clip.0.max(0),
        clip.1.max(0),
        clip.2.min(w as i64),
        clip.3.min(h as i64),
    );
    let mut canvas = Canvas {
        raster: Raster::new(w, h),
        clip,
    };
    for item in &label.items {
        match item {
            Item::Rect { x, y, w, h, paint } => canvas.fill_rect(*x, *y, *w, *h, *paint),
            Item::Path { path, paint } => {
                if let Some(b) = path.bounds() {
                    canvas.fill_mask(path_builder(path), b, FillRule::EvenOdd, *paint);
                }
            }
            Item::Bitmap {
                x,
                y,
                bitmap,
                paint,
            } => {
                let (ox, oy) = (x.round() as i64, y.round() as i64);
                for by in 0..bitmap.height {
                    for bx in 0..bitmap.width {
                        if bitmap.get(bx, by) {
                            canvas.apply(ox + bx as i64, oy + by as i64, *paint);
                        }
                    }
                }
            }
            Item::Text(run) => draw_text(&mut canvas, run),
        }
    }
    let mut raster = canvas.raster;
    if label.inverted || label.mirrored || label.rotation != Rotation::R0 {
        let t = label.page_transform();
        let (ow, oh) = if label.rotation.swaps_axes() {
            (h, w)
        } else {
            (w, h)
        };
        let mut out = Raster::new(ow, oh);
        for y in 0..h {
            for x in 0..w {
                if raster.get(x, y) {
                    let (nx, ny) = t.apply(x as f64 + 0.5, y as f64 + 0.5);
                    let (nx, ny) = (nx.floor() as i64, ny.floor() as i64);
                    if nx >= 0 && ny >= 0 && (nx as u32) < ow && (ny as u32) < oh {
                        out.pixels[(ny as u32 * ow + nx as u32) as usize] = 1;
                    }
                }
            }
        }
        raster = out;
    }
    raster
}
