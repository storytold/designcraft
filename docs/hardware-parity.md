# Hardware and platform parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** major (first version) · **Target:** Adobe InDesign 2026 (21.6)

What InDesign does with hardware, per platform, against DesignCraft. Layout is not a
GPU-hungry workload, so this dimension carries little weight, but a start that fails on a
graphics driver loses the user entirely. Overall numbers: [target-app-parity.md](target-app-parity.md).

**Hardware: ~45% ready, 20–40 h. Platforms: ~55% ready, 40–80 h.** (estimated)

## Hardware features

| Feature | InDesign 2026 | DesignCraft macOS | Windows | Linux | Web |
|---|---|---|---|---|---|
| GPU display (canvas) | GPU Performance: GPU preview, animated zoom (Metal on macOS; Windows with supported GPUs) | canvas rasterized on the CPU (vello_cpu, SIMD, multithreaded), presented by wgpu (Metal) | wgpu DirectX 12 (since #297), Vulkan/OpenGL fallback | Vulkan, OpenGL fallback | WebGPU, WebGL2 fallback |
| Start-up on any GPU | robust | Metal device lost on Intel Macs (#334) | AMD OpenGL crash (#167), launch failures (#273) | some reports (#192, #220) | |
| Backend fallback | n/a | `gpu.json` skips a backend whose start never showed a frame (#297) | same | same | |
| Animated / scrubby zoom | ✓ (GPU) | ✓ (CPU tiles, damage regions) | ✓ | ✓ | ✓ |
| HiDPI / Retina | ✓ | ✓ | ✓ | ✓ | ✓ |
| UI scaling | ✓ | ✓ (menus and dialogs usable at larger scales, #281) | ✓ | ✓ | ✓ |
| Multi-monitor, floating document windows | ✓ | ~ one window; New Window for the same document | ~ | ~ | ✗ |
| Printing | system print with PPDs, PostScript, separations | PDF to `lpr` | ✗ (no `lpr`) | PDF to `lpr` (CUPS) | browser download |
| Colour-managed display (monitor profile) | ✓ | ✗ (working CMYK profile simulated; monitor profile not applied) | ✗ | ✗ | ✗ |
| Touch / pen | Touch workspace (Windows), gestures | trackpad pinch/scroll | ~ | ~ | ~ |
| Multi-core composition | single-threaded composer per story (documented by Adobe as mostly single-threaded) | parallel story composition (rayon), 6.7× on the sample | ✓ | ✓ | single-threaded |

## Platforms

| Platform | InDesign 2026 | DesignCraft | Evidence |
|---|---|---|---|
| macOS (Apple silicon, Intel) | ✓ | ✓ primary development platform | Intel GPU start failure (#334); window controls misaligned (#333); font managers (#327) |
| Windows x64 | ✓ | ~ builds and installers ship | copy/paste and context menus (#351, #312), installer confusion (#64), launch failures (#273, #167) |
| Windows ARM64 | ✓ | ~ build (CI workflow) | not exercised at runtime |
| Linux (x86_64, aarch64; riscv64 tar.gz) | ✗ | ~ deb, AppImage, Flatpak | clipboard (#164), panels don't scroll (#267), CJK UI font (#332) |
| FreeBSD | ✗ | ~ build | |
| Web (WASM) | ✗ (InDesign on the web is not shipped) | ~ same UI, file picker, downloads; no control channel | |

DesignCraft runs on more platforms than InDesign; what's missing is runtime testing on the ones
the developers don't use daily.

## Remaining effort

| Work | Opus 5.5 h |
|---|---|
| Start-up robustness on every GPU: confirm #297 with reporters, Intel Metal fix, CPU-only present fallback | 15–30 |
| Printing on Windows (system print API) and print presets | 15–30 (counted in file-format-parity.md) |
| Monitor profile in display colour management | 5–10 |
| Windows and Linux runtime smoke tests in CI | 20–40 |
| Multi-window document views | 15–30 |
| **Hardware + platforms** | **60–120** |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | major | Created from the code, InDesign 2026's GPU Performance features and the issue tracker |
