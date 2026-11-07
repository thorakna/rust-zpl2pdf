//! Native ZPL (Zebra Programming Language) to PDF and PNG converter.
//!
//! ```no_run
//! let zpl = b"^XA^FO50,50^A0N,40,40^FDHello^FS^BY2^FO50,120^BCN,80^FD12345678^FS^XZ";
//! let out = zpl2pdf::zpl_to_pdf(zpl, &zpl2pdf::Options::default()).unwrap();
//! std::fs::write("label.pdf", out.pdf).unwrap();
//! ```
//!
//! The conversion runs in three steps: the [`parser`] turns bytes into
//! commands, the [`interp`] executes them and builds a backend independent
//! [`scene`] per label, and the [`pdf`] (or [`raster`]) backend writes the
//! output.

pub mod barcode;
pub mod encoding;
mod error;
pub mod font;
pub mod graphic;
pub mod interp;
pub mod parser;
pub mod pdf;
#[cfg(feature = "raster")]
pub mod raster;
pub mod scene;
pub mod types;

pub use error::Error;
pub use scene::Label;
pub use types::Rotation;

/// Resource limits, mainly for untrusted input.
#[derive(Debug, Clone)]
pub struct Limits {
    /// Maximum number of labels (pages) produced.
    pub max_labels: usize,
    /// Maximum decoded size of a single graphic in bytes.
    pub max_graphic_bytes: usize,
    /// Maximum input size in bytes.
    pub max_input_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_labels: 1000,
            max_graphic_bytes: 16 << 20,
            max_input_bytes: 64 << 20,
        }
    }
}

/// Conversion options.
#[derive(Debug, Clone)]
pub struct Options {
    /// Printer resolution in dots per millimeter: 6, 8, 12 or 24.
    pub dpmm: u8,
    /// Label width in inches.
    pub width: f64,
    /// Label height in inches.
    pub height: f64,
    /// Only render this label (zero based); all labels when `None`.
    pub label_index: Option<usize>,
    /// Repeat labels according to `^PQ`.
    pub honor_quantity: bool,
    /// Clockwise rotation of the output pages (Labelary's `X-Rotation`).
    pub rotation: Rotation,
    /// Resource limits.
    pub limits: Limits,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            dpmm: 8,
            width: 4.0,
            height: 6.0,
            label_index: None,
            honor_quantity: false,
            rotation: Rotation::R0,
            limits: Limits::default(),
        }
    }
}

impl Options {
    /// Checks that the options are usable.
    pub fn validate(&self) -> Result<(), Error> {
        if !matches!(self.dpmm, 6 | 8 | 12 | 24) {
            return Err(Error::InvalidOptions(format!(
                "dpmm must be 6, 8, 12 or 24, got {}",
                self.dpmm
            )));
        }
        for (name, v) in [("width", self.width), ("height", self.height)] {
            if !(v.is_finite() && v > 0.0 && v <= 15.0) {
                return Err(Error::InvalidOptions(format!(
                    "{name} must be between 0 and 15 inches, got {v}"
                )));
            }
        }
        Ok(())
    }
}

/// A non fatal problem found while converting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// Index of the label (in input order) the problem belongs to.
    pub label: Option<usize>,
    /// Byte offset of the command in the input.
    pub offset: usize,
    /// The command, such as `^BC`.
    pub command: String,
    /// Description.
    pub message: String,
}

impl std::fmt::Display for Warning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.label {
            Some(l) => write!(
                f,
                "label {} at byte {}: {}: {}",
                l + 1,
                self.offset,
                self.command,
                self.message
            ),
            None => write!(
                f,
                "byte {}: {}: {}",
                self.offset, self.command, self.message
            ),
        }
    }
}

/// Rendered labels plus warnings.
#[derive(Debug, Clone)]
pub struct Rendered {
    /// Labels in output order.
    pub labels: Vec<Label>,
    /// Warnings.
    pub warnings: Vec<Warning>,
}

/// Parses and interprets ZPL into scene labels.
pub fn render(zpl: &[u8], opts: &Options) -> Result<Rendered, Error> {
    opts.validate()?;
    if zpl.len() > opts.limits.max_input_bytes {
        return Err(Error::Limit(format!(
            "input larger than {} bytes",
            opts.limits.max_input_bytes
        )));
    }
    let mut it = interp::Interpreter::new(opts);
    for raw in parser::Lexer::new(zpl) {
        let cmd = parser::parse(&raw);
        it.exec(&raw, &cmd);
    }
    let (mut labels, warnings) = it.finish();
    if labels.is_empty() {
        return Err(Error::NoLabels);
    }
    if let Some(i) = opts.label_index {
        if i >= labels.len() {
            return Err(Error::LabelIndex {
                index: i,
                count: labels.len(),
            });
        }
        labels = vec![labels.swap_remove(i)];
    }
    Ok(Rendered { labels, warnings })
}

/// PDF conversion result.
#[derive(Debug, Clone)]
pub struct PdfOutput {
    /// The PDF document.
    pub pdf: Vec<u8>,
    /// Number of pages.
    pub pages: usize,
    /// Warnings.
    pub warnings: Vec<Warning>,
}

/// Converts ZPL to a PDF document with one page per label.
pub fn zpl_to_pdf(zpl: &[u8], opts: &Options) -> Result<PdfOutput, Error> {
    let r = render(zpl, opts)?;
    let pdf = pdf::write_pdf(&r.labels, &pdf::PdfMeta::default())?;
    Ok(PdfOutput {
        pdf,
        pages: r.labels.len(),
        warnings: r.warnings,
    })
}

/// PNG conversion result.
#[cfg(feature = "raster")]
#[derive(Debug, Clone)]
pub struct PngOutput {
    /// One PNG image per label.
    pub images: Vec<Vec<u8>>,
    /// Warnings.
    pub warnings: Vec<Warning>,
}

/// Converts ZPL to PNG images, one per label.
#[cfg(feature = "raster")]
pub fn zpl_to_png(zpl: &[u8], opts: &Options) -> Result<PngOutput, Error> {
    let r = render(zpl, opts)?;
    let images = r
        .labels
        .iter()
        .map(|l| raster::render(l).to_png())
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PngOutput {
        images,
        warnings: r.warnings,
    })
}
