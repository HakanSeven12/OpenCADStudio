use super::*;

// ExtendCommand
// ══════════════════════════════════════════════════════════════════════════

pub struct ExtendCommand {
    pub(crate) all_entities: Vec<EntityType>,
    pub(crate) entity_index: ModifyEntityIndex,
    pub(crate) geos: Vec<Geo>,
    /// Replacements sent to the host, applied once their handles are known.
    pub(crate) pending: Vec<(Handle, Vec<EntityType>)>,
    pub(crate) mode: TrimMode,
    /// Whether Shift+pick uses quick TRIM's full-erase rule for plain lines.
    pub(crate) quick: bool,
    /// Boundary-edge selection; empty = every object is a boundary.
    pub(crate) edge_set: Vec<Handle>,
    /// Mode to restore if a temporary limit-selection step is cancelled.
    pub(crate) selection_previous_quick: Option<bool>,
    /// Edge option: boundaries extrapolate past their drawn extent.
    pub(crate) implied_edges: bool,
    /// Live Shift state — Shift+click trims instead of extends (#336).
    pub(crate) shift: bool,
    /// Cached live box preview; rebuilding extensions on every raw pointer
    /// event can stall large drawings. Final commit always recalculates.
    pub(crate) box_preview_wires: Vec<WireModel>,
    pub(crate) box_preview_at: Option<iced::time::Instant>,
}

impl ExtendCommand {
    fn choose_mode(&mut self, text: &str) -> Option<CmdResult> {
        let (result, rebuild) = choose_mode(&mut self.mode, &mut self.edge_set, text)?;
        match text.trim().to_uppercase().as_str() {
            "Q" | "QUICK" => {
                self.quick = true;
                self.implied_edges = false;
                self.selection_previous_quick = None;
            }
            "S" | "STANDARD" => self.quick = false,
            _ => {}
        }
        if rebuild {
            self.rebuild_geos();
        }
        Some(result)
    }

    pub fn new(all_entities: Vec<EntityType>) -> Self {
        Self::with_boundary_edges(all_entities, Vec::new())
    }

    /// Objects selected before the command are its boundary edges, as TRIM
    /// takes them as cutting edges.
    pub fn with_boundary_edges(all_entities: Vec<EntityType>, initial_edges: Vec<Handle>) -> Self {
        let all_entities: Vec<EntityType> = all_entities
            .into_iter()
            .map(entity_with_lwpolyline_world_xy_owned)
            .collect();
        let mut cmd = Self {
            all_entities,
            entity_index: ModifyEntityIndex::empty(),
            geos: Vec::new(),
            pending: Vec::new(),
            mode: starting_mode(&initial_edges),
            quick: quick_mode(),
            edge_set: initial_edges,
            selection_previous_quick: None,
            implied_edges: false,
            shift: false,
            box_preview_wires: Vec::new(),
            box_preview_at: None,
        };
        cmd.rebuild_geos();
        cmd
    }

    pub(crate) fn rebuild_geos(&mut self) {
        self.entity_index = ModifyEntityIndex::build(&self.all_entities);
        self.geos = if self.edge_set.is_empty() {
            build_geos(&self.all_entities)
        } else {
            // A set, not the Vec: every trim rebuilds this, and a selection
            // of thousands of edges made it edges × objects per click (#1867).
            let edges: std::collections::HashSet<Handle> = self.edge_set.iter().copied().collect();
            let picked: Vec<EntityType> = self
                .all_entities
                .iter()
                .filter(|e| edges.contains(&e.common().handle))
                .cloned()
                .collect();
            build_geos(&picked)
        };
        if self.implied_edges {
            imply_edge_geos(&mut self.geos);
        }
    }

    pub(crate) fn fence_run(
        &mut self,
        fence: &[[f64; 2]],
        window: Option<CrossingWindow>,
    ) -> CmdResult {
        // EXTEND's fence extends; Shift held at Enter swaps it to trim.
        let replacements = fence_pass(
            &self.all_entities,
            &self.entity_index,
            &self.geos,
            fence,
            window,
            !self.shift,
        );
        if replacements.is_empty() {
            return CmdResult::NeedPoint;
        }
        self.pending = replacements.clone();
        CmdResult::ReplaceManyContinue(replacements)
    }
}

impl CadCommand for ExtendCommand {
    fn name(&self) -> &'static str {
        "EXTEND"
    }

    fn prompt(&self) -> String {
        let edge = if self.implied_edges {
            t!(" [Edge: Extend]")
        } else {
            std::borrow::Cow::Borrowed("")
        };
        match &self.mode {
            TrimMode::Pick => {
                crate::tf!("EXTEND{edge}  Click near an end or draw a crossing box to extend multiple objects (Shift trims):")
                    .into_owned()
            }
            TrimMode::SelectEdges => crate::tf!(
                "EXTEND  Select boundary edges [{} picked, Enter = done]:",
                self.edge_set.len()
            )
            .into_owned(),
            TrimMode::Fence(pick) => crate::tf!(
                "EXTEND{edge}  Fence: pick points [{} placed, Enter = extend crossed]:",
                pick.len()
            )
            .into_owned(),
            TrimMode::QuickFence(_) => {
                crate::tf!("EXTEND{edge}  Specify next fence point:").into_owned()
            }
            TrimMode::CrossFirst => {
                crate::tf!("EXTEND{edge}  Crossing: first corner:").into_owned()
            }
            TrimMode::CrossSecond(_) => {
                crate::tf!("EXTEND{edge}  Crossing: opposite corner:").into_owned()
            }
            TrimMode::Erase => t!("EXTEND  [Enter = done]:").into_owned(),
            TrimMode::ChooseMode => mode_prompt("EXTEND"),
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        match self.mode {
            TrimMode::Pick => {
                let mut options = vec![
                    CmdOption::new("Boundary edges", "B"),
                    CmdOption::new("Fence", "F"),
                    CmdOption::new("Crossing", "C"),
                ];
                if !self.quick {
                    options.push(CmdOption::new(
                        if self.implied_edges {
                            "Edge: Extend"
                        } else {
                            "Edge: No extend"
                        },
                        "E",
                    ));
                }
                options.extend([CmdOption::new("mOde", "O"), CmdOption::enter("Done")]);
                options
            }
            TrimMode::ChooseMode => mode_options(),
            _ => vec![CmdOption::enter("Done")],
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if let Some(result) = self.choose_mode(text) {
            return Some(result);
        }
        let pick = matches!(self.mode, TrimMode::Pick);
        match text.trim().to_uppercase().as_str() {
            // Consumed in every step so they are never read as hex handles.
            "B" | "BOUNDARY" | "T" | "CUTTING" | "F" | "FENCE" | "C" | "CROSSING" | "E"
            | "EDGE"
                if !pick =>
            {
                Some(CmdResult::NeedPoint)
            }
            "E" | "EDGE" if self.quick => Some(CmdResult::ReportError(
                "Invalid option keyword.".to_string(),
            )),
            "B" | "BOUNDARY" | "T" | "CUTTING" => {
                self.selection_previous_quick = Some(self.quick);
                self.quick = false;
                self.edge_set.clear();
                self.mode = TrimMode::SelectEdges;
                Some(CmdResult::NeedPoint)
            }
            "F" | "FENCE" => {
                self.mode = TrimMode::Fence(FencePick::fence());
                Some(CmdResult::NeedPoint)
            }
            "C" | "CROSSING" => {
                self.mode = TrimMode::CrossFirst;
                Some(CmdResult::NeedPoint)
            }
            "E" | "EDGE" => {
                self.implied_edges = !self.implied_edges;
                self.rebuild_geos();
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn set_shift(&mut self, shift: bool) {
        self.shift = shift;
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.mode, TrimMode::Pick | TrimMode::SelectEdges)
    }

    fn wants_raw_point(&self) -> bool {
        self.mode.picks_selection_path()
    }

    fn on_entity_pick(&mut self, handle: Handle, pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        match self.mode {
            TrimMode::SelectEdges => {
                if let Some(pos) = self.edge_set.iter().position(|h| *h == handle) {
                    self.edge_set.remove(pos);
                } else {
                    self.edge_set.push(handle);
                }
                CmdResult::NeedPoint
            }
            _ => {
                let (px, py) = (pt.x, pt.y);
                let new_entities = if self.shift {
                    // Shift+click swaps to Trim for this pick (#336).
                    if self.quick {
                        quick_trim_at(&self.all_entities, &self.geos, handle, px, py)
                    } else {
                        pick_trim_at(&self.all_entities, &self.geos, handle, px, py)
                    }
                } else {
                    pick_extend_at(&self.all_entities, &self.geos, handle, px, py).map(|e| vec![e])
                };
                if let Some(new_entities) = new_entities {
                    // Same snapshot bookkeeping as TRIM.
                    self.pending = vec![(handle, new_entities.clone())];
                    CmdResult::ReplaceEntity(handle, new_entities)
                } else {
                    CmdResult::NeedPoint
                }
            }
        }
    }

    fn on_entity_replaced(&mut self, old: Handle, new_handles: &[document::Handle]) {
        self.on_entities_replaced(&[(old, new_handles.to_vec())]);
    }

    fn on_entities_replaced(&mut self, replacements: &[(Handle, Vec<document::Handle>)]) {
        let mut rebuild = false;
        for (old, new_handles) in replacements {
            let snapshot = Snapshot {
                all: &mut self.all_entities,
                index: &mut self.entity_index,
                geos: &mut self.geos,
                edge_set: &mut self.edge_set,
                implied_edges: self.implied_edges,
            };
            rebuild |= apply_pending_replacement(snapshot, &mut self.pending, *old, new_handles);
        }
        if rebuild {
            self.rebuild_geos();
        }
    }

    fn drag_selection_preview_enabled(&self) -> bool {
        matches!(self.mode, TrimMode::Pick) && !self.shift
    }

    fn on_drag_selection_preview(
        &mut self,
        fence: &[[f64; 2]],
        window: Option<([f64; 2], [f64; 2])>,
        crossing: bool,
        candidate_handles: &[Handle],
    ) -> Option<Vec<WireModel>> {
        if !matches!(self.mode, TrimMode::Pick) || self.shift || fence.len() < 2 {
            return None;
        }
        // A 20 Hz geometric update keeps the live box responsive without
        // intersecting every visible edge on each raw mouse move.
        if self
            .box_preview_at
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(50))
        {
            return Some(self.box_preview_wires.clone());
        }
        let window = window.map(|(min, max)| CrossingWindow { min, max });
        // The scene's interaction index has already narrowed this list to
        // the box. A bounded preview is important with survey-size DWGs;
        // the final commit has no such limit and processes all targets.
        let entities: Vec<EntityType> = candidate_handles
            .iter()
            .take(1_000)
            .filter_map(|&handle| self.entity_index.get(&self.all_entities, handle).cloned())
            .collect();
        let replacements = extend_box_pass(
            &entities,
            &self.entity_index,
            &self.geos,
            &self.edge_set,
            fence,
            window,
            crossing,
        );
        let mut preview = Vec::new();
        for (handle, extended) in replacements {
            let Some(original) = self.entity_index.get(&self.all_entities, handle) else {
                continue;
            };
            for entity in extended {
                preview.extend(extend_hover_wires(
                    original,
                    &entity,
                    &self.all_entities,
                    handle,
                    self.implied_edges,
                ));
            }
        }
        self.box_preview_wires = preview;
        self.box_preview_at = Some(iced::time::Instant::now());
        Some(self.box_preview_wires.clone())
    }

    fn on_drag_selection(
        &mut self,
        fence: &[[f64; 2]],
        window: Option<([f64; 2], [f64; 2])>,
        crossing: bool,
    ) -> Option<CmdResult> {
        // Never reuse throttled preview geometry for the final edit.
        self.box_preview_wires.clear();
        self.box_preview_at = None;
        if fence.len() < 2 {
            return None;
        }
        let window = window.map(|(min, max)| CrossingWindow { min, max });
        match self.mode {
            TrimMode::SelectEdges => {
                let handles = drag_selected_handles(
                    &self.all_entities,
                    &self.entity_index,
                    fence,
                    window,
                    crossing,
                );
                update_trim_edge_set(&mut self.edge_set, handles, self.shift);
                Some(CmdResult::NeedPoint)
            }
            TrimMode::Pick => {
                // Shift retains EXTEND's usual inverse TRIM gesture.
                if self.shift {
                    return Some(self.fence_run(fence, window));
                }
                let replacements = extend_box_pass(
                    &self.all_entities,
                    &self.entity_index,
                    &self.geos,
                    &self.edge_set,
                    fence,
                    window,
                    crossing,
                );
                if replacements.is_empty() {
                    return Some(CmdResult::NeedPoint);
                }
                self.pending = replacements.clone();
                Some(CmdResult::ReplaceManyContinue(replacements))
            }
            _ => None,
        }
    }

    fn on_empty_pick(&mut self, pt: DVec3) -> Option<CmdResult> {
        quick_fence_start(&mut self.mode, self.quick, pt)
    }

    fn accepts_drag_selection(&self) -> bool {
        matches!(self.mode, TrimMode::Pick | TrimMode::SelectEdges)
    }

    fn persistent_preview_wires(&self) -> Vec<WireModel> {
        if !matches!(self.mode, TrimMode::SelectEdges) {
            return Vec::new();
        }
        self.edge_set
            .iter()
            .filter_map(|handle| self.entity_index.get(&self.all_entities, *handle))
            .flat_map(|entity| trim_edge_glow(entity, false))
            .collect()
    }

    fn on_hover_entity(&mut self, handle: Handle, pt: DVec3) -> Vec<WireModel> {
        match self.mode {
            TrimMode::SelectEdges => {
                let mut out: Vec<WireModel> = self
                    .edge_set
                    .iter()
                    .filter_map(|h| self.entity_index.get(&self.all_entities, *h))
                    .flat_map(|entity| trim_edge_glow(entity, false))
                    .collect();
                if !handle.is_null() && !self.edge_set.contains(&handle) {
                    if let Some(e) = self.entity_index.get(&self.all_entities, handle) {
                        out.extend(trim_edge_glow(e, true));
                    }
                }
                return out;
            }
            TrimMode::Pick => {}
            _ => return vec![],
        }
        if self.shift {
            // Shift held: preview the trim result instead.
            let trim = if self.quick {
                quick_trim_at(&self.all_entities, &self.geos, handle, pt.x, pt.y)
            } else {
                pick_trim_at(&self.all_entities, &self.geos, handle, pt.x, pt.y)
            };
            if let Some(pieces) = trim {
                if pieces.is_empty() {
                    if let Some(entity) = self
                        .all_entities
                        .iter()
                        .find(|e| e.common().handle == handle)
                    {
                        return vec![WireModel::solid(
                            "trim_delete".into(),
                            entity_pts(entity),
                            TRIM_REMOVE_GRAY,
                            false,
                        )];
                    }
                    return vec![];
                }
                let mut out = Vec::new();
                for (i, e) in pieces.iter().enumerate() {
                    out.push(WireModel::solid(
                        format!("trim_keep_{i}"),
                        entity_pts(e),
                        WireModel::CYAN,
                        false,
                    ));
                }
                return out;
            }
            return vec![];
        }
        if handle.is_null() {
            return vec![];
        }

        let entity = self.entity_index.get(&self.all_entities, handle);
        match entity {
            Some(EntityType::Line(l)) => {
                let ax = l.start.x;
                let ay = l.start.y;
                let bx = l.end.x;
                let by = l.end.y;
                let dx = bx - ax;
                let dy = by - ay;
                let len2 = dx * dx + dy * dy;
                let t_click = if len2 > 1e-12 {
                    ((pt.x - ax) * dx + (pt.y - ay) * dy) / len2
                } else {
                    0.5
                };
                if let Some(ext) = extend_line(l, t_click, &self.geos) {
                    return extend_hover_wires(
                        &EntityType::Line(l.clone()),
                        &ext,
                        &self.all_entities,
                        handle,
                        self.implied_edges,
                    );
                }
            }
            Some(EntityType::Arc(a)) => {
                let ang = (pt.y - a.center.y).atan2(pt.x - a.center.x);
                let t_click = arc_t(ang, a.start_angle, a.end_angle);
                if let Some(ext) = extend_arc(a, t_click, &self.geos) {
                    return extend_hover_wires(
                        &EntityType::Arc(a.clone()),
                        &ext,
                        &self.all_entities,
                        handle,
                        self.implied_edges,
                    );
                }
            }
            Some(EntityType::Ellipse(e)) => {
                let a = (e.major_axis.x.powi(2) + e.major_axis.y.powi(2)).sqrt();
                if a >= 1e-9 {
                    let b = a * e.minor_axis_ratio;
                    if b >= 1e-9 {
                        let (nx, ny) = (e.major_axis.x / a, e.major_axis.y / a);
                        let t0 = e.start_parameter;
                        let mut t1 = e.end_parameter;
                        if t1 <= t0 {
                            t1 += TAU;
                        }
                        let rx = pt.x - e.center.x;
                        let ry = pt.y - e.center.y;
                        let xl = rx * nx + ry * ny;
                        let yl = -rx * ny + ry * nx;
                        let t_ell = ellipse_closest_parameter(a, b, xl, yl);
                        let t_click = arc_t(t_ell, t0, t1);
                        if let Some(ext) = extend_ellipse(e, t_click, &self.geos) {
                            return extend_hover_wires(
                                &EntityType::Ellipse(e.clone()),
                                &ext,
                                &self.all_entities,
                                handle,
                                self.implied_edges,
                            );
                        }
                    }
                }
            }
            Some(EntityType::LwPolyline(p)) => {
                if let Some(ext) = extend_lwpoly(p, pt.x, pt.y, &self.geos) {
                    return extend_hover_wires(
                        &EntityType::LwPolyline(p.clone()),
                        &ext,
                        &self.all_entities,
                        handle,
                        self.implied_edges,
                    );
                }
            }
            Some(EntityType::Spline(s)) => {
                let t_click = spline_nearest_t(s, pt.x, pt.y)
                    .and_then(|t_actual| {
                        let (t0, t1) = spline_range(s)?;
                        Some(t_to_rel(t_actual, t0, t1))
                    })
                    .unwrap_or(0.5);
                if let Some(ext) = extend_spline(s, t_click, &self.geos) {
                    return extend_hover_wires(
                        &EntityType::Spline(s.clone()),
                        &ext,
                        &self.all_entities,
                        handle,
                        self.implied_edges,
                    );
                }
            }
            _ => {}
        }
        vec![]
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match &mut self.mode {
            TrimMode::Fence(pick) => {
                pick.push([pt.x, pt.y]);
                CmdResult::NeedPoint
            }
            TrimMode::QuickFence(pick) => {
                pick.push([pt.x, pt.y]);
                let path = pick.path();
                self.mode = TrimMode::Pick;
                self.fence_run(&path, None)
            }
            TrimMode::CrossFirst => {
                self.mode = TrimMode::CrossSecond([pt.x, pt.y]);
                CmdResult::NeedPoint
            }
            TrimMode::CrossSecond(p1) => {
                let p1 = *p1;
                let p2 = [pt.x, pt.y];
                self.mode = TrimMode::Pick;
                let rect = [p1, [p1[0], p2[1]], p2, [p2[0], p1[1]], p1];
                let window = CrossingWindow {
                    min: [p1[0].min(p2[0]), p1[1].min(p2[1])],
                    max: [p1[0].max(p2[0]), p1[1].max(p2[1])],
                };
                self.fence_run(&rect, Some(window))
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        match &self.mode {
            TrimMode::Fence(pick) | TrimMode::QuickFence(pick) if !pick.is_empty() => {
                let mut out = vec![pick.preview([pt.x, pt.y], "extend_fence")];
                let mut fpts = pick.path();
                fpts.push([pt.x, pt.y]);
                out.extend(fence_result_preview(
                    &self.all_entities,
                    &self.entity_index,
                    &self.geos,
                    &fpts,
                    None,
                    !self.shift,
                    self.implied_edges,
                ));
                out
            }
            TrimMode::CrossSecond(p1) => {
                let p1 = *p1;
                let p2 = [pt.x, pt.y];
                let mut out = vec![crossing_box_preview(p1, p2, "extend_cross")];
                let rect = [p1, [p1[0], p2[1]], p2, [p2[0], p1[1]], p1];
                let window = CrossingWindow {
                    min: [p1[0].min(p2[0]), p1[1].min(p2[1])],
                    max: [p1[0].max(p2[0]), p1[1].max(p2[1])],
                };
                out.extend(fence_result_preview(
                    &self.all_entities,
                    &self.entity_index,
                    &self.geos,
                    &rect,
                    Some(window),
                    !self.shift,
                    self.implied_edges,
                ));
                out
            }
            _ => Vec::new(),
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match std::mem::replace(&mut self.mode, TrimMode::Pick) {
            TrimMode::SelectEdges => {
                self.selection_previous_quick = None;
                self.rebuild_geos();
                CmdResult::NeedPoint
            }
            TrimMode::Fence(pick) if pick.is_usable() => self.fence_run(&pick.path(), None),
            TrimMode::Fence(_)
            | TrimMode::QuickFence(_)
            | TrimMode::CrossFirst
            | TrimMode::CrossSecond(_)
            | TrimMode::Erase
            | TrimMode::ChooseMode => CmdResult::NeedPoint,
            TrimMode::Pick => CmdResult::Cancel,
        }
    }
    fn on_escape(&mut self) -> CmdResult {
        match std::mem::replace(&mut self.mode, TrimMode::Pick) {
            TrimMode::Pick => CmdResult::Cancel,
            TrimMode::SelectEdges => {
                self.edge_set.clear();
                if let Some(previous_quick) = self.selection_previous_quick.take() {
                    self.quick = previous_quick;
                }
                self.rebuild_geos();
                CmdResult::NeedPoint
            }
            _ => {
                self.selection_previous_quick = None;
                CmdResult::NeedPoint
            }
        }
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["EXTEND"] }); // ExtendCommand
inventory::submit!(crate::command::CommandRegistration { names: &["TRIM"] }); // TrimCommand
