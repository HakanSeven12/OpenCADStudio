use crate::notification::NotificationCollection;
use crate::CadDocument;

pub const MAX_READ_DIAGNOSTICS: usize = 1_000;
const MAX_DIAGNOSTIC_MESSAGE_CHARS: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SourceFormat {
    Dwg,
    Dxf,
}

impl std::fmt::Display for SourceFormat {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dwg => formatter.write_str("DWG"),
            Self::Dxf => formatter.write_str("DXF"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ReadStage {
    FileHeader,
    Classes,
    Header,
    Handles,
    Objects,
    RecordStream,
    Section,
}

impl std::fmt::Display for ReadStage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileHeader => formatter.write_str("file-header"),
            Self::Classes => formatter.write_str("classes"),
            Self::Header => formatter.write_str("header"),
            Self::Handles => formatter.write_str("handles"),
            Self::Objects => formatter.write_str("objects"),
            Self::RecordStream => formatter.write_str("record-stream"),
            Self::Section => formatter.write_str("section"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ReadDiagnostic {
    pub code: String,
    pub stage: ReadStage,
    pub section: Option<String>,
    pub source_offset: Option<u64>,
    pub source_offset_basis: Option<String>,
    pub source_line: Option<usize>,
    pub record_handle: Option<u64>,
    pub record_type: Option<String>,
    pub message: String,
}

impl ReadDiagnostic {
    pub fn new(code: impl Into<String>, stage: ReadStage, message: impl Into<String>) -> Self {
        let message = message.into();
        let message = if message.chars().count() > MAX_DIAGNOSTIC_MESSAGE_CHARS {
            let mut shortened: String =
                message.chars().take(MAX_DIAGNOSTIC_MESSAGE_CHARS).collect();
            shortened.push_str("… [message truncated]");
            shortened
        } else {
            message
        };
        Self {
            code: code.into(),
            stage,
            section: None,
            source_offset: None,
            source_offset_basis: None,
            source_line: None,
            record_handle: None,
            record_type: None,
            message,
        }
    }
}

pub fn push_read_diagnostic(diagnostics: &mut Vec<ReadDiagnostic>, diagnostic: ReadDiagnostic) {
    if diagnostics.len() < MAX_READ_DIAGNOSTICS.saturating_sub(1) {
        diagnostics.push(diagnostic);
    } else if diagnostics.len() == MAX_READ_DIAGNOSTICS.saturating_sub(1) {
        diagnostics.push(ReadDiagnostic::new(
            "diagnostic-limit-reached",
            ReadStage::RecordStream,
            "Additional reader diagnostics were omitted after the safety limit",
        ));
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ReadStats {
    pub source_format: Option<SourceFormat>,
    pub source_version: String,
    pub maintenance_version: u8,
    pub recovery_mode: bool,
    pub stream_completed: bool,
    pub source_sections: usize,
    pub source_records: usize,
    pub decoded_source_records: usize,
    pub skipped_source_records: usize,
    pub output_entities: usize,
    pub output_objects: usize,
    pub output_table_records: usize,
    pub recovered_errors: usize,
    pub record_stream_read: bool,
    pub diagnostics: Vec<ReadDiagnostic>,
}

impl ReadStats {
    pub(crate) fn from_document(
        document: &CadDocument,
        source_format: SourceFormat,
        source_sections: usize,
        source_records: usize,
        decoded_source_records: usize,
        skipped_source_records: usize,
        record_stream_read: bool,
        recovery_mode: bool,
        stream_completed: bool,
        diagnostics: Vec<ReadDiagnostic>,
    ) -> Self {
        Self {
            source_format: Some(source_format),
            source_version: format!("{:?}", document.version),
            maintenance_version: document.maintenance_version,
            recovery_mode,
            stream_completed,
            source_sections,
            source_records,
            decoded_source_records,
            skipped_source_records,
            output_entities: document.entities().count(),
            output_objects: document.objects.len(),
            output_table_records: document.layers.iter().count()
                + document.line_types.iter().count()
                + document.text_styles.iter().count()
                + document.block_records.iter().count()
                + document.dim_styles.iter().count()
                + document.app_ids.iter().count()
                + document.views.iter().count()
                + document.vports.iter().count()
                + document.ucss.iter().count()
                + document.vx_table.iter().count(),
            recovered_errors: recovered_error_count(&document.notifications),
            record_stream_read,
            diagnostics,
        }
    }

    pub fn output_records(&self) -> usize {
        self.output_entities
            .saturating_add(self.output_objects)
            .saturating_add(self.output_table_records)
    }

    pub fn has_usable_drawing_data(&self) -> bool {
        self.record_stream_read && self.source_records > 0 && self.decoded_source_records > 0
    }

    pub fn recovered(&self) -> bool {
        self.recovered_errors > 0
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ReadOutcome {
    pub document: CadDocument,
    pub stats: ReadStats,
}

impl ReadOutcome {
    pub fn new(mut document: CadDocument, stats: ReadStats) -> Self {
        super::loft_parameters::restore(&mut document);
        Self { document, stats }
    }
}

fn recovered_error_count(notifications: &NotificationCollection) -> usize {
    notifications.error_count()
}
