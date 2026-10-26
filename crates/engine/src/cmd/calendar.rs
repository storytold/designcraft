//! Calendars: File › New › Calendar makes a document laid out as one, and Layout › Insert Calendar
//! puts one on any page of the open document (in a given rectangle, the selected empty frame or
//! the page's margins). Either way it is a month per block or twelve months at a glance, for any
//! run of months, in one of a few styles that every look parameter can then override. Every piece
//! is an ordinary frame set in the calendar's own paragraph styles and swatches, so a restyle is
//! one style or swatch change away; [`preview`] renders the first page for the dialog.

use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate};
use designcraft_color::{Color, Swatch};
use designcraft_doc::build::NewDocument;
use designcraft_doc::{
    Align, Capitalization, CharAttrs, Content, Document, Fill, Item, ItemId, Leading, Margins, ParaAttrs, ParaFormat, ParagraphStyle, Selection,
    Shape, SpreadRef, Stroke, TabAlign, TabStop, VerticalJustification, story,
};
use designcraft_geom::{Rect, shapes};
use serde_json::{Value, json};

use super::{CommandSpec, always, bad, bool_or, cmd, has_doc, rect_param, str_param};
use crate::{DocState, EngineError, Result, Session};

const NEW: &str = "file.newCalendar";
const INSERT: &str = "layout.insertCalendar";
/// Longest run of months one call lays out (ten years).
const MAX_MONTHS: i64 = 120;
const MAX_EVENTS: usize = 2000;
/// The look names every style shares; a parameter left out keeps the style's own value.
pub const STYLES: [&str; 5] = ["classic", "modern", "minimal", "bold", "pastel"];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "file.newCalendar", "Calendar…", ["File", "New"], None,
            "{year?: (this year), startMonth?: 1–12 (1), months?: 1–120 (12), layout?: month (a month per page) | year (twelve months at a glance per page), weekStart?: sunday|monday (sunday), size?: letter|a4|a3|a5|tabloid (letter), orientation?: portrait|landscape (month: landscape, or portrait with a photo; year: portrait), width?, height? (points, override size), title?, photo?: bool (an empty picture frame above each month), notes?: bool (a lined notes area), weekNumbers?: bool (ISO), adjacentDays?: bool (days of the months either side, default true), events?: [{date: \"YYYY-MM-DD\", text}], monthNames?: [12 strings], dayNames?: [7 strings, Sunday first], style?: classic|modern|minimal|bold|pastel (classic), accent?, ink?, gridColor?, background? (\"#rrggbb\": accent colour, text colour, grid colour, day cell fill), titleFont?, titleFontStyle?, bodyFont?, bodyFontStyle?, numberFont?, numberFontStyle?, fontScale?: 0.25–4, gridWeight?: 0–12 pt, cellGap?: 0–36 pt, cellPadding?: 0–36 pt, numberPosition?: topLeft|topCenter|topRight|center|bottomLeft|bottomRight, weekdayFormat?: auto|full|short|initial, titleAlign?: left|center|right, showTitle?: bool, showYear?: bool, uppercase?: bool, headerBand?: bool, weekdayStrip?: bool, weekendShading?: bool, weekendTint?: 0–1, weekendAccent?: bool (look keys left out keep the style's own)} → {index, pages, months}",
            always, new_calendar),
        cmd!(
            "layout.insertCalendar",
            "Insert Calendar…",
            ["Layout"],
            None,
            "{page?: 1-based page (the selected empty frame's, or 1), rect?: [x0,y0,x1,y1] (spread coords; default the selected empty frame, which the calendar replaces, or the page's margins), group?: bool (true: each block one group), addPages?: bool (true: add pages at the end when the months run past the last page), year?, startMonth?, months?: 1–120 (month layout: 1, one per page from `page`; year layout: 12 per page), layout?: month|year, weekStart?, photo?, notes?, weekNumbers?, adjacentDays?, events?, monthNames?, dayNames?, style?, accent?, ink?, gridColor?, background?, titleFont?, titleFontStyle?, bodyFont?, bodyFontStyle?, numberFont?, numberFontStyle?, fontScale?, gridWeight?, cellGap?, cellPadding?, numberPosition?, weekdayFormat?, titleAlign?, showTitle?, showYear?, uppercase?, headerBand?, weekdayStrip?, weekendShading?, weekendTint?, weekendAccent? (as file.newCalendar; its styles and swatches are named `Calendar`, `Calendar 2`, … so calendars don't restyle each other)} → {ids, pages}",
            has_doc,
            insert_calendar
        ),
    ]
}

// ---------- dates (through chrono, as AGENTS.md asks) ----------

/// The date `y`-`m`-`d`, when there is one.
fn date(y: i64, m: i64, d: i64) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(i32::try_from(y).ok()?, u32::try_from(m).ok()?, u32::try_from(d).ok()?)
}

/// 0 = Sunday.
fn weekday(d: NaiveDate) -> i64 {
    i64::from(d.weekday().num_days_from_sunday())
}

fn days_in_month(y: i64, m: i64) -> i64 {
    let first = date(y, m, 1);
    let next = if m == 12 { date(y + 1, 1, 1) } else { date(y, m + 1, 1) };
    first.zip(next).map_or(0, |(a, b)| (b - a).num_days())
}

/// ISO 8601 week number of the week holding `d`.
fn iso_week(d: NaiveDate) -> i64 {
    i64::from(d.iso_week().week())
}

/// `d` moved by `n` days (None past chrono's range).
fn add_days(d: NaiveDate, n: i64) -> Option<NaiveDate> {
    d.checked_add_signed(chrono::TimeDelta::try_days(n)?)
}

/// A `YYYY-MM-DD` date in years 1–9999.
fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok().filter(|d| (1..=9999).contains(&d.year()))
}

/// This year by the clock (2027 on a platform without one, which reads 1970).
pub fn this_year() -> i64 {
    let (y, _) = designcraft_doc::time::date_parts(designcraft_doc::vars::now().div_euclid(86_400));
    if y < 2000 { 2027 } else { y }
}

// ---------- parameters ----------

fn int(id: &str, p: &Value, key: &str, default: i64, range: std::ops::RangeInclusive<i64>) -> Result<i64> {
    let Some(v) = p.get(key).filter(|v| !v.is_null()) else { return Ok(default) };
    let n = v.as_i64().ok_or_else(|| bad(id, format!("`{key}` must be a whole number")))?;
    if range.contains(&n) { Ok(n) } else { Err(bad(id, format!("`{key}` must be {}–{}", range.start(), range.end()))) }
}

fn num(id: &str, p: &Value, key: &str, range: std::ops::RangeInclusive<f64>) -> Result<Option<f64>> {
    let Some(v) = p.get(key).filter(|v| !v.is_null()) else { return Ok(None) };
    match v.as_f64() {
        Some(n) if n.is_finite() && range.contains(&n) => Ok(Some(n)),
        _ => Err(bad(id, format!("`{key}` must be a number {}–{}", range.start(), range.end()))),
    }
}

fn flag(id: &str, p: &Value, key: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(id, format!("`{key}` must be true or false"))),
    }
}

/// `key` as one of `choices` (`None` when left out or empty).
fn choice<'a>(id: &str, p: &Value, key: &str, choices: &[&'a str]) -> Result<Option<&'a str>> {
    let Some(s) = str_param(p, key).filter(|s| !s.is_empty()) else {
        return match p.get(key) {
            None | Some(Value::Null) | Some(Value::String(_)) => Ok(None),
            Some(_) => Err(bad(id, format!("`{key}` must be one of {}", choices.join(", ")))),
        };
    };
    choices.iter().find(|c| **c == s).copied().map(Some).ok_or_else(|| bad(id, format!("unknown {key} `{s}` ({})", choices.join(", "))))
}

fn color(id: &str, p: &Value, key: &str) -> Result<Option<Color>> {
    match str_param(p, key).map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Color::from_hex(s).map(Some).ok_or_else(|| bad(id, format!("`{key}`: `{s}` is not a colour (#rrggbb)"))),
        None => Ok(None),
    }
}

fn font_name(p: &Value, key: &str) -> Option<String> {
    str_param(p, key).map(str::trim).filter(|s| !s.is_empty()).map(|s| s.chars().take(128).collect())
}

fn names(id: &str, p: &Value, key: &str, defaults: &[&str]) -> Result<Vec<String>> {
    let Some(v) = p.get(key).filter(|v| !v.is_null()) else { return Ok(defaults.iter().map(|s| s.to_string()).collect()) };
    let list: Vec<String> =
        v.as_array().map(|a| a.iter().filter_map(Value::as_str).map(|s| s.chars().take(40).collect()).collect()).unwrap_or_default();
    if list.len() != defaults.len() {
        return Err(bad(id, format!("`{key}` must be a list of {} strings", defaults.len())));
    }
    Ok(list)
}

// ---------- the look ----------

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Classic,
    Modern,
    Minimal,
    Bold,
    Pastel,
}

#[derive(Clone, Copy, PartialEq)]
enum WeekdayFormat {
    Auto,
    Full,
    Short,
    Initial,
}

struct Look {
    kind: Kind,
    accent: Color,
    ink: Option<Color>,
    grid_color: Option<Color>,
    background: Option<Color>,
    title: (String, String),
    text: (String, String),
    number: (String, String),
    font_scale: f64,
    title_align: Align,
    number_align: Align,
    number_vj: VerticalJustification,
    /// Cell stroke weight; 0 = no grid.
    grid: f64,
    grid_tint: f32,
    /// A rule above each week instead of a grid.
    rules: bool,
    /// The month name on an accent block.
    band: bool,
    /// The weekday names on an accent strip.
    strip: bool,
    /// Weekend cells shaded with this tint of the accent (0 = none).
    weekend_tint: f32,
    /// Weekend day numbers in the accent.
    weekend_accent: bool,
    upper: bool,
    show_title: bool,
    show_year: bool,
    weekday_format: WeekdayFormat,
    /// Space between cells (`None` = the style's).
    cell_gap: Option<f64>,
    cell_padding: f64,
}

const SERIF: &str = "Source Serif 4";
const SANS: &str = "Source Sans 3";
const INTER: &str = "Inter";
const PASTELS: [(&str, [u8; 3]); 4] = [("Rose", [253, 226, 228]), ("Mint", [218, 238, 228]), ("Butter", [255, 241, 193]), ("Sky", [221, 231, 247])];

fn font(f: &str, s: &str) -> (String, String) {
    (f.into(), s.into())
}

fn base_look(name: &str) -> Option<Look> {
    let base = Look {
        kind: Kind::Classic,
        accent: Color::rgb8(166, 30, 45),
        ink: None,
        grid_color: None,
        background: None,
        title: font(SERIF, "Bold"),
        text: font(SERIF, "Regular"),
        number: font(SERIF, "Semibold"),
        font_scale: 1.0,
        title_align: Align::Center,
        number_align: Align::Left,
        number_vj: VerticalJustification::Top,
        grid: 0.5,
        grid_tint: 1.0,
        rules: false,
        band: false,
        strip: false,
        weekend_tint: 0.0,
        weekend_accent: true,
        upper: false,
        show_title: true,
        show_year: true,
        weekday_format: WeekdayFormat::Auto,
        cell_gap: None,
        cell_padding: 5.0,
    };
    Some(match name {
        "classic" => base,
        "modern" => Look {
            kind: Kind::Modern,
            accent: Color::rgb8(15, 118, 110),
            title: font(INTER, "SemiBold"),
            text: font(INTER, "Regular"),
            number: font(INTER, "Medium"),
            title_align: Align::Left,
            number_align: Align::Right,
            grid_tint: 0.25,
            strip: true,
            weekend_tint: 0.08,
            weekend_accent: false,
            upper: true,
            ..base
        },
        "minimal" => Look {
            kind: Kind::Minimal,
            accent: Color::rgb8(224, 90, 43),
            title: font(INTER, "Medium"),
            text: font(INTER, "Regular"),
            number: font(INTER, "Regular"),
            title_align: Align::Left,
            grid: 0.0,
            rules: true,
            upper: true,
            ..base
        },
        "bold" => Look {
            kind: Kind::Bold,
            accent: Color::rgb8(67, 56, 202),
            title: font(SANS, "Bold"),
            text: font(SANS, "Semibold"),
            number: font(SANS, "Bold"),
            title_align: Align::Left,
            grid: 1.5,
            band: true,
            weekend_tint: 0.12,
            upper: true,
            ..base
        },
        "pastel" => Look {
            kind: Kind::Pastel,
            accent: Color::rgb8(214, 92, 140),
            title: font(SANS, "Semibold"),
            text: font(SANS, "Regular"),
            number: font(SANS, "Semibold"),
            grid: 0.0,
            cell_gap: Some(3.0),
            ..base
        },
        _ => return None,
    })
}

/// The look parameters `style` stands for, keyed as the commands take them, for a dialog to show
/// before anything is overridden. Null for an unknown style.
pub fn look_defaults(style: &str) -> Value {
    let Some(l) = base_look(style) else { return Value::Null };
    let align = |a: Align| match a {
        Align::Center => "center",
        Align::Right => "right",
        _ => "left",
    };
    let position = match (l.number_align, l.number_vj) {
        (Align::Center, VerticalJustification::Center) => "center",
        (Align::Center, _) => "topCenter",
        (Align::Right, VerticalJustification::Bottom) => "bottomRight",
        (Align::Right, _) => "topRight",
        (_, VerticalJustification::Bottom) => "bottomLeft",
        _ => "topLeft",
    };
    json!({
        "accent": l.accent.to_hex(), "ink": "#000000", "gridColor": if l.kind == Kind::Bold { l.accent.to_hex() } else { "#000000".into() }, "background": "#ffffff",
        "titleFont": l.title.0, "titleFontStyle": l.title.1, "bodyFont": l.text.0, "bodyFontStyle": l.text.1,
        "numberFont": l.number.0, "numberFontStyle": l.number.1, "fontScale": l.font_scale, "gridWeight": l.grid,
        "cellGap": l.cell_gap.unwrap_or(0.0), "cellPadding": l.cell_padding, "numberPosition": position, "weekdayFormat": "auto",
        "titleAlign": align(l.title_align), "showTitle": l.show_title, "showYear": l.show_year, "uppercase": l.upper,
        "headerBand": l.band, "weekdayStrip": l.strip, "weekendShading": l.weekend_tint > 0.0, "weekendTint": l.weekend_tint, "weekendAccent": l.weekend_accent,
    })
}

fn look(id: &str, p: &Value) -> Result<Look> {
    let style = choice(id, p, "style", &STYLES)?.unwrap_or("classic");
    let mut l = base_look(style).ok_or_else(|| bad(id, format!("unknown style `{style}`")))?;
    if let Some(c) = color(id, p, "accent")? {
        l.accent = c;
    }
    l.ink = color(id, p, "ink")?;
    l.grid_color = color(id, p, "gridColor")?;
    l.background = color(id, p, "background")?;
    for (key, slot) in [("title", &mut l.title), ("body", &mut l.text), ("number", &mut l.number)] {
        if let Some(f) = font_name(p, &format!("{key}Font")) {
            // A new family starts from its Regular unless a style is given too.
            *slot = (f, "Regular".into());
        }
        if let Some(s) = font_name(p, &format!("{key}FontStyle")) {
            slot.1 = s;
        }
    }
    if let Some(v) = num(id, p, "fontScale", 0.25..=4.0)? {
        l.font_scale = v;
    }
    if let Some(v) = num(id, p, "gridWeight", 0.0..=12.0)? {
        l.grid = v;
        l.rules = false;
        if l.grid_color.is_none() && l.kind != Kind::Bold {
            l.grid_tint = l.grid_tint.max(0.25);
        }
    }
    if let Some(v) = num(id, p, "cellGap", 0.0..=36.0)? {
        l.cell_gap = Some(v);
    }
    if let Some(v) = num(id, p, "cellPadding", 0.0..=36.0)? {
        l.cell_padding = v;
    }
    if let Some(pos) = choice(id, p, "numberPosition", &["topLeft", "topCenter", "topRight", "center", "bottomLeft", "bottomRight"])? {
        use VerticalJustification as V;
        (l.number_align, l.number_vj) = match pos {
            "topCenter" => (Align::Center, V::Top),
            "topRight" => (Align::Right, V::Top),
            "center" => (Align::Center, V::Center),
            "bottomLeft" => (Align::Left, V::Bottom),
            "bottomRight" => (Align::Right, V::Bottom),
            _ => (Align::Left, V::Top),
        };
    }
    if let Some(f) = choice(id, p, "weekdayFormat", &["auto", "full", "short", "initial"])? {
        l.weekday_format = match f {
            "full" => WeekdayFormat::Full,
            "short" => WeekdayFormat::Short,
            "initial" => WeekdayFormat::Initial,
            _ => WeekdayFormat::Auto,
        };
    }
    if let Some(a) = choice(id, p, "titleAlign", &["left", "center", "right"])? {
        l.title_align = match a {
            "center" => Align::Center,
            "right" => Align::Right,
            _ => Align::Left,
        };
    }
    let set = |slot: &mut bool, key: &str| -> Result<()> {
        if let Some(b) = flag(id, p, key)? {
            *slot = b;
        }
        Ok(())
    };
    set(&mut l.show_title, "showTitle")?;
    set(&mut l.show_year, "showYear")?;
    set(&mut l.upper, "uppercase")?;
    set(&mut l.band, "headerBand")?;
    set(&mut l.strip, "weekdayStrip")?;
    set(&mut l.weekend_accent, "weekendAccent")?;
    if let Some(on) = flag(id, p, "weekendShading")? {
        l.weekend_tint = if on { l.weekend_tint.max(0.1) } else { 0.0 };
    }
    if let Some(t) = num(id, p, "weekendTint", 0.0..=1.0)? {
        l.weekend_tint = t as f32;
    }
    Ok(l)
}

// ---------- what the calendar shows ----------

struct Opts {
    year: i64,
    start_month: i64,
    months: i64,
    year_layout: bool,
    week_start: i64,
    photo: bool,
    notes: bool,
    week_numbers: bool,
    adjacent: bool,
    events: BTreeMap<NaiveDate, Vec<String>>,
    month_names: Vec<String>,
    day_names: Vec<String>,
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/// The content params; `default_months` applies to the month layout (the year layout's is 12).
fn options(id: &str, p: &Value, default_months: i64) -> Result<Opts> {
    let year_layout = choice(id, p, "layout", &["month", "year"])? == Some("year");
    let year = int(id, p, "year", this_year(), 1..=9999)?;
    let start_month = int(id, p, "startMonth", 1, 1..=12)?;
    let months = int(id, p, "months", if year_layout { 12 } else { default_months }, 1..=MAX_MONTHS)?;
    if year * 12 + start_month - 1 + months > 10_000 * 12 {
        return Err(bad(id, "the calendar must end by the year 9999"));
    }
    let week_start = i64::from(choice(id, p, "weekStart", &["sunday", "monday"])? == Some("monday"));
    let mut events: BTreeMap<NaiveDate, Vec<String>> = BTreeMap::new();
    if let Some(list) = p.get("events").filter(|v| !v.is_null()) {
        let list = list.as_array().ok_or_else(|| bad(id, "`events` must be a list of {date, text}"))?;
        if list.len() > MAX_EVENTS {
            return Err(bad(id, format!("at most {MAX_EVENTS} events")));
        }
        for e in list {
            let date = str_param(e, "date").ok_or_else(|| bad(id, "each event needs a `date`"))?;
            let day = parse_date(date).ok_or_else(|| bad(id, format!("`{date}` is not a date (YYYY-MM-DD)")))?;
            let text: String = str_param(e, "text").unwrap_or("").replace(['\n', '\r'], " ").chars().take(200).collect();
            if !text.trim().is_empty() {
                events.entry(day).or_default().push(text);
            }
        }
    }
    Ok(Opts {
        year,
        start_month,
        months,
        year_layout,
        week_start,
        photo: flag(id, p, "photo")?.unwrap_or(false),
        notes: flag(id, p, "notes")?.unwrap_or(false),
        week_numbers: flag(id, p, "weekNumbers")?.unwrap_or(false),
        adjacent: flag(id, p, "adjacentDays")?.unwrap_or(true),
        events,
        month_names: names(id, p, "monthNames", &MONTHS)?,
        day_names: names(id, p, "dayNames", &DAYS)?,
    })
}

impl Opts {
    /// (year, month 1–12) of the `i`th month.
    fn month(&self, i: i64) -> (i64, i64) {
        let n = self.year * 12 + self.start_month - 1 + i;
        (n.div_euclid(12), n.rem_euclid(12) + 1)
    }
    fn month_name(&self, m: i64) -> &str {
        usize::try_from(m - 1).ok().and_then(|i| self.month_names.get(i)).map_or("", String::as_str)
    }
    /// The weekday name of column `col`.
    fn day_name(&self, col: i64) -> &str {
        usize::try_from((col + self.week_start) % 7).ok().and_then(|i| self.day_names.get(i)).map_or("", String::as_str)
    }
    /// Blocks (pages) the run takes.
    fn blocks(&self) -> i64 {
        if self.year_layout { (self.months + 11) / 12 } else { self.months }
    }
}

// ---------- styles and swatches ----------

/// The names one calendar's styles and swatches share: `Calendar Day`, `Calendar 2 Day`, ….
struct Names {
    prefix: String,
}

impl Names {
    fn n(&self, s: &str) -> String {
        format!("{} {s}", self.prefix)
    }
    /// The first prefix no style or swatch of `d` uses yet.
    fn free(d: &Document) -> Names {
        let taken = |p: &str| d.styles.para(&format!("{p} Day")).is_some() || d.swatch(&format!("{p} Accent")).is_some();
        let prefix = std::iter::once("Calendar".to_string()).chain((2..1000).map(|n| format!("Calendar {n}"))).find(|p| !taken(p));
        Names { prefix: prefix.unwrap_or_else(|| format!("Calendar {}", d.styles.paragraph.len())) }
    }
}

struct Swatches {
    accent: String,
    ink: String,
    grid: String,
    background: Option<String>,
    pastels: Vec<String>,
}

fn add_swatches(d: &mut Document, l: &Look, n: &Names) -> Swatches {
    let mut add = |name: String, c: Color| {
        d.swatches.retain(|s| s.name != name);
        d.swatches.push(Swatch::color(&name, c));
        name
    };
    let accent = add(n.n("Accent"), l.accent);
    let ink = l.ink.map_or_else(|| designcraft_color::swatch::BLACK.to_string(), |c| add(n.n("Ink"), c));
    let grid = match l.grid_color {
        Some(c) => add(n.n("Grid"), c),
        None if l.kind == Kind::Bold => accent.clone(),
        None => ink.clone(),
    };
    let background = l.background.map(|c| add(n.n("Background"), c));
    let pastels =
        if l.kind == Kind::Pastel { PASTELS.iter().map(|(name, [r, g, b])| add(n.n(name), Color::rgb8(*r, *g, *b))).collect() } else { vec![] };
    Swatches { accent, ink, grid, background, pastels }
}

fn chars(font: &(String, String), size: f64, fill: &str) -> CharAttrs {
    CharAttrs {
        font_family: Some(font.0.clone()),
        font_style: Some(font.1.clone()),
        size: Some(size),
        leading: Some(Leading::Points(size * 1.15)),
        fill: Some(fill.into()),
        ..Default::default()
    }
}

fn add_style(d: &mut Document, name: &str, para: ParaAttrs, chars: CharAttrs) {
    let st = d.styles_mut();
    st.paragraph.retain(|s| s.name != name);
    st.paragraph.push(ParagraphStyle {
        name: name.into(),
        based_on: Some(story::BASIC_PARAGRAPH.into()),
        next_style: None,
        para: ParaAttrs { hyphenate: Some(false), ..para },
        chars,
        shortcut: String::new(),
    });
}

fn aligned(a: Align) -> ParaAttrs {
    ParaAttrs { align: Some(a), ..Default::default() }
}

/// The type scale of a block in `area` (a Letter page's margins = 1).
fn scale(area: Rect, l: &Look) -> f64 {
    (area.width().min(area.height()) / 540.0).clamp(0.02, 4.0) * l.font_scale
}

fn title_size(o: &Opts, k: f64) -> f64 {
    if o.photo && !o.year_layout { 30.0 * k } else { 40.0 * k }
}

/// Twelve-up geometry of the year layout in `area`.
struct YearGeom {
    cols: i64,
    /// The notes area under the months (0 = none).
    notes_h: f64,
    title_h: f64,
    top: f64,
    gutter: f64,
    mw: f64,
    mh: f64,
    lead: f64,
    size: f64,
}

fn year_geom(area: Rect, l: &Look, o: &Opts, k: f64) -> YearGeom {
    let h = area.height();
    let title_h = if l.show_title { 54.0 * k } else { 0.0 };
    let top = if l.show_title { title_h + 20.0 * k } else { 0.0 };
    // Notes take up to 15% under the months, which keep at least 55% of the height.
    let notes_h = if o.notes {
        let room = h - top - h * 0.55 - 12.0 * k;
        let n = (h * 0.15).max(NOTES_MIN * k).min(room);
        if n >= NOTES_LABEL * k { n } else { 0.0 }
    } else {
        0.0
    };
    let months_h = h - top - if notes_h > 0.0 { notes_h + 12.0 * k } else { 0.0 };
    let (cols, rows) = if area.width() > area.height() { (4_i64, 3_i64) } else { (3, 4) };
    let gutter = 18.0 * k;
    let mw = ((area.width() - gutter * (cols - 1) as f64) / cols as f64).max(1.0);
    let mh = ((months_h - gutter * (rows - 1) as f64) / rows as f64).max(1.0);
    // No floors: a tiny block gets tiny type rather than text too big to show.
    let lead = ((mh - 18.0 * k) / 7.0).max(0.1);
    let size = (lead * 0.62).min(mw / 7.0 * 0.5).max(0.05);
    YearGeom { cols, notes_h, title_h, top, gutter, mw, mh, lead, size }
}

/// Where one month's parts go in `area` and how big its type is, so every piece of text fits its
/// frame (text that doesn't fit is not drawn at all). The sizes come from the six-week case,
/// the tightest; `rows` sets the cell height used for drawing.
struct MonthGeom {
    photo_h: f64,
    /// Space under the title, under the accent band's top, and under the day names.
    header_gap: f64,
    band_inset: f64,
    wd_gap: f64,
    title_size: f64,
    month_h: f64,
    header_h: f64,
    wd_h: f64,
    weekday_size: f64,
    weekday_format: WeekdayFormat,
    notes_h: f64,
    wk_w: f64,
    cell_w: f64,
    cell_h: f64,
    gap: f64,
    /// Cell stroke weight, kept to a tenth of the cell; the text sits inside it.
    grid: f64,
    pad: f64,
    number_size: f64,
    event_size: f64,
    week_size: f64,
}

/// The notes area's label height, and its height with one ruled line under the label (× scale).
const NOTES_LABEL: f64 = 14.0;
const NOTES_MIN: f64 = 30.0;

/// The notes area: its label and ruled lines in the `notes_h` at the bottom of `area`. Returns
/// the new bottom for what goes above it.
fn notes_area(b: &mut Builder, area: Rect, notes_h: f64, k: f64) -> Result<f64> {
    if notes_h <= 0.0 {
        return Ok(area.y1);
    }
    let ny = area.y1 - notes_h;
    b.text(Rect::new(area.x0, ny, area.x1, ny + NOTES_LABEL * k), "Notes", "Notes")?;
    let mut ly = ny + NOTES_MIN * k;
    while ly <= area.y1 + 0.01 {
        b.line(area.x0, area.x1, ly, 0.5, 0.35)?;
        ly += 18.0 * k;
    }
    Ok(ny - 12.0 * k)
}

/// Points a line of `size` type needs in a frame: the font's full ascent and descent, which can
/// pass 1.4 × the size (Source Sans 3), with room to spare.
const LINE: f64 = 1.6;

/// About how wide `chars` characters of `size` type set, for fitting names to their frames.
fn text_width(chars: usize, size: f64, caps: bool, tracking: f64) -> f64 {
    chars as f64 * size * (if caps { 0.72 } else { 0.58 } + tracking / 1000.0)
}

fn month_geom(area: Rect, l: &Look, o: &Opts, k: f64, rows: i64) -> MonthGeom {
    let (w, h) = (area.width().max(1.0), area.height().max(1.0));
    let longest = o.month_names.iter().map(|n| n.chars().count()).max().unwrap_or(9).max(1);
    let year_w = if l.show_year && l.title_align != Align::Center { text_width(4, title_size(o, k) * 0.55, false, 60.0) + 10.0 * k } else { 0.0 };
    let title_size = title_size(o, k)
        .min(h * 0.09)
        .min((w - year_w).max(w * 0.4) / (text_width(longest, 1.0, l.upper, if l.upper { 40.0 } else { 0.0 }) + 0.1))
        .max(0.1);
    let month_h = title_size * 1.3;
    let header_h = if !l.show_title {
        0.0
    } else if l.title_align == Align::Center && l.show_year {
        month_h + title_size * 0.5
    } else {
        month_h
    };
    let (mut title_size, mut month_h, mut header_h) = (title_size, month_h, header_h);
    let mut header_gap = if l.band { 18.0 } else { 12.0 } * k;
    let mut band_inset = if l.band { 6.0 * k } else { 0.0 };
    let mut wd_h = (18.0 * k).min(h * 0.06).max(0.1);
    let mut wd_gap = if l.strip { 0.0 } else { 4.0 * k };
    // Big type in a small block: the title and day names shrink so the days keep 60%.
    let chrome = if l.show_title { header_h + header_gap + band_inset } else { 0.0 } + wd_h + wd_gap;
    if chrome > h * 0.4 {
        let f = h * 0.4 / chrome;
        for v in [&mut title_size, &mut month_h, &mut header_h, &mut header_gap, &mut band_inset, &mut wd_h, &mut wd_gap] {
            *v *= f;
        }
    }
    let header_total = if l.show_title { header_h + header_gap + band_inset } else { 0.0 };
    let wd_total = wd_h + wd_gap;
    let mut photo_h = if o.photo { h * 0.42 } else { 0.0 };
    let mut notes_h = if o.notes { (h * 0.16).max(30.0 * k) } else { 0.0 };
    // The days keep at least 40% of the height. When the picture and the notes want more than
    // what's left, both shrink in proportion; one is left out only when it would be too small
    // to use (the notes first, then the picture).
    let rest = |photo: f64, notes: f64| {
        h - header_total - wd_total - if photo > 0.0 { photo + 14.0 * k } else { 0.0 } - if notes > 0.0 { notes + 12.0 * k } else { 0.0 }
    };
    let grid_min = h * 0.4;
    // Room still missing for the days (a hair of rounding counts as none).
    let short = |photo: f64, notes: f64| {
        let d = grid_min - rest(photo, notes);
        if d > 1e-6 { d } else { 0.0 }
    };
    // In turn: the picture down to a third, the notes down to their label and one ruled line,
    // the picture down to nothing, then the notes down to their label alone.
    if photo_h > 0.0 {
        photo_h = (photo_h - short(photo_h, notes_h)).max(photo_h * 0.35);
    }
    let notes_min = NOTES_MIN * k;
    if notes_h > 0.0 {
        notes_h = (notes_h - short(photo_h, notes_h)).max(notes_min.min(notes_h));
    }
    if photo_h > 0.0 {
        photo_h -= short(photo_h, notes_h);
        if photo_h < 24.0 * k {
            photo_h = 0.0;
        }
    }
    if notes_h > 0.0 {
        notes_h -= short(photo_h, notes_h);
        if notes_h < NOTES_LABEL * k {
            notes_h = 0.0;
        }
    }
    let wk_w = if o.week_numbers { (20.0 * k).min(w * 0.08) } else { 0.0 };
    let cell_w = ((w - wk_w) / 7.0).max(0.5);
    let grid_h = rest(photo_h, notes_h).max(1.0);
    let cell_h = grid_h / rows.max(1) as f64;
    let tight_h = grid_h / 6.0;
    let gap = (l.cell_gap.unwrap_or(0.0) * k).min(cell_w * 0.12).min(tight_h * 0.12);
    let grid = l.grid.min(cell_w * 0.1).min(tight_h * 0.1);
    let (inner_w, inner_h) = ((cell_w - 2.0 * gap - grid).max(0.5), (tight_h - 2.0 * gap - grid).max(0.5));
    let pad = (l.cell_padding * k).min(inner_w * 0.15).min(inner_h * 0.15);
    let number_size = (13.0 * k).min((inner_h - 2.0 * pad) / LINE).min((inner_w - 2.0 * pad) / 1.4).max(0.1);
    let weekday_size = (7.5 * k).min(wd_h / LINE).max(0.1);
    // Day names step down to what fits the column: full, short, then one letter.
    let fits = |n: usize| text_width(n, weekday_size, true, 80.0) <= cell_w - 2.0 * gap - 4.0 * k.min(cell_w * 0.05);
    let longest_day = o.day_names.iter().map(|n| n.chars().count()).max().unwrap_or(9);
    let weekday_format = match l.weekday_format {
        WeekdayFormat::Auto | WeekdayFormat::Full if fits(longest_day) && !(l.weekday_format == WeekdayFormat::Auto && cell_w < 80.0 * k) => {
            WeekdayFormat::Full
        }
        WeekdayFormat::Initial => WeekdayFormat::Initial,
        _ if fits(3) => WeekdayFormat::Short,
        _ => WeekdayFormat::Initial,
    };
    MonthGeom {
        photo_h,
        header_gap,
        band_inset,
        wd_gap,
        title_size,
        month_h,
        header_h,
        wd_h,
        weekday_size,
        weekday_format,
        notes_h,
        wk_w,
        cell_w,
        cell_h,
        gap,
        grid,
        pad,
        number_size,
        event_size: (7.5 * k).min(number_size * 0.65).max(0.1),
        week_size: (7.5 * k).min(wk_w * 0.4).min((tight_h - 2.0 * (tight_h * 0.1).min(1.0)) / LINE).max(0.1),
    }
}

/// Lines `text` takes wrapped at `per_line` characters, word by word (a longer word breaks).
fn wrapped_lines(text: &str, per_line: usize) -> usize {
    let per_line = per_line.max(1);
    let (mut lines, mut used) = (0, 0);
    for word in text.split_whitespace() {
        let n = word.chars().count();
        if used > 0 && used + 1 + n <= per_line {
            used += 1 + n;
        } else {
            lines += n.div_ceil(per_line).max(1);
            used = match n % per_line {
                0 => per_line,
                r => r,
            };
        }
    }
    lines.max(1)
}

/// The events of a day that fit under its number in a cell `w` × `h` (inside its inset), by a
/// cautious estimate of how they wrap.
fn fitting_events<'a>(events: &'a [String], g: &MonthGeom, w: f64, h: f64) -> Vec<&'a str> {
    let line = g.event_size * 1.3;
    let per_line = (w / (g.event_size * 0.62)).floor().max(1.0) as usize;
    let mut room = h - g.number_size * LINE;
    let mut out = Vec::new();
    for e in events {
        let need = wrapped_lines(e, per_line) as f64 * line;
        if need > room {
            break;
        }
        room -= need;
        out.push(e.as_str());
    }
    out
}

fn define_styles(d: &mut Document, l: &Look, o: &Opts, n: &Names, sw: &Swatches, area: Rect) {
    let k = scale(area, l);
    let paper = designcraft_color::swatch::PAPER;
    let ink = sw.ink.as_str();
    let accent = sw.accent.as_str();
    let caps = |mut c: CharAttrs, track: f64| {
        if l.upper {
            c.capitalization = Some(Capitalization::AllCaps);
            c.tracking = Some(track);
        }
        c
    };
    let g = month_geom(area, l, o, k, 6);
    let ts = if o.year_layout { title_size(o, k).min(area.height() * 0.09) } else { g.title_size };
    add_style(d, &n.n("Month"), aligned(l.title_align), caps(chars(&l.title, ts, if l.band { paper } else { ink }), 40.0));
    // A centred title carries a small spaced-out year under the month; the others a large one
    // on the opposite side.
    let (year_align, year_size, year_track) = match l.title_align {
        Align::Center => (Align::Center, 0.3, 300.0),
        Align::Right => (Align::Left, 0.55, 60.0),
        _ => (Align::Right, 0.55, 60.0),
    };
    add_style(
        d,
        &n.n("Year"),
        aligned(year_align),
        CharAttrs { tracking: Some(year_track), ..chars(&l.text, ts * year_size, if l.band { paper } else { accent }) },
    );
    let weekday_align = match l.number_align {
        Align::Right => Align::Right,
        Align::Left if l.kind == Kind::Minimal => Align::Left,
        _ => Align::Center,
    };
    let weekday_fill = if l.strip {
        paper
    } else if l.kind == Kind::Minimal {
        accent
    } else {
        ink
    };
    add_style(
        d,
        &n.n("Weekday"),
        aligned(weekday_align),
        CharAttrs { capitalization: Some(Capitalization::AllCaps), tracking: Some(80.0), ..chars(&l.title, g.weekday_size, weekday_fill) },
    );
    let ns = g.number_size;
    add_style(d, &n.n("Day"), aligned(l.number_align), chars(&l.number, ns, ink));
    add_style(d, &n.n("Weekend"), aligned(l.number_align), chars(&l.number, ns, if l.weekend_accent { accent } else { ink }));
    add_style(d, &n.n("Other Month"), aligned(l.number_align), CharAttrs { fill_tint: Some(0.3), ..chars(&l.number, ns, ink) });
    add_style(d, &n.n("Event"), aligned(l.number_align), CharAttrs { fill_tint: Some(0.85), ..chars(&l.text, g.event_size, ink) });
    add_style(d, &n.n("Week Number"), aligned(Align::Center), CharAttrs { fill_tint: Some(0.6), ..chars(&l.text, g.week_size, ink) });
    add_style(d, &n.n("Notes"), aligned(Align::Left), caps(chars(&l.title, 8.0 * k, accent), 80.0));
    if o.year_layout {
        let g = year_geom(area, l, o, k);
        let tabs: Vec<TabStop> = (0..7)
            .map(|c| TabStop { position: (c as f64 + 0.5) * g.mw / 7.0, align: TabAlign::Center, leader: String::new(), align_on: String::new() })
            .collect();
        let tabbed = ParaAttrs { tabs: Some(tabs), ..Default::default() };
        add_style(d, &n.n("Mini Month"), aligned(Align::Left), caps(chars(&l.title, 11.0 * k, accent), 60.0));
        add_style(d, &n.n("Mini Day"), tabbed.clone(), CharAttrs { leading: Some(Leading::Points(g.lead)), ..chars(&l.number, g.size, ink) });
        add_style(d, &n.n("Mini Weekday"), tabbed, CharAttrs { leading: Some(Leading::Points(g.lead)), ..chars(&l.title, g.size * 0.85, accent) });
    }
}

// ---------- building ----------

struct Builder<'a> {
    d: &'a mut Document,
    sr: SpreadRef,
    n: &'a Names,
    sw: &'a Swatches,
    /// Every item made, for grouping.
    made: Vec<ItemId>,
}

impl Builder<'_> {
    fn text(&mut self, r: Rect, text: &str, style: &str) -> Result<ItemId> {
        let lid = self.d.default_layer();
        let (id, _) = self.d.add_text_frame(self.sr, r, lid, text, ParaFormat { style: self.n.n(style), ..Default::default() })?;
        if let Some(tf) = self.d.item_mut(id).and_then(Item::text_frame_mut) {
            tf.options.inset = [0.0; 4];
        }
        self.made.push(id);
        Ok(id)
    }
    /// A frame with `inset` and vertical justification `vj`.
    fn text_with(&mut self, r: Rect, text: &str, style: &str, inset: f64, vj: VerticalJustification) -> Result<ItemId> {
        let id = self.text(r, text, style)?;
        if let Some(tf) = self.d.item_mut(id).and_then(Item::text_frame_mut) {
            tf.options.inset = [inset; 4];
            tf.options.vertical_justification = vj;
        }
        Ok(id)
    }
    fn rect(&mut self, r: Rect, fill: Fill, stroke: Option<Stroke>) -> Result<ItemId> {
        let id = ItemId(self.d.alloc());
        let mut it = Item::new(id, self.d.default_layer(), Shape::Rectangle, shapes::rectangle(r));
        it.fill = fill;
        if let Some(s) = stroke {
            it.stroke = s;
        }
        self.d.insert_item(self.sr, it, None)?;
        self.made.push(id);
        Ok(id)
    }
    fn line(&mut self, x0: f64, x1: f64, y: f64, weight: f64, tint: f32) -> Result<()> {
        let id = ItemId(self.d.alloc());
        let mut it = Item::new(id, self.d.default_layer(), Shape::GraphicLine, shapes::line((x0, y).into(), (x1, y).into()));
        it.stroke = Stroke { weight, swatch: self.sw.ink.clone(), tint, ..Stroke::default() };
        self.d.insert_item(self.sr, it, None)?;
        self.made.push(id);
        Ok(())
    }
    fn style_fill(&mut self, id: ItemId, fill: Fill, stroke: Option<Stroke>) {
        if let Some(it) = self.d.item_mut(id) {
            it.fill = fill;
            if let Some(s) = stroke {
                it.stroke = s;
            }
        }
    }
    /// Give paragraphs from `from` (0-based) of frame `id`'s story the style `style`.
    fn para_style(&mut self, id: ItemId, from: usize, style: &str) {
        let name = self.n.n(style);
        let Some(sid) = self.d.item(id).and_then(Item::text_frame).map(|t| t.story) else { return };
        if let Some(s) = self.d.stories.get_mut(&sid) {
            for p in std::sync::Arc::make_mut(s).paras.iter_mut().skip(from) {
                p.style = name.clone();
            }
        }
    }
    /// Wrap everything made into one group, in place of its first item.
    fn group(&mut self) -> Result<Option<ItemId>> {
        if self.made.len() < 2 {
            return Ok(self.made.first().copied());
        }
        let layer = self.d.default_layer();
        let gid = ItemId(self.d.alloc());
        let sp = self.d.spread_mut(self.sr).ok_or_else(|| EngineError::Other("the calendar's spread is gone".into()))?;
        let at = sp.items.iter().position(|i| self.made.contains(&i.id)).unwrap_or(sp.items.len());
        let (kids, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut sp.items).into_iter().partition(|i| self.made.contains(&i.id));
        sp.items = rest;
        let mut g = Item::new(gid, layer, Shape::Group, designcraft_geom::PathData::default());
        g.content = Content::Group { items: kids };
        sp.items.insert(at.min(sp.items.len()), std::sync::Arc::new(g));
        self.made = vec![gid];
        Ok(Some(gid))
    }
}

fn tint(swatch: &str, t: f32) -> Fill {
    Fill { tint: t, ..Fill::swatch(swatch) }
}

/// One month in `area`. `page` is the page's own rectangle when the block fills it, so an accent
/// band can bleed off its top edge.
fn month_block(b: &mut Builder, l: &Look, o: &Opts, area: Rect, page: Option<Rect>, y: i64, m: i64) -> Result<()> {
    let k = scale(area, l);
    let no_date = || EngineError::Other(format!("no calendar for {y}-{m:02}"));
    let first = date(y, m, 1).ok_or_else(no_date)?;
    let offset = (weekday(first) - o.week_start).rem_euclid(7);
    let dim = days_in_month(y, m);
    let rows = (offset + dim + 6) / 7;
    let g = month_geom(area, l, o, k, rows);
    let mut top = area.y0;
    if g.photo_h > 0.0 {
        let id = b.rect(Rect::new(area.x0, top, area.x1, top + g.photo_h), tint(&b.sw.accent, 0.12), None)?;
        if let Some(it) = b.d.item_mut(id) {
            it.object_style = designcraft_doc::BASIC_GRAPHICS_FRAME.into();
            it.alt_text = "Calendar photo".into();
        }
        top += g.photo_h + 14.0 * k;
    }
    let centered = l.title_align == Align::Center;
    if l.show_title {
        // The band bleeds off a page it fills, and sits inside a block placed in a rectangle.
        let bleeds = page.is_some() && (top - area.y0).abs() < 0.01;
        let pad = if l.band && !bleeds { (10.0 * k).min(area.width() * 0.04) } else { 0.0 };
        if l.band {
            let bottom = top + g.header_h + g.header_gap * 0.45;
            let r = match page {
                Some(pg) if bleeds => Rect::new(pg.x0 - 9.0, pg.y0 - 9.0, pg.x1 + 9.0, bottom),
                _ => Rect::new(area.x0, top, area.x1, bottom),
            };
            b.rect(r, Fill::swatch(&b.sw.accent), None)?;
            top += if bleeds { 0.0 } else { g.band_inset };
        }
        let title_rect = Rect::new(area.x0 + pad, top, area.x1 - pad, top + g.header_h);
        let name = o.month_name(m).to_string();
        if centered {
            let split = if l.show_year { title_rect.y0 + g.month_h } else { title_rect.y1 };
            b.text_with(Rect::new(title_rect.x0, title_rect.y0, title_rect.x1, split), &name, "Month", 0.0, VerticalJustification::Bottom)?;
            if l.show_year {
                b.text_with(Rect::new(title_rect.x0, split, title_rect.x1, title_rect.y1), &y.to_string(), "Year", 0.0, VerticalJustification::Top)?;
            }
        } else {
            b.text_with(title_rect, &name, "Month", 0.0, VerticalJustification::Bottom)?;
            if l.show_year {
                b.text_with(title_rect, &y.to_string(), "Year", 0.0, VerticalJustification::Bottom)?;
            }
        }
        top += g.header_h + g.header_gap;
        if l.kind == Kind::Minimal {
            b.line(area.x0, area.x1, top - g.header_gap * 0.5, 1.0_f64.min(k), 1.0)?;
        }
    }

    let bottom = notes_area(b, area, g.notes_h, k)?;

    let gx0 = area.x0 + g.wk_w;
    let (cell_w, gap) = (g.cell_w, g.gap);
    let side = (4.0 * k).min(cell_w * 0.05);
    for col in 0..7 {
        let n = o.day_name(col);
        let label: String = match g.weekday_format {
            WeekdayFormat::Short => n.chars().take(3).collect(),
            WeekdayFormat::Initial => n.chars().take(1).collect(),
            _ => n.to_string(),
        };
        let r = Rect::new(gx0 + col as f64 * cell_w + gap, top, gx0 + (col + 1) as f64 * cell_w - gap, top + g.wd_h);
        let id = b.text_with(r, &label, "Weekday", 0.0, VerticalJustification::Center)?;
        if let Some(tf) = b.d.item_mut(id).and_then(Item::text_frame_mut) {
            tf.options.inset = [0.0, side, 0.0, side];
        }
        if l.strip {
            b.style_fill(id, Fill::swatch(&b.sw.accent), None);
        }
    }
    top += g.wd_h + g.wd_gap;

    let cell_h = ((bottom - top) / rows as f64).max(g.cell_h.min(1.0));
    let start = add_days(first, -offset).ok_or_else(no_date)?;
    let stroke = (g.grid > 0.0).then(|| Stroke {
        weight: g.grid,
        swatch: b.sw.grid.clone(),
        tint: if l.grid_color.is_some() || l.kind == Kind::Bold { 1.0 } else { l.grid_tint },
        ..Stroke::default()
    });
    for row in 0..rows {
        let ry = top + row as f64 * cell_h;
        if l.rules {
            b.line(area.x0, area.x1, ry, 0.5, 0.3)?;
        }
        if o.week_numbers {
            let r = Rect::new(area.x0, ry, gx0, ry + cell_h);
            // The ISO week of the row's Monday: a Sunday-first row's Sunday still belongs to the week before.
            let week = add_days(start, row * 7 + 1 - o.week_start).ok_or_else(no_date)?;
            b.text_with(r, &iso_week(week).to_string(), "Week Number", 1.0_f64.min(cell_h * 0.1), VerticalJustification::Top)?;
        }
        for col in 0..7 {
            let day = add_days(start, row * 7 + col).ok_or_else(no_date)?;
            let (dm, dd) = (i64::from(day.month()), day.day());
            let in_month = dm == m;
            let wd = weekday(day);
            let weekend = wd == 0 || wd == 6;
            let r = Rect::new(gx0 + col as f64 * cell_w + gap, ry + gap, gx0 + (col + 1) as f64 * cell_w - gap, ry + cell_h - gap);
            let mut text = if in_month || o.adjacent { dd.to_string() } else { String::new() };
            let events = match o.events.get(&day) {
                Some(ev) if in_month => fitting_events(ev, &g, r.width() - 2.0 * g.pad - g.grid, r.height() - 2.0 * g.pad - g.grid),
                _ => vec![],
            };
            for e in &events {
                text.push('\n');
                text.push_str(e);
            }
            let style = if !in_month {
                "Other Month"
            } else if weekend {
                "Weekend"
            } else {
                "Day"
            };
            let id = b.text_with(r, &text, style, g.pad, l.number_vj)?;
            if !events.is_empty() {
                b.para_style(id, 1, "Event");
            }
            let fill = match l.kind {
                Kind::Pastel if in_month => {
                    let i = usize::try_from((row + col) % 4).unwrap_or(0);
                    b.sw.pastels.get(i).map_or_else(Fill::none, |s| Fill::swatch(s))
                }
                Kind::Pastel => tint(designcraft_color::swatch::BLACK, 0.04),
                _ if weekend && in_month && l.weekend_tint > 0.0 => tint(&b.sw.accent, l.weekend_tint),
                _ if in_month => b.sw.background.as_deref().map_or_else(Fill::none, Fill::swatch),
                _ => Fill::none(),
            };
            b.style_fill(id, fill, stroke.clone());
        }
    }
    if l.rules {
        b.line(area.x0, area.x1, top + rows as f64 * cell_h, 0.5, 0.3)?;
    }
    Ok(())
}

/// Up to twelve months at a glance in `area`, from the `first`th month of the run.
fn year_block(b: &mut Builder, l: &Look, o: &Opts, area: Rect, page: Option<Rect>, first: i64) -> Result<()> {
    let k = scale(area, l);
    let g = year_geom(area, l, o, k);
    if l.show_title {
        let (y0, _) = o.month(first);
        let (y1, _) = o.month((first + 11).min(o.months - 1));
        if l.band {
            let r = match page {
                Some(pg) => Rect::new(pg.x0 - 9.0, pg.y0 - 9.0, pg.x1 + 9.0, area.y0 + g.title_h + 8.0 * k),
                None => Rect::new(area.x0, area.y0, area.x1, area.y0 + g.title_h + 8.0 * k),
            };
            b.rect(r, Fill::swatch(&b.sw.accent), None)?;
        }
        let pad = if l.band && page.is_none() { 10.0 * k } else { 0.0 };
        let label = if y0 == y1 { y0.to_string() } else { format!("{y0}–{y1}") };
        b.text_with(Rect::new(area.x0 + pad, area.y0, area.x1 - pad, area.y0 + g.title_h), &label, "Month", 0.0, VerticalJustification::Center)?;
    }
    notes_area(b, area, g.notes_h, k)?;
    let top = area.y0 + g.top;
    let (y_first, _) = o.month(first);
    let (y_last, _) = o.month((first + 11).min(o.months - 1));
    for i in 0..12 {
        let n = first + i;
        if n >= o.months {
            break;
        }
        let (y, m) = o.month(n);
        let (c, r) = (i % g.cols, i / g.cols);
        let x = area.x0 + c as f64 * (g.mw + g.gutter);
        let my = top + r as f64 * (g.mh + g.gutter);
        let outer = Rect::new(x - 6.0 * k, my - 6.0 * k, x + g.mw + 6.0 * k, my + g.mh + 6.0 * k);
        if l.kind == Kind::Pastel {
            let fill = b.sw.pastels.get(usize::try_from(i % 4).unwrap_or(0)).map_or_else(Fill::none, |s| Fill::swatch(s));
            b.rect(outer, fill, None)?;
        } else if l.grid > 0.0 && l.kind != Kind::Classic {
            let s = Stroke { weight: l.grid.min(1.0), swatch: b.sw.grid.clone(), tint: 0.6, ..Stroke::default() };
            b.rect(outer, b.sw.background.as_deref().map_or_else(Fill::none, Fill::swatch), Some(s))?;
        } else if let Some(bg) = b.sw.background.clone() {
            b.rect(outer, Fill::swatch(&bg), None)?;
        }
        let name = if y_first == y_last { o.month_name(m).to_string() } else { format!("{} {y}", o.month_name(m)) };
        b.text(Rect::new(x, my, x + g.mw, my + 16.0 * k), &name, "Mini Month")?;
        let mut text: String = (0..7).map(|c| format!("\t{}", o.day_name(c).chars().next().unwrap_or(' '))).collect();
        let Some(first_day) = date(y, m, 1) else { continue };
        let offset = (weekday(first_day) - o.week_start).rem_euclid(7);
        let dim = days_in_month(y, m);
        for row in 0..6 {
            text.push('\n');
            for col in 0..7 {
                let d = row * 7 + col - offset + 1;
                text.push('\t');
                if (1..=dim).contains(&d) {
                    text.push_str(&d.to_string());
                }
            }
        }
        let id = b.text(Rect::new(x, my + 18.0 * k, x + g.mw, my + g.mh), &text, "Mini Weekday")?;
        b.para_style(id, 1, "Mini Day");
    }
    Ok(())
}

/// Block `i` of the run (a month, or twelve) in `area` on spread `sr`.
fn block(b: &mut Builder, l: &Look, o: &Opts, area: Rect, page: Option<Rect>, i: i64) -> Result<()> {
    if o.year_layout {
        year_block(b, l, o, area, page, i * 12)
    } else {
        let (y, m) = o.month(i);
        month_block(b, l, o, area, page, y, m)
    }
}

// ---------- File › New › Calendar ----------

/// The document file.newCalendar makes, and its page count. `preview_size` (points) makes a
/// one-page preview of a block that size instead.
fn calendar_document(id: &str, p: &Value, preview: Option<Option<(f64, f64)>>) -> Result<(Document, usize)> {
    let l = look(id, p)?;
    let mut o = options(id, p, 12)?;
    if preview.is_some() {
        o.months = o.months.min(if o.year_layout { 12 } else { 1 });
    }
    let photo = o.photo;
    let (width, height, margin) = match preview.flatten() {
        Some((w, h)) => {
            let pad = (w.min(h) * 0.04).clamp(2.0, 18.0);
            (w + 2.0 * pad, h + 2.0 * pad, pad)
        }
        None => {
            let (w, h) = match choice(id, p, "size", &["letter", "tabloid", "a5", "a4", "a3"])?.unwrap_or("letter") {
                "tabloid" => (792.0, 1224.0),
                "a5" => (419.528, 595.276),
                "a4" => (595.276, 841.89),
                "a3" => (841.89, 1190.551),
                _ => (612.0, 792.0),
            };
            let landscape = match choice(id, p, "orientation", &["portrait", "landscape"])? {
                None => !o.year_layout && !photo,
                Some(o) => o == "landscape",
            };
            let (w, h) = if landscape { (h, w) } else { (w, h) };
            let (w, h) = (num(id, p, "width", 144.0..=5000.0)?.unwrap_or(w), num(id, p, "height", 144.0..=5000.0)?.unwrap_or(h));
            (w, h, (w.min(h) * 0.055).clamp(18.0, 54.0))
        }
    };
    if !(1.0..=5100.0).contains(&width) || !(1.0..=5100.0).contains(&height) {
        return Err(bad(id, "the calendar must be 1–5000 points on a side"));
    }
    let pages = usize::try_from(o.blocks()).map_err(|_| bad(id, "too many pages"))?;
    let (y0, _) = o.month(0);
    let (y1, _) = o.month(o.months - 1);
    let default_title = if y0 == y1 { format!("Calendar {y0}") } else { format!("Calendar {y0}–{y1}") };
    let title = str_param(p, "title").filter(|t| !t.trim().is_empty()).map_or(default_title, |t| t.chars().take(256).collect());
    let nd = NewDocument {
        title: title.clone(),
        width,
        height,
        pages,
        facing_pages: false,
        margins: Margins::uniform(margin),
        bleed: if l.band && preview.is_none() { [9.0; 4] } else { [0.0; 4] },
        ..Default::default()
    };
    let mut d = super::file::new_document(&nd);
    d.title = title;
    let n = Names { prefix: "Calendar".into() };
    let sw = add_swatches(&mut d, &l, &n);
    let inner = |x: f64| Rect::new(x + margin, margin, x + width - margin, height - margin);
    define_styles(&mut d, &l, &o, &n, &sw, inner(0.0));
    for page in 0..pages {
        let (si, pi) = d.page_loc(page).ok_or_else(|| bad(id, "page missing"))?;
        let x = d.spreads.get(si).and_then(|sp| sp.pages.get(pi)).map_or(0.0, |pg| pg.x);
        let page_rect = Rect::new(x, 0.0, x + width, height);
        let mut b = Builder { d: &mut d, sr: SpreadRef::Doc(si), n: &n, sw: &sw, made: vec![] };
        let i = i64::try_from(page).map_err(|_| bad(id, "too many pages"))?;
        block(&mut b, &l, &o, inner(x), Some(page_rect), i)?;
    }
    Ok((d, pages))
}

fn new_calendar(s: &mut Session, p: &Value) -> Result<Value> {
    let (d, pages) = calendar_document(NEW, p, None)?;
    let months = options(NEW, p, 12)?.months;
    s.untitled += 1;
    let i = s.add_document(DocState::new(d, None));
    Ok(json!({"index": i, "pages": pages, "months": months}))
}

// ---------- Layout › Insert Calendar ----------

/// Absolute index of the page of spread `si` under spread x `x`, or the nearest page to it. A
/// right-to-left spread lists its pages right to left, so each page's own bounds decide, not
/// the order.
fn page_at(d: &Document, si: usize, x: f64) -> usize {
    let before: usize = d.spreads.iter().take(si).map(|s| s.pages.len()).sum();
    let Some(sp) = d.spreads.get(si) else { return before };
    let distance = |pg: &designcraft_doc::Page| {
        if x < pg.x {
            pg.x - x
        } else if x >= pg.x + pg.width {
            x - (pg.x + pg.width)
        } else {
            0.0
        }
    };
    let pi = sp.pages.iter().enumerate().min_by(|a, b| distance(a.1).total_cmp(&distance(b.1))).map_or(0, |(i, _)| i);
    before + pi
}

/// The frame Insert Calendar fills and replaces when it is given no `page` or `rect`: the only
/// selected object, an empty (unassigned) frame at the top level of a document spread. Its id,
/// spread index and bounds. The dialog asks the same question, so what it shows is what happens.
pub fn placeholder_frame(d: &Document, selection: &[ItemId]) -> Option<(ItemId, usize, Rect)> {
    let [id] = selection else { return None };
    let it = d.item(*id).filter(|it| matches!(it.content, Content::Unassigned) && !it.is_group())?;
    let loc = d.find(*id)?;
    let SpreadRef::Doc(si) = loc.spread else { return None };
    (loc.path.len() == 1).then(|| (*id, si, it.bounds()))
}

fn insert_calendar(s: &mut Session, p: &Value) -> Result<Value> {
    let l = look(INSERT, p)?;
    let o = options(INSERT, p, 1)?;
    let group = bool_or(p, "group", true);
    let add_pages = bool_or(p, "addPages", true);
    let rect = rect_param(p, "rect");
    if p.get("rect").is_some_and(|v| !v.is_null()) && rect.is_none() {
        return Err(bad(INSERT, "`rect` must be [x0, y0, x1, y1]"));
    }
    if rect.is_some_and(|r| ![r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite()) || r.width() < 36.0 || r.height() < 36.0) {
        return Err(bad(INSERT, "`rect` must be finite and at least 36 points on a side"));
    }
    let page = match p.get("page").filter(|v| !v.is_null()) {
        Some(v) => {
            Some(v.as_u64().filter(|n| *n >= 1).and_then(|n| usize::try_from(n - 1).ok()).ok_or_else(|| bad(INSERT, "`page` must be 1 or more"))?)
        }
        None => None,
    };
    let st = s.doc()?;
    // With neither, the calendar takes the place of a selected empty frame.
    let placeholder = if rect.is_none() && page.is_none() { placeholder_frame(&st.doc, &st.selection.items) } else { None };
    let blocks = usize::try_from(o.blocks()).map_err(|_| bad(INSERT, "too many months"))?;
    s.edit(|d, sel| {
        let (p0, area0) = match placeholder {
            Some((id, si, r)) => {
                d.remove_item(id)?;
                (page_at(d, si, r.center().x), Some(r))
            }
            None => (page.unwrap_or(0), rect),
        };
        let count = d.page_count();
        if p0 >= count {
            return Err(bad(INSERT, format!("no page {} (the document has {count})", p0 + 1)));
        }
        let needed = p0.saturating_add(blocks);
        if needed > count {
            if !add_pages {
                return Err(bad(INSERT, format!("{blocks} pages from page {} run past the last page; allow addPages", p0 + 1)));
            }
            let parent = d.page(count - 1).and_then(|pg| pg.parent);
            d.insert_pages(Some(count - 1), needed - count, parent)?;
        }
        let page0_x = d.page(p0).map_or(0.0, |pg| pg.x);
        let first_page = d.page(p0).cloned().ok_or_else(|| bad(INSERT, "page missing"))?;
        let area_of = |pg: &designcraft_doc::Page| match area0 {
            Some(r) => Rect::new(r.x0 - page0_x + pg.x, r.y0, r.x1 - page0_x + pg.x, r.y1),
            None => pg.margin_rect(),
        };
        let n = Names::free(d);
        let sw = add_swatches(d, &l, &n);
        define_styles(d, &l, &o, &n, &sw, area_of(&first_page));
        let mut ids = Vec::new();
        let mut pages = Vec::new();
        for i in 0..blocks {
            let abs = p0 + i;
            let (si, pi) = d.page_loc(abs).ok_or_else(|| bad(INSERT, "page missing"))?;
            let pg = d.spreads.get(si).and_then(|sp| sp.pages.get(pi)).cloned().ok_or_else(|| bad(INSERT, "page missing"))?;
            let area = area_of(&pg);
            let mut b = Builder { d, sr: SpreadRef::Doc(si), n: &n, sw: &sw, made: vec![] };
            block(&mut b, &l, &o, area, None, i64::try_from(i).map_err(|_| bad(INSERT, "too many months"))?)?;
            if group {
                ids.extend(b.group()?);
            } else {
                ids.extend(b.made);
            }
            pages.push(abs + 1);
        }
        *sel = Selection::items(ids.clone());
        Ok(json!({"ids": ids.iter().map(|i| i.0).collect::<Vec<_>>(), "pages": pages, "styles": n.prefix}))
    })
}

// ---------- preview ----------

/// The first page (or block) of the calendar `p` describes, rendered at most `max_px` pixels on
/// its long side. `previewSize: [w, h]` (points) shows a block that size, as Insert Calendar
/// would place it; otherwise the page file.newCalendar would make.
pub fn preview(cache: &designcraft_compose::Cache, p: &Value, max_px: u32) -> Result<designcraft_render::Rendered> {
    const ID: &str = "calendar.preview";
    let size = p.get("previewSize").and_then(Value::as_array).and_then(|a| Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?)));
    let size = size.filter(|(w, h)| w.is_finite() && h.is_finite() && *w >= 36.0 && *h >= 36.0 && *w <= 5000.0 && *h <= 5000.0);
    let (d, _) = calendar_document(ID, p, Some(size))?;
    let page = d.page(0).ok_or_else(|| bad(ID, "no page"))?;
    let long = page.width.max(page.height).max(1.0);
    let scale = (f64::from(max_px.clamp(16, 4096)) / long).clamp(0.01, 8.0);
    designcraft_render::Renderer::new()
        .render_page(&d, cache, 0, scale, false, &designcraft_render::RenderOptions { printing_only: true, ..Default::default() })
        .ok_or_else(|| bad(ID, "the preview could not be drawn"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::Session;

    #[test]
    fn dates_round_trip_and_know_their_weekdays() {
        let d = |y, m, dd| date(y, m, dd).unwrap();
        // 2027-01-01 is a Friday, 2024 is a leap year, 1900 isn't.
        assert_eq!(weekday(d(2027, 1, 1)), 5);
        assert_eq!((days_in_month(2024, 2), days_in_month(1900, 2), days_in_month(2000, 2), days_in_month(9999, 12)), (29, 28, 29, 31));
        // ISO weeks: 2027-01-01 is in week 53 of 2026.
        assert_eq!(iso_week(d(2027, 1, 1)), 53);
        assert_eq!(iso_week(d(2027, 1, 4)), 1);
        // A grid that starts before January of year 1 still has its days.
        assert_eq!(add_days(d(1, 1, 1), -6).map(|x| x.year()), Some(0));
        assert_eq!(date(2027, 2, 29), None);
        assert_eq!(parse_date("2027-02-29"), None);
        assert_eq!(parse_date("2028-02-29"), Some(d(2028, 2, 29)));
        assert_eq!((parse_date("nope"), parse_date("0000-01-01"), parse_date("10000-01-01")), (None, None, None));
        assert_eq!((wrapped_lines("a bb ccc", 4), wrapped_lines("abcdefghij", 4), wrapped_lines("", 4)), (2, 3, 1));
    }

    fn texts(s: &Session) -> Vec<String> {
        s.doc().unwrap().doc.stories.values().map(|st| st.text.clone()).collect()
    }

    #[test]
    fn a_month_per_page_in_every_style() {
        for style in STYLES {
            let mut s = Session::new();
            let r = s
                .execute(
                    "file.newCalendar",
                    &json!({"year": 2027, "style": style, "weekNumbers": true, "notes": true, "events": [{"date": "2027-03-17", "text": "Launch"}]}),
                )
                .unwrap();
            assert_eq!(r["pages"], 12, "{style}");
            let doc = &s.doc().unwrap().doc;
            assert_eq!(doc.page_count(), 12);
            assert!(doc.swatch("Calendar Accent").is_some());
            assert!(doc.styles.para("Calendar Day").is_some());
            let all = texts(&s);
            assert!(all.iter().any(|t| t == "March"), "{style}");
            assert!(all.iter().any(|t| t == "17\nLaunch"), "{style}");
            assert!(all.iter().any(|t| t == "31"));
        }
    }

    #[test]
    fn year_at_a_glance_over_two_years() {
        let mut s = Session::new();
        let r =
            s.execute("file.newCalendar", &json!({"year": 2026, "startMonth": 7, "months": 18, "layout": "year", "weekStart": "monday"})).unwrap();
        assert_eq!(r["pages"], 2);
        let all = texts(&s);
        assert!(all.iter().any(|t| t == "2026–2027"));
        assert!(all.iter().any(|t| t == "July 2026"));
        // July 2026 starts on a Wednesday: two blank columns before the 1 on a Monday week.
        assert!(all.iter().any(|t| t.starts_with("\tM\tT\tW\tT\tF\tS\tS\n\t\t\t1\t2\t3\t4\t5\n")), "{all:?}");
    }

    #[test]
    fn look_parameters_override_the_style() {
        let mut s = Session::new();
        s.execute(
            "file.newCalendar",
            &json!({"year": 2027, "months": 1, "style": "modern", "ink": "#112233", "gridColor": "#ff0000", "background": "#fafafa",
                "titleFont": "Source Serif 4", "titleFontStyle": "Italic", "numberPosition": "bottomRight", "weekdayFormat": "initial",
                "showYear": false, "uppercase": false, "weekdayStrip": false, "fontScale": 1.5}),
        )
        .unwrap();
        let doc = &s.doc().unwrap().doc;
        let month = doc.styles.para("Calendar Month").unwrap();
        assert_eq!((month.chars.font_family.as_deref(), month.chars.font_style.as_deref()), (Some("Source Serif 4"), Some("Italic")));
        assert_eq!(month.chars.capitalization, None);
        // Scaled up from 40 pt, but capped so the month name leaves the days their room.
        let size = month.chars.size.unwrap();
        assert!(size > 45.0 && size < 50.0, "{size}");
        let day = doc.styles.para("Calendar Day").unwrap();
        assert_eq!((day.para.align, day.chars.fill.as_deref()), (Some(Align::Right), Some("Calendar Ink")));
        for name in ["Calendar Ink", "Calendar Grid", "Calendar Background"] {
            assert!(doc.swatch(name).is_some(), "{name}");
        }
        let all = texts(&s);
        assert!(!all.iter().any(|t| t == "2027"), "no year");
        assert!(all.iter().any(|t| t == "S") && all.iter().any(|t| t == "W"), "initials");
        let cells = doc.spreads[0].items.iter().filter(|i| i.stroke.swatch == "Calendar Grid").count();
        assert!(cells >= 28, "{cells}");
    }

    #[test]
    fn insert_on_any_page_in_a_rect_grouped() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3, "facingPages": false})).unwrap();
        let r = s
            .execute("layout.insertCalendar", &json!({"page": 2, "rect": [36, 400, 576, 756], "year": 2027, "startMonth": 5, "style": "bold"}))
            .unwrap();
        assert_eq!(r["pages"], json!([2]));
        let ids = r["ids"].as_array().unwrap();
        assert_eq!(ids.len(), 1);
        let d = &s.doc().unwrap().doc;
        let g = d.item(ItemId(ids[0].as_u64().unwrap())).unwrap();
        assert!(g.is_group());
        let b = g.bounds();
        assert!(b.x0 >= 35.0 && b.x1 <= 577.0 && b.y0 >= 399.0 && b.y1 <= 757.0, "{b:?}");
        assert_eq!(d.spreads[1].items.len(), 1, "on page 2");
        assert!(d.spreads[0].items.is_empty() && d.spreads[2].items.is_empty());
        assert!(texts(&s).iter().any(|t| t == "May"));
        // Undo takes the whole calendar away in one step.
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc().unwrap().doc.spreads[1].items.is_empty());
    }

    #[test]
    fn insert_fills_the_selected_empty_frame_and_names_its_styles_apart() {
        let mut s = Session::new();
        s.execute("file.newCalendar", &json!({"year": 2027, "months": 1})).unwrap();
        let f = s.execute("frame.create", &json!({"rect": [100, 100, 400, 400], "content": "unassigned"})).unwrap()["id"].as_u64().unwrap();
        s.execute("selection.set", &json!({"ids": [f]})).unwrap();
        let r = s.execute("layout.insertCalendar", &json!({"year": 2027, "startMonth": 2, "style": "pastel", "group": false})).unwrap();
        assert_eq!(r["styles"], "Calendar 2");
        let d = &s.doc().unwrap().doc;
        assert!(d.item(ItemId(f)).is_none(), "the placeholder is replaced");
        assert!(d.styles.para("Calendar 2 Day").is_some() && d.swatch("Calendar 2 Rose").is_some());
        assert!(r["ids"].as_array().unwrap().len() > 30, "ungrouped");
        for id in r["ids"].as_array().unwrap() {
            let b = d.item(ItemId(id.as_u64().unwrap())).unwrap().bounds();
            assert!(b.x0 >= 99.0 && b.x1 <= 401.0 && b.y0 >= 99.0 && b.y1 <= 401.0, "{b:?}");
        }
    }

    #[test]
    fn insert_runs_onto_following_pages_and_adds_them() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2})).unwrap();
        let r = s.execute("layout.insertCalendar", &json!({"page": 2, "months": 3, "year": 2027})).unwrap();
        assert_eq!(r["pages"], json!([2, 3, 4]));
        assert_eq!(s.doc().unwrap().doc.page_count(), 4);
        let mut s2 = Session::new();
        s2.execute("file.new", &json!({"pages": 2})).unwrap();
        assert!(s2.execute("layout.insertCalendar", &json!({"page": 2, "months": 3, "addPages": false})).is_err());
        assert!(s2.execute("layout.insertCalendar", &json!({"page": 9})).is_err());
    }

    #[test]
    fn previews_draw_pages_and_blocks() {
        let s = Session::new();
        let page = preview(&s.cache, &json!({"year": 2027, "style": "modern"}), 300).unwrap();
        assert!(page.width == 300 && (230..=233).contains(&page.height), "{}×{}", page.width, page.height);
        let block = preview(&s.cache, &json!({"style": "bold", "previewSize": [400, 200], "layout": "year"}), 200).unwrap();
        assert!(block.width == 200 && block.height < 110, "{}×{}", block.width, block.height);
        // Something other than white paper is drawn.
        assert!(page.pixels.chunks(4).any(|p| p[0] < 128));
        assert!(preview(&s.cache, &json!({"style": "nope"}), 200).is_err());
    }

    #[test]
    fn hostile_params_are_errors() {
        let mut s = Session::new();
        for p in [
            json!({"months": 0}),
            json!({"months": 100000}),
            json!({"year": 9999, "months": 24}),
            json!({"startMonth": 13}),
            json!({"style": "gothic"}),
            json!({"layout": "week"}),
            json!({"size": "b5"}),
            json!({"width": 1e12}),
            json!({"accent": "not a colour"}),
            json!({"ink": "#zzz"}),
            json!({"fontScale": 0}),
            json!({"gridWeight": -1}),
            json!({"weekendTint": 2}),
            json!({"numberPosition": "middle"}),
            json!({"showYear": "yes"}),
            json!({"events": [{"date": "2027-13-01", "text": "x"}]}),
            json!({"events": "x"}),
            json!({"monthNames": ["Jan"]}),
        ] {
            assert!(s.execute("file.newCalendar", &p).is_err(), "{p}");
        }
        assert!(s.active().is_none());
        s.execute("file.new", &json!({})).unwrap();
        for p in [json!({"rect": [0, 0, 10, 10]}), json!({"rect": "x"}), json!({"page": 0})] {
            assert!(s.execute("layout.insertCalendar", &p).is_err(), "{p}");
        }
    }

    /// Text that doesn't fit its frame isn't drawn: the day numbers went missing when a photo,
    /// notes and week numbers, a small rectangle, or big type and padding left the cells too
    /// short. Every frame must now fit, whatever the size and options.
    #[test]
    fn every_number_fits_its_cell_at_any_size() {
        for style in STYLES {
            for rect in [
                [36, 36, 576, 756],
                [36, 36, 756, 576],
                [36, 36, 300, 200],
                [36, 36, 200, 540],
                [36, 36, 576, 150],
                [36, 36, 150, 150],
                [36, 36, 90, 90],
            ] {
                for extra in [
                    json!({}),
                    json!({"notes": true, "photo": true, "weekNumbers": true}),
                    json!({"cellPadding": 36, "cellGap": 36, "fontScale": 4}),
                    json!({"weekdayFormat": "full", "fontScale": 2.5}),
                    json!({"layout": "year"}),
                ] {
                    let mut s = Session::new();
                    s.execute("file.new", &json!({"width": 800, "height": 800})).unwrap();
                    let mut p = json!({"year": 2027, "startMonth": 5, "style": style, "rect": rect, "page": 1,
                        "events": [{"date": "2027-05-12", "text": "Something rather long happening on this day"}, {"date": "2027-05-12", "text": "And more"}]});
                    for (k, v) in extra.as_object().unwrap() {
                        p[k] = v.clone();
                    }
                    s.execute("layout.insertCalendar", &p).unwrap();
                    let issues = s.execute("preflight.run", &json!({})).unwrap();
                    let overset = issues["issues"].as_array().unwrap().iter().filter(|i| i["kind"] == "overset").count();
                    assert_eq!(overset, 0, "{style} {rect:?} {extra}");
                    // May 2027 has six weeks: its last day is there.
                    if extra.get("layout").is_none() {
                        assert!(texts(&s).iter().any(|t| t == "31"), "{style} {rect:?} {extra}");
                    }
                }
            }
        }
        // The new-document path too, with every extra on a small page.
        let mut s = Session::new();
        s.execute("file.newCalendar", &json!({"year": 2027, "size": "a5", "photo": true, "notes": true, "weekNumbers": true, "style": "minimal"}))
            .unwrap();
        let issues = s.execute("preflight.run", &json!({})).unwrap();
        assert_eq!(issues["issues"].as_array().unwrap().iter().filter(|i| i["kind"] == "overset").count(), 0);
    }

    /// The notes area went missing with a picture frame on a portrait page: once the picture had
    /// shrunk to leave the days their room, a rounding error a hair short of it dropped the
    /// notes too. Every option asked for is there when there is room for it.
    #[test]
    fn photo_and_notes_both_stay_on_a_portrait_page() {
        for style in STYLES {
            for size in ["letter", "a4", "a5", "tabloid"] {
                let mut s = Session::new();
                s.execute(
                    "file.newCalendar",
                    &json!({"year": 2026, "months": 12, "style": style, "size": size, "photo": true, "notes": true, "weekNumbers": true}),
                )
                .unwrap();
                let doc = &s.doc().unwrap().doc;
                let notes = doc.stories.values().filter(|st| st.text == "Notes").count();
                let photos = doc.spreads.iter().flat_map(|sp| &sp.items).filter(|i| i.alt_text == "Calendar photo").count();
                assert_eq!((notes, photos), (12, 12), "{style} {size}");
            }
        }
    }

    /// A Sunday-first week is numbered by its Monday: the row with Monday 4 January 2027 is ISO
    /// week 1, not the 53 its Sunday (3 January) belongs to. A Monday-first row is unchanged.
    #[test]
    fn week_numbers_follow_each_rows_monday() {
        let numbers = |week_start: &str| {
            let mut s = Session::new();
            s.execute("file.newCalendar", &json!({"year": 2027, "months": 1, "weekNumbers": true, "weekStart": week_start})).unwrap();
            let d = &s.doc().unwrap().doc;
            let mut rows: Vec<(f64, String)> = d.spreads[0]
                .items
                .iter()
                .filter_map(|i| {
                    let st = d.story(i.text_frame()?.story)?;
                    (st.paras.first()?.style == "Calendar Week Number").then(|| (i.bounds().y0, st.text.clone()))
                })
                .collect();
            rows.sort_by(|a, b| a.0.total_cmp(&b.0));
            rows.into_iter().map(|(_, t)| t).collect::<Vec<_>>()
        };
        // Sunday first: Dec 27–Jan 2 (week 53 of 2026), Jan 3–9 (week 1), … Jan 31–Feb 6 (week 5).
        assert_eq!(numbers("sunday"), ["53", "1", "2", "3", "4", "5"]);
        // Monday first: Dec 28–Jan 3 (53), Jan 4–10 (1), … Jan 25–31 (4).
        assert_eq!(numbers("monday"), ["53", "1", "2", "3", "4"]);
    }

    /// Notes squeezed below a ruled line's height gave their room to the picture even when no
    /// picture was asked for, making a picture frame out of nothing and dropping the notes.
    /// Without a picture, the notes stay.
    #[test]
    fn squeezed_notes_never_turn_into_a_picture_frame() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 800, "height": 800})).unwrap();
        s.execute("layout.insertCalendar", &json!({"year": 2027, "rect": [36, 36, 576, 90], "fontScale": 4, "notes": true, "group": false})).unwrap();
        let doc = &s.doc().unwrap().doc;
        assert!(!doc.spreads[0].items.iter().any(|i| i.alt_text == "Calendar photo"), "no picture was asked for");
        assert_eq!(doc.stories.values().filter(|st| st.text == "Notes").count(), 1, "the notes stay");
        let issues = s.execute("preflight.run", &json!({})).unwrap();
        assert_eq!(issues["issues"].as_array().unwrap().iter().filter(|i| i["kind"] == "overset").count(), 0);
    }

    /// A right-to-left spread lists its pages right to left: a point on its left page used to
    /// find the right one, the first page whose right edge lay beyond it.
    #[test]
    fn page_at_follows_each_pages_bounds_in_any_order() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3, "facingPages": true})).unwrap();
        let mut d = (*s.doc().unwrap().doc).clone();
        let sp = d.spread_mut(SpreadRef::Doc(1)).unwrap();
        sp.pages[0].side = designcraft_doc::PageSide::Right;
        sp.pages[1].side = designcraft_doc::PageSide::Left;
        sp.relayout();
        let (right, left) = (sp.pages[0].clone(), sp.pages[1].clone());
        assert!(left.x < right.x, "laid out right to left");
        // Spread 1 holds absolute pages 1 and 2: pages[0] is 1, pages[1] is 2.
        assert_eq!(page_at(&d, 1, left.x + 10.0), 2);
        assert_eq!(page_at(&d, 1, right.x + 10.0), 1);
        // Off the spread: the nearest page.
        assert_eq!(page_at(&d, 1, left.x - 500.0), 2);
        assert_eq!(page_at(&d, 1, right.x + right.width + 500.0), 1);
        assert_eq!(page_at(&d, 0, -50.0), 0);
    }

    /// The notes still went missing: Year at a Glance never drew them, and beside a picture at
    /// a larger type size they shrank in proportion with the picture until too small, though the
    /// picture had room to give. Every page of every layout, size and type size has its notes,
    /// with at least one ruled line, and nothing overset.
    #[test]
    fn notes_are_on_every_page_whatever_the_options() {
        for style in STYLES {
            for size in ["letter", "a5", "tabloid"] {
                for orientation in ["portrait", "landscape"] {
                    for layout in ["month", "year"] {
                        for (photo, scale) in [(false, 1.0), (true, 1.0), (true, 2.0), (false, 2.0), (true, 0.5)] {
                            let mut s = Session::new();
                            let p = json!({"year": 2026, "months": 12, "style": style, "size": size, "orientation": orientation,
                                "layout": layout, "photo": photo, "notes": true, "weekNumbers": true, "fontScale": scale});
                            let pages = s.execute("file.newCalendar", &p).unwrap()["pages"].as_u64().unwrap() as usize;
                            let d = &s.doc().unwrap().doc;
                            let notes = d.stories.values().filter(|st| st.text == "Notes").count();
                            let ruled =
                                d.spreads.iter().flat_map(|sp| &sp.items).filter(|i| i.shape == Shape::GraphicLine && i.stroke.tint < 0.4).count();
                            assert_eq!(notes, pages, "{p}");
                            assert!(ruled >= pages, "{p}: {ruled} ruled lines");
                            if layout == "month" && photo {
                                let photos = d.spreads.iter().flat_map(|sp| &sp.items).filter(|i| i.alt_text == "Calendar photo").count();
                                assert_eq!(photos, pages, "{p}");
                            }
                            let issues = s.execute("preflight.run", &json!({})).unwrap();
                            assert_eq!(issues["issues"].as_array().unwrap().iter().filter(|i| i["kind"] == "overset").count(), 0, "{p}");
                        }
                    }
                }
            }
        }
    }
}
