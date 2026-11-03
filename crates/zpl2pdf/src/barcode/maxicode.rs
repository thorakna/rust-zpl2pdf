//! MaxiCode encoder (`^BD`), ISO/IEC 16023.
//!
//! Supports modes 2 and 3 (structured carrier message, as used by UPS),
//! 4 (standard), 5 (enhanced error correction) and 6 (reader programming).
//! The character sets mirror the ZXing decoder so that every symbol produced
//! here decodes back to its input.

use rxing::common::reedsolomon::{PredefinedGenericGF, ReedSolomonEncoder};

use super::maxicode_grid::BITNR;

// Special values used in the character set tables below.
const ECI: u32 = 0xFFFA;
const NS: u32 = 0xFFFB;
const PAD: u32 = 0xFFFC;
const SHIFT_A: u32 = 0xFFF0;
const SHIFT_B: u32 = 0xFFF1;
const SHIFT_C: u32 = 0xFFF2;
const SHIFT_D: u32 = 0xFFF3;
const SHIFT_E: u32 = 0xFFF4;
const TWO_SHIFT_A: u32 = 0xFFF5;
const THREE_SHIFT_A: u32 = 0xFFF6;
const LATCH_A: u32 = 0xFFF7;
const LATCH_B: u32 = 0xFFF8;
const LOCK: u32 = 0xFFF9;

/// The five code sets (A..E), 64 entries each. Derived from the ZXing
/// MaxiCode decoder (Apache License 2.0).
fn code_sets() -> &'static [[u32; 64]; 5] {
    use std::sync::OnceLock;
    static SETS: OnceLock<[[u32; 64]; 5]> = OnceLock::new();
    SETS.get_or_init(|| {
        let mut sets = [[0u32; 64]; 5];
        let fill = |set: &mut [u32; 64], parts: &[&[u32]]| {
            let all: Vec<u32> = parts.iter().flat_map(|p| p.iter().copied()).collect();
            assert_eq!(all.len(), 64, "code set must have 64 entries");
            set.copy_from_slice(&all);
        };
        let chars = |s: &str| -> Vec<u32> { s.chars().map(|c| c as u32).collect() };
        let fs = 0x1C;
        let gs = 0x1D;
        let rs = 0x1E;
        let a1 = chars("\rABCDEFGHIJKLMNOPQRSTUVWXYZ");
        let a2 = chars("\"#$%&'()*+,-./0123456789:");
        fill(
            &mut sets[0],
            &[&a1, &[ECI, fs, gs, rs, NS, ' ' as u32, PAD], &a2, &[SHIFT_B, SHIFT_C, SHIFT_D, SHIFT_E, LATCH_B]],
        );
        let b1 = chars("`abcdefghijklmnopqrstuvwxyz");
        let b2 = chars("}~\u{7F};<=>?[\\]^_ ,./:@!|");
        fill(
            &mut sets[1],
            &[
                &b1,
                &[ECI, fs, gs, rs, NS, '{' as u32, PAD],
                &b2,
                &[PAD, TWO_SHIFT_A, THREE_SHIFT_A, PAD, SHIFT_A, SHIFT_C, SHIFT_D, SHIFT_E, LATCH_A],
            ],
        );
        let c1: Vec<u32> = (0xC0..=0xDA).collect();
        let c2 = chars("\u{DB}\u{DC}\u{DD}\u{DE}\u{DF}\u{AA}\u{AC}\u{B1}\u{B2}\u{B3}\u{B5}\u{B9}\u{BA}\u{BC}\u{BD}\u{BE}\u{80}\u{81}\u{82}\u{83}\u{84}\u{85}\u{86}\u{87}\u{88}\u{89}");
        fill(
            &mut sets[2],
            &[&c1, &[ECI, fs, gs, rs, NS], &c2, &[LATCH_A, ' ' as u32, LOCK, SHIFT_D, SHIFT_E, LATCH_B]],
        );
        let d1: Vec<u32> = (0xE0..=0xFA).collect();
        let d2 = chars("\u{FB}\u{FC}\u{FD}\u{FE}\u{FF}\u{A1}\u{A8}\u{AB}\u{AF}\u{B0}\u{B4}\u{B7}\u{B8}\u{BB}\u{BF}\u{8A}\u{8B}\u{8C}\u{8D}\u{8E}\u{8F}\u{90}\u{91}\u{92}\u{93}\u{94}");
        fill(
            &mut sets[3],
            &[&d1, &[ECI, fs, gs, rs, NS], &d2, &[LATCH_A, ' ' as u32, SHIFT_C, LOCK, SHIFT_E, LATCH_B]],
        );
        let e1: Vec<u32> = (0x00..=0x1A).collect();
        let e2 = chars("\u{1F}\u{9F}\u{A0}\u{A2}\u{A3}\u{A4}\u{A5}\u{A6}\u{A7}\u{A9}\u{AD}\u{AE}\u{B6}\u{95}\u{96}\u{97}\u{98}\u{99}\u{9A}\u{9B}\u{9C}\u{9D}\u{9E}");
        fill(
            &mut sets[4],
            &[&e1, &[ECI, PAD, PAD, 0x1B, NS, fs, gs, rs], &e2, &[LATCH_A, ' ' as u32, SHIFT_C, SHIFT_D, LOCK, LATCH_B]],
        );
        sets
    })
}

fn value_in(set: usize, ch: u32) -> Option<u8> {
    code_sets()[set]
        .iter()
        .position(|&c| c == ch)
        .map(|p| p as u8)
}

fn control_in(set: usize, ctl: u32) -> u8 {
    value_in(set, ctl).expect("control code exists in set")
}

/// Encodes message bytes (Latin-1) into MaxiCode codewords with simple
/// set switching: sets A and B are latched, C, D and E are shifted per
/// character, runs of nine or more digits use numeric shift.
fn encode_message(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut set = 0usize; // 0 = A, 1 = B
    let mut i = 0;
    let only_in = |s: usize, c: u32| value_in(s, c).is_some();
    while i < data.len() {
        let digits = data[i..].iter().take_while(|b| b.is_ascii_digit()).count();
        if digits >= 9 {
            let n: u32 = std::str::from_utf8(&data[i..i + 9])
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            out.push(control_in(set, NS));
            for shift in [24, 18, 12, 6, 0] {
                out.push(((n >> shift) & 0x3F) as u8);
            }
            i += 9;
            continue;
        }
        let ch = data[i] as u32;
        if let Some(v) = value_in(set, ch) {
            out.push(v);
            i += 1;
            continue;
        }
        let other = 1 - set;
        if let Some(v) = value_in(other, ch) {
            let next_needs_other = data
                .get(i + 1)
                .is_some_and(|&n| !only_in(set, n as u32) && only_in(other, n as u32));
            if next_needs_other {
                out.push(control_in(set, if other == 0 { LATCH_A } else { LATCH_B }));
                set = other;
            } else {
                out.push(control_in(set, if other == 0 { SHIFT_A } else { SHIFT_B }));
            }
            out.push(v);
            i += 1;
            continue;
        }
        if let Some((target, v)) = (2..5).find_map(|s| value_in(s, ch).map(|v| (s, v))) {
            let shift = [SHIFT_A, SHIFT_B, SHIFT_C, SHIFT_D, SHIFT_E][target];
            out.push(control_in(set, shift));
            out.push(v);
        }
        i += 1;
    }
    let _ = (TWO_SHIFT_A, THREE_SHIFT_A, LOCK);
    out
}

fn set_bits(codewords: &mut [u8], value: u32, positions: &[u8]) {
    let n = positions.len();
    for (i, &p) in positions.iter().enumerate() {
        let bit = (value >> (n - 1 - i)) & 1;
        if bit == 1 {
            let idx = (p - 1) as usize;
            codewords[idx / 6] |= 1 << (5 - (idx % 6));
        }
    }
}

const COUNTRY_BITS: [u8; 10] = [53, 54, 43, 44, 45, 46, 47, 48, 37, 38];
const SERVICE_BITS: [u8; 10] = [55, 56, 57, 58, 59, 60, 49, 50, 51, 52];
const POSTCODE2_LENGTH_BITS: [u8; 6] = [39, 40, 41, 42, 31, 32];
const POSTCODE2_BITS: [u8; 30] = [
    33, 34, 35, 36, 25, 26, 27, 28, 29, 30, 19, 20, 21, 22, 23, 24, 13, 14, 15, 16, 17, 18, 7, 8,
    9, 10, 11, 12, 1, 2,
];
const POSTCODE3_BITS: [[u8; 6]; 6] = [
    [39, 40, 41, 42, 31, 32],
    [33, 34, 35, 36, 25, 26],
    [27, 28, 29, 30, 19, 20],
    [21, 22, 23, 24, 13, 14],
    [15, 16, 17, 18, 7, 8],
    [9, 10, 11, 12, 1, 2],
];

/// Structured carrier message fields of modes 2 and 3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Primary {
    /// Postal code.
    pub postal: String,
    /// ISO 3166 numeric country code.
    pub country: u32,
    /// Class of service.
    pub service: u32,
}

/// Splits ZPL `^BD` field data into primary fields and secondary message.
///
/// Two layouts are accepted: the ZPL "high priority message" prefix
/// (`aaabbbppppppppp...`, class, country and postal code) and the
/// `[)>RS01GSyy` transportation format where the postal code, country and
/// class are the first three fields.
pub fn split_carrier_message(data: &[u8], mode: u8) -> (Primary, Vec<u8>) {
    const HEADER: &[u8] = b"[)>\x1e01\x1d";
    if data.starts_with(HEADER) && data.len() >= HEADER.len() + 2 {
        let version = &data[HEADER.len()..HEADER.len() + 2];
        let rest = &data[HEADER.len() + 2..];
        let mut parts = rest.splitn(4, |&b| b == 0x1D);
        let postal = parts.next().unwrap_or_default();
        let country = parts.next().unwrap_or_default();
        let service = parts.next().unwrap_or_default();
        let tail = parts.next().unwrap_or_default();
        let mut secondary = HEADER.to_vec();
        secondary.extend_from_slice(version);
        secondary.extend_from_slice(tail);
        let num = |b: &[u8]| {
            String::from_utf8_lossy(b)
                .trim()
                .parse::<u32>()
                .unwrap_or(0)
        };
        return (
            Primary {
                postal: String::from_utf8_lossy(postal).into_owned(),
                country: num(country),
                service: num(service),
            },
            secondary,
        );
    }
    let take = |from: usize, len: usize| -> &[u8] {
        &data[from.min(data.len())..(from + len).min(data.len())]
    };
    let num = |b: &[u8]| {
        String::from_utf8_lossy(b)
            .trim()
            .parse::<u32>()
            .unwrap_or(0)
    };
    let service = num(take(0, 3));
    let country = num(take(3, 3));
    let postal_len = if mode == 2 { 9 } else { 6 };
    let postal = String::from_utf8_lossy(take(6, postal_len))
        .trim()
        .to_string();
    let secondary = data[(6 + postal_len).min(data.len())..].to_vec();
    (
        Primary {
            postal,
            country,
            service,
        },
        secondary,
    )
}

/// Encoded MaxiCode symbol: dark modules on the 33 x 30 grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaxiCode {
    /// `modules[row][column]`.
    pub modules: [[bool; 30]; 33],
    /// Mode actually used.
    pub mode: u8,
    /// Whether the message had to be truncated.
    pub truncated: bool,
}

/// Encodes a `^BD` field. `mode` is 2 to 6.
pub fn encode(data: &[u8], mode: u8) -> MaxiCode {
    let mut mode = if (2..=6).contains(&mode) { mode } else { 2 };
    let mut codewords = [0u8; 144];
    let mut truncated = false;

    let (message, primary) = if mode == 2 || mode == 3 {
        let (primary, secondary) = split_carrier_message(data, mode);
        if mode == 2 && !primary.postal.chars().all(|c| c.is_ascii_digit()) {
            mode = 3;
        }
        (secondary, Some(primary))
    } else {
        (data.to_vec(), None)
    };

    let mut msg = encode_message(&message);
    let pad = control_in(0, PAD);
    if let Some(p) = primary {
        if mode == 2 {
            let digits: String = p
                .postal
                .chars()
                .filter(|c| c.is_ascii_digit())
                .take(9)
                .collect();
            let value: u32 = digits.parse().unwrap_or(0);
            set_bits(&mut codewords, value, &POSTCODE2_BITS);
            set_bits(&mut codewords, digits.len() as u32, &POSTCODE2_LENGTH_BITS);
        } else {
            let mut chars: Vec<u8> = p.postal.to_ascii_uppercase().bytes().take(6).collect();
            while chars.len() < 6 {
                chars.push(b' ');
            }
            for (i, c) in chars.iter().enumerate() {
                let v =
                    value_in(0, *c as u32).unwrap_or_else(|| value_in(0, ' ' as u32).unwrap_or(32));
                set_bits(&mut codewords, v as u32, &POSTCODE3_BITS[i]);
            }
        }
        set_bits(&mut codewords, p.country.min(999), &COUNTRY_BITS);
        set_bits(&mut codewords, p.service.min(999), &SERVICE_BITS);
        codewords[0] = (codewords[0] & 0x30) | mode;
        if msg.len() > 84 {
            msg.truncate(84);
            truncated = true;
        }
        msg.resize(84, pad);
        codewords[20..104].copy_from_slice(&msg);
    } else {
        let secondary_len = if mode == 5 { 68 } else { 84 };
        let capacity = 9 + secondary_len;
        if msg.len() > capacity {
            msg.truncate(capacity);
            truncated = true;
        }
        msg.resize(capacity, pad);
        codewords[0] = mode;
        codewords[1..10].copy_from_slice(&msg[..9]);
        codewords[20..20 + secondary_len].copy_from_slice(&msg[9..]);
    }

    // Error correction.
    let mut rs =
        ReedSolomonEncoder::new(PredefinedGenericGF::MaxicodeField64.into()).expect("valid field");
    let mut primary_block: Vec<i32> = codewords[..10].iter().map(|&c| c as i32).collect();
    primary_block.extend([0; 10]);
    rs.encode(&mut primary_block, 10)
        .expect("primary error correction");
    for (i, v) in primary_block.iter().enumerate().skip(10) {
        codewords[i] = *v as u8;
    }
    let (data_len, ec_len) = if mode == 5 { (68, 56) } else { (84, 40) };
    for parity in 0..2 {
        let mut block: Vec<i32> = (0..data_len)
            .filter(|j| j % 2 == parity)
            .map(|j| codewords[20 + j] as i32)
            .collect();
        block.extend(std::iter::repeat_n(0, ec_len / 2));
        rs.encode(&mut block, ec_len / 2)
            .expect("secondary error correction");
        for (k, v) in block[data_len / 2..].iter().enumerate() {
            codewords[20 + data_len + 2 * k + parity] = *v as u8;
        }
    }

    let mut modules = [[false; 30]; 33];
    for (r, row) in BITNR.iter().enumerate() {
        for (c, &bit) in row.iter().enumerate() {
            modules[r][c] = match bit {
                -2 => true,
                b if b >= 0 => {
                    let b = b as usize;
                    codewords[b / 6] & (1 << (5 - (b % 6))) != 0
                }
                _ => false,
            };
        }
    }
    MaxiCode {
        modules,
        mode,
        truncated,
    }
}

/// Geometry of a MaxiCode symbol in dots, matching Zebra's physical size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    /// Horizontal module pitch.
    pub pitch_x: f64,
    /// Vertical row pitch.
    pub pitch_y: f64,
    /// Hexagon width (flat side to flat side).
    pub hex_width: f64,
    /// Hexagon height (point to point).
    pub hex_height: f64,
    /// Finder ring radii as `(inner, outer)` pairs of the dark rings.
    pub rings: [(f64, f64); 3],
}

impl Geometry {
    /// Geometry for a printer resolution.
    pub fn for_dpi(dpi: f64) -> Self {
        let pitch_x = dpi / 30.0;
        let pitch_y = pitch_x * 3f64.sqrt() / 2.0;
        let hex_width = pitch_x * 0.85;
        let r = |v: f64| v * pitch_x;
        Self {
            pitch_x,
            pitch_y,
            hex_width,
            hex_height: hex_width * 2.0 / 3f64.sqrt(),
            rings: [
                (r(0.583), r(1.33)),
                (r(2.128), r(2.875)),
                (r(3.622), r(4.419)),
            ],
        }
    }

    /// Center of a module relative to the symbol's top left corner.
    pub fn center(&self, row: usize, col: usize) -> (f64, f64) {
        let x = self.hex_width / 2.0
            + col as f64 * self.pitch_x
            + if row % 2 == 1 {
                self.pitch_x / 2.0
            } else {
                0.0
            };
        let y = self.hex_height / 2.0 + row as f64 * self.pitch_y;
        (x, y)
    }

    /// Center of the finder pattern.
    pub fn finder_center(&self) -> (f64, f64) {
        self.center(16, 14)
    }

    /// Total size `(width, height)`.
    pub fn size(&self) -> (f64, f64) {
        (
            self.pitch_x * 29.5 + self.hex_width,
            self.pitch_y * 32.0 + self.hex_height,
        )
    }

    /// Hexagon outline (pointy top) around a center.
    pub fn hexagon(&self, cx: f64, cy: f64) -> [(f64, f64); 6] {
        let hw = self.hex_width / 2.0;
        let hh = self.hex_height / 2.0;
        [
            (cx, cy - hh),
            (cx + hw, cy - hh / 2.0),
            (cx + hw, cy + hh / 2.0),
            (cx, cy + hh),
            (cx - hw, cy + hh / 2.0),
            (cx - hw, cy - hh / 2.0),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rxing::common::BitMatrix;

    fn to_bitmatrix(m: &MaxiCode) -> BitMatrix {
        let mut bm = BitMatrix::new(30, 33).unwrap();
        for (r, row) in m.modules.iter().enumerate() {
            for (c, &v) in row.iter().enumerate() {
                if v {
                    bm.set(c as u32, r as u32);
                }
            }
        }
        bm
    }

    fn decode(m: &MaxiCode) -> String {
        rxing::maxicode::decoder::maxicode_decoder::decode(&to_bitmatrix(m))
            .expect("decodes")
            .getText()
            .to_string()
    }

    #[test]
    fn mode2_hpm_round_trip() {
        let data = b"002840100450000[)>\x1e01\x1d961Z00004951\x1dUPSN\x1d06X610\x1d159\x1d1234567\x1d1/1\x1d\x1dY\x1d634 ALPHA DR\x1dPITTSBURGH\x1dPA\x1e\x04";
        let m = encode(data, 2);
        assert_eq!(m.mode, 2);
        let text = decode(&m);
        assert!(
            text.starts_with("[)>\u{1e}01\u{1d}96100450000\u{1d}840\u{1d}002\u{1d}1Z00004951"),
            "{text:?}"
        );
        assert!(text.contains("PITTSBURGH"));
    }

    #[test]
    fn mode2_transport_format_round_trip() {
        let data = b"[)>\x1e01\x1d96841706672\x1d840\x1d001\x1d1Z12345675\x1dUPSN\x1d12345E\x1d089\x1d\x1d1/1\x1d10.1\x1dY\x1d\x1d\x1dUT\x1e\x04";
        let m = encode(data, 2);
        let text = decode(&m);
        assert!(
            text.starts_with("[)>\u{1e}01\u{1d}96841706672\u{1d}840\u{1d}001\u{1d}1Z12345675"),
            "{text:?}"
        );
    }

    #[test]
    fn mode3_round_trip() {
        let data = b"[)>\x1e01\x1d96B1050\x1d056\x1d066\x1dhello world 123456789012\x1e\x04";
        let m = encode(data, 3);
        assert_eq!(m.mode, 3);
        let text = decode(&m);
        assert!(
            text.contains("B1050 \u{1d}056\u{1d}066\u{1d}hello world 123456789012"),
            "{text:?}"
        );
    }

    #[test]
    fn mode4_round_trip() {
        let msg = b"Mixed Case, digits 1234567890 and symbols !@#";
        let m = encode(msg, 4);
        assert_eq!(decode(&m), String::from_utf8_lossy(msg));
    }
}
