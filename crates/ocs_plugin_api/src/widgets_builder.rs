//! Plugin-side widget-surface construction helpers layered over the frozen
//! widget vocabulary in [`crate::widgets`] — the widget twin of
//! [`crate::scene_builder`]'s builder family (SCN-058, BINDING ADR-014).
//!
//! Two builders, one rule: nothing here is on the wire.
//!
//! - [`WidgetSurfaceBuilder`] assembles a [`WidgetSurface`] fluently. A
//!   surface produced by [`WidgetSurfaceBuilder::finish`] is structurally
//!   identical to one written literally and has run
//!   [`WidgetSurface::validate`] — a surface the validator rejects is
//!   returned as an [`Err`], never swallowed and never panicking (the
//!   [`SceneBuilder`](crate::scene_builder::SceneBuilder) convention).
//!
//! - [`WidgetSurfacePatchBuilder`] queues [`WidgetOp`]s against a declared
//!   base revision. [`WidgetSurfacePatchBuilder::finish`] pre-checks the two
//!   cheap whole-patch gates ([`MAX_PATCH_OPS`] and the non-monotonic
//!   revision law) builder-side so a defect fails fast; the authoritative
//!   gates stay in [`WidgetSurface::apply_patch`] exactly as they were.
//!
//! # The three StableId namespaces (verified in the real source)
//!
//! ADR-014 names three id namespaces per surface; the REAL vocabulary
//! (checked, not assumed) draws the boundaries one notch differently from
//! the ADR's sketch, and this module centralizes the REAL law:
//!
//! 1. **The surface-wide widget namespace** — widget node ids, TREE ROW ids,
//!    and LIST ITEM ids are ONE namespace (`validate_root`'s single `seen`
//!    set spans all three, widgets.rs "One surface-wide `seen` set spans the
//!    whole slice"; the same law governs patch payloads in the planner).
//!    Submenu [`MenuDef`] ids ride it too (payload, registered by
//!    `validate_menu_items`). The eager ledger below is exactly this one
//!    set. ADR-014 sketched list/tree item ids as scoped-per-node — that
//!    premise traced to a nonexistent op shape and is refuted by the real
//!    validator: a surface re-using one item id under two lists FAILS
//!    [`WidgetSurface::validate`], so a builder ledger that allowed it would
//!    construct surfaces the crate itself refuses (a semantics change, which
//!    per ADR-014's own closing clause needs a new ADR — this builder does
//!    not smuggle one in).
//! 2. **The typed-table registry namespace** — [`PanelTableDef::id_table`] is
//!    FOREIGN to the widget namespace; the registry is a per-surface map
//!    keyed by it. The builder holds that map and refuses a second def
//!    naming an already-registered `id_table` (a `BTreeMap` insert would
//!    silently overwrite — the eager ledger refuses instead).
//! 3. **Per-table row-key/column registries** — [`TableRowDef::key`] and
//!    [`TableColumnDef::id`] are unique WITHIN their table, and different
//!    tables may freely re-use keys (the real per-table namespace). Enforced
//!    by [`PanelTableDef::validate`] — eagerly at [`WidgetSurfaceBuilder::table`]
//!    (fail fast) and again at `finish` through [`WidgetSurface::validate`].
//!
//! # Conventions mirrored from the scene builder family
//!
//! Eager ledger (an id mistake is a chain-time [`WidgetError`]),
//! `&mut Self` returns so calls chain, finish-time gates (the validator is
//! THE authority), and a drained builder after `finish` (a follow-up
//! `finish` builds a fresh empty surface/patch, never a re-stamped stale
//! one). One deliberate simplification: scene elements are built field by
//! field behind the active-element accessors, while widget defs are complete
//! structs — so the typed adds take the def whole (the ledger walks its
//! payload-internal ids) and no accessory setters exist. Group nesting goes
//! through [`WidgetSurfaceBuilder::group`] /
//! [`WidgetSurfaceBuilder::end_group`], which mirror the scene builder's
//! group stack; [`WidgetSurfaceBuilder::node`] is the raw escape hatch that
//! inserts a pre-built subtree without the eager duplicate scan (its ids are
//! still ledgered; defects surface at `finish`).

use crate::scene::{MAX_PATCH_OPS, SceneRevision, StableId};
use crate::widgets::{
    Axis, IconActionDef, ItemListDef, MenuDef, MenuItemDef, PanelTableDef, TableRef,
    WidgetCellValue, WidgetError, WidgetGroup, WidgetNode, WidgetOp, WidgetSurface,
    WidgetSurfacePatch, WidgetTreeDef,
};
use std::collections::{BTreeMap, HashSet};

// ---------------------------------------------------------------------------
// WidgetSurfaceBuilder
// ---------------------------------------------------------------------------

/// Fluent builder for a [`WidgetSurface`] (see the [module docs](self) for
/// the namespace and convention story).
///
/// Typed adds append into the innermost open group (the surface root before
/// the first [`WidgetSurfaceBuilder::group`]) and return `Ok(&mut Self)` so
/// calls chain; the `Err` arm carries [`WidgetError::DuplicateWidgetId`]
/// when an id of the added payload is already live in the surface-wide
/// namespace, or [`WidgetError::InvalidCell`] when a table def names an
/// already-registered `id_table` or fails its own per-table rules.
#[derive(Debug, Clone)]
pub struct WidgetSurfaceBuilder {
    revision: SceneRevision,
    /// Root widget nodes in document order.
    roots: Vec<WidgetNode>,
    /// Index path of the innermost OPEN group, from the root list; empty =
    /// the root list is the insertion container.
    focus: Vec<usize>,
    /// Eager ledger: the ONE surface-wide namespace — widget node ids, tree
    /// row ids, list item ids, and submenu `MenuDef` ids (the real
    /// `validate_root` law; see the [module docs](self)). `finish` re-checks
    /// everything structurally through [`WidgetSurface::validate`]; this
    /// only turns id mistakes into chain-time errors.
    taken: HashSet<StableId>,
    /// The typed-table registry (the FOREIGN table-id namespace), keyed by
    /// [`PanelTableDef::id_table`].
    tables: BTreeMap<StableId, PanelTableDef>,
}

impl WidgetSurfaceBuilder {
    /// Start a surface at the given revision (wrapped into
    /// [`SceneRevision`]; the surface shares the panel's ONE revision space
    /// with its `PanelScene`).
    pub fn new(revision: u64) -> Self {
        Self {
            revision: SceneRevision(revision),
            roots: Vec::new(),
            focus: Vec::new(),
            taken: HashSet::new(),
            tables: BTreeMap::new(),
        }
    }

    /// Open a group: subsequent adds land inside it until
    /// [`WidgetSurfaceBuilder::end_group`]. The group is constructed with
    /// `name: None`, `enabled: true`, `visible: true`, and the given
    /// stacking direction / [`WidgetGroup::split_ratio`] (`None` = plain
    /// stacking, `Some(_)` = the documented SplitView).
    pub fn group(
        &mut self,
        id: u64,
        direction: Axis,
        split_ratio: Option<f32>,
    ) -> Result<&mut Self, WidgetError> {
        self.register_unique(StableId(id))?;
        let group = WidgetGroup {
            id: StableId(id),
            name: None,
            enabled: true,
            visible: true,
            direction,
            children: Vec::new(),
            split_ratio,
        };
        self.push_child(WidgetNode::Group(group));
        // The just-pushed group is the new insertion container: its index
        // within the (parent) container is the path step.
        let index = Self::container_children(&self.roots, &self.focus)
            .expect("widget builder invariant: the open-group path names a live group")
            .len()
            - 1;
        self.focus.push(index);
        Ok(self)
    }

    /// Close the innermost open group. A no-op when no group is open (the
    /// frozen [`WidgetError`] vocabulary has no arm for unbalanced closes,
    /// and dropping the request silently keeps the builder infallible —
    /// the scene builder's exact convention).
    pub fn end_group(&mut self) -> &mut Self {
        self.focus.pop();
        self
    }

    /// Append a semantic retained tree. Every row id joins the surface-wide
    /// namespace eagerly (a repeat anywhere in the surface refuses as
    /// [`WidgetError::DuplicateWidgetId`], chain-time).
    pub fn tree(&mut self, def: WidgetTreeDef) -> Result<&mut Self, WidgetError> {
        self.register_unique(def.id)?;
        for row in &def.rows {
            self.register_unique(row.id)?;
        }
        self.push_child(WidgetNode::Tree(def));
        Ok(self)
    }

    /// Append a semantic retained item list. Every item id joins the
    /// surface-wide namespace eagerly (one namespace across ALL widgets,
    /// rows, and items — the same item id under a second list refuses; see
    /// the [module docs](self)).
    pub fn list(&mut self, def: ItemListDef) -> Result<&mut Self, WidgetError> {
        self.register_unique(def.id)?;
        for item in &def.items {
            self.register_unique(item.id)?;
        }
        self.push_child(WidgetNode::List(def));
        Ok(self)
    }

    /// Append a semantic icon button (a11y `accessible_name` REQUIRED
    /// non-empty — checked at `finish` through the validator).
    pub fn icon_button(&mut self, def: IconActionDef) -> Result<&mut Self, WidgetError> {
        self.register_unique(def.id)?;
        self.push_child(WidgetNode::IconButton(def));
        Ok(self)
    }

    /// Append a semantic menu. The menu's id AND every nested submenu's
    /// `MenuDef` id join the surface-wide namespace eagerly (submenus are
    /// payload carrying their own widget id — the validator registers them
    /// too).
    pub fn menu(&mut self, def: MenuDef) -> Result<&mut Self, WidgetError> {
        self.register_unique(def.id)?;
        self.ledger_submenu_ids(&def.items)?;
        self.push_child(WidgetNode::Menu(def));
        Ok(self)
    }

    /// Append a table REFERENCE. Only the widget's own id joins the
    /// surface-wide namespace; `TableRef::table_id` is deliberately NOT
    /// registered (the FOREIGN registry namespace — several refs may name
    /// one backing table, the real `validate_root` behavior).
    pub fn table_ref(&mut self, def: TableRef) -> Result<&mut Self, WidgetError> {
        self.register_unique(def.id)?;
        self.push_child(WidgetNode::Table(def));
        Ok(self)
    }

    /// Register a typed-table def in the surface's registry. Eager gates:
    /// a second def naming an already-registered `id_table` refuses as
    /// [`WidgetError::InvalidCell`] (the registry map would otherwise
    /// silently overwrite), and the def's own per-table rules run via
    /// [`PanelTableDef::validate`] — the per-table row-key/column
    /// registries live HERE, not in the surface-wide namespace (different
    /// tables may freely re-use row keys).
    pub fn table(&mut self, def: PanelTableDef) -> Result<&mut Self, WidgetError> {
        if self.tables.contains_key(&def.id_table) {
            return Err(WidgetError::InvalidCell {
                table: def.id_table,
                key: def.id_table,
                column: def.id_table,
                reason: "duplicate table id in the surface registry".to_string(),
            });
        }
        def.validate()?;
        self.tables.insert(def.id_table, def);
        Ok(self)
    }

    /// Escape hatch: insert a fully constructed node (a composite
    /// [`WidgetNode::Group`] with its children already set, say) into the
    /// current container as a CLOSED subtree — the group stack is not
    /// extended into it. Unlike the typed adds this performs NO eager
    /// duplicate scan; the subtree's ids are still ledgered, so later typed
    /// adds collide with them, and defects inside the subtree surface from
    /// [`WidgetSurfaceBuilder::finish`] (the scene builder's `element`
    /// convention).
    pub fn node(&mut self, node: WidgetNode) -> &mut Self {
        self.ledger_subtree_ids(&node);
        self.push_child(node);
        self
    }

    /// Finish the surface: runs [`WidgetSurface::validate`] on the assembled
    /// snapshot and returns it — the gate ladder (duplicate detection across
    /// the namespaces, [`crate::scene::MAX_SCENE_NODES`] total nodes,
    /// [`crate::widgets::MAX_WIDGETS_PER_PANEL`] non-group widgets,
    /// [`crate::widgets::MAX_WIDGET_ROWS_INITIAL`] per payload vec, group
    /// nesting + row parent-chain depth, required non-empty texts, byte
    /// caps, and the registry contract) IS the validator's, so a surface the
    /// crate refuses can never leave this builder. Validation errors come
    /// back as [`WidgetError`]s and consume the staged state either way: the
    /// builder is drained and reset for reuse (a `finish` on an exhausted
    /// builder produces an empty surface).
    ///
    /// Note on [`crate::widgets::MAX_WIDGET_RANGE_LEN`]: no `WidgetSurface`
    /// field carries a [`crate::widgets::WidgetRange`] (that budget gates
    /// the event/decode side), so it has no expression in this ladder.
    pub fn finish(&mut self) -> Result<WidgetSurface, WidgetError> {
        let surface = WidgetSurface {
            revision: self.revision,
            roots: std::mem::take(&mut self.roots),
            tables: std::mem::take(&mut self.tables),
        };
        self.focus.clear();
        self.taken.clear();
        surface.validate()?;
        Ok(surface)
    }

    /// Record an id in the surface-wide namespace, rejecting the first
    /// repeat (the [`register_id`](crate::widgets) vocabulary:
    /// [`WidgetError::DuplicateWidgetId`]).
    fn register_unique(&mut self, id: StableId) -> Result<(), WidgetError> {
        if self.taken.insert(id) {
            Ok(())
        } else {
            Err(WidgetError::DuplicateWidgetId(id))
        }
    }

    /// Ledger every nested submenu `MenuDef` id of a menu payload (the
    /// validator's submenu registration, mirrored eagerly).
    fn ledger_submenu_ids(&mut self, items: &[MenuItemDef]) -> Result<(), WidgetError> {
        for item in items {
            if let Some(submenu) = &item.submenu {
                self.register_unique(submenu.id)?;
                self.ledger_submenu_ids(&submenu.items)?;
            }
        }
        Ok(())
    }

    /// Ledger every id of a pre-built subtree (node ids + tree rows + list
    /// items + submenu defs) WITHOUT any duplicate scan — the raw escape
    /// hatch's companion. Iterative over the node spine; menu/def payloads
    /// recurse (bounded by the same shapes the validator walks).
    fn ledger_subtree_ids(&mut self, node: &WidgetNode) {
        let mut stack: Vec<&WidgetNode> = vec![node];
        while let Some(next) = stack.pop() {
            match next {
                WidgetNode::Group(group) => {
                    self.taken.insert(group.id);
                    stack.extend(group.children.iter());
                }
                WidgetNode::Tree(def) => {
                    self.taken.insert(def.id);
                    for row in &def.rows {
                        self.taken.insert(row.id);
                    }
                }
                WidgetNode::List(def) => {
                    self.taken.insert(def.id);
                    for item in &def.items {
                        self.taken.insert(item.id);
                    }
                }
                WidgetNode::IconButton(def) => {
                    self.taken.insert(def.id);
                }
                WidgetNode::Menu(def) => {
                    self.taken.insert(def.id);
                    self.ledger_submenu_ids_unchecked(&def.items);
                }
                WidgetNode::Table(def) => {
                    self.taken.insert(def.id);
                }
            }
        }
    }

    /// The unchecked submenu walk (the escape hatch's arm — ledgering
    /// without refusing, mirroring `ledger_subtree_ids`' no-scan doctrine).
    fn ledger_submenu_ids_unchecked(&mut self, items: &[MenuItemDef]) {
        for item in items {
            if let Some(submenu) = &item.submenu {
                self.taken.insert(submenu.id);
                self.ledger_submenu_ids_unchecked(&submenu.items);
            }
        }
    }

    /// Append `node` into the current container. Only reachable through
    /// callers that already settled id uniqueness.
    fn push_child(&mut self, node: WidgetNode) {
        let container = Self::container_children_mut(&mut self.roots, &self.focus)
            .expect("widget builder invariant: the open-group path names a live group");
        container.push(node);
    }

    /// Descend `path` (each step indexing a child list) to the children vec
    /// of the group the path names; the empty path is `roots` itself.
    /// Returns `None` on a stale path — which cannot happen for paths this
    /// builder produces (append-only child lists keep earlier indices
    /// stable).
    fn container_children_mut<'a>(
        roots: &'a mut Vec<WidgetNode>,
        path: &[usize],
    ) -> Option<&'a mut Vec<WidgetNode>> {
        let mut children = roots;
        for &index in path {
            let down = match children.get_mut(index) {
                Some(WidgetNode::Group(group)) => &mut group.children,
                _ => return None,
            };
            children = down;
        }
        Some(children)
    }

    /// The shared view of [`Self::container_children_mut`].
    fn container_children<'a>(roots: &'a [WidgetNode], path: &[usize]) -> Option<&'a [WidgetNode]> {
        let mut children = roots;
        for &index in path {
            let down = match children.get(index) {
                Some(WidgetNode::Group(group)) => group.children.as_slice(),
                _ => return None,
            };
            children = down;
        }
        Some(children)
    }
}

// ---------------------------------------------------------------------------
// WidgetSurfacePatchBuilder
// ---------------------------------------------------------------------------

/// Typed op-queue builder for a [`WidgetSurfacePatch`] (the widget twin of
/// the scene [`PatchBuilder`](crate::scene_builder::PatchBuilder)).
///
/// Raw ids are plain `u64` (wrapped into [`StableId`] here) and the op
/// methods chain; [`WidgetSurfacePatchBuilder::finish`] is the checkpoint:
/// it pre-checks the op count against [`MAX_PATCH_OPS`] and the strictly
/// advancing successor revision BEFORE any caller can hand the patch to
/// [`WidgetSurface::apply_patch`]. Structural sense of the ops themselves
/// (live ids, live group anchors, resulting budgets, registry addresses) is
/// the apply-side's job — [`WidgetSurface::apply_patch`] stays THE
/// authority; the builder only fails fast.
///
/// The op set is the REAL [`WidgetOp`] vocabulary: [`WidgetOp::Add`],
/// [`WidgetOp::Update`], [`WidgetOp::Remove`], and [`WidgetOp::TableCell`].
/// There are no tree-children or list-items ops in the frozen enum (row and
/// item surgery rides whole-payload Add/Update); nothing here invents one.
#[derive(Debug, Clone)]
pub struct WidgetSurfacePatchBuilder {
    base_revision: SceneRevision,
    ops: Vec<WidgetOp>,
}

impl WidgetSurfacePatchBuilder {
    /// Start a patch against the given base revision.
    pub fn new(base_revision: u64) -> Self {
        Self {
            base_revision: SceneRevision(base_revision),
            ops: Vec::new(),
        }
    }

    /// Queue a [`WidgetOp::Add`] (a `Some` parent appends into that id,
    /// which must anchor a [`WidgetNode::Group`]; a `None` appends at the
    /// surface root).
    pub fn add(&mut self, parent: Option<u64>, widget: WidgetNode) -> &mut Self {
        self.ops.push(WidgetOp::Add {
            parent: parent.map(StableId),
            widget,
        });
        self
    }

    /// Queue a [`WidgetOp::Update`] (keyed by `id`; the substituted
    /// `content` must carry the same widget id — a re-key is expressed as
    /// Remove + Add, the scene Replace doctrine).
    pub fn update(&mut self, id: u64, content: WidgetNode) -> &mut Self {
        self.ops.push(WidgetOp::Update {
            id: StableId(id),
            content,
        });
        self
    }

    /// Queue a [`WidgetOp::Remove`] for the widget rooted at `id`.
    pub fn remove(&mut self, id: u64) -> &mut Self {
        self.ops.push(WidgetOp::Remove { id: StableId(id) });
        self
    }

    /// Queue a [`WidgetOp::TableCell`] — ONE typed-table cell write
    /// addressed by (registry table, stable row key, stable column id).
    pub fn table_cell(
        &mut self,
        table: u64,
        key: u64,
        column: u64,
        value: WidgetCellValue,
    ) -> &mut Self {
        self.ops.push(WidgetOp::TableCell {
            table: StableId(table),
            key: StableId(key),
            column: StableId(column),
            value,
        });
        self
    }

    /// Finish the patch at the given successor revision. Refuses an op list
    /// past [`MAX_PATCH_OPS`] ([`WidgetError::BudgetExceeded`]) and any
    /// successor that does not advance past the base
    /// ([`WidgetError::NonMonotonicRevision`]). The queued ops are drained
    /// whether the patch is accepted or not: a follow-up `finish` builds a
    /// fresh (empty) patch, never a re-stamped stale one.
    pub fn finish(&mut self, revision: u64) -> Result<WidgetSurfacePatch, WidgetError> {
        // Take first, then validate: every arm — accepted or refused —
        // leaves the builder's queue empty.
        let ops = std::mem::take(&mut self.ops);
        if ops.len() > MAX_PATCH_OPS {
            return Err(WidgetError::BudgetExceeded {
                current: ops.len(),
                max: MAX_PATCH_OPS,
            });
        }
        let successor = SceneRevision(revision);
        if successor <= self.base_revision {
            return Err(WidgetError::NonMonotonicRevision {
                current: self.base_revision,
                given: successor,
            });
        }
        Ok(WidgetSurfacePatch {
            base_revision: self.base_revision,
            revision: successor,
            ops,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::MAX_PATCH_OPS;
    use crate::widgets::{
        CellKind, DomainRef, IconRef, ItemDef, TableCell, TableColumnDef, TableRowDef,
        TreeCapabilities, TreeRowDef, MAX_WIDGETS_PER_PANEL,
    };
    use std::collections::BTreeMap;

    fn sid(id: u64) -> StableId {
        StableId(id)
    }

    fn icon(uri: &str) -> IconRef {
        IconRef {
            uri: uri.to_string(),
            size: 16.0,
        }
    }

    /// Minimal valid icon-button def (a11y name + command required).
    fn button(id: u64, name: &str) -> IconActionDef {
        IconActionDef {
            id: sid(id),
            accessible_name: name.to_string(),
            tooltip: None,
            icon: icon("svg:gear"),
            enabled: true,
            visible: true,
            active: false,
            command: format!("{name}.toggle"),
            confirmation: None,
        }
    }

    /// Minimal list def with the given items.
    fn list_def(id: u64, items: Vec<ItemDef>) -> ItemListDef {
        ItemListDef {
            id: sid(id),
            name: None,
            enabled: true,
            visible: true,
            label: None,
            items,
        }
    }

    fn item(id: u64, label: &str) -> ItemDef {
        ItemDef {
            id: sid(id),
            label: label.to_string(),
            icon: None,
            enabled: true,
            visible: true,
            command: None,
            domain_ref: None,
        }
    }

    /// Minimal tree def with the given rows.
    fn tree_def(id: u64, rows: Vec<TreeRowDef>) -> WidgetTreeDef {
        WidgetTreeDef {
            id: sid(id),
            name: None,
            enabled: true,
            visible: true,
            label: None,
            capabilities: TreeCapabilities::default(),
            rows,
        }
    }

    fn row(id: u64, parent: Option<u64>, label: &str) -> TreeRowDef {
        TreeRowDef {
            id: sid(id),
            parent: parent.map(sid),
            label: label.to_string(),
            icon: None,
            status: None,
            expandable: parent.is_some(),
            enabled: true,
            lazy_children: false,
            domain_ref: None,
        }
    }

    /// Minimal table def: `columns` scalar-text columns, `rows` rows of
    /// kind-matched text cells. `key_base` lets two tables intentionally
    /// share row keys (the per-table namespace).
    fn table_def(id_table: u64, columns: usize, rows: usize, key_base: u64) -> PanelTableDef {
        PanelTableDef {
            id_table: sid(id_table),
            columns: (0..columns)
                .map(|c| TableColumnDef {
                    id: sid(1000 + c as u64),
                    label: format!("col-{c}"),
                    kind: CellKind::Text,
                    width_hint: None,
                    stretch: false,
                })
                .collect(),
            total_rows: None,
            rows: (0..rows)
                .map(|i| TableRowDef {
                    key: sid(key_base + i as u64),
                    cells: (0..columns)
                        .map(|c| TableCell::Text(format!("cell {c}-{i}")))
                        .collect(),
                    domain_ref: None,
                })
                .collect(),
        }
    }

    /// The table_ref naming a table built by [`table_def`].
    fn table_ref_def(id: u64, table_id: u64) -> TableRef {
        TableRef {
            id: sid(id),
            name: None,
            enabled: true,
            visible: true,
            table_id: sid(table_id),
        }
    }

    #[test]
    fn builder_built_surface_equals_the_hand_written_literal() {
        // The same surface twice: once through the builder (group + tree +
        // list + button + table_ref + registry entry), once as the literal.
        let built = {
            let mut b = WidgetSurfaceBuilder::new(9);
            b.table(table_def(200, 2, 2, 300))
                .expect("table registers")
                .group(1, Axis::Vertical, None)
                .expect("group opens")
                .tree(tree_def(6, vec![row(7, None, "Alignments"), row(8, Some(7), "AL-1")]))
                .expect("tree adds")
                .list(list_def(9, vec![item(10, "profile-1")]))
                .expect("list adds")
                .icon_button(button(3, "filter"))
                .expect("button adds")
                .table_ref(table_ref_def(13, 200))
                .expect("table ref adds");
            b.finish().expect("built surface validates")
        };
        let literal = WidgetSurface {
            revision: SceneRevision(9),
            roots: vec![WidgetNode::Group(WidgetGroup {
                id: sid(1),
                name: None,
                enabled: true,
                visible: true,
                direction: Axis::Vertical,
                split_ratio: None,
                children: vec![
                    WidgetNode::Tree(tree_def(
                        6,
                        vec![row(7, None, "Alignments"), row(8, Some(7), "AL-1")],
                    )),
                    WidgetNode::List(list_def(9, vec![item(10, "profile-1")])),
                    WidgetNode::IconButton(button(3, "filter")),
                    WidgetNode::Table(table_ref_def(13, 200)),
                ],
            })],
            tables: BTreeMap::from([(sid(200), table_def(200, 2, 2, 300))]),
        };
        assert_eq!(built, literal, "builder output equals the literal (value)");
        let built_bytes = bincode::serialize(&built).expect("serialize built");
        let literal_bytes = bincode::serialize(&literal).expect("serialize literal");
        assert_eq!(built_bytes, literal_bytes, "builder output equals the literal (bincode)");
        built.validate().expect("built surface is valid");
    }

    #[test]
    fn builder_built_patch_equals_the_literal_patch() {
        let mut b = WidgetSurfacePatchBuilder::new(4);
        b.add(None, WidgetNode::IconButton(button(3, "filter")))
            .update(3, WidgetNode::IconButton(button(3, "filter2")))
            .remove(3)
            .table_cell(200, 300, 1000, WidgetCellValue::Text("edited".to_string()));
        let built = b.finish(5).expect("patch finishes");
        let literal = WidgetSurfacePatch {
            base_revision: SceneRevision(4),
            revision: SceneRevision(5),
            ops: vec![
                WidgetOp::Add {
                    parent: None,
                    widget: WidgetNode::IconButton(button(3, "filter")),
                },
                WidgetOp::Update {
                    id: sid(3),
                    content: WidgetNode::IconButton(button(3, "filter2")),
                },
                WidgetOp::Remove { id: sid(3) },
                WidgetOp::TableCell {
                    table: sid(200),
                    key: sid(300),
                    column: sid(1000),
                    value: WidgetCellValue::Text("edited".to_string()),
                },
            ],
        };
        assert_eq!(built, literal, "builder patch equals the literal (value)");
        assert_eq!(
            bincode::serialize(&built).expect("serialize built patch"),
            bincode::serialize(&literal).expect("serialize literal patch"),
            "builder patch equals the literal (bincode)"
        );
    }

    #[test]
    fn duplicate_node_id_is_refused_eagerly() {
        let mut b = WidgetSurfaceBuilder::new(1);
        b.icon_button(button(5, "first"))
            .expect("first add lands");
        let err = b
            .icon_button(button(5, "second"))
            .expect_err("duplicate node id refuses at chain time");
        assert_eq!(err, WidgetError::DuplicateWidgetId(sid(5)));
        // And the same law through a DIFFERENT payload namespace: a list
        // item re-using the live node id refuses too (one surface-wide
        // namespace across nodes, rows, and items).
        let err = b
            .list(list_def(6, vec![item(5, "impostor")]))
            .expect_err("item id colliding with a live node id refuses");
        assert_eq!(err, WidgetError::DuplicateWidgetId(sid(5)));
    }

    #[test]
    fn the_same_item_id_under_two_list_nodes_is_refused_the_surface_wide_law() {
        // ADR-014 sketched list-item ids as scoped-per-node; the REAL
        // vocabulary is one surface-wide namespace spanning nodes, tree
        // rows, and list items (`validate_root`'s single `seen` set — the
        // built surface below would FAIL `WidgetSurface::validate`, so a
        // scoped ledger would construct surfaces the crate itself refuses).
        // Per ADR-014's own closing clause that semantics change would need
        // a new ADR; this pin holds the REAL law instead: the second list
        // re-using item id 10 refuses (chain-time via the eager ledger, and
        // finish-time via the validator for the escape hatch).
        let mut b = WidgetSurfaceBuilder::new(1);
        b.list(list_def(1, vec![item(10, "a")]))
            .expect("first list lands");
        let err = b
            .list(list_def(2, vec![item(10, "b")]))
            .expect_err("the same item id under a SECOND list refuses");
        assert_eq!(err, WidgetError::DuplicateWidgetId(sid(10)));

        // The same refusal at finish time for the raw escape hatch (which
        // does not scan eagerly): the validator is the authority.
        let mut b = WidgetSurfaceBuilder::new(1);
        b.list(list_def(1, vec![item(10, "a")]))
            .expect("first list lands")
            .node(WidgetNode::List(list_def(2, vec![item(10, "b")])));
        let err = b
            .finish()
            .expect_err("a cross-list item id collision refuses at finish");
        assert_eq!(err, WidgetError::DuplicateWidgetId(sid(10)));
    }

    #[test]
    fn the_same_item_id_within_one_list_node_is_refused() {
        let mut b = WidgetSurfaceBuilder::new(1);
        let err = b
            .list(list_def(
                1,
                vec![item(10, "a"), item(11, "b"), item(10, "c")],
            ))
            .expect_err("the same item id twice within ONE list refuses");
        assert_eq!(err, WidgetError::DuplicateWidgetId(sid(10)));
    }

    #[test]
    fn the_same_tree_row_id_refuses_but_table_row_keys_are_per_table() {
        // Tree rows ride the surface-wide namespace: the same row id twice
        // in one tree refuses eagerly…
        let mut b = WidgetSurfaceBuilder::new(1);
        let err = b
            .tree(tree_def(1, vec![row(10, None, "a"), row(10, None, "b")]))
            .expect_err("the same tree row id twice in one tree refuses");
        assert_eq!(err, WidgetError::DuplicateWidgetId(sid(10)));
        // …and across two trees (still one namespace).
        let mut b = WidgetSurfaceBuilder::new(1);
        b.tree(tree_def(1, vec![row(10, None, "a")]))
            .expect("first tree lands");
        let err = b
            .tree(tree_def(2, vec![row(10, None, "b")]))
            .expect_err("the same tree row id under a second tree refuses");
        assert_eq!(err, WidgetError::DuplicateWidgetId(sid(10)));

        // TABLE row keys are the genuinely scoped namespace: two tables may
        // freely re-use keys (per-table registries), and the builder accepts
        // both, validating at finish.
        let mut b = WidgetSurfaceBuilder::new(1);
        b.table(table_def(200, 1, 2, 300))
            .expect("first table lands")
            .table(table_def(201, 1, 2, 300))
            .expect("second table re-uses the row keys freely");
        b.finish().expect("shared row keys across tables are legal");
        // …but the same row key twice within ONE table refuses (here: at
        // the eager per-table validate).
        let err = WidgetSurfaceBuilder::new(1)
            .table(PanelTableDef {
                id_table: sid(200),
                columns: vec![TableColumnDef {
                    id: sid(1000),
                    label: "col".to_string(),
                    kind: CellKind::Text,
                    width_hint: None,
                    stretch: false,
                }],
                total_rows: None,
                rows: vec![
                    TableRowDef {
                        key: sid(300),
                        cells: vec![TableCell::Text("a".to_string())],
                        domain_ref: None,
                    },
                    TableRowDef {
                        key: sid(300),
                        cells: vec![TableCell::Text("b".to_string())],
                        domain_ref: None,
                    },
                ],
            })
            .expect_err("a duplicated row key within one table refuses");
        assert!(matches!(err, WidgetError::InvalidCell { .. }));

        // And a second def naming an ALREADY-REGISTERED table id refuses
        // (the registry map would silently overwrite — the eager ledger
        // refuses instead).
        let mut b = WidgetSurfaceBuilder::new(1);
        b.table(table_def(200, 1, 1, 300))
            .expect("first table lands");
        let err = b
            .table(table_def(200, 1, 1, 900))
            .expect_err("a duplicate registry table id refuses");
        assert!(matches!(err, WidgetError::InvalidCell { .. }));
    }

    #[test]
    fn over_widget_budget_surface_refuses_at_finish() {
        // MAX_WIDGETS_PER_PANEL + 1 non-group widgets: the eager ledger has
        // nothing to say (ids are unique); the budget gate is the
        // validator's, and finish surfaces it.
        let mut b = WidgetSurfaceBuilder::new(1);
        for i in 0..=MAX_WIDGETS_PER_PANEL {
            b.icon_button(button(1000 + i as u64, &format!("button-{i}")))
                .expect("add lands");
        }
        let err = b.finish().expect_err("one widget past the cap refuses");
        assert_eq!(
            err,
            WidgetError::BudgetExceeded {
                current: MAX_WIDGETS_PER_PANEL + 1,
                max: MAX_WIDGETS_PER_PANEL,
            }
        );
    }

    #[test]
    fn over_budget_op_queue_refuses_at_finish() {
        let mut b = WidgetSurfacePatchBuilder::new(1);
        for i in 0..=MAX_PATCH_OPS {
            b.remove(i as u64);
        }
        let err = b.finish(2).expect_err("one op past the cap refuses");
        assert_eq!(
            err,
            WidgetError::BudgetExceeded {
                current: MAX_PATCH_OPS + 1,
                max: MAX_PATCH_OPS,
            }
        );
        // The refused queue is drained: a follow-up finish is a fresh
        // (empty) patch, never a re-stamped stale one.
        let fresh = b.finish(2).expect("drained builder finishes");
        assert!(fresh.ops.is_empty());
    }

    #[test]
    fn non_monotonic_revision_refuses_at_patch_finish() {
        let mut b = WidgetSurfacePatchBuilder::new(5);
        b.remove(1);
        let err = b
            .finish(5)
            .expect_err("revision == base refuses");
        assert_eq!(
            err,
            WidgetError::NonMonotonicRevision {
                current: SceneRevision(5),
                given: SceneRevision(5),
            }
        );
        let mut b = WidgetSurfacePatchBuilder::new(5);
        b.remove(1);
        let err = b
            .finish(4)
            .expect_err("revision < base refuses");
        assert_eq!(
            err,
            WidgetError::NonMonotonicRevision {
                current: SceneRevision(5),
                given: SceneRevision(4),
            }
        );
    }

    #[test]
    fn minimal_happy_path_build_patch_apply_validate() {
        // Build: a table_ref + a tree + a list, plus the backing registry.
        let mut b = WidgetSurfaceBuilder::new(7);
        b.table(table_def(200, 2, 2, 300))
            .expect("table lands")
            .table_ref(table_ref_def(120, 200))
            .expect("table ref lands")
            .tree(tree_def(6, vec![row(7, None, "Alignments")]))
            .expect("tree lands")
            .list(list_def(9, vec![item(10, "profile-1")]))
            .expect("list lands");
        let mut surface = b.finish().expect("surface validates");

        // Patch: one typed-table cell write, pre-checked builder-side,
        // applied through the REAL apply_patch.
        let mut p = WidgetSurfacePatchBuilder::new(7);
        p.table_cell(200, 300, 1000, WidgetCellValue::Text("edited".to_string()));
        let patch = p.finish(8).expect("patch finishes");
        surface.apply_patch(patch).expect("the real apply accepts");
        assert_eq!(surface.revision, SceneRevision(8));
        assert_eq!(
            surface.tables[&sid(200)].rows[0].cells[0],
            TableCell::Text("edited".to_string()),
            "the cell write landed"
        );
        surface.validate().expect("the patched surface still validates");

        // The builder pre-checks what apply would refuse anyway: a stale
        // base fails fast, BEFORE apply is ever reached.
        let mut p = WidgetSurfacePatchBuilder::new(7);
        p.remove(6);
        let patch = p.finish(9).expect("patch itself is well-formed");
        assert_ne!(patch.base_revision, surface.revision);
        let err = surface
            .apply_patch(patch)
            .expect_err("apply refuses the stale base");
        assert!(matches!(err, WidgetError::StaleBase { .. }));
    }

    #[test]
    fn group_nesting_and_the_escape_hatch_behave_like_the_scene_builder() {
        // Nesting: the inner adds land in the innermost open group;
        // end_group re-points at the just-closed group.
        let mut b = WidgetSurfaceBuilder::new(1);
        b.group(1, Axis::Vertical, None)
            .expect("outer opens")
            .icon_button(button(2, "in outer"))
            .expect("button lands in outer")
            .group(3, Axis::Horizontal, Some(0.5))
            .expect("inner opens")
            .icon_button(button(4, "in inner"))
            .expect("button lands in inner");
        b.end_group();
        b.icon_button(button(5, "back in outer"))
            .expect("button lands back in outer");
        b.end_group();
        b.icon_button(button(6, "at root"))
            .expect("button lands at root");
        let surface = b.finish().expect("surface validates");
        let WidgetNode::Group(outer) = &surface.roots[0] else {
            panic!("root is the outer group");
        };
        assert_eq!(outer.id, sid(1));
        assert_eq!(outer.children.len(), 3, "button + inner group + button");
        let WidgetNode::Group(inner) = &outer.children[1] else {
            panic!("second outer child is the inner group");
        };
        assert_eq!(inner.id, sid(3));
        assert_eq!(inner.split_ratio, Some(0.5));
        assert_eq!(inner.children.len(), 1);
        // end_group past the bottom is a no-op; finish drains and resets —
        // a follow-up finish builds a fresh EMPTY surface.
        let mut b = WidgetSurfaceBuilder::new(1);
        b.end_group().end_group();
        assert!(b.finish().expect("unbalanced closes are a no-op").roots.is_empty());
        assert!(
            b.finish()
                .expect("drained builder yields an empty surface")
                .roots
                .is_empty()
        );

        // The escape hatch inserts a closed subtree and ledgers its ids: a
        // later typed add colliding with a subtree id refuses.
        let mut b = WidgetSurfaceBuilder::new(1);
        b.node(WidgetNode::Group(WidgetGroup {
            id: sid(1),
            name: None,
            enabled: true,
            visible: true,
            direction: Axis::Vertical,
            split_ratio: None,
            children: vec![WidgetNode::List(list_def(2, vec![item(3, "a")]))],
        }));
        let err = b
            .icon_button(button(3, "impostor"))
            .expect_err("a subtree id is live for later typed adds");
        assert_eq!(err, WidgetError::DuplicateWidgetId(sid(3)));
    }

    #[test]
    fn required_text_defects_surface_at_finish() {
        // The builder does not second-guess def text at add time (def
        // structs arrive whole); the finish-time validator ladder does.
        let empty_a11y = IconActionDef {
            id: sid(1),
            accessible_name: String::new(),
            tooltip: None,
            icon: icon("svg:gear"),
            enabled: true,
            visible: true,
            active: false,
            command: "x".to_string(),
            confirmation: None,
        };
        let mut b = WidgetSurfaceBuilder::new(1);
        b.icon_button(empty_a11y).expect("add lands (ledger only)");
        let err = b.finish().expect_err("empty accessible name refuses");
        assert_eq!(
            err,
            WidgetError::EmptyRequiredText {
                id: Some(sid(1)),
                what: "accessible name",
            }
        );
    }

    #[test]
    fn empty_registry_placeholder_table_ref_is_the_documented_era() {
        // A table_ref with NO registry entry is the documented SWG-001
        // placeholder shape: legal while the registry is empty.
        let mut b = WidgetSurfaceBuilder::new(1);
        b.table_ref(table_ref_def(120, 200))
            .expect("placeholder ref lands");
        let surface = b.finish().expect("placeholder surface validates");
        assert!(surface.tables.is_empty());
    }

    #[test]
    fn domain_ref_and_menu_payload_ledgering() {
        // A menu with a submenu: the submenu's MenuDef id is payload in the
        // surface-wide namespace — a later widget re-using it refuses.
        let submenu = MenuDef {
            id: sid(20),
            name: None,
            enabled: true,
            visible: true,
            label: None,
            items: vec![MenuItemDef {
                command_id: "export.png".to_string(),
                label: "As PNG".to_string(),
                icon: None,
                shortcut: None,
                enabled: true,
                visible: true,
                submenu: None,
                confirmation: None,
            }],
        };
        let menu = MenuDef {
            id: sid(4),
            name: None,
            enabled: true,
            visible: true,
            label: None,
            items: vec![MenuItemDef {
                command_id: "export".to_string(),
                label: "Export".to_string(),
                icon: None,
                shortcut: None,
                enabled: true,
                visible: true,
                submenu: Some(Box::new(submenu)),
                confirmation: None,
            }],
        };
        let mut b = WidgetSurfaceBuilder::new(1);
        b.menu(menu).expect("menu lands");
        // The submenu id (payload, not a WidgetNode) is live.
        let err = b
            .list(list_def(20, vec![item(21, "impostor")]))
            .expect_err("a submenu MenuDef id is live in the namespace");
        assert_eq!(err, WidgetError::DuplicateWidgetId(sid(20)));
        // And DomainRef ids never enter the namespace (presentation id vs
        // domain identity are deliberately separate spaces).
        let mut b = WidgetSurfaceBuilder::new(1);
        let mut with_domain = item(30, "a");
        with_domain.domain_ref = Some(DomainRef {
            kind: "surface".to_string(),
            id: 10,
        });
        b.list(list_def(1, vec![with_domain]))
            .expect("domain id 10 is not a presentation id")
            .tree(tree_def(2, vec![row(10, None, "free")]))
            .expect("presentation id 10 is still free");
        b.finish().expect("surface validates");
    }
}
