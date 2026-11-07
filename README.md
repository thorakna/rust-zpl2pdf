# zpl2pdf

Native ZPL (Zebra Programming Language) to PDF and PNG conversion in pure Rust.
No printer, no Labelary API, no browser and no C libraries are involved.

The workspace contains three crates:

| Crate | What it is |
|---|---|
| `crates/zpl2pdf` | The library: parser, interpreter, bar code encoders, PDF and raster backends |
| `crates/zpl2pdf-cli` | The `zpl2pdf` command line tool |
| `crates/zpl2pdf-server` | An HTTP service with Labelary compatible routes |

PDF output is vector based: text, boxes and bar codes stay sharp at any zoom
level, text is selectable and searchable, and images are embedded as 1 bit
masks. A raster backend produces printer-like PNG images.

## Quick start

```sh
# Library and tools
cargo build --release

# PDF, one page per ^XA...^XZ label
target/release/zpl2pdf label.zpl -o label.pdf --dpmm 8 --size 4x6

# PNG (several labels become label-1.png, label-2.png, ...)
target/release/zpl2pdf label.zpl --png -o label.png

# Standard input / output
cat label.zpl | target/release/zpl2pdf - > label.pdf
```

CLI options: `--dpmm 6|8|12|24`, `--size WxH` in inches or `WxHmm`,
`--index N` to pick one label, `--quantity` to repeat labels by `^PQ`,
`--quiet` to hide warnings.

### Library

```rust
let zpl = std::fs::read("label.zpl")?;
let opts = zpl2pdf::Options { dpmm: 8, width: 4.0, height: 6.0, ..Default::default() };
let out = zpl2pdf::zpl_to_pdf(&zpl, &opts)?;
std::fs::write("label.pdf", &out.pdf)?;
for w in &out.warnings {
    eprintln!("{w}"); // unsupported commands, bad bar code data, ...
}
```

`zpl2pdf::render` returns the backend independent scene (`Label` items in
printer dots) if you want to draw it yourself, and `zpl_to_png` produces PNG
images. `Options::limits` bounds the number of labels, graphic sizes and input
size for untrusted input.

### HTTP service

```sh
target/release/zpl2pdf-server --listen 0.0.0.0:8080
```

| Route | Description |
|---|---|
| `POST /v1/printers/{8dpmm}/labels/{4x6}/` | Labelary compatible. `Accept: application/pdf` (default) returns all labels, `Accept: image/png` the first one |
| `POST /v1/printers/{8dpmm}/labels/{4x6}/{index}/` | One label |
| `POST /v1/convert?dpmm=8&width=4&height=6&format=pdf&index=0` | Query string variant |
| `GET /health` | Liveness check |

The body is the raw ZPL. Responses carry `X-Total-Count` (labels in the input)
and `X-Warnings`. Existing Labelary integrations only need a new base URL.

Settings (flags or environment variables): `--listen` / `ZPL2PDF_LISTEN`,
`--max-body` / `ZPL2PDF_MAX_BODY` (8 MiB), `--max-labels` /
`ZPL2PDF_MAX_LABELS` (200), `--max-graphic` / `ZPL2PDF_MAX_GRAPHIC` (8 MiB
decoded), `--concurrency` / `ZPL2PDF_CONCURRENCY` (CPU count), `--timeout` /
`ZPL2PDF_TIMEOUT` (30 s), `--auth-token` / `ZPL2PDF_AUTH_TOKEN` (empty).
Conversions run on blocking threads behind a semaphore, so slow requests
cannot starve the async runtime. [`example.env`](example.env) lists every
setting.

**Authorization:** when `ZPL2PDF_AUTH_TOKEN` is not empty, the conversion
routes require `Authorization: Bearer <token>` (the bare token is accepted
too) and answer `401 Unauthorized` otherwise. `/health` stays open. Leave the
variable empty or unset to disable the check.

Docker:

```sh
docker build -t zpl2pdf .
cp example.env .env   # set ZPL2PDF_AUTH_TOKEN here
docker run --rm -p 8080:8080 --env-file .env zpl2pdf
curl --data-binary @label.zpl -H "Accept: application/pdf" \
     -H "Authorization: Bearer $ZPL2PDF_AUTH_TOKEN" \
     http://localhost:8080/v1/printers/8dpmm/labels/4x6/ -o label.pdf
```

## Supported ZPL

**Label and field commands:** `^XA ^XZ ^FO ^FT ^FS ^FD ^FV ^FX ^FH ^FR ^FN
^FB ^TB ^FW ^FP ^CF ^CI ^CC ^CD ^CT ~CC ~CD ~CT ^BY ^LH ^LS ^LT ^LR ^PW ^PO ^PM
^PQ ^DF ^XF ^SN ^SF`

**Fonts:** `^A0` (scalable), bitmap fonts `A B C D E F G H`, `P` to `V`, `^GS`
symbols. `^A@`, `^CW` and downloaded fonts fall back to font 0 with a warning.

**Graphics:** `^GB` (including rounded corners), `^GC ^GE ^GD`, `^GF` in ASCII
hex with ZPL compression, binary and `:Z64:` / `:B64:` payloads, `~DG ^XG ^IM
^ID ~EG`, `~DY` with GRF and PNG data.

**Bar codes:** Code 128 (`^BC` modes N, U, A and D with `>` invocation codes),
Code 39 (`^B3`, `^BL`), Interleaved 2 of 5 (`^B2`), Code 93 (`^BA`), Codabar
(`^BK`), Code 11 (`^B1`), Industrial and Standard 2 of 5 (`^BI ^BJ`), EAN-13,
EAN-8, UPC-A, UPC-E (`^BE ^B8 ^BU ^B9`), QR (`^BQ`), Data Matrix ECC 200 (`^BX`),
PDF417 (`^B7`), Aztec (`^B0 ^BO`) and MaxiCode (`^BD`, modes 2 to 6).

**Character sets (`^CI`):** 0 to 13 (code page 850 with the national
variants), 27 (1252), 28 (UTF-8), 29 and 30 (UTF-16), 31 (1255), 33 to 36
(1250, 1251, 1253, 1254).

Unknown commands are skipped and reported as warnings; they never abort a
conversion.

### Not supported (yet)

Code 49, Planet, Postnet, Codablock, MicroPDF417, MSI, Plessey, GS1 DataBar,
UPC/EAN extensions, TLC39, Data Matrix ECC 000 to 140, QR model 1 (rendered as
model 2), Aztec runes, MaxiCode structured append, downloaded TrueType fonts,
`^MU` units other than dots, and serial number increments across copies.

## How close is it to a printer?

Zebra's firmware is not public, so [Labelary](https://labelary.com) serves as
the reference. Layout rules were measured on Labelary renderings (see
`tools/calibration`): font 0 advance widths for 212 characters, bitmap font
metrics and magnification rounding, rotation boxes of `^FO` and `^FT`, `^FB`
wrapping, Code 128 subset selection, interpretation line fonts and gaps, EAN
guard bars, the QR `^BY` height offset, PDF417 column selection, MaxiCode
geometry, `^PW` centering and more.

The golden test renders every fixture in `tests/fixtures` and compares it with
the Labelary image, tolerating one dot of offset:

| Fixture | Mismatch |
|---|---|
| graphics, graphic_fields (both labels) | 0.00 % |
| reverse (`^FR`, `^LR`, white boxes) | 0.26 % |
| barcodes_1d | 0.56 % |
| dhl_like | 0.64 % |
| template (`^DF`/`^XF`/`^FN`, two labels) | 0.60 to 0.80 % |
| ups_like (MaxiCode, Code 128, reverse field) | 0.78 % |
| fedex_like (PDF417, rotated text) | 0.82 % |
| tr_cargo_like (Turkish text, QR) | 3.9 % |
| barcodes_2d | 6.9 % |
| basic_text (every font, block justification) | 11.5 % |

Most of the remaining difference comes from the substitute fonts, whose glyph
shapes differ from Zebra's. Text still occupies exactly the same space because
glyphs are positioned with Zebra's metrics. For some 2D codes Zebra picks a
different but equally valid data encodation; symbol size, finder patterns and
the decoded content are the same. MaxiCode symbols for the UPS sample are
identical module for module.

`tools/check_pdf.py` renders the generated PDFs with PDFium and compares them
against the same references, which checks the PDF backend (fonts, blend
modes, image masks) as well.

## Fonts

Zebra's fonts are proprietary, so open fonts are embedded instead:

| Zebra font | Substitute | License |
|---|---|---|
| 0 (CG Triumvirate Bold Condensed) | Roboto Condensed Bold | SIL Open Font License 1.1 (`crates/zpl2pdf/fonts/RobotoCondensed-OFL.txt`) |
| A to H, P to V | DejaVu Sans Mono (Bold for B) | Bitstream Vera / DejaVu license (`crates/zpl2pdf/fonts/DejaVu-LICENSE.txt`) |

The font files are subsets (Latin, Greek, Cyrillic, punctuation and symbols)
of the upstream fonts. PDFs embed only the glyphs that are used.

## Development

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace            # unit, behaviour, property and golden tests
python tools/check_pdf.py         # optional: PDFium check, needs pypdfium2 and Pillow
```

Golden test output (our PNG, PDF and a diff image per fixture, red = only in
the reference, blue = only ours) is written to `target/golden/`.

To add a fixture, put a synthetic `.zpl` file into `tests/fixtures` and run
`python tools/fetch_references.py tests/fixtures/<name>.zpl`. This sends the
file to the public Labelary API, so never use real shipping data there.

The calibration scripts in `tools/calibration` reproduce the measurements;
`tools/gen_font0_metrics.py` regenerates the font 0 width table.

## Architecture

```
bytes ─▶ parser::Lexer ─▶ parser::parse ─▶ interp::Interpreter ─▶ scene::Label ─┬─▶ pdf::write_pdf
         (prefixes, ^GF/~DG    (typed       (printer and field state,          └─▶ raster::render
          payloads)            commands)     fonts, bar codes, ^DF/^XF)
```

The scene is a list of rectangles, paths, text runs and 1 bit bitmaps in
printer dots. Reverse printing is a paint mode: the raster backend XORs and
the PDF backend uses the Difference blend mode on a white page.

## License

MIT, see [LICENSE](LICENSE). The embedded fonts keep their own licenses (see
above); the MaxiCode module placement table comes from ZXing via rxing
(Apache License 2.0).
