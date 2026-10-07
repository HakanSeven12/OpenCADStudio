//! Host hardware facts for About and the copied system report.
//!
//! Read straight from the OS (registry, `/proc`, `sysctl`) so no extra crate
//! is pulled in for three strings. Everything degrades to "Unknown" rather
//! than failing: this is diagnostics, never a reason to refuse to open.

use std::sync::OnceLock;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SystemInfo {
    /// Marketing name, e.g. `Intel(R) Core(TM) Ultra 7 255U`.
    pub cpu: String,
    /// Logical processors available to this process.
    pub logical_cores: usize,
    /// Installed physical memory, bytes (0 = unknown).
    pub ram_total: u64,
    /// Memory available right now, bytes (0 = unknown).
    pub ram_available: u64,
    /// OS name / version / build.
    pub os: String,
}

static INFO: OnceLock<SystemInfo> = OnceLock::new();

/// Host facts, read once per run.
pub fn system_info() -> &'static SystemInfo {
    INFO.get_or_init(read)
}

/// `16.0 GB`, or `Unknown` for 0.
pub fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "Unknown".to_string();
    }
    format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

/// One-line CPU description for the About row.
pub fn cpu_summary() -> String {
    let i = system_info();
    if i.logical_cores > 0 {
        format!("{} ({} threads)", i.cpu, i.logical_cores)
    } else {
        i.cpu.clone()
    }
}

/// One-line RAM description for the About row.
pub fn ram_summary() -> String {
    format_bytes(system_info().ram_total)
}

#[cfg(not(target_arch = "wasm32"))]
fn unknown_if_empty(s: String) -> String {
    let s = s.trim().to_string();
    if s.is_empty() {
        "Unknown".to_string()
    } else {
        s
    }
}

#[cfg(target_arch = "wasm32")]
fn read() -> SystemInfo {
    SystemInfo {
        cpu: "Unknown".into(),
        logical_cores: 0,
        ram_total: 0,
        ram_available: 0,
        os: "Web".into(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read() -> SystemInfo {
    let (cpu, ram_total, ram_available, os) = platform();
    SystemInfo {
        cpu: unknown_if_empty(cpu),
        logical_cores: std::thread::available_parallelism().map_or(0, |n| n.get()),
        ram_total,
        ram_available,
        os: unknown_if_empty(os),
    }
}

#[cfg(target_os = "windows")]
fn platform() -> (String, u64, u64, String) {
    use windows_sys::Win32::System::Registry::{
        RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ,
    };
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    fn reg_string(subkey: &str, value: &str) -> String {
        let (subkey, value) = (wide(subkey), wide(value));
        let mut buf = [0u16; 512];
        let mut bytes = (buf.len() * 2) as u32;
        // SAFETY: both names are NUL-terminated and `buf` / `bytes` describe
        // the writable buffer exactly.
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                subkey.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        if status != 0 {
            return String::new();
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len])
    }

    let cpu = reg_string(
        r"HARDWARE\DESCRIPTION\System\CentralProcessor\0",
        "ProcessorNameString",
    );

    // SAFETY: `MEMORYSTATUSEX` is plain data; `dwLength` is set as required.
    let (total, avail) = unsafe {
        let mut m: MEMORYSTATUSEX = std::mem::zeroed();
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut m) != 0 {
            (m.ullTotalPhys, m.ullAvailPhys)
        } else {
            (0, 0)
        }
    };

    let key = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let mut name = reg_string(key, "ProductName");
    let build = reg_string(key, "CurrentBuildNumber");
    let display = reg_string(key, "DisplayVersion");
    // The registry still says "Windows 10" on Windows 11; the build number
    // (22000+) is the reliable tell.
    if build.parse::<u32>().is_ok_and(|b| b >= 22000) {
        name = name.replace("Windows 10", "Windows 11");
    }
    let mut os = name;
    if !display.is_empty() {
        os.push(' ');
        os.push_str(&display);
    }
    if !build.is_empty() {
        os.push_str(&format!(" (build {build})"));
    }
    (cpu, total, avail, os)
}

#[cfg(target_os = "linux")]
fn platform() -> (String, u64, u64, String) {
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let cpu = cpuinfo
        .lines()
        .find_map(|l| l.strip_prefix("model name"))
        .and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.trim().to_string())
        .unwrap_or_default();
    let meminfo = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let kb = |key: &str| -> u64 {
        meminfo
            .lines()
            .find_map(|l| l.strip_prefix(key))
            .and_then(|l| l.trim_start_matches(':').split_whitespace().next())
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(0)
            * 1024
    };
    let os_release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    let os = os_release
        .lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))
        .map(|v| v.trim_matches('"').to_string())
        .unwrap_or_default();
    (cpu, kb("MemTotal"), kb("MemAvailable"), os)
}

#[cfg(target_os = "macos")]
fn platform() -> (String, u64, u64, String) {
    fn run(cmd: &str, args: &[&str]) -> String {
        std::process::Command::new(cmd)
            .args(args)
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    }
    let cpu = run("sysctl", &["-n", "machdep.cpu.brand_string"]);
    let total = run("sysctl", &["-n", "hw.memsize"]).parse().unwrap_or(0);
    let os = format!("macOS {}", run("sw_vers", &["-productVersion"]));
    (cpu, total, 0, os)
}

#[cfg(all(
    not(target_arch = "wasm32"),
    not(any(target_os = "windows", target_os = "linux", target_os = "macos"))
))]
fn platform() -> (String, u64, u64, String) {
    (String::new(), 0, 0, String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bytes_and_unknown() {
        assert_eq!(format_bytes(0), "Unknown");
        assert_eq!(format_bytes(16 * 1024 * 1024 * 1024), "16.0 GB");
    }

    #[test]
    fn host_facts_are_never_empty() {
        let i = system_info();
        assert!(!i.cpu.is_empty());
        assert!(!i.os.is_empty());
        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
        assert!(i.ram_total > 0, "installed RAM should be readable");
    }
}
