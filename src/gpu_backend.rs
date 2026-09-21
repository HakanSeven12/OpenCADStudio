//! Cross-platform GPU-backend selection with automatic fallback for older GPUs.
//!
//! Background: older GPUs without usable DirectX 12 Feature Level 12_0 or
//! working Vulkan support (e.g. legacy Intel iGPUs and other old discrete or
//! integrated parts), and old GLES drivers, can crash the app at startup or
//! on the first drawing viewport:
//!   - DX12 path: iced's wgpu `StagingBelt` hits an invalid staging buffer
//!     and panics in `Buffer::get_mapped_range` (`panic = "abort"` in
//!     release, so the process dies silently).
//!   - GLES path: shader compilation failure (e.g. `no default precision`
//!     inside iced's own gradient pipeline) on old Mesa.
//! Selecting a working backend via `WGPU_BACKEND` before iced boots avoids
//! both classes.
//!
//! How it works:
//!   - an explicit `--backend` / `--safe-mode` / pre-set `WGPU_BACKEND` always wins;
//!   - otherwise each candidate backend (Windows: dx12, vulkan, gl; Linux:
//!     vulkan, gl) is exercised in a short-lived `--gpu-probe <backend>`
//!     child process: device request, staging-style mapped upload, render
//!     pipeline creation (catches shader-compile failures), offscreen render
//!     and readback. The first fully-passing backend wins;
//!   - backends implicated in the previous run's crash (crash sentinel) are
//!     skipped;
//!   - if no hardware backend passes, one software-adapter probe (`--gpu-probe
//!     sw`, e.g. WARP / llvmpipe / SwiftShader) is tried: slow but alive;
//!   - the winning probe also reports adapter limits, so the packed
//!     compatibility renderer is enabled automatically on GPUs without
//!     shader storage buffers.
//!
//! Probes run out-of-process because the failure modes are `panic = "abort"`
//! or driver hangs: an in-process probe would die or stall with the app
//! instead of reporting failure.

use std::path::PathBuf;
use std::time::Duration;

/// Hidden child-process flag: `--gpu-probe <dx12|vulkan|gl|sw>`.
pub const PROBE_ARG: &str = "--gpu-probe";
/// Per-backend probe timeout: broken drivers can hang enumeration, so never
/// block boot long on a single backend.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// Test hook: every probe reports failure without spawning a child.
pub const FORCE_FAIL_ENV: &str = "OCS_GPU_PROBE_FORCE_FAIL";
/// Test hook: the first candidate probe reports success without spawning.
pub const FORCE_OK_ENV: &str = "OCS_GPU_PROBE_FORCE_OK";
/// Test hook: `max_storage_buffers_per_shader_stage` of the synthetic success
/// (drives compat-renderer auto-enable in tests).
pub const FORCE_CAPS_ENV: &str = "OCS_GPU_PROBE_FORCE_CAPS";
/// Test hook: point the sentinel at a temp dir instead of the user config dir.
pub const SENTINEL_DIR_ENV: &str = "OCS_GPU_SENTINEL_DIR";

/// Wire shader path needs one storage buffer; batched hatch needs four (see
/// `DeviceCapabilities`). Below one storage buffer the packed compatibility
/// renderer is required, so the probe enables it automatically.
pub const WIRE_STORAGE_NEEDED: u32 = 1;

/// Candidate backends in preference order. Windows keeps DX12/Vulkan first so
/// the AMD OpenGL ICD is never touched unless both fail; Linux prefers Vulkan
/// because old Mesa GLES is a known crash source. macOS stays on its Metal
/// default (guard inactive there).
pub fn candidate_backends() -> Vec<String> {
    if cfg!(target_os = "windows") {
        vec!["dx12".to_string(), "vulkan".to_string(), "gl".to_string()]
    } else if cfg!(target_os = "linux") {
        vec!["vulkan".to_string(), "gl".to_string()]
    } else {
        Vec::new()
    }
}

/// Whether the resolver is active on this OS (Windows + Linux; macOS keeps
/// iced's Metal default, wasm has no native GPU choice to make).
pub fn gpu_guard_active() -> bool {
    !candidate_backends().is_empty()
}

/// Capabilities reported by a passing probe child (one JSON line on stdout).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeCaps {
    /// Backend that passed (`dx12`, `vulkan` or `gl`).
    pub backend: String,
    /// Human-readable adapter name for logs and notices.
    pub adapter: String,
    /// `max_storage_buffers_per_shader_stage` of the adapter.
    pub max_storage: u32,
    /// True when this is a software rasterizer (WARP / llvmpipe / SwiftShader).
    pub software: bool,
}

impl ProbeCaps {
    fn synthetic(backend: &str) -> Self {
        let max_storage = std::env::var(FORCE_CAPS_ENV)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8);
        Self {
            backend: backend.to_string(),
            adapter: "synthetic-test-adapter".to_string(),
            max_storage,
            software: false,
        }
    }

    /// Parse the JSON line a probe child prints on success.
    fn from_json_line(line: &str) -> Option<Self> {
        let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
        Some(Self {
            backend: v.get("backend")?.as_str()?.to_string(),
            adapter: v.get("adapter")?.as_str().unwrap_or("unknown").to_string(),
            max_storage: v.get("max_storage")?.as_u64()? as u32,
            software: v.get("software")?.as_bool()?,
        })
    }

    fn to_json_line(&self) -> String {
        serde_json::json!({
            "backend": self.backend,
            "adapter": self.adapter,
            "max_storage": self.max_storage,
            "software": self.software,
        })
        .to_string()
    }
}

/// Whether the packed compatibility renderer is needed for these caps.
/// Software rasterizers always take it: the packed path works everywhere and
/// these GPUs are slow enough already.
pub fn needs_compat_renderer(caps: &ProbeCaps) -> bool {
    caps.software || caps.max_storage < WIRE_STORAGE_NEEDED
}

/// What the resolver decided, and why (for the startup notice).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendDecision {
    /// Value to set `WGPU_BACKEND` to, or `None` to leave the env alone.
    pub backend_value: Option<String>,
    /// Human-readable reason when this is not the silent fast path.
    pub reason: Option<String>,
    /// Probe says the packed compatibility renderer is needed.
    pub compat_renderer: bool,
    /// Adapter name of the winning probe, for logs.
    pub adapter_name: Option<String>,
}

/// Pure decision function — all IO (env reads, probes, sentinel) happens in
/// the caller so this is unit-testable.
/// * `candidates`: platform-ordered backends to consider.
/// * `excluded`: backends implicated in the previous run's crash.
/// * `results`: probe outcome per tried backend, in any order.
/// * `software`: software-adapter probe outcome.
#[allow(clippy::too_many_arguments)]
pub fn decide_backend(
    explicit_cli: Option<&str>,
    env_backend: Option<&str>,
    safe_mode: bool,
    candidates: &[String],
    excluded: &[String],
    results: &[(String, Option<ProbeCaps>)],
    software: Option<ProbeCaps>,
    guard_active: bool,
) -> BackendDecision {
    if let Some(b) = explicit_cli {
        return BackendDecision {
            backend_value: Some(b.to_string()),
            reason: None,
            compat_renderer: false,
            adapter_name: None,
        };
    }
    if let Some(b) = env_backend {
        return BackendDecision {
            backend_value: Some(b.to_string()),
            reason: None,
            compat_renderer: false,
            adapter_name: None,
        };
    }
    if safe_mode {
        return BackendDecision {
            backend_value: Some("gl".to_string()),
            reason: Some("safe mode requested (--safe-mode)".to_string()),
            compat_renderer: false,
            adapter_name: None,
        };
    }
    if !guard_active {
        return BackendDecision {
            backend_value: None,
            reason: None,
            compat_renderer: false,
            adapter_name: None,
        };
    }
    let failed: Vec<&String> = results
        .iter()
        .filter(|(_, r)| r.is_none())
        .map(|(b, _)| b)
        .collect();
    // First passing backend in preference order, skipping backends that died
    // with the previous run.
    for backend in candidates {
        if excluded.iter().any(|e| e == backend) {
            continue;
        }
        if let Some((_, Some(caps))) = results.iter().find(|(b, _)| b == backend) {
            let fell_back = backend != &candidates[0] || !excluded.is_empty();
            let reason = if fell_back {
                Some(failover_reason(&failed, excluded, backend))
            } else {
                None
            };
            return BackendDecision {
                backend_value: Some(backend.clone()),
                reason,
                compat_renderer: needs_compat_renderer(caps),
                adapter_name: Some(caps.adapter.clone()),
            };
        }
    }
    if let Some(caps) = software {
        return BackendDecision {
            backend_value: Some(caps.backend.clone()),
            reason: Some(
                "no hardware-accelerated backend survived probing, \
                 so this launch uses software rendering (slow, but stable)"
                    .to_string(),
            ),
            compat_renderer: true,
            adapter_name: Some(caps.adapter.clone()),
        };
    }
    // Best effort: GL is the most widely available backend on old hardware.
    BackendDecision {
        backend_value: Some("gl".to_string()),
        reason: Some(
            "every graphics probe failed, so this launch tries the GL \
             compatibility backend as a last resort"
                .to_string(),
        ),
        compat_renderer: true,
        adapter_name: None,
    }
}

fn failover_reason(failed: &[&String], excluded: &[String], chosen: &str) -> String {
    let mut parts: Vec<String> = failed.iter().map(|b| b.to_string()).collect();
    parts.extend(excluded.iter().cloned());
    parts.sort();
    parts.dedup();
    let what_failed = if parts.is_empty() {
        "the preferred backend was skipped after the previous crash".to_string()
    } else {
        format!("{} {}", parts.join(", "), "is not usable on this GPU")
    };
    let chosen_text = match chosen {
        "gl" => "the GL compatibility backend",
        "vulkan" => "Vulkan",
        "dx12" => "DirectX 12",
        other => other,
    };
    format!("{what_failed}, so this launch uses {chosen_text}")
}

/// Notice text shown in the command line when the backend was switched.
pub fn fallback_notice(reason: &str) -> String {
    format!(
        "Graphics compatibility mode: {reason}. \
         If drawings render slowly, update the GPU driver; \
         override with --backend <dx12|vulkan|gl>."
    )
}

/// Notice text when the packed compatibility renderer was auto-enabled.
pub fn compat_notice() -> String {
    "Older GPU detected: using the packed compatibility renderer \
     (no shader storage buffers). Override with --compat-renderer / --backend."
        .to_string()
}

/// Sentinel file: armed before `app::run`, disarmed on clean exit. A stale
/// file at startup means the last run died while the GPU was active; its
/// content names the attempted backend(s) so they are skipped next launch.
pub fn sentinel_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var(SENTINEL_DIR_ENV) {
        return Some(PathBuf::from(dir).join("gpu_crash_sentinel"));
    }
    crate::config::config_dir().map(|d| d.join("gpu_crash_sentinel"))
}

pub fn previous_run_crashed() -> bool {
    sentinel_path().is_some_and(|p| p.exists())
}

/// Backends implicated in the previous run's crash, from the stale sentinel.
pub fn crashed_backends() -> Vec<String> {
    let Some(p) = sentinel_path() else {
        return Vec::new();
    };
    std::fs::read_to_string(p)
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix("backend="))
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn arm_sentinel(backend: &str) {
    if let Some(p) = sentinel_path() {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(
            &p,
            format!(
                "backend={backend}\npid={}\nt={}\n",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            ),
        );
    }
}

pub fn disarm_sentinel() {
    if let Some(p) = sentinel_path() {
        let _ = std::fs::remove_file(&p);
    }
}

/// Outcome of the whole resolution: what to set, what to tell the user, and
/// whether the packed renderer was auto-enabled.
pub struct GpuResolution {
    /// Value written to `WGPU_BACKEND` (`None` = env untouched).
    pub backend_value: Option<String>,
    /// Fallback reason for the startup notice, if any.
    pub reason: Option<String>,
    /// Probe says the packed compatibility renderer is needed.
    pub compat_renderer: bool,
}

/// Spawn one `--gpu-probe <arg>` child and return its reported capabilities
/// on success. `None` = crashed / failed / timed out.
fn run_probe_for(arg: &str) -> Option<ProbeCaps> {
    if std::env::var_os(FORCE_FAIL_ENV).is_some() {
        return None;
    }
    if std::env::var_os(FORCE_OK_ENV).is_some() {
        return Some(ProbeCaps::synthetic(arg));
    }
    let exe = std::env::current_exe().ok()?;
    let mut child = std::process::Command::new(exe)
        .arg(PROBE_ARG)
        .arg(arg)
        .env("RUST_LOG", "error")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let out = child.wait_with_output().ok()?;
                let line = String::from_utf8_lossy(&out.stdout);
                return line.lines().rev().find_map(ProbeCaps::from_json_line);
            }
            Ok(None) => {
                if start.elapsed() > PROBE_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    eprintln!("[gpu] {arg} probe timed out; trying the next backend");
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return None,
        }
    }
}

/// Resolve and apply `WGPU_BACKEND` for this launch.
///
/// Precedence: `--backend` > existing `WGPU_BACKEND` > `--safe-mode` >
/// per-backend probes (minus crash-implicated backends) > software adapter >
/// last-resort `gl`. Inactive on macOS/wasm (env untouched).
pub fn resolve_gpu(cli_backend: Option<&str>, safe_mode: bool) -> GpuResolution {
    let env_backend = std::env::var("WGPU_BACKEND").ok();
    if cli_backend.is_some() || env_backend.is_some() || safe_mode || !gpu_guard_active() {
        let d = decide_backend(
            cli_backend,
            env_backend.as_deref(),
            safe_mode,
            &[],
            &[],
            &[],
            None,
            gpu_guard_active(),
        );
        if let Some(v) = &d.backend_value {
            std::env::set_var("WGPU_BACKEND", v);
        }
        return GpuResolution {
            backend_value: d.backend_value,
            reason: d.reason,
            compat_renderer: false,
        };
    }
    let candidates = candidate_backends();
    let excluded = if previous_run_crashed() {
        crashed_backends()
    } else {
        Vec::new()
    };
    let mut results: Vec<(String, Option<ProbeCaps>)> = Vec::new();
    for backend in &candidates {
        if excluded.iter().any(|e| e == backend) {
            continue;
        }
        let caps = run_probe_for(backend);
        eprintln!(
            "[gpu] probe {backend}: {}",
            caps.as_ref()
                .map(|c| c.adapter.as_str())
                .unwrap_or("FAILED")
        );
        results.push((backend.clone(), caps));
        if results.iter().any(|(_, c)| c.is_some()) {
            break;
        }
    }
    let sw = if results.iter().all(|(_, c)| c.is_none()) {
        run_probe_for("sw")
    } else {
        None
    };
    let d = decide_backend(
        None,
        None,
        false,
        &candidates,
        &excluded,
        &results,
        sw,
        true,
    );
    if let Some(v) = &d.backend_value {
        std::env::set_var("WGPU_BACKEND", v);
        if let Some(adapter) = &d.adapter_name {
            eprintln!("[gpu] backend: {v} ({adapter})");
        }
        if let Some(reason) = &d.reason {
            eprintln!("[gpu] {}", fallback_notice(reason));
        }
        if d.compat_renderer {
            eprintln!("[gpu] {}", compat_notice());
        }
    }
    GpuResolution {
        backend_value: d.backend_value,
        reason: d.reason,
        compat_renderer: d.compat_renderer,
    }
}

/// Child-process entry point for `--gpu-probe <backend|sw>`: exercise one
/// backend the way the canvas does (device + staging-style mapped upload +
/// render pipeline creation + offscreen render + readback). Prints one JSON
/// line on success and exits 0; any failure (including a driver abort panics)
/// exits non-zero with no output.
pub fn run_probe_child(which: &str) -> ! {
    let code = match which {
        "dx12" | "vulkan" | "gl" => probe_single_backend(which).map_or(1, |caps| {
            println!("{}", caps.to_json_line());
            0
        }),
        "sw" => probe_software_adapters().map_or(1, |caps| {
            println!("{}", caps.to_json_line());
            0
        }),
        _ => {
            eprintln!("[gpu-probe] unknown backend '{which}' (want dx12|vulkan|gl|sw)");
            2
        }
    };
    std::process::exit(code);
}

fn wgpu_backends_for(name: &str) -> iced::wgpu::Backends {
    use iced::wgpu::Backends;
    match name {
        "dx12" => Backends::DX12,
        "vulkan" => Backends::VULKAN,
        "gl" => Backends::GL,
        _ => Backends::empty(),
    }
}

fn backend_name(backend: iced::wgpu::Backend) -> &'static str {
    use iced::wgpu::Backend;
    match backend {
        Backend::Dx12 => "dx12",
        Backend::Vulkan => "vulkan",
        Backend::Gl => "gl",
        Backend::Metal => "metal",
        _ => "other",
    }
}

/// Adapter names that mean "no GPU" (same family as the app's software
/// detection): these never count as a passing *hardware* probe.
const SOFTWARE_RASTERIZER_NAMES: [&str; 6] = [
    "llvmpipe",
    "lavapipe",
    "softpipe",
    "swiftshader",
    "warp",
    "microsoft basic render",
];

fn is_software_adapter(info: &iced::wgpu::AdapterInfo) -> bool {
    if info.device_type == iced::wgpu::DeviceType::Cpu {
        return true;
    }
    let name = info.name.to_ascii_lowercase();
    SOFTWARE_RASTERIZER_NAMES
        .iter()
        .any(|marker| name.contains(marker))
}

fn probe_single_backend(name: &str) -> Option<ProbeCaps> {
    use iced::wgpu;
    let backends = wgpu_backends_for(name);
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = backends;
    let instance = wgpu::Instance::new(desc);
    let adapters: Vec<wgpu::Adapter> = pollster::block_on(instance.enumerate_adapters(backends));
    // Hardware only: a software rasterizer proving "usable" would hide a
    // broken hardware path (software gets its own dedicated `sw` probe).
    let mut candidates: Vec<_> = adapters
        .into_iter()
        .filter(|a| !is_software_adapter(&a.get_info()))
        .collect();
    if candidates.is_empty() {
        eprintln!("[gpu-probe] {name}: no hardware adapters enumerated");
        return None;
    }
    candidates.sort_by_key(|a| match a.get_info().device_type {
        wgpu::DeviceType::DiscreteGpu => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        wgpu::DeviceType::Other => 2,
        _ => 3,
    });
    for adapter in &candidates {
        if let Some(caps) = exercise_adapter(adapter) {
            return Some(caps);
        }
    }
    eprintln!("[gpu-probe] {name}: no adapter survived the exercise");
    None
}

fn probe_software_adapters() -> Option<ProbeCaps> {
    use iced::wgpu;
    let backends = wgpu::Backends::DX12 | wgpu::Backends::VULKAN | wgpu::Backends::GL;
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = backends;
    let instance = wgpu::Instance::new(desc);
    let adapters: Vec<wgpu::Adapter> = pollster::block_on(instance.enumerate_adapters(backends));
    let mut software: Vec<_> = adapters
        .into_iter()
        .filter(|a| is_software_adapter(&a.get_info()))
        .collect();
    if software.is_empty() {
        // No software adapter enumerated: ask wgpu for its fallback adapter.
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::None,
            force_fallback_adapter: true,
            compatible_surface: None,
        }))
        .ok()?;
        software.push(adapter);
    }
    for adapter in &software {
        if let Some(caps) = exercise_adapter(adapter) {
            return Some(caps);
        }
    }
    eprintln!("[gpu-probe] sw: no software adapter survived the exercise");
    None
}

/// Full exercise for one adapter: device request (iced-style limits), staging
/// upload, render pipeline creation, offscreen render, readback. Any driver
/// abort or validation failure fails the probe.
fn exercise_adapter(adapter: &iced::wgpu::Adapter) -> Option<ProbeCaps> {
    use iced::wgpu;
    let info = adapter.get_info();
    eprintln!(
        "[gpu-probe] trying {} ({:?}, {:?})",
        info.name, info.backend, info.device_type
    );
    // Mirror iced's compositor: full limits first, then the downlevel set.
    let limits = [
        wgpu::Limits::default().using_resolution(adapter.limits()),
        wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
    ];
    for required_limits in limits {
        let Ok((device, queue)) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("ocs-gpu-probe"),
                required_features: wgpu::Features::empty(),
                required_limits: required_limits.clone(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
            }))
        else {
            continue;
        };
        let errors = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = errors.clone();
        device.on_uncaptured_error(std::sync::Arc::new(move |e: wgpu::Error| {
            eprintln!("[gpu-probe] uncaptured error: {e}");
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        }));
        if !staging_upload(&device)
            || !pipeline_render_readback(&device, &queue)
            || errors.load(std::sync::atomic::Ordering::SeqCst)
        {
            continue;
        }
        eprintln!("[gpu-probe] OK via {}", info.name);
        return Some(ProbeCaps {
            backend: backend_name(info.backend).to_string(),
            adapter: info.name.clone(),
            max_storage: adapter.limits().max_storage_buffers_per_shader_stage,
            software: is_software_adapter(&info),
        });
    }
    None
}

/// Mapped-memory upload through the queue (never `mapped_at_creation`): the
/// same host-mapping path `StagingBelt` and glyph uploads take.
fn staging_upload(device: &iced::wgpu::Device) -> bool {
    use iced::wgpu;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ocs-gpu-probe-staging"),
        size: 4096,
        usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let slice = staging.slice(..);
    let mapped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = mapped.clone();
    slice.map_async(wgpu::MapMode::Write, move |r| {
        flag.store(r.is_ok(), std::sync::atomic::Ordering::SeqCst);
    });
    device.poll(wgpu::PollType::wait_indefinitely()).ok();
    if !mapped.load(std::sync::atomic::Ordering::SeqCst) {
        return false;
    }
    {
        let mut view = slice.get_mapped_range_mut();
        view.slice(0..1).copy_from_slice(&[0xABu8]);
    }
    staging.unmap();
    true
}

/// Render pipeline creation + offscreen render + readback.
///
/// The fragment shader deliberately reads a `private array<vec4<f32>, 8>`,
/// the same pattern iced's gradient pipeline compiles: old GLES drivers
/// (Mesa on legacy Intel iGPUs) reject naga's `vec4[8]` GLSL with `no
/// default precision`, so pipeline creation fails there — exactly what this
/// probe must catch before the editor boots.
fn pipeline_render_readback(device: &iced::wgpu::Device, queue: &iced::wgpu::Queue) -> bool {
    use iced::wgpu;
    const SHADER: &str = r#"
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) colour: vec4<f32>,
};
var<private> palette: array<vec4<f32>, 8>;
@vertex
fn vs(@builtin(vertex_index) idx: u32) -> VsOut {
    var out: VsOut;
    let corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    out.pos = vec4<f32>(corners[idx], 0.0, 1.0);
    out.colour = palette[idx % 8u];
    return out;
}
@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return in.colour + palette[0];
}
"#;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("ocs-gpu-probe-shader"),
        source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(SHADER)),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("ocs-gpu-probe-pipeline"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs"),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Bgra8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        multiview_mask: None,
        cache: None,
    });
    // 64px offscreen render. 64*4 = 256 bytes per row satisfies
    // COPY_BYTES_PER_ROW_ALIGNMENT.
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ocs-gpu-probe-target"),
        size: wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("ocs-gpu-probe-encoder"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ocs-gpu-probe-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        pass.draw(0..3, 0..1);
    }
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ocs-gpu-probe-readback"),
        size: 256 * 64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(64 * 4),
                rows_per_image: Some(64),
            },
        },
        wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let slice = readback.slice(..);
    let ok = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = ok.clone();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        flag.store(r.is_ok(), std::sync::atomic::Ordering::SeqCst);
    });
    device.poll(wgpu::PollType::wait_indefinitely()).ok();
    if !ok.load(std::sync::atomic::Ordering::SeqCst) {
        return false;
    }
    let _ = slice.get_mapped_range();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cands(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn ok_caps(backend: &str, storage: u32) -> ProbeCaps {
        ProbeCaps {
            backend: backend.to_string(),
            adapter: format!("test {backend}"),
            max_storage: storage,
            software: false,
        }
    }

    #[test]
    fn explicit_backend_and_env_beat_everything() {
        let d = decide_backend(
            Some("vulkan"),
            None,
            true,
            &cands(&["dx12", "vulkan", "gl"]),
            &cands(&["dx12"]),
            &[],
            None,
            true,
        );
        assert_eq!(d.backend_value.as_deref(), Some("vulkan"));
        assert!(d.reason.is_none());
        let d = decide_backend(None, Some("gl"), true, &[], &[], &[], None, true);
        assert_eq!(d.backend_value.as_deref(), Some("gl"));
        assert!(d.reason.is_none());
    }

    #[test]
    fn safe_mode_forces_gl() {
        let d = decide_backend(
            None,
            None,
            true,
            &cands(&["vulkan", "gl"]),
            &[],
            &[],
            None,
            true,
        );
        assert_eq!(d.backend_value.as_deref(), Some("gl"));
        assert!(d.reason.is_some());
    }

    #[test]
    fn first_passing_backend_wins_silently() {
        let results = vec![("vulkan".to_string(), Some(ok_caps("vulkan", 8)))];
        let d = decide_backend(
            None,
            None,
            false,
            &cands(&["vulkan", "gl"]),
            &[],
            &results,
            None,
            true,
        );
        assert_eq!(d.backend_value.as_deref(), Some("vulkan"));
        assert!(d.reason.is_none());
        assert!(!d.compat_renderer);
        assert_eq!(d.adapter_name.as_deref(), Some("test vulkan"));
    }

    #[test]
    fn failover_picks_next_backend_with_reason() {
        let results = vec![
            ("dx12".to_string(), None),
            ("vulkan".to_string(), Some(ok_caps("vulkan", 8))),
        ];
        let d = decide_backend(
            None,
            None,
            false,
            &cands(&["dx12", "vulkan", "gl"]),
            &[],
            &results,
            None,
            true,
        );
        assert_eq!(d.backend_value.as_deref(), Some("vulkan"));
        let reason = d.reason.unwrap();
        assert!(reason.contains("dx12"), "{reason}");
        let notice = fallback_notice(&reason);
        assert!(notice.contains("--backend"), "{notice}");
    }

    #[test]
    fn crash_implicated_backends_are_skipped() {
        // dx12 died last run: even a passing dx12 probe must not win.
        let results = vec![
            ("dx12".to_string(), Some(ok_caps("dx12", 8))),
            ("gl".to_string(), Some(ok_caps("gl", 0))),
        ];
        let d = decide_backend(
            None,
            None,
            false,
            &cands(&["dx12", "vulkan", "gl"]),
            &cands(&["dx12"]),
            &results,
            None,
            true,
        );
        assert_eq!(d.backend_value.as_deref(), Some("gl"));
        assert!(d.reason.is_some());
        // No storage buffers on the GL probe: packed renderer auto-enabled.
        assert!(d.compat_renderer);
    }

    #[test]
    fn software_adapter_is_last_resort_before_gl() {
        let results = vec![("vulkan".to_string(), None), ("gl".to_string(), None)];
        let sw = ProbeCaps {
            backend: "vulkan".to_string(),
            adapter: "llvmpipe".to_string(),
            max_storage: 8,
            software: true,
        };
        let d = decide_backend(
            None,
            None,
            false,
            &cands(&["vulkan", "gl"]),
            &[],
            &results,
            Some(sw),
            true,
        );
        assert_eq!(d.backend_value.as_deref(), Some("vulkan"));
        assert!(d.compat_renderer);
        assert!(d.reason.unwrap().contains("software"),);
    }

    #[test]
    fn total_failure_falls_back_to_gl_with_compat() {
        let d = decide_backend(
            None,
            None,
            false,
            &cands(&["vulkan", "gl"]),
            &[],
            &[("vulkan".to_string(), None), ("gl".to_string(), None)],
            None,
            true,
        );
        assert_eq!(d.backend_value.as_deref(), Some("gl"));
        assert!(d.compat_renderer);
        assert!(d.reason.is_some());
    }

    #[test]
    fn inactive_guard_leaves_the_env_alone() {
        let results = vec![("vulkan".to_string(), None)];
        let d = decide_backend(None, None, false, &[], &[], &results, None, false);
        assert!(d.backend_value.is_none());
    }

    #[test]
    fn probe_caps_json_round_trip() {
        let caps = ProbeCaps {
            backend: "vulkan".to_string(),
            adapter: "Intel HD 405 (Braswell)".to_string(),
            max_storage: 8,
            software: false,
        };
        let line = caps.to_json_line();
        assert_eq!(ProbeCaps::from_json_line(&line), Some(caps));
        assert!(ProbeCaps::from_json_line("not json").is_none());
        assert!(ProbeCaps::from_json_line("{\"backend\":\"gl\"}").is_none());
    }

    #[test]
    fn sentinel_round_trip_records_backends() {
        let dir = std::env::temp_dir().join(format!("ocs-gpu-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        // SAFETY: single-threaded test mutating a process-local test hook.
        unsafe { std::env::set_var(SENTINEL_DIR_ENV, &dir) };
        assert!(!previous_run_crashed());
        arm_sentinel("dx12");
        assert!(previous_run_crashed());
        assert_eq!(crashed_backends(), vec!["dx12".to_string()]);
        disarm_sentinel();
        assert!(!previous_run_crashed());
        assert!(crashed_backends().is_empty());
        unsafe { std::env::remove_var(SENTINEL_DIR_ENV) };
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn force_hooks_drive_probes_without_spawning() {
        // SAFETY: single-threaded test mutating process-local test hooks.
        unsafe { std::env::set_var(FORCE_FAIL_ENV, "1") };
        assert!(run_probe_for("vulkan").is_none());
        unsafe { std::env::remove_var(FORCE_FAIL_ENV) };
        unsafe { std::env::set_var(FORCE_OK_ENV, "1") };
        let caps = run_probe_for("vulkan").expect("forced success");
        assert_eq!(caps.backend, "vulkan");
        assert!(!needs_compat_renderer(&caps));
        unsafe { std::env::set_var(FORCE_CAPS_ENV, "0") };
        let bare = run_probe_for("gl").expect("forced success");
        assert!(needs_compat_renderer(&bare));
        unsafe { std::env::remove_var(FORCE_OK_ENV) };
        unsafe { std::env::remove_var(FORCE_CAPS_ENV) };
    }
}
