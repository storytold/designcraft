//! The Table panel: row/column counts, row height, column width, cell text alignment and insets,
//! merge/unmerge — for the table at the text insertion point or the selected cells.

use designcraft_geom::Unit;
use serde_json::{Value, json};

use crate::DesignApp;
use crate::theme::Tokens;
use crate::widgets::{caption, divider, measure, number};

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Ok(info) = app.session.execute("table.get", &json!({})) else {
        ui.label(crate::rtl::widget(
            ui,
            egui::RichText::new(crate::i18n::tr(&app.ui.language, "Place the insertion point in a table, or create one.")).color(t.text_dim),
        ));
        ui.add_space(6.0);
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Create Table…"))).clicked() {
            let _ = app.run("app.insertTableDialog", json!({}));
        }
        return;
    };
    let unit = app.session.active().map(|d| d.doc.settings.horizontal_units).unwrap_or(Unit::Picas);
    let rows = info["rows"].as_array().cloned().unwrap_or_default();
    let cols = info["columns"].as_array().cloned().unwrap_or_default();
    let range = info["range"].clone();
    let (r0, c0) = (range["r0"].as_u64().unwrap_or(0) as usize, range["c0"].as_u64().unwrap_or(0) as usize);
    let tid = info["table"].clone();
    let cell = info["cells"].as_array().and_then(|cs| cs.iter().find(|c| c["row"] == r0 && c["col"] == c0)).cloned().unwrap_or(Value::Null);
    let run = |app: &mut DesignApp, id: &str, p: Value| {
        let _ = app.run(id, p);
    };
    ui.horizontal(|ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Cell Type"));
        if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Graphic…"))).clicked()
            && let Err(e) = app.run("app.graphicCell", json!({}))
        {
            app.status(format!("Table: {e}"));
        }
        if ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Text"))).clicked() {
            let _ = app.run("table.textCell", json!({}));
        }
    });
    // Move the target row / column (InDesign drags them).
    ui.horizontal(|ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Move"));
        let (nr, nc) = (rows.len(), cols.len());
        for (label, id, from, to, ok) in [
            ("Row ↑", "table.moveRow", r0, r0.saturating_sub(1), r0 > 0),
            ("Row ↓", "table.moveRow", r0, r0 + 1, r0 + 1 < nr),
            ("Col ←", "table.moveColumn", c0, c0.saturating_sub(1), c0 > 0),
            ("Col →", "table.moveColumn", c0, c0 + 1, c0 + 1 < nc),
        ] {
            if ui.add_enabled(ok, egui::Button::new(crate::rtl::widget(ui, label)).small()).clicked()
                && let Err(e) = app.run(id, json!({"from": from, "to": to}))
            {
                app.status(format!("Table: {e}"));
            }
        }
    });
    // Table and cell styles: pick to apply, + to save the current look as a new style.
    if let Some(st) = app.session.active() {
        let styles = &st.doc.styles;
        let tnames: Vec<String> = styles.table.iter().map(|s| s.name.clone()).collect();
        let cnames: Vec<String> = styles.cell.iter().map(|s| s.name.clone()).collect();
        let tcur = info["style"].as_str().filter(|s| !s.is_empty()).unwrap_or(designcraft_doc::BASIC_TABLE).to_string();
        let ccur = cell["style"].as_str().filter(|s| !s.is_empty()).unwrap_or(designcraft_doc::NO_CELL_STYLE).to_string();
        for (label, kind, names, cur) in [("Table Style", "table", tnames, tcur), ("Cell Style", "cell", cnames, ccur)] {
            ui.horizontal(|ui| {
                caption(ui, crate::i18n::tr(&app.ui.language, label));
                egui::ComboBox::from_id_salt(("tp_style", kind)).selected_text(&cur).width(130.0).show_ui(ui, |ui| {
                    for n in &names {
                        if ui.selectable_label(*n == cur, n).clicked() {
                            run(app, &format!("style.{kind}.apply"), json!({"name": n, "table": tid}));
                        }
                    }
                });
                if crate::icons::button(ui, "plus", 20.0, false, &format!("New {label}")).clicked() {
                    if kind == "cell" {
                        run(app, "style.cell.create", json!({"name": "Cell Style 1", "fromSelection": true}));
                    } else {
                        run(app, "style.table.create", json!({"name": "Table Style 1"}));
                    }
                }
            });
        }
    }
    egui::Grid::new("table_panel").num_columns(4).spacing([6.0, 6.0]).show(ui, |ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Rows"));
        if let Some(v) = number(ui, "tp_rows", Some(rows.len() as f64), "", 52.0, 0) {
            let n = v.round().max(1.0) as usize;
            if n > rows.len() {
                run(
                    app,
                    "table.insertRow",
                    json!({"table": tid, "rows": [rows.len() - 1, rows.len() - 1], "where": "below", "count": n - rows.len()}),
                );
            } else if n < rows.len() {
                run(app, "table.deleteRow", json!({"table": tid, "rows": [n, rows.len() - 1]}));
            }
        }
        caption(ui, crate::i18n::tr_context(&app.ui.language, "Columns", "table"));
        if let Some(v) = number(ui, "tp_cols", Some(cols.len() as f64), "", 52.0, 0) {
            let n = v.round().max(1.0) as usize;
            if n > cols.len() {
                run(
                    app,
                    "table.insertColumn",
                    json!({"table": tid, "cols": [cols.len() - 1, cols.len() - 1], "where": "right", "count": n - cols.len()}),
                );
            } else if n < cols.len() {
                run(app, "table.deleteColumn", json!({"table": tid, "cols": [n, cols.len() - 1]}));
            }
        }
        ui.end_row();
        let row = rows.get(r0).cloned().unwrap_or(Value::Null);
        let exactly = row["mode"] == "exactly";
        caption(ui, crate::i18n::tr(&app.ui.language, "Row Height"));
        egui::ComboBox::from_id_salt("tp_rowmode")
            .selected_text(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, if exactly { "Exactly" } else { "At Least" })))
            .width(70.0)
            .show_ui(ui, |ui| {
                for (label, mode) in [("At Least", "atLeast"), ("Exactly", "exactly")] {
                    if ui.selectable_label((mode == "exactly") == exactly, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked()
                    {
                        run(app, "table.setRowHeight", json!({"mode": mode}));
                    }
                }
            });
        if let Some(h) = measure(ui, "tp_rowh", row["height"].as_f64(), unit, 64.0) {
            run(app, "table.setRowHeight", json!({"height": h}));
        }
        ui.end_row();
        caption(ui, crate::i18n::tr(&app.ui.language, "Column Width"));
        ui.label("");
        if let Some(w) = measure(ui, "tp_colw", cols.get(c0).and_then(|c| c["width"].as_f64()), unit, 64.0) {
            run(app, "table.setColumnWidth", json!({"width": w}));
        }
        ui.end_row();
    });
    if ui
        .button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Distribute Rows Evenly")))
        .on_hover_text("Equal fixed heights for the selected rows; cell text may become overset.")
        .clicked()
        && let Err(e) = app.run("table.distributeRows", json!({}))
    {
        app.status(format!("Table: {e}"));
    }
    divider(ui);
    caption(ui, crate::i18n::tr(&app.ui.language, "Cell Text"));
    ui.horizontal(|ui| {
        for (label, vj) in [("Top", "top"), ("Center", "center"), ("Bottom", "bottom"), ("Justify", "justify")] {
            if ui.selectable_label(cell["vj"] == vj, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() {
                run(app, "table.setCell", json!({"vj": vj}));
            }
        }
    });
    ui.add_space(4.0);
    let insets: Vec<f64> = cell["insets"].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_else(|| vec![4.0; 4]);
    egui::Grid::new("table_insets").num_columns(4).spacing([6.0, 6.0]).show(ui, |ui| {
        let labels = ["Top", "Left", "Bottom", "Right"];
        for (i, l) in labels.iter().enumerate() {
            caption(ui, crate::i18n::tr(&app.ui.language, l));
            if let Some(v) = measure(ui, &format!("tp_inset{i}"), insets.get(i).copied(), unit, 56.0) {
                let mut n = insets.clone();
                n.resize(4, 4.0);
                n[i] = v.max(0.0);
                run(app, "table.setCell", json!({"insets": n}));
            }
            if i % 2 == 1 {
                ui.end_row();
            }
        }
    });
    divider(ui);
    ui.horizontal(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Merge Cells"))).clicked() {
            run(app, "table.merge", json!({}));
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Unmerge"))).clicked() {
            run(app, "table.unmerge", json!({}));
        }
    });
    ui.horizontal(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Insert Row"))).clicked() {
            run(app, "table.insertRowBelow", json!({}));
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Insert Column"))).clicked() {
            run(app, "table.insertColumnRight", json!({}));
        }
    });
    ui.horizontal(|ui| {
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete Row"))).clicked() {
            run(app, "table.deleteRow", json!({}));
        }
        if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete Column"))).clicked() {
            run(app, "table.deleteColumn", json!({}));
        }
    });
    divider(ui);
    // Column order: right to left puts the first column on the right.
    let rtl = info["options"]["direction"] == "rightToLeft";
    ui.horizontal(|ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Direction"));
        for (on, label, dir) in [(!rtl, "Left to Right", "leftToRight"), (rtl, "Right to Left", "rightToLeft")] {
            if ui.selectable_label(on, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).clicked() && !on {
                run(app, "table.options", json!({"direction": dir}));
            }
        }
    });
    let mut alt = !info["options"]["altRows"].is_null();
    if crate::widgets::native_checkbox(ui, &mut alt, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Alternating Row Fills"))).changed() {
        let v = if alt { json!({"first": 1, "firstColor": "[Black]", "firstTint": 0.1, "next": 1, "nextColor": "[None]"}) } else { Value::Null };
        run(app, "table.options", json!({"altRows": v}));
    }
    let mut rep = info["options"]["repeatHeader"].as_bool().unwrap_or(true);
    if crate::widgets::native_checkbox(ui, &mut rep, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Repeat Header Rows"))).changed() {
        run(app, "table.options", json!({"repeatHeader": rep}));
    }
    ui.horizontal(|ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Header Rows"));
        if let Some(v) = number(ui, "tp_hdr", info["headerRows"].as_f64(), "", 40.0, 0) {
            run(app, "table.options", json!({"headerRows": v.max(0.0) as u64}));
        }
        caption(ui, crate::i18n::tr(&app.ui.language, "Footer Rows"));
        if let Some(v) = number(ui, "tp_ftr", info["footerRows"].as_f64(), "", 40.0, 0) {
            run(app, "table.options", json!({"footerRows": v.max(0.0) as u64}));
        }
    });
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use designcraft_doc::table::AltFills;
    use designcraft_engine::{DocState, HistoryEntry, Session};
    use egui::{Event, Id, Key, LayerId, Modifiers, Order, PointerButton, Pos2, Rect, vec2};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use serde_json::json;

    use crate::{DesignApp, Services};

    const LABELS: [&str; 2] = ["Alternating Row Fills", "Repeat Header Rows"];

    #[derive(Clone, Copy, Debug, PartialEq)]
    struct Rows {
        ids: [u128; 2],
        rects: [Rect; 2],
        layer: LayerId,
    }

    fn rows(h: &Harness<'_, DesignApp>) -> Rows {
        let ids = LABELS.map(|label| u128::from(h.get_by_label(label).accesskit_node().id()));
        let rects = LABELS.map(|label| h.get_by_label(label).rect());
        let panel = "table".to_owned();
        let layer = LayerId::new(Order::Middle, Id::new(("floating_panel", &panel)));
        assert_ne!(ids[0], ids[1], "distinct real consumer rows");
        assert!(rects[0].bottom() <= rects[1].top(), "actual backward draw order");
        for rect in rects {
            assert!(h.ctx.content_rect().contains_rect(rect), "fixture row must be on screen");
            assert_eq!(
                h.ctx.layer_id_at(rect.center()),
                Some(layer),
                "fixture row must be uncovered: row={rect:?}, area={:?}, scale={}",
                h.ctx.memory(|m| m.area_rect(layer.id)),
                h.ctx.pixels_per_point()
            );
        }
        Rows { ids, rects, layer }
    }

    fn pointer(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }

    fn step(h: &mut Harness<'_, DesignApp>, events: Vec<Event>) {
        let before = h.ctx.cumulative_frame_nr();
        h.input_mut().events.extend(events);
        h.run_steps(1);
        assert_eq!(h.ctx.cumulative_frame_nr(), before + 1, "do not drain the deferred pass early");
    }

    fn next_document_keys() -> Vec<Event> {
        vec![
            Event::ModifiersChanged(Modifiers::COMMAND),
            Event::Key { key: Key::F6, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND },
        ]
    }

    fn flags(st: &DocState) -> (bool, bool) {
        let text = st.selection.text.unwrap();
        let cell = text.cell.unwrap();
        let table = st.doc.story(text.story).unwrap().tables.get(&cell.table).unwrap();
        (table.options.alt_rows.is_some(), table.options.repeat_header)
    }

    fn assert_history(actual: &[HistoryEntry], expected: &[HistoryEntry], context: &str) {
        assert_eq!(actual.len(), expected.len(), "{context}: history length");
        for (a, e) in actual.iter().zip(expected) {
            assert_eq!(a.label, e.label, "{context}: history label");
            assert_eq!(*a.doc, *e.doc, "{context}: history document");
            assert_eq!(a.selection, e.selection, "{context}: history selection");
        }
    }

    fn assert_state(actual: &DocState, expected: &DocState, context: &str) {
        eprintln!(
            "{context}: uid={}, flags={:?}, revision={}, undo={}, redo={}; expected flags={:?}, revision={}, undo={}, redo={}",
            actual.uid,
            flags(actual),
            actual.revision,
            actual.history.undo.len(),
            actual.history.redo.len(),
            flags(expected),
            expected.revision,
            expected.history.undo.len(),
            expected.history.redo.len()
        );
        assert_eq!(actual.uid, expected.uid, "{context}: document identity");
        assert_eq!(*actual.doc, *expected.doc, "{context}: exact model");
        assert_eq!(actual.selection, expected.selection, "{context}: exact selection");
        assert_eq!(actual.revision, expected.revision, "{context}: revision");
        assert_eq!(actual.saved_revision, expected.saved_revision, "{context}: saved revision");
        assert_eq!(*actual.saved_doc, *expected.saved_doc, "{context}: saved document");
        assert_eq!(actual.active_layer, expected.active_layer, "{context}: active layer");
        assert_eq!(actual.path, expected.path, "{context}: path");
        assert_eq!(actual.editing_parents, expected.editing_parents, "{context}: parent editing");
        assert_eq!(actual.preview_stash, expected.preview_stash, "{context}: preview document");
        assert_eq!(actual.preview_record, expected.preview_record, "{context}: preview record");
        match (&actual.fonts, &expected.fonts) {
            (None, None) => {}
            (Some(a), Some(e)) => assert!(Arc::ptr_eq(a, e), "{context}: font scope lifetime"),
            _ => panic!("{context}: font scope presence"),
        }
        assert_eq!(actual.history.limit, expected.history.limit, "{context}: history limit");
        assert!(actual.interaction.is_none() && expected.interaction.is_none());
        assert_history(&actual.history.undo, &expected.history.undo, context);
        assert_history(&actual.history.redo, &expected.history.redo, context);
    }

    // Literal model/history oracle: one intended Table Options assignment.
    // This never executes the production command to construct the expected state.
    fn expected_edit(before: &DocState, alternating: bool) -> DocState {
        let mut expected = before.clone();
        let mut doc = (*before.doc).clone();
        let text = before.selection.text.unwrap();
        let cell = text.cell.unwrap();
        let story = doc.story_mut(text.story).unwrap();
        story.rev += 1; // Literal expected cache revision; do not call table.options/table_mut.
        let table = Arc::make_mut(story.tables.get_mut(&cell.table).unwrap());
        if alternating {
            assert!(table.options.alt_rows.is_none());
            table.options.alt_rows = Some(AltFills {
                first: 1,
                first_color: "[Black]".into(),
                first_tint: 0.1,
                next: 1,
                next_color: "[None]".into(),
                next_tint: 1.0,
                skip_first: 0,
                skip_last: 0,
            });
        } else {
            assert!(!table.options.repeat_header);
            table.options.repeat_header = true;
        }
        expected.doc = Arc::new(doc);
        expected.revision += 1;
        expected.history.undo.push(HistoryEntry { label: "Table Options".into(), doc: before.doc.clone(), selection: before.selection.clone() });
        expected.history.redo.clear();
        expected
    }

    fn fixture() -> (Harness<'static, DesignApp>, [DocState; 2], Rows) {
        let mut app = DesignApp::new(Session::new(), Services::default());
        app.ui.language = "en".into();
        for title in ["Context A", "Context B"] {
            app.run("file.new", json!({"title": title, "pages": 1})).unwrap();
            let frame = app
                .run(
                    "frame.create",
                    json!({
                        "rect": [72,72,500,700], "content": "text", "text": ""
                    }),
                )
                .unwrap();
            app.run("text.select", json!({"story": frame["story"], "anchor": 0, "focus": 0})).unwrap();
            app.run("table.insert", json!({"rows": 2, "cols": 2, "headerRows": 1})).unwrap();
            app.run("table.options", json!({"altRows": null, "repeatHeader": false})).unwrap();
        }
        // B has a real Redo entry that the isolation test must preserve.
        app.run("table.options", json!({"repeatHeader": true})).unwrap();
        app.run("edit.undo", json!({})).unwrap();
        app.session.set_active(0);
        app.ui.docking = None;
        app.ui.floating = vec![("table".into(), [60.0, 60.0])];

        let mut ready = false;
        let mut h = Harness::builder().with_size(vec2(1280.0, 1200.0)).with_step_dt(1.0 / 60.0).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if !ready {
                    ready = true;
                    return; // Set max_texture_side before font installation.
                }
                let ctx = ui.ctx().clone();
                app.logic(&ctx); // Real shortcut dispatch before the real panel.
                // Match DesignApp::ui: installed fonts become available next frame.
                if !app.fonts_ready {
                    return;
                }
                egui::CentralPanel::default().show(ui, |_| {});
                crate::dock::floating(app, &ctx);
            },
            app,
        );
        h.input_mut().max_texture_side = Some(8192);
        h.run_steps(8);
        let baseline: [DocState; 2] = std::array::from_fn(|i| h.state().session.documents()[i].clone());
        assert_ne!(baseline[0].uid, baseline[1].uid);
        assert_eq!(flags(&baseline[0]), (false, false));
        assert_eq!(flags(&baseline[1]), (false, false));
        assert_eq!(baseline[1].history.redo.len(), 1);
        h.get_by_label("Repeat Header Rows").scroll_to_me();
        h.run_steps(3);
        for (i, expected) in baseline.iter().enumerate() {
            assert_state(&h.state().session.documents()[i], expected, "fixture scroll is model-neutral");
        }
        let geometry = rows(&h);

        // Establish identical real IDs and fresh bounds in BOTH docs before
        // any gesture, rather than assuming a reused widget ID after the fault.
        for active in [1, 0] {
            h.state_mut().run("window.nextDocument", json!({})).unwrap();
            step(&mut h, vec![]);
            assert_eq!(h.state().session.active_index(), Some(active));
            assert_eq!(rows(&h), geometry, "preflight: same actual rows in A and B");
            for (i, expected) in baseline.iter().enumerate() {
                assert_state(&h.state().session.documents()[i], expected, "preflight is model-neutral");
            }
        }
        assert!(!h.ctx.text_edit_focused());
        assert!(h.state().ui.dialog.is_none() && h.state().ui.palette.is_none());
        (h, baseline, geometry)
    }

    fn completed_batch(h: &mut Harness<'_, DesignApp>, geometry: Rows, baseline: &[DocState; 2]) -> DocState {
        let p = geometry.rects[1].center(); // later-drawn Repeat Header Rows
        let q = geometry.rects[0].center(); // earlier-drawn Alternating Row Fills
        assert!(!geometry.rects[1].contains(q));
        step(h, vec![Event::PointerMoved(p), pointer(p, true), Event::PointerMoved(q), pointer(q, false)]);
        assert!(h.ctx.input(|i| i.pointer.primary_pressed() && i.pointer.primary_released() && !i.pointer.primary_down()));
        assert_eq!(rows(h), geometry, "batch must not move/reidentify the consumer");
        let origin_only = expected_edit(&baseline[0], false);
        assert_state(&h.state().session.documents()[0], &origin_only, "complete batch: A origin only");
        assert_state(&h.state().session.documents()[1], &baseline[1], "complete batch: B untouched");
        origin_only
    }

    #[test]
    fn checkbox_completed_backward_settles_in_its_document() {
        let (mut h, baseline, geometry) = fixture();
        let origin_only = completed_batch(&mut h, geometry, &baseline);
        let settled = expected_edit(&origin_only, true);
        for _ in 0..2 {
            step(&mut h, vec![]);
            assert_eq!(rows(&h), geometry);
            assert_state(&h.state().session.documents()[0], &settled, "positive control: A settles once");
            assert_state(&h.state().session.documents()[1], &baseline[1], "positive control: B unchanged");
        }
    }

    #[test]
    fn checkbox_completed_document_switch_keeps_b_untouched() {
        let (mut h, baseline, geometry) = fixture();
        let origin_only = completed_batch(&mut h, geometry, &baseline);
        // No direct set_active and no pointer event: run the actual global
        // shortcut in the immediate next frame, before floating-panel layout.
        step(&mut h, next_document_keys());
        assert_eq!(h.state().session.active_index(), Some(1), "actual Cmd+F6 dispatch");
        assert!(h.ctx.input(|i| !i.pointer.primary_down() && !i.pointer.primary_pressed() && !i.pointer.primary_released()));
        assert_eq!(rows(&h), geometry, "fault classification requires stable real IDs and bounds");
        assert_state(&h.state().session.documents()[0], &origin_only, "after switch: A remains origin only");
        assert_state(&h.state().session.documents()[1], &baseline[1], "ISOLATION: deferred A gesture must not edit B or clear B Redo");
        for _ in 0..2 {
            step(&mut h, vec![]);
            assert_state(&h.state().session.documents()[0], &origin_only, "no replay into A");
            assert_state(&h.state().session.documents()[1], &baseline[1], "no delayed replay into B");
        }
    }
    #[test]
    fn checkbox_completed_undo_keeps_restored_document_and_redo() {
        let (mut h, baseline, geometry) = fixture();
        let origin_only = completed_batch(&mut h, geometry, &baseline);
        assert_eq!(h.state().session.active_index(), Some(0));
        assert!(baseline[0].history.redo.is_empty(), "A starts with no Redo");
        assert_eq!(origin_only.history.undo.len(), baseline[0].history.undo.len() + 1);
        assert!(!h.ctx.text_edit_focused());
        assert!(h.state().ui.dialog.is_none() && h.state().ui.palette.is_none());

        // Literal expected full document/history; never execute edit.undo to build it.
        // Restored document, selection, original Undo stack and saved metadata are
        // the untouched baseline. Undo increments, rather than restores, revision.
        let mut undone = baseline[0].clone();
        undone.revision = origin_only.revision + 1;
        undone.history.redo.push(HistoryEntry {
            label: "Table Options".into(),
            doc: origin_only.doc.clone(),
            selection: origin_only.selection.clone(),
        });

        // Immediate next frame: real shortcut, no new pointer press or release.
        step(
            &mut h,
            vec![
                Event::ModifiersChanged(Modifiers::COMMAND),
                Event::Key { key: Key::Z, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND },
            ],
        );
        assert_eq!(h.state().session.active_index(), Some(0), "Undo stays in A");
        assert!(h.ctx.input(|i| !i.pointer.primary_down() && !i.pointer.primary_pressed() && !i.pointer.primary_released()));
        assert_eq!(rows(&h), geometry, "Undo must preserve real IDs, bounds and layer");
        // Undo must actually restore the origin before any endpoint failure counts.
        // A pending alternating-fill assignment cannot itself clear repeatHeader.
        assert!(!flags(&h.state().session.documents()[0]).1, "actual Ctrl/Cmd+Z restored the origin");
        assert_state(&h.state().session.documents()[1], &baseline[1], "Undo: B untouched");
        assert_state(&h.state().session.documents()[0], &undone, "UNDO ISOLATION: restored A and its Redo must survive");
        for _ in 0..2 {
            step(&mut h, vec![]);
            assert_eq!(h.state().session.active_index(), Some(0));
            assert_eq!(rows(&h), geometry);
            assert_state(&h.state().session.documents()[0], &undone, "Undo: no delayed endpoint replay");
            assert_state(&h.state().session.documents()[1], &baseline[1], "Undo: B remains untouched");
        }
    }
    #[test]
    fn checkbox_completed_table_selection_keeps_other_table_untouched() {
        use designcraft_doc::{CellAddr, CellRange, Selection, TableSel, TextSel};
        use std::sync::mpsc::{Receiver, Sender, channel};

        // Literal selection-only expectation, independent of the command handler.
        fn selected(before: &DocState, target: TextSel) -> DocState {
            let mut expected = before.clone();
            let table = target.cell.unwrap().table;
            expected.selection = Selection {
                cells: Some(TableSel { story: target.story, table, range: CellRange { r0: 0, c0: 0, r1: 0, c1: 0 } }),
                text: Some(TextSel {
                    story: target.story,
                    anchor: 0,
                    focus: 0,
                    frame: before.selection.text.and_then(|t| t.frame),
                    cell: Some(CellAddr { table, row: 0, col: 0 }),
                }),
                items: Vec::new(),
                content: false,
                anchors: Vec::new(),
                key: None,
                page: None,
            };
            expected.revision += 1;
            expected // Exact document, saved state and every Undo/Redo entry unchanged.
        }
        fn enqueue(sender: &Sender<crate::ControlRequest>, target: TextSel) -> Receiver<crate::ControlResponse> {
            let (request, reply) = crate::ControlRequest::new(
                "engine.execute",
                json!({
                    "command": "table.select",
                    "params": {"story": target.story.0, "table": target.cell.unwrap().table,
                               "rows": [0, 0], "cols": [0, 0]}
                }),
            );
            assert!(sender.send(request).is_ok(), "fixture control queue is connected");
            reply
        }

        let (mut h, initial, geometry) = fixture();
        let origin = initial[0].selection.text.unwrap();
        assert_eq!(h.state().session.active_index(), Some(0));
        // Create one extra table in A through normal setup commands; no new document.
        let frame = h
            .state_mut()
            .run(
                "frame.create",
                json!({
                    "rect": [72,72,500,700], "content": "text", "text": ""
                }),
            )
            .unwrap();
        h.state_mut().run("text.select", json!({"story": frame["story"], "anchor": 0, "focus": 0})).unwrap();
        h.state_mut().run("table.insert", json!({"rows": 2, "cols": 2, "headerRows": 1})).unwrap();
        h.state_mut().run("table.options", json!({"altRows": null, "repeatHeader": false})).unwrap();
        let destination = h.state().session.documents()[0].selection.text.unwrap();
        assert_ne!(origin.story, destination.story);
        assert_ne!(origin.cell.unwrap().table, destination.cell.unwrap().table);
        assert_eq!(flags(&h.state().session.documents()[0]), (false, false));

        let (sender, receiver) = channel();
        assert!(h.state().control_rx.is_none());
        h.state_mut().control_rx = Some(receiver);
        // Supported selection route is independently checked before any gesture.
        for target in [destination, origin] {
            let expected = selected(&h.state().session.documents()[0], target);
            let reply = enqueue(&sender, target);
            step(&mut h, vec![]);
            let response = reply.try_recv().expect("app.logic must consume request in this frame");
            assert_eq!(response["ok"], json!(true), "{response:?}");
            assert_eq!(h.state().session.active_index(), Some(0));
            assert_eq!(rows(&h), geometry, "both real tables reuse identical visible rows");
            assert_state(&h.state().session.documents()[0], &expected, "selection preflight: literal state");
            assert_state(&h.state().session.documents()[1], &initial[1], "selection preflight: B unchanged");
        }
        let baseline = [h.state().session.documents()[0].clone(), initial[1].clone()];
        let origin_only = completed_batch(&mut h, geometry, &baseline);
        let expected = selected(&origin_only, destination);

        // The release already happened. Only a selection request reaches next-frame
        // app.logic; no direct command/state mutation occurs between gesture and layout.
        let reply = enqueue(&sender, destination);
        step(&mut h, vec![]);
        let response = reply.try_recv().expect("selection request must finish before Table layout");
        assert_eq!(response["ok"], json!(true), "{response:?}");
        assert_eq!(h.state().session.active_index(), Some(0));
        assert_eq!(h.state().session.documents()[0].uid, baseline[0].uid, "same document");
        assert_eq!(h.state().session.documents()[0].selection, expected.selection, "actual other-table selection");
        assert!(h.ctx.input(|i| !i.pointer.primary_down() && !i.pointer.primary_pressed() && !i.pointer.primary_released()));
        assert_eq!(rows(&h), geometry, "stable real IDs, bounds and layer after selection");
        assert_state(&h.state().session.documents()[1], &baseline[1], "selection isolation: B unchanged");
        assert_state(&h.state().session.documents()[0], &expected, "SELECTION ISOLATION: no endpoint edit of other table");
        for _ in 0..2 {
            step(&mut h, vec![]);
            assert_eq!(h.state().session.active_index(), Some(0));
            assert_eq!(rows(&h), geometry);
            assert_state(&h.state().session.documents()[0], &expected, "selection isolation: no delayed replay");
            assert_state(&h.state().session.documents()[1], &baseline[1], "selection isolation: B stays unchanged");
        }
    }
    #[test]
    fn checkbox_completed_merged_column_normalization_preserves_both_endpoints() {
        use designcraft_doc::{CellAddr, CellRange, Selection, TableSel, TextSel};

        // Local comparator; shared assert_state/assert_history remain unchanged.
        // Current Selection, all model fields, all Redo entries and the baseline
        // Undo prefix stay exact. Only newly appended A Undo Selections are omitted.
        fn assert_merged_a(actual: &DocState, expected: &DocState, baseline_undo_len: usize, context: &str) {
            eprintln!(
                "{context}: uid={}, flags={:?}, revision={}, undo={}, redo={}; expected flags={:?}, revision={}, undo={}, redo={}",
                actual.uid,
                flags(actual),
                actual.revision,
                actual.history.undo.len(),
                actual.history.redo.len(),
                flags(expected),
                expected.revision,
                expected.history.undo.len(),
                expected.history.redo.len()
            );
            assert_eq!(actual.uid, expected.uid, "{context}: document identity");
            assert_eq!(*actual.doc, *expected.doc, "{context}: exact model");
            assert_eq!(actual.selection, expected.selection, "{context}: exact selection");
            assert_eq!(actual.revision, expected.revision, "{context}: revision");
            assert_eq!(actual.saved_revision, expected.saved_revision, "{context}: saved revision");
            assert_eq!(*actual.saved_doc, *expected.saved_doc, "{context}: saved document");
            assert_eq!(actual.active_layer, expected.active_layer, "{context}: active layer");
            assert_eq!(actual.path, expected.path, "{context}: path");
            assert_eq!(actual.editing_parents, expected.editing_parents, "{context}: parent editing");
            assert_eq!(actual.preview_stash, expected.preview_stash, "{context}: preview document");
            assert_eq!(actual.preview_record, expected.preview_record, "{context}: preview record");
            match (&actual.fonts, &expected.fonts) {
                (None, None) => {}
                (Some(a), Some(e)) => assert!(Arc::ptr_eq(a, e), "{context}: font scope lifetime"),
                _ => panic!("{context}: font scope presence"),
            }
            assert_eq!(actual.history.limit, expected.history.limit, "{context}: history limit");
            assert!(actual.interaction.is_none() && expected.interaction.is_none());
            assert!(baseline_undo_len <= expected.history.undo.len(), "{context}: baseline prefix exists");
            assert_eq!(actual.history.undo.len(), expected.history.undo.len(), "{context}: Undo count");
            for (index, (a, e)) in actual.history.undo.iter().zip(&expected.history.undo).enumerate() {
                assert_eq!(a.label, e.label, "{context}: Undo {index} label");
                assert_eq!(*a.doc, *e.doc, "{context}: Undo {index} document snapshot");
                if index < baseline_undo_len {
                    assert_eq!(a.selection, e.selection, "{context}: baseline Undo {index} Selection");
                }
                // History entries may capture the caret before or after engine
                // normalization; checkbox assignment must support both.
            }
            assert_history(&actual.history.redo, &expected.history.redo, context);
        }

        for covered in [false, true] {
            let case = if covered { "covered-start" } else { "canonical-start control" };
            eprintln!("merged normalization BEGIN: {case}");
            let (mut h, original, _) = fixture();
            let original_text = original[0].selection.text.unwrap();
            let table_id = original_text.cell.unwrap().table;
            let owner = CellAddr { table: table_id, row: 0, col: 0 };
            let covered_cell = CellAddr { table: table_id, row: 0, col: 1 };
            // fixture has one header plus two body rows. The selected column includes
            // the covered header slot but must retain the original grid rectangle.
            let column = TableSel { story: original_text.story, table: table_id, range: CellRange { r0: 0, c0: 1, r1: 2, c1: 1 } };
            let covered_text = TextSel { story: original_text.story, anchor: 0, focus: 0, frame: original_text.frame, cell: Some(covered_cell) };
            let covered_selection = Selection { text: Some(covered_text), cells: Some(column), ..Default::default() };
            let canonical_text = TextSel { cell: Some(owner), ..covered_text };
            let canonical_selection = Selection { text: Some(canonical_text), ..covered_selection.clone() };
            {
                let app = h.state_mut();
                app.run("table.merge", json!({"rows": [0, 0], "cols": [0, 1]})).unwrap();
                app.run("table.select", json!({"rows": [1, 1], "cols": [1, 1]})).unwrap();
                app.run("table.selectColumn", json!({})).unwrap();
                assert_eq!(app.session.documents()[0].selection, covered_selection, "{case}: real covered-column setup");
                if !covered {
                    // Normalize through the supported command BEFORE the gesture.
                    // No flag changes and no direct write to the actual Selection.
                    app.run("table.options", json!({"repeatHeader": false})).unwrap();
                }
            }
            let expected_start = if covered { &covered_selection } else { &canonical_selection };
            let baseline = [h.state().session.documents()[0].clone(), original[1].clone()];
            let baseline_undo_len = baseline[0].history.undo.len();
            assert_eq!(h.state().session.documents().len(), 2);
            assert_eq!(h.state().session.active_index(), Some(0));
            assert_eq!(baseline[0].uid, original[0].uid);
            assert_eq!(&baseline[0].selection, expected_start, "{case}: exact starting Selection");
            assert_eq!(flags(&baseline[0]), (false, false));
            {
                let table = baseline[0].doc.story(original_text.story).unwrap().tables.get(&table_id).unwrap();
                assert_eq!((table.nrows(), table.ncols(), table.header_rows()), (3, 2, 1));
                assert_eq!(table.regions(), vec![(0, 0, 1, 2)], "{case}: one actual horizontal merge");
                assert_eq!(table.owner(0, 1), (0, 0));
                assert_eq!(table.cell(0, 0).unwrap().text.text, "");
            }
            assert_state(&h.state().session.documents()[1], &original[1], &format!("{case}: setup leaves B unchanged"));

            // Model-neutral setup frames clear setup-command cancellation and make
            // the current panel rows visible, preserving the existing layer guard.
            step(&mut h, vec![]);
            h.get_by_label("Repeat Header Rows").scroll_to_me();
            h.run_steps(3);
            for (i, expected) in baseline.iter().enumerate() {
                assert_state(&h.state().session.documents()[i], expected, &format!("{case}: setup scroll is model-neutral"));
            }
            assert!(!h.ctx.text_edit_focused());
            assert!(h.state().ui.dialog.is_none() && h.state().ui.palette.is_none());
            let geometry = rows(&h);

            // Literal origin oracle: the existing helper changes only Repeat, story
            // rev, document rev and history. Add the specified canonical TextSel;
            // preserve the grid rectangle. New Undo Selection values supplied by
            // expected_edit are deliberately NOT compared by assert_merged_a.
            // Do NOT call completed_batch: its unchanged-Selection oracle is wrong here.
            let mut origin_only = expected_edit(&baseline[0], false);
            origin_only.selection = canonical_selection.clone();
            assert_eq!(origin_only.history.undo.len(), baseline_undo_len + 1);
            assert_eq!(origin_only.selection.cells, Some(column));
            let settled = expected_edit(&origin_only, true);
            assert_eq!(settled.history.undo.len(), baseline_undo_len + 2);

            let p = geometry.rects[1].center(); // later-drawn Repeat Header Rows
            let q = geometry.rects[0].center(); // earlier-drawn Alternating Row Fills
            assert!(!geometry.rects[1].contains(q));
            step(&mut h, vec![Event::PointerMoved(p), pointer(p, true), Event::PointerMoved(q), pointer(q, false)]);
            assert!(h.ctx.input(|i| i.pointer.primary_pressed() && i.pointer.primary_released() && !i.pointer.primary_down()));
            assert_eq!(h.state().session.active_index(), Some(0));
            assert_eq!(rows(&h), geometry, "{case}: batch keeps IDs, bounds and layer");
            let actual = &h.state().session.documents()[0];
            assert_eq!(actual.selection.text, Some(canonical_text), "{case}: origin canonicalizes the caret");
            assert_eq!(actual.selection.cells, Some(column), "{case}: origin preserves the selected column");
            assert_merged_a(actual, &origin_only, baseline_undo_len, &format!("{case}: FIRST literal origin and normalization"));
            assert_eq!(flags(actual), (false, true), "{case}: FIRST is Repeat only");
            assert_state(&h.state().session.documents()[1], &baseline[1], &format!("{case}: FIRST B untouched"));
            eprintln!("merged normalization FIRST literal state accepted: {case}");

            // No commands or new input between completion and its next layout pass.
            // Require both literal edits, then one more empty frame with no replay.
            for pass in 1..=2 {
                step(&mut h, vec![]);
                assert!(h.ctx.input(|i| !i.pointer.primary_down() && !i.pointer.primary_pressed() && !i.pointer.primary_released()));
                assert_eq!(h.state().session.active_index(), Some(0));
                assert_eq!(rows(&h), geometry, "{case}: settle pass {pass} keeps actual geometry");
                assert_state(&h.state().session.documents()[1], &baseline[1], &format!("{case}: settle {pass} B unchanged"));
                assert_merged_a(
                    &h.state().session.documents()[0],
                    &settled,
                    baseline_undo_len,
                    &format!("{case}: settle {pass} requires BOTH endpoints exactly once"),
                );
                assert_eq!(flags(&h.state().session.documents()[0]), (true, true), "{case}: settle {pass} has BOTH flags");
            }
            eprintln!("merged normalization PASS: {case}");
        }
    }
}
