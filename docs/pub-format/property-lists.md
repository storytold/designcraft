<!-- Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0 -->
# Property lists (shared encoding)

Both the `Contents` stream and most sections of the Quill text store hold their data as **property lists**: a run of **properties** laid end to end. Each property is:

| Offset | Size | Meaning |
|---|---|---|
| 0 | 1 byte | **Key**: what the property means. Keys are only meaningful together with the record they sit in (the same key means different things in a page record and in a text-run record). |
| 1 | 1 byte | **Kind**: how much data follows (table below). |
| 2 | depends on kind | Data. |

All numbers in this format are little-endian. *Source: existing code; confirmed in every sample (see `spot-checks.md`).*

## Kinds and their data size

| Kind bytes | Data size | Data |
|---|---|---|
| 0x00, 0x02, 0x03, 0x04, 0x05, 0x08, 0x0A, 0x0F, 0x78 | 0 bytes | None. The property's presence is the information (for example "bold is on"). |
| 0x07, 0x10, 0x12, 0x18, 0x1A | 2 bytes | One unsigned 16-bit number. |
| 0x20, 0x22, 0x58, 0x68, 0x70, 0xB8 | 4 bytes | One 32-bit number. Usually unsigned; indents are signed (see `text-store.md`). |
| 0x28 | 8 bytes | Opaque; never read. |
| 0x38 | 16 bytes | Opaque; never read. |
| 0x48 | 24 bytes | Opaque; never read. |
| 0x80, 0x82, 0x88, 0x8A, 0x90, 0x98, 0xA0 | variable | **Nested list.** The data starts with an unsigned 32-bit **byte length that counts itself**; the remaining bytes (length − 4) are another property list. |
| 0xC0 | variable | **Text.** Same 32-bit self-counting length, then UTF-16LE text. The text normally ends with a 0x0000 unit, which is not part of the value. |

*Source: the size table is the one the existing code uses. A survey of all 142 pubsmith samples found every kind in it except 0x82 in use (0x0A, 0x12, 0x1A, 0x22 and 0x8A only in the Quill text store; 0xB8 only in the record directory). Kind 0x82 comes from the existing code only.*

In the samples, every kind-0xA0 list the reader uses (page lists, object lists, palette entries) is an **array** whose entries all have key 0x00; kind-0x88 lists hold distinct fields. This is an observation, not something the reader relies on.

## Rules for reading

- A list is read from its start offset up to its end offset. Stop when fewer than 2 bytes remain.
- A nested list whose length is below 4 is malformed.
- A property whose data would run past the end of the enclosing list (or the stream) is malformed.
- An **unknown kind cannot be skipped**, because its size is unknown; stop reading and treat the list as malformed. (No sample contains a kind outside the table.)
- Unknown **keys** are harmless: skip them using their kind's size. Every record in the samples contains many keys that are never read.
- Put a sane cap on the number of properties per list (the existing implementation stops at one million) so a damaged file can't exhaust memory.

## Self-counting length prefix

Several structures (a `Contents` record, a Quill style record, a nested list) start with an unsigned 32-bit length that **includes its own 4 bytes**, followed by a property list. A length of exactly 4 means "an empty list". This is written below as "a length-prefixed list".
