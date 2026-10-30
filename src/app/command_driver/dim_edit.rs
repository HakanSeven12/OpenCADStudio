use super::*;

impl OpenCADStudio {
    pub(super) fn handle_edit_dimension_break(
        &mut self,
        dimensions: Vec<Handle>,
        operation: crate::command::DimensionBreakOperation,
    ) {
        let i = self.active_tab;
        self.push_undo_snapshot(i, "DIMBREAK");
        let result = apply_dimbreak(&mut self.tabs[i].scene, &dimensions, operation);
        if result.changed.is_empty() {
            self.command_line.push_info(&result.message);
        } else {
            for handle in &result.changed {
                self.tabs[i].scene.invalidate_dim_block_recorded(*handle);
            }
            let changes = result
                .changed
                .iter()
                .copied()
                .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                .collect::<Vec<_>>();
            self.tabs[i].scene.bump_entities(&changes);
            self.tabs[i].dirty = true;
            self.command_line.push_output(&result.message);
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
    }

    /// One answer of DIMREASSOCIATE: the point (or object) is applied at
    /// once; the whole command is one undo step.
    pub(super) fn handle_reassociate_dimension(
        &mut self,
        dimension: Handle,
        target: crate::command::ReassociateTarget,
        last: bool,
    ) {
        let i = self.active_tab;
        if self.tabs[i].history.pending.is_none() {
            self.push_undo_snapshot(i, "DIMREASSOCIATE");
        }
        if self.tabs[i].scene.reassociate_dimension(dimension, &target) {
            self.invalidate_property_targets(i, &[dimension]);
            self.tabs[i].dirty = true;
        } else {
            self.command_line
                .push_info(crate::t!("Dimension point not reassociated.").as_ref());
        }
        if last {
            self.tabs[i].active_cmd = None;
            self.tabs[i].snap_result = None;
            self.command_line.set_step_options(Vec::new());
        } else {
            let _ = self.handle_need_point();
        }
    }

    pub(super) fn handle_edit_dimension_jog(
        &mut self,
        dimension: Handle,
        point: Option<glam::DVec3>,
    ) {
        let i = self.active_tab;
        let valid = matches!(
            self.tabs[i]
                .scene
                .document
                .get_entity(dimension)
                .and_then(crate::entities::dimension::as_dimension),
            Some(codec::entities::Dimension::Linear(_) | codec::entities::Dimension::Aligned(_))
        );
        if !valid {
            self.command_line.push_error(
                crate::t!("DIMJOGLINE: select a linear or aligned dimension.").as_ref(),
            );
        } else if !self.reject_locked_edit(i, dimension) {
            self.push_undo_snapshot(i, "DIMJOGLINE");
            let values = point.map(|point| {
                use codec::xdata::XDataValue;
                vec![
                    XDataValue::Integer16(387),
                    XDataValue::Integer16(3),
                    XDataValue::Integer16(389),
                    XDataValue::Point3D(codec::types::Vector3::new(point.x, point.y, point.z)),
                ]
            });
            crate::scene::view::dispatch::set_entity_xdata(
                &mut self.tabs[i].scene.document,
                dimension,
                "ACAD_DSTYLE_DIMJAG_POSITION",
                values,
            );
            self.tabs[i].scene.invalidate_dim_block_recorded(dimension);
            self.tabs[i]
                .scene
                .bump_entities(&[(dimension, crate::scene::ChangeKind::Modified)]);
            self.tabs[i].dirty = true;
            self.command_line.push_output(
                if point.is_some() {
                    crate::t!("DIMJOGLINE: jog added.")
                } else {
                    crate::t!("DIMJOGLINE: jog removed.")
                }
                .as_ref(),
            );
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
    }

    pub(super) fn handle_space_dimensions(
        &mut self,
        base: Handle,
        others: Vec<Handle>,
        spacing: Option<f64>,
    ) {
        let i = self.active_tab;
        // DIM's aliGn comes back to DIM's prompt, quietly.
        let in_dim = self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|command| command.name() == "DIM");
        if !others.is_empty() {
            self.push_undo_snapshot(i, if in_dim { "DIM" } else { "DIMSPACE" });
            if apply_dimspace(&mut self.tabs[i].scene, base, &others, spacing) {
                self.tabs[i].dirty = true;
                if !in_dim {
                    self.command_line
                        .push_output(crate::t!("DIMSPACE  Spacing adjusted.").as_ref());
                }
            }
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        if in_dim {
            self.tabs[i].scene.deselect_all();
            self.start_dim(i);
        }
    }

    pub(super) fn handle_distribute_dimensions(
        &mut self,
        base: Option<Handle>,
        dimensions: Vec<Handle>,
        offset: Option<f64>,
    ) {
        let i = self.active_tab;
        if !dimensions.is_empty() {
            self.push_undo_snapshot(i, "DIM");
            if apply_dim_distribute(&mut self.tabs[i].scene, base, &dimensions, offset) {
                self.tabs[i].dirty = true;
            }
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.deselect_all();
        self.start_dim(i);
    }
}

struct DimBreakResult {
    changed: Vec<Handle>,
    message: String,
}

fn wire_segments(
    models: &[crate::scene::model::wire_model::WireModel],
) -> Vec<(glam::DVec3, glam::DVec3)> {
    let mut output = Vec::new();
    for model in models {
        for run in model.points.split(|point| !point[0].is_finite()) {
            for pair in run.windows(2) {
                let first =
                    glam::DVec3::new(pair[0][0] as f64, pair[0][1] as f64, pair[0][2] as f64);
                let second =
                    glam::DVec3::new(pair[1][0] as f64, pair[1][1] as f64, pair[1][2] as f64);
                if first.is_finite()
                    && second.is_finite()
                    && first.distance_squared(second) > 1.0e-18
                {
                    output.push((first, second));
                }
            }
        }
    }
    output
}

fn segment_intersection_xy(
    first: (glam::DVec3, glam::DVec3),
    second: (glam::DVec3, glam::DVec3),
) -> Option<(glam::DVec3, glam::DVec3)> {
    let gap = kernel::space::segment_break_gap_xy(
        [first.0.to_array(), first.1.to_array()],
        [second.0.to_array(), second.1.to_array()],
        0.25,
    )?;
    Some((
        glam::DVec3::from_array(gap[0]),
        glam::DVec3::from_array(gap[1]),
    ))
}

fn break_object_handle(document: &codec::Document, dimension: Handle) -> Option<Handle> {
    document.objects.iter().find_map(|(handle, object)| {
        let codec::objects::ObjectType::DataObject(object) = object else {
            return None;
        };
        let codec::objects::DataObjectData::BreakData(data) = &object.data else {
            return None;
        };
        (data.dimension_reference == dimension).then_some(*handle)
    })
}

fn remove_dimension_break_data(document: &mut codec::Document, dimension: Handle) -> bool {
    let handles: Vec<_> = document
        .objects
        .iter()
        .filter_map(|(handle, object)| match object {
            codec::objects::ObjectType::DataObject(object) => match &object.data {
                codec::objects::DataObjectData::BreakData(data)
                    if data.dimension_reference == dimension =>
                {
                    Some(*handle)
                }
                _ => None,
            },
            _ => None,
        })
        .collect();
    if handles.is_empty() {
        return false;
    }
    for handle in &handles {
        document.objects.remove(handle);
    }
    if let Some(dictionary_handle) = document.extension_dictionary_handle(dimension) {
        if let Some(codec::objects::ObjectType::Dictionary(dictionary)) =
            document.objects.get_mut(&dictionary_handle)
        {
            dictionary.entries.retain(|(name, handle)| {
                !name.eq_ignore_ascii_case("ACAD_BREAKDATA") && !handles.contains(handle)
            });
            dictionary
                .hard_owner_entries
                .retain(|name| !name.eq_ignore_ascii_case("ACAD_BREAKDATA"));
        }
    }
    true
}

fn write_dimension_break_data(
    document: &mut codec::Document,
    dimension: Handle,
    reserved_reference: Handle,
    references: Vec<codec::objects::BreakPointReference>,
) -> bool {
    use codec::objects::{BreakData, DataObject, DataObjectData, Dictionary, ObjectType};

    if references.is_empty() {
        return remove_dimension_break_data(document, dimension);
    }
    if let Some(handle) = break_object_handle(document, dimension) {
        if let Some(ObjectType::DataObject(object)) = document.objects.get_mut(&handle) {
            object.data = DataObjectData::BreakData(BreakData {
                version: 0,
                dimension_reference: dimension,
                reserved_reference,
                point_references: references,
            });
            return true;
        }
    }

    let dictionary_handle = match document.extension_dictionary_handle(dimension) {
        Some(handle) => handle,
        None => {
            let handle = document.allocate_handle();
            let mut dictionary = Dictionary::new();
            dictionary.handle = handle;
            dictionary.owner = dimension;
            dictionary.hard_owner = true;
            document
                .objects
                .insert(handle, ObjectType::Dictionary(dictionary));
            if let Some(entity) = document.get_entity_mut(dimension) {
                entity.common_mut().xdictionary_handle = Some(handle);
            }
            handle
        }
    };

    let object_handle = document.allocate_handle();
    let mut object = DataObject::new(DataObjectData::BreakData(BreakData {
        version: 0,
        dimension_reference: dimension,
        reserved_reference,
        point_references: references,
    }));
    object.handle = object_handle;
    object.owner = dictionary_handle;
    document
        .objects
        .insert(object_handle, ObjectType::DataObject(object));
    if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&dictionary_handle) {
        dictionary
            .entries
            .retain(|(name, _)| !name.eq_ignore_ascii_case("ACAD_BREAKDATA"));
        dictionary.add_entry("ACAD_BREAKDATA", object_handle);
        dictionary.set_entry_hard_owner("ACAD_BREAKDATA", true);
    }
    true
}

fn break_reference(
    identifier: i32,
    reference_type: i32,
    first: glam::DVec3,
    second: glam::DVec3,
) -> codec::objects::BreakPointReference {
    codec::objects::BreakPointReference {
        version: 0,
        reserved: 0,
        reference_type,
        flags: 0,
        identifier,
        first_point: codec::types::Vector3::new(first.x, first.y, first.z),
        second_point: codec::types::Vector3::new(second.x, second.y, second.z),
        trailing_version: 0,
    }
}

fn apply_dimbreak(
    scene: &mut crate::scene::Scene,
    requested: &[Handle],
    operation: crate::command::DimensionBreakOperation,
) -> DimBreakResult {
    use crate::command::DimensionBreakOperation;

    let dimensions: Vec<_> = requested
        .iter()
        .copied()
        .filter(|handle| {
            matches!(
                scene.document.get_entity(*handle),
                Some(codec::EntityType::Dimension(_))
            ) && scene.locked_layer_name(*handle).is_none()
        })
        .collect();
    if dimensions.is_empty() {
        return DimBreakResult {
            changed: Vec::new(),
            message: crate::t!("DIMBREAK: no editable dimensions were selected.").into_owned(),
        };
    }

    if matches!(operation, DimensionBreakOperation::Remove) {
        let changed: Vec<_> = dimensions
            .into_iter()
            .filter(|handle| remove_dimension_break_data(&mut scene.document, *handle))
            .collect();
        return DimBreakResult {
            message: objects_modified(changed.len()),
            changed,
        };
    }

    let crossing_handles: Vec<Handle> = match &operation {
        DimensionBreakOperation::Objects(handles) => handles.clone(),
        DimensionBreakOperation::Object(handle) => vec![*handle],
        DimensionBreakOperation::Auto => scene
            .document
            .entities()
            .map(|entity| entity.common().handle)
            .filter(|handle| {
                !dimensions.contains(handle) && scene.entity_belongs_to_active_space(*handle)
            })
            .collect(),
        DimensionBreakOperation::Manual(_, _) | DimensionBreakOperation::Remove => Vec::new(),
    };
    let crossing_segments: Vec<_> = crossing_handles
        .iter()
        .flat_map(|handle| wire_segments(&scene.wire_models_for(&[*handle])))
        .collect();

    let mut pending = Vec::new();
    for dimension in &dimensions {
        let existing: Vec<_> = break_object_handle(&scene.document, *dimension)
            .and_then(|handle| scene.document.objects.get(&handle))
            .and_then(|object| match object {
                codec::objects::ObjectType::DataObject(object) => match &object.data {
                    codec::objects::DataObjectData::BreakData(data) => {
                        Some(data.point_references.clone())
                    }
                    _ => None,
                },
                _ => None,
            })
            .unwrap_or_default();
        let mut references = if matches!(operation, DimensionBreakOperation::Manual(_, _)) {
            existing
        } else {
            existing
                .into_iter()
                .filter(|reference| reference.reference_type == 2)
                .collect()
        };
        match operation {
            DimensionBreakOperation::Manual(first, second) => {
                references.push(break_reference(references.len() as i32, 2, first, second));
            }
            DimensionBreakOperation::Auto
            | DimensionBreakOperation::Objects(_)
            | DimensionBreakOperation::Object(_) => {
                let dimension_segments = wire_segments(&scene.wire_models_for(&[*dimension]));
                let mut intersections = Vec::new();
                for dim_segment in &dimension_segments {
                    for crossing_segment in &crossing_segments {
                        if let Some(points) =
                            segment_intersection_xy(*dim_segment, *crossing_segment)
                        {
                            let center = (points.0 + points.1) * 0.5;
                            if intersections.iter().all(
                                |(first, second): &(glam::DVec3, glam::DVec3)| {
                                    center.distance_squared((*first + *second) * 0.5) > 1.0e-8
                                },
                            ) {
                                intersections.push(points);
                            }
                        }
                    }
                }
                let start = references.len() as i32;
                references.extend(
                    intersections
                        .into_iter()
                        .enumerate()
                        .map(|(index, points)| {
                            break_reference(start + index as i32, 1, points.0, points.1)
                        }),
                );
            }
            DimensionBreakOperation::Remove => {}
        }
        pending.push((*dimension, references));
    }

    let reserved = match operation {
        DimensionBreakOperation::Object(handle) => handle,
        DimensionBreakOperation::Objects(handles) => {
            handles.first().copied().unwrap_or(Handle::NULL)
        }
        _ => Handle::NULL,
    };
    let changed: Vec<_> = pending
        .into_iter()
        .filter_map(|(dimension, references)| {
            write_dimension_break_data(&mut scene.document, dimension, reserved, references)
                .then_some(dimension)
        })
        .collect();
    DimBreakResult {
        message: objects_modified(changed.len()),
        changed,
    }
}

/// The count line DIMBREAK ends with.
fn objects_modified(count: usize) -> String {
    if count == 1 {
        crate::t!("1 object modified").into_owned()
    } else {
        crate::tf!("{count} objects modified").into_owned()
    }
}

/// The linear and aligned dimensions of the same space whose dimension line
/// lies on `new`'s and shares part of its length.
pub(super) fn overlapping_dimensions(scene: &crate::scene::Scene, new: Handle) -> Vec<Handle> {
    let Some((axis, _, definition)) = spacing_axis(scene, new) else {
        return Vec::new();
    };
    let length = axis[0].hypot(axis[1]);
    if length <= 0.0 {
        return Vec::new();
    }
    let along = |p: [f64; 3]| (p[0] * axis[0] + p[1] * axis[1]) / length;
    let across = |p: [f64; 3]| (p[1] * axis[0] - p[0] * axis[1]) / length;
    let span = |handle: Handle| {
        let dimension = scene
            .document
            .get_entity(handle)
            .and_then(crate::entities::dimension::as_dimension)?;
        let (a, b) = match dimension {
            codec::entities::Dimension::Linear(d) => (d.first_point, d.second_point),
            codec::entities::Dimension::Aligned(d) => (d.first_point, d.second_point),
            _ => return None,
        };
        let (a, b) = (along([a.x, a.y, a.z]), along([b.x, b.y, b.z]));
        Some((a.min(b), a.max(b)))
    };
    let Some((low, high)) = span(new) else {
        return Vec::new();
    };
    let owner = scene.document.get_entity(new).map(|e| e.common().owner_handle);
    let line = across(definition);
    let tolerance = (high - low).abs().max(1.0) * 1.0e-9;
    scene
        .document
        .entities()
        .map(|entity| entity.common().handle)
        .filter(|&handle| handle != new)
        .filter(|&handle| {
            scene.document.get_entity(handle).map(|e| e.common().owner_handle) == owner
        })
        .filter(|&handle| {
            let Some((other_axis, _, other_definition)) = spacing_axis(scene, handle) else {
                return false;
            };
            let cross = axis[0] * other_axis[1] - axis[1] * other_axis[0];
            let other_length = other_axis[0].hypot(other_axis[1]);
            if other_length <= 0.0 || (cross / (length * other_length)).abs() > 1.0e-9 {
                return false;
            }
            if (across(other_definition) - line).abs() > tolerance {
                return false;
            }
            span(handle).is_some_and(|(a, b)| a.max(low) < b.min(high) - tolerance)
        })
        .collect()
}

/// DIM's answer to an overlap: Move away steps the overlapped dimensions one
/// DIMDLI towards what they measure, Break up breaks them where the new one
/// crosses, Replace erases them, None leaves them.
pub(super) fn apply_dim_overlap(
    scene: &mut crate::scene::Scene,
    choice: char,
    new: Handle,
    others: &[Handle],
    dimdli: f64,
) {
    match choice {
        'M' => {
            let mut changes = Vec::new();
            for &handle in others {
                let Some((axis, normal, definition)) = spacing_axis(scene, handle) else {
                    continue;
                };
                let Some(frame) = kernel::space::dimension_spacing_frame(axis, normal, definition)
                else {
                    continue;
                };
                let p = frame.perpendicular;
                let measured = match scene
                    .document
                    .get_entity(handle)
                    .and_then(crate::entities::dimension::as_dimension)
                {
                    Some(codec::entities::Dimension::Linear(d)) => d.first_point,
                    Some(codec::entities::Dimension::Aligned(d)) => d.first_point,
                    _ => continue,
                };
                let toward = (measured.x - definition[0]) * p[0]
                    + (measured.y - definition[1]) * p[1]
                    + (measured.z - definition[2]) * p[2];
                let target = frame.coordinate + dimdli * toward.signum();
                if let Some(codec::EntityType::Dimension(dim)) =
                    scene.document.get_entity_mut(handle)
                {
                    let slide = |point: &mut codec::types::Vector3| {
                        let moved = kernel::space::move_to_dimension_spacing(
                            [point.x, point.y, point.z],
                            frame,
                            target,
                        );
                        *point = codec::types::Vector3::new(moved[0], moved[1], moved[2]);
                    };
                    match &mut **dim {
                        codec::entities::Dimension::Linear(d) => {
                            slide(&mut d.definition_point);
                            d.base.definition_point = d.definition_point;
                        }
                        codec::entities::Dimension::Aligned(d) => {
                            slide(&mut d.definition_point);
                            d.base.definition_point = d.definition_point;
                        }
                        _ => continue,
                    }
                    scene.invalidate_dim_block_recorded(handle);
                    changes.push((handle, crate::scene::ChangeKind::Modified));
                }
            }
            if !changes.is_empty() {
                scene.bump_entities(&changes);
            }
        }
        'B' => break_up_dimensions(scene, new, others),
        'R' => scene.erase_entities(others),
        _ => {}
    }
}

/// Break up: `new` and the dimensions it lies over become a chain on the same
/// dimension line, split at every extension line origin of them all. Each
/// piece is a new dimension made like `new`; the originals are erased.
fn break_up_dimensions(scene: &mut crate::scene::Scene, new: Handle, others: &[Handle]) {
    use codec::entities::Dimension;
    use codec::types::Vector3;
    let Some((axis, _, definition)) = spacing_axis(scene, new) else {
        return;
    };
    let length = axis[0].hypot(axis[1]);
    let Some(template) = scene.document.get_entity(new).cloned() else {
        return;
    };
    if length <= 0.0 {
        return;
    }
    let u = [axis[0] / length, axis[1] / length];
    let along = |p: Vector3| p.x * u[0] + p.y * u[1];
    let mut points: Vec<Vector3> = std::iter::once(new)
        .chain(others.iter().copied())
        .filter_map(|handle| {
            match scene
                .document
                .get_entity(handle)
                .and_then(crate::entities::dimension::as_dimension)?
            {
                Dimension::Linear(d) => Some([d.first_point, d.second_point]),
                Dimension::Aligned(d) => Some([d.first_point, d.second_point]),
                _ => None,
            }
        })
        .flatten()
        .collect();
    points.sort_by(|a, b| along(*a).total_cmp(&along(*b)));
    let tolerance = match (points.first(), points.last()) {
        (Some(a), Some(b)) => (along(*b) - along(*a)).abs().max(1.0) * 1.0e-9,
        _ => return,
    };
    points.dedup_by(|b, a| (along(*b) - along(*a)).abs() <= tolerance);
    let line = definition[0] * u[0] + definition[1] * u[1];
    let mut pieces = Vec::new();
    for pair in points.windows(2) {
        let mut entity = template.clone();
        let Some(dimension) = crate::entities::dimension::as_dimension_mut(&mut entity) else {
            continue;
        };
        // On the dimension line, across from the piece's second point.
        let shift = along(pair[1]) - line;
        let at = Vector3::new(
            definition[0] + shift * u[0],
            definition[1] + shift * u[1],
            definition[2],
        );
        match dimension {
            Dimension::Linear(d) => {
                (d.first_point, d.second_point, d.definition_point) = (pair[0], pair[1], at);
            }
            Dimension::Aligned(d) => {
                (d.first_point, d.second_point, d.definition_point) = (pair[0], pair[1], at);
            }
            _ => continue,
        }
        crate::entities::dimension::normalize_scripted_dimension(dimension);
        let base = dimension.base_mut();
        base.text_user_positioned = false;
        base.block_name.clear();
        pieces.push(entity);
    }
    let mut erased = vec![new];
    erased.extend_from_slice(others);
    scene.erase_entities(&erased);
    for piece in pieces {
        scene.add_entity_clone(piece);
    }
}

/// The axis, normal and dimension-line point of a linear or aligned dimension.
fn spacing_axis(
    scene: &crate::scene::Scene,
    handle: Handle,
) -> Option<([f64; 3], [f64; 3], [f64; 3])> {
    use codec::entities::Dimension;
    match scene
        .document
        .get_entity(handle)
        .and_then(crate::entities::dimension::as_dimension)?
    {
        Dimension::Linear(d) => Some((
            [d.rotation.cos(), d.rotation.sin(), 0.0],
            [d.base.normal.x, d.base.normal.y, d.base.normal.z],
            [d.definition_point.x, d.definition_point.y, d.definition_point.z],
        )),
        Dimension::Aligned(d) => Some((
            [
                d.second_point.x - d.first_point.x,
                d.second_point.y - d.first_point.y,
                d.second_point.z - d.first_point.z,
            ],
            [d.base.normal.x, d.base.normal.y, d.base.normal.z],
            [d.definition_point.x, d.definition_point.y, d.definition_point.z],
        )),
        _ => None,
    }
}

/// DIM's Distribute. Equal (`offset` None) keeps the outermost two of the
/// dimensions parallel to the first and spaces the rest evenly between them;
/// Offset keeps `base` and steps the others away from it, nearest first, on
/// the side each lies.
fn apply_dim_distribute(
    scene: &mut crate::scene::Scene,
    base: Option<Handle>,
    dimensions: &[Handle],
    offset: Option<f64>,
) -> bool {
    let Some(reference) = base.or_else(|| dimensions.first().copied()) else {
        return false;
    };
    let Some((axis, normal, definition)) = spacing_axis(scene, reference) else {
        return false;
    };
    let Some(frame) = kernel::space::dimension_spacing_frame(axis, normal, definition) else {
        return false;
    };
    let p = frame.perpendicular;
    let coordinate = |point: [f64; 3]| point[0] * p[0] + point[1] * p[1] + point[2] * p[2];
    let parallel = |handle: Handle| {
        spacing_axis(scene, handle).and_then(|(other, _, point)| {
            let cross = axis[0] * other[1] - axis[1] * other[0];
            let length = axis[0].hypot(axis[1]) * other[0].hypot(other[1]);
            (length > 0.0 && (cross / length).abs() < 1.0e-6).then(|| coordinate(point))
        })
    };
    let mut placed: Vec<(Handle, f64)> = dimensions
        .iter()
        .filter(|&&handle| Some(handle) != base)
        .filter_map(|&handle| parallel(handle).map(|at| (handle, at)))
        .collect();
    let mut targets = Vec::new();
    match (base, offset) {
        (Some(base), Some(offset)) => {
            let Some(origin) = parallel(base) else {
                return false;
            };
            placed.sort_by(|a, b| (a.1 - origin).abs().total_cmp(&(b.1 - origin).abs()));
            let (mut above, mut below) = (0.0, 0.0);
            for (handle, at) in placed {
                let step = if at >= origin {
                    above += 1.0;
                    above
                } else {
                    below -= 1.0;
                    below
                };
                targets.push((handle, origin + offset * step));
            }
        }
        _ => {
            if placed.len() < 3 {
                return false;
            }
            placed.sort_by(|a, b| a.1.total_cmp(&b.1));
            let (low, high) = (placed[0].1, placed[placed.len() - 1].1);
            let gap = (high - low) / (placed.len() - 1) as f64;
            for (index, (handle, _)) in placed.iter().enumerate() {
                targets.push((*handle, low + gap * index as f64));
            }
        }
    }
    // The frame measures from its own origin; the coordinates above are
    // plain projections.
    let shift = frame.coordinate - coordinate(definition);
    let mut changes = Vec::new();
    for (handle, target) in targets {
        let target = target + shift;
        if let Some(codec::EntityType::Dimension(dim)) = scene.document.get_entity_mut(handle) {
            let slide = |point: &mut codec::types::Vector3| {
                let moved = kernel::space::move_to_dimension_spacing(
                    [point.x, point.y, point.z],
                    frame,
                    target,
                );
                *point = codec::types::Vector3::new(moved[0], moved[1], moved[2]);
            };
            match &mut **dim {
                codec::entities::Dimension::Linear(d) => {
                    slide(&mut d.definition_point);
                    d.base.definition_point = d.definition_point;
                }
                codec::entities::Dimension::Aligned(d) => {
                    slide(&mut d.definition_point);
                    d.base.definition_point = d.definition_point;
                }
                _ => continue,
            }
            scene.invalidate_dim_block_recorded(handle);
            changes.push((handle, crate::scene::ChangeKind::Modified));
        }
    }
    if !changes.is_empty() {
        scene.bump_entities(&changes);
    }
    !changes.is_empty()
}

fn apply_dimspace(
    scene: &mut crate::scene::Scene,
    base_h: Handle,
    others: &[Handle],
    requested_spacing: Option<f64>,
) -> bool {
    use codec::entities::Dimension;
    let base = scene
        .document
        .get_entity(base_h)
        .and_then(crate::entities::dimension::as_dimension);
    let (axis, normal, definition, auto_spacing) = match base {
        Some(dimension @ Dimension::Linear(d)) => {
            let spacing = dimension_auto_spacing(
                &scene.document,
                dimension,
                scene.creation_annotation_multiplier(),
            );
            (
                [d.rotation.cos(), d.rotation.sin(), 0.0],
                [d.base.normal.x, d.base.normal.y, d.base.normal.z],
                [
                    d.definition_point.x,
                    d.definition_point.y,
                    d.definition_point.z,
                ],
                spacing,
            )
        }
        Some(dimension @ Dimension::Aligned(d)) => {
            let spacing = dimension_auto_spacing(
                &scene.document,
                dimension,
                scene.creation_annotation_multiplier(),
            );
            (
                [
                    d.second_point.x - d.first_point.x,
                    d.second_point.y - d.first_point.y,
                    d.second_point.z - d.first_point.z,
                ],
                [d.base.normal.x, d.base.normal.y, d.base.normal.z],
                [
                    d.definition_point.x,
                    d.definition_point.y,
                    d.definition_point.z,
                ],
                spacing,
            )
        }
        _ => return false,
    };
    let Some(frame) = kernel::space::dimension_spacing_frame(axis, normal, definition) else {
        return false;
    };

    let effective_spacing = requested_spacing.unwrap_or(auto_spacing);
    let mut changes = Vec::new();
    for (idx, &h) in others.iter().enumerate() {
        let target = frame.coordinate + effective_spacing * (idx + 1) as f64;
        let mut changed = false;
        if let Some(codec::EntityType::Dimension(dim)) = scene.document.get_entity_mut(h) {
            let slide = |p: &mut codec::types::Vector3| {
                let point =
                    kernel::space::move_to_dimension_spacing([p.x, p.y, p.z], frame, target);
                *p = codec::types::Vector3::new(point[0], point[1], point[2]);
            };
            match &mut **dim {
                Dimension::Linear(d) => {
                    slide(&mut d.definition_point);
                    d.base.definition_point = d.definition_point;
                    changed = true;
                }
                Dimension::Aligned(d) => {
                    slide(&mut d.definition_point);
                    d.base.definition_point = d.definition_point;
                    changed = true;
                }
                _ => {}
            }
        }
        if changed {
            // The dimension line moved, so its baked *D block is stale — drop it
            // so the next save re-bakes it. (#181)
            scene.invalidate_dim_block_recorded(h);
            changes.push((h, crate::scene::ChangeKind::Modified));
        }
    }
    if !changes.is_empty() {
        scene.bump_entities(&changes);
    }
    !changes.is_empty()
}

fn dimension_auto_spacing(
    document: &codec::Document,
    dimension: &codec::entities::Dimension,
    annotation_multiplier: f64,
) -> f64 {
    let style = document
        .dim_styles
        .iter()
        .find(|style| {
            style
                .name
                .eq_ignore_ascii_case(&dimension.base().style_name)
        })
        .map(|style| {
            crate::entities::dimension::resolved_dimension_style(style, dimension, document)
        });
    style
        .map(|style| {
            let scale = if style.dimscale > 1.0e-9 {
                style.dimscale
            } else {
                annotation_multiplier.max(1.0e-9)
            };
            (style.dimdli.abs() * scale).max(1.0e-6)
        })
        .unwrap_or(3.75)
}
