//! The framework forms of Step and Repeat, Corner Options and Table of Contents. Each edits the draft the native window edits
//! and leaves OK, Cancel, the live preview and the commands to the existing controller, so documents, undo and what a
//! dialog confirms are what they were.

use super::Dialog;
use super::frame_forms::{check, field, message, resolved, text};
use super::framework::Form;
use super::text_forms::{heading, pick, pick_list, push};
use crate::DesignApp;
use crate::panels::tabs::TabGlyph;
use crate::theme::Tokens;
use craft_ui_widgets::buttons::IconButton;
use craft_ui_widgets::contract::{Bind, PopupKind, Props as _, Show as _, ShowContainer as _, Trigger};
use craft_ui_widgets::display::Picture;
use craft_ui_widgets::layout_components::{Row, Stack};
use craft_ui_widgets::list::ThumbAspect;
use craft_ui_widgets::menu::{Menu, MenuItem};
use craft_ui_widgets::primitives::IconSpec;
use craft_ui_widgets::text::TextField;
use designcraft_geom::corners::CornerShape;
use egui::Ui;
use serde_json::{Value, json};

/// Edit › Step and Repeat, with its live preview.
pub(crate) const STEP_AND_REPEAT: Form = Form::new(step_and_repeat).width(520.0);
/// Object › Corner Options, with its live preview.
pub(crate) const CORNER_OPTIONS: Form = Form::new(corner_options).width(440.0);
/// Layout › Table of Contents.
pub(crate) const TOC: Form = Form::new(toc).width(700.0);

/// The status line under a form, when the controller left one in the draft.
fn status(ui: &mut Ui, d: &Dialog) {
    if let Some(text) = d.fields.get("status").and_then(Value::as_str) {
        message(ui, text);
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Step and Repeat

/// Repeat (Count, or Rows and Columns with Create as a grid), Offset (Vertical and Horizontal, in the ruler units) and Preview.
fn step_and_repeat(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let (hu, vu) = app
        .session
        .active()
        .map_or((designcraft_geom::Unit::Picas, designcraft_geom::Unit::Picas), |s| (s.doc.settings.horizontal_units, s.doc.settings.vertical_units));
    for (k, v) in [
        ("count", json!("1")),
        ("rows", json!("1")),
        ("columns", json!("1")),
        ("vertical", json!(designcraft_geom::format_measure(12.0, vu))),
        ("horizontal", json!(designcraft_geom::format_measure(12.0, hu))),
    ] {
        d.fields.entry(k).or_insert(v);
    }
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    Stack::new().id_salt("step_and_repeat").fill_width().show(ui, |ui, _| {
        Row::new().id_salt("step_and_repeat_columns").fill_width().show(ui, |ui, _| {
            Stack::new().id_salt("step_and_repeat_fields").fill_width().show(ui, |ui, _| {
                heading(ui, &tr("Repeat"));
                let fields: &[(&str, &str)] = if d.b("grid") { &[("rows", "Rows:"), ("columns", "Columns:")] } else { &[("count", "Count:")] };
                for (key, caption) in fields {
                    // A grid's columns, not a text frame's.
                    let label = crate::i18n::tr_context(&lang, caption, "grid").to_string();
                    field(ui, &label, key, |ui| {
                        text(ui, d, key, &label, true);
                    });
                }
                check(ui, d, "grid", &tr("Create as a grid"), true);
                heading(ui, &tr("Offset"));
                for (key, caption) in [("vertical", "Vertical:"), ("horizontal", "Horizontal:")] {
                    let label = tr(caption);
                    field(ui, &label, key, |ui| {
                        text(ui, d, key, &label, true);
                    });
                }
            });
            check(ui, d, "preview", &tr("Preview"), true);
        });
        status(ui, d);
    });
    super::step_and_repeat_preview(app, d);
    None
}

// ---------------------------------------------------------------------------------------------------------------------
// Corner Options

/// One corner: its icon, its size and its shape.
fn corner(ui: &mut Ui, d: &mut Dialog, lang: &str, i: usize) {
    let Some((k, caption)) = super::CORNERS.get(i) else { return };
    let tip = crate::i18n::tr(lang, caption).to_string();
    let t = Tokens::get(ui.ctx());
    let (icon, accent) = (t.icon, t.accent);
    let shapes: Vec<(String, String)> = CornerShape::ALL
        .into_iter()
        .filter_map(|s| Some((serde_json::to_value(s).ok()?.as_str()?.to_owned(), crate::i18n::tr(lang, s.label()).to_string())))
        .collect();
    Row::new().id_salt(("corner_options_corner", i)).show(ui, |ui, _| {
        // The corner of a small square, with a dot at the corner it sets.
        Picture::painter(tip.clone(), ThumbAspect::SQUARE, move |cx, rect| {
            let b = rect.shrink(3.0);
            cx.painter.rect_stroke(b, 0.0, egui::Stroke::new(1.0, icon), egui::StrokeKind::Inside);
            let at = match i {
                0 => b.left_top(),
                1 => b.right_top(),
                2 => b.right_bottom(),
                _ => b.left_bottom(),
            };
            cx.painter.rect_filled(egui::Rect::from_center_size(at, egui::vec2(4.0, 4.0)), 0.0, accent);
        })
        .id_salt(("corner_options_icon", i))
        .show(ui);
        resolved(ui, d, &format!("size{k}"), &tip, true, None);
        super::frame_forms::select(ui, d, &format!("shape{k}"), &format!("{tip} {}", crate::i18n::tr(lang, "Shape")), &shapes, true);
    });
}

/// Under "Corner Size and Shape:", each corner's size and shape (top corners above, bottom ones below, as on the frame), the
/// link button that makes all four the same, and Preview.
fn corner_options(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    super::seed_corner_options(app, d);
    super::link_corners(d);
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    Stack::new().id_salt("corner_options").fill_width().show(ui, |ui, _| {
        heading(ui, &tr("Corner Size and Shape:"));
        Row::new().id_salt("corner_options_corners").show(ui, |ui, _| {
            Stack::new().id_salt("corner_options_left").show(ui, |ui, _| {
                corner(ui, d, &lang, 0);
                corner(ui, d, &lang, 3);
            });
            let linked = d.b("link");
            let tip = tr("Make all settings the same");
            let face = TabGlyph(if linked { "link" } else { "link-broken" });
            let link =
                IconButton::new(IconSpec::glyph(&face)).id_salt("corner_options_link").label(tip.clone()).tooltip(tip).selected(linked).show(ui);
            if link.clicked() {
                d.fields.insert("link".into(), json!(!linked));
            }
            Stack::new().id_salt("corner_options_right").show(ui, |ui, _| {
                corner(ui, d, &lang, 1);
                corner(ui, d, &lang, 2);
            });
        });
        check(ui, d, "preview", &tr("Preview"), true);
        status(ui, d);
    });
    super::link_corners(d);
    super::corner_options_preview(app, d);
    None
}

// ---------------------------------------------------------------------------------------------------------------------
// Table of Contents

/// A style menu whose first choice, stored as "", is `auto` (the generated paragraph style, or [None] for a character style).
fn style_pick(ui: &mut Ui, salt: &str, label: &str, styles: &[String], auto: &str, cur: &mut String) {
    let mut options = vec![(String::new(), auto.to_owned())];
    options.extend(styles.iter().filter(|n| *n != auto).map(|n| (n.clone(), n.clone())));
    if let Some(next) = pick(ui, &format!("toc_style_{salt}"), label, cur, &options, true) {
        *cur = next;
    }
}

/// The paragraph styles to include (Add and Remove between the two lists) and, for the selected one, its entry style, page
/// number position and separator with their character styles, alphabetical sorting and level.
fn toc(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    let styles: Vec<String> = d.fields.get("styles").cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    let char_styles: Vec<String> = d.fields.get("charStyles").cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    let mut expanded = d.fields.get("moreOptions").and_then(Value::as_bool).unwrap_or(true);
    let mut rows = super::toc_rows(d);
    let mut sel = d.n("selected").map_or(0, |n| n.max(0.0) as usize);
    let mut other = d.s("other");
    let mut title_style = d.s("titleStyle");
    let none = designcraft_doc::NO_CHAR_STYLE;
    let choices = [("after", "After Entry"), ("before", "Before Entry"), ("none", "No Page Number")];
    let page_numbers: Vec<(String, String)> = choices.iter().map(|(v, l)| ((*v).to_owned(), tr(l))).collect();
    let levels: Vec<(String, String)> = (1..=9u8).map(|l| (l.to_string(), l.to_string())).collect();
    let specials = [("Tab Character", "^t"), ("Right Indent Tab", "^y"), ("Em Space", "^m"), ("En Space", "^>")];
    Stack::new().id_salt("toc").fill_width().show(ui, |ui, _| {
        Row::new().id_salt("toc_title").wrap(true).show(ui, |ui, _| {
            field(ui, &tr("Title:"), "title", |ui| {
                text(ui, d, "title", &tr("Title:"), true);
            });
            field(ui, &tr("Style:"), "title_style", |ui| style_pick(ui, "title", &tr("Style:"), &styles, "TOC Title", &mut title_style));
        });
        Row::new().id_salt("toc_more").show(ui, |ui, _| {
            heading(ui, &tr("Styles in Table of Contents"));
            if push(ui, "toc_more_options", &tr(if expanded { "Fewer Options" } else { "More Options" }), true) {
                expanded = !expanded;
            }
        });
        Row::new().id_salt("toc_lists").fill_width().show(ui, |ui, _| {
            Stack::new().id_salt("toc_included").fill_width().show(ui, |ui, _| {
                heading(ui, &tr("Include Paragraph Styles:"));
                let captions: Vec<String> =
                    rows.iter().map(|r| format!("{}{}", "  ".repeat(usize::from(r.level.saturating_sub(1))), r.style)).collect();
                if let Some(at) = pick_list(ui, "toc_included", &tr("Include Paragraph Styles:"), &captions, (sel < rows.len()).then_some(sel), 8) {
                    sel = at;
                }
            });
            Stack::new().id_salt("toc_buttons").show(ui, |ui, _| {
                let others: Vec<String> = styles.iter().filter(|n| !rows.iter().any(|r| &r.style == *n)).cloned().collect();
                if push(ui, "toc_add", &tr("« Add"), true) && others.iter().any(|n| *n == other) {
                    let level = u8::try_from(rows.len() + 1).unwrap_or(9).min(9);
                    rows.push(super::TocRow {
                        style: other.clone(),
                        level,
                        page_number: "after".into(),
                        separator: "^t".into(),
                        ..Default::default()
                    });
                    sel = rows.len() - 1;
                    other.clear();
                }
                if push(ui, "toc_remove", &tr("Remove »"), true) && sel < rows.len() {
                    other = rows.remove(sel).style;
                    sel = sel.min(rows.len().saturating_sub(1));
                }
            });
            Stack::new().id_salt("toc_other").fill_width().show(ui, |ui, _| {
                heading(ui, &tr("Other Styles:"));
                let others: Vec<String> = styles.iter().filter(|n| !rows.iter().any(|r| &r.style == *n)).cloned().collect();
                if let Some(at) = pick_list(ui, "toc_other", &tr("Other Styles:"), &others, others.iter().position(|n| *n == other), 8)
                    && let Some(name) = others.get(at)
                {
                    other = name.clone();
                }
            });
        });
        if let Some(r) = rows.get_mut(sel) {
            heading(ui, &format!("{} {}", tr("Style:"), r.style));
            let auto = format!("TOC Level {}", r.level.clamp(1, 9));
            field(ui, &tr("Entry Style:"), "toc_entry_style", |ui| style_pick(ui, "entry", &tr("Entry Style:"), &styles, &auto, &mut r.entry_style));
            if expanded {
                field(ui, &tr("Page Number:"), "toc_page_number", |ui| {
                    if let Some(next) = pick(ui, "toc_page_number", &tr("Page Number:"), &r.page_number, &page_numbers, true) {
                        r.page_number = next;
                    }
                });
                field(ui, &tr("Style:"), "toc_page_number_style", |ui| {
                    style_pick(ui, "page_number", &tr("Style:"), &char_styles, none, &mut r.page_number_style)
                });
                field(ui, &tr("Between Entry and Number:"), "toc_separator", |ui| {
                    Row::new().id_salt("toc_separator_row").show(ui, |ui, _| {
                        TextField::new(tr("Between Entry and Number:"), Bind::bound(&mut r.separator))
                            .id_salt("toc_separator_text")
                            .limit(None)
                            .show(ui);
                        let face = TabGlyph("chevron-right");
                        let label = tr("Insert Special Character");
                        let opener = IconButton::new(IconSpec::glyph(&face))
                            .id_salt("toc_special")
                            .label(label.clone())
                            .tooltip(label.clone())
                            .popup(PopupKind::Menu)
                            .show(ui);
                        let _ = Menu::new(label).id_salt("toc_special_menu").opened_by(&opener.response, Trigger::Click).show(ui, |ui, _| {
                            for (caption, code) in specials {
                                if MenuItem::new(tr(caption)).id_salt(code).show(ui).ran {
                                    super::toc_separator_append(&mut r.separator, code);
                                }
                            }
                        });
                    });
                });
                field(ui, &tr("Style:"), "toc_separator_style", |ui| {
                    style_pick(ui, "separator", &tr("Style:"), &char_styles, none, &mut r.separator_style)
                });
                check_sort(ui, &tr("Sort Entries in Alphabetical Order"), &mut r.sort);
                field(ui, &tr("Level:"), "toc_level", |ui| {
                    if let Some(l) = pick(ui, "toc_level", &tr("Level:"), &r.level.to_string(), &levels, true).and_then(|l| l.parse().ok()) {
                        r.level = l;
                    }
                });
            }
        }
        heading(ui, &tr("Options"));
        check(ui, d, "createBookmarks", &tr("Create PDF Bookmarks"), true);
        let has_toc = d.b("hasToc");
        check(ui, d, "replace", &tr("Replace Existing Table of Contents"), has_toc);
        status(ui, d);
    });
    d.fields.insert("moreOptions".into(), json!(expanded));
    d.fields.insert("included".into(), json!(rows));
    d.fields.insert("selected".into(), json!(sel));
    d.fields.insert("other".into(), json!(other));
    d.fields.insert("titleStyle".into(), json!(title_style));
    None
}

/// The sort check box over `sort`.
fn check_sort(ui: &mut Ui, label: &str, sort: &mut bool) {
    use craft_ui_widgets::choice::Checkbox;
    let out = Checkbox::new(Bind::given(*sort)).label(label.to_owned()).id_salt("toc_sort").show(ui);
    if let Some(on) = out.events.committed().copied() {
        *sort = on;
    }
}
