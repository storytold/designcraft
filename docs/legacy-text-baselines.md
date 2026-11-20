# Recovering legacy native text baselines

Earlier composition used the font ascent for Ascent and fixed multiples for Cap Height
(0.72) and X Height (0.5). The public history shows this in the parent of 84361ca9 and
in the removal in 0f4f382b. However 84361ca9 and c246f1d3 changed those metrics while
native format 1 and app version 0.2.1 were still in use. Neither field reliably identifies
old positioning. Opening a format-1 file therefore does not automatically change metrics.

For an older layout that needs those offsets, select its text frames and enable **Use
legacy first-baseline metrics** in Text Frame Options → Baseline Options. Automation uses
`object.textFrameOptions {ids: [...], legacyFirstBaseline: true}`. This is undoable;
false restores current metrics. Choosing a different First Baseline option also restores
current metrics; unrelated edits retain compatibility. New documents use current metrics.

Parent and nested frames have their own flag. Table cells, nested tables and footnotes
inherit the containing frame’s flag; note caches distinguish it. Table continuations
recompose their cells and repeated headers/footers when the destination frame uses
different metrics; fragments already placed in earlier frames retain their layouts. A row group
that is too tall for an empty frame skips ahead to later frames before it is marked overset;
headers are emitted only with the first body fragment and repeat only on continuations. A flagged native save
requires reader capability 6. The shared `craft-designcraft-format` codec owns that
capability, inference and the bundle reader/writer. The app does not duplicate its ZIP
specification or add runtime JSON/ZIP dependencies. Linked originals and proxies retain
format-5 storage. Wide unflagged documents do not require capability 6 merely because they
contain more than one million values. The existing version-4 feature-scan fallback remains;
data nested beyond the legacy scanner’s 256-level depth limit conservatively requires 6.
The legacy scanner retains only ancestor iterators, bounding memory independently of width.

Duplicate IDs are repaired by the existing loader without using them to infer text metrics.
Vertical Em Box, Leading and Fixed are unchanged invariants, not regressions fixed by this
compatibility branch. The horizontal Ascent/Cap Height/X Height and nested-text regressions
must fail when their compatibility paths are disabled.

Test-only macOS shortcut/optional-font and CLI pipe prerequisites are independently proposed
in #2549 and mirrored here until it lands; production input and fonts are unchanged.
