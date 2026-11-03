//! Two dimensional symbologies built on rxing's encoders.
//!
//! Field data is passed to the encoders as Latin-1 text, so every byte of
//! the ZPL field ends up unchanged in the symbol (no ECI is added), which is
//! what Zebra printers do as well.

use rxing::common::BitMatrix;
use rxing::datamatrix::encoder::SymbolShapeHint;
use rxing::{BarcodeFormat, Dimension, EncodeHints, Writer};

/// A rectangular grid of modules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid {
    /// Columns.
    pub width: usize,
    /// Rows.
    pub height: usize,
    /// Row major cells, `true` = dark.
    pub cells: Vec<bool>,
}

impl Grid {
    /// Dark module at `(x, y)`.
    pub fn get(&self, x: usize, y: usize) -> bool {
        x < self.width && y < self.height && self.cells[y * self.width + x]
    }

    fn from_bitmatrix(m: &BitMatrix) -> Self {
        let (w, h) = (m.getWidth() as usize, m.getHeight() as usize);
        let mut cells = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                cells.push(m.get(x as u32, y as u32));
            }
        }
        Self {
            width: w,
            height: h,
            cells,
        }
    }
}

fn latin1(data: &[u8]) -> String {
    data.iter().map(|&b| b as char).collect()
}

/// Splits `^BQ` field data (`QA,data`, `HM,Bnnnndata`, ...) into the error
/// correction level and the payload.
pub fn parse_qr_field(data: &[u8], default_ecc: u8) -> (u8, Vec<u8>) {
    let valid_ecc = |b: u8| matches!(b, b'H' | b'Q' | b'M' | b'L');
    if data.len() >= 3 && data[2] == b',' && valid_ecc(data[0].to_ascii_uppercase()) {
        let ecc = data[0].to_ascii_uppercase();
        let input = data[1].to_ascii_uppercase();
        let mut payload = &data[3..];
        if input == b'M' && !payload.is_empty() {
            let mode = payload[0].to_ascii_uppercase();
            payload = &payload[1..];
            if mode == b'B' && payload.len() >= 4 && payload[..4].iter().all(u8::is_ascii_digit) {
                payload = &payload[4..];
            }
        }
        return (ecc, payload.to_vec());
    }
    // Structured append ("D...,") or missing prefix: keep the data.
    (
        if valid_ecc(default_ecc) {
            default_ecc
        } else {
            b'Q'
        },
        data.to_vec(),
    )
}

/// Encodes a QR code.
pub fn qr(payload: &[u8], ecc: u8, mask: Option<u32>) -> Result<Grid, String> {
    if payload.is_empty() {
        return Err("empty QR code data".into());
    }
    let hints = EncodeHints {
        ErrorCorrection: Some((ecc as char).to_string()),
        CharacterSet: Some("ISO-8859-1".into()),
        Margin: Some("0".into()),
        QrMaskPattern: mask.filter(|m| *m <= 7).map(|m| m.to_string()),
        QrCompact: Some("true".into()),
        ..Default::default()
    };
    let m = rxing::qrcode::QRCodeWriter
        .encode_with_hints(&latin1(payload), &BarcodeFormat::QR_CODE, 0, 0, &hints)
        .map_err(|e| format!("QR code: {e}"))?;
    Ok(Grid::from_bitmatrix(&m))
}

/// Encodes a Data Matrix (ECC 200).
///
/// `escape` enables ZPL escape sequences: `<esc>1` (FNC1), `<esc>dNNN`
/// (byte with decimal value NNN) and a doubled escape for the character
/// itself.
pub fn datamatrix(
    payload: &[u8],
    columns: Option<u32>,
    rows: Option<u32>,
    rectangular: bool,
    escape: Option<u8>,
) -> Result<Grid, String> {
    let mut data = Vec::with_capacity(payload.len());
    let mut gs1 = false;
    let mut i = 0;
    while i < payload.len() {
        let b = payload[i];
        if Some(b) == escape && i + 1 < payload.len() {
            let next = payload[i + 1];
            match next {
                b'1' => {
                    if data.is_empty() {
                        gs1 = true;
                    } else {
                        data.push(0x1D);
                    }
                    i += 2;
                    continue;
                }
                b'd' | b'D'
                    if i + 4 < payload.len()
                        && payload[i + 2..i + 5].iter().all(u8::is_ascii_digit) =>
                {
                    let v: u32 = std::str::from_utf8(&payload[i + 2..i + 5])
                        .unwrap_or("0")
                        .parse()
                        .unwrap_or(0);
                    data.push(v.min(255) as u8);
                    i += 5;
                    continue;
                }
                _ if next == b => {
                    data.push(b);
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        data.push(b);
        i += 1;
    }
    if data.is_empty() {
        return Err("empty Data Matrix data".into());
    }
    let size = match (columns, rows) {
        (Some(c), Some(r)) if c > 0 && r > 0 => Some(Dimension::new(c as usize, r as usize)),
        (Some(c), None) if c > 0 => Some(Dimension::new(c as usize, c as usize)),
        (None, Some(r)) if r > 0 => Some(Dimension::new(r as usize, r as usize)),
        _ => None,
    };
    let shape = if size.is_some() {
        SymbolShapeHint::FORCE_NONE
    } else if rectangular {
        SymbolShapeHint::FORCE_RECTANGLE
    } else {
        SymbolShapeHint::FORCE_SQUARE
    };
    let mut hints = EncodeHints {
        DataMatrixShape: Some(shape),
        CharacterSet: Some("ISO-8859-1".into()),
        Margin: Some("0".into()),
        ..Default::default()
    };
    #[allow(deprecated)]
    if let Some(s) = size {
        hints.MinSize = Some(s);
    }
    if gs1 {
        hints.Gs1Format = Some(true);
        hints.DataMatrixCompact = Some(true);
    }
    let mut content = latin1(&data);
    if gs1 {
        content.insert(0, '\u{1D}');
    }
    let m = rxing::datamatrix::DataMatrixWriter
        .encode_with_hints(&content, &BarcodeFormat::DATA_MATRIX, 0, 0, &hints)
        .map_err(|e| format!("Data Matrix: {e}"))?;
    Ok(Grid::from_bitmatrix(&m))
}

fn pdf417_with(
    payload: &str,
    security: u32,
    cols: (u32, u32),
    rows: (u32, u32),
    compact: bool,
) -> Result<Grid, String> {
    let mut enc = rxing::pdf417::encoder::PDF417::new();
    enc.setCompact(compact);
    enc.setDimensions(cols.1, cols.0, rows.1, rows.0);
    enc.generateBarcodeLogic(payload, security)
        .map_err(|e| format!("PDF417: {e}"))?;
    let matrix = enc
        .getBarcodeMatrix()
        .as_ref()
        .ok_or("PDF417: no matrix")?
        .getMatrix();
    let height = matrix.len();
    // Trim the unused tail of each row (compact symbols are narrower).
    let width = matrix
        .iter()
        .map(|r| r.iter().rposition(|&v| v != 0).map(|p| p + 1).unwrap_or(0))
        .max()
        .unwrap_or(0);
    let mut cells = Vec::with_capacity(width * height);
    // rxing stores PDF417 rows bottom up.
    for row in matrix.iter().rev() {
        for x in 0..width {
            cells.push(row.get(x).copied().unwrap_or(0) != 0);
        }
    }
    Ok(Grid {
        width,
        height,
        cells,
    })
}

/// Encodes PDF417. Each grid row is one symbol row.
pub fn pdf417(
    payload: &[u8],
    security: Option<u32>,
    columns: Option<u32>,
    rows: Option<u32>,
    truncate: bool,
) -> Result<Grid, String> {
    let text = latin1(payload);
    let security = security.unwrap_or(0).min(8);
    let cols = columns.filter(|c| (1..=30).contains(c));
    let rows = rows.filter(|r| (3..=90).contains(r));
    match (cols, rows) {
        (Some(c), Some(r)) => pdf417_with(&text, security, (c, c), (r, r), truncate),
        (Some(c), None) => pdf417_with(&text, security, (c, c), (3, 90), truncate),
        (None, Some(r)) => pdf417_with(&text, security, (1, 30), (r, r), truncate),
        (None, None) => {
            // Zebra picks the column count whose symbol, with rows three
            // modules high, is closest to a 1:2 height to width ratio
            // (measured on Labelary for 1 to 9 columns).
            let mut best: Option<(f64, Grid)> = None;
            for c in 1..=30u32 {
                let Ok(g) = pdf417_with(&text, security, (c, c), (3, 90), truncate) else {
                    continue;
                };
                let width = 17.0 * c as f64 + 69.0;
                let score = (3.0 * g.height as f64 / width - 0.5).abs();
                if best.as_ref().is_none_or(|(s, _)| score < *s) {
                    best = Some((score, g));
                }
            }
            best.map(|(_, g)| g)
                .ok_or_else(|| "PDF417: data too long".to_string())
        }
    }
}

/// Encodes Aztec. `size` is the ZPL `^B0` size/error correction parameter.
pub fn aztec(payload: &[u8], size: Option<u32>) -> Result<Grid, String> {
    let (ecc, layers) = match size.unwrap_or(0) {
        0 => (23, 0),
        p @ 1..=99 => (p, 0),
        p @ 101..=104 => (23, -((p - 100) as i32)),
        p @ 201..=232 => (23, (p - 200) as i32),
        300 => return Err("Aztec runes are not supported".into()),
        _ => (23, 0),
    };
    let code = rxing::aztec::encoder::aztec_encoder::encode_bytes(payload, ecc, layers)
        .map_err(|e| format!("Aztec: {e}"))?;
    Ok(Grid::from_bitmatrix(code.getMatrix()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rxing::{BinaryBitmap, Luma8LuminanceSource, Reader, common::HybridBinarizer};

    fn to_image(g: &Grid, scale: usize, quiet: usize) -> (Vec<u8>, usize, usize) {
        let w = (g.width + 2 * quiet) * scale;
        let h = (g.height + 2 * quiet) * scale;
        let mut px = vec![255u8; w * h];
        for y in 0..g.height {
            for x in 0..g.width {
                if g.get(x, y) {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            px[((y + quiet) * scale + dy) * w + (x + quiet) * scale + dx] = 0;
                        }
                    }
                }
            }
        }
        (px, w, h)
    }

    fn decode(g: &Grid, reader: &mut impl Reader) -> String {
        let (px, w, h) = to_image(g, 4, 4);
        let src = Luma8LuminanceSource::new(px, w as u32, h as u32).unwrap();
        let mut bmp = BinaryBitmap::new(HybridBinarizer::new(src));
        reader
            .decode(&mut bmp)
            .expect("decodes")
            .getText()
            .to_string()
    }

    #[test]
    fn qr_field_prefixes() {
        assert_eq!(parse_qr_field(b"QA,hello", b'Q'), (b'Q', b"hello".to_vec()));
        assert_eq!(
            parse_qr_field(b"HM,B0005hello", b'Q'),
            (b'H', b"hello".to_vec())
        );
        assert_eq!(
            parse_qr_field(b"MM,N12345", b'Q'),
            (b'M', b"12345".to_vec())
        );
        assert_eq!(parse_qr_field(b"hello", b'L'), (b'L', b"hello".to_vec()));
    }

    #[test]
    fn qr_round_trip() {
        let g = qr(b"https://example.com/?a=1&b=2", b'Q', Some(7)).unwrap();
        assert_eq!(
            decode(&g, &mut rxing::qrcode::QRCodeReader),
            "https://example.com/?a=1&b=2"
        );
    }

    #[test]
    fn datamatrix_round_trip() {
        let g = datamatrix(b"ABC123 ~d065", None, None, false, Some(b'~')).unwrap();
        assert_eq!(g.width, g.height);
        assert_eq!(
            decode(&g, &mut rxing::datamatrix::DataMatrixReader),
            "ABC123 A"
        );
    }

    #[test]
    fn pdf417_shape_and_round_trip() {
        let g = pdf417(b"PDF417 test data 1234567890", Some(2), None, None, false).unwrap();
        assert!(g.height >= 3);
        let mut px_rows = Grid {
            width: g.width,
            height: g.height * 3,
            cells: Vec::new(),
        };
        for y in 0..g.height {
            for _ in 0..3 {
                px_rows
                    .cells
                    .extend_from_slice(&g.cells[y * g.width..(y + 1) * g.width]);
            }
        }
        assert_eq!(
            decode(&px_rows, &mut rxing::pdf417::PDF417Reader),
            "PDF417 test data 1234567890"
        );
    }

    #[test]
    fn aztec_round_trip() {
        let g = aztec(b"Aztec 123", None).unwrap();
        assert_eq!(decode(&g, &mut rxing::aztec::AztecReader), "Aztec 123");
    }
}
