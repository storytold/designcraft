# Placed graphic layer options

IDML `GraphicLayerOption` / `GraphicLayer` settings are retained on each
`Graphic.layer_options`, not on the shared asset. Two placements of one linked
file can therefore keep different layer choices while sharing image bytes.
Scalar option/layer attributes, layer order and parent-child relationships, layer IDs/names, original/current
visibility, link-update policy and view/export/print flags survive import,
native save/reopen and IDML export. XML Self IDs are regenerated. Native-edited
attribute maps cannot inject XML syntax, declare namespaces or override those Self IDs.
Parent indices are optional in native files; missing entries retain the older
flat representation. Invalid/cyclic parent indices, parents in closed subtrees and over-budget nesting
are emitted as top-level layers, retaining their scalar metadata without
unbounded XML recursion.

This is preservation, **not layer-aware rendering**. The current renderer and
PDF exporter use the asset's composite/default appearance and do not apply
these per-placement choices. Changed visibility and explicit view/export/print
states produce an import warning, a persistent `unsupportedGraphicLayers`
preflight issue, and a PDF export warning. Nested graphic cells, anchored
objects and parent items remain detectable after native reopening, including
when spread items also produce warnings. PDF warnings inspect the source
document so effect rasterization and transparency flattening cannot hide them.

Native files containing these settings require format version 4. Earlier
readers reject that version instead of loading and silently discarding the new
field. Documents without layer settings retain their existing version 2/3
requirements. Older graphics without `layerOptions` still deserialize with
`None`, and their JSON representation is unchanged.

Version 4 is a single reader capability shared with explicit all-on placed-PDF
layer choices (#1671): a version 4 reader must understand both, so the two
changes ship in the same release. One function in the format crate,
`required_version`, chooses the version for every save.

Synthetic regressions cover independent overrides on two placements sharing
one asset, metadata round trips, native reader requirements, nested cells,
nested layer hierarchy, preflight/PDF warnings after rasterization and flattening,
and invalid native attribute names and parent indices. Private originals
and artwork are not repository fixtures; any collection results are local
metadata checks, not an independent reference-rendering or fidelity oracle.

Local collection check (2026-10-23): a five-document pilot passed before
expansion to 205 previously converted documents. All 205 completed IDML →
native save/reopen → IDML; all 1,113 layer records across 68 documents matched
their input scalar attributes and order, ignoring regenerated Self IDs.
Persistent preflight warnings appeared in the 57 documents with overrides.
The private source files, layer names, document paths and per-file logs remain
outside this repository. Original INDD files were not opened by this check.

Remaining work: apply visibility to PSD layers/PDF optional content, implement
layer-comp choices and a layer-options UI, and validate appearance against
verified same-save reference PDFs. Gap 19 remains open.
