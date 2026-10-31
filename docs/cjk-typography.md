# CJK import and composition

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-22 · **Change:** minor (the JIS X 4051 禁則処理 push-in/push-out priority modes are applied by both composers over CJK text: the empty default and Push In First compose as push-in-first, Push Out First, Push Out Only and adjustment-amount as their names say; Latin paragraphs are not biased; preflight reports only unknown values) · **Target:** Adobe InDesign 2026

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
| Kinsoku priorities | The four JIS X 4051 禁則処理 types, applied by both composers to CJK text: push-in-first (the empty default, as in InDesign) and push-out-first weight the break candidates by the adjustment direction, push-out-only turns the end-of-line compression off, and adjustment-amount (調整量優先) takes the candidate with the smallest adjustment; Latin paragraphs are not biased. Regressions pin the breaks, the line-end fits and the mode mapping |
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
- **Kinsoku priority policy** is applied as a line-cost weighting in the total-fit composer (and
  as the single-line composer's early-break rule): push-in-first rates breaks that would have to
  stretch the line worse than ones it can reach by compressing, push-out-first the other way,
  push-out-only leaves no compression at the line end, and adjustment-amount rates every
  candidate by its adjustment alone. It is a preference within the composer's other costs, not a
  hard "always 追い込み before 追い出し" rule. Preflight reports only values it does not
  know: the four JIS X 4051 modes are not reported as unapplied. Boundary prohibition, hanging
  punctuation and the named sets are unchanged.
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
| 2026-10-22 | minor | 禁則処理 priority modes applied: the empty default and Push In First compose as push-in-first (as InDesign's Japanese default), Push Out First, Push Out Only and adjustment-amount (minimum adjustment) as their names say, for CJK text; Latin paragraphs are not biased; preflight reports only unknown values; breaks, line-end fits and the mode mapping regression-tested |
| 2026-10-10 | trivial | Reviewed in the full re-measure; status line and this table added; summarized in [typography-parity.md](typography-parity.md) and [localization-parity.md](localization-parity.md) |
| 2026-10-07 | major | Created: CJK1: IDML CJK import and composition |
