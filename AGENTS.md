<!-- craft-house-rules:start core source=dde09ea31e2e457f5b4653a6e76f7cdf55944bdf sha256=540e6cea75d12d0710c3dca89658328e81e508ecef102446ca5bb95b7c0b92bb -->
## House rules

These apply in every Craft repository. A later rule may be stricter, not weaker. The source is `standards/house-rules-core.md` in `storytold/craft-contrib`. Propose changes there, not in a copy.

1. **A person is accountable.** Whoever runs an agent answers for what it opens and for its review comments.
2. **Check, then claim.** Before writing code, read the issue comments, search pull requests for `#N` and the keywords, and check recent commits on `main`. Comment on the issue first. Release a claim you will not finish.
3. **Verify, don't guess.** Open a definition before using it. Confirm the cause in the code, even when someone names a location.
4. **Keep fixes narrow and tested.** Fix what the issue describes, where the bug is. Add a test that fails without the fix.
5. **Review the diff before opening a pull request.** Identifiers exist as used, the test fails without the fix, nothing unrelated changed, and the description is true.
6. **Look for a better pull request.** If another fixes the same thing, close yours when it is better or equal, and say why.
7. **Report only what ran.** Say what you tested and what you could not run. Without a local build or test, say so and rely on CI before marking ready.
8. **`main` changes through a pull request.** Do not push to it. Branch protection enforces this. There is no exception.
9. **Stop and ask only about** licensing outside the allowlist, publishing, releases, a new remote, secrets, spending money, destructive git, and anything else hard to undo. Otherwise keep working. `plan/` is a gitignored notebook, not rules. Without it, do not ask for it and do not stop.
10. **Keep secrets and personal details out** of the repository, logs and requests.
11. **CI stays shared.** Workflows call `storytold/craft-common-actions`. Do not change CI in a feature or fix pull request. If CI cannot cover a change, open an issue there.
12. **Guards say why.** A check that looks removable says what it prevents. Read that before changing it.
13. **Instructions must be readable by everyone.** No notes about one machine, and no links to files a reader cannot open. `craftrules` is retired. Do not link to it.
14. **Keep pull requests moving.** Concurrent pull requests are fine; unanswered actionable feedback is not. Respond in 7 days, a reminder at 14, closure eligible at 30, from when an actionable request waits on the author. Suggestions and unconfirmed bot findings do not start the clock. A fix, explanation or agreed plan resets it; unrelated activity does not. Waiting on a reviewer or maintainer pauses it and does not count against the author. A maintainer decides closure; it is never automatic. Reuse search results, do not poll, and stop at a rate limit.
15. **No agent attribution.** A pull request does not say an agent wrote it. A repository rule may not require that.
<!-- craft-house-rules:end -->
<!-- craft-house-rules:start rust source=dde09ea31e2e457f5b4653a6e76f7cdf55944bdf sha256=387e0ccb4b49e567f60951d7d6fff7305054c89f43ad812b9993dc7c38528013 -->
## Rust Craft repositories

These apply to native Craft apps and to shared Rust libraries. Select them by repository type. A repository that merely contains some Rust does not take this block. Rules later in a repository's file may be stricter, but not weaker. The source is `standards/house-rules-rust.md` in [storytold/craft-contrib](https://github.com/storytold/craft-contrib).

1. **Never crash.** Non-test code does not panic and has no `unsafe`, except where the repository file names an exception. Return errors, treat input as hostile, and bound recursion and allocations. New production crate roots copy the `deny` attribute from an existing crate: `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic`, `clippy::unimplemented`, `clippy::todo`, `clippy::unreachable`. Every crash fix comes with a test that panicked before the fix.
2. **Clean room.** Learn from observed behaviour and public specifications. Do not copy another product's code, assets, strings or artwork. Check a dependency's licence before adding it.
3. **Assets and fonts.** Every asset is original or suitably licensed, and is recorded in `ATTRIBUTION.md` in the same change. Font files live in [storytold/craft-fonts](https://github.com/storytold/craft-fonts), not in the app repository.
4. **Rust only for native Craft apps.** No handwritten JavaScript or TypeScript, and no Tauri, Electron or webview shell, in those apps. Shared tooling follows its documented technical scope and maintainer-approved exceptions. Craft PR Review's existing Tauri and TypeScript UI is one such exception. It does not relax this rule for native apps. Preserve any existing web build.
5. **Time goes through `chrono`.** Do not hand-roll calendar conversion. For dates beyond what `chrono` can represent, follow [standards/time.md](https://github.com/storytold/craft-contrib/blob/main/standards/time.md).
6. **Contributor credits.** Add or edit only your own person's entry in `contributors/people.toml`, and only after asking them. Never guess a name or hand-edit `contributors.json`.

## Licence allowlist

Dependencies and assets use SPDX identifiers. An expression is allowed when every identifier is in this list. A repository may name a shorter list. It may not add an identifier here by editing its own copy of these rules.

- `ISC`
- `MIT`
- `BSD-2-Clause`
- `BSD-3-Clause`
- `Apache-2.0`
- `OFL-1.1`
- `Ubuntu-font-1.0`
- `Bitstream-Vera`
- `CC0-1.0`
- `CC-BY-4.0`
- `CC-BY-SA-4.0`
- `LicenseRef-Public-Domain`

`LicenseRef-Storyteller-Trademark` is not in this list. It covers Storyteller brand marks only, where a repository already says so.
<!-- craft-house-rules:end -->

# DesignCraft — instructions for agents

DesignCraft is a clean-room, open-source, Rust-native page-layout application targeting Adobe InDesign parity — and superiority (speed, openness, agent control). It runs natively on macOS, Windows and Linux, and on the web via WASM. Siblings with the same conventions: `../vectorcraft` (Illustrator-class), `../photocraft` (Photoshop), `../pdfcraft` (Acrobat), `../filmcraft` (Premiere), `../lightcraft` (Lightroom).

## Start every session here
Read [`ROADMAP.md`](ROADMAP.md) and [`docs/gaps.md`](docs/gaps.md). `plan/` is an optional gitignored notebook (status, execution notes, `plan/indesign/*.md`). Do not ask for it and do not stop when it is absent.

## Never crash
People trust DesignCraft with their layouts; a crash loses their work. **This outranks feature work**: never ship a feature through a panic path, and fix a crash before building on top of it.
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
- **Fonts live in [`storytold/craft-fonts`](https://github.com/storytold/craft-fonts)**, never in this repo: don't commit new font files (the Latin UI/document fonts already in `assets/fonts/` stay). It is an optional build input, never a `Cargo.toml` dependency: `CRAFT_FONTS_DIR=/abs/path/to/craft-fonts cargo build` makes `crates/fonts/build.rs` embed its fonts as `designcraft_fonts::CRAFT_FONTS` (Japanese: BIZ UDPGothic for the UI, Mincho first for document text; web builds embed BIZ UDPGothic Regular only). Unset, `CRAFT_FONTS` is empty and the app uses its bundled and system fonts; code and tests must work that way (tests on its glyphs skip). Releases build with it.
- **Shared test corpora.** Real-file test oracles (Photoshop-authored PSDs, etc.) live in [`storytold/photocraft-corpus`](https://github.com/storytold/photocraft-corpus). Never commit large binary fixtures to this repo; fetch them pinned by commit and sha256-verified, as PhotoCraft does with `cargo xtask corpus`.
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
`ROADMAP.md` (committed) is the one-page summary: stage, headline numbers, dimensions, languages, progress log. Details: `docs/target-app-parity.md` (assessment), `docs/gaps.md` (ranked work list: pick from the top), `docs/roadmap.md` (milestones, current focus), `docs/parity-checklist.md` (presence checklist, `cargo xtask parity`), and the typography, file-format, UI, hardware and localization parity docs in `docs/`. When work lands, update the affected docs and their status lines, delete or shrink the closed gap, and add a progress-log line to ROADMAP.md.

## Contributor credits (About window)

- About ▸ Contributors/Models are compiled into the binary from `contributors/contributors.json`
  (commit stats; never hand-edit) and `contributors/people.toml` (names people chose for
  themselves). See `docs/contributors.md`.
- **Agents working for a contributor:** when you prepare a PR, check whether your human's GitHub
  username has a `[people.<username>]` entry in `contributors/people.toml`. If not, ask them once
  whether they want to be credited by more than their username: a real name, a display name, and/or
  their public GitHub profile name (`sync_github_name = true`). If yes, add **only their own** entry
  (copy the template at the top of the file)
  and include it in their PR, committed as them. If no, change nothing: they are credited as
  `@username` anyway.
- Never add, edit, guess or copy anyone else's entry or name (not from git config, commit authors or
  GitHub profiles). Never hand-edit `contributors.json`.

## CI workflows
- Workflows in `.github/workflows/` call the shared workflows in [`storytold/craft-common-actions`](https://github.com/storytold/craft-common-actions) at `@main`, with their inputs and secrets. No other jobs: if no shared workflow does what's needed, it doesn't go here.
- Don't add or change CI in a feature or fix PR. If the existing workflows can't build or test a change, link an existing craft-common-actions issue or PR, or open a new issue, or a PR only for a small fix that is safe for every app and does not increase CI time.
- Some workflow files here still run their own jobs. Don't add to them. Moving a job into craft-common-actions is its own task, never part of another PR, and the job stays here until a shared workflow runs it.

New workspace crates include `[lints] workspace = true` in `Cargo.toml`. Production crate roots declare the crate-level lint attributes before modules; crate-level `//!` documentation may precede them.
