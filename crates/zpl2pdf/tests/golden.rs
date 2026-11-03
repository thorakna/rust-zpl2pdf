//! Golden tests: renders `tests/fixtures/*.zpl` with the raster backend and
//! compares the result with Labelary reference images.
//!
//! Small offsets are tolerated: a black pixel only counts as a mismatch when
//! the other image has no black pixel within one dot. Diff images are written
//! to `target/golden/` (red: only in the reference, blue: only in ours).

use std::path::{Path, PathBuf};

use image::{GrayImage, Rgb, RgbImage};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn out_dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/golden");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Score {
    reference_black: usize,
    ours_black: usize,
    missing: usize,
    extra: usize,
}

impl Score {
    fn ratio(&self) -> f64 {
        (self.missing + self.extra) as f64 / (self.reference_black + self.ours_black).max(1) as f64
    }
}

fn near(img: &[bool], w: usize, h: usize, x: usize, y: usize, r: usize) -> bool {
    let (x0, y0) = (x.saturating_sub(r), y.saturating_sub(r));
    let (x1, y1) = ((x + r).min(w - 1), (y + r).min(h - 1));
    (y0..=y1).any(|yy| (x0..=x1).any(|xx| img[yy * w + xx]))
}

fn compare(name: &str, reference: &GrayImage, ours: &zpl2pdf::raster::Raster) -> Score {
    let (w, h) = (reference.width() as usize, reference.height() as usize);
    assert_eq!(
        (w, h),
        (ours.width as usize, ours.height as usize),
        "{name}: size differs"
    );
    let r: Vec<bool> = reference.pixels().map(|p| p.0[0] < 128).collect();
    let o: Vec<bool> = ours.pixels.iter().map(|&p| p != 0).collect();
    let mut diff = RgbImage::from_pixel(w as u32, h as u32, Rgb([255, 255, 255]));
    let mut score = Score {
        reference_black: 0,
        ours_black: 0,
        missing: 0,
        extra: 0,
    };
    for y in 0..h {
        for x in 0..w {
            let (rb, ob) = (r[y * w + x], o[y * w + x]);
            score.reference_black += rb as usize;
            score.ours_black += ob as usize;
            let px = if rb && ob {
                Rgb([0, 0, 0])
            } else if rb && !near(&o, w, h, x, y, 1) {
                score.missing += 1;
                Rgb([230, 0, 0])
            } else if ob && !near(&r, w, h, x, y, 1) {
                score.extra += 1;
                Rgb([0, 0, 230])
            } else if rb || ob {
                Rgb([150, 150, 150])
            } else {
                continue;
            };
            diff.put_pixel(x as u32, y as u32, px);
        }
    }
    diff.save(out_dir().join(format!("{name}.diff.png")))
        .unwrap();
    score
}

/// Maximum allowed mismatch ratio per fixture, a little above the measured
/// values so that regressions show up. Text heavy labels and 2D codes have a
/// higher budget: the substitute fonts differ in shape, and Zebra picks
/// other (equally valid) data encodations for some 2D symbols.
fn threshold(name: &str) -> f64 {
    match name {
        "basic_text" => 0.14,
        "barcodes_2d" => 0.09,
        "tr_cargo_like" => 0.06,
        _ => 0.02,
    }
}

#[test]
fn golden_fixtures() {
    let mut entries: Vec<_> = std::fs::read_dir(fixtures_dir())
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "zpl"))
        .collect();
    entries.sort();
    assert!(!entries.is_empty());
    let mut failures = Vec::new();
    for zpl_path in entries {
        let base = zpl_path.file_stem().unwrap().to_string_lossy().into_owned();
        let zpl = std::fs::read(&zpl_path).unwrap();
        let rendered = zpl2pdf::render(&zpl, &zpl2pdf::Options::default()).unwrap();
        for w in &rendered.warnings {
            eprintln!("{base}: warning: {w}");
        }
        let pdf = zpl2pdf::pdf::write_pdf(&rendered.labels, &Default::default()).unwrap();
        std::fs::write(out_dir().join(format!("{base}.pdf")), pdf).unwrap();
        for (i, label) in rendered.labels.iter().enumerate() {
            let name = if i == 0 {
                base.clone()
            } else {
                format!("{base}-{}", i + 1)
            };
            let png_path = zpl_path.with_file_name(format!("{name}.png"));
            if !png_path.exists() {
                eprintln!("{name}: no reference image, skipped");
                continue;
            }
            let raster = zpl2pdf::raster::render(label);
            std::fs::write(
                out_dir().join(format!("{name}.png")),
                raster.to_png().unwrap(),
            )
            .unwrap();
            let reference = image::open(&png_path).unwrap().to_luma8();
            let score = compare(&name, &reference, &raster);
            let ratio = score.ratio();
            eprintln!(
                "{name}: mismatch {:.2}% (missing {}, extra {}, reference {} / ours {} black dots)",
                ratio * 100.0,
                score.missing,
                score.extra,
                score.reference_black,
                score.ours_black
            );
            if ratio > threshold(&base) {
                failures.push(format!(
                    "{name}: {:.2}% > {:.0}%",
                    ratio * 100.0,
                    threshold(&base) * 100.0
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "golden mismatches:\n{}",
        failures.join("\n")
    );
}
