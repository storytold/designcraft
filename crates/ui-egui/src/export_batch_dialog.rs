//! File › Export Layouts and Versions…: pick the layouts, the versions (each a format and its
//! settings, written to a folder of its own) and the file names, see the files it will write,
//! and keep the setup as a preset.

use designcraft_engine::cmd::export_batch::{ExportFormat, ExportJob, ExportPreset, ExportVersion, builtin_presets, version_templates};
use serde_json::{Value, json};

use crate::DesignApp;
use crate::app::file_dialogs::Bound;
use crate::dialogs::Dialog;

pub const ID: &str = "exportBatch";
/// The most files the preview lists.
const PREVIEW_ROWS: usize = 200;
/// How high the list of files may grow before it scrolls.
const PREVIEW_ROOM: f32 = 150.0;

/// How a line of text reads.
#[derive(Clone, Copy)]
enum Note {
    Plain,
    Muted,
    Error,
    Strong,
}

/// Open the dialog: the last job used (or the Print + Screen preset), on `layouts` when given.
pub fn open(app: &mut DesignApp, layouts: Option<Vec<String>>) {
    let names: Vec<String> = app.session.active().map(|st| st.doc.layouts().into_iter().map(|l| l.name).collect()).unwrap_or_default();
    let (mut job, preset) = match app.ui.export_batch.clone() {
        Some(j) => (j, String::new()),
        None => {
            let p = builtin_presets().into_iter().next().unwrap_or_default();
            (p.job, p.name)
        }
    };
    if let Some(l) = layouts {
        job.layouts = l;
    }
    job.layouts.retain(|l| names.contains(l));
    if job.layouts.len() == names.len() {
        job.layouts.clear();
    }
    app.ui.dialog = Some(Dialog::new(ID, json!({"job": job, "preset": preset})));
}

fn job_of(d: &Dialog) -> ExportJob {
    d.fields.get("job").and_then(|j| serde_json::from_value(j.clone()).ok()).unwrap_or_default()
}

fn presets(app: &DesignApp) -> Vec<ExportPreset> {
    builtin_presets().into_iter().chain(app.session.prefs.export_presets.iter().cloned()).collect()
}

/// A PDF version's settings as one of the quick choices, or "custom".
fn pdf_kind(v: &ExportVersion) -> &'static str {
    let m = v.marks;
    let no_marks = !m.crop && !m.bleed && !m.page_info;
    let plain = v.standard == "none" && !v.spreads;
    match (v.bleed, m.crop && m.bleed && m.page_info, no_marks, v.compress_images, plain) {
        (true, true, _, false, true) => "print",
        (true, false, true, false, true) => "bleed",
        (false, false, true, true, true) => "screen",
        _ => "custom",
    }
}

fn set_pdf_kind(v: &mut ExportVersion, kind: &str) {
    let marks = designcraft_engine::cmd::export_batch::VersionMarks { crop: true, bleed: true, page_info: true };
    let none = designcraft_engine::cmd::export_batch::VersionMarks::default();
    (v.bleed, v.marks, v.compress_images, v.standard, v.spreads) = match kind {
        "print" => (true, marks, false, "none".into(), false),
        "bleed" => (true, none, false, "none".into(), false),
        "screen" => (false, none, true, "none".into(), false),
        _ => return,
    };
}

/// The body's controls: the framework's on the desktop (`sdk`), egui's in the browser. Export and Cancel are the dialog
/// host's.
struct ExportControls {
    sdk: bool,
    document: Option<(u64, u64)>,
}

struct ExportButton {
    response: egui::Response,
    clicked: bool,
}

fn workflow_region() -> egui::Id {
    egui::Id::NULL.with("design_export_batch_workflow")
}

impl ExportControls {
    fn id(&self, ui: &egui::Ui, kind: &str, label: &str) -> egui::Id {
        ui.make_persistent_id((ID, self.document, kind, label))
    }

    /// A line of controls, `wrap`ping onto more lines when the room runs out, its ids scoped by `scope`.
    fn row<R>(
        &self,
        ui: &mut egui::Ui,
        scope: impl std::hash::Hash + std::fmt::Debug,
        wrap: bool,
        add: impl FnOnce(&mut egui::Ui) -> R,
    ) -> Option<R> {
        #[cfg(not(target_arch = "wasm32"))]
        if self.sdk {
            use craft_ui_widgets::contract::{Props as _, ShowContainer as _};
            let mut add = Some(add);
            let mut out = None;
            craft_ui_widgets::layout_components::Row::new().id_salt(scope).wrap(wrap).show(ui, |ui, _| {
                if let Some(add) = add.take() {
                    out = Some(add(ui));
                }
            });
            return out;
        }
        Some(ui.push_id(scope, |ui| if wrap { ui.horizontal_wrapped(add).inner } else { ui.horizontal(add).inner }).inner)
    }

    /// Ids scoped by `scope` for what `add` shows, one under another.
    fn scope(&self, ui: &mut egui::Ui, scope: impl std::hash::Hash + std::fmt::Debug, add: impl FnOnce(&mut egui::Ui)) {
        #[cfg(not(target_arch = "wasm32"))]
        if self.sdk {
            use craft_ui_widgets::contract::{Props as _, ShowContainer as _};
            let mut add = Some(add);
            craft_ui_widgets::layout_components::Stack::new().id_salt(scope).fill_width().show(ui, |ui, _| {
                if let Some(add) = add.take() {
                    add(ui);
                }
            });
            return;
        }
        ui.push_id(scope, add);
    }

    /// A wrapping line of options, indented under the line they belong to.
    fn options(&self, ui: &mut egui::Ui, scope: impl std::hash::Hash + std::fmt::Debug, add: impl FnOnce(&mut egui::Ui)) {
        #[cfg(not(target_arch = "wasm32"))]
        if self.sdk {
            use craft_ui_widgets::contract::{Props as _, ShowContainer as _};
            let mut add = Some(add);
            craft_ui_widgets::layout_components::Row::new().id_salt(scope).wrap(true).indent(1).show(ui, |ui, _| {
                if let Some(add) = add.take() {
                    add(ui);
                }
            });
            return;
        }
        ui.push_id(scope, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.add_space(12.0);
                add(ui);
            })
        });
    }

    /// A section's title.
    fn heading(&self, ui: &mut egui::Ui, text: &str) {
        #[cfg(not(target_arch = "wasm32"))]
        if self.sdk {
            use craft_ui_widgets::contract::Show as _;
            craft_ui_widgets::display::Text::new(text.to_owned()).role(craft_ui_widgets::style::TypeRole::BodyStrong).show(ui);
            return;
        }
        ui.add_space(6.0);
        crate::rtl::label(ui, egui::RichText::new(text).strong());
    }

    /// A line of text: a note in the muted colour, a refusal in the error colour, or a count in the strong weight.
    fn note(&self, ui: &mut egui::Ui, text: &str, kind: Note) {
        #[cfg(not(target_arch = "wasm32"))]
        if self.sdk {
            use craft_ui_widgets::contract::Show as _;
            use craft_ui_widgets::display::{Shade, Text};
            let line = Text::new(text.to_owned());
            match kind {
                Note::Plain => line,
                Note::Muted => line.shade(Shade::Muted),
                Note::Error => line.tone(craft_theme::variant::Tone::Bad),
                Note::Strong => line.role(craft_ui_widgets::style::TypeRole::BodyStrong),
            }
            .show(ui);
            return;
        }
        let rich = egui::RichText::new(text);
        let rich = match kind {
            Note::Plain => rich,
            Note::Muted => rich.color(crate::theme::Tokens::get(ui.ctx()).text_dim),
            Note::Error => rich.color(ui.visuals().error_fg_color),
            Note::Strong => rich.strong(),
        };
        crate::rtl::label(ui, rich);
    }

    /// The files the export will write, at most [`PREVIEW_ROWS`] of them, in a list that scrolls.
    fn files(&self, ui: &mut egui::Ui, files: &[String]) {
        let more = (files.len() > PREVIEW_ROWS).then(|| format!("… {}", files.len() - PREVIEW_ROWS));
        #[cfg(not(target_arch = "wasm32"))]
        if self.sdk {
            use craft_ui_widgets::contract::{Props as _, Scroll, Show as _, ShowContainer as _};
            use craft_ui_widgets::display::{Shade, Text};
            craft_ui_widgets::layout_components::Stack::new()
                .id_salt("export_batch_preview")
                .fill_width()
                .scroll(Scroll::vertical())
                .room_height(PREVIEW_ROOM)
                .show(ui, |ui, _| {
                    for f in files.iter().take(PREVIEW_ROWS) {
                        Text::new(f.clone()).role(craft_ui_widgets::style::TypeRole::Mono).shade(Shade::Muted).show(ui);
                    }
                    if let Some(more) = &more {
                        Text::new(more.clone()).show(ui);
                    }
                });
            return;
        }
        let t = crate::theme::Tokens::get(ui.ctx());
        egui::ScrollArea::vertical().id_salt("export_batch_preview").max_height(PREVIEW_ROOM).auto_shrink([false, true]).show(ui, |ui| {
            for f in files.iter().take(PREVIEW_ROWS) {
                ui.label(egui::RichText::new(f).monospace().size(11.0).color(t.text_dim));
            }
            if let Some(more) = more {
                ui.label(more);
            }
        });
    }

    fn button(&self, ui: &mut egui::Ui, label: &str, enabled: bool) -> ExportButton {
        let id = self.id(ui, "button", label);
        #[cfg(not(target_arch = "wasm32"))]
        if self.sdk {
            use craft_ui_widgets::contract::{Props as _, Show as _};
            let out = craft_ui_widgets::buttons::Button::new(label.to_owned()).id(id).enabled(enabled).show(ui);
            let clicked = out.clicked();
            return ExportButton { response: out.response, clicked };
        }
        let response = ui.push_id(id, |ui| ui.add_enabled(enabled, egui::Button::new(crate::rtl::widget(ui, label)))).inner;
        let clicked = response.clicked();
        ExportButton { response, clicked }
    }

    /// A small button with the tooltip `tip`.
    fn small_button(&self, ui: &mut egui::Ui, label: &str, tip: &str) -> ExportButton {
        #[cfg(not(target_arch = "wasm32"))]
        if self.sdk {
            use craft_ui_widgets::contract::{Props as _, Show as _};
            let out = craft_ui_widgets::buttons::Button::new(label.to_owned()).id(self.id(ui, "button", label)).tooltip(tip.to_owned()).show(ui);
            let clicked = out.clicked();
            return ExportButton { response: out.response, clicked };
        }
        let response = ui.push_id(self.id(ui, "small", label), |ui| ui.small_button(crate::rtl::widget(ui, label))).inner.on_hover_text(tip);
        let clicked = response.clicked();
        ExportButton { response, clicked }
    }

    fn remove_button(&self, ui: &mut egui::Ui, label: &str) -> ExportButton {
        let id = self.id(ui, "remove", label);
        #[cfg(not(target_arch = "wasm32"))]
        if self.sdk {
            use craft_ui_widgets::contract::{Props as _, Show as _};
            use craft_ui_widgets::primitives::IconSpec;
            let out = craft_ui_widgets::buttons::IconButton::new(IconSpec::svg(std::borrow::Cow::Borrowed("trash").into()))
                .id(id)
                .label(label.to_owned())
                .show(ui);
            let clicked = out.clicked();
            return ExportButton { response: out.response, clicked };
        }
        let response = ui.push_id(id, |ui| crate::icons::button(ui, "trash", 18.0, false, label)).inner;
        let clicked = response.clicked();
        ExportButton { response, clicked }
    }

    fn text(&self, ui: &mut egui::Ui, label: &str, value: &mut String, width: f32, hint: bool) -> bool {
        let id = self.id(ui, "text", label);
        #[cfg(not(target_arch = "wasm32"))]
        if self.sdk {
            use craft_ui_widgets::contract::{Bind, Props as _, Show as _};
            let field = craft_ui_widgets::text::TextField::new(label.to_owned(), Bind::given(value.clone())).id(id).limit(None);
            let out = if hint { field.placeholder(label.to_owned()).show(ui) } else { field.show(ui) };
            if let Some(next) = out.value().filter(|next| next.as_str() != value.as_str()) {
                *value = next.clone();
                return true;
            }
            return false;
        }
        let mut field = egui::TextEdit::singleline(value).desired_width(width).id(id);
        if hint {
            field = field.hint_text(label);
        }
        let response = ui.add(field);
        response.ctx.accesskit_node_builder(response.id, |node| node.set_label(label.to_owned()));
        response.changed()
    }

    fn check(&self, ui: &mut egui::Ui, label: &str, value: &mut bool) -> bool {
        let id = self.id(ui, "check", label);
        #[cfg(not(target_arch = "wasm32"))]
        if self.sdk {
            use craft_ui_widgets::contract::{Bind, Props as _, Show as _};
            let before = *value;
            craft_ui_widgets::choice::Checkbox::new(Bind::bound(value)).id(id).label(label.to_owned()).show(ui);
            return *value != before;
        }
        ui.push_id(id, |ui| ui.checkbox(value, crate::rtl::widget(ui, label))).inner.changed()
    }

    /// Only a Commit/Set changes the job: dropdown previews and Escape cannot replace a
    /// preset, apply PDF quick settings or append a version.
    fn choice(&self, ui: &mut egui::Ui, label: &str, current: usize, entries: &[String], width: f32, placeholder: &str) -> Option<usize> {
        let id = self.id(ui, "choice", label);
        #[cfg(not(target_arch = "wasm32"))]
        if self.sdk {
            use craft_ui_widgets::contract::{Bind, Entry, Props as _, Show as _};
            let choices: Vec<_> = entries.iter().enumerate().map(|(i, name)| Entry::new(i, name.clone())).collect();
            let out = craft_ui_widgets::pickers::Select::new(label.to_owned(), Bind::given(current), &choices)
                .id(id)
                .placeholder(placeholder.to_owned())
                .show(ui);
            return out.events.committed().copied().filter(|i| *i < entries.len());
        }
        let mut picked = None;
        let shown = entries.get(current).map_or(placeholder, String::as_str);
        let response = craft_ui_legacy::ComboBox::from_id_salt(id)
            .selected_text(shown)
            .width(width)
            .show_ui(ui, |ui| {
                for (i, name) in entries.iter().enumerate() {
                    if ui.selectable_label(i == current, name).clicked() {
                        picked = Some(i);
                    }
                }
            })
            .response;
        response.ctx.accesskit_node_builder(response.id, |node| node.set_label(label.to_owned()));
        picked
    }

    fn combine(&self, ui: &mut egui::Ui, value: &mut bool, label: &str, per_layout: &str, all_layouts: &str) {
        #[cfg(not(target_arch = "wasm32"))]
        if self.sdk {
            use craft_ui_widgets::contract::{Bind, Props as _, Show as _, ShowContainer as _};
            let out =
                craft_ui_widgets::choice::RadioGroup::new(label.to_owned(), Bind::given(*value)).id(self.id(ui, "radio", label)).show(ui, |ui, _| {
                    craft_ui_widgets::layout_components::Row::new().show(ui, |ui, _| {
                        craft_ui_widgets::choice::Radio::new(false).label(per_layout.to_owned()).show(ui);
                        craft_ui_widgets::choice::Radio::new(true).label(all_layouts.to_owned()).show(ui);
                    });
                });
            if let Some(next) = out.output.events.committed() {
                *value = *next;
            }
            return;
        }
        ui.horizontal(|ui| {
            if ui.radio(!*value, crate::rtl::widget(ui, per_layout)).clicked() {
                *value = false;
            }
            if ui.radio(*value, crate::rtl::widget(ui, all_layouts)).clicked() {
                *value = true;
            }
        });
    }

    fn number(&self, ui: &mut egui::Ui, label: &str, value: &mut f64, range: std::ops::RangeInclusive<f64>, integer: bool) -> bool {
        let id = self.id(ui, "number", label);
        #[cfg(not(target_arch = "wasm32"))]
        if self.sdk {
            use craft_ui_widgets::contract::{Bind, Props as _, Show as _};
            if integer {
                craft_ui_widgets::display::Text::new(label.to_owned()).show(ui);
            }
            let field = craft_ui_widgets::text::NumberField::new(label.to_owned(), Bind::given(Some(*value))).id(id).range(range.clone()).step(1.0);
            let out = if integer { field.snap_typed(true).fixed_decimals(0).commit_decimals(0).show(ui) } else { field.unit("ppi").show(ui) };
            if let Some(Some(next)) = out.edit.events.landed().filter(|next| next.is_some_and(f64::is_finite)) {
                let next = next.clamp(*range.start(), *range.end());
                if next != *value {
                    *value = next;
                    return true;
                }
            }
            return false;
        }
        let mut drag = egui::DragValue::new(value).range(range).speed(1.0);
        if integer {
            drag = drag.max_decimals(0).prefix(format!("{label} "));
        } else {
            drag = drag.suffix(" ppi");
        }
        let response = ui.push_id(id, |ui| ui.add(drag)).inner;
        response.ctx.accesskit_node_builder(response.id, |node| node.set_label(label.to_owned()));
        response.changed()
    }

    fn add_version(&self, ui: &mut egui::Ui, label: &str) -> Option<ExportVersion> {
        let add = self.button(ui, label, true);
        let versions = version_templates();
        #[cfg(not(target_arch = "wasm32"))]
        if self.sdk {
            use craft_ui_widgets::contract::{Entry, Props as _, Trigger};
            let entries: Vec<_> = versions.iter().enumerate().map(|(i, version)| Entry::new(i, version.name.clone())).collect();
            let picked = craft_ui_widgets::menu::Menu::new(label.to_owned())
                .id(self.id(ui, "menu", label))
                .opened_by(&add.response, Trigger::Click)
                .pick(ui, &entries)
                .picked;
            return picked.and_then(|i| versions.get(i).cloned());
        }
        let mut picked = None;
        egui::Popup::menu(&add.response).show(|ui| {
            for version in &versions {
                if ui.button(&version.name).clicked() {
                    picked = Some(version.clone());
                    ui.close();
                }
            }
        });
        picked
    }
}

/// Export Layouts and Versions on the framework's dialog host.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const FORM: crate::dialogs::framework::Form = crate::dialogs::framework::Form::new(form).width(660.0);

#[cfg(not(target_arch = "wasm32"))]
fn form(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) -> Option<bool> {
    show(app, ui, d);
    None
}

/// The dialog's body: the framework form's on the desktop, the browser window's otherwise.
pub fn show(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        use craft_ui_widgets::contract::{Props as _, ShowContainer as _};
        let _ = craft_ui_widgets::runtime::inspect::record(ui, workflow_region(), |ui| {
            craft_ui_widgets::layout_components::Stack::new().id_salt("export_batch_body").fill_width().show(ui, |ui, _| show_body(app, ui, d));
        });
        return;
    }
    show_body(app, ui, d);
}

fn show_body(app: &mut DesignApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_string();
    #[cfg(not(target_arch = "wasm32"))]
    let sdk = app.framework_installed;
    #[cfg(target_arch = "wasm32")]
    let sdk = false;
    let controls = ExportControls { sdk, document: app.session.active().map(|st| (st.uid, st.identity_epoch)) };
    let mut version_generation = d.fields.get("_sdkVersionGeneration").and_then(Value::as_u64).unwrap_or(0);
    let mut job = job_of(d);
    let before = job.clone();
    let layout_names: Vec<String> = app.session.active().map(|st| st.doc.layouts().into_iter().map(|l| l.name).collect()).unwrap_or_default();

    // ---- Preset.
    let all = presets(app);
    let mut preset = d.s("preset");
    controls.row(ui, "export_preset", false, |ui| {
        controls.note(ui, &tr("Preset:"), Note::Plain);
        let current = all.iter().position(|p| p.name == preset).unwrap_or(usize::MAX);
        let names: Vec<String> = all.iter().map(|p| p.name.clone()).collect();
        if let Some(at) = controls.choice(ui, &tr("Preset:"), current, &names, 200.0, &tr("[Custom]")) {
            job = all[at].job.clone();
            preset = all[at].name.clone();
            version_generation = version_generation.wrapping_add(1);
        }
        let saved = app.session.prefs.export_presets.iter().any(|p| p.name == preset);
        if saved && controls.button(ui, &tr("Delete Preset"), true).clicked {
            match app.run("file.deleteExportPreset", json!({"name": preset})) {
                Ok(_) => preset.clear(),
                Err(e) => {
                    d.fields.insert("status".into(), json!(e));
                }
            }
        }
    });
    controls.row(ui, "export_preset_name", false, |ui| {
        let mut name = d.s("presetName");
        let changed = controls.text(ui, &tr("Preset name"), &mut name, 200.0, true);
        if changed {
            d.fields.insert("presetName".into(), json!(name));
        }
        if controls.button(ui, &tr("Save Preset"), !name.trim().is_empty()).clicked {
            match app.run("file.saveExportPreset", json!({"name": name.trim(), "job": job})) {
                Ok(_) => {
                    preset = name.trim().to_string();
                    d.fields.insert("presetName".into(), json!(""));
                    d.fields.insert("status".into(), json!(tr("Preset saved.")));
                }
                Err(e) => {
                    d.fields.insert("status".into(), json!(e));
                }
            }
        }
    });

    // ---- Layouts.
    controls.heading(ui, &tr("Layouts"));
    if layout_names.len() > 1 {
        controls.row(ui, "export_layouts", true, |ui| {
            for name in &layout_names {
                let mut on = job.layouts.is_empty() || job.layouts.contains(name);
                if controls.check(ui, name, &mut on) {
                    let mut chosen: Vec<String> = if job.layouts.is_empty() { layout_names.clone() } else { job.layouts.clone() };
                    if on {
                        chosen.push(name.clone());
                    } else {
                        chosen.retain(|x| x != name);
                    }
                    // In document order; every layout chosen is "all" (it follows new layouts too).
                    let ordered: Vec<String> = layout_names.iter().filter(|n| chosen.contains(n)).cloned().collect();
                    job.layouts = if ordered.len() == layout_names.len() {
                        vec![]
                    } else if ordered.is_empty() {
                        vec![String::new()]
                    } else {
                        ordered
                    };
                }
            }
        });
        controls.combine(ui, &mut job.combine, &tr("Layouts"), &tr("A file per layout"), &tr("All layouts in one PDF"));
    } else {
        controls.note(ui, &tr("The document has one layout: all its pages are exported."), Note::Muted);
    }

    // ---- Versions.
    controls.heading(ui, &tr("Versions"));
    let mut remove = None;
    let mut duplicate = None;
    let mut expanded: Vec<u64> = d.fields.get("expanded").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    for (i, v) in job.versions.iter_mut().enumerate() {
        controls.scope(ui, ("export_version", version_generation, i), |ui| {
            controls.row(ui, "export_version_line", false, |ui| {
                controls.text(ui, &tr("Folder name"), &mut v.name, 170.0, true);
                let formats = [ExportFormat::Pdf, ExportFormat::Png, ExportFormat::Jpeg];
                let names: Vec<String> = ["PDF", "PNG", "JPEG"].into_iter().map(str::to_string).collect();
                let current = formats.iter().position(|f| *f == v.format).unwrap_or(0);
                if let Some(at) = controls.choice(ui, &tr("Format"), current, &names, 70.0, "") {
                    v.format = formats[at];
                }
                match v.format {
                    ExportFormat::Pdf => {
                        let kind = if expanded.contains(&(i as u64)) { "custom" } else { pdf_kind(v) };
                        let label = |k: &str| match k {
                            "print" => tr("Print: marks + bleed"),
                            "bleed" => tr("Bleed, no marks"),
                            "screen" => tr("Screen"),
                            _ => tr("Custom"),
                        };
                        let kinds = ["print", "bleed", "screen", "custom"];
                        let names: Vec<String> = kinds.iter().map(|kind| label(kind)).collect();
                        let current = kinds.iter().position(|k| *k == kind).unwrap_or(3);
                        if let Some(at) = controls.choice(ui, &tr("PDF settings"), current, &names, 150.0, "") {
                            let k = kinds[at];
                            expanded.retain(|x| *x != i as u64);
                            if k == "custom" {
                                expanded.push(i as u64);
                            } else {
                                set_pdf_kind(v, k);
                            }
                        }
                    }
                    ExportFormat::Png | ExportFormat::Jpeg => {
                        controls.number(ui, &tr("Resolution"), &mut v.ppi, 36.0..=1200.0, false);
                        if v.format == ExportFormat::Jpeg {
                            let mut quality = f64::from(v.quality);
                            if controls.number(ui, tr("Quality ").trim_end(), &mut quality, 1.0..=100.0, true) {
                                let q = quality.round().clamp(1.0, 100.0) as u32;
                                v.quality = u8::try_from(q.clamp(1, 100)).unwrap_or(85);
                            }
                        }
                    }
                }
                if controls.small_button(ui, &tr("Duplicate"), &tr("Duplicate version")).clicked {
                    duplicate = Some(i);
                }
                if controls.remove_button(ui, &tr("Remove version")).clicked {
                    remove = Some(i);
                }
            });
            if v.format == ExportFormat::Pdf && (expanded.contains(&(i as u64)) || pdf_kind(v) == "custom") {
                controls.options(ui, "export_version_pdf", |ui| {
                    controls.check(ui, &tr("Bleed"), &mut v.bleed);
                    controls.check(ui, &tr("Crop Marks"), &mut v.marks.crop);
                    controls.check(ui, &tr("Bleed Marks"), &mut v.marks.bleed);
                    controls.check(ui, &tr("Page Information"), &mut v.marks.page_info);
                    controls.check(ui, &tr("Compress Images"), &mut v.compress_images);
                    controls.check(ui, &tr("Spreads"), &mut v.spreads);
                    let standards = ["none", "x4", "a2b"];
                    let names: Vec<String> = ["No standard", "PDF/X-4", "PDF/A-2b"].into_iter().map(str::to_string).collect();
                    let current = standards.iter().position(|s| *s == v.standard).unwrap_or(usize::MAX);
                    if let Some(at) = controls.choice(ui, &tr("PDF standard"), current, &names, 110.0, "No standard") {
                        v.standard = standards[at].into();
                    }
                });
            }
        });
    }
    if let Some(i) = duplicate
        && let Some(v) = job.versions.get(i).cloned()
    {
        let mut copy = v;
        copy.name = format!("{} copy", copy.name);
        job.versions.insert(i.saturating_add(1), copy);
        expanded.clear();
        version_generation = version_generation.wrapping_add(1);
    }
    if let Some(i) = remove
        && i < job.versions.len()
    {
        job.versions.remove(i);
        expanded.clear();
        version_generation = version_generation.wrapping_add(1);
    }
    d.fields.insert("expanded".into(), json!(expanded));
    controls.row(ui, "export_add_version", false, |ui| {
        if let Some(version) = controls.add_version(ui, &tr("+ Add Version")) {
            job.versions.push(version);
            version_generation = version_generation.wrapping_add(1);
        }
        controls.check(ui, &tr("Put each version in its own folder"), &mut job.folders);
    });

    // ---- File names.
    controls.heading(ui, &tr("File Names"));
    controls.row(ui, "export_file_names", false, |ui| {
        controls.text(ui, &tr("File Names"), &mut job.file_name, 240.0, false);
        for token in ["{document}", "{layout}", "{version}", "{date}"] {
            if controls.small_button(ui, token, &tr("Add to the name")).clicked {
                job.file_name.push_str(if job.file_name.is_empty() || job.file_name.ends_with(' ') { "" } else { " " });
                job.file_name.push_str(token);
            }
        }
    });

    // ---- Preview: the files it will write.
    if job != before {
        preset = all.iter().find(|p| p.job == job).map(|p| p.name.clone()).unwrap_or_default();
    }
    let key = serde_json::to_string(&job).unwrap_or_default();
    if d.s("previewFor") != key {
        let preview = if job.layouts.iter().any(String::is_empty) {
            Err(tr("Choose at least one layout."))
        } else {
            app.session.execute("file.exportBatch", &json!({"job": job, "dryRun": true})).map_err(|e| e.to_string())
        };
        let (files, error) = match preview {
            Ok(r) => {
                (r["files"].as_array().map(|a| a.iter().filter_map(|f| f["path"].as_str().map(str::to_string)).collect()).unwrap_or_default(), None)
            }
            Err(e) => (Vec::<String>::new(), Some(e)),
        };
        d.fields.insert("preview".into(), json!(files));
        d.fields.insert("previewError".into(), json!(error));
        d.fields.insert("previewFor".into(), json!(key));
    }
    let files: Vec<String> = d.fields.get("preview").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    match d.fields.get("previewError").and_then(Value::as_str) {
        Some(e) => controls.note(ui, e, Note::Error),
        None => {
            controls.note(ui, &tr("Files to export: {n}").replace("{n}", &files.len().to_string()), Note::Strong);
            controls.files(ui, &files);
        }
    }
    if let Some(status) = d.fields.get("status").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        controls.note(ui, status, Note::Muted);
    }
    d.fields.insert("job".into(), json!(job));
    d.fields.insert("preset".into(), json!(preset));
    d.fields.insert("_sdkVersionGeneration".into(), json!(version_generation));
}

/// The dialog `d` open again, saying `e`.
fn reopen(app: &mut DesignApp, d: &Dialog, e: &str) {
    let mut d = d.clone();
    d.fields.insert("status".into(), json!(e));
    app.ui.dialog = Some(d);
}

/// Export: ask for a folder (the web downloads each file), write every file, remember the job.
/// The folder dialog answers on a later frame; the export runs then.
pub fn confirm(app: &mut DesignApp, d: &Dialog) -> Result<Value, String> {
    let job = job_of(d);
    if job.layouts.iter().any(String::is_empty) {
        let e = crate::i18n::tr(&app.ui.language, "Choose at least one layout.").to_string();
        reopen(app, d, &e);
        return Err(e);
    }
    if job.versions.is_empty() {
        let e = crate::i18n::tr(&app.ui.language, "Add at least one version.").to_string();
        reopen(app, d, &e);
        return Err(e);
    }
    app.ui.export_batch = Some(job.clone());
    if app.services.download.is_some() {
        return export_files(app, d, job, None);
    }
    let title = crate::i18n::tr(&app.ui.language, "Export to Folder").to_string();
    let d = d.clone();
    app.ask_folder(&title, Bound::Document, move |app, dir| export_files(app, &d, job, Some(dir)))
}

/// Run `job` into folder `dir`, or (no folder: the web) download each file. A failure opens the
/// dialog `d` again, saying why.
fn export_files(app: &mut DesignApp, d: &Dialog, job: ExportJob, dir: Option<String>) -> Result<Value, String> {
    let mut params = json!({"job": job});
    if let Some(dir) = &dir {
        params["dir"] = json!(dir);
    }
    let r = match app.run("file.exportBatch", params) {
        Ok(r) => r,
        Err(e) => {
            reopen(app, d, &e);
            return Err(e);
        }
    };
    let files = r["files"].as_array().cloned().unwrap_or_default();
    if dir.is_none() {
        // No folders on the web: each file downloads as "Folder - Name.ext".
        let download = app.services.download.as_mut().ok_or("no download")?;
        for f in &files {
            let (Some(path), Some(b64)) = (f["path"].as_str(), f["base64"].as_str()) else { continue };
            download(&path.replace('/', " - "), &designcraft_engine::cmd::base64_decode(b64))?;
        }
    }
    app.ui.status = crate::i18n::tr(&app.ui.language, "Exported {n} files").replace("{n}", &files.len().to_string());
    Ok(r)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use egui_kittest::kittest::Queryable;
    use serde_json::json;

    use super::*;

    fn app() -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": 2, "facingPages": false, "width": 300, "height": 400, "title": "Mag"})).unwrap();
        app.run("layout.createAlternate", json!({"name": "Tablet H", "width": 400, "height": 300, "fromName": "Print"})).unwrap();
        app
    }

    #[test]
    fn the_dialog_lists_the_files_and_edits_the_versions() {
        let mut app = app();
        app.run("app.exportBatch", json!({})).unwrap();
        let d = app.ui.dialog.as_ref().unwrap();
        assert_eq!((d.id.as_str(), d.s("preset").as_str()), (ID, "Print + Screen"));
        let mut h = crate::test_window::open(app, egui::vec2(1400.0, 1000.0));
        h.run_steps(4);
        // Print + Screen: two PDFs a version, a PNG a page.
        assert!(h.query_by_label("Files to export: 8").is_some());
        assert!(h.query_by_label("PNG/Mag - Tablet H - 2.png").is_some());
        // Remove the PNG version, duplicate the first: the preset becomes [Custom].
        h.get_all_by_label("Remove version").last().unwrap().click();
        h.run_steps(3);
        h.get_all_by_label("Duplicate").next().unwrap().click();
        h.run_steps(3);
        let d = h.state().app.ui.dialog.clone().unwrap();
        let job = job_of(&d);
        assert_eq!(
            job.versions.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(),
            ["PDF Print (marks + bleed)", "PDF Print (marks + bleed) copy", "PDF Screen"]
        );
        assert_eq!(d.s("preset"), "");
        assert!(h.query_by_label("Files to export: 6").is_some());
        // One layout only.
        h.get_by_label("Tablet H").click();
        h.run_steps(3);
        assert!(h.query_by_label("Files to export: 3").is_some());
        assert_eq!(job_of(h.state().app.ui.dialog.as_ref().unwrap()).layouts, ["Print"]);
    }

    #[test]
    fn export_writes_into_the_chosen_folder_and_remembers_the_job() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app();
        app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(Some(dir.path().to_string_lossy().to_string())));
        open(&mut app, Some(vec!["Tablet H".into()]));
        // The folder dialog answers on a later frame.
        assert_eq!(crate::dialogs::confirm(&mut app).unwrap(), Value::Null);
        assert!(!dir.path().join("PNG").exists());
        app.settle_dialogs();
        assert_eq!(app.ui.status, "Exported 4 files");
        assert!(dir.path().join("PDF Print (marks + bleed)/Mag - Tablet H.pdf").is_file());
        assert!(dir.path().join("PNG/Mag - Tablet H - 1.png").is_file());
        assert_eq!(app.ui.export_batch.as_ref().unwrap().layouts, ["Tablet H"]);
        // Reopened, it is the same job.
        open(&mut app, None);
        assert_eq!(job_of(app.ui.dialog.as_ref().unwrap()).layouts, ["Tablet H"]);
    }

    #[test]
    fn a_cancelled_folder_writes_nothing_and_the_web_downloads_each_file() {
        let written = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut app = app();
        app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(None));
        open(&mut app, None);
        assert_eq!(crate::dialogs::confirm(&mut app).unwrap(), Value::Null);
        app.settle_dialogs();
        assert!(!app.ui.status.starts_with("Exported"), "{}", app.ui.status);
        let w = written.clone();
        app.services.download = Some(Box::new(move |p: &str, _: &[u8]| {
            w.lock().unwrap().push(p.to_string());
            Ok(())
        }));
        open(&mut app, Some(vec!["Print".into()]));
        crate::dialogs::confirm(&mut app).unwrap();
        let names = written.lock().unwrap().clone();
        assert_eq!(names.len(), 1 + 1 + 2);
        assert!(names.contains(&"PDF Screen - Mag - Print.pdf".to_string()), "{names:?}");
    }

    #[test]
    fn presets_are_saved_and_chosen() {
        let mut app = app();
        open(&mut app, None);
        let mut h = crate::test_window::open(app, egui::vec2(1400.0, 1000.0));
        h.run_steps(3);
        if let Some(d) = h.state_mut().app.ui.dialog.as_mut() {
            d.fields.insert("presetName".into(), json!("Client"));
        }
        h.run_steps(2);
        h.get_by_label("Save Preset").click();
        h.run_steps(3);
        let app = &h.state().app;
        assert_eq!(app.session.prefs.export_presets.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Client"]);
        assert_eq!(app.ui.dialog.as_ref().unwrap().s("preset"), "Client");
        assert!(h.query_by_label("Delete Preset").is_some());
    }

    #[test]
    fn print_with_no_layout_chosen_keeps_the_dialog_open_and_says_why() {
        let mut app = app();
        crate::dialogs::open_print(&mut app);
        if let Some(d) = app.ui.dialog.as_mut() {
            d.fields.insert("range".into(), json!("layouts"));
            d.fields.insert("layouts".into(), json!([]));
        }
        // Refused before anything is printed.
        assert!(crate::dialogs::confirm(&mut app).is_err());
        let d = app.ui.dialog.as_ref().unwrap();
        assert_eq!((d.id.as_str(), d.s("status").as_str()), ("print", "Choose at least one layout."));
    }

    #[test]
    fn clicking_a_layout_header_leaves_edit_parents() {
        let mut app = app();
        app.ui.dock_tab = "pages".into();
        app.run("layout.parents.edit", json!({"on": true})).unwrap();
        let mut h = crate::test_window::open(app, egui::vec2(1400.0, 1000.0));
        h.run_steps(3);
        h.get_by_label("Tablet H").click();
        h.run_steps(4);
        let app = &h.state().app;
        assert!(!app.session.active().unwrap().editing_parents);
        assert_eq!(crate::canvas::current_page(app), Some(2));
    }

    #[test]
    fn no_layout_chosen_keeps_the_dialog_open() {
        let mut app = app();
        open(&mut app, None);
        if let Some(d) = app.ui.dialog.as_mut() {
            let mut job = job_of(d);
            job.layouts = vec![String::new()];
            d.fields.insert("job".into(), json!(job));
        }
        assert!(crate::dialogs::confirm(&mut app).is_err());
        assert!(app.ui.dialog.as_ref().is_some_and(|d| d.id == ID));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod ui223_export_batch_workflow_tests {
    use super::*;
    use crate::theme::Tokens;
    use egui::{Event, Key, Modifiers, PointerButton};
    use egui_kittest::{Harness, kittest::Queryable};

    fn app_with(pages: usize, alternate: &str) -> DesignApp {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"pages": pages, "facingPages": false, "width": 300, "height": 400, "title": "Mag"})).unwrap();
        app.run("layout.createAlternate", json!({"name": alternate, "width": 400, "height": 300, "fromName": "Print"})).unwrap();
        open(&mut app, None);
        app
    }

    fn app() -> DesignApp {
        app_with(2, "Tablet H")
    }

    /// Body cases call the real production body with its actual Dialog. Only explicit host
    /// cases call dialogs::show, whose framework host draws Export and Cancel.
    fn harness(mut app: DesignApp, sdk: bool, native_host: bool) -> Harness<'static, DesignApp> {
        let ctx = egui::Context::default();
        if sdk {
            app.framework_opacity = true;
            app.install_framework(&ctx);
            assert!(app.framework_installed);
        }
        let mut ready = false;
        let mut locale_fonts_ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(egui::vec2(1100.0, 1050.0)).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                    crate::theme::apply(ui.ctx(), &Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                if !locale_fonts_ready {
                    locale_fonts_ready = true;
                    if app.ui.language == "ar" {
                        install_arabic_fixture(ui.ctx(), &app.ui.language);
                        return;
                    }
                }
                if native_host {
                    crate::dialogs::show(app, ui.ctx());
                } else if let Some(mut dialog) = app.ui.dialog.take() {
                    show(app, ui, &mut dialog);
                    app.ui.dialog = Some(dialog);
                }
            },
            app,
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 3);
        h
    }

    fn dialog<'a>(h: &'a Harness<'_, DesignApp>) -> &'a Dialog {
        h.state().ui.dialog.as_ref().unwrap()
    }
    fn job(h: &Harness<'_, DesignApp>) -> ExportJob {
        job_of(dialog(h))
    }
    fn undo(h: &Harness<'_, DesignApp>) -> usize {
        h.state().session.active().unwrap().history.undo.len()
    }
    // A completed non-dry-run journal record, not a failed-attempt or OS-IO spy.
    fn actual_exports(h: &Harness<'_, DesignApp>) -> usize {
        h.state().session.journal.iter().filter(|(id, payload)| id == "file.exportBatch" && payload["dryRun"] != true).count()
    }
    fn last_payload(h: &Harness<'_, DesignApp>, command: &str) -> Value {
        h.state().session.journal.iter().rev().find(|(id, _)| id == command).unwrap().1.clone()
    }
    fn sdk_element(h: &Harness<'_, DesignApp>, component: &str, label: &str, at: usize) -> craft_ui_widgets::runtime::inspect::Element {
        craft_ui_widgets::runtime::inspect::recorded(&h.ctx, workflow_region())
            .unwrap()
            .elements
            .iter()
            .filter(|e| e.part.is_none() && e.component == component && e.facts.as_ref().and_then(|f| f.name.as_deref()) == Some(label))
            .nth(at)
            .unwrap_or_else(|| panic!("missing actual SDK {component} named {label:?} at {at}"))
            .clone()
    }
    fn sdk_click(h: &mut Harness<'_, DesignApp>, component: &str, label: &str, at: usize) {
        let element = sdk_element(h, component, label, at);
        let recorded = craft_ui_widgets::runtime::inspect::recorded(&h.ctx, workflow_region()).unwrap();
        assert!(
            recorded.hits.iter().any(|hit| hit.owner == element.id && hit.rect.intersects(element.rect)),
            "the actual component must own a hit area"
        );
        click_rect(h, element.rect);
    }
    fn choose(h: &mut Harness<'_, DesignApp>, label: &str, at: usize, value: &str, sdk: bool) {
        if sdk {
            sdk_click(h, "Select", label, at);
        } else {
            assert_eq!(at, 0);
            let rect = h.get_by_role_and_label(egui::accesskit::Role::ComboBox, label).rect();
            click_rect(h, rect);
        }
        let rect = h.get_all_by_label(value).last().unwrap().rect();
        click_rect(h, rect);
    }
    fn type_text(h: &mut Harness<'_, DesignApp>, label: &str, at: usize, value: &str, sdk: bool) {
        let rect = if sdk {
            let element = sdk_element(h, "TextField", label, at);
            assert_eq!(element.facts.as_ref().unwrap().role, "TextInput");
            element.rect
        } else {
            assert_eq!(at, 0);
            h.get_by_role_and_label(egui::accesskit::Role::TextInput, label).rect()
        };
        click_rect(h, rect);
        key_mod(h, Key::A, Modifiers::COMMAND);
        input(h, Some(Event::Text(value.into())));
        idle(h, 2);
    }
    fn type_number(h: &mut Harness<'_, DesignApp>, label: &str, at: usize, value: &str) {
        let element = sdk_element(h, "NumberField", label, at);
        assert_eq!(element.facts.as_ref().unwrap().role, "SpinButton");
        click_rect(h, element.rect);
        key_mod(h, Key::A, Modifiers::COMMAND);
        input(h, Some(Event::Text(value.into())));
        key(h, Key::Enter);
    }
    fn no_idle_replay(h: &mut Harness<'_, DesignApp>) {
        let journal = h.state().session.journal.clone();
        let document = h.state().session.active().unwrap().doc.clone();
        let steps = undo(h);
        idle(h, 5);
        assert_eq!(h.state().session.journal, journal);
        assert_eq!(h.state().session.active().unwrap().doc, document);
        assert_eq!(undo(h), steps);
        assert_eq!(actual_exports(h), 0);
    }
    /// A real installed Arabic face is an explicit native fixture precondition, not an OS
    /// package install or a synthetic glyph map. The app's current SYSTEM_UI_FONTS list lacks
    /// Arab without optional CRAFT_FONTS; retain all its current definitions and add this face
    /// using the app's existing FontDb and baseline-aware fallback API.
    fn install_arabic_fixture(ctx: &egui::Context, language: &str) {
        #[cfg(target_os = "macos")]
        let family = "Geeza Pro";
        #[cfg(target_os = "windows")]
        let family = "Segoe UI";
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let family = "DejaVu Sans";
        let db = designcraft_fonts::FontDb::global();
        assert!(db.has_family(family), "the native Arabic fixture requires the already-installed {family}; do not skip Arabic or substitute tofu");
        let face = db.face(family, "Regular");
        assert!(face.family.eq_ignore_ascii_case(family), "the named fixture must resolve to {family}, not {}", face.family);
        assert!(
            !face.is_placeholder() && face.covers('\u{0627}') && face.covers('\u{0625}'),
            "the Arabic face must contain real U+0627 and U+0625 glyphs"
        );
        for key in [
            "Preset:",
            "Preset name",
            "Save Preset",
            "Delete Preset",
            "Layouts",
            "Versions",
            "Folder name",
            "Format",
            "PDF settings",
            "Print: marks + bleed",
            "Bleed, no marks",
            "Screen",
            "Custom",
            "Bleed",
            "Crop Marks",
            "Bleed Marks",
            "Page Information",
            "Compress Images",
            "Spreads",
            "PDF standard",
            "Resolution",
            "Quality",
            "Duplicate",
            "Remove version",
            "+ Add Version",
            "Put each version in its own folder",
            "File Names",
            "A file per layout",
            "All layouts in one PDF",
            "Cancel",
            "Export…",
            "مخطط الهاتف",
            "نسخة Café–α",
        ] {
            for c in crate::i18n::tr(language, key).chars().filter(|c| designcraft_fonts::is_rtl(*c) && c.is_alphabetic()) {
                assert!(face.covers(c), "{family} lacks the real glyph U+{:04X} from {key}", u32::from(c));
            }
        }
        for c in "مخطط الهاتف نسخة Café–α".chars().filter(|c| designcraft_fonts::is_rtl(*c) && c.is_alphabetic()) {
            assert!(face.covers(c), "{family} lacks the real fixture-name glyph U+{:04X}", u32::from(c));
        }
        let mut definitions = ctx.fonts(|fonts| fonts.definitions().clone());
        let fallback = crate::theme::SystemFont::new("Arab", face.family.clone(), face.data().to_vec(), face.index());
        crate::theme::add_system_fallbacks(&mut definitions, &[fallback], language);
        ctx.set_fonts(definitions);
        eprintln!(
            "Export Arabic fixture: real existing {} {}, face {}, {} font bytes, explicit Export alphabetic-glyph coverage checked",
            face.family,
            face.style,
            face.index(),
            face.data().len()
        );
    }

    fn input(h: &mut Harness<'_, DesignApp>, event: Option<Event>) {
        let mut raw = std::mem::take(h.input_mut());
        if let Some(event) = event {
            raw.events.push(event);
        }
        let ctx = h.ctx.clone();
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }

    fn idle(h: &mut Harness<'_, DesignApp>, n: usize) {
        for _ in 0..n {
            input(h, None);
        }
    }

    fn click_rect(h: &mut Harness<'_, DesignApp>, rect: egui::Rect) {
        let pos = rect.center();
        input(h, Some(Event::PointerMoved(pos)));
        for pressed in [true, false] {
            input(h, Some(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }));
        }
        idle(h, 3);
    }

    fn click(h: &mut Harness<'_, DesignApp>, label: &str) {
        let rect = h.get_by_label(label).rect();
        click_rect(h, rect);
    }

    fn key_mod(h: &mut Harness<'_, DesignApp>, key: Key, modifiers: Modifiers) {
        for pressed in [true, false] {
            input(h, Some(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }));
        }
        input(h, Some(Event::ModifiersChanged(Modifiers::NONE)));
        idle(h, 2);
    }

    fn key(h: &mut Harness<'_, DesignApp>, key: Key) {
        key_mod(h, key, Modifiers::NONE);
    }

    #[test]
    fn ui223_export_body_presets_custom_save_delete_and_disabled_save() {
        let mut h = harness(app(), true, false);
        let steps = undo(&h);
        let disabled = sdk_element(&h, "Button", "Save Preset", 0);
        assert!(disabled.facts.as_ref().unwrap().disabled);
        click_rect(&mut h, disabled.rect);
        assert!(h.state().session.prefs.export_presets.is_empty());
        choose(&mut h, "Preset:", 0, "Images per Layout", true);
        assert_eq!(job(&h), builtin_presets().into_iter().find(|p| p.name == "Images per Layout").unwrap().job);
        type_text(&mut h, "File Names", 0, "Client {document}", true);
        assert!(dialog(&h).s("preset").is_empty());
        type_text(&mut h, "Preset name", 0, "  Client Café  ", true);
        let saved = job(&h);
        // Natural Tab from the real name field, followed by Space on the actual Save button.
        key(&mut h, Key::Tab);
        let save = sdk_element(&h, "Button", "Save Preset", 0);
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(save.id));
        key(&mut h, Key::Space);
        assert_eq!(last_payload(&h, "file.saveExportPreset"), json!({"name": "Client Café", "job": saved}));
        assert_eq!(dialog(&h).s("preset"), "Client Café");
        assert_eq!(h.state().session.prefs.export_presets.len(), 1);
        sdk_click(&mut h, "Button", "Delete Preset", 0);
        assert_eq!(last_payload(&h, "file.deleteExportPreset"), json!({"name": "Client Café"}));
        assert!(h.state().session.prefs.export_presets.is_empty());
        assert_eq!(job(&h), saved);
        assert!(dialog(&h).s("preset").is_empty());
        assert_eq!(undo(&h), steps);
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_export_body_keyboard_dropdown_preview_cancel_then_commit() {
        let mut h = harness(app(), true, false);
        let original = job(&h);
        let journal = h.state().session.journal.clone();
        sdk_click(&mut h, "Select", "Preset:", 0);
        key(&mut h, Key::ArrowDown);
        assert_eq!(job(&h), original, "a dropdown lead is not an accepted preset");
        assert_eq!(h.state().session.journal, journal, "previewing must not plan a different job");
        key(&mut h, Key::Escape);
        assert_eq!(job(&h), original);
        assert_eq!(dialog(&h).s("preset"), "Print + Screen");
        // These are body/SDK popup keys, not acceptance of dialogs.rs' native footer keys.
        key(&mut h, Key::Space);
        key(&mut h, Key::End);
        key(&mut h, Key::Enter);
        assert_eq!(job(&h), builtin_presets().into_iter().last().unwrap().job);
        assert_eq!(dialog(&h).s("preset"), "All Layouts in One PDF");
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_export_body_layout_sentinel_order_and_combine_radios() {
        let mut h = harness(app(), true, false);
        let steps = undo(&h);
        assert_eq!(sdk_element(&h, "Checkbox", "Tablet H", 0).facts.as_ref().unwrap().role, "CheckBox");
        sdk_click(&mut h, "Checkbox", "Tablet H", 0);
        assert_eq!(job(&h).layouts, ["Print"]);
        sdk_click(&mut h, "Checkbox", "Print", 0);
        assert_eq!(job(&h).layouts, [""]);
        assert_eq!(dialog(&h).s("previewError"), "Choose at least one layout.");
        sdk_click(&mut h, "Checkbox", "Print", 0);
        assert_eq!(job(&h).layouts, ["Print"]);
        sdk_click(&mut h, "Checkbox", "Tablet H", 0);
        assert!(job(&h).layouts.is_empty(), "all layouts retains the follow-new-layouts policy");
        let radio = sdk_element(&h, "Radio", "All layouts in one PDF", 0);
        assert_eq!(radio.facts.as_ref().unwrap().role, "RadioButton");
        sdk_click(&mut h, "Radio", "All layouts in one PDF", 0);
        assert!(job(&h).combine);
        assert_eq!(dialog(&h).fields["preview"].as_array().unwrap().len(), 6);
        sdk_click(&mut h, "Radio", "A file per layout", 0);
        assert!(!job(&h).combine);
        assert_eq!(dialog(&h).fields["preview"].as_array().unwrap().len(), 8);
        assert_eq!(undo(&h), steps);
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_export_body_pdf_custom_marks_standard_and_quick_settings() {
        let mut h = harness(app(), true, false);
        choose(&mut h, "Preset:", 0, "All Layouts in One PDF", true);
        choose(&mut h, "PDF settings", 0, "Custom", true);
        for label in ["Bleed", "Crop Marks", "Bleed Marks", "Page Information", "Compress Images", "Spreads"] {
            sdk_click(&mut h, "Checkbox", label, 0);
        }
        choose(&mut h, "PDF standard", 0, "PDF/X-4", true);
        let version = job(&h).versions.remove(0);
        assert!(!version.bleed && version.marks.crop && version.marks.bleed && version.marks.page_info && version.compress_images && version.spreads);
        assert_eq!(version.standard, "x4");
        choose(&mut h, "PDF settings", 0, "Screen", true);
        let version = job(&h).versions.remove(0);
        assert!(
            !version.bleed && !version.marks.crop && !version.marks.bleed && !version.marks.page_info && version.compress_images && !version.spreads
        );
        assert_eq!(version.standard, "none");
        assert_eq!(pdf_kind(&version), "screen");
        assert!(dialog(&h).fields["expanded"].as_array().unwrap().is_empty());
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_export_body_formats_numbers_duplicate_remove_add_and_tokens() {
        let mut h = harness(app(), true, false);
        let steps = undo(&h);
        choose(&mut h, "Preset:", 0, "All Layouts in One PDF", true);
        choose(&mut h, "Format", 0, "JPEG", true);
        type_number(&mut h, "Resolution", 0, "240");
        type_number(&mut h, "Quality", 0, "73");
        key(&mut h, Key::ArrowUp);
        assert_eq!((job(&h).versions[0].ppi, job(&h).versions[0].quality), (240.0, 74));
        let original = job(&h).versions[0].clone();
        sdk_click(&mut h, "Button", "Duplicate", 0);
        assert_eq!(job(&h).versions.len(), 2);
        let mut copy = original.clone();
        copy.name = format!("{} copy", original.name);
        assert_eq!(job(&h).versions[1], copy);
        assert!(dialog(&h).fields["expanded"].as_array().unwrap().is_empty());
        assert_eq!(sdk_element(&h, "IconButton", "Remove version", 1).facts.as_ref().unwrap().role, "Button");
        sdk_click(&mut h, "IconButton", "Remove version", 1);
        assert_eq!(job(&h).versions, [original]);
        sdk_click(&mut h, "Button", "+ Add Version", 0);
        assert_eq!(job(&h).versions.len(), 1, "opening a real menu is not adding a version");
        click(&mut h, "PNG");
        assert_eq!(job(&h).versions[1], version_templates().into_iter().find(|v| v.name == "PNG").unwrap());
        sdk_click(&mut h, "Checkbox", "Put each version in its own folder", 0);
        assert!(job(&h).folders);
        type_text(&mut h, "File Names", 0, "{document}", true);
        for token in ["{layout}", "{version}", "{date}"] {
            sdk_click(&mut h, "Button", token, 0);
        }
        assert_eq!(job(&h).file_name, "{document} {layout} {version} {date}");
        assert!(dialog(&h).fields["preview"].as_array().unwrap().iter().all(|p| p.as_str().unwrap().contains('/')));
        assert_eq!(undo(&h), steps);
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_export_body_structural_reindex_retires_old_field_identity() {
        let mut h = harness(app(), true, false);
        type_text(&mut h, "Folder name", 1, "Client Screen", true);
        let old_screen = sdk_element(&h, "TextField", "Folder name", 1).id;
        let old_png = sdk_element(&h, "TextField", "Folder name", 2).id;
        sdk_click(&mut h, "IconButton", "Remove version", 0);
        assert_eq!(job(&h).versions.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["Client Screen", "PNG"]);
        assert_ne!(sdk_element(&h, "TextField", "Folder name", 0).id, old_screen);
        assert_ne!(sdk_element(&h, "TextField", "Folder name", 1).id, old_png);
        type_text(&mut h, "Folder name", 1, "PNG client", true);
        sdk_click(&mut h, "Button", "Duplicate", 1);
        assert_eq!(job(&h).versions.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["Client Screen", "PNG client", "PNG client copy"]);
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_export_body_held_document_action_cannot_follow_new_document() {
        let mut h = harness(app(), true, false);
        choose(&mut h, "Preset:", 0, "All Layouts in One PDF", true);
        let old = sdk_element(&h, "Button", "Duplicate", 0);
        let pos = old.rect.center();
        input(&mut h, Some(Event::PointerMoved(pos)));
        input(&mut h, Some(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }));
        h.state_mut().run("file.new", json!({"pages": 2, "facingPages": false, "width": 300, "height": 400, "title": "Other"})).unwrap();
        h.state_mut().run("layout.createAlternate", json!({"name": "Tablet H", "width": 400, "height": 300, "fromName": "Print"})).unwrap();
        idle(&mut h, 3);
        let new = sdk_element(&h, "Button", "Duplicate", 0);
        assert_ne!(new.id, old.id);
        assert!(new.rect.contains(pos), "the new action must really be under the held pointer, not merely move away");
        input(&mut h, Some(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }));
        idle(&mut h, 3);
        assert_eq!(job(&h).versions.len(), 1);
        sdk_click(&mut h, "Button", "Duplicate", 0);
        assert_eq!(job(&h).versions.len(), 2, "a fresh action still works in the new document");
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_export_body_builtin_and_long_preset_refusals_preserve_drafts() {
        let mut h = harness(app(), true, false);
        let original = job(&h);
        type_text(&mut h, "Preset name", 0, "Print + Screen", true);
        sdk_click(&mut h, "Button", "Save Preset", 0);
        assert!(dialog(&h).s("status").contains("built-in"));
        assert_eq!(dialog(&h).s("presetName"), "Print + Screen");
        let too_long = "é".repeat(81);
        type_text(&mut h, "Preset name", 0, &too_long, true);
        sdk_click(&mut h, "Button", "Save Preset", 0);
        assert!(dialog(&h).s("status").contains("80"));
        assert_eq!(dialog(&h).s("presetName"), too_long);
        assert!(h.state().session.prefs.export_presets.is_empty());
        assert_eq!(job(&h), original);
        type_text(&mut h, "Preset name", 0, "   ", true);
        let disabled = sdk_element(&h, "Button", "Save Preset", 0);
        assert!(disabled.facts.as_ref().unwrap().disabled);
        click_rect(&mut h, disabled.rect);
        assert!(h.state().session.prefs.export_presets.is_empty());
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_export_body_preview_cap_is_real_scrolled_content_not_export_io() {
        let mut h = harness(app_with(51, "Tablet H"), true, false);
        choose(&mut h, "Preset:", 0, "Images per Layout", true);
        let paths = dialog(&h).fields["preview"].as_array().unwrap().clone();
        assert_eq!(paths.len(), 204);
        assert_eq!(PREVIEW_ROWS, 200);
        let top = h.get_by_label(paths[0].as_str().unwrap()).rect().center();
        input(&mut h, Some(Event::PointerMoved(top)));
        input(
            &mut h,
            Some(Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -10000.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: Modifiers::NONE,
            }),
        );
        idle(&mut h, 8);
        assert!(h.query_by_label("… 4").is_some());
        assert!(h.query_by_label(paths[PREVIEW_ROWS - 1].as_str().unwrap()).is_some());
        assert!(h.query_by_label(paths.last().unwrap().as_str().unwrap()).is_none(), "rows beyond 200 must not be rendered even at the bottom");
        assert_eq!(actual_exports(&h), 0);
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_export_body_rtl_names_long_unicode_and_template_refusal() {
        let mut app = app_with(2, "مخطط الهاتف");
        app.ui.language = "ar".into();
        let mut h = harness(app, true, false);
        let tr = |key: &str| crate::i18n::tr("ar", key).to_string();
        let layout = sdk_element(&h, "Checkbox", "مخطط الهاتف", 0);
        assert_eq!(layout.facts.as_ref().unwrap().name.as_deref(), Some("مخطط الهاتف"));
        sdk_click(&mut h, "Checkbox", "مخطط الهاتف", 0);
        assert_eq!(job(&h).layouts, ["Print"]);
        let long = "نسخة Café–α ".repeat(18);
        type_text(&mut h, &tr("Folder name"), 0, &long, true);
        assert_eq!(job(&h).versions[0].name, long);
        assert!(dialog(&h).fields["previewError"].is_null(), "the engine safely normalizes long output names, not the UI draft");
        let invalid = "x".repeat(401);
        type_text(&mut h, &tr("File Names"), 0, &invalid, true);
        assert_eq!(job(&h).file_name, invalid);
        assert!(dialog(&h).s("previewError").contains("too long"));
        type_text(&mut h, &tr("File Names"), 0, "{document} {layout}", true);
        assert!(dialog(&h).fields["previewError"].is_null());
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_export_body_native_fallback_keeps_complete_editing_routes() {
        let mut h = harness(app(), false, false);
        assert!(craft_ui_widgets::runtime::inspect::recorded(&h.ctx, workflow_region()).is_none());
        choose(&mut h, "Preset:", 0, "All Layouts in One PDF", false);
        type_text(&mut h, "Folder name", 0, "Client Café", false);
        choose(&mut h, "Format", 0, "PNG", false);
        assert_eq!(job(&h).versions[0].format, ExportFormat::Png);
        assert_eq!(job(&h).versions[0].name, "Client Café");
        click(&mut h, "Duplicate");
        assert_eq!(job(&h).versions[1].name, "Client Café copy");
        let remove = h.get_all_by_label("Remove version").last().unwrap().rect();
        click_rect(&mut h, remove);
        click(&mut h, "+ Add Version");
        click(&mut h, "JPEG");
        assert_eq!(job(&h).versions[1].format, ExportFormat::Jpeg);
        click(&mut h, "Put each version in its own folder");
        type_text(&mut h, "File Names", 0, "Client", false);
        click(&mut h, "{layout}");
        assert_eq!(job(&h).file_name, "Client {layout}");
        assert!(job(&h).folders);
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_export_host_pointer_cancel_has_no_export_or_preference_side_effect() {
        let mut h = harness(app(), true, true);
        let document = h.state().session.active().unwrap().doc.clone();
        let steps = undo(&h);
        let preferences = h.state().session.prefs.export_presets.clone();
        assert!(sdk_element(&h, "Button", "Save Preset", 0).facts.as_ref().unwrap().disabled);
        idle(&mut h, 5);
        click(&mut h, "Cancel");
        assert!(h.state().ui.dialog.is_none());
        assert!(h.state().ui.export_batch.is_none());
        assert_eq!(h.state().session.active().unwrap().doc, document);
        assert_eq!(h.state().session.prefs.export_presets, preferences);
        assert_eq!(undo(&h), steps);
        assert_eq!(actual_exports(&h), 0);
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_export_host_empty_versions_refused_then_pointer_cancel() {
        let mut h = harness(app(), true, true);
        for _ in 0..3 {
            sdk_click(&mut h, "IconButton", "Remove version", 0);
        }
        assert!(job(&h).versions.is_empty());
        assert!(dialog(&h).s("previewError").contains("version"));
        click(&mut h, "Export…");
        assert!(h.state().ui.dialog.is_some());
        assert_eq!(dialog(&h).s("status"), "Add at least one version.");
        assert!(h.state().ui.export_batch.is_none());
        assert_eq!(actual_exports(&h), 0);
        click(&mut h, "Cancel");
        assert!(h.state().ui.dialog.is_none());
        no_idle_replay(&mut h);
    }

    #[test]
    fn ui223_export_host_queued_folder_cancel_remembers_without_exporting() {
        let mut app = app();
        app.answer_dialogs(crate::app::file_dialogs::testing::answer_with(None));
        let mut h = harness(app, true, true);
        let chosen = job(&h);
        click(&mut h, "Export…");
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(h.state().ui.export_batch.as_ref(), Some(&chosen));
        assert_eq!(actual_exports(&h), 0, "the folder callback has not run an export during rendering");
        h.state_mut().settle_dialogs();
        idle(&mut h, 3);
        assert!(!h.state().ui.status.starts_with("Exported"));
        assert_eq!(actual_exports(&h), 0);
        no_idle_replay(&mut h);
    }
}
