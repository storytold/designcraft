//! File › New › Document, laid out like InDesign's New Document window: preset tabs (Recent,
//! Saved, Print, Web, Mobile) over page cards on one side, the chosen preset's details on the
//! other. Every value lives in the dialog's fields, so the control channel sets them
//! (`ui.dialog.set`) like a user would; `confirm` turns them into `file.new`.

use designcraft_doc::Intent;
use designcraft_doc::build::{NewDocument, PRESETS};
use designcraft_geom::units::parse_measure;
use designcraft_geom::{Unit, format_measure};
use egui::{Color32, Rect, Stroke, pos2, vec2};
use serde_json::{Map, Value, json};

use crate::DesignApp;
use crate::dialogs::Dialog;

/// The Units list, as InDesign's New Document window has it.
pub(crate) const NEW_DOCUMENT_UNITS: [Unit; 8] =
    [Unit::Points, Unit::Picas, Unit::Inches, Unit::Millimeters, Unit::Centimeters, Unit::Ciceros, Unit::Agates, Unit::Pixels];
/// Tab key and English label.
const TABS: [(&str, &str); 5] = [("recent", "Recent"), ("saved", "Saved"), ("print", "Print"), ("web", "Web"), ("mobile", "Mobile")];
/// The card of the factory settings, first on the Recent tab.
const DEFAULT_CARD: &str = "[Default]";
const MAX_RECENT: usize = 12;
const EDGES: [&str; 4] = ["Top", "Bottom", "Inside", "Outside"];
/// Edge groups: field prefix (`marginTop`, `bleedTop`, `slugTop`).
const GROUPS: [&str; 3] = ["margin", "bleed", "slug"];

/// The dialog's fields for `nd`, its measures in its own units.
fn fields_for(nd: &NewDocument) -> Map<String, Value> {
    let u = nd.units;
    let m = |v: f64| json!(format_measure(v, u));
    let mut f = Map::new();
    f.insert("intent".into(), json!(nd.intent));
    f.insert("units".into(), json!(u));
    f.insert("width".into(), m(nd.width));
    f.insert("height".into(), m(nd.height));
    f.insert("pages".into(), json!(nd.pages));
    f.insert("startPage".into(), json!(nd.start_page));
    f.insert("facingPages".into(), json!(nd.facing_pages));
    f.insert("primaryTextFrame".into(), json!(nd.primary_text_frame));
    f.insert("columns".into(), json!(nd.columns));
    f.insert("gutter".into(), m(nd.gutter));
    let margins = [nd.margins.top, nd.margins.bottom, nd.margins.inside, nd.margins.outside];
    for (group, values) in GROUPS.into_iter().zip([margins, nd.bleed, nd.slug]) {
        for (edge, v) in EDGES.into_iter().zip(values) {
            f.insert(format!("{group}{edge}"), m(v));
        }
        f.insert(format!("{group}Linked"), json!(values.iter().all(|v| (v - values[0]).abs() < 1e-6)));
    }
    f
}

/// The fields a new New Document dialog starts with: the factory settings on the Recent tab.
pub(crate) fn defaults() -> Value {
    defaults_for(&NewDocument::default())
}

/// The fields New Document starts with for `nd` (the session's default new document, in the
/// default units), on the Recent tab.
pub(crate) fn defaults_for(nd: &NewDocument) -> Value {
    let mut f = fields_for(nd);
    f.insert("tab".into(), json!("recent"));
    f.insert("preset".into(), json!(DEFAULT_CARD));
    f.insert("name".into(), json!(""));
    f.insert("presetName".into(), json!(""));
    Value::Object(f)
}

fn units(d: &Dialog) -> Unit {
    d.fields.get("units").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or(Unit::Picas)
}

/// A measure field in points: a number is points, text is read in the dialog's units.
fn measure(d: &Dialog, key: &str) -> Option<f64> {
    match d.fields.get(key)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => parse_measure(s, units(d)).ok(),
        _ => None,
    }
    .filter(|v| v.is_finite())
}

fn count(d: &Dialog, key: &str) -> u64 {
    d.n(key).filter(|v| v.is_finite()).unwrap_or(1.0).clamp(1.0, 9999.0).round() as u64
}

/// Every measure field, for a change of units.
fn measure_keys() -> Vec<String> {
    let mut keys: Vec<String> = ["width", "height", "gutter"].into_iter().map(String::from).collect();
    keys.extend(GROUPS.into_iter().flat_map(|g| EDGES.into_iter().map(move |e| format!("{g}{e}"))));
    keys
}

/// `file.new` parameters for the dialog's fields.
pub(crate) fn params(d: &Dialog) -> Value {
    let m = |k: &str, default: f64| measure(d, k).unwrap_or(default).max(0.0);
    let edges = |group: &str| EDGES.map(|e| m(&format!("{group}{e}"), 0.0));
    // A control client may still set one `bleed` for every edge.
    let bleed = measure(d, "bleed").map_or_else(|| json!(edges("bleed")), |b| json!(b.max(0.0)));
    let margins = edges("margin");
    let mut p = json!({
        "intent": d.fields.get("intent").cloned().unwrap_or(json!("print")),
        "units": units(d),
        "width": measure(d, "width"),
        "height": measure(d, "height"),
        "pages": count(d, "pages"),
        "startPage": count(d, "startPage"),
        "facingPages": d.b("facingPages"),
        "columns": count(d, "columns"),
        "gutter": m("gutter", 12.0),
        "margins": {"top": margins[0], "bottom": margins[1], "inside": margins[2], "outside": margins[3]},
        "bleed": bleed,
        "slug": edges("slug"),
        "primaryTextFrame": d.b("primaryTextFrame"),
    });
    let name = d.s("name");
    if !name.trim().is_empty() {
        p["title"] = json!(name.trim());
    }
    p
}

/// The settings `file.new` was given, as a preset (no title).
fn as_preset(p: &Value) -> Option<NewDocument> {
    let mut v = p.clone();
    let o = v.as_object_mut()?;
    o.remove("title");
    o.retain(|_, v| !v.is_null());
    if let Some(b) = o.get("bleed").and_then(Value::as_f64) {
        o.insert("bleed".into(), json!([b, b, b, b]));
    }
    serde_json::from_value(v).ok()
}

/// Remember a document just made with `p` for the Recent tab.
pub(crate) fn remember(app: &mut DesignApp, p: &Value) {
    let Some(nd) = as_preset(p) else { return };
    let recent = &mut app.ui.recent_new_documents;
    recent.retain(|r| *r != nd);
    recent.insert(0, nd);
    recent.truncate(MAX_RECENT);
}

/// The `file.new` params of the document Preview shows (set while it is shown).
const PREVIEWED: &str = "_previewedDoc";
/// The params Preview last tried, made or refused (refused ones are not tried again).
const TRIED: &str = "_previewTried";
/// The params Create was refused with: the reason shows until a field changes.
const REFUSED: &str = "_refusedParams";
/// Settings changed since the preview was made, and when they last changed (egui seconds).
const PENDING: &str = "_previewPending";
/// How long the settings stay unchanged before the preview is made again.
const SETTLE_SECONDS: f64 = 0.2;

/// The text of the first field that doesn't hold a value yet (empty, half typed, negative).
fn invalid_field(d: &Dialog) -> Option<String> {
    let bad_measure = measure_keys().into_iter().find(|k| d.fields.contains_key(k) && !measure(d, k).is_some_and(|v| v >= 0.0));
    let bad_count = || {
        ["pages", "startPage", "columns"]
            .into_iter()
            .map(String::from)
            .find(|k| d.fields.contains_key(k) && !d.n(k).is_some_and(|v| v.is_finite() && v >= 1.0))
    };
    bad_measure.or_else(bad_count).map(|k| d.s(&k))
}

/// `file.new` params for Preview and Create, or `None` while a field doesn't hold a value yet.
fn preview_params(d: &Dialog) -> Option<Value> {
    invalid_field(d).is_none().then(|| params(d))
}

/// Preview on: a document with the dialog's settings shows in the workspace. It is made at once,
/// then again once changed settings have stayed the same for [`SETTLE_SECONDS`]. Settings that
/// aren't values yet, or that `file.new` refuses (a zero width), keep the last one. Off: it goes.
pub(crate) fn preview(app: &mut DesignApp, d: &mut Dialog, ctx: &egui::Context) {
    if d.fields.get(REFUSED).is_some_and(|r| *r != params(d)) {
        d.fields.remove(REFUSED);
        d.fields.remove("status");
    }
    let on = match d.fields.get("preview").and_then(Value::as_bool) {
        Some(on) => on,
        None => {
            d.fields.insert("preview".into(), json!(app.ui.new_document_preview));
            app.ui.new_document_preview
        }
    };
    app.ui.new_document_preview = on;
    if !on {
        end_preview(app);
        for k in [PREVIEWED, TRIED, PENDING] {
            d.fields.remove(k);
        }
        return;
    }
    let Some(p) = preview_params(d) else { return };
    // Refused settings are not tried again until they change, and settings back to the shown
    // ones keep that preview.
    if d.fields.get(PREVIEWED) == Some(&p) && app.session.new_document_preview().is_some() {
        d.fields.insert(TRIED.into(), p.clone());
    }
    if d.fields.get(TRIED) == Some(&p) {
        d.fields.remove(PENDING);
        return;
    }
    let now = ctx.input(|i| i.time);
    if d.fields.contains_key(TRIED) {
        let since = match d.fields.get(PENDING) {
            Some(pending) if pending.get("params") == Some(&p) => pending.get("since").and_then(Value::as_f64).unwrap_or(now),
            _ => {
                d.fields.insert(PENDING.into(), json!({"params": p, "since": now}));
                now
            }
        };
        let wait = SETTLE_SECONDS - (now - since);
        if wait > 0.0 {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
            return;
        }
    }
    let shown = app.session.new_document_preview();
    if app.session.preview_new_document(&p).is_ok() {
        forget_view(app, shown);
        d.fields.insert(PREVIEWED.into(), p.clone());
    }
    d.fields.remove(PENDING);
    d.fields.insert(TRIED.into(), p);
}

/// Create: refused while a field doesn't hold a value yet. With the settings the preview shows,
/// the preview document becomes the new document; otherwise `file.new` makes it and the preview
/// goes. A refusal leaves the window and the preview open, with the reason.
pub(crate) fn create(app: &mut DesignApp, mut d: Dialog) -> Result<Value, String> {
    let refused = |app: &mut DesignApp, mut d: Dialog, e: String| {
        app.status(e.clone());
        d.fields.insert("status".into(), json!(e));
        d.fields.insert(REFUSED.into(), params(&d));
        app.ui.dialog = Some(d);
        Err(e)
    };
    if let Some(text) = invalid_field(&d) {
        let e = format!("{}: {}", crate::i18n::tr(&app.ui.language, "Invalid value"), text.trim());
        return refused(app, d, e);
    }
    let p = params(&d);
    let shows = d.fields.get(PREVIEWED) == Some(&p) && app.session.new_document_preview().is_some();
    let kept = if d.b("preview") && shows { app.session.keep_new_document_preview() } else { None };
    let r = match kept {
        Some(i) => Ok(json!({"index": i})),
        None => app.run("file.new", p.clone()),
    };
    match r {
        Ok(v) => {
            // The new document is active, so the preview's document goes without coming back.
            end_preview(app);
            remember(app, &p);
            Ok(v)
        }
        Err(e) => {
            d.fields.remove(PENDING);
            refused(app, d, e)
        }
    }
}

/// Remove the preview document, if one is shown. Without a preview it does nothing.
pub(crate) fn end_preview(app: &mut DesignApp) {
    let uid = app.session.end_new_document_preview();
    forget_view(app, uid);
}

/// End a preview whose New Document window has gone or no longer previews (closed over the
/// control channel, or replaced by another dialog).
pub(crate) fn end_stale_preview(app: &mut DesignApp) {
    let previewing = app.ui.dialog.as_ref().is_some_and(|d| d.id == "newDocument" && d.b("preview"));
    if !previewing && app.session.new_document_preview().is_some() {
        end_preview(app);
    }
}

/// Drop the views and page histories (one per pane) of a preview document that has gone.
fn forget_view(app: &mut DesignApp, uid: Option<u64>) {
    if let Some(uid) = uid.filter(|uid| app.session.documents().iter().all(|d| d.uid != *uid)) {
        app.views.remove(&uid);
        app.views.remove(&(uid ^ (1 << 63)));
        app.page_history.remove(&uid);
        app.page_history.remove(&(uid ^ (1 << 63)));
    }
}

/// Save the details as the document preset `name` (`file.savePreset`, which replaces one of that
/// name) and show it on the Saved tab.
fn save_preset(app: &mut DesignApp, d: &mut Dialog, name: &str) -> Result<(), String> {
    let name = name.trim();
    let mut p = params(d);
    if let Some(o) = p.as_object_mut() {
        o.remove("title");
        o.retain(|_, v| !v.is_null());
        o.insert("name".into(), json!(name));
    }
    app.run("file.savePreset", p)?;
    d.fields.insert("tab".into(), json!("saved"));
    d.fields.insert("preset".into(), json!(format!("saved:{name}")));
    Ok(())
}

/// The built-in preset `nd` is the size of, if any.
fn preset_name(nd: &NewDocument) -> Option<&'static str> {
    PRESETS.iter().find(|p| p.intent == nd.intent && (p.width - nd.width).abs() < 0.01 && (p.height - nd.height).abs() < 0.01).map(|p| p.name)
}

struct Card {
    key: String,
    label: String,
    nd: NewDocument,
    /// Not a built-in size: drawn with crop marks.
    custom: bool,
}

fn cards(app: &DesignApp, tab: &str) -> Vec<Card> {
    let lang = &app.ui.language;
    let named = |key: String, nd: NewDocument| {
        let (label, custom) = match preset_name(&nd) {
            Some(n) => (crate::i18n::tr(lang, n).to_string(), false),
            None => (crate::i18n::tr(lang, "Custom").to_string(), true),
        };
        Card { key, label, nd, custom }
    };
    // Recent and Saved both start with the factory settings, as in InDesign.
    let default =
        Card { key: DEFAULT_CARD.into(), label: crate::i18n::tr(lang, DEFAULT_CARD).into(), nd: app.session.default_new_document(), custom: false };
    match tab {
        "recent" => std::iter::once(default)
            .chain(app.ui.recent_new_documents.iter().enumerate().map(|(i, nd)| named(format!("recent:{i}"), nd.clone())))
            .collect(),
        "saved" => std::iter::once(default)
            .chain(app.session.prefs.document_presets.iter().map(|nd| Card {
                key: format!("saved:{}", nd.title),
                label: nd.title.clone(),
                nd: nd.clone(),
                custom: preset_name(nd).is_none(),
            }))
            .collect(),
        tab => {
            let intent = match tab {
                "web" => Intent::Web,
                "mobile" => Intent::Mobile,
                _ => Intent::Print,
            };
            PRESETS
                .iter()
                .filter(|p| p.intent == intent)
                .filter_map(|p| {
                    NewDocument::from_preset(p.name).map(|nd| Card {
                        key: p.name.into(),
                        label: crate::i18n::tr(lang, p.name).into(),
                        nd,
                        custom: false,
                    })
                })
                .collect()
        }
    }
}

/// `210 × 297 mm`, `51p0 × 66p0`: the size of `nd` in the dialog's units `u`.
fn size_text(nd: &NewDocument, u: Unit) -> String {
    let (w, h) = (format_measure(nd.width, u), format_measure(nd.height, u));
    let suffix = u.suffix();
    if suffix.starts_with(' ')
        && let (Some(a), Some(b)) = (w.strip_suffix(suffix), h.strip_suffix(suffix))
    {
        return format!("{a} × {b}{suffix}");
    }
    format!("{w} × {h}")
}

/// A page with a folded corner, its proportions those of the preset; crop marks for a custom size.
fn paint_page(p: &egui::Painter, area: Rect, nd: &NewDocument, custom: bool, color: Color32) {
    let aspect = if nd.height > 0.0 { (nd.width / nd.height).clamp(0.2, 5.0) as f32 } else { 1.0 };
    let size =
        if aspect >= area.width() / area.height() { vec2(area.width(), area.width() / aspect) } else { vec2(area.height() * aspect, area.height()) };
    let r = Rect::from_center_size(area.center(), size);
    let fold = (size.min_elem() * 0.24).clamp(5.0, 14.0);
    let stroke = Stroke::new(1.5, color);
    let corner = pos2(r.right() - fold, r.top());
    p.add(egui::Shape::line(vec![r.left_top(), corner, pos2(r.right(), r.top() + fold), r.right_bottom(), r.left_bottom(), r.left_top()], stroke));
    p.add(egui::Shape::line(vec![corner, pos2(corner.x, r.top() + fold), pos2(r.right(), r.top() + fold)], stroke));
    if custom {
        let (gap, len) = (5.0, 10.0);
        p.line_segment([pos2(r.left() - gap - len, r.top() - gap), pos2(r.left() - gap, r.top() - gap)], stroke);
        p.line_segment([pos2(r.left() - gap, r.top() - gap - len), pos2(r.left() - gap, r.top() - gap)], stroke);
    }
}

/// Fill the details from card `c`, keeping the typed document name.
fn select(d: &mut Dialog, c: &Card) {
    let name = d.s("name");
    d.fields.extend(fields_for(&c.nd));
    d.fields.insert("name".into(), json!(name));
    d.fields.insert("preset".into(), json!(c.key));
}

/// Every edge of `group` takes the value of the field `from`.
fn link_edges(d: &mut Dialog, group: &str, from: &str) {
    if let Some(v) = d.fields.get(from).cloned() {
        for e in EDGES {
            d.fields.insert(format!("{group}{e}"), v.clone());
        }
    }
}

/// Show the measures in `u` (same lengths).
fn set_units(d: &mut Dialog, u: Unit) {
    let values: Vec<(String, Option<f64>)> = measure_keys().into_iter().map(|k| (k.clone(), measure(d, &k))).collect();
    d.fields.insert("units".into(), json!(u));
    for (k, pt) in values {
        if let Some(pt) = pt {
            d.fields.insert(k, json!(format_measure(pt, u)));
        }
    }
}

mod form;
pub(crate) use form::FORM;

#[cfg(not(target_arch = "wasm32"))]
#[cfg(test)]
mod successor_tests;
#[cfg(test)]
mod tests;
