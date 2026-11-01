# Preset shape and WordArt geometry (ECMA-376)

Source: the public ECMA-376 standard, 5th edition (2016), Part 1, downloaded from ecma-international.org. Two files from it were read on 2026-10-24: presetShapeDefinitions.xml (187 preset shapes) and presetTextWarpDefinitions.xml (40 text warps). It is allowed under docs/PUB-AGENTS.md section 2 on the owner words recorded there. It is a public standard, not a .pub reader, and says nothing about how .pub files store anything. This note is a paraphrase in our own words. Labels: SPEC (stated in the files), INFERENCE (our reasoning), UNKNOWN.

## How a preset shape is defined
SPEC: each shape has (a) a list of named formulas (guides) computed from the shape width w, height h and helper values such as hc (half the width, the horizontal centre), vc (vertical centre), hd2, hd3, hd4 (half, third and quarter of the height), and the box edges l, t, r, b; (b) one or more paths built from moveTo, lineTo, cubicBezTo, arcTo and close; (c) a text rectangle and connection points. Formula operators seen so far: "*/ a b c" means a times b divided by c, and "+- a b c" means a plus b minus c. Others (pin, sin, cos, cat2, sat2, and a conditional) appear in the text warps; read their meaning from the standard before using them.

## Heart (MS-ODRAW shape number 74)
SPEC, from the definition named heart:
- dx1 = w x 49 / 48, dx2 = w x 10 / 48.
- x1 = hc - dx1, x2 = hc - dx2, x3 = hc + dx2, x4 = hc + dx1.
- y1 = t - h/3 (above the top edge of the box).
- Path: move to (hc, h/4). Cubic curve with control points (x3, y1) and (x4, h/4), ending at (hc, b). Second cubic curve with control points (x1, h/4) and (x2, y1), ending at (hc, h/4). Close.
- Text rectangle: left w/6, top h/4, right 5w/6, bottom 2h/3.
INFERENCE: named formulas and path pieces provide a shared way to describe presets. This does not establish support for the other 186 definitions: their operators, paths and shape-number mappings need separate checks. The current interpreter supports only the heart.

## WordArt text warps
SPEC: 40 text-warp shapes are defined: textArchDown, textArchDownPour, textArchUp, textArchUpPour, textButton, textButtonPour, textCanDown, textCanUp, textCascadeDown, textCascadeUp, textChevron, textChevronInverted, textCircle, textCirclePour, textCurveDown, textCurveUp, textDeflate, textDeflateBottom, textDeflateInflate, textDeflateInflateDeflate, textDeflateTop, textDoubleWave1, textFadeDown, textFadeLeft, textFadeRight, textFadeUp, textInflate, textInflateBottom, textInflateTop, textPlain, textRingInside, textRingOutside, textSlantDown, textSlantUp, textStop, textTriangle, textTriangleInverted, textWave1, textWave2, textWave4.
SPEC, textArchUp as the example: one adjust value (default 180 degrees, written as 10800000 in 60000ths of a degree) sets where the arc starts; the formulas turn that into a start point on the shape ellipse; the path is a move to that point followed by an arc using half the width and half the height as radii. So the shape gives the PATH that the text follows.
UNKNOWN: how each letter is stretched, turned and spaced along that path is not spelled out in these files. Decide it by comparing renders with the arch and circle fixtures and the scoring-only reference images, and record what stays UNKNOWN.
INFERENCE for the name mapping: TextArchUpCurve (144) is textArchUp, TextArchDownCurve (145) is textArchDown, TextCircleCurve (146) is textCircle, TextButtonCurve (147) is textButton. Confirm on the fixtures before relying on it.

OBSERVATION: the pinned `wordart__arch.pub` stores 144 and `wordart__circle.pub` stores 146, with no stored adjustments. Their mapping to the corresponding ECMA default paths remains an INFERENCE from the names and measured renders; the two numbers are confirmed by the binary audit. UNKNOWN: fixture confirmation of 145 or 147. Default upward arch now uses existing editable path text fitted from measured font advances. Circle did not meet its reference baseline in the measured trials and retains straight text and a warning, as do other warps. See [wordart.md](wordart.md) for the audit, geometry checks, scores and remaining limits.

## Heart outline in the importer

OBSERVATION: downloaded the official fifth-edition Part 1 archive from ecma-international.org. Opened only `presetShapeDefinitions.xml` inside its geometry archive, not the text-warp file, PDF or schemas. The file contains 187 preset definitions. The `heart` entry confirms every outline formula and control point in the heart section above; no numeric correction was needed. Its three additional guides (`il`, `ir`, `ib`) describe the text rectangle, not the outline. Text insets and connector positions are not implemented by this change.

The small interpreter in `crates/pub/src/presets.rs` evaluates named guides in order, then emits a move, cubic curves and a close. For the confirmed arithmetic subset, `*/ a b c` multiplies a and b and divides by c; `+- a b c` adds a and b and subtracts c. These are the two operators already described above and used by the official heart entry. No other operator is implemented or claimed as checked. Width and height are the unrotated frame dimensions; coordinates are translated by the frame origin after evaluation. Negative control coordinates and controls beyond the frame are retained, because they determine the lobes. Existing item transforms still apply rotation and flips.

OBSERVATION: at width 96 and height 120, the guides give dx1=98, dx2=20, x1=-50, x2=28, x3=68, x4=146 and y1=-40. At frame origin (10,20), the outline starts at (58,50), curves through (78,-20) and (156,50) to (58,140), then through (-40,50) and (38,-20) back to (58,50). Unit tests check these hand-computed values, doubled dimensions, invalid arithmetic and invalid frames. Unknown guides, unsupported operators, division by zero and non-finite results return errors. No XML is bundled or read at runtime.

The only newly mapped preset is Heart, MS-ODRAW number 74, confirmed by the pinned `shapes__heart.pub` audit. It now becomes an editable closed path with two cubic curves. If its outline cannot be evaluated, the rectangle fallback gives a plain warning. Other unmatched shape numbers keep their previous behavior and warnings. Star5, diamond, hexagon, right arrow, triangle and rounded rectangle remain hand-coded and unchanged; no measured difference from the standard was established for them.

UNKNOWN: matching Publisher's own drawing has not been tested. The standard outline and the fixture's stored shape number establish the chosen outline, not pixel agreement with Publisher. Windows and non-default preset adjustments were not tested. The text-warp definitions were not used for the heart.

