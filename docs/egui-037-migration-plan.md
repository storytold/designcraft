# egui 0.37 migration plan for DesignCraft

Status update (2026-10-25): implementation is ready locally for review and includes main `7ca39c6c10f123d0cd0176eda1511e59667c89d0`. Managed run `egui037-design-main7c-ci-20261025-0817` passed all nine Linux CI stages: 3,344 test executions passed, 0 failed and 6 ignored across 81 summaries. Counts include repeated executions. The current-main integration received independent review. Dependency pins and lockfile are unchanged. Current-source Windows/packages, native IME and paired application performance remain open. Publication requires separate explicit approval; human review and human merge are required.

The original planning record below is historical.

Status: planning only. No production change, build, test, GUI run, browser run, benchmark,
push, pull request or publication was performed while writing this document. Every future test
or build runs through `maintenance/task.py run/status/finish` with its own task, source and
evidence; never run a build directly. Every future push or new pull request needs fresh explicit
human approval after the exact contents and validation are shown. The contributor credit
preference remains username-only (`@skylartaylor`); do not add display or real names or any
agent/model/tool attribution.

This plan covers only `storytold/designcraft`. It records the work needed to move the
DesignCraft desktop, web and UI crates from the current coherent 0.36.2 dependency set to
egui 0.37.0, the final reviewed suite fork and the matching final reviewed winit revision.
Shared-library work remains in `storytold/craft-libs`; this plan consumes it but does not
redefine it. Candidate `aiongg` revisions below are comparison inputs only. A production
manifest must name the final reviewed `storytold` revisions, which do not exist until the
fork/winit/craft-libs publication is reviewed and approved.

Capture acceptance follows the suite contract: a queued-not-issued hidden request and
each settling frame must request `RequestPaintWhileHidden` from logic; only an issued
screenshot command forces paint itself. Callback arrival has no implicit wake, so the
live request owns the viewport waker. Record known issuance UI bounds and a geometry
epoch, reject/retry any intervening resize/DPI/zoom/surface change even if it changes
back, and derive pixel scale from returned dimensions only under an unchanged epoch.
Default limits are 4 requests, 128 MiB/image and 256 MiB aggregate. Count reserved
in-flight and completed images together.


## 1. Baseline and provenance

### 1.1 Exact source baseline

| Item | Value |
| --- | --- |
| DesignCraft checkout | `/home/codex/work/CraftApps/egui037-migration-20261022/designcraft` |
| Baseline commit | `e74208dd7e0c801d15dfdf926d64cfe5e3b346d3` (`Merge pull request #1642 from aiongg/ci-speed`) |
| Workspace version / edition / MSRV | `0.12.0` / edition 2024 / Rust 1.95 |
| Lockfile | `Cargo.lock`, format version 4 |
| Current egui family | `egui`, `epaint`, `ecolor`, `emath`, `eframe`, `egui-wgpu`, `egui-winit`, `egui_kittest`, `epaint_default_fonts` all `0.36.2` from crates.io |
| Current winit | `git+https://github.com/storytold/winit.git?rev=3d51077fd1a966f84df9ef287839dc84fd5b4ca7` (0.30.13, W1–W14) |
| Current `craft-ui` | `git+https://github.com/storytold/craft-libs?rev=5c874d51eeb3de70e735c8ce87a59a7dedd47675#5c874d51...` |
| Upstream egui 0.37.0 | `841a328ba2dd8f6996beb7189ebb7fc00e668a2a` (release commit, not the annotated-tag object) |
| Candidate egui fork (comparison only) | `aiongg/egui` `craft/0.37` = `a4f9a17f64a6bae61350fb85804c9df0d9865916` (E1–E21; E21b/P7b still to be added) |
| Candidate winit (comparison only) | `aiongg/winit` `craft/0.30.13-lang` = `8b35388bd719e8cc86a95971ba0bc026c9a8f058` (W1–W27; W15P still to be added) |
| Final storytold egui pin | **Unknown until approved publication**: a reviewed immutable `storytold/egui` revision containing the complete P8 handoff: E1–E21, E21b/P7b, E4/P2b, P7 key pairing and P7c fixes. Never ship the candidate hash. |
| Final storytold winit pin | **Unknown until approved publication**: a reviewed immutable `storytold/winit` revision containing W1–W27 plus native W15P PRIMARY. Never ship the candidate hash. |
| Final `craft-ui` pin | **Unknown until approved publication**: `<FINAL_CRAFT_UI_REV>`, the reviewed CL-2F revision (including CL-1, and CL-3 if it has landed) derived from current craft-libs, not a PR head. D1 uses the reviewed CL-1 + CL-2 provisional revision as a pilot; CL-2F follows the DesignCraft/GridCraft adapter reviews. |
| Published predecessor | `storytold/egui` `craft/0.36.2` = `767b1cad294120689a514bb4bd68c077e46679b9`; published `storytold/winit` = `ffae5198770d7730e718043f035a772b7dbc3b1b` |
| craft-libs PR 199 | head `b68de8c5f4f1c060904d211a7b1320aa0f7bce78`; audit tree `ec8840af8c5fda695cf195cdd05e5ae078415c07`. Both are **stale trees, not bases**: they omit `crates/platform` and revert current security/settings/docking/printer fixes. Apply only the API-port hunks from `8614e4a04fae516fba16c22b31ff1008cb6be10b` (and relevant README changes from `1764bca`) onto current craft-libs `49190884278f16c9a9f5823d7698a8ffb5deb062`. |
| Related language work | `storytold/craft-libs#198`, delivered through fork E5/E9/E12–E17/E19–E21, E21b and winit W17–W27 plus W15P |

Authoritative lifecycle evidence for this plan is
`/home/codex/work/ssd2/task-evidence/egui037-migration-20261022/lifecycle-source-audit.md`.
It was checked against upstream `841a328b` and candidate `a4f9a17f`: both native
`EpiIntegration::update` and web `AppRunner::logic` call `app.logic` then `app.ui` inside
`Context::run_ui`, so `App::logic` can run once per layout pass in both hosts. After a normally
completed `run_ui`, `current_pass_index()` is zero even after multiple passes. `run_logic` does
not restore an old index. `ViewportInfo::visible()` is a method derived from `minimized`/
`occluded`; it is not a callback-phase signal, and `None` means unknown.

The IME gap is recorded in
`/home/codex/work/ssd2/task-evidence/egui037-migration-20261022/ime-surrounding-gap.md`. The
candidate never calls W27 `set_ime_surrounding_text`, and `IMEOutput` has no logical text. E21b/P7b
is a required new change, not existing capability.

The historical compiler diagnostics at
`/home/codex/work/ssd2/task-evidence/egui037-audit-20261022/{fork,stock}/designcraft.errors`
are clues only. They were produced against an earlier prepare state, their counts are not
acceptance, and the current source may differ. They do show the first API breaks: missing
`Event::source`, removed `egui::UserData`/`Event::Screenshot`, renamed roles in
`new_document.rs`, `font_menu.rs`, `panels/*`, and `FontData` blob typing in `theme.rs`.

### 1.2 Packages, binaries, examples and feature targets

| Target | Kind | Relevant manifest | Notes |
| --- | --- | --- | --- |
| `designcraft-ui-egui` | library | `crates/ui-egui/Cargo.toml` | The swappable L6 UI. Production depends on `craft-ui`, `egui`; dev depends on `egui_kittest` (`wgpu`, `snapshot`) and `designcraft-fonts` (`testing`). D1 adds an explicit `egui/default_fonts` dev feature (see §3.7). Build script only bakes contributor credits. |
| `designcraft` | binary | `apps/designcraft/Cargo.toml` | eframe features `default_fonts`, `wgpu`, `accesskit`, `wayland`, `x11`, `links`. D1 adds `clipboard` to preserve Copy/Cut and Paste on 0.37 (0.36.2 forced it through eframe; see §3.5). Native-only `rfd`, `arboard`, `winit`; macOS `muda`, `objc2*`, `fmv-macos-events`; Windows `winsafe`. |
| `designcraft-web` | wasm binary | `apps/designcraft-web/Cargo.toml` | `eframe` features `default_fonts`, `wgpu`, `accesskit`; `rfd`, `wasm-bindgen`, `web-sys`; `trunk` build. Browser clipboard/drop paths are web-specific. |
| `designcraft-cli` | binary | `apps/designcraft-cli/Cargo.toml` | No egui/eframe/winit/rfd; headless and MCP. |
| `ui_shot` | example | `crates/ui-egui/examples/ui_shot.rs` | `egui_kittest::Harness` with wgpu at 1440×900, 2×; drives control requests and saves whole-window PNGs. It steps one UI pass per `harness.step()` and is not native presentation evidence. |
| `xtask` | workspace tool | `xtask/Cargo.toml` | `cargo xtask ci`, `wasm`, `layers`, `assets`, `pdf-deps`, `test`, `stats`, `parity`. |

There are 22 workspace manifests (`Cargo.toml` plus `apps/*` and `crates/*`), no nested
workspace, no `vendor/` directory, no examples or benchmarks that use egui other than
`ui_shot`, and no `[[bench]]`, `[[test]]` or `[[example]]` entries in any manifest. The only
workspace patch section is the root `[patch.crates-io]` in `Cargo.toml`.

Relevant platform cfgs:

| Cfg | Files | What must remain true |
| --- | --- | --- |
| `target_arch = "wasm32"` | `apps/designcraft-web/src/{main,web}.rs`, `crates/ui-egui/src/lib.rs` (`now_ms`, recovery, render worker) | Browser entry point and async file/drop/download paths build and run; native-only recovery, dropped-file paths and render worker stay out. |
| `not(target_arch = "wasm32")` | `crates/ui-egui/src/lib.rs`, `apps/designcraft/Cargo.toml` | Native recovery, `arboard`, `rfd`, control server, `winit` and the enabled clipboard path keep compiling. |
| `unix` and `not(target_os = "macos")` | `apps/designcraft/src/main.rs` X11 event-loop builder | XWayland selection and file drops still work; `winit` pin changes do not remove `EventLoopBuilderExtX11`. |
| `windows` | `apps/designcraft/{Cargo.toml,src/gpu.rs}`, `build.rs` | `winsafe` display-GPU query and Windows resource build remain; DX12 backend selection and `WgpuError` fallback are tested. |
| `target_os = "macos"` | `apps/designcraft/src/{main,native_menu,mac_window,apple_events}.rs`, `crates/fonts` CoreText | Native menu, Apple events, `movable_by_window_background`, IME marked-text discard and cancelable termination keep their winit W12/W14/W22/W26 behavior. |
| X11/Wayland | `egui-winit` clipboard path, `apps/designcraft/src/main.rs` | D1 enables the explicit clipboard feature; D3 adds E4/W15P native PRIMARY on the winit data device. One regular data device only; password exclusion and unchanged-selection policy preserved. |
| `cfg(test)` / `cfg(all(test, unix))` | `crates/ui-egui/src/*`, `apps/designcraft/src/main.rs` | Test-only counters and atomic-writer tests do not leak into release; kittest strict names and missing-glyph policy are explicit. |

### 1.3 Current CI and validation commands

All future commands below run through `maintenance/task.py run` with a unique `--job-id`, the
owning `--task`, exact `--source` and durable `--evidence`; inspect with `status` and complete
with `finish` after reviews.

- `cargo xtask ci`: `fmt --all -- --check`, `pdf-deps`, `clippy --workspace --all-targets --
  -D warnings`, `nextest run --workspace --lib --bins --tests` (or `cargo test`), doctests,
  `assets`, `layers`, `wasm`.
- `cargo xtask wasm`: one `cargo check --target wasm32-unknown-unknown --keep-going` over
  L0–L5 plus `designcraft-ui-egui`.
- `cargo test -p designcraft-ui-egui --lib` is the package-scoped UI row. It must run with the
  explicit `egui/default_fonts` dev feature and with `theme::install_fonts` before layout.
- Clean-checkout dependency gate: after publication, a fresh clone runs
  `cargo fetch --locked` and `cargo metadata --locked --format-version 1` against the final
  storytold revisions. A missing/unreachable revision or a machine-local path fails the gate.
- CI workflow `.github/workflows/ci.yml` runs `cargo xtask ci` on Ubuntu 24.04 with pinned
  craft-fonts `abb83316d96aa59c1cf64784289e378fe9fa5695`, nextest, and the apt/winit build
  dependencies.
- `windows-arm64.yml` packages and installs ARM64; `freebsd.yml` runs the workspace in a
  FreeBSD VM; `release.yml` uses the shared release workflow.
- Manual UI evidence: `cargo run --release -p designcraft -- --sample --control 7979`,
  control requests, `ui.screenshot`, and `cargo run -p designcraft-ui-egui --example ui_shot
  -- script.jsonl` for offscreen whole-window screenshots.
- Native/browser workflows are listed in §6.4; `ui_shot` alone is not native focus, IME,
  picker, clipboard or browser-presentation evidence.
- There is no performance benchmark target today. `ui.inspect` exposes
  `perf.frameMs`, `perf.renderMs`, `perf.fps`, `perf.patches`; `perf.frameMs` uses
  `now_ms()` (`SystemTime` on native, `0.0` on wasm) and is not a monotonic acceptance
  timer. §7 defines the test-only raw-sample harness.

## 2. What 0.37 changes that actually affects DesignCraft

DesignCraft already uses `eframe::App::logic`/`App::ui` and a root `Ui` on 0.36.2. The
0.37 migration must preserve that structure; it is not a reason to rewrite panels into
`Context::root_ui` calls or to move every action out of `logic`.

| 0.37 change | Source anchor | DesignCraft impact |
| --- | --- | --- |
| `Event::MouseWheel` gains `source: MouseWheelSource` | `egui/src/data/input/event.rs`, `MouseWheelSource` | Every synthetic wheel event in tests must set `source` (`Unknown` for most; `Wheel`/`Trackpad`/`Momentum` where the behavior under test depends on it). Production matching already uses `..`, but the canvas's Alt-wheel state machine must be rechecked. |
| `Event::Screenshot` and `egui::UserData` are removed; `ViewportCommand::Screenshot(ScreenshotCallback)` | `egui/src/data/screenshot_callback.rs`, `viewport.rs`; upstream #8489 | `ScreenshotCallback` is one-shot and does not repaint. Replace token/`UserData` correlation with a per-request completion slot owned by the request record and a weak callback; see §3.4. |
| `WidgetType` is replaced by `accesskit::Role` (`egui::Role`); alias deprecated | `egui/src/lib.rs` compatibility table; #8555 | Map `SelectableLabel` → `Button`, `TextEdit` → `TextInput`, `CheckBox` → `CheckBox`, `Other` → `Unknown`. The full file list is in §3.7: `new_document.rs`, `panels/font_menu.rs`, `panels/pages.rs`, `panels/mod.rs`, `panels/styles.rs`, `panels/cell_stroke.rs`, `panels/properties.rs`, `panels/swatches.rs`, `panels/layers.rs`, `object_control.rs`, `widgets.rs`, `toolbar.rs`, `chrome.rs`, `taskbar.rs`, `icons.rs`, `dialogs.rs`. |
| `Id::new` deprecated for `Id::unique`; `Ui::id` renamed `Ui::scope_id`; `UiBuilder::id` renamed `scope_id` | `egui/src/id.rs`, `egui/src/ui.rs`; #8534/#8542/#8586 | Mechanical but warning-sensitive because `clippy -D warnings` treats deprecations as failures. Use `Id::unique` for root IDs and `scope_id` for child scopes. Do not change the hashed values: `Id::new` now delegates to `Id::unique`, so state keys remain stable. |
| `Panel` constructors take `impl AsIdSalt`; panel ID is `parent_ui.scope_id().with_salt(salt)`; `CentralPanel` remains last; root `Ui` is the normal app surface | `egui/src/containers/panel.rs`; #8592 | Existing `Panel::left("tools")`, `Panel::right("dock")`, etc. still type-check. Panel IDs are now parent-salted. DesignCraft does not persist egui memory (eframe persistence is off); update in-memory test lookup keys and retain the actual app-owned UiState/docking preferences. No PanelState migration is needed. Panel order must not change. `CentralPanel::default().show(ui, …)` is correct; `Context::root_ui` must not be called inside `App::ui`. |
| `default_fonts` is no longer a default `egui`/`epaint` feature | `egui/Cargo.toml`; #8583 | `eframe` still enables it in the app, but `cargo test -p designcraft-ui-egui` does not build the app and `egui_kittest` disables egui defaults. Add `egui/default_fonts` explicitly to the ui crate's dev feature set and install DesignCraft fonts before layout; do not rely on workspace unification. |
| `FontData.font` is now a shared `Blob` (`Arc<dyn AsRef<[u8]> + Send + Sync>`), not `Cow<[u8]>` | `epaint/src/text/font_data.rs` | Production struct-update code in `theme.rs` still works; tests that compare `data.as_ref()` must use `data.bytes()` or `data.font.as_ref().as_ref()`. This caused the historical `theme.rs:766/793` errors. |
| `egui_kittest` fails unnamed input widgets and panics on missing glyphs by default | `egui_kittest/src/accessibility.rs`, `builder.rs`; #8589/#8601 | The first green stage must satisfy strict names/glyph policy, not disable it globally. D1 adds names/policy for the existing test surface; D4 completes coverage. Deliberate tofu fixtures use `HarnessBuilder::allow_missing_glyphs()` with a named reason. |
| Native eframe can discover system fonts through `egui_system_font_provider`, and `NativeOptions::system_font_fallback` defaults true | `eframe/src/native/epi_integration.rs`; #8512 | DesignCraft already catalogs system fonts in `designcraft-fonts` with script-specific families and baseline tweaks. Set the option false and keep the feature off, or font order and line metrics change. Add the #8512 cold-start workload in §7. |
| Clipboard is no longer forced on by eframe; `egui-winit` clipboard is an explicit feature | `eframe-0.36.2/Cargo.toml` target dep forces `egui-winit/clipboard`; `eframe-0.37.0/Cargo.toml` exposes `clipboard = ["egui-winit/clipboard"]` and depends on egui-winit with defaults off; `egui-winit-0.37.0/src/clipboard.rs` stores only in-process without arboard/smithay | 0.36.2 wrote Copy/Cut to the OS clipboard. On 0.37 without the feature, `ctx.copy_text` is in-process only and `RequestPaste` cannot read the OS clipboard. D1 enables `eframe/clipboard` to preserve native Copy/Cut/Paste, with the control-channel guard. D3 adds E4/W15P PRIMARY on the same winit data-device path. |
| X11/Wayland PRIMARY selection is new in `egui-winit`, and the candidate's E4 path replaces arboard with winit's data device | `egui-winit/src/clipboard.rs`, `lib.rs`; #8428; E4 | Enabling clipboard makes PRIMARY reachable. The final fork must include E4 plus native W15P; test pointer- and keyboard-triggered ownership (including keyboard-only focus), serial freshness, owner/seat changes, empty/absent offers, self-paste, canceled/slow transfers on wlroots/KWin/Mutter, password exclusion and no second regular data device. |
| `TextEdit` selection behavior changed on click-away | upstream #8763, unmerged PR #8764 | The fork owns the selected fix. Source audit finds no DesignCraft control that reads an egui `TextEdit` selection in the same pass: formatting acts on the engine `TextSel`; dialog/search buttons read buffers. The regression test documents stock 0.37 click-away behavior and asserts the app's engine selection/target is unchanged; if a future control reads `TextEdit` state, carry #8764 or defer the command with a tested app change. |
| Text tessellation is reported ~60% slower for plain text | upstream #8731 | UI frame cost may rise even though layout is unchanged. The fix must keep color emoji correct; branch once per row, not clamp-only. Measure the fixed 120-label workload. |
| DX12 pipeline creation is reported ~4× slower from `wrap_texel_coord` | upstream #8736 | Windows startup can get ~40 ms slower. Measure renderer/shader construction separately from first frame; do not call shader compile time UI performance. |
| `#8748` hidden-parent/deferred-child is closed and unmerged | upstream #8748 | Do not assume hidden-window child viewport updates are fixed by 0.37. The fork owns a tested solution; DesignCraft must open/update/close its immediate second viewport while the parent is hidden/occluded/overdue, with unknown visibility (`visible() == None`) as well as `Some(false)`. |
| `Context::root_ui`, `run_ui`, `run_pass`, `run_logic`; root `Ui` cannot be recursively entered; both native and web `App::logic` run per `run_ui` pass | `egui/src/context.rs`; `eframe/src/native/epi_integration.rs`; `eframe/src/web/app_runner.rs`; #8535; lifecycle audit | Use the passed `&mut Ui` in `DesignApp::ui`; use `run_ui` for UI tests and `run_logic` only for side-effect tests. Recursive `root_ui` panics. Do not use `visible()` as a logic/UI phase signal; a forced hidden paint is still a UI pass. |
| Style defaults changed (roomier buttons, larger corner radius) | #8617/#8445 | Custom `Tokens` still drive most colors, but egui widget spacing/rounding inherited from `Visuals` can shift geometry. Visual baselines need an explained intended change, not a blind update. |
| E21b/P7b outgoing IME surrounding text is missing | `IMEOutput`/`egui-winit` candidate source; `ime-surrounding-gap.md` | Add the bounded transient payload, built-in `TextEdit` producer and W27 Wayland adapter; DesignCraft's custom canvas editor supplies a real model/preedit/UTF-8 adapter, not `None`. |

## 3. Source audit and concrete migration design

### 3.1 Existing frame flow

The desktop host implements `eframe::App` in `apps/designcraft/src/main.rs`:
`App::logic` calls `startup.enter`, `take_notice`, the `frames == 2` presented check,
`frames += 1`, startup error-log installation, macOS native menu/Apple-event polling, then
`DesignApp::logic`; `App::ui` calls `DesignApp::ui`, then macOS IME/movable handling.
`App::raw_input_hook` delegates to `DesignApp::raw_input_hook`. The web shell does the same
in `apps/designcraft-web/src/web.rs`, with asynchronous drop-file reading and a graphics
health check.

Source facts that constrain the design (verified against the pinned trees):

1. Native `EpiIntegration::update` (`eframe-0.37.0/src/native/epi_integration.rs:349-361`)
   calls `Context::run_ui(raw_input, |ui| { app.logic(ui.ctx(), …); app.ui(ui, …) })`.
   Web `AppRunner::logic` (`eframe-0.37.0/src/web/app_runner.rs:290-293`) does the same.
   A multipass frame therefore calls host and app `logic` once per pass, not once per frame.
2. `Context::run_logic` (`egui-0.37.0/src/context.rs:1174-1202`) only refreshes viewport/
   window/focus input; events and time stay from the previous UI pass.
3. `run_dyn` passes `new_input.take()` to each `begin_pass`; the second pass has no raw
   events. `PointerState::button_clicked` scans `pointer_events`, which are cleared and
   repopulated from that empty event list, so a button does not re-fire merely because a
   pass was discarded. Command-producing UI still needs a one-pass/undo test because a
   discarded first pass may act on pre-discard geometry; it does not need a global
   `!will_discard()` ban.
4. After a completed `run_ui`, `ContextImpl::end_pass` takes `viewport.output`; the returned
   output owns the completed-pass count and `current_pass_index()` reads the now-zero
   viewport output. `run_logic` does not restore a stale index. The earlier comment alleging
   a leftover nonzero count was wrong.
5. `ViewportInfo` has `minimized`/`occluded` fields and a `visible() -> Option<bool>` method;
   there is no public `visible` field. `visible() == None` is unknown, and eframe forces a
   real UI pass for `RequestPaintWhileHidden`/pending `Screenshot` even when hidden.
6. `update_logic_only` stores the prepared raw input in `pending_raw_input`; the next
   `update` prepends it to new input. Normal pointer/key/text/IME events can therefore be
   replayed after hidden logic.

`DesignApp::logic` (`crates/ui-egui/src/lib.rs:1102`) currently does all of:
`egui_ctx` capture, `window_maximized`, one-time CMS activation through `color_applied`,
`ctx.set_zoom_factor` from `ui.ui_scale`, `zoom_with_keyboard=false`, font installation and
the 250 ms system-font poll, `theme::apply`/`restyle`, FPS smoothing from `i.time`, recovery
autosave from `i.time`, control drain, stale new-document preview cleanup, inbox/open-OS-
documents, synthetic repaint requests, screenshot collect/issue, shortcut handling, window-
command flush, native dropped-file handling and quit handling. `DesignApp::ui` (`lib.rs:1472`)
does layout: app bar, control bar, status bar, Tools, dock, central canvas, second viewport,
floating panels/story editor/dialogs/about/palette, resize borders, URLs and frame timing.

This is a 0.36.2-shaped flow, not a 0.37 regression. However, 0.37 makes the latent problems
more important:

1. Every per-frame side effect in `logic` runs again on a discarded second pass. That
   includes font/theme setup, recovery checks, FPS sampling, screenshot issue/collect and
   side effects that are not input-driven.
2. While minimized/occluded, eframe normally runs `Context::run_logic` and no UI pass. All
   `ctx.input` except viewport/window/focus information is stale. A forced hidden paint is
   different: `run_ui` does run, with fresh prepared input, even though `visible()` may be
   `false` or `None`.
3. Host `App::logic` counts passes in `frames`; on a multipass frame `frames` advances twice,
   so `startup.presented()` and startup error logging happen at the wrong pass.
4. Input buffered by `update_logic_only`, and synthetic control events injected in
   `raw_input_hook` during a hidden tick, can replay into a later UI pass with a different
   focus or document state.

The correct migration is to make the frame stages explicit, not to merge `logic` into
`ui`, not to add a generic `Context` bridge, and not to use `visible()` as a phase signal.

### 3.2 Frame-stage contract

Introduce an explicit, testable split inside `DesignApp` while keeping the public
`logic(&Context)` and `ui(&mut Ui)` entry points that hosts and tests use.

```rust
// crates/ui-egui/src/lib.rs
pub fn logic(&mut self, ctx: &egui::Context) {
    // Every call (first pass, later discarded pass, or hidden run_logic) may poll consumed
    // queues and deadlines. It must never replay an already-consumed message.
    self.logic_tick(ctx);
    // current_pass_index is 0 on the first pass and on hidden run_logic, and >0 on later
    // passes of the same run_ui. It is not a freshness test and must not gate input reads.
    if ctx.current_pass_index() == 0 {
        self.logic_frame(ctx);
    }
}

pub fn ui(&mut self, ui: &mut egui::Ui) {
    let first_pass = ui.ctx().current_pass_index() == 0;
    self.layout(ui, first_pass);
}
```

The guard is deliberately narrow: it removes duplicate per-run_ui side effects, but a hidden
`run_logic` tick still has index 0, so autosave, control commands, inbox results and deadlines
continue. A command that arrives between passes is retained in its queue (or its reply is
pending); the control server requests a repaint, and the next frame's first pass consumes it.
Never put an unconditional `return` for `current_pass_index() != 0` around queue draining,
screenshot completion or background deadlines.

`logic_tick` (every call) owns work whose identity is a consumed queue item, revision or
monotonic deadline:

- `drain_control`; each request is taken once. Screenshot requests add a request record and
  do not issue from the queue.
- `drain_inbox` and `open_os_documents` after fonts are ready; inbox items are consumed once.
- `handle_quit` (fresh `close_requested` in `i.viewport()`), `flush_window_commands`.
- Screenshot result draining, late-result rejection and deadline replies; take results out
  of the slot under a short lock before encoding/writing.
- While a control gesture still has queued synthetic events, request a paint while hidden
  (§3.3). Never inject a synthetic event during a logic-only tick.
- Filter buffered OS input after a hidden tick using the focus/document generation recorded
  at the tick (§3.2 input policy).

`logic_frame` (first pass only) owns genuine once-per-run_ui work:

- `egui_ctx` capture; `window_maximized` update (`lib.rs:1106`).
- One-time CMS activation through `color_applied` (`lib.rs:1107-1112`).
- `ctx.set_zoom_factor` from `ui.ui_scale` and `zoom_with_keyboard=false` (`lib.rs:1114-1119`).
- Font installation/reinstallation from `theme::install_fonts`, `fonts_system` readiness and
  the 250 ms `request_repaint_after` while the font scan runs (`lib.rs:1121-1131`).
- `theme::apply`/`restyle` (`lib.rs:1132-1135`).
- Recovery autosave (`lib.rs:1136-1147`), changed from `ctx.input(i.time)` to a monotonic
  `Instant` clock so it continues during hidden logic and never uses a stale frame time.
  Recovery is native-only (`#[cfg(not(target_arch = "wasm32"))]`); wasm uses its browser
  time source rather than `Instant`/`now_ms()`.
- FPS smoothing only on a real UI pass, using a monotonic `Instant` on native and the
  browser frame time on wasm; do not inject zero/duplicate samples from hidden logic.
- `new_document::end_stale_preview(self)` (`lib.rs:1157`) and the synthetic/control
  `ctx.request_repaint()` while events remain (`lib.rs:1160-1162`).
- Screenshot issue after the 120 ms/3-frame settling rule (`lib.rs:1636-1648`).
- `menus::shortcuts` moves out of `logic` into the first-pass `input_stage` below.
- Native dropped files move out of `logic` into `pending_drops`, captured in
  `raw_input_hook`; `logic` never reads stale `ctx.input` drops.
- `handle_quit` stays in `logic_tick` because `close_requested` is fresh viewport data and a
  close request must not wait for a first pass.

`layout` calls `input_stage(ui)` only on the first pass, before the app bar:

- `menus::shortcuts(self, ui.ctx())` runs against live input;
- native dropped files are taken from `pending_drops` (captured in `raw_input_hook`), not
  read from stale `ctx.input` in logic;
- no other input event is read twice. `canvas::show` and text fields continue to use egui's
  normal per-widget input. Shortcut consumption happens before layout, preserving the
  existing order.

`DesignApp::ui` must never call `ctx.root_ui(...)`: the root `Ui` is already borrowed by
the host callback, and `Context::root_ui` panics in that situation (#8535). The passed
`&mut Ui` is the one to use for `Panel`, `CentralPanel`, `Area`, and `show_viewport_immediate`.

Host guard: `apps/designcraft/src/main.rs` must not increment `frames` or run macOS menu/
Apple-event polling on every pass. Wrap the host body in the same first-pass condition (or
dedupe with `ctx.cumulative_frame_nr()`), while always delegating to `self.app.logic(ctx)`,
which has its own narrow guard. `frames` then counts presented `run_ui` frames, preserving
the `frames == 2` startup.presented semantics. Add a host-level forced-multipass test through
the actual `App` wrapper, not only `DesignApp`.

`raw_input_hook` remains the one place that sees the exact `RawInput` before a pass. It
continues the current batching: one pointer event per host iteration, a key press/release
pair together, with trailing Text retained for a later iteration (`lib.rs:1456-1469`). Add:

- native `raw.dropped_files` capture into `pending_drops` and removal from the raw input, so
  a hidden `run_logic` cannot re-open the last drop;
- a focus/document generation snapshot on each hidden logic tick;
- a filter that drops buffered `Text`/`Ime`/`Paste` events when the focus target or active
  document changed since the hidden tick, and drops an incomplete pointer chord (press
  without release or release without press) across a hidden->visible transition. Complete
  pointer/key events are replayed in order; pointer hover is refreshed or cleared with
  `PointerGone`. Record the policy and counts in the test-only instrumentation.

On web, `WebShell::raw_input_hook` is called in both the visible and hidden `AppRunner`
paths. It must capture dropped files and start the asynchronous `bytes_async()` read there,
pushing the result into the existing inbox. `WebShell::logic` must not take
`ctx.input_mut` for drops. This is a source correction: the current
`apps/designcraft-web/src/web.rs:351-370` reads drops in `logic`, while `raw_input_hook`
(`web.rs:374-379`) does not.

Test-only instrumentation may record stage counts:
`#[cfg(test)] logic_tick_ticks`, `logic_frame_ticks`, `input_ticks`,
`screenshot_callbacks`, so forced-multipass and hidden tests can prove one side effect and
one input stage per frame. The counters must not exist in release builds.

`Context::run_ui` is the test entry point for layout and `Context::run_logic` for
side-effect-only tests. Do not initialize fonts inside `run_logic` in a UI test: no pass
runs, so font installation/layout does not behave as in a visible frame. `ui_shot` and
kittest harnesses call `app.logic(ui.ctx())` and `app.ui(ui)` inside the UI closure as they
do now, with the same pass-index guard. Tests must also drive the real host/web callback
order, not a fictional once-per-frame native host.

### 3.3 Control input while hidden

The control channel is used for automation even when the window is minimized. Under
0.37, a hidden window normally runs only `logic`; synthetic pointer/key/text/IME events
injected through `raw_input_hook` are not applied unless a UI pass runs. A
`ViewportCommand::RequestPaintWhileHidden` holds for **one frame** and must be sent again for
another (`egui/src/viewport.rs:1239-1247`); a plain `request_repaint` does not force a hidden
paint (`egui-winit/src/lib.rs:1922-1924`).

Policy:

- Do not use `visible()` as a phase signal. Use it only as a hint for whether to request a
  hidden paint. `None` (unknown visibility) is treated like hidden for this purpose.
- raw_input_hook runs before both host paths, so deliberately allow its drain-based
  injection into pending_raw_input during a logic-only iteration. Eframe retains those
  events for the next full pass. Send RequestPaintWhileHidden while an admitted gesture
  has queued OR injected-but-unapplied events; acknowledge delivery only from known UI
  phase. Preserve current pointer/key batching, generation filtering and event order.
  Test hidden injection followed by forced paint: one action/reply, no dropped or replayed
  events. An empty synthetic queue alone does not mean pending input was already applied.
- Pure engine commands (`engine.execute`) never request a hidden paint.
- `ui.screenshot` already forces a paint through the `Screenshot` viewport command; do not
  add a second `RequestPaintWhileHidden` for it. Still send `Focus` as today.
- `ui.dialog.confirm` and other methods whose result depends on layout follow the same
  pending-gesture rule.
- The control reply may be `ok` before all events are applied (current behavior). Tests and
  documentation must assert the final command/undo/document result, not the number of
  events sent or a single paint request.

### 3.4 Screenshot callback migration

Current code (`lib.rs:587`, `drain_control`, `issue_screenshots` at 1636,
`collect_screenshots` at 1652) stores `(token, path, reply, deadline)` in `pending_shots`,
queues `(token, at, frames)` in `queued_shots`, sends
`ViewportCommand::Screenshot(UserData::new(token))`, then scans `Event::Screenshot` for
`user_data.downcast_ref::<u64>()`. All of that changes.

Shared contract: the suite selects one bounded, cancelable completion slot per admitted
request. Eleven existing craft-ui creative consumers adopt its helper; DeckCraft, FigCraft and ArtCraft Native keep a
small local contract-compatible adapter. DesignCraft uses craft-ui. The helper is staged as
CL-2 (provisional) and frozen as CL-2F after DesignCraft and GridCraft adapters are reviewed.

DesignCraft request record:

- Keep the existing token counter, path, reply channel, 8 s deadline and 120 ms/3-frame
  settling rule in `DesignCraft`. These are app-owned.
- Each `pending_shots` record owns one `craft_ui::capture::CaptureSlot` (provisional API;
  final name/API after CL-2F). The slot holds the request id, target `ViewportId`, deadline,
  a weak wake handle and at most one `Arc<ColorImage>`.
- `issue_screenshots` sends
  `ctx.send_viewport_cmd(ViewportCommand::Screenshot(ScreenshotCallback::new(move |image| {
      if let Some(slot) = slot.upgrade() {
          slot.complete(image); // weak slot; no DesignApp/Context strong capture
      }
  })))`.
  The callback captures only a `Weak` reference to the slot. It must not capture
  `egui::Context` strongly (no context self-retention cycle), must not encode PNGs, write
  files, run commands, hold a long lock or borrow mutable app state.
- `ScreenshotCallback` is one-shot; clones share one callback and complete at most once.
- Bound admission by both request count and aggregate pending image bytes before issuing a
  capture. Compute the allowed surface dimensions, reject an oversized/stale image on
  completion, release its reservation and image, and account for renderer/readback transient
  memory separately in peak RSS. A post-callback size check alone is not a bound.
- Capture the target `ViewportId`; use `request_repaint_of(viewport)` from the live slot, not
  an implicit current/default viewport. Release the slot lock before waking.
- `drain_screenshot_results` runs in `logic_tick`, matches the slot record, takes the image
  out under a short lock, then calls `control::save_screenshot(self, image.as_ref(), path)`
  on the UI thread. Late callbacks for timed-out/canceled/closed records release the image
  and send no reply/file.
- Correlate only `ViewportId::ROOT` for `ui.screenshot` until the protocol grows an explicit
  viewport field; do not guess from the pointer.
- Account for E2 surface fitting: the returned image may be smaller than the window. Derive
  x/y scale from the returned image dimensions and the recorded logical viewport rect/DPI;
  a resize/DPI generation mismatch retries within the deadline or returns an error. Never
  combine old pixels with current geometry.

Tests: two outstanding tokens/viewports resolve to their own paths; completion is one-shot;
out-of-order completion; timeout plus late callback; cancel/close/shutdown; delayed readback;
hidden-window wakeup; aggregate memory returns to zero after drain; a reused viewport/document
generation does not bind an old capture to a new document; no file/reply from a canceled
capture; `ui_shot` still renders whole-window PNGs.

### 3.5 Input, IME, clipboard and keyboard layouts

`Event::MouseWheel` constructors in `canvas.rs`, `menus.rs`, `dialogs.rs`, `chrome.rs`,
`new_document.rs`, `object_control.rs`, `panel_docking_tests.rs` and `lib.rs` must add
`source`. Use `egui::MouseWheelSource::Unknown` for synthetic wheel input unless a test
is specifically about `Trackpad`/`Momentum`; the canvas's `wheel_alt` only depends on
`phase` and `modifiers`, so behavior is preserved.

IME:

- The canvas (`canvas::ime_event`, `canvas::delete_surrounding`) currently handles
  `Preedit`, `Commit`, `DeleteSurrounding { before_chars, after_chars }`.
  `delete_surrounding` loops `text.delete` up to 64 times (`canvas.rs:2695-2701`), and each
  `text.delete` is an ordinary undoable command (`crates/engine/src/lib.rs:565-585`), so the
  current path can create up to 64 undo entries.
- E21 adds `ImeEvent::DeleteSurroundingBytes { before_bytes, after_bytes }`. Implement a
  single mandatory engine command, `text.deleteSurroundingBytes { beforeBytes, afterBytes }`
  (or an equivalent single `edit`), that deletes the exact UTF-8 ranges around the caret in
  one `s.edit` call, producing one undo entry. Do not make this optional and do not
  approximate bytes as characters/`text.delete` loops.
- Validate both counts are bounded (default cap 4096 bytes each and no larger than the story/
  selection), both ranges are valid UTF-8 boundaries via `str::get`, no range extends past
  the story, and no split grapheme is produced. Preserve track-changes behavior by marking
  deleted text and stepping the caret, as `text.delete` does. Test replacement, deletion-
  only batches, combining mark/flag/ZWJ, preedit/commit sequencing, `tool_composing()`,
  focus/serial changes and canceled compositions.
- The web text agent keeps its existing `DeleteSurrounding` character behavior.
- E21b/P7b: add the bounded outgoing surrounding-text payload to `IMEOutput`, the built-in
  `TextEdit` producer and the egui-winit W27 Wayland adapter. DesignCraft's custom canvas
  editor (`canvas::ime_output`, `canvas.rs:2720-2753`) supplies a real model/preedit/UTF-8
  adapter: logical committed document text around the caret, preedit excluded, UTF-8 byte
  cursor/anchor, at most 4000 bytes, explicit Unavailable context (distinct from available-empty) when the selection
  cannot fit, and clearing on focus loss, no IME, reset and password/no-context. It must not
  pass display-bidi text or set the payload to `None` as a placeholder. Audit every
  `IMEOutput` constructor/destructure. Tests cover outgoing text after delete+commit and
  deletion-only batches, supplementary/combining/RTL text, preedit exclusion, long lines,
  field/window switching and the full E21b contract: owner/context generation even
  when unavailable, available-empty distinct, W27 committed clear boundary, latest-pass
  None replacing earlier payload, old-focus-epoch invalidation, and current-epoch done
  serial mismatch applying incoming edits once while deferring outgoing until a match.
  Include initially empty→non-empty and normal→password→normal protocol cases.

Clipboard:

- Source contract: `eframe-0.36.2/Cargo.toml:234-237` forces
  `egui-winit` with `features = ["clipboard"]` even with eframe defaults off. 0.37's eframe
  exposes `clipboard = ["egui-winit/clipboard"]` and depends on egui-winit with
  `default-features = false` (`eframe-0.37.0/Cargo.toml:60,239-241`). Without the feature,
  `egui-winit-0.37.0/src/clipboard.rs:113-141` stores `set_text` only in-process. DesignCraft's
  Copy/Cut calls `ctx.copy_text` (`crates/ui-egui/src/lib.rs:806-824`), and its focused-field
  Paste path sends `ViewportCommand::RequestPaste` (`menus.rs:2717`).
- Enable `eframe/clipboard` in the D1 source-compatibility PR to preserve native OS
  Copy/Cut/Paste. This activates arboard/smithay in egui-winit alongside the app's arboard
  read path; keep the app-owned `install_clipboard_guard` so control actions never write or
  read the user's clipboard, and keep `Services::clipboard_text` for the menu Paste path that
  must not go through egui.
- Native evidence in D1: select text, Copy/Cut, read from an independent arboard
  process/handle on X11 and Wayland; test object copy and SVG/text paste; verify focused
  `TextEdit` paste and canvas paste are not doubled; verify control-channel privacy.
- D1 includes E4/W15P in its complete fork pin; D3 expands native policy coverage. E4 routes regular clipboard through winit's Wayland data device with an
  arboard fallback; W15P adds a native PRIMARY path on the same connection/event queue and
  per-seat lifecycle using SCTK 0.19.2 `PrimarySelectionManagerState`/devices/sources/offers.
  Do not bind a second regular `wl_data_device`. Test PRIMARY pointer- and keyboard-triggered ownership (including keyboard-only focus), serial
  freshness, owner/seat changes, empty/absent offers, self-paste, canceled/slow transfers,
  password exclusion and no unchanged-selection reclaim on wlroots/KWin/Mutter. Native
  capability evidence is still required; compile-only is not acceptance.
- `Event::PasteImage` is new but DesignCraft has no image-paste contract. Leave it
  unhandled rather than inventing a placement path. Add a test that it does not panic and
  does not leak into text paste.

Keyboard layouts: after the fork pin, `RawInput::keyboard_layout_is_latin` and
`RawInput::latin_layout_chars` (E19) must preserve tool and command shortcuts on Russian,
Arabic and Hebrew layouts. W25 covers Linux Latin-layout lookup, W26 macOS deletion and W27 Wayland surrounding
text; Windows keeps its own native input path. E19 also needs a real Windows test
with a non-Latin layout; a synthetic `KeyboardEvent` test is not enough
for the OS layout table.

### 3.6 Panels, docking and viewports

`DesignApp::ui` keeps the existing order:

1. `chrome::app_bar` (`Panel::top("app_bar")`)
2. optional `chrome::control_bar`
3. `chrome::status_bar` (`Panel::bottom("status_bar")`)
4. `toolbar::show` (`Panel::left("tools")`)
5. `dock::show` / `panel_docking::show` (`Panel::right("dock")`,
   `Panel::right("shared-panel-dock")`, icon columns)
6. `CentralPanel::default()` last among panels
7. second-window `show_viewport_immediate`
8. floating panels/flyouts, story editor, dialogs/about/palette (areas/windows/modals)
9. custom-titlebar resize borders

0.37's `Panel` constructors accept `AsIdSalt`; the string salts remain stable, but the
panel `Id` is now `parent_ui.scope_id().with_salt(salt)` (`egui/src/containers/panel.rs:974-
975`), whereas the 0.36 panel id was the salt alone. D1 updates test helpers such as `dock.rs:473` and `panel_docking_tests.rs:639` to
load the parent-salted key using the actual parent Ui scope. Eframe persistence is disabled:
there is no saved egui PanelState migration. Preserve app-owned UiState/docking layout through
`save_prefs` and test its save/reopen behavior and panel rectangles. Keep the docking identity
`designcraft-panel-docking` unchanged for saved layout and live drag state.

`craft-ui` CL-1:

- Do not use PR199's `b68de8c`/`ec8840a` trees as a base. They are stale relative to current
  `4919088`, omit `crates/platform`, and revert current security/settings/docking/printer
  fixes (including floating-group move/resize guards `d66e345`/`fd9cf54`).
- Apply only the API-port hunks from `8614e4a` (and relevant README changes from
  `1764bca`) onto current `4919088`, preserving intervening fixes, `crates/platform` and
  unrelated crates. Review a diff against both `5c874d51` (the current DesignCraft pin) and
  `4919088`; pin the exact resulting immutable revision.
- `WidgetType` → `Role` in `buttons/mod.rs`, `docking/surface.rs`, `layout/resize.rs`,
  `tabs/mod.rs`; `Id::new` → `Id::unique`; `Ui::id`/`UiBuilder::id` → `scope_id`.
- The corrected revision keeps the same docking transaction model and `DockArea` API
  DesignCraft uses. A model/permission change is not incidental: if intentionally changed,
  state it as a product decision with tests.
- CL-3 adds the explicit opt-in `DockArea::direction(DockDirection::{LeftToRight,
  RightToLeft})` visual policy. DesignCraft calls `LeftToRight` explicitly for now (no E9
  global mirroring) and relies on CL-3's geometry/hit/drag/keyboard tests. Saved physical
  splits, rectangles and logical tab order must be unchanged by direction; never reverse
  saved panel order or pass display-bidi text into shaping/clipboard.

The second window uses
`ViewportId::from_hash_of("designcraft_second_window")` and
`ViewportBuilder::with_title(...).with_inner_size([960, 720])`. Keep the ID and the
immediate viewport. Add tests for: opening/closing the second window through
`window.newWindow`; the main window's pane focus returning on click; close-request
cancellation of the main viewport; screenshot correlation to the root viewport only;
`CentralPanel` not consuming the second window's available rect; and #8748 hidden-parent/
deferred-child behavior while the root is occluded/minimized/overdue and while
`visible() == None` (open, update, close, and acknowledge without restoring the parent).

### 3.7 Fonts, glyphs, RTL and accessibility

`theme::font_definitions` builds `FontDefinitions` from bundled Source Sans 3/JetBrains
Mono plus optional craft-fonts (`CRAFT_FONTS_DIR`) and installed system faces selected by
`SYSTEM_UI_FONTS`. `install_fonts` returns false while the background font scan is
running and the app reinstalls when `system_fonts_ready()` becomes true. This must stay the
source of truth.

Concrete changes:

- `theme::SystemFont::new` and the `FontData { index, tweak, ..from_owned }` constructions
  are source-compatible with 0.37's `FontData`; only test assertions that read
  `definitions.font_data[name].font` or `data.as_ref()` need `data.bytes()`/`font.as_ref()`.
- Set `NativeOptions::system_font_fallback = false` explicitly and keep eframe's
  `system_fonts` feature off. This prevents `egui_system_font_provider` from adding fonts
  ahead of DesignCraft's script-specific order and makes the policy visible in code.
  Document that DesignCraft's own `FontDb` provides system fallbacks.
- `FontDefinitions::default()` is empty when `default_fonts` is off
  (`epaint-0.37.0/src/text/font_definitions.rs:111-117`). Add `egui/default_fonts` explicitly
  to the `designcraft-ui-egui` dev feature set and install DesignCraft's fonts before layout
  in package-scoped tests, together with explicit `monochrome_emoji_fonts` where used.
  This restores egui's built-in icon/emoji fallback without relying
  on the app binary's feature unification.
- Production keeps `MissingGlyphPolicy::Tofu` (the default). Tests that deliberately draw
  unsupported scripts use `HarnessBuilder::allow_missing_glyphs()` with a named reason; tests
  proving coverage keep the panic policy. Do not set tofu globally in production tests
  merely to silence a regression.
- Test fixtures use the licensed fonts pinned in the egui plan §10.6 (craft-fonts
  `2481a8a`, Noto Sans Arabic/Thai/CJK, BIZ UDPGothic; Noto Sans Hebrew and Noto Color Emoji
  from their upstream OFL sources with the recorded SHA256/licences). DesignCraft keeps its
  own policy: Latin UI/document fonts stay in `assets/fonts/`; central craft-fonts remains an
  optional `CRAFT_FONTS_DIR` input; do not copy new font binaries into this repository.
  Fetch fixtures into the task's explicit fixture directory and retain licences.
- Add a glyph-coverage test over every `i18n` catalog string and every UI font family,
  using `FontDb` faces, not egui's `has_glyph` alone (it can report false for the face
  that supplies the replacement glyph). Gate craft-fonts-specific CJK expectations on
  `CRAFT_FONTS`; when absent, assert the built-in coverage and skip CJK glyph assertions
  explicitly. Existing Ukrainian/Russian tests in `theme.rs` are the model.

RTL/bidi single owner:

- At D1 the fork's E5/E12 becomes the only bidi owner for egui-rendered UI/label text.
  `rtl::widget`/`rtl::label` pass logical `LayoutJob`s (with explicit language/direction
  where needed) to `fonts.layout_job`; remove the manual `BidiInfo::visual_runs` reordering,
  section reversal, neutral-character mirroring, Arabic family swap and per-row
  `row.pos.x += width - row.size.x` right-alignment in `rtl.rs`. Keeping them would process
  Arabic/Hebrew a second time in epaint's `BidiLevels::new` (the fork's
  `text_layout.rs:120` always runs bidi; `LayoutJob::direction` defaults to `Auto`).
  The document composer's own `crates/compose/src/bidi.rs`/`shape.rs` remains the bidi
  owner for document text and canvas glyph placement; this change is about the egui label
  path, not a rewrite of document composition.
- Keep `Galley::job.text` as the logical source and use logical strings for clipboard,
  save, accessibility, search and commands. Never pass visual-order/display-bidi text to
  shaping or the clipboard. Keep the bounded layout cache (256 labels/4096 bytes) keyed by
  the logical job, font token and language/direction.
- Editable `TextEdit` paths use the fork's logical text and grapheme/caret mapping; the
  custom canvas editor must not feed visual-order strings.
- D3 differential tests: Arabic/Hebrew/mixed fixtures assert visual glyph order, logical
  `Galley::job.text`, row widths, caret/hit positions, selection across boundaries, marks,
  end caret and exactly one alignment (no double right-shift). ASCII fixtures must remain
  identical to stock 0.37 where the API is shared. Run the fixtures at D3, not "only if
  replaced".

`Context::set_language` (E17, fork only) is applied once when `ui.language` changes,
after fonts are installed and before layout. It must not change stored document text,
ids, commands or persisted workspace data. `Context::language` is the fallback on the
web if eframe does not set it.

Do not enable `Style::right_to_left` (E9). DesignCraft keeps an LTR panel/docking model with
RTL text inside labels, matching the persisted `craft_ui::docking::Layout` and existing
workspaces. Enabling E9 would mirror menus, scrollbars and docking chrome without a product
decision and would double-mirror shaped text. The explicit `DockArea::direction(LeftToRight)`
above is the only docking direction policy in this migration.

Accessibility labels: kittest's `check_accessibility` now runs by default, and the first
green stage must satisfy strict names/glyph policy. Add explicit names where a control is
an input (`Role::Button`, `TextInput`, `CheckBox`, `Slider`, `SpinButton`, `ComboBox`, `Tab`,
`Splitter`):

- `icons::button`, `widgets::icon_toggle_sized`, `widgets::checkbox`,
  `widgets::dropdown`, toolbar/fill/stroke chips: use the existing localized tip/`label`
  via `Response::accessible_name` or `WidgetInfo`.
- `widgets::NumField`: add a label field or `.accessible_name`; do not leave the
  internal `id` as the only name.
- `dialogs::text_field`: pass the visible label id and use `ui.label_inputs_by(label.id,
  …)` or `ui.name_inputs(tr(key), …)`; section labels added as plain `ui.label` do not
  automatically name the field.
- `panels/font_menu.rs`, `panels/pages.rs`, `panels/layers.rs`, `panels/library.rs`,
  `story_editor.rs`, `new_document.rs`: bind each field to its visible label; icon-only
  star/expand buttons use their localized tooltip.
- Role migration files: the plan's earlier list omitted `panels/properties.rs`,
  `panels/swatches.rs`, `panels/layers.rs` and `object_control.rs`. Map: swatch/color well →
  `Role::Button` with the swatch name/value; layer row → `Role::ListItem`/`Role::Button`
  matching its selectable row behavior; object-control label → `Role::Label`; combo/dropdown →
  `Role::ComboBox`; numeric field → `Role::SpinButton`; text field → `Role::TextInput`;
  checkbox → `Role::CheckBox`; tab → `Role::Tab`; split/resize → `Role::Splitter`.
- `craft_ui::docking::DockArea` already names tab/overflow/resize controls after CL-1/CL-3;
  keep `Role::Tab`, `Role::Button`, `Role::Splitter` and labels stable.
- `Response::labelled_by` is preferred when a visible label exists;
  `Response::accessible_name` for icon-only controls; `Ui::name_inputs` only for
  third-party/grouped widgets that cannot be named at their construction site. Do not
  paper over a missing label with a generic "field" name.

### 3.8 Theme and visual defaults

`theme::apply` sets `Visuals`, text styles and widget palette from `Tokens`; most colors
are explicit. 0.37's roomier button defaults and larger corner radius can change layout
even when tokens are unchanged. Audit:
- `v.button_padding`, `v.menu_margin`, `v.window_margin`, `v.item_spacing`,
  `v.interact_size` and `CornerRadius` in `theme::apply`;
- `widgets::FIELD_H`, `BUTTON_H`, `SEG_W`;
- panel default sizes in `dock.rs`, `panel_docking.rs`, `toolbar.rs`;
- menu popup width/height and `menus::menu_button`;
- `chrome::app_bar` / `control_bar` / `status_bar` heights.

Do not replace explicit geometry with 0.37 defaults. A visual diff is an intended change
only after review and is recorded in evidence.

## 4. Dependency and publication topology

### 4.1 Intended suite graph

For suite validation, DesignCraft resolves exactly one copy of each egui-family crate and
one winit. The revisions below are placeholders until the reviewed `storytold` publications
exist; candidate `aiongg` hashes are comparison inputs only.

```
designcraft / designcraft-web / designcraft-ui-egui
  -> egui 0.37.0          git <FINAL_STORYTOLD_EGUI_REV>   (contains E1-E21, E21b, E4/P2b)
  -> eframe 0.37.0        git <FINAL_STORYTOLD_EGUI_REV>
  -> egui-winit 0.37.0    git <FINAL_STORYTOLD_EGUI_REV>
  -> egui-wgpu 0.37.0     git <FINAL_STORYTOLD_EGUI_REV>
  -> egui_kittest 0.37.0  git <FINAL_STORYTOLD_EGUI_REV>   (dev)
  -> epaint/ecolor/emath/epaint_default_fonts  git <FINAL_STORYTOLD_EGUI_REV>
  -> winit 0.30.13        git <FINAL_STORYTOLD_WINIT_REV>  (W1-W27 plus W15P PRIMARY)
  -> craft-ui             git <FINAL_CRAFT_UI_REV>         (CL-2F after adapter review; includes CL-1/CL-3)
  -> wgpu 30.0.1          registry
  -> accesskit 0.25.1     registry
```

`egui_extras` is not used by DesignCraft; do not add or patch it. `egui_plot` and
`egui_dock` are not used; do not assume version numbers for them. `egui_system_font_provider`
is not used because the `system_fonts` feature stays off.

### 4.2 Root `[patch.crates-io]` additions

A dependency's own `[patch]` section does not propagate to this consumer. Add all of the
following to the root `Cargo.toml`, each with the same full immutable git revision and no
moving branch or machine-local path:

```toml
[patch.crates-io]
egui = { git = "https://github.com/storytold/egui", rev = "<FINAL_STORYTOLD_EGUI_REV>" }
eframe = { git = "https://github.com/storytold/egui", rev = "<FINAL_STORYTOLD_EGUI_REV>" }
egui-winit = { git = "https://github.com/storytold/egui", rev = "<FINAL_STORYTOLD_EGUI_REV>" }
egui-wgpu = { git = "https://github.com/storytold/egui", rev = "<FINAL_STORYTOLD_EGUI_REV>" }
egui_kittest = { git = "https://github.com/storytold/egui", rev = "<FINAL_STORYTOLD_EGUI_REV>" }
epaint = { git = "https://github.com/storytold/egui", rev = "<FINAL_STORYTOLD_EGUI_REV>" }
epaint_default_fonts = { git = "https://github.com/storytold/egui", rev = "<FINAL_STORYTOLD_EGUI_REV>" }
ecolor = { git = "https://github.com/storytold/egui", rev = "<FINAL_STORYTOLD_EGUI_REV>" }
emath = { git = "https://github.com/storytold/egui", rev = "<FINAL_STORYTOLD_EGUI_REV>" }
winit = { git = "https://github.com/storytold/winit", rev = "<FINAL_STORYTOLD_WINIT_REV>" }
```

The egui remote must be the published, reachable `storytold/egui` mirror containing the
reviewed fork history. Pinning `aiongg/egui` directly is not the production plan; it would be
an explicit policy decision, not a default. The winit remote must likewise be the final
reviewed `storytold/winit`, not the candidate URL/revision. A branch name, a machine-local
path, or a `path` override must never appear in a shipped manifest.

Workspace dependency versions change from `0.36` to `0.37` for `egui`, `eframe` and
`egui_kittest`. `egui_extras` is present in the workspace dependencies but is not used by
DesignCraft; bump it only if a future consumer adds a real dependency on it.
`apps/designcraft` keeps its eframe feature list and adds `clipboard` in D1 (see §3.5). Do
not add `system_fonts`.

### 4.3 Lockfile, clean checkout and duplicate checks

After generating a lockfile through the managed build workflow, verify:

- `Cargo.lock` has exactly one `egui`, `epaint`, `ecolor`, `emath`, `eframe`,
  `egui-winit`, `egui-wgpu`, `egui_kittest` and `winit` entry.
- Every egui-family source is `git+https://github.com/storytold/egui?rev=<FINAL...>#<FINAL...>`;
  no `registry+.../egui-0.36.2` or `-0.37.0` remains and no candidate `aiongg` source is
  shipped.
- `winit` source is the exact final storytold revision; no registry 0.30.13 and no old
  `3d51077f...` copy remains.
- `wgpu` stays `30.0.1`; `accesskit` moves to the version egui 0.37 requires
  (`0.25.1` in the reference graph).
- `cargo metadata --locked --format-version 1` resolves the same graph as the lock.
- `cargo tree -d` shows no duplicate egui-family crate and no duplicate winit. A
  duplicate `accesskit_consumer` from unrelated crates is a warning to inspect, not an
  automatic failure.
- `cargo xtask layers` still reports `designcraft-ui-egui` as the only egui consumer
  below L7, and `cargo xtask wasm` includes `designcraft-ui-egui`.
- `cargo xtask ci` passes with the exact lock and with and without `CRAFT_FONTS_DIR` for
  the font tests that support both.
- Clean checkout: `cargo fetch --locked` and `cargo metadata --locked --format-version 1`
  succeed without local overrides. Negative fixture: replacing the final egui/winit revision
  with a candidate hash or the old DesignCraft winit override must fail the gate.

The root `[patch.crates-io]` also contains the unrelated PDF entries. Keep
`krilla`, `hayro`, `hayro-interpret`, `hayro-syntax` and `hayro-jbig2` at
`storytold/craft-libs` `89dcbb3060377a26c99a6d80caa16084366704c4` unchanged. They are not
egui vendors and have their own provenance, tests and removal conditions.

### 4.4 craft-libs CL-1/CL-2 and language work

CL-1 is a rebased API port, not a merge of PR 199's tree. Start from current
`4919088`, apply only the `8614e4a` hunks (plus relevant `1764bca` README changes), and
preserve `d66e345`/`fd9cf54`, `crates/platform`, security/settings/printing and unrelated
crates. Review the diff against both `5c874d51` (DesignCraft's current pin) and `4919088`.
The corrected revision must:
- keep `craft-ui` production source compatible with **stock** egui 0.37.0;
- expose the same docking transaction model and `DockArea` API DesignCraft uses, plus the
  CL-3 opt-in direction API;
- provide the CL-2 capture slot and CL-2F frozen API used by DesignCraft's screenshot adapter;
- pass its registry-egui 0.37 check, all-feature clippy and tests;
- publish an immutable revision that DesignCraft can pin.

DesignCraft does not take fork-only API from `craft-ui`: the fork lives in the dependency
patch table, not in `craft-ui`'s manifest. This keeps `craft-ui` publishable and lets
DesignCraft choose the fork independently.

Language work (#198) enters DesignCraft only through the final fork pin and the selected app
changes in §3.5/§3.7. It must not introduce ICU locale conversion, change document language
semantics, mirror the whole UI, add fonts, or replace the docking model.

## 5. Existing patches and vendored fixes: keep/remove audit

DesignCraft has no `vendor/` directory. Its relevant non-registry inputs are:

| Patch / input | Current revision | Provenance | Decision for this migration | Removal condition |
| --- | --- | --- | --- | --- |
| `winit` | `storytold/winit` `3d51077f...` (W1–W14) | storytold/winit `craft/0.30.13`; X11 `with_x11`, macOS `with_close_requested_on_terminate`, `mac_window::sync_movable` | Replace with `<FINAL_STORYTOLD_WINIT_REV>` (W1–W27 plus W15P). W12/W14 remain; W15/W15P/W25/W26/W27 are needed by E4/E19/E21. | Upstream winit 0.30/0.31 contains the equivalent patches and the suite no longer needs the fork. |
| `egui`/`eframe`/`egui-winit`/`egui-wgpu`/`egui_kittest`/`epaint`/`ecolor`/`emath` | registry 0.36.2 | crates.io | Replace by `<FINAL_STORYTOLD_EGUI_REV>` containing E1–E21, E21b and E4/P2b in D1 after the isolated stock-0.37 comparison, including P7c/P8. Candidate `a4f9a17f` is comparison input, not the pin. | Upstream egui release contains the equivalent behavior (or the suite drops each patch when its `PATCHES.md` condition is met). |
| `egui_extras` | registry 0.36.2, only in `craft-ui` dev-dependencies | crates.io; not used by DesignCraft | Leave alone. Do not add a patch entry unless a DesignCraft crate directly uses it. | No removal needed. |
| `krilla` | craft-libs `89dcbb30...`, version `=0.8.2` | PDF backend vendor patch in craft-libs; unrelated to egui | Keep exactly. | Its own PDF backend gate. |
| `hayro`, `hayro-interpret`, `hayro-syntax`, `hayro-jbig2` | craft-libs `89dcbb30...`, pinned versions | PDF parser vendor patches in craft-libs | Keep exactly. | Their own PDF parser gates. |
| `craft-ui` | `5c874d51...` | craft-libs published revision | Move to the reviewed CL-1/`<CL-2_PROVISIONAL_REV>` in the D1 capture pilot, then `<FINAL_CRAFT_UI_REV>` (CL-2F/CL-3). | Replaced by the final reviewed revisions. |
| `craft-pdf`, `craft-atomic`, `craft-cms`, `craft-docpath`, `craft-print` | individual craft-libs revisions | Shared libraries, not egui | Keep. Do not conflate their revisions with the UI migration. | Their own release cadence. |

Do not delete an unrelated vendor or patch merely because the lockfile changes. Do not copy
`aiongg/egui`'s workspace `[patch]` section into DesignCraft's manifest; only the root
`[patch.crates-io]` entries above apply.

## 6. Test and regression strategy

### 6.1 Fixtures and determinism

- Use `file.newSample` / `designcraft_engine::sample::magazine()` for a deterministic
  multi-page document. Avoid user files and environment-dependent font caches.
- Run native tests with `DESIGNCRAFT_NO_PREFS=1` and a dedicated
  `DESIGNCRAFT_CONFIG_DIR`, or an explicit empty override, so preferences and recovery do
  not leak between runs.
- Build with the pinned craft-fonts ref `abb83316d96aa59c1cf64784289e378fe9fa5695` for
  release/font evidence, and also without `CRAFT_FONTS_DIR` to keep the bundled-font path
  tested. Use the §10.6 font fixtures for Arabic/Thai/CJK/Japanese/Hebrew/color-emoji
  coverage with their recorded SHA256/licences; keep them out of app source.
- Use fixed window sizes 1440×900 and 1280×800, at 1× and 2× `pixels_per_point`, and
  fixed themes `mediumDark`, `light`, `highContrast`. Record the exact fixture, font
  directory, DPI and OS/renderer with every screenshot.
- Use `CRAFT_UI_DOCKING_FIXTURES` for the existing explicit docking visual fixtures;
  do not turn those optional captures into unconditional CI jobs.
- Every command runs through `maintenance/task.py run/status/finish` with an allocated
  target/VM and durable evidence; do not run tests directly.

### 6.2 Unit / kittest regressions

| Area | Test (new or adapted) | User-observable assertion |
| --- | --- | --- |
| Frame staging | `logic_frame_runs_once_across_a_discarded_pass` | Force a `request_discard` by making the Tools panel single-column too tall; assert one font/theme/recovery/FPS tick and one input stage, not two. |
| Host staging | `host_multipass_counts_one_presented_frame` | Drive the real host `App` wrapper through a forced discard; `frames`/startup.presented/error logging advance once, and macOS menu/Apple-event polling is not duplicated when available. |
| Hidden logic | `hidden_logic_ticks_after_completed_multipass_keep_working` | A completed multipass frame followed by repeated `run_logic` ticks runs font readiness/recovery/queue checks each tick with fresh viewport data and no stale index assumption. |
| Hidden forced paint | `forced_hidden_paint_is_a_ui_pass` | `RequestPaintWhileHidden` with `visible() == Some(false)` and with `visible() == None` runs `logic` + `ui`; visibility is not used as a phase guard. |
| Control while hidden | `control_gesture_requests_paint_until_all_events_apply` | A hidden multi-frame `ui.click`/`ui.drag`/`ui.key` sends `RequestPaintWhileHidden` on each hidden frame while events remain; the final command/undo result is correct. Pure `engine.execute` requests no paint. |
| Screenshots | `two_screenshot_viewports_resolve_to_their_own_paths` | Internal capture-helper test (the `ui.screenshot` control method stays ROOT-only): two outstanding requests with different tokens/viewports receive the matching image; no cross-write. |
| Screenshot lifecycle | `screenshot_cancel_timeout_and_late_callback_release` | Timeout/cancel/close writes no file and sends no second reply; a late callback releases its image; aggregate pending bytes return to zero. |
| Screenshot memory | `screenshot_admission_bounds_aggregate_bytes` | Oversized/concurrent captures are rejected or bounded; peak retained bytes stay within the admitted budget; E2 surface-fit scaling is correct. |
| Mouse wheel | `wheel_events_keep_their_source_and_phase` | Alt-wheel zoom, plain-wheel scroll, trackpad phase end and momentum source do not latch modifiers. |
| Roles/IDs | `role_and_id_migration_keeps_existing_state_keys` | A saved `UiState`/workspace round-trips; panel tabs, layer rows and styles resolve by the same stable IDs. |
| Panel state | `saved_panel_state_migrates_to_parent_salted_ids` | Old-format panel rects/widths load into the new parent-salted keys; the same outer rects and panel order result. |
| Panels | `panel_order_and_central_rect_are_unchanged` | App bar/control/status/tools/dock occupy the same outer rects; central canvas rect is unchanged at 1440×900 and after narrow-resize. |
| Docking | `saved_workspace_restores_tree_float_and_hidden_panel` (existing plus 0.37) | Docking tree, floating geometry and hidden placement survive serialization; invalid actions are atomic; direction changes do not reorder saved splits/tabs. |
| Second window | `second_window_opens_closes_and_keeps_main_focus` | Popout exists, close request removes it, main pane focus returns; main close can still be cancelled. |
| Hidden child (#8748) | `hidden_parent_child_viewport_updates_and_closes` | With the parent occluded/minimized/overdue and with `visible() == None`, the child opens/updates/closes and acknowledges without restoring the parent. |
| Accessibility | `every_visible_input_has_an_accessible_name` | `Harness::unnamed_widgets()` is empty; specific roles/labels for tabs, icon buttons, numeric fields, combo boxes, layer rows, swatches and text fields. |
| Glyphs | `every_catalog_character_has_a_face_or_a_declared_gap` | Bundled fonts cover Latin/Cyrillic catalogs; craft-fonts CJK assertions run when present; tests use panic policy for covered text and explicit tofu only for deliberate gaps. |
| IME exact bytes | `ime_replacement_deletes_exact_bytes_in_one_undo_step` | `DeleteSurroundingBytes` replaces a combining mark/letter and Undo restores the exact prior text in one step; `DeleteSurrounding` character path still works; caps/UTF-8/track changes are enforced. |
| IME surrounding text | `outgoing_surrounding_text_is_logical_and_bounded` | DesignCraft canvas emits logical model text after delete+commit/deletion-only; preedit excluded; <=4000 bytes at UTF-8 boundaries; focus/password/no-IME clears; no `None` placeholder; latest completed pass including None replaces the descriptor/payload atomically; owner remains present for Unavailable and W27 clear/epoch/serial behavior follows P7b. |
| IME composition | existing `canvas`/`story_editor` preedit-commit-cancel tests | Marked text does not duplicate, click-away discards on macOS, no undo step for a cancelled composition. |
| Text selection (#8763/#8764) | `click_away_then_button_acts_on_engine_selection` | A button reading the app's engine `TextSel` after clicking away from an egui `TextEdit` still acts on the intended range; stock 0.37's TextEdit cursor/selection loss is recorded and does not change the engine target; if a future control reads TextEdit state, the command is deferred with an explanation. |
| Commands/undo | `multipass_edit_is_one_undo_step` | A text edit, live-preview commit, dialog apply or toolbar/app-bar button produces exactly one history entry across discarded passes (including the second viewport). |
| Save/reopen | `save_reopen_preserves_document_and_workspace` | Save to a temp `.designcraft`, reopen through `file.openBytes`, compare document model/selection/style/swatches; saved workspace and panel layout restore. |
| Clipboard native | `copy_cut_write_the_system_clipboard` | On X11 and Wayland, select text/objects, Copy/Cut, read from an independent arboard reader; paste back; control actions never read or replace the clipboard. |
| CLI control privacy | existing `control_paste_never_reads_the_system_clipboard` | Control actions still never read or replace the user's clipboard. |
| Web drop | `web_drop_is_captured_in_raw_input_hook` | A browser `drop` with a synthetic File starts the async read in `raw_input_hook`, reaches the inbox once, and opens the document; no drop read remains in `WebShell::logic`. |
| Buffered input | `hidden_restore_drops_stale_text_ime_and_incomplete_pointer` | After a hidden tick, changed focus/document discards buffered text/IME/paste; a partial pointer chord is dropped; complete pointer/key events replay in order; two windows stay separate. |
| Menu/palette | existing `menus` tests plus 0.37 role checks | Menus open/close, keyboard activation, overflow within viewport, Quick Apply runs one command. |
| Canvas | existing guide/column/frame-hit tests plus new 0.37 geometry | Hit tests and smart guides do not regress from panel rect changes. |

### 6.3 Semantic accessibility assertions

Add `Harness::check_accessibility()` (the default) to UI tests and explicit queries:

- Menus: root menu items have `Role::MenuItem` or `Role::MenuBar`; disabled items remain
  reachable by name.
- Panel tabs: `Role::Tab`, selected state and localized label; tab drag still starts from
  the tab rect.
- Icon-only controls: tooltip text is the accessible name; star/expand, eye/lock, fill/
  stroke chips, screen-mode toggles, toolbar overflow.
- Text fields: each has a visible `labelled_by` relationship or a `placeholder`; a blank
  label is not an acceptable name. Numeric fields expose `Role::SpinButton` and a value.
- Swatches/color wells, layer rows, object-control labels, combo boxes: the mappings in
  §3.7, with name/value/selected state.
- Sliders and split handles: `Role::Slider`/`Role::Splitter`, value/range and accessible
  name; keyboard arrow changes still work.
- Dialog modals: `Role::Dialog` or a named `Area`; the title is the name; Escape cancels.
- Focus: Tab order reaches the control bar, panels, canvas and dialog buttons; focus ring
  uses `Tokens::accent`/`focus_stroke`; no invisible widget is exposed.

These are semantic assertions, not only screenshot checks. A screen reader sees the same
names and roles across 0.36.2 -> 0.37 except where a deliberate mapping (e.g.
`SelectableLabel` -> `Button`) requires a new role.

### 6.4 Visual, native and browser workflows

- Whole-window PNGs at 1440×900/1× and 2×, medium-dark and light, with the same sample
  document and fonts. Compare against 0.36.2 baseline produced in the same environment.
- Justify tolerances in advance: geometry assertions use exact `Rect` comparisons for
  panel/canvas/tab rects (within 0.01 pt), and pixel comparisons allow antialiasing noise
  only where the change is explained (for example 0.37 corner radius). Do not accept a
  golden because it changed; inspect `.new` vs `.old`, list the intended deltas, and keep
  the old baseline for rollback.
- Add high-contrast, Arabic, Hebrew, CJK and color-emoji fixtures. For RTL, compare glyph
  order, logical `Galley::job.text`, row width and caret/hit positions, not only pixels.
- Windows: use a uniquely owned clone and `C:\CraftApps\Launch-CraftDesktop.ps1` through
  the scheduled-task helper. Drive control-channel edit/save/reopen, dock/float/popout and
  `ui.screenshot`, inspect fresh screenshots, and record the software renderer
  (Microsoft Basic Render Driver) as a separate row.
- Linux: native X11 and Wayland runs with compositor identity; exercise panels, second
  window, copy/paste/PRIMARY, file drop, IME (Wayland) and visible edited/saved/reopened
  workflows. No inherited display/compositor exists at planning time; provision an
  isolated display/window manager and compositor before claiming this row.
- Browser: run the Trunk build in a pinned browser with WebGPU and `?webgl`; load the sample,
  edit, save/download, drag-drop a synthetic file, take screenshots and record the browser/
  adapter/backend. Wasm compile and `ui_shot` are not browser presentation evidence.
- Keep software-renderer and real-GPU results separate. Capture native screenshots with
  `ui.screenshot` or a native window capture; `ui_shot` is offscreen only.

### 6.5 Deliberate broken-behavior probes

Before accepting the suite, temporarily break each behavior and confirm the test fails:

1. Remove an `accessible_name` from a toolbar button -> accessibility test names it.
2. Change a panel salt (`"dock"` -> `"dock2"`) or load the wrong parent-salted test key
   -> panel identity/geometry test fails; app-owned saved layout remains separately asserted.
3. Call `ctx.root_ui` inside `DesignApp::ui` -> test panics with the documented recursive
   borrow message (run in a `catch_unwind` probe, not production).
4. Remove the first-pass `logic_frame` guard -> forced-multipass test sees two logic-frame
   ticks. Do not use `visible()` to "fix" it.
5. Correlate screenshots by arrival order instead of the per-request slot -> two-token/
   two-viewport test writes the wrong file; cancel a slot and let a late callback arrive ->
   no file/reply.
6. Drop `DeleteSurroundingBytes` handling or fall back to the 64-step `text.delete` loop ->
   exact-byte/one-undo test leaves replacement text or extra undo steps.
7. Remove the E21b surrounding-text adapter or emit display-bidi text -> logical/bounded
   outgoing-text test fails.
8. Restore `rtl.rs`'s manual reorder alongside fork E5/E12 -> RTL differential test shows
   double processing/double alignment.
9. Restore `Id::new`/`Ui::id` -> `clippy -D warnings` fails, proving the warning gate.
10. Reintroduce `Event::Screenshot` scanning or a strong `Context` capture in the callback ->
    compile/test or cancel/shutdown test fails.
11. Move web drop reading back into `WebShell::logic` -> browser drop test fails.
12. Remove the host first-pass guard -> host multipass test sees two frame counts.

## 7. Performance measurement protocol

### 7.1 Baseline timing and environment

Record a baseline **before** changing production code, from the exact current SHA
`e74208dd...`, lockfile, feature set and craft-ui revision. Run the candidate from the
exact migration revision. Use separate fresh processes for each sample; do not compare a
warm process with a cold one.

The benchmark harness is test-only work and must exist in both baseline and candidate
trees. Stage it in a separate test-only commit, cherry-pick it into a read-only baseline
worktree at `e74208dd...`, and run the baseline first through `task.py`. It records
`Instant`-based raw samples around `run_ui`/operation boundaries; it must not use
`perf.frame_ms`/`now_ms()` (`SystemTime` on native, `0.0` on wasm) as the acceptance timer.
`ui.inspect` reports only the last frame and `ui_shot` prints control replies, not per-frame
timings; neither is a sample source.

Record with every result:
- source SHA (baseline/candidate/stock-0.37 control), `Cargo.lock` SHA-256, feature list,
  `CRAFT_FONTS_DIR` and craft-fonts ref, fixture name/hash;
- OS and version, CPU/GPU, driver, backend (`Vulkan`/`DX12`/`Metal`/`GL`/`WebGPU`),
  renderer type (hardware vs WARP/llvmpipe), display scale, window size, pixels-per-point;
- profile (`release` unless a debug-only diagnostic is named), warmups, repetitions,
  raw samples, median/p95/p99, peak RSS, allocations, texture upload bytes and idle
  CPU/repaint count.

Host capacity/qualification facts: read-only inspection finds an i915 PCI `8086:3E96`
render device and a readable/writable `/dev/dri/renderD128` to the parent; the worker
sandbox may hide it. No display session or compositor is inherited. Availability is not a
qualified Vulkan/OpenGL/native compositor result; the managed renderer probe must identify
the actual backend. The Windows baseline is software-rendered (Microsoft Basic Render
Driver). A 128 GiB Windows full clone may not fit current capacity; use an allocated
baseline-backed overlay with an adequate reservation or retain the platform row as blocked.
macOS and physical Windows/audio remain separate rows.

### 7.2 Operations

1. **Startup**: at least 20 fresh processes; measure wall/`Instant` from process start to
   first presented frame and to first interactive sample document. Report RSS and the
   #8512 system-font discovery/first-missing-glyph component separately. The #8736 DX12
   renderer/shader/pipeline construction is a separate Windows row, never a UI frame.
2. **Idle UI**: open the sample, leave it untouched for 60 s, count frames/repaints and CPU.
   No continuous repaint storm; the font scan's 250 ms poll must stop once cataloged.
3. **Frame distribution**: at least 1,000 individual frames per configuration, with at
   least 10 warmups and raw per-frame samples; report p50/p95/p99. Run the sample, the Tools
   panel two-column state and a floating dock. Record shapes, vertices/indices, texture
   set/free count/bytes, galley cache size and requested-repaint count.
4. **Steady operations**: at least 30 timed iterations per operation after 10 warmups:
   layout, panel open/float/dock/activate latency, canvas zoom/pan/damage patches,
   selection and caret movement, text layout/tessellation.
5. **Text tessellation #8731**: fixed 120-label ASCII scene plus a mixed/color-glyph
   control; time `Context::tessellate` separately for 0.36, stock 0.37 and the final fork.
   If the ~60% plain-text regression reproduces, require a color-safe row-level fix or a
   documented exception; clamp-only is not a fix.
6. **#8736 DX12**: Windows clone; time renderer/pipeline/shader construction separately from
   first frame, with predictable filtering disabled/enabled as applicable. Record adapter
   and driver.
7. **#8512 fonts**: cold process with bundled fonts, with `CRAFT_FONTS_DIR` and with a
   test-only system provider fixture; compare discovery/startup and glyph coverage. Do not
   enable the system provider in production to win the measurement.
8. **#8763/#8764 selection**: select text then click a toolbar/context action; record
   command latency and assert the intended range/undo. This is correctness evidence; the
   tail timing is a separate sample.
9. **#8748 hidden child**: open/update/close the second viewport while the parent is
   hidden/occluded/overdue; record first visible child frame and close acknowledgement.
10. **Screenshots**: visible and hidden `ui.screenshot` requests; record reply latency,
    file size, retained bytes, cancellation/timeout and late-callback cleanup.
11. **Memory**: RSS after sample load, after 10 pan cycles, after 30 screenshots and after
    a drain/shutdown; retained image bytes must return to zero.
12. **Stock-0.37 control**: where the operation exists unchanged on stock 0.37, run the same
    minimal adapter/harness against 0.36, stock 0.37 and the final fork; explain semantic
    differences. Unavailable equivalence is an attribution limitation, not proof an
    upstream regression is unavoidable.

Use matched commands and the same fonts/fixture for baseline and candidate. Do not report
`cargo build` time as UI performance.

### 7.3 Thresholds and noise policy

Predefine before looking at candidate numbers. The shared suite floors are minimums; an app
may tighten, never loosen:

- UI frame median: candidate <= baseline + 5%; p95 <= baseline + 10%; p99 (>=1,000 frames)
  <= baseline + 15%.
- Canvas render median: <= baseline + 5%; p95 <= +10%; patch count no increase beyond
  fixture determinism.
- Idle CPU: <= baseline + 20% and no more than one extra repaint per second after fonts
  are ready; zero paints in 2 s after settle.
- Startup: <= baseline +5% median / +10% p95, with the #8736 DX12 shader-compilation component reported
  separately. A #8736-sized regression needs the upstream/fork fix or explicit approval.
- Screenshot reply p95: <= baseline + 10%; retained bytes bounded and zero after drain.
- Peak RSS: investigate a repeatable increase over +5% or +16 MiB, whichever is reached
  first; report absolute and percentage. (Use the app's existing stricter budget if one is
  found.)
- Text tessellation: if the isolated probe reproduces #8731's ~60% regression, require a
  color-safe row-level fix or a documented exception; never accept a clamp-only shader
  change.
- Sub-millisecond operations: the relative increase must also exceed 0.05 ms or be
  corroborated by stable instruction/allocation counts.

Sample counts: at least 10 warmups and 30 timed iterations per steady operation; at least 20
fresh processes for startup; at least 1,000 individual frames for frame distributions. Use
five independent sessions of at least 200 frame samples for tail percentiles, and pool the
raw samples for p99; do not report p99 from 30 samples. Alternate baseline/candidate rounds;
discard a session if a build/test/profiling process competes for CPU or the coefficient of
variation of medians exceeds 3% (5% for microbenchmarks), then rerun. If the difference is
within 2×MAD of paired samples, call it equal and do not chase it. Re-run and profile before
filing or accepting a regression. Never auto-update a snapshot or lower a threshold to hide
a timing or geometry change.

## 8. Staged implementation PRs and rollback

The dependency DAG is:

```text
plans land -> baseline harness + measurements
completed winit -> egui P8 (P2b/P7/P7b/P7c included)
  -> craft-libs CL-1 -> CL-2 reviewed capture prototype
      -> DesignCraft D1 / GridCraft G1 pilot adapters -> CL-2F API freeze
DesignCraft D1 -> D2 lifecycle -> D3 extended native/language qualification
craft-libs CL-3 + D1 -> D4 docking/accessibility
D2/D3/D4 -> D5 visual/workflow acceptance -> D6 final platform/performance evidence
```

- **D1 — atomic final-fork compatibility pilot.** Pin the complete final storytold
  egui/winit graph and reviewed craft-ui CL-1/CL-2 prototype; regenerate the lock.
  Apply roles/IDs/FontData/wheel changes, explicit native clipboard and test fonts,
  strict minimum names/glyphs, the shared capture-slot adapter and its cancellation/
  bounds/wake tests, parent-salted test lookup keys, and the required E21/E21b custom
  editor adapter. Remove `rtl.rs` duplicate visual reorder/alignment as the final fork
  becomes the bidi owner. Preserve app-owned UiState/docking preferences; no invented
  egui-memory migration. Gate: cargo xtask ci/wasm, UI/native tests, one-family graph,
  clean locked fetch, capture and IME contract tests, clipboard and save/reopen. This
  reviewed pilot plus GridCraft's adapter permits CL-2F; stock 0.37 is an isolated
  benchmark/control workspace, never a production handoff. Rollback restores complete
  e74208dd source/manifests/lock/executable and prior preferences.
- **D2 — frame stages and hidden input.** Add the narrow once-per-layout-frame guard,
  host pacing/queue separation, monotonic deadlines, buffered synthetic-input policy
  and per-step hidden paint. The shared capture API is already adopted in D1; extend
  hidden/multipass/restore stress tests here. Do not replace it with a local interim
  slot. Revert D2 as a behavior/test unit only if D1's compatibility checks still pass.
- **D3 — extended native/language qualification.** After D2 and CL-2F, use the same
  complete final fork and frozen helper; qualify E4/W15P, P7 key release, E21/E21b and
  P7c selection/hidden-child behavior on native platforms and add language policy as
  required. Required IME/bidi API adaptations already land in D1. Rollback removes
  only these later policy/test changes; it never strips fork patches while retaining
  fork-only source. A dependency rollback restores the full D1 predecessor set.
- **D4 — accessibility and glyph contracts.** Add the remaining names/roles, full role
  file list, explicit `DockArea::direction(LeftToRight)`, and coverage tests. Gate:
  `unnamed_widgets()` empty, explicit role/label queries, glyph coverage with and without
  craft-fonts, direction geometry/hit/drag and saved-layout tests. Rollback: revert the
  label/direction changes; no document or command behavior changes.
- **D5 — visual/geometry baselines and workflows.** Review every intended 0.37 visual
  delta, update fixtures only with explanation, add the save/reopen/dock/popout, native
  X11/Wayland, Windows VM and browser workflow tests. Gate: geometry/hit tests, pixel
  comparisons, deliberate broken-behavior probes. Rollback: restore the previous reviewed
  baselines; production remains on D3/D4.
- **D6 — qualification and performance.** Windows VM, Linux native, browser, macOS gate,
  paired performance protocol with the stock-0.37 control. No production behavior change
  unless a measured regression requires a separately reviewed fix. Rollback: no code
  rollback expected; keep the evidence and the old dependency set until acceptance.

Each PR must be one coherent outcome, use one task id per commit, and carry the validation
results at the reviewed revision. No PR is published without fresh direct human approval.

## 9. Risks, unresolved questions and blocking acceptance criteria

### Blocking unknowns

1. **Final dependency publications.** The final reviewed `storytold/egui` and
   `storytold/winit` revisions do not exist yet. Candidate `aiongg` hashes are comparison
   inputs only. D1 cannot ship until the complete P8 handoff and winit revisions are published with E1–E21,
   E21b, E4/P2b, W1–W27 and W15P, and a clean checkout fetches them `--locked`.
2. **craft-libs CL-1/CL-2F revisions.** The 0.37-compatible craft-ui revision must be the
   `8614e4a` port onto current `4919088`, preserving `d66e345`/`fd9cf54`, `crates/platform`
   and unrelated fixes. PR199's `b68de8c`/`ec8840a` trees are not bases. The capture API
   freezes only after the DesignCraft/GridCraft adapter review.
3. **Clipboard/PRIMARY.** D1 enables `eframe/clipboard` to preserve OS Copy/Cut/Paste. D3
   needs E4/W15P native evidence on X11 and Wayland (wlroots/KWin/Mutter): pointer-
   and keyboard-triggered ownership including keyboard-only focus, serial freshness, owner/seat changes, empty/absent offers, self-paste, canceled/
   slow transfers, password exclusion, no unchanged-selection reclaim and no second regular
   data device. A compile-only or X11-only result is not acceptance.
4. **Windows GPU fallback.** E2/E3 turns some egui-wgpu configure/renderer failures into
   `WgpuError` values rather than panics. DesignCraft's `gpu::finish` already handles
   `eframe::Error::Wgpu(_)`, but the exact startup fallback behavior must be tested on the
   Windows clone with a deliberately failing adapter/backend. Do not claim it from unit
   tests only.
5. **macOS native evidence.** macOS menu shortcuts, press-and-hold IME, `deleteSurrounding`
   bytes, `movable_by_window_background`, Dock Quit and viewport screenshots need real
   hardware or remain explicitly unresolved. A software/VM screenshot is not native
   evidence.
6. **`#8731` text tessellation.** The upstream report is a measured regression, not a
   DesignCraft result. If the candidate reproduces it, a color-safe fix (row-level branch)
   or an explicit exception is required before accepting the performance row.
7. **`#8736` DX12 startup.** The shader-compile cost is a Windows startup risk. Measure it
   separately and do not hide it inside a UI frame average.
8. **`#8748` hidden-parent/deferred-child.** Closed and unmerged. The final fork must own a
   tested solution; DesignCraft's immediate second viewport must open/update/close while the
   parent is hidden/occluded/overdue and while visibility is unknown.
9. **Accessibility scope.** The exact number of unnamed inputs will only be known after
   D1's first strict `Harness::check_accessibility()` run. D1 must fix the existing test
   surface or record an explicit per-test policy; D4 completes the remaining controls. If
   the D4 remainder is large, split "core chrome/panels" and "dialogs/rare panels"; do not
   disable the check globally.
10. **Text selection #8763/#8764.** The source audit finds no control that reads an egui
    `TextEdit` selection in the same pass, so no app workaround is planned. The fork owns
    the upstream fix. If implementation finds such a control, carry #8764 or defer the
    command with a tested app change; do not hide it in the test.
11. **E21b/P7b.** The incoming E21/W27 path is not full surrounding-text integration.
    DesignCraft's canvas needs a real logical model/preedit/UTF-8 adapter, bounded at 4000
    bytes and cleared on focus/password/no-IME; `None` placeholders are not acceptance.
    Native Wayland IME trace remains required.
12. **DockArea direction.** CL-3's opt-in direction is a visual policy, not a schema change.
    DesignCraft passes `LeftToRight` explicitly for now; saved physical splits/rectangles
    and logical tab order must remain unchanged. No display-bidi text reaches shaping or
    clipboard.
13. **Platform capacity/qualification.** The parent can see i915 PCI `8086:3E96` and
    `/dev/dri/renderD128`; the worker sandbox may hide it, and no display session/compositor
    is inherited. Windows needs a uniquely owned clone/overlay within allocator capacity
    (a 128 GiB full clone may not fit). Browser WebGPU/WebGL and real native X11/Wayland
    workflows need provisioning. These rows remain blockers until executed; availability
    alone is not a pass.
14. **Capture API freeze.** CL-2F must be reviewed after both adapters. A strong `Context`
    capture, unbounded pending images, a late file write/reply or a lock held across wakeup
    is a blocking failure, not a follow-up.
15. **`Cargo.lock` coherence.** Regenerate, never hand-edit. A lock with both 0.36.2 and
    0.37, a candidate `aiongg` source, or a machine-local path is an immediate failure.

### Non-blocking but important

- Style default changes may move toolbar/panel geometry even with explicit tokens.
- `egui_kittest` 0.37's missing-glyph panic can make an unrelated test fail first; fix the
  fixture/name or use a named per-test allow with a reason, not a global tofu/disable switch.
- The web build's new glyph rasterizer can change emoji/fallback rendering. Verify with
  WebGPU and `?webgl`, and keep texture filter/wrap expectations.
- `Cargo.lock` must be regenerated through the managed workflow, not hand-edited.
- The `designcraft-testkit` workspace dependency points at a missing `crates/testkit` in
  the baseline; it is unused, but the dependency audit must not mistake it for an egui
  consumer. Do not add or remove it as part of this migration.
- Saved docking direction must never be implemented by reversing saved panel order or
  serialized tab order.

## 10. Acceptance checklist

- [ ] Exact baseline SHA and dependency revisions recorded; old 0.36.2 set retained for
      rollback.
- [ ] All 22 manifests, root lockfile, root patch section, build scripts, `ui_shot`,
      platform cfgs and CI workflows audited.
- [ ] Exactly one egui family and one winit resolve from the final reviewed storytold
      revisions; no registry 0.36.2, candidate `aiongg` source or machine-local path remains.
- [ ] Clean checkout passes `cargo fetch --locked` and `cargo metadata --locked`.
- [ ] `cargo xtask ci`, `cargo xtask wasm`, `cargo test -p designcraft-ui-egui --lib`,
      `trunk build --release`, native Windows/Linux and browser workflows pass at the
      reviewed revision.
- [ ] `App::logic`/`App::ui` root-`Ui` flow preserved; native and web per-pass logic tested;
      no recursive `root_ui`; multipass, repeated hidden ticks, forced hidden paint and
      host counters tested; visibility is not used as a phase.
- [ ] No egui PanelState persistence is invented; parent-salted test keys, panel order/rects and app-owned workspace
      restore.
- [ ] Screenshots use the craft-ui completion slot: weak callback, viewport-aware wakeup,
      one-shot, timeout/cancel/close, out-of-order, bounded aggregate bytes and no strong
      `Context` cycle.
- [ ] Commands, undo transactions, save/reopen, workspace persistence, clipboard privacy
      and native Copy/Cut/Paste are unchanged.
- [ ] E4/W15P PRIMARY is evidenced natively on X11/Wayland, or explicitly unresolved;
      password/unchanged-selection policy holds.
- [ ] E21b outgoing surrounding text is logical, bounded at 4000 bytes, preedit-excluded and
      cleared on focus/password/no-IME; no `None` placeholder.
- [ ] Fork E5/E12 is the only bidi owner for egui UI/label text (the document composer
      remains the document-text owner); no display-bidi text reaches UI shaping or clipboard.
- [ ] Accessibility role/name assertions pass under strict kittest; glyph coverage is
      explicit with and without craft-fonts.
- [ ] Visual baselines have explained intended deltas; geometry/hit/direction/dock tests
      pass; #8763/#8764 and #8748 tests have the documented expected outcome.
- [ ] Paired performance results meet the predefined thresholds with >=10 warmups/30
      steady iterations/20 fresh startups/1,000-frame distributions and a stock-0.37
      control, or every regression has a reviewed fix/exception.
- [ ] macOS, real-GPU, Wayland/GNOME and non-Latin-layout gates are either evidenced or
      listed as unresolved; no result is implied from a compile or software renderer.

## 11. References

Primary source inspected locally (read-only; no checkout switch):

- egui upstream `0.37.0` = `841a328ba2dd8f6996beb7189ebb7fc00e668a2a`:
  `crates/egui/src/data/input/event.rs`, `data/input/mouse_wheel_source.rs`,
  `data/input/viewport_info.rs`, `data/screenshot_callback.rs`, `viewport.rs`, `context.rs`,
  `containers/panel.rs`, `id.rs`, `ui.rs`, `data/output.rs`, `accessibility.rs`,
  `input_state/mod.rs`; `crates/epaint/src/text/font_definitions.rs`,
  `crates/epaint/src/text/font_data.rs`;
  `crates/eframe/src/{epi.rs,native/epi_integration.rs,native/wgpu_integration.rs,web/app_runner.rs}`;
  `crates/egui-winit/src/{clipboard.rs,lib.rs}`; `crates/egui-wgpu/src/{setup.rs,winit.rs}`;
  `crates/egui_kittest/src/{builder.rs,accessibility.rs}`.
- Candidate fork for comparison: `aiongg/egui` `craft/0.37` =
  `a4f9a17f64a6bae61350fb85804c9df0d9865916`, including `PATCHES.md` (E1–E21; E21b/P7b
  still to be added). This is not the production pin.
- Candidate winit for comparison: `aiongg/winit` `craft/0.30.13-lang` =
  `8b35388bd719e8cc86a95971ba0bc026c9a8f058`, including `PATCHES.md` (W1–W27; W15P still
  to be added). This is not the production pin.
- `storytold/craft-libs` PR 199 head =
  `b68de8c5f4f1c060904d211a7b1320aa0f7bce78`; audit merge
  `ec8840af8c5fda695cf195cdd05e5ae078415c07`; API-port source
  `8614e4a04fae516fba16c22b31ff1008cb6be10b`; current craft-libs baseline
  `49190884278f16c9a9f5823d7698a8ffb5deb062`.
- `storytold/egui` published 0.36.2 =
  `767b1cad294120689a514bb4bd68c077e46679b9`; `storytold/winit` published 0.30.13 =
  `ffae5198770d7730e718043f035a772b7dbc3b1b`.
- DesignCraft baseline =
  `e74208dd7e0c801d15dfdf926d64cfe5e3b346d3`.
- Lifecycle evidence:
  `/home/codex/work/ssd2/task-evidence/egui037-migration-20261022/lifecycle-source-audit.md`
  (native/web per-pass logic, zero pass index after completed `run_ui`, `visible()` is not a
  phase).
- IME evidence:
  `/home/codex/work/ssd2/task-evidence/egui037-migration-20261022/ime-surrounding-gap.md`
  (E21b/P7b gap and custom-editor inventory).
- Shared suite contract:
  `/home/codex/work/CraftApps/egui037-migration-20261022/craft-libs/docs/egui-037-suite-rollout.md`
  (completion slot, DockArea direction, performance sample counts, stock-0.37 control,
  platform evidence) and `craft-libs/docs/egui-037-migration-plan.md` (§7.7, §7.8, §10).
- Font fixtures: `egui/docs/egui-037-migration-plan.md` §10.6 (craft-fonts `2481a8a`,
  Noto Sans Arabic/Thai/CJK, BIZ UDPGothic, Noto Sans Hebrew, Noto Color Emoji; exact
  SHA256/licences). No font binaries are added by this plan.
- Historical diagnostics:
  `/home/codex/work/ssd2/task-evidence/egui037-audit-20261022/{fork,stock}/designcraft.{errors,log,prepare.diff}`.
- Upstream reports and notes reviewed through the local snapshot
  `/home/codex/work/ssd2/task-evidence/egui037-migration-20261022/upstream-discussions.json`:
  [#8731](https://github.com/emilk/egui/issues/8731),
  [#8736](https://github.com/emilk/egui/issues/8736),
  [#8763](https://github.com/emilk/egui/issues/8763) / PR
  [#8764](https://github.com/emilk/egui/pull/8764) (unmerged),
  [#8748](https://github.com/emilk/egui/pull/8748) (closed, not merged),
  [#8631](https://github.com/emilk/egui/pull/8631),
  [#8535](https://github.com/emilk/egui/pull/8535),
  [#8489](https://github.com/emilk/egui/pull/8489),
  [#8555](https://github.com/emilk/egui/pull/8555),
  [#8583](https://github.com/emilk/egui/pull/8583),
  [#8512](https://github.com/emilk/egui/pull/8512),
  [#8428](https://github.com/emilk/egui/pull/8428),
  [#8657](https://github.com/emilk/egui/pull/8657),
  [#8577](https://github.com/emilk/egui/pull/8577),
  [#8641](https://github.com/emilk/egui/pull/8641).

These links identify the behavior and reports inspected. They are not fresh DesignCraft
validation, and an upstream status may have changed since the snapshot.


## Local implementation update — 2026-10-23

The user clarified that reviewed plans remain local and implementation proceeds before publication. The earlier requirement to publish/land this plan first is superseded; pushes and PR publication still require explicit approval.

Canonical `storytold/designcraft` `main` now resolves to `2e36dcf909627c8f0e5662fe6b4825b1c1c24420`. The owned checkout was fast-forwarded to this revision. Earlier baseline SHAs and line anchors above are retained as historical review evidence; use this new revision and its exact lockfile for current-main integration and paired measurements. The original refs remain reachable.

Preserve browser page zoom and canvas touch-action:none alongside packaging fixes.

The complete static delta review and exact old/new object identities are retained in the task evidence: `canonical-diffs-refresh-20261023.json` and `overnight-consumer-review-20261023.md`. This amendment claims no new build, native GUI or performance acceptance.

### Local pinned font inputs (2026-10-23)

Parent fetched and verified all eight manifest fonts at `storytold/craft-fonts`
`2481a8a324b986a45954e90045ed1f8f2efaaf3c` into
`/home/codex/work/CraftApps/egui037-migration-20261022/fixtures/craft-fonts`.
Use this exact directory as the optional `CRAFT_FONTS_DIR` build input. All
manifest SHA256 hashes match; licenses retained. This does not waive the
required no-corpus test configuration. The separately pinned Hebrew fixture
is at `/home/codex/work/CraftApps/egui037-migration-20261022/fixtures/noto-sans-hebrew/NotoSansHebrew.ttf`
with adjacent OFL.txt/provenance.json. Keep these fonts out of app source;
required test fixtures may use an explicit external input with verified hash
and clear missing-input behavior, never silently disable glyph checks.

## Final local landing record — 2026-10-25

This section records the current local landing state. Everything above remains historical planning and
implementation evidence: the older candidate hashes, baseline anchors and "planning only" wording
describe earlier states and are not the current pins. No commit, index write, ref update, fetch,
push, pull request, comment, merge, build, fmt, test, Cargo, VM, resource or cleanup action was
performed while writing this record; the qualification results below were produced separately by
the parent through the managed `task.py` workflow and are cited as evidence, not rerun here.

### Current dependency pins

| Item | Revision |
| --- | --- |
| egui family (`egui`, `epaint`, `ecolor`, `emath`, `eframe`, `egui-wgpu`, `egui-winit`, `egui_kittest`, `egui_extras`, `epaint_default_fonts`) | `storytold/egui` `6d518fc799681571963842f95b55a6b68431b15a` |
| winit | `storytold/winit` `a517b3846ee8457d84ee4975110bfda6797c44dc` |
| craft-libs (`craft-ui`, `craft-print`) | `storytold/craft-libs` `25da1314d443171b4adeaad2cd9454b273f35c6f` |
| CI craft-fonts | `storytold/craft-fonts` `b076417b5880bd4154bd22f00695b1d28c6bea83` |

`Cargo.toml` and `Cargo.lock` resolve one egui family and one winit from these storytold revisions;
the lock has no registry 0.36.2 egui family and no candidate `aiongg` source. `crates/fonts/build.rs`
continues to treat craft-fonts as an optional `CRAFT_FONTS_DIR` input, not a manifest dependency.

### Exact dirty union

| Item | Value |
| --- | --- |
| Checkout | `/home/codex/work/CraftApps/egui037-migration-20261022/designcraft-implementation` |
| Git worktree HEAD (unchanged) | `80d13911b2d20fbb5cd9398696627bb3a715417b` |
| Earlier integrated main base in the dirty state | `d8d78851729a2c1a10e5bfad7c591f3de78651a0` |
| Imported new main | `45ed6cfce0f86e59ff66ab18a05c3ec03ccef531` (`refs/migration-observations/20261024-1610-main`) |
| New-main distance from the earlier base | 141 commits, 127 changed paths |
| Files in the union | 619 = 613 tracked + 6 untracked |
| Worktree status | 186 entries = 180 modified + 6 untracked |
| Merge state | no `MERGE_HEAD`; index empty; no conflict markers; `git diff --check` clean |

The six untracked paths are exactly:

- `crates/compose/src/placed.rs`
- `crates/idml/src/tests/facing_ltr.rs`
- `crates/idml/src/tests/frame_grid.rs`
- `crates/ui-egui/src/page_history.rs`
- `crates/ui-egui/src/panels/story.rs`
- `docs/egui-037-migration-plan.md`

The 16:28 reconciliation packet recorded 185 status entries: the 92 pre-existing dirty paths plus the
127 imported paths with 34 overlaps. The current tree adds one later modified tracked path,
`apps/designcraft/src/native_menu.rs` (the native-menu/action host-order repair), for 186. Of the 127 imported paths, 103 were adopted whole
and 24 were reviewed 3-way content merges; later reviewed deviations are the Windows-only test fixtures
in `apps/designcraft-cli/tests/it/cli.rs` and `crates/engine/src/file_access.rs`, the documented
`Id::new` → `Id::unique` port in `crates/ui-egui/src/panels/story.rs`, and the host-order changes above.

`80d13911` is an ancestor of `45ed6cf`, so an ordinary merge would fast-forward. The eventual manual
union commit therefore needs both parents, `80d13911b2d20fbb5cd9398696627bb3a715417b` and
`45ed6cfce0f86e59ff66ab18a05c3ec03ccef531`, to preserve both the migration branch and the imported
main history. No such commit exists yet.

### Review-1643 findings on the current source

Both actionable defects from the 16:43 review are fixed in the current source:

- **Document generation identity.** `InputGeneration` and `ScreenshotGeometry` now compare
  `DocState::identity_epoch` through `active_document_epoch()` (`crates/ui-egui/src/lib.rs`), so a
  `file.revert` that preserves the UID still invalidates buffered input, native drops and admitted or
  in-flight screenshots. Regression tests cover a buffered input batch, a buffered native drop and
  screenshots before issue and in flight; all pass in the Linux full-CI job below.
- **NumField disabled presets.** The preset segment now uses `Sense::hover()` and reports
  `self.enabled == false` when the field is disabled, does not open its menu, and dims its chevron;
  `disabled_numeric_presets_are_disabled_and_do_not_open` passes.
- The earlier `panels/properties.rs` evidence mismatch is also closed: the current file is
  `9515f944ddbbf61bb2099324ac6825fd7603262adc605686c2a932948a81f768`, matching the later
  Windows-qualified manifest and the current worktree.

No other real source defect was found in this bounded audit. Residual items below are qualification
limits, not claimed defects.

### Current qualification evidence

- **Linux full CI** `egui037-designcraft-final-characterization-ci-20261025-0050`: all 9 steps
  passed (fmt, pdf-deps, clippy, test, i18n-report, doctest, assets, layers, wasm); 77 result
  summaries total **3230 passed, 0 failed, 6 ignored**. The job's captured unstaged patch is
  byte-identical to the current tracked worktree diff, so it qualifies the current tracked source.
  The run includes the corrected canvas characterization test over three batch shapes and the
  discard/no-discard matrix.
- **Windows qualification** job 102: all 9 steps passed, **3198 passed across 77 result summaries**
  on the 619-path source manifest `5208ddb75ad8d06a48c334b48e5e80dc193ae2b9739b119dd50942aa3ebd5a40`.
  Exactly two test-only Windows fixture paths differ from the applied prior manifest
  (`apps/designcraft-cli/tests/it/cli.rs`, `crates/engine/src/file_access.rs`); the engine production
  prefix is unchanged. Qualified desktop executable
  `a34478f09ccae2571320b3ada489665adae450031ca24918ec4e8b165edfc47a`; qualified CLI
  `318ec6cb7521f941aaab01a63b2c0f970bd70bfb50e5545aa645f63d1589cbee`.
- **Native Windows GUI** `native-gui-2238`: edit, undo, redo, selection, save and reopen were captured
  with document, UI and window evidence; the native input diagnostic ordered three production
  click/End replays on the qualified executable, each reaching click caret 6 and End caret 12, with
  all three text strings unchanged and the document clean. Six screenshots were reviewed, but the state JSON is the exact caret
  oracle because a blinking caret can make two screenshots share pixels.
- **Native input diagnostic** `native-order-review0048.txt`: no application event loss is shown. The
  trace records the End press/release pair arriving before the click pair; the app acted correctly
  for the order it received. The temporary diagnostic build was fully restored to the exact 619-file
  qualified source and binaries in the VM before the ordered production replays.
- **Exact-source boundary.** The Windows 619-path manifest predates the host-only canvas
  characterization test. In the current tree, `crates/ui-egui/src/canvas.rs` differs from that
  manifest only inside `mod tests` (the first difference starts at line 6016; the test module starts
  at line 4046), so production bytes are unchanged. The Windows run did not execute that new test;
  Linux full CI did. This document is amended after the Windows manifest, so the documentation file
  is also not byte-identical to that earlier snapshot.

### Remaining limits and regression risk

The union includes 141 upstream main commits and 127 changed paths, only part of which is egui 0.37
API work. It also imports page history/Go Back, the Story panel, notes and table work, optical-margin
options, linked-file read caps, SVG preflight, IDML facing/frame-grid tests, and other current-main
features. Review the 180-file tracked diff and the 24 manual overlap resolutions as product changes,
not as a mechanical dependency bump; the imported tests reduce but do not remove that risk.

Still unqualified for this exact union: macOS native menus/IME/Apple events, real-GPU rendering and
the DirectX 12 startup row, native Linux X11/Wayland presentation, native Windows IME and clipboard/
PRIMARY selection, physical-device input ordering, browser presentation beyond the wasm check,
paired performance against the thresholds in §7, and packaging/release artifacts. The Windows VM
uses a software renderer. Cross-device QMP command order is not a guest-delivery guarantee; the
ordered replay gates input on an observed click state rather than zero-delay host issuance.
