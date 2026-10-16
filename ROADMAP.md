# DesignCraft roadmap

**Stage: pre-alpha** · next: alpha, ~5–7 h away; beta ~28 points and ~420–750 h beyond that (the core-workflow gate, not the percentage: ready for real work is already ~47%)
- FID1: PDF export normalizes mixed emitted gradient color spaces to RGB with an explicit warning; homogeneous device-space gradients retain their components. Linear/radial, midpoint, archival and print policies have synthetic regression coverage.

- AR1.1: Kashida justification tests verify space fallback without Tatweel fonts using an isolated font database, while retaining elongation assertions when fonts support it.

- AR1: Arabic IDML controls now retain character direction, kashida switches, diacritic offsets/forms, paragraph policy values and independent story/table directions. Shaping preserves joining context and script/language selection; paragraph-level bidi, contextual digit conversion, mark-safe elongation, RTL columns/tables and physical column insertion have regression coverage. Vendor-specific justification and diacritic presets remain explicit preflight warnings; see docs/arabic-typography.md.
- CJK1: IDML composite fonts now retain character mappings and metrics; explicit aki, tsume, jidori, em alignment/leading, named/custom kinsoku boundaries, hanging punctuation, tate-chu-yoko offsets and kenten symbols flow through composition and IDML export. Automatic kerning no longer imports inactive numeric values as enormous manual spacing. Composite definitions are editable through commands with undo/cache invalidation. Mojikumi definitions are retained and unsupported spacing-table / push-in-push-out policies are reported by preflight. Full InDesign CJK composition parity, frame-grid layout and detailed ruby remain unfinished. Warichu sets the selected run in smaller lines inside the line after the line is broken.

DesignCraft aims at full Adobe InDesign parity — and to be better: faster, open (documented JSON format + IDML), scriptable by agents (MCP), and available on the web.

## Status (2026-10-01)

**Working today**
- Document model: facing/non-facing spreads, parent pages with page-number markers, sections, layers, frames (text / graphic / unassigned), threaded stories, paragraph/character/object styles with based-on, swatches (process/spot/tints/gradients), corner options, text wrap, drop shadow, opacity/blend.
- Text engine: Knuth–Plass paragraph composer and single-line composer, dictionary hyphenation (public-domain Moby list + our own trained Liang patterns), justification with word/letter/glyph-scaling ranges, keep options (keep with next, keep lines together, widows/orphans), balance ragged lines, optical margin alignment, hyphenation zone/limit, columns, threading across frames and spreads, tabs, bullets/numbering, rules, shading, baseline grid, first-baseline options, vertical justification, wrap exclusions, overset detection, caret/hit testing.
- FID2: IDML underline/strikethrough Text Color resets survive style inheritance and roundtrip. Raster and PDF paint underlines behind glyphs, retain strikethrough above, and center rules on their offsets; synthetic overlap tests cover both backends.
- Rendering: vello_cpu (SIMD, multithreaded) — pages, items, images with mip levels, gradients, composed text.
- UI (egui): application bar + menus, Control panel (object and text modes), Tools panel with flyouts, document tabs, rulers, pasteboard, guides (margins, columns, bleed, baseline grid), frame edges, selection handles, text ports and threads, Properties / Pages / Layers dock, Swatches, Styles, Character, Paragraph, Stroke, Text Wrap panels, New Document and Preferences dialogs, Quick Apply (⌘Return), 4 brightness themes.
- Pen tool, Direct Selection anchor/handle editing, rotate from corners, Rotate/Scale/Shear tools, Eyedropper, snapping & smart guides, Align/Distribute, Find/Change (text + GREP), Story Editor, Paragraph Style Options, Effects panel, hidden characters, native macOS menu bar, measured InDesign 2026 look (Medium Dark): Properties sections per selection state, 21 pt spinner fields with arithmetic, Contextual Task Bar, inverse (black) text selection, Pages panel with drag-and-drop reorder / parent apply, Layers panel with per-object rows.
- Tools: Selection (click, marquee, move, Alt-duplicate, resize handles), Direct Selection, Type (draw frame, click caret, select, type), Rectangle/Ellipse/Polygon (+ frame variants), Line, Hand, Zoom.
- PDF export (krilla): real selectable text with embedded font subsets, DeviceCMYK/RGB + spot Separations, bleed boxes, crop/bleed marks + page info, pages or spreads, PDF/A-2b (PDF/X-4 output intent pending) — `file.exportPdf`, File › Export PDF…, `designcraft-cli run --export out.pdf`. File › Export PDF… now opens an "Export PDF" options dialog (General / Compression / Marks and Bleeds / Advanced) and remembers the last-used settings across exports and sessions.
- IDML interchange (`designcraft-idml`): export and import of swatches, styles, fonts, preferences, parent spreads, spreads/pages, frames, groups, images (embedded or linked), formatted threaded stories — opens in InDesign 2026 and round-trips InDesign-exported files (`file.exportIdml`, `file.openIdml`, File → Export IDML…, CLI `--in x.idml` / `--export x.idml`).
- Tables (M8): tables anchored in stories (header/footer/body rows, merged cells, per-edge strokes, fills + alternating fills, insets, vertical justification, at-least/exact row heights); composed into the text column (columns scale to fit, rows break across columns/frames with repeating headers/footers, overset); rendered, exported to PDF (real text) and IDML (export + import); Table menu, Table panel, Create Table dialog, caret/typing/Tab navigation and cell selection in cells; `table.*` commands.
- Hyperlinks (text or frames → URL / e-mail / page) and bookmarks, exported as PDF link annotations and outline.
- EPUB 3 (reflowable) export: stories in reading order, CSS from paragraph/character styles, images, navigation (`file.exportEpub`, CLI `--export x.epub`).
- Data Merge (one linked CSV, TSV, text, or Excel file; text, image, QR, and hyperlink placeholders; preview; a new document with one record per page or records tiled on a single page), spell checking (public-domain Moby list + document dictionary, suggestions), Step and Repeat (count or grid), snippets, object styles, Numbering & Section Options.
- Footnotes: Type › Insert Footnote (caret moves into the note), Document Footnote Options (numbering style incl. symbols, start/restart per page/spread/section, prefix/suffix, reference position/character style, paragraph style, separator, spacing, first baseline, rule above); notes composed at the bottom of the referencing column with the body text making room; edited in place on the canvas; rendered, exported to PDF (real text) and IDML (InDesign's structure, verified opening in InDesign 2026) and imported from IDML; `footnote.*` commands.
- Paragraph Rules: Rule Above / Rule Below (on, weight, colour, tint, column or text width, offset, left/right indent) set in Type › Paragraph Rules…, the Paragraph panel menu and Paragraph Style Options › Paragraph Rules; inherited style values shown, and `type.para` / `style.paragraph.edit` change only the rule fields named.
- Cross-references and text anchors: Insert Cross-Reference (paragraph by style, or text anchor), 11 formats with InDesign's building blocks (full/partial paragraph, paragraph text/number, page number, anchor name, chapter, file name) plus user formats; resolved live at composition — never out of date, no Update step; unresolved destinations show `??`; PDF link annotations to the destination page; IDML export/import as InDesign cross-reference sources, hyperlinks, text destinations and formats (verified in InDesign 2026); `xref.*` / `anchor.*` commands.
- Index: page references (topic from the selection or up to 4 levels, sort keys; current page, to end of story, next n paragraphs, suppressed, See / See also), Generate / Update Index (section headings, nested or run-in, page ranges merged, Index Title / Section Head / Level 1–4 styles); IDML export/import as InDesign topics, page references and topic cross-references (verified in InDesign 2026); `index.*` commands.
- Text clipboard keeps formatting (character/paragraph formats, footnotes, cross-references, index markers, tables) and falls back to plain text when the system clipboard changed; Paste without Formatting (⇧⌘V); Change Case (UPPERCASE, lowercase, Title Case, Sentence case — formatting kept); Type › Insert Special Character / White Space / Break Character submenus.
- Anchored objects: items flow in the text inline (on the baseline, with Y offset; lines grow to fit) or above the line (left/center/right, space before/after); paste copied items into text to anchor them; Anchored Object Options and Release (back onto the page where shown); rendered, exported to PDF and to/from IDML (InDesign anchored object settings; verified in InDesign 2026); copy/paste and threading carry them; `anchored.*` commands.
- Ruler guides: drag out of the rulers (page guides, or spread guides on the pasteboard), drag to move, drop on a ruler to delete; Layout › Create Guides (rows/columns with gutters, fit to margins or page); Delete All Guides on Spread; `guide.*` commands.
- Links panel: every placed graphic with status (OK / Modified / Missing / Embedded, checked against the file on disk), page and effective PPI (low resolution flagged); Relink…, Go To, Update, Embed; `links.*` commands.
- Missing fonts: highlighted pink on screen (never in output), listed by Preflight; Type › Find/Replace Font (fonts in the document, missing first; replace in text, cells, footnotes and styles); `font.list` / `font.replace`.
- Missing glyphs, as in InDesign: a character the applied font lacks is drawn as that font's missing-glyph box (its .notdef; a drawn box when the font's is empty), on screen and in PDF (as a path, so PDF/A and PDF/UA exports take it), and Preflight lists the characters by font. Preferences › Composition › Draw Missing Glyphs from Fallback Fonts (`document.preferences {glyphFallback}`, stored with the document) draws them from other fonts instead; it is off in new documents and IDML imports, on in documents saved before it existed. A missing font's substitute always uses fallback fonts.
- Damage-region repaint: after an edit the canvas re-renders only the frames whose composed text (or the items that) changed and patches its texture — pixel-identical to a full render; typing in a frame repaints in under a millisecond instead of a full-viewport render (22 ms measured on the sample).
- Crash recovery: unsaved documents are written to the recovery folder every 30 s and reopened (unsaved, remembering their files) after a crash; File › Revert and Save a Copy…; `file.recovery.*`, `file.revert`, `file.saveACopy`.
- Graphics backend chosen before the window exists (DirectX 12 on Windows, Vulkan on Linux, Metal on macOS, OpenGL as the fallback; `WGPU_BACKEND` overrides) with a start-up fallback: a backend whose start never showed a frame is skipped at the next start and the status bar says so (`gpu.json`). Found on an AMD hybrid-graphics laptop whose Vulkan driver faulted at the first present.
- Edit › Paste Into (copied objects become a frame's content, clipped by it; rendered, PDF and IDML both ways — verified in InDesign 2026); Polygon Settings (sides, star inset; double-click the Polygon tool).
- Color panel (fill/stroke proxy, CMYK/RGB/Lab sliders with channel ramps, tint slider for swatches, spectrum ramp, Add to Swatches) and Color Picker (double-click the proxy); mixed colours are unnamed (not listed in Swatches until added); `object.color`, `swatch.addToSwatches`, `swatch.addUnnamed`.
- Place text files: Word (.docx: styles by name with their attributes, bold/italic/underline/size/font, footnotes, tables, tabs, breaks), RTF and plain text, into the insertion point, the selected frame or a new frame; autoflow adds pages and threaded frames until the text fits; Remove Styles option (`designcraft-textimport`).
- Gradient Feather (⇧G tool, `object.gradientFeather`): objects fade along a linear or radial opacity gradient, on screen and as a soft mask in PDF.
- Script Label (`object.label`, `object.findByLabel`), alt text (`object.altText`), Isolate Blending / Knockout Group (`object.transparencyGroup`; on screen, PDF isolates), Table ▸ Sort (`table.sortRows`, numeric-aware, header/footer fixed).
- File ▸ Export Text (`file.exportText`): a story as Text Only or RTF (fonts, styles, alignment, indents, tables as tab-separated rows); Export EPUB now asks for a path in the UI.
- Tagged PDF (`file.exportPdf {tagged}`, on by default from the UI): structure tree with stories as paragraphs in reading order, figures carrying their alt text, parent-page items and printer's marks as artifacts.
- Global Light (`object.globalLight`, `globalLight` on drop/inner shadows; Use Global Light in the Effects panel) and Generate Static Caption (`object.caption`: template over name, path, alt text, label, effective ppi, dimensions, format; any side, offset, style).
- Control panel scale X/Y, rotation and shear fields (`transform.info`, absolute `transform.set {scaleX, scaleY, rotation, shear}` about the reference point) and Preferences › Transformations are Totals for nested objects and placed graphics.
- Variable fonts: each named instance is a style (Font menu, shaping with feature variations, metrics, outlines) and embeds at its axis settings in PDF; free axis sliders still to come.
- Object ▸ Select (first/next above, next/last below, container, content, previous/next in group), full Object ▸ Convert Shape (`object.convertShape`), Table ▸ Split Cell Horizontally/Vertically.
- Attributes panel (`object.attributes`: Overprint Fill/Stroke/Gap, Nonprinting), with Overprint Gap in Overprint Preview and IDML.
- Page Numbering View (Preferences: section or absolute): page labels everywhere follow it; Go to Page takes section names, numbers or `+n`.
- Preferences › Advanced Type (document `advancedType`: superscript/subscript size and position; IDML TextPreference); subscripts now drop 33.3% like InDesign.
- Preferences › Composition › Highlight: keep violations, H&J violations (three shades), custom tracking/kerning, substituted fonts.
- File ▸ Export HTML (`file.exportHtml`): one self-contained page in reading order, styles as CSS, images inline, alt text from Object Export Options (also in EPUB).
- File ▸ Export PowerPoint (`file.exportPptx`, CLI `--export x.pptx`): every page becomes a slide of editable objects: shapes (preset or custom geometry, solid and gradient fills, dashed/compound strokes with arrowheads, shadows, glow, feather, opacity), pictures cropped like their frames (SVG kept as SVG), native tables (merged cells, fills, per-edge borders) and text boxes. By default the text keeps DesignCraft's line breaks, justified spacing and leading; threaded stories split at their frames and lines pushed aside by a wrap get boxes of their own. `lineBreaks: reflow` writes one box per frame in native columns instead. TrueType fonts are embedded where their licences allow, and objects without a PowerPoint equivalent are rasterised. Checked against PowerPoint for Mac and LibreOffice.
- Swatches panel colour groups (`swatch.newColorGroup`, `moveToGroup`, `ungroupColorGroup`, `renameColorGroup`; folders and a row context menu; IDML ColorGroup).
- Object ▸ Generate QR Code (`object.qrCode`: web link, text, SMS, email, business card; vector modules as a compound path, into a frame or re-encoded in place).
- Conditional text (`condition.new/options/delete/apply/list`, Conditional Text panel): hidden conditions drop their text from layout and exports, indicators underline on screen, IDML Condition/AppliedConditions.
- Endnotes (`endnote.insert/edit/delete/list/options`, Type ▸ Insert Endnote): references number through the document, the endnote frame on a new last page lists them under a heading; IDML keeps the listed text only.
- Editorial notes (`note.new/edit/delete/convertToText/list`, Notes panel): anchored in text, flagged on screen, never printed or exported.
- File ▸ Print Booklet (`file.printBooklet`): saddle-stitch or 2-up consecutive printer spreads as PDF, padded with blanks, with a gap between pages.
- Edit ▸ Transparency Blend Space (`edit.transparencyBlendSpace`; RGB for web/mobile documents; IDML TransparencyPreference) — exported PDFs give pages with transparency a DeviceCMYK or DeviceRGB page group, so viewers and RIPs blend in that space. On screen, spreads with transparency in a CMYK blend space are shown through the working CMYK profile (their colours keep to its gamut, as when composited in CMYK).
- High Contrast interface theme (fifth Interface Color Theme: black chrome, white text, yellow selection).
- Place Excel workbooks (.xlsx): the first worksheet's used range becomes a table (shared/inline strings, numbers, booleans).
- Object Library (`library.new/open/add/place/remove/list/json/close`, Library panel): `.dclib` files of snippets that keep stories, styles, swatches and images.
- Ink Manager (`ink.list`, `ink.options`; Swatches panel menu): All Spots to Process, per-ink conversion and ink aliases in PDF output; IDML ConvertToProcess/AliasInkName.
- Appearance of Black complete: Printing/Exporting rich black for RGB output (PNG), Overprint [Black] Swatch at 100% (document, IDML OverprintBlack).
- Hyperlinks and Bookmarks panels (Window ▸ Interactive): new from the selection, edit (`hyperlink.edit`), go to source (`hyperlink.goToSource`), rename bookmarks (`bookmark.rename`), go to page, delete.
- Nested line styles (`nestedLineStyles` paragraph attribute, Paragraph Style Options › Drop Caps and Nested Styles, IDML AllNestedLineStyles): the first lines restyle until the line breaks settle.
- User dictionary hyphenation exceptions (`hyphenation.addException/removeException/list`, Edit ▸ Spelling ▸ User Dictionary): `ex~am~ple` breaks only at `~`, a plain word never hyphenates.
- Frame Fitting Options (`object.fittingOptions`, Object ▸ Fitting ▸ Frame Fitting Options…): Auto-Fit refits on resize, fitting, Align From reference point, crop amounts; IDML FrameFittingOption.
- Paragraph borders (per-side weights, colour, tint, offsets) and shading offsets, drawn per column part of split paragraphs (also when overset); Paragraph panel controls; IDML ParagraphBorder*/ParagraphShading*Offset.
- Font menu: search, favourites (stars, Show Favorites Only; `favoriteFonts` preference) and each family previewed in its own face. Western (and other-script) families come first, then Japanese, Simplified Chinese, Traditional Chinese and Korean ones, each group alphabetical and set off by a separator; the group comes from the font's own data (`meta` languages, family names in a CJK language, OS/2 code pages, else what it covers), read once (installed fonts by the font scan, without loading them). CJK families show their native names, the English name on hover; Preferences › Type › Show Font Names in English (`showFontNamesInEnglish`) shows English names throughout. Search matches either name; documents store the English name. Every family list (Character panel, Control bar, Properties, style dialogs, Find Font, Glyphs) follows this order.
- Application preferences (engine `Prefs`) now persist across launches (desktop: prefs.json beside ui.json).
- View ▸ Fit Selection in Window (⌥⌘=) and Find/Change ▸ Object (`find.objects`, `find.changeObjects`: by fill, stroke, weight, opacity, kind, layer, label).
- Custom workspaces (`window.newWorkspace/deleteWorkspace/resetWorkspace`; workspace switcher shows the current one): bars and panel arrangement saved by name.
- Word/RTF Import Options (File ▸ Place with Import Options…, `place.styles`, `file.place {styleMap, styleConflicts}`): map imported styles onto the document's, use existing / redefine / auto-rename on conflicts, or remove formatting.
- Custom anchored objects (`anchored.insert/options {position: custom}`): placed relative to the anchor, frame, column, page margins or page edge with reference points and offsets, no space in the text, keep within column; IDML Anchored position.
- Variable font axes: Character panel sliders; any axis values (`fontStyle: "Bold {wght:650}"`) make that instance for layout, screen and PDF.
- Image Import Options › Crop to for placed PDFs (`file.place {pdfCrop: crop|trim|bleed|art|media}`, Place PDF dialog): the frame shows that page box.
- Stroke styles: striped (thick-thin… and custom bands), wavy and straight-hash strokes now draw on screen and in PDF; named custom stripe/dash/dot styles (`strokeStyle.new/delete/list`, Stroke panel Type list; IDML Striped/Dashed/DottedStrokeStyle).
- Object ▸ Primary Text Frame (`object.primaryTextFrame`): any text frame's story can become (or stop being) the primary story Smart Text Reflow follows.
- Column rules (Text Frame Options › Column Rules, `object.textFrameOptions {columnRule, columnRuleWeight, columnRuleColor, columnRuleTint, columnRuleOffset, columnRuleTopInset, columnRuleBottomInset}`, reported by `document.inspect`): a bar centred in each gutter of a multi-column frame, inside the insets and between the columns of right-to-left and vertical frames too, broken around paragraphs that span columns, with weight, colour, tint, horizontal offset and top/bottom insets; on screen, in PDF (tagged as an artifact) and in IDML (`ColumnRule…` attributes of `TextFramePreference`, also in object styles). The dialog shows the frame's options and changes only the fields edited. Rule stroke types and overprint aren't there yet.
- Place a page of an IDML or DesignCraft document (`file.place {layoutPage}`): its objects, stories, styles and swatches as one group (INDD files are not readable).
- Articles panel (`article.new/add/remove/options/delete/list`): reading order and content of EPUB and HTML exports.
- Object ▸ Clipping Path (`object.clippingPath {type: alpha|edges, threshold, tolerance}`): traces the placed image and makes the outline the frame.
- Gridify: arrow keys while dragging a frame or shape tool add columns/rows (`frame.grid`), one undo step.
- Gap tool (U, `gap.move`): drag the space between objects (or an object and the page edge) and both sides resize.
- XML basics (`xml.newTag/deleteTag/tag/mapStyle/structure`, `file.exportXml`, `file.importXml`; Tags panel with structure; View ▸ Show Tagged Frames): tagged frames export in reading order, mapped paragraph styles become elements, imported elements flow into frames with matching tags. Inline text tagging (`xml.tagText`, an `xmlTag` character attribute, exported as nested elements) and tag markers (View ▸ Structure ▸ Show Tag Markers) too. DTDs (`xml.loadDtd`, `xml.validate`, `xml.deleteDtd`; Tags panel): loading one adds its elements as tags and names the root; validation checks content models (sequences, choices, `?*+`, mixed content, EMPTY/ANY, parameter entities) and required attributes, and lists each problem with its element path.
- Adjust Layout / Layout Adjustment (`adjustLayout` on Document Setup and Margins and Columns): objects follow each page's margin box to its new size and position.
- Content Collector and Placer tools (B; `conveyor.collect/place/list/clear`): copies of objects (with stories, styles, images) ride a conveyor between pages and documents; Alt-click keeps them on it.
- Linked stories (Edit ▸ Place and Link, `story.placeAndLink/links/updateLink/unlink`): child stories copy a parent, show out of date in the Links panel and update.
- Edit ▸ Menus (`window.hideMenuItem`): hide menu items; menus with hidden items end in Show All Menu Items.
- View ▸ Separations Preview (`view.separations`): one process plate as ink density, or areas over an ink limit in red (a GCR preview of the rendered view; spot plates and the flattener preview aren't there).
- Separations Preview plates now come from each object's own colour (CMYK as authored; RGB and images through the colour settings, images via a cached 17³ table), so pure black shows only on the black plate.
- Language (Character panel ▸ Language, Properties ▸ Character, `type.char {language}`): text in other languages isn't hyphenated or spell-checked with the English dictionaries, typographer's quotes follow the language (German „…“, French and Russian «…», Swiss, Spanish, Dutch, Nordic, Japanese and Traditional Chinese 「…」, Simplified Chinese “…”), the font's localized forms (OpenType `locl`: Turkish i, Bulgarian Cyrillic, Japanese / Chinese / Korean glyph variants) follow it too, and so does the fallback for CJK characters the font lacks (with Draw Missing Glyphs from Fallback Fonts on): Japanese, Simplified Chinese, Traditional Chinese and Korean each try their own installed system fonts (Japanese starts with the craft-fonts Japanese faces, Mincho first, when built with them; Hangul, kana and Bopomofo pick their language's fonts whatever the language). Language names from IDML keep their spelling; locale codes as InDesign writes them for some languages (`de_DE_2006`, `nl_NL_2005`; any `ll`, `ll_CC`, `ll-CC`) are their language and region for all of the above, and Chinese is also recognised in its common spellings ("Simplified Chinese", "Chinese (Traditional)", `zh_CN`, `zh-Hant`). Spanish is hyphenated with Javier Bezos's Liang patterns (`hyph-es.tex` 5.0, MIT) through the `hypher` crate; hyphenation for other languages isn't bundled yet.
- Books (`book.new/open/add/remove/list/styleSource/paginate/syncStyles/exportPdf`, Book panel): `.dcbook` files, continuous page numbering, style sync from a style source, one merged PDF.
- Track Changes (`changes.track/list/acceptAll/rejectAll`, Type ▸ Track Changes): typing is marked added (highlighted on screen), deletions are kept but hidden from layout and exports until accepted or rejected. Per-change accept/reject and authors aren't there yet.
- Track Changes: per-change Accept / Reject (`changes.accept`, `changes.reject`) and a Track Changes panel listing every change.
- Move table rows and columns (`table.moveRow`, `table.moveColumn`, Table panel buttons; with whole rows or columns selected, drag them with the Type tool — `table.dropCells`).
- Distribute Rows Evenly (`table.distributeRows`, Table menu and panel): selected rows receive equal fixed heights preserving their composed total height, with undo/redo. Row-spanning merges and rows outside the composed frames are rejected; shrinking tall cells can overset their text.
- Object States (`states.create/show/rename/release/list`, Object States panel): multi-state objects show one state on screen and in PDF.
- Buttons (`button.set/clear/list`, Buttons and Forms panel): go to page / next / previous / first / last / URL as PDF link annotations. Form fields too (`form.set/clear/list`: text fields, check boxes, combo and list boxes, signature fields; required, multiline, defaults) exported as AcroForm widgets through an incremental update (not in PDF/X).
- Power Zoom (Hand tool: press and hold still for half a second, or Alt-press): zoom out to the spread, aim the red view rectangle, release to zoom back in there.
- Graphic cells (`table.placeGraphic`, `table.textCell`, Table › Convert Cell Type, Table panel): images in table cells, fitted or filled, clipped to the cell on screen and in PDF.
- Live Distribute (Space while dragging a selection handle; `transform.resize {distribute}`): objects keep their size and spread with the bounds.
- Liquid Layout (`liquid.pageRule/object`, `guide.liquid`, `layout.createAlternate`, Liquid Layout panel): scale / re-center / guide-based / object-based page rules applied on page resize and Document Setup; dashed liquid guides; alternate layouts as resized page copies in a named section. Text in an alternate layout stays linked to the original stories (Links panel shows them out of date; `story.updateLink`).
- Split Window / New Window (`window.split`, `window.newWindow`, Window ▸ Arrange): two side-by-side views of the document with their own zoom, scroll and render cache. New Window puts the second view in its own OS window (an egui viewport; a floating window on the web).
- Transparency flattener (`file.exportPdf {flatten: high|medium|low|ppi}`, Edit ▸ Transparency Flattener Presets): each object involving transparency becomes an opaque image of its area over what's beneath it, so everything else stays vector (spreads with transparent parent items are rasterised whole).
- Video and sound (place .mp4/.mov/.webm/.mp3/.wav…, `media.options/get`, Media panel): media frames show a poster or placeholder on screen, in print and in PDF, and export as HTML5 `<video>`/`<audio>` in EPUB. Interactive PDF export (`file.exportPdf {media: true}`, File ▸ Export PDF (Interactive)) embeds each file and plays it in a Screen annotation through a rendition action (controls, loop, play on page load). No in-app playback.
- Right-to-left text (World-Ready basics): right-to-left runs are shaped right to left, lines are reordered by the Unicode Bidirectional Algorithm (matched against the reference implementation), Paragraph Direction (`type.para {direction}`, Paragraph panel, IDML ParagraphDirection, EPUB `dir`). Digits (`digits`: Default, Arabic, Hindi, Farsi, Native by language; Character panel for right-to-left text; IDML DigitsType) draw 0–9 in another script. Kashidas (`kashidas` paragraph attribute, on by default): justified Arabic lines stretch at one join per word (after seen/sad when possible, never inside lam–alef) with a drawn tatweel before word spaces widen. Carets follow the drawing: the caret before a right-to-left glyph sits at its right edge, arrow keys move as drawn (by words in the line's direction), clicks find the nearest drawn caret position and selections highlight the selected glyphs. Binding: Right to Left (Document Setup, `layout.documentSetup {binding}`, IDML PageBinding) makes page 1 a left page and lays spreads out right to left.
- Interface language (Edit ▸ Interface Language, `app.language`): German, French, Spanish, Italian, Japanese, Simplified Chinese and Arabic menu titles, common menu items and panel names (our own translations). Common dialog, panel and tooltip labels are also localized, with English fallback for untranslated strings. Arabic translations cover every key in the shared interface translation table, including menu commands and utility labels. German and Japanese cover the whole interface: every key of every catalog. The Type menu's Story Direction, Horizontal/Vertical, Tate-Chu-Yoko, Ruby… and Kenten are translated too. Arabic labels use Unicode bidi run ordering and contextual shaping; welcome and new-document form layouts follow RTL direction independently of document binding. CJK and Arabic glyphs come from [craft-fonts](https://github.com/storytold/craft-fonts) when built with `CRAFT_FONTS_DIR` (all releases): BIZ UDPGothic and Noto Sans CJK SC for the UI (Chinese forms first in the Chinese interface), Noto Sans Arabic, and Mincho for Japanese documents (the web build embeds BIZ UDPGothic only); otherwise from system fonts. Ukrainian covers all 1,430 existing catalog keys, including publishing dialogs and built-in names, with persisted selection, grammatical count captions, English fallback and bundled Cyrillic UI glyph coverage.
- Page transitions (`page.transition/transitions`, Page Transitions panel): twelve transition types with speed and direction per spread, written into exported PDFs as `/Trans` entries through an incremental update.
- PDF/X-4 export (`file.exportPdf {standard: "x4"}`): an output intent with our own Generic CMYK ICC profile (lut16 tables generated from the parametric press model), GTS_PDFXVersion in Info and XMP, `pdf:Trapped` in XMP as in Info, RGB images without a profile tagged sRGB, no image interpolation (none in PDF/A either), and built-in PDF/X-4 checks reported as warnings. qpdf finds no errors in the result.
- EPS place: placed at its (high-resolution) bounding box and shown, printed and exported through its TIFF preview, or a placeholder when it has none. PostScript itself isn't interpreted.
- Preferences: Dictionary (user dictionary; `spelling.setWords/words`), Spelling (Dynamic Spelling: red squiggles on the canvas, also Edit ▸ Spelling), Autocorrect (word list, applied as you type), Notes, Track Changes, Story Editor Display and File Handling (recovery interval) sections.
- Colour management exposed: Edit ▸ Color Settings (`color.settings`: RGB/CMYK working spaces, intent, black-point compensation; `color.loadProfile` for ICC files; `color.convert` through the working spaces, with gamut checks) and View ▸ Proof Setup / Proof Colors (press, sRGB, legacy Mac RGB, colour-blindness simulations, simulate paper) on the canvas.
- Keyboard: Tab / Shift+Tab hide and show all panels / all but Tools (`window.hidePanels`, `window.hidePanelsExceptTools`); Cmd+F6 / Cmd+Shift+F6 cycle documents; panel fields are reached with Tab and Escape returns to the layout.
- Smart guides and snapping (View ▸ Snap to Guides / Snap to Document Grid / Smart Guides, `view.snapPreferences` and Preferences ▸ Guides & Pasteboard): moving, drawing, resizing, rotating and pen/anchor drags snap in order to the document and baseline grids, ruler guides, margins and columns, other objects' edges and centres (parent-page items too, by visible stroked bounds), equal spacing, matching dimensions and matching rotation, with guides, gap marks and measurements drawn on the canvas. Shift and Cmd constrain the gesture.
- View ▸ Rotate Spread (`view.rotateSpread`, `layout.rotateSpreadView`): the spread in view turns in quarter turns on screen (stored with the document; output is unaffected). The canvas maps each spread through its own transform, so tools, snapping, resize handles, guides, overlays and rendering all follow the turned spread.
- Vertical type (Vertical Type Tool, Type ▸ Story Direction `type.storyDirection`, IDML StoryOrientation): the direction belongs to the story, so every frame of a thread is vertical or horizontal together and a frame threaded onto a story takes its direction (`object.textFrameOptions {vertical}` sets the frames' stories); lines run top to bottom and follow each other right to left; the arrow keys follow them (Down and Up along the line, Left and Right to the next and previous line, keeping the position along it); CJK characters stay upright and other text turns, on screen and in PDF; CJK line breaking with basic kinsoku (also in horizontal text); Korean breaks at spaces, word by word (Hangul in any language, hanja in Korean text), or between syllables when the paragraph asks for it (`type.para {koreanCharBreaks}`, Paragraph Style Options ▸ Justification; not written to IDML). Upright text is shaped top to bottom: punctuation and brackets take their vertical forms (OpenType `vert`; `vkrn` and `vpal` through OpenType features), glyphs advance by their vertical metrics (`vmtx`) and hang from their vertical origins (`VORG`) on the centre of the ideographic em box (`BASE`, else OS/2), which the first line's ascent follows; turned text keeps its horizontal forms. Tate-chu-yoko (Type ▸ Tate-Chu-Yoko, `type.tateChuYoko`, IDML `Tatechuyoko`) sets a run such as "12" side by side within one em of the line. Ruby (Type ▸ Ruby…, `type.ruby`, IDML RubyString; group ruby spread 1-2-1 or centred) and kenten emphasis dots (`type.kenten`, IDML KentenKind) are set over their text — to its right in vertical frames — without changing line breaks. Warichu and ruby options (per-character ruby, positions, overhang) aren't there yet.
- Math expressions (Type ▸ Insert Math Expression…, `math.insert`, `math.svg`): a LaTeX subset (fractions, scripts, roots, big operators with limits, Greek, relations and operators with math spacing, `\left…\right`, `\text`) typeset by our own engine into SVG, placed inline at the cursor or as a frame; the LaTeX is kept as alt text and can be edited.
- Object Layer Options for placed PDFs (`object.pdfLayers`, `object.layerOptions`, Object ▸ Object Layer Options…): the file's optional-content layers listed and shown or hidden per frame on screen; in exported PDFs such graphics go out as 300 ppi images of their visible layers.
- Effects: Inner Glow (edge or centre), Bevel and Emboss (inner bevel lit from the light angle), Satin (`object.innerGlow`, `object.bevel`, `object.satin`); PDF export now keeps soft effects: objects with shadows, glows, feathers, bevels or satin go out as 300 ppi transparent images of their appearance (threaded text frames excepted).
- Directional Feather (`object.directionalFeather {widths: [top, left, bottom, right]}`): each side fades over its own width.
- View ▸ Flattener Preview (`view.flattenerPreview`): objects involving transparency highlighted in red; knockout groups export as images (PDF output has no knockout groups).
- Live captions (`object.caption {live: true}`, Object ▸ Captions ▸ Generate Live Caption): the caption text is re-filled from the source object's metadata after every edit (same undo step).
- Style Export Tagging (`style.exportTag`, Paragraph Style Options ▸ Export Tagging): paragraph and character styles choose their HTML element and class in EPUB/HTML.
- Tagged Text: export (`file.exportText {format: "tagged"}`) and place (detected by its header; ASCII, UTF-8 or UTF-16) with paragraph and character styles, font, typeface and size, and `<0x…>` characters.
- Tagged PDF: one structure element per paragraph (H1–H6 and BlockQuote from Export Tagging, else P) in reading order, frame fills, strokes, rules and tables as artifacts, figures with alt text.
- EPUB: cover image from the first page (`file.exportEpub {cover}`), and Fixed Layout export (`file.exportFixedEpub`, File ▸ Export EPUB (Fixed Layout)…): pre-paginated pages as 144 ppi images with their text kept for search and read-aloud.
- Interactive PDF options (`file.exportPdf {fullScreen, bookmarksPanel, pageLayout, view, advanceSeconds}`, File ▸ Export PDF (Interactive)…): page mode, layout, opening view and page timing in the catalog and pages.
- Object Export Options (`object.exportOptions`): tagged-PDF artifact, EPUB/HTML rasterize (the object as an image), alignment and page break before, alt text.
- Scripts panel (Window ▸ Utilities ▸ Scripts, `script.run`): saved command scripts (with `$N.path` references to earlier results) run as one undo step; edit, add, load and delete them in the panel. Scripts use DesignCraft's command language, not ExtendScript.
- Drag and drop text editing: drag selected text to move it (Alt copies) with its formatting (`text.release`).
- Image Import Options: place any page of a multi-page PDF (`file.place {pdfPage}`; the app asks which page), shown on screen and embedded as that page in PDF export.
- Define Lists: named numbered lists that continue across stories in page order (`list.define`, paragraph `listName` / `startAt`), round-tripped through IDML; `null` in `type.para` / `type.char` now removes an override.
- View › Overprint Preview (⌥⇧⌘Y): overprinting fills, strokes and 100% [Black] text mix with what's beneath on screen.
- Type on a Path (⇧T tool, `type.onPath`): a story along any line, curve or shape on screen and in PDF; Type on a Path Options (start, flip, baseline/ascender/descender/centre) and Delete Type from Path. Round-trips through IDML as TextPath; on-path caret drawing is still to come.
- Preferences › Appearance of Black: 100% K on screen accurately (dark grey) or as rich black (`window.richBlack`); the printing/export side is still to come.
- Spreads of up to 10 pages: Add to Previous Spread and Allow Spread to Shuffle (Pages panel; `layout.pages.toSpread`, `layout.spreadShuffle`); inserted pages go around kept spreads.
- Page tool (⇧P) and `layout.pageSize`: pages of their own size (or a preset); the spine stays put and objects keep their place on their page.
- File › Print (⌘P): printer, copies, page range, spreads, marks, bleed; printed as PDF through the system spooler (`file.print`, `file.printers`, `dryRun` for agents). Shortcuts now act like choosing the menu item (dialogs open), and UI commands win shortcut ties (⌘N opens New Document).
- Nested styles (through/up to N sentences, words, characters, letters, digits, tabs, breaks, spaces or given characters) and GREP styles (regex → character style), edited in Paragraph Style Options and composed under local formatting; `ui.dialog.open` for agents. Nested line styles are still to come; nested and GREP styles round-trip through IDML.
- Table and cell styles (round-trip through IDML): cell styles (fill, insets, vertical justification, strokes, paragraph style) and table styles (region cell styles, border, alternating rows, spacing); apply from the Table panel, edits re-apply to every user; `style.cell.*`, `style.table.*`.
- Style groups: paragraph and character styles shown in group folders, Move to Group (new, existing, none); `style.group` renames every use (styles, stories, table cells, footnotes, object styles).
- Scrubby zoom: drag the Zoom tool left or right to zoom continuously around the press point.
- Guides belong to the active layer: hidden with it (or with its Show Guides off) and locked with it.
- Pencil tool (N): freehand strokes simplified to smooth paths (Alt closes); Smooth and Erase tools re-fit or remove the stretch you drag along (`path.smooth`, `path.erase`).
- Edit › Keyboard Shortcuts: every command, click and press the new keys, conflicts shown, per-command and global reset; `window.setShortcut`.
- Primary Text Frame (New Document) and Smart Text Reflow: pages are added while the primary story oversets and empty ones at the end removed, in the same undo step (Preferences › Type).
- Glyphs panel: every character of any font (rendered by our renderer), search by character or U+code, click to insert, recently used.
- UI scaling (Preferences › Interface, 50–200%; `window.uiScale`); the window title steps aside when the bar is crowded.
- Underline / Strikethrough Options: weight, offset, colour and tint (Character panel; screen, PDF and IDML).
- Relink to Folder and Relink File Extension (`links.relinkFolder`; missing links found by name).
- File › Package (⌥⇧⌘P): the document relinked to a Links folder, the placed files, the font files that draw its text in a Document Fonts folder (the fonts it names and any fallback fonts that draw characters those lack; except fonts whose licence restricts it, OS/2 `fsType`), an IDML copy, an optional PDF and a report (fonts, fallback fonts, links, preflight, instructions); Copy Links To (`links.copyTo`).
- Document fonts: opening a `.designcraft` or `.idml` file loads the fonts in the `Document Fonts` folder beside it (as Package writes it), ahead of installed fonts with the same name, so a packaged document opens with its fonts on another machine. Damaged, oversized (over 256 MB per file, 512 MB in all) and surplus (over 200) font files are skipped and listed in `file.open`'s `warnings`; `font.list` gives each font's `source`. Document fonts belong to their document, as in InDesign: other open documents don't see them (each document with a same-named font draws with its own), and closing the document (or reverting it, or a book chapter's export ending) makes them unavailable. Their files stay in memory until DesignCraft quits, so reopening a document doesn't read or load them again; on the web there is no folder to read.
- Load Swatches / Save Swatches for Exchange (.ase: CMYK, RGB, Lab, Gray; spot or process) from the Swatches panel menu; `swatch.load` / `swatch.save`.
- Pathfinder (Object › Pathfinder and a Pathfinder panel): Add, Subtract, Intersect, Exclude Overlap, Minus Back; curves stay curves (flo_curves); `object.pathfinder`.
- Make / Release Compound Path (⌘8; nested paths become holes), Create Outlines (⇧⌘O; one path per text colour), Scissors tool (C) with `path.split`.
- Transform Again / Individually / Sequence Again (⌥⌘3; a moved copy repeats like Step and Repeat), Clear Transformations; Layers: Merge, Delete Unused, Hide/Lock Others, Show/Unlock All; Break Link to Style (paragraph and character).
- Floating panels: tear a panel off the dock (its float button, or drag its header away) into a movable, resizable window; Dock puts it back; positions persist; `window.floatPanel` / `window.dockPanel`.
- Place SVG (vector in PDF export via krilla-svg, resvg on screen, text in the bundled fonts), Photoshop (.psd/.psb composite in its colour mode — Bitmap, Grayscale, Duotone, Indexed, RGB, CMYK, Lab, 8/16-bit — with merged transparency; extra alpha and spot channels don't change the image), BMP, and Illustrator files saved with PDF compatibility (.ai). New `designcraft-images` crate (L2) for sniffing, sizes and decoding. EPS is still missing.
- View › Display Performance: Fast (grey boxes, no effects, ⌥⇧⌘Z), Typical (72 ppi proxies, ⌥⌘Z), High Quality (⌥⌘H, our default — the renderer is fast enough).
- OpenType menu (Character panel): discretionary ligatures, fractions, ordinal, swash, titling, contextual alternates, slashed zero, the four figure styles and stylistic sets 1–20; `type.openType`; mapped to IDML's OTF attributes.
- Add Anchor Point (=), Delete Anchor Point (-) and Convert Direction Point (⇧C) tools (click toggles smooth/corner, drag pulls out handles), with `path.addAnchor`, `path.deleteAnchor`, `path.convertAnchor`.
- Gradients: the Gradient Swatch tool (G) drags a gradient's start and end across objects (Shift: 45°); the Gradient panel sets type, angle, reverse and edits stops on a ramp (drag, add, drag off to remove, location, colour). Edited gradients are unnamed swatches; `object.gradient`; the vector round-trips through IDML.
- Document Setup (⌥⌘P): intent, number of pages, start page # (an even start begins with a left page), facing pages, size presets and orientation, bleed and slug per edge; `layout.documentSetup {}` reports the setup.
- Stroke panel: cap, join, miter limit, alignment, type, Start/End arrowheads (12 kinds, drawn on screen and in PDF; the path is shortened under pointed heads) and gap colour under dashes and dots.
- Preferences (⌘K: General, Type, Units & Increments, Grids, Guides & Pasteboard, Display Performance) backed by `prefs.set` (application) and `document.preferences` (units, increments, grids — undoable, saved with the document; right-click a ruler to pick its units, the horizontal and vertical rulers each have their own); scaling applies to content and scales stroke weights (Include Stroke Weight), and X/Y/W/H measure the stroke's outer edge (Dimensions Include Stroke Weight). Quick Apply (⌘Return) finds paragraph, character and object styles as well as commands.
- Parent item overrides: Cmd+Shift-click a parent item on a page (or Override All Parent Page Items, ⌥⇧⌘L) makes an editable local copy that hides the parent's; Remove All Local Overrides and Detach All Objects from Parent in the Pages panel menu.
- Place PDF: placed PDF pages are sized by their crop box, drawn on screen through hayro (with mip levels) and embedded in exported PDFs as vector form XObjects (krilla), `<PDF>` in IDML.
- Menus expose the engine's features (TOC, numbering & sections, text variables, hyperlinks, cross-references, footnotes, step and repeat, spelling, fitting/content, effects, Data Merge, Preflight, EPUB); any menu command with parameters gets a dialog generated from its parameter documentation.
- Table of contents (generate/update, dot leaders) and text variables (running headers, last page number, chapter number, file name, dates, custom) resolved per page.
- Soft effects: blurred drop shadow, inner shadow, outer glow, basic feather.
- ~200 commands, all reachable through the JSON control channel; headless CLI rendering to PNG.
- MCP server (`designcraft-cli mcp [--connect PORT]`, docs/mcp.md): headless engine or the running app; commands, batch, document/story inspection, page renders as images, window screenshots, pointer/keyboard/dialog input.
- Web build (`apps/designcraft-web`, trunk): the same UI on WebGPU with a WebGL2 fallback; open/place via the browser file picker or drag-and-drop, save/export as downloads.

**Next (in order):** books · New Window / split view · Ink Manager and Separations Preview · XML structure, tags and export · buttons and forms · PDF/X-4 output intent (needs krilla support) · EPS place.

## How far from full parity (estimate, 2026-10-04)

**Breadth: ~99% weighted** (P0 core 99%, P1 100%, P2 97%) over the 275 features of the InDesign catalogue, scored
row by row in [docs/parity.md](docs/parity.md) (`cargo xtask parity` recomputes it). Many features scored done still
lack some of InDesign's options or dialog details, so **overall parity including depth is about 83%**.

**Remaining work: about 170 wall-clock hours of a single Claude Opus 5.5 agent** (±30%), or roughly 60–85 hours with
four agents in parallel on separate crates:

| Work | Estimate |
|---|---|
| Open P0 (2: PDF/X-4 validation against a certified checker, PostScript interpretation for EPS) | 8 h |
| Open P1 (none partial) | 0 h |
| Open P2 (3 partial: localization breadth, INDD pages (not readable clean-room), in-app media playback) | 14 h |
| Depth and pixel fidelity of every dialog, panel and menu against InDesign 2026 | 90 h |
| Performance (incremental composition, GPU raster) and interchange hardening (IDML/PDF corpus) | 55 h |

Basis: since the first estimate this effort landed about 45 arcs (from parent overrides, Preferences, Stroke panel,
gradients and Pathfinder to table/cell styles, nested/GREP styles and Print) in roughly 14 hours on a heavily loaded
machine — about 1–2 hours per catalogue row.

## Agents: CLI and MCP

Every command is reachable from `designcraft-cli` (`run`, `script` with `$N.path` result references, `app` for the
running window, `describe`, `commands`), from MCP (`designcraft-cli mcp`, tools including `execute` and `batch` with the
same references), and from the app's JSON control channel. See [docs/agents.md](docs/agents.md).

## Milestones

| # | Milestone | State |
**Stage: pre-alpha** · next: alpha, ~0% and ~15–25 h away; beta ~28 points and ~420–750 h beyond that (the core-workflow gate, not the percentage: ready for real work is already ~47%)

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-12 · **Change:** minor (text threads by hand; the alpha gate waits on Primary Text Frame page fixes) · **Target:** Adobe InDesign 2026
> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-13 · **Change:** minor (parent canvas refresh) · **Target:** Adobe InDesign 2026
> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-13 · **Change:** minor (bounded control click/drag input) · **Target:** Adobe InDesign 2026
> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-12 · **Change:** minor (text stroke behind fill) · **Target:** Adobe InDesign 2026
> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-13 · **Change:** minor (text threads by hand; the alpha gate waits on Primary Text Frame page fixes; Links source paths) · **Target:** Adobe InDesign 2026
> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-13 · **Change:** minor (table anchors keep their own paragraphs) · **Target:** Adobe InDesign 2026

DesignCraft aims at full Adobe InDesign parity, and to be better: faster, open (documented JSON
format + IDML), scriptable by agents (MCP), and available on the web.

## Headline numbers

| | Value | Kind |
|---|---|---|
| **Feature breadth** | **~80%** (69% of InDesign 21.6's in-scope menu items matched; 99% of our 275-row checklist) | measured bounds, estimate between |
| **Ready for real work** | **~47%** (range 42–52%) | estimated: weighted sum over the dimensions below |
| Mainstream practitioner | **~42%** (weekly core 60.7% × 0.88 interaction × 0.92 stability × 0.85 file exchange) | estimated |
| Essentials user | **~54%** (core features 72% × 0.90 launch × 0.88 discoverability × 0.95 files received) | estimated |
| Remaining to **beta** | **420–750 Opus 5.5 agent-hours** (~120–200 wall-clock hours with 4–5 agents) | estimated |
| Remaining to **full parity** | **1,150–1,950 Opus 5.5 agent-hours** | estimated |

| Audience | Ready | Opus 5.5 agent-hours to ~95% | Work that dominates |
|---|---|---|---|
| Full target (ready for real work) | ~47% | 1,150–1,950 (~70% parallel) | depth across all areas, scripting compatibility, localization, typography fidelity, INDD/IDML |
| Mainstream practitioner | ~42% | 500–850 (~65% parallel) | INDD import and IDML corpus, composition fidelity, weekly-area depth, PDF/X-1a and printing |
| Essentials user | ~54% | 135–250 (~50% parallel) | start-up robustness, context menus and copy/paste, threading, fonts, Windows printing |

**Why pre-alpha:** it fails one of InDesign's core workflows, the alpha gate in
[docs/roadmap.md](docs/roadmap.md#alpha-gate). Text threads by hand (click a port, then click or
draw a frame; thread and unthread commands), but Primary Text Frame autoflow adds pages users
can't delete (#165). Everything else in the gate works end to end on macOS (document setup,
styles, images, object editing, PDF export, save and reopen), and ready for real work (~47%) is
above the alpha bar, so the Primary Text Frame page fixes are the whole distance to alpha.
Beyond that, beta needs INDD opening, IDML proven on real files, composition compared with
InDesign's, and basic editing fixed on Windows and Linux. Details:
[docs/target-app-parity.md](docs/target-app-parity.md).

## By dimension

| Dimension | Ready | Hours | Doc |
|---|---|---|---|
| Features (depth) | 55% | 250–400 | [target-app-parity.md](docs/target-app-parity.md) |
| Typography and composition fidelity | 45% | 120–200 | [typography-parity.md](docs/typography-parity.md) |
| File formats | 40% | 150–250 + 20–60 (INDD) | [file-format-parity.md](docs/file-format-parity.md) |
| UI/UX fidelity | 45% | 120–180 | [ui-parity.md](docs/ui-parity.md) |
| Stability | 45% | 60–100 | [gaps.md](docs/gaps.md) |
| Performance | 65% | 40–80 | [gaps.md](docs/gaps.md) |
| Localization | 45% | 120–180 | [localization-parity.md](docs/localization-parity.md) |
| Platforms | 55% | 40–80 | [hardware-parity.md](docs/hardware-parity.md) |
| Hardware | 45% | 20–40 | [hardware-parity.md](docs/hardware-parity.md) |
| Ecosystem (scripting, plug-ins) | 15% | 150–250 | [gaps.md](docs/gaps.md) |
| AI features | 15% | 60–120 | [gaps.md](docs/gaps.md) |

All "Ready" figures are estimates; the weights and evidence are in target-app-parity.md.

## Features

Breadth from [docs/parity-checklist.md](docs/parity-checklist.md) (`cargo xtask parity`,
measured); Ready estimated.

| Area | Breadth | Ready | Hours |
|---|---|---|---|
| Application shell & workspace | 99% | 55% | 40–60 |
| Documents, pages, spreads | 100% | 65% | 20–35 |
| Layers | 100% | 75% | 5–10 |
| Frames, shapes & paths | 100% | 60% | 25–40 |
| Transform | 100% | 70% | 10–20 |
| Fill, stroke, colour | 100% | 60% | 20–35 |
| Effects & transparency | 100% | 50% | 20–30 |
| Placing & links | 93% | 55% | 20–35 |
| Type & text frames | 99% | 45% | 40–60 |
| Typography | 100% | 45% | see typography-parity.md |
| Styles | 100% | 60% | 15–25 |
| Tables | 100% | 55% | 25–40 |
| Long documents | 100% | 45% | 30–50 |
| Interactivity & digital | 100% | 40% | 30–50 |
| Output & production | 92% | 55% | 40–70 |
| XML & automation | 100% | 35% | see Ecosystem |
| View & navigation | 100% | 65% | 10–20 |
| Undo, history, saving | 100% | 70% | 5–10 |
| Accessibility | 100% | 50% | 15–25 |

## Languages

Interface strings translated, of 1,724 measured. Detail: [docs/localization-parity.md](docs/localization-parity.md).

| Language | Code | Status | Translated |
|---|---|---|---|
| English | en | full | 100% |
| Simplified Chinese | zh | partial (no canvas IME) | 83% |
| Spanish | es | partial | 83% |
| Hindi | hi | none | 0% |
| Arabic | ar | partial (RTL UI and composition) | 85% |
| French | fr | partial | 83% |
| Portuguese (Brazil) | pt-br | partial | 61% |
| Indonesian | id | none | 0% |
| Japanese | ja | partial (no canvas IME) | 95% |
| German | de | partial | 83% |
| Korean | ko | none | 0% |
| Vietnamese | vi | none | 0% |

Also shipped: Italian (85%), Ukrainian (83%), Georgian (100%).

## Upcoming

Ranked; detail and estimates in [docs/roadmap.md](docs/roadmap.md), the full list in
[docs/gaps.md](docs/gaps.md).

1. **Alpha gate:** Primary Text Frame page fixes: autoflow adds pages users can't delete (#165) (5–7 h)
2. Basic editing on every platform: copy/paste, context menus, caret formatting (30–50 h)
3. CI running `cargo xtask ci` on pull requests (4–8 h)
4. Open INDD through a clean-room converter, owner decision on #114 (20–60 h)
5. IDML real-file corpus and fixes (60–100 h)
6. Composition oracle against InDesign; optical kerning; overset rule (80–140 h)
7. Start on any GPU; find every installed font (25–50 h)

## Documents

[target-app-parity.md](docs/target-app-parity.md) (assessment) · [gaps.md](docs/gaps.md) (work
list) · [roadmap.md](docs/roadmap.md) (milestones, plan) · [parity-checklist.md](docs/parity-checklist.md)
(presence checklist) · [typography-parity.md](docs/typography-parity.md) ·
[file-format-parity.md](docs/file-format-parity.md) · [ui-parity.md](docs/ui-parity.md) ·
[hardware-parity.md](docs/hardware-parity.md) · [localization-parity.md](docs/localization-parity.md) ·
[architecture.md](docs/architecture.md) · agents: [agents.md](docs/agents.md), [mcp.md](docs/mcp.md),
[control-protocol.md](docs/control-protocol.md) · CJK: [cjk.md](docs/cjk.md),
[cjk-typography.md](docs/cjk-typography.md) · Arabic: [arabic-typography.md](docs/arabic-typography.md)

## Progress log

- **2026-10-16** · `anchored.frame`: the text frame and column an anchored object is set in after composition, with the column's width and the frame's bounds (null while overset), as InDesign scripts read parentTextFrames to size inline graphics to the column. No readiness percentage change.
- **2026-10-16** · Keyed script labels: IDML `KeyValuePair` labels are read and written (they were dropped), and `object.insertLabel`, `object.extractLabel` and `object.labels` set and read them, as InDesign scripts do with insertLabel/extractLabel; `object.findByLabel` finds objects by key and value and now sees anchored objects. No readiness percentage change.
- **2026-10-13** · Switching between document and parent pages refreshes the canvas immediately, preventing black parent pages and stale document objects when the zoom stays the same. Ordinary edits retain partial repainting. No readiness percentage change.

Newest first.

- **2026-10-14** · The Type tool draws a new text frame over a locked empty frame, as InDesign does, instead of turning the locked frame into a text frame. No readiness percentage change.
- **2026-10-14** · 100% black shows as pure black on screen by default, as InDesign's Appearance of Black does out of the box (Display All Blacks as Rich Black); Preferences › Appearance of Black › Display All Blacks Accurately still shows it as the dark grey it prints, and a saved choice is kept. No readiness percentage change.
- **2026-10-14** · Pathfinder keeps the frontmost object, as InDesign does: a photo placed over shapes and intersected with them shows through the shapes (it was dropped, and the shapes kept their black fill); Add, Exclude Overlap and Minus Back take the front object's fill too, Subtract still keeps the back one. No readiness percentage change.
- **2026-10-14** · The Color panel's Tint field keeps what is typed while it has focus and applies it on Enter or when it loses focus, clamped to 0–100 %. Typing 60 over 100 % no longer snaps back to 100 after the first digit (10060), and a typed tint is no longer ignored. No readiness percentage change.
- **2026-10-15** · IDML: ruler guides import and export (they were dropped). Each page's guides come in measured from the document's ruler origin (spread, page or spine), with Fit to Page, Locked and their layer, parent pages' included, and go out again under their pages. No readiness percentage change.
- **2026-10-15** · Find Font no longer reports a style missing when it differs from the installed one only in spacing or case (InDesign files can name `Regular ` with a trailing space); the text was already drawn in the right face. No readiness percentage change.
- **2026-10-15** · A line of text is set when its baseline fits in the frame, as in InDesign: descenders may hang below the frame's bottom. A frame sized to the cap height of a big heading (103 pt in a 2.77 cm frame, as Zero2Graphic's magazine tutorial draws them) no longer oversets. No readiness percentage change.
- **2026-10-15** · A story that ends with a return is no longer overset when only its empty last paragraph doesn't fit, as in InDesign (Preflight said "Overset text: 0 characters" for a caption InDesign shows with no errors). No readiness percentage change.
- **2026-10-15** · Flex Layout (InDesign 2026): Object › Flex Layout (Add to, Convert to, Remove) and a Flex Layout panel laid out as InDesign's, also in Properties. A container lays out its objects by direction, wrap, justification, the alignment grid, Fixed/Auto width and height, padding and spacing, after every edit; Add detects aligned objects (direction, Auto size, Space Between at their spacing); resizing a container reflows its objects instead of scaling them; dragging an object in one reorders it. Montilla Design's tutorial replays step for step. No readiness percentage change.
- **2026-10-14** · Layout › Margins and Columns and Layout › Create Guides are laid out as InDesign's: Margins in Top/Bottom and Inside/Outside pairs with a link button that makes all four the same, Rows and Columns side by side, Fit Guides to Margins or Page, and a Preview checkbox that shows the change on the canvas before OK. A measurement field works out arithmetic when you leave it (`0.4375 in*2` becomes `0.875 in`). No readiness percentage change.
- **2026-10-14** · The Pathfinder panel is laid out as InDesign's: Paths (Join, Open, Close, Reverse), Pathfinder, Convert Shape and Convert Point, each a row of icon buttons. Join Path closes an open path or joins two at their nearest ends (`path.join`), and Convert Point makes the selected points plain, corner, smooth or symmetrical (`path.convertPoints`). No readiness percentage change.
- **2026-10-12** · Linked text files (#597): placed Word, RTF and text files can stay linked; the Links panel shows them OK / Modified / Missing, and Update imports the file again into the same frames with the saved style map (the Links panel asks before replacing edits made in the layout). No readiness percentage change.
- **2026-10-15** · IDML: the CJK leading model InDesign writes on [No paragraph style] of every document (Aki Below) only applies when the document uses a Japanese composer, as in InDesign; a Western document's paragraphs after a size change no longer sit lower than in InDesign (Montilla Design's Flex Layout file: its captions came out overset). No readiness percentage change.
- **2026-10-14** · Type › Fill with Placeholder Text fills a frame with paragraphs of a few sentences, as InDesign does, so one paragraph can be styled on its own (it was one long paragraph, and the filler ran sentences together, "interit.Ovid"). No readiness percentage change.
- **2026-10-13** · Object › Fitting has InDesign's shortcuts: Fill Frame Proportionally ⌘⌥⇧C, Fit Content Proportionally ⌘⌥⇧E, Fit Frame to Content ⌘⌥C, Fit Content to Frame ⌘⌥E, Center Content ⌘⇧E. No readiness percentage change.
- **2026-10-13** · Gradients between process colours blend as inks, as InDesign shows them: magenta to cyan passes through C 50 M 50 (a light lilac) on screen instead of the darker RGB mix, and a midpoint between CMYK or grey stops no longer turns the PDF shading RGB. No readiness percentage change.
- **2026-10-14** · The Swatches panel has InDesign's header: a Fill/Stroke proxy, Formatting Affects Container/Text and the Tint of the colour in front (a tint alone keeps each object's own swatch). Clicking a swatch applies it to the fill or stroke in front (of the text, when formatting affects text), and the bottom row has New Color Group, New Swatch and Delete Swatch. No readiness percentage change.
- **2026-10-14** · With the Page tool, the Control panel shows the page as InDesign's does: W/H from a reference point, page-size preset (US Business Card added), orientation, Liquid Page Rule (now with Controlled by Parent) and Show Parent Page Overlay, which draws the parent page's size over a resized page. No readiness percentage change.
- **2026-10-14** · On an object-based page the Page tool shows a selected object's liquid pins and resize locks as InDesign does: a click pins a side to the page edge or locks the width or height, with InDesign's tooltips. No readiness percentage change.
- **2026-10-14** · The Liquid Layout panel follows InDesign's: Liquid Page Rule with the page number, Content › Auto-Fit, and Object Constraints › Resize with Page and Pin for the selected object on an object-based page; Create Alternate Layout moved to the Layout menu. No readiness percentage change.
- **2026-10-14** · With the Page tool, the Control panel shows the page as InDesign's does: W/H from a reference point, page-size preset (US Business Card added), orientation, Liquid Page Rule (now with Controlled by Parent) and Show Parent Page Overlay, which draws the parent page's size over a resized page. No readiness percentage change.
- **2026-10-14** · With the Page tool, the Control panel shows the page as InDesign's does: W/H from a reference point, page-size preset (US Business Card added), orientation, Liquid Page Rule and Show Parent Page Overlay, which draws the parent page's size over a resized page. No readiness percentage change.
- **2026-10-14** · The Page tool works as InDesign's: a click selects a page and shows its handles and liquid page rule; dragging a handle previews the rule and springs back, Alt-drag resizes the page from the opposite edge, Shift keeps proportions, with a W/H readout. No readiness percentage change.
- **2026-10-14** · With the Page tool, a guide dragged from a ruler is a liquid guide (dashed), as in InDesign; guide-based pages stretch the objects it crosses. No readiness percentage change.
- **2026-10-14** · The Pages panel menu follows InDesign's order: Insert Pages…, Move Pages…, Duplicate Spread (and at End of Document), Delete Page/Spread, New Parent…, Apply Parent to Pages, Override All Parent Page Items, Parent Pages, Create Alternate Layout…, Numbering & Section Options…, Allow Selected Spread to Shuffle. No readiness percentage change.
- **2026-10-14** · Pages panel: Alt-drag copies a page, a page dropped on Create new page is duplicated at the end, Edit page size offers the presets and Custom Page Size…; duplicated pages keep their own size, rule and guides; thumbnails of pages of their own size stay in their rows. No readiness percentage change.
- **2026-10-12 (PDFW)** · PDF, booklet and book composition share guarded writing, metadata and font preparation through craft-pdf. Layout, shaping, placed graphics, colour policy, tagging and PDF/X postprocessing remain in DesignCraft. Failed native drawing returns an export error; failed writer cleanup cannot serialize an incomplete document. No readiness percentage change.

- **2026-10-13** · Copy with a placed graphic selected (Direct Selection) copies just the graphic, uncropped, where it sits on the page and without the frame's stroke, so Paste Into puts the whole image in place; Cut and Delete then remove only the graphic and leave its frame empty. Nested selections copy in their spread coordinates. No readiness percentage change.
- **2026-10-13** · Object effects (Basic Feather, Inner Glow, shadows, glows, bevel, satin, directional and gradient feathers) apply to frames with pasted-in content, on screen and in image exports: the frame, its clipped content and its stroke are one object to the effect. PDF export still has no soft effects for any object. No readiness percentage change.
- **2026-10-13** · Links rows show their source paths; selecting a link shows its full path wrapped inside the panel, so same-named files from different folders can be distinguished. No readiness percentage change.
- **2026-10-13** · The Text Wrap panel is laid out as InDesign's: the wrap buttons with Invert, and the four offsets (Top, Left, Bottom, Right) showing the object's values, with the link button that sets them all the same. It used to show a single offset, always 0. No readiness percentage change.
- **2026-10-13** · Radial gradients stretch with the object's box, as InDesign draws them: by default an ellipse reaching the frame's sides (it was a circle reaching only the longer side), and a Gradient Swatch tool drag keeps the box's proportions; on screen and in PDF. No readiness percentage change.
- **2026-10-13** · The Window menu follows InDesign's order (Arrange, Workspace, then the panels alphabetically with Color, Editorial, Object & Layout, Styles… submenus), and Window › Workspace lists the workspaces as InDesign does, now with Essentials Classic: the Control panel and an icon column of named panel groups. No readiness percentage change.
- **2026-10-14** · In a facing document, pages take their side from their section's page numbers, as InDesign shuffles them: a section that starts at 1 on a left page moves it to the right and the pages after it follow, with their objects. No readiness percentage change.
- **2026-10-14** · The Type menu follows InDesign's order (Size, Character ⌘T, Paragraph ⌥⌘T, Glyphs, the style panels, Notes, footnotes and endnotes, then the insert submenus and Show Hidden Characters); Current Page Number has ⌥⇧⌘N. No readiness percentage change.
- **2026-10-14** · Colour parameters: a bare `{c,m,y,k}` is always read as 0..100, so C 1 is a 1 % tint and no longer
  solid cyan; fractions take the colour's own `{model: "cmyk", …}` form (#910). No readiness percentage change.
- **2026-10-14** · The Tools panel follows InDesign's: Gradient Swatch and Gradient Feather are tools of their own; in two columns Content Collector and Placer, Apply Color / Gradient / None and Normal / Preview each get a button; View Options holds Frame Edges, Rulers, Guides, Smart Guides, Baseline Grid and Hidden Characters; Apply Color (,) and Apply Gradient (.) reuse the last colour and gradient. No readiness percentage change.
- **2026-10-13** · Create Outlines makes one compound path per line (and per text colour within a line), grouped, as InDesign does: Ungroup then leaves each line on its own to be spaced and made into one compound path again. A single line in one colour stays a single compound path. No readiness percentage change.
- **2026-10-13** · Gradient swatches work on strokes: drawn across the path's box on screen and in PDF (they drew nothing), set with `object.gradient {target: stroke}` and in the Gradient panel when the Stroke box is in front; the stroke's gradient vector moves with the shape and round-trips through IDML. No readiness percentage change.
- **2026-10-13** · Text takes gradient swatches: a gradient fill or stroke on characters spans the text frame's box (on a path: the path's), on screen and in PDF. No readiness percentage change.
- **2026-10-12** · Text threads by hand: click a selected text frame's in or out port to load
  the text cursor, then click a frame to thread it, click empty space for a column-wide frame or
  drag one (it snaps like frame drawing); Shift-click autoflows, Alt-click stays loaded on the
  frame just added; click the linked frame or double-click a port to break the thread.
  `text.thread`, `text.unthread`, `text.removeFromThread` with undo; locked frames and layers
  refuse threads. The alpha gate now waits only on the Primary Text Frame page fixes (#165). No
  readiness percentage change.
- **2026-10-14** · Georgian interface (`ka`, Edit ▸ Interface Language ▸ ქართული): every catalog key
  translated, count captions as `noun: n`.
- **2026-10-13** · Layout ▸ Table of Contents opens a real dialog: title and title style, Include Paragraph Styles and Other Styles lists with Add and Remove, and per style its entry style, page number position (after, before, none), separator and level. Replace Existing regenerates the current table of contents in place; otherwise OK makes a new frame on the page in view. IDML export writes the definition as a TOC style that import reads back, so Update works after a round trip. No readiness percentage change.
- **2026-10-13** · Control-channel clicks and drags validate positive counts and the complete pending-event budget before queuing input (#732). Oversized requests leave pending gestures intact, preventing unbounded click-count allocation. No readiness percentage change.
- **2026-10-13** · Backspace between two consecutive tables, pasting a table next to a table anchor, and pasting several paragraphs right before one keep every table on its own anchor paragraph. Previously a table was dropped or left without its anchor, and the saved file failed to reopen. No readiness percentage change.
- **2026-10-13** · Dialogs and the About window move when you drag their title row, so the page behind them can be seen. A dragged dialog may go partly off-screen, but its title row stays reachable; each dialog opens centred, and again after the window is resized. No readiness percentage change.
- **2026-10-13** · Command scripts look up only a well-formed `$N.path` / `$last.path` reference; other strings starting with `$`, such as a GREP replacement of `$1` and a nonbreaking space, stay as written, and `$$` writes a literal `$` (#749). No readiness percentage change.
- **2026-10-13** · An explicit recovery folder (`file.recovery.save` with `dir`) never overwrites or deletes another running app's checkpoints. Document uids start at 1 in every process, so two apps saving recovery to one chosen folder replaced or removed each other's `<uid>.designcraft`. A run replaces or removes only the entries it wrote or recovered there, refuses an occupied uid, and refuses a folder whose run lock another app holds. No readiness percentage change.
- **2026-10-13** · Path edits (`path.setAnchor`, `path.moveAnchors`, `path.appendAnchor`, `path.reverse`, `path.close`, anchor add/delete/convert, Erase, Smooth and Scissors) refuse a path that is locked, sits in a locked group or on a locked layer (#735). Properties links to the Stroke, Character, Paragraph, Effects, Text Wrap and Info panels open them in a customized panel layout too (#736). No readiness percentage change.
- **2026-10-13** · IDML export keeps parent item overrides: each page lists its overridden parent items and the local copy keeps its link to the parent item, so a reopened IDML file shows only the local copy (#725). No readiness percentage change.
- **2026-10-13** · macOS: a command chosen with the mouse in the menu bar runs at once. It used to wait for the next mouse move or key press over the window, so File › Open, Place and Save As seemed to take 10–20 seconds to open their dialogs. No readiness percentage change.
- **2026-10-13** · Text recomposes when the document baseline grid changes, when a layer holding a text-wrap object is shown or hidden, or when a section's start number, numbering style, prefix or marker changes; the composition cache keyed only on the section count and item identities and kept the old layout. No readiness percentage change.
- **2026-10-13** · Reflowable EPUB and HTML export include tables (header and footer rows, merged cells), footnotes (a linked note after the paragraph that refers to it) and endnotes (a linked section at the end of the book). No readiness percentage change.
- **2026-10-13** · Word import: table cells read text by the same rules as body text, so deleted text, moved-away text and field codes stay out (#721). Word's Normal comes in as its own paragraph style "Normal" over the document defaults, so text formatted only by Normal keeps its look; it conflicts only with a document's own Normal, and the style map can send it to [Basic Paragraph] (#722). No readiness percentage change.
- **2026-10-12** · macOS Dock Quit uses the same unsaved-document prompts as window and menu Quit. Repeated requests keep the current answers; Cancel retains every tab and unsaved recovery checkpoint, and completed Save/Don't Save answers exit through the ordinary event loop. No readiness percentage change.
- **2026-10-12** · Text strokes can sit behind the fill: the stroke lies wholly outside the letters at its full weight on screen, in PNG and in PDF, set from the Properties panel or Character Style Options. IDML `StrokeAlignment` on text imports and exports as this setting, so imported text with outside-aligned strokes keeps its letter shapes (#619). No readiness percentage change.
- **2026-10-12** · Placed Word documents keep their bullets and numbering (#626): `numbering.xml` levels become bullet or numbered paragraphs with Word's number formats, label text, start numbers, restarts and list indents, also when a paragraph style carries the list. No readiness percentage change.
- **2026-10-12** · Orderly native exit writes the latest recovery state after finalizing current quit approvals; incomplete or stale plans keep their documents, and Undo to the saved state clears stale recovery. Unavailable or failed recovery is reported. Cancelable macOS Dock Quit remains open (#544). No readiness percentage change.
- **2026-10-12** · Recovered documents keep a checkpoint until saved, closed or reverted, including a second crash before autosave. Restarted document IDs and explicit recovery folders preserve other snapshots; failed republishing retains the original. No readiness percentage change.
- **2026-10-12** · Book numbering and style updates prepare every chapter before saving. Only closed native chapters are changed; imported IDML remains a read-only style/PDF source. If a later disk write fails, the error names the completed chapters; the batch is not a whole-book transaction. No readiness percentage change.
- **2026-10-12** · Desktop write callbacks and CLI raster exports use the atomic writer too; open readers retain the original bytes, while private Unix modes and symlink targets survive replacement. Permanent tests cover both actual callers. No readiness percentage change.
- **2026-10-12** · Save As, library edits and book collection edits persist prospective values before changing live state; failed writes preserve the document and existing collections. Save a Copy rebases data-source paths for its destination without changing the open document. No readiness percentage change.
- **2026-10-12** · The Pen tool continues an open path from either end point, as in InDesign: the cursor shows it over an end point, a click there carries on drawing that path, a drag first reshapes the end point's direction lines, and the other end closes it. Object › Paths › Reverse Path (`path.reverse`); `path.setAnchor`; `path.appendAnchor` and `path.close` take a subpath. No readiness percentage change.
- **2026-10-12** · Editing type on a path: the Type tool clicks, drags and double-clicks along the curve (the character under the pointer, not one on the straightened line), a click on the letters themselves hits the frame, and the caret and highlight bend with the path. No readiness percentage change.
- **2026-10-12** · Type on a path has start and end brackets, as in InDesign: selected path type shows them across the path, and dragging either with the Selection or Direct Selection tool moves where the text starts or ends (text past the end bracket is overset). `type.pathOptions {end}`; IDML reads and writes EndBracket. No readiness percentage change.
- **2026-10-12** · The Eyedropper puts a colour sampled from an image on the stroke when the toolbox's Stroke box is in front, as in InDesign (a line takes it on its stroke). No readiness percentage change.
- **2026-10-12** · Window close and application/menu Quit ask about unsaved documents; later Cancel keeps every tab open, including earlier Don't Save choices. Approvals are rechecked after edits. Cancelable macOS Dock Quit remains open (#544). No readiness percentage change.
- **2026-10-12** · Each running app keeps recovery data in its own locked folder; current folder and lock ownership are checked before writes and cleanup. Failed isolation disables recovery with a warning instead of sharing document IDs. No readiness percentage change.
- **2026-10-12** · Native engine saves and exports replace files atomically; failed writes retain the original, private staging preserves Unix permissions, and valid long filenames work. Desktop Services and CLI raster exports remain separate follow-ups. No readiness percentage change.
- **2026-10-12** · Hyphenation Zone applies to the Single-line Composer only, as in InDesign. With the default Paragraph Composer, ragged text hyphenates where InDesign does (aper-natem), giving the same line breaks. No readiness percentage change.
- **2026-10-12** · The Tools panel's Fill/Stroke keys work: D (default fill and stroke), X (fill or stroke in front), Shift+X (swap per object, tints included) and / (Apply None), as `color.*` commands; swap and default are one undo step (`object.swapFillStroke`, `object.defaultFillStroke`); the Color panel follows the proxy's fill/stroke choice. No readiness percentage change.
- **2026-10-11** · Corner Options keep curved sides curved and round every corner point, including where an arc meets a line (a Pathfinder quarter circle with rounded corners stayed a triangle). No readiness percentage change.
- **2026-10-11** · `story.replaceRange`, `story.setText`, notes, footnotes, endnotes, type on a path and table cells read `\r` and `\r\n` as paragraph returns, as `frame.create` and `text.insert` already did. No readiness percentage change.
- **2026-10-11** · The View menu follows InDesign's: proofing first, then zoom, Rotate Spread, Screen Mode, Display Performance, rulers, Extras, Grids & Guides and Structure; the view items that sat at the end of the Type menu moved there. No readiness percentage change.
- **2026-10-12** · Underlines and strikethroughs are drawn as one bar per stretch of text on screen, as in PDF export: a bar per letter left faint seams between the letters. No readiness percentage change.
- **2026-10-13** · Placed PDFs are drawn through pdfcraft's patched hayro crates: calculator functions whose `idiv` divides by zero or overflows, chains of stitching functions a hundred thousand deep, and mesh shadings with points far off the page, ten thousand overlapping triangles or forty thousand patches no longer panic, overflow the stack, run for over a minute or use more than 2 GiB of memory. A page with a `/UserUnit`, and its trim, bleed and art boxes, are sized in points. No readiness percentage change.
- **2026-10-12** · Properties › Transform shows rotation, shear and X/Y scale, and Rotate 90° and Flip Horizontal/Vertical buttons, as InDesign's does. No readiness percentage change.
- **2026-10-12** · Edit › Step and Repeat has its InDesign dialog: Repeat (Count, or Rows and Columns with Create as a grid), Offset (Vertical, Horizontal, in ruler units) and Preview, which shows the copies while the dialog is open; Cancel takes them away and OK makes them as one undo step. No readiness percentage change.
- **2026-10-12** · Double-clicking the Fill or Stroke box in the Tools panel opens the Color Picker with the selection's colour, as in InDesign (it opened the Swatches panel). No readiness percentage change.
- **2026-10-12** · The Effects panel shows the level its settings apply to ("Group: Color Dodge 100%") and, as in InDesign, Isolate Blending and Knockout Group for a selected group. No readiness percentage change.
- **2026-10-11** · Properties gains InDesign's Pathfinder section (Add, Subtract, Intersect, Exclude Overlap, Minus Back) with several objects selected; section titles and icon buttons have accessible names. No readiness percentage change.
- **2026-10-12** · The Eyedropper samples a placed image: clicking the photo with text (or objects) selected fills them with the image's colour under the pointer, as an unnamed colour (`color.sample`), as in InDesign. Images on locked layers can be sampled. No readiness percentage change.
- **2026-10-12** · The section marker character shows its section's marker text on document pages (nothing when the section has none) instead of the word "Section", which stays on parent pages; editing the marker recomposes (#556). No readiness percentage change.
- **2026-10-12** · On screen, blend modes blend with the objects below and not with the paper, as in InDesign and in PDF export: a Color Dodge or Screen object over blank paper shows as it is. No readiness percentage change.
- **2026-10-12** · A gradient feather on a group fades the group as one, on screen and in PDF. Before, the screen ignored it, and PDF masked the group with a box of zero size because a group has no path of its own. No readiness percentage change.
- **2026-10-12** · In a CMYK transparency blend space (print documents), blend modes blend in CMYK on screen as in InDesign. Color Dodge with 100% K text turns coloured areas under it white and leaves the paper alone, where RGB blending changed almost nothing. PDF export already did this. No readiness percentage change.
- **2026-10-11** · Moving a sheared and rotated text frame no longer leaves trails of text behind: canvas repaints cover the frame's whole text box, not just its outline. No readiness percentage change.
- **2026-10-12** · Words in capitals hyphenate like capitalized words (FERRUM COM-MODI), as in InDesign; Hyphenate Capitalized Words off keeps them whole. No readiness percentage change.
- **2026-10-12** · Load Swatches refuses an `.ase` file with more than 100,000 colors, and `swatch.load` merges a file of 100,000 colors in under 0.1 s (it took 9.6 s of CPU in a test build). No readiness percentage change.
- **2026-10-11** · `transform.move {copy}`, Edit › Duplicate and Step and Repeat return the copies' ids, so a script can go on with them. No readiness percentage change.
- **2026-10-11** · `object.fill` and `object.stroke` refuse a swatch name the document doesn't have (it painted nothing) and suggest the name meant (`Paper` → `[Paper]`). No readiness percentage change.
- **2026-10-13** · Dropping an `.ase` file on the window loads its swatches, as Load Swatches does (the drop failed with "Can't Place the File"). No readiness percentage change.
- **2026-10-11 (printing):** `file.printers` and `file.print` use craft-libs/craft-print for CUPS and Windows. DesignCraft retains PDF export, page selection, spreads, marks and bleed; dry runs create no temporary PDF or print job. Windows selects matching or default driver paper and shrinks oversized sheets without enlarging smaller ones; queue duplex defaults are preserved. Headless tests exercise exported bytes, job settings, dry runs and refusals; shared native Windows CI passes, including paused-queue submission and cleanup. PostScript, production print controls and physical output validation remain open. No readiness percentage change.

- **2026-10-10 (UI.4–UI.5):** Panel tabs can move into other groups, split the dock, float, resize and return to their saved locations through shared docking transactions. Layouts persist in custom workspaces; UI and control commands share the same validated action path. Shared pane geometry and widget-state theme application retain all five palettes and local fonts. Split panes send keyboard input only to the focused canvas and scope contextual toolbar IDs per pane. No readiness percentage change.
- **2026-10-10** · Version 0.6.0; releases add a Linux riscv64 tar.gz (cross-compiled, CLI
  smoke-tested under QEMU).
- **2026-10-10** · Readiness per audience: full ~47% (method aligned with the standard, was
  42%), mainstream practitioner ~42%, essentials user ~54%, each with hours to ~95%.
- **2026-10-10** · Stage re-normalized to **pre-alpha** under craftrules' core-workflow gate: text
  can't be threaded by hand (no out-port click, no thread command).
- **2026-10-10** · Full re-measure against InDesign 2026 21.6 (menu dump, bundle formats and
  localizations, 124 issues): breadth ~80%, ready ~42% (down from the ~83% "including depth" of
  2026-10-04, on user evidence and the INDD gap); progress docs restructured to the craftrules
  standard (`docs/parity.md` → `docs/parity-checklist.md`; new target-app-parity, gaps, roadmap,
  architecture, typography, file-format, UI, hardware and localization docs). Landed today:
  split columns (#295), PDF export options dialog (#196), column rules (#274), Paragraph Rules
  dialog (#263), Character Style Options (#144), PDF/X-4 RGB handling (#223), Spanish
  hyphenation (#222), Japanese (#272), Italian (#218) and Ukrainian (#236) interfaces, relocated packages relink (#244), graphics backend chosen
  before the window (#297), IDML table styles (#288), warichu (#280), list numbering from IDML
  (#298), security hardening (#153, #155), many text-composition fixes (#282–#293).
- **2026-10-09** · Version 0.5.0; many user reports triaged into issues (#160–#265).
- **2026-10-08** · Version 0.4.0.
- **2026-10-07** · Community PRs: data merge, Brazilian Portuguese, PDF/X-4 flags, Office fonts
  on macOS; CJK 1.x (Korean line breaking, fallback fonts in Package); PrintCraft renamed PdfCraft.
- **2026-10-06** · Snapping to parent items, Windows packaging, ARM64 workflow, control-channel
  hardening.
- **2026-10-05** · FreeBSD build; caret-boundary fixes.
- **2026-10-04** · Never-crash pass across importers and the engine; rotate spread view; kashida
  in styles; previous estimate: breadth 99%, "~83% including depth", ~170 h remaining.
- **2026-10-03** · Release 0.1.1; separations, XML tagging, track changes, PDF/X-4 output
  intent, split window, buttons, menu customization.
- **2026-10-02** · Hyperlinks/Bookmarks panels, conditional text, tagged PDF, lists across
  stories, style groups, Glyphs panel, High Contrast theme.
- **2026-10-01** · First commit; skeleton, IDML, PDF, tables, footnotes, cross-references, index,
  spelling and parallel composition on the first day.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-12 | minor | Manual text threading landed; alpha distance 15–25 h → 5–7 h (Primary Text Frame page fixes, #165) |
| 2026-10-10 | minor | Full number 42% → 47% (method aligned with the standard: plain weighted sum, no new evidence); readiness table with hours per audience |
| 2026-10-10 | minor | Headline adds mainstream practitioner (~42%) and essentials user (~54%); method in docs/target-app-parity.md |
| 2026-10-10 | minor | Stage alpha → pre-alpha under the new core-workflow gate: manual text threading is missing; alpha gate table added to docs/roadmap.md |
| 2026-10-10 | major | Restructured to the progress-docs standard; full re-measure; the "Working today" inventory moved to docs/target-app-parity.md, milestones to docs/roadmap.md, the CLI/MCP section left to docs/agents.md (which already covered it) |
| 2026-10-04 | major | Parity estimate: breadth 99%, depth ~83%, ~170 h |
| 2026-10-01 | major | First roadmap: status, milestones |


### Windows installer unification — release preparation, 2026-10-12

The shared craft-installer UI preserves this revision's install options and provides persistent installation, repair and Windows Settings removal completion screens. Silent modes stay unattended. Earlier Windows acceptance is recorded in packaging/windows/README.md; normal CI and production-profile upgrade qualification against pinned current releases are in progress. No release or PR has been published.

- **2026-10-15** · Windows recovery folders validate the identity of the live locked file without reading it through a second handle. Missing or replaced locks disable writes and cleanup; regression tests cover retained exclusive locking and replacement-file protection. No readiness percentage change.
