<!-- Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0 -->
# The `Contents` stream (layout of the publication)

The root stream named `Contents` describes the publication's structure: page size, page order, master pages, which objects sit on which page, a few per-object facts that the drawing stream doesn't carry (text link, table flag, vertical text alignment), and the document colour palette. Object geometry and appearance are **not** here; they are in the Escher drawing stream (MS-ODRAW, see `README.md`).

Everything in this stream is a set of **records**. A record is a length-prefixed list (`property-lists.md`) at some byte offset in the stream. Records are found through a **record directory**.

## 1. Stream header

| Offset | Size | Meaning |
|---|---|---|
| 0x00 | 4 bytes | **Format signature.** `E8 AC 2C 00` = Publisher 2002 and later. `E8 AC 22 00` = Publisher 97/98/2000 (a different layout; refuse it with a "re-save in Publisher 2002 or later" message). Anything else: not a Publisher file. |
| 0x04 | 4 bytes | `E8 03 0A 00` in every sample. Not read. |
| 0x08 | 4 bytes | Varies (0x0310 – 0x2882 across samples). Not read. |
| 0x0C | 14 bytes | `1A 00 00 01` then zeros in every sample. Not read. |
| 0x1A | 4 bytes, unsigned | **Offset of the trailer record** (in bytes from the start of the stream). |
| 0x1E | … | Records. |

*Source: signature values from the existing code (the 97/98/2000 value was not seen in any sample; all 142 pubsmith samples and the first synthetic sample start with `E8 AC 2C 00 E8 03 0A 00`). Trailer offset: existing code, confirmed in every sample.*

## 2. Trailer record

The trailer is a length-prefixed list at the offset given at 0x1A. It holds:

| Key | Kind | Meaning |
|---|---|---|
| 0x01 | 0x20 | A count; in the first synthetic sample it is 0x104 = 260, the number of entries in the record directory (the generated `crates/pub/tests/data/hello.pub` reproduces this count). Not read. |
| 0x02 | 0x20 | Another number (0x103 in the first synthetic sample, also reproduced by the generated `hello.pub`). Not read. |
| 0x03 | 0x90 | **The record directory** (nested list). The reader finds it as "the first property of kind 0x90 in the trailer". |

*Source: existing code (looks for the first kind-0x90 property); keys 0x01/0x02 seen in samples only.*

## 3. Record directory and sequence numbers

The record directory is an array. **Each entry's position in the array (0, 1, 2, …), counting every entry including empty ones, is that record's sequence number.** Sequence numbers are how records refer to each other everywhere in the file (page lists, master links, the drawing stream).

- Entries of kind 0x78 (no data) are empty slots. In every sample the first 256 entries are empty, so real records start at sequence number 0x100 = 256.
- Entries of kind 0x88 describe a record. Their fields:

| Key | Kind | Meaning |
|---|---|---|
| 0x02 | 0x18 | **Record type** (table below). |
| 0x04 | 0xB8 | **Byte offset of the record** in the `Contents` stream. |
| 0x06 | 0x10 | Some other record's sequence number. Not read. |

An entry missing its type or its offset is ignored (its sequence number is still used up).

*Source: existing code; confirmed in every sample. "First 256 are empty": seen in all samples.*

### Record types

| Type | What the record is | Read? |
|---|---|---|
| 0x01 | An ordinary object (shape, text box, picture, line) | yes |
| 0x10 | A table | yes |
| 0x20 | An object of another kind; in the samples, WordArt and lines, curves and polylines | yes, like 0x01 |
| 0x30 | A group | yes, like 0x01 |
| 0x43 | A page (ordinary page, master page, or a placeholder; see §5) | yes |
| 0x44 | The document | yes |
| 0x5C | The colour palette | yes |
| others (0x46, 0x4A–0x4F, 0x54, 0x5B, 0x60, 0x61, 0x65, 0x6C, 0x72, 0x73, 0x77, 0x7F, 0x8A, …) | Present in Publisher-written files; meaning unknown | no |

*Source: the meanings of 0x01, 0x10, 0x20, 0x30, 0x43, 0x44, 0x5C come from the existing code. Sample counts across the 142 pubsmith files: type 0x01 ×138, 0x20 ×12 (the 8 `wordart__*` files and `shapes__curve`, `shapes__line`, `shapes__line-arrow`, `shapes__polyline`), 0x10 ×3 (only in the `table__*` files), 0x30 ×2 (only in the `group__*` files).*

## 4. Document record (type 0x44)

Exactly one is expected; without it the file is malformed. Fields read:

| Key | Kind | Meaning |
|---|---|---|
| 0x02 | 0xA0 | **Page list**: an array whose entries (key 0x00, kind 0x70) are the sequence numbers of page records, in page order. Includes master pages and Publisher's placeholder pages (see §5). |
| 0x12 | 0x88 | **Page size**, a nested list: key 0x01 (kind 0x20) = page width in EMU, key 0x02 (kind 0x20) = page height in EMU. 12 700 EMU = 1 pt. |

If the width or height is missing or zero, the file is malformed. Other keys (0x01, 0x03, 0x04, 0x08, 0x18–0x23, 0x2A, 0x2C, 0x2D, 0x31, 0x39, 0x3C, 0x41, 0x44, 0x4D, …) appear in samples and are not read.

*Source: existing code; values confirmed: the first synthetic sample 0x7698F0 × 0x997AA0 EMU = 612 × 792 pt (US Letter); `text__bold.pub` the same.*

## 5. Page records (type 0x43)

Fields read:

| Key | Kind | Meaning |
|---|---|---|
| 0x02 | 0xA0 | **Objects on the page**: an array of sequence numbers (entries of kind 0x70) of the object records drawn on this page. Absent on an empty page. |
| 0x0D | any number kind (0x68 in samples) | **Master page used** by this page: the master's sequence number. Absent = no master. |
| 0x0E | 0xC0 | **Master label** (text). Non-empty (for example `A`) = this page record **is a master page**. Empty text on ordinary pages. |

Seen but not read: 0x01, 0x03, 0x05 (a nested list of two EMU sizes), 0x06 (8-byte kind 0x28), 0x09, 0x0B, 0x0F (a longer master name such as `Master Page A`), 0x10, 0x11.

*Source: existing code; confirmed in `text__bold.pub` (master record with label `A` and name `Master Page A`; page record pointing at it with key 0x0D) and the first synthetic sample (empty labels).*

### Sorting the page list into ordinary pages, masters and placeholders

1. Each page-list entry whose record is a type-0x43 page record with a non-empty master label is a **master page**. Masters keep page-list order.
2. Of the remaining entries (non-masters, plus entries that don't point to a page record at all), Publisher always appends **four placeholders** at the end. One of them is the **scratch area** (the pasteboard beside the pages); Publisher's own export doesn't show objects placed there.
3. Treat the last four non-master entries as placeholders only when **both** hold: there are more than four non-master entries, and at most one of those last four is a page record that lists objects. Otherwise (a file from another writer) there are no placeholders.
4. Ordinary pages = every non-master, non-placeholder entry that points to a page record, in page-list order. Entries that don't point to a type-0x43 record are skipped.
5. If a placeholder lists objects, warn that scratch-area objects were not imported.
6. If no ordinary page is left, use the first placeholder page that lists no objects, so that a publication with one blank page still has a page.

*Source: rules 2–6 come from the existing code and its comments ("observed in every sample"). Confirmed in `text__bold.pub`: page list = 0x107 (master A), 0x10A (the page), 0x10D, 0x110, 0x113, 0x117 (four placeholders). the first synthetic sample has a one-entry page list (no placeholders), which is why rule 3 exists.*

## 6. Object records (types 0x01, 0x10, 0x20, 0x30)

Every object record is read the same way; type 0x10 additionally marks the object as a **table**. Fields read:

| Key | Kind | Meaning |
|---|---|---|
| 0x27 | number (0x20 in samples) | **Story number**: which story in the Quill text store this object shows (it matches a story id in the Quill story-id list; see `text-store.md` §4). Absent = no text. |
| 0x28 | number | **Position in a chain of linked text boxes** that show the same story: 0 for the first box, 1 for the next, and so on. Absent = 0. |
| 0x35 | number (0x20 in samples) | **Vertical text alignment** in the box: 1 = middle, 2 = bottom; absent or anything else = top. |

Seen but not read: 0xAA and 0xAB (kind 0x20; EMU sizes, for example 0x37CF80 = 288 pt and 0xDF3E0 = 72 pt in the first synthetic sample), 0xB7, and many others.

How these records connect to drawing: each Escher shape's client-data record (MS-ODRAW OfficeArtClientData, record type 0xF011) carries property-like pairs, and the pair with id 0x6801 holds the shape's `Contents` sequence number. That is how geometry from the drawing stream meets the facts in this section. (The drawing reader already does this; listed here so the link is clear.)

A story number that doesn't exist in the text store means the object has no text.

*Source: keys 0x27 and 0x35 from the existing code, confirmed in samples: `text__vanchor-middle.pub` 0x35 = 1, `text__vanchor-bottom.pub` 0x35 = 2, `text2__two-boxes.pub` two objects with story numbers 2 and 3, `table__2x2.pub` a type-0x10 record with story number 3. Key 0x28 comes from the existing code only: **no sample in the corpus contains it**, so its kind and numbering are unconfirmed.*

## 7. Palette record (type 0x5C)

The palette is the document's colour scheme; colours elsewhere can refer to it by index.

- Look at the record's properties of kind 0xA0 (arrays). Each array entry is one palette colour, numbered 0, 1, 2… in order (across all such arrays, in order).
- An entry of kind 0x88 holds key 0x01 (kind 0x20) = the colour as **R, G, B in the low three bytes, red lowest** (byte 0 = red, byte 1 = green, byte 2 = blue; the top byte is 0).
- Any other entry (in the samples entry 0 is an empty kind-0x78 entry), or an 0x88 entry without key 0x01, is **black**. It still uses up its index.

*Source: existing code; confirmed in `text__run-red-word.pub`: entry 0 empty, entry 1 bytes `5B 9B D5` = RGB(91, 155, 213), …, entry 7 `FF FF FF` = white. All 142 pubsmith samples have the same 8-entry palette.*

## 8. Colour references

A 32-bit colour value used by the drawing stream and the Quill colour table is read by its top byte:

| Top byte | Meaning |
|---|---|
| 0x00 | Direct colour: R, G, B in the low three bytes, red lowest. |
| 0x08 | Palette colour: the low 24 bits are an index into the palette record (§7). A missing index gives black plus a warning. |
| anything else | Scheme or system colour: not decoded; black plus a warning. |

*Source: the rule is in the crate's import code (not one of the two files being rebuilt) and matches MS-ODRAW's OfficeArtCOLORREF in spirit (low three bytes RGB, flags in the top byte). Seen in samples: top byte 0x00 ×145 and 0x08 ×142 in the Quill colour tables; nothing else.*
