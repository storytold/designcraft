<!-- Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0 -->
# Opening Microsoft Publisher files

DesignCraft opens Microsoft Publisher 2002–2021 publications (`.pub`) as a **converted, unsaved
copy**. The original file is only read, never written: Save asks for a new `.designcraft` file.
The importer is the `designcraft-pub` crate (`crates/pub`, layer 3); `file.open` and
`file.openBytes` reach it, so the desktop app, the web app, the CLI, the control channel and MCP
all open `.pub` the same way. The format notes it is written from are in
[pub-format/](pub-format/README.md).

Every open returns:
- `warnings`: one line per kind of loss (an object, picture or setting that did not come over, or
  came over approximated). Unsupported objects are reported, never dropped silently.
- `notes`: information that is not a loss, such as formatting fields this version does not
  understand (with their field numbers) and text that does not fit its frame.
- `notice`: a fixed sentence the app shows above the document, because not every difference from
  what Publisher shows can be detected and warned about.

## What comes over

- Pages, page size and master pages (as parent pages, applied to the pages that use them).
- Text frames with their stories: fonts by name, bold/italic, size, underline, text colour,
  capitalization, superscript/subscript position, tracking; alignment, space before/after,
  indents, bullets, exact and multiple line spacing; insets and vertical alignment. Linked frames
  are threaded in chain order, the story imported once.
- Shapes with their position, size, rotation and flips: rectangles, rounded rectangles, ovals,
  the simple polygon presets, lines, freeform curves and the heart preset (ECMA-376 outline). Other
  shape types become a rectangle of the same size, with a warning.
- Fills (solid, fill transparency, shaded fills as gradient swatches), lines (weight, colour,
  dashes, arrowheads) and the simple shape shadow.
- Groups, nested too, with their rotation.
- Pictures as stored: PNG, JPEG, TIFF and bitmaps (DIB) with rectangular crops; WMF and EMF
  pictures as vector SVG.
- WordArt as editable text with its font, style, alignment and letter fill and line. The default
  upward arch follows its curved path; other warps come over straight.
- Tables with their row and column sizes, merged cells and cell text.

## What does not (each reported as a warning or a note)

- Publisher 97, 98 and 2000 files: refused with a message to re-save in Publisher 2002 or later.
- Table cell fills, borders and padding; WordArt warps other than the default arch, and exact
  letter stretching; pattern and texture fills (approximated by the fill colour); picture fills;
  crops other than rectangular; PICT pictures (an empty graphic frame keeps their place); text,
  bitmaps, arcs and pattern fills inside WMF/EMF pictures; custom tab stops; numbering; fields such
  as page numbers and dates; shadow types other than the simple one.
- Fonts: the document asks for the fonts Publisher used, by name. Microsoft fonts that are not
  installed show in DesignCraft's fallback, so text can set wider or narrower than in Publisher;
  the open does not pop up Find/Replace Font (Type ▸ Find/Replace Font stays available).
- Text that does not fit its frame in DesignCraft is listed in `notes`; Publisher may fit it
  differently.

## Limits on hostile input

Files over 512 MB and streams over 256 MB are refused. Counts read from the file (pages, records,
properties, table cells, metafile records) are capped before anything is allocated for them,
groups nest at most 16 deep, and every read is bounds-checked: a damaged file is an error that
names the format, never a crash.

## Provenance

- The importer was written without any other Publisher reader. Nobody working on it read
  libmspub, Apache POI's Publisher code, or the source code of any other `.pub` reader or writer,
  nor descriptions of how they behave.
- The notes in [pub-format/](pub-format/README.md) were written from the bytes of the data files
  listed there and from public specifications (MS-CFB, MS-ODRAW, MS-WMF, MS-EMF, and two ECMA-376
  definition files for shape outlines only). The person writing the notes was allowed to read an
  earlier reader of this project; the `Contents`, Quill and bounded-reader modules
  (`contents.rs`, `quill.rs`, `read.rs`) were then rewritten from the notes alone by someone who
  never saw that earlier code. The other modules were written and extended from the notes and the
  public specifications.
- `truth/*.json` exports that come with the controlled samples were used only to score imports,
  never to work out how anything is encoded.
- No Publisher-authored file is committed. The committed test files under
  `crates/pub/tests/data/` are synthetic: the importer's tests write every byte of them from
  scratch (`crates/pub/tests/support/mod.rs`), and a test checks that each committed file is
  exactly that output. They are listed in `ASSETS.md`.
- Some early observations of the compound-file container, cited in the notes as "the first
  synthetic sample", came from a small test file written by a different tool. Only its bytes were
  examined (data only, no code), and it is not committed.

## How it is checked

- Unit and synthetic-file tests in `crates/pub` and the engine run with `cargo xtask ci`.
- Tests marked `#[ignore]` need the external samples: `PUB_CORPUS=dir` (the controlled samples)
  and `PUB_REAL_CORPUS=dir`. They are never committed. Over the 142 controlled samples the importer
  opens 142/142; 111 of 111 scored shape and text positions are within 1 pt of the `truth` exports,
  52/52 texts and 111/111 styles match. Of the 24 real-world samples, 2 are refused (Publisher
  2000 and older) and 3 are reported as damaged; the rest open, 10 of them with warnings.
- Not checked: Publisher 97/98/2000 files (none available), Publisher 2002–2019 files beyond the
  few real-world samples, and how text sets with the Microsoft fonts installed.
