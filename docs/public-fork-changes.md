# INDD files, tools and guide controls

This public contribution includes no serial-number or activation requirement. Its scope is INDD files, control and contextual bars, toolbox colour controls, and ruler guides.

The branch is updated to current DesignCraft rather than replacing its newer import and toolbox implementations. INDD/INDT opening, embedded missing-link previews, selected-guide highlighting and X/Y positioning, and document drawing-colour defaults are already present upstream and are retained.

## Changes in this update

- The Control bar is visible in new preferences. Older saved preferences receive a one-time restoration of the Control and contextual bars, with any off-screen contextual pin cleared. Later visibility and pin choices are respected.
- The contextual bar stays within the canvas even when the selected frame is off-screen.
- The fill/stroke swap arrow has a larger clickable target.
- Page size, liquid-page rule, character style and language dropdowns in the now-visible Control bar have accessible names.
- Selecting one guide shows **Copy to** beside its position in the Control bar. Vertical guides use X; horizontal guides use Y. The destination uses the same ruler origin and units as the position field. For example, a vertical guide at X = 100 pt copied to 200 pt leaves the original at 100 pt and selects the new guide at 200 pt.
- `guide.copy` preserves the guide's orientation, layer, colour and guide properties. It validates its destination and source, respects locked or hidden layers, caps guide count, and creates one undo step. Undo restores the previous guide selection.
- Open-file labels refer to **INDD files**, **INDT templates** and **IDML files**.

## Source map

- `crates/engine/src/cmd/guides.rs`: the `guide.copy` command and synthetic regression tests.
- `crates/ui-egui/src/object_control.rs`: ruler-aware Copy to control.
- `crates/ui-egui/src/lib.rs` and `prefs.rs`: control visibility default and preference migration.
- `crates/ui-egui/src/canvas.rs` and `taskbar.rs`: contextual bar visibility and placement.
- `crates/ui-egui/src/toolbar.rs`: fill/stroke swap hit target.
- `crates/ui-egui/src/chrome.rs`: accessible Control bar dropdown names.
- `crates/ui-egui/src/theme.rs`: existing bundled font fallback for keyboard symbols, with a regression test independent of optional or installed fonts.
- UI test fixtures: distinguish the Properties orientation button from its Control bar counterpart; use host-correct Command/Control modifiers and shortcut labels, install the application fonts in panel-only fixtures, and skip the optional Arabic-font portion when its build input is absent.
- `apps/designcraft/src/main.rs`: file-picker labels.
- `crates/pdf/src/gradient_inks.rs` and PDF export test helpers in `crates/engine/src/cmd/export.rs`: equivalent Boolean expressions to satisfy the current Clippy gate; no rendering behavior change.

Malayalam input, Manglish, InScript, FML conversion, Thiyaara Effects, customer records, payments and activation code are excluded from this contribution. Existing open-source notices remain applicable.

## Validation

The repository's required gate is `cargo xtask ci`: formatting, PDF dependency checks, Clippy, workspace tests, translation reporting, doctests, asset attribution, dependency layers and WebAssembly checks. The regression tests cover copy/undo/redo, invalid destinations, guide limits and layer protection, toolbar preference migration and off-screen contextual-bar placement. Native Windows installer testing is separate from these source checks.
