# Hungarian spelling and hyphenation

The document's character language selects Hungarian (`hu`, `hu-HU`, Hungarian
or Magyar); choosing the UI language alone does not change a document's language.

DesignCraft delegates Hungarian spelling, suggestions and nonstandard
hyphenation to `craft-proof`. Nonstandard hyphenation uses craft-i18n's pinned
**hu Language pack**, never a vendored Spellbook crate or app-local
dictionary/pattern files. On native systems,
spelling searches the configured dictionary folders, installed Language packs
and system Hunspell dictionaries before falling back to an embedded pack.
Consequently a native dictionary may supply Hungarian spelling even when the hu
pack is embedded. The original English proofing path is retained.

## Data and build inputs

craft-i18n's hu pack retains the existing Hunspell aff/dic and standard hyph-hu
patterns, adding an optional Huhyphn nonstandard file with exact upstream bytes.
The standard file is required alongside it. The shared reader logs and skips
two malformed upstream replacement records; the builder never repairs data.
Older installers reject the added filename, so release updated craft-proof and
craft-font-store first, update consumers, then publish a new immutable pack/index.
Original authors, full MPL-2.0/MPL-1.1 texts and source information travel with
the pack. No language data is added to this repository.

For release/test builds, download and verify the pack before building, then set:

```sh
export CRAFT_PROOF_PACKS_DIR=/absolute/path/to/unpacked-language-packs
export CRAFT_PROOF_EMBED_PACKS=hu
```

The folder contains `hu/pack.toml`, spelling files, hyphenation files and
`hu/LICENCES/`. The shared build script includes only explicitly selected packs
and their licence texts. Parsing is lazy and Hungarian text alone uses the hu
dictionary. The same optional input works on native systems and WASM, which
cannot read an installed pack with std::fs. Installed native packs can also be
selected through the shared pack API. For Hungarian, the generic locale loader
tries the embedded spelling pack only when it finds no matching native pair.
If the first matching native dictionary is unreadable or invalid, it reports
that load error without trying the embedded pack, even when that pack is usable.
Hungarian spelling may therefore be unavailable because data is missing or
because a selected native dictionary failed to load.
A native spelling dictionary alone does not provide the nonstandard hyphenation
patterns used for inserted letters.

## Layout and editing

A nonstandard break is a source-character boundary plus display-only letters.
For example: `asszony` -> `asz-szony`, `könnyen` -> `köny-nyen`, and `hosszú` ->
`hosz-szú`. Line measurement, greedy/paragraph composition, all/small caps,
rendering and PDF export include those letters only at a selected break. Source
text is unchanged; PDF ActualText retains the original text. User exceptions,
soft hyphens, no-break ranges and language boundaries keep their existing rules.
Automatic hyphenation bounds each letter run to 4096 UTF-8 bytes before allocating
matching tables. Larger runs stay unbroken; neighbouring words and explicit soft
hyphens retain their existing handling. The source text is never truncated.
Native checks and suggestions run in the existing background workers. Browser
checking retains the existing resumable scan and frame-budget tests.

## Preserved checks

The shared library keeps the original 550 Hunspell verdicts, ten expected
corrections and 857 native Hyphen cases (with the original 2/2 minima), plus
provenance and licences. Two raw decomposed-accent verdicts intentionally differ
from the application's NFC-aware result; the original TSVs are unchanged.
With unchanged upstream data, the native comparison retains two known differences:
`masszázszokni` and `giccsetek` reference the skipped malformed records. These are
reported explicitly; the other 855 rows must still agree.
Small synthetic rule, malformed input, casing and normalization regressions are
also retained. Spellbook storage-specific tests are historical evidence of the
removed implementation, not tests of craft-proof. Application layout, source
text, PDF, mixed language, lazy loading and UI responsiveness regressions remain
in their original crates.

HU-data-dependent application tests are explicitly ignored in the default suite,
with the required pack recorded in each ignore reason. They assert the selected
pack when invoked; an unselected pack cannot silently pass. Selection assertions
confirm the build input, not the provenance of a native spelling dictionary
selected ahead of it.
For pack-spelling verification, use an isolated environment with no matching
native dictionaries; the shared library also tests the verified pack directly.
After setting the verified build inputs above, run the pack-dependent checks
explicitly:

```sh
cargo test --locked -p designcraft-compose hungarian -- --include-ignored --test-threads=1
cargo test --locked -p designcraft-pdf hungarian -- --include-ignored --test-threads=1
cargo test --locked -p designcraft-engine cmd::spelling:: -- --include-ignored --test-threads=1
cargo test --locked -p designcraft-ui-egui spelling_ui::tests::browser_scan_deadline_yields_and_preserves_results -- --ignored --test-threads=1
cargo test --locked -p designcraft-ui-egui spelling_ui::tests::background_cache_tracks_user_words_and_language_changes -- --ignored --test-threads=1
cargo test --locked -p designcraft-ui-egui spelling_ui::tests:: -- --test-threads=1
```

Run those commands with one test thread on constrained machines. Builds without
the optional pack still compile and test the regular paths; they do not claim
Hungarian-data coverage. The full recorded data checks remain in craft-proof
and run explicitly with the verified pack.

The shared default opportunity API retains its all-caps exclusion. DesignCraft
explicitly uses the layout opt-in for nonstandard all-caps breaks, so ASSZONY
can become ASZ-SZONY when capitalized-word hyphenation is enabled.

Companion pack change: [craft-i18n #152](https://github.com/storytold/craft-i18n/pull/152).
