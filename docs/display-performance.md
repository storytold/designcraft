# Display performance for image-heavy documents

Use **View › Display Performance › Typical Display** for lower-resolution screen images, or
**Fast Display** for grey artwork boxes. High Quality retains sharp images at larger zooms.
These view choices do not reduce the resolution of original artwork or PDF export.

Decoded artwork is shared between canvas and page-thumbnail renderers. Requested previews are
looked up before loading full-resolution pixels; larger cached previews can supply smaller ones.
The full-pixel cache has a 192 MiB budget, with a separate 96 MiB preview budget. Canvas and
thumbnail renders trim old original pixels after drawing. Active renders can temporarily hold
additional pixels; the budgets describe cache ownership, not a process memory limit. Closed
files are not kept alive by the image cache.

On desktop, disposable raster/PDF previews live in the profile's `Previews` directory (including
isolated `DESIGNCRAFT_CONFIG_DIR` profiles). Cache keys contain a version, SHA-256 of original
bytes, PDF-page index, mip level and active colour settings. A renderer/dependency fingerprint invalidates previews
when decoding code or pinned dependencies change, including development builds. Preview files preserve premultiplied pixels exactly and
have checksums; corrupt, missing or unwritable caches fall back to originals. The disk budget
is 512 MiB. PDF dimensions come from the pinned rasterizer without drawing the page, so cached PDF
previews can be reused immediately. SVG previews remain in memory because they can depend on
document or installed fonts. CLI and web have no disk cache unless a native host enables it.

Page thumbnails render on a separate native worker, one visible page at a time, prioritizing
pages closest to the current page. Old revision results cannot replace new thumbnails. The texture cache retains at most 128 thumbnails. Local
changes invalidate the affected spreads; global styles, parents, page structures and asset
changes conservatively refresh all. Browser/no-thread fallback renders at most one thumbnail
per frame. A source replacement gets new preview keys automatically.
Document-page thumbnails follow the Pages panel's size and Show Thumbnails options; only
visible document rows request background rendering. Colour-setting changes invalidate decoded
image pixels and document-page thumbnail textures.

This is demand loading of **decoded pixels**. Native linked documents still read and retain
encoded original files on opening, so source status, export, packaging, undo and self-contained
recovery keep their existing semantics. Fully lazy original-file reads require a separate asset
storage design and are not provided by these screen-rendering changes.

`ui.inspect.perf` exposes image-cache bytes, decode/preview/disk counts and thumbnail rendered/
reused counts. `cargo run -p designcraft-ui-egui --example perf_calendar -- DOCUMENT [CACHE_DIR]`
measures interface stalls, navigation settling and two all-page render passes on a local copy.

## Image-frame dragging

Native views can prepare three disposable textures when a single top-level graphic is selected:
the paint stack below it, its cropped image/frame, and the stack above it. Translation moves only
the middle texture, keeping foreground text and normal transparency in order. Preparation runs
on a separate single-thread worker, with draft motion until the layers are ready; each texture is bounded to 2048 pixels per side. A move does
not rewrite the artwork or replace its original bytes. Snapping and frame overlays use live
engine geometry; commit/cancel/undo use the existing interaction transaction.

Backdrop-dependent effects, non-normal blends, live text wrap and nested/copy/cross-spread
moves, or an active document grid, cannot use the three-layer shortcut. Compatible small changes keep full-quality partial
redraws. Large or unbounded changes, proof/ink-limit views and the browser's no-thread path
use a reduced-resolution canvas when a complete repaint is needed during the interaction.
Full-quality pixels are restored after release, asynchronously on native views. This affects
screen previews only. Complex cases retain their real rendering semantics in the draft and final
result instead of flattening blends against the wrong background. Existing page thumbnails stay
visible during the transaction; committed edits then invalidate the affected spreads.

Superseded native canvas jobs check a generation token between items and around rasterization.
An image decoder already in progress completes before the next cancellation checkpoint. Cancelled
jobs never publish a partial frame. `ui.inspect.perf` reports prepared/reused/preparing drag frames
and whether the current canvas is a temporary draft. `perf_drag` exercises page 4's large graphic,
commit, cancel and undo on an in-memory copy; all benchmark timings must state the build profile,
pixel ratio and thread count. Release and development builds should be compared on the same file.

Native canvas reuse also checks the texture pixel size at the current display scale. A Retina,
monitor or UI-scale change refreshes the texture before partial updates; every patch must fit
the actual GPU allocation. Logical viewport coverage alone does not establish pixel compatibility.

On macOS the native app uses eframe's nonreturning event loop. Its save/on-exit hooks run before
termination, avoiding a late AppKit Touch Bar observer exception after the final window closes.
Native validation covers presented GPU frames across UI-scale changes, editing and a clean exit.

## Scrolling previews

The canvas keeps disposable 128-pixel spread previews under its sharp viewport texture. Newly
exposed regions show a coarse page image while a full-quality pan or zoom render is pending.
Cold pages show paper until their preview arrives, rather than exposing the black page shadow.
Only actual page areas receive paper/previews; facing-page gaps remain pasteboard. Originals,
layout geometry, saved documents and export resolution are unchanged.

Each canvas retains at most 32 preview textures (at most 2 MiB of pixel data), requesting visible
spreads followed by their immediate neighbours. A separate single-thread native worker handles
one request at a time, with no navigation backlog. Browser fallback processes one request per
frame. Document revisions, document switches, parent editing, display quality and proof/ink views
invalidate the cache; superseded results cannot reappear. New previews pause during an active
interaction. Proof, plates and ink-limit treatment match the canvas; High Quality previews use
Typical Display resolution until sharp pixels arrive. Fast Display keeps its intentional boxes.

`ui.inspect.perf` includes `scrollPreviewsRendered` and `scrollPreviewsReused`. For reproducible
visual checks, the `ui_shot` example accepts `{"holdSharpCanvas":true}` after a sharp frame has
rendered, retaining the old texture as if a viewport render were delayed. Set it to false to
resume rendering. This is a test-harness control, not a product option.

A canvas already sampled down to the GPU texture limit is reused at rest; its expected pixel size uses the same fitting calculation as upload. Edits still reject full-resolution partial uploads into a reduced texture and repaint it whole.
