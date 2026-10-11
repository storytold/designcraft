# UI and interaction parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-11 · **Change:** minor (document views row: scrollbars, Alt pans while editing text) · **Target:** Adobe InDesign 2026 (21.6, Medium Dark, macOS)

Tools, handles, snapping, nudging, modifiers, shortcuts, panels, menus, context menus and feel,
against InDesign 2026. The look was measured from the running app
(`plan/indesign/11-observed-ui.md`, local); behaviour from `plan/indesign/02-ui-ux.md`,
`05-tools.md`, `06-panels.md`, `07-dialogs.md` and `09-shortcuts.md`, and from user reports.
Overall numbers: [target-app-parity.md](target-app-parity.md).

**Dimension score: ~45% ready (estimated), 120–180 h.** The chrome looks like InDesign; the
basics of editing fail for too many users on Windows and Linux.

## Measured

| What | Value | How |
|---|---|---|
| InDesign 21.6 menu items matched (in scope) | 331 / 482 (69%) | script over the running app's menu dump; see target-app-parity.md |
| Menu bar | 9 menus, 49 submenus, 340 items | `MENUS` in `crates/ui-egui/src/menus.rs` |
| Commands | 446 engine (188 in menus) + 99 UI | `crates/engine/src/cmd/*`, `UI_COMMANDS` |
| Theme colours | measured hex values of Medium Dark, 4 brightness themes + High Contrast | `theme::Tokens` vs `11-observed-ui.md` §1 |

## Checklist

**✓** like InDesign · **~** present, differs or unreliable · **✗** missing.

| Area | InDesign 2026 | DesignCraft | Status | Evidence |
|---|---|---|---|---|
| Chrome: app bar, Control panel, Tools panel, dock, doc tabs, status bar | Medium Dark, measured sizes | measured look | ✓ | 11-observed-ui.md |
| Properties panel and Contextual Task Bar | per selection | ✓, task bar movable (#143) | ✓ | |
| Selection tool: click, marquee, move, Alt-duplicate, resize handles, rotate from corners | ✓ | ✓ | ✓ | |
| Direct Selection, Pen family, Pencil, Scissors | ✓ | ✓ | ~ | moving reference point "0" fails (#337) |
| Live corners (yellow handle) | ✓ | ✓ | ~ | not working for some users (#186) |
| Content grabber, frame fitting by double-click | ✓ | ✓ | ✓ | |
| Smart guides, snapping to guides/grid/baseline | ✓ | ✓ (2,049-line `snap.rs`) | ~ | vertical smart spacing between two rectangles (#188) |
| Nudging with arrows, Shift ×10 | ✓ | ✓ | ✓ | |
| Document views: scrollbars (each split pane), Hand tool by Space, middle drag, or Alt while editing text | ✓ | ✓ | ✓ | `canvas.rs` `scrollbar_tests` |
| Gridify while drawing, Gap tool, Live Distribute | ✓ | ✓ | ✓ | |
| Type tool: caret, selection, double/triple click, drag-and-drop text | ✓ | ✓ | ~ | formatting at a caret discarded (#289); text editing complaints (#23, #174) |
| Threading: click out port, load text cursor, click frame | ✓ | ✗ ports drawn only; no thread/unthread command; Primary Text Frame and autoflow on Place | ✗ | **alpha blocker** (#352, #194, #165, #140) |
| Copy / cut / paste objects and text, Paste Into / in Place | ✓ | ✓ on macOS | ~ | **failing on Windows/Linux** (#351, #348, #312, #185, #164, #201, #212) |
| Context menus (canvas, panels, rulers) | ✓ everywhere | 9 context-menu sites, ruler units by right-click | ~ | **not opening for several users** (#238, #187, #160) |
| IME text input on the canvas | ✓ | ✗ | ✗ | #322 |
| Font menu: search, preview, favourites, type-ahead | ✓ | ✓ | ~ | search field focus on macOS (#335, #86) |
| Keyboard shortcuts: InDesign defaults, editor, sets | ✓ | ✓ editor; defaults from 09-shortcuts.md | ✓ | |
| Quick Apply | ✓ | ✓ | ✓ | |
| Numeric fields: units, arithmetic, scrubbing | ✓ | 21 pt spinner fields with arithmetic | ✓ | |
| Colour fields: hex, picker from any swatch field | ✓ | ✓ | ~ | hex entry and picker issues (#323, #190) |
| Panels: Pages, Layers, Swatches, Color, Stroke, Character, Paragraph, Styles, Links, Effects, Text Wrap, Align, Pathfinder, Table, Index, TOC, Hyperlinks, Bookmarks, Articles, Tags, Notes, Track Changes, Object States, Buttons and Forms, Preflight, Data Merge, Library, Scripts, Glyphs, Info | ✓ | all present | ~ | **Tabs panel missing** (#326); Navigator, Trap Presets, Animation, Timing missing; pull-down panels can't scroll on Linux (#267) |
| Dialogs: New Document, Document Setup, Preferences (all pages), Paragraph/Character/Object/Table/Cell Style Options, Export PDF, Print, Package, Find/Change | ✓ | present | ~ | generated command dialogs fill the long tail; depth varies |
| Workspaces | Essentials, Advanced, Book, Digital Publishing, Interactive for PDF, Printing and Proofing, Typography, Touch | ✓ named workspaces + custom | ~ | tab bar overlaps menus in Book/Advanced (#182) |
| Story Editor | ✓ | ✓ | ~ | style-name column, depth ruler missing |
| Undo granularity (a drag = one step) | ✓ | ✓ (Begin/Preview/Commit) | ✓ | |
| Closing with unsaved changes asks | ✓ | ✓ (#146) | ✓ | |
| UI scaling, System appearance | ✓ | scaling ✓ (#281); follows macOS light/dark ✗ (#237) | ~ | |
| Window controls, full screen | ✓ | ~ | ~ | misaligned on macOS (#333) |

Counted 2026-10-11: 28 rows, 12 ✓, 14 ~, 2 ✗ → **68% presence**, scoring ✓ = 1, ~ = 0.5. Ready
for real work is lower, ~45%, because threading is missing and three of the ~ rows (copy/paste,
context menus, typing at a caret) are things a layout artist does every minute, and their failures stop work.

## Remaining effort

| Work | Opus 5.5 h |
|---|---|
| Copy/paste, context menus, threading clicks, caret formatting on every platform, with control-channel regression tests | 30–50 |
| Canvas IME (port from VectorCraft) | 10–20 |
| Missing menu items (~150) and panels (Tabs, Navigator, Trap Presets) | 40–70 |
| Handle/tool feel fixes (#186, #188, #337), font search focus | 10–20 |
| Dialog depth pass against 07-dialogs.md | 30–40 |
| **Total** | **120–180** (overlaps gaps 1, 8, 16) |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | minor | Threading row corrected to ✗ after checking the code (no out-port interaction, no thread command) |
| 2026-10-10 | major | Created: measured menu coverage, 27-row interaction checklist from the code and issues |
