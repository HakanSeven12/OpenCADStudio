//! The external references a drawing lists, with the state each one is in.

/// Describes a single external reference found in a document.
#[derive(Debug, Clone)]
pub struct XrefInfo {
    /// Block name (e.g. the filename stem).
    pub name: String,
    /// Resolved file path (or raw path if not found).
    pub path: String,
    pub status: XrefStatus,
    /// Reader diagnostics retained for the recovery report.
    pub diagnostics: Vec<String>,
    pub read_stats: Option<document::ReadStats>,
    pub source_sha256: Option<String>,
    /// The reference loaded a drawing whose FingerprintGUID differs from
    /// the one it held before in this session: another drawing took the
    /// file's place, not a revision of it.
    pub other_drawing: bool,
}

/// Status of an external reference block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XrefStatus {
    /// File was found and loaded successfully.
    Loaded,
    /// File was loaded with recoverable parser errors.
    Recovered,
    /// File was found but could not produce usable drawing data.
    Failed,
    /// File path is set but the file could not be found or read.
    NotFound,
    /// XRef is marked Unloaded in the host DWG — opening honors that and
    /// skips resolving the external file. The user can re-load via UI.
    Unloaded,
}
