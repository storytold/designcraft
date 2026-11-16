# Inset columns

Issue [#2873](https://github.com/storytold/designcraft/issues/2873) is split into pure geometry,
frame fitting, and per-stream catchwords. The geometry lives in `designcraft_compose::inset`.
The frame option and runtime integration are still pending #2872. No user command enables
this geometry yet.

`layout` receives fixed page slices as `Stream<C>` values, a frame rectangle, gutter, main
stream index, and a measurement callback. Streams are ordered from the spine outward. The
caller mirrors regions for `PageSide::Left`; single pages use the unmirrored geometry.
Paragraph direction does not affect region placement.

Initial columns divide the available width by stream ratios. Finished streams have neither a
column nor a gutter. When a stream ends, its adjacent neighbours absorb its region. The main
text takes adjacent released space on either side. When the main text ends between two
commentaries, those neighbours divide its region by their ratios. All other live columns
keep their positions and widths. The last stream gets the full frame width.

`MainPosition` resolves a delayed main start to a vertical distance and independent left/right
insets. Before that start the commentaries use the whole width. At the start they move into
the side regions. The caller computes the distance from the main text's initial line metrics;
the pure geometry does not infer font sizes or line leading.

The callback measures whole lines within the offered rectangle and returns used height,
completion, a new cursor and a placement. The cursor must retain source offsets, last baseline
and pending paragraph spacing. The rectangle top clips placement; it does not restart the
first baseline. Geometry first measures to the next start or frame bottom, finds the earliest
stream end, then measures all streams again to that end. Only the latter measurements are
committed. A line that cannot fit remains in its cursor for the next region.

There are at most one main-start event and one end event per stream. Input is limited to 64
streams, finite positive widths and ratios, and nonnegative finite gutters and insets. Invalid
geometry or callback heights return an error. Text-sized allocations and pagination belong to
the caller, not this module.

Unit cases in `crates/compose/src/inset/tests.rs` cover the issue's numeric examples, main
ending first, either commentary ending first, ties, empty streams, two/four streams, facing
mirrors, a positioned main start and independent baseline/cursor continuity. Real-text
composition, page-cut search, incremental fitting and UI proof belong to the integration slice.
