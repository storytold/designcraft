//! The menu bar tree (InDesign order), UI-only commands, keyboard shortcuts and Quick Apply.

use serde_json::{Value, json};

use crate::{DesignApp, ScreenMode};

/// UI commands: (id, label, shortcut, params).
pub const UI_COMMANDS: &[(&str, &str, Option<&str>, &str)] = &[
    ("app.newDocumentDialog", "New Document…", Some("Cmd+N"), "{}"),
    ("app.openDialog", "Open…", Some("Cmd+O"), "{}"),
    ("app.placeDialog", "Place…", Some("Cmd+D"), "{}"),
    ("app.saveDialog", "Save As…", Some("Cmd+Shift+S"), "{}"),
    ("app.save", "Save", Some("Cmd+S"), "{}"),
    ("app.saveCopyDialog", "Save a Copy…", Some("Cmd+Alt+S"), "{} — choose where to write a copy (the document stays as it is)"),
    ("app.loadSwatches", "Load Swatches…", None, "{} — pick a swatch exchange (.ase) file"),
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
        "{lang: \"\"|de|fr|es|ja|zh|ar|pt-br|it|uk} — menus and panel names (the macOS menu bar follows on the next launch)",
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
    ("app.footnoteOptionsDialog", "Document Footnote Options…", None, "{} — numbering, formatting and layout of footnotes"),
    ("app.rubyDialog", "Ruby…", None, "{} — the reading set over the selected text"),
    (
        "app.paragraphRulesDialog",
        "Paragraph Rules…",
        None,
        "{rule?: ruleAbove|ruleBelow} — Rule Above / Rule Below of the selected paragraphs (type.para)",
    ),
    ("app.findFontDialog", "Find/Replace Font…", None, "{} — fonts used (missing ones flagged) and replacing them"),
    ("app.insertXrefDialog", "Insert Cross-Reference…", None, "{} — New Cross-Reference dialog (paragraph or text anchor, format)"),
    ("app.deleteAllGuides", "Delete All Guides on Spread", None, "{} — the spread in view"),
    ("app.deletePage", "Delete Page", None, "{} — deletes the page in view"),
    ("app.duplicateSpread", "Duplicate Spread", None, "{} — duplicates the spread in view (pages and items)"),
    ("app.tablePanel", "Table Panel", Some("Shift+F9"), "{}"),
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
        "app.formattingAffectsText",
        "Formatting Affects Text",
        Some("J"),
        "{on?: bool} — with text frames selected, colours go to their text (on) or the frames (off); toggles without `on`",
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
    ("view.textThreads", "Show/Hide Text Threads", Some("Cmd+Alt+Y"), "{}"),
    ("view.hiddenCharacters", "Show/Hide Hidden Characters", Some("Cmd+Alt+I"), "{}"),
    ("view.taggedFrames", "Show/Hide Tagged Frames", None, "{} — XML-tagged frames outlined in their tag colour"),
    ("window.hidePanels", "Show/Hide Panels", Some("Tab"), "{} — every panel, the Tools panel included"),
    ("window.hidePanelsExceptTools", "Show/Hide Panels Except Tools", Some("Shift+Tab"), "{}"),
    ("window.nextDocument", "Next Document", Some("Cmd+F6"), "{}"),
    ("window.previousDocument", "Previous Document", Some("Cmd+Shift+F6"), "{}"),
    ("app.layerOptions", "Object Layer Options…", None, "{} — show or hide the layers of the selected placed PDF"),
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
    ("view.goToPage", "Go to Page…", Some("Cmd+J"), "{page: number (position) | \"name\" (section page name, or \"+n\" for a position)}"),
    (
        "window.panel",
        "Show Panel",
        None,
        "{panel: properties|pages|layers|swatches|paragraphStyles|characterStyles|stroke|character|paragraph|textWrap|links|table}",
    ),
    ("window.floatPanel", "Float Panel", None, "{panel, x?, y?} — the panel in its own movable window"),
    ("window.dockPanel", "Dock Panel", None, "{panel} — back into the dock's icon column"),
    ("window.controlBar", "Control", Some("Cmd+Alt+6"), "{}"),
    ("window.split", "Split Window", None, "{on?: bool} — two views of the document side by side, each with its own zoom and scroll"),
    ("window.newWindow", "New Window", None, "{on?: bool} — another view of the active document in its own window"),
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
        "{name: Essentials|Advanced|Book|Digital Publishing|Interactive for PDF|Printing and Proofing|Typography}",
    ),
    ("window.newWorkspace", "New Workspace…", None, "{name} — saves the current bars and panel arrangement"),
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
            ">Interface Language",
            "ui:app.language|English|{\"lang\": \"\"}",
            "ui:app.language|Deutsch|{\"lang\": \"de\"}",
            "ui:app.language|Français|{\"lang\": \"fr\"}",
            "ui:app.language|Español|{\"lang\": \"es\"}",
            "ui:app.language|日本語|{\"lang\": \"ja\"}",
            "ui:app.language|简体中文|{\"lang\": \"zh\"}",
            "ui:app.language|العربية|{\"lang\": \"ar\"}",
            "ui:app.language|Português (Brasil)|{\"lang\": \"pt-br\"}",
            "ui:app.language|Italiano|{\"lang\": \"it\"}",
            "ui:app.language|Українська|{\"lang\": \"uk\"}",
            "<",
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
            "-",
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
            "ui:app.findFontDialog",
            "-",
            "cmd:type.createOutlines",
            ">Bulleted and Numbered Lists",
            "cmd:list.define",
            "<",
            ">Type on a Path",
            "cmd:type.pathOptions",
            "cmd:type.pathOptions|Delete Type from Path|{\"delete\": true}",
            "<",
            "ui:window.panel|Glyphs|{\"panel\": \"glyphs\"}",
            "cmd:type.fillWithPlaceholder",
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
            "cmd:text.insert|Current Page Number|{\"text\": \"\u{e000}\", \"raw\": true}",
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
            "-",
            ">Change Case",
            "cmd:type.changeCase|UPPERCASE|{\"case\": \"upper\"}",
            "cmd:type.changeCase|lowercase|{\"case\": \"lower\"}",
            "cmd:type.changeCase|Title Case|{\"case\": \"title\"}",
            "cmd:type.changeCase|Sentence case|{\"case\": \"sentence\"}",
            "<",
            "-",
            "cmd:type.alignLeft",
            "cmd:type.alignCenter",
            "cmd:type.alignRight",
            "cmd:type.justify",
            "ui:app.paragraphRulesDialog",
            "-",
            "cmd:type.bold",
            "cmd:type.italic",
            "cmd:type.underline",
            "cmd:type.allCaps",
            "-",
            "cmd:type.sizeUp",
            "cmd:type.sizeDown",
            "-",
            "ui:view.hiddenCharacters",
            "ui:view.taggedFrames",
            ">Rotate Spread",
            "ui:view.rotateSpread|90° CW|{\"angle\": 90}",
            "ui:view.rotateSpread|90° CCW|{\"angle\": -90}",
            "ui:view.rotateSpread|180°|{\"angle\": 180}",
            "ui:view.rotateSpread|Clear Rotation|{\"angle\": 0}",
            "<",
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
            "ui:view.flattenerPreview",
            "ui:view.tagMarkers",
            ">Separations Preview",
            "ui:view.separations|Off|{\"plate\": null, \"inkLimit\": null}",
            "ui:view.separations|Cyan|{\"plate\": \"cyan\"}",
            "ui:view.separations|Magenta|{\"plate\": \"magenta\"}",
            "ui:view.separations|Yellow|{\"plate\": \"yellow\"}",
            "ui:view.separations|Black|{\"plate\": \"black\"}",
            "ui:view.separations|Ink Limit 300%|{\"inkLimit\": 300}",
            "<",
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
            "cmd:footnote.insert",
            "cmd:endnote.insert",
            "cmd:math.insert",
            ">Story Direction",
            "cmd:type.storyDirection|Horizontal|{\"vertical\": false}",
            "cmd:type.storyDirection|Vertical|{\"vertical\": true}",
            "<",
            "cmd:type.tateChuYoko",
            "ui:app.rubyDialog",
            "cmd:type.kenten",
            "cmd:type.warichu",
            ">Track Changes",
            "cmd:changes.track",
            "cmd:changes.acceptAll",
            "cmd:changes.rejectAll",
            "<",
            "ui:app.footnoteOptionsDialog",
            "cmd:footnote.goToReference",
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
            ">Paths",
            "cmd:object.makeCompoundPath",
            "cmd:object.releaseCompoundPath",
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
            "cmd:object.fit|Fill Frame Proportionally|{\"mode\":\"fillProportionally\"}",
            "cmd:object.fit|Fit Content Proportionally|{\"mode\":\"fitProportionally\"}",
            "cmd:object.fit|Fit Frame to Content|{\"mode\":\"fitFrameToContent\"}",
            "cmd:object.fit|Fit Content to Frame|{\"mode\":\"fitContentToFrame\"}",
            "cmd:object.fit|Center Content|{\"mode\":\"centerContent\"}",
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
            "cmd:table.merge",
            "cmd:table.unmerge",
            ">Convert Cell Type",
            "ui:app.graphicCell|Convert Cell to Graphic Cell…",
            "cmd:table.textCell",
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
            "ui:view.zoomIn",
            "ui:view.zoomOut",
            "ui:view.fitPage",
            "ui:view.fitSelection",
            "ui:view.fitSpread",
            "ui:view.actualSize",
            "ui:view.entirePasteboard",
            "-",
            "ui:view.overprintPreview",
            "ui:view.togglePreview",
            ">Display Performance",
            "ui:view.fastDisplay",
            "ui:view.typicalDisplay",
            "ui:view.highQualityDisplay",
            "<",
            "-",
            "ui:view.rulers",
            "ui:view.frameEdges",
            "ui:view.textThreads",
            "ui:view.hiddenCharacters",
            "-",
            ">Grids & Guides",
            "ui:view.guides",
            "ui:view.snapToGuides",
            "ui:view.snapToDocumentGrid",
            "ui:view.smartGuides",
            "ui:view.baselineGrid",
            "-",
            "ui:app.deleteAllGuides",
            "<",
        ],
    ),
    (
        "Window",
        &[
            ">Arrange",
            "ui:window.newWindow",
            "ui:window.split",
            "<",
            "-",
            "ui:window.hidePanels",
            "ui:window.hidePanelsExceptTools",
            "ui:window.nextDocument",
            "ui:window.previousDocument",
            "-",
            "ui:window.controlBar",
            "ui:window.taskBar",
            "ui:window.toolsDoubleColumn",
            "-",
            "ui:window.panel",
            "-",
            ">Type & Tables",
            "ui:window.panel|Conditional Text|{\"panel\": \"conditions\"}",
            "ui:window.panel|Notes|{\"panel\": \"notes\"}",
            "ui:window.panel|Glyphs|{\"panel\": \"glyphs\"}",
            "<",
            ">Interactive",
            "ui:window.panel|Hyperlinks|{\"panel\": \"hyperlinks\"}",
            "ui:window.panel|Bookmarks|{\"panel\": \"bookmarks\"}",
            "ui:window.panel|Articles|{\"panel\": \"articles\"}",
            "ui:window.panel|Object States|{\"panel\": \"states\"}",
            "ui:window.panel|Buttons and Forms|{\"panel\": \"buttons\"}",
            "ui:window.panel|Media|{\"panel\": \"media\"}",
            "ui:window.panel|Page Transitions|{\"panel\": \"transitions\"}",
            "ui:window.panel|Track Changes|{\"panel\": \"trackChanges\"}",
            "ui:window.panel|Scripts|{\"panel\": \"scripts\"}",
            "ui:window.panel|Liquid Layout|{\"panel\": \"liquid\"}",
            "ui:window.panel|Tags|{\"panel\": \"tags\"}",
            "ui:window.panel|Book|{\"panel\": \"book\"}",
            "<",
            ">Utilities",
            "ui:window.panel|Data Merge|{\"panel\":\"dataMerge\"}",
            "<",
            ">Output",
            "ui:window.panel|Attributes|{\"panel\": \"attributes\"}",
            "cmd:preflight.run",
            "<",
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

pub fn ui_label(id: &str) -> Option<(&'static str, Option<&'static str>)> {
    UI_COMMANDS.iter().find(|c| c.0 == id).map(|c| (c.1, c.2))
}

/// Run a UI command; `None` if `id` isn't one.
pub fn run_ui(app: &mut DesignApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    let rect = app.canvas_rect;
    let flag = |b: &mut bool| {
        *b = !*b;
        Ok(json!(*b))
    };
    Some(match id {
        "app.newDocumentDialog" => {
            app.ui.dialog = Some(crate::dialogs::Dialog::new("newDocument", json!({})));
            Ok(Value::Null)
        }
        "app.openDialog" => app.pick_and_open("open"),
        "app.placeDialog" => app.pick_and_open("place"),
        "app.placeWithOptions" => {
            // Word/RTF Import Options: style mapping and conflicts before the text lands.
            let Some(path) = app.services.pick_open.as_mut().and_then(|f| f("place")) else { return Some(Ok(Value::Null)) };
            let l = path.to_lowercase();
            if !(l.ends_with(".docx") || l.ends_with(".rtf")) {
                return Some(app.open_file("file.place", json!({"path": path})));
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
        "app.exportXml" => export_bytes(app, p, "xml", "file.exportXml"),
        "app.printBooklet" => export_bytes(app, p, "pdf", "file.printBooklet"),
        "app.menus" => {
            app.ui.dialog = Some(crate::dialogs::Dialog::new("menus", json!({"query": ""})));
            Ok(Value::Null)
        }
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
                        app.status(format!(
                            "Packaged {} files into {}",
                            v["files"].as_array().map_or(0, |f| f.len()),
                            v["dir"].as_str().unwrap_or("")
                        ));
                    }
                    r
                }
                None => Ok(Value::Null),
            }
        }
        "app.language" => {
            let v = p.get("lang").and_then(Value::as_str).unwrap_or("");
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
            let sid = p.get("story").and_then(Value::as_u64).map(designcraft_doc::StoryId).or_else(|| {
                let st = app.session.active()?;
                st.selection.text.map(|t| t.story).or_else(|| st.selection.items.iter().find_map(|i| st.doc.item(*i)?.text_frame().map(|t| t.story)))
            });
            match sid {
                Some(s) => {
                    app.story_editor = if app.story_editor == Some(s) { None } else { Some(s) };
                    Ok(Value::Null)
                }
                None => Err("select a text frame or place the cursor in text".into()),
            }
        }
        "app.findChange" => {
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
            app.ui.dialog = Some(crate::dialogs::Dialog::new("ruby", json!({"text": cur.unwrap_or_default()})));
            Ok(Value::Null)
        }
        "app.paragraphRulesDialog" => {
            let Some(a) = crate::panels::text_attrs(app) else { return Some(Err("select text or a text frame".into())) };
            let rule = if p.get("rule").and_then(Value::as_str) == Some("ruleBelow") { "ruleBelow" } else { "ruleAbove" };
            let current = json!({"ruleAbove": a["para"]["ruleAbove"], "ruleBelow": a["para"]["ruleBelow"]});
            app.ui.dialog = Some(crate::dialogs::Dialog::new("paragraphRules", json!({"rule": rule, "current": current})));
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
            let spread = if st.editing_parents { json!({"kind": "parent", "index": 0}) } else { json!(st.doc.page_loc(page).map_or(0, |l| l.0)) };
            return Some(app.run("guide.deleteAll", json!({"spread": spread})));
        }
        "app.deletePage" | "app.duplicateSpread" => {
            let Some(st) = app.session.active() else { return Some(Err("no document open".into())) };
            let page = crate::canvas::current_page(app).unwrap_or(0);
            let r = if id == "app.deletePage" {
                app.run("layout.pages.delete", json!({"pages": [page]}))
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
            f["section"] = json!("general");
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
            app.ui.screen_mode = match p.get("mode").and_then(Value::as_str).unwrap_or("normal") {
                "preview" => ScreenMode::Preview,
                "bleed" => ScreenMode::Bleed,
                "slug" => ScreenMode::Slug,
                "presentation" => ScreenMode::Presentation,
                _ => ScreenMode::Normal,
            };
            Ok(Value::Null)
        }
        "view.togglePreview" => {
            app.ui.screen_mode = if app.ui.screen_mode == ScreenMode::Normal { ScreenMode::Preview } else { ScreenMode::Normal };
            Ok(Value::Null)
        }
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
        "window.panel" => {
            let panel = p.get("panel").and_then(Value::as_str).unwrap_or("properties").to_string();
            if crate::dock::DOCK_TABS.iter().any(|(id, _, _)| *id == panel) {
                app.ui.dock_tab = panel;
                app.ui.dock_expanded = true;
                app.ui.open_panel = None;
            } else if !app.ui.floating.iter().any(|(p, _)| *p == panel) {
                app.ui.open_panel = if app.ui.open_panel.as_deref() == Some(panel.as_str()) { None } else { Some(panel) };
            }
            Ok(Value::Null)
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
            if !crate::dock::ICON_PANELS.iter().any(|(id, _, _)| *id == panel) {
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
        "window.newWorkspace" => {
            let Some(name) = p.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()) else {
                app.ui.dialog = Some(crate::dialogs::Dialog::new("newWorkspace", json!({"name": ""})));
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
            let name = p.get("name").and_then(Value::as_str).unwrap_or("Essentials");
            if let Some(w) = app.ui.custom_workspaces.iter().find(|w| w.name == name).cloned() {
                app.ui.control_bar = w.control_bar;
                app.ui.task_bar = w.task_bar;
                app.ui.task_bar_pin = w.task_bar_pin;
                app.ui.tools_double_column = w.tools_double_column;
                app.ui.dock_tab = w.dock_tab;
                app.ui.dock_expanded = w.dock_expanded;
                app.ui.open_panel = w.open_panel;
                app.ui.floating = w.floating;
                app.ui.workspace = w.name;
                return Some(Ok(Value::Null));
            }
            // Workspaces choose which bars and panels are visible.
            app.ui.control_bar = matches!(name, "Advanced" | "Typography" | "Printing and Proofing" | "Book");
            let dock_tab = match name {
                "Typography" => Some("properties"),
                "Interactive for PDF" | "Digital Publishing" => Some("pages"),
                _ => None,
            };
            if let Some(tab) = dock_tab {
                app.ui.dock_tab = tab.into();
                app.ui.dock_expanded = true;
            }
            app.ui.open_panel = match name {
                "Typography" => Some("paragraphStyles".into()),
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
fn download_document(app: &mut DesignApp) -> Result<Value, String> {
    let name = app.session.active().map(|d| format!("{}.designcraft", d.doc.title)).ok_or("no document")?;
    let ser = app.run("file.serialize", json!({}))?;
    let bytes = designcraft_engine::cmd::base64_decode(ser["base64"].as_str().unwrap_or_default());
    if let Some(download) = app.services.download.as_mut() {
        download(&name, &bytes);
    }
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
    let r = app.run("file.exportPdf", params)?;
    let bytes = designcraft_engine::cmd::base64_decode(r["base64"].as_str().unwrap_or_default());
    match app.services.write.as_mut() {
        Some(w) => w(&path, &bytes).map(|_| json!({"path": path, "bytes": bytes.len(), "pages": r["pages"], "warnings": r["warnings"]})),
        None => Err("no writer".into()),
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

/// Is a command enabled (for menu greying)?
pub fn enabled(app: &DesignApp, id: &str) -> bool {
    if waits_for_ime(app, id) {
        return false;
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

/// Check state of a toggle command (None = not a toggle).
pub fn checked(app: &DesignApp, id: &str, params: &Value) -> Option<bool> {
    Some(match id {
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
    if ui_label(id).is_some() {
        return app.session.active().is_some() || id.starts_with("app.") || id.starts_with("window.");
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
    if params.is_null() && id == "file.close" {
        close_document(app, None);
        return;
    }
    if params.is_null() && id == "layout.documentSetup" {
        crate::dialogs::open_document_setup(app);
        return;
    }
    if params.is_null() && id == "file.print" {
        crate::dialogs::open_print(app);
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
    if !(ctx.text_edit_focused() && params.is_null() && forward_to_text_field(app, ctx, id)) {
        activate(app, id, params);
    }
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
fn forward_to_text_field(app: &mut DesignApp, ctx: &egui::Context, id: &str) -> bool {
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
        menu_button(ui, crate::rtl::widget(ui, crate::i18n::tr(&lang, menu)), |ui| {
            if crate::i18n::is_rtl(&lang) {
                ui.set_max_width(320.0);
            }
            ui.with_layout(egui::Layout::top_down_justified(if crate::i18n::is_rtl(&lang) { egui::Align::Max } else { egui::Align::Min }), |ui| {
                ui.set_min_width(240.0);
                let hidden = menu_items(app, ui, &entries, menu);
                if hidden > 0 && !app.ui.show_full_menus {
                    ui.separator();
                    if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&lang, "Show All Menu Items"))).clicked() {
                        app.ui.show_full_menus = true;
                    }
                }
            });
        });
    }
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
                    hidden += menu_items(app, ui, children, &sub);
                });
            }
            Item::Cmd { label, .. } if !app.ui.show_full_menus && app.ui.hidden_menu_items.contains(&menu_key(path, label)) => hidden += 1,
            Item::Cmd { label, id, params, shortcut: _ } => {
                let label = crate::i18n::tr(&app.ui.language, label);
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
            "Delete" => key = Some(egui::Key::Delete),
            k => key = egui::Key::from_name(k),
        }
    }
    if sc.ends_with("+-") || sc == "-" {
        key = Some(egui::Key::Minus);
    }
    key.map(|k| (m, k))
}

/// Global keyboard shortcuts: menu commands and single-key tool shortcuts.
pub fn shortcuts(app: &mut DesignApp, ctx: &egui::Context) {
    // Only a focused text field takes the keys: the canvas (or a button) having focus after a click
    // must not swallow tool shortcuts until Esc clears it.
    // While an IME composes at the text caret, the keyboard is its own.
    if ctx.text_edit_focused() || app.ui.dialog.is_some() || app.ui.palette.is_some() || app.session.tool_composing() {
        return;
    }
    let typing = app.session.wants_text();
    let events = ctx.input(|i| i.events.clone());
    for e in events {
        let egui::Event::Key { key, pressed: true, modifiers, repeat: false, .. } = e else { continue };
        // Command shortcuts.
        let mut fired: Option<String> = None;
        let all = effective_shortcuts(app);
        for (id, sc) in &all {
            let id = id.as_str();
            if let Some((m, k)) = parse_shortcut(sc)
                && k == key
                && m.command == modifiers.command
                && m.shift == modifiers.shift
                && m.alt == modifiers.alt
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
                fired = Some(id.to_string());
                break;
            }
        }
        if let Some(id) = fired {
            if app.native_shortcuts.contains(&id) && !app.ui.shortcuts.contains_key(&id) {
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
            } else if key == egui::Key::Escape && app.session.tool_id() != "selection" && !app.session.tool_busy() {
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
        let items = quick_apply_items(&app.session, &q);
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
                    b = b.shortcut_text(it.kind);
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
pub fn quick_apply_items(session: &designcraft_engine::Session, query: &str) -> Vec<QuickItem> {
    let ql = query.to_lowercase();
    let hit = |label: &str, id: &str| ql.is_empty() || label.to_lowercase().contains(&ql) || id.to_lowercase().contains(&ql);
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
    let commands = designcraft_engine::command_specs()
        .iter()
        .filter(|c| !c.menu.is_empty() || c.shortcut.is_some())
        .map(|c| (c.id, c.label, c.shortcut))
        .chain(UI_COMMANDS.iter().map(|c| (c.0, c.1, c.2)))
        .filter(|(id, l, _)| hit(l, id))
        .map(|(id, l, sc)| QuickItem { label: l.to_string(), id: id.to_string(), params: json!({}), shortcut: sc.map(str::to_string), kind: "" });
    out.extend(commands);
    out.truncate(14);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "words", "caret": false})).unwrap();
        let (fid, sid) = (r["id"].as_u64().unwrap(), r["story"].as_u64().unwrap());
        app.run("selection.set", json!({"ids": [fid]})).unwrap();
        let text_fill = |app: &crate::DesignApp| {
            let st = app.session.active().unwrap().doc.story(designcraft_doc::StoryId(sid)).unwrap().clone();
            st.char_format_at(2).over.fill.clone()
        };
        let frame_fill = |app: &crate::DesignApp| app.session.active().unwrap().doc.item(designcraft_doc::ItemId(fid)).unwrap().fill.swatch.clone();
        assert!(!crate::panels::colors_affect_text(&app));
        crate::panels::apply_swatch(&mut app, false, "Cyan", None).unwrap();
        assert_eq!(frame_fill(&app), "Cyan");
        assert_eq!(text_fill(&app), None);
        let r = run_ui(&mut app, "app.formattingAffectsText", &json!({})).unwrap().unwrap();
        assert_eq!(r["on"], true, "toggles on");
        assert!(crate::panels::colors_affect_text(&app));
        crate::panels::apply_swatch(&mut app, false, "Magenta", Some(0.5)).unwrap();
        assert_eq!(frame_fill(&app), "Cyan", "the frame is left alone");
        assert_eq!(text_fill(&app).as_deref(), Some("Magenta"));
        assert_eq!(crate::panels::color_target(&mut app).map(|t| (t.0, t.1)), Some(("Magenta".to_string(), 0.5)));
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
    fn interface_language_switches_and_validates() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        run_ui(&mut app, "app.language", &json!({"lang": "de"})).unwrap().unwrap();
        assert_eq!(app.ui.language, "de");
        assert_eq!(checked(&app, "app.language", &json!({"lang": "de"})), Some(true));
        assert_eq!(crate::i18n::tr(&app.ui.language, "Window"), "Fenster");
        assert!(run_ui(&mut app, "app.language", &json!({"lang": "xx"})).unwrap().is_err());

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
        assert!(menu_tree().iter().flat_map(|(_, entries)| entries.iter()).any(|entry| {
            matches!(entry, Item::Sub(label, children) if label == "Interface Language" && children.iter().any(|child| {
                matches!(child, Item::Cmd { label, id, params, .. } if label == "Українська" && id == "app.language" && params["lang"] == "uk")
            }))
        }));
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
        for lang in ["uk", "zh", "", "ar", "pt-br", "it"] {
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
        let items = quick_apply_items(&app.session, "");
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
    fn quick_apply_lists_styles_then_commands() {
        let mut s = designcraft_engine::Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Body Copy"})).unwrap();
        let items = quick_apply_items(&s, "body");
        assert_eq!(items[0].id, "style.paragraph.apply");
        assert_eq!(items[0].params, json!({"name": "Body Copy"}));
        assert_eq!(items[0].kind, "Paragraph Style");
        assert!(quick_apply_items(&s, "").iter().all(|i| i.kind.is_empty()));
        assert!(quick_apply_items(&s, "preferences").iter().any(|i| i.id == "app.preferences"));
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
        assert!(all.iter().any(|(l, id, p)| l == "Fill Frame Proportionally" && id == "object.fit" && p["mode"] == "fillProportionally"));
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
}
