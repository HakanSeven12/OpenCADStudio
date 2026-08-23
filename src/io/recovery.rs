use std::path::{Path, PathBuf};

use crate::io::xref::{XrefInfo, XrefStatus};
use crate::scene::OpenTimings;
#[cfg(not(target_arch = "wasm32"))]
use sha2::{Digest, Sha256};

const REPORT_SCHEMA_VERSION: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryStatus {
    Recovered,
    Failed,
}

#[derive(Debug, Clone)]
pub struct RecoveryReport {
    pub report_id: String,
    pub status: RecoveryStatus,
    pub tab_id: Option<u64>,
    pub file_name: String,
    pub source_path: Option<PathBuf>,
    pub size_bytes: u64,
    pub source_sha256: Option<String>,
    pub read_stats: Option<acadrust::ReadStats>,
    pub entities_scanned: usize,
    pub entities_removed: usize,
    pub referenced_entities_removed: usize,
    pub parser_errors_recovered: usize,
    pub diagnostics: Vec<(String, String)>,
    pub references_checked: usize,
    pub references_loaded: usize,
    pub references_recovered: usize,
    pub references_missing: usize,
    pub references_failed: usize,
    pub references_skipped: usize,
    pub reference_details: Vec<(String, String, String, Option<String>)>,
    pub reference_stats: Vec<(String, acadrust::ReadStats)>,
    pub timings: OpenTimings,
    pub total_ms: u32,
    pub failure_phase: Option<String>,
    pub error: Option<String>,
    pub save_as_required: bool,
    pub log_path: Option<PathBuf>,
    pub log_error: Option<String>,
    pub created_unix_seconds: u64,
}

impl RecoveryReport {
    pub fn recovered(
        tab_id: u64,
        path: &Path,
        size_bytes: u64,
        source_sha256: Option<String>,
        read_stats: Option<acadrust::ReadStats>,
        entities_scanned: usize,
        entities_removed: usize,
        referenced_entities_removed: usize,
        references: &[XrefInfo],
        notifications: &acadrust::notification::NotificationCollection,
        save_as_required: bool,
        timings: OpenTimings,
        total_ms: u32,
    ) -> Self {
        let references_loaded = references
            .iter()
            .filter(|item| matches!(item.status, XrefStatus::Loaded | XrefStatus::Recovered))
            .count();
        let references_recovered = references
            .iter()
            .filter(|item| item.status == XrefStatus::Recovered)
            .count();
        let references_missing = references
            .iter()
            .filter(|item| item.status == XrefStatus::NotFound)
            .count();
        let references_failed = references
            .iter()
            .filter(|item| item.status == XrefStatus::Failed)
            .count();
        let references_skipped = references
            .iter()
            .filter(|item| item.status == XrefStatus::Unloaded)
            .count();
        let reference_details = references
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let status = match item.status {
                    XrefStatus::Loaded => "loaded",
                    XrefStatus::Recovered => "recovered",
                    XrefStatus::Failed => "failed",
                    XrefStatus::NotFound => "not found",
                    XrefStatus::Unloaded => "skipped",
                };
                (
                    format!("reference-{:02}", index + 1),
                    private_path_label(&item.path),
                    status.to_string(),
                    item.source_sha256.clone(),
                )
            })
            .collect();
        let reference_stats = references
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                let mut stats = item.read_stats.clone()?;
                for diagnostic in &mut stats.diagnostics {
                    diagnostic.message = redact_known_paths(
                        &diagnostic.message,
                        path,
                        references,
                    );
                }
                Some((format!("reference-{:02}", index + 1), stats))
            })
            .collect();
        let parser_errors_recovered = read_stats
            .as_ref()
            .map(|stats| stats.recovered_errors)
            .unwrap_or_else(|| {
                notifications
                    .iter()
                    .filter(|item| {
                        item.notification_type
                            == acadrust::notification::NotificationType::Error
                    })
                    .count()
            });
        let mut diagnostics: Vec<(String, String)> = notifications
            .iter()
            .map(|item| {
                (
                    item.notification_type.to_string(),
                    redact_known_paths(&item.message, path, references),
                )
            })
            .collect();
        if notifications.omitted_count() > 0 {
            diagnostics.insert(0, (
                "truncated".to_string(),
                format!(
                    "{} additional parser notifications were omitted after the safety limit",
                    notifications.omitted_count()
                ),
            ));
        }
        for (index, reference) in references.iter().enumerate() {
            diagnostics.extend(reference.diagnostics.iter().map(|message| {
                (
                    format!("Reference {:02}", index + 1),
                    redact_known_paths(message, path, references),
                )
            }));
        }
        let created_unix_seconds = unix_seconds();
        Self {
            report_id: report_id(source_sha256.as_deref(), created_unix_seconds),
            status: RecoveryStatus::Recovered,
            tab_id: Some(tab_id),
            file_name: display_name(path),
            source_path: Some(path.to_path_buf()),
            size_bytes,
            source_sha256,
            read_stats,
            entities_scanned,
            entities_removed,
            referenced_entities_removed,
            parser_errors_recovered,
            diagnostics,
            references_checked: references.len(),
            references_loaded,
            references_recovered,
            references_missing,
            references_failed,
            references_skipped,
            reference_details,
            reference_stats,
            timings,
            total_ms,
            failure_phase: None,
            error: None,
            save_as_required,
            log_path: None,
            log_error: None,
            created_unix_seconds,
        }
    }

    pub fn failed(
        path: Option<PathBuf>,
        file_name: String,
        size_bytes: u64,
        source_sha256: Option<String>,
        read_stats: Option<acadrust::ReadStats>,
        failure_phase: String,
        error: String,
        total_ms: u32,
    ) -> Self {
        let created_unix_seconds = unix_seconds();
        Self {
            report_id: report_id(source_sha256.as_deref(), created_unix_seconds),
            status: RecoveryStatus::Failed,
            tab_id: None,
            file_name,
            source_path: path,
            size_bytes,
            source_sha256,
            parser_errors_recovered: read_stats
                .as_ref()
                .map(|stats| stats.recovered_errors)
                .unwrap_or(0),
            read_stats,
            entities_scanned: 0,
            entities_removed: 0,
            referenced_entities_removed: 0,
            diagnostics: Vec::new(),
            references_checked: 0,
            references_loaded: 0,
            references_recovered: 0,
            references_missing: 0,
            references_failed: 0,
            references_skipped: 0,
            reference_details: Vec::new(),
            reference_stats: Vec::new(),
            timings: OpenTimings::default(),
            total_ms,
            failure_phase: Some(failure_phase),
            error: Some(error),
            save_as_required: false,
            log_path: None,
            log_error: None,
            created_unix_seconds,
        }
    }

    pub fn removed_total(&self) -> usize {
        self.entities_removed
            .saturating_add(self.referenced_entities_removed)
    }

    pub fn issues_found(&self) -> usize {
        self.parser_errors_recovered
            .saturating_add(self.references_recovered)
            .saturating_add(self.references_failed)
            .saturating_add(self.references_missing)
            .saturating_add(self.removed_total())
            .saturating_add(usize::from(self.error.is_some()))
    }

    pub fn log_text(&self) -> String {
        let mut lines = Vec::with_capacity(24);
        lines.push("Open CAD Studio drawing recovery report".to_string());
        lines.push(format!("Report schema: {}", REPORT_SCHEMA_VERSION));
        lines.push(format!("Report ID: {}", self.report_id));
        lines.push(format!("Application version: {}", env!("CARGO_PKG_VERSION")));
        lines.push(format!("Application revision: {}", env!("OCS_GIT_REV")));
        lines.push(format!("Build profile: {}", env!("OCS_BUILD_PROFILE")));
        lines.push(format!("Build features: {}", env!("OCS_BUILD_FEATURES")));
        lines.push(format!("Reader version: {}", acadrust::VERSION));
        lines.push(format!("Reader revision: {}", reader_revision()));
        lines.push(format!("Platform: {}", diagnostic_platform()));
        lines.push(format!("Created (Unix seconds): {}", self.created_unix_seconds));
        lines.push(format!(
            "File: {}",
            private_file_label(&self.file_name, self.source_sha256.as_deref())
        ));
        lines.push(format!("Size: {} bytes", self.size_bytes));
        lines.push(format!(
            "Source SHA-256: {}",
            self.source_sha256.as_deref().unwrap_or("unavailable")
        ));
        lines.push(format!(
            "Result: {}",
            match self.status {
                RecoveryStatus::Recovered => "opened with a recovery report",
                RecoveryStatus::Failed => "could not be opened",
            }
        ));
        lines.push(String::new());
        lines.push("Summary".to_string());
        if let Some(stats) = &self.read_stats {
            lines.push(format!(
                "Format: {}",
                stats
                    .source_format
                    .map(|format| format.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            ));
            lines.push(format!("Format version: {}", stats.source_version));
            lines.push(format!("Maintenance version: {}", stats.maintenance_version));
            lines.push(format!("Recovery mode: {}", stats.recovery_mode));
            lines.push(format!("Stream completed: {}", stats.stream_completed));
            lines.push(format!("Source sections: {}", stats.source_sections));
            lines.push(format!("Observed source records: {}", stats.source_records));
            lines.push(format!(
                "Successfully decoded source records: {}",
                stats.decoded_source_records
            ));
            lines.push(format!(
                "Known skipped source records: {}",
                stats.skipped_source_records
            ));
            lines.push(format!("Output entities: {}", stats.output_entities));
            lines.push(format!("Output objects: {}", stats.output_objects));
            lines.push(format!(
                "Output table records: {}",
                stats.output_table_records
            ));
        }
        lines.push(format!("Entities scanned: {}", self.entities_scanned));
        lines.push(format!("Entities removed: {}", self.entities_removed));
        lines.push(format!(
            "Referenced entities removed: {}",
            self.referenced_entities_removed
        ));
        lines.push(format!(
            "Parser errors recovered: {}",
            self.parser_errors_recovered
        ));
        lines.push(format!("References checked: {}", self.references_checked));
        lines.push(format!("References loaded: {}", self.references_loaded));
        lines.push(format!(
            "References recovered: {}",
            self.references_recovered
        ));
        lines.push(format!("References unavailable: {}", self.references_missing));
        lines.push(format!("References failed: {}", self.references_failed));
        lines.push(format!("References skipped: {}", self.references_skipped));
        lines.push(format!(
            "Save as new file required: {}",
            if self.save_as_required { "yes" } else { "no" }
        ));
        if !self.reference_details.is_empty() {
            lines.push(String::new());
            lines.push("References".to_string());
            for (name, path, status, source_sha256) in &self.reference_details {
                lines.push(format!(
                    "[{status}] {name}: {path}; SHA-256={}",
                    source_sha256.as_deref().unwrap_or("unavailable")
                ));
            }
        }
        if !self.reference_stats.is_empty() {
            lines.push(String::new());
            lines.push("Referenced source statistics".to_string());
            for (name, stats) in &self.reference_stats {
                lines.push(format!(
                    "{name}: format={} version={} recovery={} completed={} observed-records={} decoded={} known-skipped={}",
                    stats
                        .source_format
                        .map(|format| format.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    stats.source_version,
                    stats.recovery_mode,
                    stats.stream_completed,
                    stats.source_records,
                    stats.decoded_source_records,
                    stats.skipped_source_records,
                ));
                for diagnostic in stats.diagnostics.iter().take(50) {
                    lines.push(format!(
                        "{name} {}",
                        structured_diagnostic_line(diagnostic)
                    ));
                }
            }
        }
        lines.push(String::new());
        lines.push("Timing".to_string());
        lines.push(format!("Parse: {} ms", self.timings.parse_ms));
        lines.push(format!("Validation: {} ms", self.timings.purge_ms));
        lines.push(format!("References: {} ms", self.timings.xref_ms));
        lines.push(format!("Scene caches: {} ms", self.timings.caches_ms));
        lines.push(format!("Total: {} ms", self.total_ms));
        if !self.diagnostics.is_empty() {
            lines.push(String::new());
            lines.push("Diagnostics".to_string());
            for (kind, message) in self.diagnostics.iter().take(200) {
                lines.push(format!("[{kind}] {message}"));
            }
            if self.diagnostics.len() > 200 {
                lines.push(format!(
                    "[truncated] {} additional diagnostics omitted",
                    self.diagnostics.len() - 200
                ));
            }
        }
        if let Some(stats) = &self.read_stats {
            if !stats.diagnostics.is_empty() {
                lines.push(String::new());
                lines.push("Structured reader diagnostics".to_string());
                for diagnostic in stats.diagnostics.iter().take(100) {
                    lines.push(redact_source_path(
                        &structured_diagnostic_line(diagnostic),
                        self.source_path.as_deref(),
                    ));
                }
                if stats.diagnostics.len() > 100 {
                    lines.push(format!(
                        "[truncated] {} additional diagnostics omitted",
                        stats.diagnostics.len() - 100
                    ));
                }
            }
        }
        if let Some(error) = &self.error {
            lines.push(String::new());
            lines.push("Failure".to_string());
            if let Some(phase) = &self.failure_phase {
                lines.push(format!("Phase: {phase}"));
            }
            lines.push(redact_source_path(error, self.source_path.as_deref()));
        }
        lines.push(String::new());
        lines.push("The source file was not modified by this operation.".to_string());
        if self.save_as_required {
            lines.push(
                "Save the repaired drawing as a new file before continuing work.".to_string(),
            );
        }
        lines.push(String::new());
        lines.join("\n")
    }

    pub fn suggested_download_name(&self) -> String {
        format!("drawing_recovery_{}.log", safe_stem(&self.report_id))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn persist(&mut self) {
        match write_report(self) {
            Ok(path) => self.log_path = Some(path),
            Err(error) => self.log_error = Some(error),
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn persist(&mut self) {}
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn sha256_file(path: &Path) -> Result<String, String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 128 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex_digest(hasher.finalize()))
}

#[cfg(target_arch = "wasm32")]
pub fn sha256_file(_path: &Path) -> Result<String, String> {
    Err("filesystem hashing is unavailable on this target".to_string())
}

#[cfg(not(target_arch = "wasm32"))]
fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn report_id(source_sha256: Option<&str>, created_unix_seconds: u64) -> String {
    let fingerprint = source_sha256
        .and_then(|value| value.get(..12))
        .unwrap_or("nohash");
    format!("R2-{created_unix_seconds}-{fingerprint}")
}

fn private_file_label(file_name: &str, source_sha256: Option<&str>) -> String {
    let extension = Path::new(file_name)
        .extension()
        .and_then(|value| value.to_str())
        .filter(|value| value.chars().all(|ch| ch.is_ascii_alphanumeric()))
        .map(|value| format!(".{value}"))
        .unwrap_or_default();
    let fingerprint = source_sha256
        .and_then(|value| value.get(..12))
        .unwrap_or("redacted");
    format!("drawing-{fingerprint}{extension}")
}

fn private_path_label(value: &str) -> String {
    let normalized = value.replace('\\', "/");
    let extension = Path::new(&normalized)
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| extension.chars().all(|ch| ch.is_ascii_alphanumeric()))
        .map(|extension| format!(".{extension}"))
        .unwrap_or_default();
    format!("reference-redacted{extension}")
}

fn redact_known_paths(message: &str, source: &Path, references: &[XrefInfo]) -> String {
    let mut redacted = redact_source_path(message, Some(source));
    for reference in references {
        let normalized = reference.path.replace('\\', "/");
        let basename = Path::new(&normalized)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        for candidate in [&reference.path, &normalized, basename] {
            if !candidate.is_empty() {
                redacted = redacted.replace(candidate, "<reference-path>");
            }
        }
        if !reference.name.is_empty() {
            redacted = redacted.replace(&reference.name, "<reference-name>");
        }
    }
    redacted
}

fn redact_source_path(message: &str, source: Option<&Path>) -> String {
    let Some(source) = source else {
        return message.to_string();
    };
    let mut redacted = message.replace(&source.to_string_lossy().to_string(), "<source-path>");
    if let Some(file_name) = source.file_name().and_then(|value| value.to_str()) {
        redacted = redacted.replace(file_name, "<source-file>");
    }
    redacted
}

fn structured_diagnostic_line(diagnostic: &acadrust::ReadDiagnostic) -> String {
    let section = diagnostic.section.as_deref().unwrap_or("-");
    let offset = diagnostic
        .source_offset
        .map(|value| format!("0x{value:X}"))
        .unwrap_or_else(|| "-".to_string());
    let offset_basis = diagnostic.source_offset_basis.as_deref().unwrap_or("-");
    let line = diagnostic
        .source_line
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string());
    let handle = diagnostic
        .record_handle
        .map(|value| format!("0x{value:X}"))
        .unwrap_or_else(|| "-".to_string());
    let record_type = diagnostic
        .record_type
        .as_deref()
        .unwrap_or("-");
    format!(
        "[code={} stage={} section={} offset={} offset-basis={} line={} handle={} type={}] {}",
        diagnostic.code,
        diagnostic.stage,
        section,
        offset,
        offset_basis,
        line,
        handle,
        record_type,
        diagnostic.message
    )
}

fn reader_revision() -> String {
    include_str!("../../Cargo.toml")
        .lines()
        .find(|line| line.trim_start().starts_with("acadrust = { git ="))
        .and_then(|line| line.split("rev = \"").nth(1))
        .and_then(|value| value.split('"').next())
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_string()
}

fn diagnostic_platform() -> String {
    format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH)
}

#[cfg(not(target_arch = "wasm32"))]
fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(target_arch = "wasm32")]
fn unix_seconds() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

fn safe_stem(value: &str) -> String {
    let filtered: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    if filtered.is_empty() {
        "drawing".to_string()
    } else {
        filtered
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn write_report(report: &RecoveryReport) -> Result<PathBuf, String> {
    let file_name = report.suggested_download_name();
    let body = report.log_text();
    let mut errors = Vec::new();

    if let Some(directory) = crate::config::config_dir().map(|path| path.join("recovery_logs")) {
        if let Err(error) = std::fs::create_dir_all(&directory) {
            errors.push(format!("Could not create private recovery-log directory: {error}"));
        } else {
            match write_unique(&directory, &file_name, &body) {
                Ok(path) => return Ok(path),
                Err(error) => errors.push(error),
            }
        }
    }

    Err(if errors.is_empty() {
        "No private recovery-log directory was available".to_string()
    } else {
        errors.join("; ")
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn write_unique(directory: &Path, file_name: &str, body: &str) -> Result<PathBuf, String> {
    use std::io::Write;

    let stem = Path::new(file_name)
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "drawing_recovery".to_string());
    for suffix in 0..100u8 {
        let candidate = if suffix == 0 {
            directory.join(file_name)
        } else {
            directory.join(format!("{stem}_{suffix}.log"))
        };
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&candidate) {
            Ok(mut file) => {
                file.write_all(body.as_bytes())
                    .map_err(|error| format!("Could not write recovery log: {error}"))?;
                return Ok(candidate);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Could not create recovery log: {error}")),
        }
    }
    Err("Could not allocate a unique recovery-log name".to_string())
}

#[cfg(test)]
mod tests {
    use super::{report_id, RecoveryReport, RecoveryStatus};

    #[test]
    fn report_id_includes_timestamp_and_source_fingerprint() {
        assert_eq!(
            report_id(
                Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
                1_777_777_777,
            ),
            "R2-1777777777-0123456789ab"
        );
        assert_eq!(report_id(None, 1_777_777_777), "R2-1777777777-nohash");
    }

    #[test]
    fn failed_report_uses_current_timestamp_in_report_id() {
        let report = RecoveryReport::failed(
            None,
            "invalid.dwg".to_string(),
            42,
            None,
            None,
            "parse".to_string(),
            "invalid drawing".to_string(),
            7,
        );

        assert_eq!(report.status, RecoveryStatus::Failed);
        assert!(report.created_unix_seconds > 0);
        assert_eq!(
            report.report_id,
            report_id(report.source_sha256.as_deref(), report.created_unix_seconds)
        );
    }
}
