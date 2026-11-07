//! IFCCAD filename aliases and context for failed production IFCX reads.
//! Diagnostic inspection never accepts a file.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use document::{Document, Error, Format, Loss};
use serde_json::Value;

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
