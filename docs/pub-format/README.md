<!-- Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0 -->
# Microsoft Publisher (`.pub`) format notes

Plain-English facts about the parts of the `.pub` format that the importer (`crates/pub`) reads.
They are written so that someone who has never seen a `.pub` reader can write one from these
notes, the public Microsoft specifications and sample files. There is no code here, on purpose.
What the importer does and does not support is in [../publisher-import.md](../publisher-import.md).

| File | Covers |
|---|---|
| `property-lists.md` | The key/kind/data encoding that the `Contents` stream and the Quill text store share. |
| `contents-stream.md` | The `Contents` stream: header, record directory, document, pages and masters, objects, palette, colour references. |
| `text-store.md` | The Quill text store: section directory, text, story ids and lengths, fonts, text colours, default styles, character and paragraph runs. |

Every statement in these notes is marked with its source: a named sample file, the public
specifications, or "existing code". **Existing code** means the earlier reader that the person
writing the notes was allowed to read. That reader is not in this repository: the importer's
`Contents`, Quill and bounded-reader modules were then rewritten from these notes by someone who
never saw it (see [../publisher-import.md](../publisher-import.md#provenance)).

## How a `.pub` file fits together

A `.pub` file is an OLE2 compound file (**MS-CFB**). Streams used:

| Stream | What it holds | Described in |
|---|---|---|
| `Contents` | Publication structure: page size, page order, masters, which objects are on which page, per-object text links, the palette. | `contents-stream.md` |
| `Quill/QuillSub/CONTENTS` | All text and its formatting. | `text-store.md` |
| `Escher/EscherStm` | Drawing records: every object's position, size, rotation, shape type, fill and line. | **MS-ODRAW** (OfficeArt): OfficeArtDgContainer, OfficeArtSpgrContainer, OfficeArtSpContainer, OfficeArtFSP, OfficeArtFOPT and the property sets of section 2.3, OfficeArtClientAnchor, OfficeArtChildAnchor and OfficeArtClientData (record type 0xF011, whose contents are host-defined; see `contents-stream.md` §6). |
| `Escher/EscherDelayStm` | Picture data (blips) referenced from the drawing records. | **MS-ODRAW**: OfficeArtBStoreContainer, OfficeArtFBSE and the OfficeArtBlip records. |

Also present but not read: `CompObj`, `Quill/QuillSub/CompObj`, `Envelope`, `Internal`,
`SummaryInformation`, `DocumentSummaryInformation` (the last two are standard OLE property sets,
**MS-OLEPS**).

How the pieces link:

1. The `Contents` **record directory** gives every record a **sequence number** (its position in
   the directory).
2. The **document record** lists the page records by sequence number; each **page record** lists
   its objects by sequence number and may name a master page by sequence number.
3. Each drawing record in `Escher/EscherStm` carries its object's sequence number in its client
   data (id 0x6801), which joins the drawing to the page lists.
4. An **object record** in `Contents` may name a **story id**; the Quill store's story-id list maps
   that id to a slice of the text.
5. Character runs refer to fonts (Quill `FONT` list) and text colours (Quill `PL  ` list); a text
   colour may point into the `Contents` **palette**.

## Versions

- Publisher **2002 to 2021** write the layout described here. The `Contents` stream starts with
  `E8 AC 2C 00`.
- Publisher **97, 98 and 2000** start `Contents` with `E8 AC 22 00` and use a different layout,
  not described here. The importer refuses them with a message suggesting re-saving in a newer
  Publisher. No sample of these versions was available.
- All 142 controlled samples (below) were made with Publisher 2021 and start
  `E8 AC 2C 00 E8 03 0A 00`. No sample from 2002–2019 was available, so differences between those
  versions are **not known**; the importer treats them all alike.
- The notes also cite **the first synthetic sample**: an early test file written by a program other
  than Publisher, not in this repository. It is minimal (no placeholder pages, two style sheets
  with no entries, a one-font list), so a reader must accept such files.
  `crates/pub/tests/data/hello.pub` is a different file, written from scratch by the importer's
  tests.

## Glossary

| Name | Meaning |
|---|---|
| **property** | One key byte, one kind byte, and data whose size the kind fixes. |
| **property list** | Properties laid end to end. |
| **length-prefixed list** | A 32-bit length that counts itself, then a property list. |
| **nested list** | A property whose data is a length-prefixed list. |
| **array** | A nested list whose entries all use key 0x00. |
| **record** | A length-prefixed list in `Contents`, found through the record directory. |
| **record directory** | The array in the `Contents` trailer that lists every record's type and offset. |
| **sequence number** | A record's position in the record directory; used for all references. |
| **record type** | The number in a directory entry saying what a record is (page, object, …). |
| **document record**, **page record**, **object record**, **palette record** | Records of type 0x44, 0x43, 0x01/0x10/0x20/0x30, 0x5C. |
| **master label** | Text property of a page record; non-empty means master page. |
| **placeholders** | The four entries Publisher appends to the page list, including the scratch area. |
| **story** | One continuous text, shown in one text box or a chain of linked ones. |
| **story id** | The number that links an object record to a story. |
| **colour reference** | A 32-bit colour value whose top byte says whether it's RGB or a palette index. |
| **EMU** | English Metric Unit; 914 400 per inch, 12 700 per point. |

## Data files used

Only the bytes of these files were examined. None of them is committed; they are fetched pinned by
commit when needed.

| Files | Source | Use |
|---|---|---|
| 142 controlled `.pub` samples, Publisher 2021, with `truth/*.json` exports | `FuncularLabs/pubsmith`, `tests/fixtures/pub` at commit `0ccdf27` (the fixture files only, never the project's source code) | Format observations from the `.pub` bytes; the `truth/*.json` files only score imports and were never used to work out an encoding |
| 24 real-world `.pub` samples | `apache/poi`, `test-data/publisher` at commit `bf7c9b6a1490046517cbd8e1504e702a89be4bc9` (the `.pub` files only) | Robustness: damaged, older and unusual files |

## Public specifications

- **[MS-CFB]** Compound File Binary File Format: the container.
- **[MS-ODRAW]** Office Drawing Binary File Format: the Escher drawing and picture streams, and the
  colour-reference layout.
- **ECMA-376**, 5th edition, Part 1: `presetShapeDefinitions.xml` and
  `presetTextWarpDefinitions.xml` only, for preset outlines and WordArt paths. They describe shape
  outlines and say nothing about how `.pub` files store anything.
- **[MS-WMF]** and **[MS-EMF]**: the metafile records that pictures stored as WMF or EMF use.
