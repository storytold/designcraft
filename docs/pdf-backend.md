# Shared PDF backend

DesignCraft pins `craft-pdf`, patched Krilla 0.8.2 and the four Hayro reader crates to
craft-libs revision `89dcbb3060377a26c99a6d80caa16084366704c4`. Existing `craft-ui` and `craft-print` revisions
remain unchanged. The reader migration is reviewed on top of the separate writer change.

## Writing

The `write-pdf` capability is required for placed PDF pages and book merging.

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

## Reading

Placed-PDF geometry, layers and raster output use `craft-pdf` with its explicit `raster`
feature. The facade and four patched Hayro crates are pinned to one immutable craft-libs
revision. The backend source was transferred from PdfCraft revision
`7437bde378f43856eada40af25b983812e615e8a`; the per-crate `vendor/SOURCE.json` manifests and `vendor/README.craft-reader.md`
record the transfer and subsequent craft-libs patches. Asset provenance and hashes
are recorded separately in ASSETS.md.
`cargo xtask pdf-deps` checks unique resolved packages, exact versions and sources.

DesignCraft retains placement, layer-name policy, document state and the application render
cache. Render output is premultiplied RGBA8. Page boxes use the same crop, rotation and
UserUnit transform as raster output, including fractional placed-PDF dimensions.
Layered-PDF export stops on failed layer rewriting, missing pages or reported incomplete
content rather than saving an unchanged or incomplete placed page. Shared diagnostics only
report known backend warnings; they do not certify all PDF fidelity.

DesignCraft uses a 512 MiB reader input bound, checked before parsing or copying.
Raster output has shared geometry/allocation bounds. Native unwind guards turn escaped
backend panics into errors; WASM aborts and process-wide allocation failures are not recoverable.
Application fonts, image assets, editing and export presets remain app-owned. Existing
craft-ui/craft-print versions are preserved. The reader migration builds on the independently qualified writer change.

The shared commit must be published before a fresh public checkout can resolve these pins.
No sibling checkout is required. Licences and existing backend asset notices are recorded in
NOTICE, ASSETS.md and licenses/pdf-backend. The backend's removed LAB.icc is not restored.
