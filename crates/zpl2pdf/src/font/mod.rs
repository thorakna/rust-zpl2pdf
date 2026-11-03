//! Fonts: embedded substitute faces and Zebra font metrics.
//!
//! Zebra's fonts are proprietary, so the glyph shapes come from open fonts:
//! Roboto Condensed Bold stands in for font 0 (CG Triumvirate Bold
//! Condensed) and DejaVu Sans Mono for the fixed pitch bitmap fonts. Glyph
//! positions, cap heights and baselines follow measurements of Zebra's fonts,
//! so text occupies the same space as on a printer.

mod font0_metrics;

use std::sync::OnceLock;

use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};

/// Embedded font faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FaceId {
    /// Roboto Condensed Bold, used for scalable fonts.
    Sans = 0,
    /// DejaVu Sans Mono, used for bitmap fonts.
    Mono = 1,
    /// DejaVu Sans Mono Bold, used for bitmap font B.
    MonoBold = 2,
}

impl FaceId {
    /// All faces.
    pub const ALL: [FaceId; 3] = [FaceId::Sans, FaceId::Mono, FaceId::MonoBold];
}

/// A loaded font face.
pub struct Face {
    /// Identifier.
    pub id: FaceId,
    /// Raw font file.
    pub data: &'static [u8],
    /// PostScript name.
    pub postscript_name: &'static str,
    /// Units per em.
    pub units_per_em: f64,
    /// Cap height in font units.
    pub cap_height: f64,
    /// Ascender in font units.
    pub ascent: f64,
    /// Descender in font units (negative).
    pub descent: f64,
    font: FontRef<'static>,
}

impl std::fmt::Debug for Face {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Face")
            .field("id", &self.id)
            .field("postscript_name", &self.postscript_name)
            .finish()
    }
}

static SANS_DATA: &[u8] = include_bytes!("../../fonts/RobotoCondensed-Bold.ttf");
static MONO_DATA: &[u8] = include_bytes!("../../fonts/DejaVuSansMono.ttf");
static MONO_BOLD_DATA: &[u8] = include_bytes!("../../fonts/DejaVuSansMono-Bold.ttf");

fn load(id: FaceId, data: &'static [u8], postscript_name: &'static str) -> Face {
    let font = FontRef::new(data).expect("embedded font is valid");
    let metrics = font.metrics(Size::unscaled(), LocationRef::default());
    let units_per_em = metrics.units_per_em as f64;
    let cap_height = metrics
        .cap_height
        .map(|c| c as f64)
        .filter(|c| *c > 0.0)
        .or_else(|| {
            let gid = font.charmap().map('H')?;
            font.glyph_metrics(Size::unscaled(), LocationRef::default())
                .bounds(gid)
                .map(|b| b.y_max as f64)
        })
        .unwrap_or(units_per_em * 0.7);
    Face {
        id,
        data,
        postscript_name,
        units_per_em,
        cap_height,
        ascent: metrics.ascent as f64,
        descent: metrics.descent as f64,
        font,
    }
}

/// Returns an embedded face.
pub fn face(id: FaceId) -> &'static Face {
    static FACES: OnceLock<[Face; 3]> = OnceLock::new();
    let faces = FACES.get_or_init(|| {
        [
            load(FaceId::Sans, SANS_DATA, "RobotoCondensed-Bold"),
            load(FaceId::Mono, MONO_DATA, "DejaVuSansMono"),
            load(FaceId::MonoBold, MONO_BOLD_DATA, "DejaVuSansMono-Bold"),
        ]
    });
    &faces[id as usize]
}

impl Face {
    /// Glyph id for a character.
    pub fn glyph(&self, ch: char) -> Option<u16> {
        self.font
            .charmap()
            .map(ch)
            .map(|g| g.to_u32() as u16)
            .filter(|&g| g != 0)
    }

    /// Advance width of a glyph in font units.
    pub fn advance(&self, gid: u16) -> f64 {
        self.font
            .glyph_metrics(Size::unscaled(), LocationRef::default())
            .advance_width(GlyphId::new(gid as u32))
            .unwrap_or(0.0) as f64
    }

    /// Draws a glyph outline in font units (y up).
    pub fn outline(&self, gid: u16, pen: &mut impl OutlinePen) -> bool {
        let Some(glyph) = self.font.outline_glyphs().get(GlyphId::new(gid as u32)) else {
            return false;
        };
        glyph
            .draw(
                DrawSettings::unhinted(Size::unscaled(), LocationRef::default()),
                pen,
            )
            .is_ok()
    }

    /// Glyph bounding box in font units `(x_min, y_min, x_max, y_max)`.
    pub fn bounds(&self, gid: u16) -> Option<(f64, f64, f64, f64)> {
        self.font
            .glyph_metrics(Size::unscaled(), LocationRef::default())
            .bounds(GlyphId::new(gid as u32))
            .map(|b| {
                (
                    b.x_min as f64,
                    b.y_min as f64,
                    b.x_max as f64,
                    b.y_max as f64,
                )
            })
    }
}

/// How a font treats lower case letters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseMode {
    /// Upper and lower case.
    Mixed,
    /// Lower case prints as upper case.
    UpperOnly,
    /// Lower case letters are not printed.
    UpperBlank,
}

/// Metrics of a fixed size Zebra font at magnification 1, in dots.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FixedMetrics {
    /// Cell height (the matrix height).
    pub height: f64,
    /// Matrix width (the default width parameter).
    pub width: f64,
    /// Character advance.
    pub advance: f64,
    /// Distance from the cell top to the top of capital letters.
    pub cap_top: f64,
    /// Height of capital letters.
    pub cap_height: f64,
    /// Lower case handling.
    pub case: CaseMode,
    /// Face used to draw the glyphs.
    pub face: FaceId,
}

/// Metrics of the fixed size fonts, measured at 8 dots/mm.
pub fn fixed_metrics(letter: u8, dpmm: u8) -> Option<FixedMetrics> {
    let m = |height, width, advance, cap_top, cap_height| FixedMetrics {
        height,
        width,
        advance,
        cap_top,
        cap_height,
        case: CaseMode::Mixed,
        face: FaceId::Mono,
    };
    let hi_res = dpmm >= 12;
    Some(match letter {
        b'A' => m(9.0, 5.0, 6.0, 0.0, 7.0),
        b'B' => FixedMetrics {
            case: CaseMode::UpperOnly,
            face: FaceId::MonoBold,
            ..m(11.0, 7.0, 9.0, 0.0, 11.0)
        },
        b'C' | b'D' => m(18.0, 10.0, 12.0, 0.0, 14.0),
        b'E' if hi_res => m(42.0, 20.0, 30.0, 4.0, 30.0),
        b'E' => m(28.0, 15.0, 20.0, 3.0, 20.0),
        b'F' => m(26.0, 13.0, 16.0, 0.0, 21.0),
        b'G' => m(60.0, 40.0, 48.0, 0.0, 48.0),
        b'H' if hi_res => FixedMetrics {
            case: CaseMode::UpperBlank,
            ..m(34.0, 22.0, 30.0, 0.0, 34.0)
        },
        b'H' => FixedMetrics {
            case: CaseMode::UpperBlank,
            ..m(21.0, 13.0, 19.0, 0.0, 21.0)
        },
        b'P' => m(20.0, 18.0, 10.0, 3.0, 12.0),
        b'Q' => m(28.0, 24.0, 14.0, 4.0, 17.0),
        b'R' => m(35.0, 31.0, 18.0, 5.0, 22.0),
        b'S' => m(40.0, 35.0, 20.333, 6.0, 25.0),
        b'T' => m(48.0, 42.0, 24.0, 6.0, 31.0),
        b'U' => m(59.0, 53.0, 30.667, 8.0, 38.0),
        b'V' => m(80.0, 71.0, 40.667, 12.0, 50.0),
        _ => return None,
    })
}

/// Advance of a character in font 0, as a fraction of the width parameter.
pub fn font0_advance(ch: char) -> Option<f64> {
    let cp = ch as u32;
    font0_metrics::FONT0_ADVANCES
        .binary_search_by_key(&cp, |&(c, _)| c)
        .ok()
        .map(|i| font0_metrics::FONT0_ADVANCES[i].1 as f64)
}

/// Font 0: cap height and baseline as a fraction of the height parameter.
pub const FONT0_CAP_RATIO: f64 = 0.75;
/// Font 0 advance of `H` relative to the width parameter.
const FONT0_H_ADVANCE: f64 = 0.6084;

/// Kind specific data of a resolved font.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FontKind {
    /// Scalable font 0. `width` is the width parameter in dots.
    Scalable { width: f64 },
    /// Fixed pitch font with an advance in dots.
    Fixed { advance: f64, case: CaseMode },
}

/// A font at a concrete size, ready for layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedFont {
    /// Face used to draw glyphs.
    pub face: FaceId,
    /// Em size of the face in dots.
    pub em: f64,
    /// Horizontal scale applied to glyph shapes.
    pub scale_x: f64,
    /// Cell height in dots (line height and rotation box height).
    pub cell_height: f64,
    /// Distance from the cell top to the baseline.
    pub baseline: f64,
    /// Width related metrics.
    pub kind: FontKind,
}

/// The default font settings from `^CF`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DefaultFont {
    /// Font name.
    pub name: u8,
    /// Height parameter.
    pub height: Option<f64>,
    /// Width parameter.
    pub width: Option<f64>,
}

impl Default for DefaultFont {
    fn default() -> Self {
        Self {
            name: b'A',
            height: Some(9.0),
            width: Some(5.0),
        }
    }
}

impl DefaultFont {
    fn sizes(&self) -> (f64, f64) {
        match (self.height, self.width) {
            (Some(h), Some(w)) => (h, w),
            (Some(h), None) => (h, h),
            (None, Some(w)) => (w, w),
            (None, None) => (9.0, 5.0),
        }
    }
}

fn mag(requested: f64, base: f64) -> f64 {
    (requested / base).round().max(1.0)
}

/// Resolves a font name and the optional `^A` sizes into a concrete font.
pub fn resolve(
    name: u8,
    height: Option<f64>,
    width: Option<f64>,
    default: &DefaultFont,
    dpmm: u8,
) -> ResolvedFont {
    let height = height.filter(|v| *v > 0.0);
    let width = width.filter(|v| *v > 0.0);
    if let Some(fm) = fixed_metrics(name, dpmm) {
        let (my, mx) = match (height, width) {
            (Some(h), Some(w)) => (mag(h, fm.height), mag(w, fm.width)),
            (Some(h), None) => {
                let m = mag(h, fm.height);
                (m, m)
            }
            (None, Some(w)) => {
                let m = mag(w, fm.width);
                (m, m)
            }
            (None, None) => {
                // ^CF sizes; without a ^CF width the horizontal
                // magnification follows the vertical one.
                let my = mag(
                    default.height.or(default.width).unwrap_or(fm.height),
                    fm.height,
                );
                let mx = default.width.map(|w| mag(w, fm.width)).unwrap_or(my);
                (my, mx)
            }
        };
        let f = face(fm.face);
        let cap = fm.cap_height * my;
        let em = cap * f.units_per_em / f.cap_height;
        let advance = fm.advance * mx;
        let natural = f
            .glyph('H')
            .map(|g| f.advance(g))
            .unwrap_or(f.units_per_em * 0.6)
            * em
            / f.units_per_em;
        return ResolvedFont {
            face: fm.face,
            em,
            scale_x: if natural > 0.0 {
                advance / natural
            } else {
                1.0
            },
            cell_height: fm.height * my,
            baseline: (fm.cap_top + fm.cap_height) * my,
            kind: FontKind::Fixed {
                advance,
                case: fm.case,
            },
        };
    }
    // Scalable font 0 (also used for unknown or downloaded fonts).
    let (h, w) = match (height, width) {
        (Some(h), Some(w)) => (h, w),
        (Some(h), None) => (h, h),
        (None, Some(w)) => (w, w),
        (None, None) => {
            if fixed_metrics(default.name, dpmm).is_none() {
                default.sizes()
            } else {
                let (h, _) = default.sizes();
                (h, h)
            }
        }
    };
    let f = face(FaceId::Sans);
    let cap = FONT0_CAP_RATIO * h;
    let em = cap * f.units_per_em / f.cap_height;
    let natural_h = f
        .glyph('H')
        .map(|g| f.advance(g))
        .unwrap_or(f.units_per_em * 0.6)
        * em
        / f.units_per_em;
    ResolvedFont {
        face: FaceId::Sans,
        em,
        scale_x: if natural_h > 0.0 {
            FONT0_H_ADVANCE * w / natural_h
        } else {
            1.0
        },
        cell_height: h,
        baseline: cap,
        kind: FontKind::Scalable { width: w },
    }
}

/// A glyph placed on a line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedGlyph {
    /// Character (for text extraction).
    pub ch: char,
    /// Glyph id in the face.
    pub gid: u16,
    /// Horizontal position of the glyph origin, in dots from the line start.
    pub x: f64,
}

impl ResolvedFont {
    /// Character that is actually drawn, `None` for blank characters.
    pub fn map_char(&self, ch: char) -> Option<char> {
        match self.kind {
            FontKind::Fixed {
                case: CaseMode::UpperOnly,
                ..
            } => ch.to_uppercase().next(),
            FontKind::Fixed {
                case: CaseMode::UpperBlank,
                ..
            } if ch.is_lowercase() => None,
            // Zebra's font 0 hyphen is as wide as an en dash.
            FontKind::Scalable { .. } if ch == '-' => Some('\u{2013}'),
            _ => Some(ch),
        }
    }

    /// Natural advance of a glyph of this font's face, in dots.
    fn natural_advance(&self, gid: u16) -> f64 {
        let f = face(self.face);
        f.advance(gid) * self.em / f.units_per_em * self.scale_x
    }

    /// Advance of a character in dots.
    pub fn advance(&self, ch: char) -> f64 {
        match self.kind {
            FontKind::Fixed { advance, .. } => advance,
            FontKind::Scalable { width } => match font0_advance(ch) {
                Some(a) => a * width,
                None => {
                    let f = face(self.face);
                    match f.glyph(ch) {
                        Some(g) => self.natural_advance(g),
                        None => 0.5 * width,
                    }
                }
            },
        }
    }

    /// Lays out a single line. `gap` is added after every character (`^FP`).
    /// Returns the glyphs and the total advance.
    pub fn layout(&self, text: &str, gap: f64) -> (Vec<PlacedGlyph>, f64) {
        self.layout_spaced(text, gap, 0.0)
    }

    /// Like [`ResolvedFont::layout`] with `space_extra` dots added to every
    /// space (used for justified text).
    pub fn layout_spaced(&self, text: &str, gap: f64, space_extra: f64) -> (Vec<PlacedGlyph>, f64) {
        let f = face(self.face);
        let mut x = 0.0;
        let mut glyphs = Vec::with_capacity(text.len());
        for ch in text.chars() {
            if ch.is_control() {
                continue;
            }
            let adv = self.advance(ch);
            if let Some(drawn) = self.map_char(ch)
                && !drawn.is_whitespace()
                && let Some(gid) = f
                    .glyph(drawn)
                    .or_else(|| fallback_char(drawn).and_then(|c| f.glyph(c)))
            {
                // Center the substitute glyph in the Zebra advance.
                let offset = (adv - self.natural_advance(gid)) / 2.0;
                glyphs.push(PlacedGlyph {
                    ch,
                    gid,
                    x: x + offset,
                });
            }
            x += adv + gap;
            if ch == ' ' {
                x += space_extra;
            }
        }
        (glyphs, x)
    }

    /// Width of a line of text in dots.
    pub fn measure(&self, text: &str, gap: f64) -> f64 {
        text.chars()
            .filter(|c| !c.is_control())
            .map(|c| self.advance(c) + gap)
            .sum()
    }
}

/// Simple fallbacks for characters missing in the embedded fonts.
fn fallback_char(ch: char) -> Option<char> {
    Some(match ch {
        '‗' => '_',
        '\u{A0}' => ' ',
        '“' | '”' | '„' => '"',
        '‘' | '’' | '‚' => '\'',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_load() {
        for id in FaceId::ALL {
            let f = face(id);
            assert!(f.units_per_em > 0.0 && f.cap_height > 0.0);
            assert!(f.glyph('H').is_some(), "{id:?}");
            assert!(f.glyph('Ğ').is_some(), "{id:?}");
        }
    }

    #[test]
    fn font0_metrics_match_zebra() {
        let f = resolve(b'0', Some(100.0), Some(100.0), &DefaultFont::default(), 8);
        assert!((f.baseline - 75.0).abs() < 1e-9);
        assert!((f.advance('H') - 60.84).abs() < 0.01);
        let (_, w) = f.layout("HHHH", 0.0);
        assert!((w - 243.36).abs() < 0.1);
    }

    #[test]
    fn fixed_font_magnification() {
        let d = DefaultFont::default();
        let f = resolve(b'A', Some(14.0), Some(10.0), &d, 8);
        assert_eq!(f.cell_height, 18.0);
        assert_eq!(
            f.kind,
            FontKind::Fixed {
                advance: 12.0,
                case: CaseMode::Mixed
            }
        );
        let f = resolve(b'A', Some(13.0), Some(8.0), &d, 8);
        assert_eq!(f.cell_height, 9.0);
        assert_eq!(
            f.kind,
            FontKind::Fixed {
                advance: 12.0,
                case: CaseMode::Mixed
            }
        );
        let f = resolve(b'D', Some(36.0), None, &d, 8);
        assert_eq!(
            f.kind,
            FontKind::Fixed {
                advance: 24.0,
                case: CaseMode::Mixed
            }
        );
        assert_eq!(f.baseline, 28.0);
    }

    #[test]
    fn default_font_is_a() {
        let f = resolve(b'A', None, None, &DefaultFont::default(), 8);
        assert_eq!(f.cell_height, 9.0);
    }

    #[test]
    fn case_modes() {
        let d = DefaultFont::default();
        let b = resolve(b'B', None, None, &d, 8);
        assert_eq!(b.map_char('x'), Some('X'));
        let h = resolve(b'H', None, None, &d, 8);
        assert_eq!(h.map_char('x'), None);
        assert_eq!(h.map_char('X'), Some('X'));
    }
}
