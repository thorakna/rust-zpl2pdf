//! Monochrome bitmaps: `^GF`, `~DG` and `~DY` payload decoding.

use base64::Engine as _;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

/// A 1 bit per pixel image, rows packed MSB first, `1` meaning a printed
/// (black) dot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bitmap {
    /// Width in dots.
    pub width: u32,
    /// Height in dots.
    pub height: u32,
    /// Bytes per row.
    pub row_bytes: u32,
    /// Packed rows.
    pub data: Vec<u8>,
}

impl Bitmap {
    /// Creates an empty (all white) bitmap.
    pub fn new(width: u32, height: u32) -> Self {
        let row_bytes = width.div_ceil(8);
        Self {
            width,
            height,
            row_bytes,
            data: vec![0; (row_bytes * height) as usize],
        }
    }

    /// Returns whether the dot at `(x, y)` is black.
    pub fn get(&self, x: u32, y: u32) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let byte = self.data[(y * self.row_bytes + x / 8) as usize];
        byte & (0x80 >> (x % 8)) != 0
    }

    /// Sets the dot at `(x, y)`.
    pub fn set(&mut self, x: u32, y: u32, black: bool) {
        if x >= self.width || y >= self.height {
            return;
        }
        let idx = (y * self.row_bytes + x / 8) as usize;
        let mask = 0x80 >> (x % 8);
        if black {
            self.data[idx] |= mask;
        } else {
            self.data[idx] &= !mask;
        }
    }

    /// Whether the bitmap has no dots at all.
    pub fn is_blank(&self) -> bool {
        self.data.iter().all(|&b| b == 0)
    }

    /// Scales the bitmap by integer factors (used by `^XG` magnification).
    pub fn magnify(&self, mx: u32, my: u32) -> Bitmap {
        let (mx, my) = (mx.max(1), my.max(1));
        if mx == 1 && my == 1 {
            return self.clone();
        }
        let mut out = Bitmap::new(self.width * mx, self.height * my);
        for y in 0..self.height {
            for x in 0..self.width {
                if self.get(x, y) {
                    for dy in 0..my {
                        for dx in 0..mx {
                            out.set(x * mx + dx, y * my + dy, true);
                        }
                    }
                }
            }
        }
        out
    }
}

fn base64_engine() -> GeneralPurpose {
    GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        GeneralPurposeConfig::new()
            .with_decode_padding_mode(DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true),
    )
}

/// CRC-16/XMODEM, used by Zebra for `:Z64:` and `:B64:` payloads.
pub fn crc16_xmodem(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// Result of decoding a graphic payload.
#[derive(Debug)]
pub struct Decoded {
    /// The bitmap.
    pub bitmap: Bitmap,
    /// Non fatal problems found while decoding.
    pub warnings: Vec<String>,
}

/// Decodes a `:Z64:` / `:B64:` payload into raw bytes.
fn decode_base64_payload(
    data: &[u8],
    max_bytes: usize,
    warnings: &mut Vec<String>,
) -> Result<Vec<u8>, String> {
    let compressed = data.starts_with(b":Z64:");
    let body = &data[5..];
    let (b64, crc) = match body.iter().position(|&b| b == b':') {
        Some(i) => (&body[..i], Some(&body[i + 1..])),
        None => (body, None),
    };
    let b64: Vec<u8> = b64
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if let Some(crc) = crc {
        let expected = std::str::from_utf8(crc)
            .ok()
            .and_then(|s| u16::from_str_radix(s.trim(), 16).ok());
        if let Some(expected) = expected {
            let actual = crc16_xmodem(&b64);
            if actual != expected {
                warnings.push(format!(
                    "graphic CRC mismatch (expected {expected:04X}, computed {actual:04X})"
                ));
            }
        }
    }
    let raw = base64_engine()
        .decode(&b64)
        .map_err(|e| format!("invalid base64 graphic data: {e}"))?;
    if !compressed {
        return Ok(raw);
    }
    miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&raw, max_bytes)
        .or_else(|_| miniz_oxide::inflate::decompress_to_vec_with_limit(&raw, max_bytes))
        .map_err(|e| format!("invalid Z64 graphic data: {e:?}"))
}

/// Expands ZPL ASCII hex graphic data, including the run length compression
/// scheme (`G`..`Y`, `g`..`z` repeat counts, `,` `!` `:` row shortcuts).
pub fn decode_ascii_hex(data: &[u8], row_bytes: usize, max_bytes: usize) -> Vec<u8> {
    let row_nibbles = row_bytes * 2;
    let mut out: Vec<u8> = Vec::new();
    let mut row: Vec<u8> = Vec::with_capacity(row_nibbles);
    let mut prev: Option<Vec<u8>> = None;
    let mut count: usize = 0;
    let max_nibbles = max_bytes.saturating_mul(2);
    let mut total = 0usize;

    let flush =
        |row: &mut Vec<u8>, prev: &mut Option<Vec<u8>>, out: &mut Vec<u8>, total: &mut usize| {
            if *total < max_nibbles {
                out.extend_from_slice(row);
                *total += row.len();
            }
            *prev = Some(std::mem::take(row));
        };

    for &c in data {
        if total >= max_nibbles {
            break;
        }
        match c {
            b'G'..=b'Y' => count += (c - b'G' + 1) as usize,
            b'g'..=b'z' => count += (c - b'g' + 1) as usize * 20,
            b'0'..=b'9' | b'A'..=b'F' | b'a'..=b'f' => {
                let nibble = (c as char).to_digit(16).unwrap_or(0) as u8;
                let mut n = count.max(1);
                count = 0;
                while n > 0 {
                    let room = row_nibbles - row.len();
                    let take = n.min(room);
                    row.extend(std::iter::repeat_n(nibble, take));
                    n -= take;
                    if row.len() == row_nibbles {
                        flush(&mut row, &mut prev, &mut out, &mut total);
                        if total >= max_nibbles {
                            break;
                        }
                    }
                }
            }
            b',' | b'!' => {
                count = 0;
                let fill = if c == b',' { 0 } else { 0xF };
                row.resize(row_nibbles, fill);
                flush(&mut row, &mut prev, &mut out, &mut total);
            }
            b':' => {
                count = 0;
                let src = prev.clone().unwrap_or_else(|| vec![0; row_nibbles]);
                let start = row.len();
                row.extend_from_slice(&src[start..row_nibbles]);
                flush(&mut row, &mut prev, &mut out, &mut total);
            }
            _ => {}
        }
    }
    if !row.is_empty() {
        row.resize(row_nibbles, 0);
        out.extend_from_slice(&row);
    }
    out.chunks(2)
        .map(|p| (p[0] << 4) | p.get(1).copied().unwrap_or(0))
        .collect()
}

/// Decodes a graphic payload (`^GF` or `~DG`) into a bitmap.
///
/// `format` is the ZPL compression type (`A` ASCII hex, `B` binary, `C`
/// compressed binary), `total_bytes` and `row_bytes` come from the command.
pub fn decode_graphic(
    format: u8,
    total_bytes: usize,
    row_bytes: usize,
    data: &[u8],
    max_bytes: usize,
) -> Result<Decoded, String> {
    if row_bytes == 0 {
        return Err("graphic has zero bytes per row".into());
    }
    if total_bytes > max_bytes {
        return Err(format!(
            "graphic of {total_bytes} bytes exceeds the limit of {max_bytes}"
        ));
    }
    let mut warnings = Vec::new();
    let trimmed: Vec<u8> = data
        .iter()
        .copied()
        .skip_while(|b| b.is_ascii_whitespace())
        .collect();
    let mut bytes = if trimmed.starts_with(b":Z64:") || trimmed.starts_with(b":B64:") {
        decode_base64_payload(&trimmed, max_bytes, &mut warnings)?
    } else {
        match format.to_ascii_uppercase() {
            b'B' => data.to_vec(),
            b'C' => return Err("compressed binary (^GFC) graphics are not supported".into()),
            _ => decode_ascii_hex(&trimmed, row_bytes, max_bytes),
        }
    };
    let total = if total_bytes == 0 {
        bytes.len()
    } else {
        total_bytes
    };
    let height = total.div_ceil(row_bytes);
    bytes.resize(height * row_bytes, 0);
    Ok(Decoded {
        bitmap: Bitmap {
            width: (row_bytes * 8) as u32,
            height: height as u32,
            row_bytes: row_bytes as u32,
            data: bytes,
        },
        warnings,
    })
}

/// Decodes a PNG file (for `~DY` with the `P` extension) into a bitmap.
/// Pixels darker than 50% and not transparent become black.
pub fn decode_png(data: &[u8], max_bytes: usize) -> Result<Bitmap, String> {
    let mut decoder = png::Decoder::new_with_limits(
        std::io::Cursor::new(data),
        png::Limits {
            bytes: max_bytes.max(1 << 20),
        },
    );
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("invalid PNG: {e}"))?;
    let size = reader.output_buffer_size().ok_or("PNG too large")?;
    let mut buf = vec![0; size];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("invalid PNG: {e}"))?;
    let channels = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err("unexpected indexed PNG output".into()),
    };
    let mut bmp = Bitmap::new(info.width, info.height);
    for y in 0..info.height {
        let line = &buf[y as usize * info.line_size..];
        for x in 0..info.width {
            let px = &line[x as usize * channels..x as usize * channels + channels];
            let (luma, alpha) = match channels {
                1 => (px[0] as u32, 255),
                2 => (px[0] as u32, px[1] as u32),
                3 => (
                    (px[0] as u32 * 299 + px[1] as u32 * 587 + px[2] as u32 * 114) / 1000,
                    255,
                ),
                _ => (
                    (px[0] as u32 * 299 + px[1] as u32 * 587 + px[2] as u32 * 114) / 1000,
                    px[3] as u32,
                ),
            };
            if alpha >= 128 && luma < 128 {
                bmp.set(x, y, true);
            }
        }
    }
    Ok(bmp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_hex() {
        let d = decode_graphic(b'A', 4, 2, b"FF000F0F", 1 << 20).unwrap();
        assert_eq!(d.bitmap.data, [0xFF, 0x00, 0x0F, 0x0F]);
        assert_eq!((d.bitmap.width, d.bitmap.height), (16, 2));
    }

    #[test]
    fn compressed_hex() {
        // Row 1: 3 x "F" then fill zeros. Row 2: repeat. Row 3: all ones.
        let d = decode_graphic(b'A', 6, 2, b"IF,:!", 1 << 20).unwrap();
        assert_eq!(d.bitmap.data, [0xFF, 0xF0, 0xFF, 0xF0, 0xFF, 0xFF]);
        // g = 20 repeats.
        let d = decode_ascii_hex(b"gA", 10, 1 << 20);
        assert_eq!(d, vec![0xAA; 10]);
    }

    #[test]
    fn z64_payload() {
        let raw = [0xF0u8, 0x0F, 0xAA, 0x55];
        let z = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6);
        let b64 = base64::engine::general_purpose::STANDARD.encode(&z);
        let crc = crc16_xmodem(b64.as_bytes());
        let payload = format!(":Z64:{b64}:{crc:04X}");
        let d = decode_graphic(b'A', 4, 2, payload.as_bytes(), 1 << 20).unwrap();
        assert_eq!(d.bitmap.data, raw);
        assert!(d.warnings.is_empty());
    }

    #[test]
    fn limits_are_enforced() {
        assert!(decode_graphic(b'A', 10_000_000, 100, b"", 1 << 20).is_err());
        let d = decode_ascii_hex(b"zzzzzzzzzzzzzzzzF", 1, 4);
        assert!(d.len() <= 5);
    }

    #[test]
    fn magnify() {
        let mut b = Bitmap::new(2, 1);
        b.set(1, 0, true);
        let m = b.magnify(2, 3);
        assert_eq!((m.width, m.height), (4, 3));
        assert!(m.get(2, 2) && m.get(3, 0) && !m.get(1, 1));
    }
}
