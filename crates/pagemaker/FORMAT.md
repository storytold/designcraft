# PageMaker file format: what the importer reads

These are our own notes, worked out from sample files (the public dexvert sample set and the
Archive Team file-format wiki's identification notes). libpagemaker was used only as a black box
(its `pmd2text`/`pmd2svg` output compared with ours on the two PageMaker 6/7 samples it reads);
its source and notes were not read. Offsets are in bytes, numbers in the file's byte order
unless stated. "Twips" are 1/1440 inch (1/20 point).

## Container

- **PageMaker 6, 6.5 and 7** (`.pm6`, `.p65`, `.pmd`, templates `.pt6`, `.t65`, `.pmt`): an OLE
  compound file (signature `D0 CF 11 E0 A1 B1 1A E1`, read after Microsoft's [MS-CFB]) with one
  stream, `PageMaker`. That stream is laid out like a whole PageMaker 4–5 file.
- **PageMaker 4 and 5** (`.pm4`, `.pm5`, `.pt4`, `.pt5`): the stream is the file.
- **Mac files** often have no extension; the content identifies them.

## Header (first 64 bytes of the stream)

| Offset | Meaning |
|---|---|
| 6–7 | `FF 99`: PC, little-endian. `99 FF`: Mac, big-endian. |
| 10 | Version code (u16): `0x0201`–`0x0204` PageMaker 4, `0x0205`–`0x0208` PageMaker 5, `0x0209` PageMaker 6–7. A small number (5) is PageMaker 3, a different format. |
| 18 | (u32) points into the record area (not used). |
| 22, 26 | (u32) stream length. |
| 42 | (u16) format number: 42 (PM4), 47 (PM5), 50 (PM6.0), 51 (PM6.5/7). |
| 44 | Index descriptor, 10 bytes: `[0][kind][count u16][offset u32][FF FF]`. |

Index descriptor kind 0: `count` 16-byte entries at `offset`. Kind 1: `count` 10-byte block
references (same layout as the descriptor), each naming a run of 16-byte entries.

## Index entries (16 bytes)

`[0][type][count u16][offset u32][FF FF][flag][type][0 × 4]`

`count` records of `type` start at `offset`. A type-1 entry with flag 3 is a **group**: `offset`
holds `count` 10-byte chunk references `[0][subtype][count u16][offset u32][FF FF]`; the subtype's
records are the chunks joined (large stories, many pages, embedded images, where `count` is a
byte count).

| Type | Records | Size by version (PM4 / PM5 / PM6.0 / PM6.5–7) |
|---|---|---|
| 0x05 | page settings | — |
| 0x09 | lines of text (layout cache, not read) | 16 |
| 0x0B | paragraph runs | 70 / 76 / 80 / 80 |
| 0x0C | (per story, not read) | — |
| 0x0D | story text, one byte per character | 1 |
| 0x0E | embedded image data (in groups) | bytes |
| 0x0F | embedded metafile data (in groups) | bytes |
| 0x10 | links (placed files) | — / — / 332 / — |
| 0x13 | fonts: name in the first 32 bytes | 144 / 94 / 94 / 94 |
| 0x14 | paragraph styles (not read yet) | — |
| 0x15 | colours: name in the first 32 bytes | 64 / 86 / 210 / 210 |
| 0x18 | document record | — |
| 0x19 | objects of one page | 58 / 78 / 136 / 258 |
| 0x1B | story records | 24 |
| 0x1C | character runs | 26 / 30 / 30 / 30 |
| 0x28 | transforms (rotation) | — / 26 / 26 / 26 |

## Document record (0x18)

The left page's rectangle, then the right page's (twips, about the spread's centre line; the same
rectangle twice when pages don't face), at 40 (PM4), 42 (PM5) or 54 (PM6–7). PM4 and PM5 follow
them with the left page's margin box.

## Object lists (0x19)

In index order: one list of objects off the pages (two from PageMaker 6.5), the left and right
master pages, then one list per page. Coordinates are spread-wide: x from the spine (facing
pages) or the page's centre, y from the page's vertical centre, in twips; an object listed with
one page may lie on the other page of its spread.

Object record: type at 0 (1 text block, 3 line, 4 box, 5 oval, 6 image, 10 metafile, 11–12
polygon); bounds (4 × i16) at 4 (PM4) or 6 (PM5 and later).

- Text block: block id (u32) and story id (u32) at 26/30 (PM4) or 32/36 (later). Blocks with one
  story are its threaded frames, in block-id order.
- Transform id (u32, `FFFFFFFF` = none) at 28, from PM5: a 0x28 record with the rotation at 0
  (i32, thousandths of a degree, counter-clockwise), the unrotated box at 10 and its id at 22.
- Image: its pixel size (2 × u16) at 38 (PM4, sometimes a whole multiple of the image's own size)
  or 44 (later); the embedded image of that size is its picture.
- Line: end points (4 × i16) at 30 (PM4) or 40 (later); weight in tenths of a point at 35 (u8,
  from PM5).

## Stories

Story records (0x1B, 24 bytes): length at 0 (two u16 words, low word first), id at 20 (u32). A
list of story records is followed by the text (0x0D), character runs (0x1C) and paragraph runs
(0x0B) of all of them, one story after another.

Text: CR ends a paragraph; 0x09 and 0x19 are tabs; 0x1A is a forced line break; Windows-1252 on
the PC, Mac OS Roman on the Mac.

Character run: length 0, font index 2, size in tenths of a point 4, leading in tenths 6 (`FFFF`
auto), colour index 8, style bits 10 (1 bold, 2 italic, 4 underline).

Paragraph run: length 0, alignment byte 3 (0 left, 1 right, 2 centre, 3–4 justified), style index
6, then in twips: left indent 10, first-line indent 12, right indent 14, space before 16, space
after 18; justification settings follow.

## Colours (0x15)

The fixed colours come first: Registration, Paper, Black (PM4–5); None, Registration, Paper,
Black (PM6–7). Others: PM4–5 C, M, Y, K (u16, 65535 = 100 %) at 34; PM6–7 a model at 34 (`0x18`
RGB bytes at 38, `0x08` CMYK u16s at 38).

## Not read yet

Paragraph styles and tab stops (0x14), box and oval fill and stroke, polygon points, metafile and
EPS graphics, text wrap, and PageMaker 3's block-based format.
