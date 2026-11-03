//! `zpl2pdf` command line tool.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;

/// Converts ZPL labels to PDF (one page per label) or PNG images.
#[derive(Parser, Debug)]
#[command(name = "zpl2pdf", version, about)]
struct Args {
    /// Input ZPL file, `-` or nothing for standard input.
    input: Option<PathBuf>,

    /// Output file, `-` for standard output. Defaults to the input name with
    /// a .pdf or .png extension (standard output when reading stdin).
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Printer resolution in dots per millimeter (6, 8, 12 or 24).
    #[arg(long, default_value_t = 8)]
    dpmm: u8,

    /// Label size as WIDTHxHEIGHT in inches, or with an `mm` suffix.
    #[arg(long, default_value = "4x6")]
    size: String,

    /// Write PNG instead of PDF. Several labels become name-1.png, name-2.png, ...
    #[arg(long)]
    png: bool,

    /// Only convert the label with this zero based index.
    #[arg(long)]
    index: Option<usize>,

    /// Repeat labels according to ^PQ.
    #[arg(long)]
    quantity: bool,

    /// Do not print warnings.
    #[arg(short, long)]
    quiet: bool,
}

fn parse_size(s: &str) -> Result<(f64, f64), String> {
    let lower = s.trim().to_ascii_lowercase();
    let (body, factor) = match lower.strip_suffix("mm") {
        Some(b) => (b.to_string(), 1.0 / 25.4),
        None => (lower.trim_end_matches("in").to_string(), 1.0),
    };
    let (w, h) = body
        .split_once('x')
        .ok_or_else(|| format!("invalid size {s:?}, expected WIDTHxHEIGHT"))?;
    let w: f64 = w
        .trim()
        .parse()
        .map_err(|_| format!("invalid width in {s:?}"))?;
    let h: f64 = h
        .trim()
        .parse()
        .map_err(|_| format!("invalid height in {s:?}"))?;
    Ok((w * factor, h * factor))
}

fn run(args: Args) -> Result<(), String> {
    let (width, height) = parse_size(&args.size)?;
    let opts = zpl2pdf::Options {
        dpmm: args.dpmm,
        width,
        height,
        label_index: args.index,
        honor_quantity: args.quantity,
        ..Default::default()
    };
    let stdin_input = args.input.as_deref().is_none_or(|p| p == Path::new("-"));
    let input = if stdin_input {
        let mut buf = Vec::new();
        std::io::stdin()
            .read_to_end(&mut buf)
            .map_err(|e| format!("reading standard input: {e}"))?;
        buf
    } else {
        let path = args.input.as_ref().expect("checked above");
        std::fs::read(path).map_err(|e| format!("reading {}: {e}", path.display()))?
    };
    let ext = if args.png { "png" } else { "pdf" };
    let output: Option<PathBuf> = match &args.output {
        Some(p) if p == Path::new("-") => None,
        Some(p) => Some(p.clone()),
        None if stdin_input => None,
        None => Some(
            args.input
                .as_ref()
                .expect("checked above")
                .with_extension(ext),
        ),
    };

    let (files, warnings): (Vec<(Option<PathBuf>, Vec<u8>)>, _) = if args.png {
        let out = zpl2pdf::zpl_to_png(&input, &opts).map_err(|e| e.to_string())?;
        let n = out.images.len();
        let files = out
            .images
            .into_iter()
            .enumerate()
            .map(|(i, img)| {
                let path = output.as_ref().map(|p| {
                    if n == 1 {
                        p.clone()
                    } else {
                        let stem = p
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        p.with_file_name(format!("{stem}-{}.png", i + 1))
                    }
                });
                (path, img)
            })
            .collect();
        (files, out.warnings)
    } else {
        let out = zpl2pdf::zpl_to_pdf(&input, &opts).map_err(|e| e.to_string())?;
        (vec![(output.clone(), out.pdf)], out.warnings)
    };

    if !args.quiet {
        for w in &warnings {
            eprintln!("warning: {w}");
        }
    }
    for (path, data) in files {
        match path {
            Some(p) => {
                std::fs::write(&p, data).map_err(|e| format!("writing {}: {e}", p.display()))?
            }
            None => std::io::stdout()
                .write_all(&data)
                .map_err(|e| format!("writing standard output: {e}"))?,
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_size;

    #[test]
    fn sizes() {
        assert_eq!(parse_size("4x6").unwrap(), (4.0, 6.0));
        assert_eq!(parse_size("4x6in").unwrap(), (4.0, 6.0));
        let (w, h) = parse_size("101.6x152.4mm").unwrap();
        assert!((w - 4.0).abs() < 1e-9 && (h - 6.0).abs() < 1e-9);
        assert!(parse_size("4by6").is_err());
    }
}
