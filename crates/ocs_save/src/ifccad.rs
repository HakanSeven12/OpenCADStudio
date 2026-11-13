//! IFCCAD filename aliases and context for failed production IFCX reads.
//! Diagnostic inspection never accepts a file.

use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, Mutex};

use document::{Document, Error, Format, Loss};
use serde_json::Value;

/// State owned by one open tab, shared with that tab's save snapshots.
#[derive(Clone, Debug)]
pub struct IfccadSession(Arc<Mutex<document::IfccadRoundtripContext>>);

impl IfccadSession {
    /// Human-readable notices for preserved content that has no CAD geometry.
    pub fn blocked_entities(&self) -> Vec<(u64, String)> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .blocked_entities()
            .map(|(id, reason)| (id, reason.to_owned()))
            .collect()
    }

    /// Whether this session carries content that ordinary CAD formats cannot retain.
    pub fn has_blocked_entities(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .blocked_entities()
            .next()
            .is_some()
    }

    /// Prevent a save to a format that cannot carry the session's retained roots.
    pub fn check_destination(&self, ext: &str) -> Result<(), String> {
        if super::is_ifccad_extension(ext) || !self.has_blocked_entities() {
            Ok(())
        } else {
            Err("This drawing has retained IFCCAD content. Save it as .ifccad to keep that content.".into())
        }
    }

    pub(super) fn export(
        &self,
        doc: &Document,
        preview: bool,
    ) -> Result<document::IfccadExportOutcome, String> {
        let mut context = self
            .0
            .lock()
            .map_err(|_| "IFCCAD save state is unavailable")?;
        let result = if preview {
            document::Ifcx.export_ifccad_preserving(doc, &mut context.clone())
        } else {
            document::Ifcx.export_ifccad_preserving(doc, &mut context)
        };
        result.map_err(|error| error.to_string())
    }
}

/// Read IFCCAD for a drawing tab. A context is needed while it retains roots.
pub fn read_ifccad_for_edit(
    bytes: &[u8],
) -> Result<(document::ReadOutcome, Option<IfccadSession>), String> {
    let read = || -> Result<_, Error> {
        let file = document::ifccad::load_ifccad_bytes(bytes, Default::default())?;
        document::Ifcx.import_ifccad_preserving(file.document())
    };
    let (outcome, context) = read().map_err(|error| read_error(bytes, &error.to_string()))?;
    let session = Some(IfccadSession(Arc::new(Mutex::new(context))));
    Ok((
        super::external_read_outcome(outcome.document, outcome.losses),
        session,
    ))
}

/// Application filenames; the underlying IFCX codec and file contents stay unchanged.
pub(super) struct IfccadFormat;

impl Format for IfccadFormat {
    fn name(&self) -> &str {
        document::Ifcx.name()
    }

    fn extensions(&self) -> &[&str] {
        &["ifccad", "ifcx"]
    }

    fn read(&self, bytes: &[u8]) -> Result<(Document, Vec<Loss>), Error> {
        document::Ifcx.read(bytes)
    }

    fn write(&self, document: &Document) -> Result<(Vec<u8>, Vec<Loss>), Error> {
        document::Ifcx.write(document)
    }
}

// Ask the pinned writer for its profile instead of maintaining a second version constant.
static SUPPORTED_PROFILE: LazyLock<Option<String>> = LazyLock::new(|| {
    let (bytes, _) = document::Ifcx.write(&Document::new()).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    drawing_values(&value)?
        .values()
        .find_map(|drawing| drawing.get("profileVersion")?.as_str().map(str::to_owned))
});

fn drawing_values(value: &Value) -> Option<BTreeMap<&str, &Value>> {
    let mut drawings = BTreeMap::new();
    for node in value.get("data")?.as_array()? {
        let path = node.get("path")?.as_str()?;
        if let Some(drawing) = node
            .get("attributes")
            .and_then(|a| a.get("ifccad::drawing"))
        {
            // A later fragment replaces the same attribute, as in the production reader.
            drawings.insert(path, drawing);
        }
    }
    Some(drawings)
}

pub(super) fn read_error(bytes: &[u8], detail: &str) -> String {
    let context = serde_json::from_slice::<Value>(bytes).ok().and_then(|value| {
        let syntax = value.get("header")?.get("ifcxVersion")?.as_str()?;
        if syntax != "ifcx_alpha" {
            return Some(format!(
                "This file uses an unsupported IFCX version ({syntax}). This version of OCS supports the experimental IFCX alpha syntax."
            ));
        }
        value.get("imports")?.as_array()?;
        value.get("schemas")?.as_object()?;
        let drawings = drawing_values(&value)?;
        if drawings.is_empty() {
            return Some(
                "This file does not contain an IFCCAD drawing that Open CAD Studio can open. OCS currently supports IFCX files containing CAD drawing data in the experimental IFCCAD profile. Opening BIM models or generating drawings from BIM geometry is not yet supported. To try this feature, save a supported drawing as IFCCAD (.ifccad) and reopen it with the same OCS version."
                    .to_string(),
            );
        }
        if drawings.len() == 1 {
            let version = drawings.values().next()?.get("profileVersion")?.as_str()?;
            if let Some(supported) = SUPPORTED_PROFILE.as_deref() {
                if version != supported {
                    return Some(format!(
                        "This file uses an unsupported IFCCAD profile version ({version}). This version of OCS supports profile {supported}. Alpha file compatibility and migration are not guaranteed. Use the OCS version that created the file, or return to the original drawing."
                    ));
                }
            }
        }
        None
    }).unwrap_or_else(||
        "This IFCX / IFCCAD file could not be read. Its syntax or drawing data may be invalid or unsupported by this version of OCS.".to_string()
    );
    format!("{context}\n\nDetails: {detail}")
}
