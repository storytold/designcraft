<p align="center">
  <a href="https://getartcraft.com/">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="docs/brand/artcraft-logo-white.svg">
      <img alt="ArtCraft" src="docs/brand/artcraft-logo.svg" width="200">
    </picture>
  </a>
</p>


<h1 align="center">DesignCraft</h1>

<p align="center">
  <b>Page layout and publishing; an open-source, clean-room reimplementation of Adobe InDesign, rebuilt in pure Rust.</b>
</p>

<p align="center">
  A fast, open-source, clean-room take on the Adobe InDesign workflow. It runs natively on macOS,
  Windows and Linux, and in the browser via WebAssembly.<br>
  <i>By the ArtCraft team.</i>
</p>

<p align="center">
  <img alt="Written in Rust" src="https://img.shields.io/badge/written%20in-Rust-4d7a0a?style=flat-square&logo=rust&logoColor=white">
  <img alt="Runs on macOS, Windows, Linux and the web" src="https://img.shields.io/badge/runs%20on-macOS%20%C2%B7%20Windows%20%C2%B7%20Linux%20%C2%B7%20Web-7bb51c?style=flat-square">
  <img alt="License: MIT OR Apache-2.0" src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-4d7a0a?style=flat-square">
  <img alt="Agent-drivable over MCP" src="https://img.shields.io/badge/agents-MCP-7bb51c?style=flat-square">
</p>

<p align="center">
  <a href="https://discord.gg/artcraft"><img alt="Join the ArtCraft community on Discord" src="https://img.shields.io/badge/Join%20us%20on%20Discord-5865F2?style=for-the-badge&logo=discord&logoColor=white" height="40"></a>
</p>

<p align="center">
  <a href="https://getartcraft.com/apps/designcraft"><b>DesignCraft on getartcraft.com</b></a> ·
  <a href="https://getartcraft.com/">ArtCraft</a> ·
  <a href="https://getartcraft.com/apps">All Crafting Apps</a>
</p>

<br>

<p align="center">
  <img src="docs/images/ui-spread.png" alt="DesignCraft showing a magazine spread: a threaded three-column story is selected with its in/out ports and thread line, the Control panel shows its position in picas and the Properties panel its text frame options" width="100%">
  <br><sub><b>Quarterly, Spring Issue</b>: threaded three-column body text, a wrapped pull quote and parent-page folios, all set by DesignCraft's own paragraph composer.</sub>
</p>

> [!NOTE]
> **ArtCraft is a community of artists from all walks of life.** Digital, generative, music,
> games &mdash; if you make things, you're one of us. **[Come say hi on Discord](https://discord.gg/artcraft).**

<p align="center">
  <a href="#the-sample-magazine">The sample magazine</a> ·
  <a href="#why-designcraft">Why DesignCraft</a> ·
  <a href="#quick-start">Quick start</a> ·
  <a href="#web">Web</a> ·
  <a href="#architecture">Architecture</a> ·
  <a href="#downloads">Downloads</a> ·
  <a href="#the-crafting-apps">The Crafting Apps</a> ·
  <a href="#license-and-credits">License and credits</a>
</p>

## The sample magazine

Every page below was laid out by DesignCraft from code (`crates/engine/src/sample.rs`) and exported
by its own renderer. Try it yourself with `File → New → Sample Document`, or start the app with
`--sample`.

<table>
<tr>
<td width="50%" valign="top"><img src="docs/images/page-cover.png" alt="Magazine cover for The Spring Issue, No. 01: a dusk landscape of layered purple hills under a large pale sun, with the white serif headline The Quiet Art of Layout and an italic deck below" width="100%"><p align="center"><sub><b>Cover.</b> A full-bleed graphic frame, display type and an italic deck.</sub></p></td>
<td width="50%" valign="top"><img src="docs/images/page-2.png" alt="Feature opener titled Notes on the Grid, with an orange FEATURE · DESIGN kicker over a rule, an italic deck, a wide landscape picture with a small caption, and two columns of justified body text above a QUARTERLY folio" width="100%"><p align="center"><sub><b>Styles.</b> Kicker rule, headline, deck, caption and justified two-column body.</sub></p></td>
</tr>
<tr>
<td width="50%" valign="top"><img src="docs/images/page-3.png" alt="Three-column page of justified, hyphenated body text that wraps around a shaded pull quote reading The best layouts disappear. What remains is the story., with a gradient rule and SPRING ISSUE folio at the foot" width="100%"><p align="center"><sub><b>Threading, columns and text wrap.</b> One story flows through three columns and around the pull quote.</sub></p></td>
<td width="50%" valign="top"><img src="docs/images/page-4.png" alt="Coming Next page titled Color, Ink and Paper on a plum background, with four swatch circles labeled Ink Plum, Sunset, C=100 M=0 Y=0 K=0 and Paper Warm, above a short paragraph of body text" width="100%"><p align="center"><sub><b>Swatches.</b> Named colors and a CMYK process swatch, laid out as a palette.</sub></p></td>
</tr>
</table>

## Why DesignCraft

- **Familiar.** InDesign's layout, tools, menus, panels and shortcuts: spreads and parent pages,
  frames and threaded stories, the Control panel, paragraph and character styles, swatches, text
  wrap and more. If you know InDesign, you already know how to use it.
- **Beautiful type.** A Knuth–Plass paragraph composer (plus single-line), dictionary hyphenation
  (the public-domain Moby word list plus our own trained patterns), word, letter and glyph-scaling
  justification, keeps, optical margin alignment, columns, baseline grid, tabs, rules and shading.
  Line breaks are identical on screen and in PDF.
- **Fast.** Multithreaded SIMD rendering (vello_cpu), copy-on-write documents with O(1) undo
  snapshots, and cached composition.
- **Open.** A documented native format, IDML import and export, and PNG, PDF (PDF/X-4, PDF/A-2b)
  and EPUB export.
  No subscription, no licence server, no telemetry.
- **Agent-native.** Every menu item, tool gesture, panel control and dialog can be driven over a
  JSON control channel and an **MCP server**, so Claude and other agents can lay out and edit
  documents like a designer. See [`docs/control-protocol.md`](docs/control-protocol.md) and
  [`docs/mcp.md`](docs/mcp.md).
- **Everywhere.** One Rust codebase for desktop and the web.

## Quick start

```sh
cargo run --release -p designcraft                         # desktop app (start screen)
cargo run --release -p designcraft -- --sample             # open the sample magazine
cargo run --release -p designcraft -- --sample --control 7979   # + JSON control channel
cargo run --release -p designcraft-cli -- run --sample --all-pages out/       # headless: render every page to PNG
cargo run --release -p designcraft-cli -- commands         # list every command
cargo xtask ci                                             # fmt, clippy, tests, assets, layering, wasm
```

Japanese text (UI and documents) uses fonts from
[storytold/craft-fonts](https://github.com/storytold/craft-fonts), an optional build input (font
files are never committed here; see craftrules
[`standards/fonts.md`](https://github.com/storytold/craftrules/blob/main/standards/fonts.md)):

```sh
git clone https://github.com/storytold/craft-fonts ../craft-fonts
CRAFT_FONTS_DIR="$PWD/../craft-fonts" cargo run --release -p designcraft   # absolute path
```

Without it, Japanese falls back to the system's fonts (none on the web). Release builds always
include it.

To drive a running app, send JSON lines to `127.0.0.1:7979`. The protocol is described in
[`docs/control-protocol.md`](docs/control-protocol.md).

### Logs

The desktop app writes its `log` records to standard error and to `logs/designcraft.log` in the
settings directory, beside `ui.json` and `prefs.json` (Linux `~/.config/designcraft/logs/`, or
`$XDG_CONFIG_HOME/designcraft/logs/`; macOS `~/Library/Application Support/DesignCraft/logs/`;
Windows `%APPDATA%\DesignCraft\logs\`). A start launched from a desktop menu or the Dock has no
terminal, so this file is what to attach to a bug report: the crash guard's panic report and a
failed crash-recovery save land there. Each launch moves the previous log to `designcraft.1.log`
(and that one to `designcraft.2.log`), so the log of a run that crashed survives the next start.
The file stops growing at 16 MiB. `--version` writes no file.

| Variable | Effect |
|---|---|
| `RUST_LOG` | Log levels for standard error and the log file. Default: `info` for DesignCraft's own crates, `warn` for everything else. env_logger-style directives replace that, e.g. `RUST_LOG=debug`, `RUST_LOG=warn,designcraft_render=trace` or `RUST_LOG=info,wgpu_core=warn`; a directive ending in `*` covers every target starting with it (`designcraft*=debug`). |
| `WGPU_BACKEND` | The graphics backend that composites the window: `dx12`, `vulkan`, `gl` or `metal` (a comma-separated list lets wgpu choose among them). Default: DirectX 12 on Windows, Vulkan on Linux, Metal on macOS, with OpenGL as the fallback. Setting it also turns off the start-up fallback described below. |

The logger is `apps/designcraft/src/logging.rs`; the web build logs to the browser console instead.

### Graphics backend

The canvas is rendered on the CPU; the GPU only composites the interface, through wgpu. A
graphics driver that faults takes the process down before any Rust code can catch it, so the
backend is chosen before the window exists, and a start that never showed a frame is remembered:
`gpu.json` in the settings directory records the backend being tried until the first frame has
been presented. A record left behind means that backend crashed (or the app was killed) at
start-up, so the next start tries the next one (DirectX 12, Vulkan, then OpenGL on Windows;
Vulkan, then OpenGL on Linux) and says so in the status bar. The list starts over once every
backend has failed, or with a new DesignCraft version; delete the file or set `WGPU_BACKEND` to
start afresh. Seen in the wild: on an AMD hybrid-graphics laptop (a Radeon RX 6800M beside an
integrated Radeon) the AMD Vulkan driver faulted at the first present, which is why DirectX 12 is
the Windows default. The code is `apps/designcraft/src/gpu.rs`.

### Web

```sh
cd apps/designcraft-web && trunk build --release          # → dist/web (serve it with any static server)
cd apps/designcraft-web && trunk serve --release          # http://127.0.0.1:8767
```

You need [trunk](https://trunkrs.dev) and the `wasm32-unknown-unknown` target. The same app runs
through eframe's web runner on WebGPU, falling back to WebGL2 (`?webgl` forces it; `?sample` opens
the sample magazine). Open and Place use the browser's file picker, and dropping files works too.
Save and Export download the file. The web build has no control channel.

## Architecture

DesignCraft is an engine-first Cargo workspace with enforced layering (`cargo xtask layers`). The
egui frontend is a separate crate, so the UI can be swapped without touching the engine.

| Layer | Crates |
|---|---|
| L0 | `geom` (paths, units & measurement parsing, corner options) · `color` (CMYK/RGB/Lab, swatches, tints, gradients) |
| L1 | `doc` (spreads, pages, parents, layers, frames, stories, styles) · `fonts` (font DB, shaping, outlines) |
| L2 | `compose` (the text engine) |
| L3 | `render` (vello_cpu) |
| L4 | `tools` (pointer events → commands + overlays) |
| L5 | `engine` (session, history, command registry) |
| L6 | `ui-egui` (InDesign-style UI, control channel) |
| L7 | `apps/designcraft`, `apps/designcraft-cli`, `apps/designcraft-web` |

- **Status and milestones:** [ROADMAP.md](ROADMAP.md) (stage: pre-alpha); how close to InDesign: [`docs/target-app-parity.md`](docs/target-app-parity.md); known gaps: [`docs/gaps.md`](docs/gaps.md)
- **Contributor and agent rules** (clean-room, asset policy, quality gates): [`AGENTS.md`](AGENTS.md)
- **Bundled assets:** every one is listed with its licence in [`ASSETS.md`](ASSETS.md)
- **App icon and colour:** a calico cat in a polka-dot scarf on DesignCraft green `#7bb51c`; see [`assets/app-icon/`](assets/app-icon/README.md)

## Downloads

**New to DesignCraft?** Download it from the [DesignCraft page on getartcraft.com](https://getartcraft.com/apps/designcraft). That's the easiest way to install it.

**Want a specific build or format?** On GitHub, the [latest release](https://github.com/storytold/designcraft/releases/latest) has every build listed below, and [all releases](https://github.com/storytold/designcraft/releases) has earlier versions and their notes. `<ver>` in the file names is the version number, and `SHA256SUMS.txt` lists a checksum for every file.

### Windows

| Build | Installer | Portable |
|---|---|---|
| x64 (64-bit Intel/AMD) | `designcraft-<ver>-windows-x64.msi` | `designcraft-<ver>-windows-x64-portable.zip` |
| arm64 (Snapdragon and other ARM PCs) | `designcraft-<ver>-windows-arm64.msi` | `designcraft-<ver>-windows-arm64-portable.zip` |
| x86 (32-bit) | `designcraft-<ver>-windows-x86.msi` | `designcraft-<ver>-windows-x86-portable.zip` |

Installers and executables are code-signed.

### macOS

| Build | File | Notes |
|---|---|---|
| App, universal (Apple silicon + Intel) | `designcraft-<ver>-macos-universal.dmg` | Signed and notarized |
| Command-line tool, universal | `designcraft-cli-<ver>-macos-universal.zip` | Signed and notarized |

### Linux

| Format | x86_64 | aarch64 (ARM64) | Notes |
|---|---|---|---|
| AppImage | `designcraft-<ver>-linux-x86_64.AppImage` | `designcraft-<ver>-linux-aarch64.AppImage` | Runs anywhere; updates itself with [AppImageUpdate](https://github.com/AppImageCommunity/AppImageUpdate) (`.zsync` files) |
| Flatpak | `designcraft-<ver>-linux-x86_64.flatpak` | `designcraft-<ver>-linux-aarch64.flatpak` | Sandboxed; `flatpak install --user <file>` |
| Debian/Ubuntu | `designcraft-<ver>-linux-x86_64.deb` | `designcraft-<ver>-linux-aarch64.deb` | |
| Fedora/RHEL/openSUSE | `designcraft-<ver>-linux-x86_64.rpm` | `designcraft-<ver>-linux-aarch64.rpm` | |
| Tarball | `designcraft-<ver>-linux-x86_64.tar.gz` | `designcraft-<ver>-linux-aarch64.tar.gz` | Unpack anywhere |

RISC-V (riscv64): `designcraft-<ver>-linux-riscv64.tar.gz` only, cross-compiled; needs glibc 2.39+ (Ubuntu 24.04 or newer).

### FreeBSD

| Build | File |
|---|---|
| x86_64 | `designcraft-<ver>-freebsd-x86_64.tar.gz` |

### Web (WebAssembly)

| Build | File | Notes |
|---|---|---|
| Static site | `designcraft-web-<ver>.zip` | Runs in a modern browser; host it on any static server |

## The Crafting Apps

DesignCraft is one of the **Crafting Apps**: free, open-source creative tools from the
[ArtCraft](https://getartcraft.com/) team, each written from scratch in Rust and each able to
stand on its own.

| | App | What it's for | Code | Learn more |
|:-:|---|---|---|---|
| <img src="https://raw.githubusercontent.com/storytold/photocraft/main/assets/app-icon/hicolor/64x64/apps/ai.storyteller.photocraft.png" alt="" width="32" height="32"> | **PhotoCraft** | Image editing: layers, masks, type and real PSD files | [GitHub](https://github.com/storytold/photocraft) | [Website](https://getartcraft.com/apps/photocraft) |
| <img src="https://raw.githubusercontent.com/storytold/vectorcraft/main/assets/app-icon/hicolor/64x64/apps/ai.storyteller.vectorcraft.png" alt="" width="32" height="32"> | **VectorCraft** | Vector illustration | [GitHub](https://github.com/storytold/vectorcraft) | [Website](https://getartcraft.com/apps/vectorcraft) |
| <img src="https://raw.githubusercontent.com/storytold/filmcraft/main/assets/app-icon/hicolor/64x64/apps/ai.storyteller.filmcraft.png" alt="" width="32" height="32"> | **FilmCraft** | Video editing, color and sound | [GitHub](https://github.com/storytold/filmcraft) | [Website](https://getartcraft.com/apps/filmcraft) |
| <img src="https://raw.githubusercontent.com/storytold/lightcraft/main/assets/app-icon/hicolor/64x64/apps/ai.storyteller.lightcraft.png" alt="" width="32" height="32"> | **LightCraft** | Photo library and raw development | [GitHub](https://github.com/storytold/lightcraft) | [Website](https://getartcraft.com/apps/lightcraft) |
| <img src="https://raw.githubusercontent.com/storytold/pdfcraft/main/assets/app-icon/hicolor/64x64/apps/ai.storyteller.pdfcraft.png" alt="" width="32" height="32"> | **PdfCraft** | Reading, organizing and protecting PDFs | [GitHub](https://github.com/storytold/pdfcraft) | [Website](https://getartcraft.com/apps/pdfcraft) |
| <img src="https://raw.githubusercontent.com/storytold/effectcraft/main/assets/app-icon/hicolor/64x64/apps/ai.storyteller.effectcraft.png" alt="" width="32" height="32"> | **EffectCraft** | Motion graphics and visual effects | [GitHub](https://github.com/storytold/effectcraft) | [Website](https://getartcraft.com/apps/effectcraft) |
| <img src="https://raw.githubusercontent.com/storytold/designcraft/main/assets/app-icon/hicolor/64x64/apps/ai.storyteller.designcraft.png" alt="" width="32" height="32"> | **DesignCraft** | **Page layout and publishing · you are here** | [GitHub](https://github.com/storytold/designcraft) | [Website](https://getartcraft.com/apps/designcraft) |

And [**ArtCraft**](https://getartcraft.com/) itself, our AI image and video studio for artists who want real control.

<br>

<p align="center">
  <a href="https://discord.gg/artcraft"><img alt="Join the ArtCraft community on Discord" src="https://img.shields.io/badge/Join%20us%20on%20Discord-5865F2?style=for-the-badge&logo=discord&logoColor=white" height="40"></a>
</p>

<h3 align="center">Come make things with us</h3>

<p align="center">
  Our Discord is where artists of every kind hang out: people who paint, shoot, draw, cut film,
  set type, and people still figuring out what they like to make. Share what you're working on,
  ask for help, tell us what's broken, or tell us what you wish these tools could do.
  Whatever your medium and however long you've been at it, you're welcome here.
</p>

<p align="center">
  <a href="https://discord.gg/artcraft"><b>discord.gg/artcraft</b></a> ·
  <a href="https://getartcraft.com/">getartcraft.com</a> ·
  <a href="https://getartcraft.com/apps">The Crafting Apps</a> ·
  <a href="https://getartcraft.com/apps/designcraft">DesignCraft</a>
</p>

## License and credits

DesignCraft is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
Copyright (c) 2026 ArtCraft Team and the DesignCraft contributors. Required notices are in [NOTICE](NOTICE).

Bundled fonts, icons, images and other assets keep their own open licenses; each one is listed
with its author, source and license in [ASSETS.md](ASSETS.md).

The bundled fonts, and the craft-fonts fonts embedded by release builds, are under the SIL Open
Font License; all UI icons are drawn in code and are original.

The ArtCraft name, wordmark and logos in [`docs/brand/`](docs/brand/) are trademarks of the
ArtCraft Team and are not covered by this license. They may be used only unmodified, and only as
part of this repository and DesignCraft, under [`docs/brand/LICENSE-brand.txt`](docs/brand/LICENSE-brand.txt).
Forks and modified versions must remove them.

<sub>Adobe, Photoshop, Illustrator, Premiere Pro, Lightroom, Acrobat, After Effects and InDesign are trademarks or registered trademarks of Adobe Inc. in the United States and/or other countries. DesignCraft is an independent, open-source project and is not affiliated with, sponsored by or endorsed by Adobe Inc.; these names are used only to describe the workflows it is compatible with.</sub>

<br>

<p align="center">
  <a href="https://getartcraft.com/"><img alt="ArtCraft" src="docs/brand/artcraft-mark.svg" width="28"></a><br>
  <sub>Made by the <a href="https://getartcraft.com/">ArtCraft</a> team and community.</sub>
</p>
