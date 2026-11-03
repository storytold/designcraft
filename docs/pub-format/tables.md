<!-- Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0 -->
# Publisher tables

Source: only the binary Contents, Quill and Escher streams in the allowed
pubsmith fixtures at `0ccdf27`. A directory-only scan of all 142 Contents
streams found type-0x10 records only in `table__2x2.pub`, `table__3x2.pub`, and
`table__merged.pub`. No external Publisher reader or truth JSON was consulted.

## Contents: table and grid

A table is the type-0x10 object already described in `contents-stream.md`.
Its key 0x27 is the Quill story id (3 in all three samples). Additional fields:

| Key | Kind | Observed meaning |
|---|---|---|
| 0x66 | 0x20 | Row count: 2, 3, 2 respectively. |
| 0x67 | 0x20 | Column count: 2 in all three. |
| 0x68 / 0x69 | 0x20 | Overall width / height in EMU. |
| 0x6B | 0x70 | Sequence number of the type-0x63 cell-list record. |
| 0x6D | 0x90 | Dimension list: first `columns` entries describe columns, then `rows` entries describe rows. |

Each dimension entry is kind 0x88, with key 0x01 the cumulative ending position
and key 0x02 its size, both kind 0x20 in EMU. Columns are 1828800 EMU (144 pt)
wide; rows are 457200 EMU (36 pt) high. Width is 3657600 EMU (288 pt); heights
are 914400 / 1371600 / 914400 EMU. Validate positive dimensions, cumulative
positions, declared totals and bounded counts before allocating a grid.

## Type-0x63 cell list and merges

Key 0x01 / kind 0x18 gives the number of actual cell records (4 / 6 / 3).
Key 0x02 / kind 0xA0 is an array of kind-0x88 cell records. Numeric fields:

| Keys | Meaning |
|---|---|
| 0x01 / 0x02 | Inclusive first / last row, zero-based, omitted zero. |
| 0x03 / 0x04 | Inclusive first / last column, zero-based, omitted zero. |

The ordinary grids enumerate cells row-major. The merged sample instead lists
(1,1,0,0), (1,1,1,1), (0,0,0,1): bottom-left, bottom-right, top row spanning
two columns. Preserve coordinates, not array index, to place cells. Compute
spans as inclusive end minus start plus one. Covered grid slots remain blank
in DesignCraft. Reject out-of-range, overlapping or incomplete cell rectangles.
A row-spanning example is absent; the same inclusive rectangle representation
can be exercised synthetically, but is not independently confirmed by a file.

Other repeated cell fields are 0x09 = 228180, 0x0A–0x0D = 36576, and 0x0E =
114300. Their precise meanings are not confirmed by differing samples; do not
use them to invent fills, borders, padding or alignment.

## Quill cell text: `TCD `

All three files have exactly one story and one `TCD ` section. At section
position 0 is a u32 equal to cell-record count minus one. At positions 4 and 8
are 0 and 0xFF00 (meaning unconfirmed). Starting at position 12 are `count+1`
u32 text boundaries in UTF-16 units within the story:

- 2×2: 2, 5, 8, 12; TEXT is `A1 CR B1 CR A2 CR B2 CR`.
- 3×2: 0, 1, 2, 3, 4, 6; TEXT is six CRs (empty cells).
- Merged: 0, 1, 3; TEXT is three CRs (empty cells).

Intermediate boundaries point at the separating CR (excluded from cell text).
The next cell starts one unit later. The final boundary is the story length,
including its final CR, which is stripped as usual. Validate separator units,
monotonic boundaries and exact final length. Do not split on every CR: cells
may contain paragraph breaks, and only TCD boundaries are cell separators.

For the unmerged 2×2 sample, cell text order and cell-record order are row-major,
confirmed by `A1`, `B1`, `A2`, `B2`. All merged cells in the available fixture are
empty: text association for nonempty merged cells is **not confirmed**. Preserve
such unassigned text after the table with a warning, rather than guessing which
merged cell owns it. Association across multiple stories/TCD sections is also
unconfirmed; retain text and warn when that association is ambiguous.

## Cell fills and borders: unconfirmed

The three fixtures have no independent cell-fill or border-setting variants.
Their Quill `MCLD` sections contain one formatting block per cell (130 bytes per
block in these files). Block fields include 0x00–0x03 values near 0x1F00 and
0x06–0x09 = 36576; no differing setting establishes their interpretation.
Escher supplies table-frame line properties but does not establish per-cell
formatting. Do not reinterpret the unknown MCLD integers as RGB colours, line
weights, or swatch indices. Import confirmed geometry, merges and text; use
unpainted cell fills/edges and warn explicitly that cell fills, borders and
other cell formatting were not decoded. In particular, clear DesignCraft's
normal default black borders rather than inventing them.

## Native mapping and limits

DesignCraft `Table` has row heights, column widths, a row-major cell grid and
inclusive-region spans represented as top-left owners plus row/column spans.
Each cell owns a frameless `Story`; keep supported character and paragraph
formatting from the imported story when cutting cell text. A table is anchored
in its host story with `Story::insert_table`, rather than a set of text boxes.
Use at most 256 rows/columns, 10000 grid slots and 10000 TCD cells; reject
malformed dimensions/rectangles/ranges. Bound total formatting-copy work too.

Across a publication, cap parsed/linked/native grid cells at 100000, table text
processing at 16000000 UTF-16 units, and formatting-copy work at 1000000 steps.
These aggregate caps also guard against repeated references to the same records.


## Row positions from stored row endings

Eight of the real-world samples (`Sample*.pub`, Apache POI test data at
`bf7c9b6a1490046517cbd8e1504e702a89be4bc9`) store row endings that differ from
the running sum of row sizes. Their drawn table frames are 657564 EMU high,
matching the last ending; the three controlled table samples match both values.
So row positions come from the stored endings. Why the quantities differ is not
known.

For rows, use consecutive key-0x01 endings, beginning at zero, to derive native
row heights/positions. Leave the native row-height mode at its previous default
(`AtLeast`); the stored data does not establish a fixed-height mode. Require positive strictly increasing endings within the
signed coordinate range, and positive bounded derived heights. Keep the
existing positive/bounded key-0x02 sizes, checked size sums, dimension/cell
counts, grid bounds and overflow checks. Size sums must still equal declared
width/height; **the last row ending need not equal declared height**. Columns
retain their existing size-based layout, cumulative validation and exact
size-sum/declared-width check. No column-ending exception is established here.

If any row ending differs from its running size sum, emit one bounded warning
per affected table: stored endings were used for row positions, the largest
absolute difference in EMU, and “cause unknown.” Do not infer a cause. No new
anchor key, unit or group-transform rule is added. Why endings differ, whether
3188 is constant, any relationship to a border, and real tables with unequal
row heights remain unexplained. Synthetic checks do not establish new sample
encodings.

## One table story among several

When a publication has several stories, one table and one `TCD ` section, the
table's text is recovered only if the `Contents` link (key 0x27) names the
story, the `TCD ` boundaries match that story and no other, and the cell records
are row-major. Otherwise the text is kept after the table with a warning; no
story is chosen by order and the directory occurrence field is not interpreted.
