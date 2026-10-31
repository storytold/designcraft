//! Object › Generate QR Code: a vector QR code (a compound path) as a new object, inside the
//! selected frame, or replacing a selected code (Edit QR Code).

use designcraft_doc::{Content, Fill, Item, ItemId, Selection, Shape};
use designcraft_geom::Rect;
use designcraft_geom::qr::{Ecc, Finder, Modules, QrPart, QrStyle};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, rect_param, spread_param, str_param};
use crate::{Result, Session};

/// Alt text prefix that marks a generated code (and carries its content for editing).
const MARK: &str = "QR code: ";

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "object.qrCode",
        "Generate QR Code…",
        ["Object"],
        None,
        "{type?: url|text|sms|email|vcard (default url), content (url/text), number?, message?, to?, subject?, body?, name?, phone?, email?, org?, url?, color? (swatch, [Black]: the code; a gradient swatch too), finderColor? (swatch for the three corner patterns; none: the code's), background? (swatch behind the code and its quiet zone; none: transparent), rect? (new object; else into the selected frame, or re-encode a selected code), spread?, ecc?: low|medium|quartile|high (error correction; high with a logo), modules?: square|rounded|dots|connected, roundness? (0–1), moduleSize? (0.5–1 of a cell: below 1 the modules stand apart), finder?: square|rounded|circle (the three corner patterns), logoSize? (0–0.34 of the side, left clear in the middle), logo? (an image file placed there, fitted; logoSize 0.25 and ecc high unless given)} — a re-encoded code keeps its style unless given → {id, content, logo?}",
        has_doc,
        qr_code
    )]
}

/// The string a scanner reads, per InDesign's QR types.
fn encode(p: &Value) -> std::result::Result<String, String> {
    let g = |k: &str| str_param(p, k).unwrap_or("").trim().to_string();
    let esc = |s: String| s.replace('\\', r"\\").replace(';', r"\;").replace(':', r"\:").replace(',', r"\,");
    let out = match str_param(p, "type").unwrap_or("url") {
        "url" | "text" => g("content"),
        "sms" => format!("SMSTO:{}:{}", g("number"), g("message")),
        "email" => format!("MATMSG:TO:{};SUB:{};BODY:{};;", esc(g("to")), esc(g("subject")), esc(g("body"))),
        "vcard" => {
            let mut v = format!("MECARD:N:{};", esc(g("name")));
            for (k, tag) in [("org", "ORG"), ("phone", "TEL"), ("email", "EMAIL"), ("url", "URL")] {
                if !g(k).is_empty() {
                    v.push_str(&format!("{tag}:{};", esc(g(k))));
                }
            }
            v + ";"
        }
        t => return Err(format!("unknown type `{t}` (url, text, sms, email, vcard)")),
    };
    if out.is_empty() { Err("nothing to encode".into()) } else { Ok(out) }
}

/// The dialog fields a code's text was made from: [`encode`] the other way round (Edit QR Code
/// shows what the code holds).
pub fn decode(text: &str) -> Value {
    // Fields separated by unescaped `;`, unescaped.
    let split = |body: &str| -> Vec<String> {
        let mut out = vec![String::new()];
        let mut esc = false;
        for ch in body.chars() {
            match (esc, ch) {
                (true, c) => {
                    if let Some(last) = out.last_mut() {
                        last.push(c);
                    }
                    esc = false;
                }
                (false, '\\') => esc = true,
                (false, ';') => out.push(String::new()),
                (false, c) => {
                    if let Some(last) = out.last_mut() {
                        last.push(c);
                    }
                }
            }
        }
        out
    };
    let tagged = |body: &str, tags: &[(&str, &str)]| {
        let mut v = serde_json::Map::new();
        for field in split(body) {
            if let Some((tag, value)) = field.split_once(':')
                && let Some((_, key)) = tags.iter().find(|(t, _)| *t == tag)
            {
                v.insert((*key).into(), json!(value));
            }
        }
        v
    };
    if let Some(body) = text.strip_prefix("MECARD:") {
        let mut v = tagged(body, &[("N", "name"), ("ORG", "org"), ("TEL", "phone"), ("EMAIL", "email"), ("URL", "url")]);
        v.insert("type".into(), json!("vcard"));
        return Value::Object(v);
    }
    if let Some(body) = text.strip_prefix("MATMSG:") {
        let mut v = tagged(body, &[("TO", "to"), ("SUB", "subject"), ("BODY", "body")]);
        v.insert("type".into(), json!("email"));
        return Value::Object(v);
    }
    if let Some(body) = text.strip_prefix("SMSTO:") {
        let (number, message) = body.split_once(':').unwrap_or((body, ""));
        return json!({"type": "sms", "number": number, "message": message});
    }
    let kind = if text.contains("://") || text.starts_with("www.") { "url" } else { "text" };
    json!({"type": kind, "content": text})
}

/// What Edit QR Code starts from for the selected object, if it is, holds or is part of a
/// generated code: the dialog's fields for its content, style and colours.
pub fn edit_fields(d: &designcraft_doc::Document, sel: ItemId) -> Option<Value> {
    let code = find_code(d, sel)?;
    let c = d.item(code)?;
    let mut v = decode(c.alt_text.trim_start_matches(MARK));
    let q = c.qr.unwrap_or_default();
    let name = |x: Value| x.as_str().unwrap_or_default().to_string();
    v["ecc"] = json!(name(serde_json::to_value(q.ecc).unwrap_or_default()));
    v["modules"] = json!(name(serde_json::to_value(q.modules).unwrap_or_default()));
    v["finder"] = json!(name(serde_json::to_value(q.finder).unwrap_or_default()));
    v["roundness"] = json!(q.roundness);
    v["moduleSize"] = json!(q.scale);
    v["logoSize"] = json!(q.logo);
    let colors = Colors::of(d, code);
    v["color"] = json!(colors.code);
    v["finderColor"] = json!(colors.finders.unwrap_or_default());
    v["background"] = json!(colors.background.unwrap_or_default());
    v["editing"] = json!(true);
    Some(v)
}

/// The objects a code is made of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parts {
    /// The frame it sits in: its fill is the background (none for a code made before).
    pub frame: Option<ItemId>,
    /// The code: its modules, and its finder patterns unless they have a colour of their own.
    pub code: ItemId,
    pub finders: Option<ItemId>,
}

/// The parts of the code the selected object is, holds or is part of.
pub fn parts(d: &designcraft_doc::Document, sel: ItemId) -> Option<Parts> {
    let code = find_code(d, sel)?;
    let siblings = parent_of(d, code).and_then(|g| d.item(g)).map(|g| g.children().to_vec()).unwrap_or_default();
    Some(Parts { frame: qr_frame_of(d, code), code, finders: siblings.iter().find(|i| i.qr_part == Some(QrPart::Finders)).map(|i| i.id) })
}

/// The style `p` asks for, starting from `base` (a re-encoded code's own): every key given
/// replaces that part. A logo file without a size or error correction gets room and High.
fn style_param(p: &Value, base: QrStyle) -> std::result::Result<QrStyle, String> {
    let mut st = base;
    let word = |k: &str| str_param(p, k).map(str::to_ascii_lowercase);
    if let Some(e) = word("ecc") {
        st.ecc = match e.as_str() {
            "low" | "l" => Ecc::Low,
            "medium" | "m" => Ecc::Medium,
            "quartile" | "q" => Ecc::Quartile,
            "high" | "h" => Ecc::High,
            _ => return Err(format!("unknown ecc `{e}` (low, medium, quartile, high)")),
        };
    }
    if let Some(m) = word("modules") {
        st.modules = match m.as_str() {
            "square" => Modules::Square,
            "rounded" => Modules::Rounded,
            "dots" => Modules::Dots,
            "connected" => Modules::Connected,
            _ => return Err(format!("unknown modules `{m}` (square, rounded, dots, connected)")),
        };
    }
    if let Some(f) = word("finder") {
        st.finder = match f.as_str() {
            "square" => Finder::Square,
            "rounded" => Finder::Rounded,
            "circle" => Finder::Circle,
            _ => return Err(format!("unknown finder `{f}` (square, rounded, circle)")),
        };
    }
    let num = |k: &str| -> std::result::Result<Option<f64>, String> {
        match p.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v.as_f64().filter(|v| v.is_finite()).map(Some).ok_or_else(|| format!("`{k}` must be a number")),
        }
    };
    if let Some(v) = num("roundness")? {
        st.roundness = v;
    }
    if let Some(v) = num("moduleSize")? {
        st.scale = v;
    }
    match num("logoSize")? {
        Some(v) => st.logo = v,
        None if str_param(p, "logo").is_some() && st.logo <= 0.0 => st.logo = 0.25,
        None => {}
    }
    if str_param(p, "logo").is_some() && p.get("ecc").is_none() {
        st.ecc = Ecc::High;
    }
    Ok(st.sanitized())
}

fn qr_item(id: ItemId, layer: designcraft_doc::LayerId, text: &str, r: Rect, color: &str, style: &QrStyle) -> std::result::Result<Item, String> {
    let path = designcraft_geom::qr::qr_styled_path(text, r, style).ok_or("too much content for a QR code")?;
    let mut it = Item::new(id, layer, Shape::Path, path);
    it.qr = (!style.is_classic()).then_some(*style);
    it.fill = Fill::swatch(color);
    it.alt_text = format!("{MARK}{text}");
    Ok(it)
}

/// The colours of a code's parts: the code, its finder patterns (with the code when `None`) and
/// its background, the fill of the frame it sits in (none when `None`). Each is a swatch: a
/// colour, a tint or a gradient.
#[derive(Clone, Debug, PartialEq)]
struct Colors {
    code: String,
    finders: Option<String>,
    background: Option<String>,
}

impl Colors {
    /// The colours `p` gives, over `base` (an edited code's own). `finderColor`/`background` of
    /// `""` or `none` take that part away.
    fn from(p: &Value, base: Colors) -> Colors {
        let part = |k: &str, cur: Option<String>| match str_param(p, k) {
            Some(v) if v.is_empty() || v.eq_ignore_ascii_case("none") || v == designcraft_color::swatch::NONE => None,
            Some(v) => Some(v.to_string()),
            None => cur,
        };
        Colors {
            code: str_param(p, "color").map_or(base.code, str::to_string),
            finders: part("finderColor", base.finders),
            background: part("background", base.background),
        }
    }

    /// The colours code `code` has now: as its objects are filled, whichever tool filled them.
    fn of(d: &designcraft_doc::Document, code: ItemId) -> Colors {
        let swatch = |it: &Item| (!it.fill.is_none()).then(|| it.fill.swatch.clone());
        let c = d.item(code);
        let frame = qr_frame_of(d, code).and_then(|f| d.item(f));
        let siblings: Vec<std::sync::Arc<Item>> = parent_of(d, code).and_then(|g| d.item(g)).map(|g| g.children().to_vec()).unwrap_or_default();
        let part = |k: QrPart| siblings.iter().find(|i| i.qr_part == Some(k));
        Colors {
            code: c.map_or_else(|| designcraft_color::swatch::BLACK.into(), |c| c.fill.swatch.clone()),
            finders: part(QrPart::Finders).and_then(|i| swatch(i)),
            // The frame's fill; a code made before the frame carried it: its own square's.
            background: frame.and_then(swatch).or_else(|| part(QrPart::Background).and_then(|i| swatch(i))),
        }
    }
}

/// An object of a code drawn anew with `path`: an edited code's own object when there is one, so
/// what the usual tools set on it (a gradient's direction, a stroke, effects, opacity) stays, and
/// its fill changes only when the colour does.
fn redraw(old: Option<&Item>, id: ItemId, layer: designcraft_doc::LayerId, path: designcraft_geom::PathData, swatch: &str) -> Item {
    let mut it = match old {
        Some(o) => {
            let mut it = o.clone();
            it.path = path;
            it
        }
        None => Item::new(id, layer, Shape::Path, path),
    };
    if old.is_none_or(|o| o.fill.swatch != swatch) {
        it.fill = Fill::swatch(swatch);
    }
    it
}

/// The objects a code in `r` is made of, back to front: the code (with its finder patterns unless
/// they have a colour of their own) and the finder patterns, redrawn from an edited code's own
/// (`old_code`, `old_finders`).
#[allow(clippy::too_many_arguments)]
fn qr_parts(
    d: &mut designcraft_doc::Document,
    layer: designcraft_doc::LayerId,
    text: &str,
    r: Rect,
    style: &QrStyle,
    colors: &Colors,
    old_code: Option<&Item>,
    old_finders: Option<&Item>,
) -> std::result::Result<Vec<Item>, String> {
    let shapes = designcraft_geom::qr::qr_shapes(text, r, style).ok_or("too much content for a QR code")?;
    let fresh = qr_item(ItemId(0), layer, text, r, &colors.code, style)?;
    let id = old_code.map_or_else(|| ItemId(d.alloc()), |c| c.id);
    let path = if colors.finders.is_some() { shapes.modules } else { fresh.path };
    let mut code = redraw(old_code, id, layer, path, &colors.code);
    code.qr = fresh.qr;
    code.alt_text = fresh.alt_text;
    let mut out = vec![code];
    if let Some(fc) = &colors.finders {
        let id = old_finders.map_or_else(|| ItemId(d.alloc()), |f| f.id);
        let mut it = redraw(old_finders, id, layer, shapes.finders, fc);
        it.qr_part = Some(QrPart::Finders);
        out.push(it);
    }
    Ok(out)
}

/// The group or frame an object sits in, if any (not the spread).
fn parent_of(d: &designcraft_doc::Document, id: ItemId) -> Option<ItemId> {
    let loc = d.find(id)?;
    let (_, up) = loc.path.split_last()?;
    let mut items = &d.spread(loc.spread)?.items[..];
    let mut parent = None;
    for &i in up {
        let it = items.get(i)?;
        parent = Some(it.id);
        items = it.children();
    }
    parent
}

/// How far a search for a code goes into groups and frames.
const MAX_DEPTH: usize = 8;

/// A generated code that is `it` or lies inside it, the frontmost one.
fn code_under(it: &Item, depth: usize) -> Option<ItemId> {
    if it.alt_text.starts_with(MARK) {
        return Some(it.id);
    }
    if depth == 0 {
        return None;
    }
    it.children().iter().rev().find_map(|c| code_under(c, depth - 1))
}

/// The code the selected object is, holds (a code's frame, a group with one in it), or is a part
/// of (its finder patterns or logo).
fn find_code(d: &designcraft_doc::Document, sel: ItemId) -> Option<ItemId> {
    let it = d.item(sel)?;
    if let Some(c) = code_under(it, MAX_DEPTH) {
        return Some(c);
    }
    if !matches!(it.qr_part, Some(QrPart::Finders | QrPart::Logo)) {
        return None;
    }
    code_under(d.item(parent_of(d, sel)?)?, 1)
}

/// The frame a code sits in, if it sits in one (not in a group): its fill is the background.
fn qr_frame_of(d: &designcraft_doc::Document, code: ItemId) -> Option<ItemId> {
    let p = parent_of(d, code)?;
    (d.item(p)?.shape != Shape::Group).then_some(p)
}

/// Puts `new` where the object `id` is (in its group or frame, or on its spread).
fn replace_item(d: &mut designcraft_doc::Document, id: ItemId, new: Item) -> Result<()> {
    let loc = d.find(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
    let Some((&last, up)) = loc.path.split_last() else { return Err(designcraft_doc::DocError::NoItem(id).into()) };
    let slot = if up.is_empty() {
        d.spread_mut(loc.spread).and_then(|sp| sp.items.get_mut(last))
    } else {
        let parent = designcraft_doc::ItemLoc { spread: loc.spread, path: up.to_vec() };
        d.item_mut_at(&parent).and_then(Item::children_mut).and_then(|items| items.get_mut(last))
    };
    *slot.ok_or(designcraft_doc::DocError::NoItem(id))? = std::sync::Arc::new(new);
    Ok(())
}

/// A new code's frame on `r`'s square: the background as its fill, no stroke.
fn qr_frame(d: &mut designcraft_doc::Document, layer: designcraft_doc::LayerId, r: Rect, colors: &Colors) -> Item {
    let mut f = Item::new(ItemId(d.alloc()), layer, Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
    f.fill = colors.background.as_deref().map_or_else(Fill::none, Fill::swatch);
    f.qr_part = Some(QrPart::Background);
    f
}

/// What [`qr_code`]'s edit leaves to do after it: the frame, where a logo goes (frame space), and
/// a logo frame moved to a new area, to fit its image again.
struct Made {
    select: ItemId,
    frame: ItemId,
    logo_area: Option<Rect>,
    refit: Option<ItemId>,
}

fn qr_code(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "object.qrCode";
    let text = encode(p).map_err(|e| bad(ID, e))?;
    let rect = rect_param(p, "rect");
    let sr = spread_param(p, ID)?;
    let logo_file = str_param(p, "logo").map(str::to_string);
    let st = s.doc()?;
    let lid = st.active_layer;
    let sel = st.selection.items.clone();
    let editing = if rect.is_some() { None } else { sel.first().and_then(|t| find_code(&st.doc, *t)) };
    // An edited code keeps its style and colours unless the params change them.
    let (base_style, base_colors) = match editing.and_then(|code| Some((st.doc.item(code)?.qr.unwrap_or_default(), Colors::of(&st.doc, code)))) {
        Some(b) => b,
        None => (QrStyle::default(), Colors { code: designcraft_color::swatch::BLACK.into(), finders: None, background: None }),
    };
    let style = style_param(p, base_style).map_err(|e| bad(ID, e))?;
    let colors = Colors::from(p, base_colors);
    let made = s.edit(|d, selection| {
        for c in [Some(&colors.code), colors.finders.as_ref(), colors.background.as_ref()].into_iter().flatten() {
            if d.swatch(c).is_none() {
                return Err(bad(ID, format!("no swatch `{c}`")));
            }
        }
        let logo_in = |r: Rect| designcraft_geom::qr::qr_shapes(&text, r, &style).and_then(|q| q.logo);
        let made = match (rect, sel.first().copied(), editing) {
            (Some(r), _, _) => {
                // A new code: a frame on the code's square (with its quiet zone), the parts in it.
                let square = designcraft_geom::qr::qr_shapes(&text, r, &style).ok_or_else(|| bad(ID, "too much content for a QR code"))?.frame;
                let parts = qr_parts(d, lid, &text, square, &style, &colors, None, None).map_err(|e| bad(ID, e))?;
                let mut f = qr_frame(d, lid, square, &colors);
                f.content = Content::Group { items: parts.into_iter().map(std::sync::Arc::new).collect() };
                let fid = d.insert_item(sr, f, None)?;
                Made { select: fid, frame: fid, logo_area: logo_in(square), refit: None }
            }
            (None, _, Some(code_id)) => {
                // Edit QR Code: the parts drawn anew in the code's frame; what else is in it stays.
                let c = d.item(code_id).ok_or(designcraft_doc::DocError::NoItem(code_id))?.clone();
                let fid = match qr_frame_of(d, code_id) {
                    Some(f) => f,
                    None => {
                        // A code made without a frame: a frame on its square takes its place.
                        // The symbol: the code with its finder patterns, wherever they are.
                        let beside = parent_of(d, code_id)
                            .and_then(|g| d.item(g))
                            .and_then(|g| g.children().iter().find(|i| i.qr_part == Some(QrPart::Finders)).cloned());
                        let finders_beside = beside.and_then(|f| f.path.bounds().map(|b| (c.xf.inverse() * f.xf).transform_rect_bbox(b)));
                        let symbol = c.path.bounds().into_iter().chain(finders_beside).reduce(|a, b| a.union(b)).unwrap_or(Rect::ZERO);
                        let n = designcraft_geom::qr::qr_size_at(c.alt_text.trim_start_matches(MARK), c.qr.unwrap_or_default().ecc).unwrap_or(21);
                        let quiet = symbol.width() / n as f64 * 2.0;
                        let side = symbol.width().max(symbol.height()) + 2.0 * quiet;
                        let r = Rect::from_center_size(symbol.center(), (side, side));
                        let mut f = qr_frame(d, c.layer, r, &colors);
                        f.xf = c.xf;
                        let mut inner = c.clone();
                        inner.xf = designcraft_geom::Affine::IDENTITY;
                        let mut items = vec![std::sync::Arc::new(inner)];
                        // Its own background square and finder patterns, beside it in a group,
                        // move in with it (the frame's fill takes over the background).
                        let legacy: Vec<std::sync::Arc<Item>> = parent_of(d, code_id)
                            .and_then(|g| d.item(g))
                            .map(|g| {
                                g.children().iter().filter(|i| matches!(i.qr_part, Some(QrPart::Background | QrPart::Finders))).cloned().collect()
                            })
                            .unwrap_or_default();
                        for l in legacy {
                            d.remove_item(l.id)?;
                            if l.qr_part == Some(QrPart::Finders) {
                                let mut l = (*l).clone();
                                l.xf = c.xf.inverse() * l.xf;
                                items.push(std::sync::Arc::new(l));
                            }
                        }
                        f.content = Content::Group { items };
                        let fid = f.id;
                        replace_item(d, code_id, f)?;
                        fid
                    }
                };
                let frame = d.item(fid).ok_or(designcraft_doc::DocError::NoItem(fid))?.clone();
                let r = frame.inner_bounds();
                let kids = frame.children();
                // The code as it sits in the frame now: a code just moved into one has handed its
                // transform to the frame, and must not keep it as well.
                let c = kids.iter().find(|i| i.id == code_id).map_or(c, |i| (**i).clone());
                let finders = kids.iter().find(|i| i.qr_part == Some(QrPart::Finders)).map(|i| (**i).clone());
                let logo = kids.iter().find(|i| i.qr_part == Some(QrPart::Logo)).map(|i| (**i).clone());
                let parts = qr_parts(d, c.layer, &text, r, &style, &colors, Some(&c), finders.as_ref()).map_err(|e| bad(ID, e))?;
                let area = logo_in(r);
                // The logo: kept and moved to the new clear area, or gone with it or a new file.
                let logo = logo.filter(|_| area.is_some() && logo_file.is_none());
                let mut refit = None;
                let logo = logo.map(|mut l| {
                    if let Some(a) = area
                        && l.xf.transform_rect_bbox(l.inner_bounds()) != a
                    {
                        l.path = designcraft_geom::shapes::rectangle(a);
                        l.xf = designcraft_geom::Affine::IDENTITY;
                        refit = Some(l.id);
                    }
                    l
                });
                let f = d.item_mut(fid).ok_or(designcraft_doc::DocError::NoItem(fid))?;
                let own = (!f.fill.is_none()).then(|| f.fill.swatch.clone());
                if own != colors.background || f.qr_part.is_none() {
                    f.fill = colors.background.as_deref().map_or_else(Fill::none, Fill::swatch);
                }
                f.qr_part = Some(QrPart::Background);
                let items = f.children_mut().ok_or(designcraft_doc::DocError::NoItem(fid))?;
                let at = items.iter().position(|i| i.id == code_id).unwrap_or(0);
                items.retain(|i| i.id != code_id && i.qr_part.is_none());
                let at = at.min(items.len());
                for (k, it) in parts.into_iter().chain(logo).enumerate() {
                    items.insert((at + k).min(items.len()), std::sync::Arc::new(it));
                }
                // The selection stays (the frame, a group around it, a part); else the frame.
                let keep = sel.first().copied().filter(|s| d.item(*s).is_some());
                Made { select: keep.unwrap_or(fid), frame: fid, logo_area: if logo_file.is_some() { area } else { None }, refit }
            }
            (None, Some(t), None) => {
                // Into the frame: it becomes the code's frame, the parts its content.
                let it = d.item(t).ok_or_else(|| bad(ID, "no such object"))?.clone();
                if !matches!(it.content, Content::Unassigned | Content::Group { .. }) || it.shape == Shape::Group {
                    return Err(bad(ID, "select an empty frame, a QR code, or give `rect`"));
                }
                let r = it.inner_bounds();
                let parts = qr_parts(d, it.layer, &text, r, &style, &colors, None, None).map_err(|e| bad(ID, e))?;
                let f = d.item_mut(t).ok_or(designcraft_doc::DocError::NoItem(t))?;
                f.content = Content::Group { items: parts.into_iter().map(std::sync::Arc::new).collect() };
                f.fill = colors.background.as_deref().map_or_else(Fill::none, Fill::swatch);
                f.qr_part = Some(QrPart::Background);
                Made { select: t, frame: t, logo_area: logo_in(r), refit: None }
            }
            (None, None, _) => return Err(bad(ID, "give `rect` or select a frame")),
        };
        *selection = Selection::items(vec![made.select]);
        Ok(made)
    })?;
    let mut out = json!({"id": made.select.0, "frame": made.frame.0, "content": text});
    if let Some(l) = made.refit {
        s.execute("object.fit", &json!({"mode": "fitProportionally", "ids": [l.0]}))?;
    }
    // A logo file: a frame on the clear area with the image fitted in it, moved into the code's
    // frame (all part of this one undo step, as everything this command runs).
    if let (Some(file), Some(area)) = (logo_file, made.logo_area) {
        let d = &s.doc()?.doc;
        let fid = made.frame;
        let loc = d.find(fid).ok_or(designcraft_doc::DocError::NoItem(fid))?;
        let to_spread = d.parent_xf(&loc) * d.item(fid).ok_or(designcraft_doc::DocError::NoItem(fid))?.xf;
        let a = to_spread.transform_rect_bbox(area);
        let spread = match loc.spread {
            designcraft_doc::SpreadRef::Doc(i) => json!({"kind": "doc", "index": i}),
            designcraft_doc::SpreadRef::Parent(i) => json!({"kind": "parent", "index": i}),
        };
        let logo = s.execute("frame.create", &json!({"rect": [a.x0, a.y0, a.x1, a.y1], "content": "unassigned", "spread": spread}))?["id"].clone();
        s.execute("file.place", &json!({"path": file, "frame": logo}))?;
        s.execute("object.fit", &json!({"mode": "fitProportionally", "ids": [logo]}))?;
        let lid = ItemId(logo.as_u64().ok_or_else(|| bad(ID, "no logo frame"))?);
        let select = made.select;
        s.edit(|d, selection| {
            let mut l = (*d.remove_item(lid)?).clone();
            l.xf = to_spread.inverse() * l.xf;
            l.stroke = designcraft_doc::Stroke::none();
            l.qr_part = Some(QrPart::Logo);
            let items = d.item_mut(fid).and_then(Item::children_mut).ok_or(designcraft_doc::DocError::NoItem(fid))?;
            items.push(std::sync::Arc::new(l));
            *selection = Selection::items(vec![select]);
            Ok(())
        })?;
        out["logo"] = logo;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use designcraft_doc::{Content, Item, ItemId};
    use designcraft_geom::qr::{Ecc, Finder, Modules, QrPart, QrStyle};
    use serde_json::json;

    use super::MARK;

    use crate::Session;

    fn new_doc() -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s
    }

    fn item(s: &Session, id: ItemId) -> Item {
        s.doc().unwrap().doc.item(id).unwrap().clone()
    }

    /// The code an object is, holds or is part of.
    fn code(s: &Session, id: ItemId) -> Item {
        let d = &s.doc().unwrap().doc;
        d.item(super::find_code(d, id).unwrap()).unwrap().clone()
    }

    fn id(v: &serde_json::Value) -> ItemId {
        ItemId(v["id"].as_u64().unwrap())
    }

    /// A code as Generate QR Code made it before codes had a frame: one path on the spread.
    fn lone_code(s: &mut Session, text: &str, r: designcraft_geom::Rect, color: &str) -> ItemId {
        s.edit(|d, _| {
            let lid = d.layers[0].id;
            let it = super::qr_item(ItemId(d.alloc()), lid, text, r, color, &QrStyle::default()).unwrap();
            Ok(d.insert_item(designcraft_doc::SpreadRef::Doc(0), it, None)?)
        })
        .unwrap()
    }

    #[test]
    fn generate_and_edit_qr_codes() {
        let mut s = new_doc();
        let r = s.execute("object.qrCode", &json!({"content": "https://example.com", "rect": [72, 72, 216, 216]})).unwrap();
        let frame = id(&r);
        let f = item(&s, frame);
        assert_eq!(f.shape, designcraft_doc::Shape::Rectangle, "a code is a frame");
        assert_eq!(f.qr_part, Some(QrPart::Background));
        let a = code(&s, frame);
        assert!(a.path.subpaths.len() > 20 && a.alt_text.contains("example.com"));
        let b0 = a.bounds();
        assert!(b0.x0 >= 72.0 && b0.x1 <= 216.0);
        // Edit QR Code: new content, same place.
        let r = s.execute("object.qrCode", &json!({"type": "email", "to": "a@b.c", "subject": "Hi"})).unwrap();
        assert_eq!(r["content"], "MATMSG:TO:a@b.c;SUB:Hi;BODY:;;");
        let b1 = code(&s, frame).bounds();
        assert!((b1.center() - b0.center()).hypot() < 1.0, "{b0:?} → {b1:?}");
        assert!((b1.width() - b0.width()).abs() < 4.0);
        assert_eq!(s.doc().unwrap().doc.spreads[0].items.len(), 1, "still one object");
        // Into an empty frame.
        let f = s.execute("frame.create", &json!({"rect": [300, 300, 400, 400], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.qrCode", &json!({"type": "vcard", "name": "Ada", "phone": "+44 1"})).unwrap();
        let fr = item(&s, ItemId(f));
        assert!(fr.has_nested_items() && fr.qr_part == Some(QrPart::Background));
        assert!(s.execute("object.qrCode", &json!({"type": "fax", "content": "x", "rect": [0, 0, 9, 9]})).is_err());
    }

    /// A styled code keeps its style (stored on the object) when its content is edited, and a
    /// classic code stores none.
    #[test]
    fn a_styled_code_keeps_its_style_when_edited() {
        let mut s = new_doc();
        let classic = id(&s.execute("object.qrCode", &json!({"content": "a", "rect": [10, 10, 110, 110]})).unwrap());
        assert!(code(&s, classic).qr.is_none());
        let r = s
            .execute(
                "object.qrCode",
                &json!({"content": "https://example.com", "rect": [200, 200, 300, 300], "modules": "dots", "finder": "circle", "ecc": "q", "moduleSize": 0.8, "roundness": 0.5}),
            )
            .unwrap();
        let fid = id(&r);
        let style = |s: &Session| code(s, fid).qr.unwrap();
        let st = style(&s);
        assert_eq!((st.modules, st.finder, st.ecc, st.scale, st.roundness), (Modules::Dots, Finder::Circle, Ecc::Quartile, 0.8, 0.5));
        // Edit QR Code with new content only: the look stays; a given key still changes it.
        s.execute("selection.set", &json!({"ids": [fid.0]})).unwrap();
        s.execute("object.qrCode", &json!({"content": "https://example.org"})).unwrap();
        assert_eq!(style(&s), st);
        s.execute("object.qrCode", &json!({"content": "https://example.org", "modules": "connected"})).unwrap();
        assert_eq!(style(&s).modules, Modules::Connected);
        for bad in [
            json!({"content": "x", "rect": [0, 0, 9, 9], "modules": "hearts"}),
            json!({"content": "x", "rect": [0, 0, 9, 9], "ecc": "max"}),
            json!({"content": "x", "rect": [0, 0, 9, 9], "roundness": "round"}),
        ] {
            assert!(s.execute("object.qrCode", &bad).is_err(), "{bad}");
        }
    }

    /// A logo file goes on the clear area in the middle, fitted, inside the code's frame and
    /// without a stroke, at High error correction; the whole is one undo step, and editing the
    /// code keeps the logo, moved to a new clear area.
    #[test]
    fn a_logo_is_placed_in_the_middle_in_one_undo_step() {
        let dir = std::env::temp_dir().join(format!("dc-qr-logo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("logo.png");
        std::fs::write(&png, designcraft_render::Rendered { width: 40, height: 40, pixels: vec![200u8; 40 * 40 * 4] }.to_png()).unwrap();
        let mut s = new_doc();
        let undo = |s: &Session| s.doc().unwrap().history.undo.len();
        let before = undo(&s);
        let r = s
            .execute("object.qrCode", &json!({"content": "https://example.com", "rect": [100, 100, 300, 300], "logo": png.to_string_lossy()}))
            .unwrap();
        assert_eq!(undo(&s), before + 1, "one undo step");
        let fid = id(&r);
        let logo_of = |s: &Session| item(s, fid).children().iter().find(|c| c.qr_part == Some(QrPart::Logo)).map(|c| (**c).clone());
        let logo = logo_of(&s).expect("the logo is in the code's frame");
        assert!(matches!(logo.content, Content::Graphic(_)), "the image is in the logo frame");
        assert!(logo.stroke.is_none(), "no stroke around the logo");
        assert_eq!(s.doc().unwrap().doc.spreads[0].items.len(), 1, "one object");
        let c = code(&s, fid);
        assert_eq!(c.qr.unwrap().ecc, Ecc::High, "High unless given");
        assert!((c.qr.unwrap().logo - 0.25).abs() < 1e-9);
        let b = logo.bounds();
        assert!((b.center() - designcraft_geom::Point::new(200.0, 200.0)).hypot() < 1.0, "centred: {b:?}");
        assert!(b.width() < 200.0 * 0.3, "within the clear area: {b:?}");
        // Edited with a bigger clear area: the same logo, grown with it; with none, it goes.
        s.execute("selection.set", &json!({"ids": [fid.0]})).unwrap();
        s.execute("object.qrCode", &json!({"content": "https://example.org", "logoSize": 0.3})).unwrap();
        let grown = logo_of(&s).unwrap();
        assert_eq!(grown.id, logo.id);
        assert!(grown.bounds().width() > b.width() + 1.0, "{:?} → {:?}", b, grown.bounds());
        s.execute("object.qrCode", &json!({"content": "https://example.org", "logoSize": 0})).unwrap();
        assert!(logo_of(&s).is_none());
        for _ in 0..3 {
            s.execute("edit.undo", &json!({})).unwrap();
        }
        assert!(s.doc().unwrap().doc.spreads[0].items.is_empty(), "undo takes code and logo");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The frame's fill is the background and its stroke the code's outline, as on any frame; the
    /// code and its finder patterns are ordinary objects inside it. What the usual tools set on
    /// them (a fill, a gradient's direction, a stroke) survives editing, and the dialog shows it.
    #[test]
    fn the_usual_tools_colour_a_code_and_editing_keeps_it() {
        let mut s = new_doc();
        // A gradient swatch to colour the code with.
        let g = s.execute("frame.create", &json!({"rect": [500, 500, 520, 520], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.gradient", &json!({"ids": [g], "stops": [{"location": 0, "color": {"c": 100, "m": 0, "y": 0, "k": 0}}, {"location": 100, "color": {"c": 0, "m": 100, "y": 0, "k": 0}}]})).unwrap();
        s.execute("selection.set", &json!({"ids": [g]})).unwrap();
        let grad = s.execute("swatch.new", &json!({})).unwrap()["name"].as_str().unwrap().to_string();
        let r = s
            .execute(
                "object.qrCode",
                &json!({"content": "https://example.com", "rect": [100, 100, 200, 200], "color": grad, "finderColor": "C=100 M=0 Y=0 K=0", "background": "[Paper]"}),
            )
            .unwrap();
        let fid = id(&r);
        let f = item(&s, fid);
        assert_eq!(f.fill.swatch, "[Paper]", "the background is the frame's fill");
        let kids: Vec<(Option<QrPart>, bool, String)> =
            f.children().iter().map(|c| (c.qr_part, c.alt_text.starts_with(MARK), c.fill.swatch.clone())).collect();
        assert_eq!(kids, [(None, true, grad.clone()), (Some(QrPart::Finders), false, "C=100 M=0 Y=0 K=0".to_string())]);
        // The usual tools: the frame's fill and stroke; the code (selected inside) a gradient's
        // direction and a stroke.
        let cid = code(&s, fid).id;
        s.execute("object.fill", &json!({"ids": [fid.0], "swatch": "[Black]", "tint": 0.2})).unwrap();
        s.execute("object.stroke", &json!({"ids": [fid.0], "swatch": "[Black]", "weight": 2})).unwrap();
        s.execute("object.stroke", &json!({"ids": [cid.0], "swatch": "[Black]", "weight": 0.5})).unwrap();
        s.edit(|d, _| {
            d.item_mut(cid).unwrap().fill.gradient_vector = Some([100.0, 100.0, 200.0, 200.0]);
            Ok(())
        })
        .unwrap();
        let fields = super::edit_fields(&s.doc().unwrap().doc, fid).unwrap();
        assert_eq!((fields["background"].as_str(), fields["color"].as_str()), (Some("[Black]"), Some(grad.as_str())));
        // Edit from the code itself (selected inside the frame) with new content only.
        s.execute("selection.set", &json!({"ids": [cid.0]})).unwrap();
        s.execute("object.qrCode", &json!({"content": "https://example.org/a/longer/address"})).unwrap();
        let (f, c) = (item(&s, fid), code(&s, fid));
        assert_eq!((f.fill.swatch.as_str(), f.fill.tint, f.stroke.weight), ("[Black]", 0.2, 2.0), "the frame keeps fill, tint and stroke");
        assert_eq!((c.fill.swatch.clone(), c.fill.gradient_vector, c.stroke.weight), (grad.clone(), Some([100.0, 100.0, 200.0, 200.0]), 0.5));
        assert!(c.alt_text.ends_with("longer/address"));
        assert_eq!(s.doc().unwrap().selection.items, vec![cid], "the code stays selected");
        // A colour picked in the dialog replaces the fill; the finder patterns back to the code's.
        s.execute("selection.set", &json!({"ids": [fid.0]})).unwrap();
        s.execute("object.qrCode", &json!({"content": "x", "color": "[Black]", "finderColor": "none", "background": "none"})).unwrap();
        let f = item(&s, fid);
        assert!(f.fill.is_none() && f.children().len() == 1 && f.children()[0].fill.swatch == "[Black]");
        assert!(s.execute("object.qrCode", &json!({"content": "x", "rect": [0, 0, 9, 9], "background": "Nope"})).is_err());
    }

    /// Codes made before codes had a frame open as they were and edit like new ones: a lone code
    /// (one path), a group of a background square, the code and finder patterns, and a frame
    /// holding a background square and the code. Each becomes a frame where it was, with its
    /// colours, and nothing doubles.
    #[test]
    fn codes_made_before_stay_editable() {
        // A lone code.
        let mut s = new_doc();
        let r = designcraft_geom::Rect::new(100.0, 100.0, 200.0, 200.0);
        let lone = lone_code(&mut s, "https://old.example", r, "[Black]");
        let before = item(&s, lone).bounds();
        let fields = super::edit_fields(&s.doc().unwrap().doc, lone).unwrap();
        assert_eq!(
            (fields["content"].as_str(), fields["color"].as_str(), fields["background"].as_str()),
            (Some("https://old.example"), Some("[Black]"), Some(""))
        );
        s.execute("selection.set", &json!({"ids": [lone.0]})).unwrap();
        let out = s.execute("object.qrCode", &json!({"content": "https://old.example"})).unwrap();
        let items = &s.doc().unwrap().doc.spreads[0].items;
        assert_eq!(items.len(), 1);
        let f = &items[0];
        assert_eq!((f.qr_part, f.fill.is_none()), (Some(QrPart::Background), true));
        let after = code(&s, f.id).bounds();
        assert!((after.x0 - before.x0).abs() < 0.01 && (after.x1 - before.x1).abs() < 0.01, "{before:?} → {after:?}");
        assert_eq!(code(&s, f.id).id, lone, "the code keeps its id");
        assert!(out["id"].as_u64().is_some());

        // A group of a background square, the code and finder patterns (as the first preview made).
        let mut s = new_doc();
        let r = s
            .execute(
                "object.qrCode",
                &json!({"content": "https://example.com", "rect": [100, 100, 200, 200], "finderColor": "[Registration]", "background": "[Paper]"}),
            )
            .unwrap();
        let fid = id(&r);
        let f = item(&s, fid);
        let (code_it, finders) = (f.children()[0].clone(), f.children()[1].clone());
        let gid = s
            .edit(|d, _| {
                d.remove_item(fid)?;
                let lid = d.layers[0].id;
                let mut bg =
                    Item::new(ItemId(d.alloc()), lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(f.inner_bounds()));
                bg.fill = designcraft_doc::Fill::swatch("[Paper]");
                bg.qr_part = Some(QrPart::Background);
                let mut g = Item::new(ItemId(d.alloc()), lid, designcraft_doc::Shape::Group, designcraft_geom::PathData::default());
                g.content = Content::Group { items: vec![std::sync::Arc::new(bg), code_it.clone(), finders.clone()] };
                Ok(d.insert_item(designcraft_doc::SpreadRef::Doc(0), g, None)?)
            })
            .unwrap();
        let fields = super::edit_fields(&s.doc().unwrap().doc, gid).unwrap();
        assert_eq!((fields["finderColor"].as_str(), fields["background"].as_str()), (Some("[Registration]"), Some("[Paper]")));
        s.execute("selection.set", &json!({"ids": [gid.0]})).unwrap();
        s.execute("object.qrCode", &json!({"content": "https://example.org"})).unwrap();
        let g = item(&s, gid);
        assert_eq!(g.children().len(), 1, "the group holds the code's frame alone");
        let f = &g.children()[0];
        assert_eq!(f.fill.swatch, "[Paper]");
        let kinds: Vec<Option<QrPart>> = f.children().iter().map(|c| c.qr_part).collect();
        assert_eq!(kinds, [None, Some(QrPart::Finders)], "code and finder patterns once each");

        // A frame holding a background square and the code (Generate QR Code into a frame).
        let mut s = new_doc();
        let fr = s.execute("frame.create", &json!({"rect": [100, 100, 200, 200], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
        s.execute("object.qrCode", &json!({"content": "https://example.com", "background": "[Paper]"})).unwrap();
        s.edit(|d, _| {
            let lid = d.layers[0].id;
            let f = d.item_mut(ItemId(fr)).unwrap();
            let b = f.inner_bounds();
            f.qr_part = None;
            f.fill = designcraft_doc::Fill::none();
            let mut bg = Item::new(ItemId(1_000_000), lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(b));
            bg.fill = designcraft_doc::Fill::swatch("[Paper]");
            bg.qr_part = Some(QrPart::Background);
            f.children_mut().unwrap().insert(0, std::sync::Arc::new(bg));
            Ok(())
        })
        .unwrap();
        s.execute("selection.set", &json!({"ids": [fr]})).unwrap();
        s.execute("object.qrCode", &json!({"content": "https://example.org"})).unwrap();
        let f = item(&s, ItemId(fr));
        assert_eq!((f.fill.swatch.as_str(), f.qr_part), ("[Paper]", Some(QrPart::Background)), "its square's colour moves to the frame");
        assert_eq!(f.children().len(), 1);
    }

    /// A code made before codes had a frame, then moved and rotated with the tools, stays where
    /// it is when edited: the frame takes its transform, and the code doesn't keep it as well.
    #[test]
    fn a_moved_and_rotated_code_from_before_stays_in_place_when_edited() {
        let mut s = new_doc();
        let r = designcraft_geom::Rect::new(100.0, 100.0, 200.0, 200.0);
        let lone = lone_code(&mut s, "https://old.example", r, "[Black]");
        let xf = designcraft_geom::Affine::translate((60.0, 40.0))
            * designcraft_geom::Affine::rotate_about(0.3, designcraft_geom::Point::new(150.0, 150.0));
        s.edit(|d, _| {
            d.item_mut(lone).unwrap().xf = xf;
            Ok(())
        })
        .unwrap();
        // The code's symbol on the spread: its path through every transform above it.
        let on_spread = |s: &Session| {
            let d = &s.doc().unwrap().doc;
            let loc = d.find(lone).unwrap();
            let it = d.item(lone).unwrap();
            (d.parent_xf(&loc) * it.xf).transform_rect_bbox(it.path.bounds().unwrap())
        };
        let before = on_spread(&s);
        s.execute("selection.set", &json!({"ids": [lone.0]})).unwrap();
        s.execute("object.qrCode", &json!({"content": "https://old.example"})).unwrap();
        let after = on_spread(&s);
        assert!((after.center() - before.center()).hypot() < 0.5, "{before:?} → {after:?}");
        assert!((after.width() - before.width()).abs() < 0.5, "{before:?} → {after:?}");
        assert_eq!(code(&s, lone).xf, designcraft_geom::Affine::IDENTITY, "the frame carries the transform");
    }

    /// Edit QR Code shows what a code holds: every type's fields come back from its text.
    #[test]
    fn decode_gives_back_the_fields_encode_was_given() {
        for p in [
            json!({"type": "url", "content": "https://example.com/a?b=c"}),
            json!({"type": "text", "content": "Hello; world"}),
            json!({"type": "sms", "number": "+49 170 1", "message": "Hi there"}),
            json!({"type": "email", "to": "a@b.c", "subject": "Re: one; two", "body": "x"}),
            json!({"type": "vcard", "name": "Ada Lovelace", "org": "Analytical; Engines", "phone": "+44 1", "email": "ada@x.y", "url": "https://x.y"}),
        ] {
            let text = super::encode(&p).unwrap();
            let back = super::decode(&text);
            for (k, v) in p.as_object().unwrap() {
                assert_eq!(&back[k], v, "{k} of {p}");
            }
        }
    }
}
