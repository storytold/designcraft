# Inset columns

An inset frame holds its main story and commentary series in a shared area. Live outside
commentary keeps its edge; the main text takes adjacent space when a commentary finishes.
If the main text ends first, its neighbours share that space. The last stream uses full width.

Create commentary series with `footnote.series.create {name, placement:"stream"}`, insert
notes into the main story with `footnote.insert {series,text}`, then set the main frames with
`textFrame.options` or `object.textFrameOptions`:

```json
{
  "ids": [23],
  "insetColumns": {
    "streams": [{"series": 1}, {"ratio": 2}, {"series": 2}],
    "gutter": 12,
    "mainStartLines": 10,
    "mainLeftInset": 210,
    "mainRightInset": 162
  }
}
```

The numbers are document-local ids returned by the create commands. A stream with no `series`
is the frame's own main story; exactly one is required. Ratios default to 1. Empty series have
no column or gutter. Streams are listed from the spine outward. Left pages mirror the regions
and insets; right and single pages use the unmirrored layout, independently of binding or
paragraph direction. Without positioning options, streams start at the top with ratio widths.

`mainStartLines` uses the main text's initial line metrics. Left and right insets are independent;
in a 600-point frame, 210 and 162 give a 228-point main block. Commentary fills the area above
that block and its available side areas. Once a side ends, main can extend across its previous
start edge. A surviving outside commentary keeps its column. `insetColumns:null` clears the
option. A separate `noteStream` route cannot also be an inset main frame. A UI panel is not yet
provided.

Each stream retains its source cursor and baseline state. Width changes do not restart
first-baseline padding or paragraph spacing. A line whose top still intersects the previous
band keeps its old horizontal span until it clears the ended neighbour, while its baseline
continues at normal leading. This prevents mixed-size lines from colliding at a width change.
Paragraph keeps treat same-page bands as one logical column; real page and column boundaries
retain their keep constraints.

The page fitter uses full-width main line ends as search keys. Each isolated trial lays out
fixed page slices and rounds its cut to a whole main line from that layout. It retains the
longest cut actually measured feasible, with that exact geometry, rather than assuming
feasibility is monotone. Search plus the progress probe has a budget of ceil(log2(keys)) + 2
probes; each probe may re-lay once to resolve its whole-line cut. An incoming commentary tail
may take a page with no main text only when it shrinks. An impossible start can overflow only
on the actual first main line; later anchors move with their main text.

Incremental traces store each page's incoming main offset and commentary state, outgoing cut
and state, accepted regions and note placements. A text or formatting edit resumes at the
first affected page. Re-fitting stops when the outgoing cut and continuation state rejoin the
stored trace and the remaining main text and formatting are unchanged. A changed note is
followed through its remaining continuation before rejoining.

Native saves retain the options. PDF and raster export use the composed regions. IDML cannot
retain inset geometry and emits an explicit export warning. Deleting a series removes its
entry from inset frames and moves its notes to series 0, as for ordinary stream routes.

The implementation is in `crates/compose/src/inset`: pure geometry, bounded page trials and
real-text regressions. Tests cover the issue's numeric layouts, mirrored and empty regions,
positioned starts, mixed leading, overlap, source continuity over pages, progress overflow,
nonmonotone search and an edit on page three that leaves earlier pages unchanged.

Per-stream catchwords are the remaining slice of
[#2873](https://github.com/storytold/designcraft/issues/2873), following #2870.
