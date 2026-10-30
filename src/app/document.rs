use crate::command::CadCommand;
use crate::modules::draw::modify::block_edit::BlockEditSession;
use crate::modules::draw::modify::refedit::RefEditSession;
use crate::scene::pick::grip::GripEdit;
use crate::scene::GripDef;
use crate::scene::{ObjectIsolationState, Scene};
use crate::snap::SnapResult;
use crate::t;
use crate::ui::{LayerPanel, PropertiesPanel};
use codec::tables::{normalize_name, Ucs};
use codec::{Document, EntityType, Handle};
use iced;
use std::any::Any;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static NEXT_DOCUMENT_TAB_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum LayisoMode {
    Off,
    LockAndFade,
}

#[derive(Clone)]
pub(super) struct LayisoLayerSnapshot {
    pub(super) name: String,
    pub(super) before: codec::tables::Layer,
    pub(super) after: codec::tables::Layer,
}

#[derive(Clone)]
pub(super) struct LayisoState {
    pub(super) mode: LayisoMode,
    pub(super) layers: Vec<LayisoLayerSnapshot>,
}

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
    /// Angle from the last point, displayed and entered in the drawing's units.
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

/// An open sketch — the state CREATESKETCH parks so FINISHSKETCH can put it
/// back.
///
/// A sketch here is a *mode*, not a document object: the drawing plane, a
/// square-on view, and snapping forced live. The geometry it produces is
/// ordinary model-space entities, which is what keeps the result a normal
/// DWG that other software can open. `opened_with` is the handle set at the
/// moment the sketch opened, so the difference on finish is exactly what the
/// user drew — that difference is what gets offered to EXTRUDE.
pub(super) struct SketchSession {
    /// Display name, e.g. `"Sketch1"`.
    pub(super) name: String,
    /// UCS active before the sketch opened. Restored on finish.
    pub(super) previous_ucs: Option<Ucs>,
    /// Snap master state before the sketch forced it on, so a user who works
    /// with snapping off gets that back rather than silently keeping it.
    pub(super) previous_snap_enabled: bool,
    /// Entity handles already present when the sketch opened.
    pub(super) opened_with: std::collections::HashSet<codec::Handle>,
}

// ── Per-document tab state ─────────────────────────────────────────────────

pub(super) struct DocumentTab {
    /// Stable identity across tab insert/remove operations. Background work
    /// must never target a tab by its transient vector index.
    pub(super) id: u64,
    pub(super) scene: Scene,
    pub(super) table_grip: Option<super::visibility::TableGrip>,
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
    /// A whole document replaced this tab's content (file open, template new,
    /// system open) and native plugins have not been told yet: the
    /// message boundary (`OpenCADStudio::notify_plugins_document_opened`,
    /// API v9) dispatches `DocumentOpened` once and clears it, so edits and
    /// switching between tabs never announce the same load again.
    pub(super) document_opened_pending: bool,
    pub(super) tab_title: String,
    pub(super) properties: PropertiesPanel,
    pub(super) layers: LayerPanel,
    /// The LAYWALK pallet session, while a walk is open on this drawing.
    pub(super) layer_walk: Option<crate::ui::window::layer_walk::LayerWalkSession>,
    /// Rows cached for that session, so the pallet and the pure logic agree on
    /// one view of the layer table (rebuilt by `walk_rebuild`).
    pub(super) layer_walk_rows: Vec<crate::ui::window::layer_walk::LayerWalkRow>,
    /// The pallet asked for a pick (Select Objects) and is hidden while the
    /// selection prompt owns the pointer.
    pub(super) layer_walk_picking: bool,
    /// Saved wildcard filters, per drawing (not persisted with the DWG).
    pub(super) layer_walk_filters: Vec<String>,
    /// Dropdown state for `layer_walk_filters`.
    pub(super) layer_walk_combo: iced::widget::combo_box::State<String>,
    pub(super) active_cmd: Option<Box<dyn CadCommand>>,
    /// Follow-up command for a Parameter Set macro after parameter placement.
    pub(super) pending_parameter_set_follow_up: Option<&'static str>,
    pub(super) pending_xy_box_set: Option<(codec::Handle, crate::scene::block_actions::Kind, u8)>,
    pub(super) pending_action_pair: Option<(codec::Handle, crate::scene::block_actions::Kind, u8)>,
    pub(super) measurement: super::measurement::MeasurementDisplay,
    /// Remaining command tokens queued behind a PAUSE (or `\`) awaiting user interaction.
    pub(super) pending_pause_tokens: Option<Vec<String>>,
    /// The last LAYISO state for this drawing. It is intentionally kept on the
    /// tab rather than globally, so LAYUNISO never crosses drawing tabs.
    pub(super) layiso_state: Option<LayisoState>,
    /// The mode used by the next LAYISO command. AutoCAD defaults to Off.
    pub(super) layiso_mode: LayisoMode,
    /// The selection set the most recent command worked on, captured when a
    /// finishing command drops the live selection — re-selectable with the
    /// "Previous" keyword at any Select objects prompt (#426).
    pub(super) prev_selection: Vec<codec::Handle>,
    pub(super) last_cmd: Option<String>,
    /// Most recently created path drawable. A fresh LINE/PLINE can accept its
    /// current final endpoint with Enter before the first click.
    pub(super) last_draw_anchor: Option<Handle>,
    pub(super) snap_result: Option<SnapResult>,
    pub(super) active_grip: Option<GripEdit>,
    pub(super) selected_grips: Vec<GripDef>,
    /// Entity handle for each entry in `selected_grips`.
    pub(super) selected_grip_handles: Vec<Handle>,
    /// Plugin-declared custom grip points for this tab (API v11): session
    /// display state only — never persisted, never host-interpreted.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) plugin_grips: crate::plugin::grip_points::PluginGripStore,
    /// Plugin-declared entity class registry (API v14): tab-scoped
    /// display-session state — entries die with their plugin (re-declared
    /// per load) and are never persisted; the staged instances stamp XDATA
    /// identity keys + record linkages instead.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) plugin_classes: crate::plugin::plugin_classes::PluginClassStore,
    /// DIRTY REGION accumulation (API v13): the identity set of what plugin
    /// and built-in paths changed since the previous boundary notification —
    /// `DocumentChangedRegions` carries it so boundary work is O(changed),
    /// not O(store). Cleared at each boundary; over the cap the sets
    /// degrade to empty (an honest full-walk signal).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(super) plugin_dirty_entities: rustc_hash::FxHashSet<codec::Handle>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(super) plugin_dirty_records: rustc_hash::FxHashSet<(codec::Handle, String)>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(super) plugin_dirty_overflow: bool,
    /// Geometry epoch the last region set was published at; a plugin write
    /// starting from another epoch means something else changed the drawing
    /// in between, so the sets degrade to empty.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(super) plugin_dirty_epoch: u64,
    /// The active custom-grip drag, if any (at most one; rides the same
    /// drag/active-command lifecycle as built-in grip drags).
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) active_plugin_grip: Option<crate::plugin::grip_points::ActivePluginGrip>,
    /// Shift-selected grips, keyed by entity and object-local grip id.
    pub(super) hot_grips: rustc_hash::FxHashSet<(Handle, usize)>,
    /// Grip-mode "Copy" toggle (context menu): each grip placement leaves the
    /// original in place and adds a modified copy, until Enter / Esc.
    pub(super) grip_copy: bool,
    /// Grip-mode "Base Point" (context menu): the next left-click re-bases
    /// the active grip edit instead of committing it.
    pub(super) grip_base_pending: bool,
    pub(super) selected_handle: Option<Handle>,
    pub(super) selected_authoring: Option<Handle>,
    pub(super) action_bars: super::block_action_bars::ViewState,
    /// Dynamic-block visibility grip for the current single selection.
    pub(super) visibility_grip: Option<super::visibility::VisibilityGrip>,
    pub(super) wireframe: bool,
    pub(super) render_mode: codec::entities::ViewportRenderMode,
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
    /// Drawing history is suspended while `history` belongs to BEDIT.
    pub(super) block_edit_history: Option<super::block_editor::BlockEditHistory>,
    pub(super) block_test: Option<super::block_test::BlockTestSession>,
    pub(super) active_layer: String,
    /// Block references whose properties panel shows per-axis Scale X/Y/Z even
    /// though the three factors are currently equal — the user unchecked the
    /// "Uniform scale" box for them (#427). Per-tab: fresh documents restart
    /// handle numbering, so an app-global set keyed by raw handle leaks one
    /// tab's opt-out into another tab reusing the value (#47).
    pub(super) props_asym_scale: std::collections::HashSet<u64>,
    /// OLE objects' scale base and aspect lock, by handle.
    pub(super) ole_frames: std::collections::HashMap<u64, crate::entities::ole2frame::FrameState>,
    /// Node graph shown by the command line's graph button.
    pub(super) graph: crate::ui::node_graph::Graph,
    /// Currently active UCS. `None` means WCS (identity transform).
    pub(super) active_ucs: Option<Ucs>,
    /// Open sketch, if CREATESKETCH is in effect. `None` is the normal
    /// direct-modelling state.
    pub(super) sketch_session: Option<SketchSession>,
    /// Sketches opened in this tab so far, so each gets a distinct name.
    pub(super) sketch_count: u32,
    /// Custom model-space background color.  `None` = default dark grey.
    pub(super) bg_color: Option<[f32; 4]>,
    /// Custom paper-space background color.  `None` = default off-white grey.
    pub(super) paper_bg_color: Option<[f32; 4]>,
    /// Active REFEDIT session, if any.
    pub(super) refedit_session: Option<RefEditSession>,
    /// Sessions closed by REFCLOSE, reopened by undoing it.
    pub(super) refedit_closed: Vec<RefEditSession>,
    /// Sessions ended by undoing REFEDIT, reopened by redoing it.
    pub(super) refedit_reopen: Vec<RefEditSession>,
    /// COUNT mode, while it is on.
    pub(crate) count: Option<crate::ui::window::count_palette::CountMode>,
    /// Open BEDIT block tabs. Definitions are edited live; each tab owns its
    /// entry snapshot and camera so nested blocks can remain open independently.
    pub(super) block_edits: Vec<BlockEditSession>,
    /// Index of the block tab currently shown, or `None` for Model/Paper space.
    pub(super) active_block_edit: Option<usize>,
    /// Currently active MLeader style name.
    pub(super) active_mleader_style: String,
    /// Last camera_generation value written back to the document.
    pub(super) last_synced_camera_gen: u64,
    /// Session set of unloaded reference keys (Task 8b). Owns the set that
    /// `collect_entries` takes as `unloaded`, so CLI and palette agree.
    pub(super) xref_unloaded: crate::io::xref_model::UnloadSet,
    /// References attached with a relative path while the drawing had no
    /// file yet: stored full until the first save, then made relative.
    pub(super) xref_relative_on_save: rustc_hash::FxHashSet<String>,
    /// Load-time mtimes per reference key (Task 8b). Written on every
    /// palette refresh; `Stale` is detectable from the second refresh on.
    pub(super) xref_stat_cache: crate::io::xref_model::RefStatCache,
    /// NotFound count from the last file-open xref resolution (Task 8b).
    /// The palette renders a neutral "open EXTERNALREFERENCES" notice while non-zero;
    /// never auto-opens a modal. Cleared by a clean palette refresh.
    pub(super) xref_missing: usize,
    /// Every external reference resolved when this drawing was opened:
    /// (name, path, status). Automation reports it in `state.xrefs`.
    pub(super) xref_report: Vec<(String, String, crate::io::xref::XrefStatus)>,
    /// Sentinel "Welcome / Start" tab. Always at index 0 when present.
    /// Cannot be closed; the viewport area renders a welcome page instead
    /// of the model-space shader. The scene is still constructed so the
    /// rest of the code can treat it as a normal tab when reading.
    pub(super) is_start: bool,
    /// Room the docked edges took beside the viewport (left, right) when
    /// this drawing was last on screen, and the window size then. A dock
    /// change pans the view by the difference so the drawing stays put,
    /// and either change sets the viewport's new size right away.
    pub(super) dock_inset: Option<(f32, f32, (f32, f32))>,
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
    /// `suspended_cmd` was parked by a transparent command (`'ZOOM`) and is
    /// restored as soon as the transparent one ends.
    pub(super) transparent_resume: bool,
    /// Model-space corners of the last crossing window drawn with no command
    /// running: STRETCH on that selection stretches through it (#1068).
    pub(super) last_crossing_window: Option<([f64; 2], [f64; 2])>,
    /// Colours picked in this drawing this session, newest first. Not saved.
    pub(super) recent_colors: Vec<codec::types::Color>,
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

    /// The [`crate::scene::parametric_constraints::ParametricScope`] a new persistent
    /// constraint should attach to right now — whatever's actually being
    /// edited: the open block definition if a BEDIT session is active, model
    /// space otherwise.
    pub(super) fn current_parametric_scope(
        &self,
    ) -> crate::scene::parametric_constraints::ParametricScope {
        match self.active_block_edit_session() {
            Some(session) => {
                crate::scene::parametric_constraints::ParametricScope::Block(session.br_handle)
            }
            None => crate::scene::parametric_constraints::ParametricScope::ModelSpace,
        }
    }

    /// Record one entity a plugin write touched (API v13 dirty regions).
    /// `epoch_before` is the geometry epoch before that write.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(super) fn mark_plugin_entity_dirty(&mut self, handle: codec::Handle, epoch_before: u64) {
        if !self.plugin_dirty_degraded(epoch_before) {
            self.plugin_dirty_entities.insert(handle);
        }
    }

    /// Record one (entity, app) XDATA/XRecord touch (API v13 dirty regions).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(super) fn mark_plugin_record_dirty(
        &mut self,
        handle: codec::Handle,
        app: String,
        epoch_before: u64,
    ) {
        if !self.plugin_dirty_degraded(epoch_before) {
            self.plugin_dirty_records.insert((handle, app));
        }
    }

    /// Degrade the sets to "walk everything" when they can no longer be
    /// complete: over the cap, or when the drawing changed outside plugin
    /// writes since the last publish.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn plugin_dirty_degraded(&mut self, epoch_before: u64) -> bool {
        const DIRTY_CAP: usize = 10_000;
        if epoch_before != self.plugin_dirty_epoch
            || self.plugin_dirty_entities.len() + self.plugin_dirty_records.len() >= DIRTY_CAP
        {
            self.plugin_dirty_overflow = true;
            self.plugin_dirty_entities.clear();
            self.plugin_dirty_records.clear();
        }
        self.plugin_dirty_overflow
    }

    /// Take the region sets for a boundary publish at the current epoch;
    /// empty sets ("walk everything") when degraded.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(super) fn take_plugin_dirty(
        &mut self,
    ) -> (Vec<codec::Handle>, Vec<(codec::Handle, String)>) {
        let entities = std::mem::take(&mut self.plugin_dirty_entities);
        let records = std::mem::take(&mut self.plugin_dirty_records);
        let degraded = std::mem::replace(&mut self.plugin_dirty_overflow, false);
        self.plugin_dirty_epoch = self.scene.geometry_epoch;
        if degraded {
            (Vec::new(), Vec::new())
        } else {
            (entities.into_iter().collect(), records.into_iter().collect())
        }
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

    /// True when the active pane has a drawing coordinate plane.
    pub(super) fn editing_model_space(&self) -> bool {
        self.scene.current_layout == "Model"
            || self.scene.active_viewport.is_some()
            || self.active_ucs.is_some()
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
        if h.model_space_ucs_name.is_empty() && super::helpers::UcsXform::from_ucs(&u).is_identity()
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
            Some(codec::EntityType::Viewport(vp)) => vp,
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

    pub(super) fn ucs_from_layout(&self) -> Option<Ucs> {
        let layout = self.scene.document.objects.values().find_map(|object| {
            let codec::objects::ObjectType::Layout(layout) = object else {
                return None;
            };
            (layout.name == self.scene.current_layout).then_some(layout)
        })?;
        let mut ucs = self
            .scene
            .document
            .ucss
            .iter()
            .find(|ucs| ucs.handle == layout.named_ucs)
            .cloned()
            .unwrap_or_else(|| Ucs::new("*PAPERUCS*"));
        ucs.origin = codec::types::Vector3::new(
            layout.ucs_origin.0,
            layout.ucs_origin.1,
            layout.ucs_origin.2,
        );
        ucs.x_axis = codec::types::Vector3::new(
            layout.ucs_x_axis.0,
            layout.ucs_x_axis.1,
            layout.ucs_x_axis.2,
        );
        ucs.y_axis = codec::types::Vector3::new(
            layout.ucs_y_axis.0,
            layout.ucs_y_axis.1,
            layout.ucs_y_axis.2,
        );
        ucs.elevation = layout.elevation;
        ucs.ortho_type = layout.ucs_ortho_type;
        ucs.named_ucs_handle = layout.named_ucs;
        ucs.base_ucs_handle = layout.base_ucs;
        let is_world = layout.named_ucs.is_null()
            && layout.base_ucs.is_null()
            && layout.ucs_ortho_type == 0
            && layout.elevation.abs() <= f64::EPSILON
            && super::helpers::UcsXform::from_ucs(&ucs).is_identity();
        (!is_world).then_some(ucs)
    }

    /// Set `active_ucs` to the UCS of the *current pane*: the entered viewport's
    /// own per-viewport UCS, the model header UCS, or the layout UCS. Keeps the
    /// ViewCube in lock-step. Call on every pane
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
            self.ucs_from_layout()
        };
        if self.active_block_edit.is_none()
            && self.scene.active_viewport.is_none()
            && self.scene.current_layout != "Model"
        {
            self.sync_paper_ucs_header();
        }
        self.sync_ucs_to_scene();
    }

    fn sync_paper_ucs_header(&mut self) {
        use codec::types::Vector3;
        let header = &mut self.scene.document.header;
        match &self.active_ucs {
            Some(ucs) => {
                header.paper_space_ucs_origin = ucs.origin;
                header.paper_space_ucs_x_axis = ucs.x_axis;
                header.paper_space_ucs_y_axis = ucs.y_axis;
                header.paper_elevation = ucs.elevation;
                header.paper_space_ucs_name = if (ucs.named_ucs_handle.is_valid()
                    || ucs.handle.is_valid())
                    && !ucs.name.starts_with('*')
                {
                    ucs.name.clone()
                } else {
                    String::new()
                };
                header.paper_ucs_ortho_ref = ucs.base_ucs_handle;
                header.paper_ucs_ortho_view = ucs.ortho_type;
            }
            None => {
                header.paper_space_ucs_origin = Vector3::ZERO;
                header.paper_space_ucs_x_axis = Vector3::UNIT_X;
                header.paper_space_ucs_y_axis = Vector3::UNIT_Y;
                header.paper_elevation = 0.0;
                header.paper_space_ucs_name.clear();
                header.paper_ucs_ortho_ref = Handle::NULL;
                header.paper_ucs_ortho_view = 0;
            }
        }
    }

    /// Persist `active_ucs` back to its pane's storage so it round-trips: the
    /// entered viewport's per-viewport UCS fields, or the document header's
    /// model-space UCS in the Model tab, or the layout UCS. Call after a change.
    pub(super) fn persist_active_ucs(&mut self) {
        use codec::types::Vector3;
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
            if let Some(codec::EntityType::Viewport(vp)) = self.scene.document.get_entity_mut(h) {
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
        } else {
            let (origin, x_axis, y_axis, elevation, ortho, named, base) = match &self.active_ucs {
                Some(ucs) => (
                    ucs.origin,
                    ucs.x_axis,
                    ucs.y_axis,
                    ucs.elevation,
                    ucs.ortho_type,
                    if ucs.named_ucs_handle.is_valid() {
                        ucs.named_ucs_handle
                    } else {
                        ucs.handle
                    },
                    ucs.base_ucs_handle,
                ),
                None => (
                    Vector3::ZERO,
                    Vector3::UNIT_X,
                    Vector3::UNIT_Y,
                    0.0,
                    0,
                    Handle::NULL,
                    Handle::NULL,
                ),
            };
            for object in self.scene.document.objects.values_mut() {
                let codec::objects::ObjectType::Layout(layout) = object else {
                    continue;
                };
                if layout.name == self.scene.current_layout {
                    layout.ucs_origin = (origin.x, origin.y, origin.z);
                    layout.ucs_x_axis = (x_axis.x, x_axis.y, x_axis.z);
                    layout.ucs_y_axis = (y_axis.x, y_axis.y, y_axis.z);
                    layout.elevation = elevation;
                    layout.ucs_ortho_type = ortho;
                    layout.named_ucs = named;
                    layout.base_ucs = base;
                    break;
                }
            }
            self.sync_paper_ucs_header();
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
        scene.populate_new_drawing_defaults();
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
            document_opened_pending: true,
            prev_selection: Vec::new(),
            tab_title: format!("Drawing{}", n),
            properties: PropertiesPanel::empty(),
            layers: LayerPanel::default(),
            active_cmd: None,
            pending_parameter_set_follow_up: None,
            pending_xy_box_set: None,
            pending_action_pair: None,
            measurement: super::measurement::MeasurementDisplay::default(),
            pending_pause_tokens: None,
            layiso_state: None,
            layiso_mode: LayisoMode::Off,
            last_cmd: None,
            last_draw_anchor: None,
            snap_result: None,
            active_grip: None,
            selected_grips: vec![],
            selected_grip_handles: vec![],
            #[cfg(not(target_arch = "wasm32"))]
            plugin_grips: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            plugin_classes: Default::default(),
            plugin_dirty_entities: Default::default(),
            plugin_dirty_records: Default::default(),
            plugin_dirty_overflow: false,
            plugin_dirty_epoch: 0,
            #[cfg(not(target_arch = "wasm32"))]
            active_plugin_grip: None,
            hot_grips: rustc_hash::FxHashSet::default(),
            grip_copy: false,
            grip_base_pending: false,
            selected_handle: None,
            selected_authoring: None,
            action_bars: Default::default(),
            visibility_grip: None,
            table_grip: None,
            wireframe: false,
            render_mode: codec::entities::ViewportRenderMode::Wireframe2D,
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
            block_edit_history: None,
            block_test: None,
            active_layer: "0".to_string(),
            props_asym_scale: std::collections::HashSet::new(),
            ole_frames: std::collections::HashMap::new(),
            graph: Default::default(),
            active_ucs: None,
            sketch_session: None,
            sketch_count: 0,
            bg_color: None,
            paper_bg_color: None,
            refedit_session: None,
            refedit_closed: Vec::new(),
            refedit_reopen: Vec::new(),
            count: None,
            block_edits: Vec::new(),
            active_block_edit: None,
            active_mleader_style: "Standard".to_string(),
            last_synced_camera_gen: 0,
            xref_unloaded: crate::io::xref_model::UnloadSet::default(),
            xref_relative_on_save: rustc_hash::FxHashSet::default(),
            xref_stat_cache: crate::io::xref_model::RefStatCache::default(),
            xref_missing: 0,
            xref_report: Vec::new(),
            is_start: false,
            dock_inset: None,
            pan_mode: false,
            orbit_mode: false,
            zoom_dynamic_mode: false,
            plugin_state: HashMap::new(),
            suspended_cmd: None,
            transparent_resume: false,
            last_crossing_window: None,
            recent_colors: Vec::new(),
            layer_walk: None,
            layer_walk_rows: Vec::new(),
            layer_walk_picking: false,
            layer_walk_filters: Vec::new(),
            layer_walk_combo: iced::widget::combo_box::State::new(Vec::new()),
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

    /// A whole document just replaced this tab's content (file open, template
    /// new, system open): the next message boundary announces
    /// `DocumentOpened` once for this load (API v9). Call after the document
    /// and its derived caches have settled.
    pub(super) fn note_document_loaded(&mut self) {
        self.document_opened_pending = true;
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
    // Boxed: `DeltaSnapshot` grew past the other variants once
    // `parametric_constraints` (ERASE-undo fix) was added, and every undo/redo
    // stack slot costs as much as this enum's largest variant regardless of
    // which one it actually holds.
    Delta(Box<DeltaSnapshot>),
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
                .saturating_add(d.active_layer.as_ref().map_or(0, |(before, after)| {
                    before.len().saturating_add(after.len())
                }))
                .saturating_add(
                    d.parametric_constraints
                        .iter()
                        .map(|entry| {
                            entry
                                .before
                                .constraints
                                .len()
                                .saturating_add(entry.after.constraints.len())
                        })
                        .sum::<usize>()
                        .saturating_mul(96),
                )
                .saturating_add(d.named_parameters.as_ref().map_or(0, |(before, after)| {
                    before.len().saturating_add(after.len()).saturating_mul(128)
                }))
                .saturating_add(d.visibility_states.as_ref().map_or(0, |(before, after)| {
                    (before.len() + after.len()).saturating_mul(32)
                }))
                .saturating_add(d.reference_designations.len().saturating_mul(16))
                .saturating_add(d.label.len()),
            HistorySnapshot::ObjectVisibility(v) => v
                .before
                .hidden
                .len()
                .saturating_add(v.before.keep.as_ref().map_or(0, rustc_hash::FxHashSet::len))
                .saturating_add(v.after.hidden.len())
                .saturating_add(v.after.keep.as_ref().map_or(0, rustc_hash::FxHashSet::len))
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
    /// Editor current-state indices must follow undo even when two states show identical objects.
    pub(super) visibility_states: Option<(
        rustc_hash::FxHashMap<Handle, usize>,
        rustc_hash::FxHashMap<Handle, usize>,
    )>,
    pub(super) entities: Vec<(Handle, Option<Arc<EntityType>>, Option<Arc<EntityType>>)>,
    pub(super) current_layout_before: String,
    pub(super) current_layout_after: String,
    pub(super) selected_before: Vec<Handle>,
    pub(super) selected_after: Vec<Handle>,
    pub(super) dirty_before: bool,
    pub(super) dirty_after: bool,
    pub(super) active_layer: Option<(String, String)>,
    /// LAYISO state changed by this history entry, if any. Other commands leave
    /// the isolation state untouched, so their history entries keep this None.
    pub(super) layiso_state: Option<(Option<LayisoState>, Option<LayisoState>)>,
    /// Opposite non-entity document state. `apply_delta_state` swaps this with
    /// the live structure, so the same allocation shuttles between undo/redo.
    pub(super) structure: Option<StructureSnapshot>,
    /// Parametric-constraint scopes this same command changed alongside its
    /// entities (e.g. ERASE removing a constraint set's touched constraints)
    /// — `parametric_constraints` lives on `Scene`, not in `document`/
    /// `document.objects`, so it needs its own channel here rather than
    /// riding along with `entities`/`structure`. Almost always empty.
    pub(super) parametric_constraints: Vec<ParametricConstraintsEntryDelta>,
    /// Reference-only designations (API v9) this same command changed — set
    /// state on `Scene` (see `scene::reference_only`), recorded as explicit
    /// first-touch before/afters so undo/redo restores the designation with
    /// the entity it belongs to (e.g. a plugin scope that both erases and
    /// re-designates). Almost always empty.
    pub(super) reference_designations: Vec<ReferenceDesignationDelta>,
    /// Drawing-wide parameter table changed by this transaction.
    pub(super) named_parameters: Option<(
        crate::scene::named_parameters::ParameterTable,
        crate::scene::named_parameters::ParameterTable,
    )>,
    pub(super) label: String,
}

#[derive(Clone)]
pub(super) enum StructureSnapshot {
    /// A broad save-time synchronization followed by a command's bounded edits.
    Combined(Vec<StructureSnapshot>),
    /// Compatibility fallback for genuinely broad structural commands.
    Full(Document),
    /// Exact layer-table entries touched by one command.
    Layers(Vec<TableEntryDelta<codec::tables::Layer>>),
    /// Exact text-style entries touched by one command.
    TextStyles(Vec<TableEntryDelta<codec::tables::TextStyle>>),
    /// Exact dimension-style entries touched by one command.
    DimStyles(Vec<TableEntryDelta<codec::tables::DimStyle>>),
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
            Self::Combined(parts) => parts.iter().map(Self::estimated_bytes).sum(),
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
            || matches!(self,Self::Combined(parts) if parts.iter().any(Self::is_full))
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
    pub(super) before: Option<codec::objects::ObjectType>,
    pub(super) after: Option<codec::objects::ObjectType>,
}

/// One parametric-constraint scope's before/after image within an entity delta.
/// This keeps constraint cleanup atomic with edits such as entity erasure.
#[derive(Clone)]
pub(super) struct ParametricConstraintsEntryDelta {
    pub(super) scope: crate::scene::parametric_constraints::ParametricScope,
    pub(super) before: crate::scene::parametric_constraints::ParametricConstraintSet,
    pub(super) after: crate::scene::parametric_constraints::ParametricConstraintSet,
}

/// One reference-only designation's before/after value within an entity delta
/// (API v9). The designation set lives on `Scene`, not in the document, so
/// the value rides its own channel — symmetric like every other delta field,
/// undo applies the before value, redo the after value.
#[derive(Clone)]
pub(super) struct ReferenceDesignationDelta {
    pub(super) handle: Handle,
    pub(super) before: bool,
    pub(super) after: bool,
}

#[derive(Default)]
pub(super) struct HistoryState {
    pub(super) undo_stack: Vec<HistorySnapshot>,
    pub(super) redo_stack: Vec<HistorySnapshot>,
    pub(super) pending: Option<PendingHistorySnapshot>,
}

pub(super) struct PendingHistorySnapshot {
    pub(super) visibility_states_before: rustc_hash::FxHashMap<Handle, usize>,
    pub(super) label: String,
    pub(super) current_layout: String,
    pub(super) active_layer: String,
    pub(super) selected_before: Vec<Handle>,
    pub(super) dirty_before: bool,
    pub(super) structure_before: Document,
    pub(super) recorder: Arc<codec::document::EntityChangeRecorder>,
    /// First-touch before-values of reference-only designations (API v9)
    /// mutated inside this plugin scope (see `scene::reference_only`). Set
    /// mutations are not document writes, so the scope closes by reading the
    /// set's current state as the after-image and keeping entries whose value
    /// actually flipped.
    pub(super) reference_designations_before: Vec<(Handle, bool)>,
    /// Parametric constraint sets when the scope opened; the scope closes
    /// with a before/after entry for every set that changed, so undo also
    /// removes a constraint a plugin created inside its scope.
    pub(super) parametric_constraints_before:
        Vec<crate::scene::parametric_constraints::ParametricConstraintSet>,
    /// Opened by a plugin's `push_undo`: while a plugin request runs, the
    /// commands it drives join this scope instead of closing it.
    pub(super) plugin_owned: bool,
    /// Kept as an undo step even when nothing changed.
    pub(super) keep_empty: bool,
    /// A plugin's undo group: message boundaries leave it open until the
    /// plugin ends it or the user runs a command.
    pub(super) held: bool,
}
