# Data merge, phase 2

Design for phase 2 of data merge. Approved in conversation on 2026-10-07. Phase 1 shipped on main; its planning spec was removed with the review fixes.

Phase 2 adds four slices, in this order: JSON, more than one source, filter and sort, then a drawn grid. Each slice leaves the existing suite green. Phase 1 pack-to-fit tiling stays. It is used when the document has no grid item.

## Record list

Merge and preview build one list.

1. Skip disabled sources.
2. For each enabled source, in panel order, start from its cached rows, apply its filter, then its sort.
3. Concatenate those lists. Each output record belongs to one source.
4. Apply the phase 1 record choice (`all`, `one`, or `range`) and `limit` to the combined list. Range syntax is unchanged: 1-based numbers and inclusive ranges, separated by commas.

That numbered list is what the preview stepper shows. Grid record offset and record advance then walk this list at merge time. They do not change the stepper.

A filter or sort that names a field the source does not have is an error and adds no document. If every source is disabled, or the combined list is empty, that is an error and adds no document.

Placeholders for any source other than the record's source are blank in that copy. Text is empty, an image or QR frame is empty, and no hyperlink is created. Static text stays. A blank placeholder is expected and is not a warning. An empty grid cell is the same: the static prototype is drawn, its placeholders are blank, and there is no warning.

A missing or changed file still sets that source's status on a successful merge, uses the cached rows, and does not push an undo step. Filter and sort then run on those cached rows. The warning strings from phase 1 stay.

## JSON

JSON is another reader on the phase 1 source path. A `.json` file, file bytes whose name ends in `.json`, or an inline `json` string must be one top-level array of objects. The first object's key order is the field list. `@` and `#` on a key mark image and QR the same way as a text header. A later extra key is warned and ignored. The warning names the 1-based array index.

These are errors and do not attach a source: a top-level value that is not an array, an array element that is not an object, a nested array or object, an empty array, a first object with no keys, and text that is not UTF-8. A leading byte-order mark is stripped. Numbers, booleans, and null use the same text as inline rows. An inline `json` string does not create a file link.

## Sources

`sources` remains a list. Phase 2 allows more than one entry.

Each source gains:

- `enabled`: bool, default true. A disabled source stays linked and is skipped.
- `filter`: match `all` or `any`, and a list of rules. Default is `all` and no rules, which keeps every row.
- `sort`: a list of `{ field, direction }`. Direction is `asc` or `desc`. Default is an empty list, which keeps file order.

A rule is `{ field, op, value }`. Operators are `equals`, `notEquals`, `contains`, `startsWith`, `empty`, and `notEmpty`. `empty` and `notEmpty` ignore `value`. Comparison is the cell string, case-sensitive. A rule value is read as text: a string is used as-is, a number uses the same number text as a cell, a boolean is `true` or `false`, and null is empty. A nested rule value is an error.

Sort runs after the filter. It is stable. Equal keys fall through to the next sort field.

`data.source.select` adds a source and assigns a new id. Pass an existing `id` to replace that source and keep the id. A second select without an id does not replace the first source. Update re-reads one source and keeps its id, enabled flag, filter, and sort.

`data.source.update`, `data.source.remove`, `data.source.enabled`, `data.source.filter`, and `data.source.sort` take an `id`. The id may be omitted when the document has exactly one source. It is required when the document has more than one. An unknown id is an error. Filter and sort replace the whole list. An empty rule list or an empty sort list clears that setting. These five edits are undoable.

`data.fields` stays a query. Each entry gains `sourceId`, `sourceName`, and `enabled`. Disabled sources are included. A passed `json`, `csv`, `rows`, `bytes`, or `path` payload is parsed and not attached.

`data.placeholder.add` takes `source`. If it is omitted, the field is looked up on the first enabled source. If no source is enabled, that is an error. A placeholder may name a disabled source when `source` is passed.

## Drawn grid

A grid is a document item. Its children are the prototype cell. A document may contain any number of grids. Every grid must sit on a document page. Several grids may sit on the same page. The template may have other pages, including facing pages.

The grid rectangle, cell size, and gutter are in the item's inner space. The item's transform places every cell on the page, so a rotated or scaled grid repeats along its own axes. Cell width is `(width - gutter * (columns - 1)) / columns`, and cell height uses the same formula. Rows and columns are at least 1, and their product is at most 500. Gutter is finite and at least 0.

The origin is `topLeft` (default), `topRight`, `bottomLeft`, or `bottomRight`. Arrange is `rows` (default) or `columns`. Flow starts at the origin cell and walks in that arrange direction, reversing an axis when the corner requires it. Children are stored relative to the origin cell's top-left corner. They are not clipped to the cell.

Record offset and record advance belong to each grid. Offset skips that many records in front of that grid, the first time it is visited. Default 0. Later visits do not skip again. Advance steps between that grid's cells. Default 1. Advance 0 repeats one record in every cell of that visit, then moves one record forward.

Grids are visited in document order: pages from first to last, and on a page the stored item order, earlier items first. They share one cursor into the combined list. A visit consumes records for that grid's cells, including records that advance skips. The next grid continues at the cursor.

A cycle is one copy of the template, in page order, and one visit to each grid. The record in the first grid's origin cell fills every item that is not a child of a grid. Each grid's children repeat once per cell of that grid. A cycle is produced only when the first grid's origin cell has a record. Later cells and later grids in that cycle may be empty. The merged document contains ordinary copies. It does not contain a grid item.

Preview record N, 1-based in the combined list, starts that same walk at record N and visits each grid once. It does not repeat the template, and it does not apply offsets. A step past the end of the list leaves that cell blank. Advance 0 fills every cell of that grid from the record under the cursor, then moves one record forward before the next grid. Items that are not children of a grid show record N.

With no grid item, merge behaves as in phase 1. `per_page` multiple still requires one non-facing page and still packs the frames that are already on that page. The layout is one type: records per page, or a grid. Creating a grid sets `per_page` to single. Setting `per_page` to multiple releases every grid and puts its children back on the page. Choosing single record in the panel does the same release. A merge that asks for both is an error and adds no document.

These are errors and add no document:

- A grid on a parent page, or a grid that does not sit on a document page.
- `per_page` multiple while any grid item exists.
- Rows or columns below 1, or a product above 500.
- A gutter that is not finite or is negative.
- A cell width or height that is not finite or is not positive.
- An unknown origin or arrange value.
- A record offset on the first grid that leaves its origin cell empty before any cycle is produced.

## Commands

Phase 1 commands keep their jobs. `data.options` still stores the record choice, `per_page`, pack-to-fit insets and spacing, fitting, and limit. It does not store filter, sort, or grid settings. `data.merge` still does not write the options back.

New and changed commands:

- `data.source.select`: adds a source. Optional `id` replaces that source. Accepts `json` beside `path`, `bytes`, `csv`, and `rows`.
- `data.source.enabled`: `{ id, enabled }`.
- `data.source.filter`: `{ id, match, rules }`.
- `data.source.sort`: `{ id, fields }`, where each field is `{ field, direction }`. A missing direction means `asc`.
- `data.grid.create`: the same `rect` as `frame.create`, on the active page. Optional rows, columns, gutter, `recordOffset`, `recordAdvance`, origin, and arrange. Defaults are 2, 2, 0, 0, 1, `topLeft`, and `rows`. A document may already contain grids. Items whose bounds are contained in the new grid's origin cell become children, with positions rewritten relative to that cell. An item that only overlaps the cell stays on the page. Another grid is not adopted.
- `data.grid.set`, `data.grid.adopt`, and `data.grid.release` take the grid item `id`. The id may be omitted when the document has exactly one grid. It is required when the document has more than one. An unknown id is an error.
- `data.grid.set`: changes that grid's properties.
- `data.grid.adopt`: parents the current selection into that grid's origin cell. Items on another page, the grid itself, or a different grid are an error and the grid is unchanged.
- `data.grid.release`: puts that grid's children back on the page as ordinary items and deletes that grid. Other grids stay.

Grid commands and the source edits above are undoable. Merge still adds a new document. The template undo stack gains no merge step.

## Panel

The Data Merge panel lists every source with an enable checkbox, its name, its status, and its row count after filter and sort. Select Data Source adds a file. The desktop open dialog for this purpose also offers `.json`. Update and Remove act on the selected source. That source has a match-all or match-any filter and a sort list. Fields are grouped under their source. Preview and the stepper use the combined list. Create Merged Document calls `data.merge` with an empty parameter object.

A Create Grid button calls `data.grid.create` with the active page's margin box as the rectangle. Pressing it again adds another grid. When a grid is selected, the panel edits that grid's rows, columns, gutter, offset, advance, origin, and arrange. New strings go through the existing i18n table. Colors come from `theme::Tokens`.

On the web, and from the control channel, the caller passes `json` the same way it passes `csv`, `rows`, or `bytes`. There is no file dialog there.

## Errors and warnings

Errors return `Result`, attach nothing, and add no document. Warnings ride on a successful merge. Phase 1 errors and warnings still apply.

Phase 2 errors: bad JSON, a nested JSON value, an unknown filter or sort field, a bad operator, match, direction, origin, or arrange, a nested rule value, no enabled source, an empty combined list, an unknown source id, a missing id when several sources exist, a placeholder field that is not on the named source, an unknown grid id, a missing grid id when several grids exist, a grid off a document page, pack-to-fit combined with any grid, a bad row, column, or gutter value, a cell that is not positive, an offset on the first grid that leaves its origin cell empty before any cycle, and adopting a grid or items from another page.

Phase 2 warnings: an extra JSON key. Blank placeholders and empty grid cells are not warnings.

No `unwrap`, `expect`, `panic`, or `unsafe` in the new code. Sizes use checked arithmetic. The existing row and column caps still bound a source. The 500-cell cap bounds each grid.

## Tests

Engine tests, with synthetic strings and tiny files:

- JSON keeps the first object's key order. A number, a boolean, and null become the same text as inline rows. An extra key on a later object warns and is ignored. A nested object, a top-level object, and an empty array do not attach a source.
- Inline `json` merges and leaves the template with no source.
- A second select adds a source. Replace with `id` keeps that id and drops the old rows.
- Enabled sources concatenate in list order. A disabled source is skipped. A placeholder for the other source is blank, with no warning. No enabled source adds no document.
- Filter `equals` and `contains` are case-sensitive. Match `any` keeps a row that hits one rule. `empty` keeps a blank cell. An unknown field adds no document.
- Sort by one field descending, then a second field ascending, is stable for the remaining ties.
- Two sources of three rows each, range `3-4`, yields the last row of the first source and the first row of the second.
- A changed file on the second source sets that source to `modified`, uses the cached rows, and leaves the template undo length unchanged.
- A 3-page template with a 2 by 2 grid on page 2, rectangle `[72, 72, 272, 272]`, gutter 0, offset 0, advance 1, origin `topLeft`, arrange `rows`, and 6 records, writes 6 pages. Cycle 1 fills pages 1 and 3 from record 1, and the grid cells from records 1, 2, 3, 4 at `(72, 72)`, `(172, 72)`, `(72, 172)`, `(172, 172)`. Cycle 2 fills pages 1 and 3 from record 5, and the grid cells from records 5 and 6 plus two blank cells. The merged document has no grid item.
- The same rectangle with origin `topRight` and arrange `rows` places the first four cells at `(172, 72)`, `(72, 72)`, `(172, 172)`, `(72, 172)`.
- Advance 0 and two records write two grid pages, each cell on a page showing that page's one record.
- Offset 1 with four records and a 2 by 2 grid starts at record 2 and leaves the last cell blank. Companion pages show record 2.
- Two 2 by 1 grids on one page, advance 1, offset 0, and 6 records write two cycles. The first grid's rectangle is `[72, 72, 272, 172]` and its cells are records 1 and 2 at `(72, 72)` and `(172, 72)`. The second grid's rectangle is `[72, 200, 272, 300]` and its cells are records 3 and 4 at `(72, 200)` and `(172, 200)`. The second cycle places records 5 and 6 in the first grid and leaves the second grid blank.
- A grid on page 1 and a grid on page 2 are visited in that page order. The page 2 grid receives the records the page 1 grid did not consume.
- A grid on a parent page, pack-to-fit together with a grid, and a gutter that collapses a cell each add no document. Creating a grid while `per_page` is multiple leaves `per_page` single. Setting `per_page` to multiple removes the grids.
- With no grid item, the phase 1 3-across tiling test still passes.
- Preview record 2, with one grid and advance 1, shows that record in the origin cell and the following records in the following cells. A step past the last record is a blank cell. With two grids, preview record 1 fills the first grid from record 1 and continues the same cursor into the second grid.

UI: the panel groups fields by source, the enable checkbox calls `data.source.enabled`, and Create Merged Document calls `data.merge`. The desktop filter list for purpose `dataMerge` includes `json`. Wasm still accepts an inline `json` string.

## Out of scope

Phase 3 is scoped in `docs/superpowers/specs/2026-10-07-data-merge-phase-3-scope.md`. Phase 2 does not include skip on warning, prefix and postfix rules, source filename and merge index fields, a drag tool for the grid, joining sources on a key, a global sort across sources, QR codes from an unmarked column, inline pictures, per-record parent-page fields, downloading remote images, Excel formulas, Excel date formatting, `.xls`, or merging straight to PDF.
