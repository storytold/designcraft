<!-- Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0 -->
# Drawing observations: warning audit, round 2

## Shape shadows (2026-10-18)

Compared only `shadow__on.pub`, `shadow__offset.pub` and `wordart__shadow.pub`
from the allowed binary fixtures at `0ccdf27`, before implementation. Both
shape-shadow files have primary 0x023F = 0x00020002: use bit 17 and enabled
bit 1. Their tertiary 0x023F = 0x00040000 does not override those bits; retain
the existing per-use-bit Escher merge. Both specify shadow colour 0x0201 =
0x08000000 (palette entry zero, black). `shadow__offset.pub` additionally has
0x0205 = 0x0206 = 0x000129A8: 76200 EMU, or 6 points right/down. Neither
contains an explicit shadow type, transform or opacity. Their Publisher truth
exports flatten each shape and shadow to one image, not separate editable
shadow properties; these exports cannot establish additional shadow types.

Extend the retained WordArt shadow mapping to ordinary items: 0x0201 colour,
0x0204 16.16 opacity, 0x0205/0x0206 signed EMU offsets. The existing WordArt
defaults remain: opaque, 2-point X/Y offsets, grey if colour is absent.
This reuses existing importer rules; the shape samples do not independently
establish a non-default opacity or absent-colour default. Synthetic tests cover
those existing rules, while both real shape-shadow samples cover the newly
connected import path. A native sharp drop shadow uses size/spread zero,
distance `hypot(dx,dy)` and light angle `atan2(dy,-dx)` (the renderer maps that
angle back to offset `(-cos(angle), sin(angle)) * distance`). It does not
change the item's bounds, fill, border or text.

Only the default simple shadow type (absent 0x0200, or explicit zero) is
supported. No fixture confirms double, perspective, embossed or other types;
warn and omit those rather than collapsing them to a drop shadow. Likewise
warn/omit shadows with additional unconfirmed properties in 0x0202–0x023E
outside the supported opacity/offset fields. Disabled shadows do not warn.
Offset magnitudes retain the existing 1000-point safety cap; warn if clipping
is necessary. No new file reads or input-sized allocations are needed.

Binary observations below use only the allowed pubsmith `.pub` fixtures at
`0ccdf27`. No external reader or truth file was consulted. The Microsoft
MS-ODRAW specification landing page was requested, but the proxy returned 403;
no specification text was obtained. These observations precede implementation.

## Solid fill opacity

The visible rectangle in `fill__transparency-50.pub` has OfficeArt property
0x0181 = 0x00FF0000 (blue in the already documented BGR colour representation)
and **0x0182 = 0x00008000**. It has the same enabled solid-fill and line flags
as the rectangles in `fill__fill-grey.pub` and `fill__fill-red.pub`, which omit
0x0182. The 50% setting agrees with the 16.16 fill-opacity interpretation already
used for gradient endpoints by this repository's retained drawing importer:
0x10000 is opaque, 0x8000 is half opaque, and zero is transparent.

Import enabled solid fills with opacity below one using a native linear
gradient with two identical colour/opacity stops. This represents uniform
fill transparency exactly with the existing model: `Fill` has no opacity
field, but gradient stops do. Do not set `Item.opacity`: that would also fade
the independently opaque border and any text/content. Preserve existing solid
swatches for absent/opaque opacity, and keep disabled/picture-fill behavior.
Do not apply this newly confirmed solid-fill mapping to unconfirmed pattern or
picture fills. The conversion uses two stops regardless of input size.

## Remaining drawing warnings

| Fixtures | Observation and decision |
|---|---|
| `shapes__polyline.pub`, `shapes__curve.pub` | Both have pVertices (0x0145), pSegmentInfo (0x0146), and geometry bounds. Polyline has four vertices and words 0x4000, 0xAC00, 0x0003, 0xAC00, 0x6001, 0x8000. Curve has four vertices and words 0x4000, 0xAD00, 0x2001, 0xAD00, 0x8000. The extra 0xAC00/0xAD00 words are not interpreted by the importer. Their semantics are unconfirmed by these samples; do not silently discard them. Keep the rectangle-fallback warning. |
| `shapes__heart.pub` | The visible shape is type 74, with no explicit vertex/segment arrays. A type number does not establish its exact outline. Keep the rectangle-fallback warning rather than inventing control points. |
| `wordart__arch.pub`, `wordart__circle.pub` | Geometry-text strings/font properties match the straight `wordart__base.pub` structure, but shape types are 144 and 146 rather than 136. The samples do not establish an exact glyph warp mapping. Keep the straight-text warning. |
| `pages__size-5.75x4.75.pub` | The publication still contains scratch-area objects. Skipping them is an actual omission, so retain the warning. |
| `wordart__shadow.pub` | The native shadow is already imported. The batch warning comes from PDF export rasterizing a soft effect; it describes a real output representation change, not an unknown Publisher field. Retain it. |

Table cell paint remains unconfirmed for all three table-bearing fixtures;
see [tables.md](tables.md#cell-fills-and-borders-unconfirmed). None provides a
differentiated cell-shading/border setting. The text defaults 0x39/0x48 and
paragraph default 0x30 remain unconfirmed as described in
[text-store.md §13](text-store.md); the only additional paragraph run field is
0x3C, kind 0x12, value 18482 in `text__align-justify.pub`. Supported justification
is already retained; that additional field's meaning is not established.
Neither constant defaults nor a single unexplained run establish harmless
metadata, so their precise field warnings remain.
