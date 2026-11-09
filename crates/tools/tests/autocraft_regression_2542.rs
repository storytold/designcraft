use designcraft_compose::Cache;
use designcraft_doc::build::NewDocument;
use designcraft_doc::{Document, ParaFormat, Selection, SpreadRef};
use designcraft_geom::{Point, Rect, Unit};
use designcraft_tools::{CanvasLayout, Cursor, Mods, SnapView, ToolContext, create, thread};

#[test]
fn hovering_a_port_shows_the_loaded_text_cursor() {
    let mut d = Document::new(&NewDocument::default());
    let lid = d.default_layer();
    let (a, _) = d
        .add_text_frame(SpreadRef::Doc(0), Rect::new(100.0, 100.0, 250.0, 160.0), lid, &"Words flow on. ".repeat(40), ParaFormat::default())
        .unwrap();
    let (s, c, l) = (Selection::items(vec![a]), Cache::new(), CanvasLayout::new(&d, false));
    let cx = ToolContext {
        doc: &d,
        selection: &s,
        cache: &c,
        layout: &l,
        zoom: 1.0,
        layer: lid,
        snap: SnapView::OFF,
        unit: Unit::Points,
        stroke_in_front: false,
        loaded: None,
    };
    let t = create("selection");
    let out_port = thread::port_center(&d, &l, a, true, 1.0).unwrap();
    let in_port = thread::port_center(&d, &l, a, false, 1.0).unwrap();
    assert_eq!(t.cursor(&cx, out_port, Mods::default()), Cursor::LoadedText);
    assert_eq!(t.cursor(&cx, in_port, Mods::default()), Cursor::LoadedText);
    // Away from the ports the frame's own cursors stay.
    let inside = l.to_canvas(SpreadRef::Doc(0), Point::new(175.0, 130.0));
    assert_ne!(t.cursor(&cx, inside, Mods::default()), Cursor::LoadedText);
}
