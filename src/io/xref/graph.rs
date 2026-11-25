//! Finite XREF expansion. Parsed resources are cached independently of occurrences.
use super::*;
use crate::io::xref_model::{child_key, normalize_lexical};
use document::ifccad::IfccadExternalSourceContent;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum XrefResourceId {
    Desktop(String),
    Browser(u64),
}

pub(super) fn desktop_identity(path: &Path) -> XrefResourceId {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    XrefResourceId::Desktop(normalize_lexical(&canonical.to_string_lossy()))
}

#[derive(Clone, Debug)]
pub(super) struct ParsedXrefSource {
    pub document: Document,
    pub context: Option<document::IfccadRoundtripContext>,
    pub content: Option<IfccadExternalSourceContent>,
    pub path: PathBuf,
    pub unavailable: bool,
}

#[derive(Clone, Debug)]
pub(super) struct XrefEdge {
    pub handle: Handle,
    pub name: String,
    pub locator: String,
    pub overlay: bool,
    pub unloaded: bool,
    pub placed: bool,
    pub source: Option<document::ifccad::IfccadExternalSource>,
}

pub(super) trait XrefResourceAccess {
    fn cache_scope(&self, _owner: &ParsedXrefSource, _edge: &XrefEdge) -> String {
        String::new()
    }
    fn remember(
        &mut self,
        _owner: &ParsedXrefSource,
        _edge: &XrefEdge,
        _id: &XrefResourceId,
        _child: &ParsedXrefSource,
    ) {
    }
    fn enter(&mut self, _id: &XrefResourceId, _source: &ParsedXrefSource) {}
    fn resolve(
        &mut self,
        owner: &XrefResourceId,
        source: &ParsedXrefSource,
        edge: &XrefEdge,
    ) -> Result<XrefResourceId, String>;
    fn read(
        &mut self,
        id: &XrefResourceId,
        source: &ParsedXrefSource,
        edge: &XrefEdge,
    ) -> Result<Arc<ParsedXrefSource>, String>;
}

pub(super) struct PreparedXrefClosure {
    pub document: Document,
    pub rows: Vec<ocs_save::ExternalReferenceRow>,
    pub diagnostics: Vec<String>,
}

fn edges(source: &ParsedXrefSource) -> Vec<XrefEdge> {
    // Follow ordinary block INSERTs as well, without treating unused definitions
    // or AutoCAD's cached dependent records as direct placements.
    let mut owners = vec![source.document.header.model_space_block_handle];
    let mut seen = HashSet::default();
    let mut refs = HashSet::default();
    while let Some(owner) = owners.pop() {
        if !seen.insert(owner) {
            continue;
        }
        for entity in source.document.entities().filter(|e| {
            e.common().owner_handle == owner
                || (owner == source.document.header.model_space_block_handle
                    && e.common().owner_handle.is_null())
        }) {
            if let EntityType::Insert(insert) = entity {
                if let Some(record) = source.document.block_records.get(&insert.block_name) {
                    if record.flags.is_xref || record.flags.is_xref_overlay {
                        refs.insert(record.handle);
                    } else {
                        owners.push(record.handle);
                    }
                }
            }
        }
    }
    source
        .document
        .block_records
        .iter()
        .filter(|r| {
            (r.flags.is_xref || r.flags.is_xref_overlay)
                && (refs.contains(&r.handle) || !r.flags.is_external)
        })
        .map(|r| XrefEdge {
            handle: r.handle,
            name: r.name.clone(),
            locator: r.xref_path.clone(),
            overlay: r.flags.is_xref_overlay,
            unloaded: r.flags.is_xref_unloaded,
            placed: refs.contains(&r.handle),
            source: source
                .context
                .as_ref()
                .and_then(|c| c.external_source(r.handle))
                .cloned(),
        })
        .collect()
}

fn empty_display_definition(doc: &mut Document, handle: Handle) {
    let owned: Vec<_> = doc
        .entities()
        .filter(|e| e.common().owner_handle == handle)
        .map(|e| e.common().handle)
        .collect();
    for h in owned {
        doc.remove_entity(h);
    }
    if let Some(r) = doc.block_records.iter_mut().find(|r| r.handle == handle) {
        r.entity_handles.clear();
    }
}

fn display_definition(doc: &mut Document, handle: Handle) {
    if let Some(r) = doc.block_records.iter_mut().find(|r| r.handle == handle) {
        // Only this disposable display projection changes flags. The original
        // parsed resource/context retains the authored external relationship.
        r.flags.is_xref = false;
        r.flags.is_xref_overlay = false;
        r.flags.is_external = true;
    }
}

struct Frame {
    id: XrefResourceId,
    source: Arc<ParsedXrefSource>,
    display: Document,
    edges: std::collections::VecDeque<XrefEdge>,
    key: u64,
    incoming: Option<XrefEdge>,
}

/// Expand one directly referenced source; its own overlays never propagate.
/// `ancestors` contains the open drawing and is separate from the read cache.
pub(super) fn prepare_xref_closure(
    id: XrefResourceId,
    source: Arc<ParsedXrefSource>,
    key: u64,
    ancestors: &[XrefResourceId],
    access: &mut impl XrefResourceAccess,
) -> Result<PreparedXrefClosure, String> {
    let mut rows = Vec::new();
    let mut diagnostics = Vec::new();
    if ancestors.contains(&id) {
        let mut display = source.document.clone();
        let owned: Vec<_> = display.entities().map(|e| e.common().handle).collect();
        for h in owned {
            display.remove_entity(h);
        }
        // No recursively named definitions from the repeated root may escape.
        let names: Vec<_> = display
            .block_records
            .iter()
            .filter(|r| !r.is_layout())
            .map(|r| r.name.clone())
            .collect();
        for name in names {
            let _ = display.block_records.remove(&name);
        }
        return Ok(PreparedXrefClosure {
            document: display,
            rows,
            diagnostics: vec![
                "Circular XREF: direct self-reference interrupted for this session".into(),
            ],
        });
    }
    access.enter(&id, &source);
    let mut active = ancestors.to_vec();
    active.push(id.clone());
    let mut cache = HashMap::default();
    cache.insert((id.clone(), String::new()), Arc::clone(&source));
    let mut stack = vec![Frame {
        id,
        display: source.document.clone(),
        edges: edges(&source).into(),
        source,
        key,
        incoming: None,
    }];
    loop {
        let Some(frame) = stack.last_mut() else {
            return Err("XREF traversal lost its root".into());
        };
        if let Some(edge) = frame.edges.pop_front() {
            empty_display_definition(&mut frame.display, edge.handle);
            display_definition(&mut frame.display, edge.handle);
            if edge.overlay {
                continue;
            }
            let key = child_key(frame.key, &edge.name, &edge.locator);
            let source_id = edge.source.as_ref().map_or(edge.handle.value(), |s| s.id.0);
            let mut row = ocs_save::ExternalReferenceRow {
                key,
                parent_key: frame.key,
                name: edge.name.clone(),
                locator: edge.locator.clone(),
                found_at: None,
                status: "NotFound".into(),
                diagnostic: None,
                overlay: false,
                owner_resource: match frame.id {
                    XrefResourceId::Browser(id) => Some(id),
                    _ => None,
                },
                source_id,
            };
            if !edge.placed {
                row.status = "Unreferenced".into();
                row.found_at = access
                    .resolve(&frame.id, &frame.source, &edge)
                    .ok()
                    .map(|id| match id {
                        XrefResourceId::Desktop(path) => path,
                        XrefResourceId::Browser(id) => format!("resource:{id}"),
                    });
                rows.push(row);
                continue;
            }
            if edge.unloaded {
                row.status = "Unloaded".into();
                rows.push(row);
                continue;
            }
            let child_id = match access.resolve(&frame.id, &frame.source, &edge) {
                Ok(id) => id,
                Err(reason) => {
                    diagnostics.push(format!("XREF {}: {reason}", edge.name));
                    row.diagnostic = Some(reason);
                    rows.push(row);
                    continue;
                }
            };
            row.found_at = Some(match &child_id {
                XrefResourceId::Desktop(path) => path.clone(),
                XrefResourceId::Browser(id) => format!("resource:{id}"),
            });
            if active.contains(&child_id) {
                let reason = format!(
                    "Circular XREF: {} -> {} interrupted for this session",
                    stack
                        .iter()
                        .map(|f| f
                            .source
                            .path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("root"))
                        .collect::<Vec<_>>()
                        .join(" -> "),
                    edge.name
                );
                row.status = "Circular".into();
                row.diagnostic = Some(reason.clone());
                diagnostics.push(reason);
                rows.push(row);
                continue;
            }
            let cache_key = (child_id.clone(), access.cache_scope(&frame.source, &edge));
            let child = if let Some(child) = cache.get(&cache_key) {
                Arc::clone(child)
            } else {
                match access.read(&child_id, &frame.source, &edge) {
                    Ok(child) => {
                        cache.insert(cache_key, Arc::clone(&child));
                        child
                    }
                    Err(error) => return Err(format!("XREF {}: {error}", edge.name)),
                }
            };
            access.remember(&frame.source, &edge, &child_id, &child);
            if let (Some(expected), Some(actual)) = (&edge.source, &child.content) {
                if expected.source.format() != actual.format()
                    || expected.source.base_point() != actual.base_point()
                    || expected
                        .source
                        .insertion_unit()
                        .is_some_and(|unit| Some(unit) != actual.insertion_unit())
                {
                    return Err(format!("XREF {}: source format, base point or insertion unit changed; reassess placement and scale", edge.name));
                }
                if expected
                    .source
                    .fingerprint_guid()
                    .zip(actual.fingerprint_guid())
                    .is_some_and(|(a, b)| a != b)
                {
                    diagnostics.push(format!(
                        "XREF {}: source FingerprintGUID changed",
                        edge.name
                    ));
                }
            }
            row.status = if child.unavailable { "Stale" } else { "Loaded" }.into();
            if child.unavailable {
                let reason = format!(
                    "XREF {}: source unavailable; previous graphics retained",
                    edge.name
                );
                row.diagnostic = Some(reason.clone());
                diagnostics.push(reason);
            }
            rows.push(row);
            access.enter(&child_id, &child);
            active.push(child_id.clone());
            stack.push(Frame {
                id: child_id,
                display: child.document.clone(),
                edges: edges(&child).into(),
                source: child,
                key,
                incoming: Some(edge),
            });
        } else {
            let finished = stack.pop().unwrap();
            active.pop();
            if let Some(parent) = stack.last_mut() {
                let edge = finished.incoming.unwrap();
                merge_xref_display_block(
                    &mut parent.display,
                    &edge.name,
                    edge.handle,
                    finished.display,
                );
                display_definition(&mut parent.display, edge.handle);
            } else {
                return Ok(PreparedXrefClosure {
                    document: finished.display,
                    rows,
                    diagnostics,
                });
            }
        }
    }
}

pub(super) struct ProductionAccess<'a> {
    pub session: Option<&'a ocs_save::IfccadSession>,
    pending: Vec<(String, ocs_save::ExternalDrawingSnapshot)>,
}

pub(super) fn prepare_read(
    host: &Document,
    handle: Handle,
    locator: &str,
    read: &mut XrefRead,
    session: Option<&ocs_save::IfccadSession>,
) -> Result<(), String> {
    let content = read.source_content();
    let Some(Ok(outcome)) = &read.outcome else {
        return Ok(());
    };
    if outcome.stats.recovered()
        || outcome.stats.skipped_source_records > 0
        || !outcome.stats.stream_completed
    {
        return Ok(());
    }
    let path = read
        .resolved
        .clone()
        .unwrap_or_else(|| PathBuf::from(locator));
    let resource = read.resource_id.or_else(|| {
        session
            .and_then(|s| s.external_source_bytes(handle, locator))
            .map(|r| r.resource_id)
    });
    let id = resource.map_or_else(|| desktop_identity(&path), XrefResourceId::Browser);
    let mut ancestors = vec![XrefResourceId::Browser(1)];
    if let Some(path) = &host.source_path {
        ancestors.push(desktop_identity(Path::new(path)));
    }
    read.circular_root = ancestors.contains(&id);
    if !read.circular_root {
        if let (Some(session), Some(content)) = (session, &content) {
            session.check_external_source_reload(handle, content)?;
        }
    }

    if let (Some(resource), Some(context), Some(session)) = (resource, &read.context, session) {
        session.remember_resource_context(resource, context.clone());
    }
    let root = Arc::new(ParsedXrefSource {
        document: outcome.document.clone(),
        context: read.context.clone(),
        content,
        path,
        unavailable: false,
    });
    let mut access = ProductionAccess {
        session,
        pending: Vec::new(),
    };
    let prepared = prepare_xref_closure(id, root, handle.value(), &ancestors, &mut access)?;
    if let Some(session) = session {
        for (key, snapshot) in access.pending {
            session.remember_source_snapshot(key, snapshot);
        }
    }
    if let Some(Ok(outcome)) = &mut read.outcome {
        outcome.document = prepared.document;
    }
    read.nested_rows = prepared.rows;
    read.graph_diagnostics = prepared.diagnostics;
    Ok(())
}

impl XrefResourceAccess for ProductionAccess<'_> {
    fn cache_scope(&self, owner: &ParsedXrefSource, edge: &XrefEdge) -> String {
        resolved_child_path(owner, edge)
            .or_else(|| {
                self.session
                    .and_then(|s| s.source_snapshot(&snapshot_key(owner, edge)))
                    .map(|s| s.lookup_path.clone())
            })
            .and_then(|p| p.parent().map(|p| normalize_lexical(&p.to_string_lossy())))
            .unwrap_or_default()
    }
    fn remember(
        &mut self,
        owner: &ParsedXrefSource,
        edge: &XrefEdge,
        id: &XrefResourceId,
        child: &ParsedXrefSource,
    ) {
        if let XrefResourceId::Desktop(path) = id {
            self.pending.push((
                snapshot_key(owner, edge),
                ocs_save::ExternalDrawingSnapshot {
                    resource_path: path.clone(),
                    lookup_path: child.path.clone(),
                    document: child.document.clone(),
                    context: child.context.clone(),
                    content: child.content.clone(),
                },
            ));
        }
    }
    fn enter(&mut self, id: &XrefResourceId, source: &ParsedXrefSource) {
        if let (XrefResourceId::Browser(resource), Some(session)) = (id, self.session) {
            session.reset_resource_sources(*resource);
            for edge in edges(source) {
                let descriptor =
                    edge.source
                        .unwrap_or_else(|| document::ifccad::IfccadExternalSource {
                            id: document::ifccad::IfccadExternalSourceId(edge.handle.value()),
                            name: edge.name,
                            location: document::external_source_location_from_cad_path(
                                &edge.locator,
                            )
                            .unwrap_or(
                                document::ifccad::IfccadExternalSourceLocation::UriReference {
                                    uri: edge.locator,
                                },
                            ),
                            source: document::ifccad::IfccadExternalSourceContent::Dwg {
                                base_point: source
                                    .document
                                    .block_records
                                    .iter()
                                    .find(|r| r.handle == edge.handle)
                                    .map_or([0.; 3], |r| {
                                        [r.base_point.x, r.base_point.y, r.base_point.z]
                                    }),
                                insertion_unit: None,
                                fingerprint_guid: None,
                            },
                        });
                session.remember_resource_source(*resource, descriptor);
            }
        }
    }
    fn resolve(
        &mut self,
        owner: &XrefResourceId,
        source: &ParsedXrefSource,
        edge: &XrefEdge,
    ) -> Result<XrefResourceId, String> {
        if let (XrefResourceId::Browser(owner), Some(session)) = (owner, self.session) {
            let id = edge.source.as_ref().map_or(edge.handle.value(), |s| s.id.0);
            return session
                .resource_binding(*owner, id, &edge.locator)
                .map(|r| XrefResourceId::Browser(r.resource_id))
                .ok_or_else(|| "Source requires an explicit choice".into());
        }
        let location = document::external_source_location_from_cad_path(&edge.locator)
            .and_then(|l| document::external_source_location_to_cad_path(&l))
            .map_err(|e| e.to_string())?;
        resolve_path(&location, source.path.parent().unwrap_or(Path::new(".")))
            .map(|p| desktop_identity(&p))
            .or_else(|| {
                self.session
                    .and_then(|s| s.source_snapshot(&snapshot_key(source, edge)))
                    .map(|s| XrefResourceId::Desktop(s.resource_path.clone()))
            })
            .ok_or_else(|| "Source not found".into())
    }

    fn read(
        &mut self,
        id: &XrefResourceId,
        source: &ParsedXrefSource,
        edge: &XrefEdge,
    ) -> Result<Arc<ParsedXrefSource>, String> {
        let format = edge.source.as_ref().map(|s| s.source.format());
        let read = match id {
            XrefResourceId::Desktop(path) => {
                let lookup =
                    resolved_child_path(source, edge).unwrap_or_else(|| PathBuf::from(path));
                read_xref_file_for_format(
                    &lookup.to_string_lossy(),
                    source.path.parent().unwrap_or(Path::new(".")),
                    None,
                    format,
                )
            }
            XrefResourceId::Browser(resource) => {
                let session = self.session.ok_or("Source context unavailable")?;
                let bytes = session
                    .source_resource_bytes(*resource)
                    .ok_or("Source bytes unavailable")?;
                read_source_bytes_for_format(
                    &bytes.file_name,
                    &bytes.bytes,
                    format.unwrap_or_else(|| source_read::inferred_format(&bytes.bytes)),
                    None,
                )
            }
        };
        let content = read.source_content();
        if read.outcome.is_none() {
            if let Some(snapshot) = self
                .session
                .and_then(|s| s.source_snapshot(&snapshot_key(source, edge)))
            {
                return Ok(Arc::new(ParsedXrefSource {
                    document: snapshot.document.clone(),
                    context: snapshot.context.clone(),
                    content: snapshot.content.clone(),
                    path: snapshot.lookup_path.clone(),
                    unavailable: true,
                }));
            }
        }
        let outcome = read.outcome.ok_or("Source not found")??;
        if outcome.stats.recovered()
            || outcome.stats.skipped_source_records > 0
            || !outcome.stats.stream_completed
        {
            return Err("Source has incomplete drawing records".into());
        }
        if let (XrefResourceId::Browser(resource), Some(context), Some(session)) =
            (id, &read.context, self.session)
        {
            session.remember_resource_context(*resource, context.clone());
        }
        Ok(Arc::new(ParsedXrefSource {
            document: outcome.document,
            context: read.context,
            content,
            path: read.resolved.unwrap_or_else(|| PathBuf::from(".")),
            unavailable: false,
        }))
    }
}

fn snapshot_key(source: &ParsedXrefSource, edge: &XrefEdge) -> String {
    format!(
        "{:?}|{}|{}|{}",
        desktop_identity(&source.path),
        normalize_lexical(
            &source
                .path
                .parent()
                .unwrap_or(Path::new("."))
                .to_string_lossy()
        ),
        edge.source.as_ref().map_or(edge.handle.value(), |s| s.id.0),
        edge.locator
    )
}

fn resolved_child_path(source: &ParsedXrefSource, edge: &XrefEdge) -> Option<PathBuf> {
    let path = document::external_source_location_from_cad_path(&edge.locator)
        .and_then(|l| document::external_source_location_to_cad_path(&l))
        .ok()?;
    resolve_path(&path, source.path.parent().unwrap_or(Path::new(".")))
}
