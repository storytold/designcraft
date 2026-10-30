//! Which graphics adapter the window renders with, and what happens when it can't show the window.
//!
//! The canvas is rendered on the CPU (`designcraft-render`); the GPU only composites it and draws
//! the interface, so any adapter that can present to the window will do. Which backends the
//! instance is created with is decided first, with a fallback for drivers that take the process
//! down ([`backend`]). Among the adapters of those backends, [`adapter_order`] ranks: those that
//! can't present to the window's surface never, then the one `WGPU_ADAPTER_NAME` names, hardware
//! before software, the native backends (Vulkan, Metal, DX12) before OpenGL, the GPU that drives a
//! display and the one that drives the primary display ([`display_gpus`]), the power preference
//! ([`power_preference`]) among the rest, and a GPU's DX12 adapter before its Vulkan one. eframe is
//! handed [`selector`], which takes the first of that order.
//!
//! A GPU without a monitor is the wrong one even when it can present: on a desktop whose monitors
//! all hang off the discrete GPU, power saving picked the Ryzen's integrated GPU, Windows had to
//! present every frame across adapters, and the integrated GPU's driver reset under that, which
//! lost the graphics device, or took the desktop's compositor and every monitor down for minutes.
//! So, unless `WGPU_POWER_PREF` says otherwise, the GPU that drives the display the user looks at
//! comes first, and the power preference only orders the rest. Windows says which adapter drives
//! which display (`EnumDisplayDevices`), Linux through sysfs; elsewhere, or when nothing matches,
//! the order is the power preference's.
//!
//! An adapter can report that it presents to the window and still fail once it does: on a hybrid
//! Linux desktop under Wayland, the compositor runs on one GPU and may refuse the frame buffers
//! another GPU allocates, which kills the window's Wayland connection; egui-wgpu then panics while
//! configuring the surface. Creating the device can fail too: on an Intel Mac, wgpu's check of
//! indirect draw arguments needs a compute shader the Metal driver couldn't compile, and the device
//! was lost before the window showed (#334; DesignCraft never draws indirectly, so that check is
//! off, [`instance_flags`]). Nothing in the process can show a window after that, so when the
//! graphics fail while the window is starting up, [`finish`] starts the app again without that
//! adapter, as the next one in the order would be tried. When no adapter of the start's backends
//! is left, it starts again with the next backend ([`backend::Fallback::next_after`]). Software
//! renderers (WARP, llvmpipe) come last of all: while a backend is left to try, a start passes
//! over them, and only once no hardware adapter of any backend could show the window does the app
//! start again with them allowed. Each restart leaves out one more adapter or backend, or allows
//! software once, so it ends when none is left.
//!
//! An adapter can also hang instead of failing: on a hybrid laptop, the discrete GPU never put the
//! window's first frame on the screen, with no error and no panic, and the app stayed in the
//! background without a window. [`watch_first_frame`] notices a first frame that is late and,
//! when the UI thread is stuck in the graphics stack for [`HANG_TIMEOUT`], starts the app again
//! without that adapter in the same way.

pub mod backend;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use eframe::egui_wgpu::NativeAdapterSelectorMethod;
use eframe::wgpu::{self, Backend, DeviceType, PowerPreference};

/// The argument that tells a restarted app what the window failed on: comma-separated adapter
/// [`key`]s (`Vulkan:1002:164e`, `Metal:Intel Iris Pro Graphics`), [`backend::Backend::key`]s
/// (`Dx12`), [`SOFTWARE_LEFT`] and [`SOFTWARE`]. Added by the app itself when it starts again; an
/// argument rather than an environment variable, so the processes the app starts (a browser for a
/// link, `lpr`) don't inherit it.
pub const SKIP_ARG: &str = "--gpu-skip=";

/// In a skip list: a start passed over a software renderer, so one is left to try.
const SOFTWARE_LEFT: &str = "software-left";

/// In a skip list: no hardware adapter could show the window, so software renderers may draw it.
const SOFTWARE: &str = "software";

/// In a skip list: the record's backend failures were forgotten once for a start that found no
/// adapter at all, so it isn't done again (see [`finish`]).
const RETRIED: &str = "retried";

/// wgpu's variable for choosing an adapter by (part of) its name, any case.
const NAME_ENV: &str = "WGPU_ADAPTER_NAME";

/// Where to read how to choose another adapter, for the error that ends the app.
const HELP: &str =
    "to choose a graphics adapter or backend, see https://github.com/storytold/designcraft/blob/main/README.md#graphics-backend-and-processor";

/// Frames the UI ran before the failure, below which it was still starting up. A failing adapter
/// fails on the first or second frame; later, the adapter has proved itself, and a lost window is
/// something else (such as the compositor restarting).
pub const STARTUP_FRAMES: u64 = 10;

/// How long the window's first frame may take to reach the screen, from the app's creation, before
/// [`watch_first_frame`] logs it and says so in the status bar. The first frame draws the whole
/// interface and compiles its shaders, a second at most on a slow machine.
pub const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(20);

/// How long the UI thread may be stuck in the graphics stack before the window's first frame,
/// with the window not known to be hidden, before its adapter counts as hanging
/// ([`watch_first_frame`]).
pub const HANG_TIMEOUT: Duration = Duration::from_secs(60);

/// The preference used unless `WGPU_POWER_PREF` sets one. Windows and macOS show frames from any
/// GPU (the integrated one avoids the flicker of presenting a discrete GPU's frames through it).
/// Elsewhere, the system's own order: Mesa's device-select layer puts the GPU the desktop runs on
/// first (the integrated one on hybrid laptops), and a Wayland compositor may not show frames from
/// another GPU.
pub const AUTOMATIC: PowerPreference = if cfg!(any(windows, target_os = "macos")) { PowerPreference::LowPower } else { PowerPreference::None };

/// The power preference that orders the adapters: the `WGPU_POWER_PREF` environment variable
/// (`low`, `high`, `none`) when set, otherwise [`AUTOMATIC`].
pub fn power_preference(env: Option<PowerPreference>) -> PowerPreference {
    env.unwrap_or(AUTOMATIC)
}

/// Whether the adapter is the app's to choose: `WGPU_POWER_PREF` names no kind of GPU. Then the
/// GPU that drives the display comes first ([`display_gpus`]); a user's own choice is kept as it is.
pub fn automatic(env: Option<PowerPreference>) -> bool {
    env.is_none()
}

/// The GPUs to rank first: those that drive a display, when the choice is [`automatic`].
pub fn preferred_displays(env: Option<PowerPreference>) -> Vec<DisplayGpu> {
    if automatic(env) { display_gpus() } else { Vec::new() }
}

/// wgpu's instance flags without its check of indirect draw and dispatch arguments: nothing in
/// DesignCraft or egui draws indirectly, so the check only costs a compute shader at start-up, one
/// some drivers can't compile (Metal on an Intel Iris Pro lost the device, #334).
/// `WGPU_VALIDATION_INDIRECT_CALL=1` turns it back on, like wgpu's other flag variables.
pub fn instance_flags() -> wgpu::InstanceFlags {
    (wgpu::InstanceFlags::from_build_config() - wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL).with_env()
}

/// One adapter, as [`adapter_order`] sees it.
#[derive(Clone, Debug)]
pub struct Candidate {
    /// Identifies the adapter across a restart ([`key`]).
    pub key: String,
    pub name: String,
    pub backend: Backend,
    pub device_type: DeviceType,
    /// PCI vendor and device ids, to match a [`DisplayGpu`]; zero where the backend reports none
    /// (Metal, and OpenGL's device id).
    pub pci: (u32, u32),
    /// Reports that it can present to the window's surface.
    pub presents: bool,
}

impl Candidate {
    fn new(adapter: &wgpu::Adapter, surface: Option<&wgpu::Surface<'_>>) -> Self {
        let info = adapter.get_info();
        Self {
            key: key(info.backend, info.vendor, info.device, &info.name),
            presents: surface.is_none_or(|s| adapter.is_surface_supported(s)),
            name: info.name,
            backend: info.backend,
            device_type: info.device_type,
            pci: (info.vendor, info.device),
        }
    }

    /// Whether it drives one of `displays`, and whether that is the primary one.
    fn drives(&self, displays: &[DisplayGpu]) -> (bool, bool) {
        let shown = displays.iter().find(|g| g.pci == self.pci);
        (shown.is_some(), shown.is_some_and(|g| g.primary))
    }
}

/// A GPU with a monitor attached, as the system reports it ([`display_gpus`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayGpu {
    /// PCI `(vendor, device)` ids, as wgpu's `AdapterInfo` reports them.
    pub pci: (u32, u32),
    /// Drives the display the user most likely looks at: on Windows the primary display, on Linux
    /// a built-in panel (`eDP`, `LVDS` or `DSI`).
    pub primary: bool,
}

impl std::fmt::Display for DisplayGpu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04x}:{:04x}{}", self.pci.0, self.pci.1, if self.primary { " (primary)" } else { "" })
    }
}

/// The GPUs that drive a display, each once. Windows enumerates its display devices
/// (`EnumDisplayDevices`: those attached to the desktop, and the primary one); Linux reads the
/// connected connectors in `/sys/class/drm`. macOS and the rest report nothing, so the power
/// preference decides; so does a system whose GPUs report no PCI ids.
pub fn display_gpus() -> Vec<DisplayGpu> {
    #[cfg(windows)]
    {
        windows_display_gpus()
    }
    #[cfg(target_os = "linux")]
    {
        linux_display_gpus(std::path::Path::new("/sys/class/drm"))
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Vec::new()
    }
}

/// Record that the GPU `pci` drives a display, merging it with its other displays.
#[cfg(any(windows, target_os = "linux", test))]
fn add_display_gpu(gpus: &mut Vec<DisplayGpu>, pci: (u32, u32), primary: bool) {
    match gpus.iter_mut().find(|g| g.pci == pci) {
        Some(gpu) => gpu.primary |= primary,
        None => gpus.push(DisplayGpu { pci, primary }),
    }
}

/// Windows' display devices: one per GPU output; those `ATTACHED_TO_DESKTOP` show a monitor, one
/// of them the `PRIMARY_DEVICE`. The registry doesn't tell a user which adapter drives a display,
/// and WMI reports a resolution for a GPU without a monitor, so this is the way.
#[cfg(windows)]
fn windows_display_gpus() -> Vec<DisplayGpu> {
    use winsafe::co::DISPLAY_DEVICE as Flags;
    let mut gpus = Vec::new();
    // A machine has a handful of display devices; the cap only bounds a runaway enumeration.
    for device in winsafe::EnumDisplayDevices(None, None).take(256) {
        let device = match device {
            Ok(d) => d,
            // The enumeration ends with "no more items" or, from a driver, with any other error.
            Err(e) => {
                log::debug!("display devices: {e}");
                break;
            }
        };
        if !device.StateFlags.has(Flags::ATTACHED_TO_DESKTOP) {
            continue;
        }
        // Remote Desktop and other non-PCI devices (`ROOT\BasicDisplay\0000`) are left out.
        let Some(pci) = pci_ids(&device.DeviceID()) else { continue };
        add_display_gpu(&mut gpus, pci, device.StateFlags.has(Flags::PRIMARY_DEVICE));
    }
    gpus
}

/// The PCI `(vendor, device)` of a Windows device id such as
/// `PCI\VEN_10DE&DEV_2204&SUBSYS_40421458&REV_A1`, any case; `None` for anything else.
#[cfg(any(windows, test))]
fn pci_ids(device_id: &str) -> Option<(u32, u32)> {
    let field = |key: &str| {
        device_id.split(['\\', '&']).find_map(|part| {
            let (name, hex) = part.split_at_checked(key.len())?;
            if name.eq_ignore_ascii_case(key) { u32::from_str_radix(hex, 16).ok() } else { None }
        })
    };
    Some((field("VEN_")?, field("DEV_")?))
}

/// Linux: every connector `cardN-CONNECTOR` under `drm` whose `status` is `connected`, with the
/// PCI ids of its card (`cardN/device/{vendor,device}`). A built-in panel is the primary display.
#[cfg(any(target_os = "linux", test))]
fn linux_display_gpus(drm: &std::path::Path) -> Vec<DisplayGpu> {
    let read_hex = |p: std::path::PathBuf| -> Option<u32> {
        let s = std::fs::read_to_string(p).ok()?;
        u32::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok()
    };
    let mut gpus = Vec::new();
    let Ok(entries) = std::fs::read_dir(drm) else { return gpus };
    // A machine has a handful of connectors; the cap only bounds a pathological sysfs.
    for entry in entries.flatten().take(256) {
        let name = entry.file_name();
        let Some((card, connector)) = name.to_str().and_then(|n| n.split_once('-')) else { continue };
        let connected = std::fs::read_to_string(entry.path().join("status")).is_ok_and(|s| s.trim() == "connected");
        if !connected {
            continue;
        }
        let device = drm.join(card).join("device");
        let (Some(v), Some(d)) = (read_hex(device.join("vendor")), read_hex(device.join("device"))) else { continue };
        add_display_gpu(&mut gpus, (v, d), is_internal_panel(connector));
    }
    gpus
}

/// A laptop's built-in panel: an embedded DisplayPort, LVDS or DSI connector.
#[cfg(any(target_os = "linux", test))]
fn is_internal_panel(connector: &str) -> bool {
    ["eDP", "LVDS", "DSI"].iter().any(|kind| connector.starts_with(kind))
}

/// `backend:vendor:device`, e.g. `Vulkan:1002:164e` (PCI ids, as `MESA_VK_DEVICE_SELECT` takes them).
/// Metal reports no ids (`0000:0000` for every GPU), so an adapter without them is `backend:name`
/// (`Metal:Intel Iris Pro Graphics`): otherwise leaving out the one that failed would leave out
/// every GPU of a dual-GPU Mac. Commas, which separate [`SKIP_ARG`]'s keys, become spaces.
fn key(backend: Backend, vendor: u32, device: u32, name: &str) -> String {
    if vendor == 0 && device == 0 {
        format!("{backend:?}:{}", name.replace(',', " ").trim())
    } else {
        format!("{backend:?}:{vendor:04x}:{device:04x}")
    }
}

/// The order to try `candidates` in (their indices): only those that present to the window and
/// aren't in `skip`; the one whose name contains `named` (any case) first; hardware before
/// software; native backends before OpenGL; the GPUs that drive one of `displays`, the primary
/// display's first; then by `power`; then DX12 before Vulkan, keeping the system's order among
/// equals (all of it for [`PowerPreference::None`]). Windows lists each GPU under both when both
/// backends are in the instance (`WGPU_BACKEND=dx12,vulkan`), and Intel's Vulkan driver made the
/// whole window flicker black where DX12 and OpenGL didn't; elsewhere there is no DX12 adapter, so
/// the order is unchanged.
///
/// With no `displays`, or none a candidate's PCI ids match (OpenGL reports no device id, Metal no
/// ids at all), the order is the power preference's alone.
pub fn adapter_order(candidates: &[Candidate], power: PowerPreference, displays: &[DisplayGpu], named: Option<&str>, skip: &[String]) -> Vec<usize> {
    let named = named.map(str::to_lowercase).filter(|n| !n.is_empty());
    let mut order: Vec<(usize, &Candidate)> = candidates.iter().enumerate().filter(|(_, c)| c.presents && !skip.contains(&c.key)).collect();
    // Stable, so equals keep the system's order.
    order.sort_by_key(|(_, c)| {
        let unnamed = named.as_ref().is_some_and(|n| !c.name.to_lowercase().contains(n));
        let (drives_display, drives_primary) = c.drives(displays);
        (
            unnamed,
            c.device_type == DeviceType::Cpu,
            c.backend == Backend::Gl,
            !drives_display,
            !drives_primary,
            power_rank(c.device_type, power),
            c.backend == Backend::Vulkan,
        )
    });
    order.into_iter().map(|(i, _)| i).collect()
}

/// wgpu's own ranking for a power preference: the preferred kind of GPU, the other kind, unknown,
/// virtual, software.
fn power_rank(t: DeviceType, power: PowerPreference) -> u8 {
    match (power, t) {
        (PowerPreference::None, _) => 0,
        (PowerPreference::LowPower, DeviceType::IntegratedGpu) | (PowerPreference::HighPerformance, DeviceType::DiscreteGpu) => 0,
        (_, DeviceType::IntegratedGpu | DeviceType::DiscreteGpu) => 1,
        (_, DeviceType::Other) => 2,
        (_, DeviceType::VirtualGpu) => 3,
        (_, DeviceType::Cpu) => 4,
    }
}

/// The skip list of a [`SKIP_ARG`] argument; `None` for any other argument.
pub fn skip_arg(arg: &str) -> Option<Vec<String>> {
    arg.strip_prefix(SKIP_ARG).map(parse_skip)
}

fn parse_skip(v: &str) -> Vec<String> {
    v.split(',').map(str::trim).filter(|k| !k.is_empty()).map(String::from).collect()
}

/// The window's start, shared by [`selector`], [`watch_first_frame`], the app and [`finish`].
#[derive(Default)]
pub struct Startup {
    /// What the restart that began this start left out ([`SKIP_ARG`]).
    skip: Vec<String>,
    /// The key of the adapter [`selector`] picked.
    adapter: OnceLock<String>,
    /// [`selector`] passed over a software renderer.
    software_left: AtomicBool,
    /// The UI's context, which counts the frames run.
    ui: OnceLock<egui::Context>,
    /// When the app was created, the files named at the start opened: the first frame is due.
    ready: OnceLock<Instant>,
    /// The UI thread is in the app's own code ([`Startup::enter`]).
    busy: AtomicBool,
    /// How often the UI thread left the app's code: it moves while the thread is alive.
    leaves: AtomicU64,
    /// A note for the status bar from [`watch_first_frame`].
    notice: Mutex<Option<String>>,
    /// The backend fallback of this start (`gpu.json`); `None` when `WGPU_BACKEND` chooses.
    fallback: Mutex<Option<backend::Fallback>>,
    /// Documents the OS handed this run outside its arguments (macOS Apple events), for a start
    /// again to open: the OS doesn't send them twice.
    documents: Mutex<Vec<String>>,
}

impl Startup {
    pub fn new(fallback: Option<backend::Fallback>, skip: Vec<String>) -> Self {
        Self { skip, fallback: Mutex::new(fallback), ..Self::default() }
    }

    /// The UI thread runs the app's code (`logic`, `ui`): a long wait now is the app being busy,
    /// never a graphics hang.
    pub fn enter(&self) {
        self.busy.store(true, Ordering::Relaxed);
    }

    /// The UI thread leaves the app's code, to eframe and the graphics stack.
    pub fn leave(&self) {
        self.busy.store(false, Ordering::Relaxed);
        self.leaves.fetch_add(1, Ordering::Relaxed);
    }

    /// Note documents the OS handed this run outside its arguments, for [`restart`] to pass on.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn opening(&self, paths: impl IntoIterator<Item = String>) {
        self.documents.lock().unwrap_or_else(PoisonError::into_inner).extend(paths);
    }

    /// The note [`watch_first_frame`] left for the status bar, once.
    pub fn take_notice(&self) -> Option<String> {
        self.notice.lock().unwrap_or_else(PoisonError::into_inner).take()
    }

    /// While a backend is left to try, software renderers are passed over ([`SOFTWARE`]).
    fn software_last(&self) -> bool {
        !self.skip.iter().any(|k| k == SOFTWARE) && self.with_fallback(|_| ()).is_some()
    }

    /// The app was created on `ctx`.
    pub fn created(&self, ctx: &egui::Context) {
        // Set once; the window is created once.
        let _ = self.ui.set(ctx.clone());
    }

    /// The app is ready to draw its first frame.
    pub fn ready(&self) {
        // Set once, like `created`.
        let _ = self.ready.set(Instant::now());
    }

    /// The first frame was presented, or the app exits normally: no driver fault took this start
    /// down ([`backend::Fallback::finished`]).
    pub fn presented(&self) {
        self.with_fallback(|f| f.finished());
    }

    /// The start's backend couldn't find an adapter that can show the window: forget what the
    /// record blamed, so the next start tries every backend again ([`backend::Fallback::forget`]).
    pub fn forget_failures(&self) {
        self.with_fallback(|f| f.forget());
    }

    /// Whether the record blamed a backend before this start: it moved away from one that never
    /// showed a frame, or left one out that failed earlier. Such a blame is only proven by the
    /// process dying at the driver, so a start that finds no adapter at all must not trust it
    /// ([`finish`]).
    fn blamed_a_backend(&self) -> bool {
        self.with_fallback(|f| f.blamed_a_backend()).unwrap_or(false)
    }

    /// Clear the record of the backend being tried before starting again (the failure was caught),
    /// returning it for [`Startup::retry`].
    fn caught(&self) -> Option<backend::Backend> {
        self.with_fallback(|f| f.finished()).flatten()
    }

    /// Starting again failed: the record says this start is still trying `trying`.
    fn retry(&self, trying: Option<backend::Backend>) {
        self.with_fallback(|f| f.retry(trying));
    }

    /// wgpu picked an adapter of `backend` ([`backend::Fallback::started_with`]).
    pub fn started_with(&self, backend: wgpu::Backend) {
        if let Some(b) = backend::Backend::of(backend) {
            self.with_fallback(|f| f.started_with(b));
        }
    }

    /// The status-bar line for a start that moved away from a backend that crashed.
    pub fn status_line(&self) -> Option<String> {
        self.with_fallback(|f| f.status_line()).flatten()
    }

    fn with_fallback<T>(&self, f: impl FnOnce(&mut backend::Fallback) -> T) -> Option<T> {
        self.fallback.lock().unwrap_or_else(PoisonError::into_inner).as_mut().map(f)
    }
}

/// One look at the window's start, every [`WATCH_TICK`].
#[derive(Clone, Copy, Debug)]
struct Sample {
    /// Passes the UI ran; from 2 on, the first frame was presented.
    frames: u64,
    /// The UI thread is in the app's own code.
    busy: bool,
    /// [`Startup::leaves`].
    leaves: u64,
    /// The window is known to be minimized, occluded or without focus: a compositor may hold its
    /// frames back (a Wayland surface that isn't shown blocks the next frame).
    hidden: bool,
}

/// What [`Watch::step`] makes of the window's start.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Wait,
    /// The first frame was presented: nothing more to watch.
    Shown,
    /// The first frame is late: log it and say so, once.
    Late,
    /// The UI thread has been stuck in the graphics stack for [`HANG_TIMEOUT`].
    Hung,
}

/// The first-frame watch. VectorCraft gives up on an adapter once the first frame is
/// [`FIRST_FRAME_TIMEOUT`] late. That also fired on healthy starts: a window on a locked screen or
/// another workspace whose compositor holds its frames back, or a large document composing on a
/// slow machine. So here a late frame is only reported; the adapter counts as hanging only when the
/// UI thread is stuck outside the app's code (in eframe, wgpu or the driver), makes no progress at
/// all for [`HANG_TIMEOUT`], and the window isn't known to be hidden. A busy UI thread, however
/// long, never counts.
#[derive(Debug, Default)]
struct Watch {
    /// Time waited for the first frame, the window not known to be hidden.
    waited: Duration,
    /// Time the UI thread has been stuck outside the app's code without progress.
    stuck: Duration,
    leaves: u64,
    late: bool,
}

impl Watch {
    fn step(&mut self, s: Sample, dt: Duration) -> Verdict {
        if s.frames >= 2 {
            return Verdict::Shown;
        }
        if s.hidden {
            self.stuck = Duration::ZERO;
            return Verdict::Wait;
        }
        self.waited = self.waited.saturating_add(dt);
        if s.busy || s.leaves != self.leaves {
            self.leaves = s.leaves;
            self.stuck = Duration::ZERO;
        } else {
            self.stuck = self.stuck.saturating_add(dt);
        }
        if self.stuck >= HANG_TIMEOUT {
            Verdict::Hung
        } else if self.waited >= FIRST_FRAME_TIMEOUT && !self.late {
            self.late = true;
            Verdict::Late
        } else {
            Verdict::Wait
        }
    }
}

/// How often [`watch_first_frame`] looks.
const WATCH_TICK: Duration = Duration::from_millis(250);

/// Watch the window's first frame from another thread, from the app's creation
/// ([`Startup::ready`]); see [`Watch`]. Each look also asks for a repaint, so a UI thread that is
/// only idle draws its second frame and ends the watch. When the adapter hangs, start the app again
/// without it, as [`finish`] does for one that failed, and end this process, whose UI thread is
/// stuck in the graphics driver.
pub fn watch_first_frame(startup: Arc<Startup>) {
    let watch = move || {
        let mut watch = Watch::default();
        loop {
            std::thread::sleep(WATCH_TICK);
            if startup.ready.get().is_none() {
                continue;
            }
            let ui = startup.ui.get();
            let hidden = ui.is_some_and(|c| {
                c.input(|i| {
                    let v = i.viewport();
                    v.minimized == Some(true) || v.occluded == Some(true) || v.focused == Some(false)
                })
            });
            let sample = Sample {
                frames: ui.map_or(0, egui::Context::cumulative_frame_nr),
                busy: startup.busy.load(Ordering::Relaxed),
                leaves: startup.leaves.load(Ordering::Relaxed),
                hidden,
            };
            if let Some(c) = ui {
                c.request_repaint();
            }
            let adapter = startup.adapter.get().map_or("(none)", String::as_str);
            match watch.step(sample, WATCH_TICK) {
                Verdict::Wait => continue,
                Verdict::Shown => return,
                Verdict::Late => {
                    log::warn!(
                        "the window's first frame hasn't reached the screen in {}s on graphics adapter {adapter}",
                        FIRST_FRAME_TIMEOUT.as_secs()
                    );
                    *startup.notice.lock().unwrap_or_else(PoisonError::into_inner) = Some(format!(
                        "The window took over {} s to show on {adapter}. If it stays black, see the log; WGPU_ADAPTER_NAME or WGPU_POWER_PREF chooses another graphics processor.",
                        FIRST_FRAME_TIMEOUT.as_secs()
                    ));
                    continue;
                }
                Verdict::Hung => {}
            }
            let mut skip = startup.skip.clone();
            let Some(failed) = restart_without(true, sample.frames, startup.adapter.get().map(String::as_str), &skip) else { return };
            log::error!(
                "the window's first frame didn't reach the screen and the UI thread was stuck in the graphics stack for {}s on graphics adapter {failed}: starting again without it",
                HANG_TIMEOUT.as_secs()
            );
            skip.push(failed.to_string());
            match restart(&skip, &startup) {
                Ok(()) => end_hung_process(),
                Err(e) => {
                    log::error!("starting again failed: {e}");
                    return;
                }
            }
        }
    };
    if let Err(e) = std::thread::Builder::new().name("first frame".into()).spawn(watch) {
        log::warn!("no watch on the window's first frame: {e}");
    }
}

/// End this process from the watch thread while its UI thread is stuck in the graphics driver.
/// On Windows `exit` runs the DLLs' detach code, which can wait on that thread forever and leave an
/// invisible process holding the log and the control port, so the process is terminated.
fn end_hung_process() {
    log::logger().flush();
    #[cfg(windows)]
    if let Err(e) = winsafe::HPROCESS::GetCurrentProcess().TerminateProcess(0) {
        log::error!("ending the process failed: {e}");
    }
    std::process::exit(0);
}

/// eframe's adapter choice: the first of [`adapter_order`] for `power`, the GPUs that drive a
/// `displays`, `WGPU_ADAPTER_NAME` and the adapters a restart left out. Every adapter, the displays
/// and the choice are logged: which GPU draws is the first question in every black-window report.
pub fn selector(power: PowerPreference, displays: Vec<DisplayGpu>, startup: Arc<Startup>) -> NativeAdapterSelectorMethod {
    let named = std::env::var(NAME_ENV).ok();
    let skip = startup.skip.clone();
    Arc::new(move |adapters, surface| {
        let candidates: Vec<Candidate> = adapters.iter().map(|a| Candidate::new(a, surface)).collect();
        let order = adapter_order(&candidates, power, &displays, named.as_deref(), &skip);
        let (order, passed_over) = without_software(order, &candidates, startup.software_last());
        if passed_over {
            startup.software_left.store(true, Ordering::Relaxed);
            log::info!("software renderers passed over while another backend is left to try");
        }
        let listed: Vec<String> = candidates
            .iter()
            .map(|c| {
                let (drives_display, drives_primary) = c.drives(&displays);
                let note = if !c.presents {
                    ", can't show the window"
                } else if skip.contains(&c.key) {
                    ", failed before"
                } else if drives_primary {
                    ", drives the primary display"
                } else if drives_display {
                    ", drives a display"
                } else {
                    ""
                };
                format!("{} ({:?}, {:?}, {}{note})", c.name.trim(), c.backend, c.device_type, c.key)
            })
            .collect();
        log::info!("graphics adapters: {}", if listed.is_empty() { "none".to_string() } else { listed.join("; ") });
        let first = order.first().and_then(|&i| adapters.get(i).zip(candidates.get(i)));
        let Some((adapter, chosen)) = first else {
            return Err(format!("no graphics adapter can show the window (power preference {power:?}, {} adapters)", adapters.len()));
        };
        let why = match chosen.drives(&displays) {
            (_, true) => "it drives the primary display".to_string(),
            (true, false) => "it drives a display".to_string(),
            (false, false) if displays.is_empty() => format!("power preference {power:?}"),
            (false, false) => format!("no adapter matches a display GPU, so power preference {power:?}"),
        };
        log::info!("drawing on {} ({:?}, {:?}): {why}", chosen.name.trim(), chosen.backend, chosen.device_type);
        // Set once; eframe picks the adapter once.
        let _ = startup.adapter.set(chosen.key.clone());
        Ok(adapter.clone())
    })
}

/// `order` without software renderers when `software_last`, and whether it dropped any.
fn without_software(order: Vec<usize>, candidates: &[Candidate], software_last: bool) -> (Vec<usize>, bool) {
    if !software_last {
        return (order, false);
    }
    let before = order.len();
    let order: Vec<usize> = order.into_iter().filter(|&i| candidates.get(i).is_some_and(|c| c.device_type != DeviceType::Cpu)).collect();
    let dropped = order.len() != before;
    (order, dropped)
}

/// The last panic came from the graphics stack (see [`watch_panics`]).
static GRAPHICS_PANIC: AtomicBool = AtomicBool::new(false);

/// Note whether each panic comes from the graphics stack, so [`finish`] can tell a failing adapter
/// from a bug. The previous hook still reports it.
pub fn watch_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        GRAPHICS_PANIC.store(info.location().is_some_and(|l| in_graphics_stack(l.file())), Ordering::Relaxed);
        previous(info);
    }));
}

/// A source file of wgpu (`wgpu-core-30.0.1/src/…`, `wgpu-hal-…`), egui-wgpu or naga.
fn in_graphics_stack(file: &str) -> bool {
    file.split(['/', '\\']).any(|dir| ["wgpu-", "egui-wgpu-", "naga-"].iter().any(|p| dir.starts_with(p)))
}

/// The adapter to start again without: the one picked, when the graphics failed while the window
/// was starting up and it wasn't already left out.
fn restart_without<'a>(graphics_failed: bool, frames: u64, picked: Option<&'a str>, skip: &[String]) -> Option<&'a str> {
    picked.filter(|p| graphics_failed && frames < STARTUP_FRAMES && !skip.iter().any(|s| s == p))
}

/// After the window's run: `Ok` when it closed normally or the app started again (see the module
/// docs), otherwise why it failed. `outcome` is eframe's result, or the message of a panic that
/// ended it.
pub fn finish(outcome: Result<eframe::Result, String>, startup: &Startup) -> Result<(), String> {
    let (why, graphics_failed) = match outcome {
        Ok(Ok(())) => return Ok(()),
        Ok(Err(e @ eframe::Error::Wgpu(_))) => (e.to_string(), true),
        Ok(Err(e)) => (e.to_string(), false),
        Err(panic) => (panic, GRAPHICS_PANIC.load(Ordering::Relaxed)),
    };
    let frames = startup.ui.get().map_or(0, egui::Context::cumulative_frame_nr);
    let starting = graphics_failed && frames < STARTUP_FRAMES;
    let next = if starting {
        let fallback = startup.with_fallback(|f| (f.backend, f.next_after(&backend::skipped_backends(&startup.skip))));
        next_start(&startup.skip, startup.adapter.get().map(String::as_str), fallback, startup.software_left.load(Ordering::Relaxed))
    } else {
        None
    };
    let Some((skip, what)) = next else {
        // Nothing is left to try. When not even an adapter could be picked, the backend simply
        // can't show a window here — which says nothing about the driver faults the record
        // remembers. Without this, a start that was killed before its first frame (so the next
        // one moved to a fallback backend that can't present on this machine) would leave the
        // app unable to start at all. Forget the record and start over, once.
        if graphics_failed && startup.adapter.get().is_none() && startup.blamed_a_backend() && !startup.skip.iter().any(|k| k == RETRIED) {
            log::error!("the window failed ({why}): no graphics adapter could show it; forgetting the recorded failures and starting again");
            startup.forget_failures();
            return restart(&retry_skip(&startup.skip), startup).map_err(|e| format!("{why}; starting again failed: {e}"));
        }
        return Err(if graphics_failed { format!("{why} ({HELP})") } else { why });
    };
    log::error!("the window failed ({why}): starting again {what}");
    restart(&skip, startup).map_err(|e| format!("{why}; starting again failed: {e}"))
}

/// The skip list of the start after one that found no graphics adapter at all: the adapters
/// earlier restarts left out stay out (nothing recorded says they recovered), while the backend
/// exclusions go, since dropping those is what this retry is for.
fn retry_skip(skip: &[String]) -> Vec<String> {
    let mut retry: Vec<String> = skip.iter().filter(|k| leaves_out_an_adapter(k)).cloned().collect();
    retry.push(RETRIED.to_string());
    retry
}

/// Whether a skip key names one adapter (`Dx12:10de:2204`, `Metal:Intel Iris Pro Graphics`)
/// rather than a whole backend's [`backend::Backend::key`] or one of this module's markers.
fn leaves_out_an_adapter(key: &str) -> bool {
    !matches!(key, SOFTWARE | SOFTWARE_LEFT | RETRIED) && backend::Backend::from_key(key).is_none()
}

/// The skip list to start again with after the graphics failed while the window was starting up,
/// and what it changes (for the log); `None` when nothing is left to try. In this order: without the
/// adapter `picked`; on the next backend (`fallback`: this start's backend and the next one, see
/// [`backend::Fallback::next_after`]); with software renderers allowed, once, when a start passed
/// one over (`software_left`, or [`SOFTWARE_LEFT`] in `skip`).
fn next_start(
    skip: &[String],
    picked: Option<&str>,
    fallback: Option<(backend::Backend, Option<backend::Backend>)>,
    software_left: bool,
) -> Option<(Vec<String>, String)> {
    let mut next = skip.to_vec();
    if let Some(failed) = restart_without(true, 0, picked, skip) {
        next.push(failed.to_string());
        return Some((next, format!("without graphics adapter {failed}")));
    }
    let software_left = software_left || skip.iter().any(|k| k == SOFTWARE_LEFT);
    if let Some((from, Some(to))) = fallback {
        next.push(from.key().to_string());
        if software_left && !next.iter().any(|k| k == SOFTWARE_LEFT) {
            next.push(SOFTWARE_LEFT.to_string());
        }
        return Some((next, format!("with {} (no {} graphics adapter could show the window)", to.name(), from.name())));
    }
    if software_left && !skip.iter().any(|k| k == SOFTWARE) {
        // Every backend again, now with their software renderers; the adapters that failed stay out.
        next.retain(|k| k != SOFTWARE_LEFT && backend::Backend::from_key(k).is_none());
        next.push(SOFTWARE.to_string());
        return Some((next, "on a software renderer (no graphics processor could show the window)".to_string()));
    }
    None
}

/// Start the app again with the same arguments, the documents the OS handed this run
/// ([`Startup::opening`]) and the skip list `skip` ([`SKIP_ARG`]). On Unix
/// the new app replaces this process (same process id, so an AppImage keeps its files mounted),
/// and this returns only on failure; on Windows it starts beside this one, which then exits. The
/// failure was caught, so `gpu.json` stops blaming the backend first, and blames it again when
/// starting again fails.
fn restart(skip: &[String], startup: &Startup) -> std::io::Result<()> {
    log::logger().flush();
    let mut c = std::process::Command::new(std::env::current_exe()?);
    c.args(std::env::args_os().skip(1).filter(|a| a.to_str().is_none_or(|a| skip_arg(a).is_none())));
    c.args(startup.documents.lock().unwrap_or_else(PoisonError::into_inner).iter());
    c.arg(format!("{SKIP_ARG}{}", skip.join(",")));
    let trying = startup.caught();
    #[cfg(unix)]
    let result = {
        use std::os::unix::process::CommandExt as _;
        Err(c.exec())
    };
    #[cfg(not(unix))]
    let result = c.spawn().map(|_| ());
    if result.is_err() {
        startup.retry(trying);
    }
    result
}

#[cfg(test)]
mod tests;
