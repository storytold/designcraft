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
        return Err(if graphics_failed { format!("{why} ({HELP})") } else { why });
    };
    log::error!("the window failed ({why}): starting again {what}");
    restart(&skip, startup).map_err(|e| format!("{why}; starting again failed: {e}"))
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
mod tests {
    use super::*;

    fn gpu(name: &str, backend: Backend, device_type: DeviceType) -> Candidate {
        Candidate { key: format!("{backend:?}:{name}"), name: name.into(), backend, device_type, pci: (0, 0), presents: true }
    }

    fn names(c: &[Candidate], order: &[usize]) -> Vec<String> {
        order.iter().map(|&i| c[i].name.clone()).collect()
    }

    const NVIDIA_3090: (u32, u32) = (0x10de, 0x2204);
    const AMD_IGPU: (u32, u32) = (0x1002, 0x164e);
    const INTEL_IGPU: (u32, u32) = (0x8086, 0xa780);
    const WARP: (u32, u32) = (0x1414, 0x008c);

    fn pci_gpu(name: &str, backend: Backend, device_type: DeviceType, pci: (u32, u32)) -> Candidate {
        Candidate { key: key(backend, pci.0, pci.1, name), name: name.into(), backend, device_type, pci, presents: true }
    }

    fn monitor(pci: (u32, u32)) -> DisplayGpu {
        DisplayGpu { pci, primary: false }
    }

    fn primary(pci: (u32, u32)) -> DisplayGpu {
        DisplayGpu { pci, primary: true }
    }

    /// A Windows desktop as wgpu lists it with every backend: a Ryzen's integrated GPU without a
    /// monitor and an RTX 3090 driving both monitors, each under Vulkan and DX12, then WARP and
    /// OpenGL (which reports no device id).
    fn ryzen_desktop() -> Vec<Candidate> {
        vec![
            pci_gpu("NVIDIA GeForce RTX 3090", Backend::Vulkan, DeviceType::DiscreteGpu, NVIDIA_3090),
            pci_gpu("AMD Radeon(TM) Graphics", Backend::Vulkan, DeviceType::IntegratedGpu, AMD_IGPU),
            pci_gpu("NVIDIA GeForce RTX 3090", Backend::Dx12, DeviceType::DiscreteGpu, NVIDIA_3090),
            pci_gpu("AMD Radeon(TM) Graphics", Backend::Dx12, DeviceType::IntegratedGpu, AMD_IGPU),
            pci_gpu("Microsoft Basic Render Driver", Backend::Dx12, DeviceType::Cpu, WARP),
            pci_gpu("NVIDIA GeForce RTX 3090/PCIe/SSE2", Backend::Gl, DeviceType::Other, (NVIDIA_3090.0, 0)),
        ]
    }

    /// Power saving picked the integrated GPU, whose driver reset when Windows had to present its
    /// frames on the monitors of the NVIDIA card. The GPU that drives the displays comes first,
    /// through DX12; the integrated GPU stays the fallback a restart reaches.
    #[test]
    fn a_desktop_draws_on_the_gpu_that_drives_its_monitors() {
        let c = ryzen_desktop();
        for displays in [vec![primary(NVIDIA_3090)], vec![monitor(NVIDIA_3090)]] {
            assert_eq!(adapter_order(&c, PowerPreference::LowPower, &displays, None, &[]), [2, 0, 3, 1, 5, 4], "{displays:?}");
            assert_eq!(adapter_order(&c, PowerPreference::None, &displays, None, &[]), [2, 0, 3, 1, 5, 4], "{displays:?}");
        }
        // Without the display information the order is power saving's.
        assert_eq!(adapter_order(&c, PowerPreference::LowPower, &[], None, &[]), [3, 1, 2, 0, 5, 4]);
        // After the NVIDIA adapters failed, the integrated GPU is next.
        let skip = vec![c[2].key.clone(), c[0].key.clone()];
        assert_eq!(adapter_order(&c, PowerPreference::LowPower, &[primary(NVIDIA_3090)], None, &skip), [3, 1, 5, 4]);
    }

    /// A hybrid laptop: the panel (primary) on the integrated GPU, an external monitor on the
    /// discrete one. Power saving and the display agree, in either enumeration order.
    #[test]
    fn a_hybrid_laptop_draws_on_the_integrated_gpu_that_drives_its_panel() {
        let c = ryzen_desktop();
        for displays in [vec![primary(AMD_IGPU), monitor(NVIDIA_3090)], vec![monitor(NVIDIA_3090), primary(AMD_IGPU)], vec![primary(AMD_IGPU)]] {
            assert_eq!(adapter_order(&c, PowerPreference::LowPower, &displays, None, &[]), [3, 1, 2, 0, 5, 4], "{displays:?}");
        }
        // Linux names no primary unless there is a built-in panel: a monitor on each GPU keeps the
        // system's order among them (Mesa puts the compositor's GPU first), and a GPU without one
        // comes after both.
        let linux = vec![
            pci_gpu("AMD Radeon Graphics (RADV RAPHAEL_MENDOCINO)", Backend::Vulkan, DeviceType::IntegratedGpu, AMD_IGPU),
            pci_gpu("NVIDIA GeForce RTX 3090", Backend::Vulkan, DeviceType::DiscreteGpu, NVIDIA_3090),
            pci_gpu("Intel(R) Arc(TM) A380", Backend::Vulkan, DeviceType::DiscreteGpu, INTEL_IGPU),
            pci_gpu("llvmpipe (LLVM 20.1.8, 256 bits)", Backend::Vulkan, DeviceType::Cpu, (0x10005, 0)),
        ];
        let both = [monitor(AMD_IGPU), monitor(NVIDIA_3090)];
        assert_eq!(adapter_order(&linux, PowerPreference::None, &both, None, &[]), [0, 1, 2, 3]);
        assert_eq!(adapter_order(&linux, PowerPreference::None, &[monitor(INTEL_IGPU)], None, &[]), [2, 0, 1, 3]);
        assert_eq!(adapter_order(&linux, PowerPreference::None, &[monitor(NVIDIA_3090), primary(AMD_IGPU)], None, &[]), [0, 1, 2, 3]);
    }

    /// A Windows desktop with a monitor on each GPU: the window opens on the primary display, so
    /// its GPU draws, whichever kind it is.
    #[test]
    fn the_primary_display_decides_between_gpus_that_both_drive_a_monitor() {
        let c = ryzen_desktop();
        assert_eq!(adapter_order(&c, PowerPreference::LowPower, &[monitor(AMD_IGPU), primary(NVIDIA_3090)], None, &[]), [2, 0, 3, 1, 5, 4]);
        assert_eq!(adapter_order(&c, PowerPreference::LowPower, &[primary(AMD_IGPU), monitor(NVIDIA_3090)], None, &[]), [3, 1, 2, 0, 5, 4]);
        // The displays only matter among adapters that can show the window; a GPU driving no
        // display still loses to one that does.
        assert_eq!(adapter_order(&c, PowerPreference::LowPower, &[monitor(AMD_IGPU), monitor(INTEL_IGPU)], None, &[]), [3, 1, 2, 0, 5, 4]);
    }

    /// Display GPUs none of the adapters match (another driver's ids, OpenGL's missing device id,
    /// Metal's zeros) change nothing: power saving's order.
    #[test]
    fn displays_no_adapter_matches_leave_the_power_preference_in_charge() {
        let c = ryzen_desktop();
        let low = adapter_order(&c, PowerPreference::LowPower, &[], None, &[]);
        for displays in [vec![monitor(INTEL_IGPU)], vec![primary((0x8086, 0x1234)), monitor((0x8086, 0x5678))], vec![primary((0, 0))]] {
            assert_eq!(adapter_order(&c, PowerPreference::LowPower, &displays, None, &[]), low, "{displays:?}");
        }
        assert_eq!(names(&c, &adapter_order(&c, PowerPreference::HighPerformance, &[monitor(INTEL_IGPU)], None, &[]))[0], "NVIDIA GeForce RTX 3090");
        // The Wayland desktop's adapters report no ids in these tests: the primary display never matches.
        let c = wayland_desktop();
        assert_eq!(
            adapter_order(&c, PowerPreference::None, &[primary(NVIDIA_3090)], None, &[]),
            adapter_order(&c, PowerPreference::None, &[], None, &[])
        );
        assert!(adapter_order(&[], PowerPreference::LowPower, &[primary(NVIDIA_3090)], None, &[]).is_empty());
    }

    /// `WGPU_ADAPTER_NAME` and "can't present" still come before the displays.
    #[test]
    fn a_named_adapter_comes_before_the_one_driving_the_display() {
        let mut c = ryzen_desktop();
        let displays = [primary(NVIDIA_3090)];
        assert_eq!(adapter_order(&c, PowerPreference::LowPower, &displays, Some("radeon"), &[]), [3, 1, 2, 0, 5, 4]);
        c[2].presents = false;
        c[0].presents = false;
        assert_eq!(adapter_order(&c, PowerPreference::LowPower, &displays, None, &[]), [3, 1, 5, 4]);
    }

    /// `WGPU_POWER_PREF`, the user's own choice, is kept as it is; only without it do the displays
    /// count.
    #[test]
    fn only_automatic_considers_which_gpu_drives_the_display() {
        assert!(automatic(None));
        assert!(!automatic(Some(PowerPreference::None)));
        assert!(!automatic(Some(PowerPreference::LowPower)));
        assert!(preferred_displays(Some(PowerPreference::HighPerformance)).is_empty());
        // Automatic reports whatever this machine has (nothing on macOS, in CI, or under Remote Desktop).
        assert_eq!(preferred_displays(None), display_gpus());
    }

    #[test]
    fn pci_ids_come_from_windows_device_ids() {
        assert_eq!(pci_ids("PCI\\VEN_10DE&DEV_2204&SUBSYS_40421458&REV_A1"), Some(NVIDIA_3090));
        assert_eq!(pci_ids("PCI\\VEN_1002&DEV_164E&SUBSYS_88771043&REV_C1"), Some(AMD_IGPU));
        assert_eq!(pci_ids("pci\\ven_1002&dev_164e"), Some(AMD_IGPU));
        // Remote Desktop and other non-PCI display devices, and malformed ids.
        assert_eq!(pci_ids("ROOT\\BasicDisplay\\0000"), None);
        assert_eq!(pci_ids("PCI\\VEN_10DE&SUBSYS_40421458"), None);
        assert_eq!(pci_ids("PCI\\VEN_10DE&DEV_ZZZZ"), None);
        assert_eq!(pci_ids("VEN_&DEV_"), None);
        assert_eq!(pci_ids("PCI\\VEN_10DÉ&DEV_2204"), None);
        assert_eq!(pci_ids(""), None);
    }

    /// Whatever this machine has: every GPU listed is a PCI device, at most one drives the primary
    /// display, and none is listed twice. (A headless CI runner may list none.)
    #[cfg(windows)]
    #[test]
    fn windows_lists_each_gpu_with_a_monitor_once() {
        let gpus = windows_display_gpus();
        assert!(gpus.iter().filter(|g| g.primary).count() <= 1, "{gpus:?}");
        for (i, g) in gpus.iter().enumerate() {
            assert_ne!(g.pci.0, 0, "{gpus:?}");
            assert!(!gpus[..i].iter().any(|h| h.pci == g.pci), "{gpus:?}");
        }
    }

    #[test]
    fn internal_panels_are_edp_lvds_and_dsi_connectors() {
        for name in ["eDP-1", "LVDS-1", "DSI-1"] {
            assert!(is_internal_panel(name), "{name}");
        }
        for name in ["DP-3", "HDMI-A-1", "DVI-D-1", "VGA-1", "Writeback-1", ""] {
            assert!(!is_internal_panel(name), "{name}");
        }
    }

    /// A sysfs like a hybrid laptop's with an external monitor: the panel and a disconnected port
    /// on the integrated GPU (card0), the monitor on the discrete one (card1), and a card without
    /// PCI ids. Each GPU is listed once, the one with the panel as primary.
    #[test]
    fn linux_display_gpus_come_from_the_connected_connectors() -> std::io::Result<()> {
        let dir = std::env::temp_dir().join(format!("designcraft-drm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let card = |name: &str, (vendor, device): (u32, u32)| -> std::io::Result<()> {
            std::fs::create_dir_all(dir.join(name).join("device"))?;
            std::fs::write(dir.join(name).join("device/vendor"), format!("{vendor:#06x}\n"))?;
            std::fs::write(dir.join(name).join("device/device"), format!("{device:#06x}\n"))
        };
        let connector = |name: &str, status: &str| -> std::io::Result<()> {
            std::fs::create_dir_all(dir.join(name))?;
            std::fs::write(dir.join(name).join("status"), format!("{status}\n"))
        };
        card("card0", AMD_IGPU)?;
        card("card1", NVIDIA_3090)?;
        std::fs::create_dir_all(dir.join("card2"))?;
        connector("card0-eDP-1", "connected")?;
        connector("card0-HDMI-A-1", "disconnected")?;
        connector("card1-DP-1", "connected")?;
        connector("card1-DP-2", "connected")?;
        connector("card2-Virtual-1", "connected")?;
        std::fs::write(dir.join("renderD128"), "")?;
        let mut gpus = linux_display_gpus(&dir);
        gpus.sort_by_key(|g| g.pci);
        assert_eq!(gpus, [primary(AMD_IGPU), monitor(NVIDIA_3090)]);
        assert!(linux_display_gpus(&dir.join("no such dir")).is_empty());
        std::fs::remove_dir_all(&dir)
    }

    #[test]
    fn display_gpus_are_logged_as_pci_ids() {
        assert_eq!(primary(NVIDIA_3090).to_string(), "10de:2204 (primary)");
        assert_eq!(monitor(AMD_IGPU).to_string(), "1002:164e");
        let mut gpus = vec![];
        add_display_gpu(&mut gpus, NVIDIA_3090, false);
        add_display_gpu(&mut gpus, NVIDIA_3090, true);
        add_display_gpu(&mut gpus, AMD_IGPU, false);
        assert_eq!(gpus, [primary(NVIDIA_3090), monitor(AMD_IGPU)]);
    }

    /// A dual-GPU Linux desktop as Vulkan lists it with Mesa's device-select layer: the GPU KDE's
    /// compositor runs on (NVIDIA) first, then the Ryzen's integrated GPU, Mesa's software
    /// renderer, and OpenGL.
    fn wayland_desktop() -> Vec<Candidate> {
        vec![
            gpu("NVIDIA GeForce RTX 5070", Backend::Vulkan, DeviceType::DiscreteGpu),
            gpu("AMD Ryzen 9 7900X (RADV RAPHAEL_MENDOCINO)", Backend::Vulkan, DeviceType::IntegratedGpu),
            gpu("llvmpipe (LLVM 20.1.8, 256 bits)", Backend::Vulkan, DeviceType::Cpu),
            gpu("NVIDIA GeForce RTX 5070/PCIe/SSE2", Backend::Gl, DeviceType::Other),
        ]
    }

    #[test]
    fn automatic_keeps_the_systems_order_on_linux_and_saves_power_elsewhere() {
        let c = wayland_desktop();
        let order = adapter_order(&c, power_preference(None), &[], None, &[]);
        let first = &c[order[0]].name;
        if cfg!(any(windows, target_os = "macos")) {
            assert_eq!(first, "AMD Ryzen 9 7900X (RADV RAPHAEL_MENDOCINO)");
        } else {
            // The integrated GPU couldn't show frames to a compositor on the NVIDIA GPU.
            assert_eq!(first, "NVIDIA GeForce RTX 5070");
        }
    }

    #[test]
    fn each_preference_orders_the_adapters_then_falls_back_to_opengl_and_software() {
        let c = wayland_desktop();
        let order = |power| names(&c, &adapter_order(&c, power, &[], None, &[]));
        assert_eq!(
            order(PowerPreference::None),
            [
                "NVIDIA GeForce RTX 5070",
                "AMD Ryzen 9 7900X (RADV RAPHAEL_MENDOCINO)",
                "NVIDIA GeForce RTX 5070/PCIe/SSE2",
                "llvmpipe (LLVM 20.1.8, 256 bits)"
            ]
        );
        assert_eq!(
            order(PowerPreference::LowPower),
            [
                "AMD Ryzen 9 7900X (RADV RAPHAEL_MENDOCINO)",
                "NVIDIA GeForce RTX 5070",
                "NVIDIA GeForce RTX 5070/PCIe/SSE2",
                "llvmpipe (LLVM 20.1.8, 256 bits)"
            ]
        );
        assert_eq!(
            order(PowerPreference::HighPerformance),
            [
                "NVIDIA GeForce RTX 5070",
                "AMD Ryzen 9 7900X (RADV RAPHAEL_MENDOCINO)",
                "NVIDIA GeForce RTX 5070/PCIe/SSE2",
                "llvmpipe (LLVM 20.1.8, 256 bits)"
            ]
        );
    }

    /// A restart leaves out the adapter the window failed on, and once every adapter failed there
    /// is nothing to try. A dual-GPU Mac reports both GPUs under Metal with no PCI ids: their keys
    /// still differ, so when the integrated GPU fails, the restart renders on the discrete one.
    #[test]
    fn gpus_without_pci_ids_are_told_apart_by_name() {
        let mac = |name: &str, device_type| Candidate {
            key: key(Backend::Metal, 0, 0, name),
            name: name.into(),
            backend: Backend::Metal,
            device_type,
            pci: (0, 0),
            presents: true,
        };
        let c = vec![mac("NVIDIA GeForce GT 750M", DeviceType::DiscreteGpu), mac("Intel Iris Pro Graphics", DeviceType::IntegratedGpu)];
        assert_eq!((c[0].key.as_str(), c[1].key.as_str()), ("Metal:NVIDIA GeForce GT 750M", "Metal:Intel Iris Pro Graphics"));
        let first = adapter_order(&c, PowerPreference::LowPower, &[], None, &[]);
        assert_eq!(names(&c, &first)[0], "Intel Iris Pro Graphics");
        let skip = parse_skip(&c[first[0]].key);
        assert_eq!(names(&c, &adapter_order(&c, PowerPreference::LowPower, &[], None, &skip)), ["NVIDIA GeForce GT 750M"]);
        // A comma in a name can't split the list.
        assert_eq!(parse_skip(&key(Backend::Gl, 0, 0, "Mesa, llvmpipe")), ["Gl:Mesa  llvmpipe"]);
    }

    fn sample(frames: u64, busy: bool, leaves: u64, hidden: bool) -> Sample {
        Sample { frames, busy, leaves, hidden }
    }

    /// Runs `ticks` looks with the same sample; the verdicts other than `Wait`.
    fn run(w: &mut Watch, s: Sample, ticks: u32) -> Vec<(u32, Verdict)> {
        (1..=ticks).filter_map(|t| Some((t, w.step(s, WATCH_TICK))).filter(|(_, v)| *v != Verdict::Wait)).collect()
    }

    const TICKS_PER_S: u32 = 4;

    /// The UI thread stuck in the graphics stack (not in the app's code, no progress) with the
    /// window not known to be hidden: late after 20 s, hung after 60 s.
    #[test]
    fn a_ui_thread_stuck_in_the_graphics_stack_hangs_its_adapter() {
        let mut w = Watch::default();
        let v = run(&mut w, sample(1, false, 1, false), 61 * TICKS_PER_S);
        assert_eq!(v.first(), Some(&(20 * TICKS_PER_S, Verdict::Late)));
        // `leaves` moved from 0 to 1 on the first look, so the stuck time starts one look later.
        assert_eq!(v.get(1), Some(&(60 * TICKS_PER_S + 1, Verdict::Hung)));
        // The second frame began: the first was presented.
        assert_eq!(Watch::default().step(sample(2, false, 9, false), WATCH_TICK), Verdict::Shown);
    }

    /// A busy UI thread (a large document composing on a slow machine) is only ever late.
    #[test]
    fn a_busy_ui_thread_never_hangs_its_adapter() {
        let mut w = Watch::default();
        let v = run(&mut w, sample(0, true, 0, false), 600 * TICKS_PER_S);
        assert_eq!(v, [(20 * TICKS_PER_S, Verdict::Late)]);
        // Nor does one that keeps coming back from the graphics stack.
        let mut w = Watch::default();
        for t in 0..(600 * TICKS_PER_S) {
            assert_ne!(w.step(sample(1, false, u64::from(t), false), WATCH_TICK), Verdict::Hung);
        }
    }

    /// A window known to be hidden (locked screen, another workspace, minimized) may have its frames
    /// held back by the compositor: that time counts for nothing and starts the stuck time over.
    #[test]
    fn time_while_the_window_is_hidden_does_not_count() {
        let mut w = Watch::default();
        assert!(run(&mut w, sample(1, false, 0, true), 600 * TICKS_PER_S).is_empty());
        assert!(run(&mut w, sample(1, false, 0, false), 50 * TICKS_PER_S).iter().all(|(_, v)| *v == Verdict::Late));
        assert!(run(&mut w, sample(1, false, 0, true), 10 * TICKS_PER_S).is_empty());
        assert!(run(&mut w, sample(1, false, 0, false), 59 * TICKS_PER_S).is_empty());
        assert_eq!(run(&mut w, sample(1, false, 0, false), TICKS_PER_S), [(TICKS_PER_S, Verdict::Hung)]);
    }

    #[test]
    fn a_hung_adapter_is_left_out_once() {
        let skip = vec!["Dx12:10de:28a0".to_string()];
        assert_eq!(restart_without(true, 1, Some("Dx12:10de:28a0"), &skip), None);
        assert_eq!(restart_without(true, 1, Some("Dx12:8086:a7a8"), &skip), Some("Dx12:8086:a7a8"));
    }

    /// Windows, a hardware DX12 adapter failing: the software renderer (WARP) is passed over while
    /// another backend is left, the restarts go through OpenGL and Vulkan, and only then back to
    /// every backend with software allowed. The chain ends.
    #[test]
    fn software_renderers_come_after_every_backends_hardware() {
        use backend::Backend as B;
        let c = ryzen_desktop();
        let order = adapter_order(&c, PowerPreference::LowPower, &[], None, &[]);
        assert_eq!(without_software(order.clone(), &c, false), (order.clone(), false));
        let (hardware, dropped) = without_software(order, &c, true);
        assert!(dropped && !hardware.contains(&4), "{hardware:?}");

        let dx12 = "Dx12:10de:2204".to_string();
        // The adapter that failed first.
        let (skip, _) = next_start(&[], Some(&dx12), Some((B::Dx12, Some(B::Gl))), true).unwrap();
        assert_eq!(skip, [dx12.as_str()]);
        // Only WARP left on DX12 (passed over, nothing picked): OpenGL next.
        let (skip, what) = next_start(&skip, None, Some((B::Dx12, Some(B::Gl))), true).unwrap();
        assert_eq!(skip, [dx12.as_str(), "Dx12", SOFTWARE_LEFT]);
        assert!(what.contains("OpenGL"), "{what}");
        // OpenGL has no adapter, Vulkan next; then Vulkan has none and no backend is left.
        let (skip, _) = next_start(&skip, None, Some((B::Gl, Some(B::Vulkan))), false).unwrap();
        assert_eq!(skip, [dx12.as_str(), "Dx12", SOFTWARE_LEFT, "Gl"]);
        let (skip, what) = next_start(&skip, None, Some((B::Vulkan, None)), false).unwrap();
        assert_eq!(skip, [dx12.as_str(), SOFTWARE], "{what}");
        assert!(backend::skipped_backends(&skip).is_empty());
        // With software allowed and nothing left: the end.
        assert_eq!(next_start(&skip, None, Some((B::Dx12, None)), false), None);
        // Nothing was passed over (a Mac whose GPUs all failed): no software round.
        assert_eq!(next_start(&[], None, Some((B::Metal, None)), false), None);
        // Without the backend fallback (`WGPU_BACKEND`), only adapters.
        assert_eq!(next_start(&[], None, None, false), None);
    }

    /// The skip list travels as an argument, which processes the app starts don't inherit.
    #[test]
    fn the_skip_list_is_an_argument() {
        assert_eq!(skip_arg("--gpu-skip=Dx12:10de:2204,Dx12"), Some(vec!["Dx12:10de:2204".to_string(), "Dx12".to_string()]));
        assert_eq!(skip_arg("--gpu-skip="), Some(vec![]));
        assert_eq!(skip_arg("--gpu-skip"), None);
        assert_eq!(skip_arg("layout.designcraft"), None);
    }

    #[test]
    fn a_restart_tries_the_next_adapter() {
        let c = wayland_desktop();
        let mut skip = vec![];
        let mut tried = vec![];
        while let Some(&i) = adapter_order(&c, PowerPreference::LowPower, &[], None, &skip).first() {
            tried.push(c[i].name.clone());
            skip.push(c[i].key.clone());
        }
        assert_eq!(
            tried,
            [
                "AMD Ryzen 9 7900X (RADV RAPHAEL_MENDOCINO)",
                "NVIDIA GeForce RTX 5070",
                "NVIDIA GeForce RTX 5070/PCIe/SSE2",
                "llvmpipe (LLVM 20.1.8, 256 bits)"
            ]
        );
    }

    #[test]
    fn adapters_that_cannot_present_to_the_window_are_never_tried() {
        let mut c = wayland_desktop();
        c[0].presents = false;
        c[3].presents = false;
        for power in [PowerPreference::None, PowerPreference::LowPower, PowerPreference::HighPerformance] {
            let order = adapter_order(&c, power, &[], None, &[]);
            assert!(!order.contains(&0) && !order.contains(&3), "{power:?}: {order:?}");
            assert_eq!(order.len(), 2);
        }
        for p in &mut c {
            p.presents = false;
        }
        assert!(adapter_order(&c, PowerPreference::None, &[], None, &[]).is_empty());
        assert!(adapter_order(&[], PowerPreference::HighPerformance, &[], None, &[]).is_empty());
    }

    #[test]
    fn wgpu_adapter_name_picks_an_adapter_first_and_the_rest_stay_as_fallbacks() {
        let c = wayland_desktop();
        let order = adapter_order(&c, PowerPreference::HighPerformance, &[], Some("radv"), &[]);
        assert_eq!(c[order[0]].name, "AMD Ryzen 9 7900X (RADV RAPHAEL_MENDOCINO)");
        assert_eq!(order.len(), 4);
        // Any case; every match comes before the rest, natively first.
        let order = adapter_order(&c, PowerPreference::LowPower, &[], Some("GeForce"), &[]);
        assert_eq!(names(&c, &order[..2]), ["NVIDIA GeForce RTX 5070", "NVIDIA GeForce RTX 5070/PCIe/SSE2"]);
        // An empty name or one nothing matches changes nothing.
        for named in ["", "no such gpu"] {
            assert_eq!(
                adapter_order(&c, PowerPreference::LowPower, &[], Some(named), &[]),
                adapter_order(&c, PowerPreference::LowPower, &[], None, &[])
            );
        }
        // A named adapter that failed is left out like any other.
        let skip = vec![c[1].key.clone()];
        assert_eq!(adapter_order(&c, PowerPreference::None, &[], Some("radv"), &skip)[0], 0);
    }

    /// A Windows machine lists every GPU twice when Vulkan and DX12 are both in the instance:
    /// power saving still takes the integrated GPU first, each GPU through DX12 first.
    #[test]
    fn hybrid_laptop_on_windows_renders_on_the_integrated_gpu_with_power_saving() {
        let c = vec![
            gpu("Intel(R) Arc(TM) A370M", Backend::Vulkan, DeviceType::DiscreteGpu),
            gpu("Intel(R) Iris(R) Xe Graphics", Backend::Vulkan, DeviceType::IntegratedGpu),
            gpu("Intel(R) Arc(TM) A370M", Backend::Dx12, DeviceType::DiscreteGpu),
            gpu("Intel(R) Iris(R) Xe Graphics", Backend::Dx12, DeviceType::IntegratedGpu),
            gpu("Microsoft Basic Render Driver", Backend::Dx12, DeviceType::Cpu),
        ];
        let order = adapter_order(&c, PowerPreference::LowPower, &[], None, &[]);
        assert_eq!(order, [3, 1, 2, 0, 4]);
        assert_eq!(adapter_order(&c, PowerPreference::HighPerformance, &[], None, &[]), [2, 0, 3, 1, 4]);
    }

    /// One Intel GPU whose Vulkan driver made the window flicker black: DX12 comes first whatever
    /// the preference; Vulkan stays a fallback, and `WGPU_ADAPTER_NAME` or a restart leaving DX12
    /// out still reach it.
    #[test]
    fn a_windows_gpu_renders_through_dx12_before_vulkan() {
        let c = vec![
            gpu("Intel(R) Graphics", Backend::Vulkan, DeviceType::IntegratedGpu),
            gpu("Intel(R) Graphics", Backend::Dx12, DeviceType::IntegratedGpu),
            gpu("Microsoft Basic Render Driver", Backend::Dx12, DeviceType::Cpu),
            gpu("Intel(R) Graphics", Backend::Gl, DeviceType::Other),
        ];
        for power in [PowerPreference::LowPower, PowerPreference::HighPerformance, PowerPreference::None] {
            assert_eq!(adapter_order(&c, power, &[], None, &[]), [1, 0, 3, 2], "{power:?}");
        }
        assert_eq!(adapter_order(&c, PowerPreference::LowPower, &[], None, &[c[1].key.clone()]), [0, 3, 2], "DX12 failed: Vulkan next");
    }

    #[test]
    fn power_preference_follows_the_environment() {
        assert_eq!(power_preference(None), AUTOMATIC);
        for p in [PowerPreference::LowPower, PowerPreference::HighPerformance, PowerPreference::None] {
            assert_eq!(power_preference(Some(p)), p);
        }
        assert_eq!(AUTOMATIC, if cfg!(any(windows, target_os = "macos")) { PowerPreference::LowPower } else { PowerPreference::None });
    }

    /// The Intel Mac of #334 lost its Metal device compiling the compute shader of wgpu's indirect
    /// call check: off unless `WGPU_VALIDATION_INDIRECT_CALL` turns it on.
    #[test]
    fn the_indirect_call_check_is_off() {
        if std::env::var_os("WGPU_VALIDATION_INDIRECT_CALL").is_none() {
            assert!(!instance_flags().contains(wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL));
        }
        // The other flags of the build stay as wgpu sets them.
        let others = wgpu::InstanceFlags::from_build_config() - wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL;
        if std::env::vars_os().all(|(k, _)| !k.to_string_lossy().starts_with("WGPU_")) {
            assert_eq!(instance_flags(), others);
        }
    }

    #[test]
    fn skipped_adapters_parse_from_a_comma_separated_list() {
        assert_eq!(parse_skip("Vulkan:1002:164e, Gl:10de:2f04,,"), ["Vulkan:1002:164e", "Gl:10de:2f04"]);
        assert!(parse_skip("").is_empty());
        assert_eq!(key(Backend::Vulkan, 0x1002, 0x164e, "AMD Radeon"), "Vulkan:1002:164e");
        assert_eq!(key(Backend::Gl, 0x10de, 0x2f04, "NVIDIA"), "Gl:10de:2f04");
        // A whole backend left out is its bare name; it never matches an adapter's key.
        let skip = parse_skip("Dx12,Dx12:10de:2204");
        assert_eq!(backend::skipped_backends(&skip), [backend::Backend::Dx12]);
        let c = vec![pci_gpu("NVIDIA GeForce RTX 3090", Backend::Dx12, DeviceType::DiscreteGpu, NVIDIA_3090)];
        assert!(adapter_order(&c, PowerPreference::None, &[], None, &skip).is_empty());
        assert_eq!(adapter_order(&c, PowerPreference::None, &[], None, &skip[..1]), [0]);
    }

    #[test]
    fn panics_in_wgpu_egui_wgpu_and_naga_are_graphics_failures() {
        for file in [
            "/home/u/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-wgpu-0.36.2/src/winit.rs",
            r"C:\Users\u\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\wgpu-core-30.0.1\src\device\mod.rs",
            "/rustc/deps/wgpu-hal-30.0.1/src/vulkan/adapter.rs",
            "naga-30.0.0/src/back/spv/writer.rs",
        ] {
            assert!(in_graphics_stack(file), "{file}");
        }
        for file in [
            "apps/designcraft/src/main.rs",
            "/home/wgpu/designcraft/crates/ui-egui/src/lib.rs",
            "egui-0.36.2/src/context.rs",
            "winit-0.30.12/src/x.rs",
        ] {
            assert!(!in_graphics_stack(file), "{file}");
        }
    }

    #[test]
    fn only_a_graphics_failure_while_starting_up_restarts_without_the_adapter() {
        let amd = Some("Vulkan:1002:164e");
        assert_eq!(restart_without(true, 0, amd, &[]), amd);
        assert_eq!(restart_without(true, STARTUP_FRAMES - 1, amd, &[]), amd);
        // After start-up the adapter has shown frames; a lost window is something else.
        assert_eq!(restart_without(true, STARTUP_FRAMES, amd, &[]), None);
        // A bug outside the graphics, or no adapter picked: nothing another adapter would change.
        assert_eq!(restart_without(false, 0, amd, &[]), None);
        assert_eq!(restart_without(true, 0, None, &[]), None);
        // Never the same adapter twice, so restarts end.
        assert_eq!(restart_without(true, 0, amd, &["Vulkan:1002:164e".into()]), None);
    }

    /// Without a backend fallback (`WGPU_BACKEND` set), a start whose graphics failed with no
    /// adapter to leave out ends with the error and the hint; a bug ends with its message.
    #[test]
    fn a_normal_exit_or_another_failure_does_not_restart() {
        let startup = Startup::new(None, Vec::new());
        assert_eq!(finish(Ok(Ok(())), &startup), Ok(()));
        // No adapter was picked (none can show the window): the error is returned, nothing restarts.
        let none =
            eframe::Error::Wgpu(eframe::egui_wgpu::WgpuError::CustomNativeAdapterSelectionError("no graphics adapter can show the window".into()));
        assert_eq!(
            finish(Ok(Err(none)), &startup),
            Err(format!("WGPU error: Adapter selection failed: no graphics adapter can show the window ({HELP})"))
        );
        let _ = startup.adapter.set("Vulkan:1002:164e".into());
        GRAPHICS_PANIC.store(false, Ordering::Relaxed);
        assert_eq!(finish(Err("a bug".into()), &startup), Err("a bug".into()));
        // Nothing to record without a fallback.
        startup.presented();
        startup.started_with(wgpu::Backend::Gl);
        assert_eq!(startup.status_line(), None);
    }
}
