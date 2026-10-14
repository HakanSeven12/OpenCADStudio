//! The shared block-definition service: delete, rename and purge.
//!
//! Every path that removes or renames a block definition — the command line
//! (`RENAME BLOCK`), the Blocks palette, the plugin API
//! (`TableOperation::BlockDelete` / `BlockRename`), the control/"MCP"
//! `block_delete` op and `PURGE` — routes through this module so the same
//! definition gets the same answer, the same error text and the same undo
//! label everywhere.
//!
//! The module lives next to [`crate::scene::entity`] rather than in
//! `src/app` because everything it needs is document/scene level: a
//! [`CadDocument`] for the reference scan and a [`Scene`] for the erase. The
//! callers own the app-level concerns (undo snapshots, command-line output,
//! panel refreshes).

use codec::objects::ObjectType;
use codec::tables::BlockRecord;
use codec::types::Handle;
use codec::{CadDocument, EntityType, TableEntry};
use std::collections::HashSet;
use std::fmt;

use crate::scene::render_graph::{entity_block_uses, BlockRole, BlockUse};
use crate::scene::Scene;

/// One reference a definition has. The kind names the relationship, `source`
/// is the handle of the entity or object that holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockReferenceKind {
    /// An INSERT, in model space or nested inside another definition.
    Insert,
    /// A dimension's generated picture block.
    DimensionPicture,
    /// The anonymous picture block a table draws through.
    TablePicture,
    /// A block-valued table cell.
    TableCell,
    /// A multi-leader's block content.
    MultiLeaderContent,
    /// A dimension-style / leader / multi-leader arrow block.
    ArrowHead,
    /// A DGN line-style symbol block.
    LineStyleSymbol,
    /// Extension point for dynamic-block variant families (visibility states,
    /// flip/stretch action sources, `*U` records). Detectors push these
    /// variants through [`dynamic_variant_references`]; the enum carries the
    /// catch-all so a new family never changes the call sites.
    DynamicVariant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockReference {
    pub kind: BlockReferenceKind,
    pub source: Handle,
}

/// Every reference to `name`, of every kind any current path counts.
///
/// The union of the paths' rules (see the reference matrix below):
///   * INSERTs anywhere in the flat document entity list — what the palette,
///     the control path and the plugin scan,
///   * dimension picture blocks, table pictures and block-valued table cells,
///     multi-leader block content and line-style symbols — what the render
///     graph (and so PURGE's reachability walk) counts,
///   * dimension-style / leader / multi-leader arrow blocks, by handle and by
///     name, plus multi-leader styles — the plugin's extra scan,
///   * dynamic-block variant families, through the extension point below.
pub fn references_of(document: &CadDocument, name: &str) -> Vec<BlockReference> {
    let Some(record) = document.block_records.get(name.trim()) else {
        return Vec::new();
    };
    let stored = record.name.clone();
    let target = record.handle;
    let mut out = Vec::new();
    for entity in document.entities() {
        for use_ in entity_block_uses(document, entity, 1.0) {
            if use_.covers(target, &stored) {
                out.push(BlockReference {
                    kind: kind_of(use_.role),
                    source: entity.common().handle,
                });
            }
        }
    }
    collect_style_references(document, target, &stored, &mut out);
    out.extend(dynamic_variant_references(document, record));
    out
}

/// Extension point for dynamic-block variant families. A detector for one
/// family (visibility states, action sources, `*U` records) returns the
/// references it finds; the block itself stays protected regardless, via
/// [`dynamic_block_handles`], which is the conservative fallback while a
/// family has no detector.
pub fn dynamic_variant_references(
    _document: &CadDocument,
    _record: &BlockRecord,
) -> Vec<BlockReference> {
    Vec::new()
}

fn kind_of(role: BlockRole) -> BlockReferenceKind {
    match role {
        BlockRole::Insert => BlockReferenceKind::Insert,
        BlockRole::DimensionPicture => BlockReferenceKind::DimensionPicture,
        BlockRole::TablePicture => BlockReferenceKind::TablePicture,
        BlockRole::TableCell => BlockReferenceKind::TableCell,
        BlockRole::MultiLeaderContent => BlockReferenceKind::MultiLeaderContent,
        BlockRole::ArrowHead => BlockReferenceKind::ArrowHead,
        BlockRole::LineStyleSymbol => BlockReferenceKind::LineStyleSymbol,
    }
}

impl BlockUse {
    fn covers(&self, target: Handle, stored: &str) -> bool {
        (!self.block.is_null() && self.block == target)
            || self.insert.block_name.eq_ignore_ascii_case(stored)
    }
}

/// References held by document-level styles and objects rather than by an
/// entity: dimension-style arrow blocks (by handle and by name),
/// multi-leader styles and DGN line-style symbols. Each entry names the
/// object that holds the reference so a caller can report or repoint it.
fn collect_style_references(
    document: &CadDocument,
    target: Handle,
    stored: &str,
    out: &mut Vec<BlockReference>,
) {
    for style in document.dim_styles.iter() {
        for handle in [style.dimblk, style.dimblk1, style.dimblk2, style.dimldrblk] {
            if handle == target {
                out.push(BlockReference {
                    kind: BlockReferenceKind::ArrowHead,
                    source: style.handle,
                });
            }
        }
        for name in [&style.dimblk_name, &style.dimblk1_name, &style.dimblk2_name] {
            if name.eq_ignore_ascii_case(stored) && document.block_records.get(name).is_some() {
                out.push(BlockReference {
                    kind: BlockReferenceKind::ArrowHead,
                    source: style.handle,
                });
            }
        }
    }
    for (handle, object) in document.objects.iter() {
        let ObjectType::MultiLeaderStyle(style) = object else {
            continue;
        };
        if style.arrowhead_handle == Some(target) {
            out.push(BlockReference {
                kind: BlockReferenceKind::ArrowHead,
                source: *handle,
            });
        }
        if style.block_content_handle == Some(target) {
            out.push(BlockReference {
                kind: BlockReferenceKind::MultiLeaderContent,
                source: *handle,
            });
        }
    }
    for line_type in document.line_types.iter() {
        for symbol in crate::scene::convert::dgn_linestyle::symbol_blocks(document, &line_type.name)
        {
            if symbol.block == target {
                out.push(BlockReference {
                    kind: BlockReferenceKind::LineStyleSymbol,
                    source: line_type.handle,
                });
            }
        }
    }
}

/// Why an operation is refused. One [`fmt::Display`] on
/// [`BlockServiceError`] renders the user-facing text, so every path shows
/// exactly the same sentence for the same scenario.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockServiceError {
    NotFound(String),
    InvalidName(String),
    NameExists(String),
    Referenced { name: String, count: usize },
    InEditSession(String),
    Protected(String),
}

impl fmt::Display for BlockServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockServiceError::NotFound(name) => write!(f, "block \"{name}\" does not exist"),
            BlockServiceError::InvalidName(message) => f.write_str(message),
            BlockServiceError::NameExists(name) => write!(f, "block \"{name}\" already exists"),
            BlockServiceError::Referenced { name, count } => write!(
                f,
                "block \"{name}\" is referenced {count} time(s) in the drawing and cannot be removed"
            ),
            BlockServiceError::InEditSession(name) => write!(
                f,
                "block \"{name}\" is open in the block editor; close the editor first"
            ),
            BlockServiceError::Protected(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for BlockServiceError {}

fn missing(name: &str) -> BlockServiceError {
    BlockServiceError::NotFound(name.to_owned())
}

/// The name rules the plugin path already applies to block names, now shared
/// by every path: 1-255 characters, no control characters and none of the
/// symbol-table-reserved characters (`*` marks anonymous blocks, `|`
/// xref-dependent ones; the rest are the DXF symbol-name exclusions). Names
/// are trimmed. Matching a name against the table is case-insensitive
/// (`Table::get` normalises to upper case).
pub fn validate_block_name(raw: &str) -> Result<String, BlockServiceError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(BlockServiceError::InvalidName(
            "a block name cannot be empty".to_owned(),
        ));
    }
    if name.chars().count() > 255 {
        return Err(BlockServiceError::InvalidName(
            "a block name is limited to 255 characters".to_owned(),
        ));
    }
    if let Some(bad) = name
        .chars()
        .find(|c| "<>/\\\":;?*|,=`".contains(*c) || c.is_control())
    {
        return Err(BlockServiceError::InvalidName(format!(
            "a block name cannot contain {bad:?}"
        )));
    }
    Ok(name.to_owned())
}

/// Records a direct delete or rename may never touch: the model / paper space
/// containers and layout blocks, externally referenced records and
/// dynamic-block records.
fn protected_reason(
    document: &CadDocument,
    record: &BlockRecord,
    dynamic: &HashSet<Handle>,
) -> Option<String> {
    if is_layout_record(document, record) {
        return Some(format!(
            "block \"{}\" is a model or paper space layout record",
            record.name
        ));
    }
    if record.flags.is_xref || record.flags.is_xref_overlay || record.name.contains('|') {
        return Some(format!(
            "block \"{}\" is an externally referenced block",
            record.name
        ));
    }
    if dynamic.contains(&record.handle) {
        return Some(format!(
            "block \"{}\" holds dynamic-block data",
            record.name
        ));
    }
    None
}

/// A layout record: a named `*`-space container or one backed by a Layout
/// object (paper space layouts and any orphaned layout record).
fn is_layout_record(document: &CadDocument, record: &BlockRecord) -> bool {
    let upper = record.name.to_ascii_uppercase();
    upper.starts_with("*MODEL_SPACE")
        || upper.starts_with("*PAPER_SPACE")
        || !record.layout.is_null()
        || matches!(
            document.objects.get(&record.layout),
            Some(ObjectType::Layout(_))
        )
}

/// Candidate block names for the Edit Block Definition (BEDIT) dialog:
/// excludes standard layouts (*MODEL_SPACE, *PAPER_SPACE), layout records,
/// anonymous blocks (*U, *D, *T, etc.), and externally referenced blocks.
/// Sorted alphabetically, case-insensitive.
pub fn bedit_candidate_block_names(document: &CadDocument) -> Vec<String> {
    let mut names: Vec<String> = document
        .block_records
        .iter()
        .filter(|record| {
            !record.is_standard()
                && !is_layout_record(document, record)
                && !record.is_anonymous()
                && !record.name.starts_with('*')
                && !record.flags.is_xref
                && !record.flags.is_xref_overlay
                && !record.name.contains('|')
        })
        .map(|record| record.name.clone())
        .collect();
    names.sort_by_key(|n| n.to_ascii_lowercase());
    names
}

/// The extension-dictionary key on a dynamic block's record.
const ENHANCED_BLOCK_KEY: &str = "ACAD_ENHANCEDBLOCK";

/// Whether a dictionary key names the enhanced-block (dynamic block) record.
/// Keys longer than the prefix count, so `ACAD_ENHANCEDBLOCK_…` variants from
/// newer writers are caught too; the comparison is case-insensitive because
/// dictionary keys are not normalised.
fn is_enhanced_block_key(key: &str) -> bool {
    key.len() >= ENHANCED_BLOCK_KEY.len()
        && key[..ENHANCED_BLOCK_KEY.len()].eq_ignore_ascii_case(ENHANCED_BLOCK_KEY)
}

/// Handles PURGE may not remove either: dynamic blocks (above), plus every
/// anonymous record a dynamic block sources its `*U` variants from.
fn dynamic_block_handles(document: &CadDocument) -> HashSet<Handle> {
    let mut protected: HashSet<Handle> = HashSet::new();
    for record in document.block_records.iter() {
        let Some(dictionary) = extension_dictionary(document, record.handle) else {
            continue;
        };
        if !dictionary
            .entries
            .iter()
            .any(|(key, _)| is_enhanced_block_key(key))
        {
            continue;
        }
        protected.insert(record.handle);
        collect_dictionary_tree(document, dictionary.handle, &mut protected);
    }
    let records: HashSet<Handle> = document.block_records.iter().map(|r| r.handle).collect();
    protected.retain(|handle| records.contains(handle));
    protected
}

/// Every handle reachable from a dynamic block's extension dictionary: the
/// record itself plus the anonymous records its visibility-state dictionary
/// points at, so a `*U` record is never purged out from under its block.
fn collect_dictionary_tree(document: &CadDocument, handle: Handle, out: &mut HashSet<Handle>) {
    if !out.insert(handle) {
        return;
    }
    let Some(ObjectType::Dictionary(dictionary)) = document.objects.get(&handle) else {
        return;
    };
    for (_, entry) in dictionary.entries.clone() {
        collect_dictionary_tree(document, entry, out);
    }
}

fn extension_dictionary(
    document: &CadDocument,
    owner: Handle,
) -> Option<&codec::objects::Dictionary> {
    match document
        .extension_dictionary_handle(owner)
        .and_then(|handle| document.objects.get(&handle))?
    {
        ObjectType::Dictionary(dictionary) => Some(dictionary),
        _ => None,
    }
}

/// Blocks PURGE may remove: every record that is not protected, not referenced
/// by any kind and not open in a block editor. The pass repeats through the
/// caller's loop (a removed definition frees whatever only its members
/// referenced), so this is one evaluation of the fixed point.
pub fn purge_candidates(document: &CadDocument, open_edits: &[Handle]) -> Vec<String> {
    let dynamic = dynamic_block_handles(document);
    document
        .block_records
        .iter()
        .filter(|record| {
            protected_reason(document, record, &dynamic).is_none()
                && !open_edits.contains(&record.handle)
                && references_of(document, &record.name).is_empty()
        })
        .map(|record| record.name.clone())
        .collect()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DeleteOptions<'a> {
    /// Erase the referencing INSERTs instead of refusing (the control/"MCP"
    /// `"force": true` request). References that are not INSERTs — a
    /// dimension-style arrow handle, a multi-leader's block content — are
    /// reported but cannot be erased.
    pub force: bool,
    /// Blocks with an open BEDIT session. Nested sessions all count, so this
    /// is the whole session stack, not just the active one.
    pub open_edits: &'a [Handle],
}

#[derive(Clone, Debug)]
pub struct DeleteOutcome {
    /// The stored name that was removed.
    pub name: String,
    pub handle: Handle,
    /// Every entity the operation removed: the definition's members, the
    /// BLOCK / ENDBLK markers and, on a force delete, the referencing INSERTs.
    pub erased: usize,
    /// References the delete overrode (empty unless forced).
    pub references: Vec<BlockReference>,
}

/// Remove one block definition and its child entities, markers and table
/// record. A referenced block is refused unless `opts.force` is set, in which
/// case the referencing INSERTs are erased in the same (undoable) step.
pub fn delete(
    scene: &mut Scene,
    name: &str,
    opts: DeleteOptions<'_>,
) -> Result<DeleteOutcome, BlockServiceError> {
    let record = record_of(&scene.document, name)?;
    let dynamic = dynamic_block_handles(&scene.document);
    if let Some(reason) = protected_reason(&scene.document, record, &dynamic) {
        return Err(BlockServiceError::Protected(reason));
    }
    if record.is_anonymous() {
        return Err(BlockServiceError::Protected(format!(
            "block \"{}\" is an anonymous block",
            record.name
        )));
    }
    if opts.open_edits.contains(&record.handle) {
        return Err(BlockServiceError::InEditSession(record.name.clone()));
    }
    let stored = record.name.clone();
    let handle = record.handle;
    let references = references_of(&scene.document, &stored);
    if !references.is_empty() && !opts.force {
        return Err(BlockServiceError::Referenced {
            name: stored,
            count: references.len(),
        });
    }
    let erased = erase(scene, &stored).unwrap_or(0);
    Ok(DeleteOutcome {
        name: stored,
        handle,
        erased,
        references,
    })
}

/// Erase a definition the caller has already cleared through
/// [`purge_candidates`]. Returns `Some(entities erased)` when the record was
/// there. This is the one erase every path shares, so a purge, a delete and a
/// force delete leave the same document behind.
pub fn erase(scene: &mut Scene, name: &str) -> Option<usize> {
    let stored = scene.document.block_records.get(name.trim())?.name.clone();
    let references = references_of(&scene.document, &stored);
    Some(erase_definition(scene, &stored, &references))
}

/// Erase the definition named `stored`: its members, both markers and the
/// referencing INSERTs in `references`. Returns how many entities went.
fn erase_definition(scene: &mut Scene, stored: &str, references: &[BlockReference]) -> usize {
    let Some(record) = scene.document.block_records.get(stored) else {
        return 0;
    };
    let mut victims: Vec<Handle> = record.entity_handles.clone();
    victims.push(record.block_entity_handle);
    victims.push(record.block_end_handle);
    for reference in references {
        if reference.kind == BlockReferenceKind::Insert {
            victims.push(reference.source);
        }
    }
    victims.sort_unstable();
    victims.dedup();
    let present: Vec<Handle> = victims
        .iter()
        .copied()
        .filter(|h| scene.document.get_entity(*h).is_some())
        .collect();
    let erased = present.len();
    scene.erase_entities(&present);
    // `erase_entities` honours layer locks; a member on a locked layer would
    // otherwise dangle against a record that no longer exists.
    let survivors: Vec<Handle> = present
        .iter()
        .copied()
        .filter(|h| scene.document.get_entity(*h).is_some())
        .collect();
    for handle in survivors {
        scene.document.remove_entity(handle);
    }
    scene.document.block_records.remove(stored);
    drop_orphaned_draw_order_tables(&mut scene.document);
    scene.invalidate_dependency_index();
    scene.bump_geometry();
    erased
}

/// Drop `SortEntitiesTable` objects whose owning block record is gone (a
/// stale draw-order table points at a block that no longer exists).
fn drop_orphaned_draw_order_tables(document: &mut CadDocument) {
    let live: HashSet<Handle> = document.block_records.iter().map(|r| r.handle).collect();
    let orphans: Vec<Handle> = document
        .objects
        .iter()
        .filter_map(|(handle, object)| match object {
            ObjectType::SortEntitiesTable(table) if !live.contains(&table.block_owner_handle) => {
                Some(*handle)
            }
            _ => None,
        })
        .collect();
    for handle in orphans {
        document.objects.remove(&handle);
    }
}

/// Rename a block definition: re-key the record, update the BLOCK marker,
/// repoint every INSERT and every name-based style reference (the
/// dimension-style arrow-block names). Handle-based references keep working
/// because the record's handle does not change.
pub fn rename(
    scene: &mut Scene,
    old: &str,
    new: &str,
    open_edits: &[Handle],
) -> Result<Handle, BlockServiceError> {
    let record = record_of(&scene.document, old)?;
    let dynamic = dynamic_block_handles(&scene.document);
    if let Some(reason) = protected_reason(&scene.document, record, &dynamic) {
        return Err(BlockServiceError::Protected(reason));
    }
    if record.is_anonymous() {
        return Err(BlockServiceError::Protected(format!(
            "block \"{}\" is an anonymous block",
            record.name
        )));
    }
    if open_edits.contains(&record.handle) {
        return Err(BlockServiceError::InEditSession(record.name.clone()));
    }
    let new = validate_block_name(new)?;
    let stored = record.name.clone();
    if new.eq_ignore_ascii_case(&stored) {
        return Err(BlockServiceError::InvalidName(
            "the new block name matches the current one (a case-only change is not supported)"
                .to_owned(),
        ));
    }
    if scene.document.block_records.get(&new).is_some() {
        return Err(BlockServiceError::NameExists(new));
    }
    let handle = record.handle;
    let marker = record.block_entity_handle;
    let mut record = scene
        .document
        .block_records
        .remove(&stored)
        .ok_or_else(|| missing(&stored))?;
    record.name = new.clone();
    if scene.document.block_records.add(record).is_err() {
        return Err(BlockServiceError::NameExists(new));
    }
    if let Some(EntityType::Block(block)) = scene.document.get_entity_mut(marker) {
        block.name = new.clone();
    }
    repoint_name_references(scene, &stored, &new);
    scene.invalidate_dependency_index();
    scene.bump_geometry();
    Ok(handle)
}

/// Every name-based reference follows a rename: INSERTs, a dimension's
/// picture-block name and the dimension styles' arrow-block names.
fn repoint_name_references(scene: &mut Scene, stored: &str, new: &str) {
    for entity in scene.document.entities_mut() {
        match entity {
            EntityType::Insert(insert) if insert.block_name.eq_ignore_ascii_case(stored) => {
                insert.block_name = new.to_owned();
            }
            EntityType::Dimension(dimension)
                if dimension.base().block_name.eq_ignore_ascii_case(stored) =>
            {
                dimension.base_mut().block_name = new.to_owned();
            }
            _ => {}
        }
    }
    for style in scene.document.dim_styles.iter_mut() {
        for field in [
            &mut style.dimblk_name,
            &mut style.dimblk1_name,
            &mut style.dimblk2_name,
        ] {
            if field.eq_ignore_ascii_case(stored) {
                *field = new.to_owned();
            }
        }
    }
}

fn record_of<'a>(
    document: &'a CadDocument,
    name: &str,
) -> Result<&'a BlockRecord, BlockServiceError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(BlockServiceError::InvalidName(
            "a block name cannot be empty".to_owned(),
        ));
    }
    document
        .block_records
        .get(name)
        .ok_or_else(|| missing(name))
}

/// Checks whether inserting `inserted_block` into `container_block` would create a circular
/// block nesting dependency, directly or transitively.
///
/// Matching is case-insensitive, consistent with the rest of the block service.
///
/// Xrefs (`|` in name or xref flags) and anonymous blocks (`*` prefix) are handled as follows:
/// - Anonymous blocks (`*U...`) are fully traversed like normal blocks because their geometry
///   and nested inserts live in the document.
/// - Externally referenced blocks (xrefs) have definitions that come from external files and
///   are not edited inline in the host document, but if an xref name is tested, it is matched
///   case-insensitively and its local block record entities (if any) are traversed.
pub fn would_create_cycle(
    document: &CadDocument,
    container_block: &str,
    inserted_block: &str,
) -> bool {
    let container = container_block.trim();
    let inserted = inserted_block.trim();
    if container.is_empty() || inserted.is_empty() {
        return false;
    }
    if container.eq_ignore_ascii_case(inserted) {
        return true;
    }

    let mut visited: HashSet<String> = HashSet::default();
    let mut stack: Vec<String> = vec![inserted.to_ascii_uppercase()];

    while let Some(current_upper) = stack.pop() {
        if !visited.insert(current_upper.clone()) {
            continue;
        }

        // Find block record matching current_upper case-insensitively
        let Some(record) = document.block_records.get(&current_upper) else {
            continue;
        };

        // Check entities owned by this block record: either registered in entity_handles
        // or having owner_handle == record.handle in the document.
        let mut sub_inserts = Vec::new();
        for &handle in &record.entity_handles {
            if let Some(EntityType::Insert(sub_ins)) = document.get_entity(handle) {
                sub_inserts.push(sub_ins.block_name.trim().to_string());
            }
        }
        for entity in document.entities() {
            if entity.common().owner_handle == record.handle {
                if let EntityType::Insert(sub_ins) = entity {
                    sub_inserts.push(sub_ins.block_name.trim().to_string());
                }
            }
        }

        for sub_name in sub_inserts {
            if sub_name.eq_ignore_ascii_case(container) {
                return true;
            }
            let sub_upper = sub_name.to_ascii_uppercase();
            if !visited.contains(&sub_upper) {
                stack.push(sub_upper);
            }
        }
    }

    false
}

pub const CIRCULAR_BLOCK_DEPENDENCY_ERROR: &str =
    "Cannot insert block: circular dependency detected.";

/// Standard command-line error text when block insertion would create a circular dependency.
pub fn circular_dependency_error(container_block: &str, inserted_block: &str) -> String {
    format!(
        "Cannot insert block \"{}\" into block \"{}\": circular dependency detected.",
        inserted_block, container_block
    )
}

#[cfg(test)]

mod tests {
    use super::*;
    use codec::entities::{
        Block as BlockEntity, BlockEnd, Dimension, DimensionAligned, Insert, Line, MultiLeader,
        Table,
    };
    use codec::objects::{
        DgnLsComponent, DgnLsComponentType, DgnLsDefinition, Dictionary, MultiLeaderStyle,
    };
    use codec::tables::{DimStyle, LineType};
    use codec::types::Vector3;

    fn scene() -> Scene {
        Scene::new()
    }

    /// One block definition with a single LINE member, its BLOCK and ENDBLK
    /// markers registered in the table (the codec's `add_entity` files the
    /// member under the record's `entity_handles` by owner).
    fn define(scene: &mut Scene, name: &str) -> Handle {
        let record = scene.document.allocate_handle();
        let marker = scene.document.allocate_handle();
        let end = scene.document.allocate_handle();
        let mut block_record = BlockRecord::new(name);
        block_record.handle = record;
        block_record.block_entity_handle = marker;
        block_record.block_end_handle = end;
        scene.document.block_records.add(block_record).unwrap();
        let mut block = BlockEntity::new(name, Vector3::ZERO);
        block.common.handle = marker;
        block.common.owner_handle = record;
        scene.document.add_entity(EntityType::Block(block)).unwrap();
        let mut block_end = BlockEnd::new();
        block_end.common.handle = end;
        block_end.common.owner_handle = record;
        scene
            .document
            .add_entity(EntityType::BlockEnd(block_end))
            .unwrap();
        let mut line = Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
        line.common.owner_handle = record;
        scene.document.add_entity(EntityType::Line(line)).unwrap();
        record
    }

    fn add_insert(scene: &mut Scene, name: &str, owner: Option<Handle>) {
        let mut insert = Insert::new(name.to_string(), Vector3::ZERO);
        if let Some(owner) = owner {
            insert.common.owner_handle = owner;
        }
        scene
            .document
            .add_entity(EntityType::Insert(insert))
            .unwrap();
    }

    fn has_kind(references: &[BlockReference], kind: BlockReferenceKind) -> bool {
        references.iter().any(|r| r.kind == kind)
    }

    /// The table is keyed by name, so a record handle is checked by scanning.
    fn record_exists(scene: &Scene, handle: Handle) -> bool {
        scene
            .document
            .block_records
            .iter()
            .any(|record| record.handle == handle)
    }

    #[test]
    fn references_of_reports_every_reference_kind() {
        let mut scene = scene();
        let target = define(&mut scene, "SVC_TARGET");
        // An INSERT in model space and one nested inside another definition:
        // the flat entity list covers both.
        add_insert(&mut scene, "SVC_TARGET", None);
        let outer = define(&mut scene, "SVC_OUTER");
        add_insert(&mut scene, "SVC_TARGET", Some(outer));

        // A dimension's picture block, by name.
        let mut dimension =
            DimensionAligned::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
        dimension.base.block_name = "SVC_TARGET".to_string();
        scene
            .document
            .add_entity(EntityType::Dimension(Dimension::Aligned(dimension)))
            .unwrap();

        // A table's picture block and a block-valued cell, by handle.
        let mut table = Table::new(Vector3::ZERO, 2, 2);
        table.block_record_handle = Some(target);
        let cell = table.cell_mut(0, 0).unwrap();
        cell.contents
            .push(codec::entities::table::CellContent::block(target));
        scene
            .document
            .add_entity(EntityType::Table(Box::new(table)))
            .unwrap();

        // A multi-leader's block content and arrowhead, by handle.
        let mut leader = MultiLeader::new();
        leader.content_type = codec::entities::LeaderContentType::Block;
        leader.context.has_block_contents = true;
        leader.block_content_handle = Some(target);
        leader.arrowhead_handle = Some(target);
        scene
            .document
            .add_entity(EntityType::MultiLeader(Box::new(leader)))
            .unwrap();

        // Document-level styles: a dimension style's arrow block by handle and
        // by name, and a multi-leader style's block content.
        let mut style = DimStyle::new("SVC_STYLE");
        style.dimblk = target;
        style.dimblk1_name = "SVC_TARGET".to_string();
        style.handle = Handle::new(900);
        scene.document.dim_styles.add(style).unwrap();
        let mut mleader_style = MultiLeaderStyle::new("SVC_MLSTYLE");
        mleader_style.handle = Handle::new(901);
        mleader_style.block_content_handle = Some(target);
        mleader_style.arrowhead_handle = Some(target);
        scene.document.objects.insert(
            mleader_style.handle,
            ObjectType::MultiLeaderStyle(mleader_style),
        );

        // A DGN line-style symbol block: the linetype name reaches the
        // definition, whose compound component points at the block.
        let (definition, root, symbol) = (Handle::new(910), Handle::new(911), Handle::new(912));
        scene.document.dgn_ls_definitions.insert(
            definition,
            DgnLsDefinition {
                handle: definition,
                name: "SVC_PIPE".to_string(),
                root_component: root,
            },
        );
        for (handle, component_type, refs) in [
            (root, DgnLsComponentType::Compound, vec![symbol]),
            (symbol, DgnLsComponentType::Symbol, vec![target]),
        ] {
            scene.document.dgn_ls_components.insert(
                handle,
                DgnLsComponent {
                    handle,
                    component_type,
                    description: String::new(),
                    refs,
                    scale: 1.0,
                },
            );
        }
        let mut line_type = LineType::new("SVC_PIPE");
        line_type.handle = Handle::new(902);
        scene.document.line_types.add(line_type).unwrap();

        let references = references_of(&scene.document, "SVC_TARGET");
        for kind in [
            BlockReferenceKind::Insert,
            BlockReferenceKind::DimensionPicture,
            BlockReferenceKind::TablePicture,
            BlockReferenceKind::TableCell,
            BlockReferenceKind::MultiLeaderContent,
            BlockReferenceKind::ArrowHead,
            BlockReferenceKind::LineStyleSymbol,
        ] {
            assert!(
                has_kind(&references, kind),
                "missing {kind:?} in {references:?}"
            );
        }
        assert!(
            references.iter().all(|r| !r.source.is_null()),
            "every reference names its source"
        );
        // 2 INSERTs + picture + table picture + cell + multileader content +
        // multileader arrow + style arrow by handle + style arrow by name +
        // multileader-style content + multileader-style arrow + line symbol
        assert_eq!(references.len(), 12);
    }

    #[test]
    fn rename_follows_every_name_based_reference() {
        let mut scene = scene();
        let target = define(&mut scene, "SVC_RENAMED");
        add_insert(&mut scene, "SVC_RENAMED", None);
        let mut dimension =
            DimensionAligned::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
        dimension.base.block_name = "SVC_RENAMED".to_string();
        scene
            .document
            .add_entity(EntityType::Dimension(Dimension::Aligned(dimension)))
            .unwrap();
        let mut style = DimStyle::new("SVC_STYLE");
        style.dimblk = target;
        style.dimblk_name = "SVC_RENAMED".to_string();
        scene.document.dim_styles.add(style).unwrap();

        rename(&mut scene, "SVC_RENAMED", "SVC_RENAMED_NEW", &[]).expect("rename should work");

        let names: Vec<String> = scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                EntityType::Insert(insert) => Some(insert.block_name.clone()),
                EntityType::Dimension(dimension) => Some(dimension.base().block_name.clone()),
                _ => None,
            })
            .collect();
        assert!(
            names.iter().all(|name| name == "SVC_RENAMED_NEW"),
            "a name-based reference was left behind: {names:?}"
        );
        let style = scene.document.dim_styles.get("SVC_STYLE").unwrap();
        assert_eq!(style.dimblk_name, "SVC_RENAMED_NEW");
        assert_eq!(
            style.dimblk, target,
            "a handle-based reference must survive a rename"
        );
        assert!(scene.document.block_records.get("SVC_RENAMED").is_none());
    }

    #[test]
    fn delete_refuses_layout_anonymous_xref_and_dynamic_records() {
        let mut scene = scene();
        let free = define(&mut scene, "SVC_FREE");
        define(&mut scene, "*SVC_ANON");
        define(&mut scene, "SVC_XREF");
        scene
            .document
            .block_records
            .get_mut("SVC_XREF")
            .unwrap()
            .flags
            .is_xref = true;
        define(&mut scene, "*Paper_Space1");
        scene
            .document
            .block_records
            .get_mut("*Paper_Space1")
            .unwrap()
            .layout = Handle::new(77);

        for (name, expected) in [
            ("SVC_FREE", None),
            (
                "*SVC_ANON",
                Some("block \"*SVC_ANON\" is an anonymous block"),
            ),
            (
                "SVC_XREF",
                Some("block \"SVC_XREF\" is an externally referenced block"),
            ),
            (
                "*Paper_Space1",
                Some("block \"*Paper_Space1\" is a model or paper space layout record"),
            ),
        ] {
            let error = delete(&mut scene, name, DeleteOptions::default())
                .err()
                .map(|e| e.to_string());
            assert_eq!(error.as_deref(), expected, "deleting {name}");
        }
        // The free block went on the first pass of the loop above, so a second
        // call reports the record as gone.
        assert_eq!(
            delete(&mut scene, "SVC_FREE", DeleteOptions::default())
                .unwrap_err()
                .to_string(),
            "block \"SVC_FREE\" does not exist"
        );
        assert!(scene.document.block_records.get("SVC_FREE").is_none());
        assert!(scene.document.block_records.get("*SVC_ANON").is_some());
        assert!(scene.document.block_records.get("SVC_XREF").is_some());
        assert!(scene.document.block_records.get("*Paper_Space1").is_some());
        assert!(!record_exists(&scene, free));
    }

    /// A record whose extension dictionary carries an `ACAD_ENHANCEDBLOCK`
    /// key, plus the `*U` anonymous record its variants dictionary points at.
    fn attach_dynamic_block(scene: &mut Scene, record: &str) -> Handle {
        let record_handle = scene.document.block_records.get(record).unwrap().handle;
        let variant = define(scene, "*U_VARIANT");
        let xdict = scene.document.allocate_handle();
        let variants = scene.document.allocate_handle();
        let mut dictionary = Dictionary::new();
        dictionary.handle = xdict;
        dictionary.owner = record_handle;
        dictionary.add_entry("ACAD_ENHANCEDBLOCK", variants);
        scene
            .document
            .objects
            .insert(xdict, ObjectType::Dictionary(dictionary));
        let mut variants_dictionary = Dictionary::new();
        variants_dictionary.handle = variants;
        variants_dictionary.owner = xdict;
        variants_dictionary.add_entry("VisibilityState1", variant);
        scene
            .document
            .objects
            .insert(variants, ObjectType::Dictionary(variants_dictionary));
        record_handle
    }

    #[test]
    fn a_dynamic_block_and_its_u_variants_are_protected() {
        let mut scene = scene();
        define(&mut scene, "SVC_DYN");
        let record = attach_dynamic_block(&mut scene, "SVC_DYN");
        let dynamic = dynamic_block_handles(&scene.document);
        assert!(dynamic.contains(&record), "the enhanced-block record");
        assert!(
            scene
                .document
                .block_records
                .get("*U_VARIANT")
                .is_some_and(|r| dynamic.contains(&r.handle)),
            "the *U variant sourced by the dictionary"
        );

        assert_eq!(
            delete(&mut scene, "SVC_DYN", DeleteOptions::default())
                .unwrap_err()
                .to_string(),
            "block \"SVC_DYN\" holds dynamic-block data"
        );
        assert_eq!(
            rename(&mut scene, "SVC_DYN", "SVC_PLAIN", &[])
                .unwrap_err()
                .to_string(),
            "block \"SVC_DYN\" holds dynamic-block data"
        );
        assert!(
            !purge_candidates(&scene.document, &[]).contains(&"SVC_DYN".to_string())
                && !purge_candidates(&scene.document, &[]).contains(&"*U_VARIANT".to_string()),
            "purge took a dynamic block or its variant"
        );
    }

    #[test]
    fn purge_candidates_takes_only_unreferenced_unprotected_records() {
        let mut scene = scene();
        let free = define(&mut scene, "SVC_PURGE_ME");
        let referenced = define(&mut scene, "SVC_KEEP");
        add_insert(&mut scene, "SVC_KEEP", None);
        let anonymous = define(&mut scene, "*SVC_ANON");
        let referenced_anonymous = define(&mut scene, "*SVC_ANON2");
        add_insert(&mut scene, "*SVC_ANON2", None);

        let candidates = purge_candidates(&scene.document, &[]);
        assert!(candidates.contains(&"SVC_PURGE_ME".to_string()));
        assert!(candidates.contains(&"*SVC_ANON".to_string()));
        assert!(!candidates.contains(&"SVC_KEEP".to_string()));
        assert!(!candidates.contains(&"*SVC_ANON2".to_string()));

        for name in &candidates {
            erase(&mut scene, name).expect("a candidate erases");
        }
        assert!(scene.document.block_records.get("SVC_PURGE_ME").is_none());
        assert!(scene.document.block_records.get("*SVC_ANON").is_none());
        assert!(scene.document.block_records.get("SVC_KEEP").is_some());
        assert!(scene.document.block_records.get("*SVC_ANON2").is_some());
        assert!(!record_exists(&scene, free));
        assert!(!record_exists(&scene, anonymous));
        assert!(record_exists(&scene, referenced));
        assert!(record_exists(&scene, referenced_anonymous));
        // A second pass finds nothing, so the caller's loop ends.
        assert!(purge_candidates(&scene.document, &[]).is_empty());
    }

    #[test]
    fn delete_and_rename_refuse_a_block_open_in_an_edit_session() {
        let mut scene = scene();
        let first = define(&mut scene, "SVC_EDIT_ONE");
        let second = define(&mut scene, "SVC_EDIT_TWO");
        let open = [first];
        assert_eq!(
            delete(
                &mut scene,
                "SVC_EDIT_ONE",
                DeleteOptions {
                    open_edits: &open,
                    ..Default::default()
                }
            )
            .unwrap_err(),
            BlockServiceError::InEditSession("SVC_EDIT_ONE".to_string())
        );
        assert_eq!(
            rename(&mut scene, "SVC_EDIT_ONE", "SVC_EDIT_THREE", &open).unwrap_err(),
            BlockServiceError::InEditSession("SVC_EDIT_ONE".to_string())
        );
        assert!(
            !purge_candidates(&scene.document, &open).contains(&"SVC_EDIT_ONE".to_string()),
            "purge skipped the open session"
        );
        // A different block is untouched by another block's session.
        assert!(delete(
            &mut scene,
            "SVC_EDIT_TWO",
            DeleteOptions {
                open_edits: &open,
                ..Default::default()
            }
        )
        .is_ok());
        assert!(scene.document.block_records.get("SVC_EDIT_TWO").is_none());
        assert!(!record_exists(&scene, second));
    }

    #[test]
    fn validation_rejects_reserved_characters_and_matches_case_insensitively() {
        assert_eq!(
            validate_block_name("  "),
            Err(BlockServiceError::InvalidName(
                "a block name cannot be empty".to_string()
            ))
        );
        for bad in ["bad/name", "a*b", "left|right", "tag=1", "q`uote"] {
            assert!(
                matches!(validate_block_name(bad), Err(BlockServiceError::InvalidName(message)) if message.contains("cannot contain")),
                "{bad} was accepted"
            );
        }
        assert_eq!(
            validate_block_name("  SVC_TRIM  "),
            Ok("SVC_TRIM".to_string())
        );

        let mut scene = scene();
        define(&mut scene, "SVC_LOWER");
        define(&mut scene, "SVC_EXISTING");
        // The table matches case-insensitively, so an upper-case rename target
        // collides with a lower-case record.
        assert_eq!(
            rename(&mut scene, "svc_lower", "SVC_EXISTING", &[]).unwrap_err(),
            BlockServiceError::NameExists("SVC_EXISTING".to_string())
        );
    }

    #[test]
    fn a_forced_delete_erases_the_referencing_inserts() {
        let mut scene = scene();
        define(&mut scene, "SVC_FORCED");
        add_insert(&mut scene, "SVC_FORCED", None);
        let outer = define(&mut scene, "SVC_FORCED_WRAPPER");
        add_insert(&mut scene, "SVC_FORCED", Some(outer));
        let outcome = delete(
            &mut scene,
            "SVC_FORCED",
            DeleteOptions {
                force: true,
                open_edits: &[],
            },
        )
        .expect("a forced delete erases the references");
        assert_eq!(outcome.references.len(), 2);
        // 2 INSERTs, the member LINE and both markers.
        assert_eq!(outcome.erased, 5);
        assert!(scene.document.block_records.get("SVC_FORCED").is_none());
        assert_eq!(
            scene
                .document
                .entities()
                .filter(|entity| matches!(entity, EntityType::Insert(insert) if insert.block_name == "SVC_FORCED"))
                .count(),
            0
        );
        assert!(scene
            .document
            .block_records
            .get("SVC_FORCED_WRAPPER")
            .is_some());
    }

    #[test]
    fn the_formatter_words_every_error_the_same_way() {
        assert_eq!(
            BlockServiceError::NotFound("A".to_string()).to_string(),
            "block \"A\" does not exist"
        );
        assert_eq!(
            BlockServiceError::Referenced {
                name: "A".to_string(),
                count: 2
            }
            .to_string(),
            "block \"A\" is referenced 2 time(s) in the drawing and cannot be removed"
        );
        assert_eq!(
            BlockServiceError::NameExists("A".to_string()).to_string(),
            "block \"A\" already exists"
        );
        assert_eq!(
            BlockServiceError::InEditSession("A".to_string()).to_string(),
            "block \"A\" is open in the block editor; close the editor first"
        );
    }

    #[test]
    fn locked_would_create_cycle_self_insert() {
        let mut scene = scene();
        define(&mut scene, "BLK_A");
        assert!(would_create_cycle(&scene.document, "BLK_A", "BLK_A"));
        assert!(would_create_cycle(&scene.document, "blk_a", "BLK_A"));
        assert!(would_create_cycle(&scene.document, "BLK_A", "blk_a"));
    }

    #[test]
    fn locked_would_create_cycle_direct_pair() {
        let mut scene = scene();
        let blk_a = define(&mut scene, "BLK_A");
        define(&mut scene, "BLK_B");
        // A contains B
        add_insert(&mut scene, "BLK_B", Some(blk_a));
        // Inserting A into B would create a cycle
        assert!(would_create_cycle(&scene.document, "BLK_B", "BLK_A"));
        // Inserting B into A is not allowed either since A already inserts B
        assert!(!would_create_cycle(&scene.document, "BLK_A", "BLK_B"));
    }

    #[test]
    fn locked_would_create_cycle_three_level_chain() {
        let mut scene = scene();
        let blk_a = define(&mut scene, "BLK_A");
        let blk_b = define(&mut scene, "BLK_B");
        define(&mut scene, "BLK_C");
        // A contains B, B contains C
        add_insert(&mut scene, "BLK_B", Some(blk_a));
        add_insert(&mut scene, "BLK_C", Some(blk_b));
        // Inserting A into C creates cycle C -> A -> B -> C
        assert!(would_create_cycle(&scene.document, "BLK_C", "BLK_A"));
        // Inserting B into C creates cycle C -> B -> C
        assert!(would_create_cycle(&scene.document, "BLK_C", "BLK_B"));
    }

    #[test]
    fn locked_would_create_cycle_shared_child_not_a_cycle() {
        let mut scene = scene();
        let blk_a = define(&mut scene, "BLK_A");
        let blk_b = define(&mut scene, "BLK_B");
        define(&mut scene, "BLK_SHARED");
        // A contains SHARED, B contains SHARED. Inserting B into A is fine (diamond/DAG, not cycle).
        add_insert(&mut scene, "BLK_SHARED", Some(blk_a));
        add_insert(&mut scene, "BLK_SHARED", Some(blk_b));
        assert!(!would_create_cycle(&scene.document, "BLK_A", "BLK_B"));
        assert!(!would_create_cycle(&scene.document, "BLK_B", "BLK_A"));
    }

    #[test]
    fn locked_would_create_cycle_case_differences() {
        let mut scene = scene();
        let blk_a = define(&mut scene, "BlockA");
        let _blk_b = define(&mut scene, "BlockB");
        add_insert(&mut scene, "BlockB", Some(blk_a));
        assert!(would_create_cycle(&scene.document, "blockb", "BLOCKA"));
        assert!(!would_create_cycle(&scene.document, "blocka", "BLOCKB"));
    }

    #[test]
    fn locked_bedit_candidate_block_names_filters_and_sorts() {
        let mut scene = scene();
        define(&mut scene, "zebra");
        define(&mut scene, "Alpha");
        define(&mut scene, "Beta");

        // Anonymous block
        let mut anon = BlockRecord::new("*U1");
        anon.handle = Handle::new(9901);
        anon.flags.anonymous = true;
        scene.document.block_records.add(anon).unwrap();

        // Xref block
        let mut xref = BlockRecord::new("XrefBlock");
        xref.handle = Handle::new(9902);
        xref.flags.is_xref = true;
        scene.document.block_records.add(xref).unwrap();

        // Xref dependent symbol
        let mut xref_dep = BlockRecord::new("HOST|CHILD");
        xref_dep.handle = Handle::new(9903);
        scene.document.block_records.add(xref_dep).unwrap();

        let candidates = bedit_candidate_block_names(&scene.document);
        assert_eq!(candidates, vec!["Alpha", "Beta", "zebra"]);
    }
}
