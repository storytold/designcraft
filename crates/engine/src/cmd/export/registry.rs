use super::formats::{epub_split_param, export_epub, export_html, export_pptx, export_text, has_story_target, print_booklet};
use super::pdf::export_pdf;
use super::{CommandSpec, Value, cmd, has_doc, json};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "file.exportPdf", "Export PDF…", ["File"], None,
        "{path?, pages?: \"1-3,5\" | [1,3] (1-based positions; default all), flatten?: high|medium|low|ppi (Transparency Flattener: spreads with transparency are rasterised; with standard x1a, the resolution complex transparent regions are rasterized at: high 300, medium 150, low 72, default 300), spreads?: bool, fullScreen?: bool, bookmarksPanel?: bool, pageLayout?: single|continuous|twoUp|twoUpCover|twoUpContinuous, view?: fitPage|fitWidth|actual, advanceSeconds?: number (interactive PDF), bleed?: bool (document bleed), marks?: bool | {crop?, bleed?, pageInfo?, weight?, offset?}, standard?: \"none\"|\"x4\"|\"x1a\"|\"a2b\" (x1a: PDF/X-1a:2003, the PDF/X-4 file flattened in its inks by craft-prepress: no transparency, only CMYK, gray and spot color), compressImages?: bool, tagged?: bool (structure tree: stories as paragraphs, figures with alt text), media?: bool (interactive: embed placed video and sound), title?, author?, created?: int (creation date in Unix seconds; default now, for reproducible files)} → {path, bytes, pages, warnings} (no path: {base64, …})",
        has_doc, export_pdf),
        cmd!(noundo "file.exportEpub", "Export EPUB (Reflowable)…", ["File"], None,
        "{path?, title?, author?, language?: \"en\", identifier?: str (dc:identifier; a stable urn:uuid by default), coverImage?: path (a PNG or JPEG as the cover), tocTitle?: str (the table of contents' title; from the language by default), cover?: bool (the first page as the cover image), embedFonts?: bool (package the stylesheet's fonts whose licence allows it, whole files), split?: \"off\"|\"exportTags\"|{style: name} (Split Document for this export, not stored; the document's `document.epubSplit` by default), fixedLayout?: bool (pre-paginated: each page as an image with its text)} → {path, bytes, warnings} (no path: {base64, bytes, warnings}); warnings name images without alt text, exported as decorative, and fonts that could not be embedded",
        has_doc, export_epub),
        cmd!(
            "document.epubSplit",
            "EPUB Split Document",
            [],
            None,
            "{split?: \"off\"|\"exportTags\"|{style: name}} — EPUB Export Options › Split Document, stored with the document: one content file (off, the default), a file before each paragraph whose style has Export Tagging › Split Document on (exportTags), or before each paragraph of one style → {split}",
            has_doc,
            |s, p| {
                const ID: &str = "document.epubSplit";
                let cur = s.doc()?.doc.settings.epub_split.clone();
                let Some(v) = p.get("split") else { return Ok(json!({"split": cur})) };
                let split = epub_split_param(ID, &s.doc()?.doc, v)?;
                if split != cur {
                    let stored = split.clone();
                    s.edit(move |d, _| {
                        d.settings.epub_split = stored;
                        Ok(Value::Null)
                    })?;
                }
                Ok(json!({"split": split}))
            }
        ),
        cmd!(noundo "file.exportFixedEpub", "Export EPUB (Fixed Layout)…", ["File"], None,
        "{path?, title?, author?, language?} — pre-paginated EPUB → {path, bytes} (no path: {base64, bytes})",
        has_doc, |s, p| {
            export_epub(s, &crate::cmd::with_param(p, "fixedLayout", serde_json::json!(true)))
        }),
        cmd!(noundo "file.printBooklet", "Print Booklet…", ["File"], None,
        "{path?, type?: saddleStitch|twoUpConsecutive, spaceBetween? (pt)} — printer spreads as PDF (pages imposed in booklet order) → {path, bytes, sheets} (no path: {base64, …})",
        has_doc, print_booklet),
        cmd!(noundo "file.exportHtml", "Export HTML…", ["File"], None,
        "{path?, title?, language?} — one self-contained page (styles inline, images embedded), stories and graphics in reading order → {path, bytes, warnings} (no path: {text, bytes, warnings})",
        has_doc, export_html),
        cmd!(noundo "file.exportPptx", "Export PowerPoint…", ["File"], None,
        "{path?, pages?: \"1-3,5\" | [1,3] (1-based positions; default all), lineBreaks?: keep|reflow (keep: DesignCraft's lines, spacing and leading; reflow: one box per frame, PowerPoint sets the lines), rasterize?: bool (default true: objects PowerPoint can't show — placed PDF/EPS, type on a path, some effects — go in as 300 ppi images), embedFonts?: bool (default true: the TrueType fonts whose licences allow it), title?, author?} — every page as a slide of editable shapes, pictures, text boxes and tables → {path, bytes, slides, warnings} (no path: {base64, …})",
        has_doc, export_pptx),
        cmd!(noundo "file.exportText", "Export Text…", ["File"], None,
        "{path?, format?: \"txt\"|\"rtf\"|\"tagged\" (Tagged Text; default from the path, else txt), story?, frame?} — the story being edited or of the selected frame → {path, bytes} (no path: {text, bytes})",
        has_story_target, export_text),
    ]
}
