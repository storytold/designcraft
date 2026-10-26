//! The Font menu, laid out like InDesign's: a search field, a Filters row (classification,
//! favourites, recently added, similar fonts), the sample text and its size, then the recently used
//! fonts and every family. A family with several styles shows how many and opens to list them,
//! each previewed in its own face; the format of each family shows after its sample.

use std::collections::HashSet;

use designcraft_fonts::{FaceKind, FamilyInfo, FontClass, FontFormat};
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::DesignApp;
use crate::theme::Tokens;

/// A font picked in the menu: the family, and the style when one of its styles was picked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontPick {
    pub family: String,
    pub style: Option<String>,
}

/// The sample texts the menu can preview (the first is the text selection's own).
pub const SAMPLES: [(&str, &str); 5] = [
    ("selected", "Selected Text"),
    ("sample", "Sample"),
    ("pairs", "Aa Bb Cc"),
    ("pangram", "The quick brown fox jumps over the lazy dog"),
    ("digits", "0123456789"),
];
/// Preview sizes: (row height, sample size in points).
const SIZES: [(f32, f32); 3] = [(22.0, 13.0), (28.0, 17.0), (40.0, 26.0)];
/// Fonts listed as recently used, and a recently added font's age.
const RECENT: usize = 10;
const ADDED_WITHIN: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 60 * 60);
/// Families Show Similar Fonts lists.
const SIMILAR: usize = 30;

/// The menu's state while it is open.
#[derive(Clone, Default)]
struct MenuState {
    query: String,
    only_favs: bool,
    only_recent: bool,
    class: Option<FontClass>,
    /// Show Similar Fonts: the family the list is like.
    similar: Option<String>,
    expanded: HashSet<String>,
    /// The Filters and Sample choices, shown in the menu (a menu can't hold another popup).
    show_classes: bool,
    show_samples: bool,
    highlight: Option<usize>,
    frame: Option<u64>,
}

/// What the menu knows about a family it shows (read once, then kept for the session).
#[derive(Clone)]
struct FamilyMeta {
    styles: Vec<String>,
    kind: FaceKind,
}

fn regular(styles: &[String]) -> String {
    styles.iter().find(|s| *s == "Regular").or(styles.first()).cloned().unwrap_or_else(|| "Regular".into())
}

fn meta(ui: &Ui, fonts: &designcraft_fonts::ScopedFonts<'static>, scope: u32, family: &str) -> FamilyMeta {
    let key = egui::Id::unique(("font_meta", family, scope));
    if let Some(m) = ui.data(|d| d.get_temp::<FamilyMeta>(key)) {
        return m;
    }
    let styles = fonts.styles(family);
    let kind = fonts.face(family, &regular(&styles)).kind();
    let m = FamilyMeta { styles, kind };
    ui.data_mut(|d| d.insert_temp(key, m.clone()));
    m
}

/// Every family's kind and whether it was added recently: the class and Recently Added filters and
/// Show Similar Fonts need them for all families at once, and reading them opens each font file,
/// so they are read on a background thread, once per document font scope.
#[derive(Default)]
struct Kinds {
    scope: Option<u32>,
    of: std::collections::HashMap<String, (FaceKind, bool)>,
    done: bool,
}

type SharedKinds = std::sync::Arc<std::sync::Mutex<Kinds>>;

fn lock(kinds: &SharedKinds) -> std::sync::MutexGuard<'_, Kinds> {
    kinds.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A family's kind, and whether its file was added within [`ADDED_WITHIN`]. Read without keeping
/// the family loaded: the filters look at every installed family.
fn family_kind(fonts: &designcraft_fonts::ScopedFonts<'static>, family: &str) -> (FaceKind, bool) {
    fonts
        .family_kind(family)
        .map_or((FaceKind::default(), false), |(kind, added)| (kind, added.and_then(|t| t.elapsed().ok()).is_some_and(|age| age < ADDED_WITHIN)))
}

/// The families' kinds, starting to read them the first time they're needed for `scope`.
fn kinds(ctx: &egui::Context, fonts: &designcraft_fonts::ScopedFonts<'static>, scope: u32) -> SharedKinds {
    let shared: SharedKinds = ctx.data_mut(|d| d.get_temp_mut_or_default::<SharedKinds>(egui::Id::unique("font_kinds")).clone());
    let start = {
        let mut k = lock(&shared);
        let start = k.scope != Some(scope);
        if start {
            *k = Kinds { scope: Some(scope), ..Default::default() };
        }
        start
    };
    if start {
        let (job_ctx, job_kinds, job_fonts) = (ctx.clone(), shared.clone(), fonts.clone());
        let job = move || read_kinds(&job_ctx, &job_kinds, &job_fonts, scope);
        #[cfg(not(target_arch = "wasm32"))]
        if std::thread::Builder::new().name("font-kinds".into()).spawn(job).is_err() {
            // No thread to spare: read them here.
            read_kinds(ctx, &shared, fonts, scope);
        }
        // The web has no threads, and only its few bundled fonts.
        #[cfg(target_arch = "wasm32")]
        job();
    }
    shared
}

fn read_kinds(ctx: &egui::Context, shared: &SharedKinds, fonts: &designcraft_fonts::ScopedFonts<'static>, scope: u32) {
    for chunk in fonts.families().chunks(64) {
        let read: Vec<(String, (FaceKind, bool))> = chunk.iter().map(|f| (f.clone(), family_kind(fonts, f))).collect();
        let mut k = lock(shared);
        // Another document's fonts were asked for since.
        if k.scope != Some(scope) {
            return;
        }
        k.of.extend(read);
        drop(k);
        ctx.request_repaint();
    }
    let mut k = lock(shared);
    if k.scope == Some(scope) {
        k.done = true;
    }
    drop(k);
    ctx.request_repaint();
}

/// One row of the list.
#[derive(Clone)]
enum Row {
    /// A family; `recent` rows are the Recently Used ones at the top.
    Family {
        info: FamilyInfo,
        recent: bool,
    },
    Style {
        family: String,
        style: String,
    },
    Separator,
}

/// A five-pointed star (filled, or outlined).
fn paint_star(p: &egui::Painter, c: egui::Pos2, r: f32, filled: bool, color: Color32) {
    let pt = |i: usize, rad: f32| {
        let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
        c + egui::vec2(a.cos(), a.sin()) * rad
    };
    let outline: Vec<egui::Pos2> = (0..10).map(|i| pt(i, if i % 2 == 0 { r } else { r * 0.42 })).collect();
    if filled {
        // The inner pentagon and the five points (each convex).
        let inner: Vec<egui::Pos2> = (0..5).map(|i| outline[2 * i + 1]).collect();
        p.add(egui::Shape::convex_polygon(inner, color, Stroke::NONE));
        for i in 0..5 {
            p.add(egui::Shape::convex_polygon(vec![outline[(2 * i + 9) % 10], outline[2 * i], outline[2 * i + 1]], color, Stroke::NONE));
        }
    } else {
        p.add(egui::Shape::closed_line(outline, Stroke::new(1.0, color)));
    }
}

/// The format badge after a family's sample: TT, O (PostScript outlines), a variable font's
/// slider, or a colour font's four dots (in `color` and `accent`).
fn paint_format(p: &egui::Painter, r: Rect, format: FontFormat, color: Color32, accent: Color32) {
    let c = r.center();
    match format {
        FontFormat::TrueType => {
            p.text(c, Align2::CENTER_CENTER, "TT", FontId::proportional(10.0), color);
        }
        FontFormat::OpenType => {
            p.circle_stroke(c, 5.0, Stroke::new(1.3, color));
        }
        FontFormat::Variable => {
            p.line_segment([c - vec2(6.0, 0.0), c + vec2(6.0, 0.0)], Stroke::new(1.2, color));
            p.circle_filled(c + vec2(2.0, 0.0), 2.6, color);
        }
        FontFormat::Color => {
            for (i, dot) in [accent, color, color, accent].into_iter().enumerate() {
                let (dx, dy) = ((i % 2) as f32 * 5.0 - 2.5, (i / 2) as f32 * 5.0 - 2.5);
                p.circle_filled(c + vec2(dx, dy), 2.1, dot);
            }
        }
    }
}

fn format_name(format: FontFormat) -> &'static str {
    match format {
        FontFormat::TrueType => "TrueType font",
        FontFormat::OpenType => "OpenType font",
        FontFormat::Variable => "Variable font",
        FontFormat::Color => "Color font",
    }
}

/// `text` in `family`'s `style`, rendered once per text, size and colour.
fn preview(
    ui: &Ui,
    fonts: &designcraft_fonts::ScopedFonts<'static>,
    scope: u32,
    family: &str,
    style: &str,
    text: &str,
    size: f32,
    color: Color32,
) -> egui::TextureHandle {
    let ppp = ui.ctx().pixels_per_point();
    let px = (size * ppp).round() as u32;
    let key = egui::Id::unique(("font_preview", family, style, scope, text, px, color.to_array()));
    let cache_id = egui::Id::unique("font_previews");
    if let Some(t) = ui.data_mut(|d| d.get_temp_mut_or_default::<Previews>(cache_id).get(key)) {
        return t;
    }
    let img = designcraft_render::glyphs::text_line(fonts, family, style, text, px, color.to_array());
    let ci = egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels);
    let t = crate::widgets::load_texture(ui.ctx(), format!("font_preview_{family}_{style}"), ci, egui::TextureOptions::LINEAR);
    ui.data_mut(|d| d.get_temp_mut_or_default::<Previews>(cache_id).insert(key, t.clone()));
    t
}

/// Font menu previews kept for reuse, at most [`Previews::CAP`]: each sample text, size and colour
/// renders new ones, so the oldest are let go (their textures freed) as new ones come in.
#[derive(Clone, Default)]
struct Previews {
    map: std::collections::HashMap<egui::Id, egui::TextureHandle>,
    order: std::collections::VecDeque<egui::Id>,
}

impl Previews {
    /// About two screens of rows in each of two sample sizes.
    const CAP: usize = 256;

    fn get(&mut self, key: egui::Id) -> Option<egui::TextureHandle> {
        let t = self.map.get(&key).cloned()?;
        // Most recently used last.
        if let Some(i) = self.order.iter().position(|k| *k == key) {
            self.order.remove(i);
        }
        self.order.push_back(key);
        Some(t)
    }

    fn insert(&mut self, key: egui::Id, t: egui::TextureHandle) {
        if self.map.insert(key, t).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > Self::CAP {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
    }
}

/// A small toggle in the filter row (an icon), with its tooltip.
fn toggle(ui: &mut Ui, on: bool, tip: &str, paint: impl FnOnce(&egui::Painter, Rect, Color32)) -> bool {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(24.0, 22.0), Sense::click());
    if on {
        ui.painter().rect(r, 3.0, t.well, Stroke::new(1.0, t.well_rim), egui::StrokeKind::Inside);
    } else if resp.hovered() {
        ui.painter().rect_filled(r, 3.0, t.hover);
    }
    paint(ui.painter(), r, if on { t.text_strong } else { t.icon });
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, tip));
    resp.on_hover_text(tip).clicked()
}

fn clock(p: &egui::Painter, r: Rect, color: Color32) {
    let c = r.center();
    p.circle_stroke(c, 6.5, Stroke::new(1.2, color));
    p.line_segment([c, c - vec2(0.0, 4.0)], Stroke::new(1.2, color));
    p.line_segment([c, c + vec2(3.0, 1.5)], Stroke::new(1.2, color));
}

/// The Font menu inside any font field's popup or combo box. The search field has the keyboard
/// while the menu is open (empty each time it opens); ↑/↓ move through the rows and Return picks
/// one. A click on a family picks it (its Regular style), on one of its styles that style; the
/// arrow opens a family, the star marks a favourite and ≈ shows similar fonts, all keeping the menu
/// open (it closes on a click outside only).
pub fn font_menu_body(app: &mut DesignApp, ui: &mut Ui, menu: &[FamilyInfo], current: &str, width: f32) -> Option<FontPick> {
    let lang = app.ui.language.clone();
    let tr = |s: &'static str| crate::i18n::tr(&lang, s);
    let (fonts, scope) = (super::fonts(app), super::font_scope(app));
    let english = app.session.prefs.show_font_names_in_english;
    let mut favs = app.session.prefs.favorite_fonts.clone();
    let state_id = egui::Id::unique("font_menu_state");
    let mut st: MenuState = ui.data(|d| d.get_temp(state_id)).unwrap_or_default();
    let frame = ui.ctx().cumulative_frame_nr();
    if st.frame.is_none_or(|f| f.saturating_add(1) < frame) {
        // Just opened: an empty search, nothing highlighted.
        st.query.clear();
        st.highlight = None;
    }
    st.frame = Some(frame);
    let t = Tokens::get(ui.ctx());
    ui.set_width(width.max(560.0));
    let mut pick: Option<FontPick> = None;
    let mut favs_changed = false;
    // The keys the menu uses are its own: a dialog under it never sees Return or Escape.
    let (down, up, enter, escape) = ui.input_mut(|i| {
        let mut key = |k| i.consume_key(egui::Modifiers::NONE, k);
        (key(egui::Key::ArrowDown), key(egui::Key::ArrowUp), key(egui::Key::Enter), key(egui::Key::Escape))
    });

    // Search.
    let search = ui
        .horizontal(|ui| {
            let search = ui.add(
                egui::TextEdit::singleline(&mut st.query)
                    .hint_text(crate::rtl::widget(ui, tr("Search fonts")))
                    .desired_width(ui.available_width() - 30.0)
                    // Return picks the highlighted row instead of leaving the field.
                    .return_key(None),
            );
            let clear = ui.add_enabled(!st.query.is_empty(), egui::Button::new("×").frame(false)).on_hover_text(tr("Clear"));
            if clear.clicked() {
                st.query.clear();
            }
            search
        })
        .inner;
    // Typing always goes to the search, never to the document under the menu.
    if !search.has_focus() {
        search.request_focus();
    }

    // Filters, sample text and size.
    let sample_key = app.ui.font_sample.clone();
    let size_index = usize::from(app.ui.font_preview_size).min(SIZES.len() - 1);
    ui.horizontal(|ui| {
        crate::rtl::label(ui, egui::RichText::new(tr("Filters:")).color(t.text));
        let class = st.class.map_or_else(|| tr("All Classes").to_string(), |c| tr(c.label()).to_string());
        if ui.selectable_label(st.show_classes, format!("{class} ▾")).clicked() {
            st.show_classes = !st.show_classes;
            st.show_samples = false;
        }
        if toggle(ui, st.only_favs, tr("Show Favorites Only"), |p, r, c| paint_star(p, r.center(), 6.5, true, c)) {
            st.only_favs = !st.only_favs;
        }
        if toggle(ui, st.only_recent, tr("Show Recently Added"), clock) {
            st.only_recent = !st.only_recent;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for (i, (_, size)) in SIZES.iter().enumerate().rev() {
                let tip = [tr("Small Sample Text Size"), tr("Medium Sample Text Size"), tr("Large Sample Text Size")][i];
                if toggle(ui, size_index == i, tip, |p, r, c| {
                    p.text(r.center(), Align2::CENTER_CENTER, "A", FontId::proportional(8.0 + *size * 0.35), c);
                }) {
                    app.ui.font_preview_size = i as u8;
                }
            }
            let shown = SAMPLES.iter().find(|(k, _)| *k == sample_key).map_or(SAMPLES[0].1, |(_, l)| l);
            if ui.selectable_label(st.show_samples, format!("{} ▾", tr(shown))).clicked() {
                st.show_samples = !st.show_samples;
                st.show_classes = false;
            }
        });
    });
    if st.show_classes {
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(st.class.is_none(), tr("All Classes")).clicked() {
                st.class = None;
                st.show_classes = false;
            }
            for c in FontClass::ALL {
                if ui.selectable_label(st.class == Some(c), tr(c.label())).clicked() {
                    st.class = Some(c);
                    st.show_classes = false;
                }
            }
        });
    }
    if st.show_samples {
        ui.horizontal_wrapped(|ui| {
            for (key, label) in SAMPLES {
                if ui.selectable_label(sample_key == key, tr(label)).clicked() {
                    app.ui.font_sample = key.to_string();
                    st.show_samples = false;
                }
            }
        });
    }
    if let Some(like) = st.similar.clone() {
        ui.horizontal(|ui| {
            crate::rtl::label(ui, egui::RichText::new(tr("Similar to {family}").replace("{family}", &like)).color(t.text_strong));
            if ui.small_button("×").on_hover_text(tr("Clear")).clicked() {
                st.similar = None;
            }
        });
    }
    let selected = super::text_attrs(app).and_then(|a| a["selectedText"].as_str().map(str::to_string)).filter(|s| !s.trim().is_empty());
    let sample = match app.ui.font_sample.as_str() {
        "selected" => selected.unwrap_or_else(|| tr("Sample").to_string()),
        key => tr(SAMPLES.iter().find(|(k, _)| *k == key).map_or("Sample", |(_, l)| l)).to_string(),
    };
    let (row_h, sample_size) = SIZES[usize::from(app.ui.font_preview_size).min(SIZES.len() - 1)];
    ui.separator();

    // The rows: Recently Used (only for the plain list), then the families that pass the filters.
    let q = st.query.to_lowercase();
    let filtered = st.class.is_some() || st.only_recent || st.similar.is_some();
    let mut families: Vec<&FamilyInfo> = menu.iter().filter(|f| f.matches(&q) && (!st.only_favs || favs.contains(&f.family))).collect();
    // While the kinds are read, the families read so far.
    let mut loading = false;
    if filtered {
        let shared = kinds(ui.ctx(), &fonts, scope);
        let mut k = lock(&shared);
        loading = !k.done;
        // A family the read didn't list (installed or activated since) is read here once, after the
        // read is done, and kept.
        if k.done {
            let missing: Vec<String> = families.iter().filter(|f| !k.of.contains_key(&f.family)).map(|f| f.family.clone()).collect();
            for f in missing {
                let v = family_kind(&fonts, &f);
                k.of.insert(f, v);
            }
        }
        let kind = |f: &str| k.of.get(f).copied();
        families.retain(|f| kind(&f.family).is_some_and(|(kind, added)| st.class.is_none_or(|c| kind.class == c) && (!st.only_recent || added)));
        if let Some(like) = &st.similar {
            let base = meta(ui, &fonts, scope, like).kind;
            let mut near: Vec<(u32, &FamilyInfo)> = families
                .iter()
                .filter_map(|f| {
                    if f.family == *like {
                        return Some((0, *f));
                    }
                    let (k, _) = kind(&f.family)?;
                    (k.class == base.class).then(|| (designcraft_fonts::panose_distance(&base.panose, &k.panose).unwrap_or(u32::MAX / 2) + 1, *f))
                })
                .collect();
            near.sort_by_key(|(d, _)| *d);
            families = near.into_iter().take(SIMILAR).map(|(_, f)| f).collect();
        }
    }
    let mut rows: Vec<Row> = Vec::new();
    if q.is_empty() && !filtered && !st.only_favs {
        let recent: Vec<Row> = app
            .session
            .prefs
            .recent_fonts
            .iter()
            .filter_map(|r| menu.iter().find(|f| f.family == *r))
            .take(RECENT)
            .map(|f| Row::Family { info: f.clone(), recent: true })
            .collect();
        if !recent.is_empty() {
            rows.extend(recent);
            rows.push(Row::Separator);
        }
    }
    let mut group = None;
    for f in families {
        // A separator between the Western fonts and each CJK language's.
        if group.is_some_and(|g| g != f.group) && st.similar.is_none() {
            rows.push(Row::Separator);
        }
        group = Some(f.group);
        rows.push(Row::Family { info: f.clone(), recent: false });
        if st.expanded.contains(&f.family) {
            for s in meta(ui, &fonts, scope, &f.family).styles {
                rows.push(Row::Style { family: f.family.clone(), style: s });
            }
        }
    }

    // Keyboard: ↑/↓ through the pickable rows, Return picks.
    let pickable: Vec<usize> = rows.iter().enumerate().filter(|(_, r)| !matches!(r, Row::Separator)).map(|(i, _)| i).collect();
    if search.changed() {
        st.highlight = if q.is_empty() { None } else { Some(0) };
    }
    let last = pickable.len().checked_sub(1);
    st.highlight = match (st.highlight, last) {
        (_, None) => None,
        (None, Some(_)) if down => Some(0),
        (Some(h), Some(l)) if down => Some((h + 1).min(l)),
        (Some(h), Some(l)) if up => Some(h.saturating_sub(1).min(l)),
        (h, Some(l)) => h.map(|h| h.min(l)),
    };
    let highlighted_row = st.highlight.and_then(|h| pickable.get(h).copied());
    if enter && let Some(row) = highlighted_row.and_then(|i| rows.get(i)) {
        pick = match row {
            Row::Family { info, .. } => Some(FontPick { family: info.family.clone(), style: None }),
            Row::Style { family, style } => Some(FontPick { family: family.clone(), style: Some(style.clone()) }),
            Row::Separator => None,
        };
    }

    let mut toggle_expand = None;
    let mut toggle_fav = None;
    let mut similar = None;
    // A popup only offers last frame's height: ask for the rows' own (a family just opened, a
    // filter just cleared), up to the list's limit.
    let list_h: f32 = rows
        .iter()
        .map(|r| match r {
            Row::Family { .. } => row_h + ui.spacing().item_spacing.y,
            Row::Style { .. } => row_h.min(30.0) + ui.spacing().item_spacing.y,
            Row::Separator => 8.0,
        })
        .sum();
    egui::ScrollArea::vertical().max_height(420.0).min_scrolled_height(list_h.clamp(24.0, 420.0)).auto_shrink([false, true]).show(ui, |ui| {
        if loading {
            crate::rtl::label(ui, egui::RichText::new(tr("Loading fonts…")).color(t.text_dim));
        } else if rows.is_empty() {
            crate::rtl::label(ui, egui::RichText::new(tr("No fonts match")).color(t.text_dim));
        }
        for (i, row) in rows.iter().enumerate() {
            let (family, style, recent) = match row {
                Row::Separator => {
                    ui.separator();
                    continue;
                }
                Row::Family { info, recent } => (info, None, *recent),
                Row::Style { family, style } => {
                    let Some(info) = menu.iter().find(|f| f.family == *family) else { continue };
                    (info, Some(style.as_str()), false)
                }
            };
            let f = &family.family;
            let (r, mut resp) =
                ui.allocate_exact_size(vec2(ui.available_width(), if style.is_some() { row_h.min(30.0) } else { row_h }), Sense::click());
            let highlighted = highlighted_row == Some(i);
            if highlighted && (down || up) {
                ui.scroll_to_rect(r, None);
            }
            if !ui.is_rect_visible(r) {
                continue;
            }
            let label = match style {
                Some(s) => s.to_string(),
                None => family.label(english).to_string(),
            };
            let is_current = style.is_none() && f == current;
            resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, is_current, &label));
            if style.is_none() && label != *f {
                resp = resp.on_hover_text(f);
            }
            // Over the row or one of its buttons (the star, the arrow, ≈ are on top of it).
            let hovered = resp.contains_pointer();
            if is_current {
                ui.painter().rect_filled(r, 0.0, t.row_selected);
            } else if highlighted || hovered {
                ui.painter().rect_filled(r, 0.0, t.hover);
            }
            let p = ui.painter_at(r);
            let m = meta(ui, &fonts, scope, f);
            let cy = r.center().y;
            let mut handled = false;
            if style.is_none() {
                // Favourite star.
                let star = Rect::from_center_size(pos2(r.min.x + 10.0, cy), vec2(16.0, 16.0));
                let fav = favs.contains(f);
                let sr = ui.interact(star, ui.scope_id().with(("fav", f, recent)), Sense::click());
                paint_star(&p, star.center(), 6.5, fav, if fav { t.accent } else { t.text_dim });
                if sr.clicked() {
                    toggle_fav = Some(f.clone());
                    handled = true;
                }
                // The arrow opens a family of several styles.
                if m.styles.len() > 1 && !recent {
                    let arrow = Rect::from_center_size(pos2(r.min.x + 28.0, cy), vec2(16.0, 16.0));
                    let ar = ui.interact(arrow, ui.scope_id().with(("open", f)), Sense::click());
                    ar.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, tr(if st.expanded.contains(f) { "Hide Styles" } else { "Show Styles" }))
                    });
                    let open = st.expanded.contains(f);
                    crate::icons::paint(&p, arrow.shrink(2.0), if open { "chevron-down" } else { "chevron-right" }, t.icon);
                    if ar.clicked() {
                        toggle_expand = Some(f.clone());
                        handled = true;
                    }
                }
            }
            // Name (and how many styles), then the sample in the face, then the format.
            let name_x = r.min.x + if style.is_some() { 48.0 } else { 40.0 };
            let name = if style.is_none() && m.styles.len() > 1 { format!("{label} ({})", m.styles.len()) } else { label.clone() };
            p.text(pos2(name_x, cy), Align2::LEFT_CENTER, &name, FontId::proportional(12.5), t.text);
            let preview_x = r.min.x + (r.width() * 0.42).max(190.0);
            let icons_x = r.max.x - 52.0;
            let face_style = style.map_or_else(|| regular(&m.styles), str::to_string);
            let tex = preview(ui, &fonts, scope, f, &face_style, &sample, sample_size, t.text);
            let size = tex.size_vec2() / ui.ctx().pixels_per_point();
            let shown = Rect::from_min_size(pos2(preview_x, cy - size.y / 2.0), size);
            let clip = Rect::from_min_max(pos2(preview_x, r.min.y), pos2(icons_x - 6.0, r.max.y));
            ui.painter_at(clip).image(tex.id(), shown, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            // Show Similar Fonts (on hover), and the format.
            if style.is_none() && (hovered || st.similar.as_deref() == Some(f.as_str())) {
                let sim = Rect::from_center_size(pos2(icons_x + 12.0, cy), vec2(18.0, 18.0));
                let sr = ui.interact(sim, ui.scope_id().with(("similar", f, recent)), Sense::click());
                sr.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tr("Show Similar Fonts")));
                p.text(sim.center(), Align2::CENTER_CENTER, "≈", FontId::proportional(15.0), if sr.hovered() { t.text_strong } else { t.icon });
                if sr.on_hover_text(tr("Show Similar Fonts")).clicked() {
                    similar = Some(f.clone());
                    handled = true;
                }
            }
            if style.is_none() {
                let badge = Rect::from_center_size(pos2(r.max.x - 16.0, cy), vec2(20.0, 16.0));
                paint_format(&p, badge, m.kind.format, t.text_dim, t.accent);
                ui.interact(badge, ui.scope_id().with(("format", f, recent)), Sense::hover()).on_hover_text(tr(format_name(m.kind.format)));
            }
            if resp.clicked() && !handled {
                pick = Some(FontPick { family: f.clone(), style: style.map(str::to_string) });
            }
        }
    });
    if let Some(f) = toggle_expand
        && !st.expanded.remove(&f)
    {
        st.expanded.insert(f);
    }
    if let Some(f) = toggle_fav {
        if favs.contains(&f) {
            favs.retain(|x| *x != f);
        } else {
            favs.push(f);
        }
        favs_changed = true;
    }
    if let Some(f) = similar {
        st.similar = if st.similar.as_deref() == Some(f.as_str()) { None } else { Some(f) };
        st.query.clear();
    }
    ui.data_mut(|d| d.insert_temp(state_id, st));
    if favs_changed {
        let _ = app.run("prefs.set", json!({ "favoriteFonts": favs }));
    }
    if let Some(p) = &pick {
        // Recently used, newest first.
        let mut recent = app.session.prefs.recent_fonts.clone();
        recent.retain(|f| *f != p.family);
        recent.insert(0, p.family.clone());
        recent.truncate(RECENT);
        let _ = app.run("prefs.set", json!({ "recentFonts": recent }));
        ui.close();
    } else if escape {
        ui.close();
    }
    pick
}

#[cfg(test)]
mod preview_cache_tests {
    use super::Previews;

    #[test]
    fn the_preview_cache_keeps_at_most_its_cap_and_drops_the_oldest() {
        let ctx = egui::Context::default();
        let tex = |i: usize| ctx.load_texture(format!("t{i}"), egui::ColorImage::new([1, 1], vec![egui::Color32::WHITE]), Default::default());
        let mut c = Previews::default();
        let key = |i: usize| egui::Id::unique(("p", i));
        for i in 0..Previews::CAP + 40 {
            c.insert(key(i), tex(i));
            // The first one stays in use, so it is kept.
            assert!(c.get(key(0)).is_some(), "{i}");
        }
        assert_eq!((c.map.len(), c.order.len()), (Previews::CAP, Previews::CAP));
        assert!(c.get(key(1)).is_none(), "the oldest unused one was let go");
        assert!(c.get(key(Previews::CAP + 39)).is_some());
    }
}
