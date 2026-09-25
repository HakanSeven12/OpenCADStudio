// XREF resolution — scan a loaded document for external-reference blocks and
// populate them with geometry from the referenced DWG/DXF files.

use codec::entities::{Block, BlockEnd};
use codec::tables::TableEntry;
use codec::types::{Handle, Vector3};
use codec::{CadDocument, EntityType};
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
    pub read_stats: Option<codec::ReadStats>,
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
    resolve_xrefs_with_filter(doc, base_dir, None, None)
}

/// Reload only the direct xref block records identified by `keys`.
///
/// Unlike [`resolve_xrefs`], this never touches unmatched references.  This is
/// the operation behind `XREF Reload <name>`: a targeted reload must not
/// silently reload (or alter the stale baseline of) every other reference.
pub fn resolve_xrefs_for_keys(
    doc: &mut CadDocument,
    base_dir: &Path,
    keys: &HashSet<Handle>,
) -> (Vec<XrefInfo>, usize) {
    resolve_xrefs_with_filter(doc, base_dir, None, Some(keys))
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
    resolve_xrefs_with_filter(doc, base_dir, progress, None)
}

fn resolve_xrefs_with_filter(
    doc: &mut CadDocument,
    base_dir: &Path,
    progress: Option<std::sync::Arc<dyn Fn(usize, usize) + Send + Sync>>,
    only: Option<&HashSet<Handle>>,
) -> (Vec<XrefInfo>, usize) {
    // Auto-resolve every xref — frustum + LOD culling keep GPU cost bounded.
    let xref_entries: Vec<(String, String, Handle)> = doc
        .block_records
        .iter()
        .filter(|br| (br.flags.is_xref || br.flags.is_xref_overlay) && !br.xref_path.is_empty())
        .filter(|br| only.is_none_or(|keys| keys.contains(&br.handle)))
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
        Option<Result<codec::ReadOutcome, String>>,
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
                    item.notification_type == codec::notification::NotificationType::Error
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
                // Reload is replacement, never append. Keep the prior merge
                // intact until parsing succeeded, then discard its generated
                // entities, sortents, and dependent symbols immediately before
                // importing the replacement content.
                //
                // VISRETAIN=1 (`header.retain_xref_visibility`): host-side
                // layer overrides survive the purge — snapshot them first and
                // re-apply onto the freshly merged layers below. Without this
                // every reload silently reset frozen/off/color tweaks the
                // user made on `name|*` layers back to file defaults.
                let visretain = doc.header.retain_xref_visibility;
                let kept_layers = if visretain {
                    snapshot_dependent_layers(doc, &block_name)
                } else {
                    HashMap::default()
                };
                let _ = unload_reference(doc, br_handle.value());
                remove_pipe_symbols(doc, &block_name);
                ensure_block_entities(doc, &block_name);
                dropped += merge_xref_into_block(
                    doc,
                    &block_name,
                    br_handle,
                    outcome.document,
                );
                if visretain {
                    restore_dependent_layers(doc, &block_name, kept_layers);
                }
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

/// Underlay files named by a relative (or moved) path: read them from where
/// the drawing is and register the bytes under the stored path, which is
/// what the underlay's definition names.
#[cfg(not(target_arch = "wasm32"))]
pub fn register_underlay_sources(doc: &CadDocument, base_dir: &Path) {
    for object in doc.objects.values() {
        let codec::objects::ObjectType::UnderlayDefinition(def) = object else {
            continue;
        };
        let stored = def.file_path.as_str();
        if stored.is_empty() || Path::new(stored).is_absolute() && Path::new(stored).exists() {
            continue;
        }
        if let Some(bytes) = resolve_path(stored, base_dir).and_then(|p| std::fs::read(p).ok()) {
            crate::scene::model::pdf_raster::register_source(stored, std::sync::Arc::new(bytes));
        }
    }
}

/// Layer properties an override can change, as compared for the
/// "Layer property overrides" row.
#[derive(Clone, PartialEq)]
struct LayerLook {
    color: codec::types::Color,
    line_type: String,
    line_weight: codec::types::LineWeight,
    off: bool,
    frozen: bool,
    locked: bool,
    plottable: bool,
}

impl LayerLook {
    fn of(layer: &codec::tables::Layer) -> Self {
        Self {
            color: layer.color.clone(),
            line_type: layer.line_type.to_uppercase(),
            line_weight: layer.line_weight,
            off: layer.flags.off,
            frozen: layer.flags.frozen,
            locked: layer.flags.locked,
            plottable: layer.is_plottable,
        }
    }
}

/// The source file's layers by upper-case name, read once per file version.
fn source_layers(path: &Path) -> Option<HashMap<String, LayerLook>> {
    type Cache = std::sync::Mutex<HashMap<PathBuf, (std::time::SystemTime, HashMap<String, LayerLook>)>>;
    static CACHE: std::sync::OnceLock<Cache> = std::sync::OnceLock::new();
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    let cache = CACHE.get_or_init(Default::default);
    if let Some((stamp, layers)) = cache.lock().ok()?.get(path) {
        if *stamp == modified {
            return Some(layers.clone());
        }
    }
    let doc = super::load_file(path).ok()?;
    let layers: HashMap<String, LayerLook> = doc
        .layers
        .iter()
        .map(|layer| (layer.name.to_uppercase(), LayerLook::of(layer)))
        .collect();
    cache
        .lock()
        .ok()?
        .insert(path.to_path_buf(), (modified, layers.clone()));
    Some(layers)
}

/// Whether any of reference `name`'s layers was changed in the host — the
/// Properties "Layer property overrides" row. Compares the host's dependent
/// layers with the source file's own.
pub fn layer_overrides(doc: &CadDocument, name: &str, raw_path: &str, base_dir: &Path) -> bool {
    let Some(source) = resolve_path(raw_path, base_dir).and_then(|p| source_layers(&p)) else {
        return false;
    };
    let prefix = format!("{}|", name.to_uppercase());
    doc.layers.iter().any(|layer| {
        let upper = layer.name.to_uppercase();
        let Some(own) = upper.strip_prefix(&prefix) else {
            return false;
        };
        let Some(original) = source.get(own) else {
            return false;
        };
        let mut here = LayerLook::of(layer);
        // Dependent linetypes carry the reference prefix.
        if let Some(stripped) = here.line_type.strip_prefix(&prefix) {
            here.line_type = stripped.to_string();
        }
        here != *original
    })
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

/// Give a block record its BLOCK / ENDBLK markers, linked through the
/// record's marker handles. The DWG writer emits the markers the record
/// points at; a record left pointing at a null handle is written with
/// handle 0, which makes the whole drawing unreadable elsewhere.
pub(crate) fn ensure_block_entities(doc: &mut CadDocument, block_name: &str) {
    let Some((owner, begin, end, path)) = doc.block_records.get(block_name).map(|br| {
        (
            br.handle,
            br.block_entity_handle,
            br.block_end_handle,
            br.xref_path.clone(),
        )
    }) else {
        return;
    };
    if begin.is_null() || !matches!(doc.get_entity(begin), Some(EntityType::Block(_))) {
        let handle = doc.allocate_handle();
        let mut block = Block::new(block_name, Vector3::zero());
        if !path.is_empty() {
            block = block.with_xref_path(&path);
        }
        block.common.handle = handle;
        block.common.owner_handle = owner;
        let _ = doc.add_entity(EntityType::Block(block));
        if let Some(br) = doc.block_records.get_mut(block_name) {
            br.block_entity_handle = handle;
        }
    }
    if end.is_null() || !matches!(doc.get_entity(end), Some(EntityType::BlockEnd(_))) {
        let handle = doc.allocate_handle();
        let mut block_end = BlockEnd::new();
        block_end.common.handle = handle;
        block_end.common.owner_handle = owner;
        let _ = doc.add_entity(EntityType::BlockEnd(block_end));
        if let Some(br) = doc.block_records.get_mut(block_name) {
            br.block_end_handle = handle;
        }
    }
}

/// Shared symbol-remap core for XREF load-merge and BIND (SPIKE3 REFACTOR
/// verdict: extract, don't duplicate).
///
/// Both paths import the source doc's symbol tables into the host and rewrite
/// every entity reference to the imported copies. They differ only in naming:
/// the load path prefixes `{xref}|{sym}` (collisions overwrite), BIND renames
/// `{parent}$N${sym}` (N bumped past collisions).
///
/// `bind_limitations` — symbol references this helper can NOT remap (counted
/// as unremapped; BIND reports the count per bind, the load path ignores it):
/// - MLINE `style_name`/`style_handle`: no host MLineStyle-table import —
///   the handle is nulled, the name kept verbatim.
/// - Material / plotstyle / full-face-edge visual-style / color-book handles
///   (`EntityCommon` + ACIS `material_handle` bindings on `Solid3D`): the
///   target objects are never imported — handles nulled.
/// - Tolerance `dimension_style_handle`, Shape `style_handle`, MultiLeader
///   `style_handle`/`text_style_handle`, Table `table_style_handle` and an
///   unmapped `block_record_handle`: nulled.
/// - Kept verbatim (audited, benign): linetype handles (the name carries the
///   lookup), Leader `annotation_handle`, Hatch `boundary_handles`,
///   `attdef_handle`, reactors, xdata, extension dictionaries, graphic data,
///   Dimension anonymous-block names (`*D…` — geometry regenerates from the
///   style), Hatch pattern names (no pattern table exists), Shape style
///   names, MultiLeader line-type/arrowhead handles, Table cell internals
///   (rows/cells keep xref text-style names), Viewport visual-style handles,
///   SectionSymbol style handles, `Extended` entities.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum SymbolKind {
    Layer,
    Linetype,
    TextStyle,
    DimStyle,
    Block,
}

/// Per-kind collision sets for BIND naming, preloaded from the host doc
/// (uppercased — table lookup is case-insensitive).
#[derive(Debug, Default)]
struct BindTaken {
    layers: HashSet<String>,
    linetypes: HashSet<String>,
    text_styles: HashSet<String>,
    dim_styles: HashSet<String>,
    blocks: HashSet<String>,
}

impl BindTaken {
    fn capture(doc: &CadDocument) -> Self {
        let upper_names =
            |names: Vec<String>| names.into_iter().map(|n| n.to_uppercase()).collect();
        Self {
            layers: upper_names(doc.layers.names().map(|s| s.to_string()).collect()),
            linetypes: upper_names(doc.line_types.names().map(|s| s.to_string()).collect()),
            text_styles: upper_names(
                doc.text_styles.names().map(|s| s.to_string()).collect(),
            ),
            dim_styles: upper_names(doc.dim_styles.names().map(|s| s.to_string()).collect()),
            blocks: upper_names(
                doc.block_records
                    .iter()
                    .map(|b| b.name.clone())
                    .collect(),
            ),
        }
    }

    fn set(&mut self, kind: SymbolKind) -> &mut HashSet<String> {
        match kind {
            SymbolKind::Layer => &mut self.layers,
            SymbolKind::Linetype => &mut self.linetypes,
            SymbolKind::TextStyle => &mut self.text_styles,
            SymbolKind::DimStyle => &mut self.dim_styles,
            SymbolKind::Block => &mut self.blocks,
        }
    }

    /// Claim `{parent}$N${sym}`, bumping N past every taken name, and record
    /// the winner so sequential imports compose transitive chains like
    /// `PLAN$0$DETAIL$0$WALLS`.
    fn claim(&mut self, kind: SymbolKind, parent: &str, sym: &str) -> String {
        let mut n = 0u32;
        loop {
            let candidate = format!("{parent}${n}${sym}");
            if self.set(kind).insert(candidate.to_uppercase()) {
                return candidate;
            }
            n += 1;
        }
    }
}

/// How imported symbol names are formed.
enum SymbolNaming {
    /// Load path: `{prefix}|{sym}` (collisions overwrite via add_or_replace).
    MergePipe { prefix: String },
    /// BIND path: `{parent}$N${sym}` with per-kind collision dedup.
    BindDollar { parent: String, taken: BindTaken },
}

impl SymbolNaming {
    fn name_for(&mut self, kind: SymbolKind, sym: &str) -> String {
        match self {
            SymbolNaming::MergePipe { prefix } => format!("{prefix}|{sym}"),
            SymbolNaming::BindDollar { parent, taken } => taken.claim(kind, parent, sym),
        }
    }
}

/// Imported-name maps for one source document (old UPPER name → new name,
/// old handle → new handle).
#[derive(Default)]
struct XrefSymbolMaps {
    layers: HashMap<String, String>,
    linetypes: HashMap<String, String>,
    text_styles: HashMap<String, String>,
    text_style_handles: HashMap<Handle, Handle>,
    dim_styles: HashMap<String, String>,
    blocks: HashMap<String, String>,
    br_handles: HashMap<Handle, Handle>,
    image_defs: HashMap<Handle, Handle>,
    underlay_defs: HashMap<Handle, Handle>,
}

/// Import a source doc's symbol tables + IMAGE/UNDERLAY definitions into the
/// host doc under `naming`, returning the old→new maps for entity remap.
///
/// Covers layers (incl. "0"), linetypes (minus sentinels), text styles,
/// dim styles, nested block records (layout `*` records and nested xrefs
/// skipped — BIND resolves those before merging), and every image/underlay
/// definition (fresh handle, detached owner, verbatim path — BIND retargets
/// image paths via COLLECT before merging).
fn import_xref_symbols(
    doc: &mut CadDocument,
    xref_doc: &CadDocument,
    naming: &mut SymbolNaming,
) -> XrefSymbolMaps {
    let mut maps = XrefSymbolMaps::default();

    // A layer's plot style and material point at objects of the file it came
    // from; carried over verbatim they point at whatever the host keeps under
    // those handles. Take the host's defaults (its layer 0) instead.
    // ponytail: every imported layer gets the default plot style and material;
    // map them by name if a reference ever carries non-default ones.
    let (host_plotstyle, host_material) = doc
        .layers
        .get("0")
        .map(|layer| (layer.plotstyle_handle, layer.material))
        .unwrap_or((Handle::NULL, Handle::NULL));

    for layer in xref_doc.layers.iter() {
        let old = layer.name.clone();
        let new = naming.name_for(SymbolKind::Layer, &old);
        let mut cloned = layer.clone();
        cloned.name = new.clone();
        cloned.plotstyle_handle = host_plotstyle;
        cloned.material = host_material;
        cloned.set_handle(doc.allocate_handle());
        doc.layers.add_or_replace(cloned);
        maps.layers.insert(old.to_uppercase(), new);
    }

    for ts in xref_doc.text_styles.iter() {
        // A shape-file style has no name of its own; the host's style for the
        // same shape file serves the imported linetypes.
        if ts.is_shape_file {
            let same_file = doc
                .text_styles
                .iter()
                .find(|host| host.is_shape_file && host.font_file.eq_ignore_ascii_case(&ts.font_file))
                .map(|host| host.handle);
            if let Some(host_handle) = same_file {
                maps.text_style_handles.insert(ts.handle, host_handle);
                continue;
            }
        }
        let old = ts.name.clone();
        let new = naming.name_for(SymbolKind::TextStyle, &old);
        let mut cloned = ts.clone();
        cloned.name = new.clone();
        let new_handle = doc.allocate_handle();
        cloned.set_handle(new_handle);
        cloned.xref_dependent = false;
        doc.text_styles.add_or_replace(cloned);
        maps.text_style_handles.insert(ts.handle, new_handle);
        maps.text_styles.insert(old.to_uppercase(), new);
    }

    for lt in xref_doc.line_types.iter() {
        let old = lt.name.clone();
        if is_sentinel_linetype(&old) {
            continue;
        }
        let new = naming.name_for(SymbolKind::Linetype, &old);
        let mut cloned = lt.clone();
        cloned.name = new.clone();
        // Shape and text segments name their style by handle: point them at
        // the imported copy, not at the source file's record.
        for element in &mut cloned.elements {
            if let Some(complex) = element.complex.as_mut() {
                if let Some(new_style) = maps.text_style_handles.get(&complex.style_handle) {
                    complex.style_handle = *new_style;
                }
            }
        }
        cloned.set_handle(doc.allocate_handle());
        doc.line_types.add_or_replace(cloned);
        maps.linetypes.insert(old.to_uppercase(), new);
    }

    for ds in xref_doc.dim_styles.iter() {
        let old = ds.name.clone();
        let new = naming.name_for(SymbolKind::DimStyle, &old);
        let mut cloned = ds.clone();
        cloned.name = new.clone();
        cloned.set_handle(doc.allocate_handle());
        cloned.xref_reference = false;
        cloned.xref_resolved = false;
        cloned.xref_dependent = false;
        cloned.xref_handle = Handle::NULL;
        doc.dim_styles.add_or_replace(cloned);
        maps.dim_styles.insert(old.to_uppercase(), new);
    }

    // Nested block records: prefixed clones with cleared membership, detached
    // layout pointer, fresh handles (same shape as the load path always made).
    for br in xref_doc.block_records.iter() {
        if br.name.starts_with('*') || br.flags.is_xref || br.flags.is_xref_overlay {
            continue;
        }
        let old = br.name.clone();
        let new = naming.name_for(SymbolKind::Block, &old);
        let mut cloned = br.clone();
        cloned.name = new.clone();
        cloned.entity_handles.clear();
        cloned.insert_handles.clear();
        cloned.layout = Handle::NULL;
        let new_h = doc.allocate_handle();
        cloned.set_handle(new_h);
        cloned.block_entity_handle = doc.allocate_handle();
        cloned.block_end_handle = doc.allocate_handle();
        maps.br_handles.insert(br.handle, new_h);
        maps.blocks.insert(old.to_uppercase(), new);
        doc.block_records.add_or_replace(cloned);
    }

    // IMAGE / UNDERLAY definitions: the entities reference these by handle,
    // so without an import every bound image loses its file link. Fresh
    // handle, detached owner; the path stays verbatim here.
    {
        use codec::objects::ObjectType;
        for (handle, obj) in xref_doc.objects.iter() {
            match obj {
                ObjectType::ImageDefinition(def) => {
                    let mut cloned = def.clone();
                    let new_h = doc.allocate_handle();
                    cloned.handle = new_h;
                    cloned.owner = Handle::NULL;
                    doc.objects
                        .insert(new_h, ObjectType::ImageDefinition(cloned));
                    maps.image_defs.insert(*handle, new_h);
                }
                ObjectType::UnderlayDefinition(def) => {
                    let mut cloned = def.clone();
                    let new_h = doc.allocate_handle();
                    cloned.handle = new_h;
                    cloned.owner_handle = Handle::NULL;
                    cloned.reactors.clear();
                    doc.objects
                        .insert(new_h, ObjectType::UnderlayDefinition(cloned));
                    maps.underlay_defs.insert(*handle, new_h);
                }
                _ => {}
            }
        }
    }

    maps
}

/// Null one optional foreign handle, counting it as unremapped when set.
/// Returns 1 when a handle was cleared, 0 otherwise.
fn null_handle(handle: &mut Option<Handle>) -> usize {
    if handle.is_some() {
        *handle = None;
        1
    } else {
        0
    }
}

/// Rewrite one entity's symbol references to the imported copies in place.
/// Returns the count of unremappable foreign handles (see
/// `bind_limitations` above) — ignored on the load path, reported per BIND.
fn remap_xref_entity(
    host: &CadDocument,
    entity: &mut EntityType,
    maps: &XrefSymbolMaps,
) -> usize {
    let mut unremapped = 0usize;
    let rename = |map: &HashMap<String, String>, name: &mut String| {
        if let Some(new) = map.get(&name.to_uppercase()) {
            *name = new.clone();
        }
    };

    // ── Common layer / linetype + material-style handles ──
    {
        let c = entity.common_mut();
        rename(&maps.layers, &mut c.layer);
        if !is_sentinel_linetype(&c.linetype) {
            rename(&maps.linetypes, &mut c.linetype);
        }
        unremapped += null_handle(&mut c.material_handle);
        unremapped += null_handle(&mut c.plotstyle_handle);
        unremapped += null_handle(&mut c.full_visual_style_handle);
        unremapped += null_handle(&mut c.face_visual_style_handle);
        unremapped += null_handle(&mut c.edge_visual_style_handle);
        unremapped += null_handle(&mut c.color_book_handle);
    }

    // ── Text-style names (incl. embedded MText in attributes) ──
    let remap_mtext_style = |mtext: &mut codec::entities::MText, maps: &XrefSymbolMaps| {
        rename(&maps.text_styles, &mut mtext.style);
    };
    match entity {
        EntityType::Text(t) => rename(&maps.text_styles, &mut t.style),
        EntityType::MText(m) => remap_mtext_style(m, maps),
        EntityType::AttributeDefinition(a) => {
            rename(&maps.text_styles, &mut a.text_style);
            if let Some(m) = a.embedded_mtext.as_mut() {
                remap_mtext_style(m, maps);
            }
        }
        EntityType::AttributeEntity(a) => {
            rename(&maps.text_styles, &mut a.text_style);
            if let Some(m) = a.embedded_mtext.as_mut() {
                remap_mtext_style(m, maps);
            }
        }
        // ── Dim-style names (+ anonymous-block refs stay verbatim) ──
        EntityType::Dimension(d) => rename(&maps.dim_styles, &mut d.base_mut().style_name),
        EntityType::Leader(l) => rename(&maps.dim_styles, &mut l.dimension_style),
        EntityType::Tolerance(t) => {
            rename(&maps.dim_styles, &mut t.dimension_style_name);
            unremapped += null_handle(&mut t.dimension_style_handle);
        }
        // ── Block refs by name ──
        EntityType::Insert(ins) => rename(&maps.blocks, &mut ins.block_name),
        // ── Table: style handles nulled, legacy block refs remapped ──
        EntityType::Table(t) => {
            unremapped += null_handle(&mut t.table_style_handle);
            if let Some(br) = t.block_record_handle {
                match maps.br_handles.get(&br) {
                    Some(&new_br) => t.block_record_handle = Some(new_br),
                    None => {
                        t.block_record_handle = None;
                        unremapped += 1;
                    }
                }
            }
            rename(&maps.blocks, &mut t.block_name);
        }
        // ── MultiLeader / MLine / Shape style handles (names verbatim) ──
        EntityType::MultiLeader(m) => {
            unremapped += null_handle(&mut m.style_handle);
            unremapped += null_handle(&mut m.text_style_handle);
        }
        EntityType::MLine(m) => {
            unremapped += null_handle(&mut m.style_handle);
        }
        EntityType::Shape(s) => {
            unremapped += null_handle(&mut s.style_handle);
        }
        EntityType::Solid3D(s) => {
            for m in &mut s.acis_data.materials {
                unremapped += null_handle(&mut m.material_handle);
            }
        }
        // ── IMAGE / UNDERLAY definition links ──
        EntityType::RasterImage(img) => {
            if let Some(old) = img.definition_handle {
                if let Some(&new) = maps.image_defs.get(&old) {
                    img.definition_handle = Some(new);
                    // Keep the entity path in sync with its definition (bind
                    // COLLECT retargets the definition before merging).
                    use codec::objects::ObjectType;
                    if let Some(ObjectType::ImageDefinition(def)) = host.objects.get(&new) {
                        img.file_path = def.file_name.clone();
                    }
                }
            }
            img.definition_reactor_handle = None;
        }
        EntityType::Underlay(ul) => {
            if let Some(&new) = maps.underlay_defs.get(&ul.definition_handle) {
                ul.definition_handle = new;
            }
        }
        _ => {}
    }
    unremapped
}

/// Re-point a source doc's draw-order tables at the merged blocks/handles, so
/// an explicit DRAWORDER inside the source survives the merge.
fn copy_xref_sortents(
    doc: &mut CadDocument,
    xref_doc: &CadDocument,
    target_owner: Handle,
    br_handle_map: &HashMap<Handle, Handle>,
    entity_handle_map: &HashMap<Handle, Handle>,
) {
    use codec::objects::{ObjectType, SortEntitiesTable};
    let xref_ms_handle = xref_doc.header.model_space_block_handle;
    for obj in xref_doc.objects.values() {
        let ObjectType::SortEntitiesTable(t) = obj else {
            continue;
        };
        if t.is_empty() {
            continue;
        }
        let new_block = if t.block_owner_handle == xref_ms_handle {
            target_owner
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

    // ── Symbol tables + definitions (shared helper) ─────────────────────
    // Same prefixed scheme as before ("{xref}|{sym}"); the helper additionally
    // imports text styles, dim styles, and IMAGE/UNDERLAY definitions.
    let mut naming = SymbolNaming::MergePipe {
        prefix: prefix.to_string(),
    };
    let maps = import_xref_symbols(doc, &xref_doc, &mut naming);
    // Dependent layers and linetypes carry the xref flag and their owning
    // reference, the way the file stores them; without it the host writes
    // plain records with a "|" in the name, which other readers reject.
    for name in maps.layers.values() {
        if let Some(layer) = doc.layers.get_mut(name) {
            layer.flags.xref_dependent = true;
            layer.xref_block_record_handle = br_handle;
        }
    }
    for name in maps.linetypes.values() {
        if let Some(linetype) = doc.line_types.get_mut(name) {
            linetype.xref_dependent = true;
            linetype.xref_block_record_handle = br_handle;
        }
    }

    // ── Entities (shared helper) ────────────────────────────────────────
    // The unremapped-handle count is ignored on the load path (BIND reports
    // it per bind).
    let (dropped, _, entity_handle_map) =
        merge_source_entities(doc, &xref_doc, br_handle, &maps);

    // ── Draw-order overrides (shared helper) ──────────────────────────────
    copy_xref_sortents(doc, &xref_doc, br_handle, &maps.br_handles, &entity_handle_map);
    dropped
}

/// Source entities in display order: each block record's `entity_handles`
/// chain is the draw order inside that block, while the flat entity list is
/// stream order — merging in stream order scrambles the source's own stacking
/// (fills over outlines etc.), since the host ranks draw order by the
/// freshly-allocated handles. Entities missing from every chain keep their
/// stream order at the end. BLOCK/BlockEnd markers are excluded.
fn ordered_source_entities(xref_doc: &CadDocument) -> Vec<(Handle, EntityType)> {
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
        .into_iter()
        .filter_map(|h| xref_doc.get_entity(h).map(|e| (h, e.clone())))
        .filter(|(_, e)| !matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_)))
        .collect()
}

/// Merge one source doc's entities into `target`, routing model-space content
/// into `target_br` and nested-block content into its imported counterpart
/// (`maps.br_handles`). Returns corrupt-drop count, unremapped-handle count,
/// and the old→new entity handle map for the sortents copy.
fn merge_source_entities(
    target: &mut CadDocument,
    source: &CadDocument,
    target_br: Handle,
    maps: &XrefSymbolMaps,
) -> (usize, usize, HashMap<Handle, Handle>) {
    let source_ms_handle = source.header.model_space_block_handle;
    let mut entity_handle_map: HashMap<Handle, Handle> = HashMap::default();
    let mut dropped = 0usize;
    let mut unremapped = 0usize;
    for (old_h, mut entity) in ordered_source_entities(source) {
        // Drop parser-garbage entities (bad normals / vertex counts / inf
        // coords) before they enter the host doc — they trigger huge
        // allocations and unbounded recursion in the wire pipeline.
        if super::is_entity_corrupt(&entity) {
            dropped += 1;
            continue;
        }
        // Remap every symbol reference so the host's resolver hits the copies
        // just imported (load path ignores the unremapped count; BIND reports
        // it per bind).
        unremapped += remap_xref_entity(target, &mut entity, maps);
        // Route entity to the correct host block record. Entities owned by
        // the source model_space land in the target block; entities owned by
        // one of the nested BRs land in its imported counterpart.
        // Paper-space / unknown owners are skipped.
        let old_owner = entity.common().owner_handle;
        let new_owner = if old_owner == source_ms_handle {
            target_br
        } else if let Some(&h) = maps.br_handles.get(&old_owner) {
            h
        } else {
            continue;
        };
        entity.common_mut().owner_handle = new_owner;
        // Clear the foreign handle so opencadcodec assigns a new one.
        set_handle(&mut entity, Handle::NULL);
        if let Ok(new_h) = target.add_entity(entity) {
            entity_handle_map.insert(old_h, new_h);
        }
    }
    (dropped, unremapped, entity_handle_map)
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

/// Re-exported so CLI + tests share one derivation with the model.
pub use crate::io::xref_model::child_key;

/// Unified reference list for the XREF manager.
///
/// Scans `doc` for DWG xref block-records, RasterImage-linked image
/// definitions, and PDF underlay definitions. `saved_path` is always the raw
/// stored string verbatim; `found_at`/`size_bytes`/`modified` come from
/// [`resolve_path`] against `base_dir`. Keys in `unloaded` come back as
/// `Unloaded` (loaded=false) without touching the filesystem.
///
/// `prev` holds cached load-time mtimes per key (see
/// [`RefStatCache`](crate::io::xref_model::RefStatCache)). Empty on first
/// call → no `Stale` on bootstrap. When both the live mtime and the cached
/// baseline exist and live > cached + 1s slack, a `Loaded` entry reports
/// `Stale`. Nested children whose host entry is `NotFound`/`Failed` report
/// `Orphaned` (overriding `Loaded`).
///
/// Key namespacing: top-level keys are host handle values (unchanged).
/// Nested child keys are
/// [`child_key`](crate::io::xref_model::child_key)`(parent_key, name,
/// saved_path)` so foreign handles that collide with host handles never
/// alias a host row. Both `collect_entries` and the palette refresh path go
/// through this fn, and the palette keys rows off `(key, saved_path)`, so
/// nested rows keep resolving.
///
/// Nested enumeration (SPIKE5): each LOADED DwgXref whose file resolved is
/// opened read-only and its *direct* xref block-records are appended as child
/// entries (no geometry merge). Parsed docs live in a per-path
/// cache inside the call; already-seen normalized `saved_path`s are skipped,
/// so a self- or cyclic reference enumerates once and never recurses. A
/// nested file that fails to parse is skipped silently. Synchronous by
/// design; async stat is Task 8.
pub fn collect_entries(
    doc: &codec::CadDocument,
    base_dir: &Path,
    unloaded: &std::collections::HashSet<crate::io::xref_model::UnloadKey>,
) -> Vec<crate::io::xref_model::ReferenceEntry> {
    collect_entries_with_prev(doc, base_dir, unloaded, &std::collections::HashMap::new())
}

/// [`collect_entries`] with a load-time mtime baseline for `Stale` detection.
/// See [`collect_entries`] for the full contract.
pub fn collect_entries_with_prev(
    doc: &codec::CadDocument,
    base_dir: &Path,
    unloaded: &std::collections::HashSet<crate::io::xref_model::UnloadKey>,
    prev: &std::collections::HashMap<u64, std::time::SystemTime>,
) -> Vec<crate::io::xref_model::ReferenceEntry> {
    use codec::entities::UnderlayType;
    use codec::objects::ObjectType;
    use crate::io::xref_model::{
        child_key, decide_status, normalize_lexical, RefKind, RefStatus, RefType,
        ReferenceEntry, UnloadKey,
    };

    /// Resolve `raw` via `base_dir`; on success record `found_at`, stat
    /// size/mtime, and flip the entry to Loaded. Missing/empty stays NotFound.
    ///
    /// The filesystem stat is desktop-only by `cfg` (not a runtime check): the
    /// wasm binary contains no `std::fs::metadata` call at all, so a web
    /// session can never block on — or fail from — a host-filesystem probe.
    /// Path-based web references resolve through the file-handle loader and
    /// simply stay `NotFound` when unresolvable.
    fn stat_into(entry: &mut ReferenceEntry, raw: &str, base_dir: &Path) {
        if raw.is_empty() {
            return;
        }
        let Some(found) = resolve_path(raw, base_dir) else {
            return;
        };
        entry.found_at = Some(found.to_string_lossy().into_owned());
        entry.status = RefStatus::Loaded;
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(meta) = std::fs::metadata(&found) {
            entry.size_bytes = Some(meta.len());
            entry.modified = meta.modified().ok();
        }
    }

    /// Resolve `raw` into `found_at` without statting: unloaded rows keep no
    /// size/date (spec) but still know where their file lives, so previews
    /// and path operations work without loading.
    fn resolve_only(entry: &mut ReferenceEntry, raw: &str, base_dir: &Path) {
        if raw.is_empty() {
            return;
        }
        if let Some(found) = resolve_path(raw, base_dir) {
            entry.found_at = Some(found.to_string_lossy().into_owned());
        }
    }

    fn file_name_only(raw: &str) -> &str {
        raw.rsplit(['/', '\\']).next().unwrap_or(raw)
    }

    let mut entries: Vec<ReferenceEntry> = Vec::new();

    // ── DWG xrefs ──
    for br in doc.block_records.iter() {
        if !(br.flags.is_xref || br.flags.is_xref_overlay) {
            continue;
        }
        let key = br.handle.value();
        let mut entry = ReferenceEntry::new(key, br.name.clone(), RefKind::DwgXref);
        entry.ref_type = if br.flags.is_xref_overlay {
            RefType::Overlay
        } else {
            RefType::Attach
        };
        entry.saved_path = br.xref_path.clone();
        if unloaded.contains(&UnloadKey::Direct(key)) {
            entry.status = RefStatus::Unloaded;
            resolve_only(&mut entry, &br.xref_path, base_dir);
        } else {
            stat_into(&mut entry, &br.xref_path, base_dir);
            entry.status = decide_status(
                entry.status,
                entry.modified,
                prev.get(&key).copied(),
                false,
            );
        }
        entries.push(entry);
    }

    // ── Raster images ────────────────────────────────────────────────────
    // Include unreferenced definitions too. They are actionable file
    // references (and standard reference managers expose them) rather than
    // silently disappearing from the palette.
    let mut referenced_images: HashSet<Handle> = HashSet::default();
    for e in doc.entities() {
        if let EntityType::RasterImage(img) = e {
            if let Some(h) = img.definition_handle {
                referenced_images.insert(h);
            }
        }
    }
    for (handle, obj) in doc.objects.iter() {
        let ObjectType::ImageDefinition(def) = obj else {
            continue;
        };
        let is_unreferenced = !referenced_images.contains(handle);
        let key = handle.value();
        let mut entry = ReferenceEntry::new(key, file_name_only(&def.file_name), RefKind::Image);
        entry.saved_path = def.file_name.clone();
        if unloaded.contains(&UnloadKey::Direct(key)) {
            entry.status = RefStatus::Unloaded;
            resolve_only(&mut entry, &def.file_name, base_dir);
        } else {
            stat_into(&mut entry, &def.file_name, base_dir);
            entry.status = decide_status(
                entry.status,
                entry.modified,
                prev.get(&key).copied(),
                false,
            );
            if is_unreferenced {
                entry.status = RefStatus::Unreferenced;
            }
        }
        entries.push(entry);
    }

    // ── PDF underlays ──
    // Same referenced/unreferenced split as raster images: an underlay
    // definition no entity points at lists as Unreferenced (manageable, never
    // merged) rather than vanishing from the palette.
    let mut referenced_underlays: HashSet<Handle> = HashSet::default();
    for e in doc.entities() {
        if let EntityType::Underlay(u) = e {
            if u.definition_handle.is_valid() {
                referenced_underlays.insert(u.definition_handle);
            }
        }
    }
    for (handle, obj) in doc.objects.iter() {
        let ObjectType::UnderlayDefinition(def) = obj else {
            continue;
        };
        if def.underlay_type != UnderlayType::Pdf {
            continue;
        }
        let key = handle.value();
        let name = if def.name.trim().is_empty() {
            file_name_only(&def.file_path).to_owned()
        } else {
            def.name.clone()
        };
        let mut entry = ReferenceEntry::new(key, name, RefKind::Pdf);
        entry.saved_path = def.file_path.clone();
        if def.unloaded || unloaded.contains(&UnloadKey::Direct(key)) {
            entry.status = RefStatus::Unloaded;
            resolve_only(&mut entry, &def.file_path, base_dir);
        } else {
            stat_into(&mut entry, &def.file_path, base_dir);
            entry.status = decide_status(
                entry.status,
                entry.modified,
                prev.get(&key).copied(),
                false,
            );
            if !referenced_underlays.contains(handle) {
                entry.status = RefStatus::Unreferenced;
            }
        }
        entries.push(entry);
    }

    // ── Nested enumeration: full transitive closure, read-only, no merge ──
    // Paths stored in a nested drawing are relative to *that drawing*, not to
    // the host.  The work queue carries each parent's resolved file so every
    // level gets the correct base directory.  `seen` is keyed by resolved
    // absolute identity and breaks A→B→A cycles as well as shared closures.
    let mut seen: HashSet<String> = HashSet::default();
    let mut work: Vec<(u64, PathBuf)> = Vec::new();
    for e in &entries {
        if e.kind == RefKind::DwgXref && matches!(e.status, RefStatus::Loaded | RefStatus::Stale) {
            if let Some(found) = &e.found_at {
                let path = PathBuf::from(found);
                seen.insert(normalize_lexical(&path.to_string_lossy()));
                work.push((e.key, path));
            }
        }
    }
    let mut nested_cache: HashMap<String, CadDocument> = HashMap::default();
    while let Some((parent_key, parent_path)) = work.pop() {
        let cache_key = normalize_lexical(&parent_path.to_string_lossy());
        let nested_doc = if let Some(cached) = nested_cache.get(&cache_key) {
            cached.clone()
        } else {
            match super::load_file(&parent_path) {
                Ok(nested) => {
                    nested_cache.insert(cache_key, nested.clone());
                    nested
                }
                Err(_) => continue,
            }
        };
        let parent_dir = parent_path.parent().unwrap_or(base_dir);
        for br in nested_doc.block_records.iter() {
            if !(br.flags.is_xref || br.flags.is_xref_overlay) {
                continue;
            }
            let key = child_key(parent_key, &br.name, &br.xref_path);
            let mut child = ReferenceEntry::new(key, br.name.clone(), RefKind::DwgXref);
            child.parent_key = Some(parent_key);
            child.ref_type = if br.flags.is_xref_overlay { RefType::Overlay } else { RefType::Attach };
            child.saved_path = br.xref_path.clone();
            if unloaded.contains(&UnloadKey::Nested(key)) {
                child.status = RefStatus::Unloaded;
            } else {
                stat_into(&mut child, &br.xref_path, parent_dir);
                // Nested baselines live in the same stat cache under the
                // child's high-bit-tagged key, which can never alias a host
                // handle (see `child_key`) — so the lookup below is safe.
                child.status = decide_status(child.status, child.modified, prev.get(&key).copied(), false);
            }
            if let Some(found) = &child.found_at {
                let resolved_id = normalize_lexical(found);
                if seen.insert(resolved_id) {
                    work.push((key, PathBuf::from(found)));
                }
            }
            entries.push(child);
        }
    }

    // Stale propagation (plan §3.1): a Loaded parent with a Stale descendant
    // shows Stale. Roots append before their children, so one reverse pass
    // carries staleness transitively up any depth: by the time the iteration
    // reaches a parent, the parent already reflects its own subtree.
    // Unloaded/NotFound/Failed rows are never overridden.
    {
        let index: HashMap<u64, usize> =
            entries.iter().enumerate().map(|(i, e)| (e.key, i)).collect();
        for rev in (0..entries.len()).rev() {
            if entries[rev].status != RefStatus::Stale {
                continue;
            }
            if let Some(pk) = entries[rev].parent_key {
                if let Some(&pi) = index.get(&pk) {
                    if entries[pi].status == RefStatus::Loaded {
                        entries[pi].status = RefStatus::Stale;
                    }
                }
            }
        }
    }

    entries
}

/// Direct (host-owned) reference target for a top-level key.
///
/// Nested child keys are stable hashes, never handle values, so they resolve
/// to `None` here — callers treat that as "not directly actionable".
enum RefTarget {
    DwgXref { handle: Handle, name: String },
    Image { handle: Handle, name: String },
    Pdf { handle: Handle, name: String },
}

fn find_target(doc: &CadDocument, key: u64) -> Option<RefTarget> {
    for br in doc.block_records.iter() {
        if (br.flags.is_xref || br.flags.is_xref_overlay) && br.handle.value() == key {
            return Some(RefTarget::DwgXref {
                handle: br.handle,
                name: br.name.clone(),
            });
        }
    }
    for (handle, obj) in doc.objects.iter() {
        use codec::objects::ObjectType;
        match obj {
            ObjectType::ImageDefinition(def) if handle.value() == key => {
                return Some(RefTarget::Image {
                    handle: *handle,
                    name: def.file_name.clone(),
                });
            }
            ObjectType::UnderlayDefinition(def) if handle.value() == key => {
                return Some(RefTarget::Pdf {
                    handle: *handle,
                    name: if def.name.trim().is_empty() {
                        def.file_path.clone()
                    } else {
                        def.name.clone()
                    },
                });
            }
            _ => {}
        }
    }
    None
}

/// Drop merged content for one xref without removing its definition.
///
/// Deletes every entity whose `owner_handle` is the xref BlockRecord handle
/// plus that block's draw-order tables; the BlockRecord (flags, path) stays.
/// Image/PDF keys need no doc mutation (session-unloaded covers them) and
/// return `Ok`. Unknown (nested-hash) keys error — nested rows are never
/// directly unloadable.
pub fn unload_reference(doc: &mut CadDocument, key: u64) -> Result<String, String> {
    let Some(target) = find_target(doc, key) else {
        return Err(crate::t!("XREF: no loaded reference with that key.").to_string());
    };
    match target {
        RefTarget::DwgXref { handle, name } => {
            let owned: Vec<Handle> = doc
                .entities()
                .filter(|e| e.common().owner_handle == handle)
                .map(|e| e.common().handle)
                .collect();
            let owned_set: HashSet<Handle> = owned.iter().copied().collect();
            for h in owned {
                doc.remove_entity(h);
            }
            // Prune the membership chain so the block reads back empty.
            if let Some(br) = doc.block_records.iter_mut().find(|b| b.handle == handle) {
                br.entity_handles.retain(|h| !owned_set.contains(h));
            }
            // Drop draw-order overrides owned by the unloaded block.
            {
                use codec::objects::ObjectType;
                let dead: Vec<Handle> = doc
                    .objects
                    .iter()
                    .filter_map(|(h, o)| match o {
                        ObjectType::SortEntitiesTable(t)
                            if t.block_owner_handle == handle =>
                        {
                            Some(*h)
                        }
                        _ => None,
                    })
                    .collect();
                for h in dead {
                    doc.objects.remove(&h);
                }
            }
            Ok(name)
        }
        RefTarget::Image { name, .. } => Ok(name),
        // A PDF keeps its unloaded state on the definition (saved with it).
        RefTarget::Pdf { handle, name } => {
            if let Some(codec::objects::ObjectType::UnderlayDefinition(def)) =
                doc.objects.get_mut(&handle)
            {
                def.unloaded = true;
            }
            Ok(name)
        }
    }
}

/// Detach one direct reference: erase all its instances, delete the
/// BlockRecord/definition and this xref's `name|*` dependent symbols.
///
/// - Nested keys (stable hashes, no direct definition) have no `find_target`
///   hit → `Err`. The CLI maps entries with `parent_key.is_some()` to the
///   "cannot detach nested" message with the entry name.
/// - INSERT erase walks the flat store, which holds block-owned entities too
///   (SPIKE2 item 6), so instances inside other block definitions are covered.
/// - Images/PDFs: remove the definition object plus every entity referencing
///   it by handle.
pub fn detach_reference(doc: &mut CadDocument, key: u64) -> Result<String, String> {
    let Some(target) = find_target(doc, key) else {
        return Err(crate::t!("XREF: no loaded reference with that key.").to_string());
    };
    match target {
        RefTarget::DwgXref { handle, name } => {
            // (b) erase ALL INSERT instances (flat store includes block-owned).
            let inserts: Vec<Handle> = doc
                .entities()
                .filter_map(|e| match e {
                    EntityType::Insert(ins)
                        if ins.block_name.eq_ignore_ascii_case(&name) =>
                    {
                        Some(e.common().handle)
                    }
                    _ => None,
                })
                .collect();
            for h in inserts {
                doc.remove_entity(h);
            }
            // Merged content owned by the xref block.
            let owned: Vec<Handle> = doc
                .entities()
                .filter(|e| e.common().owner_handle == handle)
                .map(|e| e.common().handle)
                .collect();
            for h in owned {
                doc.remove_entity(h);
            }
            // (c) `name|*` dependents for THIS xref only (case-insensitive).
            remove_pipe_symbols(doc, &name);
            // Draw-order tables owned by the detached block.
            {
                use codec::objects::ObjectType;
                let dead: Vec<Handle> = doc
                    .objects
                    .iter()
                    .filter_map(|(h, o)| match o {
                        ObjectType::SortEntitiesTable(t)
                            if t.block_owner_handle == handle =>
                        {
                            Some(*h)
                        }
                        _ => None,
                    })
                    .collect();
                for h in dead {
                    doc.objects.remove(&h);
                }
            }
            doc.block_records.remove(&name);
            Ok(name)
        }
        RefTarget::Image { handle, name } => {
            let refs: Vec<Handle> = doc
                .entities()
                .filter_map(|e| match e {
                    EntityType::RasterImage(img)
                        if img.definition_handle == Some(handle) =>
                    {
                        Some(e.common().handle)
                    }
                    _ => None,
                })
                .collect();
            for h in refs {
                doc.remove_entity(h);
            }
            doc.objects.remove(&handle);
            Ok(name)
        }
        RefTarget::Pdf { handle, name } => {
            let refs: Vec<Handle> = doc
                .entities()
                .filter_map(|e| match e {
                    EntityType::Underlay(ul) if ul.definition_handle == handle => {
                        Some(e.common().handle)
                    }
                    _ => None,
                })
                .collect();
            for h in refs {
                doc.remove_entity(h);
            }
            doc.objects.remove(&handle);
            Ok(name)
        }
    }
}

// ── BIND (Task 9) ─────────────────────────────────────────────────────────

/// Outcome of binding one reference.
#[derive(Debug)]
pub struct BindOutcome {
    /// Bound block / definition name.
    pub name: String,
    /// Foreign style handles that could not be remapped (see
    /// `bind_limitations` on the shared remap helper).
    pub unremapped: usize,
}

/// Bind one direct reference: fold the external file into the host drawing.
///
/// - DWG xrefs become plain local blocks (same name, so existing INSERTs keep
///   resolving). Dependent symbols `name|sym` become `name$0$sym` (N bumped
///   past collisions); nested children bind first so chains compose
///   transitively (`PLAN$0$DETAIL$0$WALLS`); the whole bind-closure is
///   merged — nested geometry is never dropped.
/// - Images are COLLECTed: the file is copied next to the host drawing and
///   the definition retargeted to the relative filename. A copy failure
///   errors before the host doc is touched (no partial state).
/// - PDFs error explicitly — vector import is unavailable, and silently
///   dropping them would lose data.
/// - Nested keys have no host definition and never reach here (callers map
///   them to the "bind it in its host drawing" message, mirroring Detach).
pub fn bind_reference(
    doc: &mut CadDocument,
    key: u64,
    base_dir: &Path,
    host_dir: &Path,
) -> Result<BindOutcome, String> {
    let Some(target) = find_target(doc, key) else {
        return Err(crate::t!("XREF: no loaded reference with that key.").to_string());
    };
    match target {
        RefTarget::DwgXref { handle, name } => bind_dwg(doc, handle, &name, base_dir, host_dir),
        RefTarget::Image { handle, name } => {
            collect_bind_images(doc, &name, base_dir, host_dir, Some(handle))?;
            Ok(BindOutcome { name, unremapped: 0 })
        }
        RefTarget::Pdf { .. } => Err(crate::t!(
            "XREF: PDF bind (vector import) is not available in this version."
        )
        .to_string()),
    }
}

fn bind_failed(name: &str, reason: impl std::fmt::Display) -> String {
    crate::tf!("XREF: cannot bind \"{}\": {}.", name, reason.to_string()).to_string()
}

fn bind_dwg(
    doc: &mut CadDocument,
    br_handle: Handle,
    name: &str,
    base_dir: &Path,
    host_dir: &Path,
) -> Result<BindOutcome, String> {
    let saved = doc
        .block_records
        .iter()
        .find(|b| b.handle == br_handle)
        .map(|b| b.xref_path.clone())
        .unwrap_or_default();
    let Some(found) = resolve_path(&saved, base_dir) else {
        return Err(bind_failed(name, "file not found"));
    };
    let source_dir = found
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    // Load the closure (this file + every nested xref, children first). All
    // file copies happen below, before the host doc is touched — a copy or
    // parse failure leaves no partial document state.
    let mut source = load_bind_source(name, &found)?;
    check_no_pdf_underlays(&source)?;
    let mut stack = vec![crate::io::xref_model::normalize_lexical(
        &found.to_string_lossy(),
    )];
    let mut unremapped = bind_nested_closure(&mut source, &source_dir, host_dir, &mut stack)?;
    collect_bind_images(&mut source, name, &source_dir, host_dir, None)?;
    // Drop the previous merge + `name|*` symbols. INSERTs of the xref stay —
    // they now reference the bound block.
    unload_reference(doc, br_handle.value())?;
    remove_pipe_symbols(doc, name);
    let taken = BindTaken::capture(doc);
    let (u, _) = merge_bound_doc(doc, name, br_handle, &source, taken);
    unremapped += u;
    // Convert the xref BlockRecord into a plain local block (same name, so
    // existing INSERTs keep resolving).
    if let Some(br) = doc.block_records.iter_mut().find(|b| b.handle == br_handle) {
        br.flags.is_xref = false;
        br.flags.is_xref_overlay = false;
        br.xref_path.clear();
    }
    Ok(BindOutcome {
        name: name.to_string(),
        unremapped,
    })
}

fn load_bind_source(ref_name: &str, found: &Path) -> Result<CadDocument, String> {
    match super::load_file(found) {
        Ok(doc) => Ok(doc),
        Err(e) => Err(bind_failed(ref_name, e)),
    }
}

/// Reject PDF underlays anywhere in a bind closure: vector import is
/// unavailable, and silently dropping them would lose data.
fn check_no_pdf_underlays(source: &CadDocument) -> Result<(), String> {
    use codec::entities::UnderlayType;
    use codec::objects::ObjectType;
    let has_pdf = source.objects.values().any(|o| {
        matches!(o, ObjectType::UnderlayDefinition(d) if d.underlay_type == UnderlayType::Pdf)
    });
    if has_pdf {
        return Err(crate::t!(
            "XREF: PDF bind (vector import) is not available in this version."
        )
        .to_string());
    }
    Ok(())
}

/// Recursively bind every nested xref inside `source`, children first, so the
/// parent merge composes transitive chains (`PLAN$0$DETAIL$0$WALLS`).
/// Unresolvable nested files error the whole bind; cyclic closures enumerate
/// once (mirroring `collect_entries`). Returns the nested unremapped count.
fn bind_nested_closure(
    source: &mut CadDocument,
    source_dir: &Path,
    host_dir: &Path,
    stack: &mut Vec<String>,
) -> Result<usize, String> {
    let children: Vec<(String, Handle, String)> = source
        .block_records
        .iter()
        .filter(|br| (br.flags.is_xref || br.flags.is_xref_overlay) && !br.xref_path.is_empty())
        .map(|br| (br.name.clone(), br.handle, br.xref_path.clone()))
        .collect();
    let mut taken = BindTaken::capture(source);
    let mut unremapped = 0usize;
    for (child_name, child_br, child_saved) in children {
        let Some(child_found) = resolve_path(&child_saved, source_dir) else {
            return Err(bind_failed(&child_name, "file not found"));
        };
        let norm =
            crate::io::xref_model::normalize_lexical(&child_found.to_string_lossy());
        if stack.contains(&norm) {
            continue;
        }
        stack.push(norm);
        let mut child_doc = load_bind_source(&child_name, &child_found)?;
        let child_dir = child_found
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        unremapped += bind_nested_closure(&mut child_doc, &child_dir, host_dir, stack)?;
        stack.pop();
        check_no_pdf_underlays(&child_doc)?;
        collect_bind_images(&mut child_doc, &child_name, &child_dir, host_dir, None)?;
        let (u, taken_back) = merge_bound_doc(source, &child_name, child_br, &child_doc, taken);
        unremapped += u;
        taken = taken_back;
        if let Some(br) = source
            .block_records
            .iter_mut()
            .find(|b| b.handle == child_br)
        {
            br.flags.is_xref = false;
            br.flags.is_xref_overlay = false;
            br.xref_path.clear();
        }
    }
    Ok(unremapped)
}

/// Merge one fully child-bound source doc into `target` under BIND names
/// (`{parent}$N${sym}`), threading `taken` through for collision dedup.
/// Returns the merge's unremapped count plus the updated taken sets.
fn merge_bound_doc(
    target: &mut CadDocument,
    parent: &str,
    target_br: Handle,
    source: &CadDocument,
    taken: BindTaken,
) -> (usize, BindTaken) {
    ensure_block_entities(target, parent);
    let mut naming = SymbolNaming::BindDollar {
        parent: parent.to_string(),
        taken,
    };
    let maps = import_xref_symbols(target, source, &mut naming);
    let (_, unremapped, entity_handle_map) =
        merge_source_entities(target, source, target_br, &maps);
    copy_xref_sortents(
        target,
        source,
        target_br,
        &maps.br_handles,
        &entity_handle_map,
    );
    let taken = match naming {
        SymbolNaming::BindDollar { taken, .. } => taken,
        SymbolNaming::MergePipe { .. } => unreachable!("bind merge always uses BindDollar"),
    };
    (unremapped, taken)
}

/// COLLECT every raster image in `source`: copy the file next to the host
/// drawing (first claimant keeps the filename, later same-name collisions get
/// `stem$N.ext`) and retarget definitions + entity paths to the relative
/// filename. Copies run before any host mutation — a failure errors with no
/// partial document state. `only` restricts the pass to one definition
/// (direct image bind); `None` collects the whole closure.
fn collect_bind_images(
    source: &mut CadDocument,
    ref_name: &str,
    source_dir: &Path,
    host_dir: &Path,
    only: Option<Handle>,
) -> Result<(), String> {
    use codec::objects::ObjectType;
    let defs: Vec<(Handle, String)> = source
        .objects
        .iter()
        .filter_map(|(h, o)| match o {
            ObjectType::ImageDefinition(d)
                if only.is_none_or(|want| want == *h) =>
            {
                Some((*h, d.file_name.clone()))
            }
            _ => None,
        })
        .collect();
    if defs.is_empty() {
        return Ok(());
    }
    // Plan every copy first (dedupe repeat references to one source file).
    let mut planned: HashMap<String, String> = HashMap::default();
    let mut ops: Vec<(Handle, PathBuf, String)> = Vec::new();
    for (handle, saved) in &defs {
        let Some(resolved) = resolve_path(saved, source_dir) else {
            return Err(bind_failed(ref_name, format!("image not found: {saved}")));
        };
        let norm = crate::io::xref_model::normalize_lexical(&resolved.to_string_lossy());
        let dest_name = if let Some(prior) = planned.get(&norm) {
            prior.clone()
        } else {
            let base = file_name_only(saved);
            let mut candidate = base.clone();
            let mut n = 0u32;
            while host_dir.join(&candidate).exists()
                && !same_file(&resolved, &host_dir.join(&candidate))
            {
                n += 1;
                candidate = unique_file_name(&base, n);
            }
            planned.insert(norm, candidate.clone());
            candidate
        };
        ops.push((*handle, resolved, dest_name));
    }
    for (_, src, dest_name) in &ops {
        let dest = host_dir.join(dest_name);
        if same_file(src, &dest) {
            continue;
        }
        if let Err(e) = std::fs::copy(src, &dest) {
            return Err(bind_failed(ref_name, format!("cannot collect image: {e}")));
        }
    }
    // Retarget definitions + entity paths to the collected relative names.
    for (handle, _, dest_name) in ops {
        if let Some(ObjectType::ImageDefinition(def)) = source.objects.get_mut(&handle) {
            def.file_name = dest_name.clone();
        }
        let owned: Vec<Handle> = source
            .entities()
            .filter_map(|e| match e {
                EntityType::RasterImage(img) if img.definition_handle == Some(handle) => {
                    Some(e.common().handle)
                }
                _ => None,
            })
            .collect();
        for eh in owned {
            if let Some(EntityType::RasterImage(img)) = source.get_entity_mut(eh) {
                img.file_path = dest_name.clone();
            }
        }
    }
    Ok(())
}

fn file_name_only(raw: &str) -> String {
    raw.rsplit(['/', '\\']).next().unwrap_or(raw).to_string()
}

fn unique_file_name(base: &str, n: u32) -> String {
    match base.rfind('.') {
        Some(i) if i > 0 => format!("{}${n}.{}", &base[..i], &base[i + 1..]),
        _ => format!("{base}${n}"),
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// Make the named references' paths relative to `host` (their drawing's
/// first save); a reference that cannot be relative keeps its full path.
pub fn make_relative(
    doc: &mut CadDocument,
    names: &rustc_hash::FxHashSet<String>,
    host: &Path,
) {
    for name in names {
        let Some(key) = doc.block_records.get(name).map(|br| br.handle.value()) else {
            continue;
        };
        let _ = apply_pathtype(doc, key, crate::io::xref_model::Pathtype::Relative, host);
    }
}

/// Drop from a save copy what resolving each xref merged into the host but
/// the file never stores: the reference's own geometry, its nested block
/// definitions and its text and dimension styles. Dependent layers and
/// linetypes stay (flagged), so layer overrides kept under VISRETAIN persist.
pub fn strip_resolved_xref_content(doc: &mut CadDocument) {
    let xrefs: Vec<(String, Handle)> = doc
        .block_records
        .iter()
        .filter(|br| br.flags.is_xref || br.flags.is_xref_overlay)
        .map(|br| (br.name.clone(), br.handle))
        .collect();
    for (name, handle) in xrefs {
        let (begin, end) = doc
            .block_records
            .get(&name)
            .map(|br| (br.block_entity_handle, br.block_end_handle))
            .unwrap_or((Handle::NULL, Handle::NULL));
        let owned: Vec<Handle> = doc
            .entities()
            .filter(|e| e.common().owner_handle == handle)
            .map(|e| e.common().handle)
            .filter(|h| *h != begin && *h != end)
            .collect();
        for h in owned {
            doc.remove_entity(h);
        }
        if let Some(br) = doc.block_records.get_mut(&name) {
            br.entity_handles.clear();
        }
        remove_dependent_symbols(doc, &name, false);
    }
}

/// Text styles a kept linetype draws its shapes or text with.
fn linetype_styles(doc: &CadDocument) -> HashSet<Handle> {
    doc.line_types
        .iter()
        .flat_map(|linetype| linetype.elements.iter())
        .filter_map(|element| element.complex.as_ref().map(|complex| complex.style_handle))
        .filter(|handle| !handle.is_null())
        .collect()
}

/// Remove one xref's `name|*` dependent symbols (case-insensitive): layers,
/// linetypes, text styles, dim styles, and nested block records — plus every
/// entity and draw-order table owned by those dependent blocks.
///
/// Shared by Detach (which then also erases the BlockRecord itself) and BIND
/// (which drops the previous merge before re-merging under `$N$` names). The
/// xref BlockRecord itself, its INSERTs, and non-prefixed symbols stay.
fn remove_pipe_symbols(doc: &mut CadDocument, name: &str) {
    remove_dependent_symbols(doc, name, true);
}

fn remove_dependent_symbols(doc: &mut CadDocument, name: &str, layers_too: bool) {
    let prefix = format!("{}|", name.to_uppercase());
    let is_dep = |n: &str| n.to_uppercase().starts_with(&prefix);
    // Dependent blocks first — their handles route the owned-entity cleanup.
    let dep_brs: HashSet<Handle> = doc
        .block_records
        .iter()
        .filter(|b| is_dep(&b.name))
        .map(|b| b.handle)
        .collect();
    let owned: Vec<Handle> = doc
        .entities()
        .filter(|e| dep_brs.contains(&e.common().owner_handle))
        .map(|e| e.common().handle)
        .collect();
    for h in owned {
        doc.remove_entity(h);
    }
    {
        use codec::objects::ObjectType;
        let dead: Vec<Handle> = doc
            .objects
            .iter()
            .filter_map(|(h, o)| match o {
                ObjectType::SortEntitiesTable(t) if dep_brs.contains(&t.block_owner_handle) => {
                    Some(*h)
                }
                _ => None,
            })
            .collect();
        for h in dead {
            doc.objects.remove(&h);
        }
    }
    let layers: Vec<String> = doc
        .layers
        .names()
        .filter(|n| layers_too && is_dep(n))
        .map(|s| s.to_string())
        .collect();
    for n in layers {
        doc.layers.remove(&n);
    }
    let lts: Vec<String> = doc
        .line_types
        .names()
        .filter(|n| layers_too && is_dep(n))
        .map(|s| s.to_string())
        .collect();
    for n in lts {
        doc.line_types.remove(&n);
    }
    // Styles a kept dependent linetype still draws with stay, flagged.
    let used = if layers_too {
        HashSet::default()
    } else {
        linetype_styles(doc)
    };
    let styles: Vec<String> = doc
        .text_styles
        .iter()
        .filter(|style| is_dep(&style.name) && !used.contains(&style.handle))
        .map(|style| style.name.clone())
        .collect();
    for n in styles {
        doc.text_styles.remove(&n);
    }
    let owner = doc
        .block_records
        .get(name)
        .map(|br| br.handle)
        .unwrap_or(Handle::NULL);
    for style in doc.text_styles.iter_mut() {
        if is_dep(&style.name) {
            style.xref_dependent = true;
            style.xref_block_record_handle = owner;
        }
    }
    let dims: Vec<String> = doc
        .dim_styles
        .names()
        .filter(|n| is_dep(n))
        .map(|s| s.to_string())
        .collect();
    for n in dims {
        doc.dim_styles.remove(&n);
    }
    let brs: Vec<String> = doc
        .block_records
        .iter()
        .map(|b| b.name.clone())
        .filter(|n| is_dep(n))
        .collect();
    for n in brs {
        doc.block_records.remove(&n);
    }
}

/// Snapshot one xref's dependent-layer overrides for VISRETAIN=1 reloads.
///
/// Returns `name|*` layers keyed by uppercased name. The reload path purges
/// these entries and re-merges file defaults; without the snapshot the
/// user's frozen/off/color tweaks would be silently lost on every reload.
fn snapshot_dependent_layers(
    doc: &CadDocument,
    name: &str,
) -> HashMap<String, codec::tables::Layer> {
    let prefix = format!("{}|", name.to_uppercase());
    doc.layers
        .names()
        .filter(|n| n.to_uppercase().starts_with(&prefix))
        .filter_map(|n| {
            doc.layers
                .get(n)
                .cloned()
                .map(|l| (l.name.to_uppercase(), l))
        })
        .collect()
}

/// Re-apply a [`snapshot_dependent_layers`] snapshot onto freshly merged
/// layers. Only layers present post-merge are touched (deleted-in-file layers
/// stay deleted); handles and file-structural links (material, plotstyle,
/// xref block-record pointer) stay on the fresh entries — everything a user
/// can tweak in a layer UI is restored.
fn restore_dependent_layers(
    doc: &mut CadDocument,
    name: &str,
    kept: HashMap<String, codec::tables::Layer>,
) {
    if kept.is_empty() {
        return;
    }
    let prefix = format!("{}|", name.to_uppercase());
    let targets: Vec<String> = doc
        .layers
        .names()
        .filter(|n| n.to_uppercase().starts_with(&prefix))
        .map(|s| s.to_string())
        .collect();
    for n in targets {
        let (Some(cur), Some(old)) = (
            doc.layers.get_mut(&n),
            kept.get(&n.to_uppercase()),
        ) else {
            continue;
        };
        cur.flags = old.flags.clone();
        cur.color = old.color.clone();
        cur.color_name = old.color_name.clone();
        cur.book_name = old.book_name.clone();
        cur.line_type = old.line_type.clone();
        cur.line_weight = old.line_weight.clone();
        cur.plot_style = old.plot_style.clone();
        cur.is_plottable = old.is_plottable;
        cur.transparency = old.transparency.clone();
    }
}

/// Store `new_raw` verbatim on the definition (no synthesis).
pub fn set_ref_path(
    doc: &mut CadDocument,
    key: u64,
    new_raw: &str,
) -> Result<String, String> {
    let Some(target) = find_target(doc, key) else {
        return Err(crate::t!("XREF: no loaded reference with that key.").to_string());
    };
    match target {
        RefTarget::DwgXref { name, .. } => {
            if let Some(br) = doc.block_records.get_mut(&name) {
                br.xref_path = new_raw.to_string();
                return Ok(name);
            }
            Err(crate::t!("XREF: no loaded reference with that key.").to_string())
        }
        RefTarget::Image { handle, name } => {
            use codec::objects::ObjectType;
            match doc.objects.get_mut(&handle) {
                Some(ObjectType::ImageDefinition(def)) => {
                    def.file_name = new_raw.to_string();
                    Ok(name)
                }
                _ => Err(crate::t!("XREF: no loaded reference with that key.").to_string()),
            }
        }
        RefTarget::Pdf { handle, name } => {
            use codec::objects::ObjectType;
            match doc.objects.get_mut(&handle) {
                Some(ObjectType::UnderlayDefinition(def)) => {
                    def.file_path = new_raw.to_string();
                    Ok(name)
                }
                _ => Err(crate::t!("XREF: no loaded reference with that key.").to_string()),
            }
        }
    }
}

/// Convert one reference's stored path to `pathtype` against `host_path`.
/// Verbatim store on success; neutral errors otherwise.
pub fn apply_pathtype(
    doc: &mut CadDocument,
    key: u64,
    pathtype: crate::io::xref_model::Pathtype,
    host_path: &Path,
) -> Result<String, String> {
    let saved = match find_target(doc, key) {
        Some(RefTarget::DwgXref { name, .. }) => doc
            .block_records
            .get(&name)
            .map(|b| b.xref_path.clone())
            .unwrap_or_default(),
        Some(RefTarget::Image { handle, .. }) => {
            use codec::objects::ObjectType;
            match doc.objects.get(&handle) {
                Some(ObjectType::ImageDefinition(def)) => def.file_name.clone(),
                _ => String::new(),
            }
        }
        Some(RefTarget::Pdf { handle, .. }) => {
            use codec::objects::ObjectType;
            match doc.objects.get(&handle) {
                Some(ObjectType::UnderlayDefinition(def)) => def.file_path.clone(),
                _ => String::new(),
            }
        }
        None => return Err(crate::t!("XREF: no loaded reference with that key.").to_string()),
    };
    // A stored relative path first has to be resolved against its current host
    // folder.  Passing it straight to `to_pathtype_result` made `Full` a
    // no-op (`refs/a.dwg` stayed relative instead of becoming C:/…/refs/a).
    let normalized = crate::io::xref_model::normalize_lexical(&saved);
    let bytes = normalized.as_bytes();
    let looks_absolute = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
        || normalized.starts_with("//")
        || normalized.starts_with('/');
    let stored_relative = !looks_absolute;
    let effective = if stored_relative {
        host_path
            .parent()
            .map(|dir| dir.join(&saved).to_string_lossy().into_owned())
            .unwrap_or(saved.clone())
    } else {
        saved
    };
    match crate::io::xref_model::to_pathtype_result(&effective, host_path, pathtype) {
        Ok(next) => {
            set_ref_path(doc, key, &next)?;
            Ok(next)
        }
        Err(crate::io::xref_model::PathtypeError::AcrossDrives) => {
            Err(crate::t!("XREF: cannot make path relative across drives.").to_string())
        }
        Err(crate::io::xref_model::PathtypeError::UnsavedHost) => Err(
            crate::t!("XREF  Save the drawing first to resolve relative XREF paths.").to_string(),
        ),
    }
}

/// Find & Replace: rewrite `old` → `new` as a component-wise prefix replace
/// wherever the normalized saved path has the normalized `old` path as its
/// leading run of `/`-components. Returns rewrites.
///
/// Component-wise (never a raw byte splice): matching happens on normalized
/// forms and the result is rejoined from normalized components, so `..`-laden
/// or case-folded saved paths rewrite correctly and multibyte paths can never
/// panic on a mid-char cut.
pub fn replace_path_prefix(doc: &mut CadDocument, old: &str, new: &str) -> usize {
    use crate::io::xref_model::normalize_lexical;
    let old_n = normalize_lexical(old);
    if old_n.is_empty() {
        return 0;
    }
    let old_comps: Vec<&str> = old_n.split('/').collect();
    let new_n = normalize_lexical(new);
    let splice = |saved: &mut String| -> bool {
        let saved_n = normalize_lexical(saved);
        let saved_comps: Vec<&str> = saved_n.split('/').collect();
        if saved_comps.len() < old_comps.len() {
            return false;
        }
        if saved_comps[..old_comps.len()] != old_comps[..] {
            return false;
        }
        let rest = &saved_comps[old_comps.len()..];
        let mut out = new_n.clone();
        if !rest.is_empty() {
            if !out.is_empty() {
                out.push('/');
            }
            out.push_str(&rest.join("/"));
        }
        *saved = out;
        true
    };
    let mut n = 0usize;
    for br in doc.block_records.iter_mut() {
        if !(br.flags.is_xref || br.flags.is_xref_overlay) || br.xref_path.is_empty() {
            continue;
        }
        if splice(&mut br.xref_path) {
            n += 1;
        }
    }
    {
        use codec::objects::ObjectType;
        for obj in doc.objects.values_mut() {
            match obj {
                ObjectType::ImageDefinition(def) if !def.file_name.is_empty() => {
                    if splice(&mut def.file_name) {
                        n += 1;
                    }
                }
                ObjectType::UnderlayDefinition(def) if !def.file_path.is_empty() => {
                    if splice(&mut def.file_path) {
                        n += 1;
                    }
                }
                _ => {}
            }
        }
    }
    n
}

/// Flip Attach ↔ Overlay for one DWG xref. DWG only; images/PDFs error.
///
/// The flip affects future nesting propagation (overlays do not re-export
/// through the host); current geometry is untouched.
pub fn set_ref_type(
    doc: &mut CadDocument,
    key: u64,
    ref_type: crate::io::xref_model::RefType,
) -> Result<String, String> {
    let Some(target) = find_target(doc, key) else {
        return Err(crate::t!("XREF: no loaded reference with that key.").to_string());
    };
    match target {
        RefTarget::DwgXref { name, .. } => {
            if let Some(br) = doc.block_records.get_mut(&name) {
                match ref_type {
                    crate::io::xref_model::RefType::Attach => {
                        br.flags.is_xref = true;
                        br.flags.is_xref_overlay = false;
                    }
                    crate::io::xref_model::RefType::Overlay => {
                        br.flags.is_xref = false;
                        br.flags.is_xref_overlay = true;
                    }
                }
                return Ok(name);
            }
            Err(crate::t!("XREF: no loaded reference with that key.").to_string())
        }
        RefTarget::Image { .. } | RefTarget::Pdf { .. } => {
            Err(crate::t!("XREF: overlays apply to drawing references only.").to_string())
        }
    }
}

/// Save-As rebase helper, wired in `queue_native_save` (Task 8b).
///
/// Call with the old/new parent dirs before serializing: every relative
/// saved path is made absolute via `old_base` then re-expressed relative to
/// `new_base` (absolute paths and cross-drive targets are left absolute).
/// Returns rewrite count.
///
/// Both the save snapshot and, on Save-As completion (`on_save_finished`),
/// the live document are rebased so the running session agrees with the file
/// just written.
pub fn rebase_relative_paths_for_save_as(
    doc: &mut CadDocument,
    old_base: &Path,
    new_base: &Path,
) -> usize {
    use crate::io::xref_model::{normalize_lexical, to_pathtype_result, Pathtype};
    let is_relative = |s: &str| {
        let n = normalize_lexical(s);
        if n.is_empty() {
            return false;
        }
        let b = n.as_bytes();
        if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
            return false;
        }
        if n.starts_with("//") || n.starts_with('/') {
            return false;
        }
        true
    };
    // Fake host file inside the new base so `to_pathtype_result` has a parent.
    let new_host = new_base.join("__host__.dwg");
    let mut n = 0usize;
    let mut rebase = |saved: &mut String| {
        if !is_relative(saved) {
            return;
        }
        let abs = old_base.join(saved.clone());
        let abs_s = abs.to_string_lossy().into_owned();
        if let Ok(rel) = to_pathtype_result(&abs_s, &new_host, Pathtype::Relative) {
            *saved = rel;
            n += 1;
        }
    };
    for br in doc.block_records.iter_mut() {
        if !(br.flags.is_xref || br.flags.is_xref_overlay) || br.xref_path.is_empty() {
            continue;
        }
        rebase(&mut br.xref_path);
    }
    {
        use codec::objects::ObjectType;
        for obj in doc.objects.values_mut() {
            match obj {
                ObjectType::ImageDefinition(def) if !def.file_name.is_empty() => {
                    rebase(&mut def.file_name)
                }
                ObjectType::UnderlayDefinition(def) if !def.file_path.is_empty() => {
                    rebase(&mut def.file_path)
                }
                _ => {}
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::collect_entries;
    use super::collect_entries_with_prev;
    use super::HashSet;
    use super::{
        apply_pathtype, child_key, detach_reference, replace_path_prefix,
        resolve_xrefs_for_keys, restore_dependent_layers, set_ref_path,
        set_ref_type, snapshot_dependent_layers, unload_reference,
    };
    use crate::io::xref_model::{Pathtype, RefKind, RefStatus, RefType};

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn collect_single_xref_loaded_with_verbatim_path() {
        let tmp = std::env::temp_dir().join(format!(
            "ocs_xref_collect_{}_{}.dwg",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&tmp, b"").unwrap();
        let saved = tmp.to_string_lossy().into_owned();

        let mut doc = codec::CadDocument::new();
        let mut br = codec::tables::BlockRecord::new("PLAN");
        br.flags.is_xref = true;
        br.xref_path = saved.clone();
        br.handle = doc.allocate_handle();
        doc.block_records.add(br).expect("add xref block record");

        let base = tmp.parent().unwrap();
        let entries = collect_entries(&doc, base, &std::collections::HashSet::new());

        std::fs::remove_file(&tmp).ok();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].status, RefStatus::Loaded);
        assert_eq!(entries[0].saved_path, saved);
        assert_eq!(entries[0].kind, RefKind::DwgXref);
    }

    fn xref_doc_with(name: &str, saved: &str) -> (codec::CadDocument, u64) {
        let mut doc = codec::CadDocument::new();
        let mut br = codec::tables::BlockRecord::new(name);
        br.flags.is_xref = true;
        br.xref_path = saved.to_string();
        br.handle = doc.allocate_handle();
        let key = br.handle.value();
        doc.block_records.add(br).expect("add xref");
        (doc, key)
    }

    fn xref_tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ocs_xref_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_nested_fixture(dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
        // mid.dwgxrefs inner.dwg; both must parse (enumeration reads the
        // mid file's block-record table). Returns (mid, inner) abs paths.
        std::fs::write(dir.join("inner.dwg"), b"inner-bytes").unwrap();
        let mut mid = codec::CadDocument::new();
        let mut br = codec::tables::BlockRecord::new("INNER");
        br.flags.is_xref = true;
        br.xref_path = dir.join("inner.dwg").to_string_lossy().into_owned();
        mid.block_records.add(br).unwrap();
        let bytes = crate::io::save_to_bytes(&mid, "dwg", mid.version).unwrap();
        std::fs::write(dir.join("mid.dwg"), &bytes).unwrap();
        (dir.join("mid.dwg"), dir.join("inner.dwg"))
    }

    #[test]
    fn nested_stale_propagates_to_parent() {
        // Plan §3.1: a Loaded parent with a Stale nested descendant shows
        // Stale. The nested baseline predates the live mtime (epoch), so the
        // child is Stale and the reverse pass carries it to the parent —
        // without touching Unloaded/Failed rows.
        use std::time::UNIX_EPOCH;
        let dir = xref_tmpdir("staleprop");
        let (mid_path, inner_path) = write_nested_fixture(&dir);
        let (doc, mid_key) = xref_doc_with(
            "MID",
            &mid_path.to_string_lossy(),
        );
        let inner_key = child_key(
            mid_key,
            "INNER",
            &inner_path.to_string_lossy(),
        );
        let mut prev = std::collections::HashMap::new();
        prev.insert(inner_key, UNIX_EPOCH);
        let entries = collect_entries_with_prev(
            &doc,
            &dir,
            &std::collections::HashSet::new(),
            &prev,
        );
        let inner = entries.iter().find(|e| e.key == inner_key).expect("nested INNER listed");
        assert_eq!(inner.status, RefStatus::Stale);
        let mid = entries.iter().find(|e| e.key == mid_key).expect("MID listed");
        assert_eq!(mid.status, RefStatus::Stale, "parent reflects stale child");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn xref_end_to_end_engine_lifecycle() {
        // One true integration pass over a real DWG fixture (nested xref +
        // referenced image + unreferenced PDF): attach-equivalent list →
        // unload → targeted reload → detach. Catches integration skew that
        // unit tests on single ops cannot.
        use codec::objects::{ImageDefinition, ObjectType, UnderlayDefinition};
        let dir = xref_tmpdir("e2e");
        let (mid_path, _) = write_nested_fixture(&dir);
        std::fs::write(dir.join("img.png"), b"fake-png").unwrap();
        let (mut doc, mid_key) =
            xref_doc_with("MID", &mid_path.to_string_lossy());
        // Referenced image → Loaded entry.
        let img_h = doc.allocate_handle();
        let mut img_def = ImageDefinition::with_dimensions("img.png", 8, 8);
        img_def.handle = img_h;
        doc.objects.insert(img_h, ObjectType::ImageDefinition(img_def));
        let mut img = codec::entities::RasterImage::new(
            "img.png",
            codec::types::Vector3::ZERO,
            8.0,
            8.0,
        );
        img.definition_handle = Some(img_h);
        doc.add_entity(codec::EntityType::RasterImage(img)).unwrap();
        // Unreferenced PDF definition → listed, never merged.
        let pdf_h = doc.allocate_handle();
        let mut pdf_def = UnderlayDefinition::pdf("doc.pdf", "1");
        pdf_def.handle = pdf_h;
        doc.objects.insert(pdf_h, ObjectType::UnderlayDefinition(pdf_def));

        let empty = std::collections::HashSet::new();
        let no_prev = std::collections::HashMap::new();
        // 1. List: everything visible with the right statuses.
        let listed = collect_entries_with_prev(&doc, &dir, &empty, &no_prev);
        assert!(listed.iter().any(|e| e.name == "MID" && e.status == RefStatus::Loaded));
        assert!(listed.iter().any(|e| e.name == "INNER" && e.parent_key.is_some()));
        assert!(listed.iter().any(|e| e.name == "img.png" && e.status == RefStatus::Loaded));
        assert!(listed.iter().any(|e| e.name == "doc.pdf" && e.status == RefStatus::Unreferenced));
        // 2. Unload: definition retained, content marked.
        unload_reference(&mut doc, mid_key).expect("unload");
        let mut unloaded = std::collections::HashSet::new();
        unloaded.insert(crate::io::xref_model::UnloadKey::Direct(mid_key));
        let listed = collect_entries_with_prev(&doc, &dir, &unloaded, &no_prev);
        assert!(listed.iter().any(|e| e.name == "MID" && e.status == RefStatus::Unloaded));
        assert!(doc.block_records.get("MID").is_some(), "definition retained");
        // 3. Targeted reload: back to Loaded.
        let mut handles = HashSet::default();
        handles.insert(doc.block_records.get("MID").unwrap().handle);
        let (infos, _) = resolve_xrefs_for_keys(&mut doc, &dir, &handles);
        assert!(infos.iter().any(|i| i.name == "MID"));
        let listed = collect_entries_with_prev(&doc, &dir, &empty, &no_prev);
        assert!(listed.iter().any(|e| e.name == "MID" && e.status == RefStatus::Loaded));
        // 4. Detach: instances, definition, and nested rows all gone.
        detach_reference(&mut doc, mid_key).expect("detach");
        let listed = collect_entries_with_prev(&doc, &dir, &empty, &no_prev);
        assert!(!listed.iter().any(|e| e.name == "MID" || e.name == "INNER"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn nested_child_key_never_aliases_host() {
        let host_key = 42u64;
        let nested = child_key(100, "DETAIL", "refs/detail.dwg");
        assert_ne!(nested, host_key);
    }

    #[test]
    fn set_ref_path_stores_verbatim() {
        let (mut doc, key) = xref_doc_with("PLAN", "old/path.dwg");
        set_ref_path(&mut doc, key, "new\\raw path.dwg").expect("set path");
        let br = doc.block_records.get("PLAN").unwrap();
        assert_eq!(br.xref_path, "new\\raw path.dwg");
    }

    #[test]
    fn apply_pathtype_across_drives_errors() {
        let (mut doc, key) = xref_doc_with("PLAN", "D:/Lib/plan.dwg");
        let host = std::path::Path::new("C:/Drawings/host.dwg");
        let err = apply_pathtype(&mut doc, key, Pathtype::Relative, host).unwrap_err();
        assert_eq!(err, "XREF: cannot make path relative across drives.");
    }

    #[test]
    #[cfg(windows)]
    fn apply_pathtype_full_resolves_existing_relative_path() {
        let (mut doc, key) = xref_doc_with("PLAN", "refs/plan.dwg");
        let host = std::path::Path::new("C:/Drawings/host.dwg");
        apply_pathtype(&mut doc, key, Pathtype::Full, host).expect("full path");
        assert_eq!(
            doc.block_records.get("PLAN").unwrap().xref_path,
            "C:/Drawings/refs/plan.dwg"
        );
    }

    #[test]
    fn overlay_on_image_errors() {
        use codec::objects::{ImageDefinition, ObjectType};
        let mut doc = codec::CadDocument::new();
        let h = doc.allocate_handle();
        let mut def = ImageDefinition::with_dimensions("img.png", 8, 8);
        def.handle = h;
        doc.objects.insert(h, ObjectType::ImageDefinition(def));
        let err = set_ref_type(&mut doc, h.value(), RefType::Overlay).unwrap_err();
        assert_eq!(err, "XREF: overlays apply to drawing references only.");
    }

    #[test]
    fn ref_type_attach_reachable_roundtrip() {
        // F10: `set_ref_type(Attach)` (palette Type control, Attach
        // direction) flips an overlay back to a full attach.
        let (mut doc, key) = xref_doc_with("PLAN", "plan.dwg");
        set_ref_type(&mut doc, key, RefType::Overlay).expect("to overlay");
        assert!(doc.block_records.get("PLAN").unwrap().flags.is_xref_overlay);
        let name = set_ref_type(&mut doc, key, RefType::Attach).expect("to attach");
        assert_eq!(name, "PLAN");
        let br = doc.block_records.get("PLAN").unwrap();
        assert!(br.flags.is_xref && !br.flags.is_xref_overlay);
    }

    #[test]
    fn unload_drops_merged_entities_keeps_definition() {
        let (mut doc, key) = xref_doc_with("PLAN", "missing.dwg");
        let br_h = doc.block_records.get("PLAN").unwrap().handle;
        let mut line = codec::entities::Line::new();
        line.common.handle = codec::types::Handle::NULL;
        line.common.owner_handle = br_h;
        doc.add_entity(codec::EntityType::Line(line)).unwrap();
        assert_eq!(doc.block_records.get("PLAN").unwrap().entity_handles.len(), 1);
        unload_reference(&mut doc, key).expect("unload");
        assert!(doc.block_records.get("PLAN").is_some());
        let owned: Vec<_> = doc
            .entities()
            .filter(|e| e.common().owner_handle == br_h)
            .collect();
        assert!(owned.is_empty());
    }

    #[test]
    fn detach_erases_inserts_and_definition() {
        let (mut doc, key) = xref_doc_with("PLAN", "missing.dwg");
        let ins = codec::entities::Insert::new("PLAN", codec::types::Vector3::ZERO);
        doc.add_entity(codec::EntityType::Insert(ins)).unwrap();
        detach_reference(&mut doc, key).expect("detach");
        assert!(doc.block_records.get("PLAN").is_none());
        assert!(!doc.entities().any(|e| matches!(
            e,
            codec::EntityType::Insert(i) if i.block_name.eq_ignore_ascii_case("PLAN")
        )));
    }

    #[test]
    fn visretain_snapshot_restore_roundtrip() {
        // VISRETAIN=1 reload path: host-side overrides on `name|*` layers
        // must survive purge + re-merge. Exercises the mechanism directly:
        // snapshot → mutate to defaults → restore → overrides back.
        let (mut doc, _) = xref_doc_with("PLAN", "plan.dwg");
        let mut layer = codec::tables::Layer::new("PLAN|WALLS");
        layer.flags.off = true;
        layer.flags.frozen = true;
        layer.color = codec::types::Color::from_index(1);
        doc.layers.add_or_replace(layer);
        let kept = snapshot_dependent_layers(&doc, "PLAN");
        assert!(kept.contains_key("PLAN|WALLS"));
        // Simulate the purge + file-default re-merge.
        doc.layers.remove("PLAN|WALLS");
        doc.layers.add_or_replace(codec::tables::Layer::new("PLAN|WALLS"));
        assert!(!doc.layers.get("PLAN|WALLS").unwrap().flags.off);
        restore_dependent_layers(&mut doc, "PLAN", kept);
        let back = doc.layers.get("PLAN|WALLS").unwrap();
        assert!(back.flags.off, "off override restored");
        assert!(back.flags.frozen, "frozen override restored");
        assert_eq!(back.color, codec::types::Color::from_index(1));
    }

    #[test]
    fn visretain_restore_ignores_deleted_layers() {
        // Layers gone from the new file version stay gone — restore only
        // touches entries present post-merge, never resurrects.
        let (mut doc, _) = xref_doc_with("PLAN", "plan.dwg");
        let mut layer = codec::tables::Layer::new("PLAN|GONE");
        layer.flags.off = true;
        doc.layers.add_or_replace(layer);
        let kept = snapshot_dependent_layers(&doc, "PLAN");
        doc.layers.remove("PLAN|GONE");
        restore_dependent_layers(&mut doc, "PLAN", kept);
        assert!(doc.layers.get("PLAN|GONE").is_none());
    }

    #[test]
    fn replace_prefix_rewrites_all() {
        let (mut doc, _) = xref_doc_with("PLAN", "refs/old/plan.dwg");
        let n = replace_path_prefix(&mut doc, "refs/old", "refs/new");
        assert_eq!(n, 1);
        assert_eq!(
            doc.block_records.get("PLAN").unwrap().xref_path,
            "refs/new/plan.dwg"
        );
    }

    #[test]
    fn replace_prefix_dotdot_saved_path() {
        // `..`-laden saved path normalizes to the old prefix; the splice must
        // follow normalized components, not raw bytes.
        let (mut doc, _) = xref_doc_with("PLAN", "refs/a/../old/plan.dwg");
        let n = replace_path_prefix(&mut doc, "refs/old", "refs/new");
        assert_eq!(n, 1);
        assert_eq!(
            doc.block_records.get("PLAN").unwrap().xref_path,
            "refs/new/plan.dwg"
        );
    }

    #[test]
    #[cfg(windows)]
    fn replace_prefix_case_folded_match() {
        let (mut doc, _) = xref_doc_with("PLAN", "REFS/OLD/plan.dwg");
        let n = replace_path_prefix(&mut doc, "refs/old", "refs/new");
        assert_eq!(n, 1);
        assert_eq!(
            doc.block_records.get("PLAN").unwrap().xref_path,
            "refs/new/plan.dwg"
        );
    }

    #[test]
    fn replace_prefix_multibyte_no_panic() {
        // Old code cut the RAW string by `old.len()` bytes after matching on
        // normalized forms — a normalized match with a shorter raw prefix
        // panics on a mid-char cut. Component-wise rejoin never slices raw.
        // Here old `a/` normalizes to `a`, which prefixes normalized
        // `aé/x.dwg`; raw cut at 2 lands inside `é` → old code panicked.
        let (mut doc, _) = xref_doc_with("PLAN", "aé/x.dwg");
        let n = replace_path_prefix(&mut doc, "a/", "q/");
        assert_eq!(n, 0);
        assert_eq!(
            doc.block_records.get("PLAN").unwrap().xref_path,
            "aé/x.dwg"
        );
        let (mut doc2, _) = xref_doc_with("PLAN", "répértoire/old/plan.dwg");
        let n2 = replace_path_prefix(&mut doc2, "répértoire/old", "répértoire/new");
        assert_eq!(n2, 1);
        assert_eq!(
            doc2.block_records.get("PLAN").unwrap().xref_path,
            "répértoire/new/plan.dwg"
        );
    }

    #[test]
    fn stale_only_after_cached_baseline() {
        use std::collections::{HashMap, HashSet};
        use std::time::{Duration, UNIX_EPOCH};
        // No fs: drive decide_status directly via fabricated times is covered
        // in xref_model; here assert the collect wrapper threads `prev`
        // through without touching the filesystem (missing file → NotFound,
        // never Stale even with a prev entry).
        let (doc, key) = xref_doc_with("PLAN", "definitely-missing-xyz.dwg");
        let mut prev: HashMap<u64, SystemTimeAlias> = HashMap::new();
        prev.insert(key, UNIX_EPOCH + Duration::from_secs(1));
        let entries = super::collect_entries_with_prev(
            &doc,
            std::path::Path::new("."),
            &HashSet::new(),
            &prev,
        );
        assert_eq!(entries[0].status, RefStatus::NotFound);
    }

    #[allow(dead_code)]
    type SystemTimeAlias = std::time::SystemTime;

    // ── BIND (Task 9) ────────────────────────────────────────────────────
    mod bind_tests {
        use super::super::bind_reference;
        use super::xref_doc_with;

        fn bind_dir(tag: &str) -> std::path::PathBuf {
            let dir = std::env::temp_dir().join(format!(
                "ocs_xref_bind_{tag}_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            dir
        }

        fn write_xref_file(
            dir: &std::path::Path,
            name: &str,
            build: impl FnOnce(&mut codec::CadDocument),
        ) -> String {
            let mut doc = codec::CadDocument::new();
            build(&mut doc);
            let bytes = crate::io::save_to_bytes(&doc, "dwg", doc.version).expect("save xref");
            let path = dir.join(name);
            std::fs::write(&path, &bytes).expect("write xref");
            path.to_string_lossy().into_owned()
        }

        fn line_on(layer: &str) -> codec::EntityType {
            let mut line = codec::entities::Line::new();
            line.common.layer = layer.to_string();
            codec::EntityType::Line(line)
        }

        fn add_layer(doc: &mut codec::CadDocument, name: &str) {
            doc.layers
                .add(codec::tables::Layer::new(name))
                .expect("add layer");
        }

        #[test]
        fn bind_rename_basic() {
            let dir = bind_dir("basic");
            let xref_path = write_xref_file(&dir, "plan.dwg", |doc| {
                add_layer(doc, "WALLS");
                doc.add_entity(line_on("WALLS")).unwrap();
            });
            let (mut host, key) = xref_doc_with("PLAN", &xref_path);
            let outcome = bind_reference(&mut host, key, &dir, &dir).expect("bind");
            assert_eq!(outcome.name, "PLAN");
            assert_eq!(outcome.unremapped, 0);
            let br = host.block_records.get("PLAN").unwrap();
            assert!(!br.flags.is_xref && !br.flags.is_xref_overlay);
            assert!(br.xref_path.is_empty());
            assert!(host.layers.get("PLAN$0$WALLS").is_some());
            let owned: Vec<_> = host
                .entities()
                .filter(|e| e.common().owner_handle == br.handle)
                .collect();
            assert_eq!(owned.len(), 1);
            assert_eq!(owned[0].common().layer, "PLAN$0$WALLS");
            assert!(!host.layers.names().any(|n| n.contains('|')));
            std::fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn bind_collision_increments() {
            let dir = bind_dir("collision");
            let xref_path = write_xref_file(&dir, "plan.dwg", |doc| {
                add_layer(doc, "WALLS");
                doc.add_entity(line_on("WALLS")).unwrap();
            });
            let (mut host, key) = xref_doc_with("PLAN", &xref_path);
            add_layer(&mut host, "PLAN$0$WALLS");
            let outcome = bind_reference(&mut host, key, &dir, &dir).expect("bind");
            assert_eq!(outcome.name, "PLAN");
            assert!(host.layers.get("PLAN$1$WALLS").is_some());
            let br = host.block_records.get("PLAN").unwrap();
            let owned: Vec<_> = host
                .entities()
                .filter(|e| e.common().owner_handle == br.handle)
                .collect();
            assert_eq!(owned.len(), 1);
            assert_eq!(owned[0].common().layer, "PLAN$1$WALLS");
            std::fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn bind_transitive_chain() {
            let dir = bind_dir("chain");
            let detail_path = write_xref_file(&dir, "detail.dwg", |doc| {
                add_layer(doc, "WALLS");
                doc.add_entity(line_on("WALLS")).unwrap();
            });
            let plan_path = write_xref_file(&dir, "plan.dwg", |doc| {
                add_layer(doc, "PLANLAYER");
                doc.add_entity(line_on("PLANLAYER")).unwrap();
                let mut br = codec::tables::BlockRecord::new("DETAIL");
                br.flags.is_xref = true;
                br.xref_path = detail_path.clone();
                doc.block_records.add(br).expect("add nested xref");
            });
            let (mut host, key) = xref_doc_with("PLAN", &plan_path);
            let outcome = bind_reference(&mut host, key, &dir, &dir).expect("bind");
            assert_eq!(outcome.name, "PLAN");
            assert!(host.layers.get("PLAN$0$DETAIL$0$WALLS").is_some());
            assert!(host.layers.get("PLAN$0$PLANLAYER").is_some());
            let child = host.block_records.get("PLAN$0$DETAIL").expect("bound child block");
            assert!(!child.flags.is_xref && !child.flags.is_xref_overlay);
            assert!(!host
                .block_records
                .iter()
                .any(|b| b.flags.is_xref || b.flags.is_xref_overlay));
            std::fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn bind_remaps_hatch_dim_block_refs() {
            let dir = bind_dir("refs");
            let xref_path = write_xref_file(&dir, "plan.dwg", |doc| {
                add_layer(doc, "WALLS");
                // Nested block with content + a model-space INSERT of it.
                let mut door = codec::tables::BlockRecord::new("DOOR");
                door.handle = doc.allocate_handle();
                let door_h = door.handle;
                doc.block_records.add(door).expect("add door");
                let mut inner = codec::entities::Line::new();
                inner.common.layer = "WALLS".to_string();
                inner.common.owner_handle = door_h;
                doc.add_entity(codec::EntityType::Line(inner)).unwrap();
                let ins = codec::entities::Insert::new(
                    "DOOR",
                    codec::types::Vector3::ZERO,
                );
                doc.add_entity(codec::EntityType::Insert(ins)).unwrap();
                // Hatch (pattern names have no table — preserved verbatim).
                let mut hatch = codec::entities::Hatch::new();
                hatch.pattern.name = "ANSI31".to_string();
                doc.add_entity(codec::EntityType::Hatch(hatch)).unwrap();
                // Dim-style reference via a tolerance entity.
                doc.dim_styles
                    .add(codec::tables::DimStyle::new("ARCH"))
                    .expect("add dimstyle");
                let mut tol = codec::entities::Tolerance::new();
                tol.dimension_style_name = "ARCH".to_string();
                doc.add_entity(codec::EntityType::Tolerance(tol)).unwrap();
            });
            let (mut host, key) = xref_doc_with("PLAN", &xref_path);
            bind_reference(&mut host, key, &dir, &dir).expect("bind");
            let door = host.block_records.get("PLAN$0$DOOR").expect("bound block");
            assert!(!door.flags.is_xref);
            let inserts: Vec<_> = host
                .entities()
                .filter_map(|e| match e {
                    codec::EntityType::Insert(i) => Some(i.block_name.clone()),
                    _ => None,
                })
                .collect();
            assert!(inserts.iter().any(|n| n == "PLAN$0$DOOR"), "{inserts:?}");
            let door_lines: Vec<_> = host
                .entities()
                .filter(|e| e.common().owner_handle == door.handle)
                .collect();
            assert_eq!(door_lines.len(), 1);
            assert_eq!(door_lines[0].common().layer, "PLAN$0$WALLS");
            let br = host.block_records.get("PLAN").unwrap();
            let hatches: Vec<_> = host
                .entities()
                .filter(|e| {
                    e.common().owner_handle == br.handle
                        && matches!(e, codec::EntityType::Hatch(_))
                })
                .collect();
            assert_eq!(hatches.len(), 1);
            assert!(host.dim_styles.get("PLAN$0$ARCH").is_some());
            let tols: Vec<_> = host
                .entities()
                .filter_map(|e| match e {
                    codec::EntityType::Tolerance(t) => Some(t.dimension_style_name.clone()),
                    _ => None,
                })
                .collect();
            assert_eq!(tols, vec!["PLAN$0$ARCH".to_string()]);
            std::fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn bind_remaps_text_styles_and_counts_unmapped() {
            let dir = bind_dir("styles");
            let xref_path = write_xref_file(&dir, "plan.dwg", |doc| {
                doc.text_styles
                    .add(codec::tables::TextStyle::new("NOTE"))
                    .expect("add text style");
                let mut text = codec::entities::Text::new();
                text.style = "NOTE".to_string();
                doc.add_entity(codec::EntityType::Text(text)).unwrap();
                // Two foreign handles that cannot be remapped (plotstyle_flags
                // set as a real writer would — otherwise the round-trip
                // drops the handle).
                let mut line = codec::entities::Line::new();
                line.common.plotstyle_flags = 0b11;
                line.common.plotstyle_handle = Some(doc.allocate_handle());
                doc.add_entity(codec::EntityType::Line(line)).unwrap();
                let mut mline = codec::entities::MLine::new();
                mline.style_name = "STD".to_string();
                mline.style_handle = Some(doc.allocate_handle());
                doc.add_entity(codec::EntityType::MLine(mline)).unwrap();
            });
            let (mut host, key) = xref_doc_with("PLAN", &xref_path);
            let outcome = bind_reference(&mut host, key, &dir, &dir).expect("bind");
            assert_eq!(outcome.unremapped, 2);
            assert!(host.text_styles.get("PLAN$0$NOTE").is_some());
            let styles: Vec<_> = host
                .entities()
                .filter_map(|e| match e {
                    codec::EntityType::Text(t) => Some(t.style.clone()),
                    _ => None,
                })
                .collect();
            assert_eq!(styles, vec!["PLAN$0$NOTE".to_string()]);
            let br = host.block_records.get("PLAN").unwrap();
            let lines: Vec<_> = host
                .entities()
                .filter(|e| {
                    e.common().owner_handle == br.handle
                        && matches!(e, codec::EntityType::Line(_))
                })
                .collect();
            assert_eq!(lines.len(), 1);
            assert!(lines[0].common().plotstyle_handle.is_none());
            let mlines: Vec<_> = host
                .entities()
                .filter_map(|e| match e {
                    codec::EntityType::MLine(m) => Some(m.style_handle),
                    _ => None,
                })
                .collect();
            assert_eq!(mlines, vec![None]);
            std::fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn bind_image_collect_copies_file() {
            use codec::objects::{ImageDefinition, ObjectType};
            let src_dir = bind_dir("imgsrc");
            let host_dir = bind_dir("imghost");
            std::fs::write(src_dir.join("pixel.png"), b"fakepng").unwrap();
            let xref_path = write_xref_file(&src_dir, "plan.dwg", |doc| {
                let h = doc.allocate_handle();
                let mut def = ImageDefinition::with_dimensions("pixel.png", 8, 8);
                def.handle = h;
                doc.objects.insert(h, ObjectType::ImageDefinition(def));
                let mut img = codec::entities::RasterImage::new(
                    "pixel.png",
                    codec::types::Vector3::ZERO,
                    8.0,
                    8.0,
                );
                img.definition_handle = Some(h);
                doc.add_entity(codec::EntityType::RasterImage(img)).unwrap();
            });
            let (mut host, key) = xref_doc_with("PLAN", &xref_path);
            bind_reference(&mut host, key, &src_dir, &host_dir).expect("bind");
            let copied = host_dir.join("pixel.png");
            assert!(copied.exists());
            assert_eq!(std::fs::read(&copied).unwrap(), b"fakepng");
            let defs: Vec<_> = host
                .objects
                .values()
                .filter_map(|o| match o {
                    ObjectType::ImageDefinition(d) => Some(d.file_name.clone()),
                    _ => None,
                })
                .collect();
            assert_eq!(defs, vec!["pixel.png".to_string()]);
            std::fs::remove_dir_all(&src_dir).ok();
            std::fs::remove_dir_all(&host_dir).ok();
        }

        #[test]
        fn bind_image_missing_errors_without_partial_state() {
            use codec::objects::{ImageDefinition, ObjectType};
            let dir = bind_dir("imgmissing");
            let xref_path = write_xref_file(&dir, "plan.dwg", |doc| {
                let h = doc.allocate_handle();
                let mut def = ImageDefinition::with_dimensions("ghost.png", 8, 8);
                def.handle = h;
                doc.objects.insert(h, ObjectType::ImageDefinition(def));
                let mut img = codec::entities::RasterImage::new(
                    "ghost.png",
                    codec::types::Vector3::ZERO,
                    8.0,
                    8.0,
                );
                img.definition_handle = Some(h);
                doc.add_entity(codec::EntityType::RasterImage(img)).unwrap();
            });
            let (mut host, key) = xref_doc_with("PLAN", &xref_path);
            let err = bind_reference(&mut host, key, &dir, &dir).unwrap_err();
            assert!(err.contains("ghost.png"), "got: {err:?}");
            // No partial state: the definition is untouched.
            let br = host.block_records.get("PLAN").unwrap();
            assert!(br.flags.is_xref);
            assert_eq!(br.xref_path, xref_path);
            assert!(!host.layers.names().any(|n| n.contains("PLAN$0")));
            std::fs::remove_dir_all(&dir).ok();
        }

        #[test]
        fn bind_pdf_errors() {
            use codec::objects::{ObjectType, UnderlayDefinition};
            let mut host = codec::CadDocument::new();
            let h = host.allocate_handle();
            let mut def = UnderlayDefinition::pdf("doc.pdf", "1");
            def.handle = h;
            host.objects.insert(h, ObjectType::UnderlayDefinition(def));
            let err =
                bind_reference(&mut host, h.value(), std::path::Path::new("."), std::path::Path::new("."))
                    .unwrap_err();
            assert_eq!(
                err,
                "XREF: PDF bind (vector import) is not available in this version."
            );
        }

        #[test]
        fn bind_unknown_key_errors() {
            let (mut host, _) = xref_doc_with("PLAN", "missing.dwg");
            let err = bind_reference(
                &mut host,
                0xDEAD_BEEF,
                std::path::Path::new("."),
                std::path::Path::new("."),
            )
            .unwrap_err();
            assert_eq!(err, "XREF: no loaded reference with that key.");
        }
    }
}
