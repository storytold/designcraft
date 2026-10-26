# UI and interaction parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-24 · **Change:** minor (Story panel; text Control panel Character/Paragraph modes; Properties ••• expands Character and Paragraph; live corners work; document views row: scrollbars, Alt pans while editing text; Tabs panel listed as present (#343); spring-loaded Zoom navigation; paragraph style override feedback and clearing) · **Target:** Adobe InDesign 2026 (21.6, Medium Dark, macOS)

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
| Chrome: app bar, Control panel, Tools panel, dock, doc tabs, status bar | Medium Dark, measured sizes | measured look; the text Control panel switches between Character (A) and Paragraph (¶) Formatting Controls (¶ with the Bulleted and Numbered List buttons, Shading and Border), and Escape in a field reverts what was typed | ✓ | 11-observed-ui.md; Learn Skills Daily course, chapter 16 |
| Properties panel and Contextual Task Bar | per selection | ✓, task bar movable (#143); ••• expands Character (scales, baseline shift, skew, language, case and position buttons) and Paragraph (indents, spacing, drop cap, baseline grid, hyphenation) in place | ✓ | Learn Skills Daily course, chapter 16; `properties.rs` `more_options_expand_character_and_paragraph_in_place` |
| Selection tool: click, marquee, move, Alt-duplicate, resize handles, rotate from corners | ✓ | ✓ | ✓ | Cmd/Ctrl held with another tool borrows the selection tool used last (Selection or Direct Selection) for one gesture, #919 |
| Direct Selection, Pen family, Pencil, Scissors | ✓ | ✓ | ~ | moving reference point "0" fails (#337) |
| Live corners (yellow handle) | ✓ | ✓ widget on a selected rectangle frame; diamonds drag the corner size (Shift: one corner), Alt-click steps the shape (Shift+Alt: all) | ✓ | #186 |
| Content grabber, frame fitting by double-click | ✓ | ✓ | ✓ | |
| Overset mark (red + at the out port) | selected frame only | every overset frame, groups included, while frame edges show | ✓ | a deliberate step past InDesign: grouped labels that overflow were invisible until Preflight |
| Smart guides, snapping to guides/grid/baseline | ✓ | ✓ (2,049-line `snap.rs`) | ~ | vertical smart spacing between two rectangles (#188) |
| Nudging with arrows, Shift ×10 | ✓ | ✓ | ✓ | |
| Document views: scrollbars (each split pane), Hand tool by Space, middle drag, or Alt while editing text | ✓ | ✓ | ✓ | `canvas.rs` `scrollbar_tests` |
| Gridify while drawing, Gap tool, Live Distribute | ✓ | ✓ | ✓ | |
| Type tool: caret, selection, double/triple click, drag-and-drop text | ✓ | ✓ | ~ | formatting at a caret discarded (#289); text editing complaints (#23, #174) |
| Threading: click out port, load text cursor, click frame | ✓ | ✗ ports drawn only; no thread/unthread command; Primary Text Frame and autoflow on Place | ✗ | **alpha blocker** (#352, #194, #165, #140) |
| Copy / cut / paste objects and text, Paste Into / in Place | ✓ | ✓ on macOS | ~ | **failing on Windows/Linux** (#351, #348, #312, #185, #164, #201, #212) |
| Context menus (canvas, panels, rulers) | ✓ everywhere | 9 context-menu sites, ruler units by right-click | ~ | **not opening for several users** (#238, #187, #160) |
| IME text input on the canvas | ✓ | ✓ | ~ | not checked by hand with a system IME (#322) |
| Font menu: search, preview, favourites, type-ahead, filters (class, recently added), styles per family, sample text and size, recently used, similar fonts | ✓ | ✓ | ~ | search field focus on macOS (#335, #86); Adobe Fonts (Find More, activated) out of scope |
| Keyboard shortcuts: InDesign defaults, editor, sets | ✓ | ✓ editor; defaults from 09-shortcuts.md; panels take keys (InDesign's F5–F12 defaults where free) and tool keys are editable; imports an InDesign Show Set `.txt`, panel and tool items included | ✓ | no Object Styles or Transform panel to give Cmd+F7 or F9; Links (Cmd+Shift+D) keeps DesignCraft's owner of that key |
| Quick Apply | ✓ | ✓ | ✓ | |
| Numeric fields: units, arithmetic, scrubbing | ✓ | 21 pt spinner fields with arithmetic | ✓ | |
| Colour fields: hex, picker from any swatch field | ✓ | ✓ | ~ | hex entry and picker issues (#323, #190) |
| Panels: Pages, Layers, Swatches, Color, Stroke, Character, Paragraph, Styles, Links, Effects, Text Wrap, Align, Pathfinder, Table, Index, TOC, Hyperlinks, Bookmarks, Articles, Tags, Notes, Track Changes, Object States, Buttons and Forms, Preflight, Data Merge, Library, Scripts, Glyphs, Info, Tabs, Story | ✓ | all present | ~ | Navigator, Trap Presets, Animation, Timing missing; pull-down panels can't scroll on Linux (#267) |
| Dialogs: New Document, Document Setup, Preferences (all pages), Paragraph/Character/Object/Table/Cell Style Options, Export PDF, Print, Package, Find/Change | ✓ | present | ~ | generated command dialogs fill the long tail; depth varies |
| Workspaces | Essentials, Essentials Classic, Advanced, Book, Digital Publishing, Interactive for PDF, Printing and Proofing, Review, Typography, Touch | ✓ named workspaces (Essentials Classic: Control panel and iconic panel groups) + custom; Window › Workspace; no Review or Touch | ~ | tab bar overlaps menus in Book/Advanced (#182) |
| Story Editor | ✓ | ✓ | ~ | style-name column, depth ruler missing |
| Undo granularity (a drag = one step) | ✓ | ✓ (Begin/Preview/Commit) | ✓ | |
| Closing with unsaved changes asks | ✓ | ✓ File › Close and window/menu quit (#146, #545) | ✓ | macOS Dock Quit still bypasses the prompt (#544) |
| UI scaling, System appearance | ✓ | scaling ✓ (#281); follows macOS light/dark ✗ (#237) | ~ | |
| Window controls, full screen | ✓ | ~ | ~ | misaligned on macOS (#333) |

Counted 2026-10-11: 28 rows, 12 ✓, 14 ~, 2 ✗ → **68% presence**, scoring ✓ = 1, ~ = 0.5. Ready
for real work is lower, ~45%, because threading is missing and three of the ~ rows (copy/paste,
context menus, typing at a caret) are things a layout artist does every minute, and their failures stop work.

Navigation also supports held Space as Hand and held Cmd+Space as Zoom (Ctrl+Space on
Windows/Linux), with Alt-click zooming out. Releasing restores the previous tool and its state.
The temporary Zoom uses the selected Zoom tool's click and horizontal scrubby drag behavior;
marquee zoom is not implemented. It is suppressed while typing in a document text frame or a
focused text field, and does not interrupt an existing tool drag. Synthetic canvas input tests
cover the cursors, click anchors, scrub/release ownership, restoration, typing guards and undo
history (DC-1).

## Remaining effort

Applying a paragraph or character style now removes local values equal to the applied style,
so text used to capture a style follows later edits to that style. Different local formatting
and named character styles stay intact. Paragraph-style application treats selection endpoints
as exclusive; Clear Overrides removes character formatting only inside a nonempty selection.
Regressions cover the capture/edit workflow at 2:16 and 6:02 in
[Jessica Garrity's styles tutorial](https://www.youtube.com/watch?v=pG9JkAPLgs8).

Redefining a character style at a caret also normalizes the sampled source run, so subsequent
style edits reach that text while different local overrides in adjacent runs remain intact.

The selected style's context menu now offers Redefine Style for paragraph and character styles.
It incorporates the source text's local changes, updates every use, and removes matching source
overrides in one undo step. Named character styles remain separate when redefining a paragraph
style. Commands: `style.paragraph.redefine` and `style.character.redefine` (optional `name`,
which must match the source). This implements the global color, size and spacing updates at
about 5:10–6:00 in [Jessica Garrity's styles tutorial](https://www.youtube.com/watch?v=pG9JkAPLgs8).

The Paragraph Styles panel now includes character formatting in its `+` indicator, on both
the current-style summary and the selected row. The row menu offers Apply "name", Clear
Overrides through the existing command, retaining named character styles. Selection attribute
counts compare effective values with their style definitions, so equal local values and named
character styles alone do not report overrides. This covers the color-override/clear workflow
at 4:25–5:20 in [Jessica Garrity's styles tutorial](https://www.youtube.com/watch?v=pG9JkAPLgs8).

Style Preview rejects conflicting scalar/nested automation fields without a panic. Replacing a
previewing dialog through the control channel cancels its transaction, restoring the original
document and normal undo recording before the replacement opens.

Paragraph and Character Style Options now preview edits to existing styles. Preview updates
every use, remains outside undo history, and restores the starting document when disabled or
cancelled. OK applies the final edit once. The color-preview step at about 4:00 in
[Jessica Garrity's styles tutorial](https://www.youtube.com/watch?v=pG9JkAPLgs8) is reproducible
offscreen. New-style creation, renaming and export tagging are committed on OK; they are not
included in the visual preview.

Paragraph Style Options' General pane shows a Style Settings summary of differences from its
parent, including captured color, size and spacing. Pending edits appear before OK without
changing the document. Fully captured default attributes do not overwhelm the summary. This
covers checking the sampled and redefined settings in the styles tutorial (2:45 and 5:15).
| Work | Opus 5.5 h |
|---|---|
| Copy/paste, context menus, threading clicks, caret formatting on every platform, with control-channel regression tests | 30–50 |
| Canvas IME: checks by hand with system IMEs, and fixes | 10–20 |
| Missing menu items (~150) and panels (Tabs, Navigator, Trap Presets) | 40–70 |
| Handle/tool feel fixes (#188, #337), font search focus | 10–20 |
| Dialog depth pass against 07-dialogs.md | 30–40 |
| **Total** | **120–180** (overlaps gaps 1, 8, 16) |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-24 | minor | Panels row: Story panel (Optical Margin Alignment and its size) |
| 2026-10-21 | minor | Chrome row: the text Control panel's A/¶ modes; Properties row: ••• expands Character and Paragraph in place |
| 2026-10-11 | minor | Live corners row to ✓: widget, diamonds, drag, Shift-drag, Alt-click shape cycling (#186) |
| 2026-10-11 | minor | Added held Cmd+Space / Cmd+Alt+Space Zoom navigation and canvas input regression tests |
| 2026-10-10 | minor | Threading row corrected to ✗ after checking the code (no out-port interaction, no thread command) |
| 2026-10-10 | major | Created: measured menu coverage, 27-row interaction checklist from the code and issues |
