use super::format::{char_flag_on, format_chars, format_paras, format_targets};
use super::{Result, Session, Value, bad, json};

pub(super) fn warichu_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let has_opts = ["lines", "size", "lineSpacing", "align", "charsBeforeBreak", "charsAfterBreak"].iter().any(|k| p.get(k).is_some());
    let on = match p.get("on").and_then(Value::as_bool) {
        Some(v) => v,
        None if has_opts => true,
        None => !format_targets(s).first().is_some_and(|t| {
            s.doc()
                .ok()
                .and_then(|d| d.doc.text_story(t.story, t.cell))
                .is_some_and(|st| st.char_format_at(if t.range.is_empty() { t.range.start } else { t.range.start + 1 }).over.warichu == Some(true))
        }),
    };
    let mut body = serde_json::Map::new();
    body.insert("warichu".into(), json!(on));
    if on {
        if let Some(v) = p.get("lines").and_then(Value::as_u64) {
            body.insert("warichuLines".into(), json!(v.clamp(2, 16)));
        }
        if let Some(v) = p.get("size").and_then(Value::as_f64).filter(|v| v.is_finite()) {
            body.insert("warichuSize".into(), json!(v.clamp(1.0, 1000.0)));
        }
        if let Some(v) = p.get("lineSpacing").and_then(Value::as_f64).filter(|v| v.is_finite()) {
            body.insert("warichuLineSpacing".into(), json!(v.clamp(-10_000.0, 10_000.0)));
        }
        if let Some(v) = p.get("align").and_then(Value::as_str) {
            body.insert("warichuAlignment".into(), json!(v));
        }
        if let Some(v) = p.get("charsBeforeBreak").and_then(Value::as_u64) {
            body.insert("warichuCharsBeforeBreak".into(), json!(v.min(100)));
        }
        if let Some(v) = p.get("charsAfterBreak").and_then(Value::as_u64) {
            body.insert("warichuCharsAfterBreak".into(), json!(v.min(100)));
        }
    }
    format_chars(s, &Value::Object(body))
}

// Kenten, shatai, auto tate-chu-yoko and grid settings: validated settings for the CJK commands.

/// Longest font, style or swatch name taken from parameters, in characters.
const NAME_MAX_CHARS: usize = 256;

/// A number parameter: absent or `null` → `None`; anything but a finite number is an error.
fn finite_param(p: &Value, key: &str, cmd: &str) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|v| v.is_finite()).map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be a finite number"))),
    }
}

/// A text parameter, cut to `max` characters: absent or `null` → `None`; not text is an error.
fn text_param(p: &Value, key: &str, cmd: &str, max: usize) -> Result<Option<String>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(t)) => Ok(Some(t.chars().take(max).collect())),
        Some(_) => Err(bad(cmd, format!("`{key}` must be text"))),
    }
}

/// A boolean parameter: absent or `null` → `None`; not a boolean is an error.
pub(super) fn bool_param(p: &Value, key: &str, cmd: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_bool().map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be true or false"))),
    }
}

/// An enumeration parameter checked against `T`, as its JSON value.
fn enum_param<T: serde::de::DeserializeOwned + serde::Serialize>(p: &Value, key: &str, cmd: &str) -> Result<Option<Value>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let t: T = serde_json::from_value(v.clone()).map_err(|e| bad(cmd, format!("`{key}`: {e}")))?;
            serde_json::to_value(t).map(Some).map_err(|e| bad(cmd, format!("`{key}`: {e}")))
        }
    }
}

/// A ruby enum parameter, checked against its type.
fn ruby_enum<T: serde::de::DeserializeOwned>(p: &Value, key: &str) -> Result<Option<Value>> {
    let Some(v) = p.get(key) else { return Ok(None) };
    serde_json::from_value::<T>(v.clone()).map_err(|e| bad("type.ruby", format!("{key}: {e}")))?;
    Ok(Some(v.clone()))
}

/// A ruby number parameter clamped to `lo..=hi`, then divided by `div` (100 for percentages);
/// `null` gives `Value::Null` (automatic).
fn ruby_num(p: &Value, key: &str, lo: f64, hi: f64, div: f64) -> Result<Option<Value>> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(Value::Null)),
        Some(v) => {
            let x = v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| bad("type.ruby", format!("{key}: expected a finite number")))?;
            Ok(Some(json!(x.clamp(lo, hi) / div)))
        }
    }
}

pub(super) fn kenten_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    use designcraft_doc::cjk_settings::{AdornmentOverprint, KENTEN_MARK_MAX_CHARS, KentenAlignment, KentenKind, KentenPosition};
    const CMD: &str = "type.kenten";
    let mut body = serde_json::Map::new();
    let mut set = |k: &str, v: Value| {
        body.insert(k.to_string(), v);
    };
    // Validate every setting first, so a bad one changes nothing.
    if let Some(v) = enum_param::<KentenKind>(p, "kind", CMD)? {
        // A preset stores its mark as the character too, as IDML import does, so a custom
        // character left from before isn't drawn in its place.
        if let Some(mark) = serde_json::from_value::<KentenKind>(v.clone()).ok().and_then(KentenKind::mark) {
            set("kentenCharacter", json!(mark));
        }
        set("kentenKind", v);
    }
    if let Some(c) = text_param(p, "character", CMD, KENTEN_MARK_MAX_CHARS)? {
        set("kentenCharacter", json!(c.chars().filter(|c| !c.is_control()).collect::<String>()));
    }
    for (key, attr) in [("font", "kentenFont"), ("fontStyle", "kentenFontStyle"), ("fill", "kentenFill"), ("stroke", "kentenStroke")] {
        if let Some(t) = text_param(p, key, CMD, NAME_MAX_CHARS)? {
            set(attr, json!(t));
        }
    }
    if let Some(v) = finite_param(p, "size", CMD)? {
        set("kentenSize", json!(v.clamp(0.1, 1296.0)));
    }
    if let Some(v) = finite_param(p, "distance", CMD)? {
        set("kentenDistance", json!(v.clamp(-1296.0, 1296.0)));
    }
    for (key, attr) in [("xScale", "kentenXScale"), ("yScale", "kentenYScale")] {
        if let Some(v) = finite_param(p, key, CMD)? {
            set(attr, json!(v.clamp(1.0, 1000.0) / 100.0));
        }
    }
    for (key, attr) in [("fillTint", "kentenFillTint"), ("strokeTint", "kentenStrokeTint")] {
        if let Some(v) = finite_param(p, key, CMD)? {
            set(attr, json!(v.clamp(0.0, 100.0) / 100.0));
        }
    }
    if let Some(v) = finite_param(p, "strokeWeight", CMD)? {
        set("kentenStrokeWeight", json!(v.clamp(0.0, 1000.0)));
    }
    if let Some(v) = enum_param::<KentenPosition>(p, "position", CMD)? {
        set("kentenPosition", v);
    }
    if let Some(v) = enum_param::<KentenAlignment>(p, "alignment", CMD)? {
        set("kentenAlignment", v);
    }
    for (key, attr) in [("overprintFill", "kentenOverprintFill"), ("overprintStroke", "kentenOverprintStroke")] {
        if let Some(v) = enum_param::<AdornmentOverprint>(p, key, CMD)? {
            set(attr, v);
        }
    }
    // `null` returns a setting to the style's (size: half the text size by default).
    for (key, attr) in
        [("size", "kentenSize"), ("fillTint", "kentenFillTint"), ("strokeTint", "kentenStrokeTint"), ("strokeWeight", "kentenStrokeWeight")]
    {
        if p.get(key).is_some_and(Value::is_null) {
            set(attr, Value::Null);
        }
    }
    let on = match bool_param(p, "on", CMD)? {
        Some(v) => v,
        None if !body.is_empty() => true,
        None => !char_flag_on(s, |f| f.kenten),
    };
    if !on {
        // Off keeps the settings for the next time kenten are switched on.
        body = serde_json::Map::new();
    }
    body.insert("kenten".into(), json!(on));
    format_chars(s, &Value::Object(body))
}

pub(super) fn shatai_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "type.shatai";
    let mut body = serde_json::Map::new();
    if let Some(v) = finite_param(p, "magnification", CMD)? {
        body.insert("shataiMagnification".into(), json!(v.clamp(0.0, 90.0)));
    }
    if let Some(v) = finite_param(p, "angle", CMD)? {
        body.insert("shataiAngle".into(), json!(v.rem_euclid(180.0)));
    }
    if let Some(v) = bool_param(p, "adjustRotation", CMD)? {
        body.insert("shataiAdjustRotation".into(), json!(v));
    }
    if let Some(v) = bool_param(p, "adjustTsume", CMD)? {
        body.insert("shataiAdjustTsume".into(), json!(v));
    }
    if body.is_empty() {
        return Err(bad(CMD, "nothing to set: magnification, angle, adjustRotation or adjustTsume"));
    }
    format_chars(s, &Value::Object(body))
}

pub(super) fn auto_tcy_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "type.autoTateChuYoko";
    let mut body = serde_json::Map::new();
    if let Some(v) = finite_param(p, "digits", CMD)? {
        let max = f64::from(designcraft_compose::shape::AUTO_TCY_MAX);
        body.insert("autoTcy".into(), json!(v.round().clamp(0.0, max) as u32));
    }
    if let Some(v) = bool_param(p, "includeRoman", CMD)? {
        body.insert("autoTcyIncludeRoman".into(), json!(v));
    }
    if body.is_empty() {
        return Err(bad(CMD, "nothing to set: digits or includeRoman"));
    }
    format_paras(s, &Value::Object(body))
}

pub(super) fn grid_settings_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    use designcraft_doc::cjk::CharacterAlignment as R;
    const CMD: &str = "type.gridSettings";
    let mut body = serde_json::Map::new();
    let first_only = bool_param(p, "firstLineOnly", CMD)?;
    match text_param(p, "alignment", CMD, 32)?.as_deref() {
        None => {
            if let Some(f) = first_only {
                body.insert("gridAlign".into(), json!(if f { "firstLineOnly" } else { "allLines" }));
            }
        }
        Some("none") => {
            body.insert("gridAlign".into(), json!("none"));
        }
        Some(name) => {
            let reference = match name {
                "romanBaseline" | "baseline" => R::Baseline,
                "emTop" => R::EmTop,
                "emCenter" => R::EmCenter,
                "emBottom" => R::EmBottom,
                "icfTop" => R::IcfTop,
                "icfBottom" => R::IcfBottom,
                other => return Err(bad(CMD, format!("unknown alignment `{other}`"))),
            };
            body.insert("gridAlign".into(), json!(if first_only == Some(true) { "firstLineOnly" } else { "allLines" }));
            body.insert("gridReference".into(), json!(reference));
        }
    }
    if let Some(v) = finite_param(p, "gyoudori", CMD)? {
        let max = 100.0;
        body.insert("gridGyoudori".into(), json!(v.round().clamp(0.0, max) as u32));
    }
    if let Some(v) = bool_param(p, "paragraphGyoudori", CMD)? {
        body.insert("paragraphGyoudori".into(), json!(v));
    }
    if body.is_empty() {
        return Err(bad(CMD, "nothing to set: alignment, firstLineOnly, gyoudori or paragraphGyoudori"));
    }
    format_paras(s, &Value::Object(body))
}

fn ruby_text<'a>(p: &'a Value, key: &str) -> Result<Option<&'a str>> {
    let Some(v) = p.get(key) else { return Ok(None) };
    let t = v.as_str().ok_or_else(|| bad("type.ruby", format!("{key}: expected a string")))?;
    if t.chars().count() > designcraft_doc::ruby::MAX_RUBY_CHARS {
        return Err(bad("type.ruby", format!("{key}: longer than {} characters", designcraft_doc::ruby::MAX_RUBY_CHARS)));
    }
    Ok(Some(t))
}

pub(super) fn ruby_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    use designcraft_doc::cjk_settings::AdornmentOverprint;
    use designcraft_doc::ruby::{RubyAlignment, RubyOverhang, RubyParentSpacing, RubyPosition, RubyType};
    let mut body = serde_json::Map::new();
    let mut put = |attr: &str, v: Option<Value>| {
        if let Some(v) = v {
            body.insert(attr.into(), v);
        }
    };
    put("ruby", ruby_text(p, "text")?.map(|t| json!(t)));
    put("rubyType", ruby_enum::<RubyType>(p, "type")?);
    put("rubyAlignment", ruby_enum::<RubyAlignment>(p, "alignment")?);
    put("rubyPosition", ruby_enum::<RubyPosition>(p, "position")?);
    put("rubyOverhangAmount", ruby_enum::<RubyOverhang>(p, "overhang")?);
    put("rubyParentSpacing", ruby_enum::<RubyParentSpacing>(p, "parentSpacing")?);
    put("rubyOverprintFill", ruby_enum::<AdornmentOverprint>(p, "overprintFill")?);
    put("rubyOverprintStroke", ruby_enum::<AdornmentOverprint>(p, "overprintStroke")?);
    put("rubyXOffset", ruby_num(p, "xOffset", -1000.0, 1000.0, 1.0)?.filter(|v| !v.is_null()));
    put("rubyYOffset", ruby_num(p, "yOffset", -1000.0, 1000.0, 1.0)?.filter(|v| !v.is_null()));
    put("rubyFontSize", ruby_num(p, "size", 0.1, 1296.0, 1.0)?);
    put("rubyXScale", ruby_num(p, "xScale", 1.0, 1000.0, 100.0)?.filter(|v| !v.is_null()));
    put("rubyYScale", ruby_num(p, "yScale", 1.0, 1000.0, 100.0)?.filter(|v| !v.is_null()));
    put("rubyScalingMin", ruby_num(p, "scalingPercent", 10.0, 100.0, 100.0)?.filter(|v| !v.is_null()));
    put("rubyFillTint", ruby_num(p, "fillTint", 0.0, 100.0, 100.0)?);
    put("rubyStrokeTint", ruby_num(p, "strokeTint", 0.0, 100.0, 100.0)?);
    put("rubyStrokeWeight", ruby_num(p, "strokeWeight", 0.0, 800.0, 1.0)?);
    put(
        "rubyAutoTcyDigits",
        ruby_num(p, "autoTcyDigits", 0.0, 9.0, 1.0)?.filter(|v| !v.is_null()).and_then(|v| v.as_f64()).map(|v| json!(v.round() as u32)),
    );
    for (key, attr) in [
        ("openTypePro", "rubyOpenTypePro"),
        ("autoTcyIncludeRoman", "rubyAutoTcyIncludeRoman"),
        ("autoTcyAutoScale", "rubyAutoTcyAutoScale"),
        ("autoAlign", "rubyAutoAlign"),
        ("autoScaling", "rubyAutoScaling"),
    ] {
        if let Some(v) = p.get(key) {
            let b = v.as_bool().ok_or_else(|| bad("type.ruby", format!("{key}: expected true or false")))?;
            put(attr, Some(json!(b)));
        }
    }
    put("rubyFont", ruby_text(p, "font")?.map(|t| json!(t)));
    put("rubyFontStyle", ruby_text(p, "fontStyle")?.map(|t| json!(t)));
    for (key, attr) in [("fill", "rubyFill"), ("stroke", "rubyStroke")] {
        if let Some(name) = ruby_text(p, key)? {
            let known = name.is_empty() || name == designcraft_doc::color::swatch::NONE || s.doc()?.doc.swatch(name).is_some();
            if !known {
                return Err(bad("type.ruby", format!("{key}: no swatch `{name}`")));
            }
            put(attr, Some(json!(name)));
        }
    }
    format_chars(s, &Value::Object(body))
}

#[cfg(test)]
mod tcy_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn tate_chu_yoko_toggles_on_the_selection() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 200, 400], "content": "text", "text": "令和12年", "vertical": true})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        let at = "令和".len();
        s.execute("text.select", &json!({"story": sid, "anchor": at, "focus": at + 2})).unwrap();
        let get = |s: &Session| s.doc().unwrap().doc.stories[&designcraft_doc::StoryId(sid)].char_format_at(at + 1).over.tate_chu_yoko;
        s.execute("type.tateChuYoko", &json!({})).unwrap();
        assert_eq!(get(&s), Some(true));
        s.execute("type.tateChuYoko", &json!({})).unwrap();
        assert_eq!(get(&s), Some(false));
        s.execute("type.tateChuYoko", &json!({"on": true})).unwrap();
        assert_eq!(get(&s), Some(true));
    }

    #[test]
    fn ruby_and_kenten_on_the_selection() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "漢字です"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": "漢字".len()})).unwrap();
        s.execute("type.ruby", &json!({"text": "かんじ"})).unwrap();
        s.execute("type.kenten", &json!({})).unwrap();
        let f = |s: &Session| s.doc().unwrap().doc.stories[&designcraft_doc::StoryId(sid)].char_format_at(1).over.clone();
        assert_eq!(f(&s).ruby.as_deref(), Some("かんじ"));
        assert_eq!(f(&s).kenten, Some(true));
        s.execute("type.kenten", &json!({})).unwrap();
        s.execute("type.ruby", &json!({"text": ""})).unwrap();
        assert_eq!(f(&s).kenten, Some(false));
        assert_eq!(f(&s).ruby.as_deref(), Some(""));
    }

    #[test]
    fn ruby_options_are_checked_and_clamped() {
        use designcraft_doc::ruby::{RubyAlignment, RubyPosition, RubyType};
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "漢字です"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": "漢字".len()})).unwrap();
        let f = |s: &Session| s.doc().unwrap().doc.stories[&designcraft_doc::StoryId(sid)].char_format_at(1).over.clone();
        s.execute(
            "type.ruby",
            &json!({"text": "かん\u{3000}じ", "type": "perCharacter", "alignment": "oneAki", "position": "belowLeft", "size": 1e9, "xScale": -50, "scalingPercent": 5, "autoTcyDigits": 99, "fillTint": 250, "yOffset": -1e12, "fill": "[Black]"}),
        )
        .unwrap();
        let a = f(&s);
        assert_eq!(
            (a.ruby.as_deref(), a.ruby_type, a.ruby_position),
            (Some("かん\u{3000}じ"), Some(RubyType::PerCharacter), Some(RubyPosition::BelowLeft))
        );
        assert_eq!(a.ruby_font_size, Some(Some(1296.0)));
        assert_eq!(a.ruby_x_scale, Some(0.01));
        assert_eq!(a.ruby_scaling_min, Some(0.1));
        assert_eq!(a.ruby_auto_tcy_digits, Some(9));
        assert_eq!(a.ruby_fill_tint, Some(Some(1.0)));
        assert_eq!(a.ruby_y_offset, Some(-1000.0));
        // Options alone keep the reading.
        s.execute("type.ruby", &json!({"alignment": "center"})).unwrap();
        assert_eq!((f(&s).ruby, f(&s).ruby_alignment), (a.ruby.clone(), Some(RubyAlignment::Center)));
        // Refused, and nothing changes: unknown names, non-numbers, overlong text, unknown swatches.
        let long = "あ".repeat(designcraft_doc::ruby::MAX_RUBY_CHARS + 1);
        for p in [
            json!({"alignment": "sideways"}),
            json!({"size": "big"}),
            json!({"text": long}),
            json!({"fill": "Nope"}),
            json!({"autoAlign": 1}),
            json!({"text": 5}),
        ] {
            assert!(s.execute("type.ruby", &p).is_err(), "{p}");
        }
        assert_eq!(f(&s).ruby_alignment, Some(RubyAlignment::Center));
        assert_eq!(f(&s).ruby, a.ruby);
    }

    #[test]
    fn warichu_toggles_on_the_selection_and_undoes() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "注釈です"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        let n = "注釈".len();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": n})).unwrap();
        s.execute("type.warichu", &json!({"lines": 2, "size": 40, "align": "left", "lineSpacing": 1, "charsBeforeBreak": 2, "charsAfterBreak": 2}))
            .unwrap();
        let f = |s: &Session| s.doc().unwrap().doc.stories[&designcraft_doc::StoryId(sid)].char_format_at(1).over.clone();
        let a = f(&s);
        assert_eq!(a.warichu, Some(true));
        assert_eq!(a.warichu_lines, Some(2));
        assert_eq!(a.warichu_size, Some(40.0));
        assert_eq!(a.warichu_line_spacing, Some(1.0));
        assert_eq!(a.warichu_alignment, Some(designcraft_doc::cjk::WarichuAlignment::Left));
        assert_eq!(a.warichu_chars_before_break, Some(2));
        assert_eq!(a.warichu_chars_after_break, Some(2));
        // The characters after the selection are not part of the note.
        assert_eq!(s.doc().unwrap().doc.stories[&designcraft_doc::StoryId(sid)].char_format_at(n + 1).over.warichu, None);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(f(&s).warichu, None);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(f(&s).warichu, Some(true));
        s.execute("type.warichu", &json!({})).unwrap();
        assert_eq!(f(&s).warichu, Some(false));
    }

    #[test]
    fn warichu_up_and_down_move_between_the_rows() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 200], "content": "text", "text": "ABCDEFGHZ"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 8})).unwrap();
        s.execute("type.warichu", &json!({"lines": 2, "align": "left"})).unwrap();
        s.execute("text.select", &json!({"story": sid, "anchor": 0, "focus": 0})).unwrap();
        let down = s.execute("text.move", &json!({"dir": "down"})).unwrap()["pos"].as_u64().unwrap() as usize;
        assert!((4..8).contains(&down), "down enters the second row: {down}");
        let up = s.execute("text.move", &json!({"dir": "up"})).unwrap()["pos"].as_u64().unwrap() as usize;
        assert!(up < 4, "up returns to the first row: {up}");
        s.execute("text.select", &json!({"story": sid, "anchor": 8, "focus": 8})).unwrap();
        let stayed = s.execute("text.move", &json!({"dir": "down"})).unwrap()["pos"].as_u64().unwrap() as usize;
        assert_eq!(stayed, 8, "down from the character after the note does not fall back into a row");
    }
}
