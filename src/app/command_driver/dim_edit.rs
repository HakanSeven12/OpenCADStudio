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

    pub(super) fn handle_edit_dimension_jog(&mut self, dimension: Handle, point: Option<glam::DVec3>) {
        let i = self.active_tab;
        let valid = matches!(
            self.tabs[i].scene.document.get_entity(dimension),
            Some(acadrust::EntityType::Dimension(
                acadrust::entities::Dimension::Linear(_)
                    | acadrust::entities::Dimension::Aligned(_)
            ))
        );
        if !valid {
            self.command_line.push_error(
                crate::t!("DIMJOGLINE: select a linear or aligned dimension.").as_ref(),
            );
        } else if !self.reject_locked_edit(i, dimension) {
            self.push_undo_snapshot(i, "DIMJOGLINE");
            let values = point.map(|point| {
                use acadrust::xdata::XDataValue;
                vec![
                    XDataValue::Integer16(387),
                    XDataValue::Integer16(3),
                    XDataValue::Integer16(389),
                    XDataValue::Point3D(acadrust::types::Vector3::new(
                        point.x, point.y, point.z,
                    )),
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

    pub(super) fn handle_space_dimensions(&mut self, base: Handle, others: Vec<Handle>, spacing: Option<f64>) {
        let i = self.active_tab;
        self.push_undo_snapshot(i, "DIMSPACE");
        if apply_dimspace(&mut self.tabs[i].scene, base, &others, spacing) {
            self.tabs[i].dirty = true;
            self.command_line
                .push_output(crate::t!("DIMSPACE  Spacing adjusted.").as_ref());
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
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
    let gap = cadkernel::space::segment_break_gap_xy(
        [first.0.to_array(), first.1.to_array()],
        [second.0.to_array(), second.1.to_array()],
        0.25,
    )?;
    Some((
        glam::DVec3::from_array(gap[0]),
        glam::DVec3::from_array(gap[1]),
    ))
}

fn break_object_handle(document: &acadrust::CadDocument, dimension: Handle) -> Option<Handle> {
    document.objects.iter().find_map(|(handle, object)| {
        let acadrust::objects::ObjectType::DataObject(object) = object else {
            return None;
        };
        let acadrust::objects::DataObjectData::BreakData(data) = &object.data else {
            return None;
        };
        (data.dimension_reference == dimension).then_some(*handle)
    })
}

fn remove_dimension_break_data(document: &mut acadrust::CadDocument, dimension: Handle) -> bool {
    let handles: Vec<_> = document
        .objects
        .iter()
        .filter_map(|(handle, object)| match object {
            acadrust::objects::ObjectType::DataObject(object) => match &object.data {
                acadrust::objects::DataObjectData::BreakData(data)
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
        if let Some(acadrust::objects::ObjectType::Dictionary(dictionary)) =
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
    document: &mut acadrust::CadDocument,
    dimension: Handle,
    reserved_reference: Handle,
    references: Vec<acadrust::objects::BreakPointReference>,
) -> bool {
    use acadrust::objects::{BreakData, DataObject, DataObjectData, Dictionary, ObjectType};

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
) -> acadrust::objects::BreakPointReference {
    acadrust::objects::BreakPointReference {
        version: 0,
        reserved: 0,
        reference_type,
        flags: 0,
        identifier,
        first_point: acadrust::types::Vector3::new(first.x, first.y, first.z),
        second_point: acadrust::types::Vector3::new(second.x, second.y, second.z),
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
                Some(acadrust::EntityType::Dimension(_))
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
            message: crate::tf!(
                "DIMBREAK: removed breaks from {} dimension(s).",
                changed.len()
            )
            .into_owned(),
            changed,
        };
    }

    let crossing_handles: Vec<Handle> = match operation {
        DimensionBreakOperation::Object(handle) => vec![handle],
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
                acadrust::objects::ObjectType::DataObject(object) => match &object.data {
                    acadrust::objects::DataObjectData::BreakData(data) => {
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
            DimensionBreakOperation::Auto | DimensionBreakOperation::Object(_) => {
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
        message: if changed.is_empty() {
            crate::t!("DIMBREAK: no intersections were found.").into_owned()
        } else {
            crate::tf!("DIMBREAK: updated {} dimension(s).", changed.len()).into_owned()
        },
        changed,
    }
}

fn apply_dimspace(
    scene: &mut crate::scene::Scene,
    base_h: Handle,
    others: &[Handle],
    requested_spacing: Option<f64>,
) -> bool {
    use acadrust::entities::Dimension;
    let (axis, normal, definition, auto_spacing) = match scene.document.get_entity(base_h) {
        Some(acadrust::EntityType::Dimension(dimension @ Dimension::Linear(d))) => {
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
        Some(acadrust::EntityType::Dimension(dimension @ Dimension::Aligned(d))) => {
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
    let Some(frame) = cadkernel::space::dimension_spacing_frame(axis, normal, definition) else {
        return false;
    };

    let effective_spacing = requested_spacing.unwrap_or(auto_spacing);
    let mut changes = Vec::new();
    for (idx, &h) in others.iter().enumerate() {
        let target = frame.coordinate + effective_spacing * (idx + 1) as f64;
        let mut changed = false;
        if let Some(acadrust::EntityType::Dimension(dim)) = scene.document.get_entity_mut(h) {
            let slide = |p: &mut acadrust::types::Vector3| {
                let point =
                    cadkernel::space::move_to_dimension_spacing([p.x, p.y, p.z], frame, target);
                *p = acadrust::types::Vector3::new(point[0], point[1], point[2]);
            };
            match dim {
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
    document: &acadrust::CadDocument,
    dimension: &acadrust::entities::Dimension,
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
