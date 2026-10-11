# Where DesignCraft falls short of InDesign

> **Last reviewed:** 2026-10-11 · **Last updated:** 2026-10-11 · **Change:** minor (Optical kerning from outlines (#181); 2026-10-10: stage re-normalized to pre-alpha by the core-workflow gate; earlier today: major, first version: gaps from the full re-measure and the issue tracker) · **Target:** Adobe InDesign 2026 (21.6)

Every known shortfall against InDesign 2026, one entry each, ranked by how much it stops an
InDesign professional from doing real work. This is the work list: agents pick from the top.
Numbers and weights are in [target-app-parity.md](target-app-parity.md); the plan is in
[roadmap.md](roadmap.md). Estimates are Opus 5.5 agent-hours, calibrated in
target-app-parity.md. When you close a gap, delete its entry (or shrink it), add a line to the
ROADMAP progress log and update the timestamps.

Kind: **F** feature · **U** UI/UX · **T** typography · **FF** file format · **H** hardware ·
**L** localization · **P** performance · **S** stability · **E** ecosystem · **A** AI.

## Alpha blocker (the core-workflow gate in [roadmap.md](roadmap.md#alpha-gate))

### 0. Text can't be threaded by hand (F, U) · **ALPHA BLOCKER**
- **Missing:** threading a story from one frame to another. The canvas draws in and out ports
  (`crates/ui-egui/src/canvas.rs`), but clicking the out port does nothing: no loaded text
  cursor, no click-a-frame or draw-a-frame to continue the story, no unthread by double-clicking
  a port, and no `frame.thread` / `frame.unthread` command for the control channel or MCP
  (`Document::thread` exists in `crates/doc/src/edit.rs` but only autoflow, IDML import and the
  sample call it). Long text flows only through autoflow on Place and Primary Text Frame, which
  users report adding pages they can't delete (#165).
- **Evidence:** #352, #194, #165, #140 (Windows and unspecified platforms; the code shows it's
  missing everywhere).
- **Impact:** a story can't run from page 2 to page 7, or from one column frame to the next
  that the designer drew. That is InDesign's core text workflow.
- **Estimate:** 15–25 h (commands with undo and tests 4–6, out-port loaded cursor and
  click/draw/unthread 6–12, Primary Text Frame page add/delete fixes 5–7).
- **Doc:** [ui-parity.md](ui-parity.md)

## Blocking (beta can't be declared while these are open)

### 1. Basic editing fails for many users (U, S)
- **Missing:** object copy/paste on Windows and Linux (#351, #348, #312, #185, #164, #201, text RTF
  paste #212); Paste in Place onto the selected page (#348); right-click context menus that don't
  open (#238, #187, #160, #312); threading (gap 0); double-click word selection in fields
  (#189); character formatting set at a caret is discarded (#289).
- **Evidence:** 16 issues from 2026-10-05 to 2026-10-10, mostly against the 0.4/0.5 release builds.
- **Impact:** a layout artist can't finish the simplest job. This dominates the
  ready-for-real-work number.
- **Estimate:** 30–50 h (reproduce per platform through the control channel, regression tests).
- **Doc:** [ui-parity.md](ui-parity.md)

### 2. InDesign documents (.indd) can't be opened (FF)
- **Missing:** INDD (and INDT, INDB, INDL) reading. Everything InDesign users receive is INDD.
- **Evidence:** #21. Two clean-room converters are proposed: PR #98 and `indd-utils` (#114),
  which reports 96.9% of InDesign's IDML values reproduced on ~650 INDD/IDML pairs, 0 failures on
  ~4,300 files, and output that DesignCraft's IDML importer already reads.
- **Impact:** without it, every job starts with "ask the client for an IDML".
- **Estimate:** 20–60 h to integrate indd-utils (INDD → IDML → existing importer), warnings UI,
  corpus test; **owner decision** on provenance and where the crate lives.
- **Doc:** [file-format-parity.md](file-format-parity.md)

### 3. IDML round-trip isn't proven on real files (FF)
- **Missing:** a corpus test of real InDesign-authored IDML (via `storytold/photocraft-corpus` or
  a sibling) measuring what survives import → export → reopen in InDesign. Open bugs: frames not
  rendered without a warning (#193), `$ID/` font names reported missing (#338), no
  `PDFAttribute` on export (#339), Package writes build-machine absolute paths (#340), drag-drop
  open of IDML on the app icon (#211).
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
  skip list); not yet confirmed by the reporters.
- **Estimate:** 15–30 h.
- **Doc:** [hardware-parity.md](hardware-parity.md)

### 6. Installed fonts not always found (F)
- **Missing:** fonts in user font folders used for text (#161, #110, #107), fonts activated by
  font managers and Adobe Fonts (#327, #261), reload when fonts change on disk (#328).
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

### 8. Canvas text input has no IME (U, L)
- **Missing:** composition input for Chinese, Japanese and Korean on the canvas (#322).
  VectorCraft has an implementation to port.
- **Estimate:** 10–20 h. **Doc:** [localization-parity.md](localization-parity.md)

### 9. PDF output presets and standards (FF)
- **Missing:** PDF/X-1a and PDF/X-3 (no flattener to PDF 1.3/1.4), Adobe PDF Presets and
  `.joboptions` load/save, output-intent choice beyond the built-in profile, PDF/UA validation;
  #159, #32.
- **Have:** PDF/X-4 with output intent and a validator (`#200`), PDF/A-2b, tagged PDF, CMYK and
  spot, marks and bleed, the options dialog with remembered settings (#196).
- **Estimate:** 40–70 h. **Doc:** [file-format-parity.md](file-format-parity.md)

### 10. Printing (H, F)
- **Missing:** a real print path: `file.print` sends PDF to `lpr`, so Windows can't print; no
  PostScript, printer PPDs, separations to device, print presets, or trapping.
- **Estimate:** 25–45 h. **Doc:** [hardware-parity.md](hardware-parity.md)

### 11. Paragraph and character style editing reliability (F)
- **Missing:** saving edits and deleting a paragraph style reported failing (#169); Tabs panel
  for setting tab stops (#326).
- **Estimate:** 10–20 h. **Doc:** [ui-parity.md](ui-parity.md)

### 12. Tables in depth (F)
- **Missing:** anchored objects in cells not drawn (#163); Table ▸ Table Options and Cell Options
  submenus as InDesign lays them out, Diagonal Lines, Convert Rows to Header/Body/Footer, Edit
  Header/Footer, Go to Row; general reliability (#20).
- **Estimate:** 25–40 h.

### 13. Hyphenation and spelling languages (T, L)
- **Missing:** hyphenation patterns for every language but English and Spanish; spelling for
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
- **Missing:** Books beyond a list of documents: style/swatch synchronization, book-wide
  pagination, book PDF export, book TOC and index; Table of Contents Styles.
- **Estimate:** 30–50 h.

### 16. Menu items still missing (U)
- **Missing:** 151 of 482 in-scope items of InDesign 21.6's menu dump have no match (measured 2026-10-10; some exist under other labels): Type ▸ Notes
  and Track Changes submenus (the panels and commands exist), hyperlink and cross-reference
  update commands, Convert Bullets/Numbering to Text, Convert Variable to Text, Object ▸ Clear
  Effects / Clear All Transparency / Rotate 180° / Reverse Path, Layout ▸ Go Back/Forward,
  Window ▸ Arrange (2-up … 6-up), Trap Presets, Overlays, Edit ▸ Assign Profiles / Convert to
  Profile / Edit Original, File ▸ File Info / Open PDF / Document and Print Presets / User
  Settings, Type ▸ Size presets, Table ▸ Table Options / Cell Options submenus, Paste Before/After,
  Convert Rows/Columns, Object ▸ Interactive ▸ Convert to (form fields), Flex Layout (new in 2026),
  View ▸ Extras badges and Story Editor options.
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
- **Missing:** EPS is shown through its preview only (no PostScript interpreter); PSD layer
  comps beyond visibility; Edit Original / Edit With; PDF placed as editable vectors (#227, #24);
  QuarkXPress import (#73).
- **Estimate:** 30–60 h (PostScript interpreter is a large separate project: +80–150 h).

### 20. Performance on large documents (P)
- **Missing:** incremental composition (recompose from the edited paragraph; 8 ms keystroke
  budget), benchmarks against InDesign on long books and image-heavy catalogues.
- **Have:** compose 300k characters ~38 ms warm, damage-region repaint (< 1 ms while typing),
  parallel story composition.
- **Estimate:** 40–80 h.

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
| 2026-10-10 | minor | Gap 0 added and marked alpha blocker: manual text threading is missing (stage re-normalized to pre-alpha) |
| 2026-10-10 | major | Created: 25 ranked gaps from the full re-measure against InDesign 2026 21.6 and issues #1–#352 |
