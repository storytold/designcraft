# Private INDD corpus checks — 2026-10-22

Local development evidence for roadmap gaps 2, 3, 4 and 6. This is not a
redistributable corpus, an independent IDML oracle, or a visual-fidelity score.
No private documents, filenames, font files, or artwork are included here.

## Baseline

The five-document pilot passed before expansion. Of 295 discovered INDD paths,
205 had local contents and were tested (203 unique SHA-256 hashes). Ninety
cloud-only inputs were unavailable. Two directory-enumeration timeouts mean
the discovery count is not guaranteed exhaustive.

All 205 tested inputs passed the standalone conversion → IDML import → PDF and
IDML export path, and also the actual `file.open` INDD path → independent
preflight/PDF/IDML stages → reopen/export. The standalone converter was
`d249788285ac083fd29f3150695c9a21337caadc`; this DesignCraft revision's
lockfile pins its internal converter at `7f3060e5bce78d21ea70e0d5eacb96edce15dfbe`.
Those paths remain separately reported.

The checks reuse the existing CLI, engine commands, sandbox/integrity helpers,
and indd-utils' IDML element/story-text extraction. IDs are regenerated on
export, so text multisets and thread-chain shapes are compared independently
of Self IDs. Repeated story text retains multiplicity.

- Direct and separated exports had equal element counts, story-text multisets
  and text-associated thread signatures in all 205 documents.
- Converted IDML versus DesignCraft-exported IDML had exact extracted story
  text in 204/205 documents. The remaining case adds a paragraph break after
  an anchored table. It remains a serialization investigation candidate, not
  evidence of missing text. The text-associated thread signature also changes
  there because it includes the story-text hash.
- Element-count differences include defaults/resources that DesignCraft does
  not retain, and SVG → Image representation changes. Counts alone do not
  establish visible loss or equivalent styling/geometry.
- Existing preflight found missing-font issues in 47 documents, missing links
  in 176, and 123 overset issues across 28 documents. Resource candidates are
  statted without relinking: missing, cloud-only, locally present, and exact
  old-iCloud-root suffix matches remain distinguishable. These availability
  findings must be resolved before attributing reflow to composition bugs.

## Confirmed importer regression and fix

INDD opening supplies raster stand-ins for unavailable linked artwork while
retaining its declared PDF/EPS type. The default IDML exporter embeds those
stand-in bytes. Previously, reopening sniffed their raster format and changed
the placed PDF/EPS into an Image. This affected 36 of the 205 documents
(33 had PDF changes; nine had EPS changes; some had both).

The importer now recognizes embedded raster bytes on a PDF/EPS graphic as a
stand-in, retains the declared graphic type, PDF page and crop, and emits an
explicit warning. Actual bytes read from a linked replacement still determine
its format. No artwork is recovered or relinked by this fix.

The synthetic regression failed before the fix. With the fix, all 161 IDML
library tests pass, including the vector-preview round trip and a linked
raster-replacement counterexample. Reopening all 205 baseline generated IDMLs
with the patched CLI succeeds: PDF/EPS type-change documents fall from 36 to
zero, all compared element counts remain equal, and extracted story text is
equal in all 205. The corrected CLI binary is recorded separately from the
baseline; the baseline results are not overwritten.

## Remaining work

The standalone indd-utils checkout also fixes false orientation warnings caused
by exact comparison of near-identity text transforms. Seven frames in two
documents differed from identity only by 3.6e-15–7.1e-15. A tolerance of
`64 * f64::EPSILON` removes the seven warnings. Re-conversion of all 205 staged
inputs succeeds and all IDML package entries remain byte-for-byte equal to the
baseline. Synthetic tests reject meaningful scaling, shear, the opposite
rotation and non-finite values. This separate converter fix is **not** in
DesignCraft's pinned dependency until it lands upstream and the lockfile is
updated; the importer fix above is already in this local DesignCraft checkout.

Keep gaps 2–4 open: no verified reference rendering or original same-save IDML
was available, and INDT was not tested. Independent page geometry, formatting,
per-cell text, stacking order, and composition comparisons are still needed.
Font/link failures need minimal synthetic reproductions once external resource
availability is controlled. Private corpus checks stay local; public CI can
run the synthetic regression tests without private data.

The alpha Primary Text Frame/autoflow blocker and cross-platform editing gates
are unaffected. No readiness percentage or milestone is raised by this study.

The harness is a sibling of this checkout (`../development.py`,
`../validate_fix.py`, `../validate_converter.py`). It operates on previously
verified staged copies and writes to a separate, non-synced local directory.
Document tools run with OS-enforced network denial and writes confined to
their per-document output directory. Exact per-file findings and logs remain
in the local results, outside this repository.
