use super::*;

pub struct TrimCommand {
    pub(crate) all_entities: Vec<EntityType>,
    /// Exploded block contents used only as virtual cutting geometry.
    pub(crate) block_boundaries: Vec<(Handle, Vec<EntityType>)>,
    /// Look up virtual INSERT boundaries in O(1) during nearby cutter scans,
    /// rather than searching all block contents for each geometry.
    pub(crate) block_boundary_owners: rustc_hash::FxHashSet<Handle>,
    pub(crate) entity_index: ModifyEntityIndex,
    pub(crate) geos: Vec<Geo>,
    /// Lazily built for cursor-local cutter scans. Reused across hover
    /// targets; cleared whenever a committed edit changes the boundaries.
    pub(crate) geo_spatial: std::cell::RefCell<Option<TrimAabbIndex>>,
    /// Bounded cache of cursor-independent cutting points for hover previews.
    pub(crate) hover_cut_cache: rustc_hash::FxHashMap<Handle, Vec<f64>>,
    pub(crate) hover_cut_order: std::collections::VecDeque<Handle>,
    /// Replacements sent to the host, applied once their handles are known.
    pub(crate) pending: Vec<(Handle, Vec<EntityType>)>,
    pub(crate) mode: TrimMode,
    /// Whether a no-intersection line pick is a quick full erase.
    pub(crate) quick: bool,
    /// Cutting-edge selection; empty = every object cuts (quick mode).
    pub(crate) edge_set: Vec<Handle>,
    /// Mode to restore if a temporary limit-selection step is cancelled.
    pub(crate) selection_previous_quick: Option<bool>,
    /// Edge option: boundaries extrapolate past their drawn extent.
    pub(crate) implied_edges: bool,
    /// Live Shift state — Shift+click extends instead of trims (#336).
    pub(crate) shift: bool,
}

impl TrimCommand {
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
        Self::with_cutting_edges(all_entities, Vec::new())
    }

    pub fn with_cutting_edges(all_entities: Vec<EntityType>, initial_edges: Vec<Handle>) -> Self {
        Self::with_block_boundaries(all_entities, Vec::new(), initial_edges)
    }

    pub fn with_block_boundaries(
        all_entities: Vec<EntityType>,
        block_boundaries: Vec<(Handle, Vec<EntityType>)>,
        initial_edges: Vec<Handle>,
    ) -> Self {
        let all_entities: Vec<EntityType> = all_entities
            .into_iter()
            .map(entity_with_lwpolyline_world_xy_owned)
            .collect();
        let block_boundaries: Vec<(Handle, Vec<EntityType>)> = block_boundaries
            .into_iter()
            .map(|(handle, entities)| {
                (
                    handle,
                    entities
                        .into_iter()
                        .map(entity_with_lwpolyline_world_xy_owned)
                        .collect(),
                )
            })
            .collect();

        let block_boundary_owners = block_boundaries.iter().map(|(owner, _)| *owner).collect();
        let mut cmd = Self {
            all_entities,
            block_boundaries,
            block_boundary_owners,
            entity_index: ModifyEntityIndex::empty(),
            geos: Vec::new(),
            geo_spatial: std::cell::RefCell::new(None),
            hover_cut_cache: rustc_hash::FxHashMap::default(),
            hover_cut_order: std::collections::VecDeque::new(),
            pending: Vec::new(),
            mode: starting_mode(&initial_edges),
            quick: quick_mode(),
            edge_set: initial_edges,
            selection_previous_quick: None,
            implied_edges: false,
            shift: false,
        };

        cmd.rebuild_geos();

        cmd
    }

    /// Boundary geometry from the edge selection (or everything when none),
    /// with the Edge option's implied extrapolation applied on top.
    pub(crate) fn rebuild_geos(&mut self) {
        self.hover_cut_cache.clear();
        self.hover_cut_order.clear();
        self.geo_spatial.get_mut().take();
        self.entity_index = ModifyEntityIndex::build(&self.all_entities);
        let mut geos = if self.edge_set.is_empty() {
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
        let selected_edges: Option<std::collections::HashSet<Handle>> =
            (!self.edge_set.is_empty()).then(|| self.edge_set.iter().copied().collect());
        for (insert_handle, entities) in &self.block_boundaries {
            if selected_edges
                .as_ref()
                .is_none_or(|handles| handles.contains(insert_handle))
            {
                geos.extend(build_geos_with_handle(entities, Some(*insert_handle)));
            }
        }
        self.geos = geos;
        if self.implied_edges {
            imply_edge_geos(&mut self.geos);
        }
    }

    /// Exact analytic boundaries whose boxes overlap the picked entity. The
    /// host already narrowed the cursor pick; this second broad phase keeps
    /// TRIM preview/intersection work local on dense drawings.
    pub(crate) fn nearby_geos(&self, handle: Handle) -> Vec<Geo> {
        if self.implied_edges {
            return self.geos.clone();
        }
        let Some(handles) = self.entity_index.nearby_handles(&self.all_entities, handle) else {
            return self.geos.clone();
        };
        let Some(target_entity) = self.entity_index.get(&self.all_entities, handle) else {
            return self.geos.clone();
        };
        let Some(target_bounds) = modify_aabb(target_entity) else {
            return self.geos.clone();
        };
        if !target_bounds.iter().all(|value| value.is_finite()) {
            return self.geos.clone();
        }
        // Include near-contact tolerance as well as true bounding-box
        // overlap. Rays and splines have no conservatively bounded index
        // entry, so they are always included in candidate indices.
        let slack = CUT_TOLERANCE * 8.0;
        let target_bounds = [
            target_bounds[0] - slack,
            target_bounds[1] - slack,
            target_bounds[2] + slack,
            target_bounds[3] + slack,
        ];
        let candidate_indices = {
            let mut cached = self.geo_spatial.borrow_mut();
            cached
                .get_or_insert_with(|| TrimAabbIndex::new(self.geos.iter().map(trim_geo_aabb)))
                .overlapping(target_bounds)
        };
        candidate_indices
            .into_iter()
            .filter_map(|index| self.geos.get(index))
            .filter(|geo| {
                handles.contains(&geo.handle())
                    || (self.block_boundary_owners.contains(&geo.handle())
                        && geo_overlaps_aabb(geo, target_bounds))
            })
            .cloned()
            .collect()
    }

    /// The mouse chooses a span, but the analytic cutting points remain
    /// constant until an edit changes the geometry. Only cache 32 handles.
    pub(crate) fn cached_hover_cut_params(&mut self, handle: Handle) -> Option<Vec<f64>> {
        let entity = self.entity_index.get(&self.all_entities, handle)?;
        if !matches!(
            entity,
            EntityType::Line(_)
                | EntityType::Arc(_)
                | EntityType::Circle(_)
                | EntityType::Ray(_)
                | EntityType::XLine(_)
                | EntityType::Ellipse(_)
                | EntityType::Spline(_)
                | EntityType::LwPolyline(_)
        ) {
            return None;
        }
        if let Some(cuts) = self.hover_cut_cache.get(&handle) {
            return Some(cuts.clone());
        }
        let solve_started = crate::perf::enabled().then(iced::time::Instant::now);
        let nearby = self.nearby_geos(handle);
        let nearby_ms = crate::perf::elapsed_ms(solve_started);
        let entity = self.entity_index.get(&self.all_entities, handle)?;
        let cuts = match entity {
            EntityType::Line(l) => {
                line_seg_ts(l.start.x, l.start.y, l.end.x, l.end.y, handle, &nearby)
            }
            EntityType::Arc(a) => arc_seg_ts(
                a.center.x,
                a.center.y,
                a.radius,
                a.start_angle,
                a.end_angle,
                handle,
                &nearby,
            ),
            EntityType::Circle(c) => {
                arc_seg_ts(c.center.x, c.center.y, c.radius, 0.0, TAU, handle, &nearby)
            }
            EntityType::Ray(r) => cut_params(&ray_curve(r), handle, &nearby),
            EntityType::XLine(x) => cut_params(&xline_curve(x), handle, &nearby),
            EntityType::Ellipse(e) => {
                let a = e.major_axis.x.hypot(e.major_axis.y);
                let b = a * e.minor_axis_ratio;
                if a < 1e-9 || b < 1e-9 {
                    Vec::new()
                } else {
                    let (nx, ny) = (e.major_axis.x / a, e.major_axis.y / a);
                    let t0 = e.start_parameter;
                    let mut t1 = e.end_parameter;
                    if t1 <= t0 {
                        t1 += TAU;
                    }
                    ellipse_seg_ts(
                        e.center.x, e.center.y, a, b, nx, ny, t0, t1, handle, &nearby,
                    )
                }
            }
            EntityType::Spline(s) => spline_seg_ts(s, handle, &nearby),
            EntityType::LwPolyline(p) => lwpoly_cut_params(p, &nearby),
            _ => return None,
        };
        let solve_ms = crate::perf::elapsed_ms(solve_started);
        if solve_ms >= 8.0 {
            crate::perf_record!(
                "[perf] trim-hover-cuts {solve_ms:.2}ms search={nearby_ms:.2}ms intersection={:.2}ms boundaries={} cuts={}",
                solve_ms - nearby_ms,
                nearby.len(),
                cuts.len(),
            );
        }
        const CAP: usize = 32;
        if self.hover_cut_order.len() >= CAP {
            if let Some(oldest) = self.hover_cut_order.pop_front() {
                self.hover_cut_cache.remove(&oldest);
            }
        }
        self.hover_cut_order.push_back(handle);
        self.hover_cut_cache.insert(handle, cuts.clone());
        Some(cuts)
    }

    fn hatch_trim_at(&self, handle: Handle, point: DVec3) -> Option<hatch_region::HatchTrim> {
        let EntityType::Hatch(hatch) = self.entity_index.get(&self.all_entities, handle)? else {
            return None;
        };
        let nearby = (!self.implied_edges)
            .then(|| self.entity_index.nearby_handles(&self.all_entities, handle))
            .flatten();
        let edges: std::collections::HashSet<_> = self.edge_set.iter().copied().collect();
        let cutters = self.all_entities.iter().filter(|entity| {
            let handle = entity.common().handle;
            is_trim_boundary_entity(entity)
                && (edges.is_empty() || edges.contains(&handle))
                && nearby
                    .as_ref()
                    .is_none_or(|handles| handles.contains(&handle))
        });
        hatch_region::trim(hatch, cutters, point, self.implied_edges)
    }

    pub(crate) fn fence_run(
        &mut self,
        fence: &[[f64; 2]],
        window: Option<CrossingWindow>,
    ) -> CmdResult {
        let replacements = fence_pass_with(
            &self.all_entities,
            &self.entity_index,
            &self.geos,
            fence,
            window,
            self.shift,
            !self.implied_edges,
        );
        if replacements.is_empty() {
            return CmdResult::NeedPoint;
        }
        self.pending = replacements.clone();
        CmdResult::ReplaceManyContinue(replacements)
    }
}

impl CadCommand for TrimCommand {
    fn name(&self) -> &'static str {
        "TRIM"
    }

    fn prompt(&self) -> String {
        let edge = if self.implied_edges {
            t!(" [Edge: Extend]")
        } else {
            std::borrow::Cow::Borrowed("")
        };
        match &self.mode {
            TrimMode::Pick => {
                crate::tf!("TRIM{edge}  Click segment to remove (Shift+click extends):")
                    .into_owned()
            }
            TrimMode::SelectEdges => crate::tf!(
                "TRIM  Select cutting edges [{} picked, Enter = done]:",
                self.edge_set.len()
            )
            .into_owned(),
            TrimMode::Fence(pick) => crate::tf!(
                "TRIM{edge}  Fence: pick points [{} placed, Enter = trim crossed]:",
                pick.len()
            )
            .into_owned(),
            TrimMode::QuickFence(_) => {
                crate::tf!("TRIM{edge}  Specify next fence point:").into_owned()
            }
            TrimMode::CrossFirst => crate::tf!("TRIM{edge}  Crossing: first corner:").into_owned(),
            TrimMode::CrossSecond(_) => {
                crate::tf!("TRIM{edge}  Crossing: opposite corner:").into_owned()
            }
            TrimMode::Erase => {
                t!("TRIM  Erase: click objects to delete [Enter = done]:").into_owned()
            }
            TrimMode::ChooseMode => mode_prompt("TRIM"),
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        match self.mode {
            TrimMode::Pick => {
                let mut options = vec![
                    CmdOption::new("Cutting edges", "T"),
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
                options.extend([
                    CmdOption::new("Erase", "R"),
                    CmdOption::new("mOde", "O"),
                    CmdOption::enter("Done"),
                ]);
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
        // Consumed inputs return Some(NeedPoint) — None would be offered to
        // the command a second time by the driver.
        if let Some(result) = self.choose_mode(text) {
            return Some(result);
        }
        let pick = matches!(self.mode, TrimMode::Pick);
        match text.trim().to_uppercase().as_str() {
            // Options apply at the main prompt only, but stay consumed in
            // every step so "E"/"F"/"C" are never read as hex handles.
            "T" | "CUTTING" | "B" | "BOUNDARY" | "F" | "FENCE" | "C" | "CROSSING" | "E"
            | "EDGE" | "R" | "ERASE"
                if !pick =>
            {
                Some(CmdResult::NeedPoint)
            }
            "E" | "EDGE" if self.quick => Some(CmdResult::ReportError(
                "Invalid option keyword.".to_string(),
            )),
            "T" | "CUTTING" | "B" | "BOUNDARY" => {
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
            "R" | "ERASE" => {
                self.mode = TrimMode::Erase;
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn set_shift(&mut self, shift: bool) {
        self.shift = shift;
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(
            self.mode,
            TrimMode::Pick | TrimMode::SelectEdges | TrimMode::Erase
        )
    }

    fn wants_raw_point(&self) -> bool {
        self.mode.picks_selection_path()
    }

    fn entity_pick_includes_fills(&self) -> bool {
        matches!(self.mode, TrimMode::Pick)
    }

    fn on_entity_pick(&mut self, handle: Handle, pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        match self.mode {
            TrimMode::SelectEdges => {
                // Toggle membership; the boundary rebuilds on Enter.
                if let Some(pos) = self.edge_set.iter().position(|h| *h == handle) {
                    self.edge_set.remove(pos);
                } else {
                    self.edge_set.push(handle);
                }
                CmdResult::NeedPoint
            }
            TrimMode::Erase => {
                // Erase option: delete without needing an intersection.
                self.pending = vec![(handle, Vec::new())];
                CmdResult::ReplaceEntity(handle, vec![])
            }
            _ => {
                // A detached, complete circular HATCH is a standalone fill
                // fragment, not a hole in another hatch. TRIM removes it on
                // a click as requested; no cutting line is necessary.
                if !self.shift
                    && self
                        .entity_index
                        .get(&self.all_entities, handle)
                        .is_some_and(|entity| {
                            matches!(
                                entity,
                                EntityType::Hatch(hatch) if hatch_is_standalone_circle(hatch)
                            )
                        })
                {
                    self.pending = vec![(handle, Vec::new())];
                    return CmdResult::ReplaceEntity(handle, Vec::new());
                }
                let (px, py) = (pt.x, pt.y);
                let entity = self.entity_index.get(&self.all_entities, handle);
                // Block references can provide cutting edges, but are not
                // editable TRIM targets. Avoid scanning/cloning all cutters
                // for INSERTs and other unsupported entity types.
                if !matches!(
                    entity,
                    Some(
                        EntityType::Line(_)
                            | EntityType::Arc(_)
                            | EntityType::Circle(_)
                            | EntityType::Ray(_)
                            | EntityType::XLine(_)
                            | EntityType::Ellipse(_)
                            | EntityType::Spline(_)
                            | EntityType::LwPolyline(_)
                            | EntityType::MLine(_)
                            | EntityType::Hatch(_)
                    )
                ) {
                    return CmdResult::NeedPoint;
                }
                let new_entities = if self.shift {
                    // Shift+click swaps to Extend for this pick (#336).
                    pick_extend_at(&self.all_entities, &self.geos, handle, px, py).map(|e| vec![e])
                } else if matches!(entity, Some(EntityType::Hatch(_))) {
                    // Treat a HATCH as a filled arrangement of cells. This extends
                    // the legacy half-plane solver to multi-edge partitions and
                    // curved / tilted boundaries while retaining exact paths.
                    self.hatch_trim_at(handle, pt)
                        .map(|trim| vec![EntityType::Hatch(Box::new(trim.kept))])
                } else if let Some(EntityType::LwPolyline(poly)) = entity {
                    // A hovered polyline has already calculated its cutting
                    // points. A click chooses a span; no need to solve every
                    // intersection a second time.
                    if let Some(cuts) = self.hover_cut_cache.get(&handle) {
                        trim_lwpolyline_with_cuts(poly, px, py, cuts)
                    } else {
                        let geos = self.nearby_geos(handle);
                        trim_lwpolyline(poly, px, py, &geos)
                    }
                } else {
                    let geos = self.nearby_geos(handle);
                    if self.quick {
                        quick_trim_at(&self.all_entities, &geos, handle, px, py)
                    } else {
                        pick_trim_at(&self.all_entities, &geos, handle, px, py)
                    }
                };
                if let Some(new_entities) = new_entities {
                    // The snapshot updates in on_entity_replaced once the
                    // document has accepted the edit and assigned handles.
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
        if !replacements.is_empty() {
            self.hover_cut_cache.clear();
            self.hover_cut_order.clear();
        }
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
        } else if !replacements.is_empty() {
            // Non-rebuilding replacement still mutates self.geos in place.
            // The cached spatial indices must never point at old positions.
            self.geo_spatial.get_mut().take();
        }
    }

    fn on_drag_selection(
        &mut self,
        fence: &[[f64; 2]],
        window: Option<([f64; 2], [f64; 2])>,
        crossing: bool,
    ) -> Option<CmdResult> {
        if fence.len() < 2 {
            return None;
        }
        let window = window.map(|(min, max)| CrossingWindow { min, max });
        if matches!(self.mode, TrimMode::SelectEdges) {
            let handles = drag_selected_handles(
                &self.all_entities,
                &self.entity_index,
                fence,
                window,
                crossing,
            );
            update_trim_edge_set(&mut self.edge_set, handles, self.shift);
            return Some(CmdResult::NeedPoint);
        }
        if !matches!(self.mode, TrimMode::Pick) {
            return None;
        }
        Some(self.fence_run(fence, window))
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
            TrimMode::Erase => {
                // Erase mode: the hovered object previews fully gray.
                if let Some(e) = self
                    .all_entities
                    .iter()
                    .find(|e| e.common().handle == handle)
                {
                    return vec![WireModel::solid(
                        "erase_prev".into(),
                        entity_pts(e),
                        TRIM_REMOVE_GRAY,
                        false,
                    )];
                }
                return vec![];
            }
            TrimMode::Pick => {}
            _ => return vec![],
        }
        if self.shift {
            // Shift held: preview the extend result instead.
            if let Some(ext) = pick_extend_at(&self.all_entities, &self.geos, handle, pt.x, pt.y) {
                if let Some(orig) = self
                    .all_entities
                    .iter()
                    .find(|e| e.common().handle == handle)
                {
                    return extend_hover_wires(
                        orig,
                        &ext,
                        &self.all_entities,
                        handle,
                        self.implied_edges,
                    );
                }
                return vec![WireModel::solid(
                    "extend_prev".into(),
                    entity_pts(&ext),
                    WireModel::CYAN,
                    false,
                )];
            }
            return vec![];
        }
        if handle.is_null() {
            return vec![];
        }
        // A picked INSERT is not an editable entity for TRIM. Its nested
        // geometry still acts as cutting edges for external targets, but
        // hovering the block must not query/clone thousands of cutters.
        if !matches!(
            self.entity_index.get(&self.all_entities, handle),
            Some(
                EntityType::Line(_)
                    | EntityType::Arc(_)
                    | EntityType::Circle(_)
                    | EntityType::Ray(_)
                    | EntityType::XLine(_)
                    | EntityType::Ellipse(_)
                    | EntityType::Spline(_)
                    | EntityType::LwPolyline(_)
                    | EntityType::Hatch(_)
            )
        ) {
            return vec![];
        }

        let hover_started = crate::perf::enabled().then(iced::time::Instant::now);
        let hover_cuts = if self.implied_edges {
            None
        } else {
            self.cached_hover_cut_params(handle)
        };
        // A cache hit needs neither a spatial query nor another intersection.
        let nearby_geos = if hover_cuts.is_some() {
            Vec::new()
        } else {
            self.nearby_geos(handle)
        };
        let geos = nearby_geos.as_slice();
        let entity = self.entity_index.get(&self.all_entities, handle);

        let mut hover_wires = match entity {
            Some(EntityType::Line(l)) => {
                let ax = l.start.x;
                let ay = l.start.y;
                let bx = l.end.x;
                let by = l.end.y;
                let ts = hover_cuts
                    .clone()
                    .unwrap_or_else(|| line_seg_ts(ax, ay, bx, by, handle, geos));
                if ts.is_empty() {
                    if self.quick && (bx - ax).hypot(by - ay) > 1e-9 {
                        return vec![WireModel::solid(
                            "trim_delete".into(),
                            entity_pts(&EntityType::Line(l.clone())),
                            TRIM_REMOVE_GRAY,
                            false,
                        )];
                    }
                    return vec![];
                }
                let dx = bx - ax;
                let dy = by - ay;
                let len2 = dx * dx + dy * dy;
                let t_click = if len2 > 1e-12 {
                    ((pt.x - ax) * dx + (pt.y - ay) * dy) / len2
                } else {
                    0.5
                };
                let survivors = trim_line(l, &ts, t_click);
                let removed_points = trimmed_line_removed_preview(l, &survivors, t_click)
                    .unwrap_or_else(|| entity_pts(&EntityType::Line(l.clone())));
                let removed =
                    WireModel::solid("trim_rm".into(), removed_points, TRIM_REMOVE_GRAY, false);
                let mut out = vec![removed];
                for (i, e) in survivors.iter().enumerate() {
                    let pts = entity_pts(e);
                    out.push(WireModel::solid(
                        format!("trim_keep_{i}"),
                        pts,
                        WireModel::CYAN,
                        false,
                    ));
                }
                out
            }
            Some(EntityType::Arc(a)) => {
                let cx = a.center.x;
                let cy = a.center.y;
                let a0 = a.start_angle;
                let a1 = a.end_angle;
                let ts = hover_cuts
                    .clone()
                    .unwrap_or_else(|| arc_seg_ts(cx, cy, a.radius, a0, a1, handle, geos));
                if ts.is_empty() {
                    return vec![];
                }
                let click_angle = (pt.y - cy).atan2(pt.x - cx);
                let t_click = arc_t(click_angle, a0, a1);
                let survivors = trim_arc(a, &ts, t_click);
                let orig_pts = arc_pts(cx, cy, a.radius, a0, a1, a.center.z);
                let removed = WireModel::solid("trim_rm".into(), orig_pts, TRIM_REMOVE_GRAY, false);
                let mut out = vec![removed];
                for (i, e) in survivors.iter().enumerate() {
                    let pts = entity_pts(e);
                    out.push(WireModel::solid(
                        format!("trim_keep_{i}"),
                        pts,
                        WireModel::CYAN,
                        false,
                    ));
                }
                out
            }
            Some(EntityType::Circle(c)) => {
                let cx = c.center.x;
                let cy = c.center.y;
                let ts = hover_cuts
                    .clone()
                    .unwrap_or_else(|| arc_seg_ts(cx, cy, c.radius, 0.0, TAU, handle, geos));
                if ts.len() < 2 {
                    return vec![];
                }
                let click_angle = (pt.y - cy).atan2(pt.x - cx);
                let t_click = arc_t(click_angle, 0.0, TAU);
                let survivors = trim_circle(c, &ts, t_click);
                if survivors.is_empty() {
                    return vec![];
                }
                let orig_pts = arc_pts(cx, cy, c.radius, 0.0, TAU, c.center.z);
                let removed = WireModel::solid("trim_rm".into(), orig_pts, TRIM_REMOVE_GRAY, false);
                let mut out = vec![removed];
                for (i, e) in survivors.iter().enumerate() {
                    let pts = entity_pts(e);
                    out.push(WireModel::solid(
                        format!("trim_keep_{i}"),
                        pts,
                        WireModel::CYAN,
                        false,
                    ));
                }
                out
            }
            Some(EntityType::Ray(r)) => {
                let bx = r.base_point.x;
                let by = r.base_point.y;
                let curve = ray_curve(r);
                let ts = hover_cuts
                    .clone()
                    .unwrap_or_else(|| cut_params(&curve, handle, geos));
                if ts.is_empty() {
                    return vec![];
                }
                let t_click = curve.parameter_at([pt.x, pt.y]);
                let survivors = trim_ray(r, &ts, t_click);
                // Show a finite preview section (20 units) for the original ray
                let far = [
                    (bx + r.direction.x * 20.0) as f32,
                    (by + r.direction.y * 20.0) as f32,
                    r.base_point.z as f32,
                ];
                let base = [bx as f32, by as f32, r.base_point.z as f32];
                let removed =
                    WireModel::solid("trim_rm".into(), vec![base, far], TRIM_REMOVE_GRAY, false);
                let mut out = vec![removed];
                for (i, e) in survivors.iter().enumerate() {
                    let pts = entity_pts(e);
                    out.push(WireModel::solid(
                        format!("trim_keep_{i}"),
                        pts,
                        WireModel::CYAN,
                        false,
                    ));
                }
                out
            }
            Some(EntityType::XLine(x)) => {
                let bx = x.base_point.x;
                let by = x.base_point.y;
                let curve = xline_curve(x);
                let ts = hover_cuts
                    .clone()
                    .unwrap_or_else(|| cut_params(&curve, handle, geos));
                if ts.is_empty() {
                    return vec![];
                }
                let t_click = curve.parameter_at([pt.x, pt.y]);
                let survivors = trim_xline(x, &ts, t_click);
                let neg = [
                    (bx - x.direction.x * 20.0) as f32,
                    (by - x.direction.y * 20.0) as f32,
                    x.base_point.z as f32,
                ];
                let pos_pt = [
                    (bx + x.direction.x * 20.0) as f32,
                    (by + x.direction.y * 20.0) as f32,
                    x.base_point.z as f32,
                ];
                let removed =
                    WireModel::solid("trim_rm".into(), vec![neg, pos_pt], TRIM_REMOVE_GRAY, false);
                let mut out = vec![removed];
                for (i, e) in survivors.iter().enumerate() {
                    let pts = entity_pts(e);
                    out.push(WireModel::solid(
                        format!("trim_keep_{i}"),
                        pts,
                        WireModel::CYAN,
                        false,
                    ));
                }
                out
            }
            Some(EntityType::Ellipse(e)) => {
                let a = (e.major_axis.x.powi(2) + e.major_axis.y.powi(2)).sqrt();
                if a < 1e-9 {
                    return vec![];
                }
                let b = a * e.minor_axis_ratio;
                if b < 1e-9 {
                    return vec![];
                }
                let (nx, ny) = (e.major_axis.x / a, e.major_axis.y / a);
                let t0 = e.start_parameter;
                let mut t1 = e.end_parameter;
                if t1 <= t0 {
                    t1 += TAU;
                }
                let ts = hover_cuts.clone().unwrap_or_else(|| {
                    ellipse_seg_ts(e.center.x, e.center.y, a, b, nx, ny, t0, t1, handle, geos)
                });
                if ts.is_empty() {
                    return vec![];
                }
                let rx = pt.x - e.center.x;
                let ry = pt.y - e.center.y;
                let xl = rx * nx + ry * ny;
                let yl = -rx * ny + ry * nx;
                let t_ell = ellipse_closest_parameter(a, b, xl, yl);
                let t_click = arc_t(t_ell, t0, t1);
                let survivors = trim_ellipse(e, &ts, t_click);
                let orig_pts =
                    ellipse_pts(e.center.x, e.center.y, a, b, nx, ny, t0, t1, e.center.z);
                let removed = WireModel::solid("trim_rm".into(), orig_pts, TRIM_REMOVE_GRAY, false);
                let mut out = vec![removed];
                for (i, ent) in survivors.iter().enumerate() {
                    let pts = entity_pts(ent);
                    out.push(WireModel::solid(
                        format!("trim_keep_{i}"),
                        pts,
                        WireModel::CYAN,
                        false,
                    ));
                }
                out
            }
            Some(EntityType::Spline(s)) => {
                let ts = hover_cuts
                    .clone()
                    .unwrap_or_else(|| spline_seg_ts(s, handle, geos));
                if ts.is_empty() {
                    return vec![];
                }
                let t_click = spline_nearest_t(s, pt.x, pt.y)
                    .and_then(|t_actual| {
                        let (t0, t1) = spline_range(s)?;
                        Some(t_to_rel(t_actual, t0, t1))
                    })
                    .unwrap_or(0.5);
                let orig_pts = spline_pts_wire(s);
                let removed = WireModel::solid("trim_rm".into(), orig_pts, TRIM_REMOVE_GRAY, false);
                let survivors = trim_spline(s, &ts, t_click);
                let mut out = vec![removed];
                for (i, ent) in survivors.iter().enumerate() {
                    let pts = entity_pts(ent);
                    out.push(WireModel::solid(
                        format!("trim_keep_{i}"),
                        pts,
                        WireModel::CYAN,
                        false,
                    ));
                }
                out
            }
            Some(EntityType::Hatch(hatch)) => {
                if hatch_is_standalone_circle(hatch) {
                    return hatch
                        .paths
                        .iter()
                        .enumerate()
                        .filter_map(|(index, path)| {
                            let mut points = crate::scene::hatch_path_ring(path)?;
                            points.push(*points.first()?);
                            Some(WireModel::solid_f64(
                                format!("trim_hatch_erase_{index}"),
                                points
                                    .into_iter()
                                    .map(|p| [p[0], p[1], hatch.elevation])
                                    .collect(),
                                TRIM_REMOVE_GRAY,
                                false,
                            ))
                        })
                        .collect();
                }
                let started = crate::perf::enabled().then(iced::time::Instant::now);
                let preview = self
                    .hatch_trim_at(handle, pt)
                    .map(|trim| trim.preview_wires())
                    .unwrap_or_default();
                let ms = crate::perf::elapsed_ms(started);
                if ms >= 8.0 {
                    crate::perf_record!("[perf] trim-hover-hatch {ms:.2}ms");
                }
                preview
            }
            Some(EntityType::LwPolyline(p)) => {
                let started = crate::perf::enabled().then(iced::time::Instant::now);
                let result = if let Some(cuts) = hover_cuts.as_ref() {
                    trim_lwpolyline_with_cuts(p, pt.x, pt.y, cuts)
                } else {
                    trim_lwpolyline(p, pt.x, pt.y, geos)
                };
                let ms = crate::perf::elapsed_ms(started);
                if ms >= 8.0 {
                    crate::perf_record!("[perf] trim-hover-polyline {ms:.2}ms");
                }
                let Some(survivors) = result else {
                    return vec![];
                };
                let orig = WireModel::solid(
                    "trim_rm".into(),
                    entity_pts(entity.unwrap()),
                    TRIM_REMOVE_GRAY,
                    false,
                );
                let mut out = vec![orig];
                for (i, ent) in survivors.iter().enumerate() {
                    out.push(WireModel::solid(
                        format!("trim_keep_{i}"),
                        entity_pts(ent),
                        WireModel::CYAN,
                        false,
                    ));
                }
                out
            }
            _ => vec![],
        };
        // Edge: Extend — explain cuts landing on an IMPLIED boundary with a
        // dashed guide from that boundary's drawn end to the cut point (#336).
        if self.implied_edges && !hover_wires.is_empty() {
            if let (Some(orig), Some(pieces)) = (
                entity,
                pick_trim_at(&self.all_entities, geos, handle, pt.x, pt.y),
            ) {
                let cuts = piece_cut_points(orig, &pieces);
                hover_wires.extend(implied_cut_guides(&self.all_entities, handle, &cuts));
            }
        }
        let hover_ms = crate::perf::elapsed_ms(hover_started);
        if hover_ms >= 8.0 {
            crate::perf_record!(
                "[perf] trim-hover-total {hover_ms:.2}ms cached_cuts={} wires={}",
                hover_cuts.is_some(),
                hover_wires.len(),
            );
        }
        hover_wires
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
                // The rectangle's edges act as a 4-segment fence, and objects
                // wholly inside the window are picked too (crossing-selection
                // semantics).
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
                let mut out = vec![pick.preview([pt.x, pt.y], "trim_fence")];
                let mut fpts = pick.path();
                fpts.push([pt.x, pt.y]);
                out.extend(fence_result_preview(
                    &self.all_entities,
                    &self.entity_index,
                    &self.geos,
                    &fpts,
                    None,
                    self.shift,
                    self.implied_edges,
                ));
                out
            }
            TrimMode::CrossSecond(p1) => {
                let p1 = *p1;
                let p2 = [pt.x, pt.y];
                let mut out = vec![crossing_box_preview(p1, p2, "trim_cross")];
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
                    self.shift,
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

// ══════════════════════════════════════════════════════════════════════════
