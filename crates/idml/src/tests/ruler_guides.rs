//! Layout › Ruler Guides: each guide's colour and view threshold (`GuideColor`, `ViewThreshold`)
//! and the document's defaults for new guides (`GuidePreference`'s `RulerGuidesColor` and
//! `RulerGuidesViewThreshold`) are read, written and read back.

use std::io::Read;
use std::sync::Arc;

use super::*;
use designcraft_doc::RulerGuideOptions;

/// One page with a vertical guide per `(ViewThreshold, GuideColor property)` pair, and a
/// `GuidePreference` from `preference` (its attributes and children, or none).
fn fixture(guides: &[(&str, &str)], preference: Option<&str>) -> Vec<u8> {
    let map = r#"<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" Self="d">
  <idPkg:Preferences src="Resources/Preferences.xml"/>
  <Layer Self="L1" Name="Layer 1" Visible="true" Locked="false"/>
  <idPkg:Spread src="Spreads/S.xml"/>
</Document>"#;
    let prefs = format!(
        r#"<Preferences><DocumentPreference PageWidth="500" PageHeight="700" FacingPages="false"/>{}</Preferences>"#,
        preference.map(|p| format!("<GuidePreference {p}</GuidePreference>")).unwrap_or_default()
    );
    let guides: String = guides
        .iter()
        .enumerate()
        .map(|(i, (threshold, color))| {
            format!(
                r#"<Guide Self="g{i}" Orientation="Vertical" Location="{}" FitToPage="true" {threshold} Locked="false" ItemLayer="L1" GuideType="Ruler">{color}</Guide>"#,
                10 * (i + 1)
            )
        })
        .collect();
    let spread = format!(
        r#"<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
<Spread Self="s" PageCount="1"><Page Self="p" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 0 -350">{guides}</Page></Spread>
</idPkg:Spread>"#
    );
    zip_files(&[("designmap.xml", map), ("Resources/Preferences.xml", &prefs), ("Spreads/S.xml", &spread)])
}

/// A `GuideColor`-style property: a UI colour name, or an RGB list.
fn named(tag: &str, name: &str) -> String {
    format!(r#"<Properties><{tag} type="enumeration">{name}</{tag}></Properties>"#)
}

/// A `GuideColor`-style property as an RGB list of `rgb`'s items.
fn listed(tag: &str, rgb: &[&str]) -> String {
    let items: String = rgb.iter().map(|v| format!(r#"<ListItem type="double">{v}</ListItem>"#)).collect();
    format!(r#"<Properties><{tag} type="list">{items}</{tag}></Properties>"#)
}

/// The options of the first page's guides, in order.
fn options(d: &Document) -> Vec<RulerGuideOptions> {
    d.spreads[0].pages[0].guides.iter().map(|g| g.options).collect()
}

/// Each guide's `ViewThreshold` and `GuideColor` are read; a threshold past 4000% is held to it,
/// and one below 5%, a value that is not a number and a colour that can't be read keep the default.
#[test]
fn guide_colours_and_view_thresholds_are_read() {
    let o = |color: Option<[u8; 3]>, view_threshold: f64| RulerGuideOptions { color, view_threshold };
    let cases: Vec<(String, String, RulerGuideOptions)> = vec![
        // InDesign's own: Cyan from 5%.
        (r#"ViewThreshold="5""#.into(), named("GuideColor", "Cyan"), RulerGuideOptions::default()),
        (r#"ViewThreshold="150""#.into(), named("GuideColor", "Red"), o(Some([255, 0, 0]), 1.5)),
        (r#"ViewThreshold="75""#.into(), named("GuideColor", "LightBlue"), o(Some([43, 155, 255]), 0.75)),
        (r#"ViewThreshold="4000""#.into(), listed("GuideColor", &["12", "34", "255"]), o(Some([12, 34, 255]), 40.0)),
        // A list that spells out Cyan is the default colour.
        (String::new(), listed("GuideColor", &["0", "255", "255"]), RulerGuideOptions::default()),
        // Below and past InDesign's 5–4000 range, and not numbers: held to it, or the default.
        (r#"ViewThreshold="0""#.into(), String::new(), RulerGuideOptions::default()),
        (r#"ViewThreshold="-20""#.into(), String::new(), RulerGuideOptions::default()),
        (r#"ViewThreshold="90000""#.into(), String::new(), o(None, 40.0)),
        (r#"ViewThreshold="NaN""#.into(), String::new(), RulerGuideOptions::default()),
        (r#"ViewThreshold="half""#.into(), String::new(), RulerGuideOptions::default()),
        // Colours it can't read keep the default.
        (String::new(), named("GuideColor", "Chartreuse"), RulerGuideOptions::default()),
        (String::new(), listed("GuideColor", &["1", "2"]), RulerGuideOptions::default()),
        (String::new(), listed("GuideColor", &["1", "2", "3", "4"]), RulerGuideOptions::default()),
        (String::new(), listed("GuideColor", &["300", "0", "0"]), RulerGuideOptions::default()),
        (String::new(), listed("GuideColor", &["-1", "0", "0"]), RulerGuideOptions::default()),
        (String::new(), listed("GuideColor", &["inf", "0", "0"]), RulerGuideOptions::default()),
    ];
    let guides: Vec<(&str, &str)> = cases.iter().map(|(t, c, _)| (t.as_str(), c.as_str())).collect();
    let d = import_idml(&fixture(&guides, None)).unwrap();
    d.check().unwrap();
    assert_eq!(options(&d), cases.iter().map(|(_, _, want)| *want).collect::<Vec<_>>());
    assert!(d.settings.ruler_guides.is_default(), "no GuidePreference: the defaults");
}

/// `GuidePreference`'s `RulerGuidesViewThreshold` and `RulerGuidesColor` become the document's
/// defaults for new guides.
#[test]
fn the_guide_preference_sets_the_defaults_for_new_guides() {
    for (pref, want) in [
        (
            format!(r#"RulerGuidesViewThreshold="200">{}"#, named("RulerGuidesColor", "Magenta")),
            RulerGuideOptions { color: Some([255, 79, 255]), view_threshold: 2.0 },
        ),
        (
            format!(r#"RulerGuidesViewThreshold="5">{}"#, listed("RulerGuidesColor", &["9", "8", "7"])),
            RulerGuideOptions { color: Some([9, 8, 7]), view_threshold: 0.05 },
        ),
        (format!(r#"GuidesShown="true">{}"#, named("RulerGuidesColor", "Cyan")), RulerGuideOptions::default()),
    ] {
        let d = import_idml(&fixture(&[], Some(&pref))).unwrap();
        assert_eq!(d.settings.ruler_guides, want, "{pref}");
    }
}

/// Export writes each guide's threshold and colour and the `GuidePreference`, and import reads the
/// same options back.
#[test]
fn guide_options_are_written_and_read_back() {
    let mut source = import_idml(&fixture(&[("", ""), ("", ""), ("", "")], None)).unwrap();
    let looks = [
        RulerGuideOptions::default(),
        RulerGuideOptions { color: Some([255, 153, 0]), view_threshold: 1.25 },
        RulerGuideOptions { color: Some([1, 2, 3]), view_threshold: 40.0 },
    ];
    for (g, o) in Arc::make_mut(&mut source.spreads[0]).pages[0].guides.iter_mut().zip(looks) {
        g.options = o;
    }
    source.settings.ruler_guides = RulerGuideOptions { color: Some([0, 84, 0]), view_threshold: 0.5 };
    let bytes = export_idml(&source);
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    let part = |archive: &mut zip::ZipArchive<_>, path: &str| {
        let mut b = Vec::new();
        archive.by_name(path).unwrap().read_to_end(&mut b).unwrap();
        xml::parse(&b).unwrap()
    };
    let spread = part(&mut archive, &format!("Spreads/Spread_u{:x}.xml", source.spreads[0].id.0));
    let written: Vec<_> = spread.find("Spread").unwrap().find("Page").unwrap().find_all("Guide").collect();
    // Each guide's threshold, and its colour: a name, or the RGB list's items.
    let attrs: Vec<(Option<&str>, String)> = written
        .iter()
        .map(|g| {
            let color = g.prop_el("GuideColor").unwrap();
            let shown = match color.get("type") {
                Some("list") => color.find_all("ListItem").map(|i| i.text_content().trim().to_string()).collect::<Vec<_>>().join(" "),
                _ => color.text_content().trim().to_string(),
            };
            (g.get("ViewThreshold"), shown)
        })
        .collect();
    assert_eq!(attrs, [(Some("5"), "Cyan".to_string()), (Some("125"), "Gold".to_string()), (Some("4000"), "1 2 3".to_string())]);
    assert_eq!(written[2].prop_el("GuideColor").and_then(|c| c.get("type")), Some("list"));
    // Properties come first in a Guide, as IDML has them.
    assert!(written.iter().all(|g| g.elements().next().is_some_and(|e| e.local() == "Properties")));
    let prefs = part(&mut archive, "Resources/Preferences.xml");
    let gp = prefs.find("GuidePreference").expect("GuidePreference is written");
    assert_eq!(gp.get("RulerGuidesViewThreshold"), Some("50"));
    assert_eq!(gp.prop_el("RulerGuidesColor").map(|c| c.text_content()), Some("DarkGreen".to_string()));
    let back = import_idml(&bytes).unwrap();
    assert_eq!(options(&back), looks);
    assert_eq!(back.settings.ruler_guides, source.settings.ruler_guides);
}

/// Every one of InDesign's UI colours (IDML `UIColors`) a guide can have is read as itself and
/// written back by the same name.
#[test]
fn every_ui_colour_name_round_trips() {
    const UI_COLORS: [&str; 37] = [
        "LightBlue",
        "Red",
        "Green",
        "Blue",
        "Yellow",
        "Magenta",
        "Cyan",
        "Gray",
        "Black",
        "Orange",
        "DarkGreen",
        "Teal",
        "Tan",
        "Brown",
        "Violet",
        "Gold",
        "DarkBlue",
        "Pink",
        "Lavender",
        "BrickRed",
        "OliveGreen",
        "Peach",
        "Burgundy",
        "GrassGreen",
        "Ochre",
        "Purple",
        "LightGray",
        "Charcoal",
        "GridBlue",
        "GridOrange",
        "Fiesta",
        "LightOlive",
        "Lipstick",
        "CuteTeal",
        "Sulphur",
        "GridGreen",
        "White",
    ];
    let props: Vec<String> = UI_COLORS.iter().map(|n| named("GuideColor", n)).collect();
    let guides: Vec<(&str, &str)> = props.iter().map(|p| (r#"ViewThreshold="5""#, p.as_str())).collect();
    let d = import_idml(&fixture(&guides, None)).unwrap();
    let colors: Vec<[u8; 3]> = options(&d).iter().map(RulerGuideOptions::rgb).collect();
    let distinct: std::collections::HashSet<_> = colors.iter().collect();
    assert_eq!(distinct.len(), UI_COLORS.len(), "each name is a colour of its own: {colors:?}");
    let bytes = export_idml(&d);
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    let mut b = Vec::new();
    archive.by_name(&format!("Spreads/Spread_u{:x}.xml", d.spreads[0].id.0)).unwrap().read_to_end(&mut b).unwrap();
    let spread = xml::parse(&b).unwrap();
    let written: Vec<String> = spread
        .find("Spread")
        .unwrap()
        .find("Page")
        .unwrap()
        .find_all("Guide")
        .map(|g| g.prop_el("GuideColor").unwrap().text_content().trim().to_string())
        .collect();
    assert_eq!(written, UI_COLORS);
}

/// Layers without a `LayerColor`, or with a name that isn't a UI colour, take the UI colours in
/// turn and never White: a layer's selection outlines are drawn in its colour.
#[test]
fn layers_without_a_colour_take_the_ui_colours_in_turn_and_never_white() {
    let layers: String = (0..38)
        .map(|i| {
            let color = if i == 37 { named("LayerColor", "Chartreuse") } else { String::new() };
            format!(r#"<Layer Self="L{i}" Name="Layer {i}" Visible="true" Locked="false">{color}</Layer>"#)
        })
        .collect();
    let map = format!(
        r#"<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" Self="d">
  <idPkg:Preferences src="Resources/Preferences.xml"/>
  {layers}
  <idPkg:Spread src="Spreads/S.xml"/>
</Document>"#
    );
    let prefs = r#"<Preferences><DocumentPreference PageWidth="500" PageHeight="700" FacingPages="false"/></Preferences>"#;
    let spread = r#"<idPkg:Spread xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging">
<Spread Self="s" PageCount="1"><Page Self="p" GeometricBounds="0 0 700 500" ItemTransform="1 0 0 1 0 -350"/></Spread>
</idPkg:Spread>"#;
    let d = import_idml(&zip_files(&[("designmap.xml", &map), ("Resources/Preferences.xml", prefs), ("Spreads/S.xml", spread)])).unwrap();
    let colors: Vec<[u8; 3]> = d.layers.iter().map(|l| l.color).collect();
    let count = |rgb: [u8; 3]| colors.iter().filter(|c| **c == rgb).count();
    assert_eq!(colors.len(), 38);
    assert_eq!(count([255, 255, 255]), 0, "no layer is White: {colors:?}");
    // The 37th and 38th start again at Light Blue and Red; every other colour is used once.
    assert_eq!((count([43, 155, 255]), count([255, 0, 0])), (2, 2));
    assert_eq!((count([0, 0, 135]), count([156, 222, 156])), (1, 1), "Dark Blue and Grid Green");
}
