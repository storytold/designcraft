# Legacy native text baselines

Native format 1 used the font ascent for the Ascent first baseline, and fixed multiples
of that ascent for Cap Height (0.72) and X Height (0.5). Opening those documents now pins
that calculation per text frame, including parent and nested frames. New documents retain
measured typographic ascenders and cap/x heights. Changing a frame to another First Baseline
option adopts current metrics; unrelated frame edits retain compatibility, and Undo restores it.

A saved document with a legacy baseline requires reader capability 6. Versions through 5
reject it rather than silently moving text. Linked originals and proxies retain upstream's
format-5 storage semantics. Documents without legacy baselines retain the older capability
required by their contents. Format-1 schema upgrades still run through the shared codec.

Formats 2 through 5 cannot reliably distinguish layouts created before and after the metric
change. They are not automatically marked legacy: doing so would move modern documents.
