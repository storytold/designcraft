//! View › Separations Preview on the canvas: each visible spread exported as Export PDF prints
//! it and separated into plates (`designcraft_engine::separations`), drawn over the spread.
//!
//! Sheets are kept per spread, keyed by document revision, resolution, print settings and color
//! settings (the CMYK working space is the printing condition plates are separated for), so
//! panning and redrawing reuse them. Separating runs on a background thread (native); while it
//! runs the previous sheet of that spread stays on screen. On the web it runs on the UI thread
//! at a lower resolution.

use std::sync::Arc;

use designcraft_doc::SpreadRef;
use designcraft_engine::separations::{MAX_DPI, MIN_DPI, Print, Sheet};
use designcraft_geom::Point;
use designcraft_tools::CanvasLayout;
use egui::{Color32, pos2};

use crate::DesignApp;
use crate::canvas::Xf;

/// View › Separations Preview settings (`None` in `UiState::separations` is off).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Separations {
    /// The ink shown alone as film; `None` shows every ink as printed.
    pub plate: Option<String>,
    /// Ink Limit: areas whose total ink is over this percentage show in red over gray.
    pub ink_limit: Option<f32>,
}

/// Sheets kept at most (each holds one plane per ink).
const MAX_SHEETS: usize = 6;
/// The highest resolution the web build separates at (on the UI thread).
#[cfg(target_arch = "wasm32")]
const WEB_MAX_DPI: f32 = 72.0;

/// What a sheet was separated from.
#[derive(Clone, Debug, PartialEq)]
pub struct Key {
    pub uid: u64,
    pub revision: u64,
    pub spread: usize,
    pub dpi: f32,
    pub print: Print,
    /// The color settings it was separated under ([`designcraft_color::cms::generation`]).
    pub cms: u64,
}

struct Entry {
    key: Key,
    sheet: Result<Arc<Sheet>, String>,
    /// The texture of the current view of `sheet`.
    texture: Option<(Separations, egui::TextureHandle)>,
    /// The frame it was last drawn in.
    used: u64,
}

struct Job {
    key: Key,
    doc: Arc<designcraft_doc::Document>,
    cache: Arc<designcraft_compose::Cache>,
}

struct Done {
    key: Key,
    sheet: Result<Sheet, String>,
    ms: f64,
}

#[cfg(not(target_arch = "wasm32"))]
struct Worker {
    tx: std::sync::mpsc::Sender<Job>,
    rx: std::sync::mpsc::Receiver<Done>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Worker {
    fn spawn(ctx: egui::Context) -> Option<Self> {
        let (tx, jobs) = std::sync::mpsc::channel::<Job>();
        let (done_tx, rx) = std::sync::mpsc::channel::<Done>();
        std::thread::Builder::new()
            .name("designcraft-separations".into())
            .spawn(move || {
                while let Ok(first) = jobs.recv() {
                    // Of the jobs waiting, only the newest per spread matters.
                    let mut batch = vec![first];
                    while let Ok(next) = jobs.try_recv() {
                        batch.retain(|j| j.key.spread != next.key.spread);
                        batch.push(next);
                    }
                    for job in batch {
                        let done = separate(job);
                        if done_tx.send(done).is_err() {
                            return;
                        }
                        ctx.request_repaint();
                    }
                }
            })
            .ok()?;
        Some(Worker { tx, rx })
    }
}

fn separate(job: Job) -> Done {
    let t0 = crate::now_ms();
    let sheet =
        designcraft_engine::separations::separate(&job.doc, &job.cache, job.key.spread, job.key.dpi, &job.key.print).map_err(|e| e.to_string());
    Done { key: job.key, sheet, ms: crate::now_ms() - t0 }
}

/// The canvas's separated sheets.
#[derive(Default)]
pub struct Preview {
    entries: Vec<Entry>,
    pending: Vec<Key>,
    #[cfg(not(target_arch = "wasm32"))]
    worker: Option<Worker>,
    #[cfg(not(target_arch = "wasm32"))]
    worker_started: bool,
    frame: u64,
    /// The spread and the point in it under the pointer, for the panel's readout.
    pub pointer: Option<(usize, Point)>,
    /// How long the last separation took, in milliseconds.
    pub last_ms: f64,
    /// The pixels per point the canvas last drew at (0 before it drew).
    ppp: f64,
}

impl Preview {
    fn insert(&mut self, key: Key, sheet: Result<Arc<Sheet>, String>) {
        self.pending.retain(|k| *k != key);
        let frame = self.frame;
        match self.entries.iter_mut().find(|e| e.key.spread == key.spread) {
            Some(e) => *e = Entry { key, sheet, texture: None, used: frame },
            None => self.entries.push(Entry { key, sheet, texture: None, used: frame }),
        }
        if self.entries.len() > MAX_SHEETS {
            self.entries.sort_by_key(|e| std::cmp::Reverse(e.used));
            self.entries.truncate(MAX_SHEETS);
        }
    }

    /// Collects finished separations.
    fn poll(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut done = Vec::new();
            if let Some(w) = &self.worker {
                while let Ok(d) = w.rx.try_recv() {
                    done.push(d);
                }
            }
            for d in done {
                self.last_ms = d.ms;
                self.insert(d.key, d.sheet.map(Arc::new));
            }
        }
    }

    /// Asks for `key` to be separated, unless it is already or is being.
    fn request(&mut self, ctx: &egui::Context, job: Job) {
        if self.pending.contains(&job.key) || self.entries.iter().any(|e| e.key == job.key) {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if !self.worker_started {
                self.worker_started = true;
                self.worker = Worker::spawn(ctx.clone());
            }
            if let Some(w) = &self.worker {
                self.pending.push(job.key.clone());
                if w.tx.send(job).is_ok() {
                    return;
                }
                self.pending.pop();
                self.worker = None;
                return;
            }
        }
        let _ = ctx;
        let done = separate(job);
        self.last_ms = done.ms;
        self.insert(done.key, done.sheet.map(Arc::new));
    }

    /// The newest sheet of spread `si` (it may be of an earlier revision while the current one
    /// is being separated).
    pub fn sheet(&self, si: usize) -> Option<&Result<Arc<Sheet>, String>> {
        self.entries.iter().find(|e| e.key.spread == si).map(|e| &e.sheet)
    }
}

/// The Export PDF settings the preview separates with.
pub fn print_settings(app: &DesignApp) -> Print {
    let s = &app.ui.pdf_export;
    let flatten = if s.flatten.is_empty() { app.ui.flattener.as_str() } else { s.flatten.as_str() };
    Print {
        standard: designcraft_engine::separations::Standard::parse(&s.standard).unwrap_or_default(),
        compress_images: s.compress_images,
        flatten: match flatten {
            "high" => Some(300.0),
            "medium" => Some(150.0),
            "low" => Some(72.0),
            _ => None,
        },
    }
}

/// The resolution for a view at `zoom` with `ppp` pixels per point: device pixels per point,
/// rounded up to a power of two of 72 dpi so that zooming within a step keeps the plates.
pub fn dpi_for(zoom: f64, ppp: f64) -> f32 {
    let want = (zoom * ppp).max(1e-3);
    let step = want.log2().ceil().exp2() * 72.0;
    let dpi = (step as f32).clamp(MIN_DPI, MAX_DPI);
    #[cfg(target_arch = "wasm32")]
    let dpi = dpi.min(WEB_MAX_DPI);
    dpi
}

fn key(app: &DesignApp, si: usize, dpi: f32) -> Option<(Key, Job)> {
    let st = app.session.active()?;
    let key = Key { uid: st.uid, revision: st.revision, spread: si, dpi, print: print_settings(app), cms: designcraft_color::cms::generation() };
    Some((key.clone(), Job { key, doc: st.doc.clone(), cache: app.session.cache.clone() }))
}

/// The sheet of the spread nearest the view center, separated now if the canvas hasn't yet
/// (for commands and automation, which want an answer).
pub fn current_sheet(app: &mut DesignApp) -> Result<Arc<Sheet>, String> {
    let st = app.session.active().ok_or("no document")?;
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    let slot = crate::canvas::current_slot(app, &layout).and_then(|i| layout.slots.get(i)).map(|s| s.spread);
    let si = match slot {
        Some(SpreadRef::Doc(si)) => si,
        Some(_) => return Err("parent spreads aren't separated".into()),
        None => st.doc.page_loc(0).map(|(si, _)| si).ok_or("no spread")?,
    };
    let ppp = if app.canvas.separations.ppp > 0.0 { app.canvas.separations.ppp } else { 1.0 };
    let dpi = app.view().map_or(72.0, |v| dpi_for(v.zoom, ppp));
    let (key, job) = key(app, si, dpi).ok_or("no document")?;
    let preview = &mut app.canvas.separations;
    preview.poll();
    if let Some(e) = preview
        .entries
        .iter()
        .find(|e| e.key.uid == key.uid && e.key.revision == key.revision && e.key.spread == si && e.key.print == key.print && e.key.cms == key.cms)
    {
        return e.sheet.clone();
    }
    let done = separate(job);
    preview.last_ms = done.ms;
    let sheet = done.sheet.map(Arc::new);
    preview.insert(done.key, sheet.clone());
    sheet
}

/// The preview's state for `ui.inspect`: the view, and each kept sheet's plates (`null` when the
/// preview is off).
pub fn inspect(app: &DesignApp) -> serde_json::Value {
    let Some(view) = &app.ui.separations else { return serde_json::Value::Null };
    let revision = app.session.active().map(|st| (st.uid, st.revision));
    let preview = &app.canvas.separations;
    let sheets: Vec<serde_json::Value> = preview
        .entries
        .iter()
        .map(|e| {
            let mut v = serde_json::json!({"spread": e.key.spread, "dpi": e.key.dpi, "current": revision == Some((e.key.uid, e.key.revision))});
            match &e.sheet {
                Ok(sheet) => v["inks"] = sheet.inks().into_iter().map(|(name, spot)| serde_json::json!({"name": name, "spot": spot})).collect(),
                Err(err) => v["error"] = serde_json::json!(err),
            }
            v
        })
        .collect();
    serde_json::json!({
        "plate": view.plate.as_deref().unwrap_or("all"),
        "inkLimit": view.ink_limit,
        "pending": preview.pending.len(),
        "lastMs": preview.last_ms,
        "sheets": sheets,
    })
}

/// Draws the preview over each visible spread of the canvas.
pub fn paint(app: &mut DesignApp, ctx: &egui::Context, painter: &egui::Painter, xf: &Xf, layout: &CanvasLayout, screen: egui::Rect) {
    let Some(view) = app.ui.separations.clone() else {
        // Off: free the plates.
        app.canvas.separations.entries.clear();
        return;
    };
    let ppp = f64::from(ctx.pixels_per_point());
    let dpi = dpi_for(xf.zoom, ppp);
    app.canvas.separations.ppp = ppp;
    app.canvas.separations.poll();
    app.canvas.separations.frame += 1;
    let frame = app.canvas.separations.frame;
    let hover = ctx.pointer_hover_pos().filter(|p| screen.contains(*p)).map(|p| xf.to_canvas(p));
    app.canvas.separations.pointer = None;
    let Some(st) = app.session.active() else { return };
    let doc = st.doc.clone();
    for slot in &layout.slots {
        let SpreadRef::Doc(si) = slot.spread else { continue };
        let Some(sp) = doc.spreads.get(si) else { continue };
        if !xf.rect(slot.xf.transform_rect_bbox(sp.bounds())).intersects(screen) {
            continue;
        }
        if let Some(c) = hover {
            let p = slot.xf.inverse() * c;
            if sp.bounds().contains(p) {
                app.canvas.separations.pointer = Some((si, p));
            }
        }
        if let Some((_, job)) = key(app, si, dpi) {
            app.canvas.separations.request(ctx, job);
        }
        let Some(entry) = app.canvas.separations.entries.iter_mut().find(|e| e.key.spread == si) else { continue };
        entry.used = frame;
        let Ok(sheet) = &entry.sheet else { continue };
        let texture = match &entry.texture {
            Some((v, tex)) if *v == view => tex.clone(),
            _ => {
                let plate = view.plate.as_deref().and_then(|n| sheet.ink(n));
                let rgba = sheet.image(plate, view.ink_limit);
                let size = [sheet.plates.width as usize, sheet.plates.height as usize];
                if size[0] == 0 || size[1] == 0 || rgba.len() != size[0] * size[1] * 4 {
                    continue;
                }
                let ci = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                let ci = crate::widgets::fit_texture(ci, ctx.input(|i| i.max_texture_side));
                let tex = ctx.load_texture(format!("separations-{si}"), ci, egui::TextureOptions::LINEAR);
                entry.texture = Some((view.clone(), tex.clone()));
                tex
            }
        };
        let a = sheet.area;
        let corners = [
            (Point::new(a.x0, a.y0), pos2(0.0, 0.0)),
            (Point::new(a.x1, a.y0), pos2(1.0, 0.0)),
            (Point::new(a.x1, a.y1), pos2(1.0, 1.0)),
            (Point::new(a.x0, a.y1), pos2(0.0, 1.0)),
        ];
        let mut mesh = egui::Mesh::with_texture(texture.id());
        for (c, uv) in corners {
            mesh.vertices.push(egui::epaint::Vertex { pos: xf.to_screen(slot.xf * c), uv, color: Color32::WHITE });
        }
        mesh.indices.extend([0, 1, 2, 0, 2, 3]);
        painter.add(egui::Shape::mesh(mesh));
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::menus::run_ui;

    #[test]
    fn view_separations_lists_the_spot_plate_and_shows_it_alone() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        app.session.execute("swatch.create", &json!({"name": "Gold", "spot": true, "color": {"c": 0, "m": 20, "y": 80, "k": 10}})).unwrap();
        let id = app.session.execute("frame.create", &json!({"rect": [72, 72, 172, 172]})).unwrap()["id"].clone();
        app.session.execute("object.fill", &json!({"swatch": "Gold", "ids": [id]})).unwrap();

        let r = run_ui(&mut app, "view.separations", &json!({"plate": "Gold"})).unwrap().unwrap();
        assert_eq!((r["on"].clone(), r["plate"].clone()), (json!(true), json!("Gold")), "{r}");
        let inks: Vec<(String, bool)> =
            r["inks"].as_array().unwrap().iter().map(|i| (i["name"].as_str().unwrap().to_string(), i["spot"] == true)).collect();
        assert_eq!(inks.last(), Some(&("Gold".to_string(), true)), "{r}");
        assert_eq!(inks.len(), 5, "the process inks and Gold: {r}");
        assert_eq!(app.ui.separations.as_ref().and_then(|s| s.plate.as_deref()), Some("Gold"));

        // The menu's params keep working.
        run_ui(&mut app, "view.separations", &json!({"plate": "cyan"})).unwrap().unwrap();
        assert_eq!(app.ui.separations.as_ref().and_then(|s| s.plate.as_deref()), Some("Cyan"));
        let r = run_ui(&mut app, "view.separations", &json!({"inkLimit": 300})).unwrap().unwrap();
        assert_eq!(r["inkLimit"], 300.0);
        let r = run_ui(&mut app, "view.separations", &json!({"plate": null, "inkLimit": null})).unwrap().unwrap();
        assert_eq!(r["on"], false);
        assert!(app.ui.separations.is_none() && r.get("inks").is_none_or(Value::is_null));
    }

    /// Plates separated under other color settings (another CMYK working space) aren't reused.
    #[test]
    fn plates_of_other_color_settings_are_separated_again() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", &json!({})).unwrap();
        let first = super::current_sheet(&mut app).unwrap();
        assert!(std::sync::Arc::ptr_eq(&first, &super::current_sheet(&mut app).unwrap()), "the same settings reuse the sheet");
        for e in &mut app.canvas.separations.entries {
            e.key.cms = e.key.cms.wrapping_add(1);
        }
        let again = super::current_sheet(&mut app).unwrap();
        assert!(!std::sync::Arc::ptr_eq(&first, &again), "a sheet of other color settings is separated again");
        assert!(app.canvas.separations.entries.iter().all(|e| e.key.cms == designcraft_color::cms::generation()));
    }
}
