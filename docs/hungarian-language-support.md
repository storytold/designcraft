# Hungarian spelling and hyphenation

This change adds Hungarian text support independently of the interface language. Set the text language to **Hungarian** in the Character panel; paragraph hyphenation must be enabled. The new Hungarian interface strings extend the localization supplied by PR #1223.

## Behavior

The existing spelling command and dynamic underlines dispatch by text language. Hungarian uses Spellbook 0.4.2 with the pinned Magyar Ispell 1.9 dictionary. English retains the existing checker; unsupported languages are not silently checked against English. The spelling command initializes the Hungarian dictionary only if a story in the requested checking scope contains a Hungarian language run. English-only, unsupported-language, empty-document and missing-story checks do not request it; initialization errors are still reported for Hungarian text. NFC normalization is lookup-only, preserving document bytes and selections. The spelling dialog supports suggestions, change, skip and document-dictionary additions, with stale-document checks and normal undo. Within one `spelling.check` command, suggestion results are reused for identical (language, case-preserving word) pairs across the selected stories. Each story’s language runs are resolved once; disabling suggestions avoids suggestion computation entirely. The cache is discarded when the command finishes.

Huhyphn extended patterns support nonstandard Hungarian breaks such as **asz-szony**, **köny-nyen**, **hosz-szú**, **megy-gyel** and **gics-cse-tek**. Inserted letters exist only in the composed display. Their shaped advance, OpenType features and tracking are used consistently in both line composers, including real small caps. Original word bytes survive reflow and saving. PDF exports render these inserted letters and their hyphen as outlines, avoiding duplicate text in extraction.

Paragraph limits, no-break formatting, explicit soft hyphens and user exceptions retain precedence. For separator-separated input, minimum word length, leading/trailing letter counts and capitalization limits apply independently to each word. The 1,024-byte safety bound also applies per word: a long sequence of short words remains eligible, while an oversized individual word is skipped. Source character offsets and replacement-letter suffixes are retained, including for decomposed accents. Mixed-language words are left unbroken. Two malformed upstream pattern declarations are corrected and documented in the data's MODIFICATIONS.txt.

On native platforms the full report is built on a background thread. On the web, scanning processes small chunks of at most 128 character/run-boundary steps or eight completed tokens. A browser-safe monotonic clock is checked between chunks: scanning yields after approximately 2 ms, with a hard cap of 16,384 steps and 1,024 tokens per frame even on low-resolution clocks, including inside a single long story. Multiple UI callers share that frame budget; changed document revisions discard the pending scan. Only complete, current results are displayed. Dictionary initialization and individual word/suggestion lookups are still synchronous on the web; the scan budget is not a hard wall-clock deadline and does not eliminate the initial dictionary-load pause.

## Dependencies and licensing

| Component | Selected license | Source and notices |
| --- | --- | --- |
| Spellbook 0.4.2 with local fixes | MPL-2.0 | vendor/spellbook/LICENSE, MODIFICATIONS.txt, sources.json |
| Magyar Ispell 1.9, Németh László / Godó Ferenc | MPL-2.0 | assets/dictionaries/hu_HU/ |
| Huhyphn, Bence Nagy; nonstandard patch by László Németh | MPL-1.1 | assets/hyphenation/hu/ |
| hyphenation / hyphenation_commons 0.8.4 | MIT OR Apache-2.0 | pinned Cargo dependencies |

The original alternative-license notices are retained; this contribution selects the MPL options. No native Hunspell/Hyphen library is linked. Unused upstream GPL English fixtures, French dictionaries, examples and legacy fixture suites are not distributed in the minimal Spellbook vendor tree. Its independent integration test target covers the local fixes without those fixtures. Existing upstream unit-test source is retained, but its fixture-dependent lib/doctests are not enabled by the minimal manifest.

Local Spellbook fixes cover signed syllable accounting, permitted suffix chains, COMPOUNDRULE endings (including numbers), compound-aware suggestions, ngram prefix/suffix expansion, restored-stem conditions in alternating three-affix chains, the SIMPLIFIEDTRIPLE look-behind guard, and forbidden-word/affix interactions. See the vendor modification notice and regression target.

## Reproducing the data

From the repository root, verify the spelling transformation against pinned original files:

```sh
python3 tools/hu-dictionary/rebuild.py --source-dir /path/to/original/hu_HU --check
```

Or explicitly download the public, checksum-verified originals:

```sh
python3 tools/hu-dictionary/rebuild.py --download --check
```

Omit `--check` only when intentionally replacing the generated spelling assets. The original URLs, commit and SHA256 checksums are recorded in sources.json.

Regenerate the corrected hyphenation binary with:

```sh
cargo run --manifest-path tools/hu-hyphenation/Cargo.toml
```

The fallible parser rejects the two originally malformed replacement declarations. The generated binary is byte-reproducible; its current SHA256 is a6519e424fb86a0b449b204e86a07b298e6963e881d5ff67adef05f1cdd0a101.

## Validation and limits

Run `CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo xtask ci`. This runs the workspace tests plus the standalone Spellbook compatibility suite and hyphenation generator tests. See tools/hu-language-tests/README.md for focused commands and corpus provenance.

Recorded comparisons cover 550 spelling verdicts, ten expected corrections and 857 hyphenation words. Command regressions count suggestion computations and language resolution, preserve exact ranges/results across repeated words and stories, distinguish language and case, and cover disabled suggestions. Direct hyphenation regressions cover input longer than 1,024 bytes, including short words on both sides of an oversized word. Synthetic cases protect the overflow and suffix fixes, including positive/negative three-affix conditions checked against native Hunspell 1.7.5. Incremental-scan tests compare exact ranges with the full scan, preserve words across tiny budgets, and bound long tokens/punctuation, elapsed-time deadlines and repeated per-frame requests. The ignored `browser_scan_timing` UI test measures the same scheduler on the native host (not browser FPS); run it separately with `cargo test -p designcraft-ui-egui browser_scan_timing -- --ignored --nocapture`. Layout tests cover both composers, uppercase and small caps, decomposed accents, no-break/mixed-language text, width fitting and original-source mapping; PDF tests check extracted text. These are bounded regression samples, not exhaustive compatibility certification. The local app and sample were manually accepted by Veress Róbert.

Synthetic SIMPLIFIEDTRIPLE tests cover equal and unequal preceding codepoints, including UTF-8. Hunspell 1.7.5 agrees on the ASCII-repeat cases (also with accented prefixes); it rejects the synthetic repeated-accent form `aáábc` that Spellbook accepts. The bundled Hungarian dictionary does not enable this option, so this known generic-engine difference does not affect its spelling results.

Suggestion ordering can differ from Hunspell. Other languages' spelling support is unchanged. Rare legacy compatibility-ligature replacement patterns that cannot be represented by insertion are conservatively left unbroken. Explicit user hyphenation exceptions supply source break positions, not replacement-letter rules.
