# Localization parity

> **Last reviewed:** 2026-10-11 · **Last updated:** 2026-10-23 · **Change:** minor (pt-BR catalog filled and re-measured; the other catalog counts remain the 2026-10-10 measurement) · **Target:** Adobe InDesign 2026 (21.6)

Per-language status of the DesignCraft interface and of document typesetting in each script.
Script-specific typesetting detail: [cjk.md](cjk.md), [cjk-typography.md](cjk-typography.md),
[arabic-typography.md](arabic-typography.md). Overall numbers: [target-app-parity.md](target-app-parity.md).

**Dimension score: ~45% ready (estimated), 120–180 h** plus native-speaker review (human).

## How it was measured

Interface strings live in `crates/ui-egui/src/i18n.rs` (a shared table with German, French,
Spanish, Japanese and Simplified Chinese columns) and `crates/ui-egui/src/i18n/{ar,cs,it,ja,ka,pt_br,uk}.rs`.
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
| Simplified Chinese (Mandarin) | zh | ✓ (zh_CN) | 1,438 (83%) | menus, panels, most dialogs; tooltips partly; no help | CJK typesetting ✓ (cjk.md); canvas IME ✓, not checked by hand with a system IME (#322); CJK UI font not embedded in release builds, so boxes on Linux (#332) | no | partial | 25–40 |
| Spanish | es | ✓ | 1,438 (83%) | menus, panels, most dialogs; no help | Latin ✓; Spanish hyphenation ✓ (#222) | no | partial | 5–8 |
| Hindi | hi | ✗ | 0 (0%) | none | Devanagari shaped by harfrust in documents but untested; no Hindi hyphenation or spelling | no | none | 20–30 |
| Arabic | ar | Middle East edition (English/French UI) | 1,459 (85%) | menus, panels, most dialogs; no help | RTL UI text (`rtl.rs`: bidi + shaping); Arabic composition, kashida, digits, RTL stories and tables ✓ (arabic-typography.md); list bullets and multi-column RTL open (#100) | no | partial | 15–25 |
| French | fr | ✓ | 1,438 (83%) | menus, panels, most dialogs; no help | Latin ✓; no French hyphenation | no | partial | 6–10 |
| Portuguese (Brazil) | pt-br | ✓ (pt) | 2,431 / 2,434 (~100%, 2026-10-23) | whole interface but the Control panel's `IT`, `VA` and `Y:` captions; no help | Latin ✓; Portuguese hyphenation ✓ | no | partial | 1–2 |
| Indonesian | id | ✗ | 0 (0%) | none | Latin ✓ | no | none | 12–18 |
| Japanese | ja | ✓ | 1,644 (95%) | whole interface (#272), vertical-type commands; no help | vertical text, kinsoku, aki, ruby, warichu ✓ (partial depth, cjk.md); canvas IME ✓, not checked by hand (#322); Japanese UI and Mincho fonts from craft-fonts | no | partial | 6–10 (+ IME checks, shared with zh) |
| German | de | ✓ | 1,438 (83%) | menus, panels, most dialogs; no help | Latin ✓; no German hyphenation (InDesign ships 2006 reform patterns) | no | partial | 6–10 |
| Korean | ko | ✓ | 0 (0%) | none | Korean line breaking at spaces ✓; canvas IME ✓, not checked by hand (#322) | no | none | 15–25 |
| Vietnamese | vi | ✗ | 0 (0%) | none | Latin with stacked diacritics via harfrust, untested | no | none | 12–18 |

Percentages are of the 1,724 measured strings, except pt-BR's. A language is `full` only when every user-visible
string is translated and its script renders and edits correctly; none but English is.

Brazilian Portuguese (2026-10-21): the 1,724 strings were counted at `a127c6d5` (#355). Recounting
that commit by the same rule (every catalog key, engine command label and `tr`/`t` literal, less
the language codes tests pass to `tr`) gives 1,723, with pt-BR at the same 1,057. The rule gives
**2,434 strings** at main `2e36dcf9` (#1640), so the other percentages above overstate today's
coverage. `pt_br.rs` translates 2,431 of the 2,434, and the New Document unit names, which reach
`tr` at run time and no catalog had. Like every other language it leaves
the Control panel's `IT`, `VA` and `Y:` captions in English. Tools are named "Ferramenta …", parent
pages "página principal" and stories "artigo"; each menu path is made of the menu names. The
shared catalog test checks the placeholders, padding, ellipses and colons. The 645 new
strings were AI-drafted and approved by the contributor; they are not yet checked in the running
app, and a native-speaker review is pending. Older rows still use
"página-pai" and "página mestra" for parent pages and "Matéria" for a story.

Japanese follow-up (2026-10-11): print ranges, imported style handling, the built-in `(import)`
caption, table row/column movement and row-height distribution now use the existing translation
catalog. Print, import options and an active table were rendered in Japanese and English with
craft-fonts. The language totals above retain their dated measurement; this is a targeted fix.

## Other shipped languages

| Language | Code | UI strings translated | Status |
|---|---|---|---|
| Czech | cs | 1,722 (99%), plural categories handled | partial |
| Italian | it | 1,464 (85%) | partial |
| Ukrainian | uk | 1,432 (83%), plural categories handled | partial |
| Polish | pl | 1,717 (99%), count captions category-first; AI-drafted, awaiting native review | partial |
| Russian | ru | 1,709 of the 2,434 counted at `2e36dcf9` (70%, 2026-10-23), count captions category-first | partial |
| Georgian | ka | 1,724 (~100%: every catalog key plus the command labels the others lack), count captions as `noun: n` | partial (reviewed by a native speaker) |

DesignCraft ships **6** interface languages beyond the twelve; InDesign ships 17 beyond its
eight of the twelve (cs, da, el, en_GB, fi, hu, it, nb, nl, pl, ro, ru, sq, sv, tr, uk, zh_TW).

## Document languages

InDesign 2026 hyphenates and spell-checks ~40 languages (Hunspell and Duden dictionaries).
DesignCraft hyphenates **English, Spanish, Portuguese and Norwegian** and spell-checks **English** only. Text language
can be set per paragraph and character (Properties, Character panel) and drives quotes and
locale shaping. See [typography-parity.md](typography-parity.md).

## Remaining effort

| Work | Opus 5.5 h |
|---|---|
| Fill the 83–85% catalogs (de, fr, es, zh, ar, it, uk; ~290 strings each) | 25–40 |
| pt-BR to full: check it in the running app; one term each for parent page and story | 1–2 |
| New: Hindi, Indonesian, Korean, Vietnamese (1,724 strings, plural rules, tests) | 50–75 |
| Canvas IME (zh, ja, ko): checks by hand with system IMEs, and fixes | 10–20 |
| CJK UI font in every release build; Devanagari and Vietnamese rendering tests | 10–15 |
| Catalog completeness test: every `tr` key and command label present in every catalog | 4–8 |
| Help content (at least English, then the twelve) | 15–25 |
| **Total** | **120–180** + native-speaker review for 11 languages (human) |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-23 | minor | Re-measured at `2e36dcf9`: pt-BR also translates the 12 strings main added since 2026-10-21 (2,431 of 2,434); Russian 1,709 of 2,434 |
| 2026-10-21 | minor | pt-BR catalog filled (634 strings and the unit names) and re-measured at `438d7b6d`: 2,416 of 2,419 strings; the 1,724 baseline traced to `a127c6d5`; Russian (`ru`, 1,705) listed, six languages beyond the twelve; pt-BR to full 8–12 → 1–2 h; dimension estimates unchanged |
| 2026-10-11 | minor | Localized Japanese print, import and table controls; checked Japanese and English renders |
| 2026-10-14 | minor | Georgian (`ka`) interface added |
| 2026-10-10 | major | Created: catalogs measured (1,724 strings), the twelve key languages plus Italian and Ukrainian |
| 2026-10-11 | minor | Czech (cs) added: 1,722 strings, plural categories handled |
| 2026-10-13 | minor | Polish interface added (1,717 strings) |
