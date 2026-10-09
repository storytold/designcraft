# designcraft-affinity

Opens native Affinity documents in DesignCraft: `.afpub`, `.afdesign` and `.afphoto` from
Affinity 1 and 2, and `.af` from Affinity 3 (container versions 8 to 12). It is read-only:
nothing is ever written in Affinity's format, and an opened document starts unsaved, so Save never
replaces the Affinity file.

The crate has two parts:

* **The reader** (`container`, `stream`, `model`, `paint`, `geometry`, `shapes`, `text`,
  `raster`, `preview`): a bounded reader for the archive, the tagged object stream in `doc.dat`
  and the document model. It is a copy of `vectorcraft-affinity` from
  [storytold/vectorcraft](https://github.com/storytold/vectorcraft) at commit
  `35a6015d88e62790bb5e96c8a6b445f31762c9e9` (PhotoCraft has its own copy, `photocraft-affinity`);
  improvements should go to all three.
* **The mapping** (`import`, DesignCraft's own): turns that model into a DesignCraft document.

Commands: `file.openAffinity {path | base64, name?}`; `file.open` and `file.openBytes` hand
Affinity documents to it. File › Open, drag and drop, the web build and `designcraft-cli --in`
accept the four extensions.

## What is imported

| Affinity | DesignCraft |
|---|---|
| Publisher spreads (one or two pages) | spreads with left/right pages, in points |
| Documents without pages (Designer, Photo) | a page per artboard, or one page for the canvas |
| Top-level layers | document layers, by name |
| Nested layers, groups | groups |
| Curves, rectangles, ellipses, polygons, stars, compound shapes | frames with their outline (even-odd outlines keep their holes) |
| A shape with children, a vector mask | a frame with the children pasted into it (clipped) |
| Solid fills and strokes in RGB, CMYK and grey | unnamed colours in the same space; CMYK stays CMYK |
| Linear and radial gradients | gradient swatches placed with their vector |
| Stroke weight, alignment, caps, joins, miter limit, dashes | the same |
| Artistic text | a text frame whose first baseline sits at Affinity's anchor |
| Frame text, also rotated | a text frame with one story |
| Font family, weight, italic, size, tracking, horizontal scale, fixed leading, colour, all caps, OpenType features | character overrides |
| Paragraph alignment, left/right/first-line indents, space before/after (Affinity keeps the larger of two neighbouring spaces), fixed paragraph leading | paragraph overrides (spacing converted so the result matches) |
| Automatic leading: artistic text 100 % of the size, frame text the font's own line height (ascent + descent) | the paragraph's auto leading |
| Type size: artistic text scales with its node; frame text and tables only with their text scale (`FTxS`), never with their frame's or groups' transform | font size and horizontal scale on the characters |
| Text whose characters share one transparency | a text frame with that opacity |
| Tables: grid, cell text, cells merged across columns, vertical alignment, insets, the line on every cell edge | a text frame holding a DesignCraft table (rows grow only where the fonts need it) |
| Placed images (the embedded original), pixel layers | graphic frames with embedded images |
| Pixel masks on images, images inside pixel layers (clipped to them) | worked into the image's transparency |
| Embedded Affinity documents and symbols (`EmbN`) | their content as editable objects, grouped; nested at most four deep; the cached picture only when the document can't be read |
| Opacity, visibility, lock, names, blend modes DesignCraft has | the same |

Everything that is not imported, or only approximately, is listed in the open result's
`warnings`, one line per kind with a count. Not yet imported: master pages, text threading
between frames, hyphenation settings (imported paragraphs don't hyphenate), text fields such as page numbers, inline objects
in text, table cell fills, cells merged across rows, layer effects, adjustment layers and live
filters, pixel masks on objects other than images, brush strokes, embedded files other than
Affinity documents without a cached picture, Lab colours (converted to RGB) and Affinity-only blend modes. Pages larger than 216 in, DesignCraft's largest page, are refused
with that reason.

## Provenance and clean-room

Affinity has no published file-format specification. The reader was built only from a public
description of the format and from public files, never from Affinity itself:

* **No Affinity software.** Affinity was never run, scripted, screenshotted or disassembled for
  this work, and nothing from an Affinity installation (program code, resources, presets, fonts,
  colour profiles) was read. No file was made with Affinity for it.
* **A public, permissively licensed description.** The container and object-stream layout was
  learned from [VMDevCpp/afread](https://github.com/VMDevCpp/afread) (MIT, at
  `04b672334a43e3e37ded6b5ffc57af231d589774`, written for container versions 7–11) and
  re-described in our own words before the Rust code was written; no code was translated. No
  GPL/AGPL code (such as Inkscape's Affinity extension) was read.
* **Embedded documents**: an `EmbN`'s `EmbC` archive entry holds `EmDc`, four zero bytes and a
  complete Affinity document, whose pixels are the node's units; with `PBBx` 2 the centre of its
  content is the node's origin (as for the cached pictures), otherwise its page's. Learned from
  the structure of documents their owner made earlier, confirmed against their thumbnails.
* **Paragraph indents and spacing** (`Doub` slots 2–6 of a paragraph's attributes) and frame
  text's automatic leading were identified on three public MIT documents, `tiny-text-indent.af`,
  `tiny-text-para-spacing.af` and `tiny-text-frame.af` from
  [SethRobinson/Patchy](https://github.com/SethRobinson/Patchy) at `de84eab`, by measuring their
  thumbnails; only their document data was read (sha256 as in VectorCraft's corpus manifest).
* **Tables, paragraph alignment runs, paragraph leading, artistic text's automatic leading, the
  text scale `FTxS` and horizontal text scale** were added in DesignCraft from
  the structure of documents their owner made earlier (field names, types and counts, read
  locally): a table's text holds one segment per cell, each ended by a break glyph of kind 4;
  `BrLf` marks a cell merged into its left neighbour; scaling a table node resizes its grid but
  not its type. Each was confirmed by comparing a render with the document's thumbnail.
* **Every structure checked against real files.** Every archive entry carries a CRC-32 that must
  match, and every field in the object stream names its own type, so a misread layout fails
  loudly instead of producing plausible wrong data. Meanings were fitted by comparing a render
  with the thumbnail every Affinity document embeds (Affinity's render of itself), on public
  documents (VectorCraft) and, for this mapping, on documents their owner made earlier and
  opened only locally.
* **Nothing of Affinity's is in this repository**: no program code, assets or documents.

## Tests

* `tests/import.rs`, `tests/text.rs`, `tests/outline_limits.rs` and the unit tests run on
  synthetic documents built with the `synth` feature. Those builders write only what this reader
  accepts; Affinity has never opened their output, so they are fixtures, not an exporter.
* `tests/real_files.rs` runs on real documents only when `AFFINITY_SAMPLES` names a folder of
  them (searched recursively) and skips otherwise. No Affinity documents are committed or
  fetched:

  ```sh
  AFFINITY_SAMPLES=~/Documents cargo test --release -p designcraft-affinity --test real_files -- --nocapture
  ```

  It checks that every document reads, imports into a valid DesignCraft document or is refused
  with a reason, and prints how often each warning came up.
* `fuzz/` has `cargo-fuzz` targets for the whole reader (`container`) and the object stream
  (`stream`): `cd crates/affinity && cargo +nightly fuzz run stream -- -max_total_time=600`.

## Safety limits

Every size is checked before it is allocated: 256 MiB per archive entry and 1 GiB per import by
default (`Limits`), a 64 MiB zstd window, 4096 saved revisions, array lengths no longer than the
bytes left, 16 777 216 decoded values per document stream (fields and array elements together,
counted before anything is allocated), 384 levels of object nesting, 128 levels of layers,
500 000 layers, four million curve nodes per curve; compound outlines also cap recursion at 128
levels, operand visits at 100 000 and cumulative geometry work at four million. Pixel layers are
limited to 64 megapixels; gradients to 1024 stops; text to a million characters per node.
Malformed input returns an `Error`; the crate has no panics outside tests.

## Privacy

Affinity documents can hold the folder they were saved in, user names, original image paths and
XMP metadata. This reader never imports those fields.
