//! IDML package writer for the document model.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::io::Write as _;
use std::rc::Rc;

use crate::attrs::{self, Attrs};
use crate::bytes::u32s;
use crate::model::{Document, Graphic, Item, Kind, Run, Story, Style, TA_AUTOLEAD, TA_COLOR, TA_FONT, TA_JUSTIFY, TA_LEADING, TA_SIZE, TA_STYLE};
use crate::text::{base64, num, xml_escape as esc};
use crate::{Budget, InddError, Result};

const NS: &str = r#"xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging""#;
const HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
const DOM: &str = r#"DOMVersion="16.0""#;
const MIMETYPE: &str = "application/vnd.adobe.indesign-idml-package";
const LEADING_AUTO: f64 = 1e7;
const MAX_DEPTH: usize = 32;
/// The IDML text may be this many times the file size (XML spells out binary numbers, embedded
/// files grow by a third in base64) …
pub(crate) const OUTPUT_FACTOR: usize = 16;
/// … and at least this large, for the fixed resources of small documents.
pub(crate) const OUTPUT_FLOOR: usize = 4 << 20;

fn justify(j: i64) -> Option<&'static str> {
    Some(match j {
        0 => "LeftAlign",
        1 => "CenterAlign",
        2 => "RightAlign",
        3 => "LeftJustified",
        4 => "CenterJustified",
        5 => "RightJustified",
        6 => "FullyJustified",
        7 => "ToBindingSide",
        8 => "AwayFromBindingSide",
        _ => return None,
    })
}

fn matrix(t: &[f64; 6]) -> String {
    t.iter().map(|v| num(*v)).collect::<Vec<_>>().join(" ")
}

/// An embedded graphic ready for the XML: element name, base64 contents, PDF crop box.
struct Encoded {
    kind: &'static str,
    base64: String,
    pdf_box: Option<[f64; 4]>,
}

struct Writer<'a> {
    doc: &'a Document,
    root_style: Option<&'a Style>,
    /// Encoded graphics by graphic UID (None: nothing embeddable).
    graphics: HashMap<u32, Option<Rc<Encoded>>>,
    /// Bytes of XML the package may still take.
    budget: Budget,
}

impl<'a> Writer<'a> {
    // --- colours -------------------------------------------------------------------------------
    fn color_ref(&self, uid: Option<u32>) -> String {
        let Some(sw) = uid.and_then(|u| self.doc.swatches.get(&u)) else { return "Swatch/None".into() };
        match sw.name.as_str() {
            "Black" | "Paper" | "Registration" => format!("Color/{}", sw.name),
            _ => format!("Color/u{}", uid.unwrap_or(0)),
        }
    }

    fn graphic_xml(&self) -> String {
        let mut out = vec![
            HEAD.to_string(),
            format!("<idPkg:Graphic {NS} {DOM}>"),
            r#"<Color Self="Color/Black" Model="Process" Space="CMYK" ColorValue="0 0 0 100" Name="Black"/>"#.into(),
            r#"<Color Self="Color/Paper" Model="Process" Space="CMYK" ColorValue="0 0 0 0" Name="Paper"/>"#.into(),
            r#"<Color Self="Color/Registration" Model="Registration" Space="CMYK" ColorValue="100 100 100 100" Name="Registration"/>"#.into(),
        ];
        for (uid, sw) in &self.doc.swatches {
            if matches!(sw.name.as_str(), "Black" | "Paper" | "Registration") || sw.values.is_empty() {
                continue;
            }
            let vals: Vec<String> = sw
                .values
                .iter()
                .map(|v| match sw.space {
                    "RGB" => num(v * 255.0),
                    "LAB" => num(*v),
                    _ => num(v * 100.0),
                })
                .collect();
            let name = if sw.name.is_empty() { "$ID/" } else { sw.name.as_str() };
            let model = if sw.name.to_uppercase().starts_with("PANTONE") { "Spot" } else { "Process" };
            out.push(format!(
                r#"<Color Self="Color/u{uid}" Model="{model}" Space="{}" ColorValue="{}" Name="{}"/>"#,
                sw.space,
                vals.join(" "),
                esc(name)
            ));
        }
        out.push(r#"<Swatch Self="Swatch/None" Name="None"/>"#.into());
        out.push("</idPkg:Graphic>".into());
        out.join("\n")
    }

    // --- text attributes -----------------------------------------------------------------------
    fn chain(&self, uid: Option<u32>) -> Vec<&'a Style> {
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        let mut cur = uid;
        while let Some(u) = cur.filter(|&u| u != 0) {
            let Some(st) = self.doc.styles.get(&u) else { break };
            if !seen.insert(u) {
                break;
            }
            out.push(st);
            cur = st.based_on;
        }
        out
    }

    fn is_char_style(&self, uid: u32) -> bool {
        self.chain(Some(uid)).last().is_some_and(|s| s.name == "[No character style]")
    }

    fn resolve(&self, pstyle: Option<u32>, pover: &Attrs, cstyle: Option<u32>, cover: &Attrs, skip_para_root: bool) -> Attrs {
        let mut merged = Attrs::new();
        let pchain = self.chain(pstyle);
        let root = match pchain.last() {
            Some(s) if s.name == "[No paragraph style]" => Some(*s),
            _ => self.root_style,
        };
        if let Some(r) = root
            && !skip_para_root
        {
            merged.extend(r.attrs.clone());
        }
        for st in pchain.iter().rev() {
            if root.is_none_or(|r| r.uid != st.uid) {
                merged.extend(st.attrs.clone());
            }
        }
        merged.extend(pover.clone());
        for st in self.chain(cstyle).iter().rev() {
            merged.extend(st.attrs.clone());
        }
        merged.extend(cover.clone());
        merged
    }

    fn char_attrs(&self, a: &Attrs, with_leading: bool) -> (String, String) {
        let mut parts = Vec::new();
        let mut props = String::new();
        if let Some(style) = attrs::string(a.get(&TA_STYLE)) {
            parts.push(format!(r#"FontStyle="{}""#, esc(&style)));
        }
        if let Some(size) = attrs::double(a.get(&TA_SIZE)).filter(|s| *s != 0.0) {
            parts.push(format!(r#"PointSize="{}""#, num(size)));
        }
        if let Some(color) = attrs::u32_value(a.get(&TA_COLOR)) {
            parts.push(format!(r#"FillColor="{}""#, self.color_ref(Some(color))));
        }
        if let Some(font) = attrs::u32_value(a.get(&TA_FONT)).and_then(|f| self.doc.fonts.get(&f)) {
            let _ = write!(props, r#"<AppliedFont type="string">{}</AppliedFont>"#, esc(font));
        }
        if with_leading && a.contains_key(&TA_LEADING) {
            match attrs::double(a.get(&TA_LEADING)) {
                Some(l) if l > 0.0 && l < LEADING_AUTO => {
                    let _ = write!(props, r#"<Leading type="unit">{}</Leading>"#, num(l));
                }
                _ => props.push_str(r#"<Leading type="enumeration">Auto</Leading>"#),
            }
        }
        (parts.join(" "), props)
    }

    fn para_attrs(&self, a: &Attrs) -> String {
        let mut parts = Vec::new();
        if let Some(auto) = attrs::double(a.get(&TA_AUTOLEAD)).filter(|v| *v != 0.0) {
            parts.push(format!(r#"AutoLeading="{}""#, num(auto * 100.0)));
        }
        if let Some(j) = attrs::int(a.get(&TA_JUSTIFY)).and_then(justify) {
            parts.push(format!(r#"Justification="{j}""#));
        }
        parts.join(" ")
    }

    fn style_ref(&self, uid: Option<u32>, char_style: bool) -> String {
        let name = uid.and_then(|u| self.doc.styles.get(&u)).map(|s| s.name.as_str()).unwrap_or("");
        if char_style {
            if name.is_empty() || name == "[No character style]" {
                return "CharacterStyle/$ID/[No character style]".into();
            }
            return format!("CharacterStyle/{name}");
        }
        if matches!(name, "" | "NormalParagraphStyle" | "[No paragraph style]") {
            return "ParagraphStyle/$ID/NormalParagraphStyle".into();
        }
        format!("ParagraphStyle/{name}")
    }

    // --- stories -------------------------------------------------------------------------------
    fn story_xml(&self, st: &Story) -> String {
        let text: Vec<char> = st.text.chars().collect();
        let n = text.len();
        let para_runs = spans(&st.paras, n);
        let char_runs = spans(&st.chars, n);
        let empty = Attrs::new();
        let mut out = vec![
            HEAD.to_string(),
            format!("<idPkg:Story {NS} {DOM}>"),
            format!(r#"<Story Self="u{}">"#, st.uid),
            r#"<StoryPreference StoryOrientation="Horizontal"/>"#.into(),
        ];
        let (mut para_at, mut char_at) = (RunCursor::default(), RunCursor::default());
        let mut start = 0usize;
        loop {
            let end = text.get(start..).and_then(|t| t.iter().position(|&c| c == '\r')).map(|p| start + p + 1).unwrap_or(n);
            let pr = para_at.at(&para_runs, start);
            let (pstyle, pover) = pr.map(|r| (Some(r.style), &r.attrs)).unwrap_or((None, &empty));
            let pa = self.resolve(pstyle, pover, None, &empty, false);
            out.push(format!(r#"<ParagraphStyleRange AppliedParagraphStyle="{}" {}>"#, esc(&self.style_ref(pstyle, false)), self.para_attrs(&pa)));
            let mut cuts: BTreeSet<usize> = [start, end].into_iter().collect();
            // Run starts are sorted: only the runs starting inside this paragraph are looked at.
            let first = char_runs.partition_point(|(s, _, _)| *s <= start);
            let last = char_runs.partition_point(|(s, _, _)| *s < end).max(first);
            cuts.extend(char_runs.get(first..last).unwrap_or(&[]).iter().map(|(s, _, _)| *s));
            let cuts: Vec<usize> = cuts.into_iter().collect();
            for w in cuts.windows(2) {
                let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else { continue };
                let cr = char_at.at(&char_runs, a);
                let (cstyle, cover) = cr.map(|r| (Some(r.style), &r.attrs)).unwrap_or((None, &empty));
                let ca = self.resolve(pstyle, pover, cstyle, cover, false);
                let (cattrs, cprops) = self.char_attrs(&ca, true);
                let piece: String = text.get(a..b).map(|s| s.iter().collect()).unwrap_or_default();
                let brk = piece.ends_with('\r');
                let piece: String =
                    piece.trim_end_matches('\r').chars().filter(|&c| c != '\u{FEFF}').map(|c| if c == '\n' { '\u{2028}' } else { c }).collect();
                let mut csr = format!(
                    r#"<CharacterStyleRange AppliedCharacterStyle="{}" {cattrs}><Properties>{cprops}</Properties>"#,
                    esc(&self.style_ref(cstyle, true))
                );
                if !piece.is_empty() {
                    let _ = write!(csr, "<Content>{}</Content>", esc(&piece));
                }
                if brk && end < n {
                    csr.push_str("<Br/>");
                }
                csr.push_str("</CharacterStyleRange>");
                out.push(csr);
            }
            out.push("</ParagraphStyleRange>".into());
            if end >= n {
                break;
            }
            start = end;
        }
        out.push("</Story>".into());
        out.push("</idPkg:Story>".into());
        out.join("\n")
    }

    fn styles_xml(&self) -> String {
        let empty = Attrs::new();
        let (mut chars, mut paras) = (Vec::new(), Vec::new());
        for st in self.doc.styles.values() {
            if self.root_style.is_some_and(|r| r.uid == st.uid)
                || matches!(st.name.as_str(), "" | "NormalParagraphStyle" | "[No paragraph style]" | "[No character style]")
            {
                continue;
            }
            if self.is_char_style(st.uid) {
                let (ca, cp) = self.char_attrs(&self.resolve(None, &empty, Some(st.uid), &empty, true), false);
                chars.push(format!(
                    r#"<CharacterStyle Self="CharacterStyle/{0}" Name="{0}" {ca}><Properties>{cp}</Properties></CharacterStyle>"#,
                    esc(&st.name)
                ));
            } else {
                let a = self.resolve(Some(st.uid), &empty, None, &empty, false);
                let (ca, cp) = self.char_attrs(&a, true);
                paras.push(format!(
                    r#"<ParagraphStyle Self="ParagraphStyle/{0}" Name="{0}" {ca} {1}><Properties>{cp}</Properties></ParagraphStyle>"#,
                    esc(&st.name),
                    self.para_attrs(&a)
                ));
            }
        }
        let root = self.resolve(None, &empty, None, &empty, false);
        let (ra, rp) = self.char_attrs(&root, true);
        let mut out = vec![
            HEAD.to_string(),
            format!("<idPkg:Styles {NS} {DOM}>"),
            r#"<RootCharacterStyleGroup Self="u_rcs">"#.into(),
            r#"<CharacterStyle Self="CharacterStyle/$ID/[No character style]" Name="$ID/[No character style]"/>"#.into(),
        ];
        out.extend(chars);
        out.push("</RootCharacterStyleGroup>".into());
        out.push(r#"<RootParagraphStyleGroup Self="u_rps">"#.into());
        out.push(format!(
            r#"<ParagraphStyle Self="ParagraphStyle/$ID/[No paragraph style]" Name="$ID/[No paragraph style]" {ra} {}><Properties>{rp}</Properties></ParagraphStyle>"#,
            self.para_attrs(&root)
        ));
        out.push(format!(
            r#"<ParagraphStyle Self="ParagraphStyle/$ID/NormalParagraphStyle" Name="$ID/NormalParagraphStyle" {ra}><Properties><BasedOn type="string">$ID/[No paragraph style]</BasedOn>{rp}</Properties></ParagraphStyle>"#
        ));
        out.extend(paras);
        out.push("</RootParagraphStyleGroup>".into());
        out.push("</idPkg:Styles>".into());
        out.join("\n")
    }

    // --- page items ----------------------------------------------------------------------------
    fn path_xml(&self, it: &Item) -> String {
        let mut sub = String::new();
        for sp in &it.paths {
            let mut pts = sp.points.clone();
            let mut open = sp.open;
            if open && pts.len() > 2 && pts.first().map(|p| p.anchor) == pts.last().map(|p| p.anchor) {
                let last_left = pts.last().map(|p| p.left);
                pts.pop();
                open = false;
                if let (Some(first), Some(l)) = (pts.first_mut(), last_left) {
                    first.left = l;
                }
            }
            let mut pp = String::new();
            for p in &pts {
                let _ = write!(
                    pp,
                    r#"<PathPointType Anchor="{} {}" LeftDirection="{} {}" RightDirection="{} {}"/>"#,
                    num(p.anchor.0),
                    num(p.anchor.1),
                    num(p.left.0),
                    num(p.left.1),
                    num(p.right.0),
                    num(p.right.1)
                );
            }
            let _ = write!(
                sub,
                r#"<GeometryPathType PathOpen="{}"><PathPointArray>{pp}</PathPointArray></GeometryPathType>"#,
                if open { "true" } else { "false" }
            );
        }
        format!("<Properties><PathGeometry>{sub}</PathGeometry></Properties>")
    }

    fn paint(&self, it: &Item) -> String {
        let mut s = format!(r#"FillColor="{}""#, self.color_ref(it.fill));
        if it.fill_tint >= 0.0 {
            let _ = write!(s, r#" FillTint="{}""#, num(it.fill_tint));
        }
        if it.stroke.is_some() && it.stroke_weight > 0.0 {
            let _ = write!(s, r#" StrokeColor="{}" StrokeWeight="{}""#, self.color_ref(it.stroke), num(it.stroke_weight));
            if it.stroke_tint >= 0.0 {
                let _ = write!(s, r#" StrokeTint="{}""#, num(it.stroke_tint));
            }
        } else {
            s.push_str(r#" StrokeColor="Swatch/None" StrokeWeight="0""#);
        }
        s
    }

    /// Spends `n` bytes of the output budget.
    fn charge(&mut self, n: usize) -> Result<()> {
        self.budget.take(n, "generated IDML")
    }

    /// The encoded form of a graphic, built once per graphic UID.
    fn encoded(&mut self, g: &Graphic) -> Result<Option<Rc<Encoded>>> {
        if let Some(e) = self.graphics.get(&g.uid) {
            return Ok(e.clone());
        }
        let enc = match graphic_payload(g) {
            Some((kind, payload)) => {
                // Check before encoding (the base64 text is a third larger than the payload); each
                // use of the text is charged where it is written.
                if payload.len().div_ceil(3).saturating_mul(4) > self.budget.left() {
                    return Err(InddError::TooLarge("generated IDML"));
                }
                let pdf_box = if kind == "PDF" { pdf_box(&payload) } else { None };
                Some(Rc::new(Encoded { kind, base64: base64(&payload), pdf_box }))
            }
            None => None,
        };
        self.graphics.insert(g.uid, enc.clone());
        Ok(enc)
    }

    /// Appends the XML of `it` and its children to `out`, within the output budget. The model is a
    /// tree whose size the builder bounds, so this visits each item once.
    fn item_xml(&mut self, it: &Item, layer: &str, depth: usize, out: &mut String) -> Result<()> {
        if depth > MAX_DEPTH {
            return Ok(());
        }
        let before = out.len();
        let xf = matrix(&it.transform);
        let vis = if it.hidden { r#" Visible="false""# } else { "" };
        match it.kind {
            Kind::Group => {
                let _ = write!(out, r#"<Group Self="u{}" ItemLayer="{layer}" ItemTransform="{xf}"{vis}>"#, it.uid);
                self.charge(out.len().saturating_sub(before))?;
                for c in &it.children {
                    self.item_xml(c, layer, depth + 1, out)?;
                }
                let tail = "</Group>";
                self.charge(tail.len())?;
                out.push_str(tail);
                return Ok(());
            }
            Kind::Text => {
                let story = it.story.map(|s| format!("u{s}")).unwrap_or_else(|| "n".into());
                let _ = write!(
                    out,
                    r#"<TextFrame Self="u{}" ItemLayer="{layer}" ItemTransform="{xf}" {}{vis} ParentStory="{story}" PreviousTextFrame="n" NextTextFrame="n" ContentType="TextType">{}<TextFramePreference TextColumnCount="1" FirstBaselineOffset="AscentOffset" VerticalJustification="TopAlign"/></TextFrame>"#,
                    it.uid,
                    self.paint(it),
                    self.path_xml(it)
                );
                self.charge(out.len().saturating_sub(before))?;
            }
            Kind::Graphic | Kind::Shape => {
                let inner = match (&it.graphic, it.kind) {
                    (Some(g), Kind::Graphic) => match self.encoded(g)? {
                        Some(enc) => graphic_xml_item(g, &enc),
                        None => String::new(),
                    },
                    _ => String::new(),
                };
                let ctype = if it.kind == Kind::Graphic { "GraphicType" } else { "Unassigned" };
                let _ = write!(
                    out,
                    r#"<Rectangle Self="u{}" ItemLayer="{layer}" ItemTransform="{xf}" {}{vis} ContentType="{ctype}">{}"#,
                    it.uid,
                    self.paint(it),
                    self.path_xml(it),
                );
                self.charge(out.len().saturating_sub(before))?;
                self.charge(inner.len())?;
                out.push_str(&inner);
                for c in &it.children {
                    self.item_xml(c, layer, depth + 1, out)?;
                }
                let tail = "</Rectangle>";
                self.charge(tail.len())?;
                out.push_str(tail);
            }
        }
        Ok(())
    }

    fn spread_xml(&mut self, sp: &crate::model::Spread) -> Result<String> {
        let mut out = vec![
            HEAD.to_string(),
            format!("<idPkg:Spread {NS} {DOM}>"),
            format!(r#"<Spread Self="u{}" PageCount="{}" ItemTransform="1 0 0 1 0 0" ShowMasterItems="true">"#, sp.uid, sp.pages.len()),
        ];
        for (i, pg) in sp.pages.iter().enumerate() {
            let [l, t, r, b] = pg.bounds;
            out.push(format!(
                r#"<Page Self="u{}" Name="{}" AppliedMaster="n" GeometricBounds="{} {} {} {}" ItemTransform="{}"/>"#,
                pg.uid,
                i + 1,
                num(t),
                num(l),
                num(b),
                num(r),
                matrix(&pg.transform)
            ));
        }
        let fixed: usize = out.iter().map(|s| s.len().saturating_add(1)).sum();
        self.charge(fixed)?;
        for it in &sp.items {
            let layer = it.layer.filter(|&l| l != 0).map(|l| format!("u{l}")).unwrap_or_else(|| "ul".into());
            let mut xml = String::new();
            self.item_xml(it, &layer, 0, &mut xml)?;
            out.push(xml);
        }
        out.push("</Spread>".into());
        out.push("</idPkg:Spread>".into());
        Ok(out.join("\n"))
    }
}

fn spans(runs: &[Run], n: usize) -> Vec<(usize, usize, &Run)> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    for r in runs {
        out.push((pos, pos.saturating_add(r.length).min(n), r));
        pos = pos.saturating_add(r.length);
    }
    out
}

/// Finds the run covering a position, for positions asked in increasing order: the spans from
/// [`spans`] are contiguous and sorted, so the cursor only moves forward (linear over a story).
#[derive(Default)]
struct RunCursor {
    i: usize,
}

impl RunCursor {
    fn at<'r>(&mut self, sp: &[(usize, usize, &'r Run)], pos: usize) -> Option<&'r Run> {
        while sp.get(self.i).is_some_and(|(_, e, _)| *e <= pos) {
            self.i += 1;
        }
        sp.get(self.i).filter(|(s, e, _)| *s <= pos && pos < *e).map(|(_, _, r)| *r)
    }
}

/// Embeddable graphic: PDF as is, EPS via its TIFF preview, else InDesign's proxy image.
fn graphic_payload(g: &Graphic) -> Option<(&'static str, Cow<'_, [u8]>)> {
    let payload = raw_payload(g)?;
    // Palette TIFFs (EPS previews) become PNG: the image decoder only reads RGB/grey TIFFs.
    if let ("Image", data) = &payload
        && let Some(png) = crate::tiff::palette_to_png(data)
    {
        return Some(("Image", Cow::Owned(png)));
    }
    Some(payload)
}

fn raw_payload(g: &Graphic) -> Option<(&'static str, Cow<'_, [u8]>)> {
    let raster = |d: &[u8]| matches!(d.get(..2), Some(b"II" | b"MM" | b"\xff\xd8" | b"\x89P"));
    if let Some(data) = g.data.as_deref() {
        if data.get(..4) == Some(b"%PDF") {
            return Some(("PDF", Cow::Borrowed(data)));
        }
        if data.get(..4) == Some(b"\xc5\xd0\xd3\xc6") {
            if let Some(h) = u32s(data, 4, 6)
                && let (Some(&off), Some(&len)) = (h.get(4), h.get(5))
                && len > 0
                && let Some(tiff) = data.get(off as usize..(off as usize).saturating_add(len as usize))
            {
                return Some(("Image", Cow::Borrowed(tiff)));
            }
        } else if raster(data) {
            return Some(("Image", Cow::Borrowed(data)));
        }
    }
    g.proxy.as_deref().filter(|p| raster(p)).map(|p| ("Image", Cow::Borrowed(p)))
}

fn graphic_xml_item(g: &Graphic, enc: &Encoded) -> String {
    let Some([mut l, mut t, mut r, mut b]) = g.bounds else { return String::new() };
    let kind = enc.kind;
    if let Some([mx0, my0, mx1, my1]) = enc.pdf_box {
        // PDF inner space keeps PDF y values but runs downward: y_inner = t + b - y_pdf.
        let (top, bottom) = (t, b);
        (l, t, r, b) = (mx0, top + bottom - my1, mx1, top + bottom - my0);
    }
    let link = g
        .uri
        .as_deref()
        .map(|u| format!(r#"<Link Self="u{}_link" LinkResourceURI="{}" StoredState="Embedded"/>"#, g.uid, esc(u)))
        .unwrap_or_default();
    format!(
        r#"<{kind} Self="u{}" ItemTransform="{}"><Properties><Contents><![CDATA[{}]]></Contents><GraphicBounds Left="{}" Top="{}" Right="{}" Bottom="{}"/></Properties>{link}</{kind}>"#,
        g.uid,
        matrix(&g.transform),
        enc.base64,
        num(l),
        num(t),
        num(r),
        num(b)
    )
}

/// First literal `/CropBox [a b c d]` (else `/MediaBox`) of a PDF.
fn pdf_box(data: &[u8]) -> Option<[f64; 4]> {
    for key in [&b"/CropBox"[..], &b"/MediaBox"[..]] {
        let mut from = 0usize;
        while let Some(i) = data.get(from..).and_then(|d| d.windows(key.len()).position(|w| w == key)) {
            let after = from + i + key.len();
            from = after;
            if let Some(b) = literal_box(data.get(after..).unwrap_or(&[])) {
                return Some(b);
            }
        }
    }
    None
}

fn literal_box(rest: &[u8]) -> Option<[f64; 4]> {
    // A box is a few numbers; looking further would make a PDF full of keys quadratic.
    let rest = crate::bytes::clamp(rest, 0, 256);
    let open = rest.iter().position(|c| !c.is_ascii_whitespace())?;
    if rest.get(open) != Some(&b'[') {
        return None;
    }
    let close = open + rest.get(open..)?.iter().position(|&c| c == b']')?;
    let inner = std::str::from_utf8(rest.get(open + 1..close)?).ok()?;
    let nums: Vec<f64> = inner.split_ascii_whitespace().map(|s| s.parse().ok()).collect::<Option<Vec<f64>>>()?;
    if let [a, b, c, d] = nums[..] { Some([a, b, c, d]) } else { None }
}

fn walk<'i>(items: &'i [Item], out: &mut Vec<&'i Item>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    for it in items {
        out.push(it);
        walk(&it.children, out, depth + 1);
    }
}

/// Default output budget of [`write`] (the import path derives one from the file size).
const MAX_OUTPUT: usize = 1 << 30;

/// The IDML package for a document model.
pub fn write(d: &Document) -> Result<Vec<u8>> {
    write_limited(d, Budget::for_input(MAX_OUTPUT, 1, 0))
}

pub(crate) fn write_limited(d: &Document, budget: Budget) -> Result<Vec<u8>> {
    let root_style = d.styles.values().find(|s| s.name == "[No paragraph style]" && s.attrs.len() > 100);
    let mut w = Writer { doc: d, root_style, graphics: HashMap::new(), budget };
    let mut files: Vec<(String, String)> = Vec::new();
    let (graphic, styles) = (w.graphic_xml(), w.styles_xml());
    w.charge(graphic.len().saturating_add(styles.len()))?;
    files.push(("Resources/Graphic.xml".into(), graphic));
    files.push(("Resources/Styles.xml".into(), styles));
    let pages: usize = d.spreads.iter().map(|s| s.pages.len()).sum();
    files.push((
        "Resources/Preferences.xml".into(),
        [
            HEAD.to_string(),
            format!("<idPkg:Preferences {NS} {DOM}>"),
            format!(
                r#"<DocumentPreference PageWidth="{}" PageHeight="{}" FacingPages="false" PagesPerDocument="{pages}"/>"#,
                num(d.page_size.0),
                num(d.page_size.1)
            ),
            r#"<MarginPreference ColumnCount="1" ColumnGutter="12" Top="0" Bottom="0" Left="0" Right="0"/>"#.into(),
            "</idPkg:Preferences>".into(),
        ]
        .join("\n"),
    ));
    let mut all = Vec::new();
    for sp in &d.spreads {
        walk(&sp.items, &mut all, 0);
    }
    let used_layers: BTreeSet<u32> = d.spreads.iter().flat_map(|s| s.items.iter().filter_map(|i| i.layer)).collect();
    let used_stories: BTreeSet<u32> = all.iter().filter_map(|i| i.story).collect();
    let layers: Vec<String> = d
        .layers
        .iter()
        .rev()
        .filter(|(u, _)| used_layers.contains(u))
        .map(|(u, n)| format!(r#"<Layer Self="u{u}" Name="{}" Visible="true" Locked="false" Printable="true"/>"#, esc(n)))
        .collect();
    let mut refs = vec![
        r#"<idPkg:Graphic src="Resources/Graphic.xml"/>"#.to_string(),
        r#"<idPkg:Styles src="Resources/Styles.xml"/>"#.into(),
        r#"<idPkg:Preferences src="Resources/Preferences.xml"/>"#.into(),
        layers.join("\n"),
    ];
    for sp in &d.spreads {
        let name = format!("Spreads/Spread_u{}.xml", sp.uid);
        let xml = w.spread_xml(sp)?;
        files.push((name.clone(), xml));
        refs.push(format!(r#"<idPkg:Spread src="{name}"/>"#));
    }
    let stories: BTreeMap<u32, &Story> = d.stories.iter().filter(|(u, _)| used_stories.contains(u)).map(|(u, s)| (*u, s)).collect();
    for (uid, st) in &stories {
        let name = format!("Stories/Story_u{uid}.xml");
        let xml = w.story_xml(st);
        w.charge(xml.len())?;
        files.push((name.clone(), xml));
        refs.push(format!(r#"<idPkg:Story src="{name}"/>"#));
    }
    let story_list = stories.keys().map(|u| format!("u{u}")).collect::<Vec<_>>().join(" ");
    files.push((
        "designmap.xml".into(),
        [
            HEAD.to_string(),
            r#"<?aid style="50" type="document" readerVersion="6.0" featureSet="257" product="16.0(1)" ?>"#.into(),
            format!(r#"<Document {NS} {DOM} Self="d" StoryList="{story_list}" Name="{}">"#, esc(&d.name)),
            refs.join("\n"),
            "</Document>".into(),
        ]
        .join("\n"),
    ));
    zip_package(&files)
}

fn zip_package(files: &[(String, String)]) -> Result<Vec<u8>> {
    use zip::write::SimpleFileOptions;
    let err = |e: &dyn std::fmt::Display| InddError::Write(e.to_string());
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    z.start_file("mimetype", stored).map_err(|e| err(&e))?;
    z.write_all(MIMETYPE.as_bytes()).map_err(|e| err(&e))?;
    for (name, content) in files {
        z.start_file(name.as_str(), deflated).map_err(|e| err(&e))?;
        z.write_all(content.as_bytes()).map_err(|e| err(&e))?;
    }
    let cursor = z.finish().map_err(|e| err(&e))?;
    Ok(cursor.into_inner())
}
