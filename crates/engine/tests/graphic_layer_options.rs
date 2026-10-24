//! Per-placement artwork layer metadata must survive native saves and stay visibly unsupported.
use std::io::{Cursor, Read, Write};

use designcraft_doc::{Content, GRAPHIC_LAYER_RENDERING_WARNING};
use designcraft_engine::{DocState, Session};
use serde_json::json;

fn fixture() -> Vec<u8> {
    let xml = r#"<Document><Layer Self="L" Name="Synthetic"/><Spread Self="sp"><Page Self="p" GeometricBounds="0 0 100 100"/>
      <Rectangle Self="r" ItemLayer="L" ContentType="GraphicType"><Properties><PathGeometry><GeometryPathType PathOpen="false"><PathPointArray>
      <PathPointType Anchor="0 0"/><PathPointType Anchor="0 20"/><PathPointType Anchor="20 20"/><PathPointType Anchor="20 0"/>
      </PathPointArray></GeometryPathType></PathGeometry></Properties><Image Self="im">
      <Properties><GraphicBounds Left="0" Top="0" Right="20" Bottom="20"/></Properties>
      <Link Self="lk" LinkResourceURI="file:/definitely-missing/synthetic.psd"/>
      <GraphicLayerOption UpdateLinkOption="KeepOverrides">
        <GraphicLayer Self="g0" Id="0" Name="Group"><GraphicLayer Self="g1" Id="1" Name="Hidden" OriginalVisibility="true" CurrentVisibility="false"/></GraphicLayer>
        <GraphicLayer Self="g2" Id="2" Name="Shown" OriginalVisibility="false" CurrentVisibility="true"/>
      </GraphicLayerOption></Image></Rectangle></Spread></Document>"#;
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    z.start_file("designmap.xml", zip::write::SimpleFileOptions::default()).unwrap();
    z.write_all(xml.as_bytes()).unwrap();
    z.finish().unwrap().into_inner()
}

#[test]
fn graphic_layer_options_survive_native_reopen_and_report_unsupported_output() {
    let imported = designcraft_idml::import_idml_report(&fixture(), &|_| None).unwrap();
    assert!(imported.warnings.iter().any(|w| w.contains(GRAPHIC_LAYER_RENDERING_WARNING)));
    let doc = imported.document;
    let options = |d: &designcraft_doc::Document| {
        d.spreads[0]
            .items
            .iter()
            .filter_map(|i| match &i.content {
                Content::Graphic(g) => g.layer_options.clone(),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let expected = options(&doc);
    let saved = designcraft_format::save(&doc).unwrap();
    let mut z = zip::ZipArchive::new(Cursor::new(&saved)).unwrap();
    let mut meta = String::new();
    z.by_name("meta.json").unwrap().read_to_string(&mut meta).unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&meta).unwrap()["version"], 4, "old readers must not silently drop the settings");
    let reopened = designcraft_format::load(&saved).unwrap();
    assert_eq!(options(&reopened), expected);
    let mut s = Session::new();
    s.add_document(DocState::new(reopened, None));
    let preflight = s.execute("preflight.run", &json!({})).unwrap();
    assert!(preflight["issues"].as_array().unwrap().iter().any(|i| i["kind"] == "unsupportedGraphicLayers"));
    let pdf = s.execute("file.exportPdf", &json!({})).unwrap();
    assert!(pdf["warnings"].as_array().unwrap().iter().any(|w| w == GRAPHIC_LAYER_RENDERING_WARNING));
}

#[test]
fn graphic_layer_options_in_cells_require_the_new_native_reader() {
    let mut doc = designcraft_idml::import_idml(&fixture()).unwrap();
    let graphic = doc.spreads[0]
        .items
        .iter()
        .find_map(|i| match &i.content {
            Content::Graphic(g) => Some(g.clone()),
            _ => None,
        })
        .unwrap();
    // Keep the original spread graphic too: its warning must not hide the cell warning.
    let layer = doc.default_layer();
    let (_, story) = doc
        .add_text_frame(designcraft_doc::SpreadRef::Doc(0), designcraft_geom::Rect::new(0.0, 0.0, 80.0, 80.0), layer, "Body", Default::default())
        .unwrap();
    let mut table = designcraft_doc::Table::new(1, 1, 1, 0, 0, 20.0);
    table.cells[0].graphic = Some(graphic);
    doc.story_mut(story).unwrap().insert_table(0, table);
    assert!(doc.has_graphic_layer_options());
    let reopened = designcraft_format::load(&designcraft_format::save(&doc).unwrap()).unwrap();
    assert!(reopened.has_graphic_layer_visibility_overrides());
    let mut s = Session::new();
    s.add_document(DocState::new(reopened, None));
    let preflight = s.execute("preflight.run", &json!({})).unwrap();
    let warnings: Vec<_> = preflight["issues"].as_array().unwrap().iter().filter(|i| i["kind"] == "unsupportedGraphicLayers").collect();
    assert!(warnings.iter().any(|i| i["item"].is_null()), "the cell needs an independent document-level warning");
    assert!(warnings.iter().any(|i| !i["item"].is_null()), "the spread retains its item warning");
}

#[test]
fn pdf_layer_warning_survives_soft_effects_and_flattening() {
    for flatten in [false, true] {
        let doc = designcraft_idml::import_idml(&fixture()).unwrap();
        let id = doc.spreads[0].items.iter().find(|i| i.graphic().is_some()).unwrap().id.0;
        let mut session = Session::new();
        session.add_document(DocState::new(doc, None));
        session.execute("object.feather", &json!({"ids":[id], "width":2})).unwrap();
        let out = session.execute("file.exportPdf", &json!({"flatten":flatten})).unwrap();
        assert!(out["warnings"].as_array().unwrap().iter().any(|w| w == GRAPHIC_LAYER_RENDERING_WARNING), "{out}");
    }
}
