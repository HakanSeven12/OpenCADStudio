//! LOGFILEMODE, LOGFILEON / LOGFILEOFF and LOGFILEPATH: everything the
//! command line shows is also written to a text file, so a session can be
//! sent along with a report without the drawing it was about (#2606). The
//! mode and the folder are profile settings.

use crate::app::OpenCADStudio;
use std::io::Write;
use std::path::PathBuf;

struct State {
    mode: bool,
    /// The folder chosen with LOGFILEPATH; empty is the default one.
    folder: String,
    /// This session's file, opened by its first line.
    file: Option<(PathBuf, std::fs::File, std::time::Instant)>,
}

const START: State = State {
    mode: false,
    folder: String::new(),
    file: None,
};

#[cfg(not(test))]
static STATE: std::sync::Mutex<State> = std::sync::Mutex::new(START);

// Tests run side by side and every app they make applies the profile
// settings, which reset the log file; each test thread keeps its own.
#[cfg(test)]
thread_local! {
    static STATE: std::cell::RefCell<State> = const { std::cell::RefCell::new(START) };
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    #[cfg(not(test))]
    {
        f(&mut STATE.lock().unwrap_or_else(|e| e.into_inner()))
    }
    #[cfg(test)]
    {
        STATE.with(|state| f(&mut state.borrow_mut()))
    }
}

pub(crate) fn log_file_mode() -> bool {
    with_state(|state| state.mode)
}

pub(crate) fn set_log_file_mode(on: bool) {
    with_state(|state| {
        state.mode = on;
        if !on {
            state.file = None;
        }
    });
}

/// The folder as set; empty when it is the default.
pub(crate) fn log_file_folder_setting() -> String {
    with_state(|state| state.folder.clone())
}

pub(crate) fn set_log_file_folder(folder: &str) {
    with_state(|state| {
        state.folder = folder.trim().to_string();
        // The next line starts a file in the new folder.
        state.file = None;
    });
}

/// Where log files go: the folder set with LOGFILEPATH, else `logs` in the
/// settings folder.
pub(crate) fn log_file_folder() -> Option<PathBuf> {
    let set = log_file_folder_setting();
    if !set.is_empty() {
        return Some(PathBuf::from(set));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::config::config_dir().map(|dir| dir.join("logs"))
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

/// This session's log file, once something was written to it.
pub(crate) fn log_file_path() -> Option<PathBuf> {
    with_state(|state| state.file.as_ref().map(|(path, _, _)| path.clone()))
}

/// Append one command-line line, stamped with the seconds since the file
/// was started. Failing to write is never an error of the command.
pub(crate) fn write_line(text: &str) {
    if cfg!(target_arch = "wasm32") || !log_file_mode() {
        return;
    }
    let folder = log_file_folder();
    with_state(|state| {
        if state.file.is_none() {
            let Some(folder) = folder else {
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
            let Ok(mut opened) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
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
            state.file = Some((path, opened, std::time::Instant::now()));
        }
        if let Some((_, out, started)) = state.file.as_mut() {
            let _ = writeln!(out, "[{:>9.2}s] {text}", started.elapsed().as_secs_f32());
        }
    });
}

impl OpenCADStudio {
    /// LOGFILEON / LOGFILEOFF / LOGFILEMODE [0|1].
    pub(super) fn log_file_setvar(&mut self, name: &str, value: Option<&str>) {
        let on = match name {
            "LOGFILEON" => true,
            "LOGFILEOFF" => false,
            _ => match value.map(str::trim).filter(|value| !value.is_empty()) {
                None => {
                    self.command_line
                        .push_output(&crate::ui::command_line::value_prompt(
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
                &log_file_path()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
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

    /// SAVEFILEPATH [folder]: where autosave recovery copies go; `.` puts
    /// them back in the default folder (#2671).
    pub(super) fn save_file_path_setvar(&mut self, value: Option<&str>) {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            None => {
                #[cfg(not(target_arch = "wasm32"))]
                let shown = self.save_file_folder().display().to_string();
                #[cfg(target_arch = "wasm32")]
                let shown = String::new();
                self.command_line
                    .push_output(&crate::ui::command_line::value_prompt(
                        "SAVEFILEPATH",
                        &shown,
                    ));
                self.pending_setvar = Some("SAVEFILEPATH".to_string());
            }
            Some(".") => {
                self.save_file_path.clear();
                self.save_config();
            }
            Some(folder) => {
                self.save_file_path = folder.trim_matches('"').to_string();
                self.save_config();
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
                    .push_output(&crate::ui::command_line::value_prompt(
                        "LOGFILEPATH",
                        &shown,
                    ));
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
        app.automation_op(
            &serde_json::json!({"op": "run", "cmd": format!("LOGFILEPATH {}", folder.display())})
                .to_string(),
        );
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
