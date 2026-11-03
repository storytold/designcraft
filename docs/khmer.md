# Khmer writing support

DesignCraft reuses the shared `craft-khmer` implementation proposed for
[WordCraft #5357](https://github.com/storytold/wordcraft/pull/5357). Its dependency
is pinned to [craft-libs #551](https://github.com/storytold/craft-libs/pull/551);
the optional source-data staging tool is in
[craft-i18n #122](https://github.com/storytold/craft-i18n/pull/122). These companion
contributions must land before official release integration.

## User behavior

With the Khmer data included, unspaced Khmer receives dictionary/model word
boundaries for wrapping, spelling suggestions, and preferred
spelling/phrase checks. These are limited rules, not a general grammar checker.
The model can segment unknown or misspelled words so proofreading can still
identify them. It never inserts spaces or normalizes document text.

Select **Khmer** in the Character panel, or use Khmer text in the default English
style. Explicit other-language and No Language proofreading choices remain
respected. **Edit > Spelling > Check Spelling** offers review and correction;
corrections use the existing undo history and document user dictionary.

Authored U+200B zero-width spaces are optional wrap opportunities, not mandatory
newlines. U+2060 word joiners and No Break formatting prevent wrapping. Shaped
clusters, dependent vowels and repetition signs stay attached. Paragraph and
single-line composers share the same boundary source.

## Justification

Justified Khmer follows the InDesign behavior:

- A Khmer word boundary, whether found by the model or authored as U+200B, only
  permits a line break. It has no width, so justification never opens a gap there.
- Real word spaces (U+0020, U+00A0) take the length. They grow past Word Spacing's
  maximum as far as the line needs, so a line with one real space can have one
  very large gap.
- **Type > Justification…** Glyph Scaling applies to Khmer within its minimum and
  maximum, with attached marks scaled along with their base. It stays at 100% by
  default; a Khmer book style typically uses Word Spacing 100/100/100%, Letter
  Spacing 0/0/0%, Glyph Scaling 96/100/104% and Single Word Justification Align Left.
- A line without a real word space is not stretched: it keeps its natural width.
  To justify it, add a real space where one can be placed. Single Word Justification
  Align Center and Align Right still position it; Full Justify cannot spread Khmer
  letters, so it stays at the start of the line. If the line is slightly too wide, it
  still shrinks down to Glyph Scaling's minimum to fit.
- Letter Spacing never reaches Khmer letters.

Imported Ligatures Off or tracking settings cannot disable required Khmer
shaping: `clig` remains enabled, and legacy AAT fonts retain the `liga`
substitutions they use to form subscripts. This preserves document settings
while keeping coeng sequences shaped; Latin ligature controls are unchanged.
See the [Khmer OpenType shaping specification](https://learn.microsoft.com/en-us/typography/script-development/khmer).

## Existing contributions

[DesignCraft #516](https://github.com/storytold/designcraft/pull/516), by
visarutforthaipbs, addresses Thai, Lao, Khmer and Myanmar composition using ICU4X.
Its attached-glyph reasoning informed this integration.
This change supplies the Aksara-derived shared engine for Khmer; it does not
replace that contribution's work on the other three languages. Existing
[font fallback #504](https://github.com/storytold/designcraft/pull/504) supplies
installed Khmer faces to the interface. No fonts are added here.

The shared core also credits WordCraft contributions by touchde (#3110) and
NguyenDuc2309 (#1579). Different compound boundaries can both be acceptable;
finer boundaries are useful for fitting lines, not evidence that every longer
compound is incorrectly segmented.

## Build and data

The default build contains no Khmer dictionaries. It must not report Khmer text
as checked when the data is absent; `spelling.status` exposes availability.
There is no runtime pack downloader in this change. For an explicit build,
check out craft-i18n revision `305a53fc197cce0e7b2c90b86c9c37083ae216bd` and stage
its verified source pack:

```sh
cargo run --manifest-path ../craft-i18n/Cargo.toml --bin language-packs -- stage --locale km --out ../khmer-pack
CRAFT_KHMER_DIR="$(cd ../khmer-pack && pwd)" CRAFT_KHMER_REQUIRED=1 cargo xtask ci
CRAFT_KHMER_DIR="$(cd ../khmer-pack && pwd)" CRAFT_KHMER_REQUIRED=1 cargo run --locked -p designcraft
```

Use absolute paths. On Windows, set the same variables in PowerShell. Source
hashes and the upstream MIT notice are checked during the offline build;
`CRAFT_KHMER_REQUIRED=1` rejects missing data. The six losslessly compressed
resources total 714,343 bytes, with no dictionary pruning or model quantization.
SBBIC identifies its internal work, the SEAlang frequency lists and SIL NRSI's khmerlbdict
as the data's sources; their MIT notices are in `NOTICE`. The exact composition of the
tagger's training corpus is unconfirmed.
This is resource size, not an installer-size measurement. Existing installed
Khmer fonts remain necessary unless the release's craft-fonts input supplies one.

## Validation scope

Validation covers actual composed glyph positions and line extents, not only
segmenter output. Targeted regressions cover authored boundaries, narrow columns,
formatting boundaries, repeated words, correction/undo and unchanged Latin text; the
justification regressions use a synthetic font and authored boundaries, so they need
neither the data pack nor a Khmer font.
The local corpus comes from five real Khmer books authored in InDesign: 3,331
passages, with 302,429 model-counted word occurrences (302,022 after stripping
authored zero-width spaces). Sixteen configurations cover both composers, 180/300 pt
columns, authored separators retained/removed, and default Justification settings or
Word Spacing 100/100/100% with Glyph Scaling 96/100/104%. Across 565,353 composed
lines, checks found no breaks before dependent marks/repetition signs or after
COENG/WJ, no Khmer glyph wider than its scaling allows, no scaling outside its limits
and no attached glyph moved from its base. 345,681 of 345,689 interior lines with a
real word space fill the measure within 0.6 pt; the other eight are 0.91–3.21 pt
overruns at an authored forced line break in mixed text under the default settings
(none with the Glyph Scaling settings). The 166,072 interior lines without a real word
space keep their natural width, as designed: 15–19 pt short on average under the default
settings and 7–9 pt with Glyph Scaling, until an author adds a space. Tabs on them
still reach their stops, and Latin-only lines (URLs) keep the existing Single Word
Justification. The substitute font lacked some Latin/punctuation glyphs, but no Khmer glyphs.

These are layout measurements, not linguistic accuracy ratings. It is not a
held-out linguistic gold standard, and model-counted word occurrences are not
an accuracy score. Private book text, IDML files and reference PDFs stay outside
the repository.

Native macOS checks also verified readable Khmer suggestions, correction and undo.
The two supplied passages produced no spelling findings. The read-only
`spelling.check {suggestions:false}` query avoids candidate generation for bulk
review; `spelling.suggest {story,start,end}` revalidates and returns one issue.

The local InDesign PDF and recovered IDML have different edition dates. Matched
passages can support visual comparison; exact pagination and pixel equality
are not claimed. Missing source fonts or import differences must be distinguished
from Khmer segmentation/justification failures. Windows runtime acceptance and
release packaging remain separate work.

## Known limits

The caret at an unspaced line end follows the behavior DesignCraft already has for CJK
wraps: that offset is also the start of the next line, so End and clicks past the line
end draw the caret there. WordCraft's two-position caret is not part of this change.

## Local CI baseline

On macOS with the Khmer pack, `cargo xtask ci` passes formatting, PDF-dependency policy
and Clippy; translation reporting, doctests, asset attribution, crate layering and the
16-crate WASM check pass when run separately. All 3,814 other workspace tests pass,
including every Khmer composition, proofreading and UI regression. 13 UI tests fail,
involving navigation/paste shortcuts and missing Arabic or shortcut-symbol glyphs in test
contexts. An untouched-main run at `f3e35af8` reproduces exactly the same 13 failures, so
none are specific to this change, but this is not a clean full-CI result. The Khmer
composition tests also pass without the data pack.
