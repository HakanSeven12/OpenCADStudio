//! Crash reports: written when the process dies, read back on the next
//! start.

use std::path::PathBuf;

/// Newest reports to keep. A crash loop must not fill the user's profile,
/// and the first report of a loop is the interesting one, so keep both
/// ends by pruning only what is older than this.
const KEEP: usize = 20;

/// Where reports go, beside the recovery logs.
pub fn directory() -> Option<PathBuf> {
    crate::config::config_dir().map(|path| path.join("crash_logs"))
}

/// The report a given process left behind, if it left one.
///
/// Reports are named for the process that wrote them, so a caller holding
/// a process id from somewhere else — the GPU crash sentinel does — can
/// ask what killed that exact run instead of guessing from the newest
/// file, which on a busy machine may belong to a different one.
///
/// Process ids are reused, so only a report written at or after
/// `not_before` (Unix seconds — when that run started) counts: an old
/// report from an unrelated run that once had the same id does not.
///
/// A panic during the unwind of another one writes a report too, so a run
/// can leave more than one. The first one names the cause.
pub fn report_for_pid(pid: u32, not_before: u64) -> Option<String> {
    report_for_pid_in(&directory()?, pid, not_before)
}

fn report_for_pid_in(directory: &std::path::Path, pid: u32, not_before: u64) -> Option<String> {
    let first = std::fs::read_dir(directory)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let order = report_order(&entry.file_name().to_string_lossy(), pid)?;
            (order.0 >= not_before).then_some((order, entry.path()))
        })
        .min_by_key(|(order, _)| *order)?;
    std::fs::read_to_string(first.1).ok()
}

/// The (Unix seconds, number in that second) of a report of `pid`, from
/// its name: `crash-<seconds>-<pid>.log`, then `crash-<seconds>-<pid>-2.log`.
fn report_order(name: &str, pid: u32) -> Option<(u64, u32)> {
    let rest = name.strip_prefix("crash-")?.strip_suffix(".log")?;
    let mut parts = rest.split('-');
    let when = parts.next()?.parse::<u64>().ok()?;
    if parts.next()?.parse::<u32>().ok()? != pid {
        return None;
    }
    let number = match parts.next() {
        Some(number) => number.parse::<u32>().ok()?,
        None => 1,
    };
    parts.next().is_none().then_some((when, number))
}

/// Whether a report describes the graphics device giving up.
///
/// Deliberately narrow. Anything that reaches wgpu — an exhausted device,
/// a lost one, a buffer that was never created — is the backend's
/// business; a panic in the drawing code that happens to run on a frame
/// is not, and must not cost the user a working backend.
pub fn is_device_failure(report: &str) -> bool {
    let mut panic_line = "";
    let mut at_line = "";
    for line in report.lines() {
        if let Some(rest) = line.strip_prefix("panic: ") {
            panic_line = rest;
        } else if let Some(rest) = line.strip_prefix("at: ") {
            at_line = rest;
        }
    }
    // The message wgpu panics with names the call, and the location names
    // wgpu itself; either alone is enough, since a stripped build can
    // leave the message terse. Plain words like "buffer" or "surface"
    // are not: drawing code and the modelling kernel panic with those
    // too, and that is no reason to give up a working backend.
    let haystack = format!("{panic_line} {at_line}").to_ascii_lowercase();
    ["wgpu", "out of memory", "device lost", "validation error"]
        .iter()
        .any(|needle| haystack.contains(needle))
}

/// The backend a report says was live, as written by [`report_from`].
pub fn backend_in_report(report: &str) -> Option<String> {
    report
        .lines()
        .find_map(|line| line.strip_prefix("gpu backend: "))
        .map(str::trim)
        .filter(|backend| !backend.is_empty() && *backend != "(default)")
        .map(str::to_string)
}

/// Chain a report writer onto the current panic hook.
///
/// Call once, early: a panic before this runs is still silent.
pub fn install() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // A panic while reporting a panic must not replace the original
        // with a recursion, so failures here are dropped on purpose.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| write(info)));
        previous(info);
    }));
}

/// The report body. Separated from the IO so a test can read it.
pub fn report(info: &std::panic::PanicHookInfo<'_>, when: u64) -> String {
    let payload = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".to_string());
    let where_ = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "(unknown location)".to_string());
    report_from(&payload, &where_, when)
}

/// The report body, with the two pieces a panic carries already pulled
/// out. Separate so a test can exercise the layout without installing a
/// panic hook: the hook is process-global, and a test that asserts inside
/// one fires on every other test's panic and aborts the whole binary.
pub fn report_from(payload: &str, where_: &str, when: u64) -> String {
    // `WGPU_BACKEND` is what the backend resolver settled on, so the
    // report says which graphics path was live without reaching into the
    // renderer from a panic handler.
    let backend = std::env::var("WGPU_BACKEND").unwrap_or_else(|_| "(default)".to_string());
    // `force_capture`, not `capture`: a report whose backtrace says
    // "disabled backtrace" because the user never set RUST_BACKTRACE is
    // exactly the report that helps nobody. Release builds are stripped,
    // so some frames are addresses, but the frame count and the module
    // boundaries still place the fault.
    let backtrace = std::backtrace::Backtrace::force_capture();
    [
        format!("Open CAD Studio {} crash report", env!("OCS_APP_VERSION")),
        format!("when: {when} (unix seconds)"),
        format!("os: {} {}", std::env::consts::OS, std::env::consts::ARCH),
        format!("gpu backend: {backend}"),
        format!(
            "thread: {}",
            std::thread::current().name().unwrap_or("unnamed")
        ),
        format!("panic: {payload}"),
        format!("at: {where_}"),
        String::new(),
        "backtrace:".to_string(),
        backtrace.to_string(),
        String::new(),
    ]
    .join("\n")
}

fn write(info: &std::panic::PanicHookInfo<'_>) {
    let Some(directory) = directory() else {
        return;
    };
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    let when = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    write_new(&directory, when, std::process::id(), &report(info, when));
    prune(&directory);
}

/// Writes a report to a file of its own. A panic during the unwind of
/// another one comes in the same second, so a second report of that
/// second gets `-2` and does not replace the first, which names the cause.
fn write_new(directory: &std::path::Path, when: u64, pid: u32, text: &str) {
    for number in 1..=99u32 {
        let name = match number {
            1 => format!("crash-{when}-{pid}.log"),
            _ => format!("crash-{when}-{pid}-{number}.log"),
        };
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(directory.join(name)) {
            Ok(mut file) => {
                use std::io::Write;
                let _ = file.write_all(text.as_bytes());
                return;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return,
        }
    }
}

/// Keep the newest [`KEEP`] reports.
fn prune(directory: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "log"))
        .filter_map(|e| {
            let modified = e.metadata().ok()?.modified().ok()?;
            Some((modified, e.path()))
        })
        .collect();
    if files.len() <= KEEP {
        return;
    }
    files.sort_by_key(|(when, _)| *when);
    for (_, path) in &files[..files.len() - KEEP] {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use serial_test::serial;
    /// The report has to carry the two things a maintainer cannot guess:
    /// what the message was and where it came from. It also has to be
    /// readable — an earlier version built it with `\` continuations and
    /// pasted the source indentation into every line.
    #[test]
    fn a_report_names_the_message_and_the_line() {
        let text = super::report_from("a deliberate test panic", "src/sys.rs:1:2", 42);
        assert!(text.starts_with("Open CAD Studio "), "{text}");
        assert!(text.contains("panic: a deliberate test panic"), "{text}");
        assert!(text.contains("at: src/sys.rs:1:2"), "{text}");
        assert!(text.contains("when: 42 (unix seconds)"), "{text}");
        let (header, trace) = text
            .split_once("\nbacktrace:\n")
            .expect("the report carries a backtrace section");
        assert!(!trace.trim().is_empty(), "the backtrace must not be empty");
        // Header only: a backtrace indents its own continuation lines,
        // and that is how a backtrace is meant to read.
        assert!(
            header.lines().all(|line| !line.starts_with(' ')),
            "header lines must not be indented:\n{header}"
        );
    }

    /// Only the device's own failures may cost a backend. A panic in
    /// drawing code that merely happened to run on a frame must not.
    #[test]
    fn a_device_failure_is_told_apart_from_an_ordinary_panic() {
        let device = super::report_from(
            "Error in Buffer::get_mapped_range: Validation Error",
            "wgpu-29.0.4/src/backend/wgpu_core.rs:2253:18",
            1,
        );
        assert!(super::is_device_failure(&device), "{device}");

        let oom = super::report_from("Out of Memory", "src/scene/pipeline/mod.rs:10:1", 1);
        assert!(super::is_device_failure(&oom), "{oom}");

        let ordinary = super::report_from(
            "index out of bounds: the len is 3 but the index is 7",
            "src/app/commands/draw.rs:120:5",
            1,
        );
        assert!(
            !super::is_device_failure(&ordinary),
            "an indexing bug is not the graphics device's fault:
{ordinary}"
        );
    }

    /// The backend line is what the sentinel joins on, so it has to come
    /// back out — and "(default)" names nothing to blame.
    #[test]
    #[serial]
    fn the_backend_is_read_back_out_of_a_report() {
        // SAFETY: single-threaded test mutating a process-local variable.
        unsafe { std::env::set_var("WGPU_BACKEND", "dx12") };
        let named = super::report_from("boom", "x.rs:1:1", 0);
        assert_eq!(super::backend_in_report(&named).as_deref(), Some("dx12"));

        unsafe { std::env::remove_var("WGPU_BACKEND") };
        let unnamed = super::report_from("boom", "x.rs:1:1", 0);
        assert_eq!(super::backend_in_report(&unnamed), None);
    }

    /// A panic with no message still has to produce a filed report rather
    /// than nothing at all.
    #[test]
    fn a_report_survives_a_panic_with_nothing_to_say() {
        let text = super::report_from("(no message)", "(unknown location)", 0);
        assert!(text.contains("panic: (no message)"), "{text}");
        assert!(text.contains("at: (unknown location)"), "{text}");
    }

    fn read(path: std::path::PathBuf) -> String {
        std::fs::read_to_string(path).expect("the report")
    }

    /// A panic during the unwind of another one comes in the same second.
    /// Its report must not replace the first, which names the cause.
    #[test]
    fn a_second_report_in_the_same_second_does_not_replace_the_first() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        super::write_new(dir.path(), 50, 77, "panic: the cause");
        super::write_new(
            dir.path(),
            50,
            77,
            "panic: in a function that cannot unwind",
        );
        assert_eq!(read(dir.path().join("crash-50-77.log")), "panic: the cause");
        assert_eq!(
            read(dir.path().join("crash-50-77-2.log")),
            "panic: in a function that cannot unwind"
        );
    }

    /// The GPU backend fallback asks what ended a run; the first report of
    /// the run says it.
    #[test]
    fn the_report_of_a_run_is_its_first_one() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        for (name, text) in [
            ("crash-51-77.log", "the next second"),
            ("crash-50-77-2.log", "the second panic"),
            ("crash-50-77.log", "the cause"),
            ("crash-49-78.log", "another process"),
        ] {
            std::fs::write(dir.path().join(name), text).expect("a report");
        }
        assert_eq!(
            super::report_for_pid_in(dir.path(), 77, 0).as_deref(),
            Some("the cause")
        );
    }

    /// Process ids are reused: a report from before the run does not count,
    /// and the next one of the run does.
    #[test]
    fn a_report_from_before_the_run_does_not_count() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        std::fs::write(dir.path().join("crash-40-77.log"), "an old run").expect("a report");
        std::fs::write(dir.path().join("crash-50-77-2.log"), "this run").expect("a report");
        assert_eq!(
            super::report_for_pid_in(dir.path(), 77, 50).as_deref(),
            Some("this run")
        );
        assert_eq!(super::report_for_pid_in(dir.path(), 77, 51), None);
    }

    #[test]
    fn a_report_name_gives_its_order_only_for_its_own_process() {
        assert_eq!(super::report_order("crash-50-77.log", 77), Some((50, 1)));
        assert_eq!(
            super::report_order("crash-50-77-12.log", 77),
            Some((50, 12))
        );
        assert_eq!(super::report_order("crash-50-777.log", 77), None);
        assert_eq!(super::report_order("crash-50-7.log", 77), None);
        assert_eq!(super::report_order("crash-50-77-x.log", 77), None);
        assert_eq!(super::report_order("crash-50-77-2-3.log", 77), None);
        assert_eq!(super::report_order("crash-50-77.txt", 77), None);
    }
}
