//! The PresentationML package: one blank master and layout, a theme, the slides and their media.

use std::fmt::Write as _;
use std::io::Write as _;

use crate::fonts::EmbeddedFont;
use crate::xml::{emu_len, esc};
use crate::{PptxError, Result};

pub const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
const DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
const REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
pub const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";

/// A relationship from a slide to another part.
#[derive(Clone, Debug)]
pub struct Rel {
    pub id: String,
    pub kind: &'static str,
    pub target: String,
}

/// One slide: its shape tree content (everything inside `<p:spTree>` after the group
/// properties) and relationships beyond the layout (`rId1`).
#[derive(Clone, Debug, Default)]
pub struct Slide {
    /// Page size in points.
    pub size: (f64, f64),
    pub shapes: String,
    pub rels: Vec<Rel>,
    pub name: String,
}

/// A media part (`ppt/media/<name>`).
#[derive(Clone, Debug)]
pub struct Media {
    pub name: String,
    pub bytes: Vec<u8>,
}

pub struct Package<'a> {
    pub size: (f64, f64),
    pub slides: &'a [Slide],
    pub media: &'a [Media],
    pub fonts: &'a [EmbeddedFont],
    pub title: &'a str,
    pub author: &'a str,
    pub created: Option<i64>,
}

fn rels(list: &[(String, &str, String)]) -> String {
    let mut s = format!("{DECL}<Relationships xmlns=\"{REL_NS}\">");
    for (id, kind, target) in list {
        let _ = write!(s, "<Relationship Id=\"{id}\" Type=\"{kind}\" Target=\"{}\"/>", esc(target));
    }
    s.push_str("</Relationships>");
    s
}

/// PowerPoint accepts slide sizes from 1 inch to 56 inches.
fn slide_extent(pt: f64) -> i64 {
    emu_len(pt).clamp(914_400, 51_206_400)
}

pub fn write(p: &Package) -> Result<Vec<u8>> {
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    let mut put = |name: &str, s: String| parts.push((name.to_string(), s.into_bytes()));
    let n = p.slides.len();

    // Content types.
    let mut ct = format!(
        "{DECL}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Default Extension=\"png\" ContentType=\"image/png\"/>\
<Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>\
<Default Extension=\"gif\" ContentType=\"image/gif\"/>\
<Default Extension=\"svg\" ContentType=\"image/svg+xml\"/>\
<Default Extension=\"fntdata\" ContentType=\"application/x-fontdata\"/>\
<Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/>\
<Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/>\
<Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/>\
<Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>\
<Override PartName=\"/ppt/presProps.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presProps+xml\"/>\
<Override PartName=\"/ppt/viewProps.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.viewProps+xml\"/>\
<Override PartName=\"/ppt/tableStyles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml\"/>\
<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>\
<Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/>"
    );
    for i in 1..=n {
        let _ = write!(
            ct,
            "<Override PartName=\"/ppt/slides/slide{i}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"
        );
    }
    ct.push_str("</Types>");
    put("[Content_Types].xml", ct);

    put(
        "_rels/.rels",
        rels(&[
            ("rId1".into(), "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument", "ppt/presentation.xml".into()),
            ("rId2".into(), "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties", "docProps/core.xml".into()),
            ("rId3".into(), "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties", "docProps/app.xml".into()),
        ]),
    );

    // Presentation: master rId1, slides rId2.., then the property parts.
    let (cx, cy) = (slide_extent(p.size.0), slide_extent(p.size.1));
    let mut ids = String::new();
    let mut prels: Vec<(String, &str, String)> = vec![(
        "rId1".into(),
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster",
        "slideMasters/slideMaster1.xml".into(),
    )];
    for i in 1..=n {
        let _ = write!(ids, "<p:sldId id=\"{}\" r:id=\"rId{}\"/>", 255 + i, i + 1);
        prels.push((
            format!("rId{}", i + 1),
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide",
            format!("slides/slide{i}.xml"),
        ));
    }
    for (k, (kind, target)) in
        [("presProps", "presProps.xml"), ("viewProps", "viewProps.xml"), ("theme", "theme/theme1.xml"), ("tableStyles", "tableStyles.xml")]
            .iter()
            .enumerate()
    {
        prels.push((format!("rId{}", n + 2 + k), rel_kind(kind), target.to_string()));
    }
    // Embedded fonts: one part per face, listed per family in regular, bold, italic, bold italic.
    let mut font_list = String::new();
    let mut font_parts: Vec<(String, &[u8])> = Vec::new();
    for f in p.fonts {
        let _ = write!(font_list, "<p:embeddedFont><p:font typeface=\"{}\"/>", esc(&f.family));
        for (slot, tag) in ["regular", "bold", "italic", "boldItalic"].iter().enumerate() {
            if let Some(Some(bytes)) = f.faces.get(slot) {
                let id = format!("rId{}", prels.len() + 1);
                let target = format!("fonts/font{}.fntdata", font_parts.len() + 1);
                let _ = write!(font_list, "<p:{tag} r:id=\"{id}\"/>");
                prels.push((id, "http://schemas.openxmlformats.org/officeDocument/2006/relationships/font", target.clone()));
                font_parts.push((format!("ppt/{target}"), bytes.as_slice()));
            }
        }
        font_list.push_str("</p:embeddedFont>");
    }
    let (embed_attr, font_list) = if font_parts.is_empty() {
        ("", String::new())
    } else {
        (" embedTrueTypeFonts=\"1\"", format!("<p:embeddedFontLst>{font_list}</p:embeddedFontLst>"))
    };
    put(
        "ppt/presentation.xml",
        format!(
            "{DECL}<p:presentation {NS} saveSubsetFonts=\"1\"{embed_attr}><p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst>\
<p:sldIdLst>{ids}</p:sldIdLst><p:sldSz cx=\"{cx}\" cy=\"{cy}\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/>{font_list}\
<p:defaultTextStyle><a:defPPr><a:defRPr lang=\"en-US\"/></a:defPPr></p:defaultTextStyle></p:presentation>"
        ),
    );
    put("ppt/_rels/presentation.xml.rels", rels(&prels));

    put("ppt/presProps.xml", format!("{DECL}<p:presentationPr {NS}/>"));
    put(
        "ppt/viewProps.xml",
        format!(
            "{DECL}<p:viewPr {NS}><p:normalViewPr><p:restoredLeft sz=\"15620\"/><p:restoredTop sz=\"94660\"/></p:normalViewPr>\
<p:slideViewPr><p:cSldViewPr><p:cViewPr varScale=\"1\"><p:scale><a:sx n=\"100\" d=\"100\"/><a:sy n=\"100\" d=\"100\"/></p:scale><p:origin x=\"0\" y=\"0\"/></p:cViewPr></p:cSldViewPr></p:slideViewPr>\
<p:gridSpacing cx=\"76200\" cy=\"76200\"/></p:viewPr>"
        ),
    );
    put(
        "ppt/tableStyles.xml",
        format!(
            "{DECL}<a:tblStyleLst xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" def=\"{{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}}\"/>"
        ),
    );

    // One blank master and layout.
    let tree = "<p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr></p:spTree>";
    put(
        "ppt/slideMasters/slideMaster1.xml",
        format!(
            "{DECL}<p:sldMaster {NS}><p:cSld><p:bg><p:bgRef idx=\"1001\"><a:schemeClr val=\"bg1\"/></p:bgRef></p:bg>{tree}</p:cSld>\
<p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/>\
<p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/></p:sldLayoutIdLst>\
<p:txStyles><p:titleStyle><a:lvl1pPr><a:defRPr sz=\"4400\"/></a:lvl1pPr></p:titleStyle><p:bodyStyle><a:lvl1pPr><a:defRPr sz=\"1800\"/></a:lvl1pPr></p:bodyStyle><p:otherStyle><a:lvl1pPr><a:defRPr sz=\"1800\"/></a:lvl1pPr></p:otherStyle></p:txStyles></p:sldMaster>"
        ),
    );
    put(
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        rels(&[
            (
                "rId1".into(),
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout",
                "../slideLayouts/slideLayout1.xml".into(),
            ),
            ("rId2".into(), "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme", "../theme/theme1.xml".into()),
        ]),
    );
    put(
        "ppt/slideLayouts/slideLayout1.xml",
        format!(
            "{DECL}<p:sldLayout {NS} type=\"blank\" preserve=\"1\"><p:cSld name=\"Blank\">{tree}</p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"
        ),
    );
    put(
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        rels(&[(
            "rId1".into(),
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster",
            "../slideMasters/slideMaster1.xml".into(),
        )]),
    );
    put("ppt/theme/theme1.xml", theme());

    for (i, s) in p.slides.iter().enumerate() {
        let name = if s.name.is_empty() { String::new() } else { format!(" name=\"{}\"", esc(&s.name)) };
        put(
            &format!("ppt/slides/slide{}.xml", i + 1),
            format!(
                "{DECL}<p:sld {NS}><p:cSld{name}><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>\
<p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>{}</p:spTree></p:cSld>\
<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>",
                s.shapes
            ),
        );
        let mut list: Vec<(String, &str, String)> = vec![(
            "rId1".into(),
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout",
            "../slideLayouts/slideLayout1.xml".into(),
        )];
        list.extend(s.rels.iter().map(|r| (r.id.clone(), r.kind, r.target.clone())));
        put(&format!("ppt/slides/_rels/slide{}.xml.rels", i + 1), rels(&list));
    }

    put("docProps/core.xml", core(p));
    put(
        "docProps/app.xml",
        format!(
            "{DECL}<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><Application>DesignCraft</Application><Slides>{n}</Slides><PresentationFormat>Custom</PresentationFormat></Properties>"
        ),
    );

    let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let deflate = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let zerr = |e: zip::result::ZipError| PptxError::Write(e.to_string());
    let ioerr = |e: std::io::Error| PptxError::Write(e.to_string());
    for (name, bytes) in &parts {
        zw.start_file(name.as_str(), deflate).map_err(zerr)?;
        zw.write_all(bytes).map_err(ioerr)?;
    }
    for (name, bytes) in &font_parts {
        zw.start_file(name.as_str(), deflate).map_err(zerr)?;
        zw.write_all(bytes).map_err(ioerr)?;
    }
    for m in p.media {
        // Compressed formats gain nothing from deflate.
        let opts = if m.name.ends_with(".svg") { deflate } else { stored };
        zw.start_file(format!("ppt/media/{}", m.name), opts).map_err(zerr)?;
        zw.write_all(&m.bytes).map_err(ioerr)?;
    }
    Ok(zw.finish().map_err(zerr)?.into_inner())
}

fn rel_kind(k: &str) -> &'static str {
    match k {
        "presProps" => "http://schemas.openxmlformats.org/officeDocument/2006/relationships/presProps",
        "viewProps" => "http://schemas.openxmlformats.org/officeDocument/2006/relationships/viewProps",
        "theme" => "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme",
        _ => "http://schemas.openxmlformats.org/officeDocument/2006/relationships/tableStyles",
    }
}

fn core(p: &Package) -> String {
    let mut s = format!(
        "{DECL}<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">"
    );
    if !p.title.is_empty() {
        let _ = write!(s, "<dc:title>{}</dc:title>", esc(p.title));
    }
    if !p.author.is_empty() {
        let _ = write!(s, "<dc:creator>{}</dc:creator>", esc(p.author));
    }
    if let Some(t) = p.created {
        let (y, mo, d, h, mi, se) = civil(t);
        let _ = write!(s, "<dcterms:created xsi:type=\"dcterms:W3CDTF\">{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{se:02}Z</dcterms:created>");
    }
    s.push_str("</cp:coreProperties>");
    s
}

/// Unix seconds → UTC civil date and time (proleptic Gregorian).
fn civil(t: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y.clamp(0, 9999), m, d, (secs / 3600) as u32, (secs / 60 % 60) as u32, (secs % 60) as u32)
}

/// A neutral theme: text sets its own fonts and colours, so these only matter for new objects.
fn theme() -> String {
    let fill3 = "<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>".repeat(3);
    let ln = "<a:ln w=\"6350\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln>".repeat(3);
    let fx = "<a:effectStyle><a:effectLst/></a:effectStyle>".repeat(3);
    format!(
        "{DECL}<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"DesignCraft\"><a:themeElements>\
<a:clrScheme name=\"DesignCraft\"><a:dk1><a:srgbClr val=\"000000\"/></a:dk1><a:lt1><a:srgbClr val=\"FFFFFF\"/></a:lt1><a:dk2><a:srgbClr val=\"44546A\"/></a:dk2><a:lt2><a:srgbClr val=\"E7E6E6\"/></a:lt2>\
<a:accent1><a:srgbClr val=\"4472C4\"/></a:accent1><a:accent2><a:srgbClr val=\"ED7D31\"/></a:accent2><a:accent3><a:srgbClr val=\"A5A5A5\"/></a:accent3><a:accent4><a:srgbClr val=\"FFC000\"/></a:accent4><a:accent5><a:srgbClr val=\"5B9BD5\"/></a:accent5><a:accent6><a:srgbClr val=\"70AD47\"/></a:accent6>\
<a:hlink><a:srgbClr val=\"0563C1\"/></a:hlink><a:folHlink><a:srgbClr val=\"954F72\"/></a:folHlink></a:clrScheme>\
<a:fontScheme name=\"DesignCraft\"><a:majorFont><a:latin typeface=\"Arial\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont><a:minorFont><a:latin typeface=\"Arial\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont></a:fontScheme>\
<a:fmtScheme name=\"DesignCraft\"><a:fillStyleLst>{fill3}</a:fillStyleLst><a:lnStyleLst>{ln}</a:lnStyleLst><a:effectStyleLst>{fx}</a:effectStyleLst><a:bgFillStyleLst>{fill3}</a:bgFillStyleLst></a:fmtScheme>\
</a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>"
    )
}

#[cfg(test)]
mod tests {
    use super::civil;

    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(civil(951_782_400), (2000, 2, 29, 0, 0, 0));
        assert_eq!(civil(1_791_504_000 + 3_723), (2026, 10, 9, 1, 2, 3));
    }
}
