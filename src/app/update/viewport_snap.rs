//! Snap to displayed model geometry while a paper-space command is active.
//! Viewport queries use the renderer's resident wires and interaction index,
//! then project the accepted model point onto the sheet. Ordinary selection
//! continues to use paper-space hit testing.

use iced::Point;

use document::types::Handle;

use crate::app::OpenCADStudio;
use crate::scene::viewport_ref::{AcceptedSnap, ViewportFrame};
use crate::snap::{SnapResult, SnapType};

/// Bound on the retained accepted-snap history. A command's point steps are a
/// handful at most; the cap only stops a long polyline from growing forever.
const MAX_ACCEPTED_SNAPS: usize = 64;

/// What a held Shift+letter override changed, so its release can undo it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum HeldOverride {
    /// A snap-only override (or none): undone by `clear_override`.
    Snap,
    /// Running object snaps flipped.
    Osnap,
    /// Polar tracking flipped.
    Polar,
    /// Object snap tracking flipped.
    Otrack,
}

/// The temporary override a Shift+key types (the shifted glyph on a US
/// layout for the punctuation keys): `Some(Some(t))` snaps only to `t`,
/// `Some(None)` disables snapping.
enum OverrideKey {
    Snap(Option<SnapType>),
    Toggle(HeldOverride),
}

fn override_key(ch: char) -> Option<OverrideKey> {
    Some(match ch.to_ascii_uppercase() {
        'E' | 'P' => OverrideKey::Snap(Some(SnapType::Endpoint)),
        'V' | 'M' => OverrideKey::Snap(Some(SnapType::Midpoint)),
        'C' | '<' => OverrideKey::Snap(Some(SnapType::Center)),
        'D' | 'L' => OverrideKey::Snap(None),
        'A' | '"' => OverrideKey::Toggle(HeldOverride::Osnap),
        'X' | '>' => OverrideKey::Toggle(HeldOverride::Polar),
        'Q' | '}' => OverrideKey::Toggle(HeldOverride::Otrack),
        _ => return None,
    })
}

impl OpenCADStudio {
    /// Snaps accepted by the active command's point steps, oldest first, in
    /// step order.
    pub(crate) fn accepted_snaps(&self) -> &[AcceptedSnap] {
        &self.accepted_snaps
    }

    /// Infer direct sources only when the measurement uses the active space.
    /// Projected model points must retain their acquired source identities;
    /// proximity on the sheet could bind them to unrelated paper geometry.
    pub(crate) fn infer_dimension_sources_guarded(
        &self,
        tab: usize,
        dimension: Handle,
    ) -> Vec<Option<Handle>> {
        if !self.dimension_association_allowed(tab) {
            return Vec::new();
        }
        self.tabs[tab].scene.infer_dimension_sources(dimension)
    }

    /// Whether the dimension can use ordinary active-space associations.
    /// Placement clicks are excluded by the measurement classification.
    pub(crate) fn dimension_association_allowed(&self, tab: usize) -> bool {
        matches!(
            self.dimension_measure_space(tab),
            crate::app::dim_viewport::DimensionMeasureSpace::Direct
        )
    }

    /// Forget the accepted snaps of a finished / abandoned command.
    pub(crate) fn clear_accepted_snaps(&mut self) {
        self.accepted_snaps.clear();
    }

    /// A hot grip is waiting for its placement point. Grip editing is not a
    /// command, so this is the point prompt MTP / M2P serve there.
    pub(crate) fn grip_point_prompt(&self, i: usize) -> bool {
        self.tabs[i].active_cmd.is_none()
            && self.tabs[i].suspended_cmd.is_none()
            && self.tabs[i].active_grip.as_ref().is_some_and(|grip| {
                matches!(
                    grip.mode,
                    crate::scene::pick::grip::GripEditMode::Stretch
                        | crate::scene::pick::grip::GripEditMode::RectangleResize
                )
            })
    }

    /// The active command takes a point (a point prompt, or a distance or
    /// angle prompt a point answers too) and is not picking an object or
    /// reading prose: where MTP and typed snap overrides apply (measured:
    /// `_end` works at TEXT height, CIRCLE radius, ROTATE angle and OFFSET
    /// distance as at a point prompt).
    pub(crate) fn point_modifier_step(&self, i: usize) -> bool {
        self.tabs[i].active_cmd.as_ref().is_some_and(|c| {
            // A name prompt (block, layer, style) is not one: there `END`
            // or `NODE` is the name typed.
            (!c.input_kind().wants_text()
                || c.point_step_accepts_keywords()
                || matches!(
                    c.dyn_field(),
                    crate::command::DynField::Distance | crate::command::DynField::Angle
                ))
                && !c.needs_entity_pick()
                && !c.is_selection_gathering()
        })
    }

    /// A snap override typed at a point prompt (`MID`, `END`, `NON`, ...). An
    /// option the prompt offers wins, so `CEN` where Center is offered picks
    /// Center.
    pub(crate) fn typed_snap_override(
        &self,
        i: usize,
        token: &str,
    ) -> Option<crate::snap::SnapOverrideKind> {
        let kind = crate::snap::parse_snap_override(token)?;
        if !self.point_modifier_step(i) {
            return None;
        }
        let typed = token.trim().trim_start_matches('_').to_uppercase();
        let offered = self.tabs[i]
            .active_cmd
            .as_ref()?
            .options()
            .iter()
            .any(|option| {
                option.keyword.eq_ignore_ascii_case(&typed)
                    || option
                        .label
                        .to_uppercase()
                        .replace(' ', "")
                        .starts_with(&typed)
            });
        (!offered).then_some(kind)
    }

    /// A typed coordinate under a one-shot object snap override (`END` then
    /// `40.5,0`) snaps the way a pick at that point would: the feature of the
    /// override's kind nearest the point on the object under the aperture
    /// there. `None` when no override is waiting; `Some(Err(()))` when none
    /// is found — the point is refused and the prompt asks again, the
    /// override spent.
    pub(crate) fn typed_override_snap(
        &mut self,
        i: usize,
        wcs: glam::DVec3,
    ) -> Option<Result<Option<SnapResult>, ()>> {
        let mode = self.snapper.pending_override()?;
        let Some(mode) = mode else {
            return Some(Ok(None));
        };
        let from = self.last_point;
        if let Some(hit) = self.snap_query(i, wcs, from, None) {
            return Some(Ok(Some(hit)));
        }
        // No feature inside the aperture: the object under it answers with
        // its own nearest feature of that kind, however far along it lies.
        let saved = std::mem::replace(
            &mut self.snapper.enabled,
            std::iter::once(crate::snap::SnapType::Nearest).collect(),
        );
        let under = self.snap_query(i, wcs, from, None);
        self.snapper.enabled = saved;
        let owner = under.and_then(|hit| hit.source).map(|source| source.handle);
        if let Some(owner) = owner {
            let wires: Vec<_> = self.tabs[i]
                .scene
                .hit_test_wires()
                .iter()
                .filter(|wire| {
                    crate::scene::Scene::handle_from_wire_name(&wire.name) == Some(owner)
                })
                .cloned()
                .collect();
            let radius = self.snapper.osnap_radius_px;
            self.snapper.osnap_radius_px = f32::MAX / 4.0;
            let hit = self.snap_query(i, wcs, from, Some(&wires));
            self.snapper.osnap_radius_px = radius;
            if let Some(hit) = hit {
                return Some(Ok(Some(hit)));
            }
        }
        let label = crate::snap::ALL_SNAP_MODES
            .iter()
            .chain(crate::snap::ALL_3D_SNAP_MODES.iter())
            .find(|(m, _, _)| *m == mode)
            .map(|(_, _, l)| *l)
            .unwrap_or("Snap");
        self.command_line
            .push_rejection(crate::tf!("No {label} found for specified point.").as_ref());
        // The second line is the one the prompt prints for any input it
        // cannot use (measured across point, angle, distance and dimension
        // prompts).
        let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
        if let Some(command) = self.tabs[i].active_cmd.as_deref() {
            let line = crate::command::point_rejection_line(command);
            self.command_line.push_rejection(&line);
        }
        self.snapper.clear_override();
        if let Some(prompt) = prompt {
            self.command_line.push_info(&prompt);
        }
        Some(Err(()))
    }

    /// A glyph typed with Shift alone (#3016). At a point prompt with an
    /// empty command line, a temporary-override key applies its override
    /// while held; else a user shortcut bound to `SHIFT+<key>` runs. `None`
    /// lets the glyph type as usual.
    pub(in crate::app) fn shift_glyph(
        &mut self,
        s: &str,
    ) -> Option<iced::Task<crate::app::Message>> {
        if !self.shift_down
            || self.ctrl_down
            || self.alt_down
            || !self.command_line.input.is_empty()
        {
            return None;
        }
        let mut chars = s.chars();
        let (Some(ch), None) = (chars.next(), chars.next()) else {
            return None;
        };
        let at_point = self.tabs[self.active_tab]
            .active_cmd
            .as_ref()
            .is_some_and(|c| {
                !c.needs_entity_pick() && !c.is_selection_gathering() && !c.wants_text_input()
            });
        if at_point {
            if let Some(key) = override_key(ch) {
                self.hold_override(ch, key);
                return Some(iced::Task::none());
            }
        }
        let shortcut = format!("SHIFT+{}", ch.to_uppercase());
        self.shortcut_bindings
            .contains_key(&shortcut)
            .then(|| self.update(crate::app::Message::ShortcutPressed(shortcut)))
    }

    fn hold_override(&mut self, ch: char, key: OverrideKey) {
        let held = self.held_override.map(|(c, _)| c);
        if held.is_some_and(|c| !c.eq_ignore_ascii_case(&ch)) {
            self.release_held_override();
        }
        match key {
            // Re-applied on key repeat: a pick ends a one-shot override, the
            // held one lasts until the key goes up.
            OverrideKey::Snap(Some(t)) => self.snapper.set_override(t),
            OverrideKey::Snap(None) => self.snapper.set_override_none(),
            OverrideKey::Toggle(_) if self.held_override.is_some() => return,
            OverrideKey::Toggle(kind) => self.flip_held(kind),
        }
        let kind = match key {
            OverrideKey::Snap(_) => HeldOverride::Snap,
            OverrideKey::Toggle(kind) => kind,
        };
        self.held_override = Some((ch, kind));
    }

    fn flip_held(&mut self, kind: HeldOverride) {
        match kind {
            HeldOverride::Snap => self.snapper.clear_override(),
            HeldOverride::Osnap => self.snapper.toggle_global(),
            HeldOverride::Polar => self.polar_mode = !self.polar_mode,
            HeldOverride::Otrack => self.snapper.otrack_enabled = !self.snapper.otrack_enabled,
        }
    }

    /// The held override's key (or Shift) went up: undo what it changed.
    pub(in crate::app) fn release_held_override(&mut self) {
        if let Some((_, kind)) = self.held_override.take() {
            self.flip_held(kind);
        }
    }

    /// Keys while the snap override menu is open: a snap's letter (see
    /// [`crate::snap::override_menu_key`]) picks it; any other command-line
    /// key is swallowed so it cannot land in the prompt behind the menu.
    pub(in crate::app) fn intercept_snap_override_key(
        &mut self,
        msg: &crate::app::Message,
    ) -> Option<iced::Task<crate::app::Message>> {
        use crate::app::Message;
        let typed = match msg {
            Message::CommandAppendChar(s) => s.chars().next(),
            // The focused field delivers its whole buffer; take the new char.
            Message::CommandInput(buf) => buf
                .strip_prefix(self.command_line.input.as_str())
                .and_then(|rest| rest.chars().next()),
            _ => None,
        };
        if typed.is_some_and(|ch| ch.eq_ignore_ascii_case(&'F')) {
            return Some(self.update(Message::SnapOverrideFrom));
        }
        if let Some(t) = typed.and_then(crate::snap::override_menu_snap) {
            self.apply_snap_override(crate::snap::SnapOverrideKind::Mode(t));
            return Some(iced::Task::none());
        }
        super::is_modal_blocked_key_msg(msg).then(iced::Task::none)
    }

    /// Apply a transparent one-shot snap override (from typed input or the context menu).
    pub(crate) fn apply_snap_override(&mut self, kind: crate::snap::SnapOverrideKind) {
        match kind {
            crate::snap::SnapOverrideKind::Mode(t) => {
                self.snap_override_popup = None;
                self.snapper.set_override(t);
                let label = crate::snap::ALL_SNAP_MODES
                    .iter()
                    .chain(crate::snap::ALL_3D_SNAP_MODES.iter())
                    .find(|(m, _, _)| *m == t)
                    .map(|(_, _, l)| *l)
                    .unwrap_or("Snap");
                self.command_line
                    .push_info(crate::tf!("Snap override: {label} (next pick only).").as_ref());
            }
            crate::snap::SnapOverrideKind::None => {
                self.snap_override_popup = None;
                self.snapper.set_override_none();
                self.command_line
                    .push_info(crate::t!("Snap override: None (next pick only).").as_ref());
            }
        }
    }

    /// Record the snap a point step just accepted. `snap` is the displayed snap
    /// result (already in the current space's coordinates) or `None` for a free
    /// / typed point; `committed` is the point the command actually received
    /// after ortho / polar / axis-lock / dynamic-input resolution.
    pub(crate) fn record_accepted_snap(
        &mut self,
        tab: usize,
        snap: Option<SnapResult>,
        frame: Option<ViewportFrame>,
        committed: glam::DVec3,
    ) -> bool {
        let mut accepted = match snap {
            // A viewport snap keeps its frame only when the hit really came
            // through it, so a paper-sheet snap never claims a model point.
            Some(hit) => {
                let frame = hit.viewport.and(frame);
                AcceptedSnap::from_snap(&hit, frame).with_paper_point(committed)
            }
            None => AcceptedSnap::free(committed),
        };
        if self.tabs[tab]
            .active_cmd
            .as_ref()
            .is_some_and(|cmd| cmd.measures_through_viewports())
        {
            if self.tabs[tab]
                .active_cmd
                .as_ref()
                .is_some_and(|cmd| cmd.dimension_placement_pending())
            {
                return true;
            }
            if !self.dimension_acquisition_allowed(tab, accepted.viewport) {
                return false;
            }
        }
        self.resolve_source_block_path(tab, &mut accepted);
        self.push_accepted_snap(accepted);
        true
    }

    /// Resolve the instance path once, when a feature is accepted. Failure
    /// drops source identity so a failed descent cannot attach to the insert's
    /// origin. Ambiguous intersections within one batched block remain unlinked.
    fn resolve_source_block_path(&self, tab: usize, accepted: &mut AcceptedSnap) {
        let resolve = |source: &mut crate::scene::viewport_ref::SnapSourceRef| {
            if !source.block_path.is_empty() {
                return true;
            }
            if !matches!(
                self.tabs[tab]
                    .scene
                    .document
                    .get_entity(source.source.handle),
                Some(document::EntityType::Insert(_))
            ) {
                return true;
            }
            if source.snap_type == SnapType::Insertion {
                let Some(document::EntityType::Insert(insert)) = self.tabs[tab]
                    .scene
                    .document
                    .get_entity(source.source.handle)
                else {
                    return false;
                };
                let origin = glam::DVec3::new(
                    insert.insert_point.x,
                    insert.insert_point.y,
                    insert.insert_point.z,
                );
                return origin.distance(accepted.model_point) < 1e-5;
            }
            let Some((entity, path)) = self.tabs[tab].scene.resolve_measurable_feature(
                source.source.handle,
                accepted.model_point,
                accepted.viewport,
                Some(source.snap_type),
            ) else {
                return false;
            };
            let distance = crate::scene::viewport_dimension_pick::feature_pick_distance(
                &entity,
                accepted.model_point,
                Some(source.snap_type),
            );
            if !distance.is_some_and(|distance| distance < 1e-5) {
                return false;
            }
            source.source =
                crate::command::DimensionAssociationSource::inferred(entity.common().handle);
            source.block_path = path;
            true
        };
        let Some(source) = accepted.source.as_mut() else {
            return;
        };
        if !resolve(source) {
            accepted.source = None;
            return;
        }
        if let Some(other) = source.intersection.as_mut() {
            if !resolve(other)
                || (other.source.handle == source.source.handle
                    && other.block_path == source.block_path)
            {
                source.intersection = None;
            }
        }
    }

    /// Retain a bounded history for point and object picks.
    pub(crate) fn push_accepted_snap(&mut self, accepted: AcceptedSnap) {
        if self.accepted_snaps.len() >= MAX_ACCEPTED_SNAPS {
            self.accepted_snaps.remove(0);
        }
        self.accepted_snaps.push(accepted);
    }
    /// Query visible viewport geometry with a canvas-pixel cursor and sheet point.
    /// Returns paper coordinates and canvas pixels, plus the source viewport frame.
    pub(in crate::app) fn paper_viewport_snap(
        &mut self,
        i: usize,
        cursor_canvas: Point,
        canvas: (f32, f32),
        cursor_paper: glam::DVec3,
    ) -> Option<(SnapResult, ViewportFrame)> {
        if !self.snapper.snap_enabled {
            return None;
        }
        {
            let scene = &self.tabs[i].scene;
            if scene.current_layout == "Model" || scene.active_viewport.is_some() {
                return None;
            }
        }
        if canvas.0 < 1.0 || canvas.1 < 1.0 {
            return None;
        }

        for frame in self.tabs[i]
            .scene
            .viewport_frames_at_paper_point(cursor_paper)
        {
            let handle = frame.viewport;
            let Some((cam, rect)) = self.tabs[i].scene.viewport_edit_frame_for(handle, canvas)
            else {
                continue;
            };
            if rect.width < 1.0 || rect.height < 1.0 {
                continue;
            }
            let bounds = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: rect.width,
                height: rect.height,
            };
            let local = Point::new(cursor_canvas.x - rect.x, cursor_canvas.y - rect.y);
            let view_rot = cam.view_proj_rte(bounds);
            let eye = cam.eye();
            // The cursor in MODEL space, through the frame — exactly the point
            // the viewport camera projects back onto `local`.
            let model_cursor = frame.paper_to_model(cursor_paper);

            let wires = self.tabs[i]
                .scene
                .model_wires_for_viewport_arc(handle, bounds.height);
            let candidates = self.tabs[i].scene.interaction_candidates_near(
                wires,
                model_cursor,
                view_rot,
                eye,
                bounds,
                self.snapper.osnap_radius_px,
            );

            // The grid belongs to the sheet. Translate the construction
            // anchor for perpendicular/tangent snaps into this viewport's
            // model coordinates, then restore the shared snapper state.
            let saved_grid = self.snapper.grid_snap_on;
            let saved_from = self.snapper.from_point;
            self.snapper.grid_snap_on = false;
            self.snapper.from_point = saved_from.map(|p| frame.paper_to_model(p));
            self.sync_osnap_z_plane(i);
            let hit = self.snapper.snap(
                model_cursor,
                local,
                &candidates,
                view_rot,
                eye,
                bounds,
                glam::Vec3::ZERO,
                (glam::Vec3::X, glam::Vec3::Y, glam::Vec3::Z),
                None,
            );
            self.snapper.grid_snap_on = saved_grid;
            self.snapper.from_point = saved_from;

            let Some(hit) = hit.filter(|h| h.snap_type != SnapType::Grid) else {
                continue;
            };
            // The snap aperture can reach outside the visible clip boundary.
            let paper = frame.model_to_paper(hit.world);
            if !self.tabs[i]
                .scene
                .viewport_displays_paper_point(handle, paper.truncate())
            {
                continue;
            }
            return Some((project_hit_to_paper(hit, &frame, rect, handle), frame));
        }
        None
    }
}

/// Re-express a snap taken in a viewport's model space as a paper-space result:
/// the point projects onto the sheet through `frame`, and the pane-local pixel
/// positions shift by the viewport's screen origin so they share the canvas
/// frame with the ordinary paper snap.
fn project_hit_to_paper(
    hit: SnapResult,
    frame: &ViewportFrame,
    rect: iced::Rectangle,
    viewport: Handle,
) -> SnapResult {
    let to_canvas = |p: Point| Point::new(p.x + rect.x, p.y + rect.y);
    SnapResult {
        world: frame.model_to_paper(hit.world),
        screen: to_canvas(hit.screen),
        snap_type: hit.snap_type,
        // Tangent geometry is model-space and would be consumed as paper-space
        // by TTR/TTT; drop it rather than hand over a wrong circle.
        tangent_obj: None,
        extension_base: hit.extension_base.map(to_canvas),
        extension_base2: hit.extension_base2.map(to_canvas),
        extension_origin: hit.extension_origin.map(|p| frame.model_to_paper(p)),
        extension_dir: hit.extension_dir.map(|d| {
            let mapped = frame.model_to_paper_dir(d.truncate());
            glam::DVec3::new(mapped.x, mapped.y, d.z)
        }),
        viewport: Some(viewport),
        source: hit.source,
        secondary_source: hit.secondary_source,
        model_point: Some(hit.world),
    }
}
