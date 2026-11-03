//! Bar code encoders.
//!
//! Encoders produce symbology level output (element widths, module grids);
//! the interpreter turns them into scene items at the field position.

pub mod linear;
pub mod matrix;
pub mod maxicode;
mod maxicode_grid;
