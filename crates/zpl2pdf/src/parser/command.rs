//! Typed ZPL commands.
//!
//! [`parse`] turns a [`RawCommand`] into a [`Command`]. Parameters that the
//! label omitted stay `None`, because their defaults often depend on the
//! interpreter state (`^CF`, `^BY`, `^FW`, ...).

use super::lexer::{Prefix, RawCommand};
use crate::types::{Justify, LineColor, Orientation};

/// A parsed ZPL command.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// `^XA` start of a label format.
    StartFormat,
    /// `^XZ` end of a label format.
    EndFormat,
    /// `^FO` field origin (top left corner).
    FieldOrigin {
        x: Option<f64>,
        y: Option<f64>,
        justify: Option<u8>,
    },
    /// `^FT` field typeset (baseline / bottom left corner).
    FieldTypeset {
        x: Option<f64>,
        y: Option<f64>,
        justify: Option<u8>,
    },
    /// `^FS` field separator.
    FieldSeparator,
    /// `^FD` field data.
    FieldData(Vec<u8>),
    /// `^FV` field variable.
    FieldVariable(Vec<u8>),
    /// `^FX` comment.
    Comment,
    /// `^FH` hexadecimal indicator for the current field.
    FieldHex(u8),
    /// `^FR` field reverse print.
    FieldReverse,
    /// `^FN` field number.
    FieldNumber(u32),
    /// `^FB` field block.
    FieldBlock {
        width: f64,
        max_lines: u32,
        line_spacing: f64,
        justify: Justify,
        hanging_indent: f64,
    },
    /// `^TB` text block.
    TextBlock {
        orientation: Option<Orientation>,
        width: Option<f64>,
        height: Option<f64>,
    },
    /// `^FW` default field orientation.
    FieldOrientation {
        orientation: Option<Orientation>,
        justify: Option<u8>,
    },
    /// `^FP` field parameter (direction and character gap).
    FieldParameter { direction: u8, gap: f64 },
    /// `^A` font selection.
    Font {
        name: u8,
        orientation: Option<Orientation>,
        height: Option<f64>,
        width: Option<f64>,
    },
    /// `^A@` font selection by file name.
    FontByName {
        orientation: Option<Orientation>,
        height: Option<f64>,
        width: Option<f64>,
        path: String,
    },
    /// `^CF` change default font.
    ChangeFont {
        name: Option<u8>,
        height: Option<f64>,
        width: Option<f64>,
    },
    /// `^CI` change international encoding.
    ChangeEncoding(u32),
    /// `^CW` font identifier (alias a letter to a font file).
    FontIdentifier { letter: u8, path: String },
    /// `^BY` bar code field defaults.
    BarcodeDefaults {
        module_width: Option<f64>,
        ratio: Option<f64>,
        height: Option<f64>,
    },
    /// Any `^B` bar code command.
    Barcode(BarcodeCommand),
    /// `^GB` graphic box.
    GraphicBox {
        width: Option<f64>,
        height: Option<f64>,
        thickness: Option<f64>,
        color: LineColor,
        rounding: f64,
    },
    /// `^GC` graphic circle.
    GraphicCircle {
        diameter: f64,
        thickness: f64,
        color: LineColor,
    },
    /// `^GE` graphic ellipse.
    GraphicEllipse {
        width: Option<f64>,
        height: Option<f64>,
        thickness: Option<f64>,
        color: LineColor,
    },
    /// `^GD` graphic diagonal line.
    GraphicDiagonal {
        width: Option<f64>,
        height: Option<f64>,
        thickness: Option<f64>,
        color: LineColor,
        right_leaning: bool,
    },
    /// `^GF` graphic field.
    GraphicField {
        format: u8,
        binary_bytes: usize,
        total_bytes: usize,
        row_bytes: usize,
        data: Vec<u8>,
    },
    /// `^GS` graphic symbol.
    GraphicSymbol {
        orientation: Option<Orientation>,
        height: Option<f64>,
        width: Option<f64>,
    },
    /// `^LH` label home.
    LabelHome { x: f64, y: f64 },
    /// `^LS` label shift.
    LabelShift(f64),
    /// `^LT` label top.
    LabelTop(f64),
    /// `^LR` label reverse print.
    LabelReverse(bool),
    /// `^PW` print width.
    PrintWidth(f64),
    /// `^LL` label length.
    LabelLength(f64),
    /// `^PO` print orientation.
    PrintOrientation { inverted: bool },
    /// `^PM` print mirror image.
    PrintMirror(bool),
    /// `^PQ` print quantity.
    PrintQuantity(u32),
    /// `^MU` set units of measurement.
    Units(u8),
    /// `~DG` download graphic.
    DownloadGraphic {
        name: String,
        total_bytes: usize,
        row_bytes: usize,
        data: Vec<u8>,
    },
    /// `~DY` download objects.
    DownloadObject {
        name: String,
        format: u8,
        extension: u8,
        total_bytes: usize,
        row_bytes: usize,
        data: Vec<u8>,
    },
    /// `^XG` recall graphic.
    RecallGraphic {
        name: String,
        mag_x: f64,
        mag_y: f64,
    },
    /// `^IM` image move.
    ImageMove { name: String },
    /// `^ID` object delete.
    DeleteObject { name: String },
    /// `~EG` erase all downloaded graphics.
    EraseGraphics,
    /// `^DF` download format.
    DownloadFormat { name: String },
    /// `^XF` recall format.
    RecallFormat { name: String },
    /// `^SN` serialization data.
    SerializationData {
        start: Vec<u8>,
        increment: i64,
        leading_zeros: bool,
    },
    /// `^SF` serialization field.
    SerializationField { mask: String, increment: String },
    /// A known command that has no visual effect.
    Ignored,
    /// A command this crate does not understand.
    Unknown,
}

/// Bar code symbologies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BarcodeKind {
    Code11,
    Interleaved2of5,
    Code39,
    Code49,
    Planet,
    Pdf417,
    Ean8,
    UpcE,
    Code93,
    Codablock,
    Code128,
    MaxiCode,
    Ean13,
    MicroPdf417,
    Industrial2of5,
    Standard2of5,
    Codabar,
    Logmars,
    Msi,
    Aztec,
    Plessey,
    QrCode,
    Databar,
    UpcEanExtension,
    Tlc39,
    UpcA,
    DataMatrix,
    Postnet,
}

/// Parameters of a bar code command. Which fields are set depends on the
/// symbology; the meaning of each field follows the ZPL manual.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BarcodeParams {
    /// Bar code height (or row height for stacked codes).
    pub height: Option<f64>,
    /// Print the interpretation line.
    pub interpretation: Option<bool>,
    /// Print the interpretation line above the code.
    pub interpretation_above: Option<bool>,
    /// Check digit flag (its exact meaning depends on the symbology).
    pub check_digit: Option<bool>,
    /// Symbology mode (`^BC` mode letter, `^BD` mode digit, ...).
    pub mode: Option<u8>,
    /// Module magnification (`^BQ`, `^B0`).
    pub magnification: Option<u32>,
    /// QR code model.
    pub model: Option<u32>,
    /// Error correction / security level as written.
    pub error_correction: Option<String>,
    /// QR mask.
    pub mask: Option<u32>,
    /// Columns (PDF417, Data Matrix).
    pub columns: Option<u32>,
    /// Rows (PDF417, Data Matrix).
    pub rows: Option<u32>,
    /// Truncate flag (PDF417).
    pub truncate: Option<bool>,
    /// Data Matrix quality level.
    pub quality: Option<u32>,
    /// Data Matrix escape character.
    pub escape: Option<u8>,
    /// Data Matrix aspect ratio (1 square, 2 rectangular).
    pub aspect: Option<u32>,
    /// MaxiCode / Aztec structured append position.
    pub symbol_number: Option<u32>,
    /// MaxiCode / Aztec structured append count.
    pub symbol_count: Option<u32>,
    /// Codabar start character.
    pub start_char: Option<u8>,
    /// Codabar stop character.
    pub stop_char: Option<u8>,
    /// Aztec ECI flag.
    pub eci: Option<bool>,
    /// Aztec reader initialization flag.
    pub reader_init: Option<bool>,
}

/// A parsed bar code command.
#[derive(Debug, Clone, PartialEq)]
pub struct BarcodeCommand {
    /// Symbology.
    pub kind: BarcodeKind,
    /// Field orientation, `None` means the `^FW` default.
    pub orientation: Option<Orientation>,
    /// Symbology parameters.
    pub params: BarcodeParams,
}

/// Comma separated parameter access.
struct Params<'a> {
    parts: Vec<&'a [u8]>,
}

impl<'a> Params<'a> {
    fn new(raw: &'a RawCommand) -> Self {
        let parts = if raw.args.is_empty() {
            Vec::new()
        } else {
            raw.args.split(|&b| b == raw.delimiter).collect()
        };
        Self { parts }
    }

    fn raw(&self, i: usize) -> Option<&'a [u8]> {
        self.parts.get(i).copied()
    }

    fn str(&self, i: usize) -> Option<String> {
        let s = String::from_utf8_lossy(self.raw(i)?).trim().to_string();
        (!s.is_empty()).then_some(s)
    }

    fn num(&self, i: usize) -> Option<f64> {
        let s = self.str(i)?;
        // Accept leading numeric part, ZPL printers ignore trailing junk.
        let end = s
            .char_indices()
            .find(|&(idx, c)| {
                !(c.is_ascii_digit() || c == '.' || ((c == '-' || c == '+') && idx == 0))
            })
            .map(|(idx, _)| idx)
            .unwrap_or(s.len());
        s[..end].parse::<f64>().ok().filter(|v| v.is_finite())
    }

    fn uint(&self, i: usize) -> Option<u32> {
        self.num(i)
            .filter(|v| *v >= 0.0)
            .map(|v| v.min(u32::MAX as f64) as u32)
    }

    fn byte(&self, i: usize) -> Option<u8> {
        self.raw(i)?
            .iter()
            .copied()
            .find(|b| !b.is_ascii_whitespace())
            .map(|b| b.to_ascii_uppercase())
    }

    fn yes_no(&self, i: usize) -> Option<bool> {
        match self.byte(i)? {
            b'Y' => Some(true),
            b'N' => Some(false),
            _ => None,
        }
    }

    fn orientation(&self, i: usize) -> Option<Orientation> {
        Orientation::from_byte(self.byte(i)?)
    }
}

/// Normalizes an object name such as `R:LOGO.GRF`: the device letter is
/// dropped, the name is upper cased and `default_ext` is appended when the
/// name has no extension.
pub fn object_name(s: &str, default_ext: &str) -> String {
    let s = s.trim();
    let s = match s.find(':') {
        Some(i) if i <= 1 => &s[i + 1..],
        _ => s,
    };
    let mut name = s.trim().to_ascii_uppercase();
    if name.is_empty() {
        name = "UNKNOWN".into();
    }
    if !name.contains('.') {
        name.push_str(default_ext);
    }
    name
}

/// Parses one raw command.
pub fn parse(raw: &RawCommand) -> Command {
    let p = Params::new(raw);
    match raw.prefix {
        Prefix::Control => parse_control(raw, &p),
        Prefix::Format => parse_format(raw, &p),
    }
}

fn parse_control(raw: &RawCommand, p: &Params) -> Command {
    match raw.code.as_str() {
        "DG" => Command::DownloadGraphic {
            name: object_name(&p.str(0).unwrap_or_default(), ".GRF"),
            total_bytes: p.uint(1).unwrap_or(0) as usize,
            row_bytes: p.uint(2).unwrap_or(0) as usize,
            data: raw.data.clone().unwrap_or_default(),
        },
        "DY" => {
            let ext = p.byte(2).unwrap_or(b'G');
            let ext_str = match ext {
                b'P' => ".PNG",
                b'B' => ".BMP",
                b'T' => ".TTF",
                b'E' => ".TTE",
                b'O' => ".OTF",
                b'X' => ".DAT",
                b'Z' => ".ZPL",
                _ => ".GRF",
            };
            Command::DownloadObject {
                name: object_name(&p.str(0).unwrap_or_default(), ext_str),
                format: p.byte(1).unwrap_or(b'A'),
                extension: ext,
                total_bytes: p.uint(3).unwrap_or(0) as usize,
                row_bytes: p.uint(4).unwrap_or(0) as usize,
                data: raw.data.clone().unwrap_or_default(),
            }
        }
        "EG" => Command::EraseGraphics,
        "CC" | "CD" | "CT" => Command::Ignored,
        "DB" | "DE" | "DN" | "DS" | "DT" | "DU" | "HB" | "HD" | "HI" | "HM" | "HQ" | "HS"
        | "HU" | "JA" | "JB" | "JC" | "JD" | "JE" | "JF" | "JG" | "JI" | "JL" | "JN" | "JO"
        | "JP" | "JQ" | "JR" | "JS" | "JX" | "NC" | "NR" | "NT" | "PL" | "PP" | "PR" | "PS"
        | "RO" | "SD" | "TA" | "WC" | "WQ" | "WR" | "KB" => Command::Ignored,
        _ => Command::Unknown,
    }
}

fn parse_format(raw: &RawCommand, p: &Params) -> Command {
    let code = raw.code.as_str();
    match code {
        "XA" => Command::StartFormat,
        "XZ" => Command::EndFormat,
        "FO" => Command::FieldOrigin {
            x: p.num(0),
            y: p.num(1),
            justify: p.byte(2),
        },
        "FT" => Command::FieldTypeset {
            x: p.num(0),
            y: p.num(1),
            justify: p.byte(2),
        },
        "FS" => Command::FieldSeparator,
        "FD" => Command::FieldData(raw.args.clone()),
        "FV" => Command::FieldVariable(raw.args.clone()),
        "FX" => Command::Comment,
        "FH" => Command::FieldHex(
            raw.args
                .iter()
                .copied()
                .find(|b| !b.is_ascii_whitespace())
                .unwrap_or(b'_'),
        ),
        "FR" => Command::FieldReverse,
        "FN" => Command::FieldNumber(p.uint(0).unwrap_or(0)),
        "FB" => Command::FieldBlock {
            width: p.num(0).unwrap_or(0.0),
            max_lines: p.uint(1).unwrap_or(1).max(1),
            line_spacing: p.num(2).unwrap_or(0.0),
            justify: p.byte(3).and_then(Justify::from_byte).unwrap_or_default(),
            hanging_indent: p.num(4).unwrap_or(0.0),
        },
        "TB" => Command::TextBlock {
            orientation: p.orientation(0),
            width: p.num(1),
            height: p.num(2),
        },
        "FW" => Command::FieldOrientation {
            orientation: p.orientation(0),
            justify: p.byte(1),
        },
        "FP" => Command::FieldParameter {
            direction: p.byte(0).unwrap_or(b'H'),
            gap: p.num(1).unwrap_or(0.0),
        },
        "A" => match raw.font {
            Some(b'@') => Command::FontByName {
                orientation: p.orientation(0),
                height: p.num(1),
                width: p.num(2),
                path: p.str(3).unwrap_or_default(),
            },
            Some(f) => Command::Font {
                name: f,
                orientation: p.orientation(0),
                height: p.num(1),
                width: p.num(2),
            },
            None => Command::Unknown,
        },
        "CF" => Command::ChangeFont {
            name: p.byte(0),
            height: p.num(1),
            width: p.num(2),
        },
        "CI" => Command::ChangeEncoding(p.uint(0).unwrap_or(0)),
        "CW" => match p.byte(0) {
            Some(letter) => Command::FontIdentifier {
                letter,
                path: p.str(1).unwrap_or_default(),
            },
            None => Command::Unknown,
        },
        "BY" => Command::BarcodeDefaults {
            module_width: p.num(0),
            ratio: p.num(1),
            height: p.num(2),
        },
        "GB" => Command::GraphicBox {
            width: p.num(0),
            height: p.num(1),
            thickness: p.num(2),
            color: p.byte(3).and_then(LineColor::from_byte).unwrap_or_default(),
            rounding: p.num(4).unwrap_or(0.0).clamp(0.0, 8.0),
        },
        "GC" => Command::GraphicCircle {
            diameter: p.num(0).unwrap_or(3.0),
            thickness: p.num(1).unwrap_or(1.0),
            color: p.byte(2).and_then(LineColor::from_byte).unwrap_or_default(),
        },
        "GE" => Command::GraphicEllipse {
            width: p.num(0),
            height: p.num(1),
            thickness: p.num(2),
            color: p.byte(3).and_then(LineColor::from_byte).unwrap_or_default(),
        },
        "GD" => Command::GraphicDiagonal {
            width: p.num(0),
            height: p.num(1),
            thickness: p.num(2),
            color: p.byte(3).and_then(LineColor::from_byte).unwrap_or_default(),
            right_leaning: p.byte(4) != Some(b'L'),
        },
        "GF" => Command::GraphicField {
            format: p.byte(0).unwrap_or(b'A'),
            binary_bytes: p.uint(1).unwrap_or(0) as usize,
            total_bytes: p.uint(2).unwrap_or(0) as usize,
            row_bytes: p.uint(3).unwrap_or(0) as usize,
            data: raw.data.clone().unwrap_or_default(),
        },
        "GS" => Command::GraphicSymbol {
            orientation: p.orientation(0),
            height: p.num(1),
            width: p.num(2),
        },
        "LH" => Command::LabelHome {
            x: p.num(0).unwrap_or(0.0),
            y: p.num(1).unwrap_or(0.0),
        },
        "LS" => Command::LabelShift(p.num(0).unwrap_or(0.0)),
        "LT" => Command::LabelTop(p.num(0).unwrap_or(0.0)),
        "LR" => Command::LabelReverse(p.yes_no(0).unwrap_or(false)),
        "PW" => match p.num(0) {
            Some(w) => Command::PrintWidth(w),
            None => Command::Ignored,
        },
        "LL" => match p.num(0) {
            Some(l) => Command::LabelLength(l),
            None => Command::Ignored,
        },
        "PO" => Command::PrintOrientation {
            inverted: p.byte(0) == Some(b'I'),
        },
        "PM" => Command::PrintMirror(p.yes_no(0).unwrap_or(false)),
        "PQ" => Command::PrintQuantity(p.uint(0).unwrap_or(1).max(1)),
        "MU" => Command::Units(p.byte(0).unwrap_or(b'D')),
        "XG" => Command::RecallGraphic {
            name: object_name(&p.str(0).unwrap_or_default(), ".GRF"),
            mag_x: p.num(1).unwrap_or(1.0).max(1.0),
            mag_y: p.num(2).unwrap_or(1.0).max(1.0),
        },
        "IM" => Command::ImageMove {
            name: object_name(&p.str(0).unwrap_or_default(), ".GRF"),
        },
        "ID" => Command::DeleteObject {
            name: object_name(&p.str(0).unwrap_or_default(), ".GRF"),
        },
        "DF" => Command::DownloadFormat {
            name: object_name(&p.str(0).unwrap_or_default(), ".ZPL"),
        },
        "XF" => Command::RecallFormat {
            name: object_name(&p.str(0).unwrap_or_default(), ".ZPL"),
        },
        "SN" => Command::SerializationData {
            start: p.raw(0).map(|s| s.to_vec()).unwrap_or_default(),
            increment: p.num(1).map(|v| v as i64).unwrap_or(1),
            leading_zeros: p.yes_no(2).unwrap_or(false),
        },
        "SF" => Command::SerializationField {
            mask: p.str(0).unwrap_or_default(),
            increment: p.str(1).unwrap_or_else(|| "1".into()),
        },
        _ if code.len() == 2 && code.starts_with('B') => parse_barcode(code.as_bytes()[1], p),
        "CC" | "CD" | "CT" | "CM" | "CO" | "CV" | "FC" | "FL" | "FM" | "HF" | "HG" | "HH"
        | "HT" | "HV" | "HW" | "HY" | "HZ" | "IL" | "IS" | "JB" | "JJ" | "JM" | "JS" | "JT"
        | "JU" | "JW" | "JZ" | "KD" | "KL" | "KN" | "KP" | "KV" | "LF" | "MA" | "MC" | "MD"
        | "MF" | "MI" | "ML" | "MM" | "MN" | "MP" | "MT" | "MW" | "NB" | "NC" | "ND" | "NI"
        | "NN" | "NP" | "NS" | "NT" | "NW" | "PA" | "PF" | "PH" | "PN" | "PP" | "PR" | "PS"
        | "SC" | "SE" | "SI" | "SL" | "SO" | "SP" | "SQ" | "SR" | "SS" | "ST" | "SX" | "SZ"
        | "TO" | "WA" | "WD" | "WE" | "WF" | "WI" | "WL" | "WP" | "WR" | "WS" | "WT" | "WX"
        | "XB" | "XS" | "ZZ" | "FE" | "DE" | "RF" | "RS" | "RB" | "RI" | "RM" | "RN" | "RQ"
        | "RR" | "RT" | "RU" | "RW" | "WV" | "HL" | "HR" => Command::Ignored,
        _ => Command::Unknown,
    }
}

fn parse_barcode(letter: u8, p: &Params) -> Command {
    use BarcodeKind as K;
    let kind = match letter {
        b'1' => K::Code11,
        b'2' => K::Interleaved2of5,
        b'3' => K::Code39,
        b'4' => K::Code49,
        b'5' => K::Planet,
        b'7' => K::Pdf417,
        b'8' => K::Ean8,
        b'9' => K::UpcE,
        b'A' => K::Code93,
        b'B' => K::Codablock,
        b'C' => K::Code128,
        b'D' => K::MaxiCode,
        b'E' => K::Ean13,
        b'F' => K::MicroPdf417,
        b'I' => K::Industrial2of5,
        b'J' => K::Standard2of5,
        b'K' => K::Codabar,
        b'L' => K::Logmars,
        b'M' => K::Msi,
        b'0' | b'O' => K::Aztec,
        b'P' => K::Plessey,
        b'Q' => K::QrCode,
        b'R' => K::Databar,
        b'S' => K::UpcEanExtension,
        b'T' => K::Tlc39,
        b'U' => K::UpcA,
        b'X' => K::DataMatrix,
        b'Z' => K::Postnet,
        b'Y' => {
            return Command::BarcodeDefaults {
                module_width: p.num(0),
                ratio: p.num(1),
                height: p.num(2),
            };
        }
        _ => return Command::Unknown,
    };
    let mut bp = BarcodeParams::default();
    let mut orientation = p.orientation(0);
    match kind {
        // o,e,h,f,g
        K::Code11 | K::Code39 | K::Plessey => {
            bp.check_digit = p.yes_no(1);
            bp.height = p.num(2);
            bp.interpretation = p.yes_no(3);
            bp.interpretation_above = p.yes_no(4);
        }
        // o,h,f,g,e
        K::Interleaved2of5 | K::UpcE | K::Code93 | K::UpcA => {
            bp.height = p.num(1);
            bp.interpretation = p.yes_no(2);
            bp.interpretation_above = p.yes_no(3);
            bp.check_digit = p.yes_no(4);
        }
        // o,h,f,g
        K::Ean8
        | K::Ean13
        | K::Industrial2of5
        | K::Standard2of5
        | K::UpcEanExtension
        | K::Planet
        | K::Postnet => {
            bp.height = p.num(1);
            bp.interpretation = p.yes_no(2);
            bp.interpretation_above = p.yes_no(3);
        }
        K::Code128 => {
            bp.height = p.num(1);
            bp.interpretation = p.yes_no(2);
            bp.interpretation_above = p.yes_no(3);
            bp.check_digit = p.yes_no(4);
            bp.mode = p.byte(5);
        }
        K::Logmars => {
            bp.height = p.num(1);
            bp.interpretation_above = p.yes_no(2);
        }
        K::Codabar => {
            bp.check_digit = p.yes_no(1);
            bp.height = p.num(2);
            bp.interpretation = p.yes_no(3);
            bp.interpretation_above = p.yes_no(4);
            bp.start_char = p.byte(5);
            bp.stop_char = p.byte(6);
        }
        K::Msi => {
            bp.mode = p.byte(1);
            bp.height = p.num(2);
            bp.interpretation = p.yes_no(3);
            bp.interpretation_above = p.yes_no(4);
            bp.check_digit = p.yes_no(5);
        }
        K::Pdf417 | K::Codablock => {
            bp.height = p.num(1);
            bp.error_correction = p.str(2);
            bp.columns = p.uint(3);
            bp.rows = p.uint(4);
            bp.truncate = p.yes_no(5);
            if kind == K::Codablock {
                bp.mode = p.byte(5);
            }
        }
        K::MicroPdf417 => {
            bp.height = p.num(1);
            bp.mode = p.uint(2).map(|m| m.min(255) as u8);
        }
        K::Code49 => {
            bp.height = p.num(1);
            bp.interpretation = p.yes_no(2);
            bp.mode = p.byte(3);
        }
        K::MaxiCode => {
            orientation = None;
            bp.mode = p.uint(0).map(|m| m.min(255) as u8);
            bp.symbol_number = p.uint(1);
            bp.symbol_count = p.uint(2);
        }
        K::QrCode => {
            bp.model = p.uint(1);
            bp.magnification = p.uint(2);
            bp.error_correction = p.str(3);
            bp.mask = p.uint(4);
        }
        K::DataMatrix => {
            bp.height = p.num(1);
            bp.quality = p.uint(2);
            bp.columns = p.uint(3);
            bp.rows = p.uint(4);
            bp.escape = p.raw(6).and_then(|s| s.first().copied());
            bp.aspect = p.uint(7);
        }
        K::Aztec => {
            bp.magnification = p.uint(1);
            bp.eci = p.yes_no(2);
            bp.error_correction = p.str(3);
            bp.reader_init = p.yes_no(4);
            bp.symbol_count = p.uint(5);
        }
        K::Databar => {
            bp.mode = p.uint(1).map(|m| m.min(255) as u8);
            bp.magnification = p.uint(2);
            bp.height = p.num(3);
        }
        K::Tlc39 => {
            bp.height = p.num(3);
        }
    }
    Command::Barcode(BarcodeCommand {
        kind,
        orientation,
        params: bp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::lexer::Lexer;

    fn cmds(s: &str) -> Vec<Command> {
        Lexer::new(s.as_bytes()).map(|r| parse(&r)).collect()
    }

    #[test]
    fn field_origin_and_font() {
        let c = cmds("^FO10,20^A0R,30^FDx^FS");
        assert_eq!(
            c[0],
            Command::FieldOrigin {
                x: Some(10.0),
                y: Some(20.0),
                justify: None
            }
        );
        assert_eq!(
            c[1],
            Command::Font {
                name: b'0',
                orientation: Some(Orientation::Rotated),
                height: Some(30.0),
                width: None
            }
        );
        assert_eq!(c[2], Command::FieldData(b"x".to_vec()));
    }

    #[test]
    fn font_without_orientation() {
        let c = cmds("^ADN^A0,40,20");
        assert_eq!(
            c[0],
            Command::Font {
                name: b'D',
                orientation: Some(Orientation::Normal),
                height: None,
                width: None
            }
        );
        assert_eq!(
            c[1],
            Command::Font {
                name: b'0',
                orientation: None,
                height: Some(40.0),
                width: Some(20.0)
            }
        );
    }

    #[test]
    fn barcode_code128() {
        let c = cmds("^BCN,100,Y,N,N,A");
        let Command::Barcode(b) = &c[0] else { panic!() };
        assert_eq!(b.kind, BarcodeKind::Code128);
        assert_eq!(b.params.height, Some(100.0));
        assert_eq!(b.params.interpretation, Some(true));
        assert_eq!(b.params.mode, Some(b'A'));
    }

    #[test]
    fn barcode_defaults_and_box() {
        let c = cmds("^BY3,2.5,80^GB100,,5,W,2");
        assert_eq!(
            c[0],
            Command::BarcodeDefaults {
                module_width: Some(3.0),
                ratio: Some(2.5),
                height: Some(80.0)
            }
        );
        assert_eq!(
            c[1],
            Command::GraphicBox {
                width: Some(100.0),
                height: None,
                thickness: Some(5.0),
                color: LineColor::White,
                rounding: 2.0
            }
        );
    }

    #[test]
    fn object_names() {
        assert_eq!(object_name("R:logo.grf", ".GRF"), "LOGO.GRF");
        assert_eq!(object_name("LOGO", ".GRF"), "LOGO.GRF");
        assert_eq!(object_name("E:FORM", ".ZPL"), "FORM.ZPL");
    }

    #[test]
    fn numbers_with_junk() {
        let c = cmds("^FO 10 , 20x");
        assert_eq!(
            c[0],
            Command::FieldOrigin {
                x: Some(10.0),
                y: Some(20.0),
                justify: None
            }
        );
    }
}
