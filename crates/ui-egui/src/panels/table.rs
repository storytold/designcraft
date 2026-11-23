//! The Table panel: row/column counts, row height, column width, cell text alignment and insets,
//! merge/unmerge — for the table at the text insertion point or the selected cells.

use designcraft_geom::Unit;
use serde_json::{Value, json};

use crate::DesignApp;
use crate::theme::Tokens;
use crate::widgets::{caption, divider, measure, number};

/// App-owned table identity and the existing installed-runtime boundary. The SDK owns input,
/// focus, hit testing and control styling; command/selection/Undo policy remains below.
struct Actions {
    framework: bool,
    identity: (Option<u64>, Option<u64>, Option<u64>, Option<u64>),
}

impl Actions {
    fn new(app: &DesignApp, info: &Value) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let framework = app.framework_installed;
        #[cfg(target_arch = "wasm32")]
        let framework = false;
        Self {
            framework,
            identity: (
                app.session.active().map(|st| st.uid),
                app.session.active().map(|st| st.identity_epoch),
                info["story"].as_u64(),
                info["table"].as_u64(),
            ),
        }
    }

    fn button(
        &self,
        ui: &mut egui::Ui,
        key: &'static str,
        label: &str,
        (enabled, selected): (bool, bool),
        tooltip: Option<&str>,
        legacy: impl FnOnce(&mut egui::Ui) -> egui::Response,
    ) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        if self.framework {
            use craft_ui_widgets::buttons::Button;
            use craft_ui_widgets::contract::{Props as _, Show as _};

            let button = Button::new(label.to_owned()).id_salt(("table-action", self.identity, key)).enabled(enabled).selected(selected);
            return match tooltip {
                Some(tooltip) => button.tooltip(tooltip.to_owned()).show(ui).clicked(),
                None => button.show(ui).clicked(),
            };
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (self.framework, self.identity, key, label, enabled, selected, tooltip);
        legacy(ui).clicked()
    }

    fn check(&self, ui: &mut egui::Ui, key: &'static str, label: &str, value: &mut bool) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        if self.framework {
            use craft_ui_widgets::choice::Checkbox;
            use craft_ui_widgets::contract::{Bind, Props as _, Show as _};

            let out = Checkbox::new(Bind::given(*value)).id_salt(("table-option", self.identity, key)).label(label.to_owned()).show(ui);
            if let Some(after) = out.events.committed() {
                *value = *after;
                return true;
            }
            return false;
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (self.framework, self.identity, key);
        ui.checkbox(value, crate::rtl::widget(ui, label)).changed()
    }
}

// TABLE_STYLE_CONTROLS_BEGIN
/// A choice belongs to the actual inclusive cell range, never the row's ordinal or current focus.
/// Revision and ordered catalog binding also discard open/held input after edits or reordering.
fn table_control_identity(app: &DesignApp, ui: &egui::Ui, info: &Value, key: &str, choices: &[(String, String)], current: &str) -> (egui::Id, bool) {
    let owner = app.session.active().map(|st| (st.uid, st.identity_epoch, st.revision));
    let story = info["story"].as_u64();
    let table = info["table"].as_u64();
    let range = [info["range"]["r0"].as_u64(), info["range"]["r1"].as_u64(), info["range"]["c0"].as_u64(), info["range"]["c1"].as_u64()];
    let valid = app.session.active().is_some_and(|st| {
        let (Some(story), Some(table), [Some(r0), Some(r1), Some(c0), Some(c1)]) = (story, table, range) else { return false };
        st.doc
            .story(designcraft_doc::StoryId(story))
            .and_then(|story| story.tables.get(&table))
            .is_some_and(|table| r0 <= r1 && c0 <= c1 && r1 < table.rows.len() as u64 && c1 < table.columns.len() as u64)
    });
    (ui.make_persistent_id(("table-style-control", owner, story, table, range, key, choices, current)), valid)
}

fn table_committed_choice(
    app: &DesignApp,
    ui: &mut egui::Ui,
    info: &Value,
    key: &str,
    label: &str,
    choices: &[(String, String)],
    current: &str,
    width: f32,
) -> Option<String> {
    let (id, valid) = table_control_identity(app, ui, info, key, choices, current);
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        use craft_ui_widgets::contract::{Bind, Entry, Props as _, Show as _};
        let entries: Vec<_> = choices.iter().map(|(value, label)| Entry::new(value.clone(), label.clone())).collect();
        // An orphaned current name is only a placeholder, not an invented enabled catalog entry.
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 20.0), egui::Sense::hover());
        return craft_ui_widgets::pickers::Select::new(label.to_owned(), Bind::given(current.to_owned()), &entries)
            .id(id)
            .enabled(valid && !entries.is_empty())
            .size(craft_ui_widgets::style::ControlSize::Sm)
            .placeholder(current.to_owned())
            .show_at(ui, rect)
            .events
            .committed()
            .cloned()
            .filter(|value| choices.iter().any(|(actual, _)| actual == value));
    }
    let mut committed = None;
    ui.add_enabled_ui(valid && !choices.is_empty(), |ui| {
        let selected = choices.iter().find(|(value, _)| value == current).map_or(current, |(_, label)| label.as_str());
        let combo = craft_ui_legacy::ComboBox::from_id_salt(id).selected_text(crate::rtl::widget(ui, selected)).width(width).show_ui(ui, |ui| {
            for (value, label) in choices {
                if ui.selectable_label(value == current, crate::rtl::widget(ui, label)).clicked() {
                    committed = Some(value.clone());
                }
            }
        });
        let _ = crate::widgets::name_combo(combo, label);
    });
    committed
}

#[cfg(not(target_arch = "wasm32"))]
struct TableStylePlus;
#[cfg(not(target_arch = "wasm32"))]
impl craft_ui_widgets::primitives::Glyph for TableStylePlus {
    fn paint(&self, painter: &egui::Painter, _tokens: &craft_ui_widgets::style::UiTokens, rect: egui::Rect, color: egui::Color32, _mirrored: bool) {
        crate::icons::paint(painter, rect, "plus", color);
    }
}

fn table_new_style(app: &DesignApp, ui: &mut egui::Ui, info: &Value, kind: &str, choices: &[(String, String)], current: &str, label: &str) -> bool {
    let (id, valid) = table_control_identity(app, ui, info, &format!("new-{kind}"), choices, current);
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        use craft_ui_widgets::contract::{Props as _, Show as _};
        let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
        return craft_ui_widgets::buttons::IconButton::new(craft_ui_widgets::primitives::IconSpec::glyph(&TableStylePlus))
            .id(id)
            .label(label.to_owned())
            .tooltip(label.to_owned())
            .enabled(valid)
            .size(craft_ui_widgets::style::ControlSize::Sm)
            .show_at(ui, rect)
            .clicked();
    }
    ui.push_id(id, |ui| ui.add_enabled_ui(valid, |ui| crate::icons::button(ui, "plus", 20.0, false, label).clicked()).inner).inner
}
// TABLE_STYLE_CONTROLS_END

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let Ok(info) = app.session.execute("table.get", &json!({})) else {
        ui.label(crate::rtl::widget(
            ui,
            egui::RichText::new(crate::i18n::tr(&app.ui.language, "Place the insertion point in a table, or create one.")).color(t.text_dim),
        ));
        ui.add_space(6.0);
        let actions = Actions::new(app, &Value::Null);
        let label = crate::i18n::tr(&app.ui.language, "Create Table…");
        if actions.button(ui, "create", label, (true, false), None, |ui| ui.button(crate::rtl::widget(ui, label))) {
            let _ = app.run("app.insertTableDialog", json!({}));
        }
        return;
    };
    let actions = Actions::new(app, &info);
    let unit = app.session.active().map(|d| d.doc.settings.horizontal_units).unwrap_or(Unit::Picas);
    let rows = info["rows"].as_array().cloned().unwrap_or_default();
    let cols = info["columns"].as_array().cloned().unwrap_or_default();
    let range = info["range"].clone();
    let (r0, c0) = (range["r0"].as_u64().unwrap_or(0) as usize, range["c0"].as_u64().unwrap_or(0) as usize);
    let sid = info["story"].clone();
    let tid = info["table"].clone();
    let cell = info["cells"].as_array().and_then(|cs| cs.iter().find(|c| c["row"] == r0 && c["col"] == c0)).cloned().unwrap_or(Value::Null);
    let run = |app: &mut DesignApp, id: &str, p: Value| {
        let _ = app.run(id, p);
    };
    ui.horizontal(|ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Cell Type"));
        if actions.button(ui, "graphic-cell", crate::i18n::tr(&app.ui.language, "Graphic…"), (true, false), None, |ui| {
            ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Graphic…")))
        }) && let Err(e) = app.run("app.graphicCell", json!({}))
        {
            app.status_error("Table", e);
        }
        if actions.button(ui, "text-cell", crate::i18n::tr(&app.ui.language, "Text"), (true, false), None, |ui| {
            ui.small_button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Text")))
        }) {
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
            if actions.button(ui, label, crate::i18n::tr(&app.ui.language, label), (ok, false), None, |ui| {
                ui.add_enabled(ok, egui::Button::new(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label))).small())
            }) && let Err(e) = app.run(id, json!({"from": from, "to": to}))
            {
                app.status_error("Table", e);
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
                let label = crate::i18n::tr(&app.ui.language, label).to_owned();
                caption(ui, &label);
                let choices: Vec<_> = names.into_iter().map(|name| (name.clone(), name)).collect();
                if let Some(name) = table_committed_choice(app, ui, &info, kind, &label, &choices, &cur, 130.0) {
                    run(
                        app,
                        &format!("style.{kind}.apply"),
                        json!({"name": name, "story": sid, "table": tid, "rows": [range["r0"], range["r1"]], "cols": [range["c0"], range["c1"]]}),
                    );
                }
                let new_label = crate::i18n::tr(&app.ui.language, if kind == "cell" { "New Cell Style" } else { "New Table Style" }).to_owned();
                if table_new_style(app, ui, &info, kind, &choices, &cur, &new_label) {
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
                    json!({"story": sid, "table": tid, "rows": [rows.len() - 1, rows.len() - 1], "where": "below", "count": n - rows.len()}),
                );
            } else if n < rows.len() {
                run(app, "table.deleteRow", json!({"story": sid, "table": tid, "rows": [n, rows.len() - 1]}));
            }
        }
        caption(ui, crate::i18n::tr_context(&app.ui.language, "Columns", "table"));
        if let Some(v) = number(ui, "tp_cols", Some(cols.len() as f64), "", 52.0, 0) {
            let n = v.round().max(1.0) as usize;
            if n > cols.len() {
                run(
                    app,
                    "table.insertColumn",
                    json!({"story": sid, "table": tid, "cols": [cols.len() - 1, cols.len() - 1], "where": "right", "count": n - cols.len()}),
                );
            } else if n < cols.len() {
                run(app, "table.deleteColumn", json!({"story": sid, "table": tid, "cols": [n, cols.len() - 1]}));
            }
        }
        ui.end_row();
        let row = rows.get(r0).cloned().unwrap_or(Value::Null);
        let exactly = row["mode"] == "exactly";
        caption(ui, crate::i18n::tr(&app.ui.language, "Row Height"));
        let modes: Vec<_> = [("At Least", "atLeast"), ("Exactly", "exactly")]
            .into_iter()
            .map(|(label, mode)| (mode.to_owned(), crate::i18n::tr(&app.ui.language, label).to_owned()))
            .collect();
        let row_label = crate::i18n::tr(&app.ui.language, "Row Height").to_owned();
        if let Some(mode) =
            table_committed_choice(app, ui, &info, "row-height-mode", &row_label, &modes, if exactly { "exactly" } else { "atLeast" }, 70.0)
        {
            run(app, "table.setRowHeight", json!({"mode": mode}));
        }
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
    if actions.button(
        ui,
        "distribute-rows",
        crate::i18n::tr(&app.ui.language, "Distribute Rows Evenly"),
        (true, false),
        Some(crate::i18n::tr(&app.ui.language, "Equal fixed heights for the selected rows; cell text may become overset.")),
        |ui| {
            ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Distribute Rows Evenly")))
                .on_hover_text(crate::i18n::tr(&app.ui.language, "Equal fixed heights for the selected rows; cell text may become overset."))
        },
    ) && let Err(e) = app.run("table.distributeRows", json!({}))
    {
        app.status_error("Table", e);
    }
    divider(ui);
    caption(ui, crate::i18n::tr(&app.ui.language, "Cell Text"));
    ui.horizontal(|ui| {
        for (label, vj) in [("Top", "top"), ("Center", "center"), ("Bottom", "bottom"), ("Justify", "justify")] {
            if actions.button(ui, vj, crate::i18n::tr(&app.ui.language, label), (true, cell["vj"] == vj), None, |ui| {
                ui.selectable_label(cell["vj"] == vj, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label)))
            }) {
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
        if actions.button(ui, "merge", crate::i18n::tr(&app.ui.language, "Merge Cells"), (true, false), None, |ui| {
            ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Merge Cells")))
        }) {
            run(app, "table.merge", json!({}));
        }
        if actions.button(ui, "unmerge", crate::i18n::tr(&app.ui.language, "Unmerge"), (true, false), None, |ui| {
            ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Unmerge")))
        }) {
            run(app, "table.unmerge", json!({}));
        }
    });
    ui.horizontal(|ui| {
        if actions.button(ui, "insert-row", crate::i18n::tr(&app.ui.language, "Insert Row"), (true, false), None, |ui| {
            ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Insert Row")))
        }) {
            run(app, "table.insertRowBelow", json!({}));
        }
        if actions.button(ui, "insert-column", crate::i18n::tr(&app.ui.language, "Insert Column"), (true, false), None, |ui| {
            ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Insert Column")))
        }) {
            run(app, "table.insertColumnRight", json!({}));
        }
    });
    ui.horizontal(|ui| {
        if actions.button(ui, "delete-row", crate::i18n::tr(&app.ui.language, "Delete Row"), (true, false), None, |ui| {
            ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete Row")))
        }) {
            run(app, "table.deleteRow", json!({}));
        }
        if actions.button(ui, "delete-column", crate::i18n::tr(&app.ui.language, "Delete Column"), (true, false), None, |ui| {
            ui.button(crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, "Delete Column")))
        }) {
            run(app, "table.deleteColumn", json!({}));
        }
    });
    divider(ui);
    // Column order: right to left puts the first column on the right.
    let rtl = info["options"]["direction"] == "rightToLeft";
    ui.horizontal(|ui| {
        caption(ui, crate::i18n::tr(&app.ui.language, "Direction"));
        for (on, label, dir) in [(!rtl, "Left to Right", "leftToRight"), (rtl, "Right to Left", "rightToLeft")] {
            if actions.button(ui, dir, crate::i18n::tr(&app.ui.language, label), (true, on), None, |ui| {
                ui.selectable_label(on, crate::rtl::widget(ui, crate::i18n::tr(&app.ui.language, label)))
            }) && !on
            {
                run(app, "table.options", json!({"direction": dir}));
            }
        }
    });
    let mut alt = !info["options"]["altRows"].is_null();
    if actions.check(ui, "alternating-rows", crate::i18n::tr(&app.ui.language, "Alternating Row Fills"), &mut alt) {
        let v = if alt { json!({"first": 1, "firstColor": "[Black]", "firstTint": 0.1, "next": 1, "nextColor": "[None]"}) } else { Value::Null };
        run(app, "table.options", json!({"altRows": v}));
    }
    let mut rep = info["options"]["repeatHeader"].as_bool().unwrap_or(true);
    if actions.check(ui, "repeat-header", crate::i18n::tr(&app.ui.language, "Repeat Header Rows"), &mut rep) {
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod installed_action_tests {
    use super::*;
    use craft_ui_widgets::runtime::inspect::{Element, Recorded};
    use egui::{Event, Key, Modifiers, PointerButton};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use std::sync::Arc;

    struct State {
        app: DesignApp,
        recorded: Option<Arc<Recorded>>,
    }

    fn text_app() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let frame = app.run("frame.create", json!({"rect": [72, 72, 500, 700], "content": "text", "text": ""})).unwrap();
        app.run("text.select", json!({"story": frame["story"], "anchor": 0, "focus": 0})).unwrap();
        app
    }

    fn table_app() -> (DesignApp, Value) {
        let mut app = text_app();
        let made = app.run("table.insert", json!({"rows": 3, "cols": 2})).unwrap();
        for (r, c, text) in [(0, 0, "first"), (1, 0, "second"), (2, 0, "third"), (0, 1, "other")] {
            app.run("table.setCell", json!({"row": r, "col": c, "text": text})).unwrap();
        }
        select(&mut app, &made, [0, 0], [0, 0]);
        (app, made)
    }

    fn select(app: &mut DesignApp, made: &Value, rows: [usize; 2], cols: [usize; 2]) {
        app.run("table.select", json!({"story": made["story"], "table": made["table"], "rows": rows, "cols": cols})).unwrap();
    }

    fn table(app: &DesignApp, made: &Value) -> designcraft_doc::Table {
        let story = designcraft_doc::StoryId(made["story"].as_u64().unwrap());
        let id = made["table"].as_u64().unwrap();
        (**app.session.active().unwrap().doc.story(story).unwrap().tables.get(&id).unwrap()).clone()
    }

    fn harness(mut app: DesignApp, installed: bool) -> Harness<'static, State> {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, "");
        if installed {
            app.framework_opacity = true;
            app.install_framework(&ctx);
            assert!(app.framework_installed);
        }
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        let mut h = Harness::builder().with_context(ctx).with_size(egui::vec2(640.0, 1000.0)).build_ui_state(
            |ui, state: &mut State| {
                let region = ui.scope_id();
                let (_, recorded) = craft_ui_widgets::runtime::inspect::record(ui, region, |ui| show(&mut state.app, ui));
                if recorded.is_some() {
                    state.recorded = recorded;
                }
            },
            State { app, recorded: None },
        );
        h.input_mut().max_texture_side = Some(8192);
        steps(&mut h, 3);
        h
    }

    fn steps(h: &mut Harness<'_, State>, count: usize) {
        for _ in 0..count {
            let mut raw = std::mem::take(h.input_mut());
            let ctx = h.ctx.clone();
            h.state_mut().app.raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
    }

    fn event(h: &mut Harness<'_, State>, event: Event) {
        h.input_mut().events.push(event);
        steps(h, 1);
    }

    fn key(h: &mut Harness<'_, State>, key: Key) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
        steps(h, 2);
    }

    fn click_at(h: &mut Harness<'_, State>, pos: egui::Pos2) {
        event(h, Event::PointerMoved(pos));
        for pressed in [true, false] {
            event(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
        steps(h, 2);
    }

    fn element(h: &Harness<'_, State>, component: &str, label: &str) -> Element {
        let name = crate::i18n::tr(&h.state().app.ui.language, label);
        let found: Vec<_> = h
            .state()
            .recorded
            .as_ref()
            .unwrap()
            .elements
            .iter()
            .filter(|element| element.component == component && element.facts.as_ref().is_some_and(|facts| facts.name.as_deref() == Some(name)))
            .cloned()
            .collect();
        assert_eq!(found.len(), 1, "one actual {component} named {name}");
        found[0].clone()
    }

    fn click(h: &mut Harness<'_, State>, component: &str, label: &str) {
        let target = element(h, component, label);
        let rect = h.state().recorded.as_ref().unwrap().hits.iter().find(|hit| hit.owner == target.id).map_or(target.rect, |hit| hit.rect);
        click_at(h, rect.center());
    }

    fn tab_to(h: &mut Harness<'_, State>, component: &str, label: &str) {
        // Bound traversal by this rendered fixture's actual stops, including retained legacy fields.
        let native = h.state().recorded.as_ref().unwrap().elements.iter().filter(|element| element.stop.is_some()).count();
        let legacy = h.query_all_by(|node| matches!(node.role(), egui::accesskit::Role::TextInput | egui::accesskit::Role::ComboBox)).count();
        let role = if component == "Checkbox" { egui::accesskit::Role::CheckBox } else { egui::accesskit::Role::Button };
        let name = crate::i18n::tr(&h.state().app.ui.language, label).to_owned();
        for _ in 0..=native + legacy {
            if h.query_by(|node| node.role() == role && node.label().as_deref() == Some(name.as_str())).is_some_and(|node| node.is_focused()) {
                return;
            }
            key(h, Key::Tab);
        }
        panic!("natural Tab did not reach {name} in one rendered focus cycle");
    }

    fn calls(h: &Harness<'_, State>, from: usize, id: &str) -> Vec<Value> {
        h.state().app.session.journal[from..].iter().filter(|(command, _)| command == id).map(|(_, params)| params.clone()).collect()
    }

    fn undo_len(h: &Harness<'_, State>) -> usize {
        h.state().app.session.active().unwrap().history.undo.len()
    }

    fn undo_once(h: &mut Harness<'_, State>) {
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        steps(h, 2);
    }

    #[test]
    fn installed_empty_table_panel_opens_create_without_editing_and_refuses_missing_text() {
        let mut h = harness(text_app(), true);
        let before = h.state().app.session.active().unwrap().doc.clone();
        let undo = undo_len(&h);
        let journal = h.state().app.session.journal.len();
        tab_to(&mut h, "Button", "Create Table…");
        key(&mut h, Key::Enter);
        assert_eq!(h.state().app.ui.dialog.as_ref().unwrap().id, "insertTable");
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
        assert_eq!(undo_len(&h), undo);
        assert!(calls(&h, journal, "table.insert").is_empty(), "opening is not creating a table");

        let mut app = text_app();
        app.run("selection.set", json!({"ids": []})).unwrap();
        let mut h = harness(app, true);
        let before = h.state().app.session.active().unwrap().doc.clone();
        let undo = undo_len(&h);
        click(&mut h, "Button", "Create Table…");
        assert!(h.state().app.ui.dialog.is_none(), "same command refuses an absent text insertion point");
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &before));
        assert_eq!(undo_len(&h), undo);
    }

    #[test]
    fn installed_boundary_moves_are_inert_and_eligible_pointer_and_keyboard_moves_target_one_table() {
        let (mut app, made) = table_app();
        let other_frame = app.run("frame.create", json!({"rect": [72, 72, 500, 700], "content": "text", "text": ""})).unwrap();
        app.run("text.select", json!({"story": other_frame["story"], "anchor": 0, "focus": 0})).unwrap();
        let other = app.run("table.insert", json!({"rows": 2, "cols": 2})).unwrap();
        select(&mut app, &made, [0, 0], [0, 0]);
        let untouched = table(&app, &other);
        let mut h = harness(app, true);
        let before = table(&h.state().app, &made);
        let doc = h.state().app.session.active().unwrap().doc.clone();
        let revision = h.state().app.session.active().unwrap().revision;
        let undo = undo_len(&h);
        let journal = h.state().app.session.journal.len();
        for label in ["Row ↑", "Col ←"] {
            assert!(element(&h, "Button", label).facts.as_ref().unwrap().disabled);
            click(&mut h, "Button", label);
        }
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &doc));
        assert_eq!(h.state().app.session.active().unwrap().revision, revision);
        assert_eq!(undo_len(&h), undo);
        assert!(calls(&h, journal, "table.moveRow").is_empty());
        assert!(calls(&h, journal, "table.moveColumn").is_empty());
        click(&mut h, "Button", "Row ↓");
        assert_eq!(table(&h.state().app, &made).cell(0, 0).unwrap().text.text, "second");
        assert_eq!(calls(&h, journal, "table.moveRow"), [json!({"from": 0, "to": 1})]);
        assert_eq!(undo_len(&h), undo + 1);
        steps(&mut h, 3);
        assert_eq!(calls(&h, journal, "table.moveRow").len(), 1);
        assert_eq!(table(&h.state().app, &other), untouched);
        undo_once(&mut h);
        assert_eq!(table(&h.state().app, &made), before);
        tab_to(&mut h, "Button", "Col →");
        key(&mut h, Key::Space);
        assert_eq!(calls(&h, journal, "table.moveColumn"), [json!({"from": 0, "to": 1})]);
        assert_eq!(table(&h.state().app, &made).cell(0, 0).unwrap().text.text, "other");
        assert_eq!(table(&h.state().app, &other), untouched);
        undo_once(&mut h);
        assert_eq!(table(&h.state().app, &made), before);
        h.state_mut().app.run("table.select", json!({"story": made["story"], "table": made["table"], "rows": [2, 2], "cols": [1, 1]})).unwrap();
        steps(&mut h, 2);
        assert!(element(&h, "Button", "Row ↓").facts.as_ref().unwrap().disabled);
        assert!(element(&h, "Button", "Col →").facts.as_ref().unwrap().disabled);
    }

    #[test]
    fn installed_insert_delete_merge_and_unmerge_keep_command_payloads_and_one_step_undo() {
        for (label, command) in [
            ("Insert Row", "table.insertRowBelow"),
            ("Insert Column", "table.insertColumnRight"),
            ("Delete Row", "table.deleteRow"),
            ("Delete Column", "table.deleteColumn"),
            ("Merge Cells", "table.merge"),
            ("Unmerge", "table.unmerge"),
        ] {
            let (mut app, made) = table_app();
            if matches!(command, "table.merge" | "table.unmerge") {
                select(&mut app, &made, [0, 1], [0, 1]);
            } else {
                select(&mut app, &made, [0, 0], [0, 0]);
            }
            if command == "table.unmerge" {
                app.run("table.merge", json!({})).unwrap();
            }
            let mut h = harness(app, true);
            let before = table(&h.state().app, &made);
            let undo = undo_len(&h);
            let journal = h.state().app.session.journal.len();
            click(&mut h, "Button", label);
            let after = table(&h.state().app, &made);
            assert_ne!(after, before, "{label} changes the actual table");
            assert_eq!(after.id, before.id);
            assert_eq!(calls(&h, journal, command), [json!({})], "{label}");
            assert_eq!(undo_len(&h), undo + 1, "{label} is one Undo");
            steps(&mut h, 3);
            assert_eq!(calls(&h, journal, command).len(), 1, "idle cannot repeat {label}");
            undo_once(&mut h);
            assert_eq!(table(&h.state().app, &made), before, "Undo restores {label}");
        }
    }

    #[test]
    fn installed_distribution_uses_selected_rows_and_retains_engine_refusal_status() {
        let (mut app, made) = table_app();
        for (row, height) in [(0, 40), (1, 80), (2, 30)] {
            app.run("table.setRowHeight", json!({"row": row, "height": height, "mode": "exactly"})).unwrap();
        }
        select(&mut app, &made, [0, 1], [0, 1]);
        let selected = app.session.active().unwrap().selection.cells;
        let mut h = harness(app, true);
        let before = table(&h.state().app, &made);
        let undo = undo_len(&h);
        let journal = h.state().app.session.journal.len();
        tab_to(&mut h, "Button", "Distribute Rows Evenly");
        key(&mut h, Key::Enter);
        let after = table(&h.state().app, &made);
        assert_eq!(after.rows.iter().map(|row| row.height).collect::<Vec<_>>(), [60.0, 60.0, 30.0]);
        assert_eq!(after.cells, before.cells);
        assert_eq!(h.state().app.session.active().unwrap().selection.cells, selected);
        assert_eq!(calls(&h, journal, "table.distributeRows"), [json!({})]);
        assert_eq!(undo_len(&h), undo + 1);
        undo_once(&mut h);
        assert_eq!(table(&h.state().app, &made), before);
        h.state_mut().app.run("table.merge", json!({"rows": [0, 1], "cols": [0, 0]})).unwrap();
        steps(&mut h, 2);
        let refused = table(&h.state().app, &made);
        let undo = undo_len(&h);
        let status = h.state().app.ui.status.clone();
        click(&mut h, "Button", "Distribute Rows Evenly");
        assert_eq!(table(&h.state().app, &made), refused);
        assert_eq!(undo_len(&h), undo);
        assert_ne!(h.state().app.ui.status, status, "the existing merged-row refusal is shown");
    }

    #[test]
    fn installed_direction_noop_and_option_toggles_keep_identity_payloads_and_undo() {
        let (app, made) = table_app();
        let mut h = harness(app, true);
        let before = table(&h.state().app, &made);
        let doc = h.state().app.session.active().unwrap().doc.clone();
        let undo = undo_len(&h);
        let journal = h.state().app.session.journal.len();
        let direction_id = element(&h, "Button", "Left to Right").id;
        click(&mut h, "Button", "Left to Right");
        assert!(calls(&h, journal, "table.options").is_empty());
        assert!(Arc::ptr_eq(&h.state().app.session.active().unwrap().doc, &doc));
        assert_eq!(undo_len(&h), undo);
        tab_to(&mut h, "Button", "Right to Left");
        key(&mut h, Key::Enter);
        assert_eq!(calls(&h, journal, "table.options"), [json!({"direction": "rightToLeft"})]);
        assert_eq!(undo_len(&h), undo + 1);
        assert_eq!(element(&h, "Button", "Left to Right").id, direction_id);
        undo_once(&mut h);
        assert_eq!(table(&h.state().app, &made), before);

        let journal = h.state().app.session.journal.len();
        click(&mut h, "Checkbox", "Alternating Row Fills");
        let alt = json!({"first": 1, "firstColor": "[Black]", "firstTint": 0.1, "next": 1, "nextColor": "[None]"});
        assert_eq!(calls(&h, journal, "table.options"), [json!({"altRows": alt})]);
        assert!(table(&h.state().app, &made).options.alt_rows.is_some());
        assert_eq!(undo_len(&h), undo + 1);
        click(&mut h, "Checkbox", "Alternating Row Fills");
        assert_eq!(calls(&h, journal, "table.options")[1], json!({"altRows": null}));
        assert!(table(&h.state().app, &made).options.alt_rows.is_none());
        let journal = h.state().app.session.journal.len();
        let repeat = table(&h.state().app, &made).options.repeat_header;
        tab_to(&mut h, "Checkbox", "Repeat Header Rows");
        key(&mut h, Key::Space);
        assert_eq!(calls(&h, journal, "table.options"), [json!({"repeatHeader": !repeat})]);
        assert_eq!(table(&h.state().app, &made).options.repeat_header, !repeat);
        steps(&mut h, 3);
        assert_eq!(calls(&h, journal, "table.options").len(), 1);
        undo_once(&mut h);
        assert_eq!(table(&h.state().app, &made).options.repeat_header, repeat);
    }

    #[test]
    fn installed_cell_graphic_request_is_nonediting_and_text_conversion_is_undoable() {
        let (mut app, made) = table_app();
        let png = designcraft_render::Rendered { width: 2, height: 2, pixels: [255, 0, 0, 255].repeat(4) }.to_png();
        app.run("table.placeGraphic", json!({"base64": designcraft_engine::cmd::base64_encode(&png), "name": "cell.png"})).unwrap();
        let mut h = harness(app, true);
        let before = table(&h.state().app, &made);
        assert!(before.cell(0, 0).unwrap().graphic.is_some());
        let undo = undo_len(&h);
        let journal = h.state().app.session.journal.len();
        tab_to(&mut h, "Button", "Text");
        key(&mut h, Key::Space);
        assert!(table(&h.state().app, &made).cell(0, 0).unwrap().graphic.is_none());
        assert_eq!(table(&h.state().app, &made).cell(0, 0).unwrap().text, before.cell(0, 0).unwrap().text);
        assert_eq!(calls(&h, journal, "table.textCell"), [json!({})]);
        assert_eq!(undo_len(&h), undo + 1);
        undo_once(&mut h);
        assert_eq!(table(&h.state().app, &made), before);
        click(&mut h, "Button", "Graphic…");
        assert_eq!(h.state().app.asked.len(), 1, "production request is queued, never shown by this headless fixture");
        assert_eq!(table(&h.state().app, &made), before);
        assert_eq!(undo_len(&h), undo);
        steps(&mut h, 3);
        assert_eq!(h.state().app.asked.len(), 1, "idle does not queue another dialog");
    }

    #[test]
    fn installed_cell_alignment_preserves_repeated_selection_command_policy_and_localized_names() {
        let (mut app, made) = table_app();
        app.ui.language = "de".into();
        select(&mut app, &made, [0, 1], [0, 1]);
        let mut h = harness(app, true);
        let before = table(&h.state().app, &made);
        let journal = h.state().app.session.journal.len();
        click(&mut h, "Button", "Bottom");
        assert_eq!(calls(&h, journal, "table.setCell"), [json!({"vj": "bottom"})]);
        let after = table(&h.state().app, &made);
        assert_ne!(after.cell(0, 0).unwrap().vj, before.cell(0, 0).unwrap().vj);
        assert_eq!(after.cell(0, 0).unwrap().vj, after.cell(1, 1).unwrap().vj);
        click(&mut h, "Button", "Bottom");
        assert_eq!(calls(&h, journal, "table.setCell").len(), 2, "unlike direction, existing cell alignment dispatches a repeated pick");
    }

    #[test]
    fn installed_press_does_not_cross_table_identity_or_a_new_disabled_boundary() {
        let (mut app, first) = table_app();
        let frame = app.run("frame.create", json!({"rect": [72, 72, 500, 700], "content": "text", "text": ""})).unwrap();
        app.run("text.select", json!({"story": frame["story"], "anchor": 0, "focus": 0})).unwrap();
        let second = app.run("table.insert", json!({"rows": 2, "cols": 2})).unwrap();
        app.run("table.setCell", json!({"row": 0, "col": 0, "text": "B first"})).unwrap();
        app.run("table.setCell", json!({"row": 1, "col": 0, "text": "B second"})).unwrap();
        select(&mut app, &first, [0, 0], [0, 0]);
        let mut h = harness(app, true);
        let first_before = table(&h.state().app, &first);
        let second_before = table(&h.state().app, &second);
        let journal = h.state().app.session.journal.len();
        let undo = undo_len(&h);
        let first_control = element(&h, "Button", "Row ↓");
        let pos = first_control.rect.center();
        event(&mut h, Event::PointerMoved(pos));
        event(&mut h, Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        select(&mut h.state_mut().app, &second, [0, 0], [0, 0]);
        steps(&mut h, 2);
        assert_ne!(element(&h, "Button", "Row ↓").id, first_control.id);
        event(&mut h, Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        steps(&mut h, 2);
        assert!(calls(&h, journal, "table.moveRow").is_empty(), "release has no authority in the new table");
        assert_eq!(undo_len(&h), undo);
        assert_eq!(table(&h.state().app, &first), first_before);
        assert_eq!(table(&h.state().app, &second), second_before);

        let pos = element(&h, "Button", "Row ↓").rect.center();
        event(&mut h, Event::PointerMoved(pos));
        event(&mut h, Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        select(&mut h.state_mut().app, &second, [1, 1], [0, 0]);
        steps(&mut h, 2);
        assert!(element(&h, "Button", "Row ↓").facts.as_ref().unwrap().disabled);
        event(&mut h, Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        steps(&mut h, 2);
        assert!(calls(&h, journal, "table.moveRow").is_empty(), "an enabled press cannot survive a disabled release");
        assert_eq!(undo_len(&h), undo);
        assert_eq!(table(&h.state().app, &second), second_before);
        select(&mut h.state_mut().app, &second, [0, 0], [0, 0]);
        steps(&mut h, 2);
        click(&mut h, "Button", "Row ↓");
        assert_eq!(calls(&h, journal, "table.moveRow"), [json!({"from": 0, "to": 1})]);
        assert_eq!(table(&h.state().app, &second).cell(0, 0).unwrap().text.text, "B second");
        assert_eq!(table(&h.state().app, &first), first_before);
        assert_eq!(undo_len(&h), undo + 1);
    }

    #[test]
    fn uninstalled_table_panel_keeps_existing_legacy_actions_and_boundary_disable() {
        let (app, made) = table_app();
        let mut h = harness(app, false);
        assert!(!h.state().app.framework_installed);
        assert!(h.get_by_label("Row ↑").accesskit_node().is_disabled());
        let before = table(&h.state().app, &made);
        let journal = h.state().app.session.journal.len();
        let rect = h.get_by_label("Insert Row").rect();
        click_at(&mut h, rect.center());
        assert_eq!(calls(&h, journal, "table.insertRowBelow"), [json!({})]);
        assert_eq!(table(&h.state().app, &made).nrows(), before.nrows() + 1);
        undo_once(&mut h);
        assert_eq!(table(&h.state().app, &made), before);
    }
}

// TABLE_STYLE_TESTS_BEGIN
#[cfg(all(test, not(target_arch = "wasm32")))]
mod style_control_tests {
    use super::*;
    use designcraft_doc::{Document, Selection, StoryId, Table};
    use egui::{Event, Key, Modifiers, PointerButton};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use std::sync::Arc;

    const TABLE_STYLE: &str = "Editorial table — complete second style";
    const CELL_STYLE: &str = "Editorial cell — complete second style";

    fn add_table(app: &mut DesignApp) -> Value {
        let frame = app.run("frame.create", json!({"rect": [72, 72, 500, 700], "content": "text", "text": ""})).unwrap();
        app.run("text.select", json!({"story": frame["story"], "anchor": 0, "focus": 0})).unwrap();
        app.run("table.insert", json!({"rows": 3, "cols": 3})).unwrap()
    }
    fn select(app: &mut DesignApp, made: &Value, rows: [usize; 2], cols: [usize; 2]) {
        app.run("table.select", json!({"story": made["story"], "table": made["table"], "rows": rows, "cols": cols})).unwrap();
    }
    fn fixture() -> (DesignApp, Value, Value) {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let made = add_table(&mut app);
        app.run("table.setCell", json!({"row": 1, "col": 1, "insets": [2, 3, 4, 5], "vj": "bottom", "text": "real source cell"})).unwrap();
        let other = add_table(&mut app);
        app.run("style.table.create", json!({"name": "Editorial table", "spaceBefore": 3})).unwrap();
        app.run("style.table.create", json!({"name": TABLE_STYLE, "spaceBefore": 17})).unwrap();
        app.run("style.cell.create", json!({"name": "Editorial cell", "insets": [3, 3, 3, 3]})).unwrap();
        app.run("style.cell.create", json!({"name": CELL_STYLE, "insets": [7, 8, 9, 10], "vj": "center"})).unwrap();
        select(&mut app, &made, [1, 2], [1, 2]);
        (app, made, other)
    }
    fn table(app: &DesignApp, made: &Value) -> Table {
        (**app
            .session
            .active()
            .unwrap()
            .doc
            .story(StoryId(made["story"].as_u64().unwrap()))
            .unwrap()
            .tables
            .get(&made["table"].as_u64().unwrap())
            .unwrap())
        .clone()
    }
    type History = Vec<(String, Arc<Document>, Selection)>;
    struct Quiet {
        document: Arc<Document>,
        saved: Arc<Document>,
        undo: History,
        redo: History,
        revision: u64,
        saved_revision: u64,
        epoch: u64,
        uid: u64,
        dirty: bool,
        selection: Selection,
        limit: usize,
    }
    fn quiet(app: &DesignApp) -> Quiet {
        let st = app.session.active().unwrap();
        let history = |entries: &[designcraft_engine::HistoryEntry]| -> History {
            entries.iter().map(|entry| (entry.label.clone(), entry.doc.clone(), entry.selection.clone())).collect()
        };
        Quiet {
            document: st.doc.clone(),
            saved: st.saved_doc.clone(),
            undo: history(&st.history.undo),
            redo: history(&st.history.redo),
            revision: st.revision,
            saved_revision: st.saved_revision,
            epoch: st.identity_epoch,
            uid: st.uid,
            dirty: st.is_dirty(),
            selection: st.selection.clone(),
            limit: st.history.limit,
        }
    }
    fn assert_quiet(app: &DesignApp, before: &Quiet) {
        let now = quiet(app);
        assert!(Arc::ptr_eq(&now.document, &before.document));
        assert_eq!(now.document.as_ref(), before.document.as_ref());
        assert!(Arc::ptr_eq(&now.saved, &before.saved));
        assert_eq!(now.undo, before.undo);
        assert_eq!(now.redo, before.redo);
        assert_eq!(
            (now.revision, now.saved_revision, now.epoch, now.uid, now.dirty, now.limit),
            (before.revision, before.saved_revision, before.epoch, before.uid, before.dirty, before.limit)
        );
        assert_eq!(now.selection, before.selection);
    }
    fn assert_one_edit(app: &DesignApp, before: &Quiet) {
        let st = app.session.active().unwrap();
        assert_eq!(st.history.undo.len(), before.undo.len() + 1);
        assert!(st.history.redo.is_empty());
        assert!(Arc::ptr_eq(&st.saved_doc, &before.saved));
        assert_eq!((st.uid, st.identity_epoch, st.saved_revision, st.history.limit), (before.uid, before.epoch, before.saved_revision, before.limit));
        assert_eq!(st.selection, before.selection);
        assert!(st.is_dirty());
    }
    fn harness(mut app: DesignApp, installed: bool) -> Harness<'static, DesignApp> {
        let ctx = egui::Context::default();
        if installed {
            app.framework_opacity = true;
            app.install_framework(&ctx);
            assert!(app.framework_installed);
        }
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(egui::vec2(680.0, 1100.0)).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                    crate::theme::apply(ui.ctx(), &Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                let _ = craft_ui_widgets::runtime::inspect::record(ui, egui::Id::NULL.with("table_style_test_region"), |ui| show(app, ui));
            },
            app,
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 3);
        h
    }
    fn frame(h: &mut Harness<'_, DesignApp>, event: Option<Event>) {
        let mut raw = std::mem::take(h.input_mut());
        if let Some(event) = event {
            raw.events.push(event);
        }
        let ctx = h.ctx.clone();
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    fn idle(h: &mut Harness<'_, DesignApp>, count: usize) {
        for _ in 0..count {
            frame(h, None);
        }
    }
    fn key_event(h: &mut Harness<'_, DesignApp>, key: Key, pressed: bool) {
        frame(h, Some(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE }));
    }
    fn key(h: &mut Harness<'_, DesignApp>, key: Key) {
        key_event(h, key, true);
        key_event(h, key, false);
        idle(h, 2);
    }
    fn pointer(h: &mut Harness<'_, DesignApp>, pos: egui::Pos2, pressed: bool) {
        frame(h, Some(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }));
    }
    fn click_at(h: &mut Harness<'_, DesignApp>, pos: egui::Pos2) {
        frame(h, Some(Event::PointerMoved(pos)));
        pointer(h, pos, true);
        pointer(h, pos, false);
        idle(h, 2);
    }
    fn click(h: &mut Harness<'_, DesignApp>, role: egui::accesskit::Role, label: &str) {
        let pos = h.get_by_role_and_label(role, label).rect().center();
        click_at(h, pos);
    }
    fn tab_to(h: &mut Harness<'_, DesignApp>, role: egui::accesskit::Role, label: &str) {
        let stops = h
            .query_all_by(|n| {
                matches!(
                    n.role(),
                    egui::accesskit::Role::Button
                        | egui::accesskit::Role::ComboBox
                        | egui::accesskit::Role::TextInput
                        | egui::accesskit::Role::CheckBox
                        | egui::accesskit::Role::RadioButton
                )
            })
            .count();
        for _ in 0..=stops {
            if h.get_by_role_and_label(role, label).is_focused() {
                let target = h.get_by_role_and_label(role, label).accesskit_node().id();
                let focus: Vec<_> = h.query_all_by(|node| node.is_focused()).map(|node| node.accesskit_node().id()).collect();
                assert_eq!(focus, vec![target], "exactly one actual AX focus stop before activation");
                return;
            }
            key(h, Key::Tab);
        }
        panic!("natural Tab did not reach exact {label} within the rendered focus cycle");
    }
    fn sdk(h: &Harness<'_, DesignApp>, component: &str, role: egui::accesskit::Role, label: &str) -> craft_ui_widgets::runtime::inspect::Element {
        let recorded = craft_ui_widgets::runtime::inspect::recorded(&h.ctx, egui::Id::NULL.with("table_style_test_region")).unwrap();
        let matches: Vec<_> = recorded
            .elements
            .iter()
            .filter(|e| e.component == component && e.facts.as_ref().and_then(|f| f.name.as_deref()) == Some(label))
            .collect();
        assert_eq!(matches.len(), 1, "one real SDK {component} with complete action name");
        assert_eq!(h.get_all_by_role_and_label(role, label).count(), 1);
        assert!(!matches[0].facts.as_ref().unwrap().disabled);
        matches[0].clone()
    }
    fn calls(app: &DesignApp, from: usize, command: &str) -> Vec<Value> {
        app.session.journal[from..].iter().filter(|(id, _)| id == command).map(|(_, params)| params.clone()).collect()
    }
    fn apply_payload(name: &str, made: &Value, rows: [usize; 2], cols: [usize; 2]) -> Value {
        json!({"name": name, "story": made["story"], "table": made["table"], "rows": rows, "cols": cols})
    }
    fn undo(h: &mut Harness<'_, DesignApp>, before: &Quiet) {
        h.state_mut().run("edit.undo", json!({})).unwrap();
        idle(h, 2);
        let st = h.state().session.active().unwrap();
        assert_eq!(st.doc.as_ref(), before.document.as_ref(), "one actual Undo restores the complete document/catalog/table state");
        assert!(Arc::ptr_eq(&st.saved_doc, &before.saved));
        assert_eq!(st.history.undo.len(), before.undo.len());
    }

    #[test]
    fn ui_table_cell_style_natural_preview_escape_then_commit_exact_inclusive_range_and_undo() {
        let (app, made, other) = fixture();
        let untouched = table(&app, &other);
        let mut h = harness(app, true);
        sdk(&h, "Select", egui::accesskit::Role::ComboBox, "Cell Style");
        let before = quiet(h.state());
        let journal = h.state().session.journal.clone();
        tab_to(&mut h, egui::accesskit::Role::ComboBox, "Cell Style");
        key(&mut h, Key::Enter);
        key(&mut h, Key::End);
        assert_quiet(h.state(), &before);
        assert_eq!(h.state().session.journal, journal, "open keyboard proposals cannot edit or apply a style");
        key(&mut h, Key::Escape);
        assert_quiet(h.state(), &before);
        key(&mut h, Key::Enter);
        key(&mut h, Key::End);
        key(&mut h, Key::Enter);
        assert_eq!(calls(h.state(), journal.len(), "style.cell.apply"), [apply_payload(CELL_STYLE, &made, [1, 2], [1, 2])]);
        let mut expected = table_before(&before, &made);
        let resolved = designcraft_doc::resolve_cell_style(&before.document.styles.cell, CELL_STYLE);
        for row in 1..=2 {
            for col in 1..=2 {
                resolved.apply_to(expected.cell_mut(row, col).unwrap());
            }
        }
        assert_eq!(table(h.state(), &made), expected);
        assert_eq!(table(h.state(), &other), untouched);
        assert_one_edit(h.state(), &before);
        let after = h.state().session.journal.clone();
        idle(&mut h, 3);
        assert_eq!(h.state().session.journal, after);
        undo(&mut h, &before);
    }
    fn table_before(before: &Quiet, made: &Value) -> Table {
        (**before.document.story(StoryId(made["story"].as_u64().unwrap())).unwrap().tables.get(&made["table"].as_u64().unwrap()).unwrap()).clone()
    }

    #[test]
    fn ui_table_table_style_pointer_uses_complete_catalog_name_and_exact_table_payload() {
        let (app, made, other) = fixture();
        let untouched = table(&app, &other);
        let mut h = harness(app, true);
        sdk(&h, "Select", egui::accesskit::Role::ComboBox, "Table Style");
        let before = quiet(h.state());
        let journal = h.state().session.journal.len();
        click(&mut h, egui::accesskit::Role::ComboBox, "Table Style");
        assert_quiet(h.state(), &before);
        assert_eq!(h.get_all_by_role_and_label(egui::accesskit::Role::ListBoxOption, TABLE_STYLE).count(), 1);
        click(&mut h, egui::accesskit::Role::ListBoxOption, TABLE_STYLE);
        assert_eq!(calls(h.state(), journal, "style.table.apply"), [apply_payload(TABLE_STYLE, &made, [1, 2], [1, 2])]);
        let mut expected = table_before(&before, &made);
        before.document.styles.resolve_table_style(TABLE_STYLE).apply_to(&mut expected, &before.document.styles.cell);
        assert_eq!(table(h.state(), &made), expected);
        assert_eq!(table(h.state(), &other), untouched);
        assert_one_edit(h.state(), &before);
        undo(&mut h, &before);
    }

    #[test]
    fn ui_table_new_style_plus_keeps_twenty_pixel_artwork_default_names_selection_capture_and_undo() {
        for cell in [false, true] {
            let (app, made, other) = fixture();
            let untouched = table(&app, &other);
            let mut h = harness(app, true);
            let (label, command, name) = if cell {
                ("New Cell Style", "style.cell.create", "Cell Style 1")
            } else {
                ("New Table Style", "style.table.create", "Table Style 1")
            };
            let actual = sdk(&h, "IconButton", egui::accesskit::Role::Button, label);
            assert_eq!(actual.rect.size(), egui::vec2(20.0, 20.0));
            let before = quiet(h.state());
            let journal = h.state().session.journal.len();
            if cell {
                tab_to(&mut h, egui::accesskit::Role::Button, label);
                key(&mut h, Key::Space);
            } else {
                click(&mut h, egui::accesskit::Role::Button, label);
            }
            let payload = if cell { json!({"name": name, "fromSelection": true}) } else { json!({"name": name}) };
            assert_eq!(calls(h.state(), journal, command), [payload]);
            let styles = &h.state().session.active().unwrap().doc.styles;
            if cell {
                assert_eq!(styles.cell.len(), before.document.styles.cell.len() + 1);
                assert_eq!(styles.table, before.document.styles.table);
                let made_style = styles.cell.iter().find(|s| s.name == name).unwrap();
                let source = table_before(&before, &made).cell(1, 1).unwrap().clone();
                assert_eq!(made_style.insets, Some(source.insets));
                assert_eq!(made_style.vj, Some(source.vj));
                assert_eq!(made_style.fill.as_ref(), Some(&source.fill));
            } else {
                assert_eq!(styles.table.len(), before.document.styles.table.len() + 1);
                assert_eq!(styles.cell, before.document.styles.cell);
                assert!(styles.table.iter().any(|s| s.name == name));
            }
            assert_eq!(table(h.state(), &made), table_before(&before, &made), "creation does not silently apply");
            assert_eq!(table(h.state(), &other), untouched);
            assert_one_edit(h.state(), &before);
            let calls_after = h.state().session.journal.clone();
            idle(&mut h, 3);
            assert_eq!(h.state().session.journal, calls_after);
            undo(&mut h, &before);
        }
    }

    #[test]
    fn ui_table_new_style_original_name_is_uniqued_by_engine_and_absent_target_refuses() {
        let (mut app, _, _) = fixture();
        app.run("style.cell.create", json!({"name": "Cell Style 1", "fromSelection": true})).unwrap();
        app.run("style.table.create", json!({"name": "Table Style 1"})).unwrap();
        let mut h = harness(app, true);
        for (label, command) in [("New Cell Style", "style.cell.create"), ("New Table Style", "style.table.create")] {
            let before = quiet(h.state());
            let journal = h.state().session.journal.len();
            tab_to(&mut h, egui::accesskit::Role::Button, label);
            key(&mut h, Key::Enter);
            let styles = &h.state().session.active().unwrap().doc.styles;
            let new_names: Vec<_> = if command == "style.cell.create" {
                styles.cell.iter().filter(|s| !before.document.styles.cell.iter().any(|old| old.name == s.name)).map(|s| s.name.clone()).collect()
            } else {
                styles.table.iter().filter(|s| !before.document.styles.table.iter().any(|old| old.name == s.name)).map(|s| s.name.clone()).collect()
            };
            assert_eq!(new_names.len(), 1);
            assert_ne!(new_names[0], if command == "style.cell.create" { "Cell Style 1" } else { "Table Style 1" });
            assert_eq!(calls(h.state(), journal, command).len(), 1);
            assert_one_edit(h.state(), &before);
            undo(&mut h, &before);
        }
        h.state_mut().run("selection.set", json!({"ids": []})).unwrap();
        idle(&mut h, 2);
        assert!(h.query_by_role_and_label(egui::accesskit::Role::Button, "New Cell Style").is_none());
        let before = quiet(h.state());
        assert!(
            h.state_mut().session.execute("style.cell.create", &json!({"name": "Cell Style 1", "fromSelection": true})).is_err(),
            "unchanged real engine refuses capture without a cell target"
        );
        assert_quiet(h.state(), &before);
    }

    #[test]
    fn ui_table_row_mode_commits_without_height_changes_preview_edits_or_other_table_mutation() {
        for keyboard in [false, true] {
            let (app, made, other) = fixture();
            let untouched = table(&app, &other);
            let mut h = harness(app, true);
            sdk(&h, "Select", egui::accesskit::Role::ComboBox, "Row Height");
            let before = quiet(h.state());
            let journal = h.state().session.journal.len();
            if keyboard {
                tab_to(&mut h, egui::accesskit::Role::ComboBox, "Row Height");
                key(&mut h, Key::Enter);
                key(&mut h, Key::End);
                assert_quiet(h.state(), &before);
                assert!(calls(h.state(), journal, "table.setRowHeight").is_empty());
                key(&mut h, Key::Escape);
                assert_quiet(h.state(), &before);
                key(&mut h, Key::Enter);
                key(&mut h, Key::End);
                key(&mut h, Key::Enter);
            } else {
                click(&mut h, egui::accesskit::Role::ComboBox, "Row Height");
                click(&mut h, egui::accesskit::Role::ListBoxOption, "Exactly");
            }
            assert_eq!(calls(h.state(), journal, "table.setRowHeight"), [json!({"mode": "exactly"})]);
            let mut expected = table_before(&before, &made);
            for row in &mut expected.rows[1..=2] {
                row.mode = designcraft_doc::RowHeightMode::Exactly;
            }
            assert_eq!(table(h.state(), &made), expected, "only selected row modes change; all heights/cells/columns stay byte-equivalent");
            assert_eq!(table(h.state(), &other), untouched);
            assert_one_edit(h.state(), &before);
            let exact = quiet(h.state());
            click(&mut h, egui::accesskit::Role::ComboBox, "Row Height");
            click(&mut h, egui::accesskit::Role::ListBoxOption, "At Least");
            assert_eq!(calls(h.state(), journal, "table.setRowHeight"), [json!({"mode": "exactly"}), json!({"mode": "atLeast"})]);
            assert_eq!(table(h.state(), &made), table_before(&before, &made), "returning to At Least still changes no height");
            assert_one_edit(h.state(), &exact);
            undo(&mut h, &exact);
            undo(&mut h, &before);
        }
    }

    #[test]
    fn ui_table_open_style_choice_drops_changed_range_table_catalog_or_recreated_document_and_recovers() {
        for change in ["range", "table", "reorder", "remove", "replace"] {
            let (app, made, other) = fixture();
            let mut h = harness(app, true);
            tab_to(&mut h, egui::accesskit::Role::ComboBox, "Cell Style");
            let retired_owner = sdk(&h, "Select", egui::accesskit::Role::ComboBox, "Cell Style").id;
            key(&mut h, Key::Enter);
            key(&mut h, Key::End);
            assert!(h.query_all_by_role(egui::accesskit::Role::ListBoxOption).count() > 0, "old real popup must be open before {change}");
            match change {
                "range" => select(h.state_mut(), &made, [0, 0], [0, 1]),
                "table" => select(h.state_mut(), &other, [0, 1], [1, 2]),
                "reorder" | "remove" => {
                    h.state_mut()
                        .session
                        .edit(|doc, _| {
                            if change == "remove" {
                                doc.styles_mut().cell.retain(|style| style.name != CELL_STYLE);
                            } else {
                                let styles = &mut doc.styles_mut().cell;
                                let last = styles.len() - 1;
                                styles.swap(last, last - 1);
                            }
                            Ok(())
                        })
                        .unwrap();
                }
                "replace" => {
                    let old = h.state().session.active().unwrap();
                    let uid = old.uid;
                    let epoch = old.identity_epoch;
                    let mut replacement = designcraft_engine::DocState::new(old.doc.as_ref().clone(), None);
                    replacement.uid = uid;
                    assert_ne!(replacement.identity_epoch, epoch);
                    *h.state_mut().session.active_mut().unwrap() = replacement;
                    select(h.state_mut(), &made, [1, 2], [1, 2]);
                }
                _ => unreachable!(),
            }
            // The external change is deliberate. From this point every render and focus gesture must be nonediting.
            let before = quiet(h.state());
            let journal = h.state().session.journal.len();
            idle(&mut h, 2);
            assert_quiet(h.state(), &before);
            assert!(h.state().session.journal[journal..].is_empty(), "retiring the old popup cannot dispatch an engine command after {change}");
            let current_owner = sdk(&h, "Select", egui::accesskit::Role::ComboBox, "Cell Style").id;
            assert_ne!(current_owner, retired_owner, "the actual owner must change after {change}");
            assert_eq!(
                h.query_all_by_role(egui::accesskit::Role::ListBoxOption).count(),
                0,
                "old popup must retire BEFORE refocusing after {change}"
            );
            // This is a NEW Enter, not the old proposal's held input. Reach its real current field with natural Tab.
            // Do not send it to an unobserved focus target (the measured old fixture dispatched Distribute Rows).
            tab_to(&mut h, egui::accesskit::Role::ComboBox, "Cell Style");
            assert_quiet(h.state(), &before);
            assert!(h.state().session.journal[journal..].is_empty(), "natural refocus must not edit after {change}");
            key(&mut h, Key::Enter);
            // TABLE_STALE_CONTEXT_BEGIN stale-enter
            {
                let now = h.state().session.active().unwrap();
                eprintln!(
                    "Table stale ownership change={change}, phase=current-owner-enter-after-retirement: document_arc={}, document_values={}, before_uid_epoch_revision={:?}, current_uid_epoch_revision={:?}, selection={:?}, commands={:?}",
                    Arc::ptr_eq(&now.doc, &before.document),
                    now.doc.as_ref() == before.document.as_ref(),
                    (before.uid, before.epoch, before.revision),
                    (now.uid, now.identity_epoch, now.revision),
                    now.selection,
                    &h.state().session.journal[journal..]
                );
                if !Arc::ptr_eq(&now.doc, &before.document) {
                    eprintln!("Table stale document before={:?}", before.document);
                    eprintln!("Table stale document current={:?}", now.doc);
                }
            }
            // TABLE_STALE_CONTEXT_END stale-enter
            assert_quiet(h.state(), &before);
            assert!(h.state().session.journal[journal..].is_empty(), "opening the current field cannot dispatch any engine command after {change}");
            assert!(h.query_all_by_role(egui::accesskit::Role::ListBoxOption).count() > 0, "new current popup must really open after {change}");
            assert!(calls(h.state(), journal, "style.cell.apply").is_empty(), "stale popup cannot commit after {change}");
            key(&mut h, Key::Escape);
            // TABLE_STALE_CONTEXT_BEGIN escape-after-stale-enter
            {
                let now = h.state().session.active().unwrap();
                eprintln!(
                    "Table stale ownership change={change}, phase=current-owner-escape-after-retirement: document_arc={}, document_values={}, before_uid_epoch_revision={:?}, current_uid_epoch_revision={:?}, selection={:?}, commands={:?}",
                    Arc::ptr_eq(&now.doc, &before.document),
                    now.doc.as_ref() == before.document.as_ref(),
                    (before.uid, before.epoch, before.revision),
                    (now.uid, now.identity_epoch, now.revision),
                    now.selection,
                    &h.state().session.journal[journal..]
                );
                if !Arc::ptr_eq(&now.doc, &before.document) {
                    eprintln!("Table stale document before={:?}", before.document);
                    eprintln!("Table stale document current={:?}", now.doc);
                }
            }
            // TABLE_STALE_CONTEXT_END escape-after-stale-enter
            assert_quiet(h.state(), &before);
            assert!(
                h.state().session.journal[journal..].is_empty(),
                "dismissing the current popup cannot dispatch any engine command after {change}"
            );
            assert_eq!(
                h.query_all_by_role(egui::accesskit::Role::ListBoxOption).count(),
                0,
                "Escape must dismiss the actual current popup after {change}"
            );
            // A fresh natural gesture uses the CURRENT real range/catalog, not the old proposal.
            tab_to(&mut h, egui::accesskit::Role::ComboBox, "Cell Style");
            key(&mut h, Key::Enter);
            key(&mut h, Key::End);
            key(&mut h, Key::Enter);
            let info = h.state_mut().session.execute("table.get", &json!({})).unwrap();
            let expected_name = before.document.styles.cell.last().unwrap().name.clone();
            assert_eq!(
                calls(h.state(), journal, "style.cell.apply"),
                [
                    json!({"name": expected_name, "story": info["story"], "table": info["table"], "rows": [info["range"]["r0"], info["range"]["r1"]], "cols": [info["range"]["c0"], info["range"]["c1"]]})
                ]
            );
            assert_one_edit(h.state(), &before);
        }
    }

    #[test]
    fn ui_table_held_plus_does_not_follow_range_document_or_absence_and_fresh_click_recovers() {
        for change in ["range", "switch", "close", "delete"] {
            let (app, made, _) = fixture();
            let mut h = harness(app, true);
            let pos = h.get_by_role_and_label(egui::accesskit::Role::Button, "New Cell Style").rect().center();
            frame(&mut h, Some(Event::PointerMoved(pos)));
            pointer(&mut h, pos, true);
            if change == "range" {
                select(h.state_mut(), &made, [0, 0], [0, 0]);
            } else if change == "switch" {
                let doc = h.state().session.active().unwrap().doc.as_ref().clone();
                h.state_mut().run("file.new", json!({})).unwrap();
                let st = h.state_mut().session.active_mut().unwrap();
                st.doc = Arc::new(doc);
                st.bump_revision();
                select(h.state_mut(), &made, [1, 2], [1, 2]);
            } else if change == "close" {
                h.state_mut().session.execute("file.close", &json!({"force": true})).unwrap();
            } else {
                h.state_mut().run("table.delete", json!({})).unwrap();
            }
            idle(&mut h, 2);
            let journal = h.state().session.journal.len();
            if change == "close" {
                assert!(h.state().session.active().is_none());
                pointer(&mut h, pos, false);
                idle(&mut h, 2);
                assert!(h.state().session.active().is_none());
                assert!(calls(h.state(), journal, "style.cell.create").is_empty());
                continue;
            }
            let before = quiet(h.state());
            pointer(&mut h, pos, false);
            idle(&mut h, 2);
            assert_quiet(h.state(), &before);
            assert!(calls(h.state(), journal, "style.cell.create").is_empty(), "held plus cannot follow {change}");
            if change == "delete" {
                assert!(h.query_by_role_and_label(egui::accesskit::Role::Button, "New Cell Style").is_none());
                continue;
            }
            click(&mut h, egui::accesskit::Role::Button, "New Cell Style");
            assert_eq!(calls(h.state(), journal, "style.cell.create"), [json!({"name": "Cell Style 1", "fromSelection": true})]);
            assert_one_edit(h.state(), &before);
            if change == "switch" {
                let docs = h.state().session.documents();
                assert_eq!(docs.len(), 2);
                assert!(!docs[0].doc.styles.cell.iter().any(|style| style.name == "Cell Style 1"), "the old document's catalog is untouched");
            }
        }
    }

    #[test]
    fn ui_table_removed_applied_style_is_not_an_enabled_orphan_and_uninstalled_controls_work() {
        let (mut app, made, _) = fixture();
        app.run("style.cell.apply", apply_payload(CELL_STYLE, &made, [1, 2], [1, 2])).unwrap();
        app.session
            .edit(|doc, _| {
                doc.styles_mut().cell.retain(|style| style.name != CELL_STYLE);
                Ok(())
            })
            .unwrap();
        let mut h = harness(app, true);
        let before = quiet(h.state());
        let journal = h.state().session.journal.len();
        click(&mut h, egui::accesskit::Role::ComboBox, "Cell Style");
        assert!(h.query_by_role_and_label(egui::accesskit::Role::ListBoxOption, CELL_STYLE).is_none());
        let names: Vec<_> = h.query_all_by_role(egui::accesskit::Role::ListBoxOption).map(|node| node.accesskit_node().label().unwrap()).collect();
        assert_eq!(names, before.document.styles.cell.iter().map(|style| style.name.clone()).collect::<Vec<_>>());
        key(&mut h, Key::Escape);
        assert_quiet(h.state(), &before);
        assert!(calls(h.state(), journal, "style.cell.apply").is_empty());
        let (app, made, _) = fixture();
        let mut h = harness(app, false);
        assert!(!h.state().framework_installed);
        let before = quiet(h.state());
        let journal = h.state().session.journal.len();
        click(&mut h, egui::accesskit::Role::ComboBox, "Table Style");
        let pos = h.get_by_label(TABLE_STYLE).rect().center();
        click_at(&mut h, pos);
        assert_eq!(calls(h.state(), journal, "style.table.apply"), [apply_payload(TABLE_STYLE, &made, [1, 2], [1, 2])]);
        assert_one_edit(h.state(), &before);
        undo(&mut h, &before);
        click(&mut h, egui::accesskit::Role::ComboBox, "Row Height");
        let pos = h.get_by_label("Exactly").rect().center();
        click_at(&mut h, pos);
        assert_eq!(calls(h.state(), journal, "table.setRowHeight"), [json!({"mode": "exactly"})]);
    }

    #[test]
    fn ui_table_whole_app_parent_uses_real_style_select_and_exact_engine_target() {
        let (mut app, made, other) = fixture();
        let untouched = table(&app, &other);
        app.framework_opacity = true;
        let mut h = crate::test_window::open(app, egui::vec2(1400.0, 1000.0));
        let ctx = h.ctx.clone();
        h.state_mut().app.install_framework(&ctx);
        assert!(h.state().app.framework_installed);
        h.state_mut().app.run("window.panel", json!({"panel": "table"})).unwrap();
        fn step(h: &mut Harness<'_, crate::test_window::Window>, event: Option<Event>) {
            let mut raw = std::mem::take(h.input_mut());
            if let Some(event) = event {
                raw.events.push(event);
            }
            let ctx = h.ctx.clone();
            h.state_mut().app.raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
        fn at(h: &mut Harness<'_, crate::test_window::Window>, role: egui::accesskit::Role, label: &str) {
            let pos = h.get_by_role_and_label(role, label).rect().center();
            step(h, Some(Event::PointerMoved(pos)));
            for pressed in [true, false] {
                step(h, Some(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }));
            }
            for _ in 0..2 {
                step(h, None);
            }
        }
        for _ in 0..3 {
            step(&mut h, None);
        }
        assert_eq!(h.get_all_by_role_and_label(egui::accesskit::Role::ComboBox, "Table Style").count(), 1);
        assert_eq!(h.get_all_by_role_and_label(egui::accesskit::Role::Button, "New Table Style").count(), 1);
        let before = quiet(&h.state().app);
        let journal = h.state().app.session.journal.len();
        at(&mut h, egui::accesskit::Role::ComboBox, "Table Style");
        assert_quiet(&h.state().app, &before);
        at(&mut h, egui::accesskit::Role::ListBoxOption, TABLE_STYLE);
        assert_eq!(calls(&h.state().app, journal, "style.table.apply"), [apply_payload(TABLE_STYLE, &made, [1, 2], [1, 2])]);
        assert_eq!(table(&h.state().app, &made).style, TABLE_STYLE);
        assert_eq!(table(&h.state().app, &other), untouched);
        assert_one_edit(&h.state().app, &before);
    }

    // TABLE_RESELECT_TESTS_BEGIN
    #[test]
    fn ui_table_displayed_cell_style_reselection_reapplies_mixed_range_and_overrides_once_with_real_undo() {
        const DISPLAYED: &str = "Editorial cell";
        for keyboard in [false, true] {
            let (mut app, made, other) = fixture();
            app.run("style.cell.apply", apply_payload(DISPLAYED, &made, [1, 1], [1, 1])).unwrap();
            app.run("style.cell.apply", apply_payload(CELL_STYLE, &made, [1, 2], [2, 2])).unwrap();
            app.run("table.setCell", json!({"row": 1, "col": 1, "insets": [42, 43, 44, 45]})).unwrap();
            select(&mut app, &made, [1, 2], [1, 2]);
            let mixed = table(&app, &made);
            assert_eq!(mixed.cell(1, 1).unwrap().style, DISPLAYED, "the actual first selected cell supplies the displayed name");
            assert_eq!(mixed.cell(1, 2).unwrap().style, CELL_STYLE, "another selected cell really has a different style");
            assert_eq!(mixed.cell(1, 1).unwrap().insets, [42.0, 43.0, 44.0, 45.0]);
            let untouched = table(&app, &other);
            let mut h = harness(app, true);
            sdk(&h, "Select", egui::accesskit::Role::ComboBox, "Cell Style");
            let before = quiet(h.state());
            let journal = h.state().session.journal.len();
            let at = before.document.styles.cell.iter().position(|style| style.name == DISPLAYED).unwrap();
            if keyboard {
                tab_to(&mut h, egui::accesskit::Role::ComboBox, "Cell Style");
                key(&mut h, Key::Enter);
                key(&mut h, Key::Home);
                for _ in 0..at {
                    key(&mut h, Key::ArrowDown);
                }
                assert_quiet(h.state(), &before);
                assert!(calls(h.state(), journal, "style.cell.apply").is_empty(), "previewing the displayed name cannot reapply it");
                key(&mut h, Key::Escape);
                assert_quiet(h.state(), &before);
                key(&mut h, Key::Enter);
                key(&mut h, Key::Home);
                for _ in 0..at {
                    key(&mut h, Key::ArrowDown);
                }
                assert_quiet(h.state(), &before);
                key(&mut h, Key::Enter);
            } else {
                click(&mut h, egui::accesskit::Role::ComboBox, "Cell Style");
                assert_quiet(h.state(), &before);
                assert_eq!(h.get_all_by_role_and_label(egui::accesskit::Role::ListBoxOption, DISPLAYED).count(), 1);
                click(&mut h, egui::accesskit::Role::ListBoxOption, DISPLAYED);
            }
            assert_eq!(
                calls(h.state(), journal, "style.cell.apply"),
                [apply_payload(DISPLAYED, &made, [1, 2], [1, 2])],
                "one explicit equal-name activation still sends the original complete inclusive payload"
            );
            let mut expected = mixed.clone();
            let resolved = designcraft_doc::resolve_cell_style(&before.document.styles.cell, DISPLAYED);
            for row in 1..=2 {
                for col in 1..=2 {
                    resolved.apply_to(expected.cell_mut(row, col).unwrap());
                }
            }
            let actual = table(h.state(), &made);
            assert_eq!(actual, expected, "reselection reapplies the real named style to ALL selected owners");
            assert_ne!(actual, mixed, "this is a genuine restoration/unification, not a no-op fixture");
            assert_eq!(actual.cell(1, 1).unwrap().insets, [3.0; 4]);
            assert_eq!(actual.cell(1, 2).unwrap().style, DISPLAYED);
            assert_eq!(table(h.state(), &other), untouched);
            assert_eq!(h.state().session.active().unwrap().doc.styles, before.document.styles);
            assert_one_edit(h.state(), &before);
            let after = h.state().session.journal.clone();
            idle(&mut h, 3);
            assert_eq!(h.state().session.journal, after, "idle cannot replay reapplication");
            undo(&mut h, &before);
        }
    }

    #[test]
    fn ui_table_displayed_table_style_reselection_restores_real_formatting_for_pointer_and_keyboard() {
        for keyboard in [false, true] {
            let (mut app, made, other) = fixture();
            app.run("style.table.apply", apply_payload(TABLE_STYLE, &made, [1, 2], [1, 2])).unwrap();
            app.run("table.options", json!({"spaceBefore": 41})).unwrap();
            let overridden = table(&app, &made);
            assert_eq!(overridden.style, TABLE_STYLE, "the actual table still displays its named style");
            assert_eq!(overridden.options.space_before, 41.0);
            let untouched = table(&app, &other);
            let mut h = harness(app, true);
            sdk(&h, "Select", egui::accesskit::Role::ComboBox, "Table Style");
            let before = quiet(h.state());
            let journal = h.state().session.journal.len();
            if keyboard {
                tab_to(&mut h, egui::accesskit::Role::ComboBox, "Table Style");
                key(&mut h, Key::Enter);
                key(&mut h, Key::End);
                assert_quiet(h.state(), &before);
                assert!(calls(h.state(), journal, "style.table.apply").is_empty(), "same-name proposals are not app edits");
                key(&mut h, Key::Escape);
                assert_quiet(h.state(), &before);
                key(&mut h, Key::Enter);
                key(&mut h, Key::End);
                assert_quiet(h.state(), &before);
                key(&mut h, Key::Enter);
            } else {
                click(&mut h, egui::accesskit::Role::ComboBox, "Table Style");
                assert_quiet(h.state(), &before);
                assert_eq!(h.get_all_by_role_and_label(egui::accesskit::Role::ListBoxOption, TABLE_STYLE).count(), 1);
                click(&mut h, egui::accesskit::Role::ListBoxOption, TABLE_STYLE);
            }
            assert_eq!(calls(h.state(), journal, "style.table.apply"), [apply_payload(TABLE_STYLE, &made, [1, 2], [1, 2])]);
            let mut expected = overridden.clone();
            before.document.styles.resolve_table_style(TABLE_STYLE).apply_to(&mut expected, &before.document.styles.cell);
            let actual = table(h.state(), &made);
            assert_eq!(actual, expected);
            assert_ne!(actual, overridden);
            assert_eq!(actual.options.space_before, 17.0, "explicitly reselecting the displayed name restores its genuine formatting");
            assert_eq!(table(h.state(), &other), untouched);
            assert_eq!(h.state().session.active().unwrap().doc.styles, before.document.styles);
            assert_one_edit(h.state(), &before);
            let after = h.state().session.journal.clone();
            idle(&mut h, 3);
            assert_eq!(h.state().session.journal, after);
            undo(&mut h, &before);
        }
    }
    // TABLE_RESELECT_TESTS_END
}
// TABLE_STYLE_TESTS_END
