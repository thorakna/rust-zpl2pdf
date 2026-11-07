//! PDF backend: writes scene labels as vector PDF pages.
//!
//! Every label becomes one page sized to the label. Coordinates stay in
//! printer dots through a single page transform. Reverse printing (`^FR`,
//! `^LR`) uses the Difference blend mode on a white page, which reproduces
//! the printer's XOR behaviour. Monochrome images are embedded as 1 bit
//! image masks and fonts as subsetted CID fonts with a ToUnicode map, so
//! text stays selectable.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use pdf_writer::types::{BlendMode, CidFontType, FontFlags, SystemInfo, UnicodeCmap};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str};
use subsetter::GlyphRemapper;

use crate::Error;
use crate::font::{self, FaceId};
use crate::graphic::Bitmap;
use crate::scene::{Item, Label, Paint, Segment};
use crate::types::Transform;

/// Document metadata.
#[derive(Debug, Clone, Default)]
pub struct PdfMeta {
    /// Optional document title.
    pub title: Option<String>,
}

struct FontUse {
    remapper: GlyphRemapper,
    /// new gid -> characters
    chars: BTreeMap<u16, char>,
}

struct Refs {
    next: i32,
}

impl Refs {
    fn alloc(&mut self) -> Ref {
        let r = Ref::new(self.next);
        self.next += 1;
        r
    }
}

fn compress(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
}

fn face_name(id: FaceId) -> String {
    format!("F{}", id as u8)
}

/// Deterministic six letter subset tag.
fn subset_tag(id: FaceId, gids: &[u16]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ id as u64;
    for g in gids {
        h ^= *g as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    (0..6)
        .map(|i| (b'A' + ((h >> (i * 5)) % 26) as u8) as char)
        .collect()
}

/// Writes labels into a PDF document.
pub fn write_pdf(labels: &[Label], meta: &PdfMeta) -> Result<Vec<u8>, Error> {
    if labels.is_empty() {
        return Err(Error::NoLabels);
    }
    // Collect glyph usage and remap glyph ids per face.
    let mut fonts: BTreeMap<FaceId, FontUse> = BTreeMap::new();
    for label in labels {
        for item in &label.items {
            if let Item::Text(run) = item {
                let entry = fonts.entry(run.font.face).or_insert_with(|| {
                    let mut remapper = GlyphRemapper::new();
                    remapper.remap(0);
                    FontUse {
                        remapper,
                        chars: BTreeMap::new(),
                    }
                });
                for g in &run.glyphs {
                    let new = entry.remapper.remap(g.gid);
                    entry.chars.entry(new).or_insert(g.ch);
                }
            }
        }
    }

    let mut refs = Refs { next: 1 };
    let catalog_id = refs.alloc();
    let tree_id = refs.alloc();
    let info_id = refs.alloc();
    let gs_reverse = refs.alloc();
    let font_refs: BTreeMap<FaceId, Ref> = fonts.keys().map(|k| (*k, refs.alloc())).collect();

    // Unique images.
    let mut images: Vec<Arc<Bitmap>> = Vec::new();
    let mut image_index: HashMap<*const Bitmap, usize> = HashMap::new();
    for label in labels {
        for item in &label.items {
            if let Item::Bitmap { bitmap, .. } = item
                && let std::collections::hash_map::Entry::Vacant(e) =
                    image_index.entry(Arc::as_ptr(bitmap))
            {
                e.insert(images.len());
                images.push(bitmap.clone());
            }
        }
    }
    let image_refs: Vec<Ref> = images.iter().map(|_| refs.alloc()).collect();

    let mut pdf = Pdf::new();
    pdf.set_version(1, 4);

    let mut page_ids = Vec::new();
    for label in labels {
        let page_id = refs.alloc();
        let content_id = refs.alloc();
        page_ids.push(page_id);

        let scale = 72.0 / label.dpi;
        let (page_w, page_h) = label.page_size();
        let (w_pt, h_pt) = ((page_w * scale) as f32, (page_h * scale) as f32);
        let content = page_content(label, &fonts, &image_index)?;
        let compressed = compress(&content);
        pdf.stream(content_id, &compressed)
            .filter(Filter::FlateDecode);

        let mut page = pdf.page(page_id);
        page.media_box(Rect::new(0.0, 0.0, w_pt, h_pt));
        page.parent(tree_id);
        page.contents(content_id);
        let mut resources = page.resources();
        {
            let mut f = resources.fonts();
            for (face, r) in &font_refs {
                f.pair(Name(face_name(*face).as_bytes()), *r);
            }
        }
        {
            let mut x = resources.x_objects();
            for (i, r) in image_refs.iter().enumerate() {
                x.pair(Name(format!("Im{i}").as_bytes()), *r);
            }
        }
        resources.ext_g_states().pair(Name(b"GSd"), gs_reverse);
        resources.finish();
        page.finish();
    }

    pdf.pages(tree_id)
        .kids(page_ids.iter().copied())
        .count(page_ids.len() as i32);
    pdf.catalog(catalog_id).pages(tree_id);
    {
        let mut info = pdf.document_info(info_id);
        info.producer(pdf_writer::TextStr("zpl2pdf"));
        if let Some(t) = &meta.title {
            info.title(pdf_writer::TextStr(t));
        }
    }
    pdf.ext_graphics(gs_reverse)
        .blend_mode(BlendMode::Difference);

    for (i, bmp) in images.iter().enumerate() {
        let packed = pack_rows(bmp);
        let data = compress(&packed);
        let mut img = pdf.image_xobject(image_refs[i], &data);
        img.filter(Filter::FlateDecode);
        img.width(bmp.width as i32);
        img.height(bmp.height as i32);
        img.image_mask(true);
        img.bits_per_component(1);
        img.decode([1.0, 0.0]);
        img.finish();
    }

    for (face_id, usage) in &fonts {
        write_font(&mut pdf, &mut refs, font_refs[face_id], *face_id, usage)?;
    }

    Ok(pdf.finish())
}

/// Packs bitmap rows to exactly `ceil(width / 8)` bytes per row.
fn pack_rows(bmp: &Bitmap) -> Vec<u8> {
    let row = bmp.width.div_ceil(8) as usize;
    if row == bmp.row_bytes as usize {
        return bmp.data.clone();
    }
    let mut out = Vec::with_capacity(row * bmp.height as usize);
    for y in 0..bmp.height as usize {
        let start = y * bmp.row_bytes as usize;
        out.extend_from_slice(&bmp.data[start..start + row]);
    }
    out
}

fn matrix(t: &Transform) -> [f32; 6] {
    [
        t.a as f32, t.b as f32, t.c as f32, t.d as f32, t.e as f32, t.f as f32,
    ]
}

fn set_paint(c: &mut Content, paint: Paint) {
    match paint {
        Paint::Black => {
            c.set_fill_gray(0.0);
        }
        Paint::White => {
            c.set_fill_gray(1.0);
        }
        Paint::Reverse => {
            c.set_parameters(Name(b"GSd"));
            c.set_fill_gray(1.0);
        }
    }
}

fn page_content(
    label: &Label,
    fonts: &BTreeMap<FaceId, FontUse>,
    image_index: &HashMap<*const Bitmap, usize>,
) -> Result<Vec<u8>, Error> {
    let mut c = Content::new();
    let scale = 72.0 / label.dpi;
    c.save_state();
    // Page dots, y down.
    c.transform([
        scale as f32,
        0.0,
        0.0,
        -scale as f32,
        0.0,
        (label.page_size().1 * scale) as f32,
    ]);
    c.transform(matrix(&label.page_transform()));
    // White paper: needed as backdrop for the Difference blend mode.
    c.set_fill_gray(1.0);
    c.rect(0.0, 0.0, label.width as f32, label.height as f32);
    c.fill_nonzero();
    if let Some((x, y, w, h)) = label.clip {
        c.rect(x as f32, y as f32, w as f32, h as f32);
        c.clip_nonzero();
        c.end_path();
    }
    for item in &label.items {
        c.save_state();
        match item {
            Item::Rect { x, y, w, h, paint } => {
                set_paint(&mut c, *paint);
                c.rect(*x as f32, *y as f32, *w as f32, *h as f32);
                c.fill_nonzero();
            }
            Item::Path { path, paint } => {
                set_paint(&mut c, *paint);
                for s in &path.segments {
                    match *s {
                        Segment::MoveTo(x, y) => {
                            c.move_to(x as f32, y as f32);
                        }
                        Segment::LineTo(x, y) => {
                            c.line_to(x as f32, y as f32);
                        }
                        Segment::CubicTo(a, b, cc, d, e, f) => {
                            c.cubic_to(a as f32, b as f32, cc as f32, d as f32, e as f32, f as f32);
                        }
                        Segment::Close => {
                            c.close_path();
                        }
                    }
                }
                c.fill_even_odd();
            }
            Item::Bitmap {
                x,
                y,
                bitmap,
                paint,
            } => {
                let idx = image_index[&Arc::as_ptr(bitmap)];
                set_paint(&mut c, *paint);
                let (w, h) = (bitmap.width as f32, bitmap.height as f32);
                c.transform([w, 0.0, 0.0, -h, *x as f32, *y as f32 + h]);
                c.x_object(Name(format!("Im{idx}").as_bytes()));
            }
            Item::Text(run) => {
                let usage = fonts
                    .get(&run.font.face)
                    .ok_or_else(|| Error::Output("font missing".into()))?;
                let face = font::face(run.font.face);
                let em = run.font.em;
                let sx = run.font.scale_x;
                set_paint(&mut c, *paint_of(item));
                // Text space (y up, in em units scaled by the font size) to
                // run space (y down) to label dots.
                let tm = Transform {
                    a: sx,
                    d: -1.0,
                    ..Transform::IDENTITY
                }
                .then(&run.transform);
                c.begin_text();
                c.set_font(Name(face_name(run.font.face).as_bytes()), em as f32);
                let first_x = run.glyphs.first().map(|g| g.x).unwrap_or(0.0);
                let start = Transform::translate(first_x, 0.0).then(&run.transform);
                let tm = Transform {
                    e: start.e,
                    f: start.f,
                    ..tm
                };
                c.set_text_matrix(matrix(&tm));
                {
                    let mut show = c.show_positioned();
                    let mut items = show.items();
                    let mut pos = first_x;
                    for g in &run.glyphs {
                        let delta = g.x - pos;
                        if delta.abs() > 1e-6 {
                            items.adjust((-delta * 1000.0 / (em * sx)) as f32);
                        }
                        let cid = usage.remapper.get(g.gid).unwrap_or(0);
                        items.show(Str(&cid.to_be_bytes()));
                        pos = g.x + face.advance(g.gid) / face.units_per_em * em * sx;
                    }
                }
                c.end_text();
            }
        }
        c.restore_state();
    }
    c.restore_state();
    Ok(c.finish().into_vec())
}

fn paint_of(item: &Item) -> &Paint {
    match item {
        Item::Rect { paint, .. } | Item::Path { paint, .. } | Item::Bitmap { paint, .. } => paint,
        Item::Text(run) => &run.paint,
    }
}

fn write_font(
    pdf: &mut Pdf,
    refs: &mut Refs,
    type0_ref: Ref,
    id: FaceId,
    usage: &FontUse,
) -> Result<(), Error> {
    let face = font::face(id);
    let cid_ref = refs.alloc();
    let descriptor_ref = refs.alloc();
    let file_ref = refs.alloc();
    let cmap_ref = refs.alloc();

    let gids: Vec<u16> = usage.remapper.remapped_gids().collect();
    let tag = subset_tag(id, &gids);
    let base_font = format!("{tag}+{}", face.postscript_name);
    let subset = subsetter::subset(face.data, 0, &usage.remapper)
        .map_err(|e| Error::Output(format!("font subset: {e:?}")))?;
    let to_units = 1000.0 / face.units_per_em;

    let system_info = SystemInfo {
        registry: Str(b"Adobe"),
        ordering: Str(b"Identity"),
        supplement: 0,
    };

    pdf.type0_font(type0_ref)
        .base_font(Name(base_font.as_bytes()))
        .encoding_predefined(Name(b"Identity-H"))
        .descendant_font(cid_ref)
        .to_unicode(cmap_ref);

    {
        let mut cid = pdf.cid_font(cid_ref);
        cid.subtype(CidFontType::Type2);
        cid.base_font(Name(base_font.as_bytes()));
        cid.system_info(system_info);
        cid.font_descriptor(descriptor_ref);
        cid.default_width(0.0);
        cid.cid_to_gid_map_predefined(Name(b"Identity"));
        let widths: Vec<f32> = gids
            .iter()
            .map(|g| (face.advance(*g) * to_units) as f32)
            .collect();
        cid.widths().consecutive(0, widths);
        cid.finish();
    }

    let mut flags = FontFlags::SYMBOLIC;
    if id != FaceId::Sans {
        flags |= FontFlags::FIXED_PITCH;
    }
    let bbox = gids.iter().filter_map(|g| face.bounds(*g)).fold(
        (0.0f64, face.descent, 0.0f64, face.ascent),
        |(x0, y0, x1, y1), (a, b, c, d)| (x0.min(a), y0.min(b), x1.max(c), y1.max(d)),
    );
    pdf.font_descriptor(descriptor_ref)
        .name(Name(base_font.as_bytes()))
        .flags(flags)
        .bbox(Rect::new(
            (bbox.0 * to_units) as f32,
            (bbox.1 * to_units) as f32,
            (bbox.2 * to_units) as f32,
            (bbox.3 * to_units) as f32,
        ))
        .italic_angle(0.0)
        .ascent((face.ascent * to_units) as f32)
        .descent((face.descent * to_units) as f32)
        .cap_height((face.cap_height * to_units) as f32)
        .stem_v(if id == FaceId::Mono { 80.0 } else { 120.0 })
        .font_file2(file_ref);

    let compressed = compress(&subset);
    pdf.stream(file_ref, &compressed)
        .filter(Filter::FlateDecode)
        .pair(Name(b"Length1"), subset.len() as i32);

    let mut cmap = UnicodeCmap::new(Name(b"Custom"), system_info);
    for (cid, ch) in &usage.chars {
        cmap.pair(*cid, *ch);
    }
    let cmap_data = cmap.finish();
    pdf.stream(cmap_ref, cmap_data.as_slice());
    Ok(())
}
