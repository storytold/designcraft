# DesignCraft parity with Adobe InDesign

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** minor (stage re-normalized to pre-alpha by the core-workflow gate; earlier today: major, full re-measure against InDesign 2026 21.6; replaces the 2026-10-04 estimate in ROADMAP.md) · **Target:** Adobe InDesign 2026 (21.6.0.57, macOS)

The authoritative assessment of how close DesignCraft is to Adobe InDesign. [ROADMAP.md](../ROADMAP.md)
summarizes it; [gaps.md](gaps.md) lists every known shortfall one at a time; the feature-by-feature
presence checklist is [parity-checklist.md](parity-checklist.md) (`cargo xtask parity`).

## Headline

| Number | Value | Kind |
|---|---|---|
| **Feature breadth** (does each InDesign feature exist?) | **~80%** (range 69–99%) | measured lower and upper bounds, estimate between them (below) |
| **Ready for real work** (could an InDesign professional switch for real jobs?) | **~47%** (range 42–52%) | estimated, weighted sum (below); 1,150–1,950 h to ~95% |
| Mainstream practitioner (weekly core of a typical InDesign pro) | **~42%** | 500–850 h to ~95%; estimated, [method below](#mainstream-practitioner-42) |
| Essentials user (flyers, newsletters: the core only) | **~54%** | 135–250 h to ~95%; estimated, [method below](#essentials-user-54) |
| Remaining effort to **beta** (~75% ready, main format reliable; 28 points away) | **420–750 Opus 5.5 agent-hours** | estimated |
| Remaining effort to **full parity** | **1,150–1,950 Opus 5.5 agent-hours** | estimated |
| Stage | **pre-alpha** (fails the core-workflow gate: no manual text threading); ~15–25 h to alpha | see [roadmap.md](roadmap.md#alpha-gate) |

## How breadth was measured

Two measurements bracket it, and neither is the answer by itself.

- **Checklist: 99% (upper bound, measured, self-graded rows).** `cargo xtask parity` scores the 275
  rows of [parity-checklist.md](parity-checklist.md), weighted P0 = 3, P1 = 2, P2 = 1: 269 done,
  6 partial, 0 missing. The rows are a catalogue we wrote and were graded by the agents that built
  the features; "done" means the feature exists, not that it has all of InDesign's options.
- **Menu dump: 69% (lower bound, measured 2026-10-10).** The main menu tree of the running
  InDesign 2026 21.6, dumped through its scripting DOM during black-box observation
  (`plan/indesign/screenshots/menus.txt`, local, never committed: 1,121 entries, 798 leaf items),
  was matched by a script against every label in the DesignCraft source (engine command labels,
  `UI_COMMANDS`, the `MENUS` table, dialog and panel strings; case, punctuation and "Show/Hide"
  variants normalized). Excluded as out of scope or dynamic: the installed-font list (490 items),
  Plug-Ins, Adobe cloud and AI services (Firefly, Generate, Rewrite, Auto Style, Stock, CC
  Libraries, Publish Online, Share for Review), InCopy, Help links, window tiling, recent files
  and workspace names. **331 of 482 in-scope items match (68.7%)**: File 19/26, Edit 28/31,
  Layout 21/27, Type 49/92, Object 81/110, Table 26/45, View 40/57, Window 50/73, app menu 17/20.
  It undercounts: some items exist under another label or only in a panel menu (Notes and Track
  Changes have panels and `note.*` / `changes.*` commands but not InDesign's Type ▸ Notes /
  Track Changes submenus).
- **Menu tree from documentation: 75% (cross-check).** The same match against the clean-room menu
  tree compiled from public documentation (`plan/indesign/04-menu-tree.md`, 689 items): 440 of 590
  in-scope items (74.6%).
- **Command surface (measured):** 446 engine commands (188 placed in menus) + 99 UI commands; the
  menu bar has 9 menus, 49 submenus and 340 items. The original plan targeted ~1,000 commands.

The breadth figure is taken between them, at **~80%**, because the checklist overstates (rows were
written at a coarse grain and graded by their builders) and the label match understates (label
differences, panel-only commands).

## How "ready for real work" was estimated

Each dimension below was judged from the code, the tests, the issue tracker (124 issues, 79 open on
2026-10-10, most from people trying the 0.4/0.5 releases on real work) and the InDesign 2026
reference material. Weights reflect what an InDesign professional hits first: opening the files
they receive, setting type exactly, and basic editing that never fails.

| Dimension | Weight | Ready | Remaining (Opus 5.5 h) | Evidence | Doc |
|---|---|---|---|---|---|
| Features (depth of each area) | 25% | 55% | 250–400 | 275 catalogue rows exist; many lack options (e.g. Books is 258 lines of engine code, Liquid Layout 329); see the area table | this file |
| Typography and composition fidelity | 15% | 45% | 120–200 | Knuth–Plass composer, keeps, grids, CJK, Arabic all exist; no comparison with InDesign's line breaks; optical kerning falls back to metrics (#181); overset rule differs (#180); hyphenation patterns for English and Spanish only | [typography-parity.md](typography-parity.md) |
| File formats | 20% | 40% | 150–250 (+20–60 for INDD integration) | INDD cannot be opened (#21); IDML both ways and verified in InDesign 2026, but real-file bugs open (#193, #338, #339, #340); PDF export strong; PDF/X-4 only | [file-format-parity.md](file-format-parity.md) |
| UI/UX fidelity | 15% | 45% | 120–180 | Measured InDesign 2026 look; but users report object copy/paste (#351, #348, #185, #164), right-click menus (#238, #187, #160), out-port threading (#352, #194, #165), canvas IME (#322) failing | [ui-parity.md](ui-parity.md) |
| Stability | 10% | 45% | 60–100 | Never-crash rules and last-resort guards; startup failures on some GPUs and machines (#334, #273, #220, #192, #167, #166); no CI runs the tests on pull requests | [gaps.md](gaps.md) |
| Performance | 5% | 65% | 40–80 | 300k characters compose in ~38 ms warm; typing repaints in < 1 ms (damage regions); no incremental composition; nothing benchmarked against InDesign; no large real documents | [gaps.md](gaps.md) |
| Localization | 3% | 45% | 120–180 | 9 translations beyond English; Japanese 95%, most others 83–85%, pt-BR 61%; Hindi, Indonesian, Korean, Vietnamese none | [localization-parity.md](localization-parity.md) |
| Platforms | 3% | 55% | 40–80 | macOS is where it's tested; Windows and Linux users hit basic failures; web and FreeBSD builds exist | [hardware-parity.md](hardware-parity.md) |
| Hardware | 2% | 45% | 20–40 | Canvas rasterizes on the CPU (vello_cpu, multithreaded); GPU only presents; printing is `lpr` (no Windows printing, no PostScript) | [hardware-parity.md](hardware-parity.md) |
| Ecosystem (scripting, plug-ins) | 2% | 15% | 150–250 | Own command/script/MCP API is strong; no ExtendScript/UXP DOM, so InDesign scripts don't run; no plug-ins | [gaps.md](gaps.md) |
| AI features | 0% (not weighted) | 15% | 60–120 | Agent control through MCP goes beyond InDesign; no generative features (InDesign 2026: Generative Expand, Text to Image, Auto Style) | [gaps.md](gaps.md) |
| **Weighted sum** | 100% | **47.2% → ~47%** (range 42–52%) | **1,150–1,950** | | |

**How the number is built:** the plain weighted sum of the table above, per the craftrules
standard: 25×55 + 15×45 + 20×40 + 15×45 + 10×45 + 5×65 + 3×45 + 3×55 + 2×45 + 2×15 = 47.2% →
**~47%** (range 42–52%, ±5 points of judgement in the dimension values). An earlier version
today applied a further ×0.9 for cross-cutting core-path failures and reported 42%. That
multiplicative step was dropped to match the standard (discounts belong only to the mainstream
and essentials numbers). Those failures are already in the UI/UX (45%) and Stability (45%) rows,
and the missing manual threading is what holds the stage at pre-alpha through the gate.

## Mainstream practitioner and essentials user

Two narrower questions, computed with the craftrules method so every app's numbers compare. The
stage still follows the full number (~47%) and the core-workflow gate. All three are estimates.

| Audience | Ready | Opus 5.5 agent-hours to ~95% | Parallelizes | Work that dominates |
|---|---|---|---|---|
| Full target (ready for real work) | **~47%** | **1,150–1,950** | ~70% (per crate) | depth across all 19 areas, InDesign scripting compatibility, localization to full in 12 languages, typography fidelity, INDD/IDML exchange |
| Mainstream practitioner | **~42%** | **500–850** | ~65% | INDD import and IDML real-file corpus (80–160), composition fidelity against InDesign (120–200), depth of the weekly areas (150–240), PDF/X-1a, presets and printing (65–110), editing and threading (45–75), stability and CI (25–45), fonts (10–20) |
| Essentials user | **~54%** | **135–250** | ~50% | start-up robustness on every GPU (25–45), editing and discoverability bugs: context menus, copy/paste, installer (30–50), threading (15–25), text editing and fonts (20–40), printing on Windows (15–30), depth of the core features (30–60) |

Hours are calibrated as in [Remaining effort and calibration](#remaining-effort-and-calibration)
(1–2 h per feature arc, 2–4 h per behaviour cluster matched against an oracle). Each audience's
work is a subset of the one above it: essentials ⊂ mainstream ⊂ full. Human needs: the owner's
decision on INDD (#114), InDesign oracle captures on the owner's Mac, native-speaker review (full
only).

### Mainstream practitioner: ~42%

The typical InDesign professional (magazine, book, brochure, packaging-adjacent layout artist)
uses these weekly: the alpha-gate workflows plus their everyday tools. Excluded: plug-ins and
scripting compatibility, generative/cloud AI, InCopy and team workflows, specialist hardware,
interface languages other than the user's own. Depth here is what the feature does when it
works; interaction, stability and exchange problems are the discounts, so they aren't counted
twice.

| Area | Weight | Depth | Evidence |
|---|---|---|---|
| Document setup: pages, margins, columns, parents, sections | 10% | 65% | features area table |
| Text frames: type, select, edit, thread, autoflow | 15% | 45% | manual threading missing; autoflow and Primary Text Frame only |
| Typography: composer, H&J, OpenType, tabs, lists, keeps | 15% | 65% | typography-parity.md: 18 ✓ / 16 ~; Tabs panel missing; InDesign-identical breaks counted under exchange |
| Paragraph, character and object styles | 10% | 65% | dialogs, based-on, nested/GREP, Quick Apply |
| Frames, shapes, transform, arrange, align | 10% | 70% | full tool set; copy/paste problems counted under interaction |
| Colour, swatches, stroke | 8% | 60% | hex entry, CMS depth |
| Images: place, fit, links, wrap | 10% | 60% | EPS preview only, Edit Original missing |
| Tables | 5% | 55% | anchored objects in cells, Table Options submenus |
| Layers | 3% | 75% | |
| Effects and transparency | 3% | 50% | fidelity unmeasured |
| Output: PDF export, print, package | 8% | 60% | PDF/X-4 strong; no X-1a, presets, Windows printing |
| View, navigation, undo, save | 3% | 70% | |
| **Weighted depth** | 100% | **60.7%** | |

Discounts (multiplicative):

| Discount | Factor | Evidence |
|---|---|---|
| Interaction fidelity | ×0.88 | right-click menus fail for some macOS users (#160, #238) and on Linux (#354); Paste in Place lands on page 1 (#348); formatting at a caret discarded (#289); font search focus (#335, #86) |
| Stability on real machines | ×0.92 | start-up failures on Intel Mac Metal (#334) and Windows drivers (#167, #273, #220, #192, #166); no CI on pull requests; macOS on Apple silicon is solid |
| File exchange with InDesign users | ×0.85 | `.indd` can't be opened (#21); IDML import/export bugs (#193, #338, #339, #340, #211); opened text reflows: optical kerning (#181), overset rule (#180). Harsher than VectorCraft's ×0.92 because InDesign pros receive and send native files daily |

60.7% × 0.88 × 0.92 × 0.85 = 41.8% → **~42%**. It is lower than the full number (47%)
because the full number spreads weight over dimensions DesignCraft does relatively well
(performance, platforms, breadth of the long tail: 99% checklist presence), while the weekly core
carries the gaps at full weight: threading, editing bugs and INDD exchange.

### Essentials user: ~54%

Someone making a flyer, poster, newsletter or simple booklet: they touch only the core.
Excluded: advanced options, pro workflows (preflight, separations, books, long-document
features), exchange edge cases, and everything excluded above.

| Core feature | Weight | Depth | Evidence |
|---|---|---|---|
| Launch, welcome screen, new document from a preset | 10% | 80% | presets incl. A6 (#248) |
| Type text in a frame, select and edit it | 15% | 65% | caret formatting (#289), double-click selection in fields (#189) |
| Font, size, colour, alignment | 15% | 65% | installed fonts not always found (#161, #110, #107) |
| Place an image and fit it | 10% | 75% | place gun, fitting |
| Draw shapes and lines, fill and stroke | 10% | 80% | |
| Move, resize, rotate, align, duplicate | 10% | 75% | |
| Copy and paste | 5% | 60% | works on macOS; Paste in Place to the wrong page (#348) |
| Undo / redo | 5% | 85% | |
| Save and reopen | 5% | 85% | `.designcraft` round trip, recovery |
| Export PDF / PNG | 10% | 80% | |
| Print | 5% | 40% | PDF to `lpr` only, nothing on Windows |
| **Weighted depth** | 100% | **72.0%** | |

| Discount | Factor | Evidence |
|---|---|---|
| Launch and stability | ×0.90 | the start-up failures above hit casual users first and they don't know `WGPU_BACKEND` |
| Discoverability and UI clarity | ×0.88 | a faithful, dense InDesign UI; right-click menus don't open for some users; the out port looks clickable but does nothing; Windows installer confusing (#64) |
| Opening files people send them | ×0.95 | casual users get Word, images and PDFs, which place well (docx/rtf/xlsx, PDF pages); INDD is rare for them |

72.0% × 0.90 × 0.88 × 0.95 = 54.2% → **~54%**.

### User evidence (GitHub issues and comments, 2026-10-10)

126 issues and 94 comments; no issues were filed by the maintainer (echelon).

- **Praise:** 7 people in 6 threads (#32 "so damn impressive… InDesign open source for Linux",
  #77, #107, #113, #197, #328).
- **Switched-from-InDesign reports:** 0 completed switches. 2 former InDesign users trying real
  work: #348, a retired designer who lost an Adobe licence, laying out 18 pages; #140. Both were
  blocked by core-path bugs (paste to page, threading). #320 is an art director with 10 years of
  daily InDesign asking for a feature.
- **Open issues (81):**
  - 44 are core-path bugs (54%), clustering into a few root causes: copy/paste (8),
    context menus (5), threading (4), launch (6), fonts (5).
  - 9 are InDesign file-exchange bugs.
  - 26 are niche requests or polish (HTML export, QuarkXPress import, MCP gateway, theme options).
  - 2 are meta.

So the user evidence matches the numbers: interest and goodwill are high, and real work is
stopped by a short list of core-path bugs.

### Why the number went down

The 2026-10-04 estimate said "~83% including depth" and 170 h remaining. It was written before
anyone outside the project had used the app. New evidence since then:

- 79 open issues, many about core editing on Windows and Linux (copy/paste, context menus,
  threading, fonts not found, startup crashes), filed against the 0.4 and 0.5 releases.
- The first measured menu coverage: 69% of the in-scope items of InDesign 21.6's own menu dump.
- The main file format: InDesign users receive `.indd` files, which DesignCraft can't open. The
  previous estimate treated INDD as out of scope; a professional can't.
- No comparison of composition against InDesign exists, so typographic fidelity is unproven.

## Features by area

Breadth is the checklist's measured score; Ready is this review's estimate of depth and reliability.
Hours overlap with the dimension table (UI and format work sits in both views); the dimension
table is the one summed.

| Area | Breadth (checklist) | Ready | Hours | Main gaps |
|---|---|---|---|---|
| Application shell & workspace | 99% | 55% | 40–60 | context menus not opening on some systems; window controls (#333); System theme (#237); New Window/Arrange; Help menu |
| Documents, pages, spreads | 100% | 65% | 20–35 | Layout ▸ Pages submenu items, Page Attributes / Color Label, Go Back/Forward, layout grid (CJK) |
| Layers | 100% | 75% | 5–10 | IDML layer order fixed 2026-10-09; Layer panel options depth |
| Frames, shapes & paths | 100% | 60% | 25–40 | object copy/paste failures, live-corner handle (#186), reference point relocation (#337), Join/Reverse Path/Convert Point menu |
| Transform | 100% | 70% | 10–20 | Rotate 180°; transform panel depth |
| Fill, stroke, colour | 100% | 60% | 20–35 | hex entry (#323); Assign Profiles / Convert to Profile; color-managed proofing depth; Adobe Color Themes OOS |
| Effects & transparency | 100% | 50% | 20–30 | effects fidelity vs InDesign unmeasured; Clear Effects / Clear All Transparency; flattener depth |
| Placing & links | 93% | 55% | 20–35 | EPS is preview-only (no PostScript interpreter); Edit Original; placed PDF layer visibility; INDD page place |
| Type & text frames | 99% | 45% | 40–60 | **no manual threading** (ports drawn, no out-port click, no thread/unthread command: alpha blocker; #352, #194, #165, #140); caret formatting discarded (#289); canvas IME (#322); Notes/Track Changes menus; frame grids |
| Typography | 100% | 45% | (typography dimension) | see [typography-parity.md](typography-parity.md) |
| Styles | 100% | 60% | 15–25 | Paragraph Styles edit/delete reported failing (#169); style options depth |
| Tables | 100% | 55% | 25–40 | anchored objects in cells not drawn (#163); Table ▸ Table Options submenus, Diagonal Lines, Convert Rows, Edit Header/Footer |
| Long documents | 100% | 45% | 30–50 | Books are shallow (sync, book pagination, book PDF/TOC/index); Table of Contents Styles |
| Interactivity & digital | 100% | 40% | 30–50 | HTML export basic (#314); fixed-layout EPUB is page images; no animation, timing, media playback |
| Output & production | 92% | 55% | 40–70 | PDF/X-4 only (no X-1a/X-3), no PDF/print presets or `.joboptions`, print via `lpr` (no Windows), no trapping; Output Preview panel |
| XML & automation | 100% | 35% | (ecosystem dimension) | no ExtendScript/UXP; XML structure depth |
| View & navigation | 100% | 65% | 10–20 | Navigator, display badges, Show/Hide extras (content grabber, live corners) |
| Undo, history, saving | 100% | 70% | 5–10 | Undo/Redo name the action; File ▸ Open Recent |
| Accessibility | 100% | 50% | 15–25 | tagged PDF exists; PDF/UA validation, reading order depth, screen-reader support in the UI |

## Remaining effort and calibration

**Calibrated against this repo's history.** The first commit was 2026-10-01 09:00; by 2026-10-10
`origin/main` had 470 commits (77 merged pull requests), 126,411 lines of Rust in 276 files and
1,122 `#[test]` functions. Commit timestamps show ~43 wall-clock hours of activity (gaps under
45 minutes), with one to five agents working in parallel, so roughly **100–150 Opus 5.5
agent-hours** built the current app: ~0.25 h per commit, **1–2 h per feature arc** (a command,
dialog, IDML and PDF support, tests). The community fixes of 2026-10-09/10 (text composition edge
cases #282–#298, 40–400 lines each) took about **1–2 h each**. Depth work against an oracle
(matching InDesign's behaviour exactly) is slower than adding features: **2–4 h per behaviour
cluster**, more when the oracle has to be built first.

| Work | Opus 5.5 h | Parallelizes? | Needs a human |
|---|---|---|---|
| Beta blockers (see [roadmap.md](roadmap.md)) | 420–750 | ~70% (per crate) | owner decision on INDD import (#114); InDesign oracle captures on the owner's Mac |
| Rest of full parity | 730–1,200 | ~70% | native-speaker review for 12 languages; owner decision on ExtendScript compatibility and AI models |
| **Total to full parity** | **1,150–1,950** | | |

With four to five agents in parallel and an integrator, the beta work is ~120–200 wall-clock
hours.

## Methodology and sources

- **Target:** Adobe InDesign 2026, version 21.6.0.57, installed at `/Applications/Adobe InDesign
  2026/`. Inspected without launching and without reading anything inside the bundle beyond names
  and listings (clean-room rule, AGENTS.md): `Info.plist` `CFBundleDocumentTypes` and
  `UTExportedTypeDeclarations` (formats), `.lproj` folder names (25 UI localizations), plug-in
  folder names (filters), `Presets/` folder names (swatch libraries, workspaces, shortcut sets).
- **Reference material:** `plan/indesign/01–11` (local, gitignored): overview, UI/UX, feature
  catalogue, menu tree, tools, panels, dialogs, file formats, shortcuts, typography, and
  observations of the running app.
- **DesignCraft measured from source on `origin/main` (92649ff, 2026-10-10)**: command registry,
  `MENUS`, i18n catalogs, crate line and test counts, format modules. No local build (disk).
- **User evidence:** GitHub issues #1–#352 and open PRs (65 open, including the INDD converter
  proposals #98 and #114).

## Appendix: what works today

The inventory below was the "Working today" list of ROADMAP.md (moved here 2026-10-10, unchanged).
It is evidence of breadth: each item exists. It is not a claim of InDesign-level depth.

- Document model: facing/non-facing spreads, parent pages with page-number markers, sections, layers, frames (text / graphic / unassigned), threaded stories, paragraph/character/object styles with based-on, swatches (process/spot/tints/gradients), corner options, text wrap, drop shadow, opacity/blend.
- Text engine: Knuth–Plass paragraph composer and single-line composer, dictionary hyphenation (public-domain Moby list + our own trained Liang patterns), justification with word/letter/glyph-scaling ranges, keep options (keep with next, keep lines together, widows/orphans), balance ragged lines, optical margin alignment, hyphenation zone/limit, columns, threading across frames and spreads, tabs, bullets/numbering, rules, shading, baseline grid, first-baseline options, vertical justification, wrap exclusions, overset detection, caret/hit testing.
- FID2: IDML underline/strikethrough Text Color resets survive style inheritance and roundtrip. Raster and PDF paint underlines behind glyphs, retain strikethrough above, and center rules on their offsets; synthetic overlap tests cover both backends.
- Rendering: vello_cpu (SIMD, multithreaded) — pages, items, images with mip levels, gradients, composed text.
- UI (egui): application bar + menus, Control panel (object and text modes), Tools panel with flyouts, document tabs, rulers, pasteboard, guides (margins, columns, bleed, baseline grid), frame edges, selection handles, text ports and threads, Properties / Pages / Layers dock, Swatches, Styles, Character, Paragraph, Stroke, Text Wrap panels, New Document and Preferences dialogs, Quick Apply (⌘Return), 4 brightness themes.
- Pen tool, Direct Selection anchor/handle editing, rotate from corners, Rotate/Scale/Shear tools, Eyedropper, snapping & smart guides, Align/Distribute, Find/Change (text + GREP), Story Editor, Paragraph Style Options, Effects panel, hidden characters, native macOS menu bar, measured InDesign 2026 look (Medium Dark): Properties sections per selection state, 21 pt spinner fields with arithmetic, Contextual Task Bar, inverse (black) text selection, Pages panel with drag-and-drop reorder / parent apply, Layers panel with per-object rows.
- Tools: Selection (click, marquee, move, Alt-duplicate, resize handles), Direct Selection, Type (draw frame, click caret, select, type), Rectangle/Ellipse/Polygon (+ frame variants), Line, Hand, Zoom.
- PDF export (krilla): real selectable text with embedded font subsets, DeviceCMYK/RGB + spot Separations, bleed boxes, crop/bleed marks + page info, pages or spreads, PDF/A-2b (PDF/X-4 output intent pending) — `file.exportPdf`, File › Export PDF…, `designcraft-cli run --export out.pdf`. File › Export PDF… now opens an "Export PDF" options dialog (General / Compression / Marks and Bleeds / Advanced) and remembers the last-used settings across exports and sessions.
- IDML interchange (`designcraft-idml`): export and import of swatches, styles, fonts, preferences, parent spreads, spreads/pages, frames, groups, images (embedded or linked), formatted threaded stories — opens in InDesign 2026 and round-trips InDesign-exported files (`file.exportIdml`, `file.openIdml`, File → Export IDML…, CLI `--in x.idml` / `--export x.idml`).
- Tables (M8): tables anchored in stories (header/footer/body rows, merged cells, per-edge strokes, fills + alternating fills, insets, vertical justification, at-least/exact row heights); composed into the text column (columns scale to fit, rows break across columns/frames with repeating headers/footers, overset); rendered, exported to PDF (real text) and IDML (export + import); Table menu, Table panel, Create Table dialog, caret/typing/Tab navigation and cell selection in cells; `table.*` commands.
- Hyperlinks (text or frames → URL / e-mail / page) and bookmarks, exported as PDF link annotations and outline.
- EPUB 3 (reflowable) export: stories in reading order, CSS from paragraph/character styles, images, navigation (`file.exportEpub`, CLI `--export x.epub`).
- Data Merge (one linked CSV, TSV, text, or Excel file; text, image, QR, and hyperlink placeholders; preview; a new document with one record per page or records tiled on a single page), spell checking (public-domain Moby list + document dictionary, suggestions), Step and Repeat (count or grid), snippets, object styles, Numbering & Section Options.
- Footnotes: Type › Insert Footnote (caret moves into the note), Document Footnote Options (numbering style incl. symbols, start/restart per page/spread/section, prefix/suffix, reference position/character style, paragraph style, separator, spacing, first baseline, rule above); notes composed at the bottom of the referencing column with the body text making room; edited in place on the canvas; rendered, exported to PDF (real text) and IDML (InDesign's structure, verified opening in InDesign 2026) and imported from IDML; `footnote.*` commands.
- Paragraph Rules: Rule Above / Rule Below (on, weight, colour, tint, column or text width, offset, left/right indent) set in Type › Paragraph Rules…, the Paragraph panel menu and Paragraph Style Options › Paragraph Rules; inherited style values shown, and `type.para` / `style.paragraph.edit` change only the rule fields named.
- Cross-references and text anchors: Insert Cross-Reference (paragraph by style, or text anchor), 11 formats with InDesign's building blocks (full/partial paragraph, paragraph text/number, page number, anchor name, chapter, file name) plus user formats; resolved live at composition — never out of date, no Update step; unresolved destinations show `??`; PDF link annotations to the destination page; IDML export/import as InDesign cross-reference sources, hyperlinks, text destinations and formats (verified in InDesign 2026); `xref.*` / `anchor.*` commands.
- Index: page references (topic from the selection or up to 4 levels, sort keys; current page, to end of story, next n paragraphs, suppressed, See / See also), Generate / Update Index (section headings, nested or run-in, page ranges merged, Index Title / Section Head / Level 1–4 styles); IDML export/import as InDesign topics, page references and topic cross-references (verified in InDesign 2026); `index.*` commands.
- Text clipboard keeps formatting (character/paragraph formats, footnotes, cross-references, index markers, tables) and falls back to plain text when the system clipboard changed; Paste without Formatting (⇧⌘V); Change Case (UPPERCASE, lowercase, Title Case, Sentence case — formatting kept); Type › Insert Special Character / White Space / Break Character submenus.
- Anchored objects: items flow in the text inline (on the baseline, with Y offset; lines grow to fit) or above the line (left/center/right, space before/after); paste copied items into text to anchor them; Anchored Object Options and Release (back onto the page where shown); rendered, exported to PDF and to/from IDML (InDesign anchored object settings; verified in InDesign 2026); copy/paste and threading carry them; `anchored.*` commands.
- Ruler guides: drag out of the rulers (page guides, or spread guides on the pasteboard), drag to move, drop on a ruler to delete; Layout › Create Guides (rows/columns with gutters, fit to margins or page); Delete All Guides on Spread; `guide.*` commands.
- Links panel: every placed graphic with status (OK / Modified / Missing / Embedded, checked against the file on disk), page and effective PPI (low resolution flagged); Relink…, Go To, Update, Embed; `links.*` commands.
- Missing fonts: highlighted pink on screen (never in output), listed by Preflight; Type › Find/Replace Font (fonts in the document, missing first; replace in text, cells, footnotes and styles); `font.list` / `font.replace`.
- Missing glyphs, as in InDesign: a character the applied font lacks is drawn as that font's missing-glyph box (its .notdef; a drawn box when the font's is empty), on screen and in PDF (as a path, so PDF/A and PDF/UA exports take it), and Preflight lists the characters by font. Preferences › Composition › Draw Missing Glyphs from Fallback Fonts (`document.preferences {glyphFallback}`, stored with the document) draws them from other fonts instead; it is off in new documents and IDML imports, on in documents saved before it existed. A missing font's substitute always uses fallback fonts.
- Damage-region repaint: after an edit the canvas re-renders only the frames whose composed text (or the items that) changed and patches its texture — pixel-identical to a full render; typing in a frame repaints in under a millisecond instead of a full-viewport render (22 ms measured on the sample).
- Crash recovery: unsaved documents are written to the recovery folder every 30 s and reopened (unsaved, remembering their files) after a crash; File › Revert and Save a Copy…; `file.recovery.*`, `file.revert`, `file.saveACopy`.
- Graphics backend chosen before the window exists (DirectX 12 on Windows, Vulkan on Linux, Metal on macOS, OpenGL as the fallback; `WGPU_BACKEND` overrides) with a start-up fallback: a backend whose start never showed a frame is skipped at the next start and the status bar says so (`gpu.json`). Found on an AMD hybrid-graphics laptop whose Vulkan driver faulted at the first present.
- Edit › Paste Into (copied objects become a frame's content, clipped by it; rendered, PDF and IDML both ways — verified in InDesign 2026); Polygon Settings (sides, star inset; double-click the Polygon tool).
- Color panel (fill/stroke proxy, CMYK/RGB/Lab sliders with channel ramps, tint slider for swatches, spectrum ramp, Add to Swatches) and Color Picker (double-click the proxy); mixed colours are unnamed (not listed in Swatches until added); `object.color`, `swatch.addToSwatches`, `swatch.addUnnamed`.
- Place text files: Word (.docx: styles by name with their attributes, bold/italic/underline/size/font, footnotes, tables, tabs, breaks), RTF and plain text, into the insertion point, the selected frame or a new frame; autoflow adds pages and threaded frames until the text fits; Remove Styles option (`designcraft-textimport`).
- Gradient Feather (⇧G tool, `object.gradientFeather`): objects fade along a linear or radial opacity gradient, on screen and as a soft mask in PDF.
- Script Label (`object.label`, `object.findByLabel`), alt text (`object.altText`), Isolate Blending / Knockout Group (`object.transparencyGroup`; on screen, PDF isolates), Table ▸ Sort (`table.sortRows`, numeric-aware, header/footer fixed).
- File ▸ Export Text (`file.exportText`): a story as Text Only or RTF (fonts, styles, alignment, indents, tables as tab-separated rows); Export EPUB now asks for a path in the UI.
- Tagged PDF (`file.exportPdf {tagged}`, on by default from the UI): structure tree with stories as paragraphs in reading order, figures carrying their alt text, parent-page items and printer's marks as artifacts.
- Global Light (`object.globalLight`, `globalLight` on drop/inner shadows; Use Global Light in the Effects panel) and Generate Static Caption (`object.caption`: template over name, path, alt text, label, effective ppi, dimensions, format; any side, offset, style).
- Control panel scale X/Y, rotation and shear fields (`transform.info`, absolute `transform.set {scaleX, scaleY, rotation, shear}` about the reference point) and Preferences › Transformations are Totals for nested objects and placed graphics.
- Variable fonts: each named instance is a style (Font menu, shaping with feature variations, metrics, outlines) and embeds at its axis settings in PDF; free axis sliders still to come.
- Object ▸ Select (first/next above, next/last below, container, content, previous/next in group), full Object ▸ Convert Shape (`object.convertShape`), Table ▸ Split Cell Horizontally/Vertically.
- Attributes panel (`object.attributes`: Overprint Fill/Stroke/Gap, Nonprinting), with Overprint Gap in Overprint Preview and IDML.
- Page Numbering View (Preferences: section or absolute): page labels everywhere follow it; Go to Page takes section names, numbers or `+n`.
- Preferences › Advanced Type (document `advancedType`: superscript/subscript size and position; IDML TextPreference); subscripts now drop 33.3% like InDesign.
- Preferences › Composition › Highlight: keep violations, H&J violations (three shades), custom tracking/kerning, substituted fonts.
- File ▸ Export HTML (`file.exportHtml`): one self-contained page in reading order, styles as CSS, images inline, alt text from Object Export Options (also in EPUB).
- Swatches panel colour groups (`swatch.newColorGroup`, `moveToGroup`, `ungroupColorGroup`, `renameColorGroup`; folders and a row context menu; IDML ColorGroup).
- Object ▸ Generate QR Code (`object.qrCode`: web link, text, SMS, email, business card; vector modules as a compound path, into a frame or re-encoded in place).
- Conditional text (`condition.new/options/delete/apply/list`, Conditional Text panel): hidden conditions drop their text from layout and exports, indicators underline on screen, IDML Condition/AppliedConditions.
- Endnotes (`endnote.insert/edit/delete/list/options`, Type ▸ Insert Endnote): references number through the document, the endnote frame on a new last page lists them under a heading; IDML keeps the listed text only.
- Editorial notes (`note.new/edit/delete/convertToText/list`, Notes panel): anchored in text, flagged on screen, never printed or exported.
- File ▸ Print Booklet (`file.printBooklet`): saddle-stitch or 2-up consecutive printer spreads as PDF, padded with blanks, with a gap between pages.
- Edit ▸ Transparency Blend Space (`edit.transparencyBlendSpace`; RGB for web/mobile documents; IDML TransparencyPreference) — exported PDFs give pages with transparency a DeviceCMYK or DeviceRGB page group, so viewers and RIPs blend in that space. On screen, spreads with transparency in a CMYK blend space are shown through the working CMYK profile (their colours keep to its gamut, as when composited in CMYK).
- High Contrast interface theme (fifth Interface Color Theme: black chrome, white text, yellow selection).
- Place Excel workbooks (.xlsx): the first worksheet's used range becomes a table (shared/inline strings, numbers, booleans).
- Object Library (`library.new/open/add/place/remove/list/json/close`, Library panel): `.dclib` files of snippets that keep stories, styles, swatches and images.
- Ink Manager (`ink.list`, `ink.options`; Swatches panel menu): All Spots to Process, per-ink conversion and ink aliases in PDF output; IDML ConvertToProcess/AliasInkName.
- Appearance of Black complete: Printing/Exporting rich black for RGB output (PNG), Overprint [Black] Swatch at 100% (document, IDML OverprintBlack).
- Hyperlinks and Bookmarks panels (Window ▸ Interactive): new from the selection, edit (`hyperlink.edit`), go to source (`hyperlink.goToSource`), rename bookmarks (`bookmark.rename`), go to page, delete.
- Nested line styles (`nestedLineStyles` paragraph attribute, Paragraph Style Options › Drop Caps and Nested Styles, IDML AllNestedLineStyles): the first lines restyle until the line breaks settle.
- User dictionary hyphenation exceptions (`hyphenation.addException/removeException/list`, Edit ▸ Spelling ▸ User Dictionary): `ex~am~ple` breaks only at `~`, a plain word never hyphenates.
- Frame Fitting Options (`object.fittingOptions`, Object ▸ Fitting ▸ Frame Fitting Options…): Auto-Fit refits on resize, fitting, Align From reference point, crop amounts; IDML FrameFittingOption.
- Paragraph borders (per-side weights, colour, tint, offsets) and shading offsets, drawn per column part of split paragraphs (also when overset); Paragraph panel controls; IDML ParagraphBorder*/ParagraphShading*Offset.
- Font menu: search, favourites (stars, Show Favorites Only; `favoriteFonts` preference) and each family previewed in its own face. Western (and other-script) families come first, then Japanese, Simplified Chinese, Traditional Chinese and Korean ones, each group alphabetical and set off by a separator; the group comes from the font's own data (`meta` languages, family names in a CJK language, OS/2 code pages, else what it covers), read once (installed fonts by the font scan, without loading them). CJK families show their native names, the English name on hover; Preferences › Type › Show Font Names in English (`showFontNamesInEnglish`) shows English names throughout. Search matches either name; documents store the English name. Every family list (Character panel, Control bar, Properties, style dialogs, Find Font, Glyphs) follows this order.
- Application preferences (engine `Prefs`) now persist across launches (desktop: prefs.json beside ui.json).
- View ▸ Fit Selection in Window (⌥⌘=) and Find/Change ▸ Object (`find.objects`, `find.changeObjects`: by fill, stroke, weight, opacity, kind, layer, label).
- Custom workspaces (`window.newWorkspace/deleteWorkspace/resetWorkspace`; workspace switcher shows the current one): bars and panel arrangement saved by name.
- Word/RTF Import Options (File ▸ Place with Import Options…, `place.styles`, `file.place {styleMap, styleConflicts}`): map imported styles onto the document's, use existing / redefine / auto-rename on conflicts, or remove formatting.
- Custom anchored objects (`anchored.insert/options {position: custom}`): placed relative to the anchor, frame, column, page margins or page edge with reference points and offsets, no space in the text, keep within column; IDML Anchored position.
- Variable font axes: Character panel sliders; any axis values (`fontStyle: "Bold {wght:650}"`) make that instance for layout, screen and PDF.
- Image Import Options › Crop to for placed PDFs (`file.place {pdfCrop: crop|trim|bleed|art|media}`, Place PDF dialog): the frame shows that page box.
- Stroke styles: striped (thick-thin… and custom bands), wavy and straight-hash strokes now draw on screen and in PDF; named custom stripe/dash/dot styles (`strokeStyle.new/delete/list`, Stroke panel Type list; IDML Striped/Dashed/DottedStrokeStyle).
- Object ▸ Primary Text Frame (`object.primaryTextFrame`): any text frame's story can become (or stop being) the primary story Smart Text Reflow follows.
- Column rules (Text Frame Options › Column Rules, `object.textFrameOptions {columnRule, columnRuleWeight, columnRuleColor, columnRuleTint, columnRuleOffset, columnRuleTopInset, columnRuleBottomInset}`, reported by `document.inspect`): a bar centred in each gutter of a multi-column frame, inside the insets and between the columns of right-to-left and vertical frames too, broken around paragraphs that span columns, with weight, colour, tint, horizontal offset and top/bottom insets; on screen, in PDF (tagged as an artifact) and in IDML (`ColumnRule…` attributes of `TextFramePreference`, also in object styles). The dialog shows the frame's options and changes only the fields edited. Rule stroke types and overprint aren't there yet.
- Place a page of an IDML or DesignCraft document (`file.place {layoutPage}`): its objects, stories, styles and swatches as one group (INDD files are not readable).
- Articles panel (`article.new/add/remove/options/delete/list`): reading order and content of EPUB and HTML exports.
- Object ▸ Clipping Path (`object.clippingPath {type: alpha|edges, threshold, tolerance}`): traces the placed image and makes the outline the frame.
- Gridify: arrow keys while dragging a frame or shape tool add columns/rows (`frame.grid`), one undo step.
- Gap tool (U, `gap.move`): drag the space between objects (or an object and the page edge) and both sides resize.
- XML basics (`xml.newTag/deleteTag/tag/mapStyle/structure`, `file.exportXml`, `file.importXml`; Tags panel with structure; View ▸ Show Tagged Frames): tagged frames export in reading order, mapped paragraph styles become elements, imported elements flow into frames with matching tags. Inline text tagging (`xml.tagText`, an `xmlTag` character attribute, exported as nested elements) and tag markers (View ▸ Structure ▸ Show Tag Markers) too. DTDs (`xml.loadDtd`, `xml.validate`, `xml.deleteDtd`; Tags panel): loading one adds its elements as tags and names the root; validation checks content models (sequences, choices, `?*+`, mixed content, EMPTY/ANY, parameter entities) and required attributes, and lists each problem with its element path.
- Adjust Layout / Layout Adjustment (`adjustLayout` on Document Setup and Margins and Columns): objects follow each page's margin box to its new size and position.
- Content Collector and Placer tools (B; `conveyor.collect/place/list/clear`): copies of objects (with stories, styles, images) ride a conveyor between pages and documents; Alt-click keeps them on it.
- Linked stories (Edit ▸ Place and Link, `story.placeAndLink/links/updateLink/unlink`): child stories copy a parent, show out of date in the Links panel and update.
- Edit ▸ Menus (`window.hideMenuItem`): hide menu items; menus with hidden items end in Show All Menu Items.
- View ▸ Separations Preview (`view.separations`): one process plate as ink density, or areas over an ink limit in red (a GCR preview of the rendered view; spot plates and the flattener preview aren't there).
- Separations Preview plates now come from each object's own colour (CMYK as authored; RGB and images through the colour settings, images via a cached 17³ table), so pure black shows only on the black plate.
- Language (Character panel ▸ Language, Properties ▸ Character, `type.char {language}`): text in other languages isn't hyphenated or spell-checked with the English dictionaries, typographer's quotes follow the language (German „…“, French and Russian «…», Swiss, Spanish, Dutch, Nordic, Japanese and Traditional Chinese 「…」, Simplified Chinese “…”), the font's localized forms (OpenType `locl`: Turkish i, Bulgarian Cyrillic, Japanese / Chinese / Korean glyph variants) follow it too, and so does the fallback for CJK characters the font lacks (with Draw Missing Glyphs from Fallback Fonts on): Japanese, Simplified Chinese, Traditional Chinese and Korean each try their own installed system fonts (Japanese starts with the craft-fonts Japanese faces, Mincho first, when built with them; Hangul, kana and Bopomofo pick their language's fonts whatever the language). Language names from IDML keep their spelling; locale codes as InDesign writes them for some languages (`de_DE_2006`, `nl_NL_2005`; any `ll`, `ll_CC`, `ll-CC`) are their language and region for all of the above, and Chinese is also recognised in its common spellings ("Simplified Chinese", "Chinese (Traditional)", `zh_CN`, `zh-Hant`). Spanish is hyphenated with Javier Bezos's Liang patterns (`hyph-es.tex` 5.0, MIT) through the `hypher` crate; hyphenation for other languages isn't bundled yet.
- Books (`book.new/open/add/remove/list/styleSource/paginate/syncStyles/exportPdf`, Book panel): `.dcbook` files, continuous page numbering, style sync from a style source, one merged PDF.
- Track Changes (`changes.track/list/acceptAll/rejectAll`, Type ▸ Track Changes): typing is marked added (highlighted on screen), deletions are kept but hidden from layout and exports until accepted or rejected. Per-change accept/reject and authors aren't there yet.
- Track Changes: per-change Accept / Reject (`changes.accept`, `changes.reject`) and a Track Changes panel listing every change.
- Move table rows and columns (`table.moveRow`, `table.moveColumn`, Table panel buttons; with whole rows or columns selected, drag them with the Type tool — `table.dropCells`).
- Distribute Rows Evenly (`table.distributeRows`, Table menu and panel): selected rows receive equal fixed heights preserving their composed total height, with undo/redo. Row-spanning merges and rows outside the composed frames are rejected; shrinking tall cells can overset their text.
- Object States (`states.create/show/rename/release/list`, Object States panel): multi-state objects show one state on screen and in PDF.
- Buttons (`button.set/clear/list`, Buttons and Forms panel): go to page / next / previous / first / last / URL as PDF link annotations. Form fields too (`form.set/clear/list`: text fields, check boxes, combo and list boxes, signature fields; required, multiline, defaults) exported as AcroForm widgets through an incremental update (not in PDF/X).
- Power Zoom (Hand tool: press and hold still for half a second, or Alt-press): zoom out to the spread, aim the red view rectangle, release to zoom back in there.
- Graphic cells (`table.placeGraphic`, `table.textCell`, Table › Convert Cell Type, Table panel): images in table cells, fitted or filled, clipped to the cell on screen and in PDF.
- Live Distribute (Space while dragging a selection handle; `transform.resize {distribute}`): objects keep their size and spread with the bounds.
- Liquid Layout (`liquid.pageRule/object`, `guide.liquid`, `layout.createAlternate`, Liquid Layout panel): scale / re-center / guide-based / object-based page rules applied on page resize and Document Setup; dashed liquid guides; alternate layouts as resized page copies in a named section. Text in an alternate layout stays linked to the original stories (Links panel shows them out of date; `story.updateLink`).
- Split Window / New Window (`window.split`, `window.newWindow`, Window ▸ Arrange): two side-by-side views of the document with their own zoom, scroll and render cache. New Window puts the second view in its own OS window (an egui viewport; a floating window on the web).
- Transparency flattener (`file.exportPdf {flatten: high|medium|low|ppi}`, Edit ▸ Transparency Flattener Presets): each object involving transparency becomes an opaque image of its area over what's beneath it, so everything else stays vector (spreads with transparent parent items are rasterised whole).
- Video and sound (place .mp4/.mov/.webm/.mp3/.wav…, `media.options/get`, Media panel): media frames show a poster or placeholder on screen, in print and in PDF, and export as HTML5 `<video>`/`<audio>` in EPUB. Interactive PDF export (`file.exportPdf {media: true}`, File ▸ Export PDF (Interactive)) embeds each file and plays it in a Screen annotation through a rendition action (controls, loop, play on page load). No in-app playback.
- Right-to-left text (World-Ready basics): right-to-left runs are shaped right to left, lines are reordered by the Unicode Bidirectional Algorithm (matched against the reference implementation), Paragraph Direction (`type.para {direction}`, Paragraph panel, IDML ParagraphDirection, EPUB `dir`). Digits (`digits`: Default, Arabic, Hindi, Farsi, Native by language; Character panel for right-to-left text; IDML DigitsType) draw 0–9 in another script. Kashidas (`kashidas` paragraph attribute, on by default): justified Arabic lines stretch at one join per word (after seen/sad when possible, never inside lam–alef) with a drawn tatweel before word spaces widen. Carets follow the drawing: the caret before a right-to-left glyph sits at its right edge, arrow keys move as drawn (by words in the line's direction), clicks find the nearest drawn caret position and selections highlight the selected glyphs. Binding: Right to Left (Document Setup, `layout.documentSetup {binding}`, IDML PageBinding) makes page 1 a left page and lays spreads out right to left.
- Interface language (Edit ▸ Interface Language, `app.language`): German, French, Spanish, Italian, Japanese, Simplified Chinese and Arabic menu titles, common menu items and panel names (our own translations). Common dialog, panel and tooltip labels are also localized, with English fallback for untranslated strings. Arabic translations cover every key in the shared interface translation table, including menu commands and utility labels. The Type menu's Story Direction, Horizontal/Vertical, Tate-Chu-Yoko, Ruby… and Kenten are translated too. Arabic labels use Unicode bidi run ordering and contextual shaping; welcome and new-document form layouts follow RTL direction independently of document binding. CJK and Arabic glyphs come from [craft-fonts](https://github.com/storytold/craft-fonts) when built with `CRAFT_FONTS_DIR` (all releases): BIZ UDPGothic and Noto Sans CJK SC for the UI (Chinese forms first in the Chinese interface), Noto Sans Arabic, and Mincho for Japanese documents (the web build embeds BIZ UDPGothic only); otherwise from system fonts. Ukrainian covers all 1,430 existing catalog keys, including publishing dialogs and built-in names, with persisted selection, grammatical count captions, English fallback and bundled Cyrillic UI glyph coverage.
- Page transitions (`page.transition/transitions`, Page Transitions panel): twelve transition types with speed and direction per spread, written into exported PDFs as `/Trans` entries through an incremental update.
- PDF/X-4 export (`file.exportPdf {standard: "x4"}`): an output intent with our own Generic CMYK ICC profile (lut16 tables generated from the parametric press model), GTS_PDFXVersion in Info and XMP, `pdf:Trapped` in XMP as in Info, RGB images without a profile tagged sRGB, no image interpolation (none in PDF/A either), and built-in PDF/X-4 checks reported as warnings. qpdf finds no errors in the result.
- EPS place: placed at its (high-resolution) bounding box and shown, printed and exported through its TIFF preview, or a placeholder when it has none. PostScript itself isn't interpreted.
- Preferences: Dictionary (user dictionary; `spelling.setWords/words`), Spelling (Dynamic Spelling: red squiggles on the canvas, also Edit ▸ Spelling), Autocorrect (word list, applied as you type), Notes, Track Changes, Story Editor Display and File Handling (recovery interval) sections.
- Colour management exposed: Edit ▸ Color Settings (`color.settings`: RGB/CMYK working spaces, intent, black-point compensation; `color.loadProfile` for ICC files; `color.convert` through the working spaces, with gamut checks) and View ▸ Proof Setup / Proof Colors (press, sRGB, legacy Mac RGB, colour-blindness simulations, simulate paper) on the canvas.
- Keyboard: Tab / Shift+Tab hide and show all panels / all but Tools (`window.hidePanels`, `window.hidePanelsExceptTools`); Cmd+F6 / Cmd+Shift+F6 cycle documents; panel fields are reached with Tab and Escape returns to the layout.
- Smart guides and snapping (View ▸ Snap to Guides / Snap to Document Grid / Smart Guides, `view.snapPreferences` and Preferences ▸ Guides & Pasteboard): moving, drawing, resizing, rotating and pen/anchor drags snap in order to the document and baseline grids, ruler guides, margins and columns, other objects' edges and centres (parent-page items too, by visible stroked bounds), equal spacing, matching dimensions and matching rotation, with guides, gap marks and measurements drawn on the canvas. Shift and Cmd constrain the gesture.
- View ▸ Rotate Spread (`view.rotateSpread`, `layout.rotateSpreadView`): the spread in view turns in quarter turns on screen (stored with the document; output is unaffected). The canvas maps each spread through its own transform, so tools, snapping, resize handles, guides, overlays and rendering all follow the turned spread.
- Vertical type (Vertical Type Tool, Type ▸ Story Direction `type.storyDirection`, IDML StoryOrientation): the direction belongs to the story, so every frame of a thread is vertical or horizontal together and a frame threaded onto a story takes its direction (`object.textFrameOptions {vertical}` sets the frames' stories); lines run top to bottom and follow each other right to left; the arrow keys follow them (Down and Up along the line, Left and Right to the next and previous line, keeping the position along it); CJK characters stay upright and other text turns, on screen and in PDF; CJK line breaking with basic kinsoku (also in horizontal text); Korean breaks at spaces, word by word (Hangul in any language, hanja in Korean text), or between syllables when the paragraph asks for it (`type.para {koreanCharBreaks}`, Paragraph Style Options ▸ Justification; not written to IDML). Upright text is shaped top to bottom: punctuation and brackets take their vertical forms (OpenType `vert`; `vkrn` and `vpal` through OpenType features), glyphs advance by their vertical metrics (`vmtx`) and hang from their vertical origins (`VORG`) on the centre of the ideographic em box (`BASE`, else OS/2), which the first line's ascent follows; turned text keeps its horizontal forms. Tate-chu-yoko (Type ▸ Tate-Chu-Yoko, `type.tateChuYoko`, IDML `Tatechuyoko`) sets a run such as "12" side by side within one em of the line. Ruby (Type ▸ Ruby…, `type.ruby`, IDML RubyString; group ruby spread 1-2-1 or centred) and kenten emphasis dots (`type.kenten`, IDML KentenKind) are set over their text — to its right in vertical frames — without changing line breaks. Warichu and ruby options (per-character ruby, positions, overhang) aren't there yet.
- Math expressions (Type ▸ Insert Math Expression…, `math.insert`, `math.svg`): a LaTeX subset (fractions, scripts, roots, big operators with limits, Greek, relations and operators with math spacing, `\left…\right`, `\text`) typeset by our own engine into SVG, placed inline at the cursor or as a frame; the LaTeX is kept as alt text and can be edited.
- Object Layer Options for placed PDFs (`object.pdfLayers`, `object.layerOptions`, Object ▸ Object Layer Options…): the file's optional-content layers listed and shown or hidden per frame on screen; in exported PDFs such graphics go out as 300 ppi images of their visible layers.
- Effects: Inner Glow (edge or centre), Bevel and Emboss (inner bevel lit from the light angle), Satin (`object.innerGlow`, `object.bevel`, `object.satin`); PDF export now keeps soft effects: objects with shadows, glows, feathers, bevels or satin go out as 300 ppi transparent images of their appearance (threaded text frames excepted).
- Directional Feather (`object.directionalFeather {widths: [top, left, bottom, right]}`): each side fades over its own width.
- View ▸ Flattener Preview (`view.flattenerPreview`): objects involving transparency highlighted in red; knockout groups export as images (PDF output has no knockout groups).
- Live captions (`object.caption {live: true}`, Object ▸ Captions ▸ Generate Live Caption): the caption text is re-filled from the source object's metadata after every edit (same undo step).
- Style Export Tagging (`style.exportTag`, Paragraph Style Options ▸ Export Tagging): paragraph and character styles choose their HTML element and class in EPUB/HTML.
- Tagged Text: export (`file.exportText {format: "tagged"}`) and place (detected by its header; ASCII, UTF-8 or UTF-16) with paragraph and character styles, font, typeface and size, and `<0x…>` characters.
- Tagged PDF: one structure element per paragraph (H1–H6 and BlockQuote from Export Tagging, else P) in reading order, frame fills, strokes, rules and tables as artifacts, figures with alt text.
- EPUB: cover image from the first page (`file.exportEpub {cover}`), and Fixed Layout export (`file.exportFixedEpub`, File ▸ Export EPUB (Fixed Layout)…): pre-paginated pages as 144 ppi images with their text kept for search and read-aloud.
- Interactive PDF options (`file.exportPdf {fullScreen, bookmarksPanel, pageLayout, view, advanceSeconds}`, File ▸ Export PDF (Interactive)…): page mode, layout, opening view and page timing in the catalog and pages.
- Object Export Options (`object.exportOptions`): tagged-PDF artifact, EPUB/HTML rasterize (the object as an image), alignment and page break before, alt text.
- Scripts panel (Window ▸ Utilities ▸ Scripts, `script.run`): saved command scripts (with `$N.path` references to earlier results) run as one undo step; edit, add, load and delete them in the panel. Scripts use DesignCraft's command language, not ExtendScript.
- Drag and drop text editing: drag selected text to move it (Alt copies) with its formatting (`text.release`).
- Image Import Options: place any page of a multi-page PDF (`file.place {pdfPage}`; the app asks which page), shown on screen and embedded as that page in PDF export.
- Define Lists: named numbered lists that continue across stories in page order (`list.define`, paragraph `listName` / `startAt`), round-tripped through IDML; `null` in `type.para` / `type.char` now removes an override.
- View › Overprint Preview (⌥⇧⌘Y): overprinting fills, strokes and 100% [Black] text mix with what's beneath on screen.
- Type on a Path (⇧T tool, `type.onPath`): a story along any line, curve or shape on screen and in PDF; Type on a Path Options (start, flip, baseline/ascender/descender/centre) and Delete Type from Path. Round-trips through IDML as TextPath; on-path caret drawing is still to come.
- Preferences › Appearance of Black: 100% K on screen accurately (dark grey) or as rich black (`window.richBlack`); the printing/export side is still to come.
- Spreads of up to 10 pages: Add to Previous Spread and Allow Spread to Shuffle (Pages panel; `layout.pages.toSpread`, `layout.spreadShuffle`); inserted pages go around kept spreads.
- Page tool (⇧P) and `layout.pageSize`: pages of their own size (or a preset); the spine stays put and objects keep their place on their page.
- File › Print (⌘P): printer, copies, page range, spreads, marks, bleed; printed as PDF through the system spooler (`file.print`, `file.printers`, `dryRun` for agents). Shortcuts now act like choosing the menu item (dialogs open), and UI commands win shortcut ties (⌘N opens New Document).
- Nested styles (through/up to N sentences, words, characters, letters, digits, tabs, breaks, spaces or given characters) and GREP styles (regex → character style), edited in Paragraph Style Options and composed under local formatting; `ui.dialog.open` for agents. Nested line styles are still to come; nested and GREP styles round-trip through IDML.
- Table and cell styles (round-trip through IDML): cell styles (fill, insets, vertical justification, strokes, paragraph style) and table styles (region cell styles, border, alternating rows, spacing); apply from the Table panel, edits re-apply to every user; `style.cell.*`, `style.table.*`.
- Style groups: paragraph and character styles shown in group folders, Move to Group (new, existing, none); `style.group` renames every use (styles, stories, table cells, footnotes, object styles).
- Scrubby zoom: drag the Zoom tool left or right to zoom continuously around the press point.
- Guides belong to the active layer: hidden with it (or with its Show Guides off) and locked with it.
- Pencil tool (N): freehand strokes, shown as you draw in the layer's colour, simplified to smooth paths (Alt closes); Smooth and Erase tools re-fit or remove the stretch you drag along (`path.smooth`, `path.erase`).
- Edit › Keyboard Shortcuts: every command, click and press the new keys, conflicts shown, per-command and global reset; `window.setShortcut`.
- Primary Text Frame (New Document) and Smart Text Reflow: pages are added while the primary story oversets and empty ones at the end removed, in the same undo step (Preferences › Type).
- Glyphs panel: every character of any font (rendered by our renderer), search by character or U+code, click to insert, recently used.
- UI scaling (Preferences › Interface, 50–200%; `window.uiScale`); the window title steps aside when the bar is crowded.
- Underline / Strikethrough Options: weight, offset, colour and tint (Character panel; screen, PDF and IDML).
- Relink to Folder and Relink File Extension (`links.relinkFolder`; missing links found by name).
- File › Package (⌥⇧⌘P): the document relinked to a Links folder, the placed files, the font files that draw its text in a Document Fonts folder (the fonts it names and any fallback fonts that draw characters those lack; except fonts whose licence restricts it, OS/2 `fsType`), an IDML copy, an optional PDF and a report (fonts, fallback fonts, links, preflight, instructions); Copy Links To (`links.copyTo`).
- Document fonts: opening a `.designcraft` or `.idml` file loads the fonts in the `Document Fonts` folder beside it (as Package writes it), ahead of installed fonts with the same name, so a packaged document opens with its fonts on another machine. Damaged, oversized (over 256 MB per file, 512 MB in all) and surplus (over 200) font files are skipped and listed in `file.open`'s `warnings`; `font.list` gives each font's `source`. Document fonts belong to their document, as in InDesign: other open documents don't see them (each document with a same-named font draws with its own), and closing the document (or reverting it, or a book chapter's export ending) makes them unavailable. Their files stay in memory until DesignCraft quits, so reopening a document doesn't read or load them again; on the web there is no folder to read.
- Load Swatches / Save Swatches for Exchange (.ase: CMYK, RGB, Lab, Gray; spot or process) from the Swatches panel menu; `swatch.load` / `swatch.save`.
- Pathfinder (Object › Pathfinder and a Pathfinder panel): Add, Subtract, Intersect, Exclude Overlap, Minus Back; curves stay curves (flo_curves); `object.pathfinder`.
- Make / Release Compound Path (⌘8; nested paths become holes), Create Outlines (⇧⌘O; one path per text colour), Scissors tool (C) with `path.split`.
- Transform Again / Individually / Sequence Again (⌥⌘3; a moved copy repeats like Step and Repeat), Clear Transformations; Layers: Merge, Delete Unused, Hide/Lock Others, Show/Unlock All; Break Link to Style (paragraph and character).
- Floating panels: tear a panel off the dock (its float button, or drag its header away) into a movable, resizable window; Dock puts it back; positions persist; `window.floatPanel` / `window.dockPanel`.
- Place SVG (vector in PDF export via krilla-svg, resvg on screen, text in the bundled fonts), Photoshop (.psd/.psb composite in its colour mode — Bitmap, Grayscale, Duotone, Indexed, RGB, CMYK, Lab, 8/16-bit — with merged transparency; extra alpha and spot channels don't change the image), BMP, and Illustrator files saved with PDF compatibility (.ai). New `designcraft-images` crate (L2) for sniffing, sizes and decoding. EPS is still missing.
- View › Display Performance: Fast (grey boxes, no effects, ⌥⇧⌘Z), Typical (72 ppi proxies, ⌥⌘Z), High Quality (⌥⌘H, our default — the renderer is fast enough).
- OpenType menu (Character panel): discretionary ligatures, fractions, ordinal, swash, titling, contextual alternates, slashed zero, the four figure styles and stylistic sets 1–20; `type.openType`; mapped to IDML's OTF attributes.
- Add Anchor Point (=), Delete Anchor Point (-) and Convert Direction Point (⇧C) tools (click toggles smooth/corner, drag pulls out handles), with `path.addAnchor`, `path.deleteAnchor`, `path.convertAnchor`.
- Gradients: the Gradient Swatch tool (G) drags a gradient's start and end across objects (Shift: 45°); the Gradient panel sets type, angle, reverse and edits stops on a ramp (drag, add, drag off to remove, location, colour). Edited gradients are unnamed swatches; `object.gradient`; the vector round-trips through IDML.
- Document Setup (⌥⌘P): intent, number of pages, start page # (an even start begins with a left page), facing pages, size presets and orientation, bleed and slug per edge; `layout.documentSetup {}` reports the setup.
- Stroke panel: cap, join, miter limit, alignment, type, Start/End arrowheads (12 kinds, drawn on screen and in PDF; the path is shortened under pointed heads) and gap colour under dashes and dots.
- Preferences (⌘K: General, Type, Units & Increments, Grids, Guides & Pasteboard, Display Performance) backed by `prefs.set` (application) and `document.preferences` (units, increments, grids — undoable, saved with the document; right-click a ruler to pick its units, the horizontal and vertical rulers each have their own); scaling applies to content and scales stroke weights (Include Stroke Weight), and X/Y/W/H measure the stroke's outer edge (Dimensions Include Stroke Weight). Quick Apply (⌘Return) finds paragraph, character and object styles as well as commands.
- Parent item overrides: Cmd+Shift-click a parent item on a page (or Override All Parent Page Items, ⌥⇧⌘L) makes an editable local copy that hides the parent's; Remove All Local Overrides and Detach All Objects from Parent in the Pages panel menu.
- Place PDF: placed PDF pages are sized by their crop box, drawn on screen through hayro (with mip levels) and embedded in exported PDFs as vector form XObjects (krilla), `<PDF>` in IDML.
- Menus expose the engine's features (TOC, numbering & sections, text variables, hyperlinks, cross-references, footnotes, step and repeat, spelling, fitting/content, effects, Data Merge, Preflight, EPUB); any menu command with parameters gets a dialog generated from its parameter documentation.
- Table of contents (generate/update, dot leaders) and text variables (running headers, last page number, chapter number, file name, dates, custom) resolved per page.
- Soft effects: blurred drop shadow, inner shadow, outer glow, basic feather.
- ~200 commands, all reachable through the JSON control channel; headless CLI rendering to PNG.
- MCP server (`designcraft-cli mcp [--connect PORT]`, docs/mcp.md): headless engine or the running app; commands, batch, document/story inspection, page renders as images, window screenshots, pointer/keyboard/dialog input.
- Web build (`apps/designcraft-web`, trunk): the same UI on WebGPU with a WebGL2 fallback; open/place via the browser file picker or drag-and-drop, save/export as downloads.

Task notes that headed ROADMAP.md:

- FID1: PDF export normalizes mixed emitted gradient color spaces to RGB with an explicit warning; homogeneous device-space gradients retain their components. Linear/radial, midpoint, archival and print policies have synthetic regression coverage.
- AR1.1: Kashida justification tests verify space fallback without Tatweel fonts using an isolated font database, while retaining elongation assertions when fonts support it.
- AR1: Arabic IDML controls now retain character direction, kashida switches, diacritic offsets/forms, paragraph policy values and independent story/table directions. Shaping preserves joining context and script/language selection; paragraph-level bidi, contextual digit conversion, mark-safe elongation, RTL columns/tables and physical column insertion have regression coverage. Vendor-specific justification and diacritic presets remain explicit preflight warnings; see docs/arabic-typography.md.
- CJK1: IDML composite fonts now retain character mappings and metrics; explicit aki, tsume, jidori, em alignment/leading, named/custom kinsoku boundaries, hanging punctuation, tate-chu-yoko offsets and kenten symbols flow through composition and IDML export. Automatic kerning no longer imports inactive numeric values as enormous manual spacing. Composite definitions are editable through commands with undo/cache invalidation. Mojikumi definitions are retained and unsupported spacing-table / push-in-push-out policies are reported by preflight. Full InDesign CJK composition parity, frame-grid layout and detailed ruby remain unfinished. Warichu sets the selected run in smaller lines inside the line after the line is broken.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | minor | Full number 42% → 47%: method aligned with the standard (plain weighted sum, the ×0.9 discount dropped), no new evidence; readiness table with hours for full, mainstream and essentials; beta distance 33 → 28 points |
| 2026-10-10 | minor | Added mainstream practitioner (~42%) and essentials user (~54%) numbers with written weights and discounts and a user-evidence count; full number (42%) now written as 47.2% × 0.9, value unchanged |
| 2026-10-10 | minor | Stage alpha → pre-alpha under the core-workflow gate (manual threading missing); Type & text frames row updated |
| 2026-10-10 | major | Created from ROADMAP.md's "How far from full parity" (2026-10-04) and "Working today"; full re-measure against InDesign 2026 21.6: menu coverage measured against InDesign 21.6's menu dump (69%) and the documented tree (75%), breadth set at ~80%, ready-for-real-work re-estimated 83% → 42% on user-reported evidence and the INDD gap; hours recalibrated from git history |
| 2026-10-04 | major | (in ROADMAP.md) breadth 99% weighted over 275 rows, ~83% including depth, ~170 h remaining |
