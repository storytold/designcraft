# Inset columns

An inset frame holds its main story and commentary series in a shared area. Live outside
commentary keeps its edge; the main text takes adjacent space when a commentary finishes.
If the main text ends first, its neighbours widen to its last centre axis, with half a gutter
on each side. The last stream uses full width.

Create commentary series with `footnote.series.create {name, placement:"stream"}`, insert
notes into the main story with `footnote.insert {series,text}`, then set the main frames with
`textFrame.options` or `object.textFrameOptions`:

```json
{
  "ids": [23],
  "insetColumns": {
    "streams": [{"series": 1}, {"ratio": 2}, {"series": 2}],
    "gutter": 12,
    "catchwords": true,
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

`mainStartLines` uses the main text's initial line metrics. Optional `mainStartBaseline` overrides
it and aligns the first main baseline with commentary baseline n, counted from one at the frame
top. The nearest live stream on the spine side supplies the baseline; otherwise the nearest
outside stream does. Both fields accept integers up to 65536. Left and right insets are independent;
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

Commentary left after the final frame sets the same story overset signal used by document
inspection and preflight, even when all main text fits. An unplaced column-footnote
continuation also retains that signal after the accepted page slices are replayed.

A stream series outside the inset's stream list can use standalone `noteStream` frames.
Anchors feed the route on their page, in frame order, so a narrow column can continue into
a full-width foot frame. These routes participate in the same page trials and retain their
continuations independently of inset bands. Unplaced route commentary also sets story overset.

A route can follow the composed inset core on its host's page:

```json
{"noteStream":{"story":23,"series":2,"followInset":{"edge":"start","gap":12}}}
```

`followInset` also accepts an array of one or two rules with distinct edges, for example
`[{"edge":"start","gap":0,"after":45},{"edge":"end","gap":80}]`.
Both bounds apply to the same frame, each using its own gap and optional dependency.
Duplicate edges and arrays longer than two are rejected. Bounds that meet or cross
produce an empty area with the same continuation and overset behaviour as a single rule.

The core bottom includes the inset main text, commentary, and reserved catchword rows.
When a starting routed foot follows a catchword-bearing edge, its first baseline is the
catchword baseline plus that row's leading and the follow gap. Its column still starts
at the followed row's bottom.
`edge:"end"` caps the route's bottom at core bottom plus gap; `edge:"start"`
raises its top to at least that position, keeping its bottom. The frame's original area
limits both adjustments. Set the foot frame's original top high enough to allow it to follow
a shorter core upward. Each candidate page trial composes routes with these adjusted areas,
and the final composed columns retain them. Closed areas place no text; notes continue into
the next route area or report overset. A page without a host inset frame keeps the original
route area. `gap` must be finite and within 0..15552 points. Invalid gaps are rejected by
commands and the follow option is dropped on native load. Native round trips preserve valid
options; IDML warns that it cannot retain `followInset` geometry.

Following is measured in spread space. The host's transform maps its local core bottom into
spread space; after adding `gap` in spread-space points, the route's inverse transform maps
that edge into its local text area. Translation and positive axis-aligned scaling are supported,
including containing-group transforms. Rotation, skew, or flipping on either frame makes
`followInset` a no-op for that route and adds a composition warning.

An optional `after` frame id raises the reference bottom to the greater of the core bottom
and that route frame's composed bottom, before adding the gap:

```json
{"noteStream":{"story":23,"series":2,"followInset":{"edge":"start","gap":12,"after":45}}}
```

The reference must name a route frame for the same host story on the same page, composed
earlier in this fit. Missing frames, other pages or stories, later frames and cycles make
the entire follow adjustment a no-op with a composition warning. Native saves preserve
`after`; IDML warns that it loses this dependency along with the follow geometry.

Each physical page's routes are fitted once using all host ranges on that page, including
a thread that returns to an earlier page. Chained followers use already fitted route bottoms.
Copying the host and its route chain retargets `after` to the copied frames.

`after` also accepts a host frame and margin series area:

```json
{"noteStream":{"story":23,"series":2,"followInset":{"edge":"start","gap":12,"after":{"frame":40,"series":3}}}}
```

The frame must belong to the same host story and page; the series must use margin placement
with `stack: "top"`. This explicit area replaces the inset-core constraint; numeric route
references still clear the greater of the core and referenced route. The reference bottom is the area top plus accepted entry heights and
inter-entry spacing, or the area top when empty. Fitting and placement share this calculation.
The area starts at the host's first composed line top, including the first replayed inset band.
A host with no composed line uses its text-frame area top.
Each frame/series top stack remains fixed during the final page-margin collision pass.
Other margin modes, missing frames or series, and other stories or pages make the entire
follow adjustment a no-op with a composition warning. Native saves and commands preserve both
forms. Copying remaps both frame and series ids; IDML warns that either dependency is lost.

The page fitter uses full-width main line ends as search keys. Each isolated trial lays out
fixed page slices and rounds its cut to a whole main line from that layout. It retains the
longest cut actually measured feasible, with that exact geometry, rather than assuming
feasibility is monotone. Search plus the progress probe has a budget of ceil(log2(keys)) + 2
probes. The full-page key is tried first so nonmonotone feasibility cannot hide a full-page fit.
After the midpoint search, unprobed keys above the best fit are checked in descending
order within the same budget, including when no fit has succeeded yet. If verification cannot finish, composition retains the best
proven cut and reports a trial-budget warning.
Each probe may re-lay once to resolve its whole-line cut. An incoming commentary tail
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

`catchwords:true` adds a generated catchword below each stream that has visible text on a later
page. A stream reserves and prints its row only at its last text-bearing host on that physical
page, including when a thread visits the page more than once. The word follows that stream's last rectangle at the paragraph's line end, left for
right-to-left paragraphs and right for left-to-right paragraphs. Its 18-point row is part of the band geometry, so another stream cannot
occupy it. Intermediate width changes do not add rows. Catchwords use the stream's font family
and colour at 9 points; a long word shrinks uniformly only to fit its column width.
Row height never scales the catchword's glyph size. Alternatively,
`catchwords:{"style":"Catchword"}` applies that character style. Its baseline is the
preceding line's baseline plus the style's leading. An unsized style inherits the source stream's resolved size and leading at the catchword position. The reserved row starts at the
preceding line's descent and ends at the catchword's descent. Boolean `true` retains
the existing placement and 18-point row. An unknown style produces a composition warning and uses the
9-point size, 12-point leading and 18-point row defaults. Boolean `true` and `false`
remain valid in commands and native files. IDML warns that it loses styled catchwords.
Host, route and series catchword style references follow character-style rename and
delete-with-replacement, including undo.
Ended and overset
streams show no catchword. Conditional visibility and tracked deletions determine which source words are shown. Generated words leave editable source ranges and caret positions
unchanged, and are included in native rendering and PDF export.

Routed frames opt in with `noteStream.catchword:true` (default `false`, omitted when false
in native files). The host inset's catchword setting supplies their style and must also
be enabled. `noteStream.catchword:{"style":"Route caption"}` overrides the style for that route.
Series options accept `catchwordStyle:"Series caption"` to override the host style; a route style takes precedence.
Only the last opted-in route frame of each series on a page reserves a row
directly below its last composed line. Native saves preserve the opt-in flag; IDML warns
that it loses it. A
`followInset.after` follower clears that row. Its word comes from that series' next
visible page; a finished series has no catchword. Side notes outside a stream's horizontal
range do not lower its catchword row. Followers clear every inset stream's catchword row.
When a neighbour widens into a finished stream's column, its widened lines start below
the finished stream's full catchword row, including the catchword descent. This applies
to commentary and main-text widening while preserving their baseline progression.

A separate text-variable frame can use
`variables.define {name:"Inner",type:"nextPageFirstWord",story:11,series:1}`. A routed `frame`
source supplies its host and series automatically. Without `series`, existing main-story and
default-source behaviour remains. A series source requires a host; deleting the series moves
its catchword binding to series 0 with its notes. A bound source cannot contain running-header or catchword markers; commands reject that cycle before changing the document. IDML exports all catchwords as empty
custom-text variables and reports the loss.

The implementation is in `crates/compose/src/inset`: pure geometry, bounded page trials and
real-text regressions. Tests cover the issue's numeric layouts, mirrored and empty regions,
positioned starts, mixed leading, overlap, source continuity over pages, progress overflow,
nonmonotone search and an edit on page three that leaves earlier pages unchanged.

The catchword source extends #2870; the geometry and placement complete the corresponding
slice of [#2873](https://github.com/storytold/designcraft/issues/2873).
