use designcraft_doc::ItemId;
use designcraft_engine::Session;
use designcraft_geom::Rect;
use serde_json::{Value, json};

fn fit(s: &mut Session, mut params: Value) -> designcraft_engine::Result<Value> {
    params["mode"] = json!("fitFrameToContent");
    s.execute("object.fit", &params)
}

fn fixture(text: &str, rect: [f64; 4]) -> (Session, ItemId, Value) {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let f = s.execute("frame.create", &json!({"rect": rect, "content":"text", "text":text})).unwrap();
    let id = ItemId(f["id"].as_u64().unwrap());
    s.execute("selection.set", &json!({"ids":[id.0]})).unwrap();
    (s, id, f["story"].clone())
}

fn bounds(s: &Session, id: ItemId) -> Rect {
    s.doc().unwrap().doc.item(id).unwrap().inner_bounds()
}

fn composed(s: &Session, id: ItemId) -> designcraft_compose::ComposedStory {
    let d = &s.doc().unwrap().doc;
    let sid = d.item(id).unwrap().text_frame().unwrap().story;
    designcraft_compose::compose_story(d, sid, &Default::default())
}

#[test]
fn one_line_fit_preserves_text_style_transform_and_undo() {
    let (mut s, id, _) = fixture("Привет, мир!", [72., 72., 500., 300.]);
    s.execute("type.char", &json!({"attrs":{"fontFamily":"Arial","size":18}})).unwrap();
    s.execute("object.textFrameOptions", &json!({"inset":[3,4,5,6]})).unwrap();
    s.execute("transform.rotate", &json!({"angle":23})).unwrap();
    let original = s.doc().unwrap().doc.item(id).unwrap().clone();
    let original_story = s.doc().unwrap().doc.story(original.text_frame().unwrap().story).unwrap().clone();
    fit(&mut s, json!({})).unwrap();
    let fitted = s.doc().unwrap().doc.item(id).unwrap();
    assert!(fitted.inner_bounds().width() < original.inner_bounds().width() / 2.);
    assert!(fitted.inner_bounds().height() < original.inner_bounds().height() / 2.);
    assert_eq!(fitted.xf, original.xf);
    assert_eq!(fitted.inner_bounds().origin(), original.inner_bounds().origin());
    assert_eq!(fitted.text_frame(), original.text_frame());
    assert_eq!(s.doc().unwrap().doc.story(original_story.id).unwrap(), &original_story);
    assert!(!composed(&s, id).is_overset());
    let fitted_bounds = bounds(&s, id);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(bounds(&s, id), original.inner_bounds());
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(bounds(&s, id), fitted_bounds);
}

#[test]
fn paragraphs_keep_width_and_overset_grows_instead_of_scaling_letters() {
    for height in [12., 500.] {
        let (mut s, id, _) = fixture(&"Многострочный текст для проверки переноса слов. ".repeat(8), [72., 72., 272., 72. + height]);
        let before = s.doc().unwrap().doc.item(id).unwrap().clone();
        fit(&mut s, json!({})).unwrap();
        let after = bounds(&s, id);
        assert_eq!(after.width(), before.inner_bounds().width());
        assert!((after.height() - height).abs() > 1.);
        assert!(!composed(&s, id).is_overset());
        assert!(composed(&s, id).frame(id).unwrap().lines.len() > 1);
    }
}

#[test]
fn empty_frame_stays_finite_and_selected_or_explicit_targets_match() {
    for text in ["", "Hello"] {
        let (mut a, ia, _) = fixture(text, [20., 30., 320., 330.]);
        let (mut b, ib, _) = fixture(text, [20., 30., 320., 330.]);
        fit(&mut a, json!({})).unwrap();
        fit(&mut b, json!({"ids":[ib.0]})).unwrap();
        let r = bounds(&a, ia);
        assert_eq!(r, bounds(&b, ib));
        assert!(r.width().is_finite() && r.height().is_finite() && r.width() > 0. && r.height() > 0.);
        assert!(!composed(&a, ia).is_overset());
    }
}

#[test]
fn columns_vertical_type_and_vertical_alignment_do_not_lose_text() {
    for options in [json!({"columns":2,"gutter":10}), json!({"verticalJustification":"center"}), json!({"verticalJustification":"bottom"})] {
        let (mut s, id, _) = fixture(&"Column fitting test. ".repeat(12), [30., 30., 330., 530.]);
        s.execute("object.textFrameOptions", &options).unwrap();
        fit(&mut s, json!({})).unwrap();
        assert!(!composed(&s, id).is_overset(), "{options}");
    }
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let f = s.execute("frame.create", &json!({"rect":[30,30,330,530],"content":"text","text":"Vertical text test", "vertical":true})).unwrap();
    let id = ItemId(f["id"].as_u64().unwrap());
    fit(&mut s, json!({"ids":[id.0]})).unwrap();
    assert!(!composed(&s, id).is_overset());
}

#[test]
fn linked_frame_does_not_steal_following_frames_text() {
    let (mut s, id, _) = fixture(&"Linked text fitting test. ".repeat(20), [30., 30., 200., 110.]);
    let b = s.execute("frame.create", &json!({"rect":[250,30,450,600],"content":"text","text":""})).unwrap();
    s.execute("text.thread", &json!({"from":id.0,"to":b["id"]})).unwrap();
    let before = composed(&s, id);
    fit(&mut s, json!({"ids":[id.0]})).unwrap();
    let after = composed(&s, id);
    assert_eq!(after.frame(id).unwrap().range, before.frame(id).unwrap().range);
    assert_eq!(bounds(&s, id).width(), 170.);
}

#[test]
fn selection_is_atomic_and_locked_or_non_frame_objects_are_not_changed() {
    let (mut s, id, _) = fixture("Hello", [30., 30., 330., 330.]);
    let b = s.execute("frame.create", &json!({"rect":[350,30,550,330],"content":"text","text":"Other", "shape":"ellipse"})).unwrap();
    let before = bounds(&s, id);
    assert!(fit(&mut s, json!({"ids":[id.0,b["id"]]})).is_err());
    assert_eq!(bounds(&s, id), before);
    assert!(fit(&mut s, json!({"ids":[9999999]})).is_err());
    s.execute("object.lock", &json!({"ids":[id.0]})).unwrap();
    fit(&mut s, json!({"ids":[id.0]})).unwrap();
    assert_eq!(bounds(&s, id), before);
}

#[test]
fn locked_layer_and_unassigned_frame_are_unchanged() {
    let (mut s, id, _) = fixture("Keep this frame", [30., 30., 330., 330.]);
    let layer = s.doc().unwrap().active_layer.0;
    let before = bounds(&s, id);
    s.execute("layer.set", &json!({"id":layer,"locked":true})).unwrap();
    assert_eq!(fit(&mut s, json!({"ids":[id.0]})).unwrap()["changed"], 0);
    assert_eq!(bounds(&s, id), before);
    s.execute("layer.set", &json!({"id":layer,"locked":false})).unwrap();
    let empty = s.execute("frame.create", &json!({"rect":[350,30,550,330]})).unwrap()["id"].as_u64().unwrap();
    let empty = ItemId(empty);
    let before = s.doc().unwrap().doc.item(empty).unwrap().clone();
    assert_eq!(fit(&mut s, json!({"ids":[empty.0]})).unwrap()["changed"], 0);
    assert_eq!(s.doc().unwrap().doc.item(empty).unwrap(), &before);
}

#[test]
fn type_on_a_path_and_unusable_text_area_return_errors_without_edits() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let id = s.execute("path.create", &json!({"anchors":[{"p":[30,30]},{"p":[330,30]}]})).unwrap()["id"].as_u64().unwrap();
    s.execute("type.onPath", &json!({"id":id,"text":"Text on a path"})).unwrap();
    let before = s.doc().unwrap().doc.item(ItemId(id)).unwrap().clone();
    assert!(fit(&mut s, json!({"ids":[id]})).is_err());
    assert_eq!(s.doc().unwrap().doc.item(ItemId(id)).unwrap(), &before);

    let (mut s, id, _) = fixture("No usable area", [30., 30., 40., 40.]);
    s.execute("object.textFrameOptions", &json!({"inset":[20,20,20,20]})).unwrap();
    let before = s.doc().unwrap().doc.item(id).unwrap().clone();
    assert!(fit(&mut s, json!({"ids":[id.0]})).is_err());
    assert_eq!(s.doc().unwrap().doc.item(id).unwrap(), &before);
}

#[test]
fn graphic_fitting_keeps_content_transform_and_reopens() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let png = designcraft_render::Rendered { width: 40, height: 20, pixels: vec![200; 40 * 20 * 4] }.to_png();
    let id = ItemId(s.execute("frame.create", &json!({"rect":[100,100,300,300]})).unwrap()["id"].as_u64().unwrap());
    s.execute("file.place", &json!({"frame":id.0, "base64":designcraft_engine::cmd::base64_encode(&png), "name":"synthetic.png"})).unwrap();
    let before = s.doc().unwrap().doc.item(id).unwrap().clone();
    let designcraft_doc::Content::Graphic(g) = &before.content else { panic!("expected a placed graphic") };
    let expected = g.xf.transform_rect_bbox(Rect::new(0., 0., g.size.0, g.size.1));
    fit(&mut s, json!({"ids":[id.0]})).unwrap();
    assert_eq!(bounds(&s, id), expected);
    assert_eq!(s.doc().unwrap().doc.item(id).unwrap().content, before.content);
    let bytes = s.execute("file.serialize", &json!({})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.openBytes", &json!({"name":"fit.designcraft", "base64":bytes["base64"]})).unwrap();
    assert_eq!(bounds(&reopened, id), expected);
}
