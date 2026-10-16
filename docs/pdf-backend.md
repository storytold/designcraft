# PDF writing boundary

DesignCraft pins `craft-pdf` and its patched Krilla 0.8.2 backend to craft-libs revision
`c3487320b2d1de657b860b3c8abb44e1f2b32bd8`. The `write-pdf` capability is required for
placed PDF pages and book merging. The ordinary reader dependencies, `craft-ui` and
`craft-print` revisions remain unchanged by this writer migration.

The shared library supplies fallible font copying/preparation, metadata setup, version and
archival configuration, final serialization and a native unwind guard. DesignCraft retains
layout, shaping, font choice/licensing, fallback warnings, image caches, links, tagging,
bleed/marks, PDF/X postprocessing and export presets. A shared writer does not improve the
format-conformance or product-parity claims by itself.

`export_pdf_with_report`, `export_booklet` and `merge_pdfs` guard the whole export, including
painting and postprocessing. A native drawing panic becomes `PdfError::Write`. A failed
page or surface cleanup permanently poisons the backend writer, preventing successful
serialization of an incomplete document after a narrow catch. Discard other captured app
state after an internal failure. These guards do not catch WASM aborts, allocation aborts,
or errors in preceding application operations such as print preparation.

Cargo applies patches only from the consuming workspace root. Keep the Krilla patch and
facade at the same full immutable commit, including Krilla reached through `krilla-svg`.
`cargo xtask pdf-deps` verifies their resolved versions, source and uniqueness, and runs in
`cargo xtask ci`. Publish the dependency revision before publishing this consumer branch.
Local path replacements are suitable only for development and should not be committed.

The vendored Krilla patch ledger, per-file hashes, licence texts and four unchanged CC0
Compact ICC profiles live in craft-libs (`vendor/krilla`, `licenses/pdf-writer`, and
`ATTRIBUTION.md`). No fonts or binary PDF fixtures are added to DesignCraft by this migration.

Tests cover metadata key presence, hostile page selection, successful PDF export and native
subprocess recovery for all three export entry points. Native/WASM export comparisons and
independent PDF parsing/rendering supplement the repository gates. Physical printer output,
Windows/Linux runtime and PDF/A/PDF/X certification remain separate acceptance work.
