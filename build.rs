// Build metadata and Windows executable resources.

#[cfg(windows)]
use std::path::Path;

/// `YYYYMMDD_HHMMSS` in local time, for `OCS_BUILD_STAMP` — shown in the
/// window title and (via `packaging/build_macos_signed.sh`) the macOS
/// bundle name, so a rebuilt-and-reinstalled dev build is visibly distinct
/// from whatever was running before it, independent of the (per-week, not
/// per-build) `OCS_APP_VERSION`. Shells out rather than adding a date/time
/// crate purely for this cosmetic build-script string.
fn build_stamp() -> String {
    #[cfg(windows)]
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", "Get-Date -Format yyyyMMdd_HHmmss"])
        .output();
    #[cfg(not(windows))]
    let output = std::process::Command::new("date").args(["+%Y%m%d_%H%M%S"]).output();

    output
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn main() {
    let version = std::env::var("CARGO_PKG_VERSION").expect("Cargo package version");
    let parts: Vec<&str> = version.split('.').collect();
    let app_version = if parts.len() == 3 && parts[0].len() == 4
        && parts[0].starts_with("20") && parts[2] == "0"
    {
        format!("{}.{:02}", parts[0], parts[1].parse::<u32>().expect("week number"))
    } else {
        version.clone()
    };
    println!("cargo:rustc-env=OCS_APP_VERSION={app_version}");
    println!("cargo:rerun-if-changed=.git/HEAD");
    if let Ok(head) = std::fs::read_to_string(".git/HEAD") {
        if let Some(reference) = head.trim().strip_prefix("ref: ") {
            println!("cargo:rerun-if-changed=.git/{reference}");
        }
    }
    // `OCS_BUILD_STAMP` must reflect the actual compile time, not just when
    // git state changes, so window-title/bundle-name builds run in
    // succession (the normal edit-rebuild-reinstall loop, no commit in
    // between) are still distinguishable. Once *any* rerun-if directive is
    // emitted, Cargo stops auto-rerunning on ordinary source edits, so a
    // path that can never exist forces a rerun every time regardless of the
    // other (git-state-gated) directives above.
    println!("cargo:rerun-if-changed=__ocs_force_build_rs_rerun__");
    println!("cargo:rustc-env=OCS_BUILD_STAMP={}", build_stamp());
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    let dirty = std::process::Command::new("git")
        .args(["diff-index", "--quiet", "HEAD", "--"])
        .status()
        .ok()
        .is_some_and(|status| !status.success());
    let revision = if dirty {
        format!("{revision}-dirty")
    } else {
        revision
    };
    println!("cargo:rustc-env=OCS_GIT_REV={revision}");
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=OCS_BUILD_PROFILE={profile}");
    if std::env::var("TARGET").ok().as_deref() == Some("x86_64-pc-windows-msvc")
        && profile == "debug"
    {
        println!("cargo:rustc-link-arg-bin=OpenCADStudio=/STACK:16777216");
    }
    let mut features: Vec<String> = std::env::vars()
        .filter_map(|(name, value)| {
            (value == "1")
                .then(|| name.strip_prefix("CARGO_FEATURE_").map(str::to_owned))
                .flatten()
        })
        .collect();
    features.sort();
    println!(
        "cargo:rustc-env=OCS_BUILD_FEATURES={}",
        if features.is_empty() {
            "none".to_string()
        } else {
            features.join(",")
        }
    );

    println!("cargo:rerun-if-env-changed=OCS_PATREON_TOKEN");

    // Release builds generate the icon before compiling.
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=packaging/windows/AppIcon.ico");
        if Path::new("packaging/windows/AppIcon.ico").exists() {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("packaging/windows/AppIcon.ico");
            res.set("ProductVersion", &app_version);
            res.set("FileVersion", &app_version);
            if let Err(e) = res.compile() {
                println!("cargo:warning=failed to embed Windows icon: {e}");
            }
        }
    }
}
