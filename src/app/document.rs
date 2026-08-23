use crate::command::CadCommand;
use crate::io::linetypes;
use crate::modules::draw::modify::block_edit::BlockEditSession;
use crate::modules::draw::modify::refedit::RefEditSession;
use crate::scene::pick::grip::GripEdit;
use crate::scene::GripDef;
use crate::scene::{ObjectIsolationState, Scene};
use crate::snap::SnapResult;
use crate::ui::{LayerPanel, PropertiesPanel};
use acadrust::tables::{normalize_name, Ucs};
use acadrust::{CadDocument, EntityType, Handle};
use iced;
use crate::t;
use std::any::Any;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static NEXT_DOCUMENT_TAB_ID: AtomicU64 = AtomicU64::new(1);

// ── Dynamic input ──────────────────────────────────────────────────────────

/// One quantity shown in the dynamic-input overlay near the cursor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum DynComponent {
    /// Absolute X ordinate.
    X,
    /// Absolute Y ordinate.
    Y,
    /// Absolute Z ordinate (only visible after the user types a second
    /// `,` separator from a cartesian X/Y configuration).
    Z,
    /// Linear distance from the last point.
    Distance,
    /// Angle from the last point, in degrees.
    Angle,
    /// A scalar the command reads from the command line (a count, a radius,
    /// a delta). Typed-only — it has no geometric live value derived from
    /// the cursor unless the command supplies one via `dyn_live_value`.
    Scalar,
}

/// A single editable dynamic-input field. `buffer == None` means the box
/// tracks the cursor live; once the user types, the typed text is held in
/// `buffer` and the box stops following the cursor (it is "locked").
#[derive(Clone, Debug)]
pub(super) struct DynFieldEntry {
    pub(super) component: DynComponent,
    /// Semantic role — drives the label and value scaling (e.g. diameter).
    /// Defaults to the role matching `component` on the legacy path.
    pub(super) role: crate::command::DynRole,
    pub(super) buffer: Option<String>,
}

impl DynFieldEntry {
    pub(super) fn new(component: DynComponent) -> Self {
        Self {
            component,
            role: default_role_for(component),
            buffer: None,
        }
    }
    /// Build from an explicit role (spec-driven path); the resolution
    /// component is derived from the role.
    pub(super) fn from_role(role: crate::command::DynRole) -> Self {
        Self {
            component: component_for_role(role),
            role,
            buffer: None,
        }
    }
    pub(super) fn locked(&self) -> bool {
        self.buffer.is_some()
    }
}

/// Map a [`DynRole`](crate::command::DynRole) to the ordinate/distance/angle
/// component used by point resolution.
pub(super) fn component_for_role(role: crate::command::DynRole) -> DynComponent {
    use crate::command::DynRole;
    match role {
        DynRole::X | DynRole::Width => DynComponent::X,
        DynRole::Y | DynRole::Height => DynComponent::Y,
        DynRole::Z => DynComponent::Z,
        DynRole::Distance | DynRole::Radius | DynRole::Diameter => DynComponent::Distance,
        DynRole::Angle => DynComponent::Angle,
        DynRole::Factor | DynRole::Count => DynComponent::Scalar,
    }
}

fn default_role_for(component: DynComponent) -> crate::command::DynRole {
    use crate::command::DynRole;
    match component {
        DynComponent::X => DynRole::X,
        DynComponent::Y => DynRole::Y,
        DynComponent::Z => DynRole::Z,
        DynComponent::Distance => DynRole::Distance,
        DynComponent::Angle => DynRole::Angle,
        DynComponent::Scalar => DynRole::Factor,
    }
}

// ── Per-document tab state ─────────────────────────────────────────────────

pub(super) struct DocumentTab {
    /// Stable identity across tab insert/remove operations. Background work
    /// must never target a tab by its transient vector index.
    pub(super) id: u64,
    pub(super) scene: Scene,
    pub(super) current_path: Option<PathBuf>,
    /// Persistent native edit lease for the drawing currently at
    /// `current_path`. The sidecar portion survives atomic file replacement.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) edit_lease: Option<crate::io::edit_lock::EditLease>,
    /// True when another editor owns the drawing lease. Direct Save stays
    /// blocked until Retry acquires it or the user chooses Save As.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) edit_lock_conflict: bool,
    /// Disk state observed after open or the latest successful save.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) disk_fingerprint: Option<crate::io::edit_lock::FileFingerprint>,
    pub(super) dirty: bool,
    /// Direct Save is redirected to Save As after open-time repairs so the
    /// source drawing cannot be overwritten accidentally.
    pub(super) recovery_save_as_required: bool,
    /// Monotonic committed-edit/undo/redo revision. Background save completion
    /// uses it with scene epochs so an older snapshot never clears newer work.
    pub(super) edit_revision: u64,
    pub(super) tab_title: String,
    pub(super) properties: PropertiesPanel,
    pub(super) layers: LayerPanel,
    pub(super) active_cmd: Option<Box<dyn CadCommand>>,
    /// The selection set the most recent command worked on, captured when a
    /// finishing command drops the live selection — re-selectable with the
    /// "Previous" keyword at any Select objects prompt (#426).
    pub(super) prev_selection: Vec<acadrust::Handle>,
    pub(super) last_cmd: Option<String>,
    /// Most recently created path drawable. A fresh LINE/PLINE can accept its
    /// current final endpoint with Enter before the first click.
    pub(super) last_draw_anchor: Option<Handle>,
    pub(super) snap_result: Option<SnapResult>,
    pub(super) active_grip: Option<GripEdit>,
    pub(super) selected_grips: Vec<GripDef>,
    /// Entity handle for each entry in `selected_grips`.
    pub(super) selected_grip_handles: Vec<Handle>,
    /// Shift-selected grips, keyed by entity and object-local grip id.
    pub(super) hot_grips: rustc_hash::FxHashSet<(Handle, usize)>,
    pub(super) selected_handle: Option<Handle>,
    /// Dynamic-block visibility grip for the current single selection.
    pub(super) visibility_grip: Option<super::visibility::VisibilityGrip>,
    pub(super) wireframe: bool,
    pub(super) render_mode: acadrust::entities::ViewportRenderMode,
    pub(super) visual_style: String,
    pub(super) last_cursor_world: glam::DVec3,
    pub(super) last_cursor_screen: iced::Point,
    /// Base point (`App::last_point`) projected to viewport pixels, refreshed
    /// on cursor move. Lets the dynamic-input overlay place the distance label
    /// along the rubber-band line and the angle label at its end.
    pub(super) last_point_screen: Option<iced::Point>,
    /// Dynamic-input fields shown near the cursor while a command waits
    /// for a point/distance/angle. Rebuilt whenever the active command's
    /// `dyn_field()` or the presence of a base point changes. Empty when
    /// dynamic input is not active.
    pub(super) dyn_fields: Vec<DynFieldEntry>,
    /// Guide geometry the overlay draws for the current step (set alongside
    /// `dyn_fields`). Polar arc, radius line, axis-delta projections, etc.
    pub(super) dyn_guide: crate::command::DynGuide,
    /// World-space anchor the current step's values are measured from. `None`
    /// falls back to `App::last_point`.
    pub(super) dyn_anchor: Option<glam::DVec3>,
    /// Far end of a reference line through `dyn_anchor` (for the `Perp` guide:
    /// the base edge / major axis the offset is measured square to).
    pub(super) dyn_ref: Option<glam::DVec3>,
    /// `dyn_ref` projected to viewport pixels.
    pub(super) dyn_ref_screen: Option<iced::Point>,
    /// Index of the field that TAB has focused (the one keystrokes edit).
    pub(super) dyn_active: usize,
    pub(super) history: HistoryState,
    pub(super) active_layer: String,
    /// Currently active UCS. `None` means WCS (identity transform).
    pub(super) active_ucs: Option<Ucs>,
    /// Custom model-space background color.  `None` = default dark grey.
    pub(super) bg_color: Option<[f32; 4]>,
    /// Custom paper-space background color.  `None` = default off-white grey.
    pub(super) paper_bg_color: Option<[f32; 4]>,
    /// Active REFEDIT session, if any.
    pub(super) refedit_session: Option<RefEditSession>,
    /// Open BEDIT block tabs. Definitions are edited live; each tab owns its
    /// entry snapshot and camera so nested blocks can remain open independently.
    pub(super) block_edits: Vec<BlockEditSession>,
    /// Index of the block tab currently shown, or `None` for Model/Paper space.
    pub(super) active_block_edit: Option<usize>,
    /// Currently active MLeader style name.
    pub(super) active_mleader_style: String,
    /// Last camera_generation value written back to the document.
    pub(super) last_synced_camera_gen: u64,
    /// Sentinel "Welcome / Start" tab. Always at index 0 when present.
    /// Cannot be closed; the viewport area renders a welcome page instead
    /// of the model-space shader. The scene is still constructed so the
    /// rest of the code can treat it as a normal tab when reading.
    pub(super) is_start: bool,
    /// Interactive PAN mode (the PAN command / tool). While active, a left-
    /// button drag pans the view instead of selecting — the only pan path on a
    /// device with no middle mouse button (a trackpad / web client). Exited
    /// with Esc or by starting another command.
    pub(super) pan_mode: bool,
    /// Interactive 3-D orbit mode. While active, a left-button drag follows
    /// the same camera-orbit path as Shift + middle-button drag.
    pub(super) orbit_mode: bool,
    /// Interactive ZOOM Dynamic mode. A left drag pans horizontally and zooms
    /// vertically until Escape or another command ends the mode.
    pub(super) zoom_dynamic_mode: bool,
    /// Per-plugin document state (`plugin::BuiltinPlugin` manifest id → state).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(super) plugin_state: HashMap<&'static str, Box<dyn Any + Send + Sync>>,
    pub(super) suspended_cmd: Option<Box<dyn CadCommand>>,
}

impl DocumentTab {
    pub(super) fn rename_layer(&mut self, old_name: &str, new_name: &str) -> bool {
        let active = normalize_name(&self.active_layer) == normalize_name(old_name);
        if !self.scene.rename_layer(old_name, new_name) {
            return false;
        }
        if active {
            self.active_layer = new_name.to_string();
        }
        self.dirty = true;
        true
    }

    pub(super) fn active_block_edit_session(&self) -> Option<&BlockEditSession> {
        self.active_block_edit
            .and_then(|index| self.block_edits.get(index))
    }

    pub(super) fn active_block_edit_session_mut(&mut self) -> Option<&mut BlockEditSession> {
        self.active_block_edit
            .and_then(|index| self.block_edits.get_mut(index))
    }

    /// The active WCS↔UCS converter for this tab — identity when no UCS is set.
    /// Every consumer that needs UCS-relative coordinates goes through this.
    pub(super) fn ucs_xform(&self) -> super::helpers::UcsXform {
        super::helpers::UcsXform::from_active(self.active_ucs.as_ref())
    }

    /// World-space UCS origin in full f64 precision (stored f64 in the header /
    /// viewport). Used to anchor the UCS icon and as the cursor-pick plane point
    /// — `UcsXform`'s origin is f32 and would quantize the anchor at UTM scale.
    pub(super) fn ucs_origin_world(&self) -> glam::DVec3 {
        self.active_ucs
            .as_ref()
            .map(|u| glam::dvec3(u.origin.x, u.origin.y, u.origin.z))
            .unwrap_or(glam::DVec3::ZERO)
    }

    /// True when the active pane edits **model-space** geometry: the Model tab,
    /// or inside a floating viewport (MSPACE). The UCS applies in both; plain
    /// paper space (no active viewport) is excluded. Single predicate so every
    /// UCS-aware system shares one rule. [[feedback_shared_infra]]
    pub(super) fn editing_model_space(&self) -> bool {
        self.scene.current_layout == "Model" || self.scene.active_viewport.is_some()
    }

    /// The document's saved model-space UCS (header), as a `Ucs`. `None` when it
    /// is identity (plain WCS).
    pub(super) fn model_ucs_from_header(&self) -> Option<Ucs> {
        let h = &self.scene.document.header;
        let mut u = if h.model_space_ucs_name.is_empty() {
            Ucs::new("*ACTIVE*")
        } else {
            self.scene
                .document
                .ucss
                .get(&h.model_space_ucs_name)
                .cloned()
                .unwrap_or_else(|| Ucs::new(h.model_space_ucs_name.clone()))
        };
        u.origin = h.model_space_ucs_origin;
        u.x_axis = h.model_space_ucs_x_axis;
        u.y_axis = h.model_space_ucs_y_axis;
        if h.model_space_ucs_name.is_empty()
            && super::helpers::UcsXform::from_ucs(&u).is_identity()
        {
            None
        } else {
            Some(u)
        }
    }

    /// A floating viewport's own per-viewport UCS, if it has one set. `None`
    /// when the viewport uses world coordinates or the handle is not a viewport.
    pub(super) fn ucs_from_viewport(&self, h: Handle) -> Option<Ucs> {
        let vp = match self.scene.document.get_entity(h) {
            Some(acadrust::EntityType::Viewport(vp)) => vp,
            _ => return None,
        };
        if !vp.ucs_per_viewport {
            return None;
        }
        let mut u = if vp.ucs_handle.is_null() {
            Ucs::new("*VPUCS*")
        } else {
            self.scene
                .document
                .ucss
                .iter()
                .find(|ucs| ucs.handle == vp.ucs_handle)
                .cloned()
                .unwrap_or_else(|| Ucs::new("*VPUCS*"))
        };
        u.origin = vp.ucs_origin;
        u.x_axis = vp.ucs_x_axis;
        u.y_axis = vp.ucs_y_axis;
        if vp.ucs_handle.is_null() && super::helpers::UcsXform::from_ucs(&u).is_identity() {
            None
        } else {
            Some(u)
        }
    }

    /// Set `active_ucs` to the UCS of the *current pane*: the entered viewport's
    /// own per-viewport UCS, the model header UCS in the Model tab, or none in
    /// plain paper space. Keeps the ViewCube in lock-step. Call on every pane
    /// change (enter/exit viewport, layout / tab switch, load) so one field
    /// drives all UCS-aware systems regardless of where editing happens.
    pub(super) fn refresh_active_ucs(&mut self) {
        self.active_ucs = if let Some(session) = self.active_block_edit_session() {
            session.editor_ucs.clone()
        } else if let Some(h) = self.scene.active_viewport {
            self.ucs_from_viewport(h)
        } else if self.scene.current_layout == "Model" {
            self.model_ucs_from_header()
        } else {
            None
        };
        self.sync_ucs_to_scene();
    }

    /// Persist `active_ucs` back to its pane's storage so it round-trips: the
    /// entered viewport's per-viewport UCS fields, or the document header's
    /// model-space UCS in the Model tab. No-op in plain paper space. Call after
    /// any UCS change.
    pub(super) fn persist_active_ucs(&mut self) {
        use acadrust::types::Vector3;
        if let Some(index) = self.active_block_edit {
            let active_ucs = self.active_ucs.clone();
            if let Some(session) = self.block_edits.get_mut(index) {
                session.editor_ucs = active_ucs;
            }
        } else if let Some(h) = self.scene.active_viewport {
            let (o, x, y, handle, per) = match &self.active_ucs {
                Some(u) => (u.origin, u.x_axis, u.y_axis, u.handle, true),
                None => (
                    Vector3::ZERO,
                    Vector3::UNIT_X,
                    Vector3::UNIT_Y,
                    Handle::NULL,
                    false,
                ),
            };
            if let Some(acadrust::EntityType::Viewport(vp)) = self.scene.document.get_entity_mut(h)
            {
                vp.ucs_origin = o;
                vp.ucs_x_axis = x;
                vp.ucs_y_axis = y;
                vp.ucs_handle = handle;
                vp.ucs_per_viewport = per;
            }
        } else if self.scene.current_layout == "Model" {
            let h = &mut self.scene.document.header;
            match &self.active_ucs {
                Some(u) => {
                    h.model_space_ucs_origin = u.origin;
                    h.model_space_ucs_x_axis = u.x_axis;
                    h.model_space_ucs_y_axis = u.y_axis;
                    h.model_space_ucs_name = u.name.clone();
                }
                None => {
                    h.model_space_ucs_origin = Vector3::ZERO;
                    h.model_space_ucs_x_axis = Vector3::UNIT_X;
                    h.model_space_ucs_y_axis = Vector3::UNIT_Y;
                    h.model_space_ucs_name.clear();
                }
            }
        }
    }

    /// Adopt the active pane's UCS on load (the file's saved model-space UCS in
    /// the Model tab). Thin wrapper over [`refresh_active_ucs`] kept for the
    /// load call sites.
    pub(super) fn adopt_active_ucs_from_header(&mut self) {
        self.refresh_active_ucs();
    }

    /// Push the active UCS rotation into the scene so the ViewCube composes with
    /// it. Call after any change to `active_ucs`.
    pub(super) fn sync_ucs_to_scene(&mut self) {
        self.scene.viewcube_ucs = self.ucs_xform().rotation_mat();
    }

    /// Grid origin (render/wire space) and UCS→world rotation for grid snap and
    /// the grid overlay. Identity / origin-at-zero outside model space.
    pub(super) fn ucs_grid_basis(&self) -> (glam::Vec3, glam::Mat4) {
        if !self.editing_model_space() {
            return (glam::Vec3::ZERO, glam::Mat4::IDENTITY);
        }
        let xf = self.ucs_xform();
        let (o, ..) = xf.axes();
        let origin = glam::Vec3::new(o.x as f32, o.y as f32, o.z as f32);
        (origin, xf.rotation_mat())
    }

    pub(super) fn new_drawing(n: usize) -> Self {
        let mut scene = Scene::new();
        linetypes::populate_document(&mut scene.document);
        // Paper layouts start as A4 landscape.
        for obj in scene.document.objects.values_mut() {
            if let acadrust::objects::ObjectType::Layout(l) = obj {
                if l.name != "Model" {
                    l.min_limits = (0.0, 0.0);
                    l.max_limits = (297.0, 210.0);
                    l.min_extents = (0.0, 0.0, 0.0);
                    l.max_extents = (297.0, 210.0, 0.0);
                    l.paper_width = 297.0;
                    l.paper_height = 210.0;
                    l.plot_paper_units = 1;
                    l.plot_scale_numerator = 1.0;
                    l.plot_scale_denominator = 1.0;
                    l.paper_size = "ISO_A4_(297.00_x_210.00_MM)".into();
                }
            }
        }
        Self {
            id: NEXT_DOCUMENT_TAB_ID.fetch_add(1, Ordering::Relaxed),
            scene,
            current_path: None,
            #[cfg(not(target_arch = "wasm32"))]
            edit_lease: None,
            #[cfg(not(target_arch = "wasm32"))]
            edit_lock_conflict: false,
            #[cfg(not(target_arch = "wasm32"))]
            disk_fingerprint: None,
            dirty: false,
            recovery_save_as_required: false,
            edit_revision: 0,
            prev_selection: Vec::new(),
            tab_title: format!("Drawing{}", n),
            properties: PropertiesPanel::empty(),
            layers: LayerPanel::default(),
            active_cmd: None,
            last_cmd: None,
            last_draw_anchor: None,
            snap_result: None,
            active_grip: None,
            selected_grips: vec![],
            selected_grip_handles: vec![],
            hot_grips: rustc_hash::FxHashSet::default(),
            selected_handle: None,
            visibility_grip: None,
            wireframe: false,
            render_mode: acadrust::entities::ViewportRenderMode::Wireframe2D,
            visual_style: "Wireframe 2D".into(),
            last_cursor_world: glam::DVec3::ZERO,
            last_cursor_screen: iced::Point::ORIGIN,
            last_point_screen: None,
            dyn_fields: Vec::new(),
            dyn_guide: crate::command::DynGuide::Polar,
            dyn_anchor: None,
            dyn_ref: None,
            dyn_ref_screen: None,
            dyn_active: 0,
            history: HistoryState::default(),
            active_layer: "0".to_string(),
            active_ucs: None,
            bg_color: None,
            paper_bg_color: None,
            refedit_session: None,
            block_edits: Vec::new(),
            active_block_edit: None,
            active_mleader_style: "Standard".to_string(),
            last_synced_camera_gen: 0,
            is_start: false,
            pan_mode: false,
            orbit_mode: false,
            zoom_dynamic_mode: false,
            plugin_state: HashMap::new(),
            suspended_cmd: None,
        }
    }

    /// Welcome / Start tab. Carries a dummy Scene so the rest of the app
    /// can read tab state uniformly; the viewport renderer detects
    /// `is_start` and shows a welcome page instead.
    pub(super) fn new_start() -> Self {
        let mut t = Self::new_drawing(0);
        t.tab_title = "Start".to_string();
        t.is_start = true;
        t
    }

    pub(super) fn tab_display_name(&self) -> String {
        match &self.current_path {
            Some(p) => p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            None => {
                if self.is_start {
                    t!("Start").into_owned()
                } else {
                    self.tab_title.clone()
                }
            }
        }
    }
}

/// One undo/redo entry. Every edit is represented as a first-touch entity delta
/// plus an optional structure-only document image, so history never clones the
/// full entity store.
#[derive(Clone)]
pub(super) enum HistorySnapshot {
    Delta(DeltaSnapshot),
    ObjectVisibility(ObjectVisibilitySnapshot),
}

impl HistorySnapshot {
    pub(super) fn label(&self) -> &str {
        match self {
            HistorySnapshot::Delta(d) => &d.label,
            HistorySnapshot::ObjectVisibility(v) => &v.label,
        }
    }

    /// Approximate retained history memory. Entity images are shared through
    /// `Arc`; the estimate conservatively budgets their payloads and optional
    /// structure state to keep pathological histories bounded.
    pub(super) fn estimated_bytes(&self) -> usize {
        match self {
            HistorySnapshot::Delta(d) => d
                .entities
                .len()
                .saturating_mul(512)
                .saturating_add(
                    d.structure
                        .as_ref()
                        .map_or(0, StructureSnapshot::estimated_bytes),
                )
                .saturating_add(d.selected_before.len().saturating_mul(16))
                .saturating_add(d.selected_after.len().saturating_mul(16))
                .saturating_add(
                    d.active_layer
                        .as_ref()
                        .map_or(0, |(before, after)| before.len().saturating_add(after.len())),
                )
                .saturating_add(d.label.len()),
            HistorySnapshot::ObjectVisibility(v) => v
                .before
                .hidden
                .len()
                .saturating_add(
                    v.before
                        .keep
                        .as_ref()
                        .map_or(0, rustc_hash::FxHashSet::len),
                )
                .saturating_add(v.after.hidden.len())
                .saturating_add(
                    v.after
                        .keep
                        .as_ref()
                        .map_or(0, rustc_hash::FxHashSet::len),
                )
                .saturating_add(v.selected_before.len())
                .saturating_add(v.selected_after.len())
                .saturating_mul(16)
                .saturating_add(v.label.len()),
        }
    }
}

/// Symmetric undo/redo image for session-only object visibility. It contains
/// no document entity data and therefore can never make isolation serializable.
#[derive(Clone)]
pub(super) struct ObjectVisibilitySnapshot {
    pub(super) before: ObjectIsolationState,
    pub(super) after: ObjectIsolationState,
    pub(super) selected_before: Vec<Handle>,
    pub(super) selected_after: Vec<Handle>,
    pub(super) label: String,
}

/// A transactional undo entry: for each touched handle, its before-image and
/// after-image (`None` = the entity was absent on that side, i.e. an add or an
/// erase). Symmetric — undo applies the before side, redo the after side — so
/// one delta moves between the undo and redo stacks without re-capturing. The
/// Optional structure state covers layers, objects, blocks and tables without
/// cloning the flat entity store.
#[derive(Clone)]
pub(super) struct DeltaSnapshot {
    pub(super) entities: Vec<(Handle, Option<Arc<EntityType>>, Option<Arc<EntityType>>)>,
    pub(super) current_layout_before: String,
    pub(super) current_layout_after: String,
    pub(super) selected_before: Vec<Handle>,
    pub(super) selected_after: Vec<Handle>,
    pub(super) dirty_before: bool,
    pub(super) dirty_after: bool,
    pub(super) active_layer: Option<(String, String)>,
    /// Opposite non-entity document state. `apply_delta_state` swaps this with
    /// the live structure, so the same allocation shuttles between undo/redo.
    pub(super) structure: Option<StructureSnapshot>,
    pub(super) label: String,
}

#[derive(Clone)]
pub(super) enum StructureSnapshot {
    /// Compatibility fallback for genuinely broad structural commands.
    Full(CadDocument),
    /// Exact layer-table entries touched by one command.
    Layers(Vec<TableEntryDelta<acadrust::tables::Layer>>),
    /// Exact text-style entries touched by one command.
    TextStyles(Vec<TableEntryDelta<acadrust::tables::TextStyle>>),
    /// Exact dimension-style entries touched by one command.
    DimStyles(Vec<TableEntryDelta<acadrust::tables::DimStyle>>),
    /// Exact object-map entries touched by one command. This supports commands
    /// such as groups/dictionaries without retaining every unrelated object.
    Objects(Vec<ObjectEntryDelta>),
    /// The bounded set of style tables, style objects, current-style pointers,
    /// and matching ribbon state touched by one Style Manager transaction.
    Styles {
        before: super::style_ops::StyleStateSnapshot,
        after: super::style_ops::StyleStateSnapshot,
        text_names: Vec<String>,
        dim_names: Vec<String>,
        object_handles: Vec<Handle>,
    },
}

impl StructureSnapshot {
    pub(super) fn estimated_bytes(&self) -> usize {
        match self {
            Self::Full(doc) => doc.objects.len().saturating_mul(192),
            Self::Layers(entries) => entries.len().saturating_mul(256),
            Self::TextStyles(entries) => entries.len().saturating_mul(320),
            Self::DimStyles(entries) => entries.len().saturating_mul(1024),
            Self::Objects(entries) => entries.len().saturating_mul(384),
            Self::Styles { before, after, .. } => before
                .estimated_bytes()
                .saturating_add(after.estimated_bytes()),
        }
    }

    pub(super) fn is_full(&self) -> bool {
        matches!(self, Self::Full(_))
    }
}

#[derive(Clone)]
pub(super) struct TableEntryDelta<T> {
    pub(super) name: String,
    pub(super) before: Option<T>,
    pub(super) after: Option<T>,
}

#[derive(Clone)]
pub(super) struct ObjectEntryDelta {
    pub(super) handle: Handle,
    pub(super) before: Option<acadrust::objects::ObjectType>,
    pub(super) after: Option<acadrust::objects::ObjectType>,
}

#[derive(Default)]
pub(super) struct HistoryState {
    pub(super) undo_stack: Vec<HistorySnapshot>,
    pub(super) redo_stack: Vec<HistorySnapshot>,
    pub(super) pending: Option<PendingHistorySnapshot>,
}

pub(super) struct PendingHistorySnapshot {
    pub(super) label: String,
    pub(super) current_layout: String,
    pub(super) active_layer: String,
    pub(super) selected_before: Vec<Handle>,
    pub(super) dirty_before: bool,
    pub(super) structure_before: CadDocument,
    pub(super) recorder: Arc<acadrust::document::EntityChangeRecorder>,
}
