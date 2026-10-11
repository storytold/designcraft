# Localization parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** major (first version; catalogs measured) · **Target:** Adobe InDesign 2026 (21.6)

Per-language status of the DesignCraft interface and of document typesetting in each script.
Script-specific typesetting detail: [cjk.md](cjk.md), [cjk-typography.md](cjk-typography.md),
[arabic-typography.md](arabic-typography.md). Overall numbers: [target-app-parity.md](target-app-parity.md).

**Dimension score: ~45% ready (estimated), 120–180 h** plus native-speaker review (human).

## How it was measured

Interface strings live in `crates/ui-egui/src/i18n.rs` (a shared table with German, French,
Spanish, Japanese and Simplified Chinese columns) and `crates/ui-egui/src/i18n/{ar,it,ja,pt_br,uk}.rs`.
Untranslated strings fall back to English. A script (2026-10-10) built the set of English
interface strings as the union of every catalog's keys, every engine command label and every
literal passed to `tr`/`t` in the UI crate: **1,724 strings**. A further ~145 capitalized literals
in the UI crate appear in no catalog (some are test data or proper names), so the true total is
somewhat higher and every percentage below is a slight overestimate. Help: DesignCraft has no
help content in any language (Help ▸ opens the project's pages).

InDesign 2026 ships 25 interface localizations (`.lproj` folders: cs, da, de, el, en, en_GB, es,
fi, fr, hu, it, ja, ko, nb, nl, pl, pt, ro, ru, sq, sv, tr, uk, zh_CN, zh_TW); Arabic and Hebrew
come as Middle East editions with an English or French interface and right-to-left features.

## Key languages

| Language | Code | InDesign 2026 UI | UI strings translated | Dialogs / tooltips / help | Script support (documents and UI) | Native review | Status | To `full` (h) |
|---|---|---|---|---|---|---|---|---|
| English | en | ✓ | 1,724 / 1,724 (100%) | all / all / none | Latin ✓ | yes | full | 0 |
| Simplified Chinese (Mandarin) | zh | ✓ (zh_CN) | 1,438 (83%) | menus, panels, most dialogs; tooltips partly; no help | CJK typesetting ✓ (cjk.md); **no canvas IME** (#322); CJK UI font not embedded in release builds, so boxes on Linux (#332) | no | partial | 25–40 |
| Spanish | es | ✓ | 1,438 (83%) | menus, panels, most dialogs; no help | Latin ✓; Spanish hyphenation ✓ (#222) | no | partial | 5–8 |
| Hindi | hi | ✗ | 0 (0%) | none | Devanagari shaped by harfrust in documents but untested; no Hindi hyphenation or spelling | no | none | 20–30 |
| Arabic | ar | Middle East edition (English/French UI) | 1,459 (85%) | menus, panels, most dialogs; no help | RTL UI text (`rtl.rs`: bidi + shaping); Arabic composition, kashida, digits, RTL stories and tables ✓ (arabic-typography.md); list bullets and multi-column RTL open (#100) | no | partial | 15–25 |
| French | fr | ✓ | 1,438 (83%) | menus, panels, most dialogs; no help | Latin ✓; no French hyphenation | no | partial | 6–10 |
| Portuguese (Brazil) | pt-br | ✓ (pt) | 1,057 (61%) | menus, panels, some dialogs; no help | Latin ✓; no Portuguese hyphenation | no | partial | 8–12 |
| Indonesian | id | ✗ | 0 (0%) | none | Latin ✓ | no | none | 12–18 |
| Japanese | ja | ✓ | 1,644 (95%) | whole interface (#272), vertical-type commands; no help | vertical text, kinsoku, aki, ruby, warichu ✓ (partial depth, cjk.md); **no canvas IME** (#322); Japanese UI and Mincho fonts from craft-fonts | no | partial | 6–10 (+ IME, shared with zh) |
| German | de | ✓ | 1,438 (83%) | menus, panels, most dialogs; no help | Latin ✓; no German hyphenation (InDesign ships 2006 reform patterns) | no | partial | 6–10 |
| Korean | ko | ✓ | 0 (0%) | none | Korean line breaking at spaces ✓; **no IME** (#322) | no | none | 15–25 |
| Vietnamese | vi | ✗ | 0 (0%) | none | Latin with stacked diacritics via harfrust, untested | no | none | 12–18 |

Percentages are of the 1,724 measured strings. A language is `full` only when every user-visible
string is translated and its script renders and edits correctly; none but English is.

## Other shipped languages

| Language | Code | UI strings translated | Status |
|---|---|---|---|
| Italian | it | 1,464 (85%) | partial |
| Ukrainian | uk | 1,432 (83%), plural categories handled | partial |

DesignCraft ships **2** interface languages beyond the twelve; InDesign ships 17 beyond its
eight of the twelve (cs, da, el, en_GB, fi, hu, it, nb, nl, pl, ro, ru, sq, sv, tr, uk, zh_TW).

## Document languages

InDesign 2026 hyphenates and spell-checks ~40 languages (Hunspell and Duden dictionaries).
DesignCraft hyphenates **English and Spanish** and spell-checks **English** only. Text language
can be set per paragraph and character (Properties, Character panel) and drives quotes and
locale shaping. See [typography-parity.md](typography-parity.md).

## Remaining effort

| Work | Opus 5.5 h |
|---|---|
| Fill the 83–85% catalogs (de, fr, es, zh, ar, it, uk; ~290 strings each) | 25–40 |
| pt-BR to full | 8–12 |
| New: Hindi, Indonesian, Korean, Vietnamese (1,724 strings, plural rules, tests) | 50–75 |
| Canvas IME (zh, ja, ko) | 10–20 |
| CJK UI font in every release build; Devanagari and Vietnamese rendering tests | 10–15 |
| Catalog completeness test: every `tr` key and command label present in every catalog | 4–8 |
| Help content (at least English, then the twelve) | 15–25 |
| **Total** | **120–180** + native-speaker review for 11 languages (human) |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | major | Created: catalogs measured (1,724 strings), the twelve key languages plus Italian and Ukrainian |
