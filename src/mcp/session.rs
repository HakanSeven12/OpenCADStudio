use super::cache::{BatchExecution, batch_result, batch_step_id, stale_before_first_step, trim_batches};
use super::tools::{READ_OPS, required_string, resolve_batch_step};
use super::transport::{
    CallError, CancelPump, PumpEvent, exchange, interactive_timeout, iso8601_now, mcp_key,
    random_id, wait_deadline,
};
use super::with_bridge_identity;
use serde::Deserialize;
use serde_json::{Map, Value, json};
#[cfg(any(windows, target_os = "macos"))]
use std::collections::HashSet;
use std::{
    collections::{HashMap, VecDeque},
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime},
};

#[derive(Clone, Deserialize)]
pub(super) struct Descriptor {
    pub(super) session_id: String,
    pub(super) port: u16,
    pub(super) token: String,
    /// GUI process id when the descriptor writer knows it. Used to skip dead
    /// sessions without a (possibly hanging) TCP probe; absent on legacy
    /// files, which keep the old probe path.
    #[serde(default)]
    pub(super) pid: Option<u64>,
}

/// True when `pid` currently exists. Linux checks /proc (no spawn, no new
/// dependencies); other platforms go through one shared snapshot per
/// [`descriptors`] pass (see below) instead of per-pid spawns.
#[cfg(target_os = "linux")]
pub(super) fn pid_alive(pid: u64) -> bool {
    // Fail open where /proc is unavailable (containers, chroots): without it
    // every pid would read "dead" and live sessions would be dropped en masse.
    Path::new("/proc").exists() && Path::new(&format!("/proc/{pid}")).exists()
}

/// One snapshot of all live PIDs. `None` on any failure so callers fail open
/// (treat every descriptor as alive) instead of dropping live sessions
/// because enumeration broke.
#[cfg(windows)]
pub(super) fn live_pids_snapshot() -> Option<HashSet<u64>> {
    use std::os::windows::process::CommandExt;
    // A GUI-subsystem process has no console to lend tasklist; without this
    // flag Windows opens a visible one for it.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut set = HashSet::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut fields = line.split("\",\"");
        fields.next()?;
        if let Some(pid) = fields
            .next()
            .and_then(|field| field.trim_matches('"').parse::<u64>().ok())
        {
            set.insert(pid);
        }
    }
    (!set.is_empty()).then_some(set)
}

/// macOS has no /proc: same one-snapshot approach as Windows via `ps`.
#[cfg(target_os = "macos")]
pub(super) fn live_pids_snapshot() -> Option<HashSet<u64>> {
    let output = Command::new("ps").args(["-Ao", "pid="]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let mut set = HashSet::new();
    for token in String::from_utf8_lossy(&output.stdout).split_whitespace() {
        if let Ok(pid) = token.parse::<u64>() {
            set.insert(pid);
        }
    }
    (!set.is_empty()).then_some(set)
}

/// Process snapshots cost a spawn (~100ms), but `ocs_sessions` can run every
/// agent turn — cache each snapshot briefly. Failures cache as `None`, which
/// keeps the fail-open behavior below.
#[cfg(any(windows, target_os = "macos"))]
pub(super) const PID_SNAPSHOT_TTL: Duration = Duration::from_secs(10);

#[cfg(any(windows, target_os = "macos"))]
std::thread_local! {
    static PID_SNAPSHOT_CACHE: std::cell::RefCell<(Instant, Option<HashSet<u64>>)> =
        std::cell::RefCell::new((Instant::now() - PID_SNAPSHOT_TTL - Duration::from_secs(1), None));
}

#[cfg(any(windows, target_os = "macos"))]
pub(super) fn live_pids_cached() -> Option<HashSet<u64>> {
    PID_SNAPSHOT_CACHE.with(|cache| {
        {
            let (stamp, cached) = &*cache.borrow();
            if stamp.elapsed() < PID_SNAPSHOT_TTL {
                return cached.clone();
            }
        }
        let fresh = live_pids_snapshot();
        *cache.borrow_mut() = (Instant::now(), fresh.clone());
        fresh
    })
}

/// Drop the cached PID snapshot so the next discovery pass enumerates
/// fresh. Called after spawning a GUI: without this, the pre-spawn snapshot
/// does not contain the newborn pid, and the corpse cleanup below would
/// delete its just-written descriptor as "dead on arrival".
#[cfg(any(windows, target_os = "macos"))]
pub(super) fn live_pids_invalidate() {
    PID_SNAPSHOT_CACHE.with(|cache| {
        *cache.borrow_mut() = (
            Instant::now() - PID_SNAPSHOT_TTL - Duration::from_secs(1),
            None,
        );
    });
}

#[cfg(not(any(windows, target_os = "macos")))]
pub(super) fn live_pids_invalidate() {}

/// Descriptors younger than this are never deleted, even when their pid is
/// missing from the snapshot: the snapshot may predate the spawn, and slow
/// starters write late. Deletion stays for genuinely old corpses.
#[cfg(any(windows, target_os = "macos", test))]
pub(super) const DESCRIPTOR_GRACE: Duration = Duration::from_secs(120);

/// Whether a pid missing from the snapshot authorizes deletion. Pure
/// predicate so the staleness rule is unit-testable: only old files go.
#[cfg(any(windows, target_os = "macos", test))]
pub(super) fn stale_snapshot_may_delete(missing_from_snapshot: bool, file_age: Duration) -> bool {
    missing_from_snapshot && file_age >= DESCRIPTOR_GRACE
}

pub(super) struct GuiClient {
    pub(super) descriptor: Descriptor,
    pub(super) state: Value,
    pub(super) client_id: String,
    pub(super) batches: VecDeque<BatchExecution>,
    /// MCP request id (as string) -> GUI request id for ops still pending
    /// server-side. Lets an idle-arriving `notifications/cancelled` find
    /// the GUI op to dismiss. Entries leave on terminal responses and
    /// dismissals; abandoned entries mirror GUI sessions that outlive
    /// interest (pre-existing behavior, two small strings each).
    pub(super) inflight: HashMap<String, String>,
    /// GUI request ids dismissed by cancel/timeout: late completions and
    /// re-polls answer `cancelled` without touching the GUI. Capped.
    pub(super) dismissed: VecDeque<String>,
}

/// (mtime, byte length) of this executable. A rebuild changes at least one of
/// the two, so a mismatch means this bridge serves a stale tool schema.
pub(super) fn exe_fingerprint() -> Option<(SystemTime, u64)> {
    let exe = std::env::current_exe().ok()?;
    let meta = std::fs::metadata(exe).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// True when both fingerprints are known and differ (unknown counts as same
/// to avoid false-positive exits on locked-down filesystems).
pub(super) fn exe_superseded(baseline: &Option<(SystemTime, u64)>) -> bool {
    match (baseline, &exe_fingerprint()) {
        (Some(before), Some(now)) => before != now,
        _ => false,
    }
}

#[cfg(unix)]
extern "C" {
    pub(super) fn geteuid() -> u32;
}

#[cfg(unix)]
pub(super) fn private_descriptor(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(file) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if file.file_type().is_symlink() || !file.file_type().is_file() {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    let Ok(directory) = std::fs::symlink_metadata(parent) else {
        return false;
    };
    if directory.file_type().is_symlink() || !directory.file_type().is_dir() {
        return false;
    }
    // SAFETY: geteuid() takes no arguments or pointers and returns the effective UID.
    let my_uid = unsafe { geteuid() };
    file.uid() == my_uid
        && directory.uid() == my_uid
        && (file.mode() & 0o077) == 0
        && (directory.mode() & 0o077) == 0
}

#[cfg(windows)]
pub(super) fn private_descriptor(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        EqualSid, GetFileSecurityW, GetSecurityDescriptorOwner, GetTokenInformation,
        OWNER_SECURITY_INFORMATION, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let Ok(file_meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if file_meta.file_type().is_symlink() || !file_meta.file_type().is_file() {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    let Ok(dir_meta) = std::fs::symlink_metadata(parent) else {
        return false;
    };
    if dir_meta.file_type().is_symlink() || !dir_meta.file_type().is_dir() {
        return false;
    }

    struct TokenHandle(HANDLE);
    impl Drop for TokenHandle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: self.0 is an open handle from OpenProcessToken.
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }

    let mut raw_token: HANDLE = std::ptr::null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle; raw_token receives the opened token.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) } == 0
        || raw_token.is_null()
    {
        return false;
    }
    let token = TokenHandle(raw_token);

    let mut token_len: u32 = 0;
    // SAFETY: Querying required buffer length with null output buffer.
    unsafe {
        GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut token_len);
    }
    let min_token_len = std::mem::size_of::<TOKEN_USER>() as u32;
    if token_len < min_token_len || token_len > 65_536 {
        return false;
    }
    let mut token_buf = vec![0u64; (token_len as usize).div_ceil(8)];
    // SAFETY: token_buf is 8-byte aligned and has at least token_len bytes.
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            token_buf.as_mut_ptr().cast(),
            token_len,
            &mut token_len,
        )
    } == 0
    {
        return false;
    }
    // SAFETY: GetTokenInformation populated a valid TOKEN_USER header at token_buf start.
    let user_sid: PSID = unsafe { (*token_buf.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    if user_sid.is_null() {
        return false;
    }

    let owner_matches = |target: &Path| -> bool {
        let wide: Vec<u16> = target
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut needed: u32 = 0;
        // SAFETY: Querying required security descriptor buffer size for a null-terminated path.
        unsafe {
            GetFileSecurityW(
                wide.as_ptr(),
                OWNER_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                0,
                &mut needed,
            );
        }
        if needed == 0 || needed > 65_536 {
            return false;
        }
        let mut sd_buf = vec![0u64; (needed as usize).div_ceil(8)];
        // SAFETY: sd_buf is 8-byte aligned and sized to `needed` bytes.
        if unsafe {
            GetFileSecurityW(
                wide.as_ptr(),
                OWNER_SECURITY_INFORMATION,
                sd_buf.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        } == 0
        {
            return false;
        }
        let mut owner: PSID = std::ptr::null_mut();
        let mut defaulted: i32 = 0;
        // SAFETY: sd_buf holds a valid self-relative security descriptor from GetFileSecurityW.
        if unsafe {
            GetSecurityDescriptorOwner(sd_buf.as_mut_ptr().cast(), &mut owner, &mut defaulted)
        } == 0
            || owner.is_null()
        {
            return false;
        }
        // SAFETY: Both owner and user_sid are valid non-null SIDs within live buffers.
        unsafe { EqualSid(owner, user_sid) != 0 }
    };

    owner_matches(path) && owner_matches(parent)
}

#[cfg(not(any(unix, windows)))]
pub(super) fn private_descriptor(_: &Path) -> bool {
    false
}

pub(super) fn descriptors() -> Result<Vec<(Descriptor, Value)>, String> {
    let directory = crate::config::config_dir()
        .ok_or_else(|| "No user configuration directory".to_string())?
        .join("automation");
    let Ok(entries) = directory.read_dir() else {
        return Ok(Vec::new());
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect();
    paths.sort();

    #[cfg(any(windows, target_os = "macos"))]
    let live_pids = live_pids_cached();
    let mut found = Vec::new();
    for path in paths {
        if !private_descriptor(&path) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(descriptor) = serde_json::from_str::<Descriptor>(&text) else {
            continue;
        };
        // A dead GUI leaves its descriptor file behind; its TCP port may hang
        // instead of refusing, which used to stall discovery past client
        // timeouts. Skip (and delete) pid-verified corpses before probing.
        // Deletion waits out DESCRIPTOR_GRACE: the PID snapshot may predate
        // a spawn, and a newborn pid missing from it must never read "dead".
        // Unknown file age counts as newborn (fail open, probe instead).
        let file_age = std::fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .unwrap_or(Duration::ZERO);
        let pid_dead = match descriptor.pid {
            Some(pid) => {
                #[cfg(target_os = "linux")]
                {
                    !pid_alive(pid)
                }
                #[cfg(any(windows, target_os = "macos"))]
                {
                    match &live_pids {
                        None => false,
                        Some(set) => stale_snapshot_may_delete(!set.contains(&pid), file_age),
                    }
                }
                #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
                {
                    let _ = pid;
                    false
                }
            }
            None => false,
        };
        if pid_dead {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        let Ok(state) = exchange(&descriptor, json!({"op":"hello"}), Duration::from_secs(1)) else {
            continue;
        };
        if state["ok"].as_bool() == Some(true)
            && state["session_id"].as_str() == Some(descriptor.session_id.as_str())
        {
            found.push((descriptor, state));
        }
    }
    Ok(found)
}

pub(super) fn log_file() -> Result<File, String> {
    let directory = crate::config::config_dir()
        .ok_or_else(|| "No user configuration directory".to_string())?
        .join("automation");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("gui.log"))
        .map_err(|error| error.to_string())
}

/// A launch already in flight keeps its claim here so concurrent
/// `ocs_sessions(launch_if_none: true)` calls wait for the same GUI instead
/// of spawning one window each. Content is `{"pid":..,"started":unix_secs}`.
pub(super) const STARTUP_LOCK_FILE: &str = "starting.lock";
/// Claims older than this are abandoned (crashed starter, previous boot).
pub(super) const STARTUP_LOCK_TTL_SECS: u64 = 60;

pub(super) fn startup_lock_path_for(directory: &Path) -> PathBuf {
    directory.join(STARTUP_LOCK_FILE)
}

pub(super) fn now_unix_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(super) fn write_startup_lock_at(directory: &Path, pid: u64, started: u64) -> Result<(), String> {
    let text = serde_json::to_string(&json!({"pid": pid, "started": started}))
        .map_err(|error| error.to_string())?;
    // On a profile that never ran the editor nothing has made the folder yet.
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    std::fs::write(startup_lock_path_for(directory), text).map_err(|error| error.to_string())
}

pub(super) fn read_startup_lock_at(directory: &Path) -> Option<(u64, u64)> {
    let text = std::fs::read_to_string(startup_lock_path_for(directory)).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    Some((value.get("pid")?.as_u64()?, value.get("started")?.as_u64()?))
}

pub(super) fn claim_pid_alive(pid: u64) -> bool {
    #[cfg(target_os = "linux")]
    {
        pid_alive(pid)
    }
    #[cfg(any(windows, target_os = "macos"))]
    {
        live_pids_cached().is_none_or(|set| set.contains(&pid))
    }
    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    {
        let _ = pid;
        true
    }
}

/// True when this caller owns the startup claim and may spawn the GUI.
/// False means a live starter holds a fresh claim: wait for its descriptor
/// instead of spawning another window. Stale claims and claims from dead
/// pids are reclaimed. Fail open (claim granted) when the directory is
/// unusable so a broken lock can never wedge launching.
pub(super) fn try_claim_startup_lock(directory: &Path, pid: u64) -> bool {
    let now = now_unix_secs();
    if let Some((owner, started)) = read_startup_lock_at(directory) {
        let fresh = now.saturating_sub(started) < STARTUP_LOCK_TTL_SECS;
        if fresh && claim_pid_alive(owner) {
            return false;
        }
    }
    write_startup_lock_at(directory, pid, now).is_ok()
}

pub(super) fn release_startup_lock(directory: &Path) {
    let _ = std::fs::remove_file(startup_lock_path_for(directory));
}

pub(super) fn start_gui() -> Result<Child, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let log = log_file()?;
    let stderr = log.try_clone().map_err(|error| error.to_string())?;
    Command::new(executable)
        .arg("--new-instance")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| error.to_string())
}

pub(super) fn automation_dir() -> Result<PathBuf, String> {
    let base =
        crate::config::config_dir().ok_or_else(|| "No user configuration directory".to_string())?;
    Ok(base.join("automation"))
}

pub(super) fn sessions(launch_if_none: bool) -> Result<Vec<Value>, String> {
    let mut available = descriptors()?;
    if available.is_empty() && launch_if_none {
        let directory = automation_dir()?;
        // A concurrent caller may already be starting the GUI: wait for its
        // descriptor instead of spawning another window.
        let claimed = try_claim_startup_lock(&directory, std::process::id() as u64);
        let mut child = if claimed {
            let child = start_gui()?;
            // The pre-spawn PID snapshot cannot contain the newborn GUI:
            // drop it so the next discovery pass enumerates fresh instead
            // of deleting the just-written descriptor as a corpse.
            live_pids_invalidate();
            Some(child)
        } else {
            None
        };
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if let Some(child) = child.as_mut() {
                if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
                    if claimed {
                        release_startup_lock(&directory);
                    }
                    return Err(format!("OpenCADStudio exited while starting ({status})"));
                }
            }
            thread::sleep(Duration::from_millis(200));
            available = descriptors()?;
            if !available.is_empty() {
                break;
            }
        }
        if claimed {
            release_startup_lock(&directory);
        }
        if available.is_empty() {
            return Err("OpenCADStudio is still starting; call ocs_sessions again".into());
        }
    }
    Ok(available
        .into_iter()
        .map(|(_, state)| with_bridge_identity(state))
        .collect())
}

pub(super) fn insert_default(object: &mut Map<String, Value>, key: &str, value: Value) {
    if !object.contains_key(key) {
        object.insert(key.into(), value);
    }
}

impl GuiClient {
    pub(super) fn connect(session_id: &str) -> Result<Self, CallError> {
        let mut matching: Vec<_> = descriptors()
            .map_err(CallError::Infra)?
            .into_iter()
            .filter(|(descriptor, _)| descriptor.session_id == session_id)
            .collect();
        if matching.len() != 1 {
            return Err(CallError::infra(format!(
                "Choose session_id from ocs_sessions; found {} matching sessions",
                matching.len()
            )));
        }
        let (descriptor, state) = matching.remove(0);
        Ok(Self {
            descriptor,
            state,
            client_id: random_id().map_err(CallError::Infra)?,
            batches: VecDeque::new(),
            inflight: HashMap::new(),
            dismissed: VecDeque::new(),
        })
    }

    /// Best-effort dismiss of a GUI-pending interactive op: the GUI resolves
    /// a pending `user_select`/`getpoint` as cancelled on `op == "cancel"`
    /// (see `app::control`), and late completions are dropped bridge-side
    /// via `dismissed`. Failures are ignored: dismissal races a GUI that may
    /// already be gone, and the outcome (Cancelled/timeout) stands either way.
    pub(super) fn dismiss(&mut self, gui_id: Option<&str>) {
        let Some(gui_id) = gui_id else { return };
        let cancel = json!({
            "op": "cancel",
            "request_id": random_id().unwrap_or_else(|_| format!("cancel-{}", iso8601_now())),
            "client_id": self.client_id.clone(),
            "document_id": self.state["document_id"].clone(),
            "revision": self.state["revision"].clone(),
        });
        let _ = exchange(&self.descriptor, cancel, Duration::from_secs(5));
        if !self.dismissed.iter().any(|id| id == gui_id) {
            if self.dismissed.len() >= 128 {
                self.dismissed.pop_front();
            }
            self.dismissed.push_back(gui_id.to_string());
        }
    }

    pub(super) fn request(
        &mut self,
        request: Value,
        wait_seconds: f64,
        pump: &mut CancelPump,
        mcp_id: Option<&Value>,
    ) -> Result<Value, CallError> {
        let mut object = request
            .as_object()
            .cloned()
            .ok_or_else(|| "request must be an object".to_string())?;
        let op = object
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| "request must contain op".to_string())?
            .to_string();
        // The GUI requires request_id on every non-query op, capture
        // included: without one the GUI rejects the request outright, so
        // the bridge always mints one (a client-supplied id wins).
        if !READ_OPS.contains(&op.as_str()) || op == "capture" {
            insert_default(&mut object, "request_id", Value::String(random_id()?));
        }
        // Capture reads the active tab, so it needs the document the
        // GUI requires on every non-query op (revision stays absent:
        // the GUI only checks revisions callers supply).
        if op == "capture" {
            insert_default(
                &mut object,
                "document_id",
                self.state["document_id"].clone(),
            );
        }
        if !READ_OPS.contains(&op.as_str()) {
            insert_default(
                &mut object,
                "client_id",
                Value::String(self.client_id.clone()),
            );
            if op != "entities_copy_to" {
                insert_default(
                    &mut object,
                    "document_id",
                    self.state["document_id"].clone(),
                );
            }
            insert_default(&mut object, "revision", self.state["revision"].clone());
            if [
                "input",
                "property",
                "run",
                "action",
                "save",
                "save_verified",
                "undo",
                "redo",
            ]
            .contains(&op.as_str())
            {
                insert_default(&mut object, "selection", self.state["selection"].clone());
            }
        }

        let request_id = object.get("request_id").cloned();
        // Re-polls for dismissed interactions answer `cancelled` without
        // touching the GUI: the prompt is already gone.
        if op == "operation" {
            if let Some(polled) = request_id.as_ref().and_then(Value::as_str) {
                if self.dismissed.iter().any(|id| id == polled) {
                    return Ok(json!({"ok":false,"status":"cancelled","request_id":polled}));
                }
            }
        }
        let mut response = exchange(
            &self.descriptor,
            Value::Object(object),
            Duration::from_secs(15),
        )?;
        let gui_id = request_id
            .as_ref()
            .and_then(Value::as_str)
            .map(str::to_string);
        let mkey = mcp_id.map(mcp_key);
        // Track the GUI op while it stays pending so an idle-arriving
        // cancel can find and dismiss it. Terminal outcomes remove the
        // entry; deadline exits keep it (the agent re-polls and re-tracks).
        if let (Some(key), Some(gui)) = (mkey.as_ref(), gui_id.as_ref()) {
            self.inflight.insert(key.clone(), gui.clone());
        }
        let interactive = matches!(op.as_str(), "user_select" | "getpoint");
        let deadline = Instant::now() + wait_deadline(&op, wait_seconds);
        while matches!(response["status"].as_str(), Some("accepted" | "running"))
            && Instant::now() < deadline
        {
            let Some(request_id) = request_id.clone() else {
                break;
            };
            match pump.wait_line(Duration::from_millis(50), mkey.as_deref()) {
                PumpEvent::Cancelled => {
                    self.dismiss(gui_id.as_deref());
                    if let Some(key) = mkey.as_ref() {
                        self.inflight.remove(key);
                    }
                    return Err(CallError::Cancelled);
                }
                PumpEvent::Quiet => {}
            }
            response = exchange(
                &self.descriptor,
                json!({"op":"operation","request_id":request_id}),
                Duration::from_secs(15),
            )?;
        }
        let terminal = !matches!(response["status"].as_str(), Some("accepted" | "running"));
        if terminal {
            if let Some(key) = mkey.as_ref() {
                self.inflight.remove(key);
            }
            if interactive && response["status"].as_str() == Some("cancelled") {
                if let Some(gui) = gui_id.as_ref() {
                    if !self.dismissed.iter().any(|id| id == gui) {
                        if self.dismissed.len() >= 128 {
                            self.dismissed.pop_front();
                        }
                        self.dismissed.push_back(gui.clone());
                    }
                }
            }
        } else if interactive && interactive_timeout(wait_seconds) {
            // Only the ceiling itself dismisses: short waits keep the
            // classic running response so agents can re-poll. Hitting the
            // ten-minute ceiling means nobody is coming: dismiss the prompt
            // and say so plainly instead of parking it forever.
            self.dismiss(gui_id.as_deref());
            if let Some(key) = mkey.as_ref() {
                self.inflight.remove(key);
            }
            return Err(CallError::Tool(
                "timed out waiting for user (10 min); the prompt was dismissed".into(),
            ));
        }
        if response.get("state").is_some() {
            self.state = response["state"].clone();
        } else if matches!(op.as_str(), "hello" | "state") && response["ok"].as_bool() == Some(true)
        {
            self.state = response.clone();
        }
        Ok(response)
    }

    pub(super) fn execute_batch(
        &mut self,
        request: Value,
        wait_seconds: f64,
        pump: &mut CancelPump,
        mcp_id: Option<&Value>,
    ) -> Result<Value, CallError> {
        let id = required_string(&request, "request_id")?.to_owned();
        let mut batch = if let Some(position) = self.batches.iter().position(|batch| batch.id == id)
        {
            let batch = self
                .batches
                .remove(position)
                .expect("batch position exists");
            if batch.request != request {
                self.batches.push_back(batch);
                return Err("request_id was already used for a different batch".into());
            }
            batch
        } else {
            let steps = request["steps"]
                .as_array()
                .cloned()
                .ok_or_else(|| "batch requires a steps array".to_string())?;
            BatchExecution {
                id,
                request,
                steps,
                next: 0,
                active: None,
                pending: None,
                results: Vec::new(),
                changes: Vec::new(),
                state: None,
                terminal: None,
            }
        };

        if let Some(result) = batch.terminal.clone() {
            self.batches.push_back(batch);
            return Ok(result);
        }

        let deadline = Instant::now() + Duration::from_secs_f64(wait_seconds.clamp(0.0, 60.0));
        let mut attempted = false;
        loop {
            if batch.next == batch.steps.len() {
                let waiting = batch
                    .state
                    .as_ref()
                    .is_some_and(|state| !state["command"].is_null());
                let result = batch_result(
                    &batch,
                    if waiting {
                        "waiting_input"
                    } else {
                        "completed"
                    },
                    true,
                );
                batch.terminal = Some(result.clone());
                self.batches.push_back(batch);
                trim_batches(&mut self.batches);
                return Ok(result);
            }
            if attempted && Instant::now() >= deadline {
                let result = batch_result(&batch, "running", true);
                self.batches.push_back(batch);
                trim_batches(&mut self.batches);
                return Ok(result);
            }
            attempted = true;

            // Cancel between steps: dismiss a pending interactive step so
            // no late answer fires, then abandon the batch (cancel = stop).
            if let PumpEvent::Cancelled =
                pump.wait_line(Duration::ZERO, mcp_id.map(mcp_key).as_deref())
            {
                self.dismiss(batch.active.as_deref());
                return Err(CallError::Cancelled);
            }
            let step_id = batch
                .active
                .clone()
                .unwrap_or_else(|| batch_step_id(&batch.id, batch.next));
            let response = if batch.active.is_some() {
                self.request(
                    json!({"op":"operation","request_id":step_id}),
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_secs_f64(),
                    pump,
                    mcp_id,
                )
            } else if let Err(error) =
                resolve_batch_step(&batch.steps[batch.next], batch.next, &batch.results)
            {
                Ok(
                    json!({"ok":false,"status":"failed","code":"invalid_batch_binding","error":error}),
                )
            } else {
                let resolved =
                    resolve_batch_step(&batch.steps[batch.next], batch.next, &batch.results)
                        .expect("step resolution just succeeded");
                let mut step = resolved
                    .as_object()
                    .cloned()
                    .ok_or_else(|| format!("batch step {} must be an object", batch.next))?;
                for key in [
                    "revision",
                    "geometry_revision",
                    "camera_revision",
                    "selection",
                    "client_id",
                ] {
                    step.remove(key);
                }
                step.insert("request_id".into(), Value::String(step_id.clone()));
                self.request(
                    Value::Object(step),
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_secs_f64(),
                    pump,
                    mcp_id,
                )
            };
            let response = match response {
                Ok(response) => response,
                // Cancelled abandons the batch outright: resuming would
                // re-poll a dismissed step.
                Err(CallError::Cancelled) => return Err(CallError::Cancelled),
                Err(error) => {
                    batch.active = Some(step_id);
                    self.batches.push_back(batch);
                    trim_batches(&mut self.batches);
                    return Err(error);
                }
            };

            // Keep the latest command/prompt state even while a step is running.
            if let Some(state) = response.get("state") {
                batch.state = Some(state.clone());
            }
            if matches!(response["status"].as_str(), Some("accepted" | "running")) {
                batch.active = Some(step_id);
                let mut pending = response.clone();
                if let Some(object) = pending.as_object_mut() {
                    object.remove("state");
                    object.insert("step".into(), Value::from(batch.next));
                    object.insert("op".into(), batch.steps[batch.next]["op"].clone());
                }
                batch.pending = Some(pending);
                let result = batch_result(&batch, "running", true);
                self.batches.push_back(batch);
                trim_batches(&mut self.batches);
                return Ok(result);
            }

            batch.active = None;
            batch.pending = None;
            if let Some(changes) = response["changes"].as_array() {
                batch
                    .changes
                    .extend(changes.iter().cloned().map(|mut change| {
                        if let Some(object) = change.as_object_mut() {
                            object.insert("step".into(), Value::from(batch.next));
                        }
                        change
                    }));
            }
            let mut compact = response.clone();
            if let Some(object) = compact.as_object_mut() {
                object.remove("state");
                object.remove("changes");
                object.insert("step".into(), Value::from(batch.next));
                object.insert("op".into(), batch.steps[batch.next]["op"].clone());
            }
            batch.results.push(compact);
            batch.next += 1;

            // Nothing ran: forget the batch so the same request_id can retry
            // against the state this reply just refreshed (#2488).
            if stale_before_first_step(&batch, &response) {
                return Ok(batch_result(&batch, "failed", false));
            }

            if response["ok"].as_bool() == Some(false)
                || matches!(response["status"].as_str(), Some("failed" | "cancelled"))
            {
                let result = batch_result(
                    &batch,
                    response["status"].as_str().unwrap_or("failed"),
                    false,
                );
                batch.terminal = Some(result.clone());
                self.batches.push_back(batch);
                trim_batches(&mut self.batches);
                return Ok(result);
            }
            if response["status"] == "waiting_input"
                && batch
                    .steps
                    .get(batch.next)
                    .and_then(|step| step["op"].as_str())
                    .is_none_or(|op| !matches!(op, "input" | "cancel"))
            {
                let result = batch_result(&batch, "waiting_input", true);
                batch.terminal = Some(result.clone());
                self.batches.push_back(batch);
                trim_batches(&mut self.batches);
                return Ok(result);
            }
        }
    }
}

/// A fresh `state` for each held connection, so an idle one is not left stale.
/// A connection whose active document changed keeps its state: the bridge
/// must not silently retarget its next edit.
pub(super) fn refresh_client_states(clients: &mut HashMap<String, GuiClient>) {
    for client in clients.values_mut() {
        if let Ok(state) = exchange(
            &client.descriptor,
            json!({"op":"state"}),
            Duration::from_secs(5),
        ) {
            if adopt_refreshed_state(&client.state, &state) {
                client.state = state;
            }
        }
    }
}

pub(super) fn adopt_refreshed_state(current: &Value, fresh: &Value) -> bool {
    fresh["ok"] == true && fresh["document_id"] == current["document_id"]
}

pub(super) fn client<'a>(
    clients: &'a mut HashMap<String, GuiClient>,
    session_id: &str,
) -> Result<&'a mut GuiClient, CallError> {
    if !clients.contains_key(session_id) {
        clients.insert(session_id.into(), GuiClient::connect(session_id)?);
    }
    Ok(clients.get_mut(session_id).expect("client inserted"))
}

pub(super) fn select_session_candidate(
    explicit: Option<&str>,
    connected: &[String],
    discovered: &[String],
) -> Result<Option<String>, String> {
    if let Some(session_id) = explicit.filter(|id| !id.is_empty()) {
        return Ok(Some(session_id.to_string()));
    }
    let candidates = if connected.is_empty() {
        discovered
    } else {
        connected
    };
    match candidates {
        [] => Ok(None),
        [session_id] => Ok(Some(session_id.clone())),
        _ => Err(
            "Multiple OpenCADStudio sessions are available; provide ocs_session_id from ocs_sessions"
                .to_string(),
        ),
    }
}

pub(super) fn resolve_intent_session(
    arguments: &Value,
    clients: &HashMap<String, GuiClient>,
) -> Result<String, CallError> {
    let explicit = arguments["ocs_session_id"].as_str();
    let connected: Vec<String> = clients.keys().cloned().collect();
    if explicit.is_some_and(|id| !id.is_empty()) || !connected.is_empty() {
        return select_session_candidate(explicit, &connected, &[])?
            .ok_or_else(|| CallError::Tool("No OpenCADStudio session is available".to_string()));
    }
    let discovered = sessions(true).map_err(CallError::Infra)?;
    let discovered: Vec<String> = discovered
        .iter()
        .filter_map(|session| session["session_id"].as_str().map(str::to_string))
        .collect();
    select_session_candidate(None, &[], &discovered)?
        .ok_or_else(|| CallError::Tool("No OpenCADStudio session is available".to_string()))
}
