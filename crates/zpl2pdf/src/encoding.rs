//! Field data decoding for `^CI` character sets and `^FH` hex escapes.

/// Code page 850, bytes 0x80..=0xFF. Zebra's default `^CI0` maps the upper
/// half of the byte range to these characters.
const CP850_HIGH: [char; 128] = [
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', //
    'É', 'æ', 'Æ', 'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', 'ø', '£', 'Ø', '×', 'ƒ', //
    'á', 'í', 'ó', 'ú', 'ñ', 'Ñ', 'ª', 'º', '¿', '®', '¬', '½', '¼', '¡', '«', '»', //
    '░', '▒', '▓', '│', '┤', 'Á', 'Â', 'À', '©', '╣', '║', '╗', '╝', '¢', '¥', '┐', //
    '└', '┴', '┬', '├', '─', '┼', 'ã', 'Ã', '╚', '╔', '╩', '╦', '╠', '═', '╬', '¤', //
    'ð', 'Ð', 'Ê', 'Ë', 'È', 'ı', 'Í', 'Î', 'Ï', '┘', '┌', '█', '▄', '¦', 'Ì', '▀', //
    'Ó', 'ß', 'Ô', 'Ò', 'õ', 'Õ', 'µ', 'þ', 'Þ', 'Ú', 'Û', 'Ù', 'ý', 'Ý', '¯', '´', //
    '\u{AD}', '±', '‗', '¾', '¶', '§', '÷', '¸', '°', '¨', '·', '¹', '³', '²', '■', '\u{A0}',
];

/// National character substitutions of `^CI1` to `^CI12` for the bytes
/// `# 0 @ [ \ ] ^ ` { | } ~` (in that order).
const NATIONAL: [(u32, [char; 12]); 12] = [
    (
        1,
        ['#', '0', '@', '[', '\\', ']', '^', '`', '{', '|', '}', '~'],
    ), // USA 2 (same as USA 1 here)
    (
        2,
        ['£', '0', '@', '[', '\\', ']', '^', '`', '{', '|', '}', '~'],
    ), // UK
    (
        3,
        ['ƒ', '0', '§', '[', 'ĳ', ']', '^', '`', '{', 'ĳ', '}', '~'],
    ), // Holland (approximated)
    (
        4,
        ['#', '0', 'É', 'Æ', 'Ø', 'Å', 'Ü', 'é', 'æ', 'ø', 'å', 'ü'],
    ), // Denmark/Norway
    (
        5,
        ['#', '0', 'É', 'Ä', 'Ö', 'Å', 'Ü', 'é', 'ä', 'ö', 'å', 'ü'],
    ), // Sweden/Finland
    (
        6,
        ['#', '0', '§', 'Ä', 'Ö', 'Ü', '^', '`', 'ä', 'ö', 'ü', 'ß'],
    ), // Germany
    (
        7,
        ['£', '0', 'à', '°', 'ç', '§', '^', '`', 'é', 'ù', 'è', '¨'],
    ), // France 1
    (
        8,
        ['#', '0', 'à', 'â', 'ç', 'ê', 'î', 'ô', 'é', 'ù', 'è', 'û'],
    ), // France 2
    (
        9,
        ['£', '0', '§', '°', 'ç', 'é', '^', 'ù', 'à', 'ò', 'è', 'ì'],
    ), // Italy
    (
        10,
        ['#', '0', '§', '¡', 'Ñ', '¿', '^', '`', '°', 'ñ', 'ç', '~'],
    ), // Spain
    (
        11,
        ['#', '0', '@', '[', '\\', ']', '^', '`', '{', '|', '}', '~'],
    ), // Miscellaneous
    (
        12,
        ['#', '0', '@', '[', '¥', ']', '^', '`', '{', '|', '}', '~'],
    ), // Japan (ASCII + yen)
];

const NATIONAL_BYTES: [u8; 12] = *b"#0@[\\]^`{|}~";

/// Applies `^FH` hexadecimal escapes: `indicator` followed by two hex digits
/// becomes one byte. Invalid sequences are kept as they are.
pub fn apply_hex_escapes(data: &[u8], indicator: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] == indicator && i + 2 < data.len() {
            let hi = (data[i + 1] as char).to_digit(16);
            let lo = (data[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(data[i]);
        i += 1;
    }
    out
}

/// Decodes field data bytes according to a `^CI` character set number.
pub fn decode(data: &[u8], ci: u32) -> String {
    match ci {
        28 => String::from_utf8_lossy(data).into_owned(),
        29 => decode_utf16(data, true),
        30 => decode_utf16(data, false),
        27 => decode_with(encoding_rs::WINDOWS_1252, data),
        31 => decode_with(encoding_rs::WINDOWS_1255, data),
        33 => decode_with(encoding_rs::WINDOWS_1250, data),
        34 => decode_with(encoding_rs::WINDOWS_1251, data),
        35 => decode_with(encoding_rs::WINDOWS_1253, data),
        36 => decode_with(encoding_rs::WINDOWS_1254, data),
        13 => data.iter().map(|&b| cp850(b)).collect(),
        1..=12 => {
            let table = NATIONAL.iter().find(|(n, _)| *n == ci).map(|(_, t)| t);
            data.iter()
                .map(
                    |&b| match (table, NATIONAL_BYTES.iter().position(|&x| x == b)) {
                        (Some(t), Some(p)) if b != b'0' => t[p],
                        _ => cp850(b),
                    },
                )
                .collect()
        }
        // ^CI0 and anything we do not know: ASCII with code page 850 above.
        _ => data.iter().map(|&b| cp850(b)).collect(),
    }
}

fn cp850(b: u8) -> char {
    if b < 0x80 {
        b as char
    } else {
        CP850_HIGH[(b - 0x80) as usize]
    }
}

fn decode_with(enc: &'static encoding_rs::Encoding, data: &[u8]) -> String {
    enc.decode_without_bom_handling(data).0.into_owned()
}

fn decode_utf16(data: &[u8], big_endian: bool) -> String {
    let units: Vec<u16> = data
        .chunks(2)
        .map(|c| {
            let (a, b) = (c[0], *c.get(1).unwrap_or(&0));
            if big_endian {
                u16::from_be_bytes([a, b])
            } else {
                u16::from_le_bytes([a, b])
            }
        })
        .collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_escapes() {
        assert_eq!(apply_hex_escapes(b"A_5EB_7e", b'_'), b"A^B~");
        assert_eq!(apply_hex_escapes(b"_ZZ_4", b'_'), b"_ZZ_4");
        assert_eq!(apply_hex_escapes(b"x_", b'_'), b"x_");
    }

    #[test]
    fn decodings() {
        assert_eq!(decode("Ğüş".as_bytes(), 28), "Ğüş");
        assert_eq!(decode(&[0x80, 0x81], 0), "Çü");
        assert_eq!(decode(&[0xD0, 0xFE], 36), "Ğş");
        assert_eq!(decode(&[0xE9], 27), "é");
        assert_eq!(decode(b"[x]", 6), "ÄxÜ");
        assert_eq!(decode(&[0x00, 0x41], 29), "A");
    }
}
