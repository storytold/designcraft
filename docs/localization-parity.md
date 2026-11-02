# Localization parity

> **Last reviewed:** 2026-10-11 · **Last updated:** 2026-11-01 · **Change:** minor (Hungarian interface catalog, bundled-font coverage and flyout regression checks; Swedish interface) · **Target:** Adobe InDesign 2026 (21.6)

Per-language status of the DesignCraft interface and of document typesetting in each script.
Script-specific typesetting detail: [cjk.md](cjk.md), [cjk-typography.md](cjk-typography.md),
[arabic-typography.md](arabic-typography.md). Overall numbers: [target-app-parity.md](target-app-parity.md).

**Dimension score: ~45% ready (estimated), 120–180 h** plus native-speaker review (human).

## How it was measured

Interface strings live in `crates/ui-egui/src/i18n/table.rs` (a shared table with German, French,
Spanish, Japanese and Simplified Chinese columns) and `crates/ui-egui/src/i18n/{ar,cs,de,fr,hu,it,ja,ka,pl,pt_br,ru,sv,uk}.rs`.
Untranslated strings fall back to English. A script (2026-10-10) built the set of English
interface strings as the union of every catalog's keys, every engine command label and every
literal passed to `tr`/`t` in the UI crate: **1,724 strings**. A further ~145 capitalized literals
in the UI crate appear in no catalog (some are test data or proper names), so the true total is
somewhat higher and every percentage below is a slight overestimate. Help: DesignCraft has no
help content in any language (Help ▸ opens the project's pages).

Missing translations normally fall back to English and are reported without blocking a build.
Targeted regression tests can require specific captions, such as the Data Merge arrangement
label, in every registered interface language. `cargo xtask i18n-report` (also a step of
`cargo xtask ci`, so the list is in every gate log) prints how many interface strings each
language lacks, then the strings.

InDesign 2026 ships 25 interface localizations (`.lproj` folders: cs, da, de, el, en, en_GB, es,
fi, fr, hu, it, ja, ko, nb, nl, pl, pt, ro, ru, sq, sv, tr, uk, zh_CN, zh_TW); Arabic and Hebrew
come as Middle East editions with an English or French interface and right-to-left features.

## Key languages

| Language | Code | InDesign 2026 UI | UI strings translated | Dialogs / tooltips / help | Script support (documents and UI) | Native review | Status | To `full` (h) |
|---|---|---|---|---|---|---|---|---|
| English | en | ✓ | 1,724 / 1,724 (100%) | all / all / none | Latin ✓ | yes | full | 0 |
| Simplified Chinese (Mandarin) | zh | ✓ (zh_CN) | 1,438 (83%) | menus, panels, most dialogs; tooltips partly; no help | CJK typesetting ✓ (cjk.md); canvas IME ✓, not checked by hand with a system IME (#322); CJK UI font not embedded in release builds, so boxes on Linux (#332) | no | partial | 25–40 |
| Spanish | es | ✓ | 1,438 (83%) | menus, panels, most dialogs; no help | Latin ✓; Spanish hyphenation ✓ (#222) | no | partial | 5–8 |
| Hindi | hi | ✗ | 0 (0%) | none | Devanagari shaped by harfrust in documents but untested; no Hindi hyphenation or spelling | no | none | 20–30 |
| Arabic | ar | Middle East edition (English/French UI) | 1,459 (85%) | menus, panels, most dialogs; no help | RTL UI text (`rtl.rs`: bidi + shaping); Arabic composition, kashida, digits, RTL stories and tables ✓ (arabic-typography.md); list bullets and multi-column RTL open (#100) | no | partial | 15–25 |
| French | fr | ✓ | 1,438 (83%) | menus, panels, most dialogs; no help | Latin ✓; French hyphenation ✓ (hyph-fr) | no | partial | 6–10 |
| Portuguese (Brazil) | pt-br | ✓ (pt) | 1,057 (61%) | menus, panels, some dialogs; no help | Latin ✓; Portuguese hyphenation ✓ | no | partial | 8–12 |
| Indonesian | id | ✗ | 0 (0%) | none | Latin ✓ | no | none | 12–18 |
| Japanese | ja | ✓ | 1,644 (95%) | whole interface (#272), vertical-type commands; no help | vertical text, kinsoku, aki, ruby, warichu ✓ (partial depth, cjk.md); canvas IME ✓, not checked by hand (#322); Japanese UI and Mincho fonts from craft-fonts | no | partial | 6–10 (+ IME checks, shared with zh) |
| German | de | ✓ | 1,438 (83%) | menus, panels, most dialogs; no help | Latin ✓; German 2006 hyphenation ✓ (hyph-de-1996, MIT) | no | partial | 6–10 |
| Korean | ko | ✓ | 0 (0%) | none | Korean line breaking at spaces ✓; canvas IME ✓, not checked by hand (#322) | no | none | 15–25 |
| Vietnamese | vi | ✗ | 0 (0%) | none | Latin with stacked diacritics via harfrust, untested | no | none | 12–18 |

Percentages are of the 1,724 measured strings. A language is `full` only when every user-visible
string is translated and its script renders and edits correctly; none but English is.

Japanese follow-up (2026-10-11): print ranges, imported style handling, the built-in `(import)`
caption, table row/column movement and row-height distribution now use the existing translation
catalog. Print, import options and an active table were rendered in Japanese and English with
craft-fonts. The language totals above retain their dated measurement; this is a targeted fix.

Hungarian interface (2026-11-01): `hu.rs` contains **1,906 entries and four context-specific
captions**, including all 37 tool names. The initial catalog was prepared with AI assistance
and personally reviewed in the app by **Veress Róbert**, a native Hungarian speaker with over
30 years of DTP experience. Further terminology and in-context feedback is welcome. This is a
partial catalog, not a new coverage measurement against current main: uncatalogued strings
fall back to English. Preferences lists `Magyar`; interface selection does not enable the
separate spelling and hyphenation work in #1484.

The interface uses bundled **Source Sans 3 Regular** and **Source Sans 3 Semibold**; monospace
labels use **JetBrains Mono Regular** (`assets/fonts/`, registered by `theme::font_definitions`).
`hungarian_catalog_glyphs_exist_in_the_bundled_ui_fonts` checks the actual configured font
bytes, every Latin letter used by the catalog and all Hungarian accented vowels in both cases,
including **ŐőŰű**, with system and optional craft-fonts fallbacks disabled. No new font asset
or font dependency is needed.

Hungarian regressions cover placeholder/context integrity, formal address and count/column
terminology, language persistence, matching menu paths, translated tool labels and real
right-click flyouts in both Tools-panel layouts. Narrow-window coverage exercises horizontal
scrolling, complete label/shortcut accessibility and selecting a scrolled row at 180 and
240 points; the existing wide-window checks keep their assertions.

## Other shipped languages

| Language | Code | UI strings translated | Status |
|---|---|---|---|
| Hungarian | hu | 1,906 entries + 4 context captions (2026-11-01); initial catalog native-reviewed | partial |
| Czech | cs | 1,722 (99%), plural categories handled | partial |
| Italian | it | 1,464 (85%) | partial |
| Ukrainian | uk | 1,432 (83%), plural categories handled | partial |
| Polish | pl | 1,717 (99%), count captions category-first; AI-drafted, awaiting native review | partial |
| Georgian | ka | 1,724 (~100%: every catalog key plus the command labels the others lack), count captions as `noun: n` | partial (reviewed by a native speaker) |
| Swedish | sv | 2,857 of 2,891 (99%, 2026-11-01 missing-translations report): menus, panels, dialogs and messages; singular for a count of one; AI-drafted with Claude Code from the English source, spot-checked by a native speaker (not every string) | partial |

DesignCraft ships **6** interface languages beyond the twelve; InDesign ships 17 beyond its
eight of the twelve (cs, da, el, en_GB, fi, hu, it, nb, nl, pl, ro, ru, sq, sv, tr, uk, zh_TW).

## Document languages

InDesign 2026 hyphenates and spell-checks ~40 languages (Hunspell and Duden dictionaries).
DesignCraft hyphenates **English, Dutch, French, German (2006 reform), Greek, Italian, Norwegian, Portuguese and Spanish** and spell-checks **English** only. Text language
can be set per paragraph and character (Properties, Character panel) and drives quotes and
locale shaping. See [typography-parity.md](typography-parity.md).
German hyphenation accepts `German: 2006 Reform`, `German: Swiss 2006 Reform` and `de` locale
codes without a spelling year or with `2006`. Traditional names, `de_DE_1901` and unknown
German variants have no automatic hyphenation; their German shaping and quotes are unchanged.

## Remaining effort

| Work | Opus 5.5 h |
|---|---|
| Fill the 83–85% catalogs (de, fr, es, zh, ar, it, uk; ~290 strings each) | 25–40 |
| pt-BR to full | 8–12 |
| New: Hindi, Indonesian, Korean, Vietnamese (1,724 strings, plural rules, tests) | 50–75 |
| Canvas IME (zh, ja, ko): checks by hand with system IMEs, and fixes | 10–20 |
| CJK UI font in every release build; Devanagari and Vietnamese rendering tests | 10–15 |
| Catalog completeness test: every `tr` key and command label present in every catalog | 4–8 |
| Help content (at least English, then the twelve) | 15–25 |
| **Total** | **120–180** + native-speaker review for 11 languages (human) |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-01 | minor | Initial Hungarian interface: 1,906 entries, four context captions, native review, bundled-font glyph coverage and wide/narrow flyout regressions |
| 2026-10-11 | minor | Localized Japanese print, import and table controls; checked Japanese and English renders |
| 2026-10-15 | minor | Swedish (`sv`) interface added |
| 2026-10-14 | minor | Georgian (`ka`) interface added |
| 2026-10-10 | major | Created: catalogs measured (1,724 strings), the twelve key languages plus Italian and Ukrainian |
| 2026-10-11 | minor | Czech (cs) added: 1,722 strings, plural categories handled |
| 2026-10-13 | minor | Polish interface added (1,717 strings) |
| 2026-10-22 | minor | French and Italian hyphenation (hyph-fr, hyph-it): the French row and Document languages updated |
