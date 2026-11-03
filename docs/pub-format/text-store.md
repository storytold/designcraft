<!-- Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0 -->
# The Quill text store (`Quill/QuillSub/CONTENTS`)

All text of the publication lives in one stream, `CONTENTS` inside the storage `Quill/QuillSub`. It holds every story's characters back to back, plus the fonts, text colours, default styles and character/paragraph formatting. Each text box in `Contents` names its story by number (`contents-stream.md` §6).

The stream is split into **sections**, each with a four-letter ASCII tag (for example `TEXT`), found through a **section directory**. The tags are part of the file data.

## 1. Stream header (0x00–0x17)

| Offset | Size | Meaning |
|---|---|---|
| 0x00 | 8 bytes | ASCII `CHNKINK ` (with a trailing space). |
| 0x08 | 4 bytes | `04 00 07 00` in every sample. |
| 0x0C | 2 bytes | Number of sections (equals the directory count below in every sample). |
| 0x0E | 2 bytes | `00 03`. |
| 0x10 | 4 bytes | 0x200 in every sample (the offset of the first section). |
| 0x14 | 4 bytes | The stream's length in bytes. |

None of the header is needed to read the text. *Source: seen in samples only (the first synthetic sample, `text__bold.pub`); the existing code ignores the header.*

## 2. Section directory

The first directory block is at offset **0x18**. A directory block is:

| Offset in block | Size | Meaning |
|---|---|---|
| 0 | 2 bytes | 0x01F8 in every sample. Not read. |
| 2 | 2 bytes, unsigned | **Number of entries** in this block. |
| 4 | 4 bytes, unsigned | **Offset of the next directory block**, or 0xFFFFFFFF if this is the last. |
| 8 | 24 bytes × count | Entries. |

Each 24-byte entry:

| Offset in entry | Size | Meaning |
|---|---|---|
| 0 | 2 bytes | 0x0018 (the entry size) in every sample. Not read. |
| 2 | 4 bytes | **Section tag**, ASCII (`TEXT`, `STSH`, `FDPC`, …). |
| 6 | 2 bytes | Which occurrence of this tag (0, 1, 2…). The `STSH` sections are numbered 0, 1, 2. Not read: occurrence order in the directory is used instead. |
| 8 | 4 bytes | 1 in every sample. Not read. |
| 12 | 4 bytes | The tag again. Not read. |
| 16 | 4 bytes, unsigned | **Offset of the section** in the stream. |
| 20 | 4 bytes, unsigned | **Length of the section** in bytes. |

Collect all entries from all directory blocks, in order. Guard against a chain of "next" offsets that loops (the existing implementation remembers visited offsets and gives up after 64 blocks). All 142 samples have a single directory block.

*Source: offsets 2, 4, 16, 20 of the block/entry and the start at 0x18 from the existing code, confirmed in all samples; other fields seen in samples only.*

### Sections seen

`TEXT`, `STSH` (×3), `FDPP`, `FDPC`, `SYID`, `SGP `, `INK `, `BTEP`, `BTEC`, `FONT`, `STRS`, `MCLD`, `PL  ` (two trailing spaces) in Publisher-written files. The first synthetic sample (an early test file written by a program other than Publisher, not in this repository; the other notes call it "the first synthetic sample") has only `TEXT`, `STSH` ×2, `FDPP`, `FDPC`, `SYID`, `FONT`, `STRS`. Only the sections below are read; a publication with no `TEXT` section has no text (that is not an error).

## 3. `TEXT`: the characters

The whole section is UTF-16LE text: every story, one after another, in the order of the story-id list (§4). Paragraphs end with **0x000D** (carriage return), and the last paragraph of every story ends with one too. Other control units seen or handled: 0x000B = line break inside a paragraph, 0x0009 = tab. Other units below 0x0020 stand for fields (page numbers, dates, …) and have no text of their own.

*Source: layout from the existing code; confirmed in the first synthetic sample (one 31-character paragraph + CR, 32 units) and `text2__two-boxes.pub` (two stories back to back). Control-unit meanings from the crate's import code; 0x000B confirmed by `text__line-break.pub`.*

## 4. `SYID` and `STRS`: which story is which

**`SYID` — story ids.**

| Offset | Size | Meaning |
|---|---|---|
| 0 | 4 bytes | Not read. Equals the largest id in every sample. |
| 4 | 4 bytes, unsigned | **Count** of ids. |
| 8 | 4 bytes × count | **Story ids**, in the order the stories appear in `TEXT`. These are the numbers that object records refer to (`contents-stream.md` §6, key 0x27). |

**`STRS` — story lengths.**

| Offset | Size | Meaning |
|---|---|---|
| 0 | 4 bytes, unsigned | **Count** of lengths. |
| 4 | 4 bytes, unsigned | Where the lengths start, counted from offset 4 (8 in every sample, so the lengths start at offset 12). |
| 8 | 4 bytes | 0xFF in every sample. Not read. |
| 4 + (value at 4) | 4 bytes × count | **Length of each story in UTF-16 units**, including its final CR. |

Pair the i-th id with the i-th length. Story i occupies the units from the sum of the earlier lengths up to that plus its own length. Clamp to the size of `TEXT` (a story running past the end is cut short). If one list is longer than the other, the extra entries are ignored. Cap the counts read (the existing implementation stops at 100 000).

*Source: existing code; confirmed: `text2__two-boxes.pub` `SYID` = count 2, ids 2, 3; `STRS` = lengths 45 and 12 units ("First paragraph here." CR "Second paragraph here." CR = 45, "Another box" CR = 12).*

## 5. `FONT`: the font list

| Offset | Size | Meaning |
|---|---|---|
| 0 | 4 bytes | Not read. |
| 4 | 4 bytes, unsigned | **Count** of fonts. |
| 8 | 12 bytes | Not read. |
| 20 | 4 bytes × count | Offset of each font entry, counted from offset 20. Not read by the existing implementation (it walks the entries instead), but it agrees with the walk in the samples. |
| 20 + 4 × count | … | **Font entries**, back to back. |

Each font entry: 2 bytes = name length in UTF-16 units; the name (UTF-16LE, no terminator); then 4 more bytes (not read; they vary). Fonts are numbered 0, 1, 2… in order. Cap the count (the existing implementation stops at 10 000).

*Source: existing code; confirmed: `text__bold.pub` has 32 fonts starting `Calibri`, `Sylfaen`, `Tahoma`; the offset table reads 0x80, 0x94, 0xA8 (+20 = the entries' real positions). The first synthetic sample has one font, `Arial`.*

## 6. `PL  `: the text colour table

| Offset | Size | Meaning |
|---|---|---|
| 0 | 4 bytes, unsigned | **Count** of colours. |
| 4 | 8 bytes | Not read (0x28 then 0 in the samples). |
| 12 | … | **Colour entries**, back to back. |

Each colour entry is a length-prefixed list (`property-lists.md`). Key 0x01 (kind 0x22 in samples) = a **colour reference** (`contents-stream.md` §8). An entry without key 0x01 is colour reference 0 (black). Step to the next entry by the entry's length; if the length is below 4, step by 4 so a damaged entry can't stall the reader. Colours are numbered 0, 1, 2… in order. Cap the count (10 000).

*Source: existing code; confirmed in `text__run-red-word.pub`: three entries; entry 0 = 0x08000000 (palette colour 0), entry 1 = 0x00000000 (black), entry 2 = 0x000000FF (red).*

## 7. `STSH`: default styles

Publisher writes three `STSH` sections. The **second** one (by order in the directory) holds the defaults for all text:

| Offset | Size | Meaning |
|---|---|---|
| 0 | 4 bytes | Not read. |
| 4 | 4 bytes, unsigned | **Count** of style entries (0x48 = 72 in Publisher files; 0 in the first synthetic sample). |
| 8 | 12 bytes | Not read. |
| 20 | 4 bytes × count | Offset of each style entry, counted from offset 20. |

Each style entry, at (section offset + 20 + its offset), is 2 bytes (not read) followed by a **style record**: a length-prefixed list. Only the first two entries are read:

- entry 0 = **default character format** (keys as in §9);
- entry 1 = **default paragraph format** (keys as in §10).

If the second `STSH` is missing, or has fewer entries, the defaults are empty. Any value a text run doesn't set comes from these defaults.

*Source: existing code; confirmed in `text__bold.pub`: count 72, offsets 0x120 and 0x2C6; the default character record sets size 0x1F018 EMU = 10 pt and font 0 (`Calibri`); the default paragraph record sets space after 0x129A8 EMU = 6 pt.*

## 8. `FDPC` and `FDPP`: formatting pages

Character formatting (`FDPC`) and paragraph formatting (`FDPP`) are stored in 512-byte **formatting pages**; there can be several of each. Each page:

| Offset in page | Size | Meaning |
|---|---|---|
| 0 | 2 bytes, unsigned | **Number of runs** n. |
| 2 | 2 bytes | 1 in the samples. Not read. |
| 4 | 4 bytes | 0 in the samples. Not read. |
| 8 | 4 bytes × n | **End of each run**: a byte offset **in the stream** (not in `TEXT`). |
| 8 + 4n | 2 bytes × n | **Where each run's format record is**: an offset from the start of this page. |

A format record is a length-prefixed list (length 4 = no formatting, use the defaults).

To use the runs: gather the runs of all pages of one kind and sort them by end. Convert an end to a text position as (end − offset of `TEXT`) ÷ 2. Each run covers from the previous run's end (or the start of `TEXT` for the first) up to its own end, exclusive. To find a story's runs, cut each run to the story's range and drop empty pieces; positions inside a story count from the story's first unit. The last run usually ends exactly at the end of `TEXT`.

*Source: existing code; confirmed: `text__run-red-word.pub` `FDPC` = 4 runs ending at 0x20C, 0x21A, 0x230, 0x232 (`TEXT` at 0x200, so units 6, 13, 24, 25: "Hello ", "Example", " Text world", CR); the second run's record carries the colour.*

## 9. Character format keys

| Key | Kind | Meaning |
|---|---|---|
| 0x02 | 0x0A (no data) | **Bold** on. |
| 0x03 | 0x0A (no data) | **Italic** on. |
| 0x1E | 0x12 | **Underline**: non-zero = on, zero = off. If written with no data, on. |
| 0x0C | 0x22 | **Font size** in EMU (12 700 per pt). |
| 0x24 | 0x8A | **Font**: a nested list of nested lists; the first value inside its **first kind-0x88 entry** is the font number in the `FONT` list. |
| 0x44 | 0x8A | **Colour**: a nested list; its key 0x00 value is an index into the `PL  ` colour table. |
| 0x2E | number | **Colour** as a plain index into the `PL  ` table (an alternative to 0x44). |

Bold and italic can only be switched on by a run; a run without them inherits the default. A value that's absent inherits the default character format (§7).

Historical observations (caps/position/tracking are now handled as described in §12): 0x12 and 0x3E (kind 0x22, value 0x409 in most runs: a language code, US English), 0x13 (`text__smallcaps.pub`), 0x14 (`text__allcaps.pub`), 0x0F (`text__superscript.pub`, value 1), 0x1F (kind 0x1A in `text__tracking-200.pub`, value 0x7D0 = 2000), 0x58 (with every colour), 0x39 and 0x48 (in the default record).

*Source: keys and meanings from the existing code; confirmed: bold in `text__bold.pub`, italic in `text__italic.pub`, underline (kind 0x12, value 1) in `text__underline.pub`, sizes in `text__size-*.pub` (e.g. 0x4A6A0 = 304 800 EMU = 24 pt), font in `text__font-Arial.pub` (font 0x20), colour in `text__run-red-word.pub` (0x44 → index 2). Key 0x2E and the "underline with no data" case come from the existing code only: no sample contains them.*

## 10. Paragraph format keys

| Key | Kind | Meaning |
|---|---|---|
| 0x04 | 0x12 | **Alignment**, low byte: 0 = left, 1 = right, 2 = centre, 6 = justified. Anything else = left. |
| 0x12 | 0x22 | **Space before** in EMU. |
| 0x13 | 0x22 | **Space after** in EMU. |
| 0x0C | 0x22 | **First-line indent** in EMU, **signed** (negative = hanging indent). |
| 0x0D | 0x22 | **Left indent** in EMU, signed. |
| 0x0E | 0x22 | **Right indent** in EMU, signed. |

Spacing values are read as signed too. A paragraph run covers its paragraphs the same way character runs cover characters. Absent values inherit the default paragraph format (§7).

Historical observations before task 4.1 (superseded for bullets and spacing by §11): 0x34 (kind 0x22, line spacing: values in `text__ls-single.pub` 0x129A82, `text__ls-double.pub` 0x253502, `text__ls-multiple-1.5.pub` 0x1BE7C2, `text__ls-exact-14.pub` 0x15B441, `text__ls-exact-30.pub` 0x2E8241; the low byte looks like a mode, 0x02 = multiple of single, 0x41 = exact, and the upper bytes a size), 0x02 (0x1F018) / 0x03 (0x1F) / 0x57 (`text__bullets.pub`), 0x3C (`text__align-justify.pub`, 0x4832), 0x30 (in the default record).

*Source: keys and meanings from the existing code; confirmed: alignment in `text__align-center.pub` (2), `text__align-right.pub` (1), `text__align-justify.pub` (6); left indent 0x37CF8 = 228 600 EMU = 18 pt in `text__indent-left-18.pub`; first-line indent 0xFFFC8308 = −18 pt in `text__bullets.pub`; space before 0x25350 = 12 pt in `text__space-before-12.pub`. The line-spacing reading is a sample observation only.*

## 11. Observations: paragraph lists and line spacing

Compared only the binary Quill streams of the 142 pubsmith `.pub` fixtures at
`0ccdf27`; no reader source or truth JSON was used. `text__base.pub` has an empty
paragraph-run record. The following differences therefore belong to the changed
paragraph setting, rather than to text or character formatting.

### Bullets

`text__bullets.pub` adds these paragraph properties:

| Key/kind | Value | Interpretation supported by the sample |
|---|---|---|
| 0x02 / 0x22 | 127000 | Marker size, 10 pt in EMU; same as the text size in this sample. |
| 0x03 / 0x1A | 31 | Marker font index; `FONT[31]` is `Symbol`. |
| 0x0C / 0x22 | -228600 | Hanging first-line indent, -18 pt. |
| 0x0D / 0x22 | 228600 | Text left indent, 18 pt. |
| 0x57 / 0x8A | nested list | List-marker descriptor, fields below. |

The descriptor has numeric keys 0x00 = 23 (0x17), 0x01 = 183 (0xB7),
0x02 = 0. This combination identifies the observed bullet: preserve the marker
character U+00B7 and the `Symbol` font, rather than inserting a literal marker
into the story. Value 23 is only verified as a bullet descriptor; numbering
values, restart/start-at fields and key 0x02's meaning are **not established**.
Do not infer those from a superficially similar format. The symbol character
is retained as encoded; no Symbol-to-Unicode glyph transcoding is established.
Absent descriptor fields inherit defaults; an unknown descriptor is not a
verified explicit “no list” override. Only descriptor value 23 is imported.

### Line spacing

Paragraph key 0x34 / kind 0x22 carries a packed unsigned number. These exact
observations replace the tentative low-byte interpretation in §10:

| Fixture | Stored decimal / hex | Decoded setting |
|---|---|---|
| `text__ls-single.pub` | 1219202 / 0x129A82 | multiple 1 |
| `text__ls-double.pub` | 2438402 / 0x253502 | multiple 2 |
| `text__ls-multiple-1.5.pub` | 1828802 / 0x1BE7C2 | multiple 1.5 |
| `text__ls-exact-14.pub` | 1422401 / 0x15B441 | exactly 14 pt |
| `text__ls-exact-30.pub` | 3048001 / 0x2E8241 | exactly 30 pt |

For the observed settings, the low **two bits** distinguish mode 1 (exact)
from mode 2 (multiple). Remove those two bits. Exact points = remaining value
/ (8 × 12700); multiple = remaining value / 1219200. The second STSH's default
value 1450850 (0x162362) then gives multiple 1.19. Masking the entire low byte
would discard size bits (for example exact 30 has bit 6 set). Other modes,
minimum spacing, and additional flags have not been established and must not
be guessed. Zero or implausibly large decoded spacing is rejected as a usable
setting. Map exact spacing to the document model's point leading and multiples
to its automatic-leading factor. That model measures the factor against font
size; identical Publisher font-metric layout has not been measured.

### Numbering and custom tabs: missing evidence

A scan of every FDPP record in all 142 fixtures finds no other list descriptor
and no custom-tab-setting variant. None of their TEXT sections contains U+0009.
The current `main` fixture-directory listing also contains exactly these 142
`.pub` paths. Numbering and tab-stop byte layouts remain unknown. Needed:
otherwise-identical publications with no list / numbering (including style and
start changes), and default / custom tabs (position, alignment, leader).
Existing U+0009 story characters are already preserved; this does not establish
a custom tab-stop encoding. A synthetic test can verify preservation of U+0009,
but cannot serve as evidence for an invented file-format layout.

### Indents and paragraph spacing verified for 4.1

The three indent-only samples each add one kind-0x22 value 228600 (18 pt):
`text__indent-first-18` key 0x0C, `text__indent-left-18` key 0x0D,
`text__indent-right-18` key 0x0E. The bullet fixture's negative 0x0C confirms
signed decoding. `text__space-before-12` adds key 0x12 = 152400 (12 pt), while
`text__space-after-0` adds key 0x13 = 0, overriding the second STSH default
76200 (6 pt). Explicit zero must therefore override, rather than inherit.
These fields are decoded; synthetic tests cover them end to end.

## 12. Observations: character formats

Compared FDPC records in `text__base.pub` with the single-setting variants at
pubsmith `0ccdf27`. The base has language keys 0x12/0x3E = 0x409. Differences:

| Fixture | Added property | Confirmed import |
|---|---|---|
| `text__allcaps.pub` | key 0x14, kind 0x0A (presence) | Native all-caps character attribute. |
| `text__smallcaps.pub` | key 0x13, kind 0x0A (presence) | Native small-caps attribute. |
| `text__superscript.pub` | key 0x0F, kind 0x12, value 1 | Native superscript position. |
| `text__tracking-200.pub` | key 0x1F, kind 0x1A, value 2000 | Native tracking 200 (thousandths of an em); stored value divided by 10. |

Do not uppercase the story text itself. Caps presence inherits through the same
STSH/run mechanism as bold and italic. The samples establish no explicit caps
off value, no subscript value, and no strikethrough key. Import only the observed
superscript value 1; warn on other position values. Read the tracking field's
16-bit signed payload (the observed value is positive); negative tracking and
explicit zero have only synthetic coverage. Other kinds are unconfirmed.

Unrecognized character properties produce one deduplicated warning, covering
future or unconfirmed features (including strike or subscript) without inventing
their encoding. The language keys 0x12/0x3E are known metadata and are skipped.
The default STSH also carries fields not yet decoded; those produce the same
warning. No fixture proves a strikethrough encoding; do not guess a key.

## 13. Warning audit and redundant colour descriptors

The 6.4 CSV has 88 files with each “unsupported default character/paragraph”
warning and 54 files with each blanket character/paragraph warning. Binary
section-directory inspection confirms that precisely those 88 files have **no
Quill TEXT section**. None of those 88 Contents streams references a Quill story either. Their
STSH defaults describe no imported text; reporting
those defaults as lost formatting is noise. Return without style warnings when
TEXT is absent, and warn only for unsupported fields on nonempty mapped stories.
This does not suppress drawing, WordArt, transparency, scratch-area or table
warnings.

Across all 142 default character records, the still-unconfirmed keys are 0x39,
0x48 and 0x58; paragraph defaults contain key 0x30. Character runs add 0x58 in
colour variants, while the justify paragraph adds key 0x3C. The independent
size/font/colour variants do **not** establish meanings for 0x39, 0x48, 0x30 or
0x3C. Keep warnings for those fields when text uses them; identify actual field
numbers instead of asserting that unrelated unsupported features occurred.
Deduplicate fields from defaults/runs into one bounded diagnostic per kind.

### Key 0x58: observed redundant colour descriptor

Kind 0x8A contains exactly three numeric fields: 0x00 = 1, 0x01 = the same PL
colour index as the record's key 0x44 (or plain colour key 0x2E), and 0x02 =
100000. This exact pattern appears in all 142 STSH default records (index 0)
and five colour-run records (index 2) in the allowed fixtures at `0ccdf27`.
The colour-change samples show 0x58's index changing together with key 0x44;
there are no mismatches. The existing importer already applies this PL colour.
Recognize the exact matching descriptor as redundant, independently of property
order; do not warn about importing that colour twice. No extra colour transform
is established: mismatched indices, extra/duplicate keys, different mode/scale,
or an unpaired descriptor remain unsupported and warn. Do not extrapolate a tint,
opacity or alternative colour encoding from the constant 100000.

The remaining top-five warning is table cell styling (three files). The audit
in `tables.md` still supplies no controlled fill/border variants, so keep it.

## 14. Diagnostic labels for documented fields

No new encoding is inferred here. If an existing decoder rejects a property on
an established key, its warning may identify the documented feature: character
0x13 small caps, 0x14 all caps, 0x0F superscript/position and 0x1F letter spacing;
paragraph 0x34 line spacing and 0x57 list-marker descriptors. Labels describe
incomplete import of that field, not a new interpretation of its value. Merge
default/run field sets before labeling to avoid repeated labels; retain the
bounded numeric key lists and one diagnostic per formatting kind.

The meanings of character defaults 0x39/0x48 and paragraph default 0x30 remain
unconfirmed (§13). Unknown list descriptors must not be called numbering; no
key may be called custom tabs or strikethrough without describer evidence.
Unknown fields still warn numerically, including defaults applied to text.
Supported bullets, spacing, caps, superscript and tracking are retained, and
literal U+0009 is preserved; those cases need no loss warning. Numbering and
custom-tab import remain blocked on the describer's missing evidence in §11.
