use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Unconstrained,
    Partial,
    Full,
    Over,
    Unavailable,
}

impl Status {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Unconstrained => "Unconstrained",
            Self::Partial => "Partially constrained",
            Self::Full => "Fully constrained",
            Self::Over => "Over-constrained",
            Self::Unavailable => "Unavailable",
        }
    }
    fn color_slot(self) -> Option<usize> {
        match self {
            Self::Unconstrained => Some(0),
            Self::Partial => Some(1),
            Self::Full => Some(2),
            Self::Over => Some(3),
            Self::Unavailable => None,
        }
    }
}

#[derive(Default)]
pub(crate) struct Cache {
    entry: Option<CacheEntry>,
}

struct CacheEntry {
    definition: Handle,
    epoch: u64,
    constraints: Vec<ParametricConstraint>,
    parameters: ParameterTable,
    local: ParameterTable,
    report: std::sync::Arc<HashMap<Handle, Status>>,
}

pub(in crate::scene) struct Suppress<'a>(&'a std::cell::Cell<bool>, bool);
impl Drop for Suppress<'_> {
    fn drop(&mut self) {
        self.0.set(self.1);
    }
}

impl Scene {
    pub(crate) fn constraint_shading_active(&self) -> bool {
        self.block_constraint_status_mode
            && self.block_edit_block.is_some()
            && !self.constraint_status_suppressed.get()
    }

    pub(in crate::scene) fn suppress_constraint_shading(&self) -> Suppress<'_> {
        Suppress(
            &self.constraint_status_suppressed,
            self.constraint_status_suppressed.replace(true),
        )
    }

    fn invalidate_constraint_display(&self) {
        self.resident_wire_sets.borrow_mut().clear();
        self.tess_memo.borrow_mut().clear();
        self.resident_tess_memo.borrow_mut().clear();
        self.block_defn_cache.borrow_mut().clear();
        self.hatch_cache.borrow_mut().clear();
        self.frozen_hatch_cache.borrow_mut().clear();
        self.mesh_cache.borrow_mut().clear();
        self.frozen_mesh_cache.borrow_mut().clear();
    }

    pub(crate) fn set_block_constraint_display(&mut self, enabled: bool, colors: [String; 4]) {
        if self.block_constraint_status_mode != enabled
            || self.block_constraint_status_colors != colors
        {
            self.block_constraint_status_mode = enabled;
            self.block_constraint_status_colors = colors;
            self.invalidate_constraint_display();
        }
    }

    pub(crate) fn block_constraint_report(
        &self,
        definition: Handle,
    ) -> std::sync::Arc<HashMap<Handle, Status>> {
        let scope = super::super::parametric_constraints::ParametricScope::Block(definition);
        let empty = super::super::parametric_constraints::ParametricConstraintSet::new(scope);
        let set = self
            .parametric_constraints
            .iter()
            .find(|set| set.scope == scope)
            .unwrap_or(&empty);
        {
            let cache = self.constraint_status_cache.borrow();
            if let Some(entry) = &cache.entry {
                if entry.definition == definition
                    && entry.epoch == self.geometry_epoch
                    && entry.constraints == set.constraints
                    && entry.parameters == self.named_parameters
                    && entry.local == set.local_parameters
                {
                    return std::sync::Arc::clone(&entry.report);
                }
            }
        }
        let report = std::sync::Arc::new(analyze(&self.document, &self.named_parameters, set));
        let changed = self
            .constraint_status_cache
            .borrow()
            .entry
            .as_ref()
            .is_none_or(|old| {
                old.definition != definition || old.report.as_ref() != report.as_ref()
            });
        if changed && self.constraint_shading_active() {
            self.invalidate_constraint_display();
        }
        self.constraint_status_cache.borrow_mut().entry = Some(CacheEntry {
            definition,
            epoch: self.geometry_epoch,
            constraints: set.constraints.clone(),
            parameters: self.named_parameters.clone(),
            local: set.local_parameters.clone(),
            report: std::sync::Arc::clone(&report),
        });
        report
    }

    pub(in crate::scene) fn constraint_palette(&self) -> [[f32; 4]; 4] {
        let bg = self.current_bg();
        let foreground = if bg[0] + bg[1] + bg[2] < 1.5 {
            iced::Color::WHITE
        } else {
            iced::Color::BLACK
        };
        std::array::from_fn(|index| {
            let color = crate::ui::overlay::authoring::display_color(
                &self.block_constraint_status_colors[index],
                foreground,
                foreground,
            );
            [color.r, color.g, color.b, color.a]
        })
    }

    pub(in crate::scene) fn constraint_display_color(
        &self,
        report: &HashMap<Handle, Status>,
        handle: Handle,
        palette: &[[f32; 4]; 4],
    ) -> Option<[f32; 4]> {
        let definition = self.block_edit_block?;
        if !self.constraint_shading_active()
            || self.document.get_entity(handle)?.common().owner_handle != definition
        {
            return None;
        }
        report.get(&handle)?.color_slot().map(|slot| palette[slot])
    }

    pub(in crate::scene) fn shade_constraint_wires(
        &self,
        block: Handle,
        wires: &mut [super::super::WireModel],
    ) {
        if !self.constraint_shading_active() || self.block_edit_block != Some(block) {
            return;
        }
        let report = self.block_constraint_report(block);
        let palette = self.constraint_palette();
        for wire in wires {
            if wire
                .bg_adapt
                .as_ref()
                .is_some_and(|style| style.canvas_color)
            {
                continue;
            }
            if let Some(mut color) = Self::handle_from_wire_name(&wire.name)
                .and_then(|handle| self.constraint_display_color(&report, handle, &palette))
            {
                color[3] = wire.color[3];
                wire.color = color;
                wire.bg_adapt = None;
                wire.render_instance = None;
                for vertex in &mut wire.text_verts {
                    vertex.color = [color[0], color[1], color[2], vertex.color[3]];
                }
            }
        }
    }

    pub(in crate::scene) fn shade_constraint_meshes(
        &self,
        block: Handle,
        models: &mut [super::super::MeshLodSet],
    ) {
        if !self.constraint_shading_active() || self.block_edit_block != Some(block) {
            return;
        }
        let report = self.block_constraint_report(block);
        let palette = self.constraint_palette();
        for model in models {
            let handle = model.instance_handle.or_else(|| {
                model
                    .lods
                    .first()
                    .and_then(|lod| Self::handle_from_wire_name(&lod.name))
            });
            let Some(mut color) =
                handle.and_then(|handle| self.constraint_display_color(&report, handle, &palette))
            else {
                continue;
            };
            color[3] = model.display_color().map_or(1.0, |color| color[3]);
            model.instance_color = Some(color);
            model.edge_color = Some(color);
            model.face_materials.clear();
            if let Some(material) = &mut model.material {
                material.diffuse = color;
            }
            for lod in &mut model.lods {
                lod.color = color;
                lod.triangle_colors.clear();
                lod.triangle_material_handles.clear();
            }
        }
    }

    pub(crate) fn block_constraint_entity_status(
        &self,
        definition: Handle,
        entity: Handle,
    ) -> Status {
        if self
            .document
            .get_entity(entity)
            .is_none_or(|e| e.common().owner_handle != definition)
        {
            return Status::Unavailable;
        }
        self.block_constraint_report(definition)
            .get(&entity)
            .copied()
            .unwrap_or(Status::Unavailable)
    }
}

fn geometry_parameters(geometry: &EntityGeom) -> Vec<ParamId> {
    let line = |line: &GLine| vec![line.p1.x, line.p1.y, line.p2.x, line.p2.y];
    let arc = |arc: &GArc| {
        vec![
            arc.circle.center.x,
            arc.circle.center.y,
            arc.circle.rad,
            arc.start.x,
            arc.start.y,
            arc.end.x,
            arc.end.y,
            arc.start_angle,
            arc.end_angle,
        ]
    };
    match geometry {
        EntityGeom::Point(point) => vec![point.x, point.y],
        EntityGeom::Line(value)
        | EntityGeom::TextLine(value)
        | EntityGeom::Ray(value)
        | EntityGeom::XLine(value) => line(value),
        EntityGeom::Circle(circle) => vec![circle.center.x, circle.center.y, circle.rad],
        EntityGeom::Arc(value) => arc(value),
        EntityGeom::Ellipse(ellipse) => vec![
            ellipse.center.x,
            ellipse.center.y,
            ellipse.focus1.x,
            ellipse.focus1.y,
            ellipse.radmin,
        ],
        EntityGeom::Polyline { points, arcs, .. } => points
            .iter()
            .flat_map(|p| [p.x, p.y])
            .chain(arcs.iter().flatten().flat_map(|segment| arc(&segment.arc)))
            .collect(),
        EntityGeom::Spline { curve, .. } => curve.poles.iter().flat_map(|p| [p.x, p.y]).collect(),
    }
}

/// Analyze persistent equations in an isolated store, without edit-time pins,
/// write-back, or mutation of the document and constraint set.
pub(crate) fn analyze(
    document: &codec::Document,
    drawing_parameters: &ParameterTable,
    set: &super::super::parametric_constraints::ParametricConstraintSet,
) -> HashMap<Handle, Status> {
    let parameters = if set.local_parameters.is_empty() {
        drawing_parameters
    } else {
        &set.local_parameters
    };
    let scope = set.scope.owner_handle(document);
    let mut result: HashMap<_, _> = document
        .entities()
        .filter(|entity| {
            entity.common().owner_handle == scope
                && !matches!(
                    entity,
                    EntityType::Block(_) | EntityType::BlockEnd(_) | EntityType::Viewport(_)
                )
        })
        .map(|entity| (entity.common().handle, Status::Unconstrained))
        .collect();
    let mut sys = System::new();
    let mut cache = HashMap::new();
    let mut owners = Vec::new();
    let mut unavailable = HashSet::new();
    for constraint in set.constraints.iter().filter(|c| c.enabled) {
        for reference in &constraint.refs {
            if let Some(status) = result.get_mut(&reference.entity) {
                *status = Status::Partial;
            }
        }
        if constraint.refs.iter().any(|reference| {
            document.get_entity(reference.entity).is_none_or(|entity| {
                entity.common().owner_handle != set.scope.owner_handle(document)
            })
        }) {
            unavailable.extend(constraint.refs.iter().map(|r| r.entity));
            continue;
        }
        let mut equations = build_constraint(
            document,
            &mut sys,
            &mut cache,
            parameters,
            constraint,
            None,
            None,
            &HashMap::new(),
        );
        // Fixed holds the complete ellipse. The solve path also retains its
        // focus-to-center offset; include that freedom here without importing
        // the temporary shape-retention equations used during editing.
        if constraint.kind == ConstraintKind::Fixed {
            if let Some(reference) = constraint.refs.first().filter(|r| r.marker.is_none()) {
                if let Some(EntityGeom::Ellipse(ellipse)) = cache.get(&reference.entity) {
                    for parameter in [ellipse.focus1.x, ellipse.focus1.y] {
                        let value = sys.store().get(parameter);
                        let target = sys.add_param(value, true);
                        equations.push(Rc::new(Equal::new(parameter, target, 1.0)));
                    }
                }
            }
        }
        if equations.is_empty() || !refs_share_supported_plane(document, &constraint.refs) {
            unavailable.extend(constraint.refs.iter().map(|r| r.entity));
            continue;
        }
        for equation in equations {
            owners.push((Rc::clone(&equation), constraint));
            sys.add_constraint(equation);
        }
    }
    add_intrinsic_arc_rules(&mut sys, &cache);
    // A text baseline's second point is an orientation guide. Its distance is
    // fixed by text layout, not an additional editable geometric freedom.
    for geometry in cache.values() {
        if let EntityGeom::TextLine(line) = geometry {
            let store = sys.store();
            let length = (store.get(line.p2.x) - store.get(line.p1.x))
                .hypot(store.get(line.p2.y) - store.get(line.p1.y));
            let target = sys.add_param(length, true);
            sys.add_constraint(Rc::new(P2PDistance::new(line.p1, line.p2, target)));
        }
    }
    let partitions = sys.partition();
    for partition in &partitions {
        if !partition
            .calc_residual(sys.store())
            .iter()
            .all(|value| value.is_finite())
            || !partition
                .calc_jacobi(sys.store())
                .iter()
                .all(|value| value.is_finite())
        {
            for (handle, geometry) in &cache {
                if geometry_parameters(geometry)
                    .iter()
                    .any(|p| partition.plist().contains(p))
                {
                    unavailable.insert(*handle);
                }
            }
            continue;
        }
        if solve_dl(partition, sys.store_mut()) == SolveStatus::Failed {
            let _ = solve_lm(partition, sys.store_mut());
        }
    }
    let mut pinned = HashSet::new();
    let mut over = HashSet::new();
    for partition in &partitions {
        let matrix = partition.calc_jacobi(sys.store());
        if !matrix.iter().all(|value| value.is_finite()) {
            for (handle, geometry) in &cache {
                if geometry_parameters(geometry)
                    .iter()
                    .any(|p| partition.plist().contains(p))
                {
                    unavailable.insert(*handle);
                }
            }
            continue;
        }
        if matrix.nrows() != 0 && matrix.ncols() != 0 {
            let tolerance = (matrix.norm() * 1e-10).max(f64::EPSILON);
            let decomposition = matrix.svd(false, true);
            if let Some(vt) = decomposition.v_t {
                // The row-space projector fixes a coordinate only if that
                // coordinate has no component in the Jacobian's null space.
                // One SVD per partition also handles coupled entities without
                // performing a fresh rank decomposition for each object.
                for (column, parameter) in partition.plist().iter().enumerate() {
                    let projection: f64 = decomposition
                        .singular_values
                        .iter()
                        .enumerate()
                        .filter(|(_, value)| **value > tolerance)
                        .map(|(row, _)| vt[(row, column)].powi(2))
                        .sum();
                    if projection >= 1.0 - 1e-8 {
                        pinned.insert(*parameter);
                    }
                }
            }
        }
        let diagnosis = kernel_constraints::diagnosis::diagnose(partition, sys.store());
        for row in diagnosis.redundant {
            if let Some((_, owner)) = owners
                .iter()
                .find(|(equation, _)| Rc::ptr_eq(equation, &partition.constraints()[row]))
            {
                over.extend(owner.refs.iter().map(|r| r.entity));
            }
        }
    }
    for (equation, owner) in &owners {
        let error = equation.error(sys.store());
        if !error.is_finite() {
            unavailable.extend(owner.refs.iter().map(|r| r.entity));
        } else if error.abs() > 1e-6 {
            over.extend(owner.refs.iter().map(|r| r.entity));
        }
    }
    for (handle, geometry) in &cache {
        let params = geometry_parameters(geometry);
        if !params.is_empty()
            && params
                .iter()
                .all(|p| sys.store().is_driven(*p) || pinned.contains(p))
        {
            result.insert(*handle, Status::Full);
        }
    }
    for handle in unavailable {
        if let Some(status) = result.get_mut(&handle) {
            *status = Status::Unavailable;
        }
    }
    for handle in over {
        if let Some(status) = result.get_mut(&handle) {
            *status = Status::Over;
        }
    }
    result
}

#[cfg(test)]
mod tests;

pub(super) fn add_intrinsic_arc_rules(sys: &mut System, cache: &HashMap<Handle, EntityGeom>) {
    let add_arc_rules = |sys: &mut System, arc: GArc, sweep: Option<f64>| {
        let curve = Rc::new(arc);
        sys.add_constraint(Rc::new(CurveValue::new(
            arc.start,
            arc.start.x,
            curve.clone(),
            arc.start_angle,
        )));
        sys.add_constraint(Rc::new(CurveValue::new(
            arc.start,
            arc.start.y,
            curve.clone(),
            arc.start_angle,
        )));
        sys.add_constraint(Rc::new(CurveValue::new(
            arc.end,
            arc.end.x,
            curve.clone(),
            arc.end_angle,
        )));
        sys.add_constraint(Rc::new(CurveValue::new(
            arc.end,
            arc.end.y,
            curve,
            arc.end_angle,
        )));
        if let Some(sweep) = sweep {
            let target = sys.add_param(sweep, true);
            sys.add_constraint(Rc::new(Difference::new(
                arc.start_angle,
                arc.end_angle,
                target,
            )));
        }
    };
    for geom in cache.values() {
        match geom {
            EntityGeom::Arc(arc) => add_arc_rules(sys, *arc, None),
            EntityGeom::Polyline { arcs, .. } => {
                for segment in arcs.iter().flatten() {
                    add_arc_rules(sys, segment.arc, Some(segment.sweep));
                }
            }
            _ => {}
        }
    }
}
