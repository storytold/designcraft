# File format parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** major (first version: every format InDesign 2026 declares, measured against the code) · **Target:** Adobe InDesign 2026 (21.6)

Every format InDesign 2026 reads or writes, with DesignCraft's support, fidelity and tests. The
InDesign list comes from its bundle's `Info.plist` (`CFBundleDocumentTypes`,
`UTExportedTypeDeclarations`), its import/export filter folder names and its File menus; ours from
the open/place/export filters in `apps/designcraft/src/main.rs` and the `file.*` commands.
Overall numbers: [target-app-parity.md](target-app-parity.md); open problems: [gaps.md](gaps.md).

**Dimension score: ~40% ready (estimated), 150–250 h + 20–60 h for INDD integration.** The
single largest gap is that `.indd` can't be opened.

Read/Write: **✓** supported · **~** partial · **✗** missing · **—** InDesign doesn't either.
Weight: what share of an InDesign professional's file traffic the format carries (used for the
score below).

## Documents and interchange

| Format | Ext | InDesign | DesignCraft read | DesignCraft write | Fidelity / tests | Weight |
|---|---|---|---|---|---|---|
| InDesign document | `.indd` | read/write (native) | ✗ (#21; converters proposed in #98, #114) | ✗ | — | 30% |
| InDesign template | `.indt` | read/write | ✗ | ✗ | — | 2% |
| InDesign Markup | `.idml` | read/write | ✓ | ✓ | Synthetic fixtures in `crates/idml/src/tests.rs` (49 tests); exported files verified by opening in InDesign 2026 by hand; indd-utils' 98-document run imports 811 spreads / 2,866 stories / 5,812 items. Open: #193, #338, #339, #340, #211. No real-file corpus test | 20% |
| DesignCraft native | `.designcraft` | — | ✓ | ✓ | zip + JSON, versioned; round-trip tests | — |
| InDesign book | `.indb` | read/write | ✗ (own `.dcbook`) | ✗ | Books are shallow (see gaps.md) | 2% |
| InDesign library | `.indl`, `.inlx` | read/write | ✗ (own `.dclib`) | ✗ | | 1% |
| Snippet | `.idms` | read/write | ✗ (own snippets) | ✗ | | 1% |
| InCopy story / assignment | `.icml`, `.icma`, `.incx`, `.inca` | read/write | ✗ | ✗ | InCopy workflow out of scope; ICML story import would still help | 1% |
| InDesign Interchange (legacy) | `.inx` | read | ✗ | — | | <1% |
| PageMaker | `.pmd`, `.p65`, `.t65`, `.pmt` | read | ✗ | — | | <1% |
| QuarkXPress | `.qxd`, `.qxt` | read (old versions) | ✗ (#73) | — | | <1% |

## Output

| Format | Ext | InDesign | DesignCraft | Fidelity / tests | Weight |
|---|---|---|---|---|---|
| PDF (print) | `.pdf` | ✓ all presets, PDF/X-1a/3/4, PDF/A, PDF/UA | ✓ krilla: real text with subset fonts, DeviceCMYK/RGB, spot Separations, bleed/slug, marks, spreads, compression options, remembered settings (#196); **PDF/X-4** with output intent and a validator (#223, #200); **PDF/A-2b**; tagged PDF | 53 tests in `crates/pdf`; no PDF/X-1a or X-3 (needs flattening to PDF 1.3/1.4); no presets / `.joboptions`; no external validator (veraPDF / callas) in CI | 18% |
| PDF (interactive) | `.pdf` | ✓ | ✓ links, bookmarks, buttons, AcroForm fields, page transitions, object states | | 2% |
| JPEG / PNG | `.jpg`, `.png` | ✓ | PNG from the UI; JPEG from the CLI only | no resolution / colour space options dialog for JPEG | 3% |
| EPUB reflowable | `.epub` | ✓ | ✓ stories in article order, CSS from styles, images, nav | | 2% |
| EPUB fixed layout | `.epub` | ✓ (live HTML/CSS) | ~ page images with text kept for search | not live text | 1% |
| HTML | `.html` | ✓ | ~ one self-contained page (#314) | | 1% |
| HTML5 ZIP / Publish Online | | ✓ | ✗ (Publish Online out of scope) | | <1% |
| SVG export | `.svg` | ✓ (via plug-in) | ✗ | | <1% |
| EPS export | `.eps` | ✓ | ✗ | | <1% |
| Text Only / RTF / Tagged Text | `.txt`, `.rtf` | ✓ | ✓ (`file.exportText` txt, rtf, tagged) | | 1% |
| XML | `.xml` | ✓ | ✓ export and import, DTD validate | depth unmeasured | 1% |
| Print / PostScript | | ✓ printers, PPDs, separations | ~ PDF to `lpr` (no Windows, no PostScript) | | 3% |
| Package | folder | ✓ fonts, links, IDML, PDF, report | ✓ (#340: IDML inside links to absolute paths) | | 2% |

## Placed and imported content

| Format | Ext | InDesign | DesignCraft | Notes | Weight |
|---|---|---|---|---|---|
| PDF / AI (PDF-compatible) | `.pdf`, `.ai` | ✓ page boxes, layers, pages | ✓ rendered with hayro, embedded as vector in PDF export, crop to page boxes | layer visibility depth; editable vectors not offered (#227) | 3% |
| JPEG, PNG, GIF, BMP, WebP | | ✓ | ✓ | CMYK JPEG/TIFF keep inks in PDF (#35) | 2% |
| TIFF | `.tif` | ✓ incl. CMYK, alpha, paths | ✓ incl. CMYK | clipping paths from 8BIM resources unverified | 1% |
| PSD | `.psd` | ✓ layers, comps, alpha, paths | ~ composite + Object Layer Options | layer comps partial | 1% |
| EPS | `.eps` | ✓ (PostScript) | ~ preview only (no interpreter) | | <1% |
| SVG | `.svg` | ✓ | ✓ (usvg) | | <1% |
| HEIC | `.heic` | ✓ | ✗ | | <1% |
| Word | `.docx` | ✓ styles, footnotes, tables, track changes, options | ✓ styles by name, footnotes, tables, tabs, breaks, moved text (#257), style mapping and conflicts | `.doc` not supported (InDesign reads it) | 2% |
| RTF | `.rtf` | ✓ | ✓ | | <1% |
| Excel | `.xlsx`, `.xls` | ✓ | ~ `.xlsx` first sheet's used range | no sheet/range options; `.xls` ✗ | <1% |
| Plain text, Markdown | `.txt`, `.md` | ✓ (txt) | ✓ | | <1% |
| InDesign Tagged Text | `.txt` | ✓ | ✓ import and export | | <1% |
| CSV / TSV (Data Merge) | | ✓ | ✓ (+ Excel) | | <1% |
| Audio / video | `.mp4`, `.mp3`, … | ✓ placeholders for EPUB/PDF | ~ placed as media objects | no in-app playback | <1% |

## Resources and settings

| Format | Ext | InDesign | DesignCraft |
|---|---|---|---|
| Adobe Swatch Exchange | `.ase` | read/write | ✓ load and save |
| Colour books | `.acb` | read (17 libraries ship) | ✗ (licensed libraries; Pantone etc. out of scope, own libraries possible) |
| ICC profiles | `.icc`, `.icm` | read | ✓ |
| Colour settings | `.csf` | read/write | ✗ |
| PDF presets | `.joboptions`, `.pdfs` | read/write | ✗ |
| Print / flattener / document presets | `.prst`, `.flst`, `.dcst` | read/write | ✗ |
| Preflight profiles | `.idpp` | read/write | ✗ (own profiles in the document) |
| Keyboard shortcut sets | `.indk` | read/write | ✗ (own JSON) |
| User dictionary | `.udc` | read/write | ✗ (own, in the document and prefs) |
| Fonts | OTF, TTF, TTC, variable | ✓ | ✓ (Type 1 retired by both) |

## Score

Weighted by the Weight column (documents, output and placed tables; resources unweighted),
counting ✓ = 1, ~ = 0.5, and IDML at 0.6 until a real-file corpus proves it: **~45% present**.
Taking fidelity into account (no INDD, IDML unproven, PDF/X-1a and presets missing), **~40% ready
for real work** (estimated). INDD alone moves this dimension by up to 30 points.

## Remaining effort

| Work | Opus 5.5 h |
|---|---|
| INDD import via a clean-room converter (owner decision, #114) | 20–60 |
| IDML real-file corpus, metric, fixes (#193, #338, #339, #340) | 60–100 |
| PDF/X-1a and X-3 (transparency flattener), PDF presets and `.joboptions`, external validator | 40–70 |
| Print path (Windows, PostScript level 3 output or system print APIs) | 25–45 |
| ICML story import, IDMS snippets, INDT templates (once INDD reads) | 15–25 |
| HTML / fixed-layout EPUB with live text | 20–35 |
| JPEG export dialog, SVG export, EPS export | 10–20 |
| **Total** | **190–355** (150–250 excluding INDD and PostScript output) |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | major | Created from InDesign 2026's declared document types and filters, `plan/indesign/08-file-formats.md` and the code |
