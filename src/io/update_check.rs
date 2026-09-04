// Check for a newer published native release.

#[cfg(not(target_arch = "wasm32"))]
const RELEASES_API: &str =
    "https://api.github.com/repos/HakanSeven12/OpenCADStudio/releases/latest";
pub const RELEASES_PAGE: &str =
    "https://github.com/HakanSeven12/OpenCADStudio/releases/latest";

/// Give release assets time to propagate before offering an update.
#[cfg(not(target_arch = "wasm32"))]
const MIN_RELEASE_AGE_SECS: u64 = 60 * 60;

/// What `check_for_update` reports when a newer release exists.
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    /// `tag_name` with the leading `v` stripped (e.g. `0.3.7`).
    pub version: String,
    /// Release notes / markdown body from the GitHub release. May be empty
    /// when the release was published without notes.
    pub body: String,
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn check_for_update() -> Option<UpdateInfo> {
    std::thread::spawn(fetch_latest_if_outdated)
        .join()
        .ok()
        .flatten()
}

/// On wasm there is no background thread or blocking HTTP client; the web build
/// skips the self-update check (the page is always served fresh).
#[cfg(target_arch = "wasm32")]
#[allow(dead_code)]
pub async fn check_for_update() -> Option<UpdateInfo> {
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_latest_if_outdated() -> Option<UpdateInfo> {
    let agent = crate::network::agent(std::time::Duration::from_secs(5));
    let body = agent
        .get(RELEASES_API)
        .header("User-Agent", concat!("OpenCADStudio/", env!("OCS_APP_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .call()
        .ok()?
        .body_mut()
        .read_to_string()
        .ok()?;
    let metadata: serde_json::Value = serde_json::from_str(&body).ok()?;
    let suffix = if cfg!(target_os = "windows") {
        "-windows-x86_64-portable.exe"
    } else if cfg!(target_os = "macos") {
        "-macos-arm64.dmg"
    } else {
        "-linux-x86_64.AppImage"
    };
    if !metadata.get("assets")?.as_array()?.iter().any(|asset| {
        asset.get("name").and_then(|name| name.as_str()).is_some_and(|name| name.ends_with(suffix))
            && asset.get("size").and_then(|size| size.as_u64()).unwrap_or(0) > 0
    }) {
        return None;
    }
    let latest = metadata.get("tag_name")?.as_str()?
        .trim_start_matches('v')
        .to_string();
    if !is_newer(&latest, env!("OCS_APP_VERSION")) {
        return None;
    }
    // Suppress the notification until the release is old enough for the
    // Actions build to have published binaries.
    if let Some(published) = metadata.get("published_at")
        .and_then(|value| value.as_str())
        .and_then(parse_iso8601_utc)
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();
        if now.saturating_sub(published) < MIN_RELEASE_AGE_SECS {
            return None;
        }
    }
    // Release notes are optional; treat missing as empty.
    let notes = metadata.get("body").and_then(|value| value.as_str()).unwrap_or_default().to_string();
    Some(UpdateInfo { version: latest, body: notes })
}

/// Parse a GitHub timestamp like `2026-05-29T12:34:56Z` into UNIX seconds.
/// Only handles the fixed `YYYY-MM-DDTHH:MM:SSZ` format the GitHub API
/// emits; returns `None` for anything else.
#[cfg(not(target_arch = "wasm32"))]
fn parse_iso8601_utc(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T'
        || b[13] != b':' || b[16] != b':' || b[19] != b'Z'
    {
        return None;
    }
    let year: i32 = s.get(0..4)?.parse().ok()?;
    let month: u32 = s.get(5..7)?.parse().ok()?;
    let day: u32 = s.get(8..10)?.parse().ok()?;
    let hour: u32 = s.get(11..13)?.parse().ok()?;
    let minute: u32 = s.get(14..16)?.parse().ok()?;
    let second: u32 = s.get(17..19)?.parse().ok()?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour >= 24
        || minute >= 60
        || second >= 60
    {
        return None;
    }
    // Howard Hinnant's days_from_civil — converts a proleptic-Gregorian
    // (Y, M, D) to a count of days since 1970-01-01.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = (y - era * 400) as u32;
    let m = month as i32;
    let d = day as i32;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy as u32;
    let days_since_epoch = era as i64 * 146097 + doe as i64 - 719468;
    if days_since_epoch < 0 {
        return None;
    }
    Some(
        days_since_epoch as u64 * 86_400
            + hour as u64 * 3_600
            + minute as u64 * 60
            + second as u64,
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn is_newer(latest: &str, installed: &str) -> bool {
    let parse = |version: &str| {
        let version = version.trim_start_matches('v');
        if let Some((year, week)) = version.split_once('.') {
            if year.len() == 4 && year.starts_with("20") && !week.contains('.') {
                return Some(semver::Version::new(year.parse().ok()?, week.parse().ok()?, 0));
            }
        }
        semver::Version::parse(version).ok()
    };
    matches!((parse(latest), parse(installed)), (Some(latest), Some(installed)) if latest > installed)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn calendar_releases_compare_numerically() {
        assert!(is_newer("2026.35", "0.9.8"));
        assert!(is_newer("2026.10", "2026.09"));
        assert!(is_newer("2027.01", "2026.53"));
        assert!(!is_newer("2026.35", "2026.35"));
        assert!(!is_newer("0.9.8", "2026.35"));
        assert!(!is_newer("invalid", "2026.35"));
    }
}
