# Where DesignCraft falls short of InDesign

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-24 · **Change:** minor (placed graphic layer-option preservation and explicit rendering warnings; gap 16 shrunk: File › File Info; gap 16: Convert Variable to Text; gap 28 added: placed-PDF font repair remains after visible unembedded font detection, #909; gap 26 shrunk: autoflow fills the following pages before it adds pages at the end; gap 6 shrunk: Type › Reload Fonts reads fonts installed, edited or removed while running (#328); gap 3: Package links are relative, #340; gap 27 closed: the loaded text cursor threads into a frame inside a group (#1075); .indd and .indt open through indd-utils; gap 0 shrunk: text threads by hand; threading gaps added; Optical kerning from outlines (#181); gap 6: macOS lists fonts that font services and managers activate; shared print spooler; image clipping paths from IDML; Tabs panel listed as present (#343); book chapter update safety; gap 16: Based on Parent leftovers; bounded control click/drag input; swatches on placed greyscale and bitmap images) · **Target:** Adobe InDesign 2026 (21.6)

Every known shortfall against InDesign 2026, one entry each, ranked by how much it stops an
InDesign professional from doing real work. This is the work list: agents pick from the top.
Numbers and weights are in [target-app-parity.md](target-app-parity.md); the plan is in
[roadmap.md](roadmap.md). Estimates are Opus 5.5 agent-hours, calibrated in
target-app-parity.md. When you close a gap, delete its entry (or shrink it), add a line to the
ROADMAP progress log and update the timestamps.

Kind: **F** feature · **U** UI/UX · **T** typography · **FF** file format · **H** hardware ·
**L** localization · **P** performance · **S** stability · **E** ecosystem · **A** AI.

## Alpha blocker (the core-workflow gate in [roadmap.md](roadmap.md#alpha-gate))

### 0. Primary Text Frame autoflow adds pages users can't delete (F) · **ALPHA BLOCKER**
- **Missing:** fixes to Smart Text Reflow on the Primary Text Frame, which users report adding
  pages they can't delete (#165). Threading by hand works (ports, loaded text cursor, `text.thread` / `text.unthread` /
  `text.removeFromThread`).
- **Evidence:** #165.
- **Impact:** a long story in a Primary Text Frame document can leave pages the designer can't
  remove.
- **Estimate:** 5–7 h.
- **Doc:** [ui-parity.md](ui-parity.md)

## Blocking (beta can't be declared while these are open)

### 1. Basic editing fails for many users (U, S)
- **Missing:** object copy/paste on Windows and Linux (#351, #348, #312, #185, #164, #201, text RTF
  paste #212); Paste in Place onto the selected page (#348); right-click context menus that don't
  open (#238, #187, #160, #312); double-click word selection in fields
  (#189); character formatting set at a caret is discarded (#289).
- **Evidence:** 16 issues from 2026-10-05 to 2026-10-10, mostly against the 0.4/0.5 release builds.
- **Safety:** control-channel clicks and drags reject invalid or over-budget counts before
  queuing input (#732), including combined drag steps, repeated clicks and already pending events.
- **Impact:** a layout artist can't finish the simplest job. This dominates the
  ready-for-real-work number.
- **Estimate:** 30–50 h (reproduce per platform through the control channel, regression tests).
- **Doc:** [ui-parity.md](ui-parity.md)

### 2. INDD opening isn't measured on real files (FF)
- **Missing:** a real-file corpus test of opening `.indd` and `.indt`. They open through
  [storytold/indd-utils](https://github.com/storytold/indd-utils), which converts them to IDML in
  memory for the IDML importer, but DesignCraft's own tests use synthetic bytes only. INDB and
  INDL are still not read.
- **Evidence:** #21, #114 (PR #98 proposed another converter). indd-utils reports 96.9% of
  InDesign's IDML values reproduced on ~650 INDD/IDML pairs and 0 failures on ~4,300 files; what
  DesignCraft draws from those files is unmeasured, and it inherits the IDML importer's open bugs
  (gap 3).
- **Impact:** a client's `.indd` opens, but nobody knows how close it comes to the original.
- **Estimate:** 10–30 h (corpus pinned in a sibling of `storytold/photocraft-corpus`, metric,
  fixes in indd-utils or the IDML importer).
- **Doc:** [file-format-parity.md](file-format-parity.md)

### 3. IDML round-trip isn't proven on real files (FF)
- **Missing:** a corpus test of real InDesign-authored IDML (via `storytold/photocraft-corpus` or
  a sibling) measuring what survives import → export → reopen in InDesign. Open bugs: frames not
  rendered without a warning (#193), `$ID/` font names reported missing (#338), no
  `PDFAttribute` on export (#339), drag-drop open of IDML on the app icon (#211).
- **Evidence:** importer coverage is broad (styles, tables, footnotes, xrefs, index, CJK, Arabic,
  verified in InDesign 2026 with synthetic files), but nothing is measured on real documents.
- **Impact:** IDML is the main interchange format until INDD lands, and the only one that goes
  back to InDesign.
- **Estimate:** 60–100 h (corpus, metric, fixes).
- **Doc:** [file-format-parity.md](file-format-parity.md)

### 4. Composition isn't compared with InDesign (T)
- **Missing:** an oracle harness: the same synthetic stories set by InDesign 2026 (observed
  black-box, results kept under `plan/`) and by DesignCraft, comparing line breaks, overset and
  frame fit. Known differences: optical kerning is an outline-based approximation, not Adobe's
  algorithm (#181); a frame goes overset when descenders don't fit, InDesign needs only the baseline
  (#180); setting the text language was missing (#130; Language is now in Properties, #145, the issue is still open); "hyphenation doesn't work"
  (#87).
- **Impact:** an opened IDML reflows: different line breaks, different page count, overset text.
  Professionals notice on the first page.
- **Estimate:** 70–125 h (harness 20–30, optical kerning tuning 5–10, then fixes by cluster).
- **Doc:** [typography-parity.md](typography-parity.md)

### 5. Startup crashes on some graphics hardware (S, H)
- **Missing:** a start that survives every GPU/driver: Metal device lost on Intel Macs (#334),
  AMD OpenGL driver crash on Windows (#167), launch failures (#273, #220, #192, #166), blank or
  frozen canvas after graphics loss (#90).
- **Evidence:** a backend fallback landed 2026-10-10 (#297: DirectX 12 on Windows, `gpu.json`
  skip list); then VectorCraft's adapter choice (the GPU that drives the display), restart on the
  next adapter or backend when one fails or hangs at start-up, the indirect-call check off (the
  first Metal error of #334) and a logged reason when the app stops (#273). Not yet confirmed by
  the reporters. Still open for #334: wgpu asks an old macOS's Metal compiler for a language
  version it doesn't have (`-std=metal3.2`).
- **Estimate:** 15–30 h.
- **Doc:** [hardware-parity.md](hardware-parity.md)

### 6. Installed fonts not always found (F)
- **Missing:** fonts in user font folders used for text (#161, #110, #107), fonts that font
  services and managers activate on Windows, noticing by itself when fonts change on disk (#328:
  Type › Reload Fonts, `font.reload`, reads them again on request since 2026-10-16).
- **Evidence:** on macOS the scan reads the font files CoreText has available, so fonts that
  Adobe Fonts and font managers activate are listed (#261, #327, 2026-10-11); not yet confirmed
  by the reporters.
- **Impact:** documents set in the client's fonts render in Source Sans 3: wrong everywhere.
- **Estimate:** 10–20 h (VectorCraft has a port, #261).
- **Doc:** [typography-parity.md](typography-parity.md)

### 7. No CI runs the tests on pull requests (S)
- **Missing:** a PR workflow running `cargo xtask ci` (fmt, clippy, tests, layers, wasm). Today
  only FreeBSD and Windows ARM build checks run, and only when manifests change; `main` has no
  branch protection. 65 PRs are open, some integrated in bulk (#259).
- **Impact:** regressions land unseen; the 1,122 tests protect nothing they aren't run on.
- **Estimate:** 4–8 h (plus runner minutes; owner decision on required checks).

## High

### 8. Canvas IME not checked with real input methods (U, L)
- **Have:** IME composition in the Type tool on macOS, Windows, Linux and the web (#322).
- **Missing:** a check by hand with a system IME on each platform, and the fixes it finds.
- **Estimate:** 10–20 h. **Doc:** [localization-parity.md](localization-parity.md)

### 9. PDF output presets and standards (FF)
- **Missing:** PDF/X-3, Adobe PDF Presets and `.joboptions` load/save, output-intent choice
  beyond the built-in profile, PDF/UA validation; soft effects other than the drop shadow reach
  the PDF/X-1a flattener as RGB images; #159, #32.
- **Have:** PDF/X-4 with output intent and a validator (`#200`), PDF/X-1a:2003 through
  craft-prepress's flattener with its own checks, PDF/A-2b, tagged PDF, CMYK and spot, marks and
  bleed, the options dialog with remembered settings (#196).
- **Estimate:** 30–60 h. **Doc:** [file-format-parity.md](file-format-parity.md)

### 28. Placed-PDF font repair for PDF/X-4 (FF)
- **Missing:** embedding, substitution or outlining of unembedded fonts in placed PDFs; export
  does not automatically embed or outline them.
- **Have:** PDF/X-4 export warnings and `validate-pdf` report unembedded fonts drawing visible
  text, including inside placed PDF forms, with font names and pages (#909). Invisible text
  (render mode 3) is exempt from this font-embedding check. A document-wide operation limit and a
  form-nesting depth limit report incomplete font checks rather than traversing pathological
  nesting.
- **Impact:** detection now ships, but a placed PDF with visible unembedded fonts still needs
  font repair before it can meet PDF/X-4 requirements.
- **Doc:** [file-format-parity.md](file-format-parity.md)

### 10. Printing (H, F)
- **Missing:** PostScript, a printer-driver options UI, separations to device, print presets, or trapping.
- **Have:** `file.print` exports selected pages/spreads with marks and bleed, then submits through
  craft-print: CUPS on macOS/Linux and the Windows PDF raster spooler. Physical printer fidelity
  remains a hardware check.
- **Estimate:** 25–45 h. **Doc:** [hardware-parity.md](hardware-parity.md)

### 11. Paragraph and character style editing reliability (F)
- **Missing:** saving edits and deleting a paragraph style reported failing (#169).
- **Have:** the Tabs panel (Type › Tabs: a ruler over the text column, alignment, leader and
  Align On) and a Tabs section in Paragraph Style Options (#343).
- **Estimate:** 10–20 h. **Doc:** [ui-parity.md](ui-parity.md)

### 12. Tables in depth (F)
- **Missing:** anchored objects in cells not drawn (#163); Table ▸ Table Options and Cell Options
  submenus as InDesign lays them out, Diagonal Lines, Edit Header/Footer,
  Go to Row; general reliability (#20).
- **Estimate:** 25–40 h.

### 13. Hyphenation and spelling languages (T, L)
- **Missing:** hyphenation patterns for every language but English, Spanish, Portuguese, Norwegian and Greek; spelling for
  every language but English. InDesign ships dictionaries for ~40 languages.
- **Estimate:** 30–60 h (permissively licensed patterns exist for most; spelling word lists need
  licence review). **Doc:** [typography-parity.md](typography-parity.md)

### 14. Windows and Linux are thinly tested (S)
- **Missing:** runtime testing on Windows and Linux: installer confusion (#64), pull-down panels
  can't scroll on Linux Mint (#267), tab bar overlaps menus in some workspaces (#182), CJK UI
  font missing on Linux (#332).
- **Estimate:** 30–60 h (smoke tests in CI, fixes).

## Medium

### 15. Long documents in depth (F)
- **Missing:** book-wide TOC and index; Table of Contents Styles and deeper production workflows.
- **Present:** basic style synchronization, continuous book numbering and merged PDF export.
  Chapter updates prepare the full batch and write only closed native chapters; IDML is a
  read-only source. Each file is atomic, but a later I/O failure can leave completed chapters;
  the error lists them. Cross-file transactional saving remains unavailable.
- **Estimate:** 30–50 h.

### 16. Menu items still missing (U)
- **Missing:** 149 of 482 in-scope items of InDesign 21.6's menu dump have no match (measured 2026-10-10; some exist under other labels): Type ▸ Notes
- **Missing:** 150 of 482 in-scope items of InDesign 21.6's menu dump have no match (measured 2026-10-10; some exist under other labels): Type ▸ Notes
  and Track Changes submenus (the panels and commands exist), hyperlink and cross-reference
  update commands, Convert Bullets/Numbering to Text, Convert Variable to Text, Object ▸ Clear
  Effects / Clear All Transparency / Rotate 180° / Reverse Path,
  update commands, Convert Bullets/Numbering to Text, Object ▸ Clear
  Effects / Clear All Transparency / Rotate 180° / Reverse Path, Layout ▸ Go Back/Forward,
  Window ▸ Arrange (2-up … 6-up), Trap Presets, Overlays, Edit ▸ Assign Profiles / Convert to
  Profile / Edit Original, File ▸ Open PDF / Document and Print Presets / User
  Settings, Type ▸ Size presets, Table ▸ Table Options / Cell Options submenus, Paste Before/After,
  Convert Columns, Object ▸ Interactive ▸ Convert to (form fields),
  View ▸ Extras badges and Story Editor options (Show/Hide Depth Ruler is there). Pages panel ▸ Parent Options (Based on Parent is
  the `layout.parents.options` command only); overriding a base parent's items on a parent spread
  itself (overrides work on document pages only).
- **Estimate:** 40–70 h (most are thin wrappers over existing engine features).
- **Doc:** [ui-parity.md](ui-parity.md)

### 17. HTML and fixed-layout EPUB (FF)
- **Missing:** HTML export is one basic page (#314); fixed-layout EPUB writes page images, not
  live text and CSS; no animation or timing for EPUB/HTML.
- **Estimate:** 30–50 h.

### 18. Colour management depth (F)
- **Missing:** Assign Profiles / Convert to Profile, per-image rendering intents in output,
  hex entry and colour picker issues (#323, #190), keep kerning on colour change (#29).
- **Estimate:** 20–35 h.

### 19. Placed graphics depth (FF)
- **Have:** per-placement IDML graphic-layer settings survive IDML/native round trips; native format 4 guards older readers. Unsupported layer visibility is reported by import, preflight and PDF export. See [graphic-layer-options.md](graphic-layer-options.md).
- **Still missing:** DesignCraft does not apply retained IDML graphic-layer visibility to raster or PDF output; layer-comp choices and the layer-options UI are also missing. Metadata preservation does not establish visual fidelity. Placed-PDF layer choices are applied (see **Layer choices** below).
- **Safety:** PDF visibility variants preserve catalog/configuration entries and nonzero-generation
  references, redefining the existing catalog rather than choosing an arbitrary object number.
  Encrypted, hybrid-reference and over-budget rewrites are rejected; no print-fidelity claim.
- **Layer choices:** placed PDFs distinguish explicit all-on visibility from file defaults;
  choices survive native saves, raster/PDF output and embedded IDML export for the placements the
  export walks reach: document and parent spreads, groups, and objects anchored in a story. A
  placement anchored inside table-cell or footnote/endnote text is also prepared for PDF/A-2b.
  Embedded IDML applies variants inside table-cell and footnote text; endnote variants are prepared,
  but IDML endnote structures/artwork are not serialized yet. IDML layer variants are embedded
  copies; external-only export is rejected when choices would be lost.
- **Missing:** EPS is shown through its preview only (no PostScript interpreter); PSD layer
  comps beyond visibility; the Clipping Path Options dialog (tracing into a clip that keeps the
  frame, Threshold/Tolerance/Inset Frame preview, picking an alpha channel or a path saved in a
  TIFF/PSD); Text Wrap › Same As Clipping following an imported clipping path; Edit Original / Edit With; PDF placed as editable vectors (#227, #24);
  QuarkXPress import (#73); applying a swatch to a selected greyscale or bitmap image (Direct
  Selection + Swatches). A swatch already on the image (IDML `<Image FillColor>`) colours it on
  screen and in PDF and IDML export (#1094).
- **Estimate:** 30–60 h (PostScript interpreter is a large separate project: +80–150 h).

### 20. Performance on large documents (P)
- **Missing:** incremental composition (recompose from the edited paragraph; 8 ms keystroke
  budget), benchmarks against InDesign on long books and image-heavy catalogues.
- **Have:** compose 300k characters ~38 ms warm, damage-region repaint (< 1 ms while typing),
  parallel story composition.
- **Estimate:** 40–80 h.

### 26. Autoflow flows into the primary text frame of the pages that follow (F)
- **Missing:** autoflow threads a frame per column on the pages that follow, then adds pages at
  the end of the document, as InDesign does. On a page whose parent has a primary text frame it
  still sets a new frame instead of flowing into the primary one.
- **Estimate:** 2–4 h.

## Lower

### 21. Localization breadth (L)
- **Missing:** Hindi, Indonesian, Korean, Vietnamese interfaces; ~17% of strings in the 83–85%
  languages; 39% of pt-BR; native-speaker review for all. **Estimate:** 120–180 h.
  **Doc:** [localization-parity.md](localization-parity.md)

### 22. InDesign scripting compatibility (E)
- **Missing:** an ExtendScript / UXP-compatible DOM so existing InDesign scripts run. Our own
  command API, scripts panel, CLI and MCP are strong and agent-friendly, but not compatible.
- **Estimate:** 150–250 h for a useful DOM subset; **owner decision**.

### 23. Plug-ins (E)
- **Missing:** any plug-in model. InDesign's third-party ecosystem (imposition, preflight, data
  publishing) doesn't carry over and can't. **Owner decision**; not estimated.

### 24. AI features (A)
- **Missing:** InDesign 2026's Generative Expand, Text to Image and Auto Style equivalents; local
  generative vector requested (#232). **Owner decision** on openly licensed models.
- **Estimate:** 60–120 h.

### 25. Accessibility of the app itself (U)
- **Missing:** screen-reader exposure of panels and dialogs (egui AccessKit coverage unverified);
  PDF/UA validation of exports. **Estimate:** 15–25 h.

## Out of scope (by design)

Adobe cloud services (CC Libraries, Stock, Adobe Fonts activation, Publish Online, Share for
Review, Creative Cloud sign-in), InCopy assignment workflows, Bridge, Exchange extensions, Version
Cue. These are excluded from every percentage.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-22 | minor | Gap 26 shrunk: autoflow threads a frame per column on the pages that follow the story's last frame, then adds pages at the end of the document |
| 2026-10-19 | minor | Gap 28 added for remaining placed-PDF font embedding, substitution or outlining; visible unembedded font detection now ships in PDF/X-4 export and `validate-pdf`, with invisible text exempt and operation- or depth-limited checks reported incomplete (#909) |
| 2026-10-16 | minor | Gap 27 closed: the loaded text cursor threads into a text frame inside a group (#1075) |
| 2026-10-16 | trivial | Gap 13 shrunk: Greek hyphenates, so five languages do (#1077) |
| 2026-10-13 | minor | Gap 16: Parent Options dialog and overrides of base items on a parent spread |
| 2026-10-12 | minor | Gap 0 shrunk to the Primary Text Frame page fixes (#165): text threads by hand; gaps added for autoflow onto existing pages and threading into grouped frames |
| 2026-10-10 | minor | Gap 0 added and marked alpha blocker: manual text threading is missing (stage re-normalized to pre-alpha) |
| 2026-10-10 | major | Created: 25 ranked gaps from the full re-measure against InDesign 2026 21.6 and issues #1–#352 |

## External native artwork follow-ups (#856)

Opt-in linked-only working saves are implemented; see [linked-artwork.md](linked-artwork.md).
Remaining: small persistent previews for missing sources, lazy-loading full-resolution artwork,
and deciding whether new documents should default to external storage. Existing embedded files
keep their behavior until the user explicitly changes the storage mode.
