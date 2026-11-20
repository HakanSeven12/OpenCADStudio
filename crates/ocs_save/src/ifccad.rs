//! IFCCAD filename aliases and context for failed production IFCX reads.
//! Diagnostic inspection never accepts a file.

use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, Mutex};

use document::{Document, Error, Format, Loss};
use serde_json::Value;

/// State owned by one open tab, shared with that tab's save snapshots.
#[derive(Clone, Debug, Default)]
pub struct IfccadSession(
    Option<Arc<Mutex<document::IfccadRoundtripContext>>>,
    Arc<BTreeMap<document::Handle, document::ifccad::IfccadPlotStyleSource>>,
    Arc<Mutex<BTreeMap<(document::ifccad::IfccadExternalSourceId, String), ExternalSourceBytes>>>,
    Arc<Mutex<BTreeMap<document::ifccad::IfccadExternalSourceId, String>>>,
);

/// Explicit browser choice, scoped to one open drawing and stored locator.
#[derive(Clone, Debug)]
pub struct ExternalSourceBytes {
    pub file_name: String,
    pub bytes: Arc<Vec<u8>>,
}

impl IfccadSession {
    /// Last successfully loaded identity belongs to this tab, even when two
    /// native files share a name/header and allocate the same local source IDs.
    pub fn observe_external_source_guid(
        &self,
        handle: document::Handle,
        guid: &str,
    ) -> Option<bool> {
        let source = self.external_source(handle)?;
        if source.source.format() == document::ifccad::IfccadExternalSourceFormat::Ifccad {
            return Some(false);
        }
        let Some(loaded) = document::ifccad::normalize_ifccad_fingerprint_guid(guid) else {
            return Some(false);
        };
        let fingerprint_guid = source.source.fingerprint_guid().map(str::to_owned);
        let previous = self
            .3
            .lock()
            .ok()?
            .insert(source.id, loaded.clone())
            .or(fingerprint_guid);
        Some(previous.is_some_and(|previous| previous != loaded))
    }
    pub fn external_source_bytes(
        &self,
        handle: document::Handle,
        path: &str,
    ) -> Option<ExternalSourceBytes> {
        let source = self.external_source(handle)?;
        self.2
            .lock()
            .ok()?
            .get(&(source.id, path.to_owned()))
            .cloned()
    }

    pub fn bind_external_source_bytes(
        &self,
        handle: document::Handle,
        path: &str,
        file_name: String,
        bytes: Arc<Vec<u8>>,
    ) -> Result<(), String> {
        let source = self
            .external_source(handle)
            .ok_or("Unknown native external source")?;
        self.2
            .lock()
            .map_err(|_| "External source bytes are unavailable")?
            .insert(
                (source.id, path.to_owned()),
                ExternalSourceBytes { file_name, bytes },
            );
        Ok(())
    }

    pub fn has_native_context(&self) -> bool {
        self.0.is_some()
    }

    pub fn external_source(
        &self,
        handle: document::Handle,
    ) -> Option<document::ifccad::IfccadExternalSource> {
        self.0
            .as_ref()?
            .lock()
            .ok()?
            .external_source(handle)
            .cloned()
    }

    /// Remember source metadata read during an explicit native attachment.
    pub fn register_external_source(
        &self,
        doc: &Document,
        handle: document::Handle,
        content: document::ifccad::IfccadExternalSourceContent,
    ) -> Result<(), String> {
        let Some(context) = &self.0 else {
            return Ok(());
        };
        let record = doc
            .block_records
            .iter()
            .find(|r| r.handle == handle)
            .ok_or("XREF definition is unavailable")?;
        context
            .lock()
            .map_err(|_| "IFCCAD source state is unavailable")?
            .register_external_source(
                handle,
                record.name.clone(),
                record.xref_path.clone(),
                content,
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// A changed source basis/unit needs an explicit placement reassessment.
    /// A changed GUID alone continues through the shared DWG warning policy.
    pub fn check_external_source_reload(
        &self,
        handle: document::Handle,
        content: &document::ifccad::IfccadExternalSourceContent,
    ) -> Result<(), String> {
        let Some(source) = self.external_source(handle) else {
            return Ok(());
        };
        if source.source.format() != content.format()
            || source.source.base_point() != content.base_point()
            || source
                .source
                .insertion_unit()
                .is_some_and(|unit| Some(unit) != content.insertion_unit())
        {
            return Err(format!("XREF {}: source format, base point or insertion unit changed; reassess placement and scale before reloading",source.name));
        }
        Ok(())
    }

    /// Restore external definition metadata in a disposable CAD save copy.
    pub fn prepare_cad_external_sources(&self, doc: &mut Document) -> Result<(), String> {
        let Some(context) = &self.0 else {
            return Ok(());
        };
        let context = context
            .lock()
            .map_err(|_| "IFCCAD source state is unavailable")?;
        let sources = doc
            .block_records
            .iter()
            .filter_map(|r| {
                if !r.flags.is_xref {
                    return None;
                }
                Some(
                    context
                        .external_source_for_path(r.handle, &r.xref_path)
                        .map(|source| {
                            source
                                .map(|s| (r.handle, r.block_entity_handle, r.xref_path.clone(), s))
                        }),
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        for (handle, marker, path, source) in sources.into_iter().flatten() {
            if source.source.format() != document::ifccad::IfccadExternalSourceFormat::Dwg {
                return Err(format!(
                    "XREF {}: convert the external source to DWG before CAD export",
                    source.name
                ));
            }
            let base_point = source.source.base_point();
            let fingerprint_guid = source.source.fingerprint_guid();
            let base = document::Vector3::new(base_point[0], base_point[1], base_point[2]);
            if let Some(record) = doc.block_records.iter_mut().find(|r| r.handle == handle) {
                record.base_point = base;
            }
            if let Some(document::EntityType::Block(block)) = doc.get_entity_mut(marker) {
                block.base_point = base;
            }
            if let Some(guid) = fingerprint_guid {
                let dependency = doc
                    .file_dependencies
                    .iter_mut()
                    .find(|d| d.feature.eq_ignore_ascii_case("Acad:XRef") && d.file_name == path);
                if let Some(dependency) = dependency {
                    dependency.fingerprint_guid =
                        Some(format!("{{{}}}", guid.to_ascii_uppercase()));
                } else {
                    doc.file_dependencies
                        .push(document::acadrust::document::FileDependency {
                            feature: "Acad:XRef".into(),
                            file_name: path,
                            found_path: String::new(),
                            fingerprint_guid: Some(format!("{{{}}}", guid.to_ascii_uppercase())),
                            version_guid: None,
                            timestamp: 0,
                            file_size: 0,
                            affects_graphics: true,
                            reference_count: 1,
                        });
                }
            }
        }
        Ok(())
    }

    pub fn bind_plot_style(
        &mut self,
        layout: document::Handle,
        source: document::ifccad::IfccadPlotStyleSource,
    ) {
        Arc::make_mut(&mut self.1).insert(layout, source);
    }

    pub fn check_cad_plot_style_bindings(&self, doc: &Document) -> Result<(), String> {
        document::Ifcx
            .validate_cad_plot_style_sources(doc.objects.iter().filter_map(|(handle, object)| {
                let document::objects::ObjectType::Layout(layout) = object else {
                    return None;
                };
                let source = self.1.get(handle)?;
                source
                    .file_name
                    .eq_ignore_ascii_case(&layout.plot_style_sheet)
                    .then_some((layout.name.as_str(), source))
            }))
            .map_err(|error| error.to_string())
    }

    /// The external plot table declared by the IFCCAD file for this CAD layout.
    pub fn plot_style_source(
        &self,
        doc: &Document,
        layout_name: &str,
        file_name: &str,
    ) -> Option<&document::ifccad::IfccadPlotStyleSource> {
        let layout = ifccad_plot_layout(doc, layout_name)?;
        self.1
            .get(&layout)
            .filter(|source| source.file_name.eq_ignore_ascii_case(file_name))
    }

    /// Human-readable notices for preserved content that has no CAD geometry.
    pub fn blocked_entities(&self) -> Vec<(u64, String)> {
        let Some(context) = &self.0 else {
            return Vec::new();
        };
        context
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .blocked_entities()
            .map(|(id, reason)| (id, reason.to_owned()))
            .collect()
    }

    /// Whether this session carries content that ordinary CAD formats cannot retain.
    pub fn has_blocked_entities(&self) -> bool {
        self.0.as_ref().is_some_and(|context| {
            context
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .blocked_entities()
                .next()
                .is_some()
        })
    }

    /// Prevent a save to a format that cannot carry the session's retained roots.
    pub fn check_destination(&self, doc: &Document, ext: &str) -> Result<(), String> {
        if matches!(
            ext.to_ascii_lowercase().as_str(),
            "dxf" | "dwg" | "dwt" | "dws"
        ) {
            self.check_cad_plot_style_bindings(doc)?;
            for record in doc.block_records.iter().filter(|r| r.flags.is_xref) {
                if let Some(source) = self.external_source(record.handle) {
                    if source.source.format() != document::ifccad::IfccadExternalSourceFormat::Dwg {
                        return Err(format!(
                            "XREF {}: convert the external source to DWG before CAD export",
                            source.name
                        ));
                    }
                }
            }
            document::validate_cad_external_sources(doc).map_err(|e| e.to_string())?;
        }
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
        let result = if let Some(context) = &self.0 {
            let mut context = context
                .lock()
                .map_err(|_| "IFCCAD save state is unavailable")?;
            if preview {
                document::Ifcx.export_ifccad_preserving(doc, &mut context.clone())
            } else {
                document::Ifcx.export_ifccad_preserving(doc, &mut context)
            }
        } else {
            document::Ifcx.export_ifccad(doc)
        };
        let mut result = result.map_err(|error| error.to_string())?;
        let drawing = &mut result.drawing;
        let layouts = std::iter::once((drawing.model.id, "Model")).chain(
            drawing
                .paper_layouts
                .iter()
                .map(|layout| (layout.id, layout.name.as_str())),
        );
        let styles = &mut drawing.plot_styles;
        for (id, name) in layouts {
            let Some(file_name) = styles
                .layout_sources
                .get(&id)
                .and_then(|source| styles.sources.get(source))
                .map(|source| source.file_name.as_str())
            else {
                continue;
            };
            let Some(source) = self
                .plot_style_source(doc, name, file_name)
                .filter(|source| source.sha256.is_some())
            else {
                continue;
            };
            // Different layouts may declare different bytes under the same file name.
            let source_id = styles
                .sources
                .iter()
                .find_map(|(&id, candidate)| (candidate == source).then_some(id));
            let source_id = source_id.unwrap_or_else(|| {
                let id = styles.next_source_id;
                styles.next_source_id += 1;
                styles.sources.insert(id, source.clone());
                id
            });
            styles.layout_sources.insert(id, source_id);
        }
        let used_sources = styles
            .layout_sources
            .values()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        styles.sources.retain(|id, _| used_sources.contains(id));
        Ok(result)
    }
}

fn ifccad_plot_layout(doc: &Document, name: &str) -> Option<document::Handle> {
    doc.objects
        .iter()
        .find_map(|(handle, object)| match object {
            document::objects::ObjectType::Layout(layout)
                if layout.name.eq_ignore_ascii_case(name) =>
            {
                Some(*handle)
            }
            _ => None,
        })
}

/// Read IFCCAD for a drawing tab. A context is needed while it retains roots.
pub fn read_ifccad_for_edit(
    bytes: &[u8],
) -> Result<(document::ReadOutcome, Option<IfccadSession>), String> {
    let read = || -> Result<_, Error> {
        let file = document::ifccad::load_ifccad_bytes(bytes, Default::default())?;
        let drawing = file.document();
        let (outcome, context) = document::Ifcx.import_ifccad_preserving(drawing)?;
        let layouts = std::iter::once((drawing.model.id, "Model")).chain(
            drawing
                .paper_layouts
                .iter()
                .map(|layout| (layout.id, layout.name.as_str())),
        );
        let sources = layouts
            .filter_map(|(id, name)| {
                let source = drawing.plot_styles.layout_sources.get(&id)?;
                Some((
                    ifccad_plot_layout(&outcome.document, name)?,
                    drawing.plot_styles.sources.get(source)?.clone(),
                ))
            })
            .collect();
        Ok((outcome, context, sources))
    };
    let (outcome, context, sources) =
        read().map_err(|error| read_error(bytes, &error.to_string()))?;
    let session = Some(IfccadSession(
        Some(Arc::new(Mutex::new(context))),
        Arc::new(sources),
        Default::default(),
        Default::default(),
    ));
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
