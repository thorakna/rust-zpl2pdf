//! One dimensional symbologies with ZPL specific behaviour.
//!
//! The encoders return element widths in dots (bar, space, bar, ...) and the
//! interpretation line text. Code 128 follows the subset rules and the `>`
//! invocation codes of Zebra printers as observed on Labelary.

/// Code 128 bar/space patterns for values 0..=105, stop is separate.
const C128_PATTERNS: [&str; 106] = [
    "212222", "222122", "222221", "121223", "121322", "131222", "122213", "122312", "132212",
    "221213", "221312", "231212", "112232", "122132", "122231", "113222", "123122", "123221",
    "223211", "221132", "221231", "213212", "223112", "312131", "311222", "321122", "321221",
    "312212", "322112", "322211", "212123", "212321", "232121", "111323", "131123", "131321",
    "112313", "132113", "132311", "211313", "231113", "231311", "112133", "112331", "132131",
    "113123", "113321", "133121", "313121", "211331", "231131", "213113", "213311", "213131",
    "311123", "311321", "331121", "312113", "312311", "332111", "314111", "221411", "431111",
    "111224", "111422", "121124", "121421", "141122", "141221", "112214", "112412", "122114",
    "122411", "142112", "142211", "241211", "221114", "413111", "241112", "134111", "111242",
    "121142", "121241", "114212", "124112", "124211", "411212", "421112", "421211", "212141",
    "214121", "412121", "111143", "111341", "131141", "114113", "114311", "411113", "411311",
    "113141", "114131", "311141", "411131", "211412", "211214", "211232",
];
const C128_STOP: &str = "2331112";

const START_A: u8 = 103;
const START_B: u8 = 104;
const START_C: u8 = 105;
const SHIFT: u8 = 98;
const CODE_C: u8 = 99;
const CODE_B: u8 = 100;
const CODE_A: u8 = 101;
const FNC1: u8 = 102;

/// Result of a linear encoder.
#[derive(Debug, Clone, PartialEq)]
pub struct Encoded {
    /// Element widths in dots, alternating bar and space, starting with a bar.
    pub elements: Vec<f64>,
    /// Interpretation line text.
    pub text: String,
}

impl Encoded {
    /// Total width in dots.
    pub fn width(&self) -> f64 {
        self.elements.iter().sum()
    }

    /// Bars as `(x, width)` pairs.
    pub fn bars(&self) -> Vec<(f64, f64)> {
        let mut x = 0.0;
        let mut out = Vec::new();
        for (i, w) in self.elements.iter().enumerate() {
            if i % 2 == 0 && *w > 0.0 {
                out.push((x, *w));
            }
            x += w;
        }
        out
    }
}

fn modules_to_elements(patterns: &[&str], module: f64) -> Vec<f64> {
    patterns
        .iter()
        .flat_map(|p| p.bytes().map(move |b| (b - b'0') as f64 * module))
        .collect()
}

/// Code 128 modes of `^BC`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code128Mode {
    /// `N`: subset B by default, `>` invocation codes.
    None,
    /// `U`: UCC case mode, 19 digits plus a check digit.
    Ucc,
    /// `A`: automatic subset selection.
    Auto,
    /// `D`: UCC/EAN (GS1-128) mode.
    Ean,
}

impl Code128Mode {
    /// Parses the `^BC` mode letter.
    pub fn from_byte(b: Option<u8>) -> Self {
        match b {
            Some(b'U') => Self::Ucc,
            Some(b'A') => Self::Auto,
            Some(b'D') => Self::Ean,
            _ => Self::None,
        }
    }
}

/// GS1 mod 10 check digit (weights 3, 1 from the right).
pub fn mod10_check(digits: &[u8]) -> u8 {
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, d)| (d - b'0') as u32 * if i % 2 == 0 { 3 } else { 1 })
        .sum();
    b'0' + ((10 - sum % 10) % 10) as u8
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sym {
    Char(u8),
    Fnc1,
}

fn value_in_a(c: u8) -> Option<u8> {
    match c {
        0..=31 => Some(c + 64),
        32..=95 => Some(c - 32),
        _ => None,
    }
}

fn value_in_b(c: u8) -> Option<u8> {
    match c {
        32..=127 => Some(c - 32),
        _ => None,
    }
}

fn finish_code128(values: &[u8], module: f64) -> Vec<f64> {
    let mut sum = values[0] as u32;
    for (i, v) in values.iter().enumerate().skip(1) {
        sum += *v as u32 * i as u32;
    }
    let check = (sum % 103) as u8;
    let mut pats: Vec<&str> = values.iter().map(|&v| C128_PATTERNS[v as usize]).collect();
    pats.push(C128_PATTERNS[check as usize]);
    pats.push(C128_STOP);
    modules_to_elements(&pats, module)
}

/// Mode N: explicit subsets with `>` invocation codes.
fn code128_manual(data: &[u8]) -> (Vec<u8>, String) {
    let mut values = Vec::new();
    let mut text = String::new();
    let mut set = b'B';
    let mut i = 0;
    // Start code.
    if data.len() >= 2 && data[0] == b'>' {
        match data[1] {
            b'9' => {
                set = b'A';
                i = 2;
            }
            b':' => i = 2,
            b';' => {
                set = b'C';
                i = 2;
            }
            _ => {}
        }
    }
    values.push(match set {
        b'A' => START_A,
        b'C' => START_C,
        _ => START_B,
    });
    let mut shift_next = false;
    while i < data.len() {
        let c = data[i];
        if c == b'>' && i + 1 < data.len() {
            let code = data[i + 1];
            i += 2;
            let (value, shown): (Option<u8>, Option<char>) = match code {
                b'<' => (Some(62), Some('^')),
                b'0' => (Some(30), Some('>')),
                b'=' => (Some(94), Some('~')),
                b'1' => (Some(95), None),
                b'2' => (Some(96), None),
                b'3' => (Some(97), None),
                b'4' => {
                    shift_next = set != b'C';
                    (Some(SHIFT), None)
                }
                b'5' => {
                    set = b'C';
                    (Some(CODE_C), None)
                }
                b'6' => {
                    if set != b'B' {
                        set = b'B';
                    }
                    (Some(CODE_B), None)
                }
                b'7' => {
                    if set != b'A' {
                        set = b'A';
                    }
                    (Some(CODE_A), None)
                }
                b'8' => (Some(FNC1), None),
                // Start codes are only valid at the beginning.
                b'9' | b':' | b';' => (None, None),
                _ => {
                    // Not an invocation code: '>' is data.
                    i -= 1;
                    (value_in_b(b'>'), Some('>'))
                }
            };
            if let Some(v) = value {
                if set == b'C' && matches!(code, b'<' | b'0' | b'=') {
                    text.push_str(&format!("{v:02}"));
                } else if let Some(s) = shown {
                    text.push(s);
                }
                values.push(v);
            }
            continue;
        }
        let effective = if shift_next {
            shift_next = false;
            if set == b'A' { b'B' } else { b'A' }
        } else {
            set
        };
        match effective {
            b'C' => {
                if c.is_ascii_digit() && i + 1 < data.len() && data[i + 1].is_ascii_digit() {
                    values.push((c - b'0') * 10 + (data[i + 1] - b'0'));
                    text.push(c as char);
                    text.push(data[i + 1] as char);
                    i += 2;
                } else {
                    // Odd digits and other characters are dropped in subset C.
                    i += 1;
                }
            }
            b'A' => {
                if let Some(v) = value_in_a(c) {
                    values.push(v);
                    text.push(c as char);
                }
                i += 1;
            }
            _ => {
                if let Some(v) = value_in_b(c) {
                    values.push(v);
                    text.push(c as char);
                }
                i += 1;
            }
        }
    }
    (values, text)
}

fn digit_run(syms: &[Sym], from: usize) -> usize {
    syms[from..]
        .iter()
        .take_while(|s| matches!(s, Sym::Char(c) if c.is_ascii_digit()))
        .count()
}

/// Automatic subset selection (modes A, D and U).
fn code128_auto(syms: &[Sym]) -> Vec<u8> {
    let mut values = Vec::new();
    let all_digits_even =
        !syms.is_empty() && digit_run(syms, 0) == syms.len() && syms.len().is_multiple_of(2);
    let lead_fnc1 = matches!(syms.first(), Some(Sym::Fnc1));
    let first_run = digit_run(syms, if lead_fnc1 { 1 } else { 0 });
    let mut i = 0;
    let mut set;
    if all_digits_even || first_run >= 4 || (lead_fnc1 && first_run >= 2) {
        set = b'C';
        values.push(START_C);
    } else if matches!(syms.first(), Some(Sym::Char(c)) if *c < 32) {
        set = b'A';
        values.push(START_A);
    } else {
        set = b'B';
        values.push(START_B);
    }
    while i < syms.len() {
        match syms[i] {
            Sym::Fnc1 => {
                // Like ZXing, switch to subset C when digits follow FNC1.
                if set != b'C' && digit_run(syms, i + 1) >= 2 {
                    set = b'C';
                    values.push(CODE_C);
                }
                values.push(FNC1);
                i += 1;
            }
            Sym::Char(c) => {
                let run = digit_run(syms, i);
                if set == b'C' {
                    if run >= 2 {
                        let a = match syms[i] {
                            Sym::Char(a) => a,
                            Sym::Fnc1 => b'0',
                        };
                        let b = match syms[i + 1] {
                            Sym::Char(b) => b,
                            Sym::Fnc1 => b'0',
                        };
                        values.push((a - b'0') * 10 + (b - b'0'));
                        i += 2;
                        continue;
                    }
                    // Leave subset C.
                    if c < 32 {
                        set = b'A';
                        values.push(CODE_A);
                    } else {
                        set = b'B';
                        values.push(CODE_B);
                    }
                    continue;
                }
                if run >= 4 {
                    if run % 2 == 1 {
                        // Odd run: first digit in the current subset.
                        values.push(c - 32);
                        i += 1;
                    }
                    set = b'C';
                    values.push(CODE_C);
                    continue;
                }
                let in_current = if set == b'A' {
                    value_in_a(c)
                } else {
                    value_in_b(c)
                };
                if let Some(v) = in_current {
                    values.push(v);
                    i += 1;
                    continue;
                }
                // Character needs the other subset: shift for a single one.
                let other_ok = |s: &Sym| match s {
                    Sym::Char(x) => {
                        if set == b'A' {
                            value_in_a(*x).is_none() && value_in_b(*x).is_some()
                        } else {
                            value_in_b(*x).is_none() && value_in_a(*x).is_some()
                        }
                    }
                    Sym::Fnc1 => false,
                };
                let next_needs_other = syms.get(i + 1).is_some_and(other_ok);
                let other = if set == b'A' {
                    value_in_b(c)
                } else {
                    value_in_a(c)
                };
                match other {
                    Some(v) if !next_needs_other => {
                        values.push(SHIFT);
                        values.push(v);
                    }
                    Some(v) => {
                        if set == b'A' {
                            set = b'B';
                            values.push(CODE_B);
                        } else {
                            set = b'A';
                            values.push(CODE_A);
                        }
                        values.push(v);
                    }
                    None => {}
                }
                i += 1;
            }
        }
    }
    values
}

/// GS1 application identifiers whose data has a fixed length that includes
/// a check digit Zebra recalculates.
fn gs1_check_digit_ai(ai: &str) -> Option<usize> {
    match ai {
        "00" => Some(18),
        "01" | "02" => Some(14),
        _ => None,
    }
}

/// Mode D: strips parentheses and spaces, recalculates check digits of
/// fixed GTIN/SSCC application identifiers, keeps `>8` as FNC1.
fn code128_ean(data: &[u8]) -> (Vec<Sym>, String) {
    let mut syms = vec![Sym::Fnc1];
    let mut text = String::new();
    let s = String::from_utf8_lossy(data).into_owned();
    // Split into (ai, value) chunks when parentheses are used.
    let mut chunks: Vec<(Option<String>, String)> = Vec::new();
    if s.contains('(') {
        let mut rest = s.as_str();
        while let Some(open) = rest.find('(') {
            if open > 0 {
                chunks.push((None, rest[..open].to_string()));
            }
            let Some(close) = rest[open..].find(')') else {
                chunks.push((None, rest[open + 1..].to_string()));
                rest = "";
                break;
            };
            let ai = rest[open + 1..open + close].to_string();
            let after = &rest[open + close + 1..];
            let next = after.find('(').unwrap_or(after.len());
            chunks.push((Some(ai), after[..next].to_string()));
            rest = &after[next..];
        }
        if !rest.is_empty() {
            chunks.push((None, rest.to_string()));
        }
    } else {
        chunks.push((None, s.clone()));
    }
    for (ai, value) in chunks {
        let mut value = value;
        if let Some(ai) = &ai {
            if let Some(len) = gs1_check_digit_ai(ai.trim()) {
                let digits: String = value.chars().filter(|c| c.is_ascii_digit()).collect();
                if digits.len() >= len - 1 {
                    let body = &digits[..len - 1];
                    let check = mod10_check(body.as_bytes()) as char;
                    value = format!("{body}{check}{}", &digits[len.min(digits.len())..]);
                }
            }
            text.push('(');
            text.push_str(ai);
            text.push(')');
            push_ean_chars(ai.as_bytes(), &mut syms);
        }
        let bytes = value.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'>' && bytes.get(i + 1) == Some(&b'8') {
                syms.push(Sym::Fnc1);
                i += 2;
                continue;
            }
            text.push(bytes[i] as char);
            if bytes[i] != b' ' {
                syms.push(Sym::Char(bytes[i]));
            }
            i += 1;
        }
    }
    (syms, text)
}

fn push_ean_chars(bytes: &[u8], syms: &mut Vec<Sym>) {
    syms.extend(bytes.iter().filter(|b| **b != b' ').map(|&b| Sym::Char(b)));
}

/// Encodes Code 128 (`^BC`). `data` are field bytes after `^FH` and `^CI`
/// processing (characters above 127 are dropped).
pub fn code128(data: &[u8], mode: Code128Mode, module: f64) -> Encoded {
    let (values, text) = match mode {
        Code128Mode::None => code128_manual(data),
        Code128Mode::Auto => {
            let syms: Vec<Sym> = data
                .iter()
                .filter(|b| **b < 128)
                .map(|&b| Sym::Char(b))
                .collect();
            let text: String = data
                .iter()
                .filter(|b| **b < 128)
                .map(|&b| b as char)
                .collect();
            (code128_auto(&syms), text)
        }
        Code128Mode::Ean => {
            let (syms, text) = code128_ean(data);
            let syms: Vec<Sym> = syms
                .into_iter()
                .filter(|s| !matches!(s, Sym::Char(c) if *c >= 128))
                .collect();
            (code128_auto(&syms), text)
        }
        Code128Mode::Ucc => {
            let mut digits: Vec<u8> = data.iter().copied().filter(u8::is_ascii_digit).collect();
            let text_digits = digits.clone();
            digits.truncate(19);
            while digits.len() < 19 {
                digits.insert(0, b'0');
            }
            let check = mod10_check(&digits);
            digits.push(check);
            let mut syms = vec![Sym::Fnc1];
            syms.extend(digits.iter().map(|&d| Sym::Char(d)));
            let mut text: String = text_digits.iter().map(|&d| d as char).collect();
            text.push(check as char);
            (code128_auto(&syms), text)
        }
    };
    Encoded {
        elements: finish_code128(&values, module),
        text,
    }
}

/// Code 39 alphabet and patterns (9 elements, bit set = wide, MSB first).
const C39_ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-. $/+%";
const C39_PATTERNS: [u16; 43] = [
    0x034, 0x121, 0x061, 0x160, 0x031, 0x130, 0x070, 0x025, 0x124, 0x064, 0x109, 0x049, 0x148,
    0x019, 0x118, 0x058, 0x00D, 0x10C, 0x04C, 0x01C, 0x103, 0x043, 0x142, 0x013, 0x112, 0x052,
    0x007, 0x106, 0x046, 0x016, 0x181, 0x0C1, 0x1C0, 0x091, 0x190, 0x0D0, 0x085, 0x184, 0x0C4,
    0x0A8, 0x0A2, 0x08A, 0x02A,
];
const C39_ASTERISK: u16 = 0x094;

fn wide_narrow(pattern: u16, count: u32, narrow: f64, wide: f64, out: &mut Vec<f64>) {
    for i in (0..count).rev() {
        out.push(if pattern & (1 << i) != 0 {
            wide
        } else {
            narrow
        });
    }
}

/// Encodes Code 39 (`^B3`, `^BL`). `check` adds the mod 43 check character.
pub fn code39(data: &[u8], check: bool, narrow: f64, wide: f64) -> Encoded {
    let chars: Vec<u8> = data
        .iter()
        .map(|b| b.to_ascii_uppercase())
        .filter(|b| C39_ALPHABET.contains(b))
        .collect();
    let mut values: Vec<usize> = chars
        .iter()
        .map(|b| C39_ALPHABET.iter().position(|x| x == b).unwrap_or(0))
        .collect();
    if check {
        let sum: usize = values.iter().sum();
        values.push(sum % 43);
    }
    let mut elements = Vec::new();
    let mut push_char = |p: u16, last: bool| {
        wide_narrow(p, 9, narrow, wide, &mut elements);
        if !last {
            elements.push(narrow);
        }
    };
    push_char(C39_ASTERISK, false);
    for v in &values {
        push_char(C39_PATTERNS[*v], false);
    }
    push_char(C39_ASTERISK, true);
    let mut text = String::from("*");
    text.extend(values.iter().map(|&v| C39_ALPHABET[v] as char));
    text.push('*');
    Encoded { elements, text }
}

/// Code 93 alphabet; `a b c d` are the shift characters `($) (%) (/) (+)`.
const C93_ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-. $/+%abcd*";
const C93_PATTERNS: [u16; 48] = [
    0x114, 0x148, 0x144, 0x142, 0x128, 0x124, 0x122, 0x150, 0x112, 0x10A, 0x1A8, 0x1A4, 0x1A2,
    0x194, 0x192, 0x18A, 0x168, 0x164, 0x162, 0x134, 0x11A, 0x158, 0x14C, 0x146, 0x12C, 0x116,
    0x1B4, 0x1B2, 0x1AC, 0x1A6, 0x196, 0x19A, 0x16C, 0x166, 0x136, 0x13A, 0x12E, 0x1D4, 0x1D2,
    0x1CA, 0x16E, 0x176, 0x1AE, 0x126, 0x1DA, 0x1D6, 0x132, 0x15E,
];

fn code93_full_ascii(c: u8) -> Vec<u8> {
    let simple = |x: u8| vec![x];
    match c {
        b'0'..=b'9' | b'A'..=b'Z' | b'-' | b'.' | b' ' | b'$' | b'/' | b'+' | b'%' => simple(c),
        0 => vec![b'b', b'U'],
        1..=26 => vec![b'a', b'A' + c - 1],
        27..=31 => vec![b'b', b'A' + c - 27],
        b'!'..=b',' | b':' => vec![b'c', b'A' + c - b'!'],
        b';'..=b'?' => vec![b'b', b'F' + c - b';'],
        b'@' => vec![b'b', b'V'],
        b'['..=b'_' => vec![b'b', b'K' + c - b'['],
        b'`' => vec![b'b', b'W'],
        b'a'..=b'z' => vec![b'd', b'A' + c - b'a'],
        b'{'..=b'~' => vec![b'b', b'P' + c - b'{'],
        127 => vec![b'b', b'T'],
        _ => vec![],
    }
}

/// Encodes Code 93 (`^BA`) with full ASCII and both check characters.
pub fn code93(data: &[u8], module: f64) -> Encoded {
    let mut values: Vec<usize> = Vec::new();
    let mut text = String::new();
    for &c in data {
        let mapped = code93_full_ascii(c);
        if !mapped.is_empty() {
            text.push(c as char);
        }
        for m in mapped {
            values.push(C93_ALPHABET.iter().position(|&x| x == m).unwrap_or(0));
        }
    }
    let check = |vals: &[usize], max_weight: usize| -> usize {
        let mut weight = 1;
        let mut total = 0;
        for v in vals.iter().rev() {
            total += v * weight;
            weight += 1;
            if weight > max_weight {
                weight = 1;
            }
        }
        total % 47
    };
    let c = check(&values, 20);
    values.push(c);
    let k = check(&values, 15);
    values.push(k);
    let mut modules: Vec<bool> = Vec::new();
    let mut push = |p: u16| {
        for i in (0..9).rev() {
            modules.push(p & (1 << i) != 0);
        }
    };
    push(C93_PATTERNS[47]);
    for v in &values {
        push(C93_PATTERNS[*v]);
    }
    push(C93_PATTERNS[47]);
    modules.push(true);
    // Zebra shows the start and stop characters as small boxes.
    Encoded {
        elements: runs(&modules, module),
        text: format!("\u{25AF}{text}\u{25AF}"),
    }
}

/// Converts a module sequence (true = bar) into element widths.
pub fn runs(modules: &[bool], module: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut current = true;
    let mut count = 0usize;
    for &m in modules {
        if m == current {
            count += 1;
        } else {
            out.push(count as f64 * module);
            current = m;
            count = 1;
        }
    }
    out.push(count as f64 * module);
    out
}

const I25_PATTERNS: [u8; 10] = [
    0b00110, 0b10001, 0b01001, 0b11000, 0b00101, 0b10100, 0b01100, 0b00011, 0b10010, 0b01010,
];

/// Encodes Interleaved 2 of 5 (`^B2`). `check` appends a mod 10 digit.
pub fn interleaved_2of5(data: &[u8], check: bool, narrow: f64, wide: f64) -> Encoded {
    let mut digits: Vec<u8> = data.iter().copied().filter(u8::is_ascii_digit).collect();
    if check {
        let c = mod10_check(&digits);
        digits.push(c);
    }
    if digits.len() % 2 == 1 {
        digits.insert(0, b'0');
    }
    let mut elements = vec![narrow, narrow, narrow, narrow];
    for pair in digits.chunks(2) {
        let a = I25_PATTERNS[(pair[0] - b'0') as usize];
        let b = I25_PATTERNS[(pair[1] - b'0') as usize];
        for i in (0..5).rev() {
            elements.push(if a & (1 << i) != 0 { wide } else { narrow });
            elements.push(if b & (1 << i) != 0 { wide } else { narrow });
        }
    }
    elements.extend([wide, narrow, narrow]);
    Encoded {
        elements,
        text: digits.iter().map(|&d| d as char).collect(),
    }
}

/// Encodes Standard 2 of 5 (`^BJ`) and Industrial 2 of 5 (`^BI`).
pub fn standard_2of5(data: &[u8], narrow: f64, wide: f64) -> Encoded {
    let digits: Vec<u8> = data.iter().copied().filter(u8::is_ascii_digit).collect();
    let mut elements = Vec::new();
    // Both variants encode information in the bars only; spaces are narrow.
    let push_bars = |pattern: &[bool], elements: &mut Vec<f64>| {
        for (i, w) in pattern.iter().enumerate() {
            elements.push(if *w { wide } else { narrow });
            if i + 1 < pattern.len() {
                elements.push(narrow);
            }
        }
    };
    push_bars(&[true, true, false], &mut elements);
    for d in &digits {
        elements.push(narrow);
        let p = I25_PATTERNS[(d - b'0') as usize];
        let bits: Vec<bool> = (0..5).rev().map(|i| p & (1 << i) != 0).collect();
        push_bars(&bits, &mut elements);
    }
    elements.push(narrow);
    push_bars(&[true, false, true], &mut elements);
    Encoded {
        elements,
        text: digits.iter().map(|&d| d as char).collect(),
    }
}

const CODABAR_ALPHABET: &[u8] = b"0123456789-$:/.+ABCD";
const CODABAR_PATTERNS: [u8; 20] = [
    0x03, 0x06, 0x09, 0x60, 0x12, 0x42, 0x21, 0x24, 0x30, 0x48, 0x0c, 0x18, 0x45, 0x51, 0x54, 0x15,
    0x1A, 0x29, 0x0B, 0x0E,
];

/// Encodes Codabar (`^BK`).
pub fn codabar(data: &[u8], start: u8, stop: u8, narrow: f64, wide: f64) -> Encoded {
    let body: Vec<u8> = data
        .iter()
        .copied()
        .filter(|b| CODABAR_ALPHABET[..16].contains(b))
        .collect();
    let start = if b"ABCD".contains(&start) {
        start
    } else {
        b'A'
    };
    let stop = if b"ABCD".contains(&stop) { stop } else { b'A' };
    let mut all = vec![start];
    all.extend(&body);
    all.push(stop);
    let mut elements = Vec::new();
    for (i, c) in all.iter().enumerate() {
        let idx = CODABAR_ALPHABET.iter().position(|x| x == c).unwrap_or(0);
        for b in (0..7).rev() {
            elements.push(if CODABAR_PATTERNS[idx] & (1 << b) != 0 {
                wide
            } else {
                narrow
            });
        }
        if i + 1 < all.len() {
            elements.push(narrow);
        }
    }
    let mut text = String::new();
    text.push(start as char);
    text.extend(body.iter().map(|&b| b as char));
    text.push(stop as char);
    Encoded { elements, text }
}

const C11_ALPHABET: &[u8] = b"0123456789-";
const C11_PATTERNS: [u8; 11] = [
    0b00001, 0b10001, 0b01001, 0b11000, 0b00101, 0b10100, 0b01100, 0b00011, 0b10010, 0b10000,
    0b00100,
];
const C11_START: u8 = 0b00110;

/// Encodes Code 11 (`^B1`). `two_check_digits` adds C and K check digits.
pub fn code11(data: &[u8], two_check_digits: bool, narrow: f64, wide: f64) -> Encoded {
    let mut values: Vec<usize> = data
        .iter()
        .filter_map(|b| C11_ALPHABET.iter().position(|x| x == b))
        .collect();
    let check = |vals: &[usize], max: usize| -> usize {
        vals.iter()
            .rev()
            .enumerate()
            .map(|(i, v)| v * ((i % max) + 1))
            .sum::<usize>()
            % 11
    };
    let c = check(&values, 10);
    values.push(c);
    if two_check_digits {
        let k = check(&values, 9);
        values.push(k);
    }
    let mut elements = Vec::new();
    let mut push = |p: u8, last: bool| {
        for b in (0..5).rev() {
            elements.push(if p & (1 << b) != 0 { wide } else { narrow });
        }
        if !last {
            elements.push(narrow);
        }
    };
    push(C11_START, false);
    for v in &values {
        push(C11_PATTERNS[*v], false);
    }
    push(C11_START, true);
    let text: String = data
        .iter()
        .filter(|b| C11_ALPHABET.contains(b))
        .map(|&b| b as char)
        .collect();
    Encoded { elements, text }
}

const EAN_L: [&str; 10] = [
    "0001101", "0011001", "0010011", "0111101", "0100011", "0110001", "0101111", "0111011",
    "0110111", "0001011",
];
const EAN_G: [&str; 10] = [
    "0100111", "0110011", "0011011", "0100001", "0011101", "0111001", "0000101", "0010001",
    "0001001", "0010111",
];
const EAN_R: [&str; 10] = [
    "1110010", "1100110", "1101100", "1000010", "1011100", "1001110", "1010000", "1000100",
    "1001000", "1110100",
];
const EAN13_PARITY: [&str; 10] = [
    "LLLLLL", "LLGLGG", "LLGGLG", "LLGGGL", "LGLLGG", "LGGLLG", "LGGGLL", "LGLGLG", "LGLGGL",
    "LGGLGL",
];
const UPCE_PARITY: [&str; 10] = [
    "GGGLLL", "GGLGLL", "GGLLGL", "GGLLLG", "GLGGLL", "GLLGGL", "GLLLGG", "GLGLGL", "GLGLLG",
    "GLLGLG",
];

/// EAN / UPC result: modules plus layout information for the
/// interpretation line.
#[derive(Debug, Clone, PartialEq)]
pub struct EanUpc {
    /// Modules, `true` = bar.
    pub modules: Vec<bool>,
    /// Module ranges `[start, end)` of bars that extend below the others.
    pub long: Vec<(usize, usize)>,
    /// Digits printed under the bars: `(digit, first module of its 7 module slot)`.
    pub inner_digits: Vec<(char, usize)>,
    /// Digit printed left of the symbol.
    pub left_digit: Option<char>,
    /// Digit printed right of the symbol.
    pub right_digit: Option<char>,
    /// Full data including the check digit.
    pub text: String,
}

fn digits_padded(data: &[u8], n: usize) -> Vec<u8> {
    let mut d: Vec<u8> = data.iter().copied().filter(u8::is_ascii_digit).collect();
    d.truncate(n);
    while d.len() < n {
        d.insert(0, b'0');
    }
    d
}

fn push_bits(modules: &mut Vec<bool>, bits: &str) {
    modules.extend(bits.bytes().map(|b| b == b'1'));
}

/// EAN-13 (`^BE`): 12 data digits, the check digit is always calculated.
pub fn ean13(data: &[u8]) -> EanUpc {
    let mut d = digits_padded(data, 12);
    d.push(mod10_check(&d));
    let parity = EAN13_PARITY[(d[0] - b'0') as usize].as_bytes();
    let mut m = Vec::with_capacity(95);
    push_bits(&mut m, "101");
    for i in 0..6 {
        let digit = (d[i + 1] - b'0') as usize;
        push_bits(
            &mut m,
            if parity[i] == b'L' {
                EAN_L[digit]
            } else {
                EAN_G[digit]
            },
        );
    }
    push_bits(&mut m, "01010");
    for i in 0..6 {
        push_bits(&mut m, EAN_R[(d[i + 7] - b'0') as usize]);
    }
    push_bits(&mut m, "101");
    let mut inner = Vec::new();
    for i in 0..6 {
        inner.push((d[i + 1] as char, 3 + 7 * i));
        inner.push((d[i + 7] as char, 50 + 7 * i));
    }
    EanUpc {
        modules: m,
        long: vec![(0, 3), (45, 50), (92, 95)],
        inner_digits: inner,
        left_digit: Some(d[0] as char),
        right_digit: None,
        text: d.iter().map(|&c| c as char).collect(),
    }
}

/// EAN-8 (`^B8`): 7 data digits plus check digit.
pub fn ean8(data: &[u8]) -> EanUpc {
    let mut d = digits_padded(data, 7);
    d.push(mod10_check(&d));
    let mut m = Vec::with_capacity(67);
    push_bits(&mut m, "101");
    for &c in &d[..4] {
        push_bits(&mut m, EAN_L[(c - b'0') as usize]);
    }
    push_bits(&mut m, "01010");
    for &c in &d[4..] {
        push_bits(&mut m, EAN_R[(c - b'0') as usize]);
    }
    push_bits(&mut m, "101");
    let mut inner = Vec::new();
    for i in 0..4 {
        inner.push((d[i] as char, 3 + 7 * i));
        inner.push((d[i + 4] as char, 36 + 7 * i));
    }
    EanUpc {
        modules: m,
        long: vec![(0, 3), (31, 36), (64, 67)],
        inner_digits: inner,
        left_digit: None,
        right_digit: None,
        text: d.iter().map(|&c| c as char).collect(),
    }
}

/// UPC-A (`^BU`): 11 data digits plus check digit.
pub fn upca(data: &[u8], show_check: bool) -> EanUpc {
    let mut d = digits_padded(data, 11);
    d.push(mod10_check(&d));
    let mut m = Vec::with_capacity(95);
    push_bits(&mut m, "101");
    for &c in &d[..6] {
        push_bits(&mut m, EAN_L[(c - b'0') as usize]);
    }
    push_bits(&mut m, "01010");
    for &c in &d[6..] {
        push_bits(&mut m, EAN_R[(c - b'0') as usize]);
    }
    push_bits(&mut m, "101");
    let mut inner = Vec::new();
    for i in 1..6 {
        inner.push((d[i] as char, 3 + 7 * i));
        inner.push((d[i + 5] as char, 50 + 7 * (i - 1)));
    }
    EanUpc {
        modules: m,
        long: vec![(0, 10), (45, 50), (85, 95)],
        inner_digits: inner,
        left_digit: Some(d[0] as char),
        right_digit: show_check.then_some(d[11] as char),
        text: d.iter().map(|&c| c as char).collect(),
    }
}

/// Expands a UPC-E body (number system + 6 digits) to the 11 digit UPC-A
/// data used for the check digit.
fn upce_to_upca(ns: u8, b: &[u8]) -> Vec<u8> {
    let mut out = vec![ns];
    match b[5] {
        b'0' | b'1' | b'2' => {
            out.extend([b[0], b[1], b[5], b'0', b'0', b'0', b'0', b[2], b[3], b[4]]);
        }
        b'3' => out.extend([b[0], b[1], b[2], b'0', b'0', b'0', b'0', b'0', b[3], b[4]]),
        b'4' => out.extend([b[0], b[1], b[2], b[3], b'0', b'0', b'0', b'0', b'0', b[4]]),
        _ => out.extend([b[0], b[1], b[2], b[3], b[4], b'0', b'0', b'0', b'0', b[5]]),
    }
    out
}

/// UPC-E (`^B9`): number system digit and 6 digits.
pub fn upce(data: &[u8], show_check: bool) -> EanUpc {
    let digits: Vec<u8> = data.iter().copied().filter(u8::is_ascii_digit).collect();
    let (ns, body) = if digits.len() >= 7 {
        (
            if digits[0] == b'1' { b'1' } else { b'0' },
            digits[1..7].to_vec(),
        )
    } else {
        (b'0', digits_padded(&digits, 6))
    };
    let check = mod10_check(&upce_to_upca(ns, &body));
    let mut parity = UPCE_PARITY[(check - b'0') as usize].as_bytes().to_vec();
    if ns == b'1' {
        for p in parity.iter_mut() {
            *p = if *p == b'L' { b'G' } else { b'L' };
        }
    }
    let mut m = Vec::with_capacity(51);
    push_bits(&mut m, "101");
    for (i, &c) in body.iter().enumerate() {
        let digit = (c - b'0') as usize;
        push_bits(
            &mut m,
            if parity[i] == b'L' {
                EAN_L[digit]
            } else {
                EAN_G[digit]
            },
        );
    }
    push_bits(&mut m, "010101");
    let inner = body
        .iter()
        .enumerate()
        .map(|(i, &c)| (c as char, 3 + 7 * i))
        .collect();
    let mut text = String::new();
    text.push(ns as char);
    text.extend(body.iter().map(|&c| c as char));
    text.push(check as char);
    EanUpc {
        modules: m,
        long: vec![(0, 3), (45, 51)],
        inner_digits: inner,
        left_digit: Some(ns as char),
        right_digit: show_check.then_some(check as char),
        text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(data: &str, mode: Code128Mode) -> Vec<u8> {
        let enc = code128(data.as_bytes(), mode, 1.0);
        // Decode element widths back into values.
        let w: Vec<u8> = enc.elements.iter().map(|&x| x as u8).collect();
        let mut out = Vec::new();
        for chunk in w.chunks(6) {
            if chunk.len() < 6 {
                break;
            }
            let key: String = chunk.iter().map(|d| (b'0' + d) as char).collect();
            if key == "233111" {
                break;
            }
            out.push(C128_PATTERNS.iter().position(|p| *p == key).unwrap() as u8);
        }
        out.pop(); // check symbol
        out
    }

    #[test]
    fn code128_matches_labelary() {
        use Code128Mode::*;
        let cases: &[(&str, Code128Mode, &[u8])] = &[
            ("12345678", None, &[104, 17, 18, 19, 20, 21, 22, 23, 24]),
            (">:A><B>0C>=D", None, &[104, 33, 62, 34, 30, 35, 94, 36]),
            (">;1234>6AB", None, &[105, 12, 34, 100, 33, 34]),
            (">;12345", None, &[105, 12, 34]),
            ("12>4a", None, &[104, 17, 18, 98]),
            ("12345678", Auto, &[105, 12, 34, 56, 78]),
            ("ABC12345678", Auto, &[104, 33, 34, 35, 99, 12, 34, 56, 78]),
            ("12345", Auto, &[105, 12, 34, 100, 21]),
            ("123", Auto, &[104, 17, 18, 19]),
            ("12", Auto, &[105, 12]),
            ("AB12345", Auto, &[104, 33, 34, 17, 99, 23, 45]),
            ("12345AB", Auto, &[105, 12, 34, 100, 21, 33, 34]),
            ("A12345B", Auto, &[104, 33, 17, 99, 23, 45, 100, 34]),
            ("A123B", Auto, &[104, 33, 17, 18, 19, 34]),
            ("ab\tc", Auto, &[104, 65, 66, 98, 73, 67]),
            (">8123456", Auto, &[104, 30, 24, 99, 12, 34, 56]),
            ("123", Ucc, &[105, 102, 0, 0, 0, 0, 0, 0, 0, 0, 12, 36]),
            (
                "(00)123456789012345678",
                Ean,
                &[105, 102, 0, 12, 34, 56, 78, 90, 12, 34, 56, 75],
            ),
            (
                "(21)ABC(01)12345678901231",
                Ean,
                &[
                    105, 102, 21, 100, 33, 34, 35, 99, 1, 12, 34, 56, 78, 90, 12, 31,
                ],
            ),
            (
                "(01)12345678901234(10)AB>8(21)X",
                Ean,
                &[
                    105, 102, 1, 12, 34, 56, 78, 90, 12, 31, 10, 100, 33, 34, 99, 102, 21, 100, 56,
                ],
            ),
        ];
        for (data, mode, expected) in cases {
            assert_eq!(&values(data, *mode), expected, "{data} {mode:?}");
        }
    }

    #[test]
    fn code128_text() {
        assert_eq!(
            code128(b">:A><B>0C>=D", Code128Mode::None, 1.0).text,
            "A^B>C~D"
        );
        assert_eq!(code128(b"123", Code128Mode::Ucc, 1.0).text, "1236");
        assert_eq!(
            code128(b"(01)12345678901234(10)AB>8(21)X", Code128Mode::Ean, 1.0).text,
            "(01)12345678901231(10)AB(21)X"
        );
        assert_eq!(code128(b">:AB>5123", Code128Mode::None, 1.0).text, "AB12");
    }

    #[test]
    fn code39_widths_match_zebra() {
        // ^BY2,3: 8 characters including start/stop = 254 dots.
        assert_eq!(code39(b"ABC123", false, 2.0, 6.0).width(), 254.0);
        assert_eq!(code39(b"ABC123", false, 3.0, 7.0).width(), 333.0);
        assert_eq!(code39(b"ABC123", false, 2.0, 6.0).text, "*ABC123*");
    }

    #[test]
    fn i2of5_width() {
        assert_eq!(
            interleaved_2of5(b"1234567890", false, 2.0, 6.0).width(),
            198.0
        );
    }

    #[test]
    fn ean_upc() {
        let e = ean13(b"590123412345");
        assert_eq!(e.text, "5901234123457");
        assert_eq!(e.modules.len(), 95);
        let u = upca(b"01234567890", true);
        assert_eq!(u.text, "012345678905");
        let e8 = ean8(b"1234567");
        assert_eq!(e8.text, "12345670");
        let ue = upce(b"0123456", true);
        assert_eq!(ue.text, "01234565");
        assert_eq!(ue.modules.len(), 51);
    }
}
