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
        caption(ui, crate::i18n::tr(&app.ui.language, "Columns"));
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
    if ui.checkbox(&mut alt, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Alternating Row Fills"))).changed() {
        let v = if alt { json!({"first": 1, "firstColor": "[Black]", "firstTint": 0.1, "next": 1, "nextColor": "[None]"}) } else { Value::Null };
        run(app, "table.options", json!({"altRows": v}));
    }
    let mut rep = info["options"]["repeatHeader"].as_bool().unwrap_or(true);
    if ui.checkbox(&mut rep, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Repeat Header Rows"))).changed() {
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
