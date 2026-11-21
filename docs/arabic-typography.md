# Arabic import and composition

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-11-16 · **Change:** minor (opt-in Hebrew letter extension) · **Target:** Adobe InDesign 2026

Task: AR1. These changes repair verified gaps in the existing typesetting pipeline.
They do not claim full InDesign or vendor-specific Naskh composition parity.

## Architecture

Portable character controls live in `doc::arabic` and the inherited attribute
model. Story column direction and table column direction are independent of
paragraph direction. The IDML adapter translates serialized values at the boundary.
The font layer resolves script/direction runs and passes language and surrounding
text to the OpenType shaper. The composer retains complete cluster text for
paragraph-level bidi resolution, then applies line-level ordering and placement.
The renderer continues to consume placed glyphs without Arabic-specific policy.

Existing `type.char`, `type.para` and `table.options` commands edit the controls.
`story.setDirection` edits column flow with history and cache invalidation;
`story.get` reports it. Table cell indices remain logical even when their visual
positions are reversed. Physical left/right insertion commands account for direction.

## Implemented rules

| Area | Behavior |
| --- | --- |
| Mixed scripts | Separate Hebrew/Arabic script runs, resolved shaping direction, language tags and OpenType joining features |
| Style boundaries | Pre/post shaping context preserves joining when a character style changes within an Arabic word |
| Font fallback | Combining marks and join controls stay with the preceding base face |
| Bidirectional layout | Full paragraph context, cluster contents and Unicode controls survive line breaking; per-line UAX #9 ordering keeps marks attached, including lines with tab leaders |
| Character direction | Imported default/LTR/RTL overrides participate in bidi resolution; single-character punctuation mirroring is corrected against final paragraph levels |
| Digits | European, Arabic-Indic and Persian conversion in both directions, contextual defaults, language-sensitive native digits, original UTF-8 source ranges retained |
| Diacritics | Font GDEF classification and OpenType mark anchors; explicit X/Y offsets affect marks only, in thousandths of an em |
| Positional forms | Explicit isolated/initial/medial/final OpenType feature selection; automatic forms remain the default |
| Kashidas | Paragraph and character switches, actual font coverage, shaper-approved insertion boundaries, nonjoiner protection, whole-cluster movement to keep marks attached |
| Story direction | Independent right-to-left column flow, including span-column bounds |
| Table direction | Right-to-left cell placement, merged spans, physical edge styles/borders, hit testing and left/right column insertion |
| Serialization | Character controls, paragraph policy/width values, story/table directions survive native data and IDML round trips |

## Remaining limitations

- Vendor-specific Arabic/Naskh justification policies and Paragraph Kashida Width
  presets are preserved but not reproduced. Preflight reports their use.
  Automatic elongation still uses bounded, scaled tatweel insertion rather than
  full line reshaping or a vendor allocation algorithm.
- Loose/Medium/Tight/OpenType-from-baseline diacritic presets are preserved and
  reported. Composition uses font anchors and explicit offsets instead.
- Script-specific stretched alternates, contextual swash policies, complete RTL
  tab-stop semantics and re-shaping at emergency intra-word line breaks require
  further compatibility work. Tracking and arbitrary metric changes within a
  cursive word can still open joining gaps.
- A font must contain the required glyphs and appropriate OpenType tables. This
  change neither embeds proprietary fonts nor makes missing-font errors disappear.
- CJK compatibility boundaries remain documented in `cjk-typography.md`.

## Hebrew letter extension

The opt-in paragraph attribute `hebrewLetterExtension` (Extend Hebrew Letters in
Justification) widens one eligible letter per word on justified lines before
stretching spaces. It selects the last eligible letter in logical order, visits
words from the logical end, and skips an alternate that cannot fit the remaining
shortfall. The letters א ד ה כ ל ם ר ת use the font's U+FB21–U+FB28 wide forms;
ordinary mem מ is excluded. Existing shaped substitutions are preserved. Fonts
without wider forms use the existing spacing behavior. No glyph is scaled by
this option, and ragged final lines are unchanged.

Marks stay in their cluster and move by half the added width relative to the
base. Story text and PDF extraction retain the original letters. Local overrides
and paragraph styles preserve the option in native files. IDML neither reads nor
writes it because InDesign has no equivalent attribute.

Deferred work includes paragraph-wide reflow scoring of extension,
distributed/weighted allocation, OpenType `jalt` and variable axes, per-letter caps
and occurrence limits, locked spacing with underfull warnings, font anchors for
marks on wide forms, and IDML support if an equivalent becomes available.

## Verification and sources

Regression tests cover independent IDML XML, round trips, forced line breaks,
character overrides, original byte ranges, mark placement, style-boundary joining,
kashida controls, story/table flow, merged cells, command validation and undo.
Font-dependent tests use available fonts; no proprietary font or user document
is committed. Synthetic Arabic documents provide visual smoke checks.

The AR1 validation run passed all six `cargo xtask ci` gates and 616 tests
(with two existing ignored tests). WASM checks used the installed Rust standard
library sources via `-Z build-std=std,panic_abort`, because a prebuilt WASM standard
library was unavailable. The synthetic Arabic proof was exported to IDML and
reopened in the desktop app: preflight reported zero errors and zero warnings.
The original Chinese IDML was also reopened; its four Mojikumi/kinsoku-priority
compatibility warnings remain, along with separate font/link/overset findings.

- [Unicode UAX #9](https://www.unicode.org/reports/tr9/) defines paragraph resolution
  and line-level bidirectional ordering.
- [Adobe Middle Eastern scripting guide](https://developer.adobe.com/indesign/uxp/resources/recipes/rtl/)
  documents the distinct direction, digit, kashida and diacritic controls.
- [Diacritic position enumeration](https://developer.adobe.com/indesign/uxp/dom/api/d/diacritic-position-options/)
  and [positional forms](https://developer.adobe.com/indesign/uxp/dom/api/p/positional-forms/)
  identify serialized options; their presence alone does not establish visual parity.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-16 | minor | Added opt-in Hebrew letter extension using font wide forms before space justification (#1931) |
| 2026-10-10 | trivial | Reviewed in the full re-measure; status line and this table added; summarized in [typography-parity.md](typography-parity.md) and [localization-parity.md](localization-parity.md) |
| 2026-10-07 | major | Created: AR1: Arabic import and composition |
