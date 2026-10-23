# Hungarian language regression data

The fixtures are deterministic, UTF-8 test inputs and recorded oracle results, not a claim of exhaustive Hungarian language coverage.

- `main.tsv`: 418 word/verdict rows from Hunspell 1.7.5.
- `focused.tsv`: 38 additional word/verdict rows.
- `extended.tsv`: 94 additional word/verdict rows.
- `suggestions.tsv`: ten misspellings and the expected correction; suggestion ordering is intentionally not asserted.
- `hyphen-native.tsv`: 857 distinct lowercase words and the native Hyphen result using the corrected Huhyphn dictionary. The second column uses `=` at each break, including replacement letters.

The spelling cases were supplied/generated for this contribution. Hyphenation words were selected deterministically (seed 20261021) from the pinned Magyar Ispell data: 350 entries containing doubled Hungarian consonant groups, 500 general entries, and 11 explicit examples, followed by deduplication. Recorded verdicts were produced by Hunspell 1.7.5; recorded breaks by hunspell/hyphen commit c299444629416f90625a2e56fdfc4efdca131ecc. Both native engines were used solely as test oracles and are not linked into DesignCraft.

Fixture provenance, checksums and selected licenses are in `sources.json`. For the Hungarian dictionary-derived words the selected license is MPL-2.0; for Huhyphn-derived break data it is MPL-1.1. The full license texts and original author notices are in `assets/dictionaries/hu_HU` and `assets/hyphenation/hu`. Locally written tests follow their containing crate's license.

## Offline regression checks

From the repository root:

```sh
CARGO_BUILD_JOBS=1 cargo test --manifest-path vendor/spellbook/Cargo.toml --test hungarian_compatibility -- --test-threads=1
CARGO_BUILD_JOBS=1 cargo test --manifest-path tools/hu-hyphenation/Cargo.toml -- --test-threads=1
CARGO_BUILD_JOBS=1 cargo test -p designcraft-compose hungarian -- --test-threads=1
CARGO_BUILD_JOBS=1 cargo test -p designcraft-pdf hungarian -- --test-threads=1
```

Add `--offline` after `cargo test` when the dependencies are cached. These tests are also included in `cargo xtask ci`; the native reference tools are not needed to run them. The application-level spelling/UI tests remain in the workspace test suite.
