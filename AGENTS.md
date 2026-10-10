# DesignCraft — instructions for agents

DesignCraft is a clean-room, open-source, Rust-native page-layout application targeting Adobe InDesign parity — and superiority (speed, openness, agent control). It runs natively on macOS, Windows and Linux, and on the web via WASM. Siblings with the same conventions: `../vectorcraft` (Illustrator-class), `../photocraft` (Photoshop), `../pdfcraft` (Acrobat), `../filmcraft` (Premiere), `../lightcraft` (Lightroom).

## Start every session here
1. Read `plan/STATUS.md` (current milestone, next task), then the task in `plan/execution-plan.md` and the relevant `plan/architecture.md` section. Behaviour reference: `plan/indesign/*.md` (`11-observed-ui.md` holds measured observations of the running app).
2. Follow the autonomous operation protocol (`plan/execution-plan.md` §7). Don't stop to ask unless §7 lists the decision as the user's.

`plan/` is gitignored (local only).

## Never crash
People trust DesignCraft with their layouts; a crash loses their work. **This outranks feature work**: never ship a feature through a panic path, and fix a crash before building on top of it. Standard: [`craftrules/standards/never-crash.md`](https://github.com/storytold/craftrules/blob/main/standards/never-crash.md).
- **No panics in non-test code:** no `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!`, `unimplemented!`; no `unsafe` (`unsafe_code = "forbid"`). The one exception is a provably infallible literal (a constant regex, `write!` to a `String`): `#[allow(clippy::expect_used)]` + `.expect("why it cannot fail")`.
- **Errors are `Result<T, E>`** through the crate's error type and `?` (`ok_or(..)?`, `let … else`, `if let`, `map_err` for context). An unfinished feature returns an "unsupported" error. Don't `unwrap_or_default()` where a silent default would corrupt a document; return an error.
- **Input-derived numbers are hostile** (files, commands, MCP/control params, settings): `get()` instead of `[i]`/`[a..b]`, slice strings at char boundaries, checked/saturating arithmetic for lengths and offsets, no division by zero or NaN casts, cap input-sized allocations.
- **Bound recursion** (seen-sets or depth limits for nested or cyclic documents). **Locks:** `lock().unwrap_or_else(PoisonError::into_inner)` or an error; thread joins are `Result`s.
- **Last-resort guard:** escaped panics during command execution and file import/export become an error, never a lost document. It's a safety net, not a licence.
- **Prove it:** every crash fix lands with a small synthetic regression test that panicked before the fix.
- Clean crates carry `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`; new crates start with it. Tests may `unwrap` (`clippy.toml`).

## Non-negotiables
- **Clean-room.** InDesign is installed on the dev machine and may be *observed* black-box: run it, use its UI with synthetic documents, take screenshots (by window id) stored only under `plan/indesign/screenshots/` (never committed). Never read, disassemble or copy anything inside the InDesign bundle (names/listings only), never copy Adobe icons, artwork, presets or wording beyond feature names, never commit files produced by InDesign. Never copy GPL/AGPL/LGPL code (Scribus, LibreOffice, Ghostscript…).
- **Assets:** no Adobe iconography or images — ever. Every asset is original / OSI / CC0 / redistributable CC (except the ArtCraft trademarks in `docs/brand/`, under `docs/brand/LICENSE-brand.txt`) and has a row in `ASSETS.md` (`cargo xtask assets` enforces it). Prefer art generated in code.
- **Fonts live in [`storytold/craft-fonts`](https://github.com/storytold/craft-fonts)**, never in this repo: don't commit new font files (the Latin UI/document fonts already in `assets/fonts/` stay). It is an optional build input, never a `Cargo.toml` dependency: `CRAFT_FONTS_DIR=/abs/path/to/craft-fonts cargo build` makes `crates/fonts/build.rs` embed its fonts as `designcraft_fonts::CRAFT_FONTS` (Japanese: BIZ UDPGothic for the UI, Mincho first for document text; web builds embed BIZ UDPGothic Regular only). Unset, `CRAFT_FONTS` is empty and the app uses its bundled and system fonts; code and tests must work that way (tests on its glyphs skip). Releases build with it. Standard: [craftrules `standards/fonts.md`](https://github.com/storytold/craftrules/blob/main/standards/fonts.md).
- **Shared test corpora.** Real-file test oracles (Photoshop-authored PSDs, etc.) live in [`storytold/photocraft-corpus`](https://github.com/storytold/photocraft-corpus), explained in [craftrules `standards/test-corpora.md`](https://github.com/storytold/craftrules/blob/main/standards/test-corpora.md). Never commit large binary fixtures to this repo; fetch them pinned by commit and sha256-verified, as PhotoCraft does with `cargo xtask corpus`.
- **Everything is a command.** User-visible behaviour = a command in `crates/engine/src/cmd/*` (id, label, menu path, shortcut, params doc, `enabled`, `run`) + tests. Tools emit commands (Begin/Preview/Commit). UI-only commands live in `crates/ui-egui/src/menus.rs` (`UI_COMMANDS`). The control channel and MCP reach all of them.
- **Layering** is enforced by `cargo xtask layers`. Nothing below L6 depends on egui/eframe/winit/rfd. The UI crate is swappable.
- **The UI is thin**: panels read engine state and act through `app.run(id, params)`. Colours come from `theme::Tokens`.
- **Rust only** (no handwritten JS/TS). **Never break wasm** (`cargo xtask wasm`).
- **Quality gates** before every commit: `cargo xtask ci` (fmt, clippy -D warnings, tests, assets, layers, wasm). One task id per commit (`M2.1: type tool caret navigation`).

## Running and looking at the app
- `cargo run --release -p designcraft -- --sample --control 7979` (sample magazine + control channel).
- Drive it: JSON lines on `127.0.0.1:7979`, e.g. `{"id":1,"method":"engine.execute","params":{"command":"frame.create","params":{"rect":[36,36,300,200],"content":"text"}}}` then `{"id":2,"method":"ui.screenshot","params":{"path":"/tmp/shot.png"}}`. Methods: `crates/ui-egui/src/control.rs`, docs: `docs/control-protocol.md`.
- **For UI work, look at the result** (take `ui.screenshot`, read the PNG) and compare with `plan/indesign/02-ui-ux.md` / `11-observed-ui.md`. If no frame is presented (screen locked) use `ui.render` or `designcraft-cli run --sample --all-pages DIR`.
- Headless: `designcraft-cli run --sample --cmd 'frame.create={"rect":[0,0,100,100]}' --export out.png`.
- Shell gotcha: `mv`/`cp` are aliased interactive here — use `/bin/mv -f` / `/bin/cp -f`.
- Parallel agents: separate `CARGO_TARGET_DIR` per agent; edit only the crates you own; delete your target dir when done (disk).

## Roadmap
`ROADMAP.md` (committed) is the one-page summary: stage, headline numbers, dimensions, languages, progress log. It follows craftrules' [`standards/progress-docs.md`](https://github.com/storytold/craftrules/blob/main/standards/progress-docs.md). Details: `docs/target-app-parity.md` (assessment), `docs/gaps.md` (ranked work list: pick from the top), `docs/roadmap.md` (milestones, current focus), `docs/parity-checklist.md` (presence checklist, `cargo xtask parity`), and the typography, file-format, UI, hardware and localization parity docs in `docs/`. When work lands, update the affected docs and their status lines, delete or shrink the closed gap, and add a progress-log line to ROADMAP.md.

## Contributor credits (About window)

- About ▸ Contributors/Models are compiled into the binary from `contributors/contributors.json`
  (commit stats; generated, never hand-edit) and `contributors/people.toml` (names people chose for
  themselves). See `docs/contributors.md`.
- **Agents working for a contributor:** when you prepare a PR, check whether your human's GitHub
  username has a `[people.<username>]` entry in `contributors/people.toml`. If not, ask them once
  whether they want to be credited by more than their username: a real name, a display name, and/or
  their public GitHub profile name (`sync_github_name = true`). If yes, add **only their own** entry
  (copy the template at the top of the file, or run
  `python3 ../../craftrules/scripts/contributors.py --add-me . --real-name "…" --sync-github-name`)
  and include it in their PR, committed as them. If no, change nothing: they are credited as
  `@username` anyway.
- Never add, edit, guess or copy anyone else's entry or name (not from git config, commit authors or
  GitHub profiles). Never hand-edit `contributors.json`.
- Maintainers refresh the stats with `python3 ../../craftrules/scripts/contributors.py .` (it also
  re-verifies who wrote each `people.toml` entry; `--check` only verifies).
