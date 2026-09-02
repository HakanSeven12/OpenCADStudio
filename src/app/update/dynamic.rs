//! `dynamic` arms and helpers, split out of the original `update.rs` (#mechanical decomposition).

#![allow(unused_imports)]
use super::util::*;
use super::{format_size, VIEWCUBE_HIT_SIZE};
use crate::app::helpers::{
    parse_coord, polar_constrain_near, ucs_rotate_vec, ucs_to_wcs, ucs_z_axis,
    CoordKind,
};
use crate::app::{Message, OpenCADStudio, POLY_START_DELAY_MS};
use crate::modules::ModuleEvent;
use crate::scene::pick::grip::{find_hit_grip, find_hit_grip_paper, find_hit_grip_rte, GripEdit};
use crate::scene::model::object::GripApply;
use crate::scene::{
    self, hover_id, CubeRegion, Scene, VIEWCUBE_DRAW_PX, VIEWCUBE_PAD, VIEWCUBE_PX,
};
use crate::ui::PropertiesPanel;
use acadrust::types::Color as AcadColor;
use acadrust::{EntityType as AcadEntityType, Handle};
use iced::time::Instant;
use iced::{mouse, Point, Task};


impl OpenCADStudio {
    pub(in crate::app) fn active_distance_ray(
        &self,
        i: usize,
    ) -> Option<(glam::DVec3, glam::DVec3)> {
        // OTRACK already exposes its acquired world-space ray.
        if let Some((base, dir)) = self.otrack_active {
            return Some((base, dir));
        }

        // Extension measures along the ray its guide is drawn on. The snap
        // carries the acquired endpoint that guide starts from, so read the ray
        // off that rather than searching the tracking set again — a second
        // search can disagree with the guide the user is looking at, and the
        // acquisition it needs may already have aged out of the set.
        let snap = self.tabs[i].snap_result?;
        if snap.snap_type != crate::snap::SnapType::Extension {
            return None;
        }
        let origin = snap.extension_origin?;
        // The snap sits beyond its endpoint by construction, so the outward
        // direction is simply the way from one to the other.
        let dir = snap.world - origin;
        Some((origin, dir.try_normalize()?))
    }
    /// Rebuild the active tab's dynamic-input field set to match what the
    /// command is currently asking for. Called on cursor move and after
    /// command-state changes. The field set only changes shape when the
    /// command's `dyn_field()` or the presence of a base point changes;
    /// existing typed buffers survive an unchanged shape.
    pub(in crate::app) fn sync_dyn_fields(&mut self) {
        use crate::app::document::{DynComponent, DynFieldEntry};
        let i = self.active_tab;

        if !self.dyn_input {
            self.tabs[i].dyn_fields.clear();
            self.tabs[i].dyn_active = 0;
            self.tabs[i].dyn_anchor = None;
            self.tabs[i].dyn_ref = None;
            return;
        }

        // A normal 2D grip stretch behaves like a point-placement step:
        // distance and angle are measured from the grip's original position.
        //
        // Grip editing is not an `active_cmd`, so handle it before the normal
        // command-only path below.
        let grip_origin = self.tabs[i]
            .active_grip
            .as_ref()
            .filter(|grip| {
                grip.mode == crate::scene::pick::grip::GripEditMode::Stretch
            })
            .map(|grip| grip.origin_world);

        if let Some(origin) = grip_origin {
            let wanted = [DynComponent::Distance, DynComponent::Angle];
            let current: Vec<DynComponent> = self.tabs[i]
                .dyn_fields
                .iter()
                .map(|field| field.component)
                .collect();

            if current.as_slice() != wanted {
                self.tabs[i].dyn_fields = wanted
                    .into_iter()
                    .map(DynFieldEntry::new)
                    .collect();
                self.tabs[i].dyn_active = 0;
            }

            self.tabs[i].dyn_guide = crate::command::DynGuide::Polar;
            self.tabs[i].dyn_anchor = Some(origin);
            self.tabs[i].dyn_ref = None;
            return;
        }

        if self.tabs[i].active_cmd.is_none() {
            self.tabs[i].dyn_fields.clear();
            self.tabs[i].dyn_active = 0;
            self.tabs[i].dyn_anchor = None;
            self.tabs[i].dyn_ref = None;
            return;
        }
        // A command may describe its step explicitly via `dyn_spec()` — that
        // takes full control of the boxes, guide and anchor. Otherwise fall
        // back to the legacy `dyn_field()` shaping below.
        if let Some(spec) = self.tabs[i].active_cmd.as_ref().and_then(|c| c.dyn_spec()) {
            self.apply_dyn_spec(i, spec);
            return;
        }
        let field = self.tabs[i]
            .active_cmd
            .as_ref()
            .map(|c| c.dyn_field())
            .unwrap_or(crate::command::DynField::Point);
        // A text-input step reads a single scalar. The overlay shows one box
        // the user types into (or, when the command supplies a live value,
        // sets by moving the cursor); on commit the host routes it to
        // `on_text_input` instead of `on_point`. A distance prompt keeps the
        // `Distance` box (so a perpendicular-distance live value reads
        // naturally); everything else uses the typed-only `Scalar` box.
        let wants_text = self.tabs[i]
            .active_cmd
            .as_ref()
            .map(|c| c.input_kind().wants_text())
            .unwrap_or(false);
        // A point step that also accepts keyword letters (PLINE A/L/C…) keeps
        // its polar boxes: only letters reach the command line, digits stay
        // coordinates. So such a step is NOT treated as a text-only prompt.
        let point_keywords = self.tabs[i]
            .active_cmd
            .as_ref()
            .map(|c| c.point_step_accepts_keywords())
            .unwrap_or(false);
        let wants_text = wants_text && !point_keywords;
        // A step that hit-tests for an object (entity / structure pick) has no
        // coordinate to enter — clicks select, they don't place a point. Show
        // no coordinate box so the cursor stays clean and typed option keywords
        // (e.g. FILLET's "R") reach the command line instead of an X/Y field.
        let picks_object = self.tabs[i]
            .active_cmd
            .as_ref()
            .map(|c| c.needs_entity_pick() || c.needs_structure_point_pick())
            .unwrap_or(false);
        let has_base = self.last_point.is_some();
        // While aligned to an OTRACK ray, the point step reads a single
        // distance along the ray (issue #69) — show one Distance box.
        let guided_dist = self.active_distance_ray(i).is_some()
            && !wants_text
            && matches!(field, crate::command::DynField::Point);
        let default: Vec<DynComponent> = match field {
            _ if guided_dist => vec![DynComponent::Distance],
            crate::command::DynField::Distance => vec![DynComponent::Distance],
            crate::command::DynField::Angle => vec![DynComponent::Angle],
            crate::command::DynField::Scalar => vec![DynComponent::Scalar],
            // A text prompt with the default `Point` field reads free text /
            // a name / a keyword (which may itself contain digits) from the
            // command line — show no scalar box, so digits reach the command
            // line instead of being captured into a dyn buffer.
            crate::command::DynField::Point if wants_text || picks_object => vec![],
            crate::command::DynField::Point if has_base => {
                vec![DynComponent::Distance, DynComponent::Angle]
            }
            crate::command::DynField::Point => vec![DynComponent::X, DynComponent::Y],
        };
        // Multiple shapes can satisfy the same command request — e.g. a
        // `Point` is happy with either `[Distance, Angle]` (polar) or
        // `[X, Y]` / `[X, Y, Z]` (cartesian). If the user already
        // reshaped via `,` (see #35) the existing set is still a valid
        // Point configuration and must not be reverted on every mouse
        // move.
        let current: Vec<DynComponent> = self.tabs[i]
            .dyn_fields
            .iter()
            .map(|f| f.component)
            .collect();
        // Only treat a cartesian / polar variant as "good enough to keep"
        // when the user explicitly reshaped via `,`. Otherwise we follow
        // the command's default so e.g. clicking the first point of LINE
        // flips a stale `[X, Y]` (from before there was a base) over to
        // the polar `[Distance, Angle]` the prompt actually wants.
        let current_is_acceptable = if self.dyn_user_reshaped && !wants_text && !picks_object {
            match field {
                crate::command::DynField::Distance => {
                    matches!(current.as_slice(), [DynComponent::Distance])
                }
                crate::command::DynField::Angle => {
                    matches!(current.as_slice(), [DynComponent::Angle])
                }
                crate::command::DynField::Scalar => {
                    matches!(current.as_slice(), [DynComponent::Scalar])
                }
                crate::command::DynField::Point => matches!(
                    current.as_slice(),
                    [DynComponent::Distance, DynComponent::Angle]
                        | [DynComponent::X, DynComponent::Y]
                        | [DynComponent::X, DynComponent::Y, DynComponent::Z]
                ),
            }
        } else {
            current == default
        };
        if !current_is_acceptable {
            self.tabs[i].dyn_fields = default.into_iter().map(DynFieldEntry::new).collect();
            self.tabs[i].dyn_active = 0;
        }
        // Derive the guide + anchor for the legacy field set so the overlay
        // draws the right construction without each command opting in.
        let comps: Vec<DynComponent> =
            self.tabs[i].dyn_fields.iter().map(|f| f.component).collect();
        self.tabs[i].dyn_guide = match comps.as_slice() {
            [DynComponent::Distance, DynComponent::Angle] | [DynComponent::Angle] => {
                crate::command::DynGuide::Polar
            }
            [DynComponent::Distance] => crate::command::DynGuide::Radius,
            _ => crate::command::DynGuide::None,
        };
        self.tabs[i].dyn_anchor = self.last_point;
        self.tabs[i].dyn_ref = None;
    }

    /// Apply an explicit per-step [`DynSpec`](crate::command::DynSpec): rebuild
    /// the boxes from its roles (preserving typed buffers when the role set is
    /// unchanged), and set the guide + anchor.

    pub(in crate::app) fn apply_dyn_spec(&mut self, i: usize, spec: crate::command::DynSpec) {
        use crate::app::document::DynFieldEntry;
        let new_roles: Vec<crate::command::DynRole> =
            spec.fields.iter().map(|f| f.role).collect();
        let cur_roles: Vec<crate::command::DynRole> =
            self.tabs[i].dyn_fields.iter().map(|f| f.role).collect();
        if cur_roles != new_roles {
            self.tabs[i].dyn_fields =
                spec.fields.iter().map(|f| DynFieldEntry::from_role(f.role)).collect();
            self.tabs[i].dyn_active = 0;
        }
        self.tabs[i].dyn_guide = spec.guide;
        self.tabs[i].dyn_anchor = match spec.anchor {
            crate::command::DynAnchor::LastPoint => self.last_point,
            crate::command::DynAnchor::Point(p) => Some(p),
        };
        self.tabs[i].dyn_ref = spec.ref_point;
    }

    /// Track cursor dwell over a selected entity's grip. Sets
    /// `grip_hover` while the cursor sits within `GRIP_THRESHOLD_PX` of
    /// a grip and opens `grip_popup` once the dwell exceeds the
    /// threshold. Cursor drift clears both.
    /// After the active Model tile changes, mirror its stored visual style
    /// into the tab so the picker shows it and the tile renders with it
    /// (the active tile draws with the tab's live render mode).

    /// Resolve the world point implied by the current dynamic-input field
    /// values. Locked fields use their typed buffer; the rest fall back to
    /// the live cursor-derived value. Returns `None` when the field set
    /// isn't one we know how to turn into a point.
    /// Hand the active command the current full-precision coordinate frame.
    pub(in crate::app) fn push_ucs_to_cmd(&mut self, i: usize) {
        let plane = if self.tabs[i].editing_model_space() {
            self.tabs[i].ucs_xform().working_plane()
        } else {
            crate::command::WorkingPlane::default()
        };
        if let Some(c) = self.tabs[i].active_cmd.as_mut() {
            c.set_working_plane(plane);
        }
    }

    /// Push the live Ctrl state into tab `i`'s active command before it builds a
    /// preview/commit (arc-direction flip on `ARC_CONT`, etc.).
    pub(in crate::app) fn push_ctrl_to_cmd(&mut self, i: usize) {
        let ctrl = self.ctrl_down;
        if let Some(c) = self.tabs[i].active_cmd.as_mut() {
            c.set_ctrl(ctrl);
        }
    }

    /// Cache the endpoint + exit tangent of a just-committed line/arc so a
    /// following `ARC_CONT` starts tangentially from where drawing ended. Set to
    /// `None` for any other entity kind. Uses `last_point` (the final pick) to
    /// pick the endpoint the pen actually finished on.
    pub(in crate::app) fn update_cont_anchor(&mut self, entity: &acadrust::EntityType) {
        let last = self.last_point;
        self.cont_anchor = crate::modules::draw::draw::arc::continue_anchor(entity, last);
    }

    pub(in crate::app) fn dyn_resolve_point(&self) -> Option<glam::DVec3> {
        use crate::app::document::DynComponent;
        let i = self.active_tab;
        let fields = &self.tabs[i].dyn_fields;
        if fields.is_empty() {
            return None;
        }
        let w = self.tabs[i].last_cursor_world;
        let base = self.tabs[i]
            .dyn_anchor
            .or(self.last_point)
            .unwrap_or(glam::DVec3::ZERO);
        // Buffer value parsed as f32 (de-scaled by the role so a typed diameter
        // becomes a radius), or the supplied geometric live value. Width/Height
        // are shown unsigned, so a typed value normally takes the sign of the
        // cursor's delta on that axis (`live` is the signed delta in the
        // cartesian arms). But once the user separates the two values with `,`
        // — e.g. the rectangle opposite corner entered as "50,50" — the entry
        // is a cartesian coordinate pair, not cursor-signed W/H dimensions, so
        // the typed sign must stay literal. `,` is the only thing that sets
        // `dyn_user_reshaped`, so it cleanly distinguishes comma-cartesian entry
        // from Tab-separated dimension entry (#269).
        let comma_cartesian = self.dyn_user_reshaped;
        let val = |idx: usize, live: f64| -> f64 {
            match fields[idx]
                .buffer
                .as_ref()
                .map(|s| s.trim().replace(',', "."))
                .and_then(|s| crate::app::expr_eval::eval_number(&s))
                .map(|v| v / fields[idx].role.value_scale() as f64)
            {
                Some(v) => {
                    if matches!(
                        fields[idx].role,
                        crate::command::DynRole::Width | crate::command::DynRole::Height
                    ) && !comma_cartesian
                    {
                        v.abs().copysign(live)
                    } else {
                        v
                    }
                }
                None => live,
            }
        };
        // Work in the active UCS frame: the cursor delta from the base is
        // rotated into UCS, so typed cartesian/polar values are interpreted in
        // the user's coordinate system and mapped back to world on return.
        // (The delta is offset-invariant, so only the rotation matters.)
        let xf = self.tabs[i].ucs_xform();
        let d_ucs = xf.vec_to_ucs(w - base);
        let w_ucs = xf.to_ucs(w);
        let dx = d_ucs.x;
        let dy = d_ucs.y;
        let dz = d_ucs.z;
        let live_d = (dx * dx + dy * dy).sqrt();
        let live_a = dy.atan2(dx); // radians, in the UCS plane
        // A typed angle is shown unsigned (0..180); give it the sign of the
        // cursor's current side so an entry made below the X axis sweeps
        // downward to match the arc instead of mirroring up. An EXPLICIT
        // `-`/`+` prefix keeps its literal sign though — "-30" must mean
        // −30° (= 330°) regardless of where the cursor sits (#417).
        // Untyped → live.
        let angle_rad = |idx: usize| -> f64 {
            let raw = fields[idx]
                .buffer
                .as_ref()
                .map(|s| s.trim().replace(',', "."));
            let explicit_sign = raw
                .as_deref()
                .is_some_and(|s| s.starts_with('-') || s.starts_with('+'));
            match raw.and_then(|s| crate::app::expr_eval::eval_number(&s)) {
                Some(v) if explicit_sign => v.to_radians(),
                Some(mag) => mag.abs().to_radians().copysign(dy),
                None => live_a,
            }
        };
        let comps: Vec<DynComponent> = fields.iter().map(|f| f.component).collect();
        // Perpendicular offset: a single distance measured square to the
        // reference line (anchor → dyn_ref). The committed point lies on the
        // perpendicular through the anchor at that offset; the command projects
        // it. Untyped tracks the cursor's signed offset; typed takes the
        // cursor's side.
        if let (Some(ref_pt), [DynComponent::Distance]) =
            (self.tabs[i].dyn_ref, comps.as_slice())
        {
            let axis = (ref_pt - base).normalize_or_zero();
            let perp = glam::DVec3::new(-axis.y, axis.x, 0.0);
            let signed = (w - base).dot(perp);
            let typed = fields[0]
                .buffer
                .as_ref()
                .map(|s| s.trim().replace(',', "."))
                .and_then(|s| crate::app::expr_eval::eval_number(&s));
            let h = match typed {
                Some(v) => v.abs().copysign(signed),
                None => signed,
            };
            return Some(base + perp * h);
        }
        // DYN-on defaults to RELATIVE coordinates when a base point is set
        // (see #26 / #35). The live cartesian fallback is the cursor
        // position relative to base; typed values are relative deltas.
        let has_base = self.last_point.is_some();
        let relative = has_base && !self.dyn_coord_absolute;
        // Relative result: base + the typed UCS-frame offset mapped to world.
        let rel = |off_ucs: glam::DVec3| base + xf.vec_to_wcs(off_ucs);
        match comps.as_slice() {
            [DynComponent::X, DynComponent::Y] if relative => {
                Some(rel(glam::DVec3::new(val(0, dx), val(1, dy), 0.0)))
            }
            [DynComponent::X, DynComponent::Y] => {
                Some(xf.to_wcs(glam::DVec3::new(
                    val(0, w_ucs.x),
                    val(1, w_ucs.y),
                    0.0,
                )))
            }
            [DynComponent::X, DynComponent::Y, DynComponent::Z] if relative => {
                Some(rel(glam::DVec3::new(val(0, dx), val(1, dy), val(2, dz))))
            }
            [DynComponent::X, DynComponent::Y, DynComponent::Z] => {
                Some(xf.to_wcs(glam::DVec3::new(
                    val(0, w_ucs.x),
                    val(1, w_ucs.y),
                    val(2, w_ucs.z),
                )))
            }
            [DynComponent::Distance, DynComponent::Angle] => {
                let d = val(0, live_d);
                let a = angle_rad(1);
                Some(rel(glam::DVec3::new(d * a.cos(), d * a.sin(), 0.0)))
            }
            [DynComponent::Distance] => {
                // Keep the cursor's direction (in UCS), override the magnitude.
                let dir = glam::DVec3::new(dx, dy, 0.0).normalize_or(glam::DVec3::X);
                Some(rel(dir * val(0, live_d)))
            }
            [DynComponent::Angle] => {
                // Standalone angle (e.g. ROTATE): the typed value is an
                // absolute CCW angle in the UCS plane, not a cursor-signed
                // magnitude — keep it literal. Only the polar Distance+Angle
                // pair uses the cursor-signed `angle_rad`.
                let a = val(0, live_a.to_degrees()).to_radians();
                Some(rel(glam::DVec3::new(live_d * a.cos(), live_d * a.sin(), 0.0)))
            }
            _ => None,
        }
    }

    /// Handle `,` while a dynamic-input field set is showing. Locks the
    /// current field's buffer if it has one, then either advances within
    /// the existing field set or reshapes it: a polar `[Distance, Angle]`
    /// configuration becomes cartesian `[X(buf), Y]`, and a cartesian
    /// `[X, Y]` configuration extends to `[X, Y, Z]`. Default fallthrough
    /// is "advance to next field", matching `Tab`. See #35.

    pub(in crate::app) fn dyn_comma_advance(&mut self) {
        use crate::app::document::{DynComponent, DynFieldEntry};
        let i = self.active_tab;
        if self.tabs[i].dyn_fields.is_empty() {
            return;
        }
        // The user picked a shape — `sync_dyn_fields` preserves it until
        // the next commit / command-start clears the flag.
        self.dyn_user_reshaped = true;
        let active = self.tabs[i]
            .dyn_active
            .min(self.tabs[i].dyn_fields.len() - 1);
        let comps: Vec<DynComponent> = self.tabs[i]
            .dyn_fields
            .iter()
            .map(|f| f.component)
            .collect();
        let cur_buf = self.tabs[i].dyn_fields[active].buffer.clone();
        match (comps.as_slice(), active) {
            // First polar field — `,` switches to cartesian, locking the
            // typed value as X.
            ([DynComponent::Distance, DynComponent::Angle], 0) | ([DynComponent::Distance], 0) => {
                let mut x_field = DynFieldEntry::new(DynComponent::X);
                x_field.buffer = cur_buf;
                self.tabs[i].dyn_fields = vec![x_field, DynFieldEntry::new(DynComponent::Y)];
                self.tabs[i].dyn_active = 1;
            }
            // Already cartesian X (first field) — just advance to Y.
            ([DynComponent::X, DynComponent::Y], 0)
            | ([DynComponent::X, DynComponent::Y, DynComponent::Z], 0) => {
                self.tabs[i].dyn_active = 1;
            }
            // Cartesian Y — extend to 3-D by appending Z.
            ([DynComponent::X, DynComponent::Y], 1) => {
                self.tabs[i]
                    .dyn_fields
                    .push(DynFieldEntry::new(DynComponent::Z));
                self.tabs[i].dyn_active = 2;
            }
            // Cartesian Y in the 3-D set — advance to Z.
            ([DynComponent::X, DynComponent::Y, DynComponent::Z], 1) => {
                self.tabs[i].dyn_active = 2;
            }
            // Z, Angle, or any singleton: nothing further to advance to.
            _ => {}
        }
    }

    /// Whether the current dynamic field set represents a point coordinate
    /// that can be switched with `@` / `#`.
    pub(in crate::app) fn dyn_has_coordinate_fields(&self, i: usize) -> bool {
        use crate::app::document::DynComponent;
        let comps: Vec<DynComponent> = self.tabs[i]
            .dyn_fields
            .iter()
            .map(|field| field.component)
            .collect();
        comps.iter().any(|component| {
            matches!(
                component,
                DynComponent::X | DynComponent::Y | DynComponent::Z
            )
        }) || matches!(
            comps.as_slice(),
            [DynComponent::Distance, DynComponent::Angle]
        )
    }

    /// Switch the current dynamic point entry between relative (`@`) and
    /// absolute (`#`) UCS coordinates. Polar fields become cartesian because
    /// the selected mode applies to coordinate components, not distance/angle.
    pub(in crate::app) fn dyn_set_coordinate_mode(&mut self, absolute: bool) {
        use crate::app::document::{DynComponent, DynFieldEntry};
        let i = self.active_tab;
        self.dyn_coord_absolute = absolute;

        let comps: Vec<DynComponent> = self.tabs[i]
            .dyn_fields
            .iter()
            .map(|field| field.component)
            .collect();
        if matches!(
            comps.as_slice(),
            [DynComponent::Distance, DynComponent::Angle]
        ) {
            let first_buffer = self.tabs[i].dyn_fields[0].buffer.clone();
            let mut x = DynFieldEntry::new(DynComponent::X);
            x.buffer = first_buffer;
            self.tabs[i].dyn_fields = vec![x, DynFieldEntry::new(DynComponent::Y)];
            self.tabs[i].dyn_active = usize::from(self.tabs[i].dyn_fields[0].locked());
        }

        self.dyn_user_reshaped = true;
        self.sync_dyn_fields();
    }

    /// If dynamic input has at least one locked (typed) field, resolve the
    /// implied point, feed it to the active command as a point pick, reset
    /// the field buffers, and return the resulting task. Returns `None`
    /// when there is nothing typed, so the caller falls back to its normal
    /// Enter handling.
    /// Give a bare typed angle the sign of the cursor's side relative to the
    /// step's reference direction, so a commit-as-text angle rotates/sweeps the
    /// way the cursor is dragging. Only applies to steps with an `Angle` field;
    /// an explicit `+`/`-` is left untouched. Returns the (possibly re-signed)
    /// text to feed `on_text_input`.

    pub(in crate::app) fn dyn_sign_angle_text(&self, i: usize, text: String) -> String {
        let has_angle = self.tabs[i]
            .dyn_fields
            .iter()
            .any(|f| f.role == crate::command::DynRole::Angle);
        let t = text.trim();
        if !has_angle || t.is_empty() || t.starts_with('-') || t.starts_with('+') {
            return text;
        }
        if t.parse::<f32>().is_err() {
            return text;
        }
        let anchor = self.tabs[i]
            .dyn_anchor
            .or(self.last_point)
            .unwrap_or(glam::DVec3::ZERO);
        let cur = self.tabs[i].last_cursor_world;
        let a_cur = (cur.y - anchor.y).atan2(cur.x - anchor.x);
        let a_ref = self.tabs[i]
            .dyn_ref
            .map(|r| (r.y - anchor.y).atan2(r.x - anchor.x))
            .unwrap_or(0.0);
        let mut d = a_cur - a_ref;
        while d > std::f64::consts::PI {
            d -= std::f64::consts::TAU;
        }
        while d <= -std::f64::consts::PI {
            d += std::f64::consts::TAU;
        }
        if d < 0.0 {
            format!("-{t}")
        } else {
            text
        }
    }


    pub(in crate::app) fn try_dyn_commit(&mut self) -> Option<Task<Message>> {
        let i = self.active_tab;
        if !self.dyn_input
            || self.tabs[i].active_cmd.is_none()
            || self.tabs[i].dyn_fields.is_empty()
            || !self.tabs[i].dyn_fields.iter().any(|f| f.locked())
        {
            return None;
        }
        // OTRACK and Extension both allow a typed scalar to act as a
        // distance measured along the active reference ray.
        if let Some((base, dir)) = self.active_distance_ray(i) {
            let wants_text = self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.input_kind().wants_text())
                .unwrap_or(false);
            if !wants_text {
                if let Some(text) = self.tabs[i]
                    .dyn_fields
                    .iter()
                    .find_map(|f| f.buffer.clone())
                {
                    if let Some(dist) = crate::app::expr_eval::eval_number(text.trim()) {
                        let pt = base + dir * dist;
                        if !self.command_point_allowed(i, pt) {
                            return Some(Task::none());
                        }
                        self.last_point = Some(pt);
                        for f in self.tabs[i].dyn_fields.iter_mut() {
                            f.buffer = None;
                        }
                        self.tabs[i].dyn_active = 0;
                        self.dyn_user_reshaped = false;
                        self.dyn_coord_absolute = false;
                        self.sync_dyn_fields();
                        self.reset_tracking_after_point();
                        self.push_ucs_to_cmd(i);
                        let result = self.tabs[i].active_cmd.as_mut().map(|c| c.on_point(pt));
                        let task = result.map(|r| self.apply_cmd_result(r))?;
                        self.refresh_active_cmd_preview(i);
                        return Some(task);
                    }
                }
            }
        }
        // A text-input step reads its single box as a string and commits via
        // `on_text_input` (a count, radius, distance) rather than resolving a
        // point. Only the typed buffer matters here — a mouse-driven live
        // value commits through the viewport click, not Enter.
        // A point-with-keywords step (PLINE) commits a typed distance/angle as
        // a point, not as text, so it is excluded here.
        let wants_text = self.tabs[i]
            .active_cmd
            .as_ref()
            .map(|c| {
                (c.input_kind().wants_text() && !c.point_step_accepts_keywords())
                    || c.dyn_commit_as_text()
            })
            .unwrap_or(false);
        if wants_text {
            let text = self.tabs[i]
                .dyn_fields
                .iter()
                .find_map(|f| f.buffer.clone())
                .unwrap_or_default();
            let text = crate::app::expr_eval::eval_to_string(text.trim());
            // Shared rule: a bare angle typed into a commit-as-text step takes
            // the sign of the cursor's side relative to the reference, so the
            // committed direction matches the drag (the box shows magnitude
            // only). Commands receive an already-signed string.
            let text = self.dyn_sign_angle_text(i, text);
            let result = self.tabs[i]
                .active_cmd
                .as_mut()
                .and_then(|c| c.on_text_input(&text));
            for f in self.tabs[i].dyn_fields.iter_mut() {
                f.buffer = None;
            }
            self.tabs[i].dyn_active = 0;
            self.sync_dyn_fields();
            let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
            if let Some(p) = prompt {
                self.command_line.push_info(&p);
            }
            self.refresh_active_cmd_preview(i);
            return Some(match result {
                Some(r) => self.apply_cmd_result(r),
                None => self.focus_cmd_input(),
            });
        }
        let pt = self.dyn_resolve_point()?;
        if !self.command_point_allowed(i, pt) {
            return Some(Task::none());
        }
        self.last_point = Some(pt);
        self.dyn_user_reshaped = false;
        self.dyn_coord_absolute = false;
        self.sync_dyn_fields();
        self.reset_tracking_after_point();
        self.push_ucs_to_cmd(i);
        let result = self.tabs[i].active_cmd.as_mut().map(|c| c.on_point(pt));
        for f in self.tabs[i].dyn_fields.iter_mut() {
            f.buffer = None;
        }
        self.tabs[i].dyn_active = 0;
        let task = result.map(|r| self.apply_cmd_result(r))?;
        // Match the command-line path: refresh the rubber-band preview
        // so the next segment immediately starts from the new
        // last_point even though no mouse-move fires after a typed
        // coordinate. See #32.
        self.refresh_active_cmd_preview(i);
        Some(task)
    }

    /// Mutable access to the currently selected table style.

    /// Re-run the active command's preview hook against the current
    /// cursor world position. Keyboard-driven point commits (typed
    /// coordinates in the command line or dynamic input) don't fire a
    /// mouse-move event, so without this the rubber-band preview keeps
    /// dangling from the previous `last_point` until the user actually
    /// moves the mouse. See #32.
    pub(in crate::app) fn refresh_active_cmd_preview(&mut self, i: usize) {
        if self.tabs[i].active_cmd.is_none() {
            return;
        }
        // Re-resolve through the locked dyn fields first so a freshly typed
        // value reshapes the preview immediately, without waiting for the
        // next mouse move (#356). Idempotent for the polar/cartesian arms, so
        // feeding the already-resolved cursor back in is stable.
        if self.tabs[i].dyn_fields.iter().any(|f| f.buffer.is_some()) {
            if let Some(r) = self.dyn_resolve_point() {
                self.tabs[i].last_cursor_world = r;
            }
        }
        let cur = self.tabs[i].last_cursor_world;
        let previews = self.tabs[i]
            .active_cmd
            .as_mut()
            .map(|c| c.on_preview_wires(cur))
            .unwrap_or_default();
        self.tabs[i].scene.set_preview_wires(previews);
    }
}
