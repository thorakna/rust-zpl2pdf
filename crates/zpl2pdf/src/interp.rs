//! Command interpreter: executes ZPL commands and builds scene labels.
//!
//! Placement rules (field origins, rotation boxes, bar code interpretation
//! lines, QR offsets, ...) follow measurements of Labelary renderings, see
//! `tools/calibration`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::barcode::{linear, matrix, maxicode};
use crate::encoding;
use crate::font::{self, DefaultFont, ResolvedFont};
use crate::graphic::{self, Bitmap};
use crate::parser::{BarcodeCommand, BarcodeKind, Command, RawCommand};
use crate::scene::{self, Item, Label, Paint, Path, TextRun};
use crate::types::{Justify, LineColor, Orientation, Transform, place_box};
use crate::{Options, Warning};

/// Settings that persist across fields and labels (printer state).
#[derive(Debug, Clone)]
struct Settings {
    label_home: (f64, f64),
    label_shift: f64,
    label_top: f64,
    print_width: Option<f64>,
    inverted: bool,
    mirrored: bool,
    label_reverse: bool,
    encoding: u32,
    default_font: DefaultFont,
    font_aliases: HashMap<u8, String>,
    module_width: f64,
    ratio: f64,
    bar_height: f64,
    orientation: Orientation,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            label_home: (0.0, 0.0),
            label_shift: 0.0,
            label_top: 0.0,
            print_width: None,
            inverted: false,
            mirrored: false,
            label_reverse: false,
            encoding: 0,
            default_font: DefaultFont::default(),
            font_aliases: HashMap::new(),
            module_width: 2.0,
            ratio: 3.0,
            bar_height: 10.0,
            orientation: Orientation::Normal,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Origin {
    x: f64,
    y: f64,
    typeset: bool,
    right_justify: bool,
}

#[derive(Debug, Clone, Copy)]
struct FontChoice {
    name: u8,
    orientation: Option<Orientation>,
    height: Option<f64>,
    width: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
struct Block {
    width: f64,
    max_lines: u32,
    spacing: f64,
    justify: Justify,
    hanging: f64,
}

#[derive(Debug, Clone, Copy)]
struct TextBlock {
    orientation: Option<Orientation>,
    width: Option<f64>,
    height: Option<f64>,
}

#[derive(Debug, Clone)]
enum Graphic {
    Box {
        width: Option<f64>,
        height: Option<f64>,
        thickness: Option<f64>,
        color: LineColor,
        rounding: f64,
    },
    Circle {
        diameter: f64,
        thickness: f64,
        color: LineColor,
    },
    Ellipse {
        width: Option<f64>,
        height: Option<f64>,
        thickness: Option<f64>,
        color: LineColor,
    },
    Diagonal {
        width: Option<f64>,
        height: Option<f64>,
        thickness: Option<f64>,
        color: LineColor,
        right_leaning: bool,
    },
    Image(Arc<Bitmap>),
    Recall {
        name: String,
        mag_x: f64,
        mag_y: f64,
    },
}

/// State of the field being built (between `^FO`/`^FT` and `^FS`).
#[derive(Debug, Clone, Default)]
struct Field {
    origin: Option<Origin>,
    font: Option<FontChoice>,
    data: Option<Vec<u8>>,
    hex: Option<u8>,
    reverse: bool,
    number: Option<u32>,
    block: Option<Block>,
    text_block: Option<TextBlock>,
    gap: f64,
    direction: u8,
    barcode: Option<BarcodeCommand>,
    graphic: Option<Graphic>,
    symbol: Option<FontChoice>,
}

impl Field {
    fn has_content(&self) -> bool {
        self.data.is_some()
            || self.graphic.is_some()
            || self.barcode.is_some()
            || self.number.is_some()
    }
}

enum Chunk {
    Items(Vec<Item>),
    /// A `^FN` field whose data may be supplied later in the label.
    Deferred(Box<(Field, Settings)>),
}

struct LabelState {
    chunks: Vec<Chunk>,
    field: Field,
    fn_values: HashMap<u32, Vec<u8>>,
    quantity: u32,
    next_position: (f64, f64),
    index: usize,
    suppressed: bool,
}

/// Executes ZPL commands and collects rendered labels.
pub struct Interpreter<'a> {
    opts: &'a Options,
    dpmm: u8,
    dpi: f64,
    page: (f64, f64),
    settings: Settings,
    graphics: HashMap<String, Arc<Bitmap>>,
    formats: HashMap<String, Vec<(RawCommand, Command)>>,
    label: Option<LabelState>,
    recording: Option<(String, Vec<(RawCommand, Command)>)>,
    labels: Vec<Label>,
    warnings: Vec<Warning>,
    warned: HashSet<(Option<usize>, String)>,
    label_count: usize,
    offset: usize,
    command: String,
    depth: usize,
}

const INTERPRETATION_GAP_BELOW: f64 = 6.0;
const INTERPRETATION_GAP_ABOVE: f64 = 8.0;
/// Code 128 mode D prints its interpretation line in font 0 at this size
/// per dot of module width.
const GS1_TEXT_SIZE_PER_MODULE: f64 = 15.3;
/// EAN/UPC guard bars extend this many dots below the other bars.
const EAN_GUARD_EXTENSION: f64 = 13.0;

impl<'a> Interpreter<'a> {
    /// Creates an interpreter for the given options.
    pub fn new(opts: &'a Options) -> Self {
        let dpi = scene::dpmm_to_dpi(opts.dpmm);
        let page = ((opts.width * dpi).floor(), (opts.height * dpi).floor());
        Self {
            opts,
            dpmm: opts.dpmm,
            dpi,
            page,
            settings: Settings::default(),
            graphics: HashMap::new(),
            formats: HashMap::new(),
            label: None,
            recording: None,
            labels: Vec::new(),
            warnings: Vec::new(),
            warned: HashSet::new(),
            label_count: 0,
            offset: 0,
            command: String::new(),
            depth: 0,
        }
    }

    /// Number of labels produced so far.
    pub fn label_count(&self) -> usize {
        self.labels.len()
    }

    fn warn(&mut self, message: impl Into<String>) {
        let message = message.into();
        let label = self.label.as_ref().map(|l| l.index);
        if self
            .warned
            .insert((label, format!("{}{}", self.command, message)))
        {
            self.warnings.push(Warning {
                label,
                offset: self.offset,
                command: self.command.clone(),
                message,
            });
        }
    }

    /// Finishes processing and returns the labels and warnings.
    pub fn finish(mut self) -> (Vec<Label>, Vec<Warning>) {
        if self.recording.is_some() {
            self.command = "^DF".into();
            self.warn("format download not terminated by ^XZ");
            if let Some((name, cmds)) = self.recording.take() {
                self.formats.insert(name, cmds);
            }
            if let Some(l) = self.label.as_mut() {
                l.suppressed = true;
            }
        }
        if self.label.is_some() {
            self.command = "^XZ".into();
            self.warn("label not terminated by ^XZ");
            self.end_format();
        }
        (self.labels, self.warnings)
    }

    fn label_limit_reached(&self) -> bool {
        self.labels.len() >= self.opts.limits.max_labels
    }

    /// Executes one command.
    pub fn exec(&mut self, raw: &RawCommand, cmd: &Command) {
        self.offset = raw.offset;
        self.command = raw.name();
        if let Some((_, cmds)) = self.recording.as_mut() {
            if matches!(cmd, Command::EndFormat) {
                if let Some((name, cmds)) = self.recording.take() {
                    self.formats.insert(name, cmds);
                }
                if let Some(l) = self.label.as_mut() {
                    l.suppressed = true;
                }
                self.end_format();
            } else {
                cmds.push((raw.clone(), cmd.clone()));
            }
            return;
        }
        match cmd {
            Command::StartFormat => {
                if self.label.is_some() {
                    self.warn("^XA inside an open label; closing the previous label");
                    self.end_format();
                }
                self.label_count += 1;
                self.label = Some(LabelState {
                    chunks: Vec::new(),
                    field: Field::default(),
                    fn_values: HashMap::new(),
                    quantity: 1,
                    next_position: (0.0, 0.0),
                    index: self.label_count - 1,
                    suppressed: false,
                });
            }
            Command::EndFormat => {
                if self.label.is_none() {
                    return;
                }
                self.end_format();
            }
            Command::DownloadFormat { name } => {
                self.recording = Some((name.clone(), Vec::new()));
            }
            Command::RecallFormat { name } => {
                let Some(cmds) = self.formats.get(name).cloned() else {
                    self.warn(format!("format {name} not found"));
                    return;
                };
                if self.depth >= 8 {
                    self.warn("format recall nested too deeply");
                    return;
                }
                self.depth += 1;
                for (r, c) in &cmds {
                    if matches!(c, Command::StartFormat | Command::EndFormat) {
                        continue;
                    }
                    self.exec(r, c);
                }
                self.depth -= 1;
            }
            // Printer settings: allowed inside and outside labels.
            Command::LabelHome { x, y } => self.settings.label_home = (*x, *y),
            Command::LabelShift(v) => self.settings.label_shift = *v,
            Command::LabelTop(v) => self.settings.label_top = v.clamp(-120.0, 120.0),
            Command::LabelReverse(v) => self.settings.label_reverse = *v,
            Command::PrintWidth(w) => self.settings.print_width = Some(*w),
            Command::LabelLength(_) => {}
            Command::PrintOrientation { inverted } => self.settings.inverted = *inverted,
            Command::PrintMirror(v) => self.settings.mirrored = *v,
            Command::ChangeEncoding(ci) => self.settings.encoding = *ci,
            Command::ChangeFont {
                name,
                height,
                width,
            } => {
                let d = &mut self.settings.default_font;
                if let Some(n) = name {
                    d.name = *n;
                }
                if height.is_some() || width.is_some() {
                    d.height = *height;
                    d.width = *width;
                }
            }
            Command::FontIdentifier { letter, path } => {
                self.settings.font_aliases.insert(*letter, path.clone());
            }
            Command::BarcodeDefaults {
                module_width,
                ratio,
                height,
            } => {
                if let Some(w) = module_width {
                    self.settings.module_width = w.clamp(1.0, 10.0).floor();
                }
                if let Some(r) = ratio {
                    self.settings.ratio = r.clamp(2.0, 3.0);
                }
                if let Some(h) = height {
                    self.settings.bar_height = h.max(1.0);
                }
            }
            Command::FieldOrientation { orientation, .. } => {
                if let Some(o) = orientation {
                    self.settings.orientation = *o;
                }
            }
            Command::Units(u) => {
                if *u != b'D' {
                    self.warn("only dot units (^MUD) are supported; values are treated as dots");
                }
            }
            Command::DownloadGraphic {
                name,
                total_bytes,
                row_bytes,
                data,
            } => {
                match graphic::decode_graphic(
                    b'A',
                    *total_bytes,
                    *row_bytes,
                    data,
                    self.opts.limits.max_graphic_bytes,
                ) {
                    Ok(d) => {
                        for w in d.warnings {
                            self.warn(w);
                        }
                        self.graphics.insert(name.clone(), Arc::new(d.bitmap));
                    }
                    Err(e) => self.warn(e),
                }
            }
            Command::DownloadObject {
                name,
                format,
                extension,
                total_bytes,
                row_bytes,
                data,
            } => {
                self.download_object(name, *format, *extension, *total_bytes, *row_bytes, data);
            }
            Command::DeleteObject { name } => {
                if name.contains('*') {
                    self.graphics.clear();
                } else {
                    self.graphics.remove(name);
                }
            }
            Command::EraseGraphics => self.graphics.clear(),
            Command::Ignored | Command::Comment => {}
            Command::Unknown => {
                self.warn("unsupported command ignored");
            }
            // Field level commands need an open label.
            _ => {
                if self.label.is_none() {
                    self.warn("command outside of ^XA ... ^XZ ignored");
                    return;
                }
                self.exec_field(cmd);
            }
        }
    }

    fn download_object(
        &mut self,
        name: &str,
        format: u8,
        extension: u8,
        total: usize,
        row_bytes: usize,
        data: &[u8],
    ) {
        let max = self.opts.limits.max_graphic_bytes;
        let bytes: Vec<u8> = if format == b'A' && extension == b'P' {
            // ASCII hex encoded PNG.
            let hex: Vec<u8> = data
                .iter()
                .copied()
                .filter(|b| b.is_ascii_hexdigit())
                .collect();
            hex.chunks(2)
                .map(|p| {
                    let s = std::str::from_utf8(p).unwrap_or("0");
                    u8::from_str_radix(s, 16).unwrap_or(0)
                })
                .collect()
        } else {
            data.to_vec()
        };
        let result = match extension {
            b'P' => graphic::decode_png(&bytes, max).map(|b| (b, Vec::new())),
            b'G' => graphic::decode_graphic(format, total, row_bytes, &bytes, max)
                .map(|d| (d.bitmap, d.warnings)),
            _ => {
                self.warn("only GRF and PNG objects are supported by ~DY");
                return;
            }
        };
        match result {
            Ok((bmp, warnings)) => {
                for w in warnings {
                    self.warn(w);
                }
                self.graphics.insert(name.to_string(), Arc::new(bmp));
            }
            Err(e) => self.warn(e),
        }
    }

    fn field(&mut self) -> &mut Field {
        &mut self.label.as_mut().expect("label is open").field
    }

    fn exec_field(&mut self, cmd: &Command) {
        match cmd {
            Command::FieldOrigin { x, y, justify } | Command::FieldTypeset { x, y, justify } => {
                if self.field().has_content() {
                    self.finish_field();
                }
                let typeset = matches!(cmd, Command::FieldTypeset { .. });
                let next = self
                    .label
                    .as_ref()
                    .map(|l| l.next_position)
                    .unwrap_or_default();
                let s = &self.settings;
                let (dx, dy) = (s.label_home.0 - s.label_shift, s.label_home.1 + s.label_top);
                let (x, y) = if typeset {
                    (
                        x.map(|v| v + dx).unwrap_or(next.0),
                        y.map(|v| v + dy).unwrap_or(next.1),
                    )
                } else {
                    (x.unwrap_or(0.0) + dx, y.unwrap_or(0.0) + dy)
                };
                self.field().origin = Some(Origin {
                    x,
                    y,
                    typeset,
                    right_justify: *justify == Some(b'1'),
                });
            }
            Command::FieldSeparator => self.finish_field(),
            Command::FieldData(d) | Command::FieldVariable(d) => {
                let f = self.field();
                match &mut f.data {
                    Some(existing) => existing.extend_from_slice(d),
                    None => f.data = Some(d.clone()),
                }
            }
            Command::FieldHex(c) => self.field().hex = Some(*c),
            Command::FieldReverse => self.field().reverse = true,
            Command::FieldNumber(n) => self.field().number = Some(*n),
            Command::FieldBlock {
                width,
                max_lines,
                line_spacing,
                justify,
                hanging_indent,
            } => {
                self.field().block = Some(Block {
                    width: *width,
                    max_lines: (*max_lines).clamp(1, 9999),
                    spacing: *line_spacing,
                    justify: *justify,
                    hanging: *hanging_indent,
                });
            }
            Command::TextBlock {
                orientation,
                width,
                height,
            } => {
                self.field().text_block = Some(TextBlock {
                    orientation: *orientation,
                    width: *width,
                    height: *height,
                });
            }
            Command::FieldParameter { direction, gap } => {
                let f = self.field();
                f.direction = *direction;
                f.gap = gap.max(0.0);
            }
            Command::Font {
                name,
                orientation,
                height,
                width,
            } => {
                self.field().font = Some(FontChoice {
                    name: *name,
                    orientation: *orientation,
                    height: *height,
                    width: *width,
                });
            }
            Command::FontByName {
                orientation,
                height,
                width,
                path,
            } => {
                self.warn(format!("font {path} is not available; using font 0"));
                self.field().font = Some(FontChoice {
                    name: b'0',
                    orientation: *orientation,
                    height: *height,
                    width: *width,
                });
            }
            Command::Barcode(b) => self.field().barcode = Some(b.clone()),
            Command::GraphicBox {
                width,
                height,
                thickness,
                color,
                rounding,
            } => {
                self.field().graphic = Some(Graphic::Box {
                    width: *width,
                    height: *height,
                    thickness: *thickness,
                    color: *color,
                    rounding: *rounding,
                });
            }
            Command::GraphicCircle {
                diameter,
                thickness,
                color,
            } => {
                self.field().graphic = Some(Graphic::Circle {
                    diameter: *diameter,
                    thickness: *thickness,
                    color: *color,
                });
            }
            Command::GraphicEllipse {
                width,
                height,
                thickness,
                color,
            } => {
                self.field().graphic = Some(Graphic::Ellipse {
                    width: *width,
                    height: *height,
                    thickness: *thickness,
                    color: *color,
                });
            }
            Command::GraphicDiagonal {
                width,
                height,
                thickness,
                color,
                right_leaning,
            } => {
                self.field().graphic = Some(Graphic::Diagonal {
                    width: *width,
                    height: *height,
                    thickness: *thickness,
                    color: *color,
                    right_leaning: *right_leaning,
                });
            }
            Command::GraphicField {
                format,
                binary_bytes: _,
                total_bytes,
                row_bytes,
                data,
            } => {
                match graphic::decode_graphic(
                    *format,
                    *total_bytes,
                    *row_bytes,
                    data,
                    self.opts.limits.max_graphic_bytes,
                ) {
                    Ok(d) => {
                        for w in d.warnings {
                            self.warn(w);
                        }
                        self.field().graphic = Some(Graphic::Image(Arc::new(d.bitmap)));
                    }
                    Err(e) => self.warn(e),
                }
            }
            Command::GraphicSymbol {
                orientation,
                height,
                width,
            } => {
                self.field().symbol = Some(FontChoice {
                    name: b'0',
                    orientation: *orientation,
                    height: *height,
                    width: *width,
                });
            }
            Command::RecallGraphic { name, mag_x, mag_y } => {
                self.field().graphic = Some(Graphic::Recall {
                    name: name.clone(),
                    mag_x: *mag_x,
                    mag_y: *mag_y,
                });
            }
            Command::ImageMove { name } => {
                if self.field().origin.is_none() {
                    self.field().origin = Some(Origin {
                        x: 0.0,
                        y: 0.0,
                        typeset: false,
                        right_justify: false,
                    });
                }
                self.field().graphic = Some(Graphic::Recall {
                    name: name.clone(),
                    mag_x: 1.0,
                    mag_y: 1.0,
                });
                self.finish_field();
            }
            Command::PrintQuantity(q) => {
                if let Some(l) = self.label.as_mut() {
                    l.quantity = *q;
                }
            }
            Command::SerializationData { start, .. } => {
                self.warn("serialization is rendered for the first label only");
                let f = self.field();
                f.data = Some(start.clone());
            }
            Command::SerializationField { .. } => {
                self.warn("serialization is rendered for the first label only");
            }
            _ => {}
        }
    }

    fn end_format(&mut self) {
        if self.label.as_ref().is_some_and(|l| l.field.has_content()) {
            self.finish_field();
        }
        let Some(state) = self.label.take() else {
            return;
        };
        if state.suppressed {
            return;
        }
        let mut items = Vec::new();
        for chunk in state.chunks {
            match chunk {
                Chunk::Items(mut v) => items.append(&mut v),
                Chunk::Deferred(def) => {
                    let (field, settings) = *def;
                    let data = field
                        .number
                        .and_then(|n| state.fn_values.get(&n).cloned())
                        .or_else(|| field.data.clone());
                    if let Some(data) = data {
                        let mut v = self.render_field(&field, &settings, Some(data));
                        items.append(&mut v);
                    }
                }
            }
        }
        let s = &self.settings;
        let mut clip = None;
        if let Some(pw) = s.print_width
            && pw > 0.0
            && pw < self.page.0
        {
            let dx = ((self.page.0 - pw) / 2.0).floor();
            for item in &mut items {
                item.translate(dx, 0.0);
            }
            clip = Some((dx, 0.0, pw, self.page.1));
        }
        let label = Label {
            width: self.page.0,
            height: self.page.1,
            dpmm: self.dpmm,
            dpi: self.dpi,
            items,
            clip,
            inverted: s.inverted,
            mirrored: s.mirrored,
        };
        let copies = if self.opts.honor_quantity {
            state.quantity.max(1) as usize
        } else {
            1
        };
        for _ in 0..copies {
            if self.label_limit_reached() {
                break;
            }
            self.labels.push(label.clone());
        }
    }

    fn finish_field(&mut self) {
        let Some(state) = self.label.as_mut() else {
            return;
        };
        let field = std::mem::take(&mut state.field);
        if !field.has_content() {
            return;
        }
        if let Some(n) = field.number {
            if field.origin.is_none() {
                if let Some(d) = &field.data {
                    state.fn_values.insert(n, d.clone());
                }
                return;
            }
            let settings = self.settings.clone();
            if let Some(d) = &field.data {
                state.fn_values.entry(n).or_insert_with(|| d.clone());
            }
            state
                .chunks
                .push(Chunk::Deferred(Box::new((field, settings))));
            return;
        }
        let settings = self.settings.clone();
        let data = field.data.clone();
        let items = self.render_field(&field, &settings, data);
        if let Some(state) = self.label.as_mut() {
            state.chunks.push(Chunk::Items(items));
        }
    }

    fn paint(field: &Field, settings: &Settings, color: LineColor) -> Paint {
        if field.reverse || settings.label_reverse {
            Paint::Reverse
        } else if color == LineColor::White {
            Paint::White
        } else {
            Paint::Black
        }
    }

    fn origin(field: &Field) -> Origin {
        field.origin.unwrap_or(Origin {
            x: 0.0,
            y: 0.0,
            typeset: false,
            right_justify: false,
        })
    }

    fn render_field(&mut self, field: &Field, s: &Settings, data: Option<Vec<u8>>) -> Vec<Item> {
        if let Some(g) = &field.graphic {
            return self.render_graphic(field, s, g);
        }
        let bytes = data.map(|d| match field.hex {
            Some(ind) => encoding::apply_hex_escapes(&d, ind),
            None => d,
        });
        if let Some(b) = &field.barcode {
            let Some(bytes) = bytes else {
                return Vec::new();
            };
            return self.render_barcode(field, s, b, &bytes);
        }
        let Some(bytes) = bytes else {
            return Vec::new();
        };
        let mut text = encoding::decode(&bytes, s.encoding);
        if let Some(sym) = field.symbol {
            text = text
                .chars()
                .map(|c| match c.to_ascii_uppercase() {
                    'A' => "®".to_string(),
                    'B' => "©".to_string(),
                    'C' => "™".to_string(),
                    'D' => "UL".to_string(),
                    'E' => "CSA".to_string(),
                    other => other.to_string(),
                })
                .collect();
            let mut f = field.clone();
            f.font = Some(sym);
            return self.render_text(&f, s, &text);
        }
        self.render_text(field, s, &text)
    }

    // ----------------------------------------------------------------- text

    fn resolve_font(&mut self, choice: FontChoice, s: &Settings) -> ResolvedFont {
        let mut name = choice.name;
        if s.font_aliases.contains_key(&name) {
            self.warn(format!(
                "downloaded font for '{}' is not available; using font 0",
                name as char
            ));
            name = b'0';
        } else if font::fixed_metrics(name, self.dpmm).is_none() && name != b'0' {
            name = b'0';
        }
        font::resolve(
            name,
            choice.height,
            choice.width,
            &s.default_font,
            self.dpmm,
        )
    }

    fn render_text(&mut self, field: &Field, s: &Settings, text: &str) -> Vec<Item> {
        let choice = field.font.unwrap_or(FontChoice {
            name: s.default_font.name,
            orientation: None,
            height: None,
            width: None,
        });
        let orientation = choice
            .orientation
            .or(field.text_block.and_then(|t| t.orientation))
            .unwrap_or(s.orientation);
        let font = self.resolve_font(choice, s);
        let paint = Self::paint(field, s, LineColor::Black);
        let origin = Self::origin(field);
        let gap = field.gap;

        // Lines: (text, x offset, space extra, line index for placement).
        let mut lines: Vec<(String, f64, f64, usize)> = Vec::new();
        let (box_w, box_h, pitch, ft_anchor_y);
        if let Some(b) = field.block {
            let pitch_v = font.cell_height + b.spacing;
            let wrapped = wrap(&unescape_block(text), b.width, &font, gap, b.hanging);
            let last_line = b.max_lines as usize - 1;
            for (i, (line, paragraph_end)) in wrapped.iter().enumerate() {
                let indent = if i > 0 { b.hanging } else { 0.0 };
                let avail = (b.width - indent).max(0.0);
                let lw = font.measure(line, gap);
                let (x, extra) = match b.justify {
                    Justify::Left => (indent, 0.0),
                    Justify::Center => (indent + (avail - lw) / 2.0, 0.0),
                    Justify::Right => (indent + avail - lw, 0.0),
                    Justify::Justified => {
                        let spaces = line.chars().filter(|c| *c == ' ').count();
                        if *paragraph_end || spaces == 0 {
                            (indent, 0.0)
                        } else {
                            (indent, ((avail - lw) / spaces as f64).max(0.0))
                        }
                    }
                };
                lines.push((line.clone(), x, extra, i.min(last_line)));
            }
            box_w = b.width;
            box_h = b.max_lines as f64 * pitch_v;
            pitch = pitch_v;
            ft_anchor_y = last_line as f64 * pitch_v + font.baseline;
        } else if let Some(tb) = field.text_block {
            let pitch_v = font.cell_height;
            let width = tb.width.unwrap_or_else(|| font.measure(text, gap));
            let height = tb.height.unwrap_or(pitch_v);
            let max_lines = ((height / pitch_v).floor() as usize).max(1);
            for (i, (line, _)) in wrap(&unescape_block(text), width, &font, gap, 0.0)
                .into_iter()
                .take(max_lines)
                .enumerate()
            {
                lines.push((line, 0.0, 0.0, i));
            }
            box_w = width;
            box_h = height;
            pitch = pitch_v;
            ft_anchor_y = font.baseline;
        } else if field.direction == b'V' {
            // Vertical: one character per line.
            for (i, ch) in text.chars().enumerate() {
                lines.push((ch.to_string(), 0.0, 0.0, i));
            }
            let w = text.chars().map(|c| font.advance(c)).fold(0.0, f64::max);
            pitch = font.cell_height + gap;
            box_w = w;
            box_h = lines.len().max(1) as f64 * pitch;
            ft_anchor_y = font.baseline;
        } else {
            let line: String = if field.direction == b'R' {
                text.chars().rev().collect()
            } else {
                text.to_string()
            };
            box_w = font.measure(&line, gap);
            lines.push((line, 0.0, 0.0, 0));
            box_h = font.cell_height;
            pitch = font.cell_height;
            ft_anchor_y = font.baseline;
        }

        let mut x = origin.x;
        if origin.right_justify && orientation == Orientation::Normal {
            x -= box_w;
        }
        let anchor = origin.typeset.then_some((0.0, ft_anchor_y));
        let place = place_box(orientation, box_w, box_h, x, origin.y, anchor);
        let mut items = Vec::with_capacity(lines.len());
        for (line, lx, extra, slot) in &lines {
            let (glyphs, _) = font.layout_spaced(line, gap, *extra);
            if glyphs.is_empty() {
                continue;
            }
            let baseline = *slot as f64 * pitch + font.baseline;
            let transform = Transform::translate(*lx, baseline).then(&place);
            items.push(Item::Text(TextRun {
                font,
                glyphs,
                transform,
                paint,
            }));
        }
        if let Some(state) = self.label.as_mut() {
            let (nx, ny) = place.apply(box_w, ft_anchor_y);
            state.next_position = (nx, ny);
        }
        items
    }

    // ------------------------------------------------------------- graphics

    fn render_graphic(&mut self, field: &Field, s: &Settings, g: &Graphic) -> Vec<Item> {
        let origin = Self::origin(field);
        let (path, w, h, color) = match g {
            Graphic::Box {
                width,
                height,
                thickness,
                color,
                rounding,
            } => {
                let t = thickness.unwrap_or(1.0).max(1.0);
                let w = width.unwrap_or(t).max(t);
                let h = height.unwrap_or(t).max(t);
                let r = rounding / 8.0 * w.min(h) / 2.0;
                let mut p = Path::new();
                p.rounded_rect(0.0, 0.0, w, h, r);
                if 2.0 * t < w && 2.0 * t < h {
                    p.rounded_rect(t, t, w - 2.0 * t, h - 2.0 * t, (r - t).max(0.0));
                }
                (p, w, h, *color)
            }
            Graphic::Circle {
                diameter,
                thickness,
                color,
            } => {
                let d = diameter.max(1.0);
                let t = thickness.max(1.0);
                let mut p = Path::new();
                p.ellipse(d / 2.0, d / 2.0, d / 2.0, d / 2.0);
                if 2.0 * t < d {
                    p.ellipse(d / 2.0, d / 2.0, d / 2.0 - t, d / 2.0 - t);
                }
                (p, d, d, *color)
            }
            Graphic::Ellipse {
                width,
                height,
                thickness,
                color,
            } => {
                let t = thickness.unwrap_or(1.0).max(1.0);
                let w = width.unwrap_or(t).max(t);
                let h = height.unwrap_or(t).max(t);
                let mut p = Path::new();
                p.ellipse(w / 2.0, h / 2.0, w / 2.0, h / 2.0);
                if 2.0 * t < w && 2.0 * t < h {
                    p.ellipse(w / 2.0, h / 2.0, w / 2.0 - t, h / 2.0 - t);
                }
                (p, w, h, *color)
            }
            Graphic::Diagonal {
                width,
                height,
                thickness,
                color,
                right_leaning,
            } => {
                let t = thickness.unwrap_or(1.0).max(1.0);
                let w = width.unwrap_or(t).max(t);
                let h = height.unwrap_or(t).max(t);
                let mut p = Path::new();
                if *right_leaning {
                    p.polygon(&[(0.0, h), (t, h), (w + t, 0.0), (w, 0.0)]);
                } else {
                    p.polygon(&[(0.0, 0.0), (t, 0.0), (w + t, h), (w, h)]);
                }
                (p, w, h, *color)
            }
            Graphic::Image(bmp) => return vec![self.place_bitmap(origin, bmp.clone())],
            Graphic::Recall { name, mag_x, mag_y } => {
                let Some(bmp) = self.graphics.get(name).cloned() else {
                    self.warn(format!("graphic {name} not found"));
                    return Vec::new();
                };
                let (mx, my) = (
                    mag_x.round().clamp(1.0, 10.0) as u32,
                    mag_y.round().clamp(1.0, 10.0) as u32,
                );
                let magnified_bytes = (bmp.width as usize * mx as usize).div_ceil(8)
                    * bmp.height as usize
                    * my as usize;
                if magnified_bytes > self.opts.limits.max_graphic_bytes {
                    self.warn(format!(
                        "magnified graphic {name} exceeds the graphic size limit"
                    ));
                    return Vec::new();
                }
                let bmp = if mx == 1 && my == 1 {
                    bmp
                } else {
                    Arc::new(bmp.magnify(mx, my))
                };
                return vec![self.place_bitmap(origin, bmp)];
            }
        };
        let y = if origin.typeset {
            origin.y - h
        } else {
            origin.y
        };
        let transform = Transform::translate(origin.x, y);
        let paint = Self::paint(field, s, color);
        if let Some(state) = self.label.as_mut() {
            state.next_position = (origin.x + w, origin.y);
        }
        // Plain rectangles stay rectangles (cheaper in both backends).
        if path.segments.len() == 5
            && matches!(g, Graphic::Box { rounding, .. } if *rounding == 0.0)
        {
            return vec![Item::Rect {
                x: origin.x,
                y,
                w,
                h,
                paint,
            }];
        }
        vec![Item::Path {
            path: path.transform(&transform),
            paint,
        }]
    }

    fn place_bitmap(&mut self, origin: Origin, bmp: Arc<Bitmap>) -> Item {
        let y = if origin.typeset {
            origin.y - bmp.height as f64
        } else {
            origin.y
        };
        // Zebra (Labelary) does not apply ^FR / ^LR to graphic fields.
        let paint = Paint::Black;
        if let Some(state) = self.label.as_mut() {
            state.next_position = (origin.x + bmp.width as f64, origin.y);
        }
        Item::Bitmap {
            x: origin.x.round(),
            y: y.round(),
            bitmap: bmp,
            paint,
        }
    }

    // ------------------------------------------------------------ bar codes

    fn default_magnification(&self) -> u32 {
        match self.dpmm {
            0..=6 => 1,
            7..=8 => 2,
            9..=12 => 3,
            _ => 6,
        }
    }

    fn render_barcode(
        &mut self,
        field: &Field,
        s: &Settings,
        b: &BarcodeCommand,
        data: &[u8],
    ) -> Vec<Item> {
        let orientation = b.orientation.unwrap_or(s.orientation);
        let p = &b.params;
        let module = s.module_width;
        let wide = (module * s.ratio).floor().max(module);
        let height = p.height.unwrap_or(s.bar_height).max(1.0);
        let show_text = p.interpretation.unwrap_or(true);
        let above = p.interpretation_above.unwrap_or(false);
        let origin = Self::origin(field);
        let paint = Self::paint(field, s, LineColor::Black);

        let mut local: Vec<LocalItem> = Vec::new();
        let (box_w, box_h, ft_anchor): (f64, f64, (f64, f64));

        match b.kind {
            BarcodeKind::Code128
            | BarcodeKind::Code39
            | BarcodeKind::Logmars
            | BarcodeKind::Interleaved2of5
            | BarcodeKind::Code93
            | BarcodeKind::Codabar
            | BarcodeKind::Code11
            | BarcodeKind::Industrial2of5
            | BarcodeKind::Standard2of5 => {
                let mut gs1 = false;
                let enc = match b.kind {
                    BarcodeKind::Code128 => {
                        let mode = linear::Code128Mode::from_byte(p.mode);
                        gs1 = mode == linear::Code128Mode::Ean;
                        linear::code128(data, mode, module)
                    }
                    BarcodeKind::Code39 => {
                        linear::code39(data, p.check_digit.unwrap_or(false), module, wide)
                    }
                    BarcodeKind::Logmars => linear::code39(data, true, module, wide),
                    BarcodeKind::Interleaved2of5 => {
                        linear::interleaved_2of5(data, p.check_digit.unwrap_or(false), module, wide)
                    }
                    BarcodeKind::Code93 => linear::code93(data, module),
                    BarcodeKind::Codabar => linear::codabar(
                        data,
                        p.start_char.unwrap_or(b'A'),
                        p.stop_char.unwrap_or(b'A'),
                        module,
                        wide,
                    ),
                    BarcodeKind::Code11 => {
                        linear::code11(data, !p.check_digit.unwrap_or(false), module, wide)
                    }
                    _ => linear::standard_2of5(data, module, wide),
                };
                let width = enc.width();
                for (x, w) in enc.bars() {
                    local.push(LocalItem::Rect(x, 0.0, w, height));
                }
                let show = show_text
                    && !(b.kind == BarcodeKind::Logmars && p.interpretation_above != Some(true));
                if show && !enc.text.is_empty() {
                    let font = if gs1 {
                        // Font 0 at a height proportional to the module width,
                        // stretched to span the bars (Zebra "auto sizes" it).
                        let size = (GS1_TEXT_SIZE_PER_MODULE * module).round();
                        let unit: f64 = enc
                            .text
                            .chars()
                            .map(|c| font::font0_advance(c).unwrap_or(0.5))
                            .sum();
                        let target = width - 5.0 * module - 6.0;
                        let w = if unit > 0.0 && target > 0.0 {
                            target / unit
                        } else {
                            size
                        };
                        font::resolve(b'0', Some(size), Some(w), &s.default_font, self.dpmm)
                    } else {
                        font::resolve(
                            b'A',
                            Some(9.0 * module),
                            Some(5.0 * module),
                            &s.default_font,
                            self.dpmm,
                        )
                    };
                    let tw = font.measure(&enc.text, 0.0);
                    let top = if above {
                        -INTERPRETATION_GAP_ABOVE - font.cell_height
                    } else {
                        height + INTERPRETATION_GAP_BELOW
                    };
                    local.push(LocalItem::Text(
                        font,
                        enc.text.clone(),
                        (width - tw) / 2.0,
                        top + font.baseline,
                    ));
                }
                box_w = width;
                box_h = height;
                ft_anchor = (0.0, height);
            }
            BarcodeKind::Ean13 | BarcodeKind::Ean8 | BarcodeKind::UpcA | BarcodeKind::UpcE => {
                let show_check = p.check_digit.unwrap_or(true);
                let enc = match b.kind {
                    BarcodeKind::Ean13 => linear::ean13(data),
                    BarcodeKind::Ean8 => linear::ean8(data),
                    BarcodeKind::UpcA => linear::upca(data, show_check),
                    _ => linear::upce(data, show_check),
                };
                // Guard bars extend 13 dots; digits sit on a baseline 9
                // modules below the bars.
                let ext = if show_text { EAN_GUARD_EXTENSION } else { 0.0 };
                let elements = linear::runs(&enc.modules, 1.0);
                let mut x = 0usize;
                for (i, w) in elements.iter().enumerate() {
                    let w = *w as usize;
                    if i % 2 == 0 {
                        // A bar may span normal and long modules only if it is
                        // entirely inside a long range.
                        let long = enc.long.iter().any(|&(a, e)| x >= a && x + w <= e);
                        let h = if long { height + ext } else { height };
                        local.push(LocalItem::Rect(
                            x as f64 * module,
                            0.0,
                            w as f64 * module,
                            h,
                        ));
                    }
                    x += w;
                }
                let width = enc.modules.len() as f64 * module;
                if show_text {
                    let font = font::resolve(
                        b'A',
                        Some(9.0 * module),
                        Some(5.0 * module),
                        &s.default_font,
                        self.dpmm,
                    );
                    let adv = font.advance('0');
                    let baseline = height + 9.0 * module;
                    for (ch, slot) in &enc.inner_digits {
                        let cx = *slot as f64 * module + (7.0 * module - adv) / 2.0;
                        local.push(LocalItem::Text(font, ch.to_string(), cx, baseline));
                    }
                    if let Some(c) = enc.left_digit {
                        local.push(LocalItem::Text(
                            font,
                            c.to_string(),
                            -5.0 * module - adv,
                            baseline,
                        ));
                    }
                    if let Some(c) = enc.right_digit {
                        local.push(LocalItem::Text(
                            font,
                            c.to_string(),
                            width + 2.0 * module,
                            baseline,
                        ));
                    }
                }
                box_w = width;
                box_h = height;
                ft_anchor = (0.0, height);
            }
            BarcodeKind::QrCode => {
                if p.model == Some(1) {
                    self.warn("QR model 1 is rendered as model 2");
                }
                let mag = p
                    .magnification
                    .filter(|m| (1..=100).contains(m))
                    .unwrap_or(self.default_magnification()) as f64;
                let default_ecc = p
                    .error_correction
                    .as_deref()
                    .and_then(|e| e.bytes().next())
                    .unwrap_or(b'Q');
                let (ecc, payload) = matrix::parse_qr_field(data, default_ecc.to_ascii_uppercase());
                let mask = p.mask.filter(|m| *m <= 7).or(Some(7));
                let grid = match matrix::qr(&payload, ecc, mask) {
                    Ok(g) => g,
                    Err(e) => {
                        self.warn(e);
                        return Vec::new();
                    }
                };
                // Zebra quirk: with ^FO the symbol starts one ^BY bar height
                // below the origin; ^FT ignores that margin.
                let margin = s.bar_height;
                push_grid(&mut local, &grid, mag, mag, 0.0, margin);
                let size = grid.width as f64 * mag;
                box_w = size;
                box_h = size + margin;
                ft_anchor = (0.0, size + margin + 3.0 * mag);
            }
            BarcodeKind::DataMatrix => {
                if p.quality != Some(200) {
                    self.warn("Data Matrix is always rendered as ECC 200");
                }
                let m = p.height.unwrap_or(1.0).max(1.0).floor();
                let escape = p.escape.or(Some(b'~'));
                let grid = match matrix::datamatrix(
                    data,
                    p.columns,
                    p.rows,
                    p.aspect == Some(2),
                    escape,
                ) {
                    Ok(g) => g,
                    Err(e) => {
                        self.warn(e);
                        return Vec::new();
                    }
                };
                push_grid(&mut local, &grid, m, m, 0.0, 0.0);
                box_w = grid.width as f64 * m;
                box_h = grid.height as f64 * m;
                ft_anchor = (0.0, box_h);
            }
            BarcodeKind::Pdf417 => {
                let security = p
                    .error_correction
                    .as_deref()
                    .and_then(|e| e.trim().parse::<u32>().ok());
                let grid = match matrix::pdf417(
                    data,
                    security,
                    p.columns,
                    p.rows,
                    p.truncate.unwrap_or(false),
                ) {
                    Ok(g) => g,
                    Err(e) => {
                        self.warn(e);
                        return Vec::new();
                    }
                };
                let row_h = match p.height {
                    Some(h) => h.max(1.0),
                    None => (s.bar_height / grid.height as f64).floor().max(1.0),
                };
                push_grid(&mut local, &grid, module, row_h, 0.0, 0.0);
                box_w = grid.width as f64 * module;
                box_h = grid.height as f64 * row_h;
                ft_anchor = (0.0, box_h);
            }
            BarcodeKind::Aztec => {
                let mag = p
                    .magnification
                    .filter(|m| (1..=10).contains(m))
                    .unwrap_or(self.default_magnification()) as f64;
                let size = p
                    .error_correction
                    .as_deref()
                    .and_then(|e| e.trim().parse::<u32>().ok());
                let grid = match matrix::aztec(data, size) {
                    Ok(g) => g,
                    Err(e) => {
                        self.warn(e);
                        return Vec::new();
                    }
                };
                push_grid(&mut local, &grid, mag, mag, 0.0, 0.0);
                box_w = grid.width as f64 * mag;
                box_h = grid.height as f64 * mag;
                ft_anchor = (0.0, box_h);
            }
            BarcodeKind::MaxiCode => {
                let mode = p.mode.unwrap_or(2);
                if p.symbol_count.unwrap_or(1) > 1 {
                    self.warn("MaxiCode structured append is not supported");
                }
                let code = maxicode::encode(data, mode);
                if code.truncated {
                    self.warn("MaxiCode data truncated to the symbol capacity");
                }
                let geo = maxicode::Geometry::for_dpi(self.dpi);
                let mut path = Path::new();
                for (r, row) in code.modules.iter().enumerate() {
                    for (c, &dark) in row.iter().enumerate() {
                        if dark {
                            let (cx, cy) = geo.center(r, c);
                            path.polygon(&geo.hexagon(cx, cy));
                        }
                    }
                }
                let (fx, fy) = geo.finder_center();
                for (inner, outer) in geo.rings {
                    path.ellipse(fx, fy, outer, outer);
                    path.ellipse(fx, fy, inner, inner);
                }
                local.push(LocalItem::Path(path));
                let (w, h) = geo.size();
                box_w = w;
                box_h = h;
                ft_anchor = (0.0, h);
                // MaxiCode ignores orientation.
                return self.place_local(
                    local,
                    Orientation::Normal,
                    box_w,
                    box_h,
                    origin,
                    ft_anchor,
                    paint,
                );
            }
            other => {
                self.warn(format!("bar code type {other:?} is not supported"));
                return Vec::new();
            }
        }
        self.place_local(local, orientation, box_w, box_h, origin, ft_anchor, paint)
    }

    #[allow(clippy::too_many_arguments)]
    fn place_local(
        &mut self,
        local: Vec<LocalItem>,
        orientation: Orientation,
        box_w: f64,
        box_h: f64,
        origin: Origin,
        ft_anchor: (f64, f64),
        paint: Paint,
    ) -> Vec<Item> {
        let anchor = origin.typeset.then_some(ft_anchor);
        let mut x = origin.x;
        if origin.right_justify && orientation == Orientation::Normal {
            x -= box_w;
        }
        let place = place_box(orientation, box_w, box_h, x, origin.y, anchor);
        if let Some(state) = self.label.as_mut() {
            state.next_position = place.apply(box_w, ft_anchor.1);
        }
        local
            .into_iter()
            .filter_map(|item| match item {
                LocalItem::Rect(x, y, w, h) => {
                    let (x0, y0) = place.apply(x, y);
                    let (x1, y1) = place.apply(x + w, y + h);
                    Some(Item::Rect {
                        x: x0.min(x1),
                        y: y0.min(y1),
                        w: (x1 - x0).abs(),
                        h: (y1 - y0).abs(),
                        paint,
                    })
                }
                LocalItem::Path(p) => Some(Item::Path {
                    path: p.transform(&place),
                    paint,
                }),
                LocalItem::Text(font, text, x, baseline) => {
                    let (glyphs, _) = font.layout(&text, 0.0);
                    (!glyphs.is_empty()).then(|| {
                        Item::Text(TextRun {
                            font,
                            glyphs,
                            transform: Transform::translate(x, baseline).then(&place),
                            paint,
                        })
                    })
                }
            })
            .collect()
    }
}

/// Items in bar code local coordinates (before rotation and placement).
enum LocalItem {
    Rect(f64, f64, f64, f64),
    Path(Path),
    Text(ResolvedFont, String, f64, f64),
}

/// Adds the dark cells of a grid as rectangles, merging horizontal runs.
fn push_grid(out: &mut Vec<LocalItem>, grid: &matrix::Grid, mw: f64, mh: f64, ox: f64, oy: f64) {
    for y in 0..grid.height {
        let mut x = 0;
        while x < grid.width {
            if grid.get(x, y) {
                let start = x;
                while x < grid.width && grid.get(x, y) {
                    x += 1;
                }
                out.push(LocalItem::Rect(
                    ox + start as f64 * mw,
                    oy + y as f64 * mh,
                    (x - start) as f64 * mw,
                    mh,
                ));
            } else {
                x += 1;
            }
        }
    }
}

/// Handles `^FB` escapes: `\&` is a line break, `\\` a backslash.
fn unescape_block(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('&') => {
                    chars.next();
                    out.push('\n');
                    continue;
                }
                Some('\\') => {
                    chars.next();
                    out.push('\\');
                    continue;
                }
                _ => {}
            }
        }
        out.push(c);
    }
    out
}

/// Greedy word wrap. Returns lines with a flag telling whether the line ends
/// a paragraph (used for justified text).
fn wrap(
    text: &str,
    width: f64,
    font: &ResolvedFont,
    gap: f64,
    hanging: f64,
) -> Vec<(String, bool)> {
    let mut lines: Vec<(String, bool)> = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        for word in paragraph.split(' ') {
            let candidate = if current.is_empty() {
                word.to_string()
            } else {
                format!("{current} {word}")
            };
            let indent = if lines.is_empty() { 0.0 } else { hanging };
            if current.is_empty() || font.measure(&candidate, gap) <= width - indent + 0.01 {
                current = candidate;
            } else {
                lines.push((std::mem::take(&mut current), false));
                current = word.to_string();
            }
        }
        lines.push((current, true));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::DefaultFont;

    #[test]
    fn wrap_greedy() {
        let f = font::resolve(b'0', Some(30.0), Some(30.0), &DefaultFont::default(), 8);
        let lines = wrap("HHH HHH HHH HHH HHH", 120.0, &f, 0.0, 0.0);
        let texts: Vec<_> = lines.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(texts, ["HHH HHH", "HHH HHH", "HHH"]);
    }

    #[test]
    fn block_escapes() {
        assert_eq!(unescape_block("a\\&b\\\\c"), "a\nb\\c");
    }
}
