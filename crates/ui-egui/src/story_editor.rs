//! Story Editor (Edit → Edit in Story Editor, ⌘Y): the story as plain text with each paragraph's
//! style in the left column, the depth ruler beside it and the overset point marked. Edits are
//! applied as minimal range replacements, so formatting around them is kept.
//!
//! The editor is a float-only panel of the dock (`dock::FLOATING_PANELS`): a floating window that takes no other
//! panel.

use egui::pos2;
use serde_json::json;

use designcraft_compose::ComposedStory;
use designcraft_doc::StoryId;

use crate::DesignApp;
use crate::theme::Tokens;

/// The editor's panel id in the dock.
pub const PANEL: &str = "storyEditor";
/// Width of the style name column before its bar is dragged, in screen points.
pub const STYLE_COLUMN_WIDTH: f32 = 130.0;
/// Narrowest and widest the style name column can be dragged, in screen points.
const STYLE_COLUMN_RANGE: (f32, f32) = (40.0, 400.0);
/// Width of the bar between the style name column and the text, in screen points.
const STYLE_BAR_WIDTH: f32 = 6.0;
/// Width of the depth ruler, in screen points.
const RULER_WIDTH: f32 = 34.0;
/// Least distance between two of the depth ruler's labels, in screen points.
const RULER_LABEL_GAP: f32 = 10.0;
/// Color of the overset line and label.
const OVERSET_RED: egui::Color32 = egui::Color32::from_rgb(230, 40, 40);
/// Size of a style name in the column, and its row's height, in screen points.
const LABEL_SIZE: f32 = 11.0;
const LABEL_HEIGHT: f32 = 14.0;
/// Size of the depth ruler's labels, in screen points.
const RULER_LABEL_SIZE: f32 = 9.0;
/// Rows the text shows at least.
const TEXT_ROWS: usize = 20;

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

/// The editor's title while it shows a story: its name and the story's word count.
pub fn title(app: &DesignApp) -> Option<String> {
    let target = app.story_editor?;
    let story = app.session.active().filter(|st| st.uid == target.doc)?.doc.story(target.story)?;
    let words = story.text.split_whitespace().count().to_string();
    Some(crate::i18n::tr(&app.ui.language, "Story Editor — {count} words").replace("{count}", &words))
}

/// What the editor shows of its story this frame.
struct Snapshot {
    sid: StoryId,
    text: String,
    /// Each paragraph's style name, in the interface language.
    styles: Vec<String>,
    /// The first byte that does not fit the story's frames.
    overset: Option<usize>,
    /// The depth ruler's values, while it shows.
    depths: Option<Vec<(usize, f64)>>,
    unit: designcraft_geom::Unit,
    /// The style name column's width, while it shows.
    style_width: Option<f32>,
    /// The text size, in points.
    size: f32,
}

impl Snapshot {
    /// The story the editor is open on, while its document is the active one.
    fn of(app: &DesignApp) -> Option<Snapshot> {
        let target = app.story_editor?;
        let st = app.session.active().filter(|st| st.uid == target.doc)?;
        let sid = target.story;
        let story = st.doc.story(sid)?;
        let cs = app.session.cache.get(&st.doc, sid, None);
        let lang = app.ui.language.as_str();
        Some(Snapshot {
            sid,
            text: story.text.clone(),
            styles: story.paras.iter().map(|para| crate::i18n::style_name(lang, &para.style).to_owned()).collect(),
            overset: cs.overset_at,
            depths: app.ui.story_editor_depth_ruler.then(|| line_depths(&cs)),
            unit: st.doc.settings.vertical_units,
            style_width: app.ui.story_editor_style_column.then(|| style_column_width(app.ui.story_editor_style_width)),
            size: app.ui.story_editor_size.clamp(8.0, 36.0),
        })
    }

    /// The characters of the text after the overset point.
    fn overset_chars(&self, overset: usize) -> usize {
        self.text[overset.min(self.text.len())..].chars().count()
    }
}

/// What the person did in the editor this frame.
#[derive(Default)]
struct Changes {
    /// The text edit as a range replacement: start and end bytes of the story's text, and the new text.
    edit: Option<(usize, usize, String)>,
    /// The style name column's width the bar was dragged to.
    width: Option<f32>,
}

/// Applies `changes` to the story `sid`: the column width, then the text edit.
fn apply(app: &mut DesignApp, sid: StoryId, changes: Changes) {
    if let Some(w) = changes.width {
        let _ = app.run("view.storyEditorStyleColumn", json!({"width": w}));
    }
    if let Some((a, b, text)) = changes.edit {
        let _ = app.run("story.replaceRange", json!({"story": sid.0, "start": a, "end": b, "text": text}));
    }
}

/// Floats the editor's panel while it has a story, and closes it while it has none: `app.storyEditor` calls it after
/// it picked the story.
pub fn follow_target(app: &mut DesignApp) {
    let open = crate::panel_docking::floating_panels(app).iter().any(|p| p == PANEL);
    if app.story_editor.is_some() {
        let _ = crate::panel_docking::command(app, "window.panel", &json!({"panel": PANEL}));
    } else if open {
        let _ = crate::panel_docking::command(app, "window.panel.dock", &json!({"panel": PANEL}));
    }
}

/// Where the editor's window was when its document stopped being the active one, kept to open it there again.
#[derive(Clone, Copy)]
struct Parked {
    target: Target,
    at: Option<egui::Pos2>,
}

fn parked_id() -> egui::Id {
    egui::Id::new("story_editor_parked")
}

/// Keeps the editor with its story: it closes with its document or story, and hides while another document is active.
/// The dock draws its panel.
pub fn show(app: &mut DesignApp, ctx: &egui::Context) {
    if let Some(target) = app.story_editor {
        let doc = app.session.documents().iter().find(|d| d.uid == target.doc);
        let active = app.session.active().is_some_and(|st| st.uid == target.doc);
        if doc.is_none() || (active && doc.is_some_and(|d| d.doc.story(target.story).is_none())) {
            app.story_editor = None;
        }
    }
    sync_panel(app, ctx);
}

/// The editor's panel follows its story: it closes while another document is active and opens again where it was
/// when that document is back; a panel the person closed closes the editor.
fn sync_panel(app: &mut DesignApp, ctx: &egui::Context) {
    let open = crate::panel_docking::floating_panels(app).iter().any(|p| p == PANEL);
    let parked = ctx.data(|d| d.get_temp::<Parked>(parked_id()));
    let target = app.story_editor;
    match target {
        None => {
            ctx.data_mut(|d| d.remove::<Parked>(parked_id()));
            if open {
                let _ = crate::panel_docking::command(app, "window.panel.dock", &json!({"panel": PANEL}));
            }
        }
        Some(target) if visible(app) => {
            if open {
                return;
            }
            match parked.filter(|p| p.target == target) {
                Some(p) => {
                    ctx.data_mut(|d| d.remove::<Parked>(parked_id()));
                    let params = match p.at {
                        Some(at) => json!({"panel": PANEL, "x": at.x, "y": at.y}),
                        None => json!({"panel": PANEL}),
                    };
                    let _ = crate::panel_docking::command(app, "window.panel.float", &params);
                }
                // The person closed the window.
                None => app.story_editor = None,
            }
        }
        Some(target) => {
            if open {
                let at = crate::panel_docking::floating_rect(app, PANEL).map(|r| r.min);
                ctx.data_mut(|d| d.insert_temp(parked_id(), Parked { target, at }));
                let _ = crate::panel_docking::command(app, "window.panel.dock", &json!({"panel": PANEL}));
            }
        }
    }
}

/// The editor's contents in its panel.
pub fn body(app: &mut DesignApp, ui: &mut egui::Ui) {
    let Some(s) = Snapshot::of(app) else { return };
    let changes = framework::body(app, ui, &s);
    apply(app, s.sid, changes);
}

/// The edit that turns `old` into `new` as one range replacement: the bytes of `old` between their common prefix and
/// suffix, and what replaces them. `None` when they are equal.
fn minimal_edit(old: &str, new: &str) -> Option<(usize, usize, String)> {
    if old == new {
        return None;
    }
    let pre = old.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
    let pre = (0..=pre).rev().find(|&i| old.is_char_boundary(i) && new.is_char_boundary(i)).unwrap_or(0);
    let max_suf = old.len().min(new.len()) - pre;
    let suf = old.bytes().rev().zip(new.bytes().rev()).take(max_suf).take_while(|(a, b)| a == b).count();
    let suf = (0..=suf).rev().find(|&k| old.is_char_boundary(old.len() - k) && new.is_char_boundary(new.len() - k)).unwrap_or(0);
    Some((pre, old.len() - suf, new[pre..new.len() - suf].to_string()))
}

/// The depth ruler of `s` in `bar`, beside its text laid out as `galley` from screen y `top`: its strip, and a tick and
/// a label at the row each mark's line starts on.
fn paint_ruler(painter: &egui::Painter, s: &Snapshot, depths: &[(usize, f64)], bar: egui::Rect, galley: &egui::Galley, top: f32) {
    let t = Tokens::get(painter.ctx());
    let unit = s.unit;
    painter.rect_filled(bar, 0.0, t.ruler);
    let ticks = depth_ticks(depths, unit.ruler_ticks(1.0).0);
    let rows = start_rows(galley, &char_offsets(&s.text, ticks.iter().map(|tick| tick.0)));
    let marks = ticks.iter().zip(rows).map(|(&(_, depth), row)| (top + row.1, depth));
    for (y, depth) in spaced_marks(marks, RULER_LABEL_GAP) {
        painter.line_segment([pos2(bar.max.x - 8.0, y), pos2(bar.max.x, y)], egui::Stroke::new(1.0, t.ruler_tick));
        painter.text(
            pos2(bar.min.x + 2.0, y - 1.0),
            egui::Align2::LEFT_BOTTOM,
            crate::canvas::fmt_tick(unit.from_pt(depth)),
            egui::FontId::proportional(RULER_LABEL_SIZE),
            t.ruler_text,
        );
    }
}

/// The style name column's width for a saved or dragged `width`: kept between the narrowest and
/// widest the bar allows, and the starting width when `width` is not a number.
pub(crate) fn style_column_width(width: f32) -> f32 {
    if width.is_finite() { width.clamp(STYLE_COLUMN_RANGE.0, STYLE_COLUMN_RANGE.1) } else { STYLE_COLUMN_WIDTH }
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

/// The editor's body on the craft UI framework, while it is installed: one app-defined component holds the story's
/// text part and paints the style name column, its bar, the depth ruler and the overset line beside the rows the text
/// part laid out; the OVERSET line under it is framework text.
mod framework {
    use super::{
        Changes, LABEL_HEIGHT, LABEL_SIZE, OVERSET_RED, RULER_WIDTH, STYLE_BAR_WIDTH, Snapshot, TEXT_ROWS, char_offsets, minimal_edit, paint_ruler,
        paragraph_start_offsets, start_rows, style_column_width,
    };
    use crate::DesignApp;
    use crate::theme::Tokens;
    use craft_behavior::{CursorIcon, Metrics, PartKey, PlainText, PressPhase, Proposal, Sense};
    use craft_theme::variant::Tone;
    use craft_ui_widgets::contract::{
        Access, Bind, BindMut, Common, Component, Cx, Edit as _, EditResponse, Input, MeasureCx, Name, Outcome, PaintUi, PartsCx, Pointer as _,
        Props as _, Remember as _, Role, Show as _, ShowContainer as _, Sizing,
    };
    use craft_ui_widgets::display::{Shade, Text};
    use craft_ui_widgets::layout_components::{Row, Stack};
    use craft_ui_widgets::primitives::{TextKey, TextLayout, TextOptions};
    use craft_ui_widgets::style::{ComponentStyle, StyleCx, TypeRole};
    use egui::{Pos2, Rect, Response, Stroke, Vec2, pos2, vec2};

    const TEXT: PartKey = PartKey::new("text");
    const BAR: PartKey = PartKey::new("bar");
    const STYLE: PartKey = PartKey::new("style");
    /// The least room the text takes beside the columns, in screen points.
    const LEAST_TEXT: Vec2 = Vec2::new(120.0, 60.0);

    fn text_key() -> TextKey<String> {
        TextKey::new(TEXT)
    }

    /// The look of the editor's body: no surface of its own; the text part and the columns are the body.
    #[derive(Clone, Debug, PartialEq)]
    pub(super) struct StoryTextStyle;

    impl ComponentStyle for StoryTextStyle {
        fn metrics(&self) -> Metrics {
            Metrics::new()
        }
    }

    /// A drag of the bar: the column's width and the pointer's x where it started.
    #[derive(Clone, Default)]
    struct Held {
        drag: Option<(f32, f32)>,
    }

    /// What the body reports: the text's edit and the column width the bar was dragged to in this pass.
    #[derive(Debug)]
    pub(super) struct StoryTextOutput {
        pub(super) edit: EditResponse<String>,
        pub(super) width: Option<f32>,
    }

    impl Outcome for StoryTextOutput {
        type Value = String;

        fn response(&self) -> &Response {
            &self.edit.response
        }
    }

    /// Where the body's columns are: the style names and their bar while the column shows, the depth ruler while it
    /// shows, and the text in the rest, left to right.
    struct Geometry {
        column: Option<Rect>,
        bar: Option<Rect>,
        ruler: Option<Rect>,
        text: Rect,
    }

    fn geometry(s: &Snapshot, rect: Rect) -> Geometry {
        let mut x = rect.min.x;
        let mut take = |width: Option<f32>| {
            width.map(|width| {
                let taken = Rect::from_x_y_ranges(x..=(x + width).min(rect.max.x), rect.y_range());
                x = taken.max.x;
                taken
            })
        };
        let column = take(s.style_width);
        let bar = take(s.style_width.map(|_| STYLE_BAR_WIDTH));
        let ruler = take(s.depths.as_ref().map(|_| RULER_WIDTH));
        let text = Rect::from_x_y_ranges(x.min(rect.max.x)..=rect.max.x, rect.y_range());
        Geometry { column, bar, ruler, text }
    }

    /// Each paragraph's style name and the screen y of the first row it is laid out on.
    fn labels<'s>(s: &'s Snapshot, layout: &TextLayout, origin: Pos2) -> impl Iterator<Item = (&'s String, f32)> {
        let starts = paragraph_start_offsets(layout.galley.rows.iter().map(|row| (row.pos.y, row.ends_with_newline)));
        s.styles.iter().zip(starts).map(move |(name, y)| (name, origin.y + y))
    }

    /// The story's text beside its paragraphs' style names and its depth ruler. The text part is the framework's
    /// multi-line text at the editor's text size; the columns are painted at the rows it laid out this pass. The bar
    /// between the names and the text resizes the column with the pointer; the column's width is also a View menu
    /// command, so the bar takes no keys, as the native bar takes none.
    #[derive(craft_ui_widgets::contract::Component, craft_ui_widgets::contract::Props)]
    #[component(content_painter)]
    pub(super) struct StoryText<'a> {
        #[common]
        common: Common,
        value: Bind<'a, String>,
        s: &'a Snapshot,
        /// The bar's accessible name.
        divider: String,
        width: Option<f32>,
    }

    impl Component for StoryText<'_> {
        const ROLE: Role = Role::MULTILINE_TEXT_INPUT;
        const INPUT: Input = Input::Text;
        const OVERLAYS: bool = false;
        type Style = StoryTextStyle;
        type Output = StoryTextOutput;

        fn style(_: &StyleCx<'_>, _: StoryTextStyleKey) -> StoryTextStyle {
            StoryTextStyle
        }

        fn sense(&self) -> Sense {
            Sense::CLICK | Sense::FOCUS | Sense::TAKE_FOCUS
        }

        /// The room it is given, as a filling multi-line field takes; at least the columns and a little text.
        fn measure(&mut self, cx: &MeasureCx<'_, Self>) -> Vec2 {
            if let Proposal::Size(room) = cx.proposal
                && room.x.is_finite()
                && room.y.is_finite()
            {
                return room;
            }
            let columns = self.s.style_width.map_or(0.0, |w| w + STYLE_BAR_WIDTH) + self.s.depths.as_ref().map_or(0.0, |_| RULER_WIDTH);
            vec2(columns + LEAST_TEXT.x, LEAST_TEXT.y)
        }

        fn parts(&mut self, _: Rect, cx: &mut PartsCx<'_, Self>) {
            let g = geometry(self.s, cx.content_rect);
            if let Some(bar) = g.bar {
                let _ = cx.part(BAR, bar).sense(Sense::DRAG).cursor(CursorIcon::ResizeHorizontal).drag_cursor(CursorIcon::ResizeHorizontal);
            }
            let mut options = TextOptions::new();
            options.lines = Some(TEXT_ROWS);
            options.size = Some(self.s.size);
            // The text starts at the top of the editor, beside the style names and the ruler that follow its rows.
            options.top = true;
            // The editor has no edit menu of its own, as the native editor has none.
            options.own_menu = true;
            if let Some(value) = cx.value() {
                cx.text(text_key(), g.text, &PlainText, value, &options);
            }
        }

        fn bind(&mut self) -> Option<BindMut<'_, String>> {
            Some(self.value.reborrow())
        }

        /// A drag of the bar sets the column's width: the width it had when the drag started plus how far the pointer
        /// has moved since, so the bar comes back with the pointer after the drag passes the narrowest or widest width.
        fn update(&mut self, cx: &mut Cx<'_, Self>) {
            let Some(width) = self.s.style_width else { return };
            let mut held = cx.memory::<Held>().clone();
            let to = |held: &Held, x: f32| held.drag.map(|(width, origin)| style_column_width(width + x - origin));
            match cx.part(BAR).press.phase {
                PressPhase::DragStart { start, pos, .. } => {
                    held.drag = Some((width, start.origin.x));
                    self.width = to(&held, pos.x);
                }
                PressPhase::Drag { pos, .. } => self.width = to(&held, pos.x),
                PressPhase::Release { .. } => held.drag = None,
                _ => {}
            }
            *cx.memory::<Held>() = held;
        }

        fn paint(&mut self, ui: &mut PaintUi<'_>, cx: &Cx<'_, Self>) {
            let content = ui.content_painter(cx);
            let painter = &content.painter;
            let t = Tokens::get(painter.ctx());
            let g = geometry(self.s, cx.content_rect);
            if let Some(bar) = g.bar {
                let part = cx.part(BAR);
                let lit = part.hovered || part.press.phase.is_drag();
                let line = Rect::from_center_size(bar.center(), vec2(1.0, bar.height()));
                painter.rect_filled(line, 0.0, if lit { t.text_dim } else { t.section_divider });
            }
            let Some(outcome) = cx.text(&text_key()) else { return };
            let Some(layout) = outcome.layout.as_ref() else { return };
            let origin = outcome.origin;
            if let Some(column) = g.column {
                for (name, y) in labels(self.s, layout, origin) {
                    let galley = crate::rtl::truncated(painter.ctx(), name, egui::FontId::proportional(LABEL_SIZE), t.text_dim, column.width());
                    painter.galley(pos2(column.min.x, y), galley, t.text_dim);
                }
            }
            // The ruler and the overset line place the composed story's offsets on the rows: not in a pass whose typing
            // the story does not hold yet, whose rows show other text.
            if layout.full() != self.s.text {
                return;
            }
            let overset_y = self.s.overset.map(|o| {
                let at = char_offsets(&self.s.text, std::iter::once(o));
                origin.y + start_rows(&layout.galley, &at).first().map_or(0.0, |row| row.0)
            });
            if let (Some(ruler), Some(depths)) = (g.ruler, self.s.depths.as_ref()) {
                let bottom = overset_y.unwrap_or(origin.y + layout.galley.size().y);
                let bar = Rect::from_min_max(pos2(ruler.min.x, origin.y), pos2(ruler.max.x, bottom));
                paint_ruler(painter, self.s, depths, bar, &layout.galley, origin.y);
            }
            // The overset line: across the text, and the ruler when it shows, above the first character that doesn't fit.
            if let Some(y) = overset_y {
                let x0 = g.ruler.map_or(g.text.min.x, |ruler| ruler.min.x);
                painter.line_segment([pos2(x0, y), pos2(g.text.max.x, y)], Stroke::new(1.5, OVERSET_RED));
            }
        }

        /// The bar, and each style name the column shows, as nodes of their own.
        fn access(&self, access: &mut Access, cx: &Cx<'_, Self>) {
            let g = geometry(self.s, cx.content_rect);
            if let Some(bar) = g.bar {
                let node = access.node(BAR, Role::SPLITTER);
                node.name = Name::Text(self.divider.clone());
                node.bounds(bar);
            }
            let (Some(column), Some(outcome)) = (g.column, cx.text(&text_key())) else { return };
            let Some(layout) = outcome.layout.as_ref() else { return };
            for (at, (name, y)) in labels(self.s, layout, outcome.origin).enumerate() {
                let rect = Rect::from_min_size(pos2(column.min.x, y), vec2(column.width(), LABEL_HEIGHT));
                if !rect.intersects(cx.rect) {
                    continue;
                }
                let node = access.node(STYLE.at(at as u64), Role::LABEL);
                node.name = Name::Text(name.clone());
                node.bounds(rect.intersect(cx.rect));
            }
        }

        fn finish(self, cx: Cx<'_, Self>) -> StoryTextOutput {
            StoryTextOutput { edit: cx.edit_response(), width: self.width }
        }
    }

    /// The editor's body: the text with its columns, filling the panel, and the OVERSET line under it while text does
    /// not fit.
    pub(super) fn body(app: &DesignApp, ui: &mut egui::Ui, s: &Snapshot) -> Changes {
        let lang = app.ui.language.as_str();
        let tr = |text: &str| crate::i18n::tr(lang, text).to_owned();
        let mut changes = Changes::default();
        let room = ui.available_rect_before_wrap();
        Stack::new().id_salt("story_editor_body").fill().show_at(ui, room, |ui, _| {
            // The text has a fixed id, so it keeps its focus and caret when the columns before it show or hide.
            let editor =
                StoryText { common: Common::new(), value: Bind::given(s.text.clone()), s, divider: tr("Style name column divider"), width: None }
                    .label(tr("Story Editor"))
                    .id_salt("story_editor_text")
                    .sizing(Sizing::Fill)
                    .show(ui);
            changes.width = editor.width;
            changes.edit = editor.edit.value().and_then(|text| minimal_edit(&s.text, text));
            if let Some(o) = s.overset {
                Row::new().id_salt("story_editor_overset").show(ui, |ui, _| {
                    Text::new(tr("OVERSET")).role(TypeRole::BodyStrong).tone(Tone::Bad).show(ui);
                    Text::new(tr("{count} characters don't fit").replace("{count}", &s.overset_chars(o).to_string())).shade(Shade::Muted).show(ui);
                });
            }
        });
        changes
    }
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

    fn story_editor_harness(text: &str) -> egui_kittest::Harness<'static, crate::test_window::Window> {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({})).unwrap();
        let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": text, "caret": false})).unwrap();
        app.run("app.storyEditor", json!({"story": r["story"].as_u64().unwrap()})).unwrap();
        let mut h = crate::test_window::open(app, egui::vec2(1440.0, 900.0));
        h.run_steps(2);
        h
    }

    fn text_left(h: &egui_kittest::Harness<'_, crate::test_window::Window>, text: &str) -> f32 {
        h.get_by(|n| n.role() == egui::accesskit::Role::MultilineTextInput && n.value().as_deref() == Some(text)).rect().min.x
    }

    /// View › Story Editor › Show/Hide Style Name Column: hiding the column takes away the style
    /// names and the bar beside them, and the text moves left by their width; the setting is
    /// remembered.
    #[test]
    fn style_name_column_shows_and_hides_from_the_view_menu() {
        let mut h = story_editor_harness("Column story");
        assert!(h.state().app.ui.story_editor_style_column, "shown by default");
        let app = &h.state().app;
        let para_style = &app.session.active().unwrap().doc.stories.values().next().unwrap().paras[0].style;
        let style = crate::i18n::style_name(&app.ui.language, para_style).to_string();
        assert!(h.query_by_label(&style).is_some(), "the column names the paragraph's style {style:?}");
        assert!(h.query_by_label("Style name column divider").is_some());
        let with = text_left(&h, "Column story");
        h.state_mut().app.run("view.storyEditorStyleColumn", json!({})).unwrap();
        h.run_steps(2);
        let without = text_left(&h, "Column story");
        assert!(h.query_by_label(&style).is_none(), "no style names while the column is hidden");
        assert!(h.query_by_label("Style name column divider").is_none());
        assert!(
            (with - without - super::STYLE_COLUMN_WIDTH - super::STYLE_BAR_WIDTH).abs() < 20.0,
            "the text moves by the column: {with} vs {without}"
        );
        let app = &h.state().app;
        assert_eq!(crate::menus::checked(app, "view.storyEditorStyleColumn", &json!({})), Some(false));
        let saved = serde_json::to_vec(&app.ui).unwrap();
        assert!(!crate::UiState::from_preferences_json(&saved).unwrap().story_editor_style_column);
    }

    /// A style name wider than the column is cut short at the column's edge, so it does not run
    /// into the story text.
    #[test]
    fn a_long_style_name_stays_in_the_column() {
        let mut h = story_editor_harness("Narrow story");
        h.state_mut().app.run("view.storyEditorStyleColumn", json!({"width": 40})).unwrap();
        h.run_steps(2);
        let app = &h.state().app;
        let para_style = &app.session.active().unwrap().doc.stories.values().next().unwrap().paras[0].style;
        let style = crate::i18n::style_name(&app.ui.language, para_style).to_string();
        let name = h.get_by_label(&style).rect();
        let text = text_left(&h, "Narrow story");
        assert!(name.width() <= 40.5, "the name {style:?} is cut to the column: {name:?}");
        assert!(name.max.x <= text, "the name ends before the text: {name:?}, text at {text}");
    }

    /// Dragging the bar between the style names and the text resizes the column; the text moves
    /// with the bar, and the width is remembered for the next time the editor opens.
    #[test]
    fn dragging_the_style_column_bar_resizes_the_column() {
        use egui::{Event, Modifiers, PointerButton};
        let mut h = story_editor_harness("Resize story");
        let before = text_left(&h, "Resize story");
        let a = h.get_by_label("Style name column divider").rect().center();
        let b = a + egui::vec2(60.0, 0.0);
        h.input_mut().events.push(Event::PointerMoved(a));
        h.input_mut().events.push(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        h.step();
        for k in 1..=4 {
            h.input_mut().events.push(Event::PointerMoved(a + (b - a) * (k as f32 / 4.0)));
            h.step();
        }
        h.input_mut().events.push(Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        h.run_steps(3);
        let width = h.state().app.ui.story_editor_style_width;
        assert!((width - super::STYLE_COLUMN_WIDTH - 60.0).abs() < 1.0, "the column is 60 points wider: {width}");
        let after = text_left(&h, "Resize story");
        assert!((after - before - 60.0).abs() < 1.0, "the text moves with the bar: {before} → {after}");
        let saved = serde_json::to_vec(&h.state().app.ui).unwrap();
        assert_eq!(crate::UiState::from_preferences_json(&saved).unwrap().story_editor_style_width, width);
    }

    /// After a drag passes the widest width, moving the pointer back brings the bar back with it:
    /// the width is the starting width plus how far the pointer moved, kept in range.
    #[test]
    fn the_style_column_bar_follows_the_pointer_back_from_a_limit() {
        use egui::{Event, Modifiers, PointerButton};
        let mut h = story_editor_harness("Limit story");
        let a = h.get_by_label("Style name column divider").rect().center();
        h.input_mut().events.push(Event::PointerMoved(a));
        h.input_mut().events.push(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        h.step();
        for dx in [100.0, 200.0, 300.0, 400.0, 300.0, 200.0] {
            h.input_mut().events.push(Event::PointerMoved(a + egui::vec2(dx, 0.0)));
            h.step();
        }
        let b = a + egui::vec2(200.0, 0.0);
        h.input_mut().events.push(Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        h.run_steps(3);
        let width = h.state().app.ui.story_editor_style_width;
        assert!((width - super::STYLE_COLUMN_WIDTH - 200.0).abs() < 1.0, "the column is 200 points wider: {width}");
    }

    /// A saved width out of range, or not a number, gives a column the bar can still be dragged in.
    #[test]
    fn style_column_width_stays_in_range() {
        assert_eq!(super::style_column_width(200.0), 200.0);
        assert_eq!(super::style_column_width(-50.0), super::STYLE_COLUMN_RANGE.0);
        assert_eq!(super::style_column_width(1e9), super::STYLE_COLUMN_RANGE.1);
        assert_eq!(super::style_column_width(f32::NAN), super::STYLE_COLUMN_WIDTH);
        assert_eq!(super::style_column_width(f32::INFINITY), super::STYLE_COLUMN_WIDTH);
    }

    /// Showing or hiding the style name column or the depth ruler keeps the text's focus and caret:
    /// typing goes on into the story.
    #[test]
    fn typing_goes_on_after_the_columns_show_or_hide() {
        let mut h = story_editor_harness("Focus story");
        h.get_by(|n| n.role() == egui::accesskit::Role::MultilineTextInput && n.value().as_deref() == Some("Focus story")).focus();
        h.run_steps(2);
        h.event(egui::Event::Text("1".into()));
        h.run_steps(2);
        for (i, id) in ["view.storyEditorStyleColumn", "view.storyEditorDepthRuler", "view.storyEditorStyleColumn"].into_iter().enumerate() {
            h.state_mut().app.run(id, json!({})).unwrap();
            h.run_steps(2);
            h.event(egui::Event::Text((i + 2).to_string()));
            h.run_steps(2);
        }
        let text = story_text(&h.state().app, 0);
        assert!(text.contains("1234"), "every key reaches the story at the caret: {text:?}");
    }

    /// `view.storyEditorStyleColumn` sets the column's width and whether it shows, as the bar and
    /// the menu item do, and toggles it without either.
    #[test]
    fn the_style_column_command_takes_a_width() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
        assert_eq!(app.run("view.storyEditorStyleColumn", json!({"width": 250})).unwrap(), json!({"on": true, "width": 250.0}));
        assert_eq!(app.run("view.storyEditorStyleColumn", json!({"width": 1000})).unwrap()["width"], json!(400.0));
        assert_eq!(app.run("view.storyEditorStyleColumn", json!({"width": -1e40})).unwrap()["width"], json!(40.0));
        assert_eq!(app.run("view.storyEditorStyleColumn", json!({"width": 1e40})).unwrap()["width"], json!(400.0));
        assert_eq!(app.run("view.storyEditorStyleColumn", json!({"on": false})).unwrap(), json!({"on": false, "width": 400.0}));
        assert_eq!(app.run("view.storyEditorStyleColumn", json!({})).unwrap()["on"], json!(true));
        assert!(app.run("view.storyEditorStyleColumn", json!({"width": "wide"})).is_err());
        assert!(app.run("view.storyEditorStyleColumn", json!({"on": 1})).is_err());
        assert_eq!((app.ui.story_editor_style_column, app.ui.story_editor_style_width), (true, 400.0));
    }

    /// The Show/Hide Depth Ruler menu label is translated in every interface language.
    #[test]
    fn depth_ruler_menu_label_is_translated_in_every_language() {
        for (lang, _) in crate::i18n::LANGUAGES.iter().filter(|(code, _)| !code.is_empty()) {
            assert_ne!(crate::i18n::tr(lang, "Show/Hide Depth Ruler"), "Show/Hide Depth Ruler", "{lang}");
        }
    }

    /// The Show/Hide Style Name Column menu label is translated in every interface language.
    #[test]
    fn style_name_column_menu_label_is_translated_in_every_language() {
        for (lang, _) in crate::i18n::LANGUAGES.iter().filter(|(code, _)| !code.is_empty()) {
            assert_ne!(crate::i18n::tr(lang, "Show/Hide Style Name Column"), "Show/Hide Style Name Column", "{lang}");
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

    /// The editor's panel follows its document: it closes while another document is active and opens again with it;
    /// closing the panel's window closes the editor.
    #[test]
    fn the_editor_panel_follows_its_document_and_its_window() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), Default::default());
        let mut stories = Vec::new();
        for text in ["Alpha story", "Bravo story"] {
            app.run("file.new", json!({})).unwrap();
            let r = app.run("frame.create", json!({"rect": [72, 72, 300, 200], "content": "text", "text": text, "caret": false})).unwrap();
            stories.push(r["story"].as_u64().unwrap());
        }
        app.run("file.activate", json!({"index": 0})).unwrap();
        app.run("app.storyEditor", json!({"story": stories[0]})).unwrap();
        let mut h = crate::test_window::open(app, egui::vec2(1440.0, 900.0));
        let open = |h: &egui_kittest::Harness<'_, crate::test_window::Window>| {
            crate::panel_docking::floating_panels(&h.state().app).iter().any(|p| p == super::PANEL)
        };
        assert!(open(&h), "the editor opens as a floating panel");
        h.state_mut().app.run("file.activate", json!({"index": 1})).unwrap();
        h.run_steps(2);
        assert!(!open(&h), "the panel closes while another document is active");
        assert!(h.state().app.story_editor.is_some(), "the editor waits for its document");
        h.state_mut().app.run("file.activate", json!({"index": 0})).unwrap();
        h.run_steps(2);
        assert!(open(&h), "the panel opens again with its document");
        h.state_mut().app.run("window.panel.close", json!({"panel": super::PANEL})).unwrap();
        h.run_steps(2);
        assert_eq!(h.state().app.story_editor, None, "closing the panel's window closes the editor");
    }
}

/// The editor's body on the framework, alone on a context with the framework's runtime, driven by real pointer and
/// key events.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod framework_tests {
    use egui::{Event, Key, Modifiers, PointerButton, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT as _, Queryable};
    use serde_json::{Value, json};

    use crate::DesignApp;

    type Window = Harness<'static, DesignApp>;

    /// The editor open on a story of `text` set in a frame at `frame`, its body alone in a panel of `size`.
    fn open(text: &str, frame: [f64; 4], size: egui::Vec2) -> Window {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.language = "en".to_owned();
        app.run("file.new", json!({})).unwrap();
        let r = app.run("frame.create", json!({"rect": frame, "content": "text", "text": text, "caret": false})).unwrap();
        app.run("app.storyEditor", json!({"story": r["story"].as_u64().unwrap()})).unwrap();
        let ctx = egui::Context::default();
        app.install_framework(&ctx);
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(size).with_step_dt(0.05).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                    crate::theme::apply(ui.ctx(), &crate::theme::Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                super::body(app, ui);
            },
            app,
        );
        h.input_mut().max_texture_side = Some(8192);
        steps(&mut h, 4);
        h
    }

    fn steps(h: &mut Window, count: usize) {
        for _ in 0..count {
            let mut raw = std::mem::take(h.input_mut());
            let ctx = h.ctx.clone();
            h.state_mut().raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
    }

    fn event(h: &mut Window, event: Event) {
        h.input_mut().events.push(event);
        steps(h, 1);
    }

    fn key(h: &mut Window, key: Key) {
        event(h, Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
        event(h, Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    }

    /// Presses at `from`, moves to `to` in steps and releases there.
    fn drag(h: &mut Window, from: egui::Pos2, to: egui::Pos2) {
        event(h, Event::PointerMoved(from));
        event(h, Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        for k in 1..=6 {
            event(h, Event::PointerMoved(from.lerp(to, k as f32 / 6.0)));
        }
        event(h, Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        steps(h, 3);
    }

    fn story_text(h: &Window) -> String {
        h.state().session.active().unwrap().doc.stories.values().next().unwrap().text.clone()
    }

    /// The paragraph style name the column shows for the story's first paragraph.
    fn style_name(h: &Window) -> String {
        let app = h.state();
        let style = &app.session.active().unwrap().doc.stories.values().next().unwrap().paras[0].style;
        crate::i18n::style_name(&app.ui.language, style).to_owned()
    }

    /// The parameters of every `story.replaceRange` the session ran.
    fn replacements(h: &Window) -> Vec<Value> {
        h.state().session.journal.iter().filter(|(id, _)| id == "story.replaceRange").map(|(_, params)| params.clone()).collect()
    }

    /// The overset lines painted this frame.
    fn overset_lines(h: &Window) -> usize {
        fn count(shape: &egui::epaint::Shape) -> usize {
            match shape {
                egui::epaint::Shape::Vec(shapes) => shapes.iter().map(count).sum(),
                egui::epaint::Shape::LineSegment { stroke, .. } if stroke.color == super::OVERSET_RED => 1,
                _ => 0,
            }
        }
        h.output().shapes.iter().map(|clipped| count(&clipped.shape)).sum()
    }

    /// A key typed in the text reaches the story as one range replacement of what changed: an insert at the caret,
    /// the rest of the story kept.
    #[test]
    fn typing_applies_one_minimal_range_replacement() {
        let mut h = open("Typed story", [72.0, 72.0, 300.0, 200.0], vec2(560.0, 420.0));
        // Focus goes to the text, its caret at the end.
        key(&mut h, Key::Tab);
        steps(&mut h, 2);
        let before = replacements(&h);
        event(&mut h, Event::Text("!".into()));
        steps(&mut h, 3);
        assert_eq!(story_text(&h), "Typed story!");
        let after = replacements(&h);
        assert_eq!(after.len(), before.len() + 1, "one key, one replacement: {after:?}");
        let last = after.last().unwrap();
        assert_eq!((&last["start"], &last["end"], &last["text"]), (&json!(11), &json!(11), &json!("!")), "only the insert: {last}");
    }

    /// Each style name sits at the row its paragraph starts on: a paragraph that wraps over several rows puts the next
    /// paragraph's name that many rows further down, while a one-row paragraph puts it one row down.
    #[test]
    fn style_names_sit_at_the_rows_their_paragraphs_start_on() {
        let long = "Words that wrap over several rows of the editor. ".repeat(8);
        let mut h = open(&format!("First\n{long}\nThird"), [72.0, 72.0, 372.0, 672.0], vec2(560.0, 600.0));
        steps(&mut h, 2);
        let style = style_name(&h);
        let mut tops: Vec<f32> = h.query_all_by_label(&style).map(|node| node.rect().min.y).collect();
        tops.sort_by(f32::total_cmp);
        assert_eq!(tops.len(), 3, "a name for each paragraph: {tops:?}");
        let (one_row, wrapped) = (tops[1] - tops[0], tops[2] - tops[1]);
        assert!(one_row > 0.0, "the second paragraph starts below the first: {tops:?}");
        assert!(wrapped > 2.0 * one_row, "the wrapped paragraph's rows come before the third name: {tops:?}");
    }

    /// Dragging the bar between the style names and the text runs the column width command with the width the
    /// pointer moved it to, and the names take the wider column.
    #[test]
    fn dragging_the_bar_resizes_the_style_column() {
        let mut h = open("Resize story", [72.0, 72.0, 300.0, 200.0], vec2(560.0, 420.0));
        let style = style_name(&h);
        let name_before = h.get_by_label(&style).rect();
        let a = h.get_by_label("Style name column divider").rect().center();
        drag(&mut h, a, a + vec2(60.0, 0.0));
        let width = h.state().ui.story_editor_style_width;
        assert!((width - super::STYLE_COLUMN_WIDTH - 60.0).abs() < 1.0, "the column is 60 points wider: {width}");
        let name_after = h.get_by_label(&style).rect();
        assert!(name_after.width() > name_before.width() + 50.0, "the name's column widened: {name_before:?} → {name_after:?}");
        let bar = h.get_by_label("Style name column divider").rect().center();
        assert!((bar.x - a.x - 60.0).abs() < 1.0, "the bar moved with the pointer: {a:?} → {bar:?}");
    }

    /// Text that does not fit its frames shows the overset line and the OVERSET label; a story that fits shows
    /// neither.
    #[test]
    fn overset_text_shows_the_overset_line_and_label() {
        let mut h = open(&"First line\nSecond line\nThird line\n".repeat(4), [72.0, 72.0, 372.0, 172.0], vec2(560.0, 520.0));
        steps(&mut h, 2);
        assert!(h.query_by_label("OVERSET").is_some(), "the OVERSET label shows");
        assert_eq!(overset_lines(&h), 1, "one overset line");
        let mut fits = open("Fits", [72.0, 72.0, 372.0, 672.0], vec2(560.0, 520.0));
        steps(&mut fits, 2);
        assert!(fits.query_by_label("OVERSET").is_none());
        assert_eq!(overset_lines(&fits), 0);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod successor_tests;
