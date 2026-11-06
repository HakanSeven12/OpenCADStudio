//! LOGFILEMODE, LOGFILEON / LOGFILEOFF and LOGFILEPATH: everything the
//! command line shows is also written to a text file, so a session can be
//! sent along with a report without the drawing it was about (#2606). The
//! mode and the folder are profile settings.

use crate::app::OpenCADStudio;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

static MODE: AtomicBool = AtomicBool::new(false);
/// The folder chosen with LOGFILEPATH; empty is the default one.
static FOLDER: Mutex<String> = Mutex::new(String::new());
/// This session's file, opened by its first line.
static FILE: Mutex<Option<(PathBuf, std::fs::File, std::time::Instant)>> = Mutex::new(None);

pub(crate) fn log_file_mode() -> bool {
    MODE.load(Ordering::Relaxed)
}

pub(crate) fn set_log_file_mode(on: bool) {
    MODE.store(on, Ordering::Relaxed);
    if !on {
        *FILE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// The folder as set; empty when it is the default.
pub(crate) fn log_file_folder_setting() -> String {
    FOLDER.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub(crate) fn set_log_file_folder(folder: &str) {
    *FOLDER.lock().unwrap_or_else(|e| e.into_inner()) = folder.trim().to_string();
    // The next line starts a file in the new folder.
    *FILE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Where log files go: the folder set with LOGFILEPATH, else `logs` in the
/// settings folder.
pub(crate) fn log_file_folder() -> Option<PathBuf> {
    let set = log_file_folder_setting();
    if !set.is_empty() {
        return Some(PathBuf::from(set));
    }
    crate::config::config_dir().map(|dir| dir.join("logs"))
}

/// This session's log file, once something was written to it.
pub(crate) fn log_file_path() -> Option<PathBuf> {
    FILE.lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|(path, _, _)| path.clone())
}

/// Append one command-line line, stamped with the seconds since the file
/// was started. Failing to write is never an error of the command.
pub(crate) fn write_line(text: &str) {
    if cfg!(target_arch = "wasm32") || !log_file_mode() {
        return;
    }
    let mut file = FILE.lock().unwrap_or_else(|e| e.into_inner());
    if file.is_none() {
        let Some(folder) = log_file_folder() else {
            return;
        };
        if std::fs::create_dir_all(&folder).is_err() {
            return;
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let path = folder.join(format!("opencadstudio-{stamp}-{}.log", std::process::id()));
        let Ok(mut opened) = std::fs::OpenOptions::new().create(true).append(true).open(&path)
        else {
            return;
        };
        let _ = writeln!(
            opened,
            "Open CAD Studio {} ({} {}), started at Unix time {stamp}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
        );
        *file = Some((path, opened, std::time::Instant::now()));
    }
    if let Some((_, out, started)) = file.as_mut() {
        let _ = writeln!(out, "[{:>9.2}s] {text}", started.elapsed().as_secs_f32());
    }
}

impl OpenCADStudio {
    /// LOGFILEON / LOGFILEOFF / LOGFILEMODE [0|1].
    pub(super) fn log_file_setvar(&mut self, name: &str, value: Option<&str>) {
        let on = match name {
            "LOGFILEON" => true,
            "LOGFILEOFF" => false,
            _ => match value.map(str::trim).filter(|value| !value.is_empty()) {
                None => {
                    self.command_line.push_output(&crate::ui::command_line::value_prompt(
                        "LOGFILEMODE",
                        &u8::from(log_file_mode()),
                    ));
                    self.pending_setvar = Some("LOGFILEMODE".to_string());
                    return;
                }
                Some("0") => false,
                Some("1") => true,
                Some(_) => {
                    self.command_line
                        .push_error(crate::t!("Requires an integer between 0 and 1.").as_ref());
                    return;
                }
            },
        };
        if on == log_file_mode() {
            if let (true, Some(path)) = (on, log_file_path()) {
                self.command_line.push_info(&path.display().to_string());
            }
            return;
        }
        if !on {
            self.command_line.push_info(
                &log_file_path().map(|path| path.display().to_string()).unwrap_or_default(),
            );
        }
        set_log_file_mode(on);
        self.save_config();
        if on {
            // The first line opens the file; show where it is.
            self.command_line.push_info("LOGFILEMODE = 1");
            if let Some(path) = log_file_path() {
                self.command_line.push_info(&path.display().to_string());
            }
        }
    }

    /// LOGFILEPATH [folder]: `.` puts the files back in the default folder.
    pub(super) fn log_file_path_setvar(&mut self, value: Option<&str>) {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            None => {
                let shown = log_file_folder()
                    .map(|folder| folder.display().to_string())
                    .unwrap_or_default();
                self.command_line
                    .push_output(&crate::ui::command_line::value_prompt("LOGFILEPATH", &shown));
                self.pending_setvar = Some("LOGFILEPATH".to_string());
            }
            Some(".") => {
                set_log_file_folder("");
                self.save_config();
            }
            Some(folder) => {
                set_log_file_folder(folder.trim_matches('"'));
                self.save_config();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_command_line_reaches_the_log_file() {
        let mut app = crate::app::OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let folder = std::env::temp_dir().join(format!("ocs-logfile-{}", std::process::id()));
        app.automation_op(&serde_json::json!({"op": "run", "cmd": format!("LOGFILEPATH {}", folder.display())}).to_string());
        app.automation_op(r#"{"op":"run","cmd":"LOGFILEON"}"#);
        app.command_line.push_output("a line for the log");
        let path = super::log_file_path().expect("a log file");
        app.automation_op(r#"{"op":"run","cmd":"LOGFILEOFF"}"#);
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_dir_all(&folder);
        assert!(path.starts_with(&folder));
        assert!(text.contains("a line for the log"), "{text}");
        assert!(!super::log_file_mode());
        super::set_log_file_folder("");
    }
}
