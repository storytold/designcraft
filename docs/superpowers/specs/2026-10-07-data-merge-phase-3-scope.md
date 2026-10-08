# Data merge, phase 3 scope

Scope for the data-merge work that phase 2 leaves out. Phase 1 is `docs/superpowers/specs/2026-10-07-data-merge-design.md`. Phase 2 is `docs/superpowers/specs/2026-10-07-data-merge-phase-2-design.md`.

This document names what phase 3 includes. It is not the implementation design. Phase 1 and phase 2 behavior stays unless a section below says otherwise.

## Skip on warning

A merge option can drop a record when filling that record produces a warning. The record is left out of the merged document and out of the preview stepper. The option is off unless the user turns it on, so today's warnings still keep the record. The warning is still reported. A skipped record does not consume a grid cell.

## Prefix and postfix rules

A source can store rules that prepend or append text to a field's value before preview and merge. A rule names the field, the text, and whether it is a prefix or a postfix. It can require the cell to be non-empty. The stored cell is unchanged. The altered text is what placeholders see.

## Source filename and merge index fields

Two virtual fields are available on every source, in addition to the columns from the file.

- Source filename inserts the source file name. An inline source with no path uses its `name`.
- Merge index inserts the record's 1-based position in the combined list, after filter, sort, range, and limit.

They can be placed like any other text field. They are not written back into the file.

## Drag tool for the grid

A tool drags a grid rectangle on the page. On mouse-up it calls `data.grid.create` with that rectangle and the same defaults as the panel button. The Create Grid button stays. Resize and the panel still edit an existing grid.

## Joining sources on a key

Concatenation from phase 2 stays the default. A document can instead name one driving source and join other enabled sources to it. Each joined source matches a key field on the driving source to a key field on itself. One driving record plus its matching rows is one output record, so placeholders from several sources can fill together. A driving row with no match leaves the joined placeholders blank. Join does not download or invent rows.

## Global sort across sources

Phase 2 sorts each source before concatenation. Phase 3 adds a sort of the combined list, after concatenation and before range and limit. Records from different sources can interleave. A record whose source has no such field sorts as an empty string. The per-source sort still runs first.

## QR codes from an unmarked column

The QR tool can bind its value to any column of a linked source, including a column that is not marked with `#`. The column's kind stays text. The frame's role is `qr`, which already wins over the column kind. Marked `#` columns stay QR by default.

## Inline pictures

An image placeholder can sit inside a story, not only on a frame. The picture flows with the text at preview and at merge. Frame pictures stay as they are. A missing image still warns and leaves that inline picture empty.

## Per-record parent-page fields

Parent-page placeholders fill from the record used for that output page. On a grid cycle, that is the record in the first grid's origin cell, the same record loose page items already use. Phase 2's single warning for an unfilled parent placeholder remains only when the page has no record.

## Downloading remote images

An image cell whose text is an `http` or `https` URL is fetched and placed. Phase 1 treats those cells as missing and does not fetch them. A failed fetch warns, leaves the frame or inline picture empty, and does not fail the merge. Other paths still resolve beside the data file and beside the document.

## Excel formulas

A formula cell uses its computed value. Phase 1 and place-as-table keep reading the cached value only. A formula phase 3 cannot compute stays empty and adds a warning. The cached value is still preferred when the file has one.

## Excel date formatting

A cell formatted as a date becomes date text. Phase 1 leaves the stored serial number. If the number format cannot be read, the serial remains and a warning is added. Cells that are not dates stay as they are.

## .xls

`.xls` is a data-merge source, read with the same row rules as `.xlsx`: header row, empty cells kept, row and column caps. `.xlsm` stays an error. Place-as-table does not gain `.xls`.

## Merging straight to PDF

`data.merge` can write a PDF of the merged pages in the same run. The PDF uses the existing PDF export. The merged document is still added and becomes active. Omitting the PDF path leaves merge as it is in phase 2.
