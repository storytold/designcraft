# DesignCraft architecture

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** major (first committed version, from the code on main and the local plan) · **Target:** Adobe InDesign 2026

How DesignCraft is built today. The design notes behind it are in `plan/architecture.md`
(local, gitignored); this file describes what the code on `main` does. Agents: AGENTS.md has the
rules (never crash, everything is a command, layering, clean-room).

## Workspace

A Cargo workspace: 16 library crates in `crates/`, 3 apps in `apps/`, and `xtask`. 126,411
lines of Rust, 1,122 tests (2026-10-10). Pure Rust, no C dependencies by default,
`unsafe_code = "forbid"` workspace-wide.

| Layer | Crate | What it does | Lines |
|---|---|---|---|
| L0 | `geom` | kurbo re-export, units and measurement parsing (`1p6`, `12mm`, `3in+2p`), corner shapes, polygons, transforms, hit testing | 2.5k |
| L0 | `color` | CMYK/RGB/Gray (Lab in CMS), swatches, tints, gradients, blend modes, ASE, ICC colour management (`cms/`), colour harmony | 2.9k |
| L1 | `doc` | the document model: spreads, pages, parents, layers, items, stories (text + run lists), tables, notes, styles of five kinds, swatches, sections, variables, CJK and Arabic attributes | 10.2k |
| L1 | `fonts` | font database (bundled OFL fonts, system fonts, document fonts, optional craft-fonts), metrics, outlines (skrifa), shaping (harfrust), variable instances, font classification for menus | 3.5k |
| L2 | `compose` | the text engine: style resolution, shaping, Knuth–Plass and single-line breaking, Liang hyphenation, justification, tabs, keeps, columns, span/split, baseline grid, vertical justification, wrap, threading, overset, anchored objects, footnotes, tables, bidi, CJK (ruby, warichu, kinsoku), caret and hit testing | 11.7k |
| L2 | `images` | decoding placed graphics (PNG, JPEG, GIF, WebP, TIFF incl. CMYK, BMP, PSD composite, EPS preview) | 1.6k |
| L3 | `render` | document to pixels with vello_cpu (SIMD, multithreaded): items, composed text, images with mips, gradients, effects, damage regions, placed PDF via hayro | 3.5k |
| L3 | `pdf` | PDF export with krilla: real text with subset fonts, CMYK and spot, bleed and marks, tagged PDF, links, forms, transitions, PDF/X-4 and PDF/A-2b | 5.0k |
| L3 | `idml` | IDML import and export (designmap, spreads, parents, stories, styles, graphics, preferences; CJK and Arabic attributes) | 8.0k |
| L3 | `epub` | EPUB 3 reflowable and fixed-layout export | 0.9k |
| L3 | `textimport` | Word (.docx), RTF, Excel (.xlsx), Tagged Text import; text export | 1.8k |
| L3 | `format` | the native `.designcraft` zip (`document.json`, `assets/`, `meta.json`), versioned | 0.3k |
| L4 | `tools` | tool state machines: pointer events and modifiers to commands and overlays; snapping and smart guides | 5.9k |
| L5 | `engine` | `Session`: documents, history, selection, the command registry (446 commands in `cmd/`), compose cache, preflight, find/change, scripts, recovery, links | 29.3k |
| L6 | `ui-egui` | the InDesign-style egui frontend: menus, Control panel, Tools, dock and panels, dialogs, canvas, theme tokens, i18n, the control channel | 33.4k |
| L6 | `mcp` | MCP server over stdio, headless or proxying to the running app | 1.7k |
| L7 | `apps/designcraft` | desktop binary (eframe/wgpu), file dialogs, control server | 1.6k |
| L7 | `apps/designcraft-cli` | headless CLI: `run`, `script`, `app`, `describe`, `commands`, `mcp` | 1.2k |
| L7 | `apps/designcraft-web` | the same UI in WASM (WebGPU, WebGL2 fallback) | 0.3k |

`cargo xtask layers` enforces the layering: a crate depends only on lower layers (with the
intra-layer orders geom → color and doc → fonts); nothing below L6 depends on egui, eframe, winit
or rfd; L0–L6 compile for `wasm32-unknown-unknown` (`cargo xtask wasm`).

## Data model

Coordinates are points (1/72 in), y down; each spread has its own coordinate system with its
origin at the top left of its first page. A `Document` holds settings, spreads and parent
spreads (items under `Arc`, so edits are copy-on-write and undo is cheap), layers, stories,
styles (paragraph, character, object, table, cell; based-on chains and groups), swatches,
sections and text variables. A story is one `String` plus paragraph and character run lists of
`{len, style, overrides}`; effective attributes are default ← based-on chain ← style ← overrides.
Special characters (page number, footnote reference, anchored object, table) are placeholder
code points in the text. Threading is the story's ordered frame list.

## Commands, the only way to change a document

Every user-visible action is a `CommandSpec` (`id`, `label`, menu path, shortcut, parameter
doc, `enabled`, `run`, journaled, undoable) in `crates/engine/src/cmd/*`. The UI, the CLI, MCP
and the control channel all call `Session::execute(id, params)`. UI-only commands (windows,
panels, zoom, dialogs) are `UI_COMMANDS` in `crates/ui-egui/src/menus.rs`, reachable the same way.
Tool gestures emit Begin/Preview/Commit commands so a drag is one undo step. Every journaled
command is replayable as a script. Escaped panics during a command or an import/export become an
error (`engine/src/guard.rs`), never a lost document; unsaved documents are written to a recovery
folder every 30 s.

## Composition and rendering

The compose cache is keyed by story revision, frame geometry, style revision and font database
revision; stories compose in parallel (rayon) on native targets. Composition produces per-frame
lines with positioned glyph runs, decorations, anchors and overset. The canvas renders on a
background worker with vello_cpu into tiles; after an edit only frames whose composition changed
are re-rendered and patched (damage regions, pixel-identical to a full render). The GPU (wgpu:
Metal, DirectX 12, Vulkan, OpenGL; WebGPU/WebGL2 on the web) only presents; the backend is chosen
before the window exists, with a start-up fallback list in `gpu.json`, and the adapter (the GPU
that drives the display first) by the app, which starts again on the next one when an adapter
fails or hangs at start-up (`apps/designcraft/src/gpu.rs`).

## File I/O

Native `.designcraft` (zip + JSON); IDML both ways; PDF, EPUB, HTML, PNG/JPEG, text, XML export;
placed graphics through `images` (PDF/AI pages through hayro); Word/RTF/Excel/Tagged Text through
`textimport`. INDD is not readable. Details: [file-format-parity.md](file-format-parity.md).

## Agent control

The desktop app opens a loopback JSON-lines control channel (`--control PORT`) with
`engine.execute`, inspection, real input injection (`ui.pointer`, `ui.key`, `ui.click`, `ui.drag`),
dialog control, screenshots and offscreen renders. `designcraft-cli mcp` exposes the same as MCP
tools, headless or connected to the app. See [agents.md](agents.md), [mcp.md](mcp.md),
[control-protocol.md](control-protocol.md).

## Quality gates

`cargo xtask ci`: rustfmt, clippy `-D warnings`, tests, `assets` (every asset attributed in
ASSETS.md), `layers`, `wasm`. Clean crates deny `unwrap`/`expect`/`panic!`/`todo!`/
`unimplemented!`/`unreachable!`. CI on GitHub currently runs only the FreeBSD and Windows ARM64
builds (on manifest changes), packaging lint and the release workflow; `xtask ci` runs locally.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | major | Created from the code on main (92649ff) and `plan/architecture.md` |
