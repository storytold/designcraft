//! The menu bar tree (InDesign order), UI-only commands, keyboard shortcuts and Quick Apply.

use serde_json::{Value, json};

use crate::{DesignApp, ScreenMode};

/// Menu and Quick Apply entries that only CJK typesetting uses: hidden unless
/// [`crate::cjk_features`]. The commands still run from scripts, the control channel and MCP.
pub const CJK_ONLY: &[&str] = &["type.tateChuYoko", "app.rubyDialog", "type.kenten", "type.warichu"];

/// Whether command `id` has a menu item or Quick Apply entry in the current interface (CJK-only
/// ones need [`crate::cjk_features`]).
pub fn shown(app: &DesignApp, id: &str) -> bool {
    !CJK_ONLY.contains(&id) || crate::cjk_features(app)
}

/// UI commands: (id, label, shortcut, params).
pub const UI_COMMANDS: &[(&str, &str, Option<&str>, &str)] = &[
    ("app.newDocumentDialog", "New Document…", Some("Cmd+N"), "{}"),
    ("app.openDialog", "Open…", Some("Cmd+O"), "{}"),
    ("app.openRecent", "Open Recent File", None, "{index} — file.openRecent, with an alert when the file can't be opened"),
    ("app.placeDialog", "Place…", Some("Cmd+D"), "{}"),
    ("app.saveDialog", "Save As…", Some("Cmd+Shift+S"), "{}"),
    ("app.save", "Save", Some("Cmd+S"), "{}"),
    ("app.quit", "Quit DesignCraft", Some("Cmd+Q"), "{} — close the window; each document with unsaved changes asks first"),
    ("app.saveCopyDialog", "Save a Copy…", Some("Cmd+Alt+S"), "{} — choose where to write a copy (the document stays as it is)"),
    ("app.loadSwatches", "Load Swatches…", None, "{} — pick a swatch exchange (.ase) file"),
    (
        "app.loadParagraphStyles",
        "Load Paragraph Styles…",
        None,
        "{} — pick a document (.designcraft or .idml) and choose which of its paragraph styles to load (style.load)",
    ),
    ("app.loadCharacterStyles", "Load Character Styles…", None, "{} — the same for its character styles"),
    ("app.loadTextStyles", "Load All Text Styles…", None, "{} — the same for its paragraph and character styles"),
    (
        "effects.dialog",
        "Effects…",
        None,
        "{page?: transparency|dropShadow|innerShadow|outerGlow|innerGlow|bevel|satin|feather|directionalFeather|gradientFeather} — the Effects dialog for the selected objects",
    ),
    ("app.packageDialog", "Package…", Some("Cmd+Alt+Shift+P"), "{} — choose the package folder to create (file.package)"),
    ("app.saveSwatches", "Save Swatches for Exchange…", None, "{path?} — write the colour swatches as .ase"),
    ("app.exportPng", "Export Page as PNG…", Some("Cmd+E"), "{}"),
    ("app.exportIdml", "Export IDML…", None, "{path?} — InDesign Markup (IDML) package"),
    ("app.exportEpub", "Export EPUB…", None, "{path?} — reflowable EPUB 3 (with the first page as its cover)"),
    (
        "app.exportInteractivePdf",
        "Export PDF (Interactive)…",
        None,
        "{path?, fullScreen?, advanceSeconds?} — single pages fitted, bookmarks panel open, buttons, forms, transitions",
    ),
    ("app.exportFixedEpub", "Export EPUB (Fixed Layout)…", None, "{path?} — pre-paginated EPUB 3"),
    ("app.exportHtml", "Export HTML…", None, "{path?} — one self-contained web page"),
    ("app.exportPptx", "Export PowerPoint…", None, "{path?} — every page as a slide of editable shapes, pictures and text"),
    ("app.exportXml", "Export XML…", None, "{path?} — the tagged content"),
    ("app.printBooklet", "Print Booklet…", None, "{path?} — saddle-stitched printer spreads as PDF"),
    ("app.qrCode", "Generate QR Code…", None, "{} — the QR Code dialog (object.qrCode does the work)"),
    ("app.userDictionary", "User Dictionary…", None, "{} — hyphenation exceptions (hyphenation.* commands)"),
    ("app.fittingOptions", "Frame Fitting Options…", None, "{} — the dialog for object.fittingOptions"),
    ("app.menus", "Menus…", None, "{} — show or hide menu items"),
    ("window.hideMenuItem", "Hide Menu Item", None, "{item: \"Menu/Label\", hidden?: bool}"),
    ("edit.dynamicSpelling", "Dynamic Spelling", None, "{on?: bool} — underline misspelled words on the canvas"),
    (
        "app.language",
        "Interface Language",
        None,
        "{lang?: \"\"|de|fr|es|ja|zh|ar|pt-br|it|uk|ru|cs|pl|ka} — menus and panel names (the macOS menu bar follows at once); without lang, opens Preferences, where it is the first setting",
    ),
    (
        "app.flattener",
        "Transparency Flattener Presets",
        None,
        "{preset: \"\"|high|medium|low} — PDF export rasterises spreads with transparency at 300 / 150 / 72 ppi",
    ),
    ("app.graphicCell", "Convert Cell to Graphic Cell…", None, "{} — pick an image for the target table cell"),
    ("app.placeAndLink", "Place and Link", None, "{} — a linked copy of the selected frame's story, beside it"),
    ("app.placeWithOptions", "Place with Import Options…", Some("Cmd+Shift+D"), "{} — Word/RTF style mapping before placing"),
    ("app.exportText", "Export Text…", None, "{path?} — the story being edited, as Text Only (.txt) or Rich Text Format (.rtf)"),
    ("app.exportPdf", "Export PDF…", None, "{path?, …file.exportPdf options} — asks for a path when none is given"),
    ("app.palette", "Quick Apply…", Some("Cmd+Return"), "{} — search styles and commands"),
    ("app.preferences", "Preferences…", Some("Cmd+K"), "{}"),
    ("window.uiScale", "UI Scaling", None, "{scale: 0.5–3 (1 = 100%)} — the size of the whole interface"),
    ("app.keyboardShortcuts", "Keyboard Shortcuts…", None, "{} — view and change the shortcut of any command"),
    ("window.richBlack", "Appearance of Black", None, "{on: bool} — show 100% black as rich black on screen (off: accurately)"),
    ("window.setShortcut", "Set Shortcut", None, "{id, shortcut: \"Cmd+Alt+J\" | \"\" (none) | null (default)} → {shortcut, conflicts: [ids]}"),
    ("window.resetShortcuts", "Reset Shortcuts", None, "{} — every command back to its default shortcut"),
    ("app.findChange", "Find/Change…", Some("Cmd+F"), "{}"),
    ("app.insertTableDialog", "Create Table…", None, "{} — Insert Table dialog (body/header/footer rows, columns)"),
    (
        "app.cellOptionsDialog",
        "Strokes and Fills…",
        None,
        "{} — Cell Options › Strokes and Fills of the target cells: edge proxy, stroke weight, type, colour, tint, gap colour, gap tint, and cell fill (table.setCell)",
    ),
    ("app.footnoteOptionsDialog", "Document Footnote Options…", None, "{} — numbering, formatting and layout of footnotes"),
    ("app.rubyDialog", "Ruby…", None, "{} — the reading set over the selected text"),
    (
        "app.paragraphRulesDialog",
        "Paragraph Rules…",
        None,
        "{rule?: ruleAbove|ruleBelow} — Rule Above / Rule Below of the selected paragraphs (type.para)",
    ),
    (
        "app.paragraphBordersDialog",
        "Paragraph Borders and Shading…",
        None,
        "{tab?: border|shading} — border and shading of the selected paragraphs (type.para)",
    ),
    ("app.spanColumnsDialog", "Span Columns…", None, "{} — span or split columns of the selected paragraphs (type.para)"),
    ("app.bulletsNumberingDialog", "Bullets and Numbering…", None, "{} — bullets or numbers of the selected paragraphs (type.para)"),
    (
        "app.justificationDialog",
        "Justification…",
        Some("Cmd+Alt+Shift+J"),
        "{} — word and letter spacing, glyph scaling and composer of the selected paragraphs (type.para)",
    ),
    ("app.findFontDialog", "Find/Replace Font…", None, "{} — fonts used (missing ones flagged) and replacing them"),
    ("app.insertXrefDialog", "Insert Cross-Reference…", None, "{} — New Cross-Reference dialog (paragraph or text anchor, format)"),
    ("app.deleteAllGuides", "Delete All Guides on Spread", None, "{} — the spread in view"),
    ("app.deletePage", "Delete Page", None, "{} — deletes the page in view"),
    ("app.duplicateSpread", "Duplicate Spread", None, "{} — duplicates the spread in view (pages and items)"),
    ("app.tablePanel", "Table Panel", Some("Shift+F9"), "{}"),
    ("app.tabsPanel", "Tabs", Some("Cmd+Shift+T"), "{} — show or hide the Tabs panel (it opens above the selected text frame)"),
    ("app.characterPanel", "Character", Some("Cmd+T"), "{} — the Character panel"),
    ("app.paragraphPanel", "Paragraph", Some("Cmd+Alt+T"), "{} — the Paragraph panel"),
    ("app.glyphsPanel", "Glyphs", Some("Alt+Shift+F11"), "{} — the Glyphs panel"),
    ("app.characterStylesPanel", "Character Styles", Some("Cmd+Shift+F11"), "{} — the Character Styles panel"),
    ("app.paragraphStylesPanel", "Paragraph Styles", Some("Cmd+F11"), "{} — the Paragraph Styles panel"),
    ("app.insertCurrentPageNumber", "Current Page Number", Some("Cmd+Alt+Shift+N"), "{} — the Current Page Number marker at the insertion point"),
    ("app.storyEditor", "Edit in Story Editor", Some("Cmd+Y"), "{story?}"),
    ("view.zoomIn", "Zoom In", Some("Cmd+="), "{}"),
    ("view.zoomOut", "Zoom Out", Some("Cmd+-"), "{}"),
    ("view.fitPage", "Fit Page in Window", Some("Cmd+0"), "{}"),
    ("view.fitSelection", "Fit Selection in Window", Some("Cmd+Alt+="), "{}"),
    ("view.fitSpread", "Fit Spread in Window", Some("Cmd+Alt+0"), "{}"),
    ("view.actualSize", "Actual Size", Some("Cmd+1"), "{}"),
    ("view.entirePasteboard", "Entire Pasteboard", Some("Cmd+Alt+Shift+0"), "{}"),
    ("view.zoom", "Zoom To", None, "{zoom: 1.0 = 100%}"),
    ("view.screenMode", "Screen Mode", None, "{mode: normal|preview|bleed|slug|presentation}"),
    ("view.togglePreview", "Toggle Normal/Preview", Some("W"), "{}"),
    (
        "view.togglePresentation",
        "Toggle Presentation",
        Some("Shift+W"),
        "{} — enters Presentation, or returns to the mode it was entered from; Esc also leaves it",
    ),
    (
        "app.formattingAffectsText",
        "Formatting Affects Text",
        Some("J"),
        "{on?: bool} — with text frames selected, colours go to their text (on) or the frames (off); toggles without `on`",
    ),
    (
        "color.defaultFillStroke",
        "Default Fill and Stroke",
        Some("D"),
        "{} — the selection gets no fill and a black stroke (object.defaultFillStroke); text colour edits (Formatting Affects Text) get a black fill and no stroke",
    ),
    (
        "color.swapFillStroke",
        "Swap Fill and Stroke",
        Some("Shift+X"),
        "{} — each selected object's fill and stroke colours (and tints) trade places (object.swapFillStroke), or the text's with Formatting Affects Text",
    ),
    ("color.toggleFillStroke", "Toggle Fill and Stroke", Some("X"), "{} → {strokeFront} — which of fill and stroke the proxy has in front"),
    (
        "color.applyNone",
        "Apply None",
        Some("/"),
        "{} — [None] for the fill or stroke in front of the proxy, of the objects or the text (Formatting Affects Text)",
    ),
    ("color.applyColor", "Apply Color", Some(","), "{} — the colour applied last ([Black] at first) to the fill or stroke in front of the proxy"),
    (
        "color.applyGradient",
        "Apply Gradient",
        Some("."),
        "{} — the gradient applied last (else the document's first, else white to black) to the fill or stroke in front of the proxy",
    ),
    ("view.frameEdges", "Show/Hide Frame Edges", Some("Cmd+H"), "{}"),
    ("view.rulers", "Show/Hide Rulers", Some("Cmd+R"), "{}"),
    ("view.guides", "Show/Hide Guides", Some("Cmd+;"), "{}"),
    ("view.snapToGuides", "Snap to Guides", Some("Cmd+Shift+;"), "{}"),
    ("view.snapToDocumentGrid", "Snap to Document Grid", None, "{}"),
    ("view.smartGuides", "Smart Guides", None, "{}"),
    (
        "view.snapPreferences",
        "Smart Guide Options",
        None,
        "{alignEdges?, alignCenters?, smartDimensions?, smartSpacing?: bool, zone?: px} — what smart guides snap to and how close; returns the current values",
    ),
    ("view.baselineGrid", "Show/Hide Baseline Grid", Some("Cmd+Alt+'"), "{}"),
    ("view.documentGrid", "Show/Hide Document Grid", Some("Cmd+'"), "{}"),
    ("view.textThreads", "Show/Hide Text Threads", Some("Cmd+Alt+Y"), "{}"),
    ("view.hiddenCharacters", "Show Hidden Characters", Some("Cmd+Alt+I"), "{}"),
    ("view.taggedFrames", "Show/Hide Tagged Frames", None, "{} — XML-tagged frames outlined in their tag colour"),
    ("window.hidePanels", "Show/Hide Panels", Some("Tab"), "{} — every panel, the Tools panel included"),
    ("window.hidePanelsExceptTools", "Show/Hide Panels Except Tools", Some("Shift+Tab"), "{}"),
    ("window.nextDocument", "Next Document", Some("Cmd+F6"), "{}"),
    ("window.previousDocument", "Previous Document", Some("Cmd+Shift+F6"), "{}"),
    ("app.layerOptions", "Object Layer Options…", None, "{} — show or hide the layers of the selected placed PDF"),
    (
        "app.layerOptionsDialog",
        "Layer Options…",
        None,
        "{id?} (default: the active layer) — name, colour, show, lock, print, guides, text wrap (layer.set)",
    ),
    ("app.swatchOptionsDialog", "Swatch Options…", None, "{name} — name, colour type, mode and values, tint (swatch.options)"),
    ("app.colorGroupOptionsDialog", "Color Group Options…", None, "{name} — rename a colour group (swatch.renameColorGroup)"),
    ("view.tagMarkers", "Show Tag Markers", None, "{on?: bool} — brackets around inline-tagged text"),
    ("view.flattenerPreview", "Flattener Preview", None, "{on?: bool} — highlight objects that involve transparency"),
    ("view.rotateSpread", "Rotate Spread", None, "{angle: 90 (clockwise) | -90 | 180 | 0 (clear)} — turn the view in quarter turns"),
    ("view.proofColors", "Proof Colors", None, "{on?: bool} — simulate the proof target on screen"),
    (
        "view.proofSetup",
        "Proof Setup",
        None,
        "{target?: workingCmyk|srgb|legacyMacRgb|monitorRgb|protanopia|deuteranopia|cmyk:<profile>, intent?, simulatePaper?: bool|\"toggle\"}",
    ),
    ("app.colorSettings", "Color Settings…", None, "{} — the working spaces, intent and black-point compensation"),
    ("view.separations", "Separations Preview", None, "{plate?: cyan|magenta|yellow|black|null, inkLimit?: percent|null}"),
    ("view.overprintPreview", "Overprint Preview", Some("Cmd+Alt+Shift+Y"), "{} — show how overprinting inks mix"),
    ("view.fastDisplay", "Fast Display", Some("Cmd+Alt+Shift+Z"), "{} — placed graphics as grey boxes, no effects"),
    ("view.typicalDisplay", "Typical Display", Some("Cmd+Alt+Z"), "{} — low-resolution image proxies"),
    ("view.highQualityDisplay", "High Quality Display", Some("Cmd+Alt+H"), "{} — full-resolution images"),
    ("view.firstPage", "First Page", Some("Cmd+Shift+PageUp"), "{} — scroll to the document's first page (zoom kept)"),
    ("view.previousPage", "Previous Page", Some("Shift+PageUp"), "{} — scroll to the page before the one in view"),
    ("view.nextPage", "Next Page", Some("Shift+PageDown"), "{} — scroll to the page after the one in view"),
    ("view.lastPage", "Last Page", Some("Cmd+Shift+PageDown"), "{} — scroll to the document's last page"),
    ("view.nextSpread", "Next Spread", Some("Alt+PageDown"), "{} — centre the spread after the one in view"),
    ("view.previousSpread", "Previous Spread", Some("Alt+PageUp"), "{} — centre the spread before the one in view"),
    ("view.goToPage", "Go to Page…", Some("Cmd+J"), "{page: number (position) | \"name\" (section page name, or \"+n\" for a position)}"),
    (
        "window.panel",
        "Show Panel",
        None,
        "{panel: properties|pages|layers|swatches|paragraphStyles|characterStyles|stroke|character|paragraph|textWrap|links|table|preflight}",
    ),
    ("window.floatPanel", "Float Panel", None, "{panel, x?, y?} — the panel in its own movable window"),
    (
        "window.panel.layout",
        "Change Panel Layout",
        None,
        "{action} — serialized docking action, including floating geometry, divider sizes and accordion state",
    ),
    (
        "window.panel.move",
        "Move Panel",
        None,
        "{panel, anchor, zone?: tab|center|left|right|top|bottom, before?: panel} — group, split or reorder a panel",
    ),
    ("window.panel.float", "Float Panel", None, "{panel, x?, y?, width?, height?} — detach a panel inside the application window"),
    ("window.panel.dock", "Dock Panel", None, "{panel, anchor?, zone?, before?} — return a floating panel to a dock group"),
    ("window.panel.activate", "Activate Panel", None, "{panel} — show and select a panel in its current group"),
    ("window.panel.close", "Close Panel", None, "{panel} — hide a panel"),
    ("window.panel.closeGroup", "Close Tab Group", None, "{panel} — hide the panel and the panels tabbed with it"),
    ("window.collapseToIcons", "Collapse to Icons", None, "{} — the panel dock shrinks to its column of icons"),
    ("window.dockPanel", "Dock Panel", None, "{panel} — back into the dock's icon column"),
    ("window.controlBar", "Control", Some("Cmd+Alt+6"), "{}"),
    // Object › Fitting with InDesign's shortcuts (one `object.fit` mode each).
    ("fit.fillFrameProportionally", "Fill Frame Proportionally", Some("Cmd+Alt+Shift+C"), "{} — object.fit {mode: fillProportionally}"),
    ("fit.contentProportionally", "Fit Content Proportionally", Some("Cmd+Alt+Shift+E"), "{} — object.fit {mode: fitProportionally}"),
    ("fit.frameToContent", "Fit Frame to Content", Some("Cmd+Alt+C"), "{} — object.fit {mode: fitFrameToContent}"),
    ("fit.contentToFrame", "Fit Content to Frame", Some("Cmd+Alt+E"), "{} — object.fit {mode: fitContentToFrame}"),
    ("fit.centerContent", "Center Content", Some("Cmd+Shift+E"), "{} — object.fit {mode: centerContent}"),
    ("window.split", "Split Window", None, "{on?: bool} — two views of the document side by side, each with its own zoom and scroll"),
    ("window.newWindow", "New Window", None, "{on?: bool} — another view of the active document in its own window"),
    ("window.minimize", "Minimize Window", None, "{} — minimize the desktop window"),
    ("window.maximize", "Maximize/Restore Window", None, "{on?: bool} — toggle maximization when omitted"),
    ("window.close", "Close Window", None, "{} — request that the desktop window close"),
    ("window.drag", "Move Window", None, "{} — begin a native window drag while the primary mouse button is held"),
    ("window.resize", "Resize Window", None, "{edge: north|south|east|west|northEast|southEast|northWest|southWest} — begin a native edge drag"),
    ("window.taskBar", "Contextual Task Bar", None, "{}"),
    (
        "window.taskBarPin",
        "Pin Bar Position",
        None,
        "{on?: bool, at?: [x, y] (points from the canvas's top-left)} — the Contextual Task Bar stays where it is (or at `at`) instead of following the selection; with neither, toggles",
    ),
    ("window.taskBarReset", "Reset Bar Position", None, "{} — the Contextual Task Bar follows the selection again, under it"),
    ("help.discord", "Join the ArtCraft Discord…", None, "{} — opens https://discord.gg/artcraft in the browser"),
    ("help.appPage", "DesignCraft Website…", None, "{} — opens https://getartcraft.com/apps/designcraft"),
    ("help.github", "DesignCraft on GitHub…", None, "{} — opens https://github.com/storytold/designcraft"),
    ("help.issues", "Report an Issue…", None, "{} — opens the GitHub issue tracker"),
    ("help.website", "ArtCraft Website…", None, "{} — opens https://getartcraft.com"),
    ("help.app", "ArtCraft App Page…", None, "{app} — opens https://getartcraft.com/apps/{app} (e.g. photocraft)"),
    (
        "help.about",
        "About DesignCraft",
        None,
        "{open?: true, tab?: \"about\"|\"contributors\"|\"models\"} — the About window: splash with community links, contributor and model credits (open: false closes it)",
    ),
    ("window.toolsDoubleColumn", "Tools: Double Column", None, "{}"),
    (
        "window.workspace",
        "Workspace",
        None,
        "{name: Advanced|Book|Digital Publishing|Essentials|Essentials Classic|Interactive for PDF|Printing and Proofing|Typography or a saved workspace}",
    ),
    (
        "window.newWorkspace",
        "New Workspace…",
        None,
        "{name, panelLocations?: bool (default true: the bars and panels), menuCustomization?: bool (default true: the menu items hidden with Edit › Menus)} — saves the workspace",
    ),
    ("window.deleteWorkspace", "Delete Workspace…", None, "{name}"),
    ("window.resetWorkspace", "Reset Workspace", None, "{} — back to the current workspace as saved (or its defaults)"),
    ("window.brightness", "Interface Color Theme", None, "{brightness: dark|mediumDark|mediumLight|light|highContrast}"),
];

/// Menu bar: (menu, entries). Entries: `cmd:<id>`, `ui:<id>`, `-` separator, `>Submenu` … `<`.
pub const MENUS: &[(&str, &[&str])] = &[
    (
        "File",
        &[
            "ui:app.newDocumentDialog",
            "ui:app.openDialog",
            ">Open Recent",
            "cmd:file.clearRecent",
            "<",
            "-",
            "cmd:file.close",
            "ui:app.save",
            "ui:app.saveDialog",
            "ui:app.saveCopyDialog",
            "cmd:file.revert",
            "-",
            "ui:app.placeDialog",
            "ui:app.placeWithOptions",
            "-",
            "ui:app.exportPdf",
            "ui:app.packageDialog",
            "ui:app.printBooklet",
            "ui:app.exportPng",
            "ui:app.exportIdml",
            "ui:app.exportEpub",
            "ui:app.exportFixedEpub",
            "ui:app.exportInteractivePdf",
            "ui:app.exportHtml",
            "ui:app.exportPptx",
            "ui:app.exportXml",
            "ui:app.exportText",
            ">Export",
            "cmd:snippet.export",
            "<",
            "-",
            "cmd:layout.documentSetup",
        ],
    ),
    (
        "Edit",
        &[
            "cmd:edit.undo",
            "cmd:edit.redo",
            "-",
            "cmd:edit.cut",
            "cmd:edit.copy",
            "cmd:edit.paste",
            "cmd:edit.pasteWithoutFormatting",
            "cmd:edit.pasteInto",
            "cmd:edit.pasteInPlace",
            "cmd:edit.clear",
            "-",
            "cmd:edit.duplicate",
            "ui:app.placeAndLink",
            "cmd:edit.stepAndRepeat",
            "-",
            "cmd:edit.selectAll",
            "cmd:edit.deselectAll",
            "-",
            "ui:app.findChange",
            "cmd:find.next",
            "ui:app.storyEditor",
            "-",
            ">Spelling",
            "cmd:spelling.check",
            "ui:edit.dynamicSpelling",
            "cmd:spelling.addWord",
            "ui:app.userDictionary",
            "<",
            ">Transparency Blend Space",
            "cmd:edit.transparencyBlendSpace|Document RGB|{\"space\": \"rgb\"}",
            "cmd:edit.transparencyBlendSpace|Document CMYK|{\"space\": \"cmyk\"}",
            "<",
            "ui:app.colorSettings",
            ">Transparency Flattener Presets",
            "ui:app.flattener|None (keep transparency)|{\"preset\": \"\"}",
            "ui:app.flattener|High Resolution|{\"preset\": \"high\"}",
            "ui:app.flattener|Medium Resolution|{\"preset\": \"medium\"}",
            "ui:app.flattener|Low Resolution|{\"preset\": \"low\"}",
            "<",
            "-",
            "ui:app.palette",
            "ui:app.keyboardShortcuts",
            "ui:app.menus",
            "ui:app.preferences",
        ],
    ),
    (
        "Layout",
        &[
            ">Pages",
            "cmd:layout.pages.insert",
            "cmd:layout.pages.move",
            "ui:app.duplicateSpread",
            "ui:app.deletePage",
            "-",
            "cmd:layout.pages.applyParent",
            "cmd:layout.parents.new",
            "<",
            "cmd:layout.marginsAndColumns",
            "cmd:layout.createGuides",
            "cmd:layout.createAlternate",
            "-",
            "ui:view.firstPage",
            "ui:view.previousPage",
            "ui:view.nextPage",
            "ui:view.lastPage",
            "ui:view.nextSpread",
            "ui:view.previousSpread",
            "ui:view.goToPage",
            "-",
            "cmd:layout.section",
            "cmd:toc.generate",
            "cmd:toc.update",
            ">Index",
            "cmd:index.addReference",
            "cmd:index.generate",
            "cmd:index.update",
            "<",
        ],
    ),
    (
        "Type",
        &[
            ">Size",
            "cmd:type.char|6 pt|{\"attrs\": {\"size\": 6}}",
            "cmd:type.char|7 pt|{\"attrs\": {\"size\": 7}}",
            "cmd:type.char|8 pt|{\"attrs\": {\"size\": 8}}",
            "cmd:type.char|9 pt|{\"attrs\": {\"size\": 9}}",
            "cmd:type.char|10 pt|{\"attrs\": {\"size\": 10}}",
            "cmd:type.char|11 pt|{\"attrs\": {\"size\": 11}}",
            "cmd:type.char|12 pt|{\"attrs\": {\"size\": 12}}",
            "cmd:type.char|14 pt|{\"attrs\": {\"size\": 14}}",
            "cmd:type.char|18 pt|{\"attrs\": {\"size\": 18}}",
            "cmd:type.char|24 pt|{\"attrs\": {\"size\": 24}}",
            "cmd:type.char|30 pt|{\"attrs\": {\"size\": 30}}",
            "cmd:type.char|36 pt|{\"attrs\": {\"size\": 36}}",
            "cmd:type.char|48 pt|{\"attrs\": {\"size\": 48}}",
            "cmd:type.char|60 pt|{\"attrs\": {\"size\": 60}}",
            "cmd:type.char|72 pt|{\"attrs\": {\"size\": 72}}",
            "<",
            "-",
            "ui:app.characterPanel",
            "ui:app.paragraphPanel",
            "ui:app.tabsPanel",
            "ui:app.glyphsPanel",
            "-",
            "ui:app.characterStylesPanel",
            "ui:app.paragraphStylesPanel",
            "-",
            "cmd:type.createOutlines",
            "ui:app.findFontDialog",
            ">Change Case",
            "cmd:type.changeCase|UPPERCASE|{\"case\": \"upper\"}",
            "cmd:type.changeCase|lowercase|{\"case\": \"lower\"}",
            "cmd:type.changeCase|Title Case|{\"case\": \"title\"}",
            "cmd:type.changeCase|Sentence case|{\"case\": \"sentence\"}",
            "<",
            "-",
            ">Type on a Path",
            "cmd:type.pathOptions",
            "cmd:type.pathOptions|Delete Type from Path|{\"delete\": true}",
            "<",
            "-",
            ">Notes",
            "cmd:note.new",
            "ui:window.panel|Notes|{\"panel\": \"notes\"}",
            "<",
            ">Track Changes",
            "cmd:changes.track",
            "cmd:changes.acceptAll",
            "cmd:changes.rejectAll",
            "<",
            "-",
            "cmd:footnote.insert",
            "ui:app.footnoteOptionsDialog",
            "cmd:endnote.insert",
            "cmd:footnote.goToReference",
            "-",
            ">Hyperlinks & Cross-References",
            "cmd:hyperlink.create",
            "cmd:anchor.create",
            "-",
            "ui:app.insertXrefDialog",
            "cmd:xref.defineFormat",
            "<",
            ">Text Variables",
            "cmd:variables.define",
            "cmd:variables.insert",
            "<",
            "-",
            ">Bulleted & Numbered Lists",
            "cmd:list.define",
            "<",
            "-",
            ">Insert Special Character",
            ">Symbols",
            "cmd:text.insert|Bullet Character|{\"text\": \"\u{2022}\", \"raw\": true}",
            "cmd:text.insert|Copyright Symbol|{\"text\": \"\u{a9}\", \"raw\": true}",
            "cmd:text.insert|Ellipsis|{\"text\": \"\u{2026}\", \"raw\": true}",
            "cmd:text.insert|Paragraph Symbol|{\"text\": \"\u{b6}\", \"raw\": true}",
            "cmd:text.insert|Registered Trademark Symbol|{\"text\": \"\u{ae}\", \"raw\": true}",
            "cmd:text.insert|Section Symbol|{\"text\": \"\u{a7}\", \"raw\": true}",
            "cmd:text.insert|Trademark Symbol|{\"text\": \"\u{2122}\", \"raw\": true}",
            "<",
            ">Markers",
            "ui:app.insertCurrentPageNumber",
            "cmd:text.insert|Next Page Number|{\"text\": \"\u{e007}\", \"raw\": true}",
            "cmd:text.insert|Previous Page Number|{\"text\": \"\u{e008}\", \"raw\": true}",
            "cmd:text.insert|Section Marker|{\"text\": \"\u{e001}\", \"raw\": true}",
            "<",
            ">Hyphens and Dashes",
            "cmd:text.insert|Em Dash|{\"text\": \"\u{2014}\", \"raw\": true}",
            "cmd:text.insert|En Dash|{\"text\": \"\u{2013}\", \"raw\": true}",
            "cmd:text.insert|Discretionary Hyphen|{\"text\": \"\u{ad}\", \"raw\": true}",
            "cmd:text.insert|Nonbreaking Hyphen|{\"text\": \"\u{2011}\", \"raw\": true}",
            "<",
            ">Quotation Marks",
            "cmd:text.insert|Double Left Quotation Marks|{\"text\": \"\u{201c}\", \"raw\": true}",
            "cmd:text.insert|Double Right Quotation Marks|{\"text\": \"\u{201d}\", \"raw\": true}",
            "cmd:text.insert|Single Left Quotation Mark|{\"text\": \"\u{2018}\", \"raw\": true}",
            "cmd:text.insert|Single Right Quotation Mark|{\"text\": \"\u{2019}\", \"raw\": true}",
            "cmd:text.insert|Straight Double Quotation Marks|{\"text\": \"\\\"\", \"raw\": true}",
            "cmd:text.insert|Straight Single Quotation Mark (Apostrophe)|{\"text\": \"'\", \"raw\": true}",
            "<",
            ">Other",
            "cmd:text.insert|Tab|{\"text\": \"\\t\", \"raw\": true}",
            "cmd:text.insert|Right Indent Tab|{\"text\": \"\u{e006}\", \"raw\": true}",
            "cmd:text.insert|Indent to Here|{\"text\": \"\u{e005}\", \"raw\": true}",
            "<",
            "<",
            ">Insert White Space",
            "cmd:text.insert|Em Space|{\"text\": \"\u{2003}\", \"raw\": true}",
            "cmd:text.insert|En Space|{\"text\": \"\u{2002}\", \"raw\": true}",
            "cmd:text.insert|Nonbreaking Space|{\"text\": \"\u{a0}\", \"raw\": true}",
            "cmd:text.insert|Hair Space|{\"text\": \"\u{200a}\", \"raw\": true}",
            "cmd:text.insert|Sixth Space|{\"text\": \"\u{2006}\", \"raw\": true}",
            "cmd:text.insert|Thin Space|{\"text\": \"\u{2009}\", \"raw\": true}",
            "cmd:text.insert|Quarter Space|{\"text\": \"\u{2005}\", \"raw\": true}",
            "cmd:text.insert|Third Space|{\"text\": \"\u{2004}\", \"raw\": true}",
            "cmd:text.insert|Punctuation Space|{\"text\": \"\u{2008}\", \"raw\": true}",
            "cmd:text.insert|Figure Space|{\"text\": \"\u{2007}\", \"raw\": true}",
            "cmd:text.insert|Flush Space|{\"text\": \"\u{2001}\", \"raw\": true}",
            "<",
            ">Insert Break Character",
            "cmd:text.insert|Column Break|{\"text\": \"\u{e002}\", \"raw\": true}",
            "cmd:text.insert|Frame Break|{\"text\": \"\u{e003}\", \"raw\": true}",
            "cmd:text.insert|Page Break|{\"text\": \"\u{e004}\", \"raw\": true}",
            "cmd:text.insert|Odd Page Break|{\"text\": \"\u{e011}\", \"raw\": true}",
            "cmd:text.insert|Even Page Break|{\"text\": \"\u{e012}\", \"raw\": true}",
            "cmd:text.insert|Paragraph Return|{\"text\": \"\\n\", \"raw\": true}",
            "cmd:text.insert|Forced Line Break|{\"text\": \"\u{2028}\", \"raw\": true}",
            "<",
            "cmd:type.fillWithPlaceholder",
            "-",
            "ui:view.hiddenCharacters",
            "-",
            "cmd:math.insert",
            ">Story Direction",
            "cmd:type.storyDirection|Horizontal|{\"vertical\": false}",
            "cmd:type.storyDirection|Vertical|{\"vertical\": true}",
            "<",
            "cmd:type.tateChuYoko",
            "ui:app.rubyDialog",
            "cmd:type.kenten",
            "cmd:type.warichu",
        ],
    ),
    (
        "Object",
        &[
            ">Transform",
            "cmd:transform.move",
            "cmd:transform.scale",
            "cmd:transform.rotate",
            "cmd:transform.shear",
            "-",
            "cmd:transform.flip",
            "-",
            "cmd:transform.clear",
            "<",
            ">Transform Again",
            "cmd:transform.again",
            "cmd:transform.again|Transform Again Individually|{\"individually\": true}",
            "cmd:transform.again|Transform Sequence Again|{\"sequence\": true}",
            "cmd:transform.again|Transform Sequence Again Individually|{\"sequence\": true, \"individually\": true}",
            "<",
            ">Flex Layout",
            "cmd:flex.add",
            "cmd:flex.convert",
            "cmd:flex.remove",
            "-",
            "ui:window.panel|Show Flex Controls|{\"panel\": \"flexLayout\"}",
            "<",
            ">Paths",
            "cmd:object.makeCompoundPath",
            "cmd:object.releaseCompoundPath",
            "-",
            "cmd:path.reverse",
            "<",
            ">Pathfinder",
            "cmd:object.pathfinder|Add|{\"op\": \"add\"}",
            "cmd:object.pathfinder|Subtract|{\"op\": \"subtract\"}",
            "cmd:object.pathfinder|Intersect|{\"op\": \"intersect\"}",
            "cmd:object.pathfinder|Exclude Overlap|{\"op\": \"exclude\"}",
            "cmd:object.pathfinder|Minus Back|{\"op\": \"minusBack\"}",
            "<",
            ">Arrange",
            "cmd:object.bringToFront",
            "cmd:object.bringForward",
            "cmd:object.sendBackward",
            "cmd:object.sendToBack",
            "<",
            "-",
            "cmd:object.group",
            "cmd:object.ungroup",
            "cmd:object.lock",
            "cmd:object.unlockAll",
            "cmd:object.hide",
            "cmd:object.showAll",
            "-",
            "cmd:object.textFrameOptions",
            "ui:app.layerOptions",
            "cmd:object.exportOptions",
            "cmd:object.primaryTextFrame",
            "cmd:object.cornerOptions",
            "-",
            ">Effects",
            "ui:effects.dialog|Transparency…|{\"page\": \"transparency\"}",
            "cmd:object.dropShadow",
            "cmd:object.innerShadow",
            "cmd:object.outerGlow",
            "cmd:object.innerGlow",
            "cmd:object.bevel",
            "cmd:object.satin",
            "cmd:object.feather",
            "cmd:object.directionalFeather",
            "<",
            ">Select",
            "cmd:select.firstAbove",
            "cmd:select.nextAbove",
            "cmd:select.nextBelow",
            "cmd:select.lastBelow",
            "-",
            "cmd:select.container",
            "cmd:select.content",
            "-",
            "cmd:select.previousInGroup",
            "cmd:select.nextInGroup",
            "<",
            ">Convert Shape",
            "cmd:object.convertShape|Rectangle|{\"to\": \"rectangle\"}",
            "cmd:object.convertShape|Rounded Rectangle|{\"to\": \"roundedRectangle\"}",
            "cmd:object.convertShape|Beveled Rectangle|{\"to\": \"beveledRectangle\"}",
            "cmd:object.convertShape|Inverse Rounded Rectangle|{\"to\": \"inverseRoundedRectangle\"}",
            "cmd:object.convertShape|Ellipse|{\"to\": \"ellipse\"}",
            "cmd:object.convertShape|Triangle|{\"to\": \"triangle\"}",
            "cmd:object.convertShape|Polygon|{\"to\": \"polygon\"}",
            "cmd:object.convertShape|Line|{\"to\": \"line\"}",
            "cmd:object.convertShape|Orthogonal Line|{\"to\": \"orthogonalLine\"}",
            "-",
            "cmd:object.convertShape|Open Path|{\"to\": \"openPath\"}",
            "cmd:object.convertShape|Closed Path|{\"to\": \"closedPath\"}",
            "<",
            "ui:app.qrCode",
            ">Clipping Path",
            "cmd:object.clippingPathOptions|None|{\"type\": \"none\"}",
            "cmd:object.clippingPath|Alpha Channel|{\"type\": \"alpha\"}",
            "cmd:object.clippingPath|Detect Edges|{\"type\": \"edges\"}",
            "<",
            ">Captions",
            "cmd:object.caption",
            "cmd:object.caption|Generate Live Caption|{\"live\": true}",
            "<",
            ">Anchored Object",
            "cmd:anchored.options",
            "cmd:anchored.release",
            "<",
            ">Fitting",
            "ui:fit.fillFrameProportionally",
            "ui:fit.contentProportionally",
            "ui:fit.frameToContent",
            "ui:fit.contentToFrame",
            "ui:fit.centerContent",
            "-",
            "ui:app.fittingOptions",
            "<",
            ">Content",
            "cmd:object.content|Graphic|{\"type\":\"graphic\"}",
            "cmd:object.content|Text|{\"type\":\"text\"}",
            "cmd:object.content|Unassigned|{\"type\":\"unassigned\"}",
            "<",
        ],
    ),
    (
        "Table",
        &[
            "ui:app.insertTableDialog",
            "cmd:table.convertFromText",
            "cmd:table.convertToText",
            "-",
            ">Insert",
            "cmd:table.insertRowAbove",
            "cmd:table.insertRowBelow",
            "cmd:table.insertColumnLeft",
            "cmd:table.insertColumnRight",
            "<",
            ">Delete",
            "cmd:table.deleteRow",
            "cmd:table.deleteColumn",
            "cmd:table.delete",
            "<",
            ">Select",
            "cmd:table.selectRow",
            "cmd:table.selectColumn",
            "cmd:table.selectTable",
            "<",
            "-",
            ">Cell Options",
            "ui:app.cellOptionsDialog",
            "<",
            "cmd:table.merge",
            "cmd:table.unmerge",
            ">Convert Cell Type",
            "ui:app.graphicCell|Convert Cell to Graphic Cell…",
            "cmd:table.textCell",
            "<",
            ">Convert Rows",
            "cmd:table.convertRowsToHeader",
            "cmd:table.convertRowsToBody",
            "cmd:table.convertRowsToFooter",
            "<",
            "cmd:table.splitHorizontally",
            "cmd:table.splitVertically",
            "cmd:table.distributeColumns",
            "cmd:table.distributeRows",
            "-",
            "ui:app.tablePanel",
        ],
    ),
    (
        "View",
        &[
            "ui:view.overprintPreview",
            ">Proof Setup",
            "ui:view.proofSetup|Working CMYK|{\"target\": \"workingCmyk\"}",
            "ui:view.proofSetup|Internet Standard RGB (sRGB)|{\"target\": \"srgb\"}",
            "ui:view.proofSetup|Legacy Macintosh RGB|{\"target\": \"legacyMacRgb\"}",
            "ui:view.proofSetup|Monitor RGB|{\"target\": \"monitorRgb\"}",
            "ui:view.proofSetup|Color Blindness – Protanopia|{\"target\": \"protanopia\"}",
            "ui:view.proofSetup|Color Blindness – Deuteranopia|{\"target\": \"deuteranopia\"}",
            "-",
            "ui:view.proofSetup|Simulate Paper Color|{\"simulatePaper\": \"toggle\"}",
            "<",
            "ui:view.proofColors",
            ">Separations Preview",
            "ui:view.separations|Off|{\"plate\": null, \"inkLimit\": null}",
            "ui:view.separations|Cyan|{\"plate\": \"cyan\"}",
            "ui:view.separations|Magenta|{\"plate\": \"magenta\"}",
            "ui:view.separations|Yellow|{\"plate\": \"yellow\"}",
            "ui:view.separations|Black|{\"plate\": \"black\"}",
            "ui:view.separations|Ink Limit 300%|{\"inkLimit\": 300}",
            "<",
            "ui:view.flattenerPreview",
            "-",
            "ui:view.zoomIn",
            "ui:view.zoomOut",
            "ui:view.fitPage",
            "ui:view.fitSelection",
            "ui:view.fitSpread",
            "ui:view.actualSize",
            "ui:view.entirePasteboard",
            "-",
            ">Rotate Spread",
            "ui:view.rotateSpread|90° CW|{\"angle\": 90}",
            "ui:view.rotateSpread|90° CCW|{\"angle\": -90}",
            "ui:view.rotateSpread|180°|{\"angle\": 180}",
            "ui:view.rotateSpread|Clear Rotation|{\"angle\": 0}",
            "<",
            "-",
            ">Screen Mode",
            "ui:view.screenMode|Normal|{\"mode\": \"normal\"}",
            "ui:view.screenMode|Preview|{\"mode\": \"preview\"}",
            "ui:view.screenMode|Bleed|{\"mode\": \"bleed\"}",
            "ui:view.screenMode|Slug|{\"mode\": \"slug\"}",
            "ui:view.screenMode|Presentation|{\"mode\": \"presentation\"}",
            "<",
            ">Display Performance",
            "ui:view.fastDisplay",
            "ui:view.typicalDisplay",
            "ui:view.highQualityDisplay",
            "<",
            "-",
            "ui:view.rulers",
            "-",
            ">Extras",
            "ui:view.frameEdges",
            "ui:view.textThreads",
            "<",
            ">Grids & Guides",
            "ui:view.guides",
            "ui:view.snapToGuides",
            "ui:view.snapToDocumentGrid",
            "ui:view.smartGuides",
            "ui:view.baselineGrid",
            "ui:view.documentGrid",
            "-",
            "ui:app.deleteAllGuides",
            "<",
            "-",
            ">Structure",
            "ui:view.taggedFrames",
            "ui:view.tagMarkers",
            "<",
        ],
    ),
    (
        "Window",
        // InDesign's order: Arrange, Workspace, then the panels alphabetically with their groups.
        &[
            ">Arrange",
            "ui:window.newWindow",
            "ui:window.split",
            "-",
            "ui:window.nextDocument",
            "ui:window.previousDocument",
            "<",
            "ui:window.workspace",
            "-",
            "ui:window.panel|Articles|{\"panel\": \"articles\"}",
            ">Color",
            "ui:window.panel|Color|{\"panel\": \"color\"}",
            "ui:window.panel|Gradient|{\"panel\": \"gradient\"}",
            "ui:window.panel|Swatches|{\"panel\": \"swatches\"}",
            "<",
            "ui:window.taskBar",
            "ui:window.controlBar",
            ">Editorial",
            "ui:window.panel|Notes|{\"panel\": \"notes\"}",
            "ui:window.panel|Track Changes|{\"panel\": \"trackChanges\"}",
            "<",
            "ui:window.panel|Effects|{\"panel\": \"effects\"}",
            "ui:window.panel|Info|{\"panel\": \"info\"}",
            ">Interactive",
            "ui:window.panel|Bookmarks|{\"panel\": \"bookmarks\"}",
            "ui:window.panel|Buttons and Forms|{\"panel\": \"buttons\"}",
            "ui:window.panel|Hyperlinks|{\"panel\": \"hyperlinks\"}",
            "ui:window.panel|Liquid Layout|{\"panel\": \"liquid\"}",
            "ui:window.panel|Media|{\"panel\": \"media\"}",
            "ui:window.panel|Object States|{\"panel\": \"states\"}",
            "ui:window.panel|Page Transitions|{\"panel\": \"transitions\"}",
            "<",
            "ui:window.panel|Layers|{\"panel\": \"layers\"}",
            "ui:window.panel|Links|{\"panel\": \"links\"}",
            ">Object & Layout",
            "ui:window.panel|Align|{\"panel\": \"align\"}",
            "ui:window.panel|Flex Layout|{\"panel\": \"flexLayout\"}",
            "ui:window.panel|Pathfinder|{\"panel\": \"pathfinder\"}",
            "<",
            ">Output",
            "ui:window.panel|Attributes|{\"panel\": \"attributes\"}",
            "ui:window.panel|Preflight|{\"panel\": \"preflight\"}",
            "<",
            "ui:window.panel|Pages|{\"panel\": \"pages\"}",
            "ui:window.panel|Properties|{\"panel\": \"properties\"}",
            "ui:window.panel|Stroke|{\"panel\": \"stroke\"}",
            ">Styles",
            "ui:window.panel|Character Styles|{\"panel\": \"characterStyles\"}",
            "ui:window.panel|Paragraph Styles|{\"panel\": \"paragraphStyles\"}",
            "<",
            "ui:window.panel|Text Wrap|{\"panel\": \"textWrap\"}",
            ">Type & Tables",
            "ui:window.panel|Character|{\"panel\": \"character\"}",
            "ui:window.panel|Conditional Text|{\"panel\": \"conditions\"}",
            "ui:window.panel|Glyphs|{\"panel\": \"glyphs\"}",
            "ui:window.panel|Paragraph|{\"panel\": \"paragraph\"}",
            "ui:window.panel|Table|{\"panel\": \"table\"}",
            "ui:app.tabsPanel",
            "ui:window.panel|Typography|{\"panel\": \"typography\"}",
            "<",
            ">Utilities",
            "ui:window.panel|Book|{\"panel\": \"book\"}",
            "ui:window.panel|Data Merge|{\"panel\": \"dataMerge\"}",
            "ui:window.panel|Library|{\"panel\": \"library\"}",
            "ui:window.panel|Scripts|{\"panel\": \"scripts\"}",
            "ui:window.panel|Tags|{\"panel\": \"tags\"}",
            "<",
            "-",
            "ui:window.hidePanels",
            "ui:window.hidePanelsExceptTools",
            "ui:window.toolsDoubleColumn",
            "-",
            "ui:window.brightness",
        ],
    ),
    (
        "Help",
        &[
            "ui:help.discord",
            "-",
            "ui:help.appPage",
            "ui:help.github",
            "ui:help.issues",
            "ui:help.website",
            "-",
            "cmd:file.newSample",
            "-",
            "ui:help.about",
        ],
    ),
];

/// The built-in workspaces, in InDesign's order.
pub const WORKSPACES: &[&str] =
    &["Advanced", "Book", "Digital Publishing", "Essentials", "Essentials Classic", "Interactive for PDF", "Printing and Proofing", "Typography"];

/// Essentials Classic's icon column: InDesign's iconic panel groups, top to bottom.
pub const CLASSIC_GROUPS: &[&[&str]] = &[
    &["pages", "layers", "links"],
    &["stroke", "color"],
    &["swatches"],
    &["effects"],
    &["paragraph"],
    &["textWrap"],
    &["characterStyles", "paragraphStyles"],
    &["gradient"],
];

/// The Typography preset's available panels in the order observed in InDesign's workspace.
/// CC Libraries and the separate Story panel have no corresponding DesignCraft panel.
pub const TYPOGRAPHY_GROUPS: &[&[&str]] = &[
    &["pages", "layers", "swatches"],
    &["glyphs", "textWrap"],
    &["stroke", "gradient"],
    &["hyperlinks", "effects"],
    &["paragraph", "paragraphStyles"],
    &["character", "characterStyles"],
];

pub fn ui_label(id: &str) -> Option<(&'static str, Option<&'static str>)> {
    UI_COMMANDS.iter().find(|c| c.0 == id).map(|c| (c.1, c.2))
}

/// Type › Tabs: float the Tabs panel (placed above the selected text frame) or close it.
fn toggle_tabs_panel(app: &mut DesignApp) {
    if app.ui.floating.iter().any(|(p, _)| p == "tabs") {
        crate::dock::dock_panel(app, "tabs");
    } else {
        let at = app.canvas_rect.map_or(egui::pos2(300.0, 140.0), |r| r.min + egui::vec2(80.0, 60.0));
        crate::dock::float_panel(app, "tabs", at);
        app.ui.tabs_panel.snap_request = 2;
    }
}

/// Run a UI command; `None` if `id` isn't one.
pub fn run_ui(app: &mut DesignApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    let was_find = app.ui.dialog.as_ref().is_some_and(|d| d.id == "findChange");
    let result = run_ui_inner(app, id, p);
    if was_find && app.ui.dialog.as_ref().is_none_or(|d| d.id != "findChange") {
        app.session.reset_find_scope();
    }
    result
}

fn run_ui_inner(app: &mut DesignApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    if is_window_control(id) && !window_control_available(app, id) {
        return Some(Err("custom window controls are not available in this host".into()));
    }
    if let Some(result) = crate::panel_docking::command(app, id, p) {
        return Some(result);
    }
    let rect = app.canvas_rect;
    let flag = |b: &mut bool| {
        *b = !*b;
        Ok(json!(*b))
    };
    Some(match id {
        "window.minimize" => {
            app.pending_window_commands.push(egui::ViewportCommand::Minimized(true));
            Ok(Value::Null)
        }
        "window.maximize" => {
            let on = p.get("on").and_then(Value::as_bool).unwrap_or(!app.window_maximized);
            app.pending_window_commands.push(egui::ViewportCommand::Maximized(on));
            Ok(json!(on))
        }
        "window.close" => {
            app.pending_window_commands.push(egui::ViewportCommand::Close);
            Ok(Value::Null)
        }
        "window.drag" => {
            app.pending_window_commands.push(egui::ViewportCommand::StartDrag);
            Ok(Value::Null)
        }
        "window.resize" => {
            let edge = match p.get("edge").and_then(Value::as_str) {
                Some("north") => egui::ResizeDirection::North,
                Some("south") => egui::ResizeDirection::South,
                Some("east") => egui::ResizeDirection::East,
                Some("west") => egui::ResizeDirection::West,
                Some("northEast") => egui::ResizeDirection::NorthEast,
                Some("southEast") => egui::ResizeDirection::SouthEast,
                Some("northWest") => egui::ResizeDirection::NorthWest,
                Some("southWest") => egui::ResizeDirection::SouthWest,
                _ => return Some(Err("invalid window resize edge".into())),
            };
            app.pending_window_commands.push(egui::ViewportCommand::BeginResize(edge));
            Ok(Value::Null)
        }
        "app.newDocumentDialog" => {
            app.ui.dialog = Some(crate::dialogs::Dialog::new("newDocument", json!({})));
            Ok(Value::Null)
        }
        "app.openDialog" => app.pick_and_open("open"),
        "app.openRecent" => {
            let index = p.get("index").cloned().unwrap_or(Value::Null);
            let name = index
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .and_then(|i| app.session.prefs.recent_files.get(i))
                .map(|path| recent_label(path))
                .unwrap_or_default();
            app.open_named_file("file.openRecent", json!({"index": index}), name)
        }
        "app.placeDialog" => app.pick_and_open("place"),
        "app.placeWithOptions" => {
            // Word/RTF Import Options: style mapping and conflicts before the text lands.
            let Some(path) = app.services.pick_open.as_mut().and_then(|f| f("place")) else { return Some(Ok(Value::Null)) };
            let l = path.to_lowercase();
            if !(l.ends_with(".docx") || l.ends_with(".rtf")) {
                return Some(app.place_file(json!({"path": path})));
            }
            let info = match app.run("place.styles", json!({"path": path})) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            app.ui.dialog = Some(crate::dialogs::Dialog::new(
                "importOptions",
                json!({"path": path, "styles": info["paragraph"], "charStyles": info["character"], "conflicts": info["conflicts"], "styleConflicts": "useExisting", "removeStyles": false, "map": {}}),
            ));
            Ok(Value::Null)
        }
        "app.save" | "app.saveDialog" if app.services.download.is_some() => download_document(app),
        "app.save" => {
            if app.session.active().is_some_and(|d| d.path.is_some()) {
                return Some(app.run("file.save", json!({})));
            }
            return run_ui(app, "app.saveDialog", p);
        }
        "app.saveDialog" => {
            let name = app.session.active().map(|d| format!("{}.designcraft", d.doc.title)).unwrap_or_default();
            if let Some(path) = app.services.pick_save.as_mut().and_then(|f| f(&name)) {
                return Some(app.run("file.saveAs", json!({"path": path})));
            }
            Ok(Value::Null)
        }
        "app.quit" => {
            app.request_quit();
            Ok(Value::Null)
        }
        "app.saveCopyDialog" => {
            let name = app.session.active().map(|d| format!("{} copy.designcraft", d.doc.title)).unwrap_or_default();
            if let Some(path) = app.services.pick_save.as_mut().and_then(|f| f(&name)) {
                return Some(app.run("file.saveACopy", json!({"path": path})));
            }
            Ok(Value::Null)
        }
        "app.exportPng" => export_png(app, p),
        "app.exportIdml" => export_idml(app, p),
        "app.exportEpub" => {
            let mut q = if p.is_object() { p.clone() } else { json!({}) };
            q["cover"] = json!(true);
            export_bytes(app, &q, "epub", "file.exportEpub")
        }
        "app.exportInteractivePdf" => {
            let mut q = if p.is_object() { p.clone() } else { json!({}) };
            for (k, v) in [("pageLayout", json!("single")), ("view", json!("fitPage")), ("bookmarksPanel", json!(true)), ("media", json!(true))] {
                q.as_object_mut().map(|o| o.entry(k).or_insert(v));
            }
            export_pdf(app, &q)
        }
        "app.exportFixedEpub" => export_bytes(app, p, "epub", "file.exportFixedEpub"),
        "app.exportHtml" => export_bytes(app, p, "html", "file.exportHtml"),
        "app.exportPptx" => export_bytes(app, p, "pptx", "file.exportPptx"),
        "app.exportXml" => export_bytes(app, p, "xml", "file.exportXml"),
        "app.printBooklet" => export_bytes(app, p, "pdf", "file.printBooklet"),
        "app.menus" => {
            app.ui.dialog = Some(crate::dialogs::Dialog::new("menus", json!({"query": ""})));
            Ok(Value::Null)
        }
        "app.cellOptionsDialog" => crate::dialogs::open_cell_options(app),
        "window.hideMenuItem" => {
            let Some(item) = p.get("item").and_then(Value::as_str) else { return Some(Err("missing item".into())) };
            let hide = p.get("hidden").and_then(Value::as_bool).unwrap_or(true);
            app.ui.hidden_menu_items.retain(|x| x != item);
            if hide {
                app.ui.hidden_menu_items.push(item.to_string());
            }
            app.ui.show_full_menus = false;
            Ok(Value::Null)
        }
        "app.placeAndLink" => {
            let target = app.session.active().and_then(|d| {
                let id = *d.selection.items.first()?;
                let it = d.doc.item(id)?;
                let loc = d.doc.find(id)?;
                Some((it.text_frame()?.story, it.bounds(), loc.spread))
            });
            let Some((sid, b, sr)) = target else { return Some(Err("select a text frame".into())) };
            Ok(app
                .run("story.placeAndLink", json!({"story": sid.0, "rect": [b.x0 + 24.0, b.y0 + 24.0, b.x1 + 24.0, b.y1 + 24.0], "spread": sr}))
                .unwrap_or(Value::Null))
        }
        "app.fittingOptions" => {
            let g = app.session.active().and_then(|d| d.selection.items.first().and_then(|id| d.doc.item(*id)).and_then(|it| it.graphic().cloned()));
            let Some(g) = g else { return Some(Err("select a frame with a placed graphic".into())) };
            let fitting = serde_json::to_value(g.auto_fit).unwrap_or(json!("none"));
            let c = |v: f64| json!(designcraft_geom::format_measure(v, designcraft_geom::Unit::Points));
            app.ui.dialog = Some(crate::dialogs::Dialog::new(
                "fittingOptions",
                json!({"autoFit": g.auto_fit != designcraft_doc::Fitting::None, "fitting": fitting, "align": g.fit_align,
                    "cropTop": c(g.crop[0]), "cropLeft": c(g.crop[1]), "cropBottom": c(g.crop[2]), "cropRight": c(g.crop[3])}),
            ));
            Ok(Value::Null)
        }
        "app.userDictionary" => {
            app.ui.dialog = Some(crate::dialogs::Dialog::new("userDictionary", json!({"word": ""})));
            Ok(Value::Null)
        }
        "app.qrCode" => {
            app.ui.dialog = Some(crate::dialogs::Dialog::new("qrCode", json!({})));
            Ok(Value::Null)
        }
        "app.exportText" => export_bytes(app, p, "rtf", "file.exportText"),
        "app.packageDialog" => {
            let name = app.session.active().map(|d| format!("{} Folder", d.doc.title)).unwrap_or_default();
            match app.services.pick_save.as_mut().and_then(|f| f(&name)) {
                Some(dir) => {
                    let r = app.run("file.package", json!({"dir": dir}));
                    if let Ok(v) = &r {
                        app.status(
                            crate::i18n::tr(&app.ui.language, "Packaged {count} files into {dir}")
                                .replace("{count}", &v["files"].as_array().map_or(0, |f| f.len()).to_string())
                                .replace("{dir}", v["dir"].as_str().unwrap_or("")),
                        );
                    }
                    r
                }
                None => Ok(Value::Null),
            }
        }
        "app.language" => {
            let Some(v) = p.get("lang").and_then(Value::as_str) else { return Some(app.run("app.preferences", json!({}))) };
            if !crate::i18n::LANGUAGES.iter().any(|(c, _)| *c == v) {
                return Some(Err(format!("unknown language `{v}`")));
            }
            app.ui.language = v.to_string();
            Ok(json!(v))
        }
        "app.flattener" => {
            let v = p.get("preset").and_then(Value::as_str).unwrap_or("");
            if !["", "high", "medium", "low"].contains(&v) {
                return Some(Err(format!("unknown preset `{v}`")));
            }
            app.ui.flattener = v.to_string();
            Ok(json!(v))
        }
        "app.graphicCell" => {
            let Some(pick) = app.services.pick_open.as_mut() else { return Some(Err("needs a file picker".into())) };
            match pick("place") {
                Some(path) => app.run("table.placeGraphic", json!({"path": path})),
                None => Ok(Value::Null),
            }
        }
        "effects.dialog" => {
            crate::effects_dialog::open(app, p.get("page").and_then(Value::as_str).unwrap_or("transparency"));
            Ok(Value::Null)
        }
        "app.loadSwatches" => {
            if let Some(pick) = app.services.pick_open.as_mut() {
                return Some(match pick("swatches") {
                    Some(path) => app.run("swatch.load", json!({"path": path})),
                    None => Ok(Value::Null),
                });
            }
            let request = app.import_request("swatches");
            if let Some(open) = app.services.open_async.as_mut() {
                open(request);
            }
            Ok(Value::Null)
        }
        "app.loadParagraphStyles" => app.pick_styles("paragraph"),
        "app.loadCharacterStyles" => app.pick_styles("character"),
        "app.loadTextStyles" => app.pick_styles(""),
        "app.saveSwatches" => {
            let path = match p.get("path").and_then(Value::as_str) {
                Some(s) => Some(s.to_string()),
                None => app.services.pick_save.as_mut().and_then(|f| f("Swatches.ase")),
            };
            let Some(path) = path else { return Some(Ok(Value::Null)) };
            let r = match app.run("swatch.save", json!({})) {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            let bytes = designcraft_engine::cmd::base64_decode(r["base64"].as_str().unwrap_or_default());
            match app.services.write.as_mut() {
                Some(w) => w(&path, &bytes).map(|_| json!({"path": path, "bytes": bytes.len()})),
                None => Err("no writer".into()),
            }
        }
        "app.storyEditor" => {
            let target = app.session.active().and_then(|st| {
                let story = p.get("story").and_then(Value::as_u64).map(designcraft_doc::StoryId).or_else(|| {
                    st.selection
                        .text
                        .map(|t| t.story)
                        .or_else(|| st.selection.items.iter().find_map(|i| st.doc.item(*i)?.text_frame().map(|t| t.story)))
                })?;
                Some(crate::story_editor::Target { doc: st.uid, story })
            });
            match target {
                Some(t) => {
                    app.story_editor = if app.story_editor == Some(t) { None } else { Some(t) };
                    Ok(Value::Null)
                }
                None => Err("select a text frame or place the cursor in text".into()),
            }
        }
        "app.findChange" => {
            app.session.reset_find_scope();
            app.ui.dialog = Some(crate::dialogs::Dialog::new("findChange", json!({})));
            Ok(Value::Null)
        }
        "app.insertTableDialog" => {
            if app.session.active().is_none_or(|d| d.selection.text.is_none()) {
                return Some(Err("place the insertion point in a text frame to create a table".into()));
            }
            app.ui.dialog = Some(crate::dialogs::Dialog::new("insertTable", p.clone()));
            Ok(Value::Null)
        }
        "app.rubyDialog" => {
            let Some(st) = app.session.active() else { return Some(Err("no document open".into())) };
            let cur = st.selection.text.and_then(|t| {
                let story = st.doc.text_story(t.story, t.cell)?;
                let at = if t.range().is_empty() { t.range().start } else { t.range().start + 1 };
                story.char_format_at(at).over.ruby.clone()
            });
            let cur = cur.unwrap_or_default();
            // The ruby settings the panes show: the selection's resolved character attributes.
            let base = crate::panels::text_attrs(app).and_then(|a| a.get("chars").cloned()).unwrap_or(Value::Null);
            app.ui.dialog = Some(crate::dialogs::Dialog::new("ruby", json!({"text": cur, "base": base})));
            Ok(Value::Null)
        }
        "app.paragraphRulesDialog" => {
            let Some(a) = crate::panels::text_attrs(app) else { return Some(Err("select text or a text frame".into())) };
            let rule = if p.get("rule").and_then(Value::as_str) == Some("ruleBelow") { "ruleBelow" } else { "ruleAbove" };
            let current = json!({"ruleAbove": a["para"]["ruleAbove"], "ruleBelow": a["para"]["ruleBelow"]});
            app.ui.dialog = Some(crate::dialogs::Dialog::new("paragraphRules", json!({"rule": rule, "current": current})));
            Ok(Value::Null)
        }
        "app.paragraphBordersDialog" | "app.spanColumnsDialog" | "app.bulletsNumberingDialog" => {
            let Some(a) = crate::panels::text_attrs(app) else { return Some(Err("select text or a text frame".into())) };
            let section = match id {
                "app.spanColumnsDialog" => "span",
                "app.bulletsNumberingDialog" => "bullets",
                _ if p.get("tab").and_then(Value::as_str) == Some("shading") => "shading",
                _ => "border",
            };
            let fields = json!({
                "section": section,
                "current": a["para"],
                "family": a["chars"]["fontFamily"],
                "style": a["chars"]["fontStyle"],
            });
            app.ui.dialog = Some(crate::dialogs::Dialog::new("paragraphLocal", fields));
            Ok(Value::Null)
        }
        "app.justificationDialog" => {
            let Some(a) = app.session.execute("type.selectionAttrs", &json!({"mixed": true})).ok().filter(|a| !a.is_null()) else {
                return Some(Err("select text or a text frame".into()));
            };
            let mixed: Vec<&str> = a["paraMixed"].as_array().map(|m| m.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            let current: serde_json::Map<String, Value> = crate::dialogs::JUSTIFY_ATTRS
                .iter()
                .map(|k| (k.to_string(), if mixed.contains(k) { Value::Null } else { a["para"][*k].clone() }))
                .collect();
            app.ui.dialog = Some(crate::dialogs::Dialog::new("justification", json!({"current": current})));
            Ok(Value::Null)
        }
        "app.footnoteOptionsDialog" => {
            let Some(st) = app.session.active() else { return Some(Err("no document open".into())) };
            let o = serde_json::to_value(&st.doc.footnote_options).unwrap_or_default();
            app.ui.dialog = Some(crate::dialogs::Dialog::new("footnoteOptions", o));
            Ok(Value::Null)
        }
        "app.findFontDialog" => {
            if app.session.active().is_none() {
                return Some(Err("no document open".into()));
            }
            app.ui.dialog = Some(crate::dialogs::Dialog::new("findFont", json!({})));
            Ok(Value::Null)
        }
        "app.insertXrefDialog" => {
            if app.session.active().is_none_or(|d| d.selection.text.is_none()) {
                return Some(Err("place the insertion point in text to insert a cross-reference".into()));
            }
            app.ui.dialog = Some(crate::dialogs::Dialog::new("insertXref", json!({})));
            Ok(Value::Null)
        }
        "app.deleteAllGuides" => {
            let Some(st) = app.session.active() else { return Some(Err("no document open".into())) };
            let page = crate::canvas::current_page(app).unwrap_or(0);
            let spread = if st.editing_parents {
                // The parent spread the canvas shows.
                let layout = designcraft_tools::CanvasLayout::new(&st.doc, true);
                let shown = crate::canvas::current_slot(app, &layout).and_then(|i| layout.slots.get(i)).map(|s| s.spread);
                json!(shown.unwrap_or(designcraft_doc::SpreadRef::Parent(0)))
            } else {
                json!(st.doc.page_loc(page).map_or(0, |l| l.0))
            };
            return Some(app.run("guide.deleteAll", json!({"spread": spread})));
        }
        "app.deletePage" | "app.duplicateSpread" => {
            let Some(st) = app.session.active() else { return Some(Err("no document open".into())) };
            let page = crate::canvas::current_page(app).unwrap_or(0);
            let r = if id == "app.deletePage" {
                crate::dialogs::delete_pages(app, vec![page])
            } else {
                let spread = st.doc.page_loc(page).map_or(0, |l| l.0);
                app.run("layout.pages.duplicateSpread", json!({"spread": spread}))
            };
            return Some(r);
        }
        "app.tablePanel" => {
            app.ui.open_panel = if app.ui.open_panel.as_deref() == Some("table") { None } else { Some("table".into()) };
            Ok(Value::Null)
        }
        "app.tabsPanel" => {
            toggle_tabs_panel(app);
            Ok(Value::Null)
        }
        "app.exportPdf" => {
            if p.is_null() || p.as_object().is_none_or(|m| m.is_empty()) {
                crate::dialogs::open_pdf_export(app);
                Ok(Value::Null)
            } else {
                export_pdf(app, p)
            }
        }
        "app.palette" => {
            app.ui.palette = Some(String::new());
            Ok(Value::Null)
        }
        "app.preferences" => {
            let mut f = app.session.execute("prefs.set", &json!({})).unwrap_or_default();
            if app.session.active().is_some() {
                let doc = app.session.execute("document.preferences", &json!({})).unwrap_or_default();
                f["horizontalUnits"] = doc["horizontalUnits"].clone();
                f["rulerOrigin"] = doc["rulerOrigin"].clone();
                f["overprintBlack"] = doc["overprintBlack"].clone();
                f["glyphFallback"] = doc["glyphFallback"].clone();
                for k in ["superscriptSize", "superscriptPosition", "subscriptSize", "subscriptPosition"] {
                    f[format!("adv.{k}")] = json!(format!("{}", doc["advancedType"][k].as_f64().unwrap_or(0.0)));
                }
                f["verticalUnits"] = doc["verticalUnits"].clone();
                let words = app.session.execute("spelling.words", &json!({})).unwrap_or_default();
                f["userWords"] =
                    json!(words.as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n")).unwrap_or_default());
                let inc = doc["keyboardIncrement"].as_f64().unwrap_or(1.0);
                f["keyboardIncrement"] = json!(designcraft_geom::format_measure(inc, designcraft_geom::Unit::Points));
                f["kerningIncrement"] = json!(format!("{}", doc["kerningIncrement"].as_f64().unwrap_or(20.0)));
                let units = app.session.active().map(|d| d.doc.settings.horizontal_units).unwrap_or_default();
                let m = |v: &Value| json!(designcraft_geom::format_measure(v.as_f64().unwrap_or(0.0), units));
                let (bg, g) = (&doc["baselineGrid"], &doc["grid"]);
                f["bg.start"] = m(&bg["start"]);
                f["bg.increment"] = m(&bg["increment"]);
                f["bg.relativeTo"] = bg["relativeTo"].clone();
                f["bg.viewThreshold"] = json!((bg["viewThreshold"].as_f64().unwrap_or(0.75) * 100.0).round());
                f["bg.color"] = bg["color"].clone();
                f["grid.horizontal"] = m(&g["horizontal"]);
                f["grid.vertical"] = m(&g["vertical"]);
                f["grid.subdivisions"] = g["subdivisions"].clone();
                f["grid.inBack"] = g["inBack"].clone();
                f["grid.color"] = g["color"].clone();
                for k in ["marginColor", "columnColor", "bleedColor", "slugColor"] {
                    f[k] = doc[k].clone();
                }
                f["pasteboard.h"] = m(&doc["pasteboard"][0]);
                f["pasteboard.v"] = m(&doc["pasteboard"][1]);
            }
            f["snap.alignEdges"] = json!(app.ui.align_edges);
            f["snap.alignCenters"] = json!(app.ui.align_centers);
            f["snap.dimensions"] = json!(app.ui.smart_dimensions);
            f["snap.spacing"] = json!(app.ui.smart_spacing);
            f["snap.zone"] = json!(app.ui.snap_zone);
            f["displayQuality"] = json!(match app.ui.display_quality {
                designcraft_render::DisplayQuality::Fast => "fast",
                designcraft_render::DisplayQuality::Typical => "typical",
                designcraft_render::DisplayQuality::High => "high",
            });
            f["uiScale"] = json!((app.ui.ui_scale * 100.0).round());
            f["richBlack"] = json!(app.ui.rich_black);
            f["dynamicSpelling"] = json!(app.ui.dynamic_spelling);
            f["storyEditorSize"] = json!(app.ui.story_editor_size);
            let list = app.session.prefs.autocorrect_list.iter().map(|(a, b)| format!("{a} → {b}")).collect::<Vec<_>>().join("\n");
            f["autocorrectText"] = json!(list);
            // The effective setting; it becomes an explicit preference only when the user changes it,
            // so an untouched one keeps following the interface language.
            let cjk = crate::cjk_features(app);
            f["cjkFeatures"] = json!(cjk);
            f["cjkFeatures.initial"] = json!(cjk);
            f["section"] = json!("general");
            // The language previews as it changes; Cancel goes back to this one.
            f["openedLanguage"] = json!(app.ui.language);
            app.ui.dialog = Some(crate::dialogs::Dialog::new("preferences", f));
            Ok(Value::Null)
        }
        "view.zoomIn" | "view.zoomOut" => {
            if let Some(r) = rect {
                crate::canvas::zoom_at(app, r.center(), if id == "view.zoomIn" { 1.5 } else { 1.0 / 1.5 });
            }
            Ok(Value::Null)
        }
        "view.fitSelection" => {
            if let Some(r) = rect {
                crate::canvas::fit(app, r, "selection");
            }
            Ok(Value::Null)
        }
        "view.fitPage" | "view.fitSpread" | "view.entirePasteboard" => {
            if let Some(r) = rect {
                crate::canvas::fit(
                    app,
                    r,
                    if id == "view.fitPage" {
                        "page"
                    } else if id == "view.fitSpread" {
                        "spread"
                    } else {
                        "all"
                    },
                );
            }
            Ok(Value::Null)
        }
        "view.actualSize" => {
            crate::canvas::set_zoom(app, 1.0);
            Ok(Value::Null)
        }
        "view.zoom" => {
            crate::canvas::set_zoom(app, p.get("zoom").and_then(Value::as_f64).unwrap_or(1.0));
            Ok(Value::Null)
        }
        "view.screenMode" => {
            let mode = match p.get("mode").and_then(Value::as_str).unwrap_or("normal") {
                "preview" => ScreenMode::Preview,
                "bleed" => ScreenMode::Bleed,
                "slug" => ScreenMode::Slug,
                "presentation" => ScreenMode::Presentation,
                _ => ScreenMode::Normal,
            };
            set_screen_mode(app, mode);
            Ok(Value::Null)
        }
        "view.togglePreview" => {
            let mode = if app.ui.screen_mode == ScreenMode::Normal { ScreenMode::Preview } else { ScreenMode::Normal };
            set_screen_mode(app, mode);
            Ok(Value::Null)
        }
        "view.togglePresentation" => {
            let mode = match (app.ui.screen_mode, app.ui.presentation_return) {
                // A settings file can hold Presentation as the return mode; leaving must still leave.
                (ScreenMode::Presentation, ScreenMode::Presentation) => ScreenMode::Normal,
                (ScreenMode::Presentation, back) => back,
                _ => ScreenMode::Presentation,
            };
            set_screen_mode(app, mode);
            Ok(Value::Null)
        }
        "color.defaultFillStroke" => crate::toolbar::default_fill_stroke(app),
        "color.swapFillStroke" => crate::toolbar::swap_fill_stroke(app),
        "color.toggleFillStroke" => crate::toolbar::toggle_fill_stroke(app),
        "color.applyColor" => crate::toolbar::apply_color(app),
        "color.applyGradient" => crate::toolbar::apply_gradient(app),
        "color.applyNone" => crate::toolbar::apply_none(app),
        "view.frameEdges" => flag(&mut app.ui.frame_edges),
        "view.overprintPreview" => {
            app.canvas.shown = None;
            flag(&mut app.ui.overprint_preview)
        }
        "view.fastDisplay" | "view.typicalDisplay" | "view.highQualityDisplay" => {
            use designcraft_render::DisplayQuality as Q;
            app.ui.display_quality = match id {
                "view.fastDisplay" => Q::Fast,
                "view.typicalDisplay" => Q::Typical,
                _ => Q::High,
            };
            // Redraw everything at the new quality.
            app.canvas.shown = None;
            Ok(Value::Null)
        }
        "view.rulers" => flag(&mut app.ui.rulers),
        "view.guides" => flag(&mut app.ui.guides),
        "view.snapToGuides" => flag(&mut app.ui.snap_to_guides),
        "view.snapToDocumentGrid" => flag(&mut app.ui.snap_to_document_grid),
        "view.smartGuides" => flag(&mut app.ui.smart_guides),
        "view.baselineGrid" => flag(&mut app.ui.baseline_grid),
        "view.documentGrid" => {
            // The grid is drawn into the canvas texture.
            app.canvas.shown = None;
            flag(&mut app.ui.document_grid)
        }
        "view.textThreads" => flag(&mut app.ui.text_threads),
        "view.hiddenCharacters" => flag(&mut app.ui.hidden_characters),
        "view.taggedFrames" => flag(&mut app.ui.tagged_frames),
        "window.hidePanels" | "window.hidePanelsExceptTools" => {
            let mode = if id == "window.hidePanels" { 1 } else { 2 };
            app.ui.hidden_panels = if app.ui.hidden_panels == mode { 0 } else { mode };
            Ok(json!(app.ui.hidden_panels))
        }
        "window.nextDocument" | "window.previousDocument" => {
            let n = app.session.documents().len();
            if n > 1 {
                let cur = app.session.active_index().unwrap_or(0);
                let next = if id == "window.nextDocument" { (cur + 1) % n } else { (cur + n - 1) % n };
                app.session.set_active(next);
            }
            Ok(json!(app.session.active_index()))
        }
        "app.layerOptionsDialog" => crate::dialogs::open_layer_options(app, p.get("id").and_then(Value::as_u64)).map(|()| Value::Null),
        "app.swatchOptionsDialog" => {
            crate::dialogs::open_swatch_options(app, p.get("name").and_then(Value::as_str).unwrap_or("")).map(|()| Value::Null)
        }
        "app.colorGroupOptionsDialog" => {
            crate::dialogs::open_color_group_options(app, p.get("name").and_then(Value::as_str).unwrap_or("")).map(|()| Value::Null)
        }
        "app.layerOptions" => {
            let layers = match app.session.execute("object.pdfLayers", &json!({})) {
                Ok(v) => v,
                Err(e) => return Some(Err(e.to_string())),
            };
            app.ui.dialog = Some(crate::dialogs::Dialog::new("layerOptions", json!({"layers": layers})));
            Ok(Value::Null)
        }
        "view.rotateSpread" => {
            // Turns the spread in the middle of the view (InDesign's Rotate Spread).
            let angle = p.get("angle").and_then(Value::as_i64).unwrap_or(90);
            let Some(st) = app.session.active() else { return Some(Err("no document".into())) };
            let layout = designcraft_tools::CanvasLayout::new(&st.doc, st.editing_parents);
            let Some(slot) = crate::canvas::current_slot(app, &layout).and_then(|i| layout.slots.get(i).copied()) else {
                return Some(Err("no spread".into()));
            };
            let designcraft_doc::SpreadRef::Doc(si) = slot.spread else { return Some(Err("parent spreads don't turn".into())) };
            let r = app.run("layout.rotateSpreadView", json!({"spread": si, "angle": angle}));
            if let Some(rect) = app.canvas_rect {
                crate::canvas::fit(app, rect, "spread");
            }
            app.canvas.shown = None;
            r.map(|v| v["rotation"].clone())
        }
        "view.proofColors" => {
            app.ui.proof_colors = p.get("on").and_then(Value::as_bool).unwrap_or(!app.ui.proof_colors);
            app.canvas.shown = None;
            Ok(json!(app.ui.proof_colors))
        }
        "view.proofSetup" => {
            if let Some(t) = p.get("target").and_then(Value::as_str) {
                let Some(t) = designcraft_color::cms::ProofTarget::parse(t) else { return Some(Err(format!("unknown proof target `{t}`"))) };
                app.ui.proof_setup.target = t;
                app.ui.proof_colors = true;
            }
            if let Some(i) = p.get("intent").and_then(Value::as_str) {
                let Some(i) = designcraft_color::cms::Intent::parse(i) else { return Some(Err(format!("unknown intent `{i}`"))) };
                app.ui.proof_setup.intent = i;
            }
            match p.get("simulatePaper") {
                Some(Value::Bool(b)) => app.ui.proof_setup.simulate_paper = *b,
                Some(Value::String(s)) if s == "toggle" => app.ui.proof_setup.simulate_paper = !app.ui.proof_setup.simulate_paper,
                _ => {}
            }
            app.canvas.shown = None;
            Ok(json!({"target": app.ui.proof_setup.target.id(), "simulatePaper": app.ui.proof_setup.simulate_paper, "on": app.ui.proof_colors}))
        }
        "app.colorSettings" => {
            let f = app.session.execute("color.settings", &json!({})).unwrap_or_default();
            app.ui.dialog = Some(crate::dialogs::Dialog::new("colorSettings", f));
            Ok(Value::Null)
        }
        "view.separations" => {
            if let Some(v) = p.get("plate") {
                app.ui.separation = v.as_str().and_then(|s| ["cyan", "magenta", "yellow", "black"].iter().position(|x| *x == s)).map(|i| i as u8);
            }
            if let Some(v) = p.get("inkLimit") {
                app.ui.ink_limit = v.as_f64().map(|l| (l / 100.0) as f32);
            }
            app.canvas.shown = None;
            Ok(Value::Null)
        }
        "view.firstPage" | "view.previousPage" | "view.nextPage" | "view.lastPage" => {
            let n = app.session.active().map_or(0, |d| d.doc.page_count());
            if n > 0 {
                let cur = crate::canvas::current_page(app).unwrap_or(0);
                let to = match id {
                    "view.firstPage" => 0,
                    "view.previousPage" => cur.saturating_sub(1),
                    "view.nextPage" => cur.saturating_add(1).min(n - 1),
                    _ => n - 1,
                };
                crate::canvas::go_to_page(app, to);
            }
            Ok(Value::Null)
        }
        "view.nextSpread" | "view.previousSpread" => {
            crate::canvas::step_spread(app, if id == "view.nextSpread" { 1 } else { -1 });
            Ok(Value::Null)
        }
        "view.goToPage" => {
            match p.get("page") {
                Some(Value::Number(n)) => crate::canvas::go_to_page(app, (n.as_u64().unwrap_or(1) as usize).saturating_sub(1)),
                Some(Value::String(s)) => match app.session.resolve_page(s) {
                    Some(abs) => crate::canvas::go_to_page(app, abs),
                    None => return Some(Err(format!("no page \"{s}\""))),
                },
                _ => app.ui.dialog = Some(crate::dialogs::Dialog::new("goToPage", json!({}))),
            }
            Ok(Value::Null)
        }
        "app.characterPanel" | "app.paragraphPanel" | "app.glyphsPanel" | "app.characterStylesPanel" | "app.paragraphStylesPanel" => {
            let panel = match id {
                "app.characterPanel" => "character",
                "app.paragraphPanel" => "paragraph",
                "app.glyphsPanel" => "glyphs",
                "app.characterStylesPanel" => "characterStyles",
                _ => "paragraphStyles",
            };
            run_ui(app, "window.panel", &json!({"panel": panel})).unwrap_or(Ok(Value::Null))
        }
        "app.insertCurrentPageNumber" => app.run("text.insert", json!({"text": "\u{e000}", "raw": true})),
        "window.panel" => {
            let panel = p.get("panel").and_then(Value::as_str).unwrap_or("properties").to_string();
            // A panel in the workspace's icon groups opens from there (Essentials Classic's Pages).
            let iconic = app.ui.icon_groups.as_ref().is_some_and(|g| g.iter().flatten().any(|p| *p == panel));
            if panel == "tabs" {
                toggle_tabs_panel(app);
            } else if !iconic && crate::dock::DOCK_TABS.iter().any(|(id, _, _)| *id == panel) {
                app.ui.dock_tab = panel;
                app.ui.dock_expanded = true;
                app.ui.open_panel = None;
            } else if !app.ui.floating.iter().any(|(p, _)| *p == panel) {
                app.ui.open_panel = if app.ui.open_panel.as_deref() == Some(panel.as_str()) { None } else { Some(panel) };
            }
            Ok(Value::Null)
        }
        "fit.fillFrameProportionally" | "fit.contentProportionally" | "fit.frameToContent" | "fit.contentToFrame" | "fit.centerContent" => {
            let mode = match id {
                "fit.fillFrameProportionally" => "fillProportionally",
                "fit.contentProportionally" => "fitProportionally",
                "fit.frameToContent" => "fitFrameToContent",
                "fit.contentToFrame" => "fitContentToFrame",
                _ => "centerContent",
            };
            app.run("object.fit", json!({"mode": mode}))
        }
        "app.keyboardShortcuts" => {
            app.ui.dialog = Some(crate::dialogs::Dialog::new("keyboardShortcuts", json!({"query": "", "recording": ""})));
            Ok(Value::Null)
        }
        "window.setShortcut" => {
            let Some(cmd) = p.get("id").and_then(Value::as_str) else { return Some(Err("missing id".into())) };
            if designcraft_engine::find_command(cmd).is_none() && ui_label(cmd).is_none() {
                return Some(Err(format!("unknown command `{cmd}`")));
            }
            match p.get("shortcut") {
                None | Some(Value::Null) => {
                    app.ui.shortcuts.remove(cmd);
                }
                Some(Value::String(sc)) if sc.is_empty() => {
                    app.ui.shortcuts.insert(cmd.to_string(), String::new());
                }
                Some(Value::String(sc)) => {
                    if parse_shortcut(sc).is_none() {
                        return Some(Err(format!("can't read shortcut `{sc}`")));
                    }
                    app.ui.shortcuts.insert(cmd.to_string(), sc.clone());
                }
                Some(_) => return Some(Err("shortcut: a string or null".into())),
            }
            let sc = shortcut_of(app, cmd);
            let conflicts: Vec<String> =
                effective_shortcuts(app).into_iter().filter(|(i, s)| i != cmd && Some(s) == sc.as_ref()).map(|(i, _)| i).collect();
            Ok(json!({"shortcut": sc, "conflicts": conflicts}))
        }
        "window.resetShortcuts" => {
            app.ui.shortcuts.clear();
            Ok(Value::Null)
        }
        "view.snapPreferences" => {
            let b = |k: &str| p.get(k).and_then(Value::as_bool);
            let ui = &mut app.ui;
            ui.align_edges = b("alignEdges").unwrap_or(ui.align_edges);
            ui.align_centers = b("alignCenters").unwrap_or(ui.align_centers);
            ui.smart_dimensions = b("smartDimensions").unwrap_or(ui.smart_dimensions);
            ui.smart_spacing = b("smartSpacing").unwrap_or(ui.smart_spacing);
            if let Some(z) = p.get("zone").and_then(Value::as_f64).filter(|z| z.is_finite()) {
                ui.snap_zone = z.max(0.0);
            }
            Ok(json!({
                "alignEdges": ui.align_edges,
                "alignCenters": ui.align_centers,
                "smartDimensions": ui.smart_dimensions,
                "smartSpacing": ui.smart_spacing,
                "zone": ui.snap_zone,
            }))
        }
        "app.formattingAffectsText" => {
            app.ui.formatting_affects_text = p.get("on").and_then(Value::as_bool).unwrap_or(!app.ui.formatting_affects_text);
            Ok(json!({"on": app.ui.formatting_affects_text}))
        }
        "window.richBlack" => {
            app.ui.rich_black = p.get("on").and_then(Value::as_bool).unwrap_or(!app.ui.rich_black);
            app.canvas.shown = None;
            Ok(json!({"on": app.ui.rich_black}))
        }
        "window.uiScale" => {
            let v = p.get("scale").and_then(Value::as_f64).unwrap_or(1.0) as f32;
            app.ui.ui_scale = v.clamp(0.5, 3.0);
            Ok(json!({"scale": app.ui.ui_scale}))
        }
        "window.floatPanel" | "window.dockPanel" => {
            let panel = p.get("panel").and_then(Value::as_str).unwrap_or("");
            if !crate::dock::ICON_PANELS.iter().any(|(id, _, _)| *id == panel) && !crate::dock::FLOATING_PANELS.iter().any(|(id, _)| *id == panel) {
                return Some(Err(format!("{id}: unknown panel `{panel}`")));
            }
            if id == "window.floatPanel" {
                let at = egui::pos2(
                    p.get("x").and_then(Value::as_f64).unwrap_or(400.0) as f32,
                    p.get("y").and_then(Value::as_f64).unwrap_or(160.0) as f32,
                );
                crate::dock::float_panel(app, panel, at);
            } else {
                crate::dock::dock_panel(app, panel);
            }
            Ok(json!({"floating": app.ui.floating.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>()}))
        }
        "window.controlBar" => flag(&mut app.ui.control_bar),
        "view.flattenerPreview" => flag(&mut app.ui.flattener_preview),
        "view.tagMarkers" => {
            app.canvas.shown = None;
            flag(&mut app.ui.tag_markers)
        }
        "edit.dynamicSpelling" => flag(&mut app.ui.dynamic_spelling),
        "window.split" => {
            let on = p.get("on").and_then(Value::as_bool).unwrap_or(!app.split);
            app.split = on;
            if on {
                app.second_window = false;
            } else {
                app.switch_pane(0);
                app.focus_pane = 0;
            }
            Ok(json!(on))
        }
        "window.newWindow" => {
            // The second view moves into its own window (on the web: a floating window).
            app.second_window = p.get("on").and_then(Value::as_bool).unwrap_or(true);
            app.split = false;
            if !app.second_window {
                app.switch_pane(0);
                app.focus_pane = 0;
            }
            Ok(json!(app.second_window))
        }
        "window.taskBar" => flag(&mut app.ui.task_bar),
        "window.taskBarPin" => {
            let at = match p.get("at") {
                None | Some(Value::Null) => None,
                Some(v) => match v.as_array().map(|a| a.iter().map(Value::as_f64).collect::<Vec<_>>()).as_deref() {
                    Some([Some(x), Some(y)]) if x.is_finite() && y.is_finite() => Some([x.clamp(-1e6, 1e6) as f32, y.clamp(-1e6, 1e6) as f32]),
                    _ => return Some(Err("window.taskBarPin: `at` is [x, y] in points".into())),
                },
            };
            let on = p.get("on").and_then(Value::as_bool).unwrap_or(at.is_some() || app.ui.task_bar_pin.is_none());
            app.ui.task_bar_pin = match (on, at.or(app.ui.task_bar_pin).or(app.ui.task_bar_at)) {
                (false, _) => None,
                (true, Some(at)) => Some(at),
                (true, None) => return Some(Err("window.taskBarPin: the Contextual Task Bar isn't showing; give `at`".into())),
            };
            Ok(json!(app.ui.task_bar_pin))
        }
        "window.taskBarReset" => {
            app.ui.task_bar_pin = None;
            Ok(Value::Null)
        }
        "help.about" => {
            if let Some(tab) = p.get("tab").and_then(Value::as_str) {
                let Some(i) = crate::about::ABOUT_TABS.iter().position(|t| t.eq_ignore_ascii_case(tab)) else {
                    return Some(Err(format!("help.about: unknown tab `{tab}` (about, contributors, models)")));
                };
                app.ui.about_tab = u8::try_from(i).unwrap_or(0);
            }
            app.ui.about = p.get("open").and_then(Value::as_bool).unwrap_or(true);
            Ok(json!(app.ui.about))
        }
        "help.app" => match p.get("app").and_then(Value::as_str) {
            Some(slug) if !slug.is_empty() && slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') => {
                let url = format!("{}/apps/{slug}", designcraft_engine::links::WEBSITE);
                app.ui.pending_urls.push(url.clone());
                Ok(json!({"url": url}))
            }
            _ => Err("help.app: give `app` (e.g. \"photocraft\")".into()),
        },
        h if h.starts_with("help.") && designcraft_engine::links::get(&h[5..]).is_some() => {
            let url = designcraft_engine::links::get(&h[5..]).unwrap_or_default().to_string();
            app.ui.pending_urls.push(url.clone());
            Ok(json!({"url": url}))
        }
        "window.toolsDoubleColumn" => flag(&mut app.ui.tools_double_column),
        "window.collapseToIcons" => {
            app.ui.dock_expanded = false;
            app.ui.open_panel = None;
            Ok(Value::Null)
        }
        "window.newWorkspace" => {
            let Some(name) = p.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()) else {
                app.ui.dialog =
                    Some(crate::dialogs::Dialog::new("newWorkspace", json!({"name": "", "panelLocations": true, "menuCustomization": true})));
                return Some(Ok(Value::Null));
            };
            let u = &app.ui;
            let w = crate::SavedWorkspace {
                name: name.to_string(),
                control_bar: u.control_bar,
                task_bar: u.task_bar,
                task_bar_pin: u.task_bar_pin,
                tools_double_column: u.tools_double_column,
                dock_tab: u.dock_tab.clone(),
                dock_expanded: u.dock_expanded,
                open_panel: u.open_panel.clone(),
                floating: u.floating.clone(),
                docking: u.docking.clone(),
                docking_hidden: u.docking_hidden.clone(),
                icon_groups: u.icon_groups.clone(),
                keep_panels: !p.get("panelLocations").and_then(Value::as_bool).unwrap_or(true),
                hidden_menu_items: p.get("menuCustomization").and_then(Value::as_bool).unwrap_or(true).then(|| u.hidden_menu_items.clone()),
            };
            app.ui.custom_workspaces.retain(|x| x.name != name);
            app.ui.custom_workspaces.push(w);
            app.ui.workspace = name.to_string();
            Ok(Value::Null)
        }
        "window.deleteWorkspace" => {
            let name = p.get("name").and_then(Value::as_str).unwrap_or("");
            let n = app.ui.custom_workspaces.len();
            app.ui.custom_workspaces.retain(|x| x.name != name);
            if app.ui.custom_workspaces.len() == n {
                return Some(Err(format!("no saved workspace `{name}`")));
            }
            if app.ui.workspace == name {
                app.ui.workspace = "Essentials".into();
            }
            Ok(Value::Null)
        }
        "window.resetWorkspace" => {
            let name = app.ui.workspace.clone();
            return run_ui(app, "window.workspace", &json!({"name": name}));
        }
        "window.workspace" => {
            app.ui.docking_generation = app.ui.docking_generation.wrapping_add(1);
            app.ui.docking_raise = None;
            app.ui.docking_opened = None;
            let name = p.get("name").and_then(Value::as_str).unwrap_or("Essentials");
            if let Some(w) = app.ui.custom_workspaces.iter().find(|w| w.name == name).cloned() {
                if let Some(hidden) = w.hidden_menu_items {
                    app.ui.hidden_menu_items = hidden;
                }
                if w.keep_panels {
                    app.ui.workspace = w.name;
                    return Some(Ok(Value::Null));
                }
                app.ui.control_bar = w.control_bar;
                app.ui.task_bar = w.task_bar;
                app.ui.task_bar_pin = w.task_bar_pin;
                app.ui.tools_double_column = w.tools_double_column;
                app.ui.dock_tab = w.dock_tab;
                app.ui.dock_expanded = w.dock_expanded;
                app.ui.open_panel = w.open_panel;
                app.ui.floating = w.floating;
                app.ui.docking = w.docking;
                app.ui.docking_hidden = w.docking_hidden;
                app.ui.icon_groups = w.icon_groups;
                app.ui.workspace = w.name;
                return Some(Ok(Value::Null));
            }
            // Workspaces choose which bars and panels are visible.
            if app.ui.docking.is_some() {
                app.ui.floating.clear();
            }
            app.ui.docking = None;
            app.ui.docking_hidden.clear();
            app.ui.control_bar = matches!(name, "Advanced" | "Typography" | "Printing and Proofing" | "Book" | "Essentials Classic");
            // Essentials Classic: the Control panel and an icon column of grouped panels, no
            // Properties stack.
            let groups = match name {
                "Essentials Classic" => Some(CLASSIC_GROUPS),
                "Typography" => Some(TYPOGRAPHY_GROUPS),
                _ => None,
            };
            app.ui.icon_groups = groups.map(|groups| groups.iter().map(|g| g.iter().map(|p| p.to_string()).collect()).collect());
            app.ui.dock_expanded = groups.is_none();
            let dock_tab = match name {
                "Interactive for PDF" | "Digital Publishing" => Some("pages"),
                _ => None,
            };
            if let Some(tab) = dock_tab {
                app.ui.dock_tab = tab.into();
                app.ui.dock_expanded = true;
            }
            app.ui.open_panel = match name {
                "Printing and Proofing" => Some("swatches".into()),
                "Interactive for PDF" => Some("buttons".into()),
                "Digital Publishing" => Some("liquid".into()),
                _ => None,
            };
            app.ui.workspace = name.to_string();
            Ok(Value::Null)
        }
        "window.brightness" => {
            let b = p.get("brightness").and_then(Value::as_str).and_then(crate::theme::Brightness::parse).unwrap_or(crate::theme::Brightness::Dark);
            app.ui.brightness = b;
            app.restyle = true;
            Ok(Value::Null)
        }
        _ => return None,
    })
}

/// Save by handing the serialized document to the host as a download (web).
/// Switch screen modes; entering Presentation remembers the mode to return to.
fn set_screen_mode(app: &mut DesignApp, mode: ScreenMode) {
    let presenting = app.ui.screen_mode == ScreenMode::Presentation;
    if mode == ScreenMode::Presentation && !presenting {
        app.ui.presentation_return = app.ui.screen_mode;
        // Presentation fits the spread to the screen; leaving it puts the user's view back.
        app.ui.presentation_view = app.view_key().zip(app.view().copied());
    }
    if mode != ScreenMode::Presentation
        && presenting
        && let Some((k, v)) = app.ui.presentation_view.take()
        && let Some(view) = app.views.get_mut(&k)
    {
        *view = v;
    }
    app.ui.screen_mode = mode;
}

fn download_document(app: &mut DesignApp) -> Result<Value, String> {
    let name = app.session.active().map(|d| format!("{}.designcraft", d.doc.title)).ok_or("no document")?;
    let ser = app.run("file.serialize", json!({}))?;
    let bytes = designcraft_engine::cmd::base64_decode(ser["base64"].as_str().unwrap_or_default());
    let download = app.services.download.as_mut().ok_or("no download service")?;
    download(&name, &bytes).map_err(|e| format!("could not download {name}: {e}"))?;
    // Marks the document saved; on the web the engine doesn't touch a file system.
    app.run("file.saveAs", json!({"path": name}))
}

fn export_png(app: &mut DesignApp, p: &Value) -> Result<Value, String> {
    let st = app.session.active().ok_or("no document")?;
    let page = crate::control::page_index(p, crate::canvas::current_page(app).unwrap_or(0), st.doc.page_count())?;
    let scale = p.get("scale").and_then(Value::as_f64).unwrap_or(2.0);
    let mut r = designcraft_render::Renderer::new();
    let img = r
        .render_page(
            &st.doc,
            &app.session.cache,
            page,
            scale,
            false,
            &designcraft_render::RenderOptions { printing_only: true, rich_black: app.session.prefs.rich_black_output, ..Default::default() },
        )
        .ok_or("no such page")?;
    let png = img.to_png();
    let path = match p.get("path").and_then(Value::as_str) {
        Some(s) => Some(s.to_string()),
        None => {
            let name = format!("{}-p{}.png", st.doc.title, page + 1);
            app.services.pick_save.as_mut().and_then(|f| f(&name))
        }
    };
    let Some(path) = path else { return Ok(Value::Null) };
    match app.services.write.as_mut() {
        Some(w) => w(&path, &png).map(|_| json!({"path": path, "width": img.width, "height": img.height})),
        None => Err("no writer".into()),
    }
}

/// File › Export IDML…: ask for a path, export through `file.exportIdml` and write the bytes with
/// the platform writer (a download on the web).
fn export_idml(app: &mut DesignApp, p: &Value) -> Result<Value, String> {
    let st = app.session.active().ok_or("no document")?;
    let path = match p.get("path").and_then(Value::as_str) {
        Some(s) => Some(s.to_string()),
        None => {
            let name = format!("{}.idml", st.doc.title);
            app.services.pick_save.as_mut().and_then(|f| f(&name))
        }
    };
    let Some(path) = path else { return Ok(Value::Null) };
    let r = app.run("file.exportIdml", json!({}))?;
    let bytes = designcraft_engine::cmd::base64_decode(r["base64"].as_str().unwrap_or_default());
    match app.services.write.as_mut() {
        Some(w) => w(&path, &bytes).map(|_| json!({"path": path, "bytes": bytes.len()})),
        None => Err("no writer".into()),
    }
}

/// Export EPUB / Text: ask for a path (default extension `ext`), run `cmd` without one and write
/// what it returns (`base64` or `text`) with the platform writer.
fn export_bytes(app: &mut DesignApp, p: &Value, ext: &str, cmd: &str) -> Result<Value, String> {
    let st = app.session.active().ok_or("no document")?;
    let path = match p.get("path").and_then(Value::as_str) {
        Some(s) => Some(s.to_string()),
        None => {
            let name = format!("{}.{ext}", st.doc.title);
            app.services.pick_save.as_mut().and_then(|f| f(&name))
        }
    };
    let Some(path) = path else { return Ok(Value::Null) };
    let mut params = json!({});
    if cmd == "file.exportText" {
        let txt = path.to_ascii_lowercase().ends_with(".txt");
        params["format"] = json!(if txt { "txt" } else { "rtf" });
    }
    let r = app.run(cmd, params)?;
    let bytes = match r["text"].as_str() {
        Some(t) => t.as_bytes().to_vec(),
        None => designcraft_engine::cmd::base64_decode(r["base64"].as_str().unwrap_or_default()),
    };
    match app.services.write.as_mut() {
        Some(w) => w(&path, &bytes).map(|_| json!({"path": path, "bytes": bytes.len()})),
        None => Err("no writer".into()),
    }
}

/// File › Export PDF…: ask for a path, export through `file.exportPdf` and write the bytes with the
/// platform writer (a download on the web).
pub(crate) fn export_pdf(app: &mut DesignApp, p: &Value) -> Result<Value, String> {
    let st = app.session.active().ok_or("no document")?;
    let path = match p.get("path").and_then(Value::as_str) {
        Some(s) => Some(s.to_string()),
        None => {
            let name = format!("{}.pdf", st.doc.title);
            app.services.pick_save.as_mut().and_then(|f| f(&name))
        }
    };
    let Some(path) = path else { return Ok(Value::Null) };
    let mut params = if p.is_object() { p.clone() } else { json!({}) };
    if let Some(o) = params.as_object_mut() {
        o.remove("path");
        o.entry("bleed").or_insert(json!(true));
        o.entry("tagged").or_insert(json!(true));
        if !app.ui.flattener.is_empty() {
            o.entry("flatten").or_insert(json!(app.ui.flattener));
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if app.services.write.is_none() && app.services.background_write.is_none() {
            return Err("no writer".into());
        }
        let st = app.session.active().ok_or("no document")?;
        let ctx = app.egui_ctx.clone().unwrap_or_default();
        app.pdf_export.start(st, app.session.cache.clone(), path.clone(), params, ctx, app.services.background_write.clone())?;
        app.status("Exporting PDF…");
        Ok(json!({"path":path,"queued":true}))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let r = app.run("file.exportPdf", params)?;
        let bytes = designcraft_engine::cmd::base64_decode(r["base64"].as_str().unwrap_or_default());
        match app.services.write.as_mut() {
            Some(w) => w(&path, &bytes).map(|_| json!({"path": path, "bytes": bytes.len(), "pages": r["pages"], "warnings": r["warnings"]})),
            None => Err("no writer".into()),
        }
    }
}

/// Commands that act on the text being typed wait while an IME composes at the caret (its marked
/// text isn't typed yet): Undo and Redo (the native menu takes ⌘Z ahead of the IME), the clipboard
/// and the selection.
pub(crate) fn waits_for_ime(app: &DesignApp, id: &str) -> bool {
    app.session.tool_composing()
        && (matches!(
            id,
            "edit.undo" | "edit.redo" | "edit.cut" | "edit.copy" | "edit.clear" | "edit.duplicate" | "edit.selectAll" | "edit.deselectAll"
        ) || id.starts_with("edit.paste"))
}

/// The commands that act on the desktop window itself.
fn is_window_control(id: &str) -> bool {
    matches!(id, "window.minimize" | "window.maximize" | "window.close" | "window.drag" | "window.resize")
}

/// Whether the host can run a window command: all of them with our own title bar, and Minimize
/// with the macOS menu bar (Window › Minimize, ⌘M).
fn window_control_available(app: &DesignApp, id: &str) -> bool {
    app.custom_titlebar || (app.native_menu && id == "window.minimize")
}

/// Is a command enabled (for menu greying)?
pub fn enabled(app: &DesignApp, id: &str) -> bool {
    if waits_for_ime(app, id) {
        return false;
    }
    if is_window_control(id) {
        return window_control_available(app, id);
    }
    match designcraft_engine::find_command(id) {
        Some(c) => (c.enabled)(&app.session).is_ok(),
        None => true,
    }
}

/// The default shortcut of a command.
pub fn default_shortcut(id: &str) -> Option<&'static str> {
    designcraft_engine::find_command(id).and_then(|c| c.shortcut).or_else(|| ui_label(id).and_then(|(_, s)| s))
}

/// The shortcut in effect for a command (the user's, else the default).
pub fn shortcut_of(app: &DesignApp, id: &str) -> Option<String> {
    match app.ui.shortcuts.get(id) {
        Some(s) if s.is_empty() => None,
        Some(s) => Some(s.clone()),
        None => default_shortcut(id).map(str::to_string),
    }
}

/// Every command with a shortcut in effect: (id, shortcut). UI commands come first: where one
/// shares keys with an engine command (⌘N, ⌘O…) it's the one with the dialog.
pub fn effective_shortcuts(app: &DesignApp) -> Vec<(String, String)> {
    UI_COMMANDS
        .iter()
        .map(|c| c.0)
        .chain(designcraft_engine::command_specs().iter().map(|c| c.id))
        .filter_map(|id| shortcut_of(app, id).map(|s| (id.to_string(), s)))
        .collect()
}

/// A shortcut as written in this app ("Cmd+Alt+Shift+K") from a key press.
pub fn shortcut_string(m: egui::Modifiers, key: egui::Key) -> String {
    let mut s = String::new();
    if m.command {
        s += "Cmd+";
    }
    if m.alt {
        s += "Alt+";
    }
    if m.shift {
        s += "Shift+";
    }
    s += key.name();
    s
}

/// A shortcut for display: ⌃⌥⇧⌘ order on macOS, Ctrl+Alt+Shift+ elsewhere.
pub fn shortcut_text(sc: &str) -> String {
    let mac = cfg!(target_os = "macos");
    // The key is the last part ("Cmd+-" ends in an empty part before "-").
    let (mods, key) = match sc.rsplit_once('+') {
        Some((m, "")) => (m.trim_end_matches('+'), "+"),
        Some((m, k)) => (m, k),
        None => ("", sc),
    };
    let has = |m: &str| mods.split('+').any(|x| x == m);
    let mut out = String::new();
    for (m, sym, word) in [("Ctrl", "⌃", "Ctrl+"), ("Alt", "⌥", "Alt+"), ("Shift", "⇧", "Shift+"), ("Cmd", "⌘", "Ctrl+")] {
        if has(m) && !(m == "Cmd" && !mac && has("Ctrl")) {
            out += if mac { sym } else { word };
        }
    }
    out + key
}

/// A menu entry, shared by the in-window menu bar and the native macOS menu.
#[derive(Clone, Debug)]
pub enum Item {
    Sep,
    Sub(String, Vec<Item>),
    Cmd { label: String, id: String, params: Value, shortcut: Option<&'static str> },
}

fn cmd_item(id: &str, params: Value) -> Item {
    let (label, shortcut) = match ui_label(id) {
        Some((l, sc)) => (l.to_string(), sc),
        None => match designcraft_engine::find_command(id) {
            Some(c) => (c.label.to_string(), c.shortcut),
            None => (id.to_string(), None),
        },
    };
    Item::Cmd { label, id: id.to_string(), params, shortcut }
}

fn parse_entries(entries: &[&str]) -> Vec<Item> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < entries.len() {
        let e = entries[i];
        i += 1;
        if e == "-" {
            out.push(Item::Sep);
        } else if let Some(name) = e.strip_prefix('>') {
            // Find the matching `<` (submenus nest).
            let start = i;
            let mut depth = 1;
            while i < entries.len() {
                if entries[i].starts_with('>') {
                    depth += 1;
                } else if entries[i] == "<" {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                i += 1;
            }
            let sub = parse_entries(&entries[start..i]);
            i += 1;
            out.push(Item::Sub(name.to_string(), sub));
        } else if e == "ui:window.panel" {
            let items = crate::dock::DOCK_TABS
                .iter()
                .chain(crate::dock::ICON_PANELS)
                .map(|(id, label, _)| Item::Cmd { label: label.to_string(), id: "window.panel".into(), params: json!({"panel": id}), shortcut: None })
                .collect();
            out.push(Item::Sub("Panels".into(), items));
        } else if e == "ui:window.workspace" {
            // Shown as InDesign's "[Essentials]"; Reset names the current workspace (see `menu_items`).
            let mut items: Vec<Item> = WORKSPACES
                .iter()
                .map(|w| Item::Cmd { label: w.to_string(), id: "window.workspace".into(), params: json!({"name": w}), shortcut: None })
                .collect();
            items.push(Item::Sep);
            items.push(cmd_item("window.resetWorkspace", Value::Null));
            items.push(cmd_item("window.newWorkspace", Value::Null));
            out.push(Item::Sub("Workspace".into(), items));
        } else if e == "ui:window.brightness" {
            let items = crate::theme::Brightness::ALL
                .iter()
                .map(|b| Item::Cmd {
                    label: b.label().to_string(),
                    id: "window.brightness".into(),
                    params: json!({"brightness": b.id()}),
                    shortcut: None,
                })
                .collect();
            out.push(Item::Sub("Interface Color Theme".into(), items));
        } else {
            let (_, id) = e.split_once(':').unwrap_or(("cmd", e));
            // `id|Label|{params}`: a fixed-parameter variant of a command.
            let mut parts = id.splitn(3, '|');
            let id = parts.next().unwrap_or(id);
            match (parts.next(), parts.next().and_then(|j| serde_json::from_str::<Value>(j).ok())) {
                (Some(label), Some(params)) => {
                    let mut it = cmd_item(id, params);
                    if let Item::Cmd { label: l, .. } = &mut it {
                        *l = label.to_string();
                    }
                    out.push(it);
                }
                _ => out.push(cmd_item(id, Value::Null)),
            }
        }
    }
    out
}

/// The whole menu tree (InDesign order).
pub fn menu_tree() -> Vec<(&'static str, Vec<Item>)> {
    MENUS.iter().map(|(m, e)| (*m, parse_entries(e))).collect()
}

/// A row of the macOS application menu (the menu named after the app).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMenuItem {
    /// Runs `command` (no parameters); `id` is the native menu item id, `label` goes through `i18n::tr`.
    Cmd {
        id: &'static str,
        label: &'static str,
        command: &'static str,
    },
    Sep,
    Services,
    Hide,
    HideOthers,
    ShowAll,
}

/// The macOS application menu. Settings… is `app.preferences`, which `native_menu_tree` leaves out of Edit.
pub const MAC_APP_MENU: &[AppMenuItem] = &[
    AppMenuItem::Cmd { id: "dc-about", label: "About DesignCraft", command: "help.about" },
    AppMenuItem::Cmd { id: "dc-discord", label: "Join the ArtCraft Discord…", command: "help.discord" },
    AppMenuItem::Sep,
    AppMenuItem::Cmd { id: "dc-settings", label: "Settings…", command: "app.preferences" },
    AppMenuItem::Sep,
    AppMenuItem::Services,
    AppMenuItem::Sep,
    AppMenuItem::Hide,
    AppMenuItem::HideOthers,
    AppMenuItem::ShowAll,
    AppMenuItem::Sep,
    // Not the predefined Quit: it terminates the app at once, past the unsaved-changes prompt.
    AppMenuItem::Cmd { id: "dc-quit", label: "Quit DesignCraft", command: "app.quit" },
];

/// A label as the macOS menu bar library takes it: it reads "&" as a mnemonic marker and drops
/// it, and shows "&&" as one "&".
pub fn native_label(label: &str) -> String {
    label.replace('&', "&&")
}

/// The menu tree for the macOS menu bar: `menu_tree` without the commands `MAC_APP_MENU` holds,
/// so each command (and its shortcut) appears once, and with the standard Window › Minimize (⌘M)
/// at the top of the Window menu.
pub fn native_menu_tree() -> Vec<(&'static str, Vec<Item>)> {
    let in_app_menu = |id: &str| MAC_APP_MENU.iter().any(|m| matches!(m, AppMenuItem::Cmd { command, .. } if *command == id));
    let mut tree = menu_tree();
    for (title, entries) in &mut tree {
        without_commands(entries, &in_app_menu);
        if *title == "Window" {
            let minimize = Item::Cmd { label: "Minimize".into(), id: "window.minimize".into(), params: Value::Null, shortcut: Some("Cmd+M") };
            entries.splice(0..0, [minimize, Item::Sep]);
        }
    }
    tree
}

/// Drops the parameterless commands `drop` matches from `entries` (recursively), then any separator
/// the removal left at an end or next to another.
fn without_commands(entries: &mut Vec<Item>, drop: &dyn Fn(&str) -> bool) {
    let before = entries.len();
    entries.retain(|e| !matches!(e, Item::Cmd { id, params, .. } if params.is_null() && drop(id)));
    for e in entries.iter_mut() {
        if let Item::Sub(_, children) = e {
            without_commands(children, drop);
        }
    }
    if entries.len() == before {
        return;
    }
    let mut tidy: Vec<Item> = Vec::with_capacity(entries.len());
    for e in entries.drain(..) {
        if matches!(e, Item::Sep) && tidy.last().is_none_or(|l| matches!(l, Item::Sep)) {
            continue;
        }
        tidy.push(e);
    }
    if matches!(tidy.last(), Some(Item::Sep)) {
        tidy.pop();
    }
    *entries = tidy;
}

/// Check state of a toggle command (None = not a toggle).
pub fn checked(app: &DesignApp, id: &str, params: &Value) -> Option<bool> {
    Some(match id {
        "window.workspace" => params.get("name").and_then(Value::as_str) == Some(app.ui.workspace.as_str()),
        "view.rulers" => app.ui.rulers,
        "view.frameEdges" => app.ui.frame_edges,
        "view.overprintPreview" => app.ui.overprint_preview,
        "view.fastDisplay" => app.ui.display_quality == designcraft_render::DisplayQuality::Fast,
        "view.typicalDisplay" => app.ui.display_quality == designcraft_render::DisplayQuality::Typical,
        "view.highQualityDisplay" => app.ui.display_quality == designcraft_render::DisplayQuality::High,
        "view.guides" => app.ui.guides,
        "view.snapToGuides" => app.ui.snap_to_guides,
        "view.snapToDocumentGrid" => app.ui.snap_to_document_grid,
        "view.smartGuides" => app.ui.smart_guides,
        "view.baselineGrid" => app.ui.baseline_grid,
        "view.documentGrid" => app.ui.document_grid,
        "view.textThreads" => app.ui.text_threads,
        "view.hiddenCharacters" => app.ui.hidden_characters,
        "view.taggedFrames" => app.ui.tagged_frames,
        "changes.track" => app.session.active().is_some_and(|d| d.doc.settings.track_changes),
        "window.controlBar" => app.ui.control_bar,
        "window.split" => app.split,
        "window.newWindow" => app.second_window,
        "view.proofColors" => app.ui.proof_colors,
        "view.flattenerPreview" => app.ui.flattener_preview,
        "view.tagMarkers" => app.ui.tag_markers,
        "view.screenMode" => {
            let mode = match params.get("mode").and_then(Value::as_str) {
                Some("normal") => crate::ScreenMode::Normal,
                Some("preview") => crate::ScreenMode::Preview,
                Some("bleed") => crate::ScreenMode::Bleed,
                Some("slug") => crate::ScreenMode::Slug,
                Some("presentation") => crate::ScreenMode::Presentation,
                _ => return None,
            };
            app.ui.screen_mode == mode
        }
        "view.rotateSpread" => match params.get("angle").and_then(Value::as_i64) {
            Some(0) => app.session.active().is_some_and(|st| st.doc.spreads.iter().all(|sp| sp.pages.first().is_none_or(|p| p.view_rotation == 0))),
            _ => return None,
        },
        "view.proofSetup" => match (params.get("target").and_then(Value::as_str), params.get("simulatePaper")) {
            (Some(t), _) => app.ui.proof_setup.target.id() == t,
            (None, Some(_)) => app.ui.proof_setup.simulate_paper,
            _ => return None,
        },
        "edit.dynamicSpelling" => app.ui.dynamic_spelling,
        "app.language" => app.ui.language == params.get("lang").and_then(Value::as_str).unwrap_or(""),
        "app.flattener" => app.ui.flattener == params.get("preset").and_then(Value::as_str).unwrap_or(""),
        "window.taskBar" => app.ui.task_bar,
        "window.taskBarPin" => app.ui.task_bar_pin.is_some(),
        "window.toolsDoubleColumn" => app.ui.tools_double_column,
        "view.togglePreview" => app.ui.screen_mode == crate::ScreenMode::Preview,
        "view.togglePresentation" => app.ui.screen_mode == crate::ScreenMode::Presentation,
        "window.brightness" => params.get("brightness").and_then(Value::as_str) == Some(app.ui.brightness.id()),
        "type.storyDirection" => {
            let st = app.session.active()?;
            let sid = st
                .selection
                .text
                .map(|t| t.story)
                .or_else(|| st.selection.items.iter().find_map(|i| st.doc.item(*i)?.text_frame().map(|t| t.story)))?;
            st.doc.story(sid)?.vertical == params.get("vertical").and_then(Value::as_bool)?
        }
        _ => return None,
    })
}

/// Enablement for menu display (UI commands need a document unless they're app/window-level).
pub fn menu_enabled(app: &DesignApp, id: &str) -> bool {
    if waits_for_ime(app, id) {
        return false;
    }
    if is_window_control(id) {
        return window_control_available(app, id);
    }
    if id == "app.justificationDialog" {
        return enabled(app, "type.para");
    }
    if id.starts_with("fit.") {
        return enabled(app, "object.fit");
    }
    if ui_label(id).is_some() {
        return app.session.active().is_some() || id.starts_with("app.") || id.starts_with("window.") || id.starts_with("help.");
    }
    enabled(app, id)
}

/// Close document `index` (the active one when `None`) the way the user asks for it: with the
/// tab's ×, File ▸ Close or its shortcut. A document with unsaved changes asks first, because
/// closing also discards its recovery data. Scripts and agents use `file.close`, which never asks.
pub fn close_document(app: &mut DesignApp, index: Option<usize>) {
    let Some(i) = index.or(app.session.active_index()) else { return };
    match app.session.documents().get(i) {
        Some(d) if d.is_dirty() => {
            app.ui.dialog = Some(crate::dialogs::Dialog::new("closeDocument", json!({"uid": d.uid, "title": d.title(), "discard": false})));
        }
        _ => {
            let _ = app.run("file.close", json!({"index": i}));
        }
    }
}

/// A menu item was chosen. An engine command whose label ends in "…" and that takes parameters
/// opens a dialog built from its parameter documentation (see [`crate::dialogs::command_fields`]).
pub fn activate(app: &mut DesignApp, id: &str, params: &Value) {
    if waits_for_ime(app, id) {
        return;
    }
    let was_find = app.ui.dialog.as_ref().is_some_and(|d| d.id == "findChange");
    activate_inner(app, id, params);
    if was_find && app.ui.dialog.as_ref().is_none_or(|d| d.id != "findChange") {
        app.session.reset_find_scope();
    }
}

fn activate_inner(app: &mut DesignApp, id: &str, params: &Value) {
    // Object › Effects › Drop Shadow… (and its shortcut): the Effects dialog on that page.
    if params.is_null()
        && let Some(page) = crate::effects_dialog::page_of_command(id)
    {
        crate::effects_dialog::open(app, page);
        return;
    }
    if params.is_null() && id == "swatch.addColorTheme" {
        crate::dialogs::open_color_theme(app, json!({}));
        return;
    }
    if params.is_null() && id == "file.close" {
        close_document(app, None);
        return;
    }
    if params.is_null() && id == "layout.documentSetup" {
        crate::dialogs::open_document_setup(app);
        return;
    }
    if params.is_null() && id == "toc.generate" {
        crate::dialogs::open_toc(app);
        return;
    }
    if params.is_null() && id == "file.print" {
        crate::dialogs::open_print(app);
        return;
    }
    if params.is_null() && id == "layout.section" {
        crate::section_options::open(app);
        return;
    }
    if params.is_null() && id == "layout.createGuides" {
        crate::dialogs::open_create_guides(app);
        return;
    }
    if params.is_null() && id == "layout.marginsAndColumns" {
        crate::dialogs::open_margins_and_columns(app);
        return;
    }
    if params.is_null() && id == "object.textFrameOptions" {
        app.ui.dialog = Some(crate::dialogs::Dialog::new("textFrameOptions", json!({})));
        return;
    }
    // New Paragraph/Character Style…: the style options for the new style.
    if params.is_null() && matches!(id, "style.paragraph.create" | "style.character.create") {
        crate::dialogs::open_new_style(app, id == "style.paragraph.create");
        return;
    }
    // Paragraph/Character Style Options…: those of the selected text's style.
    if params.is_null() && matches!(id, "style.paragraph.edit" | "style.character.edit") {
        let para = id == "style.paragraph.edit";
        let cur = crate::panels::text_attrs(app).and_then(|a| a[if para { "paragraphStyle" } else { "characterStyle" }].as_str().map(str::to_string));
        if let Some(name) = cur {
            crate::dialogs::open_style_options(app, para, &name);
            return;
        }
    }
    if params.is_null()
        && ui_label(id).is_none()
        && let Some(c) = designcraft_engine::find_command(id)
        && !crate::dialogs::command_fields(c.params).is_empty()
        && (c.label.ends_with('…') || crate::dialogs::command_fields(c.params).iter().any(|f| !f.optional))
    {
        app.ui.dialog = Some(crate::dialogs::Dialog::new(&format!("cmd:{id}"), json!({})));
        return;
    }
    let p = if params.is_null() { json!({}) } else { params.clone() };
    // Update Link asks before replacing edits made to linked text in the layout.
    if id == "links.update" {
        crate::panels::properties::update_links(app, p);
        return;
    }
    let _ = app.run(id, p);
}

/// A menu button whose popup stays within the viewport and scrolls when needed.
/// All menu buttons share this wrapper, including workspace, zoom and panel menus.
pub fn menu_button<'a, R>(
    ui: &mut egui::Ui,
    atoms: impl egui::IntoAtoms<'a>,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    use egui::containers::menu::{MenuButton, MenuConfig};

    // A menu opens from a button already shown in the previous frame. Base its height on
    // that anchor, not the popup's eventual position, so placement cannot oscillate.
    let button = ui.ctx().read_response(ui.next_auto_id()).map(|response| response.rect);
    let max_height = menu_max_height(ui, button);
    let contents = |ui: &mut egui::Ui| {
        // Override the popup's initial available height, including when it opens upwards.
        ui.set_max_height(max_height);
        let mut scroll = egui::ScrollArea::vertical().max_height(max_height).min_scrolled_height(1.0);
        if ui.is_sizing_pass() {
            // Closing and reopening a menu starts at its first command again.
            scroll = scroll.vertical_scroll_offset(0.0);
        }
        scroll.show(ui, add_contents).inner
    };
    // Scrollbar clicks must not dismiss the popup; command rows close it explicitly.
    let config = MenuConfig::find(ui).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    let (response, inner) = if egui::containers::menu::is_in_menu(ui) {
        clipped_submenu(ui, atoms, config, contents)
    } else {
        MenuButton::new(atoms).config(config).ui(ui, contents)
    };
    egui::InnerResponse { response, inner: inner.map(|response| response.inner) }
}

/// The tallest menu that fits on either side of its anchor, including the popup frame.
fn menu_max_height(ui: &egui::Ui, button: Option<egui::Rect>) -> f32 {
    let screen = ui.ctx().content_rect();
    let space = match button {
        // Submenus open beside their row, aligned with its top or bottom.
        Some(rect) if egui::containers::menu::is_in_menu(ui) => (screen.bottom() - rect.top()).max(rect.bottom() - screen.top()),
        Some(rect) => (screen.bottom() - rect.bottom()).max(rect.top() - screen.top()),
        None => screen.height(),
    };
    let chrome = egui::Frame::menu(ui.style()).total_margin().sum().y + 8.0;
    (space.min(screen.height()) - chrome).max(1.0)
}

/// egui's submenu hover test uses the full row rect, even outside a scroll area's clip.
fn clipped_submenu<'a, R>(
    ui: &mut egui::Ui,
    atoms: impl egui::IntoAtoms<'a>,
    config: egui::containers::menu::MenuConfig,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> (egui::Response, Option<egui::InnerResponse<R>>) {
    use egui::containers::menu::{MenuState, SubMenu, SubMenuButton};

    let id = SubMenu::id_from_widget_id(ui.next_auto_id());
    let open = MenuState::from_ui(ui, |state, _| state.open_item == Some(id));
    let inactive = ui.style().visuals.widgets.inactive;
    if open {
        ui.style_mut().visuals.widgets.inactive = ui.style().visuals.widgets.open;
    }
    let button = SubMenuButton::new(atoms).config(config);
    let response = ui.add(button.button);
    ui.style_mut().visuals.widgets.inactive = inactive;
    let clip = ui.clip_rect();
    if !response.rect.intersect(clip).is_positive() {
        if open {
            MenuState::from_ui(ui, |state, _| state.open_item = None);
        }
        return (response, None);
    }
    // Undo the half-spacing expansion used by SubMenu::show after clamping to the clip.
    let hover_margin = ui.spacing().item_spacing / 2.0;
    let mut clipped = response.clone();
    clipped.rect = response.rect.expand2(hover_margin).intersect(clip).shrink2(hover_margin);
    clipped.interact_rect = response.interact_rect.intersect(clip);
    let inner = button.sub_menu.show(ui, &clipped, contents);
    (response, inner)
}

/// A native menu bar item was chosen (macOS): like [`activate`], except that the menu bar takes
/// ⌘A, ⌘C, ⌘X, ⌘V, ⌘Z and ⇧⌘Z before the window sees them, so while a text field has keyboard
/// focus, Select All, Copy, Cut, Paste, Undo and Redo act on that field, as in any Mac app.
pub fn activate_native(app: &mut DesignApp, ctx: &egui::Context, id: &str, params: &Value) {
    if params.is_null() && ctx.text_edit_focused() && forward_to_text_edit(app, ctx, id) {
        return;
    }
    // The canvas text editor is not an egui TextEdit, so its clipboard events must pass through
    // egui for the desktop integration to read/write the macOS clipboard.
    if params.is_null() && app.session.wants_text() && matches!(id, "edit.copy" | "edit.cut" | "edit.paste") && forward_to_text_edit(app, ctx, id) {
        return;
    }
    activate(app, id, params);
}

/// Whether a native menu bar item can be chosen: as in the in-window menus, but the editing
/// commands a focused text field takes are always available to it.
pub fn native_menu_enabled(app: &DesignApp, ctx: &egui::Context, id: &str) -> bool {
    (ctx.text_edit_focused() && TEXT_FIELD_COMMANDS.contains(&id)) || menu_enabled(app, id)
}

/// The editing commands a focused text field takes from the native menu bar.
const TEXT_FIELD_COMMANDS: [&str; 6] = ["edit.selectAll", "edit.copy", "edit.cut", "edit.paste", "edit.undo", "edit.redo"];

/// Hand a standard editing command to the focused text field as next frame's input; false when
/// `id` isn't one of them.
fn forward_to_text_edit(app: &mut DesignApp, ctx: &egui::Context, id: &str) -> bool {
    let key = |key: egui::Key, modifiers: egui::Modifiers| {
        [true, false].map(|pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers })
    };
    match id {
        "edit.selectAll" => app.synthetic.extend(key(egui::Key::A, egui::Modifiers::COMMAND)),
        "edit.undo" => app.synthetic.extend(key(egui::Key::Z, egui::Modifiers::COMMAND)),
        "edit.redo" => app.synthetic.extend(key(egui::Key::Z, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT)),
        "edit.copy" => app.synthetic.push(egui::Event::Copy),
        "edit.cut" => app.synthetic.push(egui::Event::Cut),
        // Only the integration reads the system clipboard: it answers with an `Event::Paste`.
        "edit.paste" => ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste),
        _ => return false,
    }
    ctx.request_repaint();
    true
}

/// The menu bar contents (inside the app bar; macOS uses the native menu instead).
pub fn menu_bar(app: &mut DesignApp, ui: &mut egui::Ui) {
    let lang = app.ui.language.clone();
    let mut menus = menu_tree();
    if crate::i18n::is_rtl(&lang) {
        menus.reverse();
    }
    for (menu, entries) in menus {
        menu_button(ui, crate::rtl::widget(ui, crate::i18n::tr(&lang, menu)), |ui| menu_column(app, ui, &[(menu, entries)]));
    }
}

/// One menu's rows, as the menu bar and the canvas context menus show them: runs of items, each
/// with the menu path its rows are hidden by (Edit › Menus).
fn menu_column<P: AsRef<str>>(app: &mut DesignApp, ui: &mut egui::Ui, runs: &[(P, Vec<Item>)]) {
    let lang = app.ui.language.clone();
    let rtl = crate::i18n::is_rtl(&lang);
    if rtl {
        ui.set_max_width(320.0);
    }
    ui.with_layout(egui::Layout::top_down_justified(if rtl { egui::Align::Max } else { egui::Align::Min }), |ui| {
        ui.set_min_width(240.0);
        let mut hidden = 0;
        for (path, items) in runs {
            hidden += menu_items(app, ui, items, path.as_ref());
        }
        if hidden > 0 && !app.ui.show_full_menus {
            ui.separator();
            if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&lang, "Show All Menu Items"))).clicked() {
                app.ui.show_full_menus = true;
            }
        }
    });
}

/// What a right-click on the canvas opens a menu for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextMenu {
    /// The selected objects.
    Object,
    /// The text being edited with the Type tool.
    Text,
    /// The page or pasteboard, where nothing was hit.
    Canvas,
}

// Canvas context menus, in the menu bar's entry syntax; `menu:Menu/Submenu` inserts that menu bar
// submenu as it is.
const CONTEXT_OBJECT: &[&str] = &[
    "cmd:edit.undo",
    "cmd:edit.redo",
    "-",
    "cmd:edit.cut",
    "cmd:edit.copy",
    "cmd:edit.paste",
    "cmd:edit.pasteInto",
    "cmd:edit.pasteInPlace",
    "-",
    ">Transform",
    "cmd:transform.rotate|Rotate 90° Clockwise|{\"angle\": -90}",
    "cmd:transform.rotate|Rotate 90° Counterclockwise|{\"angle\": 90}",
    "cmd:transform.rotate|Rotate 180°|{\"angle\": 180}",
    "-",
    "cmd:transform.flip|Flip Horizontal|{\"axis\": \"horizontal\"}",
    "cmd:transform.flip|Flip Vertical|{\"axis\": \"vertical\"}",
    "-",
    "cmd:transform.clear",
    "<",
    "menu:Object/Arrange",
    "menu:Object/Select",
    "-",
    "cmd:object.group",
    "cmd:object.ungroup",
    "cmd:object.lock",
    "cmd:object.unlockAll",
    "cmd:object.hide",
    "cmd:object.showAll",
    "-",
];
/// Added to the object menu when a graphic frame is selected.
const CONTEXT_GRAPHIC_FRAME: &[&str] = &["menu:Object/Fitting"];
const CONTEXT_OBJECT_TAIL: &[&str] = &["menu:Object/Content", "menu:Object/Effects"];
/// Added to the object menu when a text frame is selected.
const CONTEXT_TEXT_FRAME: &[&str] = &["-", "cmd:type.fillWithPlaceholder", "cmd:object.textFrameOptions"];
const CONTEXT_TEXT: &[&str] = &[
    "cmd:edit.cut",
    "cmd:edit.copy",
    "cmd:edit.paste",
    "cmd:edit.pasteWithoutFormatting",
    "-",
    "menu:Type/Insert Special Character",
    "menu:Type/Insert White Space",
    "menu:Type/Insert Break Character",
    "cmd:type.fillWithPlaceholder",
    "-",
    "menu:Type/Change Case",
    "-",
    "ui:view.hiddenCharacters",
    "ui:app.storyEditor",
    "-",
    "cmd:edit.selectAll",
];
const CONTEXT_CANVAS: &[&str] = &[
    "cmd:edit.undo",
    "cmd:edit.redo",
    "-",
    "cmd:edit.paste",
    "cmd:edit.pasteInPlace",
    "-",
    "ui:view.zoomIn",
    "ui:view.zoomOut",
    "ui:view.fitPage",
    "ui:view.fitSpread",
    "ui:view.actualSize",
    "-",
    "ui:view.rulers",
    "menu:View/Grids & Guides",
    "-",
    "cmd:edit.selectAll",
];

/// The menu-bar path context menu rows are keyed by (Edit › Menus doesn't list them); rows
/// inserted with `menu:` keep their menu bar path.
const CONTEXT_PATH: &str = "Context";

/// The entries of a canvas context menu for the current selection.
fn context_entries(app: &DesignApp, kind: ContextMenu) -> Vec<&'static str> {
    match kind {
        ContextMenu::Object => {
            let (graphic, text) = app.session.active().map_or((false, false), |st| {
                let items = || st.selection.items.iter().filter_map(|i| st.doc.item(*i));
                (items().any(|it| matches!(it.content, designcraft_doc::Content::Graphic(_))), items().any(|it| it.is_text_frame()))
            });
            let mut v = CONTEXT_OBJECT.to_vec();
            if graphic {
                v.extend(CONTEXT_GRAPHIC_FRAME);
            }
            v.extend(CONTEXT_OBJECT_TAIL);
            if text {
                v.extend(CONTEXT_TEXT_FRAME);
            }
            v
        }
        ContextMenu::Text => CONTEXT_TEXT.to_vec(),
        ContextMenu::Canvas => CONTEXT_CANVAS.to_vec(),
    }
}

/// Parses context menu entries: the menu bar's syntax plus `menu:Menu/Submenu` (not inside a `>`
/// submenu). Returns runs of items with the menu path they are hidden by: `Context`, or for an
/// inserted submenu its menu bar menu, so hiding a row in the menu bar hides it here too.
fn context_parse(entries: &[&str]) -> Vec<(String, Vec<Item>)> {
    let tree = menu_tree();
    let mut out = Vec::new();
    let mut run: Vec<&str> = Vec::new();
    for e in entries {
        if let Some(path) = e.strip_prefix("menu:") {
            out.push((CONTEXT_PATH.to_owned(), parse_entries(&std::mem::take(&mut run))));
            let menu = path.rsplit_once('/').map_or(path, |(menu, _)| menu);
            out.push((menu.to_owned(), menu_bar_submenu(&tree, path).into_iter().collect()));
        } else {
            run.push(e);
        }
    }
    out.push((CONTEXT_PATH.to_owned(), parse_entries(&run)));
    out
}

/// The menu bar's submenu at `Menu/Submenu`.
fn menu_bar_submenu(tree: &[(&str, Vec<Item>)], path: &str) -> Option<Item> {
    let (menu, sub) = path.split_once('/')?;
    tree.iter().find(|(m, _)| *m == menu)?.1.iter().find(|it| matches!(it, Item::Sub(name, _) if name == sub)).cloned()
}

/// A canvas context menu: the menu bar's rows (labels, shortcuts, check marks, greyed when they
/// can't run), scrolling when taller than the window.
pub fn context_menu(app: &mut DesignApp, ui: &mut egui::Ui, kind: ContextMenu) {
    let runs = context_parse(&context_entries(app, kind));
    let max_height = menu_max_height(ui, None);
    let mut scroll = egui::ScrollArea::vertical().max_height(max_height).min_scrolled_height(1.0);
    if ui.is_sizing_pass() {
        scroll = scroll.vertical_scroll_offset(0.0);
    }
    scroll.show(ui, |ui| menu_column(app, ui, &runs));
}

/// The key a menu item is hidden by (Edit › Menus).
pub fn menu_key(menu: &str, label: &str) -> String {
    format!("{menu}/{label}")
}

/// Draws a menu's items; returns how many were hidden.
fn menu_items(app: &mut DesignApp, ui: &mut egui::Ui, items: &[Item], path: &str) -> usize {
    let mut hidden = 0;
    for it in items {
        match it {
            Item::Sep => {
                ui.separator();
            }
            Item::Sub(name, children) => {
                let sub = format!("{path}/{name}");
                let shown = crate::i18n::tr(&app.ui.language, name).to_owned();
                menu_button(ui, crate::rtl::widget(ui, shown), |ui| {
                    if sub == OPEN_RECENT {
                        recent_menu_items(app, ui);
                    }
                    hidden += menu_items(app, ui, children, &sub);
                });
            }
            Item::Cmd { id, .. } if !shown(app, id) => {}
            Item::Cmd { label, .. } if !app.ui.show_full_menus && app.ui.hidden_menu_items.contains(&menu_key(path, label)) => hidden += 1,
            Item::Cmd { label, id, params, shortcut: _ } => {
                let lang = &app.ui.language;
                let label = match id.as_str() {
                    "window.workspace" => format!("[{}]", crate::i18n::workspace_name(lang, label)),
                    "window.resetWorkspace" => format!("{} {}", crate::i18n::tr(lang, "Reset"), crate::i18n::workspace_name(lang, &app.ui.workspace)),
                    _ => crate::i18n::tr(lang, label).to_owned(),
                };
                let text = match checked(app, id, params) {
                    Some(true) => format!("✓ {label}"),
                    Some(false) => format!("   {label}"),
                    None => label.to_owned(),
                };
                let mut b = egui::Button::new(crate::rtl::widget(ui, text));
                if let Some(sc) = shortcut_of(app, id) {
                    b = b.shortcut_text(shortcut_text(&sc));
                }
                if ui.add_enabled(menu_enabled(app, id), b).clicked() {
                    activate(app, id, params);
                    ui.close();
                }
            }
        }
    }
    hidden
}

/// The File › Open Recent submenu, whose files come before its fixed items.
pub const OPEN_RECENT: &str = "File/Open Recent";

/// A recent file's menu label: its file name.
pub fn recent_label(path: &str) -> String {
    std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().to_string())
}

/// File › Open Recent's files (`app.openRecent {index}`), then a separator; nothing when empty.
pub fn recent_items(app: &DesignApp) -> Vec<Item> {
    let mut out: Vec<Item> = app
        .session
        .prefs
        .recent_files
        .iter()
        .enumerate()
        .map(|(i, path)| Item::Cmd { label: recent_label(path), id: "app.openRecent".into(), params: json!({"index": i}), shortcut: None })
        .collect();
    if !out.is_empty() {
        out.push(Item::Sep);
    }
    out
}

/// The recent files at the top of File › Open Recent: file names (not translated), the full path
/// as tooltip, dimmed when the file is gone (choosing it then says so).
fn recent_menu_items(app: &mut DesignApp, ui: &mut egui::Ui) {
    let dim = crate::theme::Tokens::get(ui.ctx()).text_dim;
    for it in recent_items(app) {
        match it {
            Item::Cmd { label, id, params, .. } => {
                let path = params["index"].as_u64().and_then(|i| app.session.prefs.recent_files.get(i as usize)).cloned().unwrap_or_default();
                let mut text = egui::RichText::new(label);
                if !crate::chrome::recent_file_stat(ui.ctx(), &path).exists {
                    text = text.color(dim);
                }
                if ui.button(crate::rtl::widget(ui, text)).on_hover_text(&path).clicked() {
                    activate(app, &id, &params);
                    ui.close();
                }
            }
            Item::Sep => {
                ui.separator();
            }
            Item::Sub(..) => {}
        }
    }
}

/// Every menu item as (menu path, label), for Edit › Menus.
pub fn all_menu_items() -> Vec<(String, String)> {
    fn walk(items: &[Item], path: &str, out: &mut Vec<(String, String)>) {
        for it in items {
            match it {
                Item::Sub(name, children) => walk(children, &format!("{path}/{name}"), out),
                Item::Cmd { label, .. } => out.push((path.to_string(), label.clone())),
                Item::Sep => {}
            }
        }
    }
    let mut out = Vec::new();
    for (menu, entries) in menu_tree() {
        walk(&entries, menu, &mut out);
    }
    out
}

fn parse_shortcut(sc: &str) -> Option<(egui::Modifiers, egui::Key)> {
    let mut m = egui::Modifiers::NONE;
    let mut key = None;
    for part in sc.split('+').filter(|s| !s.is_empty()) {
        match part {
            "Cmd" => m.command = true,
            "Shift" => m.shift = true,
            "Alt" => m.alt = true,
            "Ctrl" => m.ctrl = true,
            "=" => key = Some(egui::Key::Equals),
            "-" => key = Some(egui::Key::Minus),
            "[" => key = Some(egui::Key::OpenBracket),
            "]" => key = Some(egui::Key::CloseBracket),
            ";" => key = Some(egui::Key::Semicolon),
            "'" => key = Some(egui::Key::Quote),
            "," => key = Some(egui::Key::Comma),
            "." => key = Some(egui::Key::Period),
            "\\" => key = Some(egui::Key::Backslash),
            "/" => key = Some(egui::Key::Slash),
            "Delete" => key = Some(egui::Key::Delete),
            k => key = egui::Key::from_name(k),
        }
    }
    if sc.ends_with("+-") || sc == "-" {
        key = Some(egui::Key::Minus);
    }
    key.map(|k| (m, k))
}

/// Whether a key press matches a parsed shortcut `m`+`k`: `None` when it doesn't, `Some(true)` for
/// the shortcut's own key and modifiers, `Some(false)` for the Plus key standing in for `=`. On
/// layouts with a dedicated + key (German) or Shift+= (US), Cmd+= arrives as Plus.
fn key_matches(m: egui::Modifiers, k: egui::Key, key: egui::Key, modifiers: egui::Modifiers) -> Option<bool> {
    let plus_for_equals = key == egui::Key::Plus;
    let matches = (k == key || (plus_for_equals && k == egui::Key::Equals))
        && m.command == modifiers.command
        && (m.shift == modifiers.shift || (plus_for_equals && !m.shift))
        && m.alt == modifiers.alt;
    matches.then_some(k == key && m.shift == modifiers.shift)
}

/// Global keyboard shortcuts: menu commands and single-key tool shortcuts.
pub fn shortcuts(app: &mut DesignApp, ctx: &egui::Context) {
    // Only a focused text field takes the keys: the canvas (or a button) having focus after a click
    // must not swallow tool shortcuts until Esc clears it.
    // While an IME composes at the text caret, the keyboard is its own. An open canvas context menu
    // takes the keys (Escape closes it), as a native menu would.
    if ctx.text_edit_focused()
        || app.ui.dialog.is_some()
        || app.ui.palette.is_some()
        || app.session.tool_composing()
        || crate::canvas::context_menu_open(ctx)
        || crate::chrome::search_had_focus(ctx)
    {
        return;
    }
    let typing = app.session.wants_text();
    let events = ctx.input(|i| i.events.clone());
    for e in events {
        let egui::Event::Key { key, pressed: true, modifiers, repeat: false, .. } = e else { continue };
        // Presentation hides every control, so Esc always leaves it, and only that: the canvas must
        // not also see the press and switch tools or deselect.
        if key == egui::Key::Escape && app.ui.screen_mode == ScreenMode::Presentation {
            ctx.input_mut(|i| i.consume_key(modifiers, key));
            let _ = app.run("view.togglePresentation", json!({}));
            continue;
        }
        // Presentation goes through the spreads with the arrow keys, Page Up/Down, Home and End.
        if app.ui.screen_mode == ScreenMode::Presentation && !modifiers.command && !modifiers.alt {
            let step = match key {
                egui::Key::ArrowRight | egui::Key::ArrowDown | egui::Key::PageDown => Some(1),
                egui::Key::ArrowLeft | egui::Key::ArrowUp | egui::Key::PageUp => Some(-1),
                egui::Key::Home => Some(isize::MIN),
                egui::Key::End => Some(isize::MAX),
                _ => None,
            };
            if let Some(step) = step {
                ctx.input_mut(|i| i.consume_key(modifiers, key));
                crate::canvas::step_spread(app, step);
                continue;
            }
        }
        // Command shortcuts.
        let mut fired: Option<(String, bool)> = None;
        let all = effective_shortcuts(app);
        for (id, sc) in &all {
            let id = id.as_str();
            if let Some((m, k)) = parse_shortcut(sc)
                && let Some(exact) = key_matches(m, k, key, modifiers)
                && (m.command || m.alt || !typing)
                && !(id == "edit.clear" && typing)
            {
                // Single-key shortcuts (W) only when not typing.
                if !m.command && !m.alt && !m.shift && typing {
                    continue;
                }
                if id == "edit.clear" || (typing && matches!(id, "edit.copy" | "edit.cut" | "edit.paste" | "edit.pasteInPlace")) {
                    continue; // Handled by the active tool / text clipboard events.
                }
                fired = Some((id.to_string(), exact));
                break;
            }
        }
        if let Some((id, exact)) = fired {
            // The native menu's accelerator fires only for the shortcut's own key and modifiers; the
            // Plus key standing in for = never reaches it.
            if exact && app.native_shortcuts.contains(&id) && !app.ui.shortcuts.contains_key(&id) {
                continue; // The native menu handles it.
            }
            // The key press belongs to the shortcut: what it opens (Quick Apply's list, a dialog's
            // default button) must not see the same press as Enter or Space this frame.
            ctx.input_mut(|i| i.consume_key(modifiers, key));
            // Like choosing the menu item: "…" commands open their dialog.
            activate(app, &id, &Value::Null);
            continue;
        }
        // Tool shortcuts (single keys, Shift+key).
        if !typing && !modifiers.command && !modifiers.alt {
            let name = key.name();
            let sc = if modifiers.shift { format!("Shift+{name}") } else { name.to_string() };
            if let Some(tool) = designcraft_tools::tool_for_shortcut(&sc) {
                log::debug!("tool shortcut {sc} -> {tool}");
                if std::env::var_os("DESIGNCRAFT_DEBUG_KEYS").is_some() {
                    eprintln!("tool shortcut {sc} ({key:?}, {modifiers:?}) -> {tool}");
                }
                app.select_tool(tool);
            } else if key == egui::Key::Escape && app.session.tool_id() != "selection" && !app.session.tool_busy() && app.ui.flyout.is_none() {
                app.select_tool("selection");
            }
        }
    }
}

/// Quick Apply (⌘Return): the document's styles and every command, searched by name.
pub fn palette(app: &mut DesignApp, ctx: &egui::Context) {
    let Some(mut q) = app.ui.palette.clone() else { return };
    let mut close = false;
    let mut run: Option<(String, Value)> = None;
    egui::Modal::new(egui::Id::new("palette")).show(ctx, |ui| {
        ui.set_width(480.0);
        let r = ui.add(
            egui::TextEdit::singleline(&mut q)
                .hint_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Search styles and commands…")))
                .desired_width(f32::INFINITY),
        );
        r.request_focus();
        let items = quick_apply_items(&app.session, &q, crate::cjk_features(app));
        // Only a fresh Return runs the first entry: ⌘Return pressed again or held down (repeats) is
        // the shortcut, not a choice.
        let enter = ui.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::Key { key: egui::Key::Enter, pressed: true, repeat: false, modifiers, .. } if !modifiers.command))
        });
        egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
            for (i, it) in items.iter().enumerate() {
                let mut b = egui::Button::new(it.label.as_str()).frame(false);
                if let Some(sc) = &it.shortcut {
                    b = b.shortcut_text(shortcut_text(sc));
                } else if !it.kind.is_empty() {
                    b = b.shortcut_text(crate::i18n::tr(&app.ui.language, it.kind));
                }
                // The button aligns its label by the layout it's in: left, like the rows with a shortcut.
                let size = egui::vec2(460.0, 22.0);
                let row =
                    ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Min), |ui| {
                        ui.add(b.min_size(size))
                    });
                if row.inner.clicked() || (i == 0 && enter) {
                    run = Some((it.id.clone(), it.params.clone()));
                }
            }
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
    });
    app.ui.palette = if close || run.is_some() { None } else { Some(q) };
    if let Some((id, params)) = run {
        let _ = app.run(&id, params);
    }
}

/// One Quick Apply entry: a style to apply or a command to run.
#[derive(Clone, Debug, PartialEq)]
pub struct QuickItem {
    pub label: String,
    pub id: String,
    pub params: Value,
    pub shortcut: Option<String>,
    /// "Paragraph Style", "Character Style", "Object Style", or "" for commands.
    pub kind: &'static str,
}

/// Quick Apply matches (styles first, as InDesign lists them), at most 14.
pub fn quick_apply_items(session: &designcraft_engine::Session, query: &str, cjk: bool) -> Vec<QuickItem> {
    let ql = query.to_lowercase();
    let hit = |label: &str, id: &str| quick_match(&ql, &[label, id]);
    let mut out = Vec::new();
    if let Some(st) = session.active() {
        let styles = &st.doc.styles;
        let mut add = |name: &str, id: &str, kind: &'static str| {
            if name.starts_with('[') || !hit(name, "") {
                return;
            }
            out.push(QuickItem { label: name.to_string(), id: id.into(), params: json!({"name": name}), shortcut: None, kind });
        };
        for p in &styles.paragraph {
            add(&p.name, "style.paragraph.apply", "Paragraph Style");
        }
        for c in &styles.character {
            add(&c.name, "style.character.apply", "Character Style");
        }
        for o in &styles.object {
            add(&o.name, "style.object.apply", "Object Style");
        }
    }
    // Styles need a query, else they would crowd out the commands.
    if ql.is_empty() {
        out.clear();
    }
    let commands = quick_commands().filter(|(id, l, _)| hit(l, id) && (cjk || !CJK_ONLY.contains(id))).map(|(id, l, sc)| QuickItem {
        label: l.to_string(),
        id: id.to_string(),
        params: json!({}),
        shortcut: sc.map(str::to_string),
        kind: "",
    });
    out.extend(commands);
    out.truncate(14);
    out
}

/// The commands Quick Apply and the application bar's search find: (id, label, default shortcut).
fn quick_commands() -> impl Iterator<Item = (&'static str, &'static str, Option<&'static str>)> {
    designcraft_engine::command_specs()
        .iter()
        .filter(|c| !c.menu.is_empty() || c.shortcut.is_some())
        .map(|c| (c.id, c.label, c.shortcut))
        .chain(UI_COMMANDS.iter().map(|c| (c.0, c.1, c.2)))
}

/// Whether any of `texts` contains the lowercased query (an empty query matches everything).
fn quick_match(query_lower: &str, texts: &[&str]) -> bool {
    query_lower.is_empty() || texts.iter().any(|t| t.to_lowercase().contains(query_lower))
}

/// The application bar's search: the commands whose label in the interface language, English
/// label or id contains `query`, labelled in the interface language with the shortcut in effect.
/// Nothing for an empty query; at most 14.
pub fn command_search(app: &DesignApp, query: &str) -> Vec<QuickItem> {
    let ql = query.trim().to_lowercase();
    if ql.is_empty() {
        return Vec::new();
    }
    let lang = app.ui.language.as_str();
    let mut out: Vec<QuickItem> = Vec::new();
    for (id, label, _) in quick_commands() {
        let shown = crate::i18n::tr(lang, label);
        if out.len() >= 14 || !self::shown(app, id) || out.iter().any(|it| it.id == id) || !quick_match(&ql, &[shown, label, id]) {
            continue;
        }
        out.push(QuickItem { label: shown.to_string(), id: id.to_string(), params: Value::Null, shortcut: shortcut_of(app, id), kind: "" });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The entries (command ids) of a menu bar menu, submenus included.
    fn menu_ids(title: &str) -> Vec<&'static str> {
        let items = MENUS.iter().find(|(t, _)| *t == title).map(|(_, items)| *items).unwrap_or_default();
        items.iter().filter_map(|e| e.split_once(':').map(|(_, rest)| rest.split('|').next().unwrap_or(rest))).collect()
    }

    #[test]
    fn view_items_are_in_the_view_menu_as_in_indesign() {
        let (view, typ) = (menu_ids("View"), menu_ids("Type"));
        for id in [
            "view.rotateSpread",
            "view.proofSetup",
            "view.proofColors",
            "view.flattenerPreview",
            "view.separations",
            "view.taggedFrames",
            "view.tagMarkers",
        ] {
            assert!(view.contains(&id), "{id} is in View");
            assert!(!typ.contains(&id), "{id} isn't in Type");
        }
        // Screen Mode and Extras are View submenus; Show Hidden Characters stays in Type.
        assert!(view.contains(&"view.screenMode") && view.contains(&"view.frameEdges"));
        assert!(typ.contains(&"view.hiddenCharacters") && !view.contains(&"view.hiddenCharacters"));
        // W still toggles Normal / Preview without a menu entry.
        assert_eq!(ui_label("view.togglePreview").and_then(|c| c.1), Some("W"));
    }

    /// The submenu paths of a menu bar menu ("View/Screen Mode"), as Edit › Menus keys them.
    fn submenu_paths(title: &str) -> Vec<String> {
        let items = MENUS.iter().find(|(t, _)| *t == title).map(|(_, items)| *items).unwrap_or_default();
        let mut stack = vec![title.to_string()];
        let mut out = Vec::new();
        for e in items {
            if let Some(name) = e.strip_prefix('>') {
                let path = format!("{}/{name}", stack.last().map_or("", String::as_str));
                out.push(path.clone());
                stack.push(path);
            } else if *e == "<" {
                stack.pop();
            }
        }
        out
    }

    /// A command's menu path names the menu entries that really hold it.
    #[test]
    fn define_lists_command_path_matches_the_type_menu() {
        let spec = designcraft_engine::command_specs().iter().find(|c| c.id == "list.define").expect("list.define is a command");
        assert_eq!(spec.menu[0], "Type");
        assert!(submenu_paths("Type").contains(&spec.menu.join("/")), "{:?} is not a Type submenu", spec.menu);
        assert!(menu_ids("Type").contains(&"list.define"));
    }

    #[test]
    fn screen_mode_shows_the_current_mode_and_view_submenus_are_translated() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let mode = |app: &DesignApp, m: &str| checked(app, "view.screenMode", &json!({ "mode": m }));
        assert_eq!((mode(&app, "normal"), mode(&app, "preview")), (Some(true), Some(false)));
        app.ui.screen_mode = crate::ScreenMode::Preview;
        assert_eq!((mode(&app, "normal"), mode(&app, "preview")), (Some(false), Some(true)));
        // Every View submenu has its Japanese path (Edit › Menus shows them).
        for path in submenu_paths("View") {
            assert_ne!(crate::i18n::tr("ja", &path), path, "{path} has no Japanese row");
        }
    }

    fn walk(items: &[Item], out: &mut Vec<(String, String, Value)>) {
        for it in items {
            match it {
                Item::Sub(_, c) => walk(c, out),
                Item::Cmd { label, id, params, .. } => out.push((label.clone(), id.clone(), params.clone())),
                Item::Sep => {}
            }
        }
    }

    /// Exercise the real dropdown, including clipping and wheel input, on a short viewport.
    #[test]
    fn oversized_edit_menu_scrolls_to_preferences() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700.0, 260.0));
        let mut tick = 0;
        let mut frame = |app: &mut DesignApp, events: Vec<egui::Event>| {
            tick += 1;
            let mut output =
                ctx.run_ui(egui::RawInput { screen_rect: Some(screen), time: Some(tick as f64 / 60.0), events, ..Default::default() }, |ui| {
                    egui::MenuBar::new().ui(ui, |ui| menu_bar(app, ui));
                });
            output.textures_delta.clear();
            let mut labels = Vec::new();
            fn collect(shape: &egui::Shape, clip: egui::Rect, labels: &mut Vec<(String, egui::Rect, egui::Rect)>) {
                match shape {
                    egui::Shape::Text(t) => labels.push((t.galley.job.text.clone(), t.visual_bounding_rect(), clip)),
                    egui::Shape::Vec(shapes) => {
                        for shape in shapes {
                            collect(shape, clip, labels);
                        }
                    }
                    _ => {}
                }
            }
            for shape in output.shapes {
                collect(&shape.shape, shape.clip_rect, &mut labels);
            }
            labels
        };
        frame(&mut app, vec![]);
        let labels = frame(&mut app, vec![]);
        let edit = labels.iter().find(|(text, _, _)| text == "Edit").unwrap().1.center();
        for pressed in [true, false] {
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(edit),
                    egui::Event::PointerButton { pos: edit, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
                ],
            );
        }
        for _ in 0..3 {
            frame(&mut app, vec![]);
        }
        let layer = ctx.layer_id_at(egui::pos2(edit.x + 80.0, 100.0)).expect("menu layer");
        let bounds = ctx.memory(|m| m.area_rect(layer.id)).expect("menu bounds");
        let track = egui::pos2(bounds.right() - 9.0, bounds.bottom() - 25.0);
        for pressed in [true, false] {
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(track),
                    egui::Event::PointerButton { pos: track, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
                ],
            );
        }
        assert!(egui::Popup::is_any_open(&ctx), "clicking the scroll track must not dismiss the menu");
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(egui::pos2(edit.x + 80.0, 100.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -3000.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let mut labels = Vec::new();
        for _ in 0..60 {
            labels = frame(&mut app, vec![]);
        }
        let (_, rect, clip) = labels.iter().find(|(text, _, _)| text == "Preferences…").expect("Preferences is rendered");
        assert!(screen.contains_rect(*rect) && clip.contains_rect(*rect), "Preferences must be visible after scrolling: {rect:?}, {clip:?}");
        let pos = rect.center();
        for pressed in [true, false] {
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
                ],
            );
        }
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("preferences"));
        assert!(!egui::Popup::is_any_open(&ctx), "choosing a command dismisses the menu");
    }

    #[test]
    fn formatting_affects_text_sends_swatches_to_the_text() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        for (name, color) in [("Cyan", "#00ffff"), ("Magenta", "#ff00ff")] {
            let created = app.run("swatch.create", json!({"name": name, "color": color})).unwrap();
            assert_eq!(created["name"], name);
        }
        let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "words", "caret": false})).unwrap();
        let (fid, sid) = (r["id"].as_u64().unwrap(), r["story"].as_u64().unwrap());
        app.run("selection.set", json!({"ids": [fid]})).unwrap();
        let text_fill = |app: &crate::DesignApp| {
            let st = app.session.active().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().clone();
            st.char_format_at(2).over.fill.clone()
        };
        let frame_fill = |app: &crate::DesignApp| app.session.active().unwrap().doc.item(designcraft_doc::ItemId(fid)).unwrap().fill.swatch.clone();
        assert!(!crate::panels::colors_affect_text(&app));
        crate::panels::apply_swatch(&mut app, false, "C=100 M=0 Y=0 K=0", None).unwrap();
        assert_eq!(frame_fill(&app), "C=100 M=0 Y=0 K=0");
        assert_eq!(text_fill(&app), None);
        let r = run_ui(&mut app, "app.formattingAffectsText", &json!({})).unwrap().unwrap();
        assert_eq!(r["on"], true, "toggles on");
        assert!(crate::panels::colors_affect_text(&app));
        crate::panels::apply_swatch(&mut app, false, "C=0 M=100 Y=0 K=0", Some(0.5)).unwrap();
        assert_eq!(frame_fill(&app), "C=100 M=0 Y=0 K=0", "the frame is left alone");
        assert_eq!(text_fill(&app).as_deref(), Some("C=0 M=100 Y=0 K=0"));
        assert_eq!(crate::panels::color_target(&mut app).map(|t| (t.0, t.1)), Some(("C=0 M=100 Y=0 K=0".to_string(), 0.5)));
    }

    #[test]
    fn transform_menu_items_open_their_dialogs() {
        for id in ["transform.move", "transform.scale", "transform.rotate", "transform.shear"] {
            let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            activate(&mut app, id, &Value::Null);
            let dialog = app.ui.dialog.as_ref().map(|d| d.id.clone());
            assert_eq!(dialog.as_deref(), Some(format!("cmd:{id}").as_str()), "{id} opens its dialog");
        }
    }

    #[test]
    fn pdf_export_menu_item_opens_the_dialog_or_exports() {
        // No params: the menu item opens the options dialog.
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        activate(&mut app, "app.exportPdf", &Value::Null);
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("pdfExport"));
        // Explicit options: programmatic callers still export directly, opening no dialog.
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        activate(&mut app, "app.exportPdf", &json!({"path": "/tmp/x.pdf"}));
        assert!(app.ui.dialog.is_none());
    }

    #[test]
    fn snap_preferences_are_a_command() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let r = run_ui(&mut app, "view.snapPreferences", &json!({"smartSpacing": false, "zone": 8})).unwrap().unwrap();
        assert_eq!(r["smartSpacing"], false);
        assert_eq!(r["alignEdges"], true, "unnamed switches keep their value");
        assert_eq!(r["zone"], 8.0);
        assert!(!app.ui.smart_spacing && (app.ui.snap_zone - 8.0).abs() < 1e-9);
        let r = run_ui(&mut app, "view.snapPreferences", &json!({"zone": -3})).unwrap().unwrap();
        assert_eq!(r["zone"], 0.0, "a negative zone turns snapping off");
    }

    #[test]
    fn custom_shortcuts_override_defaults() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        assert_eq!(shortcut_of(&app, "app.preferences").as_deref(), Some("Cmd+K"));
        let r = run_ui(&mut app, "window.setShortcut", &json!({"id": "app.preferences", "shortcut": "Cmd+Alt+Shift+9"})).unwrap().unwrap();
        assert_eq!(r["shortcut"], "Cmd+Alt+Shift+9");
        assert!(effective_shortcuts(&app).contains(&("app.preferences".into(), "Cmd+Alt+Shift+9".into())));
        // Taking another command's keys reports the conflict.
        let r = run_ui(&mut app, "window.setShortcut", &json!({"id": "app.palette", "shortcut": "Cmd+Alt+Shift+9"})).unwrap().unwrap();
        assert_eq!(r["conflicts"], json!(["app.preferences"]));
        run_ui(&mut app, "window.setShortcut", &json!({"id": "app.palette", "shortcut": ""})).unwrap().unwrap();
        assert_eq!(shortcut_of(&app, "app.palette"), None);
        assert!(run_ui(&mut app, "window.setShortcut", &json!({"id": "app.palette", "shortcut": "Cmd+Nope"})).unwrap().is_err());
        run_ui(&mut app, "window.resetShortcuts", &json!({})).unwrap().unwrap();
        // ⌘N is the New Document dialog, not a bare file.new.
        let first_cmd_n = effective_shortcuts(&app).into_iter().find(|(_, s)| s == "Cmd+N").map(|(i, _)| i);
        assert_eq!(first_cmd_n.as_deref(), Some("app.newDocumentDialog"));
        assert_eq!(shortcut_of(&app, "app.palette").as_deref(), Some("Cmd+Return"));
        assert_eq!(shortcut_string(egui::Modifiers { command: true, shift: true, ..Default::default() }, egui::Key::K), "Cmd+Shift+K");
        if cfg!(target_os = "macos") {
            assert_eq!(shortcut_text("Cmd+Alt+3"), "⌥⌘3");
            assert_eq!(shortcut_text("Cmd+Shift+."), "⇧⌘.");
        } else {
            assert_eq!(shortcut_text("Cmd+Alt+3"), "Alt+Ctrl+3");
        }
    }

    #[test]
    fn split_window_panes_have_their_own_views() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        let ctx = egui::Context::default();
        let frame = |app: &mut crate::DesignApp| {
            let input =
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
        };
        frame(&mut app);
        assert_eq!(run_ui(&mut app, "window.split", &json!({})).unwrap().unwrap(), json!(true));
        frame(&mut app);
        frame(&mut app);
        // Both panes have a canvas, each half as wide.
        let r0 = app.canvas_rect.unwrap();
        let r1 = app.other_pane.as_ref().and_then(|o| o.1).unwrap();
        assert!(r0.width() < 800.0 && r1.width() < 800.0 && (r0.center().x - r1.center().x).abs() > 300.0, "{r0:?} {r1:?}");
        // Zooming the right pane leaves the left alone.
        app.switch_pane(1);
        let z0 = app.view().unwrap().zoom;
        crate::canvas::set_zoom(&mut app, z0 * 3.0);
        app.switch_pane(0);
        assert!((app.view().unwrap().zoom - z0).abs() < 1e-9);
        app.switch_pane(1);
        assert!((app.view().unwrap().zoom - z0 * 3.0).abs() < 1e-6);
        assert_eq!(run_ui(&mut app, "window.split", &json!({})).unwrap().unwrap(), json!(false));
        assert_eq!(app.pane, 0);
        frame(&mut app);
    }

    #[test]
    fn split_window_dispatches_text_once_to_the_shared_document() {
        for focus_pane in [0, 1] {
            let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
            app.run("file.new", json!({})).unwrap();
            let made = app.run("frame.create", json!({"rect": [36, 36, 300, 200], "content": "text"})).unwrap();
            let story = made["story"].as_u64().unwrap();
            app.session.set_tool("type");
            app.run("text.select", json!({"story": story, "anchor": 0, "focus": 0})).unwrap();
            assert!(app.session.wants_text());
            app.run("window.split", json!({"on": true})).unwrap();
            app.focus_pane = focus_pane;

            let ctx = egui::Context::default();
            let frame = |app: &mut crate::DesignApp, events| {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
                    events,
                    ..Default::default()
                };
                let mut out = ctx.run_ui(input, |ui| {
                    app.logic(&ui.ctx().clone());
                    app.ui(ui);
                });
                out.textures_delta.clear();
            };
            frame(&mut app, vec![]);
            frame(&mut app, vec![]);
            frame(&mut app, vec![]);
            let undo_before = app.session.active().unwrap().history.undo.len();
            frame(&mut app, vec![egui::Event::Text("A".into())]);
            let text = app.run("story.get", json!({"story": story})).unwrap();
            assert_eq!(text["text"], "A", "one key press with pane {focus_pane} focused");
            assert_eq!(app.session.active().unwrap().history.undo.len(), undo_before + 1);
            app.run("edit.undo", json!({})).unwrap();
            assert_eq!(app.run("story.get", json!({"story": story})).unwrap()["text"], "");

            // A press and key arriving together must use the newly focused pane, whichever
            // canvas is drawn first. Middle-click changes pane focus without moving the caret.
            let other = app.other_pane.as_ref().and_then(|p| p.1).unwrap().center();
            frame(&mut app, vec![egui::Event::PointerMoved(other)]);
            frame(
                &mut app,
                vec![
                    egui::Event::PointerButton { pos: other, button: egui::PointerButton::Middle, pressed: true, modifiers: egui::Modifiers::NONE },
                    egui::Event::Text("B".into()),
                ],
            );
            assert_eq!(app.focus_pane, 1 - focus_pane);
            assert_eq!(app.run("story.get", json!({"story": story})).unwrap()["text"], "B", "a key in the pane-switch frame is dispatched once");
            assert_eq!(app.session.active().unwrap().history.undo.len(), undo_before + 1);
        }
    }

    #[test]
    fn split_window_click_focus_routes_menu_zoom_but_hover_scroll_stays_local() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("window.split", json!({"on": true})).unwrap();
        let ctx = egui::Context::default();
        let mut time = 0.0;
        let mut frame = |app: &mut crate::DesignApp, events| {
            time += 0.1;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
                time: Some(time),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
        };
        for _ in 0..3 {
            frame(&mut app, vec![]);
        }
        let left = app.canvas_rect.unwrap().center();
        let right = app.other_pane.as_ref().and_then(|p| p.1).unwrap().center();
        let cameras = |app: &mut crate::DesignApp| {
            let current = app.pane;
            app.switch_pane(0);
            let left = *app.view().unwrap();
            app.switch_pane(1);
            let right = *app.view().unwrap();
            app.switch_pane(current);
            [left, right]
        };
        let button =
            |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        frame(&mut app, vec![egui::Event::PointerMoved(right)]);
        frame(&mut app, vec![button(right, true)]);
        frame(&mut app, vec![button(right, false)]);
        assert_eq!((app.pane, app.focus_pane), (1, 1));
        let before = cameras(&mut app);
        app.run("view.zoom", json!({"zoom": 2.0})).unwrap();
        let zoomed = cameras(&mut app);
        assert_eq!(zoomed[0], before[0], "menu zoom leaves the other camera alone");
        assert!((zoomed[1].zoom - 2.0).abs() < 1e-9);

        frame(&mut app, vec![egui::Event::PointerMoved(left)]);
        frame(
            &mut app,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -40.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let scrolled = cameras(&mut app);
        assert_eq!(scrolled[1], zoomed[1], "hover scroll leaves the focused camera alone");
        assert_ne!(scrolled[0].origin, zoomed[0].origin);
        assert_eq!((app.pane, app.focus_pane), (1, 1), "hovering does not move menu focus");

        frame(&mut app, vec![button(left, true)]);
        frame(&mut app, vec![button(left, false)]);
        assert_eq!((app.pane, app.focus_pane), (0, 0));
        app.run("view.zoom", json!({"zoom": 1.0})).unwrap();
        let final_views = cameras(&mut app);
        assert!((final_views[0].zoom - 1.0).abs() < 1e-9);
        assert_eq!(final_views[1], scrolled[1]);
    }

    #[test]
    fn split_window_contextual_bars_have_independent_widget_ids() {
        fn id_warnings(shape: &egui::Shape, found: &mut Vec<String>) {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| id_warnings(shape, found)),
                egui::Shape::Text(text) if text.galley.text().contains("use of widget ID") => found.push(text.galley.text().to_string()),
                _ => {}
            }
        }

        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        for rect in [[36, 36, 280, 180], [300, 36, 550, 180]] {
            app.run("frame.create", json!({"rect": rect, "content": "unassigned"})).unwrap();
        }
        app.run("edit.selectAll", json!({})).unwrap();
        app.run("window.split", json!({"on": true})).unwrap();
        let ctx = egui::Context::default();
        ctx.options_mut(|options| options.warn_on_id_clash = true);
        let mut warnings = Vec::new();
        for _ in 0..4 {
            let input =
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
            let mut output = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            warnings.clear();
            for shape in &output.shapes {
                id_warnings(&shape.shape, &mut warnings);
            }
            output.textures_delta.clear();
        }
        assert!(warnings.is_empty(), "both bars reused the same interactive widget IDs: {warnings:?}");
        let bars: Vec<_> =
            (0_u8..2).map(|pane| ctx.memory(|memory| memory.area_rect(egui::Id::new(("task_bar", egui::ViewportId::ROOT, pane)))).unwrap()).collect();
        assert!(!bars[0].intersects(bars[1]), "each pane keeps its own bar: {bars:?}");
    }

    #[test]
    fn show_document_grid_toggles_from_the_grids_and_guides_menu() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let tree = menu_tree();
        let mut items = vec![];
        for (_, entries) in &tree {
            walk(entries, &mut items);
        }
        assert!(items.iter().any(|(label, id, _)| id == "view.documentGrid" && label == "Show/Hide Document Grid"));
        assert_eq!(shortcut_of(&app, "view.documentGrid").as_deref(), Some("Cmd+'"));
        assert_eq!(checked(&app, "view.documentGrid", &json!({})), Some(false));
        assert_eq!(run_ui(&mut app, "view.documentGrid", &json!({})).unwrap().unwrap(), json!(true));
        assert!(app.ui.document_grid);
        assert_eq!(checked(&app, "view.documentGrid", &json!({})), Some(true));
        run_ui(&mut app, "view.documentGrid", &json!({})).unwrap().unwrap();
        assert!(!app.ui.document_grid);
    }

    #[test]
    fn interface_language_switches_and_validates() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        run_ui(&mut app, "app.language", &json!({"lang": "de"})).unwrap().unwrap();
        assert_eq!(app.ui.language, "de");
        assert_eq!(checked(&app, "app.language", &json!({"lang": "de"})), Some(true));
        assert_eq!(crate::i18n::tr(&app.ui.language, "Window"), "Fenster");
        assert!(run_ui(&mut app, "app.language", &json!({"lang": "xx"})).unwrap().is_err());
        // The language is set in Preferences, not from a menu; without `lang` the command opens it.
        let mut items = Vec::new();
        for (_, entries) in menu_tree() {
            walk(&entries, &mut items);
        }
        assert!(!items.iter().any(|(_, id, _)| id == "app.language"), "no menu item sets the language");
        run_ui(&mut app, "app.language", &json!({})).unwrap().unwrap();
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("preferences"));
        assert_eq!(app.ui.language, "de");
        app.ui.dialog = None;

        run_ui(&mut app, "app.language", &json!({"lang": "zh"})).unwrap().unwrap();
        assert_eq!(app.ui.language, "zh");
        assert_eq!(crate::i18n::tr(&app.ui.language, "File"), "文件");

        app.run("file.new", json!({"pages": 4, "facingPages": true})).unwrap();
        let before = serde_json::to_value(&app.session.doc().unwrap().doc).unwrap();
        run_ui(&mut app, "app.language", &json!({"lang": "ar"})).unwrap().unwrap();
        assert_eq!(app.ui.language, "ar");
        assert_eq!(checked(&app, "app.language", &json!({"lang": "ar"})), Some(true));
        assert_eq!(before, serde_json::to_value(&app.session.doc().unwrap().doc).unwrap());
        for (title, _) in menu_tree() {
            assert_ne!(crate::i18n::tr("ar", title), title, "{title}");
        }
        // Every menu title has a translation (Japanese never matches the English).
        for (title, _) in menu_tree() {
            assert_ne!(crate::i18n::tr("ja", title), title, "{title}");
        }

        run_ui(&mut app, "app.language", &json!({"lang": "pt-br"})).unwrap().unwrap();
        assert_eq!(app.ui.language, "pt-br");
        assert_eq!(checked(&app, "app.language", &json!({"lang": "pt-br"})), Some(true));
        for (title, _) in menu_tree() {
            // "Layout" is the natural Brazilian Portuguese word, so it stays identical
            // to English; every other title must be translated.
            if title != "Layout" {
                assert_ne!(crate::i18n::tr("pt-br", title), title, "{title}");
            }
        }

        run_ui(&mut app, "app.language", &json!({"lang": "it"})).unwrap().unwrap();
        assert_eq!(app.ui.language, "it");
        assert_eq!(checked(&app, "app.language", &json!({"lang": "it"})), Some(true));
        for (title, _) in menu_tree() {
            // "File" and "Layout" are the usual Italian menu names, so they stay identical
            // to English; every other title must be translated.
            if title != "File" && title != "Layout" {
                assert_ne!(crate::i18n::tr("it", title), title, "{title}");
            }
        }

        run_ui(&mut app, "app.language", &json!({"lang": "ka"})).unwrap().unwrap();
        assert_eq!(app.ui.language, "ka");
        assert_eq!(checked(&app, "app.language", &json!({"lang": "ka"})), Some(true));
        // Every menu title, item and submenu is translated into Georgian.
        let mut untranslated = Vec::new();
        for (title, entries) in menu_tree() {
            let mut labels = vec![title.to_string()];
            let mut stack: Vec<&Item> = entries.iter().collect();
            while let Some(entry) = stack.pop() {
                match entry {
                    Item::Sub(label, children) => {
                        labels.push(label.clone());
                        stack.extend(children.iter());
                    }
                    Item::Cmd { label, id, .. } if id != "app.language" => labels.push(label.clone()),
                    _ => {}
                }
            }
            // Labels without letters ("180°") or point sizes ("72 pt") read the same in every language.
            let size = |l: &str| l.strip_suffix(" pt").is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
            untranslated
                .extend(labels.into_iter().filter(|l| l.chars().any(char::is_alphabetic) && !size(l) && crate::i18n::tr("ka", l) == l.as_str()));
        }
        assert!(untranslated.is_empty(), "untranslated Georgian menu labels: {untranslated:?}");
    }

    #[test]
    fn ukrainian_language_is_selectable_persisted_and_keeps_document_data() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 4, "facingPages": true})).unwrap();
        app.run("frame.create", json!({"rect": [36, 36, 200, 200], "content": "text", "text": "My words · Мої слова"})).unwrap();
        let before = serde_json::to_value(&app.session.doc().unwrap().doc).unwrap();
        run_ui(&mut app, "app.language", &json!({"lang": "uk"})).unwrap().unwrap();
        assert_eq!(checked(&app, "app.language", &json!({"lang": "uk"})), Some(true));
        assert_eq!(before, serde_json::to_value(&app.session.doc().unwrap().doc).unwrap());
        for (title, _) in menu_tree() {
            assert_ne!(crate::i18n::tr("uk", title), title, "{title}");
        }
        let bytes = serde_json::to_vec(&app.ui).unwrap();
        let restored: crate::UiState = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored.language, "uk");
        assert_eq!(crate::i18n::tr(&restored.language, "File"), "Файл");
        assert_eq!(crate::i18n::tr(&restored.language, "A custom name"), "A custom name");
        assert!(crate::i18n::LANGUAGES.contains(&("uk", "Українська")), "Preferences lists Ukrainian by its own name");
    }

    #[test]
    fn russian_language_is_selectable_persisted_and_keeps_document_data() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 4, "facingPages": true})).unwrap();
        app.run("frame.create", json!({"rect": [36, 36, 200, 200], "content": "text", "text": "My words · Мои слова"})).unwrap();
        let before = serde_json::to_value(&app.session.doc().unwrap().doc).unwrap();
        run_ui(&mut app, "app.language", &json!({"lang": "ru"})).unwrap().unwrap();
        assert_eq!(checked(&app, "app.language", &json!({"lang": "ru"})), Some(true));
        assert_eq!(before, serde_json::to_value(&app.session.doc().unwrap().doc).unwrap());
        for (title, _) in menu_tree() {
            assert_ne!(crate::i18n::tr("ru", title), title, "{title}");
        }
        let bytes = serde_json::to_vec(&app.ui).unwrap();
        let restored: crate::UiState = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored.language, "ru");
        assert_eq!(crate::i18n::tr(&restored.language, "File"), "Файл");
        assert_eq!(crate::i18n::tr(&restored.language, "A custom name"), "A custom name");
        assert!(crate::i18n::LANGUAGES.contains(&("ru", "Русский")), "Preferences lists Russian by its own name");
    }

    #[test]
    fn czech_language_is_selectable_persisted_and_keeps_document_data() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 4, "facingPages": true})).unwrap();
        app.run("frame.create", json!({"rect": [36, 36, 200, 200], "content": "text", "text": "My words · Příliš žluťoučký kůň"})).unwrap();
        let before = serde_json::to_value(&app.session.doc().unwrap().doc).unwrap();
        run_ui(&mut app, "app.language", &json!({"lang": "cs"})).unwrap().unwrap();
        assert_eq!(app.ui.language, "cs");
        assert_eq!(checked(&app, "app.language", &json!({"lang": "cs"})), Some(true));
        assert_eq!(before, serde_json::to_value(&app.session.doc().unwrap().doc).unwrap());
        for (title, _) in menu_tree() {
            assert_ne!(crate::i18n::tr("cs", title), title, "{title}");
        }
        let bytes = serde_json::to_vec(&app.ui).unwrap();
        let restored: crate::UiState = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored.language, "cs");
        assert_eq!(crate::i18n::tr(&restored.language, "File"), "Soubor");
        assert_eq!(crate::i18n::tr(&restored.language, "A custom name"), "A custom name");
        assert!(crate::i18n::LANGUAGES.contains(&("cs", "Čeština")), "Preferences lists Czech by its own name");
    }

    #[test]
    fn polish_language_is_selectable_persisted_and_keeps_document_data() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 2, "facingPages": true})).unwrap();
        app.run("frame.create", json!({"rect": [36, 36, 200, 200], "content": "text", "text": "Zażółć gęślą jaźń"})).unwrap();
        let before = serde_json::to_value(&app.session.doc().unwrap().doc).unwrap();
        run_ui(&mut app, "app.language", &json!({"lang": "pl"})).unwrap().unwrap();
        assert_eq!(app.ui.language, "pl");
        assert_eq!(checked(&app, "app.language", &json!({"lang": "pl"})), Some(true));
        assert_eq!(before, serde_json::to_value(&app.session.doc().unwrap().doc).unwrap());
        for (title, _) in menu_tree() {
            assert_ne!(crate::i18n::tr("pl", title), title, "{title}");
        }
        let bytes = serde_json::to_vec(&app.ui).unwrap();
        let restored: crate::UiState = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored.language, "pl");
        assert_eq!(crate::i18n::tr(&restored.language, "File"), "Plik");
        assert!(crate::i18n::LANGUAGES.contains(&("pl", "Polski")), "Preferences lists Polish by its own name");
    }

    #[test]
    fn arabic_dialog_and_language_switch_keep_finite_widget_geometry() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        let frame = |app: &mut crate::DesignApp| {
            for _ in 0..4 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))),
                        max_texture_side: Some(8192),
                        ..Default::default()
                    },
                    |ui| {
                        app.logic(&ui.ctx().clone());
                        app.ui(ui);
                    },
                );
                output.textures_delta.clear();
            }
        };
        app.run("app.language", json!({"lang": "ar"})).unwrap();
        frame(&mut app);
        app.run("app.newDocumentDialog", json!({})).unwrap();
        frame(&mut app); // egui asserts that all allocated rectangles are finite.
        crate::dialogs::confirm(&mut app).unwrap();
        frame(&mut app);
        assert_eq!(app.session.documents().len(), 1);
        for lang in ["uk", "ru", "zh", "", "ar", "pt-br", "it", "cs", "pl", "ka"] {
            app.run("app.language", json!({"lang": lang})).unwrap();
            frame(&mut app);
        }
    }

    #[test]
    fn story_direction_is_checked() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        app.session.execute("frame.create", &json!({"rect": [72, 72, 200, 400], "content": "text", "text": "縦", "vertical": true})).unwrap();
        assert_eq!(checked(&app, "type.storyDirection", &json!({"vertical": true})), Some(true));
        assert_eq!(checked(&app, "type.storyDirection", &json!({"vertical": false})), Some(false));
    }

    #[test]
    fn proof_setup_and_colors() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        assert_eq!(checked(&app, "view.proofColors", &Value::Null), Some(false));
        let r = run_ui(&mut app, "view.proofSetup", &json!({"target": "deuteranopia"})).unwrap().unwrap();
        assert_eq!(r["on"], true);
        assert_eq!(checked(&app, "view.proofSetup", &json!({"target": "deuteranopia"})), Some(true));
        run_ui(&mut app, "view.proofSetup", &json!({"simulatePaper": "toggle"})).unwrap().unwrap();
        assert!(app.ui.proof_setup.simulate_paper);
        run_ui(&mut app, "view.proofColors", &json!({})).unwrap().unwrap();
        assert!(!app.ui.proof_colors);
        assert!(run_ui(&mut app, "view.proofSetup", &json!({"target": "nope"})).unwrap().is_err());
    }

    #[test]
    fn fitting_has_indesigns_shortcuts() {
        // LYH Studio episode 6 (7:00): "Fit Content to Frame (Ctrl + Shift + Alt + C)" fills the
        // photo frames proportionally.
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let png = designcraft_render::Rendered { width: 40, height: 20, pixels: vec![200; 40 * 20 * 4] }.to_png();
        let f = app.run("frame.create", json!({"rect": [100, 100, 300, 300]})).unwrap()["id"].as_u64().unwrap();
        app.run("file.place", json!({"base64": designcraft_engine::cmd::base64_encode(&png), "name": "a.png", "frame": f})).unwrap();
        app.run("selection.set", json!({"ids": [f]})).unwrap();
        let width = |app: &crate::DesignApp| {
            let it = app.session.doc().unwrap().doc.item(designcraft_doc::ItemId(f)).unwrap().clone();
            let designcraft_doc::Content::Graphic(g) = &it.content else { panic!("graphic") };
            it.xf.transform_rect_bbox(g.xf.transform_rect_bbox(designcraft_geom::Rect::new(0.0, 0.0, g.size.0, g.size.1))).width()
        };
        for (id, keys) in [
            ("fit.fillFrameProportionally", "Cmd+Alt+Shift+C"),
            ("fit.contentProportionally", "Cmd+Alt+Shift+E"),
            ("fit.frameToContent", "Cmd+Alt+C"),
            ("fit.contentToFrame", "Cmd+Alt+E"),
            ("fit.centerContent", "Cmd+Shift+E"),
        ] {
            assert_eq!(shortcut_of(&app, id).as_deref(), Some(keys));
        }
        let ctx = egui::Context::default();
        let press = |app: &mut crate::DesignApp, key: egui::Key| {
            let modifiers =
                egui::Modifiers { alt: true, ctrl: !cfg!(target_os = "macos"), shift: true, mac_cmd: cfg!(target_os = "macos"), command: true };
            let input = egui::RawInput {
                events: vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }],
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| shortcuts(app, &ui.ctx().clone()));
            out.textures_delta.clear();
        };
        press(&mut app, egui::Key::C);
        assert!((width(&app) - 400.0).abs() < 1e-6, "filled: {}", width(&app));
        press(&mut app, egui::Key::E);
        assert!((width(&app) - 200.0).abs() < 1e-6, "fitted: {}", width(&app));
    }

    #[test]
    fn tab_and_presentation_hide_floating_panels_too() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        let ctx = egui::Context::default();
        let frame = |app: &mut crate::DesignApp| {
            for _ in 0..2 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
                    ..Default::default()
                };
                let mut out = ctx.run_ui(input, |ui| {
                    app.logic(&ui.ctx().clone());
                    app.ui(ui);
                });
                out.textures_delta.clear();
            }
        };
        // The icon column's flyout (the classic dock) and a floating panel group (the docking layout).
        let flyout = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("panel_flyout"));
        let group = egui::LayerId::new(egui::Order::Middle, egui::Id::new("designcraft-panel-docking").with(("floating", "preflight")));
        let shown = |layer: egui::LayerId| ctx.memory(|m| m.areas().is_visible(&layer));
        let toggles = [
            ("window.hidePanels", json!({}), json!({})),
            ("window.hidePanelsExceptTools", json!({}), json!({})),
            ("view.screenMode", json!({"mode": "presentation"}), json!({"mode": "normal"})),
        ];
        app.run("window.panel", json!({"panel": "preflight"})).unwrap();
        frame(&mut app);
        assert!(app.ui.docking.is_none() && shown(flyout));
        for (id, hide, show) in &toggles {
            app.run(id, hide.clone()).unwrap();
            frame(&mut app);
            assert!(!shown(flyout), "{id} hides the flyout");
            app.run(id, show.clone()).unwrap();
            frame(&mut app);
            assert!(shown(flyout), "{id} again brings it back");
        }
        app.run("window.floatPanel", json!({"panel": "preflight", "x": 400, "y": 200})).unwrap();
        frame(&mut app);
        assert!(app.ui.docking.is_some() && shown(group));
        for (id, hide, show) in &toggles {
            app.run(id, hide.clone()).unwrap();
            frame(&mut app);
            assert!(!shown(group), "{id} hides the floating panel");
            app.run(id, show.clone()).unwrap();
            frame(&mut app);
            assert!(shown(group), "{id} again brings it back");
        }
    }

    #[test]
    fn tab_hides_panels_and_documents_cycle() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        assert_eq!(shortcut_of(&app, "window.hidePanels").as_deref(), Some("Tab"));
        run_ui(&mut app, "window.hidePanels", &json!({})).unwrap().unwrap();
        assert_eq!(app.ui.hidden_panels, 1);
        run_ui(&mut app, "window.hidePanelsExceptTools", &json!({})).unwrap().unwrap();
        assert_eq!(app.ui.hidden_panels, 2);
        run_ui(&mut app, "window.hidePanelsExceptTools", &json!({})).unwrap().unwrap();
        assert_eq!(app.ui.hidden_panels, 0);
        for _ in 0..3 {
            app.session.execute("file.new", &json!({})).unwrap();
        }
        assert_eq!(app.session.active_index(), Some(2));
        run_ui(&mut app, "window.nextDocument", &json!({})).unwrap().unwrap();
        assert_eq!(app.session.active_index(), Some(0));
        run_ui(&mut app, "window.previousDocument", &json!({})).unwrap().unwrap();
        assert_eq!(app.session.active_index(), Some(2));
    }

    #[test]
    fn rotate_spread_view() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({"pages": 3})).unwrap();
        let ctx = egui::Context::default();
        let frame = |app: &mut crate::DesignApp| {
            let input =
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
        };
        frame(&mut app);
        frame(&mut app);
        assert_eq!(run_ui(&mut app, "view.rotateSpread", &json!({"angle": 90})).unwrap().unwrap(), json!(90));
        frame(&mut app);
        // Only the spread in view turns: its canvas footprint swaps width and height.
        let d = app.session.active().unwrap().doc.clone();
        let turned: Vec<usize> = d.spreads.iter().enumerate().filter(|(_, sp)| sp.pages[0].view_rotation == 1).map(|(i, _)| i).collect();
        assert_eq!(turned.len(), 1);
        let layout = designcraft_tools::CanvasLayout::new(&d, false);
        let slot = layout.slots[turned[0]];
        let b = d.spreads[turned[0]].bounds();
        assert!((slot.bounds.width() - b.height()).abs() < 1e-6 && (slot.bounds.height() - b.width()).abs() < 1e-6);
        assert_eq!(checked(&app, "view.rotateSpread", &json!({"angle": 0})), Some(false));
        run_ui(&mut app, "view.rotateSpread", &json!({"angle": 0})).unwrap().unwrap();
        assert_eq!(checked(&app, "view.rotateSpread", &json!({"angle": 0})), Some(true));
        frame(&mut app);
    }

    #[test]
    fn hand_tool_hold_starts_power_zoom() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        let ctx = egui::Context::default();
        let frame = |app: &mut crate::DesignApp, t: f64, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
                time: Some(t),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
        };
        frame(&mut app, 0.0, vec![]);
        frame(&mut app, 0.05, vec![]);
        app.select_tool("hand");
        let r = app.canvas_rect.unwrap();
        let p = r.center();
        let z0 = app.view().unwrap().zoom;
        frame(&mut app, 0.1, vec![egui::Event::PointerMoved(p)]);
        frame(
            &mut app,
            0.2,
            vec![egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }],
        );
        frame(&mut app, 0.4, vec![]);
        assert!(app.power_zoom.is_none(), "not yet");
        frame(&mut app, 0.8, vec![]);
        assert!(app.power_zoom.is_some(), "holding still starts Power Zoom");
        frame(
            &mut app,
            0.9,
            vec![egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }],
        );
        assert!(app.power_zoom.is_none());
        assert!((app.view().unwrap().zoom - z0).abs() < 1e-9, "back at the zoom it started from");
    }

    #[test]
    fn middle_drag_pans_with_any_tool() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        let ctx = egui::Context::default();
        let frame = |app: &mut crate::DesignApp, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
        };
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        // A drawing tool: a primary drag would make a frame, a middle drag must not.
        app.select_tool("rectangleFrame");
        let p = app.canvas_rect.unwrap().center();
        let v0 = *app.view().unwrap();
        let items = |app: &crate::DesignApp| app.session.active().unwrap().doc.spreads.iter().map(|s| s.items.len()).sum::<usize>();
        let items0 = items(&app);
        let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Middle, pressed, modifiers: Default::default() };
        frame(&mut app, vec![egui::Event::PointerMoved(p), button(p, true)]);
        for i in 1..=8 {
            frame(&mut app, vec![egui::Event::PointerMoved(p + egui::vec2(10.0 * i as f32, 5.0 * i as f32))]);
        }
        frame(&mut app, vec![button(p + egui::vec2(80.0, 40.0), false)]);
        let v = *app.view().unwrap();
        assert!((v.zoom - v0.zoom).abs() < 1e-9, "panning keeps the zoom");
        assert!(v.origin.x < v0.origin.x && v.origin.y < v0.origin.y, "dragging right/down moves the view: {v0:?} -> {v:?}");
        assert_eq!(items(&app), items0, "the tool saw nothing");
    }

    #[test]
    fn layout_menu_goes_through_pages_and_spreads_with_their_shortcuts() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({"pages": 7, "facingPages": true})).unwrap();
        let ctx = egui::Context::default();
        let frame = |app: &mut crate::DesignApp, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
        };
        let key = |k: egui::Key, modifiers: egui::Modifiers| {
            vec![
                egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers },
                egui::Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers },
            ]
        };
        let page = |app: &crate::DesignApp| crate::canvas::current_page(app).unwrap();
        let spread = |app: &crate::DesignApp| {
            let st = app.session.active().unwrap();
            crate::canvas::current_slot(app, &designcraft_tools::layout::CanvasLayout::new(&st.doc, false)).unwrap()
        };
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        // The Layout menu lists them, as InDesign's does, before Go to Page.
        let layout = MENUS.iter().find(|m| m.0 == "Layout").unwrap().1;
        let at = |id: &str| layout.iter().position(|e| e.starts_with(&format!("ui:{id}"))).unwrap();
        assert!(at("view.firstPage") < at("view.previousPage"));
        assert!(at("view.previousPage") < at("view.nextPage"));
        assert!(at("view.nextPage") < at("view.lastPage"));
        assert!(at("view.lastPage") < at("view.nextSpread"));
        assert!(at("view.nextSpread") < at("view.previousSpread"));
        assert!(at("view.previousSpread") < at("view.goToPage"));
        let shift = egui::Modifiers::SHIFT;
        let cmd_shift = egui::Modifiers { shift: true, command: true, ctrl: true, ..Default::default() };
        let alt = egui::Modifiers::ALT;
        // Pages 1 | 2-3 | 4-5 | 6-7.
        frame(&mut app, key(egui::Key::PageDown, shift));
        assert_eq!(page(&app), 1, "Next Page");
        frame(&mut app, key(egui::Key::PageDown, shift));
        assert_eq!(page(&app), 2, "Next Page goes to the right page of the spread");
        frame(&mut app, key(egui::Key::PageUp, shift));
        assert_eq!(page(&app), 1, "Previous Page");
        frame(&mut app, key(egui::Key::PageDown, cmd_shift));
        assert_eq!(page(&app), 6, "Last Page");
        frame(&mut app, key(egui::Key::PageDown, shift));
        assert_eq!(page(&app), 6, "Next Page stops at the last page");
        frame(&mut app, key(egui::Key::PageUp, cmd_shift));
        assert_eq!(page(&app), 0, "First Page");
        frame(&mut app, key(egui::Key::PageUp, shift));
        assert_eq!(page(&app), 0, "Previous Page stops at the first page");
        frame(&mut app, key(egui::Key::PageDown, alt));
        assert_eq!(spread(&app), 1, "Next Spread");
        frame(&mut app, key(egui::Key::PageDown, alt));
        frame(&mut app, key(egui::Key::PageDown, alt));
        frame(&mut app, key(egui::Key::PageDown, alt));
        assert_eq!(spread(&app), 3, "Next Spread stops at the last spread");
        frame(&mut app, key(egui::Key::PageUp, alt));
        assert_eq!(spread(&app), 2, "Previous Spread");
        // The spread is centred, the zoom kept.
        let zoom = app.view().unwrap().zoom;
        app.run("view.previousSpread", json!({})).unwrap();
        let st = app.session.active().unwrap();
        let slot = designcraft_tools::layout::CanvasLayout::new(&st.doc, false).slots[1];
        let (v, r) = (*app.view().unwrap(), app.canvas_rect.unwrap());
        let centre = (v.origin.x + r.width() as f64 / 2.0 / v.zoom, v.origin.y + r.height() as f64 / 2.0 / v.zoom);
        assert!((centre.0 - slot.bounds.center().x).abs() < 1e-6 && (centre.1 - slot.bounds.center().y).abs() < 1e-6);
        assert_eq!(v.zoom, zoom);
    }

    #[test]
    fn presentation_shows_one_spread_and_goes_through_them_with_keys_and_clicks() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({"pages": 7, "facingPages": true})).unwrap();
        app.session.execute("frame.create", &json!({"rect": [100, 100, 300, 300]})).unwrap();
        let id = app.session.active().unwrap().selection.items[0];
        let ctx = egui::Context::default();
        let frame = |app: &mut crate::DesignApp, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
        };
        let key = |k: egui::Key| {
            vec![
                egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() },
                egui::Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: Default::default() },
            ]
        };
        let click = |pos: egui::Pos2, modifiers: egui::Modifiers| {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers },
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers },
            ]
        };
        let spread = |app: &crate::DesignApp| {
            let st = app.session.active().unwrap();
            crate::canvas::current_slot(app, &designcraft_tools::layout::CanvasLayout::new(&st.doc, false)).unwrap()
        };
        // The spread fills the screen: as wide or as tall as it, centred.
        let fills = |app: &crate::DesignApp| {
            let st = app.session.active().unwrap();
            let b = designcraft_tools::layout::CanvasLayout::new(&st.doc, false).slots[spread(app)].bounds;
            let (v, r) = (*app.view().unwrap(), app.canvas_rect.unwrap());
            let (w, h) = (b.width() * v.zoom, b.height() * v.zoom);
            let x0 = (b.x0 - v.origin.x) * v.zoom;
            let y0 = (b.y0 - v.origin.y) * v.zoom;
            ((w - r.width() as f64).abs() < 0.01 || (h - r.height() as f64).abs() < 0.01)
                && w <= r.width() as f64 + 0.01
                && h <= r.height() as f64 + 0.01
                && (x0 * 2.0 + w - r.width() as f64).abs() < 0.01
                && (y0 * 2.0 + h - r.height() as f64).abs() < 0.01
        };
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        // The user's own view (zoomed in, scrolled) comes back after Presentation.
        let view = app.view_mut().unwrap();
        view.zoom = 2.0;
        view.origin.x += 150.0;
        view.origin.y += 300.0;
        let before = *app.view().unwrap();
        app.run("view.screenMode", json!({"mode": "presentation"})).unwrap();
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        assert_eq!(app.canvas_rect.unwrap().size(), egui::vec2(1400.0, 900.0), "the whole window");
        assert_eq!(spread(&app), 0);
        assert!(fills(&app));
        let bounds = app.session.active().unwrap().doc.item(id).unwrap().bounds();
        frame(&mut app, key(egui::Key::ArrowRight));
        frame(&mut app, vec![]);
        assert_eq!(spread(&app), 1, "Right Arrow shows the next spread");
        assert!(fills(&app));
        assert_eq!(app.session.active().unwrap().doc.item(id).unwrap().bounds(), bounds, "the arrow doesn't nudge the selection");
        frame(&mut app, key(egui::Key::PageDown));
        assert_eq!(spread(&app), 2, "Page Down");
        frame(&mut app, key(egui::Key::ArrowLeft));
        assert_eq!(spread(&app), 1, "Left Arrow");
        frame(&mut app, key(egui::Key::End));
        assert_eq!(spread(&app), 3, "End");
        frame(&mut app, key(egui::Key::ArrowRight));
        assert_eq!(spread(&app), 3, "the last spread stays");
        frame(&mut app, key(egui::Key::Home));
        assert_eq!(spread(&app), 0, "Home");
        frame(&mut app, key(egui::Key::PageUp));
        assert_eq!(spread(&app), 0, "the first spread stays");
        let c = app.canvas_rect.unwrap().center();
        frame(&mut app, click(c, egui::Modifiers::NONE));
        frame(&mut app, vec![]);
        assert_eq!(spread(&app), 1, "a click shows the next spread");
        assert!(fills(&app));
        frame(&mut app, click(c, egui::Modifiers::SHIFT));
        frame(&mut app, vec![]);
        assert_eq!(spread(&app), 0, "Shift-click the previous one");
        assert_eq!(app.session.active().unwrap().selection.items, vec![id], "clicks don't select");
        assert_eq!(app.session.tool_id(), "selection");
        // Out of Presentation, the view is the one it was entered from, and the arrow keys nudge
        // again.
        app.run("view.screenMode", json!({"mode": "normal"})).unwrap();
        assert_eq!(*app.view().unwrap(), before);
        frame(&mut app, vec![]);
        frame(&mut app, key(egui::Key::ArrowRight));
        assert_ne!(app.session.active().unwrap().doc.item(id).unwrap().bounds(), bounds);
    }

    #[test]
    fn tool_shortcuts_work_after_drawing_on_the_canvas() {
        // Clicking the canvas gives it keyboard focus; single-key tool shortcuts must still switch
        // tools without pressing Esc first (#1).
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        let ctx = egui::Context::default();
        let frame = |app: &mut crate::DesignApp, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
        };
        let key = |k: egui::Key| {
            vec![
                egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() },
                egui::Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: Default::default() },
            ]
        };
        let button = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        frame(&mut app, key(egui::Key::F));
        assert_eq!(app.session.tool_id(), "rectangleFrame");
        // Draw a frame.
        let a = app.canvas_rect.unwrap().center();
        let b = a + egui::vec2(120.0, 80.0);
        frame(&mut app, vec![egui::Event::PointerMoved(a), button(a, true)]);
        for i in 1..=4 {
            frame(&mut app, vec![egui::Event::PointerMoved(a + (b - a) * (i as f32 / 4.0))]);
        }
        frame(&mut app, vec![button(b, false)]);
        frame(&mut app, vec![]);
        assert!(app.session.active().is_some_and(|d| !d.selection.items.is_empty()), "drew a frame");
        frame(&mut app, key(egui::Key::V));
        assert_eq!(app.session.tool_id(), "selection", "V switches tools straight after drawing");
        frame(&mut app, key(egui::Key::F));
        assert_eq!(app.session.tool_id(), "rectangleFrame");
    }

    #[test]
    fn escape_and_shift_w_leave_presentation_for_the_previous_mode() {
        // Presentation hides every control, so the keyboard must get out of it (#493).
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        app.session.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "graphic"})).unwrap();
        let ctx = egui::Context::default();
        let frame = |app: &mut crate::DesignApp, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
        };
        let key = |k: egui::Key, modifiers: egui::Modifiers| {
            vec![
                egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers },
                egui::Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers },
            ]
        };
        let selected = |app: &crate::DesignApp| app.session.active().map(|d| d.selection.items.len()).unwrap_or(0);
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        app.run("view.screenMode", json!({"mode": "preview"})).unwrap();
        app.select_tool("rectangleFrame");
        assert_eq!(selected(&app), 1, "the new frame is selected");

        frame(&mut app, key(egui::Key::W, egui::Modifiers::SHIFT));
        assert_eq!(app.ui.screen_mode, ScreenMode::Presentation, "Shift+W enters Presentation");
        assert_eq!(app.session.tool_id(), "rectangleFrame", "Shift+W is not a tool shortcut");
        frame(&mut app, key(egui::Key::Escape, egui::Modifiers::NONE));
        assert_eq!(app.ui.screen_mode, ScreenMode::Preview, "Esc returns to the mode Presentation was entered from");
        assert_eq!(app.session.tool_id(), "rectangleFrame", "Esc only left Presentation");
        assert_eq!(selected(&app), 1, "Esc only left Presentation");

        frame(&mut app, key(egui::Key::W, egui::Modifiers::SHIFT));
        frame(&mut app, key(egui::Key::W, egui::Modifiers::SHIFT));
        assert_eq!(app.ui.screen_mode, ScreenMode::Preview, "Shift+W leaves Presentation for the previous mode");

        app.run("view.screenMode", json!({"mode": "presentation"})).unwrap();
        frame(&mut app, key(egui::Key::W, egui::Modifiers::NONE));
        assert_eq!(app.ui.screen_mode, ScreenMode::Normal, "W from any mode but Normal goes to Normal");

        // Outside Presentation, Esc keeps its usual meaning.
        frame(&mut app, key(egui::Key::Escape, egui::Modifiers::NONE));
        assert_eq!(app.ui.screen_mode, ScreenMode::Normal);
        assert_eq!(app.session.tool_id(), "selection");
    }

    #[test]
    fn cmd_plus_zooms_the_canvas_not_the_window() {
        // A German keyboard has its own + key, so Cmd+= arrives as Plus; it must zoom the canvas like
        // Cmd+- does, not hand the key to egui's whole-window zoom.
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        let ctx = egui::Context::default();
        let frame = |app: &mut crate::DesignApp, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
        };
        let key = |k: egui::Key, modifiers: egui::Modifiers| {
            vec![
                egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers },
                egui::Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers },
            ]
        };
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        let scale = ctx.zoom_factor();
        for (k, m, zooms_in) in [
            (egui::Key::Plus, egui::Modifiers::COMMAND, true),
            (egui::Key::Plus, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, true),
            (egui::Key::Equals, egui::Modifiers::COMMAND, true),
            (egui::Key::Minus, egui::Modifiers::COMMAND, false),
        ] {
            let z0 = app.view().unwrap().zoom;
            frame(&mut app, key(k, m));
            frame(&mut app, vec![]);
            let z = app.view().unwrap().zoom;
            assert!(if zooms_in { z > z0 } else { z < z0 }, "{k:?} {m:?}: {z0} -> {z}");
            assert_eq!(ctx.zoom_factor(), scale, "{k:?} {m:?} leaves the UI scale alone");
        }
        // With a native menu (macOS) its accelerator takes Cmd+= itself, but never sees the Plus key
        // standing in for = (#781).
        app.native_shortcuts.insert("view.zoomIn".to_string());
        for (k, m, zooms_in) in [
            (egui::Key::Plus, egui::Modifiers::COMMAND, true),
            (egui::Key::Plus, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, true),
            (egui::Key::Equals, egui::Modifiers::COMMAND, false),
        ] {
            let z0 = app.view().unwrap().zoom;
            frame(&mut app, key(k, m));
            frame(&mut app, vec![]);
            let z = app.view().unwrap().zoom;
            assert_eq!(z > z0, zooms_in, "native menu, {k:?} {m:?}: {z0} -> {z}");
        }
    }

    #[test]
    fn the_quick_apply_shortcut_opens_it_without_running_an_entry() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        let ctx = egui::Context::default();
        let frame = |app: &mut crate::DesignApp, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
        };
        let cmd_return =
            |pressed| egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::COMMAND };
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        frame(&mut app, vec![cmd_return(true), cmd_return(false)]);
        frame(&mut app, vec![]);
        assert_eq!(app.ui.palette.as_deref(), Some(""), "Quick Apply stays open");
        assert_eq!(app.session.documents().len(), 1, "the first entry (New Document) didn't run");
        // Pressing the shortcut again, or holding it until it repeats, doesn't run it either.
        frame(&mut app, vec![cmd_return(true), cmd_return(false)]);
        let repeat = egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: true, modifiers: egui::Modifiers::COMMAND };
        frame(&mut app, vec![repeat, cmd_return(false)]);
        assert_eq!(app.ui.palette.as_deref(), Some(""), "Quick Apply stays open");
        assert_eq!(app.session.documents().len(), 1, "a second ⌘Return or a repeat doesn't run the first entry");
        // A plain Return still does.
        let enter =
            |pressed| egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE };
        frame(&mut app, vec![enter(true), enter(false)]);
        assert_eq!(app.ui.palette, None, "Return runs the first entry and closes Quick Apply");
        assert_eq!(app.session.documents().len(), 2);
    }

    #[test]
    fn quick_apply_rows_are_left_aligned() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        app.ui.palette = Some(String::new());
        let ctx = egui::Context::default();
        let mut shapes = Vec::new();
        for _ in 0..3 {
            let input =
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
            shapes = out.shapes;
        }
        let label_x = |label: &str| {
            // The palette is painted last, over anything else with the same text.
            shapes.iter().rev().find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == label => Some(t.pos.x),
                _ => None,
            })
        };
        // Rows that show a shortcut or a style kind, and rows that show neither, among the first visible ones.
        let items = quick_apply_items(&app.session, "", false);
        let first = items.iter().take(10);
        let plain = first.clone().find(|it| it.shortcut.is_none() && it.kind.is_empty()).expect("a row without a shortcut");
        let with_sc = first.clone().find(|it| it.shortcut.is_some()).expect("a row with a shortcut");
        let (a, b) = (label_x(&plain.label).expect("plain row drawn"), label_x(&with_sc.label).expect("shortcut row drawn"));
        assert!((a - b).abs() < 0.5, "{:?} at x {a}, {:?} at x {b}", plain.label, with_sc.label);
    }

    #[test]
    fn copy_and_paste_keys_work_for_objects_without_a_native_menu() {
        // Without a native menu the paste key arrives only as a paste event, which needs text on
        // the system clipboard (#164, #185).
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        let ctx = egui::Context::default();
        let frame = |app: &mut crate::DesignApp, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
            out.platform_output.commands
        };
        let count = |app: &crate::DesignApp| app.session.active().unwrap().doc.spreads[0].items.len();
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        let id = app.session.execute("frame.create", &json!({"rect": [72, 72, 200, 160]})).unwrap()["id"].clone();
        app.session.execute("selection.set", &json!({"ids": [id]})).unwrap();
        let n = count(&app);
        let copied = frame(&mut app, vec![egui::Event::Copy])
            .into_iter()
            .find_map(|c| match c {
                egui::OutputCommand::CopyText(t) => Some(t),
                _ => None,
            })
            .expect("copying objects puts text on the system clipboard");
        assert!(!copied.is_empty());
        frame(&mut app, vec![egui::Event::Paste(copied.clone())]);
        assert_eq!(count(&app), n + 1, "the paste key pastes the copied rectangle");

        // Pasting into text anchors the copied object; the clipboard placeholder isn't typed.
        let r = app.session.execute("frame.create", &json!({"rect": [72, 300, 400, 400], "content": "text", "text": "Hello"})).unwrap();
        let story = r["story"].as_u64().unwrap();
        app.session.execute("tool.select", &json!({"tool": "type"})).unwrap();
        app.session.execute("text.select", &json!({"story": story, "anchor": 5, "focus": 5})).unwrap();
        frame(&mut app, vec![egui::Event::Paste(copied.clone())]);
        // Shift pastes without formatting.
        frame(&mut app, vec![egui::Event::ModifiersChanged(egui::Modifiers::SHIFT), egui::Event::Paste(copied)]);
        let text = app.session.active().unwrap().doc.story(designcraft_doc::StoryId(story)).unwrap().text.clone();
        assert!(text.starts_with("Hello") && !text.contains('\u{FFFC}'), "{text:?}");
        assert!(text.contains(designcraft_doc::OBJECT_MARK), "{text:?}");
    }

    #[test]
    fn paste_with_no_insertion_point_makes_a_text_frame_from_other_apps_text() {
        // #518: text copied in another app pastes as a new, selected text frame; the text an
        // object copy stands for still pastes the objects. One paste each way, with and without
        // a native menu.
        let selected_text = |app: &crate::DesignApp| {
            let d = app.session.active().unwrap();
            let it = d.doc.item(*d.selection.items.first()?)?;
            Some(d.doc.story(it.text_frame()?.story)?.text.clone())
        };
        let ctx = egui::Context::default();
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        native_frame(&mut app, &ctx, vec![], None);
        native_frame(&mut app, &ctx, vec![], None);
        // The paste keys without a native menu: a paste event.
        native_frame(&mut app, &ctx, vec![egui::Event::Paste("From elsewhere".into())], None);
        assert_eq!(document_items(&app), (1, 1), "pasted once");
        assert_eq!(selected_text(&app).as_deref(), Some("From elsewhere"));
        let id = app.session.execute("frame.create", &json!({"rect": [72, 72, 200, 160]})).unwrap()["id"].clone();
        app.session.execute("selection.set", &json!({"ids": [id]})).unwrap();
        let copied = native_frame(&mut app, &ctx, vec![egui::Event::Copy], None)
            .platform_output
            .commands
            .into_iter()
            .find_map(|c| match c {
                egui::OutputCommand::CopyText(t) => Some(t),
                _ => None,
            })
            .expect("copying objects puts text on the system clipboard");
        native_frame(&mut app, &ctx, vec![egui::Event::Paste(copied)], None);
        assert_eq!(document_items(&app), (3, 1));
        assert_eq!(selected_text(&app), None, "the copied rectangle, not a text frame");

        // A native menu: Paste reads the system clipboard.
        let clip = std::rc::Rc::new(std::cell::RefCell::new(Some("Menu text".to_string())));
        let read = clip.clone();
        let services = crate::Services { clipboard_text: Some(Box::new(move || read.borrow().clone())), ..Default::default() };
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), services);
        app.native_menu = true;
        app.session.execute("file.new", &json!({})).unwrap();
        native_frame(&mut app, &ctx, vec![], None);
        native_frame(&mut app, &ctx, vec![], None);
        native_frame(&mut app, &ctx, vec![], Some("edit.paste"));
        native_frame(&mut app, &ctx, vec![], None);
        assert_eq!(document_items(&app), (1, 1), "pasted once");
        assert_eq!(selected_text(&app).as_deref(), Some("Menu text"));
        // Objects copied since that text was copied paste as objects.
        let id = app.session.execute("frame.create", &json!({"rect": [72, 72, 200, 160]})).unwrap()["id"].clone();
        app.session.execute("selection.set", &json!({"ids": [id]})).unwrap();
        native_frame(&mut app, &ctx, vec![], Some("edit.copy"));
        native_frame(&mut app, &ctx, vec![], Some("edit.paste"));
        assert_eq!(document_items(&app), (3, 1));
        assert_eq!(selected_text(&app), None, "the copied rectangle, not a text frame");
        // Newer text from another app wins.
        *clip.borrow_mut() = Some("Newer".into());
        native_frame(&mut app, &ctx, vec![], Some("edit.paste"));
        assert_eq!(document_items(&app), (4, 1));
        assert_eq!(selected_text(&app).as_deref(), Some("Newer"));
    }

    #[test]
    fn sample_scripts_run() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        for (name, text) in app.ui.scripts.clone() {
            let r = app.session.execute("script.run", &json!({"text": text}));
            assert!(r.is_ok(), "{name}: {r:?}");
        }
    }

    #[test]
    fn new_window_shows_a_second_view() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        let ctx = egui::Context::default();
        let frame = |app: &mut crate::DesignApp| {
            let input =
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
        };
        frame(&mut app);
        assert_eq!(run_ui(&mut app, "window.newWindow", &json!({})).unwrap().unwrap(), json!(true));
        assert!(!app.split);
        frame(&mut app);
        frame(&mut app);
        // Pane 1 was drawn (its rect is held for it) and has its own view.
        assert!(app.other_pane.as_ref().and_then(|o| o.1).is_some() || app.pane == 1);
        app.switch_pane(1);
        assert!(app.view().is_some(), "the second window has a view");
        app.switch_pane(0);
        assert_eq!(checked(&app, "window.newWindow", &Value::Null), Some(true));
        run_ui(&mut app, "window.newWindow", &json!({"on": false})).unwrap().unwrap();
        frame(&mut app);
    }

    #[test]
    fn panels_float_and_dock() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let r = run_ui(&mut app, "window.floatPanel", &json!({"panel": "swatches", "x": 50, "y": 60})).unwrap().unwrap();
        assert_eq!(r["floating"], json!(["swatches"]));
        assert_eq!(app.ui.floating[0].1, [50.0, 60.0]);
        // Showing a floating panel doesn't also open its flyout.
        run_ui(&mut app, "window.panel", &json!({"panel": "swatches"})).unwrap().unwrap();
        assert_eq!(app.ui.open_panel, None);
        run_ui(&mut app, "window.dockPanel", &json!({"panel": "swatches"})).unwrap().unwrap();
        assert!(app.ui.floating.is_empty());
        assert!(run_ui(&mut app, "window.floatPanel", &json!({"panel": "nope"})).unwrap().is_err());
    }

    /// One frame as the desktop app runs it: queued synthetic input, then the native menu item
    /// chosen during the frame (`menu`), then the app's logic and ui.
    fn native_frame(app: &mut crate::DesignApp, ctx: &egui::Context, events: Vec<egui::Event>, menu: Option<&str>) -> egui::FullOutput {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))),
            events,
            ..Default::default()
        };
        app.raw_input_hook(&mut input);
        let mut menu = menu;
        let mut out = ctx.run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            if let Some(id) = menu.take() {
                activate_native(app, &ctx, id, &Value::Null);
            }
            app.logic(&ctx);
            app.ui(ui);
        });
        out.textures_delta.clear();
        out
    }

    /// A document with two frames, nothing selected, and Quick Apply's search field focused
    /// holding `text`.
    fn app_with_focused_field(ctx: &egui::Context, text: &str) -> crate::DesignApp {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        app.session.execute("frame.create", &json!({"rect": [36, 36, 200, 200]})).unwrap();
        app.session.execute("frame.create", &json!({"rect": [236, 36, 400, 200]})).unwrap();
        app.session.execute("edit.deselectAll", &json!({})).unwrap();
        app.ui.palette = Some(text.into());
        for _ in 0..3 {
            native_frame(&mut app, ctx, vec![], None);
        }
        assert!(ctx.text_edit_focused());
        app
    }

    fn field_selection(ctx: &egui::Context) -> Option<(usize, usize)> {
        let id = ctx.memory(|m| m.focused())?;
        let r = egui::TextEdit::load_state(ctx, id)?.cursor.char_range()?;
        Some((r.primary.index.min(r.secondary.index).into(), r.primary.index.max(r.secondary.index).into()))
    }

    fn document_items(app: &crate::DesignApp) -> (usize, usize) {
        let d = app.session.active().unwrap();
        (d.doc.spreads.iter().flat_map(|sp| &sp.items).count(), d.selection.items.len())
    }

    #[test]
    fn native_select_all_selects_the_focused_fields_text() {
        // The Mac menu bar takes ⌘A before the window sees it (#30).
        let ctx = egui::Context::default();
        let mut app = app_with_focused_field(&ctx, "frame");
        native_frame(&mut app, &ctx, vec![], Some("edit.selectAll"));
        native_frame(&mut app, &ctx, vec![], None);
        assert_eq!(field_selection(&ctx), Some((0, 5)), "the field's whole text is selected");
        assert_eq!(document_items(&app), (2, 0), "the document selection is unchanged");
    }

    #[test]
    fn native_paste_and_copy_act_on_the_focused_field() {
        let ctx = egui::Context::default();
        let mut app = app_with_focused_field(&ctx, "frame");
        native_frame(&mut app, &ctx, vec![], Some("edit.selectAll"));
        native_frame(&mut app, &ctx, vec![], None);
        // Copy puts the field's selected text on the clipboard.
        native_frame(&mut app, &ctx, vec![], Some("edit.copy"));
        let out = native_frame(&mut app, &ctx, vec![], None);
        assert!(out.platform_output.commands.contains(&egui::OutputCommand::CopyText("frame".into())), "{:?}", out.platform_output.commands);
        // Paste asks the integration for the clipboard, which it hands to the field as a paste.
        let out = native_frame(&mut app, &ctx, vec![], Some("edit.paste"));
        let commands = &out.viewport_output[&egui::ViewportId::ROOT].commands;
        assert!(commands.contains(&egui::ViewportCommand::RequestPaste), "{commands:?}");
        native_frame(&mut app, &ctx, vec![egui::Event::Paste("text".into())], None);
        assert_eq!(app.ui.palette.as_deref(), Some("text"));
        assert_eq!(document_items(&app), (2, 0), "nothing was pasted into the document");
    }

    #[test]
    fn native_copy_and_paste_use_the_system_clipboard_for_canvas_text() {
        let ctx = egui::Context::default();
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        app.session.execute("frame.create", &json!({"rect": [36, 36, 200, 200], "content": "text", "text": "hello"})).unwrap();
        let story = *app.session.active().unwrap().doc.stories.keys().next().unwrap();
        app.select_tool("type");
        app.run("text.select", json!({"story": story.0, "anchor": 0, "focus": 5})).unwrap();
        for _ in 0..3 {
            native_frame(&mut app, &ctx, vec![], None);
        }
        assert!(app.session.wants_text());

        native_frame(&mut app, &ctx, vec![], Some("edit.copy"));
        let copied = native_frame(&mut app, &ctx, vec![], None);
        assert!(copied.platform_output.commands.contains(&egui::OutputCommand::CopyText("hello".into())));

        let paste = native_frame(&mut app, &ctx, vec![], Some("edit.paste"));
        let commands = &paste.viewport_output[&egui::ViewportId::ROOT].commands;
        assert!(commands.contains(&egui::ViewportCommand::RequestPaste), "{commands:?}");
        native_frame(&mut app, &ctx, vec![egui::Event::Paste("world".into())], None);
        assert_eq!(app.session.active().unwrap().doc.story(story).unwrap().text, "world");
    }

    #[test]
    fn native_undo_and_redo_act_on_the_focused_field() {
        let ctx = egui::Context::default();
        let mut app = app_with_focused_field(&ctx, "");
        native_frame(&mut app, &ctx, vec![egui::Event::Text("abc".into())], None);
        assert_eq!(app.ui.palette.as_deref(), Some("abc"));
        native_frame(&mut app, &ctx, vec![], Some("edit.undo"));
        native_frame(&mut app, &ctx, vec![], None);
        assert_eq!(app.ui.palette.as_deref(), Some(""), "the field's typing is undone");
        assert_eq!(document_items(&app), (2, 0), "the document's last frame is still there");
        native_frame(&mut app, &ctx, vec![], Some("edit.redo"));
        native_frame(&mut app, &ctx, vec![], None);
        assert_eq!(app.ui.palette.as_deref(), Some("abc"));
        assert_eq!(document_items(&app), (2, 0));
    }

    #[test]
    fn native_editing_items_are_enabled_for_a_focused_field() {
        let ctx = egui::Context::default();
        let mut app = app_with_focused_field(&ctx, "frame");
        // Nothing is selected in the document, so its Copy and Cut are off; the field's are on.
        assert!(!menu_enabled(&app, "edit.copy") && !menu_enabled(&app, "edit.cut"));
        for id in ["edit.selectAll", "edit.copy", "edit.cut", "edit.paste", "edit.undo", "edit.redo"] {
            assert!(native_menu_enabled(&app, &ctx, id), "{id}");
        }
        assert!(!native_menu_enabled(&app, &ctx, "edit.duplicate"), "other items follow the document");
        app.ui.palette = None;
        native_frame(&mut app, &ctx, vec![], None);
        native_frame(&mut app, &ctx, vec![], None);
        assert!(!native_menu_enabled(&app, &ctx, "edit.copy"), "without a focused field, as the document says");
    }

    #[test]
    fn native_select_all_without_a_focused_field_selects_the_documents_items() {
        let ctx = egui::Context::default();
        let mut app = app_with_focused_field(&ctx, "frame");
        app.ui.palette = None;
        native_frame(&mut app, &ctx, vec![], None);
        native_frame(&mut app, &ctx, vec![], None);
        assert!(!ctx.text_edit_focused());
        native_frame(&mut app, &ctx, vec![], Some("edit.selectAll"));
        assert_eq!(document_items(&app), (2, 2));
    }

    #[test]
    fn command_search_matches_labels_in_the_interface_language() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.language = "de".into();
        let items = command_search(&app, "gruppieren");
        let group = items.iter().find(|i| i.id == "object.group").unwrap();
        assert_eq!(group.label, "Gruppieren");
        assert_eq!(group.shortcut.as_deref(), Some("Cmd+G"));
        assert!(command_search(&app, "group").iter().any(|i| i.id == "object.group"), "English labels match too");
        assert!(command_search(&app, "  ").is_empty());
        assert!(command_search(&app, "e").len() <= 14);
    }

    #[test]
    fn quick_apply_lists_styles_then_commands() {
        let mut s = designcraft_engine::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Body Copy"})).unwrap();
        let items = quick_apply_items(&s, "body", false);
        assert_eq!(items[0].id, "style.paragraph.apply");
        assert_eq!(items[0].params, json!({"name": "Body Copy"}));
        assert_eq!(items[0].kind, "Paragraph Style");
        assert!(quick_apply_items(&s, "", false).iter().all(|i| i.kind.is_empty()));
        assert!(quick_apply_items(&s, "preferences", false).iter().any(|i| i.id == "app.preferences"));
    }

    #[test]
    fn distribute_rows_menu_dispatches_an_undoable_edit() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let frame = app.run("frame.create", json!({"rect": [72, 72, 500, 700], "content": "text", "text": ""})).unwrap();
        app.run("text.select", json!({"story": frame["story"], "anchor": 0, "focus": 0})).unwrap();
        app.run("table.insert", json!({"rows": 2, "cols": 2})).unwrap();
        app.run("table.setRowHeight", json!({"row": 0, "height": 40, "mode": "exactly"})).unwrap();
        app.run("table.setRowHeight", json!({"row": 1, "height": 80, "mode": "exactly"})).unwrap();
        app.run("table.selectTable", json!({})).unwrap();
        activate(&mut app, "table.distributeRows", &Value::Null);
        assert!(app.ui.dialog.is_none());
        let info = app.run("table.get", json!({})).unwrap();
        assert_eq!(info["rows"][0]["height"], 60.0);
        assert_eq!(info["rows"][1]["height"], 60.0);
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(app.run("table.get", json!({})).unwrap()["rows"][0]["height"], 40.0);
    }

    #[test]
    fn every_menu_entry_is_a_command() {
        let mut all = Vec::new();
        for (_, items) in menu_tree() {
            walk(&items, &mut all);
        }
        assert!(all.len() > 80, "{}", all.len());
        assert!(all.iter().any(|(label, id, _)| label == "Distribute Rows Evenly" && id == "table.distributeRows"));
        for (label, id, params) in &all {
            assert!(ui_label(id).is_some() || designcraft_engine::find_command(id).is_some(), "menu entry {label}: unknown command {id}");
            assert!(!label.is_empty() && label != id, "menu entry {id} has no label");
            if let Some(o) = params.as_object() {
                assert!(!o.is_empty(), "{id}: empty fixed params");
            }
        }
        // Nested submenus parse (Type › Insert Special Character › Symbols › …).
        let ty = menu_tree().into_iter().find(|(m, _)| *m == "Type").unwrap().1;
        let special = ty.iter().find_map(|i| match i {
            Item::Sub(n, c) if n == "Insert Special Character" => Some(c.clone()),
            _ => None,
        });
        assert!(special.is_some_and(|c| c.iter().any(|i| matches!(i, Item::Sub(n, _) if n == "Symbols"))));
        // Fixed-parameter variants keep their own labels.
        assert!(all.iter().any(|(l, id, _)| l == "Fill Frame Proportionally" && id == "fit.fillFrameProportionally"));
    }

    #[test]
    fn context_menus_hold_commands_and_menu_bar_submenus() {
        let tree = menu_tree();
        for entries in [CONTEXT_OBJECT, CONTEXT_GRAPHIC_FRAME, CONTEXT_OBJECT_TAIL, CONTEXT_TEXT_FRAME, CONTEXT_TEXT, CONTEXT_CANVAS] {
            for path in entries.iter().filter_map(|e| e.strip_prefix("menu:")) {
                assert!(menu_bar_submenu(&tree, path).is_some(), "no menu bar submenu {path}");
            }
            let mut all = Vec::new();
            for (_, items) in context_parse(entries) {
                walk(&items, &mut all);
            }
            for (label, id, _) in &all {
                assert!(ui_label(id).is_some() || designcraft_engine::find_command(id).is_some(), "context entry {label}: unknown command {id}");
            }
        }
    }

    #[test]
    fn the_type_menu_follows_indesigns_order() {
        // InDesign's Type menu, as Chris Nielsen opens it in "Numbering Page Sections" (2:09).
        let ty = menu_tree().into_iter().find(|(m, _)| *m == "Type").unwrap().1;
        let labels: Vec<String> = ty
            .iter()
            .map(|i| match i {
                Item::Sep => "-".to_string(),
                Item::Sub(n, _) => format!("{n} >"),
                Item::Cmd { label, .. } => label.clone(),
            })
            .collect();
        let expected = [
            "Size >",
            "-",
            "Character",
            "Paragraph",
            "Tabs",
            "Glyphs",
            "-",
            "Character Styles",
            "Paragraph Styles",
            "-",
            "Create Outlines",
            "Find/Replace Font…",
            "Change Case >",
            "-",
            "Type on a Path >",
            "-",
            "Notes >",
            "Track Changes >",
            "-",
            "Insert Footnote",
            "Document Footnote Options…",
            "Insert Endnote",
        ];
        assert_eq!(&labels[..expected.len()], expected, "{labels:?}");
        let pos = |l: &str| labels.iter().position(|x| x == l).unwrap_or_else(|| panic!("{l} in {labels:?}"));
        assert!(pos("Hyperlinks & Cross-References >") < pos("Text Variables >"));
        assert!(pos("Text Variables >") < pos("Bulleted & Numbered Lists >"));
        assert!(pos("Bulleted & Numbered Lists >") < pos("Insert Special Character >"));
        assert!(pos("Insert Special Character >") < pos("Insert White Space >"));
        assert!(pos("Insert Break Character >") < pos("Fill with Placeholder Text"));
        assert!(pos("Fill with Placeholder Text") < pos("Show Hidden Characters"));
        // Alignment, styles and point sizes keep their shortcuts, out of the menu as in InDesign.
        for gone in ["Align Left", "Bold", "Increase Point Size", "Paragraph Rules…"] {
            assert!(!labels.iter().any(|l| l == gone), "{gone}");
        }
        let app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let all = effective_shortcuts(&app);
        for (id, sc) in [
            ("type.alignLeft", "Cmd+Shift+L"),
            ("app.characterPanel", "Cmd+T"),
            ("app.paragraphPanel", "Cmd+Alt+T"),
            ("app.insertCurrentPageNumber", "Cmd+Alt+Shift+N"),
        ] {
            assert_eq!(all.iter().find(|(i, _)| i == id).map(|(_, s)| s.as_str()), Some(sc), "{id}");
        }
        // Markers › Current Page Number carries ⌥⇧⌘N.
        let markers = ty
            .iter()
            .find_map(|i| match i {
                Item::Sub(n, c) if n == "Insert Special Character" => {
                    c.iter().find_map(|i| if let Item::Sub(n, c) = i { (n == "Markers").then(|| c.clone()) } else { None })
                }
                _ => None,
            })
            .unwrap();
        assert!(matches!(&markers[0], Item::Cmd { label, shortcut: Some("Cmd+Alt+Shift+N"), .. } if label == "Current Page Number"));
    }

    #[test]
    fn type_panel_items_open_their_panels_and_the_marker_shortcut_inserts_a_page_number() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        activate(&mut app, "app.characterPanel", &Value::Null);
        assert_eq!(app.ui.open_panel.as_deref(), Some("character"));
        // Paragraph Styles opens floating, grouped with Character Styles, as in InDesign.
        activate(&mut app, "app.paragraphStylesPanel", &Value::Null);
        let layout = app.ui.docking.as_ref().unwrap();
        assert!(layout.floating.iter().any(|g| g.panels == ["characterStyles", "paragraphStyles"] && g.active == 1));
        let story = app.run("frame.create", json!({"rect": [36, 36, 200, 100], "content": "text", "text": "Page "})).unwrap()["story"].clone();
        app.run("text.select", json!({"story": story, "anchor": 5, "focus": 5})).unwrap();
        activate(&mut app, "app.insertCurrentPageNumber", &Value::Null);
        let d = &app.session.doc().unwrap().doc;
        assert_eq!(d.story(designcraft_doc::StoryId(story.as_u64().unwrap())).unwrap().text, "Page \u{e000}");
    }

    #[test]
    fn custom_workspaces_save_apply_and_delete() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.dock_tab = "swatches".into();
        app.ui.control_bar = true;
        run_ui(&mut app, "window.newWorkspace", &json!({"name": "Mine"})).unwrap().unwrap();
        run_ui(&mut app, "window.workspace", &json!({"name": "Essentials"})).unwrap().unwrap();
        app.ui.dock_tab = "properties".into();
        run_ui(&mut app, "window.workspace", &json!({"name": "Mine"})).unwrap().unwrap();
        assert_eq!((app.ui.dock_tab.as_str(), app.ui.control_bar, app.ui.workspace.as_str()), ("swatches", true, "Mine"));
        app.ui.dock_tab = "layers".into();
        run_ui(&mut app, "window.resetWorkspace", &json!({})).unwrap().unwrap();
        assert_eq!(app.ui.dock_tab, "swatches", "reset to the saved arrangement");
        run_ui(&mut app, "window.deleteWorkspace", &json!({"name": "Mine"})).unwrap().unwrap();
        assert!(app.ui.custom_workspaces.is_empty());
        assert_eq!(app.ui.workspace, "Essentials");
    }

    /// New Workspace › Capture (Learn Skills Daily's InDesign course, 11:40): Panel Locations and
    /// Menu Customization, both on by default.
    #[test]
    fn new_workspace_captures_panel_locations_and_menu_customization() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        // The dialog offers both, checked.
        run_ui(&mut app, "window.newWorkspace", &json!({})).unwrap().unwrap();
        let d = app.ui.dialog.take().unwrap();
        assert_eq!(
            (d.id.as_str(), d.fields["panelLocations"].clone(), d.fields["menuCustomization"].clone()),
            ("newWorkspace", json!(true), json!(true))
        );
        // Both captured.
        app.ui.dock_tab = "swatches".into();
        app.ui.hidden_menu_items = vec!["Type/Glyphs".into()];
        run_ui(&mut app, "window.newWorkspace", &json!({"name": "All"})).unwrap().unwrap();
        // Menus only: choosing it leaves the panels where they are.
        app.ui.hidden_menu_items = vec!["Layout/Pages".into()];
        run_ui(&mut app, "window.newWorkspace", &json!({"name": "Menus", "panelLocations": false})).unwrap().unwrap();
        // Panels only: the menus stay as they are.
        app.ui.dock_tab = "layers".into();
        run_ui(&mut app, "window.newWorkspace", &json!({"name": "Panels", "menuCustomization": false})).unwrap().unwrap();
        app.ui.dock_tab = "properties".into();
        app.ui.hidden_menu_items.clear();
        run_ui(&mut app, "window.workspace", &json!({"name": "Menus"})).unwrap().unwrap();
        assert_eq!((app.ui.dock_tab.as_str(), app.ui.hidden_menu_items.clone()), ("properties", vec!["Layout/Pages".to_string()]));
        run_ui(&mut app, "window.workspace", &json!({"name": "Panels"})).unwrap().unwrap();
        assert_eq!((app.ui.dock_tab.as_str(), app.ui.hidden_menu_items.clone()), ("layers", vec!["Layout/Pages".to_string()]));
        run_ui(&mut app, "window.workspace", &json!({"name": "All"})).unwrap().unwrap();
        assert_eq!((app.ui.dock_tab.as_str(), app.ui.hidden_menu_items.clone()), ("swatches", vec!["Type/Glyphs".to_string()]));
        // A workspace saved before the options existed restores its panels.
        let old: crate::SavedWorkspace = serde_json::from_value(json!({"name": "Old", "dockTab": "pages"})).unwrap();
        assert!(!old.keep_panels && old.hidden_menu_items.is_none());
    }

    #[test]
    fn essentials_classic_is_the_control_panel_and_iconic_groups() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        run_ui(&mut app, "window.workspace", &json!({"name": "Essentials Classic"})).unwrap().unwrap();
        assert!(app.ui.control_bar && !app.ui.dock_expanded);
        let groups = app.ui.icon_groups.clone().unwrap();
        assert_eq!(groups[0], ["pages", "layers", "links"]);
        assert_eq!(groups.last().unwrap(), &["gradient"]);
        assert_eq!(checked(&app, "window.workspace", &json!({"name": "Essentials Classic"})), Some(true));
        assert_eq!(checked(&app, "window.workspace", &json!({"name": "Essentials"})), Some(false));
        // Window › Pages opens Pages from its icon group, not the Properties stack.
        run_ui(&mut app, "window.panel", &json!({"panel": "pages"})).unwrap().unwrap();
        assert_eq!((app.ui.open_panel.as_deref(), app.ui.dock_expanded), (Some("pages"), false));
        // A workspace saved from it keeps the groups.
        run_ui(&mut app, "window.newWorkspace", &json!({"name": "Mine"})).unwrap().unwrap();
        run_ui(&mut app, "window.workspace", &json!({"name": "Essentials"})).unwrap().unwrap();
        assert_eq!((app.ui.icon_groups.as_ref(), app.ui.dock_expanded, app.ui.control_bar), (None, true, false));
        run_ui(&mut app, "window.workspace", &json!({"name": "Mine"})).unwrap().unwrap();
        assert_eq!(app.ui.icon_groups, Some(groups));
    }

    #[test]
    fn typography_workspace_opens_its_panels_from_named_icon_groups_and_resets() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        run_ui(&mut app, "window.workspace", &json!({"name":"Typography"})).unwrap().unwrap();
        assert!(app.ui.control_bar);
        assert!(!app.ui.dock_expanded);
        assert!(app.ui.open_panel.is_none());
        let groups = app.ui.icon_groups.clone().unwrap();
        assert_eq!(groups[0], ["pages", "layers", "swatches"]);
        assert_eq!(groups[4], ["paragraph", "paragraphStyles"]);
        assert_eq!(groups[5], ["character", "characterStyles"]);
        for panel in ["pages", "paragraphStyles", "characterStyles"] {
            run_ui(&mut app, "window.panel", &json!({"panel":panel})).unwrap().unwrap();
            assert_eq!(app.ui.open_panel.as_deref(), Some(panel));
            assert!(!app.ui.dock_expanded);
        }
        run_ui(&mut app, "window.resetWorkspace", &json!({})).unwrap().unwrap();
        assert_eq!(app.ui.icon_groups, Some(groups));
        assert!(app.ui.open_panel.is_none());
        assert!(!app.ui.dock_expanded);
        run_ui(&mut app, "window.workspace", &json!({"name":"Essentials"})).unwrap().unwrap();
        assert!(app.ui.icon_groups.is_none());
        assert!(app.ui.dock_expanded);
    }

    #[test]
    fn the_window_menu_is_in_indesigns_order_and_reaches_every_panel() {
        let window = menu_tree().into_iter().find(|(m, _)| *m == "Window").unwrap().1;
        let names: Vec<&str> = window
            .iter()
            .filter_map(|i| match i {
                Item::Sub(n, _) => Some(n.as_str()),
                Item::Cmd { label, .. } => Some(label.as_str()),
                Item::Sep => None,
            })
            .collect();
        assert_eq!(&names[..4], ["Arrange", "Workspace", "Articles", "Color"]);
        let sub = |name: &str| {
            window
                .iter()
                .find_map(|i| match i {
                    Item::Sub(n, c) if n == name => Some(c.clone()),
                    _ => None,
                })
                .unwrap()
        };
        // Window › Workspace › Essentials Classic, Window › Color › Gradient & Swatches.
        assert!(
            sub("Workspace")
                .iter()
                .any(|i| matches!(i, Item::Cmd { id, params, .. } if id == "window.workspace" && params["name"] == "Essentials Classic"))
        );
        let color: Vec<String> =
            sub("Color").iter().filter_map(|i| if let Item::Cmd { label, .. } = i { Some(label.clone()) } else { None }).collect();
        assert_eq!(color, ["Color", "Gradient", "Swatches"]);
        // Every panel is in the menu.
        fn panels(items: &[Item], out: &mut Vec<String>) {
            for i in items {
                match i {
                    Item::Sub(_, c) => panels(c, out),
                    Item::Cmd { id, params, .. } if id == "window.panel" => out.extend(params["panel"].as_str().map(str::to_string)),
                    _ => {}
                }
            }
        }
        let mut found = Vec::new();
        panels(&window, &mut found);
        for (id, _, _) in crate::dock::DOCK_TABS.iter().chain(crate::dock::ICON_PANELS) {
            assert!(found.iter().any(|p| p == id), "Window menu misses the {id} panel");
        }
    }

    #[test]
    fn interactive_workspaces_show_their_panels() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.dock_expanded = false;
        run_ui(&mut app, "window.workspace", &json!({"name": "Interactive for PDF"})).unwrap().unwrap();
        assert_eq!((app.ui.dock_tab.as_str(), app.ui.dock_expanded, app.ui.open_panel.as_deref()), ("pages", true, Some("buttons")));
        run_ui(&mut app, "window.workspace", &json!({"name": "Digital Publishing"})).unwrap().unwrap();
        assert_eq!((app.ui.dock_tab.as_str(), app.ui.open_panel.as_deref()), ("pages", Some("liquid")));
        run_ui(&mut app, "window.workspace", &json!({"name": "Essentials"})).unwrap().unwrap();
        assert_eq!(app.ui.open_panel, None);
    }

    #[test]
    fn hide_and_show_menu_items() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let items = all_menu_items();
        let (menu, label) = items.iter().find(|(m, _)| m == "Edit").cloned().expect("Edit items");
        let key = menu_key(&menu, &label);
        run_ui(&mut app, "window.hideMenuItem", &json!({"item": key})).unwrap().unwrap();
        assert!(app.ui.hidden_menu_items.contains(&key));
        run_ui(&mut app, "window.hideMenuItem", &json!({"item": key, "hidden": false})).unwrap().unwrap();
        assert!(app.ui.hidden_menu_items.is_empty());
        assert!(items.len() > 100, "every menu's items are listed: {}", items.len());
    }

    /// The Type menu as drawn: every item label.
    fn drawn_type_menu(app: &mut DesignApp) -> Vec<String> {
        let ty = menu_tree().into_iter().find(|(m, _)| *m == "Type").unwrap().1;
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 4000.0));
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
            menu_items(app, ui, &ty, "Type");
        });
        out.textures_delta.clear();
        fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.job.text.trim().to_string()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
                _ => {}
            }
        }
        let mut labels = Vec::new();
        out.shapes.iter().for_each(|s| collect(&s.shape, &mut labels));
        labels
    }

    /// Tate-Chu-Yoko, Ruby, Kenten and Warichu are in the Type menu of InDesign's Japanese edition
    /// only: they show while Show CJK Features is on, which follows the interface language until
    /// the user sets it. Hidden, they still run.
    #[test]
    fn cjk_menu_items_follow_show_cjk_features() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let ty = menu_tree().into_iter().find(|(m, _)| *m == "Type").unwrap().1;
        let cjk: Vec<String> = ty
            .iter()
            .filter_map(|i| match i {
                Item::Cmd { label, id, .. } if CJK_ONLY.contains(&id.as_str()) => Some(label.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(cjk.len(), 4, "{cjk:?}");
        let drawn = |app: &mut DesignApp| {
            let labels = drawn_type_menu(app);
            assert!(labels.iter().any(|l| l == crate::i18n::tr(&app.ui.language, "Story Direction")), "in both editions: {labels:?}");
            let shown: Vec<bool> = cjk.iter().map(|l| labels.iter().any(|d| d == crate::i18n::tr(&app.ui.language, l))).collect();
            assert!(shown.iter().all(|s| *s == shown[0]), "{shown:?}");
            shown[0]
        };
        for (lang, on) in [("", false), ("de", false), ("ja", true), ("zh", true), ("ko", true)] {
            app.ui.language = lang.into();
            assert_eq!(crate::cjk_features(&app), on, "{lang}");
            assert_eq!(drawn(&mut app), on, "{lang}");
        }
        app.run("prefs.set", json!({"cjkFeatures": false})).unwrap();
        assert!(!drawn(&mut app), "off in a Korean interface");
        app.ui.language = String::new();
        app.run("prefs.set", json!({"cjkFeatures": true})).unwrap();
        assert!(drawn(&mut app), "on in an English interface");
        assert!(quick_apply_items(&app.session, "ruby", crate::cjk_features(&app)).iter().any(|i| i.id == "app.rubyDialog"));
        app.run("prefs.set", json!({"cjkFeatures": false})).unwrap();
        assert!(!drawn(&mut app));
        assert!(!quick_apply_items(&app.session, "ruby", crate::cjk_features(&app)).iter().any(|i| i.id == "app.rubyDialog"));
        app.run("app.rubyDialog", json!({})).unwrap();
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("ruby"), "hidden, not removed");
    }

    /// Preferences › Type › Show CJK Features: left as it is, it keeps following the interface
    /// language; changed, it is kept in prefs.json.
    #[test]
    fn show_cjk_features_is_a_persisted_preference() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.language = "ja".into();
        run_ui(&mut app, "app.preferences", &json!({})).unwrap().unwrap();
        assert_eq!(app.ui.dialog.as_ref().unwrap().fields["cjkFeatures"], true);
        crate::dialogs::confirm(&mut app).unwrap();
        assert_eq!(app.session.prefs.cjk_features, None, "untouched: follows the language");
        run_ui(&mut app, "app.preferences", &json!({})).unwrap().unwrap();
        app.ui.dialog.as_mut().unwrap().fields.insert("cjkFeatures".into(), json!(false));
        crate::dialogs::confirm(&mut app).unwrap();
        assert_eq!(app.session.prefs.cjk_features, Some(false));
        assert!(!crate::cjk_features(&app));
        // prefs.json round trip; a file from before the setting follows the language.
        let saved = serde_json::to_vec(&app.session.prefs).unwrap();
        assert_eq!(serde_json::from_slice::<designcraft_engine::Prefs>(&saved).unwrap().cjk_features, Some(false));
        assert_eq!(serde_json::from_str::<designcraft_engine::Prefs>(r#"{"scaleStrokes": false}"#).unwrap().cjk_features, None);
        assert_eq!(app.run("prefs.set", json!({"cjkFeatures": null})).unwrap()["cjkFeatures"], Value::Null);
        assert!(crate::cjk_features(&app), "follows the Japanese interface again");
    }

    #[test]
    fn menu_button_fits_its_anchor_and_resets_scroll_on_reopen() {
        // Workspace/panel menus may be in the middle; the status-bar zoom menu opens upwards.
        for anchor_y in [0.0, 130.0, 260.0] {
            let ctx = egui::Context::default();
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0));
            let mut tick = 0;
            let mut frame = |events: Vec<egui::Event>| {
                tick += 1;
                let mut button = None;
                let mut first = None;
                let mut last = None;
                let mut output =
                    ctx.run_ui(egui::RawInput { screen_rect: Some(screen), time: Some(tick as f64 / 60.0), events, ..Default::default() }, |ui| {
                        ui.add_space(anchor_y);
                        let response = menu_button(ui, "Long menu", |ui| {
                            for i in 0..60 {
                                let response = ui.button(format!("Item {i}"));
                                if i == 0 {
                                    first = Some((response.rect, ui.clip_rect()));
                                }
                                if i == 59 {
                                    last = Some((response.rect, ui.clip_rect()));
                                }
                            }
                        });
                        button = Some(response.response);
                    });
                output.textures_delta.clear();
                (button.expect("menu button"), first, last)
            };
            let click =
                |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
            frame(vec![]);
            let (button, _, _) = frame(vec![]);
            let anchor = button.rect;
            for pressed in [true, false] {
                frame(vec![egui::Event::PointerMoved(anchor.center()), click(anchor.center(), pressed)]);
            }
            for _ in 0..3 {
                frame(vec![]);
            }
            let popup = ctx.memory(|memory| memory.area_rect(egui::Popup::default_response_id(&button))).expect("open menu");
            assert!(screen.contains_rect(popup), "menu at {anchor_y} fits in the viewport: {popup:?}");
            assert!(
                popup.top() >= anchor.bottom() - 1.0 || popup.bottom() <= anchor.top() + 1.0,
                "menu at {anchor_y} stays on one side of its button: {popup:?}, {anchor:?}"
            );
            let (_, first, last) = frame(vec![]);
            let (first, clip) = first.expect("first row");
            assert!(clip.contains_rect(first), "newly opened menu starts at the first row");
            assert!(!clip.intersects(last.expect("last row").0), "last row starts below the scroll area");

            frame(vec![
                egui::Event::PointerMoved(popup.center()),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -5000.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            for _ in 0..60 {
                frame(vec![]);
            }
            let (_, first, last) = frame(vec![]);
            let (last, clip) = last.expect("scrolled last row");
            assert!(popup.contains_rect(last) && clip.contains_rect(last), "last row is reachable: {last:?}, {popup:?}, {clip:?}");
            assert!(!clip.intersects(first.expect("scrolled first row").0), "menu really scrolled away from its start");

            let outside = egui::pos2(screen.right() - 10.0, screen.center().y);
            for pressed in [true, false] {
                frame(vec![egui::Event::PointerMoved(outside), click(outside, pressed)]);
            }
            assert!(!egui::Popup::is_any_open(&ctx), "outside click closes the menu");
            assert!(frame(vec![]).1.is_none(), "closed menu is no longer drawn");
            for pressed in [true, false] {
                frame(vec![egui::Event::PointerMoved(anchor.center()), click(anchor.center(), pressed)]);
            }
            for _ in 0..3 {
                frame(vec![]);
            }
            let (_, first, last) = frame(vec![]);
            let (first, clip) = first.expect("reopened first row");
            assert!(popup.contains_rect(first) && clip.contains_rect(first), "reopening resets to the first row: {first:?}, {clip:?}");
            assert!(!clip.intersects(last.expect("reopened last row").0), "reopening discards the old scroll offset");
        }
    }

    #[test]
    fn tiny_menu_viewport_does_not_force_the_default_scroll_height() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 100.0));
        let mut tick = 0;
        let mut frame = |events: Vec<egui::Event>| {
            tick += 1;
            let mut button = None;
            let mut output =
                ctx.run_ui(egui::RawInput { screen_rect: Some(screen), time: Some(tick as f64 / 60.0), events, ..Default::default() }, |ui| {
                    ui.add_space(35.0);
                    let response = menu_button(ui, "Long menu", |ui| {
                        for i in 0..20 {
                            let _ = ui.button(format!("Item {i}"));
                        }
                    });
                    button = Some(response.response);
                });
            output.textures_delta.clear();
            button.expect("menu button")
        };
        frame(vec![]);
        let button = frame(vec![]);
        let pos = button.rect.center();
        for pressed in [true, false] {
            frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
            ]);
        }
        for _ in 0..3 {
            frame(vec![]);
        }
        let popup = ctx.memory(|memory| memory.area_rect(egui::Popup::default_response_id(&button))).expect("open menu");
        let space = (screen.bottom() - button.rect.bottom()).max(button.rect.top() - screen.top());
        assert!(space < 64.0, "fixture has less space than egui's default minimum scroll height");
        // A complete command row may not fit, but the scroll area's minimum must not make
        // the popup exceed its height budget or cover the button that dismisses it.
        assert!(popup.height() <= space, "tiny menu respects its available height: {popup:?}, {space}");
        assert!(screen.contains_rect(popup), "tiny menu remains inside the viewport: {popup:?}");
        assert!(
            popup.top() >= button.rect.bottom() - 1.0 || popup.bottom() <= button.rect.top() + 1.0,
            "tiny menu stays beside its anchor: {popup:?}, {:?}",
            button.rect
        );
    }

    #[test]
    fn clipped_submenu_does_not_open_when_hovering_outside_menu() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 900.0));
        let entries = vec![Item::Sub("Hidden submenu".into(), vec![cmd_item("app.preferences", Value::Null)])];
        let mut tick = 0;
        let mut frame = |spacing: f32, events: Vec<egui::Event>| {
            tick += 1;
            let mut button_rect = egui::Rect::NOTHING;
            let mut hidden_submenu = None;
            let mut output =
                ctx.run_ui(egui::RawInput { screen_rect: Some(screen), time: Some(tick as f64 / 60.0), events, ..Default::default() }, |ui| {
                    egui::MenuBar::new().ui(ui, |ui| {
                        let response = ui.menu_button("Menu", |ui| {
                            egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                                ui.add_space(spacing);
                                let button_id = ui.next_auto_id();
                                menu_items(&mut app, ui, &entries, "Test");
                                let response = ui.ctx().read_response(button_id).expect("submenu button response");
                                let open = egui::containers::menu::MenuState::from_ui(ui, |state, _| state.open_item.is_some());
                                hidden_submenu = Some((response.rect, ui.clip_rect(), open));
                            });
                        });
                        button_rect = response.response.rect;
                    });
                });
            output.textures_delta.clear();
            (button_rect, hidden_submenu)
        };
        frame(500.0, vec![]);
        let button = frame(500.0, vec![]).0.center();
        for pressed in [true, false] {
            frame(
                500.0,
                vec![
                    egui::Event::PointerMoved(button),
                    egui::Event::PointerButton { pos: button, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
                ],
            );
        }
        for _ in 0..3 {
            frame(500.0, vec![]);
        }
        let (_, Some((hidden, clip, open))) = frame(500.0, vec![]) else { panic!("test menu is open") };
        assert!(!open, "submenu starts closed");
        assert!(screen.contains_rect(hidden), "hidden row remains inside the screen: {hidden:?}");
        assert!(!clip.intersects(hidden), "submenu must be fully clipped: {hidden:?}, {clip:?}");
        let (_, Some((_, _, open))) = frame(500.0, vec![egui::Event::PointerMoved(hidden.center())]) else { panic!("test menu remains open") };
        assert!(!open, "hovering the clipped row's position outside the menu must not open its submenu");

        let partial_spacing = 500.0 - (hidden.top() - clip.bottom()) - 6.0;
        let (_, Some((partial, clip, open))) = frame(partial_spacing, vec![]) else { panic!("test menu remains open") };
        assert!(!open);
        assert!(partial.top() < clip.bottom() && partial.bottom() > clip.bottom(), "row must be partially clipped: {partial:?}, {clip:?}");
        let outside = egui::pos2(partial.center().x, clip.bottom() + 1.0);
        let (_, Some((_, _, open))) = frame(partial_spacing, vec![egui::Event::PointerMoved(outside)]) else { panic!("test menu remains open") };
        assert!(!open, "hovering the clipped portion of a partially visible row must not open its submenu");
        let visible = partial.intersect(clip).center();
        let (_, Some((_, _, open))) = frame(partial_spacing, vec![egui::Event::PointerMoved(visible)]) else { panic!("test menu remains open") };
        assert!(open, "hovering the visible portion still opens the submenu");
        let (_, Some((_, _, open))) = frame(500.0, vec![]) else { panic!("test menu remains open") };
        assert!(!open, "an open submenu closes when its row becomes fully clipped");
    }

    #[test]
    fn native_labels_keep_their_ampersands() {
        assert_eq!(native_label("Numbering & Section Options…"), "Numbering && Section Options…");
        assert_eq!(native_label("A&&B"), "A&&&&B");
        assert_eq!(native_label("Pages"), "Pages");
    }

    #[test]
    fn the_mac_window_menu_minimizes_with_cmd_m() {
        let window = native_menu_tree().into_iter().find(|(t, _)| *t == "Window").map(|(_, e)| e).unwrap_or_default();
        assert!(matches!(window.first(), Some(Item::Cmd { id, shortcut: Some("Cmd+M"), .. }) if id == "window.minimize"), "{:?}", window.first());
        assert!(matches!(window.get(1), Some(Item::Sep)));
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        assert!(run_ui(&mut app, "window.minimize", &Value::Null).is_some_and(|r| r.is_err()), "no host window controls");
        app.native_menu = true;
        assert!(menu_enabled(&app, "window.minimize") && !menu_enabled(&app, "window.maximize"));
        run_ui(&mut app, "window.minimize", &Value::Null).unwrap().unwrap();
        assert_eq!(app.pending_window_commands, [egui::ViewportCommand::Minimized(true)]);
    }

    #[test]
    fn mac_settings_follow_about_in_the_app_menu_and_leave_edit() {
        let pos = |command: &str| MAC_APP_MENU.iter().position(|m| matches!(m, AppMenuItem::Cmd { command: c, .. } if *c == command));
        let (Some(about), Some(settings)) = (pos("help.about"), pos("app.preferences")) else { panic!("About and Settings are in the app menu") };
        assert!(about < settings);
        assert_eq!(MAC_APP_MENU.get(settings - 1), Some(&AppMenuItem::Sep));
        assert_eq!(MAC_APP_MENU.get(settings + 1), Some(&AppMenuItem::Sep));
        let label = |command: &str| {
            MAC_APP_MENU.iter().find_map(|m| match m {
                AppMenuItem::Cmd { label, command: c, .. } if *c == command => Some(*label),
                _ => None,
            })
        };
        assert_eq!(label("app.preferences"), Some("Settings…"));
        assert_ne!(crate::i18n::tr("de", "Settings…"), "Settings…");
        let ids = MAC_APP_MENU.iter().filter_map(|m| if let AppMenuItem::Cmd { id, .. } = m { Some(*id) } else { None }).collect::<Vec<_>>();
        assert!(ids.iter().enumerate().all(|(i, id)| !ids[..i].contains(id)), "native ids are unique: {ids:?}");

        let edit = |tree: Vec<(&'static str, Vec<Item>)>| {
            let mut out = Vec::new();
            walk(&tree.into_iter().find(|(t, _)| *t == "Edit").map(|(_, e)| e).unwrap_or_default(), &mut out);
            out.into_iter().map(|(_, id, _)| id).collect::<Vec<_>>()
        };
        assert!(edit(menu_tree()).iter().any(|id| id == "app.preferences"), "the in-window Edit menu keeps Preferences");
        let native = native_menu_tree();
        let mut all = Vec::new();
        for (_, entries) in &native {
            walk(entries, &mut all);
        }
        assert!(!all.iter().any(|(_, id, _)| id == "app.preferences"), "the macOS menu bar has Settings only in the app menu");
        assert!(edit(native_menu_tree()).iter().any(|id| id == "app.keyboardShortcuts"));
        for (title, entries) in &native {
            assert!(!matches!(entries.first(), Some(Item::Sep)) && !matches!(entries.last(), Some(Item::Sep)), "{title}: separator at an end");
            assert!(entries.windows(2).all(|w| !matches!(w, [Item::Sep, Item::Sep])), "{title}: doubled separator");
        }
    }

    /// Web save: a download that couldn't start is an error and leaves the document unsaved; one
    /// that started marks it saved.
    #[test]
    fn web_save_marks_the_document_saved_only_after_the_download_starts() {
        let dir = tempfile::tempdir().unwrap();
        // The title names a temporary path: on native, file.saveAs writes where the web build doesn't.
        let title = dir.path().join("doc").to_string_lossy().to_string();
        let fails = std::rc::Rc::new(std::cell::Cell::new(true));
        let f = fails.clone();
        let services = crate::Services {
            download: Some(Box::new(move |_: &str, _: &[u8]| if f.get() { Err("blocked".into()) } else { Ok(()) })),
            ..Default::default()
        };
        let mut app = DesignApp::new(designcraft_engine::Session::new(), services);
        app.run("file.new", json!({"title": title})).unwrap();
        app.run("frame.create", json!({"rect": [36, 36, 300, 200]})).unwrap();
        assert!(app.session.active().unwrap().is_dirty());

        let e = app.run("app.save", json!({})).unwrap_err();
        assert!(e.contains("blocked"), "{e}");
        assert!(app.session.active().unwrap().is_dirty(), "a failed download leaves the document unsaved");

        fails.set(false);
        app.run("app.save", json!({})).unwrap();
        assert!(!app.session.active().unwrap().is_dirty());
    }

    fn start_screen_texts(app: &mut crate::DesignApp) -> Vec<String> {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "");
        let mut shapes = Vec::new();
        for _ in 0..3 {
            let input =
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            });
            out.textures_delta.clear();
            shapes = out.shapes;
        }
        shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect()
    }

    /// File › Open Recent: the files first (`app.openRecent {index}`), then Clear Recent Files.
    #[test]
    fn open_recent_lists_the_files_then_clear() {
        let tree = menu_tree();
        let file = &tree.iter().find(|(m, _)| *m == "File").unwrap().1;
        let Some(Item::Sub(name, children)) = file.get(2) else { panic!("Open Recent after Open…: {:?}", file.get(2)) };
        assert_eq!(format!("File/{name}"), OPEN_RECENT);
        assert!(matches!(children.as_slice(), [Item::Cmd { id, .. }] if id == "file.clearRecent"));
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        assert!(recent_items(&app).is_empty());
        assert!(!menu_enabled(&app, "file.clearRecent"));
        app.session.prefs.note_recent_file("/work/a.designcraft");
        app.session.prefs.note_recent_file("/work/b.idml");
        let items = recent_items(&app);
        let labels: Vec<_> = items
            .iter()
            .map(|i| match i {
                Item::Cmd { label, id, params, .. } => format!("{label} {id} {params}"),
                Item::Sep => "-".into(),
                Item::Sub(..) => "sub".into(),
            })
            .collect();
        assert_eq!(labels, ["b.idml app.openRecent {\"index\":0}", "a.designcraft app.openRecent {\"index\":1}", "-"]);
        assert!(menu_enabled(&app, "file.clearRecent"));
        assert!(menu_enabled(&app, "app.openRecent"));
    }

    /// A recent file that is gone says so in an alert (and the start screen still lists it).
    #[test]
    fn open_recent_of_a_missing_file_alerts_and_the_start_screen_lists_recent_files() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        assert!(!start_screen_texts(&mut app).iter().any(|t| t == "Recent files"), "no section without recent files");
        app.session.prefs.note_recent_file("/nonexistent/dc-recent/gone.designcraft");
        assert!(app.run("app.openRecent", json!({"index": 0})).is_err());
        let d = app.ui.dialog.take().expect("an alert");
        assert_eq!(d.fields["title"], "Can't Open the File");
        assert_eq!(d.fields["file"], "gone.designcraft");
        assert!(d.fields["message"].as_str().unwrap().contains("can't be found"));
        assert!(app.run("app.openRecent", json!({"index": 9})).is_err(), "out of range: an error, not a panic");
        app.ui.dialog = None;
        let texts = start_screen_texts(&mut app);
        assert!(texts.iter().any(|t| t == "Recent files"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "gone"), "the card names it without `.designcraft`: {texts:?}");
    }
}
