# DesignCraft roadmap

**Stage: pre-alpha** · next: alpha, ~0% and ~15–25 h away; beta ~28 points and ~420–750 h beyond that (the core-workflow gate, not the percentage: ready for real work is already ~47%)

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** minor (stage re-normalized to pre-alpha by the core-workflow gate; earlier today: major, full re-measure against InDesign 2026 21.6; restructured to the craftrules progress-docs standard) · **Target:** Adobe InDesign 2026

DesignCraft aims at full Adobe InDesign parity, and to be better: faster, open (documented JSON
format + IDML), scriptable by agents (MCP), and available on the web.

## Headline numbers

| | Value | Kind |
|---|---|---|
| **Feature breadth** | **~80%** (69% of InDesign 21.6's in-scope menu items matched; 99% of our 275-row checklist) | measured bounds, estimate between |
| **Ready for real work** | **~47%** (range 42–52%) | estimated: weighted sum over the dimensions below |
| Mainstream practitioner | **~42%** (weekly core 60.7% × 0.88 interaction × 0.92 stability × 0.85 file exchange) | estimated |
| Essentials user | **~54%** (core features 72% × 0.90 launch × 0.88 discoverability × 0.95 files received) | estimated |
| Remaining to **beta** | **420–750 Opus 5.5 agent-hours** (~120–200 wall-clock hours with 4–5 agents) | estimated |
| Remaining to **full parity** | **1,150–1,950 Opus 5.5 agent-hours** | estimated |

| Audience | Ready | Opus 5.5 agent-hours to ~95% | Work that dominates |
|---|---|---|---|
| Full target (ready for real work) | ~47% | 1,150–1,950 (~70% parallel) | depth across all areas, scripting compatibility, localization, typography fidelity, INDD/IDML |
| Mainstream practitioner | ~42% | 500–850 (~65% parallel) | INDD import and IDML corpus, composition fidelity, weekly-area depth, PDF/X-1a and printing |
| Essentials user | ~54% | 135–250 (~50% parallel) | start-up robustness, context menus and copy/paste, threading, fonts, Windows printing |

**Why pre-alpha:** it fails one of InDesign's core workflows, the alpha gate in
[docs/roadmap.md](docs/roadmap.md#alpha-gate). A user can't thread a story from one frame to
another: the in and out ports are drawn, but clicking the out port does nothing and there is no
thread or unthread command. Long text flows only through autoflow on Place and Primary Text
Frame, which users report adding and keeping pages wrongly (#165, #140, #194, #352). Everything
else in the gate works end to end on macOS (document setup, styles, images, object editing, PDF
export, save and reopen), and ready for real work (~47%) is above the alpha bar, so manual
threading is the whole distance to alpha. Beyond that, beta needs INDD opening, IDML proven on
real files, composition compared with InDesign's, and basic editing fixed on Windows and Linux.
Details: [docs/target-app-parity.md](docs/target-app-parity.md).

## By dimension

| Dimension | Ready | Hours | Doc |
|---|---|---|---|
| Features (depth) | 55% | 250–400 | [target-app-parity.md](docs/target-app-parity.md) |
| Typography and composition fidelity | 45% | 120–200 | [typography-parity.md](docs/typography-parity.md) |
| File formats | 40% | 150–250 + 20–60 (INDD) | [file-format-parity.md](docs/file-format-parity.md) |
| UI/UX fidelity | 45% | 120–180 | [ui-parity.md](docs/ui-parity.md) |
| Stability | 45% | 60–100 | [gaps.md](docs/gaps.md) |
| Performance | 65% | 40–80 | [gaps.md](docs/gaps.md) |
| Localization | 45% | 120–180 | [localization-parity.md](docs/localization-parity.md) |
| Platforms | 55% | 40–80 | [hardware-parity.md](docs/hardware-parity.md) |
| Hardware | 45% | 20–40 | [hardware-parity.md](docs/hardware-parity.md) |
| Ecosystem (scripting, plug-ins) | 15% | 150–250 | [gaps.md](docs/gaps.md) |
| AI features | 15% | 60–120 | [gaps.md](docs/gaps.md) |

All "Ready" figures are estimates; the weights and evidence are in target-app-parity.md.

## Features

Breadth from [docs/parity-checklist.md](docs/parity-checklist.md) (`cargo xtask parity`,
measured); Ready estimated.

| Area | Breadth | Ready | Hours |
|---|---|---|---|
| Application shell & workspace | 99% | 55% | 40–60 |
| Documents, pages, spreads | 100% | 65% | 20–35 |
| Layers | 100% | 75% | 5–10 |
| Frames, shapes & paths | 100% | 60% | 25–40 |
| Transform | 100% | 70% | 10–20 |
| Fill, stroke, colour | 100% | 60% | 20–35 |
| Effects & transparency | 100% | 50% | 20–30 |
| Placing & links | 93% | 55% | 20–35 |
| Type & text frames | 99% | 45% | 40–60 |
| Typography | 100% | 45% | see typography-parity.md |
| Styles | 100% | 60% | 15–25 |
| Tables | 100% | 55% | 25–40 |
| Long documents | 100% | 45% | 30–50 |
| Interactivity & digital | 100% | 40% | 30–50 |
| Output & production | 92% | 55% | 40–70 |
| XML & automation | 100% | 35% | see Ecosystem |
| View & navigation | 100% | 65% | 10–20 |
| Undo, history, saving | 100% | 70% | 5–10 |
| Accessibility | 100% | 50% | 15–25 |

## Languages

Interface strings translated, of 1,724 measured. Detail: [docs/localization-parity.md](docs/localization-parity.md).

| Language | Code | Status | Translated |
|---|---|---|---|
| English | en | full | 100% |
| Simplified Chinese | zh | partial (no canvas IME) | 83% |
| Spanish | es | partial | 83% |
| Hindi | hi | none | 0% |
| Arabic | ar | partial (RTL UI and composition) | 85% |
| French | fr | partial | 83% |
| Portuguese (Brazil) | pt-br | partial | 61% |
| Indonesian | id | none | 0% |
| Japanese | ja | partial (no canvas IME) | 95% |
| German | de | partial | 83% |
| Korean | ko | none | 0% |
| Vietnamese | vi | none | 0% |

Also shipped: Italian (85%), Ukrainian (83%).

## Upcoming

Ranked; detail and estimates in [docs/roadmap.md](docs/roadmap.md), the full list in
[docs/gaps.md](docs/gaps.md).

1. **Alpha gate:** manual text threading (out-port click, thread/unthread commands) and Primary Text Frame fixes (#165) (15–25 h)
2. Basic editing on every platform: copy/paste, context menus, caret formatting (30–50 h)
3. CI running `cargo xtask ci` on pull requests (4–8 h)
4. Open INDD through a clean-room converter, owner decision on #114 (20–60 h)
5. IDML real-file corpus and fixes (60–100 h)
6. Composition oracle against InDesign; optical kerning; overset rule (80–140 h)
7. Start on any GPU; find every installed font (25–50 h)

## Documents

[target-app-parity.md](docs/target-app-parity.md) (assessment) · [gaps.md](docs/gaps.md) (work
list) · [roadmap.md](docs/roadmap.md) (milestones, plan) · [parity-checklist.md](docs/parity-checklist.md)
(presence checklist) · [typography-parity.md](docs/typography-parity.md) ·
[file-format-parity.md](docs/file-format-parity.md) · [ui-parity.md](docs/ui-parity.md) ·
[hardware-parity.md](docs/hardware-parity.md) · [localization-parity.md](docs/localization-parity.md) ·
[architecture.md](docs/architecture.md) · agents: [agents.md](docs/agents.md), [mcp.md](docs/mcp.md),
[control-protocol.md](docs/control-protocol.md) · CJK: [cjk.md](docs/cjk.md),
[cjk-typography.md](docs/cjk-typography.md) · Arabic: [arabic-typography.md](docs/arabic-typography.md)

## Progress log

Newest first.

- **2026-10-10** · Version 0.6.0; releases add a Linux riscv64 tar.gz (cross-compiled, CLI
  smoke-tested under QEMU).
- **2026-10-10** · Readiness per audience: full ~47% (method aligned with the standard, was
  42%), mainstream practitioner ~42%, essentials user ~54%, each with hours to ~95%.
- **2026-10-10** · Stage re-normalized to **pre-alpha** under craftrules' core-workflow gate: text
  can't be threaded by hand (no out-port click, no thread command).
- **2026-10-10** · Full re-measure against InDesign 2026 21.6 (menu dump, bundle formats and
  localizations, 124 issues): breadth ~80%, ready ~42% (down from the ~83% "including depth" of
  2026-10-04, on user evidence and the INDD gap); progress docs restructured to the craftrules
  standard (`docs/parity.md` → `docs/parity-checklist.md`; new target-app-parity, gaps, roadmap,
  architecture, typography, file-format, UI, hardware and localization docs). Landed today:
  split columns (#295), PDF export options dialog (#196), column rules (#274), Paragraph Rules
  dialog (#263), Character Style Options (#144), PDF/X-4 RGB handling (#223), Spanish
  hyphenation (#222), Japanese (#272), Italian (#218) and Ukrainian (#236) interfaces, relocated packages relink (#244), graphics backend chosen
  before the window (#297), IDML table styles (#288), warichu (#280), list numbering from IDML
  (#298), security hardening (#153, #155), many text-composition fixes (#282–#293).
- **2026-10-09** · Version 0.5.0; many user reports triaged into issues (#160–#265).
- **2026-10-08** · Version 0.4.0.
- **2026-10-07** · Community PRs: data merge, Brazilian Portuguese, PDF/X-4 flags, Office fonts
  on macOS; CJK 1.x (Korean line breaking, fallback fonts in Package); PrintCraft renamed PdfCraft.
- **2026-10-06** · Snapping to parent items, Windows packaging, ARM64 workflow, control-channel
  hardening.
- **2026-10-05** · FreeBSD build; caret-boundary fixes.
- **2026-10-04** · Never-crash pass across importers and the engine; rotate spread view; kashida
  in styles; previous estimate: breadth 99%, "~83% including depth", ~170 h remaining.
- **2026-10-03** · Release 0.1.1; separations, XML tagging, track changes, PDF/X-4 output
  intent, split window, buttons, menu customization.
- **2026-10-02** · Hyperlinks/Bookmarks panels, conditional text, tagged PDF, lists across
  stories, style groups, Glyphs panel, High Contrast theme.
- **2026-10-01** · First commit; skeleton, IDML, PDF, tables, footnotes, cross-references, index,
  spelling and parallel composition on the first day.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | minor | Full number 42% → 47% (method aligned with the standard: plain weighted sum, no new evidence); readiness table with hours per audience |
| 2026-10-10 | minor | Headline adds mainstream practitioner (~42%) and essentials user (~54%); method in docs/target-app-parity.md |
| 2026-10-10 | minor | Stage alpha → pre-alpha under the new core-workflow gate: manual text threading is missing; alpha gate table added to docs/roadmap.md |
| 2026-10-10 | major | Restructured to the progress-docs standard; full re-measure; the "Working today" inventory moved to docs/target-app-parity.md, milestones to docs/roadmap.md, the CLI/MCP section left to docs/agents.md (which already covered it) |
| 2026-10-04 | major | Parity estimate: breadth 99%, depth ~83%, ~170 h |
| 2026-10-01 | major | First roadmap: status, milestones |
