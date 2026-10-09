//! Artistic and frame text: the story's characters with their font, size, colour, tracking and
//! leading, the paragraph alignment and the first baseline. Layout (line breaks in frames, kerning,
//! OpenType features) is left to the importing application.

use crate::model::{Affine, Align, Point, Reader, Text, TextRun, rect};
use crate::paint::{self, Color, Paint};
use crate::stream::{ObjId, Value};

/// Characters read per text node (stories are split into blocks; this caps hostile ones).
const MAX_CHARS: usize = 1 << 20;
/// Shared character attributes must not amplify into unbounded font/feature/gradient data.
const MAX_ATTRIBUTE_BYTES: usize = 16 << 20;

pub(crate) fn read(r: &mut Reader, id: ObjId, world: Affine) -> Option<Text> {
    let s = r.s;
    let story = s.obj(id, b"StSt")?;
    let frame = s.obj(id, b"TxtH")?;
    let artistic = s.is(frame, b"ArFr");
    let bounds = s.floats::<4>(frame, b"FrmB").map(rect)?;
    let mut runs = Vec::new();
    let mut align = None;
    let mut total = 0usize;
    let mut placeholders = false;
    let mut attribute_budget = Some(MAX_ATTRIBUTE_BYTES);
    for block in s.objs(story, b"Blok") {
        let Some(glyphs) = s.obj(block, b"Glyp") else {
            r.warn("a text block without readable characters (left out)");
            continue;
        };
        // Bound collection itself, including placeholders, before allocating the character buffer.
        let budget = MAX_CHARS - total;

        let mut chars: Vec<char> = match s.str(glyphs, b"Utf8") {
            Some(t) => t.chars().take(budget + 1).collect(),
            None => {
                // Inline objects (page numbers, anchors) each take one index before the segment text.
                let segments = s.objs(glyphs, b"Mixd");
                if segments.is_empty() {
                    r.warn("a text block without readable characters (left out)");
                    continue;
                }
                let mut v = Vec::new();
                for seg in segments {
                    let inline = s.objs(seg, b"Glys").len();
                    v.extend(std::iter::repeat_n('\u{FFFC}', inline.min(budget + 1 - v.len())));
                    placeholders |= inline > 0;
                    v.extend(s.str(seg, b"Utf8").unwrap_or_default().chars().take(budget + 1 - v.len()));
                    if v.len() > budget {
                        break;
                    }
                }
                v
            }
        };
        let truncated = chars.len() > budget;
        if truncated {
            chars.truncate(budget);
            r.warn("text longer than a million characters (truncated)");
        }
        total += chars.len();
        let mut start = 0usize;
        for run in s.obj(block, b"GAtt").map(|g| s.objs(g, b"Runs")).unwrap_or_default() {
            let Some(index) = s.int(run, b"Indx").and_then(|v| usize::try_from(v).ok()) else {
                r.warn("invalid text attribute range (bounded; unassigned characters use defaults)");
                continue;
            };
            let end = index.min(chars.len());
            if index > chars.len() || end < start {
                r.warn("invalid text attribute range (bounded; unassigned characters use defaults)");
            }
            if end <= start {
                continue;
            }
            let text = characters(&chars, start, end);
            start = end;
            push_run(r, &mut runs, s.obj(run, b"Item"), text, world, &mut attribute_budget);
        }
        if start < chars.len() {
            let text = characters(&chars, start, chars.len());
            push_run(r, &mut runs, None, text, world, &mut attribute_budget);
        }
        for paragraph in s.obj(block, b"PAtt").map(|p| s.objs(p, b"Runs")).unwrap_or_default() {
            if let Some(attrs) = s.obj(paragraph, b"Item") {
                let paragraph = paragraph_align(r, attrs);
                match align {
                    None => align = Some(paragraph),
                    Some(first) if first != paragraph => r.warn("mixed paragraph alignments (the first alignment is used)"),
                    _ => {}
                }
            }
        }
        if truncated {
            break;
        }
    }
    if placeholders {
        r.warn("text fields such as page numbers (left out of the text)");
    }
    let align = align.unwrap_or(Align::Left);
    let first_baseline = bounds.y0 + s.f64(frame, b"ArtV").unwrap_or(0.0);
    let anchor_x = match align {
        Align::Center => (bounds.x0 + bounds.x1) / 2.0,
        Align::Right => bounds.x1,
        _ => bounds.x0,
    };
    if !artistic {
        if s.objs(frame, b"ColW").len() > 1 || matches!(s.field(frame, b"ColW"), Some(Value::Array(a)) if a.len() > 1) {
            r.warn("text frames with several columns (imported as one column)");
        }
        if s.class(id) == Some(crate::stream::Tag::of(b"TxtC")) {
            r.warn("text frames with a curved outline (imported as rectangular frames)");
        }
    }
    Some(Text { runs, align, anchor: Point { x: anchor_x, y: first_baseline }, frame: if artistic { None } else { Some(bounds) }, transform: world })
}

fn characters(chars: &[char], start: usize, end: usize) -> String {
    chars.get(start..end).unwrap_or_default().iter().filter(|c| **c != '\0' && **c != '\u{FFFC}').collect()
}

fn push_run(r: &mut Reader, runs: &mut Vec<TextRun>, attrs: Option<ObjId>, text: String, world: Affine, attribute_budget: &mut Option<usize>) {
    if text.is_empty() {
        return;
    }
    let run = match attrs {
        Some(attrs) if attribute_budget.is_some() => run_attrs(r, attrs, text, world, attribute_budget),
        _ => {
            // This is a recovery policy, not an assertion about unspecified Affinity attributes.
            if attrs.is_none() {
                r.warn("text without character attributes (characters preserved with default attributes)");
            }
            TextRun {
                text,
                postscript: String::new(),
                family: String::new(),
                weight: 400,
                italic: false,
                features: Vec::new(),
                all_caps: false,
                size: 12.0 * r.dpi / 72.0,
                tracking: 0.0,
                leading: None,
                fill: Paint::Solid(Color::Gray { v: 0.0, a: 1.0 }),
            }
        }
    };
    runs.push(run);
}

fn slot<T: Copy>(v: Option<&Value>, i: usize, f: impl Fn(&Value) -> Option<T>) -> Option<T> {
    match v? {
        Value::Array(a) => a.get(i).and_then(f),
        _ => None,
    }
}

fn float(v: &Value) -> Option<f64> {
    match v {
        Value::Float(f) if f.is_finite() => Some(*f),
        _ => None,
    }
}

fn int(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        Value::UInt(u) => i64::try_from(*u).ok(),
        _ => None,
    }
}

fn run_attrs(r: &mut Reader, attrs: ObjId, text: String, world: Affine, attribute_budget: &mut Option<usize>) -> TextRun {
    let s = r.s;
    let doubles = s.field(attrs, b"Doub");
    let ints = s.field(attrs, b"Ints");
    let size = slot(doubles, 0, float).filter(|v| *v > 0.0 && *v < 1e6).unwrap_or(12.0 * r.dpi / 72.0);
    let tracking = slot(doubles, 4, float).unwrap_or(0.0);
    let leading = (slot(ints, 4, int) == Some(1)).then(|| slot(doubles, 13, float)).flatten().filter(|v| *v > 0.0);
    let objects = match s.field(attrs, b"Objs") {
        Some(Value::Array(a)) => a.iter().take(8).map(|v| if let Value::Obj(o) = v { *o } else { None }).collect(),
        _ => Vec::new(),
    };
    let mut fill = match objects.first().copied().flatten().and_then(|f| paint::descriptor(r, f, world)) {
        Some(fill) => fill,
        None => {
            r.warn("text without readable fill attributes (default black used)");
            Paint::Solid(Color::Gray { v: 0.0, a: 1.0 })
        }
    };
    // A single descriptor decode is bounded by paint::MAX_STOPS. Charge its owned stop data
    // before keeping it; subsequent runs skip attribute decoding once the budget is exhausted.
    if let Paint::Gradient(gradient) = &fill {
        let bytes = gradient.stops.len().saturating_mul(std::mem::size_of::<paint::Stop>());
        if !charge_attributes(r, attribute_budget, bytes) {
            fill = Paint::Solid(Color::Gray { v: 0.0, a: 1.0 });
        }
    }
    if attribute_budget.is_some() && objects.get(1).copied().flatten().and_then(|f| paint::descriptor(r, f, world)).is_some_and(|p| p != Paint::None)
    {
        r.warn("outlined (stroked) text");
    }
    if slot(doubles, 10, float).is_some_and(|h| (h - 1.0).abs() > 1e-3) {
        r.warn("horizontally scaled text");
    }
    // A resolved font is populated on the emoji run of the public Affinity 3 text-runs file
    // (Courier New requested, Segoe UI Emoji stored in RFnt). Empty RFnt records are common.
    // Source: https://github.com/SethRobinson/Patchy/tree/de84eab550758b30fa062e479f5778cce7693b73/test-fixtures/af
    let requested = s.obj(attrs, b"DFnt");
    let resolved =
        s.obj(attrs, b"RFnt").filter(|f| !s.str(*f, b"Famy").unwrap_or_default().is_empty() || !s.str(*f, b"Post").unwrap_or_default().is_empty());
    let font = resolved.or(requested);
    if let Some(resolved) = resolved {
        let differs = [b"Famy", b"Post"]
            .iter()
            .any(|tag| s.str(resolved, tag).unwrap_or_default() != requested.and_then(|f| s.str(f, tag)).unwrap_or_default());
        if differs {
            r.warn("text uses a stored fallback font (imported with that font)");
        }
    }
    if font.and_then(|f| s.int(f, b"Widh")).is_some_and(|w| w != 5) {
        r.warn("font width variants (exact face depends on the installed PostScript font)");
    }
    let postscript = metadata_string(r, attribute_budget, font.and_then(|f| s.str(f, b"Post")).unwrap_or_default());
    let family = metadata_string(r, attribute_budget, font.and_then(|f| s.str(f, b"Famy")).unwrap_or_default());
    let (features, all_caps) = font_features(r, objects.get(7).copied().flatten(), attribute_budget);
    let weight = font.and_then(|f| s.int(f, b"Wegt")).unwrap_or(400);
    let weight = if (1..=1000).contains(&weight) {
        weight
    } else {
        r.warn("invalid font weight (imported as regular)");
        400
    };
    TextRun {
        text,
        postscript,
        family,
        weight,
        italic: font.and_then(|f| s.bool(f, b"Ital")).unwrap_or(false),
        features,
        all_caps,
        size,
        tracking,
        leading,
        fill,
    }
}

fn charge_attributes(r: &mut Reader, budget: &mut Option<usize>, bytes: usize) -> bool {
    let Some(remaining) = *budget else { return false };
    match remaining.checked_sub(bytes) {
        Some(remaining) => {
            *budget = Some(remaining);
            true
        }
        None => {
            *budget = None;
            r.warn("text attribute budget exceeded (excess font names, features and gradients left out)");
            false
        }
    }
}

fn metadata_string(r: &mut Reader, budget: &mut Option<usize>, value: &str) -> String {
    let bytes = value.len().saturating_add(std::mem::size_of::<String>());
    if value.is_empty() || !charge_attributes(r, budget, bytes) { String::new() } else { value.to_owned() }
}

/// The public Affinity 3 caps fixture stores `smcp` and the private `CAP\x01` selector in
/// OtAt/Setn. Only the observed all-caps selector and boolean OpenType tags are mapped; other
/// values are reported, never guessed. Standard tag syntax: https://learn.microsoft.com/en-us/typography/opentype/spec/featuretags
fn font_features(r: &mut Reader, attrs: Option<ObjId>, attribute_budget: &mut Option<usize>) -> (Vec<String>, bool) {
    let mut features = Vec::new();
    let mut all_caps = false;
    if attribute_budget.is_none() {
        return (features, all_caps);
    }
    let s = r.s;
    let Some(Value::Array(settings)) = attrs.and_then(|a| s.field(a, b"Setn")) else { return (features, all_caps) };
    for (index, setting) in settings.iter().take(257).enumerate() {
        if index == 256 {
            r.warn("more than 256 text feature settings (truncated)");
            break;
        }
        let Value::Obj(Some(setting)) = setting else {
            r.warn("unreadable text feature settings (imported with defaults)");
            continue;
        };
        let setting = *setting;
        let tag = s.int(setting, b"Feat").and_then(|v| u32::try_from(v).ok()).map(u32::to_be_bytes);
        let value = r.s.int(setting, b"Valu");
        match (tag, value) {
            (Some([b'C', b'A', b'P', 1]), Some(0 | 1)) => all_caps = value == Some(1),
            (Some(tag), Some(value @ (0 | 1))) if tag.iter().all(u8::is_ascii_alphanumeric) => {
                if !charge_attributes(r, attribute_budget, std::mem::size_of::<String>() + if value == 0 { 5 } else { 4 }) {
                    break;
                }
                let name = String::from_utf8_lossy(&tag);
                features.push(if value == 0 { format!("-{name}") } else { name.into_owned() });
            }
            _ => r.warn("text feature selectors or non-boolean OpenType settings (imported with defaults)"),
        }
    }
    (features, all_caps)
}

fn paragraph_align(r: &mut Reader, p: ObjId) -> Align {
    // Nonzero offsets in these slots are present in the public text-indent and text-para-spacing
    // fixtures. Their layout is not represented by this importer; retain text and report the gap.
    if (1..=6).any(|i| slot(r.s.field(p, b"Doub"), i, float).is_some_and(|v| v.abs() > 1e-9)) {
        r.warn("paragraph indents or spacing (imported with defaults)");
    }
    match slot(r.s.field(p, b"Ints"), 0, int) {
        Some(1) => Align::Center,
        Some(2) => Align::Right,
        Some(3) => Align::Justify,
        Some(0) | None => Align::Left,
        Some(_) => {
            r.warn("an unknown paragraph alignment (imported as left)");
            Align::Left
        }
    }
}
