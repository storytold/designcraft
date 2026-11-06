# CJK import and composition

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-07 · **Change:** trivial (status line and revision history added; content unchanged in the 2026-10-10 review) · **Target:** Adobe InDesign 2026

Task: CJK1. This is incremental support, not a claim of InDesign composition parity.

## Data flow

The IDML adapter resolves font resources and named/custom kinsoku sets into portable
document data. `doc::cjk` owns those resources; `CharAttrs` and `ParaAttrs` retain
inheritance and local overrides. The composer resolves composite-font character
assignments before shaping, applies character metrics, then supplies break
constraints to both line breakers. Line placement applies em alignment, leading
references and hanging punctuation. The renderer consumes placed glyphs unchanged.

`style.compositeFont.list` and `style.compositeFont.set` expose resources through the
existing command channel. Editing uses normal document history and style cache
invalidation. Font listing, replacement and preflight resolve component fonts.

## Implemented behavior

| Rule | Behavior and coverage |
| --- | --- |
| Composite fonts | Character mappings, component family/style, relative size, scales and baseline shift; synthetic selection and command/undo tests |
| Leading/trailing aki | Explicit em spacing without outline scaling; inherited resets and IDML round trip |
| Kerning import | Automatic methods take precedence over inactive numeric values; the 1e11 sentinel cannot become manual spacing |
| Tsume | Compress nonnegative glyph sidebearings without scaling ink; full compression regression |
| Jidori | Fixed group width with protected internal boundaries and justification advances; both composers tested |
| Kinsoku sets | Custom character sets and independent common punctuation defaults for named sets; explicit disabling retained |
| Bunri kinshi / rensuuji | Protect repeated dash/ellipsis pairs and numeric sequences |
| Hanging punctuation | Regular/forced edge allowance integrated with line breaking and placement |
| Ideographic spaces | Paragraph switch controls whether fullwidth spaces participate in word spacing |
| Em alignment / leading | Em top/center/bottom alignment and baseline reference offsets; mixed-size regressions |
| Kenten / glyph forms | Preserve emphasis character, shape it with font fallback; map supported OpenType forms |
| Tate-chu-yoko offsets | Carry horizontal/vertical offsets through the existing vertical group placement |

## Unfinished behavior

- **Mojikumi**: definitions, base-set names, all override rows and paragraph references
  survive import/export. The numeric character-class spacing tables and their
  compression priorities are **not executed**. Preflight reports this explicitly.
- **Kinsoku push-in/push-out priorities**: retained, but not used in candidate
  selection. Preflight reports this explicitly. Boundary prohibition alone is not
  equivalent to implementing this priority policy.
- Named kinsoku defaults are clean-room common punctuation rules, not verified
  reproductions of every vendor preset. Explicit custom tables take precedence.
- ICF alignment currently uses font ascender/descender metrics; exact ideographic
  character-face metrics are not implemented. CenterDown currently shares the
  center leading reference. These require separate compatibility work.
- Jidori groups are scoped to resolved shaping runs. Mixed component-font or style
  changes inside one intended group need a group model above shaping.
- Frame-grid composition and detailed ruby/warichu controls are outside this change.

Preserving a parameter is not evidence that its visual rule has been implemented.
Missing fonts, missing links and overset remain separate preflight findings;
removing unsupported-rule warnings to produce a clean report is not acceptable.

## References

- [Adobe MojikumiTable API](https://developer.adobe.com/indesign/uxp/dom/api/m/mojikumi-table/)
  describes the serialized override tuple, but does not define the numeric class mapping.
- [Adobe spacing priorities](https://helpx.adobe.com/hk_en/indesign/desktop/language-and-proofing/chinese-japanese-and-korean/set-spacing-priorities-in-mojikumi-character-classes.html)
  describes ordered compression, which requires more than loading the table.

Tests use synthetic documents; user IDML files and proprietary application output
are not committed as fixtures.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | trivial | Reviewed in the full re-measure; status line and this table added; summarized in [typography-parity.md](typography-parity.md) and [localization-parity.md](localization-parity.md) |
| 2026-10-07 | major | Created: CJK1: IDML CJK import and composition |
