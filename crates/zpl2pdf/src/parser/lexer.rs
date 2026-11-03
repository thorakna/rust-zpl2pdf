//! Splits a ZPL byte stream into raw commands.
//!
//! ZPL is not line based. A command starts with the format prefix (`^` by
//! default) or the control prefix (`~` by default) followed by a two letter
//! code. Its parameters run until the next prefix character. Both prefixes and
//! the parameter delimiter can be changed in the stream (`^CC`, `^CT`, `^CD`),
//! so the lexer has to track them while it reads.

/// Which prefix introduced a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prefix {
    /// Format command, `^` by default.
    Format,
    /// Control command, `~` by default.
    Control,
}

/// A command as it appears in the stream, before parameter parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawCommand {
    /// The prefix kind.
    pub prefix: Prefix,
    /// Upper case command code without the prefix, such as `FO` or `GF`.
    /// For `^A` the code is `A` and the font name is in [`RawCommand::font`].
    pub code: String,
    /// Font name character of an `^A` command.
    pub font: Option<u8>,
    /// Parameter bytes. Carriage returns and line feeds are removed.
    pub args: Vec<u8>,
    /// Payload of commands that carry data after their parameters
    /// (`^GF`, `~DG`, `~DY`). Binary payloads are kept verbatim.
    pub data: Option<Vec<u8>>,
    /// Parameter delimiter in effect for this command.
    pub delimiter: u8,
    /// Byte offset of the prefix in the input.
    pub offset: usize,
}

impl RawCommand {
    /// The command as written with its default prefix, for messages.
    pub fn name(&self) -> String {
        let p = match self.prefix {
            Prefix::Format => '^',
            Prefix::Control => '~',
        };
        match self.font {
            Some(f) => format!("{p}{}{}", self.code, f as char),
            None => format!("{p}{}", self.code),
        }
    }
}

/// Streaming tokenizer over ZPL bytes.
pub struct Lexer<'a> {
    input: &'a [u8],
    pos: usize,
    format_prefix: u8,
    control_prefix: u8,
    delimiter: u8,
}

impl<'a> Lexer<'a> {
    /// Creates a lexer with the default prefixes `^`, `~` and delimiter `,`.
    pub fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            pos: 0,
            format_prefix: b'^',
            control_prefix: b'~',
            delimiter: b',',
        }
    }

    fn is_prefix(&self, b: u8) -> bool {
        b == self.format_prefix || b == self.control_prefix
    }

    /// Next byte that belongs to a command code, skipping line breaks.
    /// Does not consume a prefix character.
    fn code_byte(&mut self) -> Option<u8> {
        while self.pos < self.input.len() {
            let b = self.input[self.pos];
            if b == b'\r' || b == b'\n' {
                self.pos += 1;
                continue;
            }
            if self.is_prefix(b) {
                return None;
            }
            self.pos += 1;
            return Some(b);
        }
        None
    }

    /// Reads bytes until the next prefix, dropping CR and LF.
    fn until_prefix(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        while self.pos < self.input.len() {
            let b = self.input[self.pos];
            if self.is_prefix(b) {
                break;
            }
            if b != b'\r' && b != b'\n' {
                out.push(b);
            }
            self.pos += 1;
        }
        out
    }

    /// Reads parameter bytes until `count` delimiters have been seen, a prefix
    /// is found, or the input ends. The final delimiter is consumed but not
    /// returned. The flag tells whether all delimiters were found.
    fn header(&mut self, count: usize) -> (Vec<u8>, bool) {
        let mut out = Vec::new();
        let mut seen = 0;
        while self.pos < self.input.len() {
            let b = self.input[self.pos];
            if self.is_prefix(b) {
                return (out, false);
            }
            self.pos += 1;
            if b == b'\r' || b == b'\n' {
                continue;
            }
            if b == self.delimiter {
                seen += 1;
                if seen == count {
                    return (out, true);
                }
            }
            out.push(b);
        }
        (out, false)
    }

    /// Reads exactly `n` raw bytes (or what is left).
    fn exact(&mut self, n: usize) -> Vec<u8> {
        let end = self.pos.saturating_add(n).min(self.input.len());
        let out = self.input[self.pos..end].to_vec();
        self.pos = end;
        out
    }

    fn field(&self, header: &[u8], idx: usize) -> Option<String> {
        header
            .split(|&b| b == self.delimiter)
            .nth(idx)
            .map(|s| String::from_utf8_lossy(s).trim().to_string())
    }

    /// Reads the payload after a header: binary formats have an exact byte
    /// count, everything else runs until the next prefix.
    fn payload(&mut self, binary_len: Option<usize>) -> Vec<u8> {
        match binary_len {
            Some(n) => {
                let d = self.exact(n);
                self.until_prefix();
                d
            }
            None => self.until_prefix(),
        }
    }
}

impl Iterator for Lexer<'_> {
    type Item = RawCommand;

    fn next(&mut self) -> Option<RawCommand> {
        // Skip anything that is not a prefix.
        while self.pos < self.input.len() && !self.is_prefix(self.input[self.pos]) {
            self.pos += 1;
        }
        if self.pos >= self.input.len() {
            return None;
        }
        let offset = self.pos;
        let prefix = if self.input[self.pos] == self.format_prefix {
            Prefix::Format
        } else {
            Prefix::Control
        };
        self.pos += 1;
        let mut cmd = RawCommand {
            prefix,
            code: String::new(),
            font: None,
            args: Vec::new(),
            data: None,
            delimiter: self.delimiter,
            offset,
        };

        let Some(c1) = self.code_byte() else {
            return Some(cmd);
        };
        let c1 = c1.to_ascii_uppercase();
        if prefix == Prefix::Format && c1 == b'A' {
            cmd.code.push('A');
            cmd.font = self.code_byte().map(|f| f.to_ascii_uppercase());
            cmd.args = self.until_prefix();
            return Some(cmd);
        }
        let Some(c2) = self.code_byte() else {
            cmd.code.push(c1 as char);
            return Some(cmd);
        };
        let c2 = c2.to_ascii_uppercase();
        cmd.code.push(c1 as char);
        cmd.code.push(c2 as char);

        match (c1, c2) {
            (b'C', b'C') | (b'C', b'T') | (b'C', b'D') => {
                // The new character follows immediately and may be anything,
                // including the current prefix.
                if self.pos < self.input.len() {
                    let ch = self.input[self.pos];
                    self.pos += 1;
                    cmd.args = vec![ch];
                    match c2 {
                        b'C' => self.format_prefix = ch,
                        b'T' => self.control_prefix = ch,
                        _ => self.delimiter = ch,
                    }
                }
                self.until_prefix();
            }
            (b'G', b'F') if prefix == Prefix::Format => {
                let (header, complete) = self.header(4);
                let format = self
                    .field(&header, 0)
                    .unwrap_or_default()
                    .to_ascii_uppercase();
                let binary = (format == "B" || format == "C").then(|| {
                    self.field(&header, 1)
                        .and_then(|s| s.parse::<usize>().ok())
                        .unwrap_or(0)
                });
                cmd.data = Some(if complete {
                    self.payload(binary)
                } else {
                    Vec::new()
                });
                cmd.args = header;
            }
            (b'D', b'G') if prefix == Prefix::Control => {
                let (header, complete) = self.header(3);
                cmd.data = Some(if complete {
                    self.until_prefix()
                } else {
                    Vec::new()
                });
                cmd.args = header;
            }
            (b'D', b'Y') if prefix == Prefix::Control => {
                let (header, complete) = self.header(5);
                let format = self
                    .field(&header, 1)
                    .unwrap_or_default()
                    .to_ascii_uppercase();
                let binary = (format == "B" || format == "C").then(|| {
                    self.field(&header, 3)
                        .and_then(|s| s.parse::<usize>().ok())
                        .unwrap_or(0)
                });
                cmd.data = Some(if complete {
                    self.payload(binary)
                } else {
                    Vec::new()
                });
                cmd.args = header;
            }
            _ => {
                cmd.args = self.until_prefix();
            }
        }
        Some(cmd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(s: &str) -> Vec<RawCommand> {
        Lexer::new(s.as_bytes()).collect()
    }

    #[test]
    fn basic_commands() {
        let c = lex("^XA\r\n^FO50,60^A0N,30,40^FDHello, World^FS\n^XZ");
        let codes: Vec<_> = c.iter().map(|c| c.code.as_str()).collect();
        assert_eq!(codes, ["XA", "FO", "A", "FD", "FS", "XZ"]);
        assert_eq!(c[1].args, b"50,60");
        assert_eq!(c[2].font, Some(b'0'));
        assert_eq!(c[2].args, b"N,30,40");
        assert_eq!(c[3].args, b"Hello, World");
    }

    #[test]
    fn lowercase_and_garbage() {
        let c = lex("junk ^xa^fo1,2^fs^xz trailing");
        let codes: Vec<_> = c.iter().map(|c| c.code.as_str()).collect();
        assert_eq!(codes, ["XA", "FO", "FS", "XZ"]);
    }

    #[test]
    fn change_prefixes() {
        let c = lex("^XA^CC+\n+FO1,2+FDa^b+FS+CC^^FS^XZ");
        let codes: Vec<_> = c.iter().map(|c| c.code.as_str()).collect();
        assert_eq!(codes, ["XA", "CC", "FO", "FD", "FS", "CC", "FS", "XZ"]);
        assert_eq!(c[3].args, b"a^b");
    }

    #[test]
    fn change_delimiter() {
        let c = lex("^XA^CD;^FO1;2^FS^XZ");
        assert_eq!(c[2].delimiter, b';');
        assert_eq!(c[2].args, b"1;2");
    }

    #[test]
    fn graphic_field_ascii() {
        let c = lex("^XA^FO0,0^GFA,4,4,2,\nFFFF\n0000^FS^XZ");
        let gf = &c[2];
        assert_eq!(gf.code, "GF");
        assert_eq!(gf.args, b"A,4,4,2");
        assert_eq!(gf.data.as_deref(), Some(&b"FFFF0000"[..]));
    }

    #[test]
    fn graphic_field_binary_may_contain_prefix() {
        let mut input = b"^XA^GFB,3,3,1,".to_vec();
        input.extend([b'^', b'~', 0x00]);
        input.extend(b"^FS^XZ");
        let c: Vec<_> = Lexer::new(&input).collect();
        assert_eq!(c[1].data.as_deref(), Some(&[b'^', b'~', 0][..]));
        assert_eq!(c[2].code, "FS");
    }

    #[test]
    fn download_graphic() {
        let c = lex("~DGR:LOGO.GRF,4,2,\nFF00\nFF00\n^XA^XZ");
        assert_eq!(c[0].code, "DG");
        assert_eq!(c[0].prefix, Prefix::Control);
        assert_eq!(c[0].args, b"R:LOGO.GRF,4,2");
        assert_eq!(c[0].data.as_deref(), Some(&b"FF00FF00"[..]));
    }

    #[test]
    fn truncated_input_does_not_panic() {
        for s in [
            "^", "^A", "^G", "^GF", "^GFA,1", "~DY", "^CC", "^XA^FD", "~DGR:X,1",
        ] {
            let _ = lex(s);
        }
    }
}
