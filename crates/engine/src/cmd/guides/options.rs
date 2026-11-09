//! Layout › Ruler Guides: the colour and view threshold of the selected ruler guides, or, with no
//! guide selected, of the ruler guides made from then on (the document's defaults), as in
//! InDesign.

use std::collections::HashSet;

use designcraft_doc::{GuideSel, LAYER_COLORS, RulerGuideOptions};
use serde_json::{Value, json};

use super::{CommandSpec, bad, check_editable, cmd, guides_param, has_doc};
use crate::{Result, Session};

const ID: &str = "guide.options";

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "guide.options",
        "Ruler Guides…",
        ["Layout"],
        None,
        "{viewThreshold?: 5..4000 (percent; the guides are hidden below this zoom), color?: a guide colour name (Light Blue, Red, Green, Blue, Yellow, Magenta, Cyan, Gray, Black, Orange, Dark Green, Teal, Tan, Brown, Violet, Gold) or [r, g, b] (0–255), guides?: [{spread?, page, index}] (default: the selected guides), defaults?: bool (set what new guides get even with guides selected)} → {guides: n}; with no guide named or selected it sets the colour and view threshold of new ruler guides → {defaults: {viewThreshold, color}}. Options left out keep each guide's own",
        has_doc,
        run
    )]
}

/// `o` as the command and `guide.list` report it: the threshold in percent, the colour by name
/// when it has one, as [r, g, b] otherwise.
pub(super) fn describe(o: &RulerGuideOptions) -> Value {
    let rgb = o.rgb();
    let color = LAYER_COLORS.iter().find(|(_, c)| *c == rgb).map_or_else(|| json!(rgb), |(n, _)| json!(n));
    // Rounded so that 7% reads back as 7, not 7.000000000000001.
    let percent = (o.view_threshold * 100.0 * 1e6).round() / 1e6;
    json!({"viewThreshold": percent, "color": color})
}

/// The `color` parameter: a guide colour name (any case) or [r, g, b].
fn color_param(v: &Value) -> Result<[u8; 3]> {
    match v {
        Value::String(n) => LAYER_COLORS
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(n.trim()))
            .map(|(_, c)| *c)
            .ok_or_else(|| bad(ID, format!("unknown guide colour `{n}`"))),
        v => serde_json::from_value(v.clone()).map_err(|_| bad(ID, "color is a guide colour name or [r, g, b] (0–255)")),
    }
}

/// What the dialog's OK changes: each option given replaces that option only.
#[derive(Clone, Copy)]
struct Change {
    threshold: Option<f64>,
    color: Option<[u8; 3]>,
}

impl Change {
    fn apply(self, o: &RulerGuideOptions) -> RulerGuideOptions {
        let mut o = *o;
        if let Some(t) = self.threshold {
            o.view_threshold = t;
        }
        if let Some(c) = self.color {
            o.set_rgb(c);
        }
        o
    }
}

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let threshold = match p.get("viewThreshold") {
        None => None,
        Some(v) => {
            Some(v.as_f64().and_then(RulerGuideOptions::threshold_of_percent).ok_or_else(|| bad(ID, "viewThreshold is a percent from 5 to 4000"))?)
        }
    };
    let color = p.get("color").map(color_param).transpose()?;
    let defaults = match p.get("defaults") {
        None => false,
        Some(v) => v.as_bool().ok_or_else(|| bad(ID, "`defaults` is true or false"))?,
    };
    let change = Change { threshold, color };
    let named = guides_param(p, ID)?;
    let targets: Vec<GuideSel> = match named {
        Some(_) if defaults => return Err(bad(ID, "name guides or set the defaults, not both")),
        Some(list) if list.is_empty() => return Err(bad(ID, "`guides` names no guide")),
        Some(list) => list,
        None if defaults => Vec::new(),
        None => s.doc()?.selection.guides.clone(),
    };
    let d = &s.doc()?.doc;
    if targets.is_empty() {
        let now = change.apply(&d.settings.ruler_guides);
        // The same options again change nothing, and add no undo step.
        if now != d.settings.ruler_guides {
            s.edit(|d, _| {
                d.settings.ruler_guides = now;
                Ok(())
            })?;
        }
        return Ok(json!({"defaults": describe(&now)}));
    }
    let targets: Vec<GuideSel> = {
        let mut seen = HashSet::new();
        targets.into_iter().filter(|g| seen.insert(*g)).collect()
    };
    for g in &targets {
        check_editable(d, g.spread, g.page, g.index, ID)?;
    }
    let changes = targets.iter().any(|g| super::guide_of(d, *g).is_some_and(|guide| change.apply(&guide.options) != guide.options));
    if changes {
        s.edit(|d, _| {
            for g in &targets {
                let guide = d
                    .spread_mut(g.spread)
                    .and_then(|sp| sp.pages.get_mut(g.page))
                    .and_then(|pg| pg.guides.get_mut(g.index))
                    .ok_or_else(|| bad(ID, "no such guide"))?;
                guide.options = change.apply(&guide.options);
            }
            Ok(())
        })?;
    }
    Ok(json!({"guides": targets.len()}))
}

#[cfg(test)]
mod tests {
    use designcraft_doc::{Guide, SpreadRef};
    use serde_json::json;

    use super::*;

    /// A new document with a vertical guide at x = 100 and a horizontal one at y = 200 on page 1;
    /// the horizontal one, added last, is selected.
    fn session() -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("guide.add", &json!({"orientation": "vertical", "position": 100.0, "page": 0})).unwrap();
        s.execute("guide.add", &json!({"orientation": "horizontal", "position": 200.0, "page": 0})).unwrap();
        s
    }

    fn guides(s: &Session) -> Vec<Guide> {
        s.doc().unwrap().doc.spreads[0].pages[0].guides.clone()
    }

    fn undo_steps(s: &Session) -> usize {
        s.doc().unwrap().history.undo.len()
    }

    #[test]
    fn options_of_the_selected_guides_change_and_undo_in_one_step() {
        let mut s = session();
        let before = undo_steps(&s);
        let r = s.execute(ID, &json!({"viewThreshold": 150, "color": "red"})).unwrap();
        assert_eq!(r, json!({"guides": 1}));
        let g = guides(&s);
        assert_eq!(g[1].options, RulerGuideOptions { color: Some([255, 0, 0]), view_threshold: 1.5 });
        assert_eq!(g[0].options, RulerGuideOptions::default(), "an unselected guide keeps its options");
        assert_eq!(s.doc().unwrap().doc.settings.ruler_guides, RulerGuideOptions::default(), "the defaults stay");
        assert_eq!(undo_steps(&s), before + 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(guides(&s)[1].options, RulerGuideOptions::default());
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(guides(&s)[1].options.color, Some([255, 0, 0]));
    }

    #[test]
    fn an_option_left_out_keeps_each_guides_own() {
        let mut s = session();
        let both = json!([{"page": 0, "index": 0}, {"page": 0, "index": 1}]);
        s.execute(ID, &json!({"guides": [{"page": 0, "index": 0}], "viewThreshold": 300, "color": [10, 20, 30]})).unwrap();
        s.execute(ID, &json!({"guides": both, "color": "Gold"})).unwrap();
        let g = guides(&s);
        assert_eq!(g[0].options, RulerGuideOptions { color: Some([255, 153, 0]), view_threshold: 3.0 });
        assert_eq!(g[1].options, RulerGuideOptions { color: Some([255, 153, 0]), view_threshold: 0.05 });
        s.execute(ID, &json!({"guides": both, "viewThreshold": 5})).unwrap();
        let g = guides(&s);
        assert_eq!(g.iter().map(|g| g.options.view_threshold).collect::<Vec<_>>(), [0.05, 0.05]);
        assert_eq!(g.iter().map(|g| g.options.color).collect::<Vec<_>>(), [Some([255, 153, 0]); 2]);
        // Cyan is the default colour: it is stored, listed and saved as the default.
        s.execute(ID, &json!({"guides": both, "color": "Cyan"})).unwrap();
        assert!(guides(&s).iter().all(|g| g.options.is_default()));
        assert_eq!(s.execute("guide.list", &json!({})).unwrap()[0]["color"], "Cyan");
    }

    #[test]
    fn with_no_guide_selected_it_sets_what_new_guides_get() {
        let mut s = session();
        s.execute("guide.deselect", &json!({})).unwrap();
        let r = s.execute(ID, &json!({"viewThreshold": 75, "color": "Violet"})).unwrap();
        assert_eq!(r, json!({"defaults": {"viewThreshold": 75.0, "color": "Violet"}}));
        assert!(guides(&s).iter().all(|g| g.options.is_default()), "existing guides keep theirs");
        let want = RulerGuideOptions { color: Some([153, 51, 255]), view_threshold: 0.75 };
        assert_eq!(s.doc().unwrap().doc.settings.ruler_guides, want);
        // Guides from the rulers and from Create Guides get the defaults.
        s.execute("guide.add", &json!({"orientation": "both", "point": [50.0, 60.0], "page": 0})).unwrap();
        s.execute("layout.createGuides", &json!({"rows": 2, "columns": 0, "page": 0})).unwrap();
        let g = guides(&s);
        assert_eq!(g.len(), 2 + 2 + 2);
        assert!(g[2..].iter().all(|g| g.options == want), "{g:?}");
        // `defaults` sets them with guides selected too, and leaves the selected guides alone.
        let r = s.execute(ID, &json!({"defaults": true, "color": [1, 2, 3]})).unwrap();
        assert_eq!(r["defaults"]["color"], json!([1, 2, 3]));
        assert!(guides(&s)[2..].iter().all(|g| g.options == want));
        // Undo puts the defaults back.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.settings.ruler_guides, want);
    }

    #[test]
    fn the_same_options_again_add_no_undo_step() {
        let mut s = session();
        s.execute(ID, &json!({"color": "Red"})).unwrap();
        let steps = undo_steps(&s);
        assert_eq!(s.execute(ID, &json!({"color": "red"})).unwrap(), json!({"guides": 1}));
        assert_eq!(s.execute(ID, &json!({})).unwrap(), json!({"guides": 1}));
        s.execute("guide.deselect", &json!({})).unwrap();
        s.execute(ID, &json!({"viewThreshold": 5, "color": "Cyan"})).unwrap();
        assert_eq!(undo_steps(&s), steps);
    }

    #[test]
    fn bad_options_change_nothing() {
        let mut s = session();
        let doc = s.doc().unwrap().doc.clone();
        for p in [
            json!({"viewThreshold": 4.9}),
            json!({"viewThreshold": 4001}),
            json!({"viewThreshold": "50"}),
            json!({"viewThreshold": null}),
            json!({"color": "Chartreuse"}),
            json!({"color": [256, 0, 0]}),
            json!({"color": [1, 2]}),
            json!({"color": [-1, 0, 0]}),
            json!({"color": 7}),
            json!({"defaults": "yes"}),
            json!({"guides": []}),
            json!({"guides": [{"page": 0, "index": 9}]}),
            json!({"guides": [{"page": 4, "index": 0}]}),
            json!({"guides": [{"page": 0}]}),
            json!({"guides": "all"}),
            json!({"guides": [{"page": 0, "index": 0}], "defaults": true}),
            // A good guide with a missing one: neither changes.
            json!({"guides": [{"page": 0, "index": 0}, {"page": 0, "index": 2}], "color": "Red"}),
        ] {
            assert!(s.execute(ID, &p).is_err(), "{p}");
            assert_eq!(s.doc().unwrap().doc, doc, "{p}");
        }
        // The ends of the range are taken.
        s.execute(ID, &json!({"viewThreshold": 4000})).unwrap();
        assert_eq!(guides(&s)[1].options.view_threshold, 40.0);
    }

    #[test]
    fn locked_guides_and_guides_on_locked_layers_keep_their_options() {
        let mut s = session();
        let layer = s.doc().unwrap().doc.layers[0].id.0;
        s.execute("layer.set", &json!({"id": layer, "locked": true})).unwrap();
        assert!(s.execute(ID, &json!({"guides": [{"page": 0, "index": 0}], "color": "Red"})).is_err());
        s.execute("layer.set", &json!({"id": layer, "locked": false})).unwrap();
        s.edit(|d, _| {
            d.spread_mut(SpreadRef::Doc(0)).unwrap().pages[0].guides[0].locked = true;
            Ok(())
        })
        .unwrap();
        assert!(s.execute(ID, &json!({"guides": [{"page": 0, "index": 0}], "color": "Red"})).is_err());
        assert_eq!(guides(&s)[0].options, RulerGuideOptions::default());
    }

    #[test]
    fn the_guide_list_reports_each_guides_options() {
        let mut s = session();
        s.execute(ID, &json!({"viewThreshold": 2500, "color": [12, 34, 56]})).unwrap();
        let list = s.execute("guide.list", &json!({})).unwrap();
        assert_eq!((&list[0]["viewThreshold"], &list[0]["color"]), (&json!(5.0), &json!("Cyan")));
        assert_eq!((&list[1]["viewThreshold"], &list[1]["color"]), (&json!(2500.0), &json!([12, 34, 56])));
    }

    #[test]
    fn options_are_saved_with_the_document_and_old_files_read_as_defaults() {
        let mut s = session();
        s.execute(ID, &json!({"viewThreshold": 200, "color": "Teal"})).unwrap();
        s.execute("guide.deselect", &json!({})).unwrap();
        s.execute(ID, &json!({"viewThreshold": 50, "color": [9, 8, 7]})).unwrap();
        let doc = &s.doc().unwrap().doc;
        let text = serde_json::to_string(&**doc).unwrap();
        let back: designcraft_doc::Document = serde_json::from_str(&text).unwrap();
        assert_eq!(back.spreads[0].pages[0].guides, doc.spreads[0].pages[0].guides);
        assert_eq!(back.settings.ruler_guides, RulerGuideOptions { color: Some([9, 8, 7]), view_threshold: 0.5 });
        // A guide with the default options saves as before, and one saved before reads as them.
        let plain = serde_json::to_value(&doc.spreads[0].pages[0].guides[0]).unwrap();
        assert!(plain.get("options").is_none(), "{plain}");
        let old: Guide = serde_json::from_value(json!({"orientation": "vertical", "position": 3.0})).unwrap();
        assert_eq!(old.options, RulerGuideOptions::default());
    }
}
