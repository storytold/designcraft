# CJK typesetting plan

Goal: the Chinese, Japanese and Korean typesetting that InDesign offers with its East Asian
feature set (the "Japanese/CJK" feature set, `Feature Set Locale Setting` 257), so that a
DesignCraft document can be set to the standard of a Japanese, Chinese or Korean publisher,
round-trip through IDML, and export correct PDF and EPUB.

It builds on the CJK import and composition described in [cjk-typography.md](cjk-typography.md)
(composite fonts, kinsoku sets, aki, tsume, jidori, hanging punctuation, em alignment and leading
models, kenten characters, tate-chu-yoko offsets, preserved mojikumi tables) and plans what
remains.

## Sources (clean-room)

Behaviour comes from public specifications, not from InDesign's implementation:

- W3C *Requirements for Japanese Text Layout* (JLREQ): character classes, line composition,
  spacing (aki), line breaking (kinsoku), ruby, kenten, warichu, tate-chu-yoko, grids.
- W3C *Requirements for Chinese Text Layout* (CLREQ) and *Korean* (KLREQ): punctuation placement
  for Simplified and Traditional Chinese, Korean word-based line breaking.
- JIS X 4051 (published standard; JLREQ follows it).
- OpenType: `vmtx`/`vhea`/`VORG`, `BASE`, the `vert`/`vrt2`/`vkrn`/`vpal`/`palt`/`halt`/`vhal`,
  `locl`, `ruby`, width (`fwid`/`hwid`/`pwid`/`qwid`/`twid`) and form (`jp78`/`jp83`/`jp90`/`jp04`,
  `trad`, `expt`, `nlck`, `hojo`) features.
- The published IDML specification for attribute names and values.

InDesign itself is only observed black-box (feature names, which options exist); no wording,
presets or tables are copied. Our default mojikumi and kinsoku tables are built from JLREQ.

## Where we are (2026-10)

| Area | State |
|---|---|
| Vertical text, upright CJK, rotated Latin | done: story direction per story (frames of a thread agree; IDML `StoryOrientation`); glyphs turned by transforms; arrow keys follow the vertical lines |
| Vertical metrics and shaping | done: upright runs shaped top to bottom (`vert`; `vkrn`/`vpal` when asked) with `vmtx`/`VORG` advances and origins, centred on the ideographic em box (`BASE`, else OS/2); turned runs shaped horizontally, without `vrt2`; Horizontal Kana (`hkna`) takes the vertical kana (`vkna`) in upright runs. Default location of variable fonts only (no `VVAR`) |
| Kinsoku | named (hard, soft, Chinese, Korean) and custom kinsoku sets, bunri-kinshi, rensuuji, hanging punctuation (regular, force); without a set, fixed no-start / no-end lists and a break anywhere between CJK characters; kinsoku type (push in / push out priorities) kept but not applied |
| Korean line breaking | at spaces (Hangul everywhere, hanja in Korean text); per-paragraph character-based breaking (`koreanCharBreaks`); DesignCraft-only, not in IDML |
| Mojikumi | tables and paragraph references kept through IDML; not applied (Preflight says so) |
| Aki, tsume, jidori | done ([cjk-typography.md](cjk-typography.md)) |
| Character alignment, leading model | em box top / centre / bottom, ICF from ascender and descender, roman baseline; aki above / below, centre (centre down as centre) |
| Tate-chu-yoko | manual, with offsets; no auto, 3+ digits overflow the em |
| Ruby | group ruby only, fixed 50 % size, no options, doesn't affect leading |
| Kenten | the IDML kinds and a custom character, drawn as characters; no position, size, alignment, colour or font |
| Composite fonts | done: model, `style.compositeFont.*`, shaping, IDML, font list, replace and Preflight; no dialog |
| Languages | Japanese, Korean, Simplified and Traditional Chinese reach the shaper (`locl`), font fallback, line breaking and typographer's quotes; IDML language names are kept as written; locale codes (`ja_JP`, `ko-KR`, `de_DE_2006`) are their language and region, and Chinese is also recognised in its common spellings ("Simplified Chinese", "Chinese (Traditional)", `zh_CN`, `zh-Hant`) |
| Missing glyphs | as in InDesign: characters the applied font lacks are drawn as its missing-glyph box (screen and PDF) and listed by Preflight; the document setting Draw Missing Glyphs from Fallback Fonts (Preferences › Composition, off in new documents and IDML imports, on in documents saved before it existed) draws them from fallback fonts instead |
| Font menus | Western families first, then Japanese, Simplified Chinese, Traditional Chinese and Korean groups (from the font's `meta` languages, CJK family names, OS/2 code pages, else its coverage); CJK families under their native names unless Preferences › Type › Show Font Names in English; the order is recalled from InDesign, not observed |
| CJK fonts | Japanese faces (Shippori Mincho first) from the optional craft-fonts build input, in the font menus for users to apply; fallback by language (with fallback fonts on) (Japanese: the craft-fonts faces, then system fonts; Simplified and Traditional Chinese, Korean: system fonts); document fonts: the fonts of a `Document Fonts` folder beside an opened file belong to that document (composition, export, font menus), ahead of installed fonts of the same name, and are forgotten when it closes; Package copies every font file that draws text, fallback fonts included (licence permitting). Font files stay in memory for the session (reopening a document reuses them) and aren't read on the web |
| Everything else below | missing |

Known bugs to fix first:

- A justified CJK line has no stretch except Single Word Justification.

## Architecture

### Character classes

A `cjk::class_of(char, &Context) -> Class` in `designcraft-compose` with the JLREQ classes
(opening brackets, closing brackets, hyphens, dividing punctuation, middle dots, full stops,
commas, inseparable characters, iteration marks, prolonged sound mark, small kana, prefixed and
postfixed abbreviations, full-width ideographic space, hiragana, katakana, math symbols and
operators, ideographs, Western characters, numerals, ruby bases, warichu brackets, …). Language
and the paragraph's punctuation style decide the ambiguous ones (e.g. Chinese full-width
punctuation centred vs corner-placed).

### Spacing: mojikumi

A *mojikumi set* is a table indexed by (class before, class after) giving the space between them
as a min/optimum/max triple in em fractions, plus line-start and line-end rules. Sets are
document resources (`Styles::mojikumi_tables`, read from and written to IDML with their override
rows), referenced by name from the paragraph's `mojikumi`; what remains is applying them. We
ship a few built-in sets derived from JLREQ (no line-start indent, one-em indent, half-width
punctuation, full-width punctuation…), all original data, for the IDML built-in names.

Shaping keeps the glyphs' own advances; mojikumi is applied after shaping as *aki* glue between
glyphs: a full-width bracket's empty half becomes negative aki (compression) and the composer
can widen or narrow each gap within its range. This keeps `palt`-style proportional fonts
working (tsume removes the side bearings before mojikumi runs).

### Line breaking: kinsoku and the Japanese composer

- Kinsoku sets (`doc::cjk::Kinsoku`: characters not allowed at line start, at line end, that may
  hang, and inseparable pairs), the paragraph's set, hanging (none / regular / force),
  bunri-kinshi and rensuuji are in place: they constrain breaks in both breakers. Remaining: the
  built-in hard and soft sets from JLREQ's classes (today's named sets are common punctuation
  lists), and the kinsoku type (push in first / push out first / push out only / prioritise the
  adjustment amount), which needs the Japanese composer's push-in and push-out candidates.
- A `Composer::Japanese` (paragraph) and `Composer::JapaneseSingleLine` beside the Western
  composers. In `breaker::items` the Japanese composer emits:
  - boxes per glyph,
  - mojikumi glue between glyphs with priority tiers (compress punctuation first, then
    expand inter-ideograph space, then Western/CJK spacing, then letter-spacing),
  - kinsoku penalties (infinite inside prohibited pairs; push-in candidates scored by
    compression needed),
  - hanging punctuation as zero-width boxes allowed past the measure.
  Knuth–Plass already takes tiered stretch (`TIER_COST`), so this plugs into `kp_pass`.
- Justification in `layout_line` distributes by the same tiers; the last line follows the
  paragraph's last-line alignment; CJK lines justify by default (JLREQ).
- Korean: the Japanese composer with word-based breaking (spaces) by default; per-paragraph
  option for character-based breaking.

### Vertical text, properly

- Read `vmtx`, `vhea`, `VORG` and the ideographic em box (`BASE` `ideo`/`idtp`, else OS/2 typo
  metrics). `FontFace` gets `v_advance(gid)`, `v_origin(gid)` and `em_box()`.
- Shape upright runs with direction `TopToBottom` so `vert`, `vkrn` and `vpal` apply and the
  shaper reports vertical advances; rotated (Latin) runs stay horizontal and turn as a whole.
  `vrt2` only for runs that are rotated, never with an extra turn.
- Glyph placement in vertical lines uses the vertical origin, not a guessed centre.
- Story direction per story (IDML `StoryOrientation`); frames of one story agree.
- Arrow keys and carets follow the vertical line direction.

### Annotations and inline structures

All set after line layout like today's ruby, but able to affect line height:

- **Ruby**: per-character (mono), group and jukugo ruby; alignment (1-2-1, centre, left/right,
  equal space, JIS rule), position (above/right, below/left), size, font, colour, x/y scale,
  offsets, overhang onto neighbours (none / ½ / 1 ruby character / ½ base / unlimited), automatic
  scaling, spacing to the base, "ruby adjusts leading".
- **Kenten**: position, size, alignment, colour, font (the kinds and a custom character are
  there).
- **Warichu**: an inline run set in 2+ smaller lines inside one line height, with size, line
  spacing, alignment and breaking rules. Type › Warichu (`type.warichu`) does this after the
  line is broken: the run is scaled and stacked, and the rest of that line closes up. It does
  not pull more characters onto the line. The first small line keeps at least
  `WarichuCharsBeforeBreak` glyphs and the last at least `WarichuCharsAfterBreak`; the glyphs
  are split so those lines are as close in width as the minimums allow. Line spacing of 0 keeps
  the baselines one small em apart, a positive value adds to that em, and a negative value
  tightens it down to a shared baseline. The caret, the selection, and hit testing follow the
  small line the character sits on, and Up and Down move between those lines before leaving the
  parent line. Underlines follow the small baseline. Right-to-left and tate-chu-yoko runs are
  left as body text. IDML reads and writes `Warichu`, `WarichuLines`, `WarichuSize`,
  `WarichuLineSpacing`, `WarichuAlignment`, `WarichuCharsBeforeBreak` and
  `WarichuCharsAfterBreak`.
- **Tate-chu-yoko**: scale-to-fit-em, automatic TCY for runs of N digits (with or without
  Latin), rensuji (offsets are there).
- **Shatai** (oblique in the CJK sense: angle and magnification, keeping the em box),
  **character rotation**.
- **Character alignment and leading model**: ICF top and bottom from the font's ideographic
  character face (`BASE` `icft`/`icfb`) rather than its ascender and descender, and centre down
  as its own model.

Jidori, tsume, aki before and after, em-box alignment and the other leading models are in place
([cjk-typography.md](cjk-typography.md)).

### Grids

- **Layout grid** (per page / parent): character size, characters per line, lines per column,
  columns and gutter, origin; drawn like the baseline grid; snap target.
- **Frame grids**: text frames that show a character grid (horizontal or vertical), with a
  character count, named grids as document resources (font, size, scale, aki between characters
  and lines, alignment, view options), grid alignment of paragraphs (em-box centre, ICF top/bottom,
  roman baseline, gyoudori: lines spanning N grid lines), grid tracking.
- Frame Grid tools (horizontal and vertical) and Object › Frame Type (text frame ↔ frame grid).

### Fonts

- **Composite fonts** (a named font made of a base font plus fonts for character sets, with size,
  baseline, x/y scale and centre adjustments) are resolved before shaping, read from and written
  to IDML and listed, replaced and preflighted by their member fonts; PDF embeds the member fonts.
  Remaining: the Composite Fonts dialog, and the predefined character classes (kanji, kana,
  full-width symbols, punctuation, Latin, numerals) beside custom sets.
- **Missing glyphs**: InDesign doesn't substitute fonts per character: a character the applied
  font lacks shows as the font's missing-glyph box, prints that way and is a Preflight error.
  DesignCraft does the same, with a document setting (Draw Missing Glyphs from Fallback Fonts)
  that draws such characters from fallback fonts instead.
- **Language-aware fallback** (when that setting is on): Japanese, Simplified Chinese, Traditional
  Chinese and Korean each have their own fallback chain of system fonts (Japanese: the craft-fonts
  Japanese faces first); `locl` follows the character language (IDML language names).
- **No CJK fonts in the repo.** CJK fonts are large (15–25 MB each even subset) and documents
  should name the fonts they use. The Japanese UI and document faces come from the optional
  craft-fonts build input when DesignCraft is built with it (release builds are).
- **Document fonts**: fonts in a `Document Fonts` folder beside an opened `.designcraft` or
  `.idml` file are available to that document only, ahead of system fonts, and unloaded when it
  closes. File › Package copies the document's fonts into the package's `Document Fonts` folder
  (font licences permitting: OS/2 `fsType` restricted-licence fonts are listed, not copied), so a
  packaged CJK document opens with its fonts on another machine. On the web, the fonts of a
  dropped or opened package folder load the same way.

### Editing and output

- Find/Change: kana-sensitive and width-sensitive matching; full/half-width and
  hiragana/katakana conversion (Type › Change Case).
- Numbering styles: kanji, full-width, circled, parenthesised.
- Typographer's quotes per language (Simplified/Traditional Chinese, Korean).
- Character count (Info panel) by CJK conventions.
- EPUB/HTML: `writing-mode: vertical-rl`, `<ruby>`, `text-combine-upright`, `text-emphasis`.
- PDF: correct `ActualText` for ruby/warichu; vertical glyph placement as above.
- IDML: every attribute below, import and export, round-trip tested.

## Feature set (UI)

Preferences › Type gets a *Feature Set* choice (Western, CJK). CJK shows the East Asian
controls in the Character and Paragraph panels, styles dialogs and menus (Kinsoku, Mojikumi,
Composite Fonts, Named Grids, Layout Grid), makes the Japanese composer the default for new
documents and offers the frame grid tools. Western hides them; documents keep their settings
either way (opening a CJK document in Western mode loses nothing). Every feature stays reachable
as a command regardless of the feature set (agents, MCP).

## IDML attributes to support

Already read and written: `KentenKind`, `KentenCustomCharacter`, `RubyFlag`, `RubyString`,
`Tatechuyoko`, `TatechuyokoXOffset`, `TatechuyokoYOffset`, `Jidori`, `Tsume`, `LeadingAki`,
`TrailingAki`, `CharacterAlignment`, `LeadingModel`, `GlyphForm`, `OTFProportionalMetrics`,
`OTFHVKana`, `OTFRomanItalics`; paragraph `KinsokuSet`, `KinsokuType`, `KinsokuHangType`,
`BunriKinshi`, `Rensuuji`, `MojikumiTable`, `TreatIdeographicSpaceAsSpace`; resources
`KinsokuTable`, `MojikumiTable`, `CompositeFont`, `StoryPreference` direction.

Character: `KentenFont`, `KentenFontStyle`, `KentenSize`, `KentenPosition`, `KentenAlignment`,
`KentenCharacterSet`, `KentenXScale`, `KentenYScale`, `KentenFillColor`/`Tint`, `RubyType`,
`RubyAlignment`, `RubyPosition`, `RubyFont`, `RubyFontStyle`, `RubyFontSize`, `RubyXScale`,
`RubyYScale`, `RubyXOffset`, `RubyYOffset`, `RubyOverhang`, `RubyParentSpacing`,
`RubyParentOverhangAmount`, `RubyAutoAlign`, `RubyAutoScaling`, `RubyAutoTcyDigits`, `Rensuji`,
`CharacterRotation`, `ShataiMagnification`, `ShataiDegreeAngle`, `ShataiAdjustRotation`,
`ShataiAdjustTsume`, `Warichu`, `WarichuLines`, `WarichuSize`, `WarichuLineSpacing`,
`WarichuAlignment`, `WarichuCharsBeforeBreak`, `WarichuCharsAfterBreak`.

Paragraph: `Composer` (the Japanese composers), `GridAlignment` (all values),
`GridAlignFirstLineOnly`, `GridGyoudori`, `ParagraphGyoudori`, `CjkGridTracking`, `AutoTcy`,
`AutoTcyIncludeRoman`, `RotateSingleByteCharacters`, `ScaleAffectsLineHeight`.

Resources: `MojikumiTableSet`, `NamedGrid`, layout grid and frame grid preferences,
`StoryPreference` orientation.

Exact names and enumerations are checked against the IDML specification as each phase lands.

## Phases

Each phase lands as reviewable commits with tests (compose tests for layout, engine tests for
commands, IDML round trips, a rendered sample page) and `cargo xtask ci`.

| # | Phase | Contents | Estimate (agent hours) |
|---|---|---|---|
| 1 | Foundations | language passed to the shaper (`locl`); Simplified/Traditional Chinese and Korean languages; language-aware CJK fallback; document fonts folder; Korean word breaking; `vmtx`/`VORG`/em box and vertical shaping; story-level direction; vertical caret keys | 12–17 |
| 2 | Japanese composer | character classes; mojikumi applied (built-in sets and the document's tables) as aki glue; JLREQ kinsoku sets and kinsoku types (push in / push out); Japanese paragraph and single-line composers; CJK justification and last line; Chinese punctuation styles; IDML | 20–30 |
| 3 | Inline CJK | full ruby, kenten options, warichu, auto TCY and scale-to-fit, rensuji, shatai, character rotation, ICF alignment from the font; IDML | 20–30 |
| 4 | Grids | layout grid, frame grids and tools, named grids, grid alignment and gyoudori, grid tracking, character count; IDML | 20–30 |
| 5 | Fonts | Composite Fonts dialog and predefined character classes; CJK OpenType features in the OpenType menu | 5–8 |
| 6 | Editing and output | kana/width-sensitive Find/Change, width and kana conversion, CJK numbering, quotes, EPUB/HTML vertical text and ruby | 10–15 |
| 7 | Feature set UI | Preferences › Feature Set, panels and dialogs for every attribute, Japanese defaults for new documents, translations | 15–20 |

Total: roughly 100–150 agent hours.

### Phase 1 in detail

1. `CharProps.language` reaches `designcraft_fonts::shape` (harfrust buffer language) and the
   word cache key; tests: a font with `locl` for JAN vs ZHS draws different glyphs for U+76F4.
2. Language list: "Chinese: Simplified", "Chinese: Traditional", "Korean", "Japanese" (IDML
   names), with quotes and spelling/hyphenation off for them.
3. Fallback chains by language in `FontDb::fallback_for` (system fonts per platform; the
   craft-fonts Japanese faces first for Japanese); tests with stand-in fonts and a fake catalog.
4. Document fonts: opening a document loads the fonts in its `Document Fonts` folder for that
   document; Package copies used fonts there (licence permitting) and reports the ones it
   couldn't copy; tests with a synthetic package folder.
5. Korean: `cjk_break_between` doesn't break between Hangul syllables unless the paragraph asks
   for character breaking; test with a Korean sentence.
6. `FontFace::v_advance/v_origin/em_box` from `vmtx`/`vhea`/`VORG`/`BASE`; upright glyphs in
   vertical lines use them; tests against synthetic fonts with and without `vmtx` (without, the
   em box), and Shippori Mincho's tables when built with craft-fonts.
7. Upright runs shaped top-to-bottom (`vert`, `vkrn`, `vpal` apply); `vrt2` never on turned
   runs; test that a full-width comma takes its vertical form and position.
8. Story direction at story level (frames of a story agree; IDML maps both ways); arrow keys move
   along the vertical line.

## Decisions

Each follows what an InDesign user expects.

1. **Feature set gating.** East Asian controls are hidden in the Western feature set and shown in
   the CJK feature set, as in InDesign. Every feature stays reachable as a command either way.
2. **Korean line breaking** is word-based by default (KLREQ), with a per-paragraph option to break
   between syllables.
3. **PDF vertical text** keeps transformed horizontal glyphs (correct-looking, copyable via
   ActualText) rather than vertical CID fonts (`Identity-V`).
4. **No CJK fonts in the repo**; documents carry theirs in `Document Fonts`, scoped to the
   document as in InDesign. DesignCraft embeds Japanese UI and document faces only when built with
   the optional craft-fonts input (release builds are).
5. **Package copies every font that draws text**, fallback fonts included (when the document
   draws missing glyphs from them), so a package reproduces what is on screen.
6. **Korean fallback is serif first** (Myungjo), as Chinese and Japanese fall back to Song and
   Mincho: the serif faces CJK editions of layout software default to.
7. **Language names on IDML import** accept the common spellings of Simplified and Traditional
   Chinese, since the exact names InDesign writes are unconfirmed without a sample file.
8. **Missing glyphs** are drawn as the font's box and reported, as in InDesign; drawing them from
   fallback fonts is a per-document choice, on only for documents saved before it existed.
