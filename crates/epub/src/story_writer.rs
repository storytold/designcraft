use super::chapters::{CHAPTER_MARK, heading_level, starts_chapter};
use super::css::{char_classes, export_mapping, override_css, para_classes};
use super::html::asset_path;
use super::markup::{safe_mime, story_markup, xml_attr, xml_text};
use super::*;

/// Closes the open list, if any.
fn close_list(list: &mut Option<&str>, out: &mut String) {
    if let Some(l) = list.take() {
        let _ = writeln!(out, "</{}>", l.split(' ').next().unwrap_or(l));
    }
}

/// Closes the open preformatted block and list, if any.
fn close_blocks(list: &mut Option<&str>, pre: &mut Option<(String, bool)>, out: &mut String) {
    if pre.take().is_some() {
        out.push_str("</pre>\n");
    }
    close_list(list, out);
}

/// Nesting limit for stories inside stories (table cells, footnotes, tables in cells), so a deeply
/// nested or cyclic document can't recurse without bound.
const MAX_DEPTH: usize = 8;

/// Writes stories with their tables and notes, numbering notes across the stories of one export.
pub(crate) struct StoryWriter<'a, 'f> {
    doc: &'a Document,
    pc: BTreeMap<&'a str, String>,
    cc: BTreeMap<&'a str, String>,
    /// EPUB structural semantics (`epub:type`); plain HTML doesn't declare the namespace.
    epub: bool,
    /// Notes written so far, for unique element ids.
    footnote_ids: u32,
    endnote_ids: u32,
    /// Number shown on the next footnote and endnote of the current story.
    next_footnote: u32,
    next_endnote: u32,
    /// Endnotes met so far: (number, list item markup).
    endnotes: Vec<(u32, String)>,
    /// Names each packaged file (a package path, or a data URI for single-file HTML). Without
    /// it, anchored objects are left out.
    img_src: Option<&'f dyn Fn(&str, &str, &[u8]) -> String>,
    /// Images, video and sound met so far, by package name.
    pub(crate) images: BTreeMap<String, (&'a str, &'a [u8])>,
    /// Asset names of the images written without alt text (decorative).
    pub(crate) no_alt: Vec<String>,
    /// Prefix of the `id`s the current story's headings get (`{prefix}-p{paragraph index}`, the
    /// targets of the table of contents); only top-level paragraphs get one.
    heading_ids: Option<String>,
    /// Where EPUB content files start (see [`starts_chapter`]).
    pub(crate) split: EpubSplit,
}

impl<'a, 'f> StoryWriter<'a, 'f> {
    pub(crate) fn new(doc: &'a Document, epub: bool, img_src: Option<&'f dyn Fn(&str, &str, &[u8]) -> String>) -> Self {
        StoryWriter {
            doc,
            pc: para_classes(&doc.styles),
            cc: char_classes(&doc.styles),
            epub,
            footnote_ids: 0,
            endnote_ids: 0,
            next_footnote: 1,
            next_endnote: 1,
            endnotes: Vec::new(),
            img_src,
            images: BTreeMap::new(),
            no_alt: Vec::new(),
            heading_ids: None,
            split: doc.settings.epub_split.clone(),
        }
    }

    fn epub_type(&self, t: &str) -> String {
        if self.epub { format!(" epub:type=\"{t}\"") } else { String::new() }
    }

    /// One document story: paragraphs, tables, and each footnote after the paragraph that refers
    /// to it.
    pub(crate) fn story(&mut self, sid: StoryId, heading_ids: Option<&str>) -> String {
        let Some(st) = self.doc.story(sid) else { return String::new() };
        self.heading_ids = heading_ids.map(str::to_string);
        self.next_footnote = self.doc.footnote_start(sid);
        self.next_endnote = self.doc.endnote_start(sid);
        let mut out = String::new();
        self.paragraphs(st, 0, "", &mut out);
        out
    }

    /// `st`'s paragraphs. `lead` (a note's number) starts the first paragraph.
    ///
    /// Consecutive `li` paragraphs share one list (`ol` for a numbered style, `ul` for a bulleted
    /// one, an unmarked `ul` when the style has no list type of its own, e.g. typed bullets), and
    /// consecutive `pre` paragraphs of one style one block, their lines separated by newlines.
    fn paragraphs(&mut self, st: &Story, depth: usize, lead: &str, out: &mut String) {
        let doc = self.doc;
        let mut lead = lead;
        let mut list: Option<&str> = None;
        let mut pre: Option<(String, bool)> = None;
        for (pi, (r, pf)) in st.para_ranges().into_iter().zip(&st.paras).enumerate() {
            let Some(text) = st.text.get(r.clone()) else { continue };
            let table = pf.table.and_then(|id| st.tables.get(&id)).filter(|_| text.contains(story::TABLE_ANCHOR));
            let only_table = table.is_some() && text.chars().all(|c| c == story::TABLE_ANCHOR || c.is_whitespace());
            if !only_table || !lead.is_empty() {
                // Export Tagging: the style's element and class.
                let (tag, class) = export_mapping(&doc.styles, &pf.style, false, &self.pc);
                let props = doc.styles.resolve_para(pf).0;
                let rtl = props.direction == designcraft_doc::TextDirection::RightToLeft;
                // Chapter files start only at top-level paragraphs, after any open list or
                // preformatted block is closed, so each file stays balanced.
                if depth == 0 && self.heading_ids.is_some() && starts_chapter(&self.split, &doc.styles, &pf.style) {
                    close_blocks(&mut list, &mut pre, out);
                    out.push_str(CHAPTER_MARK);
                }
                let continues_pre = tag == "pre" && pre.as_ref() == Some(&(class.clone(), rtl));
                if !continues_pre && pre.take().is_some() {
                    out.push_str("</pre>\n");
                }
                let list_tag = (tag == "li").then_some(match props.list_type {
                    designcraft_doc::ListType::Numbers => "ol",
                    designcraft_doc::ListType::Bullets => "ul",
                    designcraft_doc::ListType::None => "ul style=\"list-style:none\"",
                });
                if list != list_tag {
                    close_list(&mut list, out);
                    if let Some(l) = list_tag {
                        let _ = writeln!(out, "<{l}>");
                    }
                    list = list_tag;
                }
                if continues_pre {
                    out.push('\n');
                } else {
                    let id = match &self.heading_ids {
                        Some(prefix) if depth == 0 && heading_level(tag).is_some() => format!(" id=\"{prefix}-p{pi}\""),
                        _ => String::new(),
                    };
                    let _ = write!(out, "<{tag}{id} class=\"{}\"{}>", xml_attr(&class), if rtl { " dir=\"rtl\"" } else { "" });
                }
                out.push_str(lead);
                if lead.is_empty() && text.trim().is_empty() {
                    out.push_str("&#160;");
                }
                lead = "";
                let mut after = String::new();
                if !only_table {
                    self.runs(st, r, depth, out, &mut after);
                }
                if tag == "pre" {
                    // Closed when the next paragraph isn't a line of the same block.
                    pre = Some((class, rtl));
                } else {
                    let _ = writeln!(out, "</{tag}>");
                }
                // Footnotes follow the paragraph outside any list or preformatted block.
                if !after.is_empty() {
                    close_blocks(&mut list, &mut pre, out);
                    out.push_str(&after);
                }
            }
            if let Some(t) = table
                && depth < MAX_DEPTH
            {
                close_blocks(&mut list, &mut pre, out);
                self.table(t, depth + 1, out);
            }
        }
        close_blocks(&mut list, &mut pre, out);
    }

    /// The markup for one run's text `raw` (starting at byte `a` of the story): anchored objects
    /// and notes where their marks are.
    fn run_markup(&mut self, st: &Story, raw: &str, a: usize, depth: usize, after: &mut String) -> String {
        let doc = self.doc;
        let mut t = String::new();
        let mut seg_start = 0;
        // Index of the next anchored object: marks count from the story start, as
        // `Story::object_at` counts them (counted once, at the run's first mark).
        let mut mark: Option<usize> = None;
        for (i, c) in raw.char_indices() {
            if c != FOOTNOTE_REF && c != ENDNOTE_REF && c != OBJECT_MARK {
                continue;
            }
            t.push_str(&run_text(doc, raw.get(seg_start..i).unwrap_or_default()));
            seg_start = i + c.len_utf8();
            if c == OBJECT_MARK {
                let k = mark.unwrap_or_else(|| st.text.get(..a).map_or(0, |t| t.matches(OBJECT_MARK).count()));
                mark = Some(k.saturating_add(1));
                if let Some(o) = st.objects.get(k) {
                    t.push_str(&self.object(o));
                }
            } else if depth < MAX_DEPTH {
                if c == FOOTNOTE_REF {
                    self.footnote(st, a + i, depth, &mut t, after);
                } else {
                    self.endnote(st, a + i, depth, &mut t);
                }
            }
        }
        t.push_str(&run_text(doc, raw.get(seg_start..).unwrap_or_default()));
        t
    }

    /// The runs of paragraph `para` of `st`; footnotes go to `after`.
    fn runs(&mut self, st: &Story, para: std::ops::Range<usize>, depth: usize, out: &mut String, after: &mut String) {
        let doc = self.doc;
        for (rr, f) in st.runs() {
            let a = rr.start.max(para.start);
            let b = rr.end.min(para.end);
            if a >= b {
                continue;
            }
            // Hidden conditional text isn't exported.
            if f.over.conditions.as_deref().is_some_and(|c| doc.conditions_hide(c)) || f.over.change == Some(designcraft_doc::ChangeMark::Deleted) {
                continue;
            }
            let Some(raw) = st.text.get(a..b) else { continue };
            let t = self.run_markup(st, raw, a, depth, after);
            let (ctag, cls) = if f.style != story::NO_CHAR_STYLE {
                let (tag, class) = export_mapping(&doc.styles, &f.style, true, &self.cc);
                (tag, format!(" class=\"{}\"", xml_attr(&class)))
            } else {
                ("span", String::new())
            };
            let style = override_css(doc, &f.style, &f.over);
            let style_attr = if style.is_empty() { String::new() } else { format!(" style=\"{}\"", xml_attr(&style)) };
            if cls.is_empty() && style_attr.is_empty() {
                out.push_str(&t);
            } else {
                let _ = write!(out, "<{ctag}{cls}{style_attr}>{t}</{ctag}>");
            }
        }
    }

    /// An anchored object where it is anchored: its graphics in the text, an above-line object as
    /// a block of its own. Anchored text frames are left out.
    fn object(&mut self, o: &AnchoredObject) -> String {
        let mut out = String::new();
        o.item.walk(&mut |it| {
            if let Some((markup, _)) = self.graphic(it) {
                out.push_str(&markup);
            }
        });
        match &o.position {
            AnchorPosition::AboveLine { align, .. } if !out.is_empty() => {
                let align = match align {
                    designcraft_doc::anchored::AnchorAlign::Left => "left",
                    designcraft_doc::anchored::AnchorAlign::Center => "center",
                    designcraft_doc::anchored::AnchorAlign::Right => "right",
                };
                format!("<span class=\"anchored\" style=\"display:block;text-align:{align}\">{out}</span>")
            }
            _ => out,
        }
    }

    /// The markup for one graphic item: an `<img>`, or an HTML5 player for video and sound (with
    /// `true`), its file registered in `images`. `None` for an item that isn't a graphic, whose
    /// asset has no data, or when the writer names no files.
    pub(crate) fn graphic(&mut self, it: &Item) -> Option<(String, bool)> {
        let img_src = self.img_src?;
        let doc = self.doc;
        let Content::Graphic(g) = &it.content else { return None };
        let a = doc.assets.get(&g.asset)?;
        if a.data.is_empty() {
            return None;
        }
        let mime = safe_mime(&a.mime);
        // Video and sound: an HTML5 player (with the poster, when there is one).
        if let Some(kind) = designcraft_doc::media_kind(mime) {
            let name = asset_path("media", a, mime);
            self.images.insert(name.clone(), (mime, a.data.as_slice()));
            let m = it.media.clone().unwrap_or_default();
            let mut attrs = String::new();
            for (on, attr) in [(m.controls, " controls=\"controls\""), (m.play_on_page_load, " autoplay=\"autoplay\""), (m.looping, " loop=\"loop\"")]
            {
                if on {
                    attrs.push_str(attr);
                }
            }
            if let Some(pa) = m.poster.and_then(|p| doc.assets.get(&p)) {
                let poster_mime = safe_mime(&pa.mime);
                let pn = asset_path("images", pa, poster_mime);
                self.images.insert(pn.clone(), (poster_mime, pa.data.as_slice()));
                if kind == "video" {
                    attrs.push_str(&format!(" poster=\"{}\"", xml_attr(&img_src(&pn, poster_mime, &pa.data))));
                }
            }
            let markup = format!("<{kind} src=\"{}\"{attrs}>{}</{kind}>", xml_attr(&img_src(&name, mime, &a.data)), xml_text(&a.name));
            return Some((markup, true));
        }
        let name = asset_path("images", a, mime);
        self.images.insert(name.clone(), (mime, a.data.as_slice()));
        // Alt text from Object Export Options; without one the image counts as decorative
        // (`alt=""`): a file name read aloud helps nobody.
        if it.alt_text.trim().is_empty() {
            self.no_alt.push(a.name.clone());
        }
        Some((format!("<img src=\"{}\" alt=\"{}\"/>", xml_attr(&img_src(&name, mime, &a.data)), xml_attr(&it.alt_text)), false))
    }

    /// The footnote whose reference is at byte `pos` of `st`: a note reference in `out`, the note
    /// itself in `after`.
    fn footnote(&mut self, st: &Story, pos: usize, depth: usize, out: &mut String, after: &mut String) {
        let Some(note) = st.notes.get(st.notes_before(pos)) else { return };
        let doc = self.doc;
        let o = &doc.footnote_options;
        let n = self.next_footnote;
        self.next_footnote = n.saturating_add(1);
        self.footnote_ids = self.footnote_ids.saturating_add(1);
        let id = self.footnote_ids;
        let _ = write!(
            out,
            "<sup><a{} role=\"doc-noteref\" id=\"fnref-{id}\" href=\"#fn-{id}\">{}</a></sup>",
            self.epub_type("noteref"),
            xml_text(&o.label(n, false))
        );
        let lead = format!("<a href=\"#fnref-{id}\">{}</a> ", xml_text(&o.label(n, true)));
        let mut body = String::new();
        self.paragraphs(&note.text, depth + 1, &lead, &mut body);
        let _ = write!(after, "<aside{} role=\"doc-footnote\" id=\"fn-{id}\">\n{body}</aside>\n", self.epub_type("footnote"));
    }

    /// The endnote whose reference is at byte `pos` of `st`: a note reference in `out`; the note
    /// joins the endnotes section.
    fn endnote(&mut self, st: &Story, pos: usize, depth: usize, out: &mut String) {
        let Some(note) = st.endnotes.get(st.endnotes_before(pos)) else { return };
        let n = self.next_endnote;
        self.next_endnote = n.saturating_add(1);
        self.endnote_ids = self.endnote_ids.saturating_add(1);
        let id = self.endnote_ids;
        let label = xml_text(&self.doc.endnote_options.label(n));
        let _ = write!(out, "<sup><a{} role=\"doc-noteref\" id=\"enref-{id}\" href=\"#en-{id}\">{label}</a></sup>", self.epub_type("noteref"));
        let mut body = String::new();
        self.paragraphs(&note.text, depth + 1, &format!("<a href=\"#enref-{id}\">{label}</a> "), &mut body);
        let item = format!("<li{} id=\"en-{id}\">\n{body}</li>\n", self.epub_type("endnote"));
        self.endnotes.push((n, item));
    }

    /// The endnotes met so far, in number order, as one section (empty when there are none).
    pub(crate) fn endnotes_section(&mut self) -> String {
        if self.endnotes.is_empty() {
            return String::new();
        }
        self.endnotes.sort_by_key(|(n, _)| *n);
        let doc = self.doc;
        let o = &doc.endnote_options;
        let mut out = format!("<section{} role=\"doc-endnotes\" id=\"endnotes\">\n", self.epub_type("endnotes"));
        if !o.heading.trim().is_empty() {
            let (tag, class) = export_mapping(&doc.styles, &o.heading_style, false, &self.pc);
            let _ = writeln!(out, "<{tag} class=\"{}\">{}</{tag}>", xml_attr(&class), xml_text(&o.heading));
        }
        out.push_str("<ol style=\"list-style: none; padding-left: 0\">\n");
        for (_, item) in &self.endnotes {
            out.push_str(item);
        }
        out.push_str("</ol>\n</section>\n");
        out
    }

    /// One table cell with its row and column spans, as `th` or `td`.
    fn table_cell(&mut self, text: &Story, cell_tag: &str, (rs, cs): (usize, usize), depth: usize, out: &mut String) {
        let _ = write!(out, "<{cell_tag}");
        if rs > 1 {
            let _ = write!(out, " rowspan=\"{rs}\"");
        }
        if cs > 1 {
            let _ = write!(out, " colspan=\"{cs}\"");
        }
        out.push('>');
        self.paragraphs(text, depth, "", out);
        let _ = writeln!(out, "</{cell_tag}>");
    }

    /// A table: header rows as `thead`, footer rows as `tfoot`, merged cells as row and column
    /// spans (kept within their row group).
    fn table(&mut self, t: &Table, depth: usize, out: &mut String) {
        let (nr, nc) = (t.nrows(), t.ncols());
        if nr.checked_mul(nc) != Some(t.cells.len()) {
            return;
        }
        let (h, f) = (t.header_rows(), t.footer_rows());
        let body_end = nr.saturating_sub(f).max(h);
        let mut covered = vec![false; t.cells.len()];
        out.push_str("<table>\n");
        for (rows, group, cell_tag) in [(0..h, "thead", "th"), (h..body_end, "tbody", "td"), (body_end..nr, "tfoot", "td")] {
            if rows.is_empty() {
                continue;
            }
            let _ = writeln!(out, "<{group}>");
            for r in rows.clone() {
                out.push_str("<tr>");
                for c in 0..nc {
                    let i = r * nc + c;
                    if covered.get(i).copied().unwrap_or(true) {
                        continue;
                    }
                    let Some(cell) = t.cells.get(i) else { continue };
                    let rs = (cell.row_span.max(1) as usize).min(rows.end - r);
                    let cs = (cell.col_span.max(1) as usize).min(nc - c);
                    for rr in r..r + rs {
                        for cc in c..c + cs {
                            if let Some(slot) = covered.get_mut(rr * nc + cc) {
                                *slot = true;
                            }
                        }
                    }
                    self.table_cell(&cell.text, cell_tag, (rs, cs), depth, out);
                }
                out.push_str("</tr>\n");
            }
            let _ = writeln!(out, "</{group}>");
        }
        out.push_str("</table>\n");
    }
}

/// Run text with variables replaced by their values.
fn run_text(doc: &Document, raw: &str) -> String {
    if raw.chars().any(|c| designcraft_doc::vars::var_index(c).is_some()) {
        let sub: String = raw
            .chars()
            .map(|c| match designcraft_doc::vars::var_index(c) {
                Some(i) => doc.variable_value(i, None).unwrap_or_default(),
                None => c.to_string(),
            })
            .collect();
        story_markup(&sub)
    } else {
        story_markup(raw)
    }
}

/// One story as XHTML: paragraphs, tables, and each footnote after the paragraph that refers to
/// it. Endnote references link to the endnotes section that the full export ends with.
pub fn story_html(doc: &Document, sid: StoryId) -> String {
    StoryWriter::new(doc, true, None).story(sid, None)
}
