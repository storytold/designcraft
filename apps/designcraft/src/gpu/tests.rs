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

/// The retry after a start that found no adapter at all forgets the recorded backend failures but
/// keeps the adapters earlier restarts left out: they were hung, and the retry must not pick one
/// again only to wait for it once more.
#[test]
fn the_no_adapter_retry_keeps_the_adapters_earlier_restarts_left_out() {
    let skip = vec!["Dx12:10de:2204".to_string(), "Dx12".to_string(), SOFTWARE_LEFT.to_string(), "Gl".to_string(), "Vulkan:1002:164e".into()];
    assert_eq!(retry_skip(&skip), ["Dx12:10de:2204", "Vulkan:1002:164e", RETRIED]);
    assert_eq!(retry_skip(&[]), [RETRIED]);
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
        assert_eq!(adapter_order(&c, PowerPreference::LowPower, &[], Some(named), &[]), adapter_order(&c, PowerPreference::LowPower, &[], None, &[]));
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
    for file in
        ["apps/designcraft/src/main.rs", "/home/wgpu/designcraft/crates/ui-egui/src/lib.rs", "egui-0.36.2/src/context.rs", "winit-0.30.12/src/x.rs"]
    {
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
    // Without a record nothing is blamed either, so there is nothing to forget and start over for.
    assert!(!startup.blamed_a_backend());
    let none = eframe::Error::Wgpu(eframe::egui_wgpu::WgpuError::CustomNativeAdapterSelectionError("no graphics adapter can show the window".into()));
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
