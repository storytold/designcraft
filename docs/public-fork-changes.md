# Public fork changes

Scope: INDD opening, Control/contextual bars, toolbox colour controls, guide selection, movement and duplication. Base: upstream release 0.8.0, commit 11a51d5b. No Malayalam input, FML conversion, Effects, registration, payment or activation additions are included.

## Source map

- `Cargo.toml`, `Cargo.lock`, `crates/engine/Cargo.toml`: pinned INDD converter dependency.
- `crates/engine/src/cmd/file.rs` and `interchange.rs`: recognise INDD/INDT, convert in memory, pass warnings and embedded previews to the importer. No activation gate.
- `crates/idml/src/{import,lib,tests}.rs`: missing-link preview support and regression coverage.
- `apps/designcraft/src/main.rs`, `apps/designcraft-web/src/web.rs`, `crates/ui-egui/src/lib.rs`: open-file filters and drop recognition; first-run control-bar migration.
- `crates/ui-egui/src/{chrome,canvas}.rs`: selected-guide highlight, X/Y editing and Copy to, with revision-scoped selection to avoid stale guide indices after edits or Undo.
- `crates/engine/src/cmd/guides.rs`: undoable `guide.copy` command, original preserved, orientation retained.
- `crates/ui-egui/src/taskbar.rs`: contextual bar positioning and draw order.
- `crates/ui-egui/src/panel_docking_tests.rs`: explicit fixture geometry so the Story Editor overlay test also works with the top Control bar visible; both blocking and allowed-release assertions remain.
- `crates/ui-egui/src/toolbar.rs`, `panels/mod.rs`, `crates/engine/src/lib.rs`, `cmd/object.rs`: fill/stroke defaults and swap behaviour, toolbox buttons and drawing colour propagation.

Only selected changes from the development tree were extracted. Unrelated tools, margins/layout changes, product branding changes and all commercial modules were left out. Existing upstream features remain.

## Validation

The earlier combined Mac development build passed native checks for guide selection without movement, X/Y editing, Copy to preserving originals, and Undo in both orientations. Those results do not certify this separately extracted build.

Checks completed on 26 October 2026 for this public-only checkout:

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo xtask assets`: all 58 assets attributed.
- Candidate-file scan: no commercial module references, activation keys, private credential patterns or private machine paths found.
- `cargo test --workspace`: 1,581 passed, 0 failed, 5 ignored across 58 test suites (including documentation tests).
- `cargo xtask layers`: 20 crates, no layering violations.
- `cargo xtask wasm`: all 15 selected WebAssembly crates checked successfully. An existing unused-variable warning remains in the unchanged `panels/library.rs` file.
- `cargo xtask ci`: all six required stages passed after adapting the docking test fixture to the visible Control bar.

No new Windows installer or Microsoft Store update is included in this source contribution. The checks above ran on macOS; native Windows and Linux installer testing was not performed for this source publication.
