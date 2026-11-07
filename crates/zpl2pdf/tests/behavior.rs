//! Behaviour tests of the public API: label handling, interpreter state,
//! limits and robustness against arbitrary input.

use proptest::prelude::*;
use zpl2pdf::scene::{Item, Paint};
use zpl2pdf::{Error, Options, Rotation};

fn render(zpl: &str) -> zpl2pdf::Rendered {
    zpl2pdf::render(zpl.as_bytes(), &Options::default()).expect("renders")
}

fn texts(label: &zpl2pdf::Label) -> Vec<String> {
    label
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Text(run) => Some(run.glyphs.iter().map(|g| g.ch).collect()),
            _ => None,
        })
        .collect()
}

#[test]
fn page_size_follows_options() {
    let opts = Options {
        dpmm: 12,
        width: 2.0,
        height: 1.0,
        ..Default::default()
    };
    let r = zpl2pdf::render(b"^XA^FO10,10^GB50,50,50^FS^XZ", &opts).unwrap();
    assert_eq!((r.labels[0].width, r.labels[0].height), (600.0, 300.0));
}

/// Renders a 2x1 inch label (406x203 dots) with `rotation` degrees.
fn rotated(zpl: &str, degrees: u16) -> zpl2pdf::raster::Raster {
    let opts = Options {
        width: 2.0,
        height: 1.0,
        rotation: Rotation::from_degrees(degrees).unwrap(),
        ..Default::default()
    };
    let r = zpl2pdf::render(zpl.as_bytes(), &opts).unwrap();
    zpl2pdf::raster::render(&r.labels[0])
}

#[test]
fn output_rotation_turns_the_page_clockwise() {
    // A filled 50x20 box in the top left corner.
    let zpl = "^XA^FO0,0^GB50,20,20^FS^XZ";
    // (degrees, page size, inside the box, outside the box)
    let cases = [
        (0, (406, 203), [(1, 1), (48, 18)], (52, 1)),
        (90, (203, 406), [(202, 1), (184, 48)], (1, 1)),
        (180, (406, 203), [(405, 202), (357, 184)], (1, 1)),
        (270, (203, 406), [(1, 405), (18, 357)], (1, 1)),
    ];
    for (deg, size, inside, outside) in cases {
        let img = rotated(zpl, deg);
        assert_eq!((img.width, img.height), size, "{deg}");
        for (x, y) in inside {
            assert!(img.get(x, y), "{deg}: ({x}, {y}) should be black");
        }
        assert!(
            !img.get(outside.0, outside.1),
            "{deg}: {outside:?} should be white"
        );
        let black = img.pixels.iter().filter(|&&p| p != 0).count();
        assert_eq!(black, 50 * 20, "{deg}: pixels lost or duplicated");
    }
}

#[test]
fn output_rotation_applies_after_print_orientation() {
    // ^PO I turns the label by 180 first, so 90 more equals 270 in total.
    let plain = rotated("^XA^FO0,0^GB50,20,20^FS^XZ", 270);
    let inverted = rotated("^XA^POI^FO0,0^GB50,20,20^FS^XZ", 90);
    assert_eq!(plain.pixels, inverted.pixels);
}

#[test]
fn output_rotation_swaps_the_pdf_page() {
    let media_box = |degrees| {
        let opts = Options {
            width: 2.0,
            height: 1.0,
            rotation: Rotation::from_degrees(degrees).unwrap(),
            ..Default::default()
        };
        let pdf = zpl2pdf::zpl_to_pdf(b"^XA^FO0,0^GB50,20,20^FS^XZ", &opts)
            .unwrap()
            .pdf;
        let text = String::from_utf8_lossy(&pdf).into_owned();
        let start = text.find("/MediaBox [").unwrap();
        text[start..start + text[start..].find(']').unwrap() + 1].to_string()
    };
    assert_eq!(media_box(0), "/MediaBox [0 0 144 72]");
    assert_eq!(media_box(90), "/MediaBox [0 0 72 144]");
    assert_eq!(media_box(180), "/MediaBox [0 0 144 72]");
    assert_eq!(media_box(270), "/MediaBox [0 0 72 144]");
}

#[test]
fn rotation_parsing() {
    assert_eq!("90".parse::<Rotation>().unwrap(), Rotation::R90);
    assert_eq!(" 270 ".parse::<Rotation>().unwrap(), Rotation::R270);
    assert_eq!("0".parse::<Rotation>().unwrap(), Rotation::R0);
    for bad in ["45", "-90", "360", "", "abc"] {
        assert!(bad.parse::<Rotation>().is_err(), "{bad:?}");
    }
    assert_eq!(Rotation::R180.to_string(), "180");
}

#[test]
fn label_selection_and_errors() {
    let zpl = b"^XA^FO1,1^FDA^FS^XZ^XA^FO1,1^FDB^FS^XZ";
    let r = zpl2pdf::render(zpl, &Options::default()).unwrap();
    assert_eq!(r.labels.len(), 2);
    let r = zpl2pdf::render(
        zpl,
        &Options {
            label_index: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(texts(&r.labels[0]), ["B"]);
    assert!(matches!(
        zpl2pdf::render(
            zpl,
            &Options {
                label_index: Some(5),
                ..Default::default()
            }
        ),
        Err(Error::LabelIndex { index: 5, count: 2 })
    ));
    assert!(matches!(
        zpl2pdf::render(b"no zpl here", &Options::default()),
        Err(Error::NoLabels)
    ));
    assert!(matches!(
        zpl2pdf::render(
            zpl,
            &Options {
                dpmm: 7,
                ..Default::default()
            }
        ),
        Err(Error::InvalidOptions(_))
    ));
}

#[test]
fn print_quantity() {
    let zpl = b"^XA^PQ3^FO1,1^FDA^FS^XZ";
    assert_eq!(
        zpl2pdf::render(zpl, &Options::default())
            .unwrap()
            .labels
            .len(),
        1
    );
    let opts = Options {
        honor_quantity: true,
        ..Default::default()
    };
    assert_eq!(zpl2pdf::render(zpl, &opts).unwrap().labels.len(), 3);
    let mut opts = Options {
        honor_quantity: true,
        ..Default::default()
    };
    opts.limits.max_labels = 2;
    assert_eq!(
        zpl2pdf::render(b"^XA^PQ100^FO1,1^FDA^FS^XZ", &opts)
            .unwrap()
            .labels
            .len(),
        2
    );
}

#[test]
fn unknown_commands_warn() {
    let r = render("^XA^FO1,1^QQ12^FDA^FS^XZ");
    assert_eq!(r.warnings.len(), 1);
    assert_eq!(r.warnings[0].command, "^QQ");
    assert_eq!(texts(&r.labels[0]), ["A"]);
}

#[test]
fn hex_escapes_and_utf8() {
    let r = render("^XA^CI28^FO1,1^FH^FDA_5EB^FS^FO1,50^FDĞüş^FS^XZ");
    assert_eq!(texts(&r.labels[0]), ["A^B", "Ğüş"]);
}

#[test]
fn stored_format_and_field_numbers() {
    let r = render(
        "^XA^DFR:T.ZPL^FS^FO10,10^A0N,30^FN1^FDdefault^FS^FO10,60^A0N,30^FN2^FS^XZ\
         ^XA^XFR:T.ZPL^FS^FN2^FDsecond^FS^XZ\
         ^XA^XFR:T.ZPL^FS^FN1^FDfirst^FS^FN2^FDother^FS^XZ",
    );
    assert_eq!(r.labels.len(), 2, "the ^DF label is not printed");
    assert_eq!(texts(&r.labels[0]), ["default", "second"]);
    assert_eq!(texts(&r.labels[1]), ["first", "other"]);
}

#[test]
fn label_home_shift_and_top() {
    let r = render("^XA^LH100,50^LS20^LT10^FO10,10^GB30,20,20^FS^XZ");
    let Item::Rect { x, y, w, h, .. } = r.labels[0].items[0] else {
        panic!("expected a rectangle")
    };
    assert_eq!((x, y, w, h), (90.0, 70.0, 30.0, 20.0));
}

#[test]
fn print_width_centers_and_clips() {
    let r = render("^XA^PW400^FO0,0^GB10,10,10^FS^XZ");
    let l = &r.labels[0];
    assert_eq!(l.clip, Some((206.0, 0.0, 400.0, 1218.0)));
    let Item::Rect { x, .. } = l.items[0] else {
        panic!()
    };
    assert_eq!(x, 206.0);
}

#[test]
fn reverse_fields_xor() {
    let r = render("^XA^FO0,0^GB100,100,100^FS^FO50,0^GB100,100,100^FR^FS^XZ");
    assert!(matches!(
        r.labels[0].items[1],
        Item::Rect {
            paint: Paint::Reverse,
            ..
        }
    ));
    let img = zpl2pdf::raster::render(&r.labels[0]);
    assert!(img.get(25, 50), "only the first box");
    assert!(!img.get(75, 50), "overlap is inverted");
    assert!(img.get(125, 50), "only the reversed box");
}

#[test]
fn rotated_text_box_is_placed_at_origin() {
    for o in ['N', 'R', 'I', 'B'] {
        let r = render(&format!("^XA^FO300,300^A0{o},60,60^FDHHgH^FS^XZ"));
        let img = zpl2pdf::raster::render(&r.labels[0]);
        let (mut min_x, mut min_y) = (u32::MAX, u32::MAX);
        for y in 0..img.height {
            for x in 0..img.width {
                if img.get(x, y) {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                }
            }
        }
        // Labelary: the ink starts within a few dots of the origin.
        assert!((300..=306).contains(&min_x), "{o}: x {min_x}");
        assert!((300..=306).contains(&min_y), "{o}: y {min_y}");
    }
}

#[test]
fn pdf_structure() {
    let out = zpl2pdf::zpl_to_pdf(
        b"^XA^FO50,50^A0N,40,40^FDHello^FS^FO50,120^GFA,4,4,1,F0F0F0F0^FS^FO50,200^GB100,100,100^FR^FS^XZ^XA^XZ",
        &Options::default(),
    )
    .unwrap();
    assert_eq!(out.pages, 2);
    assert!(out.pdf.starts_with(b"%PDF-1.4"));
    let text = String::from_utf8_lossy(&out.pdf);
    assert!(text.contains("/Type /Page"));
    assert!(text.contains("/FontFile2"));
    assert!(text.contains("/ImageMask true"));
    assert!(text.contains("/BM /Difference"));
    assert!(text.contains("/ToUnicode"));
    assert!(text.trim_end().ends_with("%%EOF"));
}

#[test]
fn magnified_graphic_respects_limit() {
    let mut opts = Options::default();
    opts.limits.max_graphic_bytes = 1000;
    // 50 rows of 10 bytes: one row of ones, repeated with ':'.
    let zpl = format!(
        "~DGR:A.GRF,500,10,!{}^XA^FO0,0^XGR:A.GRF,1,1^FS^FO0,100^XGR:A.GRF,10,10^FS^XZ",
        ":".repeat(49)
    );
    let r = zpl2pdf::render(zpl.as_bytes(), &opts).unwrap();
    assert_eq!(r.labels[0].items.len(), 1, "unmagnified copy is drawn");
    let r = zpl2pdf::render(zpl.replace("^FO0,0^XGR:A.GRF,1,1^FS", "").as_bytes(), &opts).unwrap();
    assert!(r.labels[0].items.is_empty());
    assert!(r.warnings.iter().any(|w| w.message.contains("limit")));
}

#[test]
fn recursive_format_recall_terminates() {
    let r = render("^XA^DFR:LOOP.ZPL^FS^XFR:LOOP.ZPL^FS^XZ^XA^XFR:LOOP.ZPL^FS^FO1,1^FDok^FS^XZ");
    assert_eq!(texts(&r.labels[0]), ["ok"]);
}

fn fragment() -> impl Strategy<Value = String> {
    let n = || 0u32..2000;
    let small = || 1u32..12;
    let o = || prop_oneof![Just('N'), Just('R'), Just('I'), Just('B')];
    let s = || "[ -~]{0,20}".prop_map(|s| s.replace(['^', '~'], ""));
    let hex = || "[0-9A-Fa-fG-Zg-z,:!]{0,40}";
    prop_oneof![
        (n(), n()).prop_map(|(x, y)| format!("^FO{x},{y}")),
        (n(), n()).prop_map(|(x, y)| format!("^FT{x},{y}")),
        (o(), n(), n()).prop_map(|(o, h, w)| format!("^A0{o},{h},{w}")),
        (o(), small()).prop_map(|(o, m)| format!("^AD{o},{}", m * 18)),
        s().prop_map(|t| format!("^FD{t}^FS")),
        Just("^FS".to_string()),
        Just("^FR".to_string()),
        Just("^LRY".to_string()),
        Just("^POI".to_string()),
        (n(), n(), n()).prop_map(|(w, h, t)| format!("^GB{w},{h},{t}")),
        (n(), small()).prop_map(|(d, t)| format!("^GC{d},{t}")),
        (n(), n(), small()).prop_map(|(w, h, t)| format!("^GD{w},{h},{t}")),
        (small(), small(), n()).prop_map(|(w, r, h)| format!("^BY{w},{r},{h}")),
        (o(), n()).prop_map(|(o, h)| format!("^BC{o},{h},Y,N,N,A")),
        (o(), n()).prop_map(|(o, h)| format!("^B3{o},N,{h}")),
        (o(), n()).prop_map(|(o, h)| format!("^BE{o},{h}")),
        small().prop_map(|m| format!("^BQN,2,{m}^FDQA,")),
        (small(), small()).prop_map(|(h, s)| format!("^B7N,{h},{s}")),
        small().prop_map(|m| format!("^BXN,{m},200")),
        (2u32..7).prop_map(|m| format!("^BD{m}")),
        small().prop_map(|m| format!("^B0N,{m}")),
        (n(), n(), small(), hex()).prop_map(|(a, b, c, d)| format!("^GFA,{a},{b},{c},{d}")),
        (n(), small(), small()).prop_map(|(w, l, sp)| format!("^FB{w},{l},{sp},J,5")),
        n().prop_map(|w| format!("^PW{w}")),
        prop_oneof![Just(0u32), Just(27), Just(28), Just(29), Just(36)]
            .prop_map(|c| format!("^CI{c}")),
        (small(), hex()).prop_map(|(w, d)| format!("~DGR:X.GRF,{},{w},{d}", w * 4)),
        (small(), small()).prop_map(|(a, b)| format!("^XGR:X.GRF,{a},{b}")),
        Just("^FH^FD_41_".to_string()),
        Just("^DFR:F.ZPL^FS".to_string()),
        Just("^XFR:F.ZPL^FS".to_string()),
        Just("^FN1".to_string()),
        Just("^XZ^XA".to_string()),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(160))]

    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..600)) {
        let mut zpl = b"^XA".to_vec();
        zpl.extend(bytes);
        zpl.extend(b"^XZ");
        let opts = Options { width: 2.0, height: 2.0, ..Default::default() };
        if let Ok(r) = zpl2pdf::render(&zpl, &opts) {
            zpl2pdf::pdf::write_pdf(&r.labels, &Default::default()).unwrap();
            for l in &r.labels {
                zpl2pdf::raster::render(l);
            }
        }
    }

    #[test]
    fn arbitrary_commands_never_panic(parts in proptest::collection::vec(fragment(), 0..25)) {
        let zpl = format!("^XA{}^XZ", parts.concat());
        let opts = Options { width: 2.0, height: 2.0, ..Default::default() };
        if let Ok(r) = zpl2pdf::render(zpl.as_bytes(), &opts) {
            zpl2pdf::pdf::write_pdf(&r.labels, &Default::default()).unwrap();
            for l in &r.labels {
                zpl2pdf::raster::render(l);
            }
        }
    }
}
