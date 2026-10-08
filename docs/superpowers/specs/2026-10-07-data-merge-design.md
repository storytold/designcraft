# Data merge

Design for phase 1 of data merge. Approved in conversation on 2026-10-07.

Phase 1 extends the existing `data.merge` command. One template, one linked table, a temporary preview, then a new document. Each record gets a copy of the template, or the items on a single page are tiled.

Phase 2 is not part of this spec. It adds more than one source, JSON, filter and sort, and a drawn grid. Phase 1 stores a source id on every placeholder so that work can land later without rewriting placeholders. Tiling stays when the grid arrives.

## What exists today

`crates/engine/src/cmd/datamerge.rs` takes an inline `csv` string or `rows` array, copies one template spread once per record inside the same document, and replaces `<<Field>>` in story text. One undo step. There is no file on disk, no Excel, no pictures, no QR codes, no hyperlinks, no preview, and no tiling.

`crates/textimport` already reads the first worksheet of an `.xlsx` file as a table of cached cell values. Place uses that path. Data merge needs rows with empty cells preserved, so place keeps its own reader.

Pictures go through `file.place` and the Links panel. Hyperlinks are `hyperlink.create` (`url`, `email`, or `page`). QR codes are `object.qrCode` (vector path, types `url`, `text`, `sms`, `email`, `vcard`). Frame fitting is `Fitting` in `crates/doc`: `none`, `fillProportionally`, `fitProportionally`, `fitContentToFrame`, `centerContent`.

The engine `Session` holds a list of documents and an active index. A merge adds a document. It does not replace the template.

## Document model

New fields on `Document` use serde defaults so older files open. Empty data merge state is omitted from the save.

`DataMerge` on the document:

- `sources`: a list. Phase 1 has zero or one entry.
- `placeholders`: the list below.
- `options`: the last merge options, saved with the document.

`DataSource`:

- `id`
- `path`: absolute path, if the source is a file
- `relative_path`: path relative to the document file, set when the document has been saved
- `name`: file name, used when the source was passed as bytes and has no path
- `delimiter`: `comma`, `tab`, or `semicolon` (text only)
- `sheet`: worksheet name, or absent for the first sheet (Excel only)
- `fields`: `{ name, kind }` where `kind` is `text`, `image`, or `qr`
- `rows`: cached rows, each the same length as `fields`. Missing cells are `""`
- `fingerprint`: file size and modification time of the last successful read, if there is a path
- `status`: `ok`, `missing`, or `modified`

`Placeholder`:

- `id`
- `source_id`
- `field`: the name, without `@` or `#`
- `role`: `text`, `image`, `qr`, or `hyperlink`
- `anchor`: either `{ story, start, end }` in Unicode scalar offsets, matching `HyperlinkSource::Text`, or `{ item }` for a frame

`MergeOptions`, with these defaults:

- `records`: `all`. Also `one` with a 1-based record number, or `range` with a range string
- `per_page`: `single` or `multiple`. Default `single`
- `arrange`: `rows` or `columns`. Default `rows`. Used only when `per_page` is `multiple`
- `insets`: top, right, bottom, left, in points. Default 36, 36, 36, 36. Used only when tiling
- `column_spacing` and `row_spacing`: points. Default 0. Used only when tiling
- `fitting`: `fitProportionally` (default), `fillProportionally`, `fitContentToFrame`, or `none`
- `center`: bool, default false. Applied after fitting, by moving the graphic to the middle of the frame
- `link_images`: bool, default true
- `limit`: absent, or a positive count taken after the record selection

Preview is session state on the open document, not a field of `Document`. It stores the 1-based record being shown, or nothing. Saving writes the unfilled template. `data.preview.stop`, closing the document, and a successful merge all clear it.

## Reading a source

`data.source.select` accepts `path`, or `bytes` plus `name`. The name's extension selects the reader. `delimiter` (`comma`, `tab`, `semicolon`) overrides sniffing for text. `sheet` names an Excel worksheet. The first sheet in workbook order is the default.

Text is UTF-8. A leading byte-order mark is stripped. Any other encoding is an error that names the file. The command does not guess a legacy encoding.

Delimiter sniff, only when the command does not set one:

- `.tsv` or `.tab`: tab
- `.csv`: comma
- `.txt`: tab if the header line contains a tab; otherwise semicolon if that line contains a semicolon and no comma; otherwise comma
- anything else with text contents: the same sniff as `.txt`

Excel (`.xlsx` only):

- Cached values. The reader does not evaluate formulas. A formula cell with no cached value is empty.
- A date is the stored number, not a formatted date.
- Booleans are `TRUE` and `FALSE`. Whole numbers drop a trailing `.0`.
- Empty cells inside the header's width are `""`.
- `.xlsm` and `.xls` are errors.

Rows:

- The first row is field names, trimmed. An empty name is an error. A duplicate name is an error.
- A leading apostrophe on a header is ignored. Then a leading `@` sets `image` and a leading `#` sets `qr`. The mark is not part of the name. Any other field is `text`.
- Each later row is one record. A short row is padded with `""`. A row whose cells are all empty is skipped. Cells past the header are ignored, and the result's `warnings` names the row.
- More than 100,000 data rows or 200 columns is an error. The command does not allocate them.

Inline `csv` on `data.merge` and `data.fields` uses this same text parser and does not create a source. Inline `rows` is an array of objects. The field list is the key order of the first object. Later objects are read by those keys. A missing key is `""`. An extra key is ignored, with a warning. `@` and `#` on a key work the same way as a header cell. Non-strings are stringified with JSON's string form for numbers and bools, and rejected for arrays and objects.

`data.source.select` returns `{ fields, records, warnings }`. `fields` is `{ name, kind }`. `records` is the data-row count. After a file read, status is `ok` and the fingerprint is stored. On open, and before a merge, if the path is set: missing file sets `missing`; a different size or modification time sets `modified`. A merge uses the cached rows either way and adds a warning for `missing` or `modified`. `data.source.update` re-reads. With no path, update is an error. `data.source.remove` drops the source and leaves placeholders in place.

When the document is saved, `relative_path` is refreshed from the document's path. On open, a missing absolute path is tried again as `relative_path` beside the document.

## Placeholders

`data.placeholder.add` takes `field`, `role`, and a target. The target is the text caret (`story` plus `at`) or a frame (`item`). The field must exist on the document's source. Phase 1 has one source, and the placeholder stores that source id.

Defaults the panel uses, which the command may also set by omitting `role`:

- Caret in a story: `text`
- A selected frame and an `image` field: `image`
- A selected frame and a `qr` field: `qr`
- A plain `text` field on a frame is `image` only when `role` is `image`. The panel calls that Bind to frame.
- `hyperlink` is always explicit.

Inserting `text` writes `<<Name>>` at the caret, using the character style already at that point, and anchors the placeholder to that range. `<<` and `>>` are ASCII 0x3C and 0x3E. Inserting `image` or `qr` anchors the placeholder to the frame and shows `<<Name>>` as the frame's label. It does not place a picture yet. Pictures and QR codes are frames, not inline in a story. Inserting `hyperlink` anchors a text range or a frame and does not change the visible text.

`data.placeholder.remove` deletes the object. A text placeholder also deletes the `<<Name>>` characters it owns. Deleting those characters by editing deletes the placeholder. The role on the placeholder wins over the column kind. An `image` column inserted as `text` merges as the path string. A `text` column with role `image` merges as a picture. A `qr` column inserted as `text` merges as the payload string.

A typed `<<Name>>` that is not covered by a text placeholder is still replaced with the field's text at preview and at merge. It never becomes a picture, a QR code, or a hyperlink. An unknown `<<Nope>>` is left as typed.

## Filling one record

Used by preview and by the merge. Record numbers are 1-based positions in the cached rows after empty rows are skipped.

- `text`: replace the anchored characters, or the typed `<<Name>>`, with the cell. Keep the character style of the first character of that range.
- `image`: resolve the cell as a file path. The first existing file wins: the path as given if it is absolute, then the path beside the data file, then the path beside the saved document. `http` and `https` are not downloaded. They are missing images. Place into that frame. `link_images` true creates a normal link. False embeds the bytes in the document assets and creates no external link. Fitting and `center` come from the merge options. A missing or unreadable file leaves the frame empty, adds `{ record, field, path }` to `missingImages`, and continues.
- `qr`: build a vector code with the same encoder as `object.qrCode` type `text`, using the cell as `content`, inside the frame, black. An empty cell leaves the frame empty and adds a warning.
- `hyperlink`: do not change visible text. If this same range is also a text placeholder, the text rule still runs. Trim the cell. Empty adds no link. If the value contains `://` or whitespace, the destination is a URL. Otherwise, if it contains exactly one `@` and no `:`, the destination is an email. Otherwise it is a URL. Create one hyperlink on that anchor for this record. A failed create adds a warning and continues.

A placeholder whose frame or story lives on a parent spread is not filled. The merge copies it onto the new document's parent still showing `<<Name>>`, and adds one warning.

## Preview

`data.preview` takes `record`. The record stepper clamps to the cached rows. Preview fills the live template, and the session keeps a stash of the unfilled document. Save, `data.preview.stop`, switching away, removing the source, and `data.merge` restore that stash before they continue. Any other edit stops preview first, so the edit applies to the unfilled template. The file on disk never contains the preview text.

## Merge

`data.merge` reads the linked source when `path`, `bytes`, `csv`, and `rows` are all absent. Passing one of those reads it for that run only and does not attach a source. Attaching is `data.source.select`.

The template document is not modified. Its undo stack is unchanged. Preview is cleared before the copy is made, so the new document is filled from the unfilled template, not from the preview. The command builds a new document, appends it to the session, and makes it active. The new document is a normal document: edit it and save it, or close it and discard the run.

Record selection, applied to the cached rows:

- `all`: every row
- `one`: that 1-based row. Out of range is an error
- `range`: a comma-separated list of tokens. A token is `N` or `A-B`, both ends inclusive, optional whitespace. `A` greater than `B` is an error. A number outside `1..=row count` is an error. Duplicates are dropped. Order is the order of first appearance
- Then `limit`, if set, keeps the first N selected records. `0` is an error

No selected records is an error. No document is created.

`per_page: single` copies the whole template once per record. A 3-page template writes 3 pages per record, all filled with that record. Pages, parent pages, styles, swatches, layers, and sections are copied. Document-page placeholders are filled. Parent placeholders stay unfilled.

`per_page: multiple` is tiling. It is an error, and no document is created, when facing pages are on or the template has more than one page. The block is every item on the document page whose bounds intersect the page, except ruler guides. Pasteboard items that miss the page are drawn on the first merged page only. Ruler guides are drawn on every merged page and are not stepped.

The block's width and height are the union of those items. `step_x` is that width plus `column_spacing`. `step_y` is that height plus `row_spacing`. The first record is placed at the block's original position, even when that position crosses an inset. Later records use whole steps `(col, row)` from that origin, with `col` and `row` starting at 0.

Rows first increases `col` until the candidate's right edge would pass `page_width - right_inset`, then sets `col` to 0 and increases `row`. Columns first increases `row` until the candidate's bottom edge would pass `page_height - bottom_inset`, then sets `row` to 0 and increases `col`. A candidate that fails the other inset (`row` against the bottom, or `col` against the right) starts a new page, and that record is `(0, 0)` on the new page. The left and top insets do not move the origin. If a second copy cannot be placed on the first page at all, every record gets its own page, with one warning.

Each merged page has the template page's size, margins, and guides. Pages after the first repeat the block and the guides, not the pasteboard-only items. The last page may be partly empty. No empty records are added to fill a grid.

The command returns:

- `records`: how many records were written
- `pages`: pages in the new document
- `missingImages`: `{ record, field, path }`
- `oversetStories`: how many stories in the new document are overset
- `warnings`: strings

Overset text does not fail the merge. The old `spread` parameter is removed.

## Panel

Window > Utilities > Data Merge opens the panel. It does not merge by itself. The panel reads engine state and calls `app.run`.

The panel shows the source file name, or "No data source", plus Update and Remove. The field list shows a kind icon drawn in code (a T, a picture mark, a module grid), the field name, and how many placeholders use that field. Preview is a checkbox. The stepper is first, previous, a record number, next, and last. It is disabled until preview is on. Create Merged Document runs `data.merge` with the saved options. Those options are edited in that dialog: which records, records per page, and, when multiple is selected, arrange, insets, and spacing. Image options are fitting, center, and link images, plus the optional limit.

Colors come from `theme::Tokens`. New strings go through the existing i18n table. No Adobe icons.

On the web, and from the control channel, the caller passes `csv`, `rows`, or `bytes` plus `name`. There is no filesystem dialog.

## Errors

Every command returns `Result`. A bad path, a bad sheet name, a bad encoding, a bad header, a bad range, a tiling refusal, and the row or column cap are errors and create no document. A missing image, an empty QR cell, a failed hyperlink, a modified source, an ignored extra cell, and a parent placeholder are warnings on a successful merge.

No `unwrap`, `expect`, `panic`, or `unsafe` in the new code. Sizes and offsets use checked arithmetic. The row and column caps bound the allocation. Locks follow the crate's poison policy.

## Tests

Engine tests, with synthetic strings and tiny files in memory:

- Quoted commas, doubled quotes, and a quoted newline stay one field. The existing comma parser already covers the first two.
- Tab and semicolon files, and `.txt` sniffing for each.
- Excel: first sheet, a named sheet, a cached number, an empty formula cell, a boolean, and a date serial left as digits.
- A non-UTF-8 file, an empty header, and a duplicate header are errors.
- `@Photo` and `#Code` become fields `Photo`/`image` and `Code`/`qr`.
- Typed `<<Title>>` is replaced. Typed `<<Nope>>` stays. A text placeholder keeps its character style.
- Image resolution: absolute, beside the data file, beside the document. A missing file continues, with `missingImages` set and an empty frame. An `https` value is missing and is not fetched.
- A `text` field with role `image` places a picture. An `image` field with role `text` inserts the path.
- A QR placeholder encodes the cell text. An empty cell warns.
- Hyperlink: `a@b.c` is email, `https://example.com` is a URL, empty adds no link.
- Single-record merge of a 2-page template and 3 records writes 6 pages. The template's page count and undo length are unchanged. The new document is active.
- Tiling a single non-facing page, six records, rows first, produces the measured grid: three across, then the next row, step equal to the block plus spacing. A block that crosses the bottom inset starts a new page.
- Tiling a facing-page document, and tiling a two-page document, each return an error and add no document.
- Update re-reads a changed fingerprint. Merge with `modified` warns and uses the cache. Remove leaves placeholders.
- 100,001 rows is an error.
- Preview of record 2 shows that row. Stop restores `<<Name>>`. The saved document bytes do not contain the preview text.
- Inline `rows` still merges, including a non-string number.

UI: the panel lists fields from `data.fields`, and Create Merged Document calls `data.merge`. A headless `designcraft-cli` run covers the command. Wasm still accepts `rows`.

## Out of scope

Multiple sources, enabling and disabling sources, JSON, filter rules, sort, skip-on-warning, source filename and merge index fields, a drawn grid (rows, columns, gutter, record offset, record advance, start corner), QR codes from an unmarked column via the QR tool, inline pictures in a story, per-record parent-page fields, downloading remote images, evaluating Excel formulas, formatting Excel dates, `.xls`, and merging straight to PDF.
