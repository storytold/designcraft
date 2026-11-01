# DesignCraft roadmap (detail)

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-13 · **Change:** minor (.indd and .indt open through indd-utils; text threads by hand; the gate waits on the Primary Text Frame page fixes) · **Target:** Adobe InDesign 2026 (21.6)

Forward-looking plan. The one-page summary is [ROADMAP.md](../ROADMAP.md); numbers are in
[target-app-parity.md](target-app-parity.md); the work list is [gaps.md](gaps.md). Hours are
Opus 5.5 agent-hours (calibration in target-app-parity.md). Driving the app from agents:
[agents.md](agents.md).

## Alpha gate

InDesign's core workflows: what a layout professional does with it every day. DesignCraft is
alpha only when every one works end to end on macOS (its main platform) and the work saves and
reopens. **Gate result (2026-10-10): fails, so the stage is pre-alpha.** Rows marked "no" are the
alpha checklist; the partial rows don't block the workflow on macOS (there is a working path)
but are next in line.

| Core workflow | Works end to end? | Evidence | Hours to pass |
|---|---|---|---|
| Set up a document (size, margins, columns, parent pages, page numbers, sections); save and reopen | yes | New Document, Document Setup, parent pages and overrides, Numbering & Section Options; `.designcraft` round-trip tests; recovery | 0 |
| Flow a long story across frames and pages: thread, unthread, autoflow | **no** | threading by hand works (ports, loaded text cursor, `text.thread`/`text.unthread`/`text.removeFromThread`); Primary Text Frame autoflow adds pages users can't delete (#165) | 5–7 |
| Format text with paragraph and character styles | partial (not blocking) | Paragraph/Character Style Options, Quick Apply, based-on, nested/GREP styles work; style edit/delete from the panel reported (#169); formatting set at a caret is discarded (#289) | 6–12 |
| Place, crop and fit images; wrap text around them | yes | place gun, frame fitting and Fitting Options, content grabber, text wrap, Links panel | 0 |
| Arrange and edit objects: select, move, transform, duplicate, copy/paste, align, layers | partial (not blocking on macOS) | all present and working on macOS; Paste in Place lands on page 1 (#348); right-click menus fail for some macOS users (#160, #238); copy/paste and context menus fail on Windows/Linux (#351, #185, #164) | 10–20 |
| Export a print-ready PDF (bleed, marks, CMYK and spot, PDF/X-4) | yes | krilla export with subset fonts, Separations, marks and bleed, PDF/X-4 with validator; 53 PDF tests | 0 |

Exchanging files with InDesign users (open INDD, round-trip IDML) is a beta requirement, not part
of the alpha gate: IDML works both ways today, INDD opens through indd-utils, and measuring both
on real files is gaps 2 and 3.

**Distance to alpha:** ~5–7 h (the threading row: Primary Text Frame page fixes). Ready for real work (~47%) is already above
the ~40% alpha bar.

## Current focus

In order. Each closes a beta blocker from [gaps.md](gaps.md).

0. **Alpha gate: Primary Text Frame page fixes** (gap 0): autoflow adds pages users can't
   delete (#165). 5–7 h.
1. **Basic editing works on every platform** (gap 1): object copy/paste, Paste in Place, context menus, caret formatting, reproduced and pinned with control-channel tests on
   macOS, Windows and Linux. 30–50 h.
2. **CI on pull requests** (gap 7): `cargo xtask ci` on every PR, at least macOS and Linux;
   Windows smoke start. 4–8 h. Needs the owner to make it a required check.
3. **INDD real-file corpus** (gap 2): `.indd` and `.indt` open through storytold/indd-utils
   (INDD → IDML in memory → existing importer, warnings in the open result); a pinned corpus and
   a metric show what survives. 10–30 h.
4. **IDML real-file corpus** (gap 3): a pinned corpus, a survival metric, and fixes for #193,
   #338, #339, #340. 60–100 h.
5. **Composition oracle** (gap 4): synthetic stories set in InDesign 2026 vs DesignCraft;
   optical kerning; the overset rule. 80–140 h.
6. **Start on any GPU; find every installed font** (gaps 5, 6). 25–50 h.

## Milestones

States re-assessed 2026-10-10 against the code and the issue tracker. "Done" means the feature
set exists; depth gaps are in gaps.md.

| # | Milestone | State |
|---|---|---|
| M0 | Skeleton + vertical slice | ✅ |
| M1 | Selection, transform, layers, pages, MCP | ✅ core; handle and snapping issues (#188, #337) |
| M2 | Type I (Type tool, threading, Character/Paragraph, composer) | partial: Type tool, composer and threading ✅; Primary Text Frame pages (#165, alpha blocker); caret formatting (#289); IME not checked by hand (#322) |
| M3 | Styles (nested/GREP, bullets, keeps, span columns) | ✅; style editing reliability (#169) |
| M4 | Color & effects | ✅ core; colour management depth |
| M5 | Graphics & links | ✅ core; EPS preview only |
| M6 | Files & export (native, IDML, PDF, PNG/JPEG) | IDML ✅, PDF ✅ (X-4, A-2b, tagged); INDD read ✅ (via indd-utils); PDF/X-1a ✅; PDF/X-3 and presets ✗ |
| M7 | Long documents (sections, TOC, index, footnotes, books) | ✅ except Books (shallow) |
| M8 | Tables | ✅ core (table/cell styles, rotation); diagonal lines, anchored objects in cells (#163) |
| M9 | Performance (MT composition, tiles) | in progress: parallel compose 6.7×, damage regions; incremental composition next |
| M10 | Find/Change, spelling, Preflight | ✅ (spelling English only) |
| M11 | Layout power features (liquid/alternate layouts, data merge) | present, shallow; Flex Layout (new in InDesign 2026) ✗ |
| M12 | Interactive & digital (EPUB, HTML, interactive PDF) | present; HTML basic (#314), fixed-layout EPUB as images |
| M13 | Automation (scripts, batch) | ✅ own command scripts, CLI, MCP; ExtendScript/UXP compatibility ✗ |
| M14 | 1.0 polish & packaging | packaging ✅ (macOS, Windows x64/ARM64, Linux deb/AppImage/Flatpak, FreeBSD, web); polish not started |
| **A1** | **Alpha** (core-workflow gate) | blocked by the Primary Text Frame page fixes (#165); see Alpha gate |
| **B1** | **Beta** (ready ≥ 75%, INDD/IDML reliable) | not started as a milestone; see below |

## To beta

Beta needs ~75% ready for real work and no blocking gap in the main file format. From ~47% today
that is ~28 points and **420–750 h** (~120–200 wall-clock hours with four to five agents):

| Work | Gap | Hours |
|---|---|---|
| Basic editing on every platform | 1 | 30–50 |
| INDD real-file corpus | 2 | 10–30 |
| IDML real-file corpus and fixes | 3 | 60–100 |
| Composition oracle and fixes | 4, 13 | 80–140 |
| Start-up robustness, fonts | 5, 6 | 25–50 |
| CI, Windows/Linux runtime tests | 7, 14 | 35–70 |
| IME | 8 | 10–20 |
| PDF/X-1a/X-3, presets, physical print validation | 9, 10 | 65–110 |
| Styles, tables, missing menu items (the most-used half) | 11, 12, 16 | 60–100 |
| Long documents (Books) | 15 | 30–50 |
| **Total** | | **420–750** |

## After beta: to full parity

Remaining gaps 16–25: the rest of the menu long tail, HTML/EPUB depth, colour management, placed
graphics depth (PostScript), performance on large documents, localization to `full` in the
twelve languages, InDesign scripting compatibility (owner decision), AI features (owner decision),
accessibility. **730–1,200 h** more; **1,150–1,950 h** in total from today.

## Superseded plan

The order the project followed until 2026-10-04 ("Next: books · New Window / split view · Ink
Manager and Separations Preview · XML structure, tags and export · buttons and forms · PDF/X-4
output intent · EPS place") is done except Books depth and EPS (preview only).

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-13 | minor | INDD and INDT open through storytold/indd-utils; focus item 3 shrinks to a real-file corpus (10–30 h) |
| 2026-10-12 | minor | Threading row: threading by hand works; the gate waits on the Primary Text Frame page fixes (#165), 5–7 h |
| 2026-10-10 | trivial | Ready figure 42% → 47%; beta distance 28 points |
| 2026-10-10 | minor | Alpha gate table added (6 core workflows); gate fails on manual threading, stage pre-alpha; A1 milestone and focus item 0 |
| 2026-10-10 | major | Created: milestones moved from ROADMAP.md and re-assessed; Current focus and the beta plan from the full re-measure |


### Windows installer unification — release preparation, 2026-10-12

The shared craft-installer UI preserves this revision's install options and provides persistent installation, repair and Windows Settings removal completion screens. Silent modes stay unattended. Earlier Windows acceptance is recorded in packaging/windows/README.md; normal CI and production-profile upgrade qualification against pinned current releases are in progress. No release or PR has been published.
