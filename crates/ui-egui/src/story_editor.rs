//! Story Editor (Edit → Edit in Story Editor, ⌘Y): the story as plain text with each paragraph's
//! style in the left column, the depth ruler beside it and the overset point marked. Edits are
//! applied as minimal range replacements, so formatting around them is kept.

use egui::{pos2, vec2};
use serde_json::json;

use designcraft_compose::ComposedStory;
use designcraft_doc::StoryId;

use crate::DesignApp;
use crate::theme::Tokens;

/// Width of the depth ruler, in screen points.
const RULER_WIDTH: f32 = 34.0;
/// Least distance between two of the depth ruler's labels, in screen points.
const RULER_LABEL_GAP: f32 = 10.0;
/// Color of the overset line and label.
const OVERSET_RED: egui::Color32 = egui::Color32::from_rgb(230, 40, 40);

/// The story open in the Story Editor. Story ids are counted per document, so the editor is bound
/// to the document the story belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    /// [`designcraft_engine::DocState::uid`] of the story's document.
    pub doc: u64,
    pub story: StoryId,
}

/// The editor is open and its document is the active one.
pub fn visible(app: &DesignApp) -> bool {
    app.story_editor.is_some_and(|t| app.session.active().is_some_and(|st| st.uid == t.doc))
}

/// Draws the editor while its document is active; it hides while another document is active and
/// closes with its document.
pub fn show(app: &mut DesignApp, ctx: &egui::Context) {
    let Some(target) = app.story_editor else { return };
    if !app.session.documents().iter().any(|d| d.uid == target.doc) {
        app.story_editor = None;
        return;
    }
    let Some(st) = app.session.active().filter(|st| st.uid == target.doc) else { return };
    let sid = target.story;
    let Some(story) = st.doc.story(sid).cloned() else {
        app.story_editor = None;
        return;
    };
    let t = Tokens::get(ctx);
    let cs = app.session.cache.get(&st.doc, sid, None);
    let overset = cs.overset_at;
    let depths = app.ui.story_editor_depth_ruler.then(|| line_depths(&cs));
    let unit = st.doc.settings.vertical_units;
    let mut open = true;
    let mut edit: Option<(usize, usize, String)> = None;
    let words = story.text.split_whitespace().count().to_string();
    egui::Window::new(crate::i18n::tr(&app.ui.language, "Story Editor — {count} words").replace("{count}", &words))
        .id(egui::Id::unique("story_editor"))
        .open(&mut open)
        .default_size(vec2(560.0, 520.0))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    // Style column: the labels are painted after the text, at the rows the text really wraps to.
                    let (column, _) = ui.allocate_exact_size(vec2(130.0, 0.0), egui::Sense::hover());
                    ui.separator();
                    // Depth ruler: painted after the text too, beside the rows the lines start on.
                    let ruler = depths.as_ref().map(|_| ui.allocate_exact_size(vec2(RULER_WIDTH, 0.0), egui::Sense::hover()).0);
                    let mut text = story.text.clone();
                    // (a fixed id: the ruler's allocation above would otherwise move the editor's auto id,
                    // and its focus and caret with it, whenever the ruler is shown or hidden)
                    let out = egui::TextEdit::multiline(&mut text)
                        .id_salt("story_text")
                        .font(egui::FontId::proportional(app.ui.story_editor_size.clamp(8.0, 36.0)))
                        .desired_width(f32::INFINITY)
                        .desired_rows(20)
                        .frame(egui::Frame::NONE)
                        .show(ui);
                    let r = out.response;
                    let changed = r.changed();
                    let starts = paragraph_start_offsets(out.galley.rows.iter().map(|row| (row.pos.y, row.ends_with_newline)));
                    for (i, y) in starts.into_iter().enumerate() {
                        let Some(para) = story.paras.get(i) else { break };
                        let rect = egui::Rect::from_min_size(egui::pos2(column.min.x, out.galley_pos.y + y), vec2(130.0, 14.0));
                        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                            let name = crate::i18n::style_name(&app.ui.language, &para.style);
                            crate::rtl::label(ui, egui::RichText::new(name).size(11.0).color(t.text_dim));
                        });
                    }
                    let overset_y = overset.filter(|_| !changed).map(|o| {
                        let ci = char_offsets(&story.text, std::iter::once(o));
                        out.galley_pos.y + start_rows(&out.galley, &ci).first().map_or(0.0, |row| row.0)
                    });
                    if let (Some(ruler), Some(depths)) = (ruler, depths.as_ref().filter(|_| !changed)) {
                        let bottom = overset_y.unwrap_or(out.galley_pos.y + out.galley.size().y);
                        let bar = egui::Rect::from_min_max(pos2(ruler.min.x, out.galley_pos.y), pos2(ruler.min.x + RULER_WIDTH, bottom));
                        ui.painter().rect_filled(bar, 0.0, t.ruler);
                        let ticks = depth_ticks(depths, unit.ruler_ticks(1.0).0);
                        let rows = start_rows(&out.galley, &char_offsets(&story.text, ticks.iter().map(|tick| tick.0)));
                        let marks = ticks.iter().zip(rows).map(|(&(_, depth), row)| (out.galley_pos.y + row.1, depth));
                        for (y, depth) in spaced_marks(marks, RULER_LABEL_GAP) {
                            ui.painter().line_segment([pos2(bar.max.x - 8.0, y), pos2(bar.max.x, y)], egui::Stroke::new(1.0, t.ruler_tick));
                            ui.painter().text(
                                pos2(bar.min.x + 2.0, y - 1.0),
                                egui::Align2::LEFT_BOTTOM,
                                crate::canvas::fmt_tick(unit.from_pt(depth)),
                                egui::FontId::proportional(9.0),
                                t.ruler_text,
                            );
                        }
                    }
                    // The overset line: across the text, and the ruler when it shows, above the first character that doesn't fit.
                    if let Some(y) = overset_y {
                        let x0 = ruler.map_or(r.rect.min.x, |rr| rr.min.x);
                        ui.painter().line_segment([pos2(x0, y), pos2(r.rect.max.x, y)], egui::Stroke::new(1.5, OVERSET_RED));
                    }
                    if changed {
                        // Minimal diff: common prefix and suffix.
                        let old = &story.text;
                        let pre = old.bytes().zip(text.bytes()).take_while(|(a, b)| a == b).count();
                        let pre = (0..=pre).rev().find(|&i| old.is_char_boundary(i) && text.is_char_boundary(i)).unwrap_or(0);
                        let max_suf = old.len().min(text.len()) - pre;
                        let suf = old.bytes().rev().zip(text.bytes().rev()).take(max_suf).take_while(|(a, b)| a == b).count();
                        let suf =
                            (0..=suf).rev().find(|&k| old.is_char_boundary(old.len() - k) && text.is_char_boundary(text.len() - k)).unwrap_or(0);
                        edit = Some((pre, old.len() - suf, text[pre..text.len() - suf].to_string()));
                    }
                });
                if let Some(o) = overset {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(crate::rtl::widget(
                            ui,
                            egui::RichText::new(crate::i18n::tr(&app.ui.language, "OVERSET")).color(OVERSET_RED).strong(),
                        ));
                        crate::rtl::label(
                            ui,
                            egui::RichText::new(
                                crate::i18n::tr(&app.ui.language, "{count} characters don't fit")
                                    .replace("{count}", &story.text[o.min(story.text.len())..].chars().count().to_string()),
                            )
                            .color(t.text_dim),
                        );
                    });
                }
            });
        });
    if let Some((a, b, text)) = edit {
        let _ = app.run("story.replaceRange", json!({"story": sid.0, "start": a, "end": b, "text": text}));
    }
    if !open {
        app.story_editor = None;
    }
}

/// Vertical offset of the first laid-out row of each paragraph, from `(row y, row ends with a newline)`
/// in layout order. A paragraph that wraps spans several rows, so counting characters per line
/// drifts from what the editor draws; the rows themselves are the truth.
fn paragraph_start_offsets(rows: impl Iterator<Item = (f32, bool)>) -> Vec<f32> {
    let mut starts = Vec::new();
    let mut at_start = true;
    for (y, ends_with_newline) in rows {
        if at_start {
            starts.push(y);
        }
        at_start = ends_with_newline;
    }
    starts
}

/// The depth ruler's values: for each composed line that fits, its first byte and how deep the
/// story's text fills its frames at that line (points). Each line covers the band of its column
/// from one leading above its bottom (its ascent and descent when larger) to its bottom; the depth
/// is the height of the bands covered so far, added up over the story's columns and threaded
/// frames. A line set below the line before it in the same column also covers the space between
/// them (paragraph spacing, or the room a wrap object takes). Lines set side by side in one column
/// (the sub-columns of a split paragraph, or text on both sides of a wrap object) cover one band,
/// so they count once.
fn line_depths(cs: &ComposedStory) -> Vec<(usize, f64)> {
    let mut out = Vec::new();
    let mut depth = 0.0;
    for ft in &cs.frames {
        let mut covered: std::collections::BTreeMap<u32, Vec<(f64, f64)>> = std::collections::BTreeMap::new();
        let mut before: Option<(u32, f64)> = None;
        for line in &ft.lines {
            let bottom = line.baseline + line.descent;
            let mut top = bottom - line.leading.max(line.ascent + line.descent);
            if let Some((column, above)) = before
                && column == line.column
                && above < top
            {
                top = above;
            }
            before = Some((line.column, bottom));
            if top.is_finite() && bottom > top {
                depth += cover(covered.entry(line.column).or_default(), top, bottom);
            }
            out.push((line.range.start, depth));
        }
    }
    out
}

/// Adds the band `top..bottom` to `bands` (sorted, apart from one another) and returns the height
/// it covers that `bands` did not.
fn cover(bands: &mut Vec<(f64, f64)>, top: f64, bottom: f64) -> f64 {
    let overlap: f64 = bands.iter().map(|&(a, b)| (b.min(bottom) - a.max(top)).max(0.0)).sum();
    let (mut a, mut b) = (top, bottom);
    bands.retain(|&(s, e)| {
        let touches = s <= b + 1e-6 && e >= a - 1e-6;
        if touches {
            (a, b) = (a.min(s), b.max(e));
        }
        !touches
    });
    let at = bands.partition_point(|&(s, _)| s < a);
    bands.insert(at, (a, b));
    (bottom - top - overlap).max(0.0)
}

/// The depth ruler's marks, one at the first line whose depth reaches each multiple of `step`
/// (points): `(line's first byte, the multiple)`. A line that passes several multiples takes the
/// deepest.
fn depth_ticks(depths: &[(usize, f64)], step: f64) -> Vec<(usize, f64)> {
    if !(step.is_finite() && step > 0.0) {
        return Vec::new();
    }
    let mut out: Vec<(usize, f64)> = Vec::new();
    let mut reached = 0.0;
    for &(at, depth) in depths {
        if !depth.is_finite() {
            continue;
        }
        let n = (depth / step).floor();
        if n > reached {
            reached = n;
            out.push((at, n * step));
        }
    }
    out
}

/// The depth ruler's marks `(y, depth)` (y ascending) that are drawn: of the marks on one editor
/// row only the deepest, and a mark on a later row only when it is at least `gap` below the one
/// drawn before it.
fn spaced_marks(marks: impl Iterator<Item = (f32, f64)>, gap: f32) -> Vec<(f32, f64)> {
    let mut out: Vec<(f32, f64)> = Vec::new();
    for mark in marks {
        match out.last_mut() {
            Some(last) if (mark.0 - last.0).abs() < 0.5 => *last = mark,
            Some(last) if mark.0 - last.0 < gap => {}
            _ => out.push(mark),
        }
    }
    out
}

/// The editor rows `(top, bottom)` (galley coordinates) that the characters `cis` (ascending) start
/// lines on, in one pass over the rows: at a soft wrap, the row a character starts rather than the
/// end of the row before.
fn start_rows(galley: &egui::Galley, cis: &[usize]) -> Vec<(f32, f32)> {
    let mut rows = galley.rows.iter().peekable();
    let mut start = 0;
    let mut out = Vec::with_capacity(cis.len());
    for &ci in cis {
        while let Some(row) = rows.peek() {
            let end = start + row.char_count_including_newline().0;
            if ci < end || rows.len() == 1 {
                break;
            }
            start = end;
            rows.next();
        }
        out.push(rows.peek().map_or((0.0, 0.0), |row| (row.min_y(), row.max_y())));
    }
    out
}

/// Character indices of the byte offsets `bytes` (ascending) in `text`, for the editor's cursors.
fn char_offsets(text: &str, bytes: impl Iterator<Item = usize>) -> Vec<usize> {
    let mut chars = text.char_indices().peekable();
    let mut count = 0;
    let mut out = Vec::new();
    for byte in bytes {
        while chars.next_if(|&(i, _)| i < byte).is_some() {
            count += 1;
        }
        out.push(count);
    }
    out
}

#[cfg(test)]
mod tests {
    use egui_kittest::kittest::Queryable;
    use serde_json::json;

    use crate::DesignApp;

    fn story_text(app: &DesignApp, doc: usize) -> String {
        let st = &app.session.documents()[doc];
        st.doc.stories.values().next().unwrap().text.clone()
    }

    /// A wrapped paragraph takes several rows; the next paragraph's label goes to the row after them.
    #[test]
    fn labels_follow_wrapped_rows() {
        let rows = [(0.0, false), (20.0, false), (40.0, true), (60.0, true), (80.0, false)];
        assert_eq!(super::paragraph_start_offsets(rows.into_iter()), vec![0.0, 60.0, 80.0]);
    }

    /// The depth runs on through the columns: it never goes back, and the first line of the second
    /// column is deeper than the last line of the first.
    #[test]
    fn depths_run_through_columns() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({})).unwrap();
        let text = "Depth ruler line of words. ".repeat(80);
        let r = app.run("frame.create", json!({"rect": [72, 72, 372, 272], "content": "text", "text": text, "caret": false})).unwrap();
        let (id, sid) = (r["id"].as_u64().unwrap(), designcraft_doc::StoryId(r["story"].as_u64().unwrap()));
        app.run("object.textFrameOptions", json!({"ids": [id], "columns": 2, "gutter": 12})).unwrap();
        let st = app.session.active().unwrap();
        let cs = app.session.cache.get(&st.doc, sid, None);
        assert!(cs.overset_at.is_some(), "the fixture overflows its frame");
        let depths = super::line_depths(&cs);
        let lines: Vec<_> = cs.frames[0].lines.iter().collect();
        let second = lines.iter().position(|l| l.column == 1).expect("text reaches the second column");
        assert!(second > 0 && depths.len() == lines.len(), "{} lines, {} depths", lines.len(), depths.len());
        assert!(depths.windows(2).all(|w| w[1].1 >= w[0].1), "depth never goes back: {depths:?}");
        assert!(depths[second].1 > depths[second - 1].1, "the second column adds to the first: {depths:?}");
    }

    /// A split paragraph's sub-columns sit side by side: a line beside one already counted adds
    /// nothing, whether the sub-columns hold several lines or one each, and the text after the
    /// split paragraphs goes on from the deeper sub-column.
    #[test]
    fn split_paragraph_sub_columns_count_once() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({})).unwrap();
        let text =
            format!("Opening line.\n{}\nMiddle line.\nTwo short lines set side by side.\nClosing line.", "Split column words go here. ".repeat(12));
        let r = app.run("frame.create", json!({"rect": [72, 72, 372, 672], "content": "text", "text": text, "caret": false})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let mut doc = (*app.session.active().unwrap().doc).clone();
        for i in [1, 3] {
            doc.story_mut(sid).unwrap().paras[i].para.span_columns = Some(designcraft_doc::SpanColumns::Split(2));
        }
        let cs = designcraft_compose::compose_story(&doc, sid, &designcraft_compose::ComposeOptions::default());
        let lines = &cs.frames[0].lines;
        let depths = super::line_depths(&cs);
        assert!(lines.windows(2).any(|w| w[0].para == 1 && w[1].para == 1 && w[1].baseline < w[0].baseline), "para 1 restarts higher");
        let side =
            (1..lines.len()).find(|&i| lines[i].para == 3 && lines[i - 1].para == 3 && (lines[i].baseline - lines[i - 1].baseline).abs() < 1e-6);
        let para3: Vec<_> = lines.iter().filter(|l| l.para == 3).map(|l| (l.baseline, l.x0)).collect();
        let side = side.unwrap_or_else(|| panic!("para 3 sets one line in each sub-column: {para3:?}"));
        assert_eq!(depths[side].1, depths[side - 1].1, "the second sub-column's line adds nothing: {depths:?}");
        for (i, l) in lines.iter().enumerate().filter(|(_, l)| l.para == 1).skip(1) {
            let beside = lines[..i].iter().any(|o| o.para == 1 && o.baseline >= l.baseline - 1e-6);
            if beside {
                assert_eq!(depths[i].1, depths[i - 1].1, "line {i} sits beside one counted: {depths:?}");
            }
        }
        let last = lines.len() - 1;
        let col_top = cs.frames[0].columns[0].y0;
        let closing = lines[last].baseline + lines[last].descent - col_top;
        assert!(depths[last].1 <= closing + 1e-6, "no deeper than the frame holds it: {} > {closing}", depths[last].1);
        assert!(depths.windows(2).all(|w| w[1].1 >= w[0].1), "{depths:?}");
    }

    /// Text wrapping on both sides of an object sets one line as two segments on one baseline: the
    /// second segment adds nothing to the depth.
    #[test]
    fn segments_beside_a_wrap_object_share_their_line_depth() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({})).unwrap();
        let text = "Words that wrap on both sides of the box in the middle. ".repeat(20);
        let r = app.run("frame.create", json!({"rect": [72, 72, 432, 472], "content": "text", "text": text, "caret": false})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let b = app.run("frame.create", json!({"rect": [232, 150, 272, 300], "content": "graphic"})).unwrap();
        app.run("object.textWrap", json!({"ids": [b["id"].as_u64().unwrap()], "mode": "boundingBox", "side": "bothSides"})).unwrap();
        let st = app.session.active().unwrap();
        let cs = app.session.cache.get(&st.doc, sid, None);
        let lines = &cs.frames[0].lines;
        let depths = super::line_depths(&cs);
        let pairs: Vec<usize> = (1..lines.len()).filter(|&i| (lines[i].baseline - lines[i - 1].baseline).abs() < 1e-6).collect();
        assert!(!pairs.is_empty(), "the fixture sets lines in two segments");
        for i in pairs {
            assert_eq!(depths[i].1, depths[i - 1].1, "segment {i} adds no depth: {depths:?}");
        }
        assert!(depths.windows(2).all(|w| w[1].1 >= w[0].1), "{depths:?}");
    }

    /// Space after a paragraph moves the next one down the frame, and the depth goes down with it.
    #[test]
    fn paragraph_spacing_adds_to_the_depth() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({})).unwrap();
        let r = app.run("frame.create", json!({"rect": [72, 72, 372, 672], "content": "text", "text": "First.\nSecond.", "caret": false})).unwrap();
        let sid = designcraft_doc::StoryId(r["story"].as_u64().unwrap());
        let mut doc = (*app.session.active().unwrap().doc).clone();
        doc.story_mut(sid).unwrap().paras[0].para.space_after = Some(72.0);
        let cs = designcraft_compose::compose_story(&doc, sid, &designcraft_compose::ComposeOptions::default());
        let lines = &cs.frames[0].lines;
        let depths = super::line_depths(&cs);
        assert_eq!(lines.len(), 2);
        let moved = (lines[1].baseline + lines[1].descent) - (lines[0].baseline + lines[0].descent);
        assert!(moved > 72.0, "the space after moves the second paragraph down: {moved}");
        assert!((depths[1].1 - depths[0].1 - moved).abs() < 1e-6, "the depth goes down as far: {depths:?}, {moved}");
    }

    /// A mark goes at the first line that reaches a multiple; a line that passes several takes only
    /// the deepest.
    #[test]
    fn ticks_go_at_the_first_line_that_reaches_a_multiple() {
        let depths = [(0, 10.0), (5, 30.0), (9, 70.0), (12, 100.0), (20, 200.0), (25, f64::NAN)];
        assert_eq!(super::depth_ticks(&depths, 60.0), vec![(9, 60.0), (20, 180.0)]);
        assert!(super::depth_ticks(&depths, 0.0).is_empty());
        assert_eq!(super::char_offsets("aé b", [0, 1, 3, 5].into_iter()), vec![0, 1, 2, 4]);
    }

    /// Several lines can start on one editor row (narrow columns, large text): only the deepest of
    /// their marks is drawn there, and marks on rows closer than a label skip, so labels never
    /// overlap and the ones kept stay spread down the ruler.
    #[test]
    fn marks_on_one_row_show_only_the_deepest_and_keep_their_spacing() {
        let marks = [(20.0, 60.0), (20.0, 120.0), (25.0, 180.0), (40.0, 240.0), (60.0, 300.0)];
        assert_eq!(super::spaced_marks(marks.into_iter(), 10.0), vec![(20.0, 120.0), (40.0, 240.0), (60.0, 300.0)]);
        let close = [(0.0, 60.0), (8.0, 120.0), (16.0, 180.0), (24.0, 240.0)];
        assert_eq!(super::spaced_marks(close.into_iter(), 10.0), vec![(0.0, 60.0), (16.0, 180.0)]);
    }

    /// A line that starts at a soft wrap in the editor is on the wrapped row, not at the end of the
    /// row before it, so the overset line and the marks sit beside the text they belong to.
    #[test]
    fn a_line_starting_at_a_soft_wrap_is_on_the_wrapped_row() {
        let ctx = egui::Context::default();
        let mut galley = None;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            galley = Some(ui.fonts_mut(|f| f.layout("aaaa bbbb".into(), egui::FontId::proportional(14.0), egui::Color32::WHITE, 40.0)));
        });
        output.textures_delta.clear();
        let galley = galley.expect("laid out");
        assert!(galley.rows.len() >= 2, "the fixture wraps: {} rows", galley.rows.len());
        let rows = super::start_rows(&galley, &[0, 4, 5, 9, 50]);
        let (first, second) = ((galley.rows[0].min_y(), galley.rows[0].max_y()), (galley.rows[1].min_y(), galley.rows[1].max_y()));
        assert_eq!(rows, vec![first, first, second, second, (galley.rows.last().unwrap().min_y(), galley.rows.last().unwrap().max_y())]);
    }

    /// View › Story Editor › Show/Hide Depth Ruler: the ruler sits between the style column and the
    /// text, which moves right by the ruler's width; the setting is remembered.
    #[test]
    fn depth_ruler_shows_and_hides_from_the_view_menu() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({})).unwrap();
        let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Ruler story", "caret": false})).unwrap();
        app.run("app.storyEditor", json!({"story": r["story"].as_u64().unwrap()})).unwrap();
        assert!(app.ui.story_editor_depth_ruler, "shown by default");
        let mut h = crate::test_window::open(app, egui::vec2(1440.0, 900.0));
        h.run_steps(2);
        let text_x = |h: &egui_kittest::Harness<'_, crate::test_window::Window>| {
            h.get_by(|n| n.role() == egui::accesskit::Role::MultilineTextInput && n.value().as_deref() == Some("Ruler story")).rect().min.x
        };
        let with = text_x(&h);
        h.state_mut().app.run("view.storyEditorDepthRuler", json!({})).unwrap();
        h.run_steps(2);
        let without = text_x(&h);
        assert!((with - without - super::RULER_WIDTH).abs() < 10.0, "the text moves by the ruler: {with} vs {without}");
        let app = &h.state().app;
        assert_eq!(crate::menus::checked(app, "view.storyEditorDepthRuler", &json!({})), Some(false));
        let saved = serde_json::to_vec(&app.ui).unwrap();
        assert!(!crate::UiState::from_preferences_json(&saved).unwrap().story_editor_depth_ruler);
    }

    #[test]
    fn depth_marks_wait_for_composition_after_editing() {
        fn marks(shape: &egui::epaint::Shape, tick: egui::Color32) -> usize {
            match shape {
                egui::epaint::Shape::Vec(shapes) => shapes.iter().map(|s| marks(s, tick)).sum(),
                egui::epaint::Shape::LineSegment { points, stroke }
                    if stroke.color == super::OVERSET_RED || (stroke.color == tick && (points[1].x - points[0].x - 8.0).abs() < 0.01) =>
                {
                    1
                }
                _ => 0,
            }
        }
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({})).unwrap();
        let text = "First line\nSecond line\nThird line\n".repeat(4);
        let r = app.run("frame.create", json!({"rect": [72, 72, 372, 172], "content": "text", "text": text, "caret": false})).unwrap();
        app.run("app.storyEditor", json!({"story": r["story"].as_u64().unwrap()})).unwrap();
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(900.0, 700.0))
            .with_accessibility_check(false)
            .build_ui_state(|ui, app| super::show(app, ui.ctx()), app);
        h.run_steps(3);
        let tick = crate::theme::Tokens::get(&h.ctx).ruler_tick;
        let count = |h: &egui_kittest::Harness<'_, DesignApp>| h.output().shapes.iter().map(|s| marks(&s.shape, tick)).sum::<usize>();
        assert!(count(&h) >= 2, "the fixture paints ruler ticks and an overset line");
        h.get_by(|n| n.role() == egui::accesskit::Role::MultilineTextInput).focus();
        h.run_steps(2);
        h.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Home);
        h.run_steps(2);
        h.event(egui::Event::Text("Inserted line\n".into()));
        h.step();
        assert_eq!(story_text(h.state(), 0), format!("Inserted line\n{text}"));
        assert_eq!(count(&h), 0, "the edit frame must not map old composition offsets to new text");
        h.step();
        assert!(count(&h) >= 2, "the next frame paints the updated composition");
    }

    /// The Show/Hide Depth Ruler menu label is translated in every interface language.
    #[test]
    fn depth_ruler_menu_label_is_translated_in_every_language() {
        for (lang, _) in crate::i18n::LANGUAGES.iter().filter(|(code, _)| !code.is_empty()) {
            assert_ne!(crate::i18n::tr(lang, "Show/Hide Depth Ruler"), "Show/Hide Depth Ruler", "{lang}");
        }
    }

    /// Showing or hiding the depth ruler keeps the text's focus and caret: typing after the toggle
    /// still reaches the story.
    #[test]
    fn depth_ruler_toggle_keeps_the_text_focused() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({})).unwrap();
        let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": "Ruler story", "caret": false})).unwrap();
        app.run("app.storyEditor", json!({"story": r["story"].as_u64().unwrap()})).unwrap();
        let mut h = crate::test_window::open(app, egui::vec2(1440.0, 900.0));
        h.get_by(|n| n.role() == egui::accesskit::Role::MultilineTextInput && n.value().as_deref() == Some("Ruler story")).focus();
        h.run_steps(2);
        for (toggle, typed) in [("!", "Ruler story!"), ("?", "Ruler story!?")] {
            h.state_mut().app.run("view.storyEditorDepthRuler", json!({})).unwrap();
            h.run_steps(2);
            h.event(egui::Event::Text(toggle.into()));
            h.run_steps(2);
            assert_eq!(story_text(&h.state().app, 0), typed);
        }
    }

    /// Story ids are counted per document: typing in the editor after switching documents edited
    /// the other document's story with the same id.
    #[test]
    fn editor_stays_with_its_document() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
        let mut stories = Vec::new();
        for text in ["Alpha story", "Bravo story"] {
            app.run("file.new", json!({})).unwrap();
            let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": text, "caret": false})).unwrap();
            stories.push(r["story"].as_u64().unwrap());
        }
        assert_eq!(stories[0], stories[1], "the fixture needs the same story id in both documents");
        app.run("file.activate", json!({"index": 0})).unwrap();
        app.run("app.storyEditor", json!({"story": stories[0]})).unwrap();
        let mut h = crate::test_window::open(app, egui::vec2(1440.0, 900.0));
        h.get_by(|n| n.role() == egui::accesskit::Role::MultilineTextInput && n.value().as_deref() == Some("Alpha story")).focus();
        h.run_steps(2);
        h.event(egui::Event::Text("!".into()));
        h.run_steps(2);
        let a = story_text(&h.state().app, 0);
        assert!(a.contains('!'), "typing reaches the open story: {a:?}");

        h.state_mut().app.run("file.activate", json!({"index": 1})).unwrap();
        h.run_steps(2);
        h.event(egui::Event::Text("?".into()));
        h.run_steps(2);
        assert_eq!(story_text(&h.state().app, 1), "Bravo story");
        assert_eq!(story_text(&h.state().app, 0), a);
        assert!(h.state().app.story_editor.is_some(), "the editor waits for its document");

        h.state_mut().app.run("file.close", json!({"index": 0})).unwrap();
        h.run_steps(2);
        assert_eq!(h.state().app.story_editor, None);
    }
}
