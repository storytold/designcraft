# Khmer writing support

DesignCraft reuses the shared `craft-khmer` implementation proposed for
[WordCraft #5357](https://github.com/storytold/wordcraft/pull/5357). Its dependency
is pinned to [craft-libs #551](https://github.com/storytold/craft-libs/pull/551);
the optional source-data staging tool is in
[craft-i18n #122](https://github.com/storytold/craft-i18n/pull/122). These companion
contributions must land before official release integration.

## User behavior

With the Khmer data included, unspaced Khmer receives dictionary/model word
boundaries for wrapping and justification, spelling suggestions, and preferred
spelling/phrase checks. These are limited rules, not a general grammar checker.
The model can segment unknown or misspelled words so proofreading can still
identify them. It never inserts spaces or normalizes document text.

Select **Khmer** in the Character panel, or use Khmer text in the default English
style. Explicit other-language and No Language proofreading choices remain
respected. **Edit > Spelling > Check Spelling** offers review and correction;
corrections use the existing undo history and document user dictionary.

Authored U+200B zero-width spaces are optional wrap opportunities, not mandatory
newlines. U+2060 word joiners and No Break formatting prevent wrapping. Shaped
clusters, dependent vowels and repetition signs stay attached. Justification
uses safe word gaps instead of stretching through Khmer clusters. Automatic
letter-spacing/glyph-scaling adjustments are suppressed inside Khmer; desired
scaling still applies. A single word without a safe word gap keeps its natural
width, including under Full Justify. Khmer letters are never spread to fill a line.
Paragraph and single-line composers share the same boundary source.

Imported Ligatures Off or tracking settings cannot disable required Khmer
shaping: `clig` remains enabled, and legacy AAT fonts retain the `liga`
substitutions they use to form subscripts. This preserves document settings
while keeping coeng sequences shaped; Latin ligature controls are unchanged.
See the [Khmer OpenType shaping specification](https://learn.microsoft.com/en-us/typography/script-development/khmer).

## Existing contributions

[DesignCraft #516](https://github.com/storytold/designcraft/pull/516), by
visarutforthaipbs, addresses Thai, Lao, Khmer and Myanmar composition using ICU4X.
Its word-gap glue and attached-glyph reasoning informed this integration.
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
This is resource size, not an installer-size measurement. Existing installed
Khmer fonts remain necessary unless the release's craft-fonts input supplies one.

## Validation scope

Validation covers actual composed glyph positions and line extents, not only
segmenter output. Targeted regressions cover authored boundaries, narrow columns,
formatting boundaries, repeated words, correction/undo and unchanged Latin text.
The local corpus comes from five real Khmer books authored in InDesign: 3,331
passages, with 302,429 model-counted word occurrences (302,022 after stripping
authored zero-width spaces). Eight configurations cover both composers, 180/300 pt
columns, and authored separators retained/removed. Across 288,032 composed lines,
checks found no breaks before dependent marks/repetition signs or after COENG/WJ,
and no changes to attached-glyph geometry. Excluding authored blank lines, 31
interior lines across the eight configurations remained short by more than
0.6 pt; Khmer letters are never spread to fill them. Each configuration had one
0.91–3.21 pt extent overrun at an authored forced line break in mixed text. The
substitute font lacked some Latin/punctuation glyphs, but no Khmer glyphs.

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

## Local CI baseline

On macOS, `cargo xtask ci` passes formatting, PDF-dependency policy and Clippy.
All four Khmer composition tests, seven engine regressions and two UI regressions
pass. The full native suite has 13 existing UI failures involving navigation/
paste shortcuts and missing Arabic or shortcut-symbol glyphs in test contexts.
An untouched-main run at `ad3b32fb` reproduces exactly the same 13 failures
(861 UI tests pass on main; 863 with this change). No candidate-only failures
were found. This is not a clean full-CI result.

The remaining checks were run separately after that baseline failure: doctests,
translation reporting, asset attribution, crate layering and the 16-crate WASM
check pass.
