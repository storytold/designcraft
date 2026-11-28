//! The main window's size and position between launches. The first launch keeps the default size
//! where the system puts it; later launches restore the size, position, maximized and full-screen
//! state saved in the settings directory (`window.json`, portable installs included), moved onto a
//! monitor that is still connected. Ported from EffectCraft's `window.rs` (storytold/effectcraft#829).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};

const FILE: &str = "window.json";
const MAX_BYTES: u64 = 4096;
/// Seconds a window opened maximized or full screen gets to say so before its size is tracked.
const SETTLE_WAIT: f64 = 2.0;

/// The first window's size, in points.
pub const DEFAULT_SIZE: [f32; 2] = [1440.0, 900.0];
/// The smallest window, in points.
pub const MIN_SIZE: [f32; 2] = [900.0, 560.0];
/// Room a restored window leaves around itself for a taskbar, Dock or menu bar when it no longer
/// fits its monitor (the system doesn't say how much of the screen they take).
const SCREEN_ROOM: [f32; 2] = [64.0, 128.0];

/// The window's geometry between launches.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowGeometry {
    /// The outer top-left corner in physical pixels; `None` where the system doesn't say (Wayland).
    pub pos: Option<[i32; 2]>,
    /// The inner size in points; while maximized or full screen, the size to return to.
    pub size: [f32; 2],
    pub maximized: bool,
    #[serde(default)]
    pub fullscreen: bool,
}

/// A monitor: its top-left corner and size in physical pixels, and its scale factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Monitor {
    pos: [i64; 2],
    size: [i64; 2],
    scale: f32,
}

impl Monitor {
    pub fn new(pos: PhysicalPosition<i32>, size: PhysicalSize<u32>, scale: f64) -> Self {
        let scale = scale as f32;
        let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
        Self { pos: [pos.x.into(), pos.y.into()], size: [size.width.into(), size.height.into()], scale }
    }

    /// Its size in points.
    fn points(&self) -> [f32; 2] {
        self.size.map(|s| s as f32 / self.scale)
    }

    /// Its size in points less room for a taskbar, Dock or menu bar.
    fn usable(&self) -> [f32; 2] {
        let points = self.points();
        std::array::from_fn(|i| points[i] - SCREEN_ROOM[i])
    }

    /// A window of `size` points on this monitor, in physical pixels.
    fn pixels(&self, size: [f32; 2]) -> [i64; 2] {
        size.map(|s| (s * self.scale) as i64)
    }

    /// How much of a window at `pos` of `size` points this monitor shows, in square pixels.
    fn overlap(&self, pos: [i32; 2], size: [f32; 2]) -> i64 {
        let px = self.pixels(size);
        let span = |i: usize| {
            let start = i64::from(pos[i]).max(self.pos[i]);
            let end = i64::from(pos[i]).saturating_add(px[i]).min(self.pos[i].saturating_add(self.size[i]));
            end.saturating_sub(start).max(0)
        };
        span(0).saturating_mul(span(1))
    }

    /// The top-left corner nearest to `pos` (centred when none) that keeps a window of `size`
    /// points on this monitor.
    fn keep_on(&self, pos: Option<[i32; 2]>, size: [f32; 2]) -> [i32; 2] {
        let px = self.pixels(size);
        std::array::from_fn(|i| {
            let lo = self.pos[i];
            let hi = lo.saturating_add(self.size[i].saturating_sub(px[i]).max(0));
            let want = pos.map_or(lo + (hi - lo) / 2, |p| i64::from(p[i]));
            i32::try_from(want.clamp(lo, hi)).unwrap_or_default()
        })
    }
}

/// Where the window opens: `saved` (the last session's geometry) kept on a connected monitor, else
/// on `home` (the monitor the system opens it on). `None` leaves the window as the system opens it:
/// on the first launch, and without monitors to check a saved geometry against.
pub fn place(saved: Option<WindowGeometry>, monitors: &[Monitor], home: Option<usize>) -> Option<WindowGeometry> {
    let saved = saved?;
    let home = home.and_then(|i| monitors.get(i)).or(monitors.first())?;
    // The monitor showing most of the window; none when it was on a monitor that is gone.
    let shown = saved.pos.and_then(|p| {
        let (area, m) = monitors.iter().map(|m| (m.overlap(p, saved.size), m)).max_by_key(|(area, _)| *area)?;
        (area > 0).then_some(m)
    });
    let m = shown.unwrap_or(home);
    // Bigger than its monitor (a smaller screen than last time): it fits, and opens maximized.
    let room = m.points();
    let too_big = (0..2).any(|i| saved.size[i] > room[i]);
    let size = fit(saved.size, if too_big { m.usable() } else { room });
    Some(WindowGeometry {
        // Moved fully onto its monitor, or centred on the home monitor.
        pos: saved.pos.map(|p| m.keep_on(shown.map(|_| p), size)),
        size,
        maximized: saved.maximized || too_big,
        fullscreen: saved.fullscreen,
    })
}

/// `size` no bigger than `room`, and no smaller than the smallest window unless `room` is.
fn fit(size: [f32; 2], room: [f32; 2]) -> [f32; 2] {
    std::array::from_fn(|i| size[i].min(room[i]).max(MIN_SIZE[i].min(room[i])).max(1.0))
}

/// Size and place the new window (eframe shows it after drawing the first frame) and return the
/// geometry it opens with; `None` when it keeps the default.
pub fn restore(window: &winit::window::Window, saved: Option<WindowGeometry>) -> Option<WindowGeometry> {
    let handles: Vec<_> = window.available_monitors().collect();
    let monitors: Vec<Monitor> = handles.iter().map(|m| Monitor::new(m.position(), m.size(), m.scale_factor())).collect();
    let home = window.current_monitor().or_else(|| window.primary_monitor()).and_then(|c| handles.iter().position(|m| *m == c));
    let g = place(saved, &monitors, home)?;
    if let Some([x, y]) = g.pos {
        window.set_outer_position(PhysicalPosition::new(x, y));
    }
    // Returns the new size when it applies at once; either way the window gets it.
    let _ = window.request_inner_size(LogicalSize::new(g.size[0], g.size[1]));
    if g.maximized {
        window.set_maximized(true);
    }
    if g.fullscreen {
        // On the monitor it was just moved to; on macOS its own Space, as the green button does.
        window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
    }
    Some(g)
}

/// Per frame: remember the window's geometry for the next launch. Minimized keeps the last one;
/// maximized and full screen only set their flags, so leaving them next time restores the last size.
pub fn track(ctx: &egui::Context, geometry: &mut Option<WindowGeometry>) {
    let zoom = ctx.zoom_factor();
    ctx.input(|i| {
        let v = i.viewport();
        if v.minimized == Some(true) {
            return;
        }
        if v.fullscreen == Some(true) {
            if let Some(g) = geometry {
                g.fullscreen = true;
            }
            return;
        }
        if v.maximized == Some(true) {
            if let Some(g) = geometry {
                g.maximized = true;
                g.fullscreen = false;
            }
            return;
        }
        let Some(inner) = v.inner_rect else { return };
        let pos = v.outer_rect.map(|r| [r.min.x, r.min.y].map(|c| (c * i.pixels_per_point).round() as i32));
        *geometry = Some(WindowGeometry { pos, size: [inner.width() * zoom, inner.height() * zoom], maximized: false, fullscreen: false });
    });
}

/// The geometry saved by the last session in `dir`; `None` when there is none or it can't be read.
pub fn load(dir: &Path) -> Option<WindowGeometry> {
    use std::io::Read;
    let mut text = String::new();
    let file = std::fs::File::open(dir.join(FILE)).ok()?;
    file.take(MAX_BYTES + 1).read_to_string(&mut text).ok()?;
    if text.len() as u64 > MAX_BYTES {
        return None;
    }
    serde_json::from_str(&text).ok()
}

/// Follows the window and saves its geometry for the next launch: when it has changed (at most
/// once a second, so a session that ends badly still has it) and on exit.
pub struct Tracker {
    geometry: Option<WindowGeometry>,
    saved: Option<WindowGeometry>,
    last_attempt: f64,
    /// Opened maximized or full screen, and the system hasn't reported it so yet (when the wait
    /// ends): the window's first frames report that size as an ordinary one, which would replace
    /// the size to return to.
    settling: Option<f64>,
    /// The settings directory; `None` reads and writes nothing (`DESIGNCRAFT_NO_PREFS`).
    dir: Option<PathBuf>,
}

impl Tracker {
    /// `geometry`: what the window opened with; `saved`: what `dir` holds.
    pub fn new(geometry: Option<WindowGeometry>, saved: Option<WindowGeometry>, dir: Option<PathBuf>) -> Self {
        let settling = geometry.is_some_and(|g| g.maximized || g.fullscreen).then_some(f64::INFINITY);
        Self { geometry, saved, last_attempt: f64::NEG_INFINITY, settling, dir }
    }

    /// Per frame.
    pub fn frame(&mut self, ctx: &egui::Context) {
        let (now, settled) = ctx.input(|i| (i.time, i.viewport().maximized == Some(true) || i.viewport().fullscreen == Some(true)));
        if let Some(until) = self.settling {
            // A system that never maximizes the window is believed after a moment.
            let until = until.min(now + SETTLE_WAIT);
            self.settling = (!settled && now < until).then_some(until);
        }
        if self.settling.is_none() {
            track(ctx, &mut self.geometry);
        }
        if now - self.last_attempt >= 1.0 {
            self.last_attempt = now;
            self.save();
        }
    }

    /// Write the geometry when it differs from the saved one.
    pub fn save(&mut self) {
        let (Some(dir), Some(g)) = (&self.dir, self.geometry) else { return };
        if self.saved == Some(g) {
            return;
        }
        let written = serde_json::to_vec(&g).map_err(|e| e.to_string()).and_then(|bytes| {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            designcraft_engine::atomic::write_atomic(&dir.join(FILE), &bytes).map_err(|e| e.to_string())
        });
        match written {
            Ok(()) => self.saved = Some(g),
            Err(e) => log::warn!("could not save the window geometry: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(x: i32, y: i32, w: u32, h: u32, scale: f64) -> Monitor {
        Monitor::new(PhysicalPosition::new(x, y), PhysicalSize::new(w, h), scale)
    }

    fn geometry(pos: Option<[i32; 2]>, size: [f32; 2], maximized: bool) -> WindowGeometry {
        WindowGeometry { pos, size, maximized, fullscreen: false }
    }

    /// A settings directory of its own, removed afterwards.
    struct Dir(PathBuf);
    impl Dir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("designcraft-window-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            Self(dir)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_first_window_keeps_the_default() {
        let big = [monitor(0, 0, 2560, 1440, 1.0)];
        assert_eq!(place(None, &big, Some(0)), None);
        assert_eq!(place(None, &[monitor(0, 0, 1366, 768, 1.0)], Some(0)), None);
        // No monitor information: nothing to check a saved geometry against.
        assert_eq!(place(Some(geometry(Some([9000, 9000]), [1e9, 1e9], true)), &[], None), None);
    }

    #[test]
    fn the_saved_window_comes_back_where_it_was() {
        let two = [monitor(0, 0, 2560, 1440, 1.0), monitor(2560, 0, 1920, 1080, 1.0)];
        for g in [
            geometry(Some([100, 80]), [1200.0, 800.0], false),
            geometry(Some([2700, 50]), [1000.0, 700.0], true),
            geometry(None, [1000.0, 700.0], false),
            WindowGeometry { fullscreen: true, ..geometry(Some([2700, 50]), [1000.0, 700.0], false) },
        ] {
            assert_eq!(place(Some(g), &two, Some(0)), Some(g));
        }
    }

    #[test]
    fn a_saved_window_stays_on_a_connected_monitor() {
        let laptop = [monitor(0, 0, 1600, 900, 1.0)];
        // Last time on a second monitor that is gone: centred on the remaining one.
        let g = place(Some(geometry(Some([2700, 50]), [1000.0, 700.0], false)), &laptop, Some(0));
        assert_eq!(g, Some(geometry(Some([300, 100]), [1000.0, 700.0], false)));
        // Full screen there: full screen here.
        let g = place(Some(WindowGeometry { fullscreen: true, ..geometry(Some([2700, 50]), [1000.0, 700.0], false) }), &laptop, Some(0));
        assert_eq!(g.map(|g| (g.pos, g.fullscreen)), Some((Some([300, 100]), true)));
        // Half off the monitor's right and bottom edges: moved fully onto it.
        let g = place(Some(geometry(Some([1200, 600]), [1000.0, 700.0], false)), &laptop, Some(0));
        assert_eq!(g.and_then(|g| g.pos), Some([600, 200]));
        // Bigger than the screen now (it was on a larger one): it fits, and opens maximized.
        let g = place(Some(geometry(Some([0, 0]), [2400.0, 1300.0], false)), &laptop, Some(0));
        assert_eq!(g, Some(geometry(Some([0, 0]), [1536.0, 772.0], true)));
        // A monitor left of the primary one (negative coordinates) still counts.
        let left = [monitor(0, 0, 1920, 1080, 1.0), monitor(-1920, 0, 1920, 1080, 1.0)];
        assert_eq!(place(Some(geometry(Some([-1800, 40]), [1000.0, 700.0], false)), &left, Some(0)).and_then(|g| g.pos), Some([-1800, 40]));
        // Nonsense from a damaged settings file is kept within the screen.
        let g = place(Some(geometry(Some([i32::MAX, i32::MIN]), [-5.0, f32::MAX], false)), &laptop, Some(0));
        assert_eq!(g, Some(geometry(Some([350, 64]), [900.0, 772.0], true)));
        let g = place(Some(geometry(Some([5, 5]), [f32::NEG_INFINITY, f32::INFINITY], false)), &laptop, Some(0));
        assert_eq!(g, Some(geometry(Some([350, 64]), [900.0, 772.0], true)));
    }

    #[test]
    fn the_window_geometry_is_tracked_for_the_next_launch() {
        let ctx = egui::Context::default();
        let mut tracked = None;
        let mut frame = |info: egui::ViewportInfo| {
            let mut input = egui::RawInput::default();
            input.viewports.insert(egui::ViewportId::ROOT, info);
            ctx.run_ui(input, |ui| track(ui.ctx(), &mut tracked)).textures_delta.clear();
            tracked
        };
        let rect = egui::Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(1000.0, 600.0));
        let normal = egui::ViewportInfo {
            native_pixels_per_point: Some(1.5),
            inner_rect: Some(rect),
            outer_rect: Some(rect),
            maximized: Some(false),
            fullscreen: Some(false),
            ..Default::default()
        };
        // Position in physical pixels, size in points.
        let g = geometry(Some([30, 45]), [1000.0, 600.0], false);
        assert_eq!(frame(normal.clone()), Some(g));
        // Maximized, full screen or minimized: the size to restore stays.
        let big = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 700.0));
        let maximized = egui::ViewportInfo { inner_rect: Some(big), outer_rect: Some(big), maximized: Some(true), ..normal.clone() };
        assert_eq!(frame(maximized.clone()), Some(WindowGeometry { maximized: true, ..g }));
        let fullscreen = egui::ViewportInfo { fullscreen: Some(true), ..maximized };
        assert_eq!(frame(fullscreen), Some(WindowGeometry { maximized: true, fullscreen: true, ..g }));
        let minimized = egui::ViewportInfo { inner_rect: None, outer_rect: None, minimized: Some(true), ..normal.clone() };
        assert_eq!(frame(minimized), Some(WindowGeometry { maximized: true, fullscreen: true, ..g }));
        assert_eq!(frame(normal), Some(g));
    }

    /// A window restored maximized or full screen keeps the size it returns to, though its first
    /// frames report that size as an ordinary one.
    #[test]
    fn opening_maximized_or_full_screen_keeps_the_size_to_return_to() {
        let ctx = egui::Context::default();
        let big = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1728.0, 1052.0));
        let small = egui::Rect::from_min_size(egui::pos2(40.0, 30.0), egui::vec2(900.0, 500.0));
        let frame = |tracker: &mut Tracker, time: f64, rect: egui::Rect, maximized: bool, fullscreen: bool| {
            let info = egui::ViewportInfo {
                inner_rect: Some(rect),
                outer_rect: Some(rect),
                maximized: Some(maximized),
                fullscreen: Some(fullscreen),
                ..Default::default()
            };
            let mut input = egui::RawInput { time: Some(time), ..Default::default() };
            input.viewports.insert(egui::ViewportId::ROOT, info);
            ctx.run_ui(input, |ui| tracker.frame(ui.ctx())).textures_delta.clear();
            tracker.geometry
        };
        let restored = geometry(Some([40, 30]), [900.0, 500.0], false);
        for g in
            [geometry(Some([40, 30]), [1000.0, 600.0], true), WindowGeometry { fullscreen: true, ..geometry(Some([40, 30]), [1000.0, 600.0], false) }]
        {
            let mut tracker = Tracker::new(Some(g), Some(g), None);
            assert_eq!(frame(&mut tracker, 0.0, big, false, false), Some(g));
            assert_eq!(frame(&mut tracker, 0.5, big, g.maximized, g.fullscreen), Some(g));
            // Left by the user afterwards: tracked as usual.
            assert_eq!(frame(&mut tracker, 0.6, small, false, false), Some(restored));
            // A system that doesn't maximize: the window's real size is tracked after the wait.
            let mut tracker = Tracker::new(Some(g), Some(g), None);
            assert_eq!(frame(&mut tracker, 10.0, small, false, false), Some(g));
            assert_eq!(frame(&mut tracker, 10.0 + SETTLE_WAIT, small, false, false), Some(restored));
        }
    }

    /// The reported bug (#786): the window opened at 1440 × 900 in the middle of the screen on
    /// every launch, whatever its size and place when the app quit.
    #[test]
    fn the_window_geometry_is_saved_and_loaded() {
        let dir = Dir::new("saved");
        assert_eq!(load(&dir.0), None);
        let g = WindowGeometry { pos: Some([30, 45]), size: [1000.0, 600.0], maximized: true, fullscreen: true };
        let mut tracker = Tracker::new(Some(g), None, Some(dir.0.clone()));
        tracker.save();
        assert_eq!(load(&dir.0), Some(g));
        // Positions the system doesn't report (Wayland) round-trip too.
        tracker.geometry = Some(WindowGeometry { pos: None, ..g });
        tracker.save();
        assert_eq!(load(&dir.0), Some(WindowGeometry { pos: None, ..g }));
        // `DESIGNCRAFT_NO_PREFS` (no directory) writes nothing.
        let other = Dir::new("none");
        Tracker::new(Some(g), None, None).save();
        assert!(!other.0.exists());
        // A file written before full screen was remembered still loads.
        std::fs::write(dir.0.join(FILE), r#"{"pos":[1,2],"size":[1000.0,600.0],"maximized":false}"#).unwrap();
        assert_eq!(load(&dir.0), Some(geometry(Some([1, 2]), [1000.0, 600.0], false)));
        // A damaged or oversized file is ignored: the window opens at its default.
        for text in ["{", "null", "{\"pos\":[1],\"size\":[1,2],\"maximized\":false}", &" ".repeat(MAX_BYTES as usize + 1)] {
            std::fs::write(dir.0.join(FILE), text).unwrap();
            assert_eq!(load(&dir.0), None, "{text:.20}");
        }
    }

    /// Frames don't each write the file: an unchanged window is not written again, and a moving
    /// one at most once a second.
    #[test]
    fn the_geometry_is_not_written_every_frame() {
        let dir = Dir::new("throttle");
        let ctx = egui::Context::default();
        let mut tracker = Tracker::new(None, None, Some(dir.0.clone()));
        let mut frame = |time: f64, x: f32| {
            let rect = egui::Rect::from_min_size(egui::pos2(x, 30.0), egui::vec2(1000.0, 600.0));
            let info = egui::ViewportInfo { inner_rect: Some(rect), outer_rect: Some(rect), ..Default::default() };
            let mut input = egui::RawInput { time: Some(time), ..Default::default() };
            input.viewports.insert(egui::ViewportId::ROOT, info);
            ctx.run_ui(input, |ui| tracker.frame(ui.ctx())).textures_delta.clear();
            load(&dir.0).and_then(|g| g.pos).map(|p| p[0])
        };
        assert_eq!(frame(0.0, 10.0), Some(10));
        // Dragged during the next second: not written yet.
        assert_eq!(frame(0.3, 20.0), Some(10));
        assert_eq!(frame(0.9, 30.0), Some(10));
        assert_eq!(frame(1.0, 40.0), Some(40));
        // Unchanged: the file is left alone (removed here, so a write would bring it back).
        std::fs::remove_file(dir.0.join(FILE)).unwrap();
        assert_eq!(frame(5.0, 40.0), None);
    }
}
