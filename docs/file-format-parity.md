# File format parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-24 · **Change:** minor (PDF/X-4 visible unembedded font detection, #909; linked-only working saves, portable-save compatibility and PDF crop-box identity, #856; IDML liquid guide types, #872; .indd and .indt open through indd-utils; XLSX cached values preserve numeric precision and text; shared print spooler; Windows CI passes; content-indexed asset deduplication, #727) · **Target:** Adobe InDesign 2026 (21.6)

Every format InDesign 2026 reads or writes, with DesignCraft's support, fidelity and tests. The
InDesign list comes from its bundle's `Info.plist` (`CFBundleDocumentTypes`,
`UTExportedTypeDeclarations`), its import/export filter folder names and its File menus; ours from
the open/place/export filters in `apps/designcraft/src/main.rs` and the `file.*` commands.
Overall numbers: [target-app-parity.md](target-app-parity.md); open problems: [gaps.md](gaps.md).

**Dimension score: ~40% ready (estimated), 150–250 h + 10–30 h for an INDD corpus.** The
largest gaps are that neither `.indd` opening nor IDML is measured on real files.

Read/Write: **✓** supported · **~** partial · **✗** missing · **—** InDesign doesn't either.
Weight: what share of an InDesign professional's file traffic the format carries (used for the
score below).

## Documents and interchange

| Format | Ext | InDesign | DesignCraft read | DesignCraft write | Fidelity / tests | Weight |
|---|---|---|---|---|---|---|
| InDesign document | `.indd` | read/write (native) | ✓ (converted to IDML in memory by [storytold/indd-utils](https://github.com/storytold/indd-utils), then the IDML importer; #21, #114) | ✗ | Converter warnings join the open result; unreadable files are open errors. Synthetic-bytes tests only; indd-utils reports 96.9% of IDML values reproduced on ~650 pairs. No real-file corpus test | 30% |
| InDesign template | `.indt` | read/write | ✓ (as `.indd`) | ✗ | As `.indd` | 2% |
| InDesign Markup | `.idml` | read/write | ✓ | ✓ | Synthetic fixtures in `crates/idml/src/tests.rs` (51 tests); exported files verified by opening in InDesign 2026 by hand; indd-utils' 98-document run imports 811 spreads / 2,866 stories / 5,812 items. Open: #193, #338, #339, #340, #211. No real-file corpus test | 20% |
| DesignCraft native | `.designcraft` | — | ✓ | ✓ | zip + JSON, versioned; round-trip tests. Asset loading and compaction index by content hash, then confirm byte equality; equal-sized distinct files fall into separate search buckets (#727). Separate links, MIME types and PDF pages retain their identity | — |
| InDesign book | `.indb` | read/write | ✗ (own `.dcbook`) | ✗ | Books are shallow (see gaps.md) | 2% |
| InDesign library | `.indl`, `.inlx` | read/write | ✗ (own `.dclib`) | ✗ | | 1% |
| Snippet | `.idms` | read/write | ✗ (own snippets) | ✗ | | 1% |
| InCopy story / assignment | `.icml`, `.icma`, `.incx`, `.inca` | read/write | ✗ | ✗ | InCopy workflow out of scope; ICML story import would still help | 1% |
| InDesign Interchange (legacy) | `.inx` | read | ✗ | — | | <1% |
| PageMaker | `.pmd`, `.p65`, `.t65`, `.pmt` | read | ✗ | — | | <1% |
| QuarkXPress | `.qxd`, `.qxt` | read (old versions) | ✗ (#73) | — | | <1% |

IDML preserves stored page liquid rules and distinguishes liquid guides from ordinary ruler
guides on document and parent pages. Guide positions, orientation, Fit to Page, lock state and
layer survive interchange; literal XML and round-trip tests are in
`crates/idml/src/tests/liquid_guides.rs`. Object liquid pins and resize constraints remain unmapped
(#872); this is not full liquid-layout interchange support.

## Output

| Format | Ext | InDesign | DesignCraft | Fidelity / tests | Weight |
|---|---|---|---|---|---|
| PDF (print) | `.pdf` | ✓ all presets, PDF/X-1a/3/4, PDF/A, PDF/UA | ✓ krilla: real text with subset fonts, DeviceCMYK/RGB, spot Separations, bleed/slug, marks, spreads, compression options, remembered settings (#196); **PDF/X-4** with output intent and a validator (#223, #200); **PDF/A-2b**; tagged PDF | 53 tests in `crates/pdf`; no PDF/X-1a or X-3 (needs flattening to PDF 1.3/1.4); no presets / `.joboptions`; no external validator (veraPDF / callas) in CI | 18% |
| PDF (interactive) | `.pdf` | ✓ | ✓ links, bookmarks, buttons, AcroForm fields, page transitions, object states | | 2% |
| JPEG / PNG | `.jpg`, `.png` | ✓ | PNG from the UI; JPEG from the CLI only | no resolution / colour space options dialog for JPEG | 3% |
| EPUB reflowable | `.epub` | ✓ | ✓ stories in article order, tables, footnotes and endnotes, CSS from styles, images, nav | | 2% |
| EPUB fixed layout | `.epub` | ✓ (live HTML/CSS) | ~ page images with text kept for search | not live text | 1% |
| HTML | `.html` | ✓ | ~ one self-contained page (#314) | | 1% |
| HTML5 ZIP / Publish Online | | ✓ | ✗ (Publish Online out of scope) | | <1% |
| SVG export | `.svg` | ✓ (via plug-in) | ✗ | | <1% |
| EPS export | `.eps` | ✓ | ✗ | | <1% |
| Text Only / RTF / Tagged Text | `.txt`, `.rtf` | ✓ | ✓ (`file.exportText` txt, rtf, tagged) | | 1% |
| XML | `.xml` | ✓ | ✓ export and import, DTD validate | depth unmeasured | 1% |
| Print / PostScript | | ✓ printers, PPDs, separations | ~ PDF to CUPS/Windows (craft-print; no PostScript) | | 3% |
| Package | folder | ✓ fonts, links, IDML, PDF, report | ✓ (#340: IDML inside links to absolute paths) | | 2% |

PDF/X-4 export warns about unembedded fonts that draw visible text, including inside placed PDF
forms; `designcraft-cli validate-pdf FILE.pdf` reports the same issues, with font names and pages
(#909). Invisible text (render mode 3) is exempt from this font-embedding check. Detection does not
automatically embed or outline fonts; embedding, substitution or outlining of placed-PDF fonts
remains missing. A document-wide operation limit and a form-nesting depth limit report incomplete
font checks rather than traversing pathological nesting. These built-in checks are not complete
external PDF/X certification.

## Placed and imported content

| Format | Ext | InDesign | DesignCraft | Notes | Weight |
|---|---|---|---|---|---|
| PDF / AI (PDF-compatible) | `.pdf`, `.ai` | ✓ page boxes, layers, pages | ✓ rendered with hayro, embedded as vector in PDF export, crop to page boxes | layer visibility depth; editable vectors not offered (#227) | 3% |
| JPEG, PNG, GIF, BMP, WebP | | ✓ | ✓ embedded ICC profiles applied on screen; CMYK JPEGs (incl. YCCK) through their own profile or the working CMYK space | CMYK JPEG/TIFF keep inks in PDF (#35) | 2% |
| TIFF | `.tif` | ✓ incl. CMYK, alpha, indexed colour, paths | ✓ incl. CMYK, embedded ICC profiles on screen | clipping paths from 8BIM resources unverified | 1% |
| PSD | `.psd` | ✓ layers, comps, alpha, paths | ~ composite + Object Layer Options; embedded ICC profile and managed CMYK on screen | layer comps partial | 1% |
| EPS | `.eps` | ✓ (PostScript) | ~ preview only (no interpreter) | | <1% |
| SVG | `.svg` | ✓ | ✓ (usvg) | | <1% |
| HEIC | `.heic` | ✓ | ✗ | | <1% |
| Word | `.docx` | ✓ styles, footnotes, tables, track changes, options | ✓ styles by name, footnotes, tables, tabs, breaks, moved text (#257), bullets and numbering from `numbering.xml` (#626; levels become list levels, parent numbers in a label level references), style mapping and conflicts | `.doc` not supported (InDesign reads it) | 2% |
| RTF | `.rtf` | ✓ | ✓ | | <1% |
| Excel | `.xlsx`, `.xls` | ✓ | ~ `.xlsx` first sheet's used range | cached numbers keep their precision and formula strings stay text; no sheet/range options; `.xls` ✗ | <1% |
| Plain text, Markdown | `.txt`, `.md` | ✓ (txt) | ✓ | | <1% |
| InDesign Tagged Text | `.txt` | ✓ | ✓ import and export | | <1% |
| CSV / TSV (Data Merge) | | ✓ | ✓ (+ Excel) | | <1% |
| Audio / video | `.mp4`, `.mp3`, … | ✓ placeholders for EPUB/PDF | ~ placed as media objects | no in-app playback | <1% |

## Resources and settings

| Format | Ext | InDesign | DesignCraft |
|---|---|---|---|
| Adobe Swatch Exchange | `.ase` | read/write | ✓ load and save |
| Colour books | `.acb` | read (17 libraries ship) | ✗ (licensed libraries; Pantone etc. out of scope, own libraries possible) |
| ICC profiles | `.icc`, `.icm` | read | ✓ (also profiles embedded in placed images) |
| Colour settings | `.csf` | read/write | ✗ |
| PDF presets | `.joboptions`, `.pdfs` | read/write | ✗ |
| Print / flattener / document presets | `.prst`, `.flst`, `.dcst` | read/write | ✗ |
| Preflight profiles | `.idpp` | read/write | ✗ (own profiles in the document) |
| Keyboard shortcut sets | `.indk`; Show Set `.txt` | read/write; write | ~ (imports a Show Set `.txt`; own JSON) |
| User dictionary | `.udc` | read/write | ✗ (own, in the document and prefs) |
| Fonts | OTF, TTF, TTC, variable | ✓ | ✓ (Type 1 retired by both) |

## Score

Weighted by the Weight column (documents, output and placed tables; resources unweighted),
counting ✓ = 1, ~ = 0.5, IDML at 0.6 and INDD at 0 until a real-file corpus measures them:
**~45% present**. Taking fidelity into account (INDD and IDML unmeasured, PDF/X-1a and presets
missing), **~40% ready for real work** (estimated). A measured INDD corpus moves this dimension by
up to 30 points.

## Remaining effort

| Work | Opus 5.5 h |
|---|---|
| INDD real-file corpus, metric, fixes (#21, #114) | 10–30 |
| IDML real-file corpus, metric, fixes (#193, #338, #339, #340) | 60–100 |
| PDF/X-1a and X-3 (transparency flattener), PDF presets and `.joboptions`, external validator | 40–70 |
| Print fidelity, PostScript level 3 output and print presets | 25–45 |
| ICML story import, IDMS snippets | 15–25 |
| HTML / fixed-layout EPUB with live text | 20–35 |
| JPEG export dialog, SVG export, EPS export | 10–20 |
| **Total** | **180–325** (150–250 excluding INDD and PostScript output) |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-20 | minor | Linked-only native working saves and legacy baseline compatibility (#856); only archives omitting linked originals need format 4, portable saves retain embedded compatibility, and distinct PDF crop boxes stay separate during compaction |
| 2026-10-19 | minor | PDF/X-4 export and `validate-pdf` report visible unembedded fonts by name and page, including placed forms; invisible text is exempt, operation- or depth-limited checks report incompleteness, and automatic font repair remains missing (#909) |
| 2026-10-15 | minor | IDML retains liquid versus ruler guide types on document and parent pages, with literal XML and mixed-guide round-trip coverage; object liquid constraints remain unsupported (#872) |
| 2026-10-13 | minor | Asset loading and compaction use content fingerprints with exact equality checks; tests cover equal-sized distinct buffers, shared copies, independent links and missing data (#727) |
| 2026-10-13 | minor | `.indd` and `.indt` read through storytold/indd-utils; INDD remaining work is a real-file corpus (10–30 h); score unchanged until it is measured |
| 2026-10-12 | minor | XLSX table placement and data merge preserve small numeric values and cached formula strings; synthetic regressions in `textimport::xlsx::tests`. Scores unchanged. |
| 2026-10-10 | major | Created from InDesign 2026's declared document types and filters, `plan/indesign/08-file-formats.md` and the code |

## Linked native artwork (2026-10-20)

Native working files can keep linked originals external through the Links panel or
`file.save {embedImages:false}`. Embedded storage remains the default for existing documents;
recovery and byte serialization retain originals. See [linked-artwork.md](linked-artwork.md).
This adds external storage; cached editing data is still loaded in memory, and missing links
do not yet have saved previews.

Native format 1 text frames now preserve ascent-derived first-baseline offsets. Files retaining
these compatibility settings use native format 5 so earlier readers reject them safely.
