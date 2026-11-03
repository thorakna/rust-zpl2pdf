//! ZPL tokenizer and command parser.

pub mod command;
pub mod lexer;

pub use command::{BarcodeCommand, BarcodeKind, BarcodeParams, Command, object_name, parse};
pub use lexer::{Lexer, Prefix, RawCommand};

/// Tokenizes and parses a whole ZPL stream.
pub fn parse_all(input: &[u8]) -> Vec<(RawCommand, Command)> {
    Lexer::new(input)
        .map(|raw| {
            let cmd = parse(&raw);
            (raw, cmd)
        })
        .collect()
}
