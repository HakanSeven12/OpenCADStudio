// XREF resolution — scan a loaded document for external-reference blocks and
// populate them with geometry from the referenced DWG/DXF files.

use acadrust::entities::{Block, BlockEnd};
use acadrust::tables::TableEntry;
use acadrust::types::{Handle, Vector3};
use acadrust::{CadDocument, EntityType};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::path::{Path, PathBuf};

#[cfg(not(target_arch = "wasm32"))]
type SourceFingerprint = crate::io::edit_lock::FileFingerprint;
#[cfg(target_arch = "wasm32")]
type SourceFingerprint = ();

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
    /// XRef is marked Unloaded in the host DWG — we honor that and
    /// skip resolving the external file. The user can re-load via UI.
    #[allow(dead_code)]
    Unloaded,
}

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
    pub read_stats: Option<acadrust::ReadStats>,
    pub source_sha256: Option<String>,
}

/// Scan `doc` for XREF block-records, resolve their paths relative to
/// `base_dir`, and populate each xref block with entities from the
/// referenced file.
///
/// Returns a list of [`XrefInfo`] describing each xref block found, plus the
/// number of corrupt xref entities dropped during merge. Purging happens
/// inline as each xref's entities are merged — xref content is parser output
/// just like the host doc, so it gets the same corrupt-entity guard. Folding
/// it in here avoids a second full-document `entities()` walk after resolve.
pub fn resolve_xrefs(doc: &mut CadDocument, base_dir: &Path) -> (Vec<XrefInfo>, usize) {
    resolve_xrefs_with_progress(doc, base_dir, None)
}

/// Resolve XREFs while reporting completed work units.
///
/// Each reference contributes 1000 parse units and 1000 merge units. Parsing
/// progress comes directly from the DWG reader when available, so one large
/// XREF advances smoothly instead of making the file-open bar appear frozen.
pub fn resolve_xrefs_with_progress(
    doc: &mut CadDocument,
    base_dir: &Path,
    progress: Option<std::sync::Arc<dyn Fn(usize, usize) + Send + Sync>>,
) -> (Vec<XrefInfo>, usize) {
    // Auto-resolve every xref — frustum + LOD culling keep GPU cost bounded.
    let xref_entries: Vec<(String, String, Handle)> = doc
        .block_records
        .iter()
        .filter(|br| (br.flags.is_xref || br.flags.is_xref_overlay) && !br.xref_path.is_empty())
        .map(|br| (br.name.clone(), br.xref_path.clone(), br.handle))
        .collect();
    let xref_count = xref_entries.len();
    let total_units = xref_count.saturating_mul(2000);
    if let Some(progress) = &progress {
        progress(0, total_units);
    }

    // Phase 1 — parse every referenced file in parallel. Each `load_file`
    // reads and decodes an independent DWG/DXF and touches nothing in the host
    // `doc`, so the expensive parse of several large discipline files overlaps
    // instead of running back-to-back. (The merge in phase 2 mutates `doc`, so
    // it stays serial.) `resolve_path` is pure and `base_dir` is shared &-ref.
    use crate::par::prelude::*;
    let parse_units: std::sync::Arc<Vec<std::sync::atomic::AtomicU16>> = std::sync::Arc::new(
        (0..xref_count)
            .map(|_| std::sync::atomic::AtomicU16::new(0))
            .collect(),
    );
    let parsed: Vec<(
        String,
        String,
        Handle,
        Option<PathBuf>,
        Option<Result<acadrust::ReadOutcome, String>>,
        Option<String>,
        Option<SourceFingerprint>,
    )> = xref_entries
        .into_par_iter()
        .enumerate()
        .map(|(xref_index, (block_name, raw_path, br_handle))| {
            let resolved = resolve_path(&raw_path, base_dir);
            #[cfg(not(target_arch = "wasm32"))]
            let initial_fingerprint = resolved.as_ref().and_then(|path| {
                crate::io::edit_lock::FileFingerprint::capture(path).ok()
            });
            #[cfg(target_arch = "wasm32")]
            let initial_fingerprint = None;
            let units = std::sync::Arc::clone(&parse_units);
            let nested_progress = progress.as_ref().map(|progress| {
                let progress = std::sync::Arc::clone(progress);
                let callback: std::sync::Arc<dyn Fn(u16) + Send + Sync> =
                    std::sync::Arc::new(move |value| {
                        units[xref_index]
                            .store(value.min(1000), std::sync::atomic::Ordering::Relaxed);
                        let completed = units
                            .iter()
                            .map(|unit| unit.load(std::sync::atomic::Ordering::Relaxed) as usize)
                            .sum();
                        progress(completed, total_units);
                    });
                callback
            });
            let xref_outcome = resolved
                .as_ref()
                .map(|path| super::load_file_with_progress(path, nested_progress));
            let source_sha256 = resolved.as_ref().and_then(|path| {
                let needs_fingerprint = match &xref_outcome {
                    Some(Ok(outcome)) => {
                        outcome.stats.recovered()
                            || outcome.stats.skipped_source_records > 0
                            || !outcome.stats.stream_completed
                    }
                    Some(Err(_)) => true,
                    None => false,
                };
                if !needs_fingerprint {
                    return None;
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    super::stable_sha256_file(path, initial_fingerprint.as_ref())
                }
                #[cfg(target_arch = "wasm32")]
                {
                    crate::io::recovery::sha256_file(path).ok()
                }
            });
            parse_units[xref_index].store(1000, std::sync::atomic::Ordering::Relaxed);
            if let Some(progress) = &progress {
                let completed = parse_units
                    .iter()
                    .map(|unit| unit.load(std::sync::atomic::Ordering::Relaxed) as usize)
                    .sum();
                progress(completed, total_units);
            }
            (
                block_name,
                raw_path,
                br_handle,
                resolved,
                xref_outcome,
                source_sha256,
                initial_fingerprint,
            )
        })
        .collect();

    // Phase 2 — merge each parsed xref into the host document, in the original
    // block order (par_iter preserves it), so handle allocation is deterministic.
    let mut result = Vec::with_capacity(parsed.len());
    let mut dropped = 0usize;
    #[allow(unused_mut, unused_variables)]
    for (
        merge_index,
        (
            block_name,
            raw_path,
            br_handle,
            resolved,
            xref_outcome,
            mut source_sha256,
            initial_fingerprint,
        ),
    ) in parsed.into_iter().enumerate()
    {
        let (status, diagnostics, read_stats) = match xref_outcome {
            Some(Ok(mut outcome)) => {
                let recovered = outcome.stats.recovered()
                    || outcome.stats.skipped_source_records > 0
                    || !outcome.stats.stream_completed
                    || outcome.document.notifications.iter().any(|item| {
                    item.notification_type == acadrust::notification::NotificationType::Error
                });
                let mut diagnostics: Vec<String> = outcome
                    .document
                    .notifications
                    .iter()
                    .map(ToString::to_string)
                    .collect();
                diagnostics.extend(
                    outcome
                        .stats
                        .diagnostics
                        .iter()
                        .map(|diagnostic| diagnostic.message.clone()),
                );
                let invalid = super::purge_corrupt_entities(&mut outcome.document);
                if invalid > 0 {
                    diagnostics.push(format!(
                        "normal read found {invalid} structurally invalid reference records"
                    ));
                    #[cfg(not(target_arch = "wasm32"))]
                    if source_sha256.is_none() {
                        source_sha256 = resolved.as_ref().and_then(|path| {
                            super::stable_sha256_file(path, initial_fingerprint.as_ref())
                        });
                    }
                }
                if recovered || invalid > 0 {
                    (XrefStatus::Failed, diagnostics, Some(outcome.stats))
                } else {
                ensure_block_entities(doc, &block_name);
                dropped += merge_xref_into_block(
                    doc,
                    &block_name,
                    br_handle,
                    outcome.document,
                );
                    (XrefStatus::Loaded, diagnostics, Some(outcome.stats))
                }
            }
            Some(Err(error)) => (XrefStatus::Failed, vec![error], None),
            None => (XrefStatus::NotFound, Vec::new(), None),
        };

        result.push(XrefInfo {
            name: block_name,
            path: resolved
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or(raw_path),
            status,
            diagnostics,
            read_stats,
            source_sha256,
        });
        if let Some(progress) = &progress {
            progress(
                xref_count
                    .saturating_mul(1000)
                    .saturating_add((merge_index + 1).saturating_mul(1000)),
                total_units,
            );
        }
    }

    (result, dropped)
}

/// Try to build an absolute path from a raw xref path string.
/// Handles absolute paths, relative paths, and Windows-style separators.
fn resolve_path(raw: &str, base_dir: &Path) -> Option<PathBuf> {
    let normalised = raw.replace('\\', "/");
    let p = PathBuf::from(&normalised);

    if p.is_absolute() {
        if p.exists() {
            return Some(p);
        }
        // Fallback: try the filename in base_dir.
        if let Some(fname) = p.file_name() {
            let c = base_dir.join(fname);
            if c.exists() {
                return Some(c);
            }
        }
        return None;
    }

    // Relative path against base_dir.
    let candidate = base_dir.join(&p);
    if candidate.exists() {
        return Some(candidate);
    }

    // Last resort: just the filename.
    if let Some(fname) = p.file_name() {
        let c = base_dir.join(fname);
        if c.exists() {
            return Some(c);
        }
    }

    None
}

/// Make sure `doc` has BLOCK + ENDBLK entities for `block_name`.
/// These are required so renderers can find the block content.
fn ensure_block_entities(doc: &mut CadDocument, block_name: &str) {
    let has_block = doc
        .entities()
        .any(|e| matches!(e, EntityType::Block(b) if b.name == block_name));
    if has_block {
        return;
    }
    let b = Block::new(block_name, Vector3::zero());
    let _ = doc.add_entity(EntityType::Block(b));
    let _ = doc.add_entity(EntityType::BlockEnd(BlockEnd::new()));
}

/// Merge an external-reference document into `doc`'s xref block.
///
/// Copies the xref's model-space entities into the host xref block, AND
/// (crucially for correct rendering) merges the xref's layer / linetype
/// tables and any *nested* block records into the host doc under prefixed
/// names ("{xref_name}|{symbol_name}"). Without this remapping, every
/// xref entity using ByLayer resolves against the host doc's layer table
/// — which doesn't know about the xref's layers — and silently falls
/// back to WHITE / 1 px line weight. AutoCAD's BIND command uses the
/// same naming scheme.
/// Returns the number of corrupt entities skipped during the merge.
fn merge_xref_into_block(
    doc: &mut CadDocument,
    xref_block_name: &str,
    br_handle: Handle,
    xref_doc: CadDocument,
) -> usize {
    let prefix = xref_block_name;

    // Carry the xref's INSUNITS onto the host BlockRecord. Used at INSERT
    // time so the inserted xref scales to the host's units (INSUNITS).
    let src_insunits = xref_doc.header.insertion_units;
    if let Some(br) = doc.block_records.iter_mut().find(|b| b.handle == br_handle) {
        br.units = src_insunits;
    }

    // ── Layers ──────────────────────────────────────────────────────────
    // Prefix every xref layer (including "0"). Entity layer references are
    // remapped below so the resolver finds the merged copy. Host layers
    // (incl. its own "0") are untouched — no collisions.
    let mut layer_map: HashMap<String, String> = HashMap::default();
    for layer in xref_doc.layers.iter() {
        let old = layer.name.clone();
        let new = format!("{}|{}", prefix, old);
        let mut cloned = layer.clone();
        xref_doc.normalize_imported_layer_defaults(&mut cloned);
        cloned.name = new.clone();
        cloned.set_handle(doc.allocate_handle());
        doc.layers.add_or_replace(cloned);
        layer_map.insert(old.to_uppercase(), new);
    }

    // ── Linetypes ───────────────────────────────────────────────────────
    // Skip the three sentinel names — "ByLayer" / "ByBlock" / "Continuous"
    // are magic strings the resolver matches verbatim in both docs.
    let mut linetype_map: HashMap<String, String> = HashMap::default();
    for lt in xref_doc.line_types.iter() {
        let old = lt.name.clone();
        if is_sentinel_linetype(&old) {
            continue;
        }
        let new = format!("{}|{}", prefix, old);
        let mut cloned = lt.clone();
        cloned.name = new.clone();
        cloned.set_handle(doc.allocate_handle());
        doc.line_types.add_or_replace(cloned);
        linetype_map.insert(old.to_uppercase(), new);
    }

    // ── Block records (nested blocks inside the xref) ───────────────────
    // First pass: create each prefixed BR with its host handle + empty
    // entity_handles. Second pass (in the entity loop below) populates
    // them by inserting entities with `owner_handle` set.
    //
    // Tracks (xref-doc BR handle → host BR handle) so entities owned by
    // a nested xref block can be routed to the right host block_record.
    let mut br_handle_map: HashMap<Handle, Handle> = HashMap::default();
    let mut block_name_map: HashMap<String, String> = HashMap::default();
    for br in xref_doc.block_records.iter() {
        // Skip layout block records (*Model_Space, *Paper_Space, *Paper_Space0…)
        // and any further-nested xrefs (we don't recurse into xref-of-xref).
        if br.name.starts_with('*') || br.flags.is_xref || br.flags.is_xref_overlay {
            continue;
        }
        let old = br.name.clone();
        let new = format!("{}|{}", prefix, old);
        let mut cloned = br.clone();
        cloned.name = new.clone();
        cloned.entity_handles.clear();
        cloned.insert_handles.clear();
        // Detach foreign layout pointer — it refers to a Layout in xref_doc
        // we don't import.
        cloned.layout = Handle::NULL;
        let new_h = doc.allocate_handle();
        cloned.set_handle(new_h);
        cloned.block_entity_handle = doc.allocate_handle();
        cloned.block_end_handle = doc.allocate_handle();
        br_handle_map.insert(br.handle, new_h);
        block_name_map.insert(old.to_uppercase(), new);
        doc.block_records.add_or_replace(cloned);
    }

    // ── Entities ────────────────────────────────────────────────────────
    let xref_ms_handle = xref_doc.header.model_space_block_handle;
    // Merge in each block's DISPLAY order: a block record's entity_handles
    // chain is the draw order inside that block, while the flat entity list
    // is stream order — merging in stream order scrambled the xref's own
    // stacking (fills over outlines etc.), since the host ranks draw order
    // by the freshly-allocated handles. Entities missing from every chain
    // keep their stream order at the end.
    let ordered: Vec<Handle> = {
        let mut ordered: Vec<Handle> = Vec::new();
        let mut seen: HashSet<Handle> = HashSet::default();
        for br in xref_doc.block_records.iter() {
            for &h in &br.entity_handles {
                if seen.insert(h) {
                    ordered.push(h);
                }
            }
        }
        for e in xref_doc.entities() {
            let h = e.common().handle;
            if seen.insert(h) {
                ordered.push(h);
            }
        }
        ordered
    };
    let entities: Vec<(Handle, EntityType)> = ordered
        .into_iter()
        .filter_map(|h| xref_doc.get_entity(h).map(|e| (h, e.clone())))
        .filter(|(_, e)| !matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_)))
        .collect();

    // old xref-doc entity handle → merged host handle, for the sortents copy.
    let mut entity_handle_map: HashMap<Handle, Handle> = HashMap::default();
    let mut dropped = 0usize;
    for (old_h, mut entity) in entities {
        // Drop parser-garbage entities (bad normals / vertex counts / inf
        // coords) before they enter the host doc — they trigger huge
        // allocations and unbounded recursion in the wire pipeline.
        if super::is_entity_corrupt(&entity) {
            dropped += 1;
            continue;
        }
        // Remap layer / linetype names so the host's resolver hits the
        // copies we just inserted.
        {
            let c = entity.common_mut();
            if let Some(new_layer) = layer_map.get(&c.layer.to_uppercase()) {
                c.layer = new_layer.clone();
            }
            if !is_sentinel_linetype(&c.linetype) {
                if let Some(new_lt) = linetype_map.get(&c.linetype.to_uppercase()) {
                    c.linetype = new_lt.clone();
                }
            }
        }

        // INSERTs reference blocks by name, not handle — remap.
        if let EntityType::Insert(ins) = &mut entity {
            if let Some(new_name) = block_name_map.get(&ins.block_name.to_uppercase()) {
                ins.block_name = new_name.clone();
            }
        }

        // Route entity to the correct host block record. Entities owned by
        // the xref's model_space land in the host xref block (br_handle);
        // entities owned by one of the nested BRs land in its prefixed
        // counterpart. Paper-space / unknown owners are skipped.
        let old_owner = entity.common().owner_handle;
        let new_owner = if old_owner == xref_ms_handle {
            br_handle
        } else if let Some(&h) = br_handle_map.get(&old_owner) {
            h
        } else {
            continue;
        };
        entity.common_mut().owner_handle = new_owner;
        // Clear the foreign handle so acadrust assigns a new one.
        set_handle(&mut entity, Handle::NULL);
        if let Ok(new_h) = doc.add_entity(entity) {
            entity_handle_map.insert(old_h, new_h);
        }
    }

    // ── Draw-order overrides ────────────────────────────────────────────
    // Re-point the xref's SortEntitiesTables at the merged blocks with the
    // merged entity handles, so an explicit DRAWORDER inside the xref
    // survives the merge.
    {
        use acadrust::objects::{ObjectType, SortEntitiesTable};
        for obj in xref_doc.objects.values() {
            let ObjectType::SortEntitiesTable(t) = obj else {
                continue;
            };
            if t.is_empty() {
                continue;
            }
            let new_block = if t.block_owner_handle == xref_ms_handle {
                br_handle
            } else if let Some(&h) = br_handle_map.get(&t.block_owner_handle) {
                h
            } else {
                continue;
            };
            let mut nt = SortEntitiesTable::new();
            nt.handle = doc.allocate_handle();
            nt.block_owner_handle = new_block;
            for e in t.entries() {
                let Some(&ne) = entity_handle_map.get(&e.entity_handle) else {
                    continue;
                };
                let ns = entity_handle_map
                    .get(&e.sort_handle)
                    .copied()
                    .unwrap_or(e.sort_handle);
                nt.add_entry(ne, ns);
            }
            if !nt.is_empty() {
                doc.objects.insert(nt.handle, ObjectType::SortEntitiesTable(nt));
            }
        }
    }
    dropped
}

fn is_sentinel_linetype(name: &str) -> bool {
    name.eq_ignore_ascii_case("ByLayer")
        || name.eq_ignore_ascii_case("ByBlock")
        || name.eq_ignore_ascii_case("Continuous")
}

/// Set the handle field of any entity variant.
fn set_handle(entity: &mut EntityType, h: Handle) {
    entity.common_mut().handle = h;
}
