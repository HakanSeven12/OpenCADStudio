// REFEDIT / -REFEDIT / REFSET / REFCLOSE — in-place reference editing.
//
// Workflow:
//   1. REFEDIT (or -REFEDIT on the command line) picks a block reference or
//      xref; the nesting chain under the pick is offered innermost first and
//      `Next` steps outward.
//   2. The chosen definition's objects (all of them, or the nested objects
//      picked) are copied into the current space with the composed placement
//      of the chain. Everything else fades by XFADECTL.
//   3. The user edits them with normal commands; REFSET moves host objects
//      into the working set or working-set objects out to the host.
//   4. REFCLOSE Save writes the working set back into the definition (and
//      into the referenced file for an xref); Discard drops the copies.

use crate::t;
use codec::{EntityType, Handle};
use glam::DVec3;
use std::collections::HashSet;

use crate::command::{CadCommand, CmdOption, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};

/// Right-edge side-toolbar buttons that move objects into or out of the
/// working set while a REFEDIT session is active (REFSET Add / Remove).
pub fn refedit_set_tools() -> Vec<ToolDef> {
    vec![
        ToolDef {
            id: "REFSET_ADD",
            label: "Add to Working Set",
            icon: IconKind::Svg(include_bytes!("../../../../assets/icons/refset_add.svg")),
            event: ModuleEvent::Command("REFSET_ADD".to_string()),
        },
        ToolDef {
            id: "REFSET_REMOVE",
            label: "Remove from Working Set",
            icon: IconKind::Svg(include_bytes!("../../../../assets/icons/refset_remove.svg")),
            event: ModuleEvent::Command("REFSET_REMOVE".to_string()),
        },
    ]
}

/// Right-edge side-toolbar buttons shown while a REFEDIT session is active:
/// save the in-place block edit, or discard it. (#136)
pub fn refedit_tools() -> Vec<ToolDef> {
    vec![
        ToolDef {
            id: "REFCLOSE_SAVE",
            label: "Save Block Edit",
            icon: IconKind::Svg(include_bytes!("../../../../assets/icons/mt_ok.svg")),
            event: ModuleEvent::Command("REFCLOSE_SAVE".to_string()),
        },
        ToolDef {
            id: "REFCLOSE_DISCARD",
            label: "Discard Block Edit",
            icon: IconKind::Svg(include_bytes!("../../../../assets/icons/mt_cancel.svg")),
            event: ModuleEvent::Command("REFCLOSE_DISCARD".to_string()),
        },
    ]
}

/// Printed by REFSET and REFCLOSE outside a session.
pub const NOT_CHECKED_OUT: &str =
    "** Command not allowed unless a reference is checked out with REFEDIT command **";

// ── Session state (held in DocumentTab) ───────────────────────────────────

/// Active REFEDIT session.  Lives in `DocumentTab::refedit_session`.
#[derive(Debug, Clone)]
pub struct RefEditSession {
    /// Name of the definition being edited (REFEDITNAME).
    pub block_name: String,
    /// Handle of the block record (owns the block entities).
    pub br_handle: Handle,
    /// The working copies of the extracted definition objects.
    pub temp_handles: Vec<Handle>,
    /// The definition objects that were copied out, index-aligned with
    /// `temp_handles`. Objects not extracted (attribute definitions when they
    /// are not displayed, objects not picked in Nested mode) stay in the
    /// definition untouched.
    pub extracted: Vec<Handle>,
    /// Host objects moved into the working set with REFSET Add.
    pub added: HashSet<Handle>,
    /// Working-set objects moved out to the host with REFSET Remove.
    pub removed: HashSet<Handle>,
    /// Definition-local → world: the composed placement of the chain.
    pub forward: codec::types::Transform,
    /// World → definition-local, applied on SAVE to bring edits back.
    pub inverse: codec::types::Transform,
    /// Handle watermark captured before the temp copies were created: every
    /// entity allocated at or above it during the session (drawn, pasted,
    /// offset, …) belongs to the working set, so Save folds it into the block
    /// and Discard removes it (#423). Catches every creation path without
    /// per-command hooks.
    pub handle_watermark: u64,
    /// The referenced file when the definition is an xref.
    pub xref_path: Option<String>,
    /// Objects outside the working set can't be selected while it is open.
    pub lock: bool,
    /// The external reference's named objects the working set uses under
    /// host names, and the host records made for the session.
    pub names: RefEditNames,
}

/// A kind of named object the working set's objects refer to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Layer,
    Linetype,
    TextStyle,
    Block,
}

/// How an external reference's dependent names (`XREF|NAME`) are used in
/// the host while its objects are checked out: as `$0$NAME` records of
/// their own (unique names), or under the plain name, sharing a host record
/// of that name when there is one.
#[derive(Debug, Clone, Default)]
pub struct RefEditNames {
    /// (kind, name used in the host, the reference's dependent name).
    pub renamed: Vec<(SymbolKind, String, String)>,
    /// Records added to the host for the session, removed when it closes.
    pub created: Vec<(SymbolKind, String)>,
}

impl RefEditNames {
    /// The reference's own name for a host name used in the session.
    pub fn original(&self, kind: SymbolKind, name: &str) -> Option<&str> {
        self.renamed
            .iter()
            .find(|(k, used, _)| *k == kind && used.eq_ignore_ascii_case(name))
            .map(|(_, _, original)| original.as_str())
    }
}

/// Rewrites every named-object reference of an entity through `map`
/// (layer, linetype, text style, block).
pub fn rename_symbols(entity: &mut EntityType, map: &dyn Fn(SymbolKind, &str) -> Option<String>) {
    let common = entity.common_mut();
    if let Some(name) = map(SymbolKind::Layer, &common.layer) {
        common.layer = name;
        common.layer_handle = None;
    }
    if let Some(name) = map(SymbolKind::Linetype, &common.linetype) {
        common.linetype = name;
        common.linetype_handle = None;
    }
    match entity {
        EntityType::Insert(insert) => {
            if let Some(name) = map(SymbolKind::Block, &insert.block_name) {
                insert.block_name = name;
            }
        }
        EntityType::Text(text) => {
            if let Some(name) = map(SymbolKind::TextStyle, &text.style) {
                text.style = name;
            }
        }
        EntityType::MText(text) => {
            if let Some(name) = map(SymbolKind::TextStyle, &text.style) {
                text.style = name;
            }
        }
        EntityType::AttributeDefinition(attdef) => {
            if let Some(name) = map(SymbolKind::TextStyle, &attdef.text_style) {
                attdef.text_style = name;
            }
        }
        _ => {}
    }
}

impl RefEditSession {
    /// Whether `handle` (a host-space object) is in the working set.
    pub fn in_working_set(&self, handle: Handle) -> bool {
        if self.removed.contains(&handle) {
            return false;
        }
        self.temp_handles.contains(&handle)
            || self.added.contains(&handle)
            || handle.value() >= self.handle_watermark
    }
}

// ── Nesting chain under a pick ────────────────────────────────────────────

/// One level of the nesting chain: the reference (top-level object handle for
/// level 0, the definition-owned INSERT handle below it) and its definition.
#[derive(Debug, Clone)]
pub struct ChainLevel {
    pub insert: Handle,
    pub block_name: String,
    /// Definition-local → world for this level.
    pub forward: codec::types::Transform,
}

/// The pickable objects of one definition, in world space, for Nested
/// selection: (definition-owned handle, polyline paths of its leaves).
pub type LevelHits = Vec<(Handle, Vec<Vec<[f64; 3]>>)>;

/// World paths of an entity for hit-testing (curves sampled by the renderer;
/// block references recurse into their definitions).
pub fn hit_paths(
    entity: &EntityType,
    document: &codec::Document,
    depth: usize,
) -> Vec<Vec<[f64; 3]>> {
    use crate::entities::traits::EntityTypeOps;
    use crate::scene::convert::acad_to_render::RenderObject;
    if let EntityType::Insert(insert) = entity {
        if depth > 8 {
            return Vec::new();
        }
        let Some(br) = document.block_records.get(&insert.block_name) else {
            return Vec::new();
        };
        let base = crate::scene::render_graph::block_base_point(document, &insert.block_name);
        let placement = crate::scene::render_graph::insert_transform_with_base(insert, base);
        return br
            .entity_handles
            .iter()
            .filter_map(|h| document.get_entity(*h))
            .filter(|e| !matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_)))
            .flat_map(|child| {
                let mut child = child.clone();
                child.as_entity_mut().apply_transform(&placement);
                hit_paths(&child, document, depth + 1)
            })
            .collect();
    }
    match entity
        .to_render_entity(document)
        .map(|render| render.object)
    {
        Some(RenderObject::Dot(point)) => vec![vec![point]],
        Some(
            RenderObject::Lines(points)
            | RenderObject::SegmentedLines(points)
            | RenderObject::TaperedLines(points, _)
            | RenderObject::BoundaryLines { points, .. },
        ) => vec![points],
        _ => Vec::new(),
    }
}

/// Distance from `point` to a set of paths.
pub fn paths_distance(paths: &[Vec<[f64; 3]>], point: DVec3) -> Option<f64> {
    let p = kernel::space::Vec3::from(point.to_array());
    paths
        .iter()
        .filter_map(|path| {
            if path.len() == 1 {
                return Some(p.distance(kernel::space::Vec3::from(path[0])));
            }
            path.windows(2)
                .filter(|s| s.iter().flatten().all(|v| v.is_finite()))
                .map(|s| p.distance_to_segment(s[0].into(), s[1].into()))
                .min_by(f64::total_cmp)
        })
        .min_by(f64::total_cmp)
}

/// The world-space objects of `level`'s definition with their hit paths.
pub fn level_hits(document: &codec::Document, level: &ChainLevel) -> LevelHits {
    let Some(br) = document.block_records.get(&level.block_name) else {
        return Vec::new();
    };
    br.entity_handles
        .iter()
        .filter_map(|h| document.get_entity(*h).map(|e| (*h, e)))
        .filter(|(_, e)| {
            !matches!(
                e,
                EntityType::Block(_) | EntityType::BlockEnd(_) | EntityType::AttributeDefinition(_)
            )
        })
        .map(|(h, e)| {
            let mut world = e.clone();
            world.as_entity_mut().apply_transform(&level.forward);
            (h, hit_paths(&world, document, 0))
        })
        .collect()
}

/// The nesting chain of `top` under `point`, outermost first: the top-level
/// reference, then every definition-owned reference the nearest object under
/// the point sits in. Without a point only the top level is returned.
pub fn nesting_chain(
    document: &codec::Document,
    top: Handle,
    point: Option<DVec3>,
) -> Vec<ChainLevel> {
    let Some(EntityType::Insert(insert)) = document.get_entity(top) else {
        return Vec::new();
    };
    let base = crate::scene::render_graph::block_base_point(document, &insert.block_name);
    let mut chain = vec![ChainLevel {
        insert: top,
        block_name: insert.block_name.clone(),
        forward: crate::scene::render_graph::insert_transform_with_base(insert, base),
    }];
    let Some(point) = point else {
        return chain;
    };
    for _ in 0..16 {
        let level = chain
            .last()
            .expect("chain starts with the top level")
            .clone();
        let nearest = level_hits(document, &level)
            .into_iter()
            .filter_map(|(h, paths)| paths_distance(&paths, point).map(|d| (h, d)))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((handle, _)) = nearest else {
            break;
        };
        let Some(EntityType::Insert(inner)) = document.get_entity(handle) else {
            break;
        };
        if chain
            .iter()
            .any(|l| l.block_name.eq_ignore_ascii_case(&inner.block_name))
        {
            break;
        }
        let base = crate::scene::render_graph::block_base_point(document, &inner.block_name);
        let local = crate::scene::render_graph::insert_transform_with_base(inner, base);
        chain.push(ChainLevel {
            insert: handle,
            block_name: inner.block_name.clone(),
            forward: local.then(&level.forward),
        });
    }
    chain
}

// ── REFEDIT / -REFEDIT pick command ───────────────────────────────────────

/// `Select reference:` — a block reference or xref of the current space.
pub struct RefEditPickCommand {
    name: &'static str,
    references: HashSet<Handle>,
}

impl RefEditPickCommand {
    /// `command_line` selects -REFEDIT (prompts) over REFEDIT.
    pub fn new(command_line: bool, references: HashSet<Handle>) -> Self {
        Self {
            name: if command_line { "-REFEDIT" } else { "REFEDIT" },
            references,
        }
    }
}

impl CadCommand for RefEditPickCommand {
    fn name(&self) -> &'static str {
        self.name
    }
    fn prompt(&self) -> String {
        t!("Select reference:").into_owned()
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: Handle, pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::ReportError(t!("Reference not found.").into_owned());
        }
        if !self.references.contains(&handle) {
            return CmdResult::ReportError(t!("Object is not a valid reference").into_owned());
        }
        CmdResult::Relaunch(
            format!(
                "REFEDIT_PICKED:{}:{}:{}:{}:{}",
                (self.name == "-REFEDIT") as u8,
                handle.value(),
                pt.x,
                pt.y,
                pt.z
            ),
            vec![handle],
        )
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::ReportError(t!("Reference not found.").into_owned())
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::ReportError(t!("Reference not found.").into_owned())
    }
}

// ── -REFEDIT options after the pick ───────────────────────────────────────

enum OptionStep {
    Nesting,
    Method,
    Nested,
    Attributes,
}

/// `Select nesting level [Ok/Next] <Next>:` → `Enter object selection method
/// [All/Nested] <All>:` (→ `Select nested objects:`) → `Display attribute
/// definitions [Yes/No] <No>:`, then the session starts.
pub struct RefEditOptionsCommand {
    top: Handle,
    chain: Vec<ChainLevel>,
    hits: Vec<LevelHits>,
    level: usize,
    step: OptionStep,
    nested: Vec<Handle>,
    /// The session settings (lock objects outside the working set, unique
    /// names for an external reference's named objects).
    lock: bool,
    unique: bool,
    /// Set when the Edit Reference dialog already chose the attribute
    /// option: the nested pick then opens the session directly.
    attributes: Option<bool>,
    /// First corner of a window started by a pick on empty space.
    corner: Option<DVec3>,
}

impl RefEditOptionsCommand {
    pub fn new(
        top: Handle,
        chain: Vec<ChainLevel>,
        hits: Vec<LevelHits>,
        lock: bool,
        unique: bool,
    ) -> Self {
        let level = chain.len().saturating_sub(1);
        Self {
            top,
            chain,
            hits,
            level,
            step: OptionStep::Nesting,
            nested: Vec::new(),
            lock,
            unique,
            attributes: None,
            corner: None,
        }
    }

    /// `Select nested objects:` for the dialog's "pick in the drawing"
    /// working set at `level`.
    pub fn nested_pick(
        top: Handle,
        chain: Vec<ChainLevel>,
        hits: Vec<LevelHits>,
        level: usize,
        attributes: bool,
        lock: bool,
        unique: bool,
    ) -> Self {
        let mut command = Self::new(top, chain, hits, lock, unique);
        command.level = level.min(command.chain.len().saturating_sub(1));
        command.step = OptionStep::Nested;
        command.attributes = Some(attributes);
        command
    }

    fn begin(&self, attributes: bool) -> CmdResult {
        let path: Vec<String> = self.chain[1..=self.level]
            .iter()
            .map(|l| l.insert.value().to_string())
            .collect();
        let nested: Vec<String> = self.nested.iter().map(|h| h.value().to_string()).collect();
        CmdResult::Relaunch(
            format!(
                "REFEDIT_BEGIN:{}:{}:{}:{}:{}:{}:{}",
                self.top.value(),
                path.join(","),
                attributes as u8,
                nested.join(","),
                self.lock as u8,
                self.unique as u8,
                self.attributes.is_some() as u8
            ),
            vec![],
        )
    }

    /// A pick at `point`: the nearest object of the level within `tolerance`
    /// (any distance when None), else the first corner of a window.
    fn pick_nested(&mut self, point: DVec3, tolerance: Option<f64>) -> CmdResult {
        if let Some(first) = self.corner.take() {
            return self.window_nested(first, point);
        }
        let nearest = self
            .hits
            .get(self.level)
            .into_iter()
            .flatten()
            .filter_map(|(h, paths)| paths_distance(paths, point).map(|d| (*h, d)))
            .filter(|(_, d)| tolerance.is_none_or(|t| *d <= t))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match nearest {
            Some((h, _)) => {
                let added = !self.nested.contains(&h);
                if added {
                    self.nested.push(h);
                }
                CmdResult::ReportMeasurement(entities_added(added as usize))
            }
            None => {
                self.corner = Some(point);
                CmdResult::NeedPoint
            }
        }
    }

    /// The level's objects in the window from `first` to `second`: wholly
    /// inside when it was dragged to the right, touching it when to the left.
    fn window_nested(&mut self, first: DVec3, second: DVec3) -> CmdResult {
        let (lo, hi) = (first.min(second), first.max(second));
        let crossing = second.x < first.x;
        let inside = |p: &[f64; 3]| p[0] >= lo.x && p[0] <= hi.x && p[1] >= lo.y && p[1] <= hi.y;
        let picked: Vec<Handle> = self
            .hits
            .get(self.level)
            .into_iter()
            .flatten()
            .filter(|(_, paths)| {
                let points = || paths.iter().flatten();
                if crossing {
                    points().any(inside)
                        || paths.iter().any(|path| {
                            path.windows(2)
                                .any(|s| segment_meets_box(s[0], s[1], lo, hi))
                        })
                } else {
                    points().next().is_some() && points().all(inside)
                }
            })
            .map(|(h, _)| *h)
            .collect();
        let mut added = 0;
        for h in picked {
            if !self.nested.contains(&h) {
                self.nested.push(h);
                added += 1;
            }
        }
        CmdResult::ReportMeasurement(entities_added(added))
    }
}

impl CadCommand for RefEditOptionsCommand {
    fn name(&self) -> &'static str {
        "-REFEDIT"
    }
    fn prompt(&self) -> String {
        match self.step {
            OptionStep::Nesting => t!("Select nesting level [Ok/Next] <Next>:").into_owned(),
            OptionStep::Method => {
                t!("Enter object selection method [All/Nested] <All>:").into_owned()
            }
            OptionStep::Nested if self.corner.is_some() => {
                t!("Specify opposite corner:").into_owned()
            }
            OptionStep::Nested => t!("Select nested objects:").into_owned(),
            OptionStep::Attributes => {
                t!("Display attribute definitions [Yes/No] <No>:").into_owned()
            }
        }
    }
    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            OptionStep::Nesting => vec![CmdOption::new("Ok", "O"), CmdOption::new("Next", "N")],
            OptionStep::Method => vec![CmdOption::new("All", "A"), CmdOption::new("Nested", "N")],
            OptionStep::Nested => Vec::new(),
            OptionStep::Attributes => {
                vec![CmdOption::new("Yes", "Y"), CmdOption::new("No", "N")]
            }
        }
    }
    fn wants_text_input(&self) -> bool {
        !matches!(self.step, OptionStep::Nested)
    }
    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, OptionStep::Nested)
    }
    fn on_entity_pick(&mut self, handle: Handle, pt: DVec3) -> CmdResult {
        // A click on empty space starts a window, as does one beside the
        // reference's objects.
        if handle.is_null() {
            return self.pick_nested(pt, Some(0.0));
        }
        self.pick_nested(pt, None)
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let word = text.trim().to_ascii_uppercase();
        if word.is_empty() {
            return None;
        }
        let matches = |full: &str| full.starts_with(word.as_str());
        Some(match self.step {
            OptionStep::Nesting if matches("OK") => {
                self.step = OptionStep::Method;
                CmdResult::NeedPoint
            }
            OptionStep::Nesting if matches("NEXT") => self.on_enter(),
            OptionStep::Method if matches("ALL") => self.on_enter(),
            OptionStep::Method if matches("NESTED") => {
                self.step = OptionStep::Nested;
                CmdResult::NeedPoint
            }
            OptionStep::Attributes if matches("YES") => self.begin(true),
            OptionStep::Attributes if matches("NO") => self.begin(false),
            OptionStep::Nested => return None,
            _ => CmdResult::ReportError(t!("Invalid option keyword.").into_owned()),
        })
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            // A typed point picks the object it lies on.
            OptionStep::Nested => self.pick_nested(pt, Some(1e-6 * pt.length().max(1.0))),
            _ => CmdResult::NeedPoint,
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            OptionStep::Nesting => {
                // Next steps outward and wraps back to the innermost level.
                self.level = if self.level == 0 {
                    self.chain.len().saturating_sub(1)
                } else {
                    self.level - 1
                };
                CmdResult::NeedPoint
            }
            OptionStep::Method => {
                self.step = OptionStep::Attributes;
                CmdResult::NeedPoint
            }
            OptionStep::Nested if self.corner.take().is_some() => CmdResult::NeedPoint,
            OptionStep::Nested => {
                let n = self.nested.len();
                if n == 0 {
                    return CmdResult::CancelWithMessage(t!("0 items selected").into_owned());
                }
                if let Some(attributes) = self.attributes {
                    // The count and the session's note follow as it opens.
                    return self.begin(attributes);
                }
                self.step = OptionStep::Attributes;
                CmdResult::ReportMeasurement(if n == 1 {
                    t!("1 item selected").into_owned()
                } else {
                    crate::tf!("{} items selected", n).into_owned()
                })
            }
            OptionStep::Attributes => self.begin(false),
        }
    }
}

/// `N entity added` / `N entities added`.
fn entities_added(n: usize) -> String {
    if n == 1 {
        t!("1 entity added").into_owned()
    } else {
        crate::tf!("{} entities added", n).into_owned()
    }
}

/// Whether the segment from `a` to `b` crosses the box `lo`..`hi` (XY).
fn segment_meets_box(a: [f64; 3], b: [f64; 3], lo: DVec3, hi: DVec3) -> bool {
    // Liang–Barsky clip of the segment against the box.
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let d = [b[0] - a[0], b[1] - a[1]];
    for (p, q) in [
        (-d[0], a[0] - lo.x),
        (d[0], hi.x - a[0]),
        (-d[1], a[1] - lo.y),
        (d[1], hi.y - a[1]),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

// ── REFSET ────────────────────────────────────────────────────────────────

/// `Enter an option [Add/Remove] <Add>:` then `Select objects:`.
pub struct RefSetCommand {
    add: bool,
    gathering: bool,
    selected: Vec<Handle>,
}

impl RefSetCommand {
    pub fn new() -> Self {
        Self {
            add: true,
            gathering: false,
            selected: Vec::new(),
        }
    }

    /// Straight to `Select objects:` for Add (`add`) or Remove.
    pub fn selecting(add: bool) -> Self {
        Self {
            add,
            gathering: true,
            selected: Vec::new(),
        }
    }
}

impl CadCommand for RefSetCommand {
    fn name(&self) -> &'static str {
        "REFSET"
    }
    fn prompt(&self) -> String {
        if self.gathering {
            t!("Select objects:").into_owned()
        } else {
            t!("Enter an option [Add/Remove] <Add>:").into_owned()
        }
    }
    fn options(&self) -> Vec<CmdOption> {
        if self.gathering {
            Vec::new()
        } else {
            vec![CmdOption::new("Add", "A"), CmdOption::new("Remove", "R")]
        }
    }
    fn wants_text_input(&self) -> bool {
        !self.gathering
    }
    fn is_selection_gathering(&self) -> bool {
        self.gathering
    }
    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        self.selected = handles;
        CmdResult::NeedPoint
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.gathering {
            return None;
        }
        let word = text.trim().to_ascii_uppercase();
        if word.is_empty() {
            return None;
        }
        Some(if "ADD".starts_with(&word) {
            self.gathering = true;
            CmdResult::NeedPoint
        } else if "REMOVE".starts_with(&word) {
            self.add = false;
            self.gathering = true;
            CmdResult::NeedPoint
        } else {
            CmdResult::ReportError(t!("Invalid option keyword.").into_owned())
        })
    }
    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        if !self.gathering {
            self.gathering = true;
            return CmdResult::NeedPoint;
        }
        if self.selected.is_empty() {
            return CmdResult::Cancel;
        }
        CmdResult::Relaunch(
            format!("REFSET_APPLY:{}", if self.add { "ADD" } else { "REMOVE" }),
            std::mem::take(&mut self.selected),
        )
    }
}

// ── REFCLOSE command ───────────────────────────────────────────────────────

/// `Enter option [Save/Discard reference changes] <Save>:`
pub struct RefCloseCommand;

impl RefCloseCommand {
    pub fn new() -> Self {
        Self
    }
}

impl CadCommand for RefCloseCommand {
    fn name(&self) -> &'static str {
        "REFCLOSE"
    }
    fn prompt(&self) -> String {
        t!("Enter option [Save/Discard reference changes] <Save>:").into_owned()
    }
    fn options(&self) -> Vec<CmdOption> {
        vec![CmdOption::new("Save", "S"), CmdOption::new("Discard", "D")]
    }
    fn wants_text_input(&self) -> bool {
        true
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim().to_uppercase();
        if t.is_empty() {
            return None;
        }
        if "SAVE".starts_with(&t) {
            Some(CmdResult::Relaunch("REFCLOSE_SAVE".into(), vec![]))
        } else if "DISCARD".starts_with(&t) {
            Some(CmdResult::Relaunch("REFCLOSE_DISCARD".into(), vec![]))
        } else {
            Some(CmdResult::ReportError(
                t!("Invalid option keyword.").into_owned(),
            ))
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Relaunch("REFCLOSE_SAVE".into(), vec![])
    }
    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }
}

// ── Geometry helpers ───────────────────────────────────────────────────────

/// Inverse of an affine placement (rotation / scale / mirror and translation),
/// or None when it is singular.
pub fn inverse_placement(t: &codec::types::Transform) -> Option<codec::types::Transform> {
    use codec::types::{Matrix3, Matrix4};
    let m = &t.matrix.m;
    let linear = Matrix3 {
        m: [
            [m[0][0], m[0][1], m[0][2]],
            [m[1][0], m[1][1], m[1][2]],
            [m[2][0], m[2][1], m[2][2]],
        ],
    }
    .inverse()?;
    let r = linear.m;
    let tr = [m[0][3], m[1][3], m[2][3]];
    let mut out = Matrix4::identity();
    for row in 0..3 {
        for col in 0..3 {
            out.m[row][col] = r[row][col];
        }
        out.m[row][3] = -(r[row][0] * tr[0] + r[row][1] * tr[1] + r[row][2] * tr[2]);
    }
    Some(codec::types::Transform::from_matrix(out))
}

/// Apply the session's forward transform to a definition-local entity so it
/// appears at its correct world-space position.
pub fn apply_insert_transform(entity: &mut EntityType, session: &RefEditSession) {
    entity.as_entity_mut().apply_transform(&session.forward);
}

/// Apply the session's inverse transform to a world-space entity to bring it
/// back to definition-local coordinates.
pub fn apply_insert_inverse_transform(entity: &mut EntityType, session: &RefEditSession) {
    entity.as_entity_mut().apply_transform(&session.inverse);
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["REFCLOSE", "REFSET", "-REFEDIT"]
});
inventory::submit!(crate::command::CommandRegistration {
    names: &["REFEDIT"]
}); // RefEditPickCommand
