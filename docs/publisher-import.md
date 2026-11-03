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

Path-based opens load adjacent Document Fonts through the existing read-root checks before
building text and computing overset diagnostics. Their scope is retained only by that document;
byte-only imports do not load adjacent files. Skipped font files are reported as warnings.

## Limits on hostile input

Files over 512 MB and streams over 256 MB are refused. Counts read from the file (pages, records,
properties, table cells, metafile records) are capped before anything is allocated for them,
groups nest at most 16 deep, and every read is bounds-checked: a damaged file is an error that
names the format, never a crash.

Contents directory parsing also has an aggregate 64 MiB byte-work budget and one million
properties, counting repeated record references. Retained picture payloads have a shared
256 MiB budget checked before copying. Exhausting either budget refuses the import rather than
silently returning an incomplete publication. Individual malformed picture warnings are capped.
WMF/EMF payloads, compressed or uncompressed, are limited to 64 MiB. Conversion retains at most
32 MiB of SVG/path data; reaching that or the record-work limit explicitly warns that remaining
drawing was omitted. These are parser/conversion limits, not a total process-memory guarantee.

## Provenance

- **Who wrote it.** The importer, its tests and the format notes were written by AI coding assistants (Claude Code and Codex) working under the direction of the author, srt8hellcat2-ui. No other Publisher reader was used. Nobody working on it read libmspub, Apache POI's Publisher code, or the source code of any other `.pub` reader or writer, nor descriptions of how they behave. What the AI models saw in their own training cannot be audited, and this document makes no claim about it.
- **Format notes.** The notes in [pub-format/](pub-format/README.md) were written by one assistant session (the describer, 2026-10-17) from the bytes of open sample `.pub` files, listed there. The describer named the public specifications MS-CFB and MS-ODRAW but did not open their text, so the notes are not a restatement of those specifications. The describer was allowed to read an earlier reader of this project, described next. The full record is `docs/pub-format/SOURCES.md` in srt8hellcat2-ui/niftydesign.
- **Earlier reader.** A first `.pub` parser written for this project by AI assistants under the author's direction, from open sample files only: repository srt8hellcat2-ui/designcraft-pub-development, branch `feat/pub-import`, commit `93ee76e`. It is a development repository and is not part of this pull request. It contains no third-party code; its author and licence are the same as this pull request's (MIT OR Apache-2.0).
- **Rewrite.** The `Contents`, Quill and bounded-reader modules (`contents.rs`, `quill.rs`, `read.rs`) were rewritten from the notes alone by a different assistant (the builder, Codex), in a repository where those original files had been deleted. The builder was allowed only the notes, the public specifications (MS-CFB, MS-ODRAW) and the sample files. The other modules were written and extended from the notes and the public specifications.
- **ECMA-376.** The outline of the heart preset follows the definition in ECMA-376, 5th edition, Part 1 (Ecma International, 2016), `presetShapeDefinitions.xml`. The formulas are re-expressed in this project's own code and notes; no Ecma file or text is committed. The Ecma copyright permission has not been separately reviewed here; a maintainer should confirm that this attribution is sufficient.
- **Samples.** Apache POI's `Sample2.pub` (SHA-256 `c87ea7cc5606023c5fa99376631bbd45a8e9bdaba34c958057995b0e7ac8cc86`) and 142 generated fixtures from the pubsmith project were used as input data only. None is committed, and no rights in them are claimed.
- `truth/*.json` exports that come with the controlled samples were used only to score imports, never to work out how anything is encoded.
- No Publisher-authored file is committed. The committed test files under `crates/pub/tests/data/` are synthetic: the importer's tests write every byte of them from scratch (`crates/pub/tests/support/mod.rs`), and a test checks that each committed file is exactly that output. They are listed in `ASSETS.md`.

## How it is checked

- Unit and synthetic-file tests in `crates/pub` and the engine run with `cargo xtask ci`.
- Tests marked `#[ignore]` need the external samples: `PUB_CORPUS=dir` (the controlled samples)
  and `PUB_REAL_CORPUS=dir`. They are never committed. Over the 142 controlled samples the importer
  opens 142/142; 111 of 111 scored shape and text positions are within 1 pt of the `truth` exports,
  52/52 texts and 111/111 styles match. Of the 24 real-world samples, 2 are refused (Publisher
  2000 and older) and 3 are reported as damaged; the rest open, 10 of them with warnings.
- Not checked: Publisher 97/98/2000 files (none available), Publisher 2002–2019 files beyond the
  few real-world samples, and how text sets with the Microsoft fonts installed.
