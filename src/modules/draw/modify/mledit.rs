use acadrust::entities::MLine;
use acadrust::objects::MLineStyle;
use acadrust::{EntityType, Handle};
use cadkernel::geom2d::{Curve, Line};
use cadkernel::space::{PlanarCurve, Plane, Vec3 as KernelVec3};
use glam::DVec3;
use rustc_hash::FxHashMap as HashMap;

use crate::command::{CadCommand, CmdOption, CmdResult};

#[derive(Clone)]
pub struct MlineEditTarget {
    pub entity: MLine,
    pub style: MLineStyle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    ClosedCross,
    OpenCross,
    MergedCross,
    ClosedTee,
    OpenTee,
    MergedTee,
    CornerJoint,
    AddVertex,
    DeleteVertex,
    CutSingle,
    CutAll,
    WeldAll,
}

enum Mode {
    Choose,
    PickFirst(Tool),
    PickSecond {
        tool: Tool,
        first: Handle,
        first_pick: DVec3,
    },
    PickRangeEnd {
        tool: Tool,
        target: Handle,
        start: DVec3,
    },
}

pub struct MlineEditCommand {
    targets: HashMap<u64, MlineEditTarget>,
    mode: Mode,
}

impl MlineEditCommand {
    pub fn new(targets: HashMap<u64, MlineEditTarget>) -> Self {
        Self {
            targets,
            mode: Mode::Choose,
        }
    }

    fn target(&self, handle: Handle) -> Option<&MlineEditTarget> {
        self.targets.get(&handle.value())
    }

    fn replace(handle: Handle, mline: MLine) -> CmdResult {
        CmdResult::ReplaceMany(
            vec![(handle, vec![EntityType::MLine(mline)])],
            Vec::new(),
        )
    }

    fn edit_vertex(&self, tool: Tool, handle: Handle, point: DVec3) -> Option<CmdResult> {
        let target = self.target(handle)?;
        let mut mline = target.entity.clone();
        match tool {
            Tool::AddVertex => {
                let (segment, fraction, projected, _) = closest_segment(&mline, point)?;
                if !(1.0e-9..1.0 - 1.0e-9).contains(&fraction) {
                    return None;
                }
                let normal = DVec3::new(mline.normal.x, mline.normal.y, mline.normal.z);
                let source: Vec<_> = (0..mline.style_element_count)
                    .filter_map(|element| {
                        let (start, end) = element_segment(&mline, segment, element)?;
                        let data = mline.vertices[segment].segments.get(element)?.clone();
                        let curve = segment_curve(start, end, normal)?;
                        let length = curve.length();
                        let split = curve.parameter_at(projected.to_array())?.clamp(0.0, 1.0)
                            * length;
                        Some((data, length, split))
                    })
                    .collect();
                if source.len() != mline.style_element_count {
                    return None;
                }
                let insert = segment + 1;
                let mut vertex = mline.vertices[segment].clone();
                vertex.position = acadrust::types::Vector3::new(
                    projected.x,
                    projected.y,
                    projected.z,
                );
                mline.vertices.insert(insert, vertex);
                crate::entities::mline::rebuild_mline_geometry(&mut mline);
                crate::modules::draw::draw::mline::sync_mline_element_parameters(
                    &mut mline,
                    &target.style,
                );
                let lengths: Vec<_> = (0..mline.style_element_count)
                    .filter_map(|element| {
                        let first = element_segment(&mline, segment, element)?;
                        let second = element_segment(&mline, insert, element)?;
                        Some((
                            segment_curve(first.0, first.1, normal)?.length(),
                            segment_curve(second.0, second.1, normal)?.length(),
                        ))
                    })
                    .collect();
                if lengths.len() != mline.style_element_count {
                    return None;
                }
                for (element, ((source, source_length, split), (first_length, second_length))) in
                    source.into_iter().zip(lengths).enumerate()
                {
                    let first_drawn = remap_ranges(
                        &drawn_ranges(&source.parameters, source_length),
                        0.0,
                        split,
                        first_length,
                    );
                    let second_drawn = remap_ranges(
                        &drawn_ranges(&source.parameters, source_length),
                        split,
                        source_length,
                        second_length,
                    );
                    let first_cuts = remap_ranges(
                        &cut_ranges(&source.area_fill_parameters, source_length),
                        0.0,
                        split,
                        first_length,
                    );
                    let second_cuts = remap_ranges(
                        &cut_ranges(&source.area_fill_parameters, source_length),
                        split,
                        source_length,
                        second_length,
                    );
                    let (before, after) = mline.vertices.split_at_mut(insert);
                    store_drawn_ranges(
                        &mut before[segment].segments[element].parameters,
                        first_length,
                        &first_drawn,
                    );
                    store_cut_ranges(
                        &mut before[segment].segments[element].area_fill_parameters,
                        first_length,
                        &first_cuts,
                    );
                    store_drawn_ranges(
                        &mut after[0].segments[element].parameters,
                        second_length,
                        &second_drawn,
                    );
                    store_cut_ranges(
                        &mut after[0].segments[element].area_fill_parameters,
                        second_length,
                        &second_cuts,
                    );
                }
            }
            Tool::DeleteVertex => {
                let vertex = closest_vertex(&mline, point)?;
                if mline.vertices.len() <= 2
                    || (mline.is_closed() && mline.vertices.len() <= 3)
                {
                    return None;
                }
                let count = mline.vertices.len();
                let merge = mline.is_closed() || (vertex > 0 && vertex + 1 < count);
                let previous = if vertex == 0 { count - 1 } else { vertex - 1 };
                let normal = DVec3::new(mline.normal.x, mline.normal.y, mline.normal.z);
                let source = merge.then(|| {
                    (0..mline.style_element_count)
                        .filter_map(|element| {
                            let first = element_segment(&mline, previous, element)?;
                            let second = element_segment(&mline, vertex, element)?;
                            Some((
                                mline.vertices[previous].segments.get(element)?.clone(),
                                segment_curve(first.0, first.1, normal)?.length(),
                                mline.vertices[vertex].segments.get(element)?.clone(),
                                segment_curve(second.0, second.1, normal)?.length(),
                            ))
                        })
                        .collect::<Vec<_>>()
                });
                if source
                    .as_ref()
                    .is_some_and(|source| source.len() != mline.style_element_count)
                {
                    return None;
                }
                let candidates = if mline.is_closed() {
                    vec![
                        ((previous + count - 1) % count, false),
                        ((vertex + 1) % count, true),
                    ]
                } else {
                    let mut candidates = Vec::new();
                    if vertex >= 2 {
                        candidates.push((vertex - 2, false));
                    }
                    if vertex + 2 < count {
                        candidates.push((vertex + 1, true));
                    }
                    candidates
                };
                let mut peripheral = Vec::new();
                for (segment, moved_start) in candidates {
                    if (merge && (segment == previous || segment == vertex))
                        || peripheral
                            .iter()
                            .any(|(existing, _, _)| *existing == segment)
                    {
                        continue;
                    }
                    let snapshot = snapshot_segment_data(&mline, segment, normal)?;
                    let target = if segment > vertex {
                        segment - 1
                    } else {
                        segment
                    };
                    peripheral.push((target, moved_start, snapshot));
                }
                mline.vertices.remove(vertex);
                crate::entities::mline::rebuild_mline_geometry(&mut mline);
                crate::modules::draw::draw::mline::sync_mline_element_parameters(
                    &mut mline,
                    &target.style,
                );
                for (segment, moved_start, snapshot) in peripheral {
                    restore_shifted_segment_data(
                        &mut mline,
                        segment,
                        normal,
                        snapshot,
                        moved_start,
                    );
                }
                if let Some(source) = source {
                    let previous = if vertex == 0 {
                        mline.vertices.len() - 1
                    } else {
                        vertex - 1
                    };
                    for (element, (first, first_length, second, second_length)) in
                        source.into_iter().enumerate()
                    {
                        let segment = element_segment(&mline, previous, element)?;
                        let target_length = segment_curve(segment.0, segment.1, normal)?.length();
                        let total = first_length + second_length;
                        let mut drawn = drawn_ranges(&first.parameters, first_length);
                        drawn.extend(
                            drawn_ranges(&second.parameters, second_length)
                                .into_iter()
                                .map(|range| {
                                    (range.0 + first_length, range.1 + first_length)
                                }),
                        );
                        let mut cuts = cut_ranges(&first.area_fill_parameters, first_length);
                        cuts.extend(
                            cut_ranges(&second.area_fill_parameters, second_length)
                                .into_iter()
                                .map(|range| {
                                    (range.0 + first_length, range.1 + first_length)
                                }),
                        );
                        let drawn = merge_ranges(remap_ranges(
                            &drawn,
                            0.0,
                            total,
                            target_length,
                        ));
                        let cuts = merge_ranges(remap_ranges(
                            &cuts,
                            0.0,
                            total,
                            target_length,
                        ));
                        let target = &mut mline.vertices[previous].segments[element];
                        store_drawn_ranges(&mut target.parameters, target_length, &drawn);
                        store_cut_ranges(
                            &mut target.area_fill_parameters,
                            target_length,
                            &cuts,
                        );
                    }
                }
            }
            _ => return None,
        }
        crate::modules::draw::draw::mline::sync_mline_element_parameters(
            &mut mline,
            &target.style,
        );
        Some(Self::replace(handle, mline))
    }

    fn edit_range(
        &self,
        tool: Tool,
        handle: Handle,
        start: DVec3,
        end: DVec3,
    ) -> Option<CmdResult> {
        let target = self.target(handle)?;
        let mut mline = target.entity.clone();
        let (segment, _, _, _) = closest_segment(&mline, start)?;
        let elements: Vec<usize> = match tool {
            Tool::CutSingle => vec![closest_element(&mline, segment, start)?],
            _ => (0..mline.style_element_count).collect(),
        };
        for element in elements {
            let (a, b) = element_segment(&mline, segment, element)?;
            let curve = segment_curve(
                a,
                b,
                DVec3::new(mline.normal.x, mline.normal.y, mline.normal.z),
            )?;
            let length = curve.length();
            if length <= 1.0e-10 {
                continue;
            }
            let first = curve.parameter_at(start.to_array())?.clamp(0.0, 1.0) * length;
            let second = curve.parameter_at(end.to_array())?.clamp(0.0, 1.0) * length;
            let low = first.min(second);
            let high = first.max(second);
            let segment = &mut mline.vertices[segment].segments[element];
            if tool == Tool::WeldAll {
                add_drawn_range(&mut segment.parameters, length, low, high);
                remove_cut_range(&mut segment.area_fill_parameters, length, low, high);
            } else {
                remove_drawn_range(&mut segment.parameters, length, low, high);
                add_cut_range(&mut segment.area_fill_parameters, length, low, high);
            }
        }
        Some(Self::replace(handle, mline))
    }

    fn edit_pair(
        &self,
        tool: Tool,
        first_handle: Handle,
        first_pick: DVec3,
        second_handle: Handle,
        second_pick: DVec3,
    ) -> Option<CmdResult> {
        if first_handle == second_handle {
            return None;
        }
        let first_target = self.target(first_handle)?;
        let second_target = self.target(second_handle)?;
        let mut first = first_target.entity.clone();
        let mut second = second_target.entity.clone();
        let (first_segment, _, _, _) = closest_segment(&first, first_pick)?;
        let (second_segment, _, _, _) = closest_segment(&second, second_pick)?;
        let first_end = if matches!(
            tool,
            Tool::ClosedTee | Tool::OpenTee | Tool::MergedTee | Tool::CornerJoint
        ) {
            Some(selected_terminal_end(&first, first_segment, first_pick)?)
        } else {
            None
        };
        let second_end = if tool == Tool::CornerJoint {
            Some(selected_terminal_end(&second, second_segment, second_pick)?)
        } else {
            None
        };
        let (intersection, first_fraction, second_fraction, sine) = segment_intersection(
            center_segment(&first, first_segment)?,
            center_segment(&second, second_segment)?,
            DVec3::new(first.normal.x, first.normal.y, first.normal.z),
        )?;
        let on_segment = |value: f64| (-1.0e-6..=1.0 + 1.0e-6).contains(&value);
        let valid = match tool {
            Tool::ClosedCross | Tool::OpenCross | Tool::MergedCross => {
                on_segment(first_fraction) && on_segment(second_fraction)
            }
            Tool::ClosedTee | Tool::OpenTee | Tool::MergedTee => {
                on_segment(second_fraction)
            }
            Tool::CornerJoint => true,
            _ => false,
        };
        if !valid || sine.abs() <= 1.0e-6 {
            return None;
        }
        let divisor = sine.abs();
        let first_gap = style_width(&second_target.style, second.scale_factor) * 0.5 / divisor;
        let second_gap = style_width(&first_target.style, first.scale_factor) * 0.5 / divisor;

        match tool {
            Tool::ClosedCross => {
                gap_elements(
                    &mut first,
                    first_segment,
                    intersection,
                    first_gap,
                    None,
                );
            }
            Tool::OpenCross => {
                gap_elements(
                    &mut first,
                    first_segment,
                    intersection,
                    first_gap,
                    None,
                );
                gap_elements(
                    &mut second,
                    second_segment,
                    intersection,
                    second_gap,
                    Some(outer_element_indices(&second_target.style)),
                );
            }
            Tool::MergedCross => {
                gap_elements(
                    &mut first,
                    first_segment,
                    intersection,
                    first_gap,
                    Some(inner_element_indices(&first_target.style)),
                );
                gap_elements(
                    &mut second,
                    second_segment,
                    intersection,
                    second_gap,
                    Some(inner_element_indices(&second_target.style)),
                );
            }
            Tool::ClosedTee | Tool::OpenTee | Tool::MergedTee => {
                move_closest_end(
                    &mut first,
                    &first_target.style,
                    first_end?,
                    intersection,
                );
                let elements = match tool {
                    Tool::ClosedTee => None,
                    Tool::OpenTee => Some(outer_element_indices(&second_target.style)),
                    Tool::MergedTee => Some(inner_element_indices(&second_target.style)),
                    _ => unreachable!(),
                };
                gap_elements(
                    &mut second,
                    second_segment,
                    intersection,
                    second_gap,
                    elements,
                );
            }
            Tool::CornerJoint => {
                move_closest_end(
                    &mut first,
                    &first_target.style,
                    first_end?,
                    intersection,
                );
                move_closest_end(
                    &mut second,
                    &second_target.style,
                    second_end?,
                    intersection,
                );
            }
            _ => return None,
        }

        Some(CmdResult::ReplaceMany(
            vec![
                (first_handle, vec![EntityType::MLine(first)]),
                (second_handle, vec![EntityType::MLine(second)]),
            ],
            Vec::new(),
        ))
    }
}

impl CadCommand for MlineEditCommand {
    fn name(&self) -> &'static str {
        "MLEDIT"
    }

    fn prompt(&self) -> String {
        match self.mode {
            Mode::Choose => crate::t!("MLEDIT  Choose an edit tool:").into_owned(),
            Mode::PickFirst(_) => crate::t!("MLEDIT  Select first multiline:").into_owned(),
            Mode::PickSecond { .. } => {
                crate::t!("MLEDIT  Select second multiline:").into_owned()
            }
            Mode::PickRangeEnd { tool: Tool::WeldAll, .. } => {
                crate::t!("MLEDIT  Specify the end of the weld range:").into_owned()
            }
            Mode::PickRangeEnd { .. } => {
                crate::t!("MLEDIT  Specify the second cut point:").into_owned()
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if !matches!(self.mode, Mode::Choose) {
            return Vec::new();
        }
        vec![
            CmdOption::new(crate::t!("Closed Cross").as_ref(), "CC"),
            CmdOption::new(crate::t!("Open Cross").as_ref(), "OC"),
            CmdOption::new(crate::t!("Merged Cross").as_ref(), "MC"),
            CmdOption::new(crate::t!("Closed Tee").as_ref(), "CT"),
            CmdOption::new(crate::t!("Open Tee").as_ref(), "OT"),
            CmdOption::new(crate::t!("Merged Tee").as_ref(), "MT"),
            CmdOption::new(crate::t!("Corner Joint").as_ref(), "CJ"),
            CmdOption::new(crate::t!("Add Vertex").as_ref(), "AV"),
            CmdOption::new(crate::t!("Delete Vertex").as_ref(), "DV"),
            CmdOption::new(crate::t!("Cut Single").as_ref(), "CS"),
            CmdOption::new(crate::t!("Cut All").as_ref(), "CA"),
            CmdOption::new(crate::t!("Weld All").as_ref(), "WA"),
        ]
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.mode, Mode::Choose)
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.mode, Mode::PickFirst(_) | Mode::PickSecond { .. })
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        self.needs_entity_pick()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if !matches!(self.mode, Mode::Choose) {
            return None;
        }
        let tool = match text.trim().to_uppercase().as_str() {
            "CC" | "CLOSED CROSS" => Tool::ClosedCross,
            "OC" | "OPEN CROSS" => Tool::OpenCross,
            "MC" | "MERGED CROSS" => Tool::MergedCross,
            "CT" | "CLOSED TEE" => Tool::ClosedTee,
            "OT" | "OPEN TEE" => Tool::OpenTee,
            "MT" | "MERGED TEE" => Tool::MergedTee,
            "CJ" | "CORNER JOINT" => Tool::CornerJoint,
            "AV" | "ADD VERTEX" => Tool::AddVertex,
            "DV" | "DELETE VERTEX" => Tool::DeleteVertex,
            "CS" | "CUT SINGLE" => Tool::CutSingle,
            "CA" | "CUT ALL" => Tool::CutAll,
            "WA" | "WELD ALL" => Tool::WeldAll,
            _ => return None,
        };
        self.mode = Mode::PickFirst(tool);
        Some(CmdResult::NeedPoint)
    }

    fn on_entity_pick(&mut self, handle: Handle, point: DVec3) -> CmdResult {
        if self.target(handle).is_none() {
            return CmdResult::NeedPoint;
        }
        match self.mode {
            Mode::PickFirst(tool @ (Tool::AddVertex | Tool::DeleteVertex)) => self
                .edit_vertex(tool, handle, point)
                .unwrap_or(CmdResult::NeedPoint),
            Mode::PickFirst(tool @ (Tool::CutSingle | Tool::CutAll | Tool::WeldAll)) => {
                self.mode = Mode::PickRangeEnd {
                    tool,
                    target: handle,
                    start: point,
                };
                CmdResult::NeedPoint
            }
            Mode::PickFirst(tool) => {
                self.mode = Mode::PickSecond {
                    tool,
                    first: handle,
                    first_pick: point,
                };
                CmdResult::NeedPoint
            }
            Mode::PickSecond {
                tool,
                first,
                first_pick,
            } => self
                .edit_pair(tool, first, first_pick, handle, point)
                .unwrap_or(CmdResult::NeedPoint),
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        match self.mode {
            Mode::PickRangeEnd {
                tool,
                target,
                start,
            } => self
                .edit_range(tool, target, start, point)
                .unwrap_or(CmdResult::NeedPoint),
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

fn center_segment(mline: &MLine, index: usize) -> Option<(DVec3, DVec3)> {
    let first = mline.vertices.get(index)?.position;
    let next = if index + 1 < mline.vertices.len() {
        index + 1
    } else if mline.is_closed() {
        0
    } else {
        return None;
    };
    let second = mline.vertices[next].position;
    Some((
        DVec3::new(first.x, first.y, first.z),
        DVec3::new(second.x, second.y, second.z),
    ))
}

fn segment_curve(first: DVec3, second: DVec3, normal: DVec3) -> Option<PlanarCurve> {
    let direction = KernelVec3::from(second.to_array()) - KernelVec3::from(first.to_array());
    let length = direction.length();
    let plane = Plane::orthonormal(
        first.to_array(),
        direction.normalize()?.to_array(),
        normal.to_array(),
    )?;
    Some(PlanarCurve::new(
        plane,
        Curve::Line(Line {
            start: [0.0, 0.0],
            end: [length, 0.0],
        }),
    ))
}

fn closest_segment(mline: &MLine, point: DVec3) -> Option<(usize, f64, DVec3, f64)> {
    let count = if mline.is_closed() {
        mline.vertices.len()
    } else {
        mline.vertices.len().saturating_sub(1)
    };
    let normal = DVec3::new(mline.normal.x, mline.normal.y, mline.normal.z);
    (0..count)
        .filter_map(|index| {
            let (a, b) = center_segment(mline, index)?;
            let curve = segment_curve(a, b, normal)?;
            let fraction = curve.parameter_at(point.to_array())?.clamp(0.0, 1.0);
            let projected = DVec3::from_array(curve.point_at(fraction));
            let distance = KernelVec3::from(projected.to_array())
                .distance_squared(KernelVec3::from(point.to_array()));
            Some((index, fraction, projected, distance))
        })
        .min_by(|left, right| left.3.total_cmp(&right.3))
}

fn closest_vertex(mline: &MLine, point: DVec3) -> Option<usize> {
    mline
        .vertices
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| {
            let point = KernelVec3::from(point.to_array());
            let left = KernelVec3::new(left.position.x, left.position.y, left.position.z)
                .distance_squared(point);
            let right = KernelVec3::new(right.position.x, right.position.y, right.position.z)
                .distance_squared(point);
            left.total_cmp(&right)
        })
        .map(|(index, _)| index)
}

fn element_segment(mline: &MLine, index: usize, element: usize) -> Option<(DVec3, DVec3)> {
    let next = if index + 1 < mline.vertices.len() {
        index + 1
    } else if mline.is_closed() {
        0
    } else {
        return None;
    };
    let point = |vertex: usize| -> Option<DVec3> {
        let item = mline.vertices.get(vertex)?;
        let offset = item.segments.get(element)?.parameters.first().copied()?;
        let position = KernelVec3::new(item.position.x, item.position.y, item.position.z);
        let miter = KernelVec3::new(item.miter.x, item.miter.y, item.miter.z);
        Some(DVec3::from_array((position + miter * offset).to_array()))
    };
    Some((point(index)?, point(next)?))
}

fn snapshot_segment_data(
    mline: &MLine,
    segment: usize,
    normal: DVec3,
) -> Option<Vec<(acadrust::entities::MLineSegment, f64)>> {
    (0..mline.style_element_count)
        .map(|element| {
            let endpoints = element_segment(mline, segment, element)?;
            Some((
                mline.vertices.get(segment)?.segments.get(element)?.clone(),
                segment_curve(endpoints.0, endpoints.1, normal)?.length(),
            ))
        })
        .collect()
}

fn restore_shifted_segment_data(
    mline: &mut MLine,
    segment: usize,
    normal: DVec3,
    source: Vec<(acadrust::entities::MLineSegment, f64)>,
    moved_start: bool,
) {
    for (element, (source, source_length)) in source.into_iter().enumerate() {
        let Some(endpoints) = element_segment(mline, segment, element) else {
            continue;
        };
        let Some(target_length) = segment_curve(endpoints.0, endpoints.1, normal)
            .map(|curve| curve.length())
        else {
            continue;
        };
        let shift = if moved_start {
            target_length - source_length
        } else {
            0.0
        };
        let drawn = shift_ranges(
            &drawn_ranges(&source.parameters, source_length),
            source_length,
            shift,
            target_length,
        );
        let cuts = shift_ranges(
            &cut_ranges(&source.area_fill_parameters, source_length),
            source_length,
            shift,
            target_length,
        );
        let Some(target) = mline
            .vertices
            .get_mut(segment)
            .and_then(|vertex| vertex.segments.get_mut(element))
        else {
            continue;
        };
        store_drawn_ranges(&mut target.parameters, target_length, &drawn);
        store_cut_ranges(&mut target.area_fill_parameters, target_length, &cuts);
    }
}

fn closest_element(mline: &MLine, segment: usize, point: DVec3) -> Option<usize> {
    let normal = DVec3::new(mline.normal.x, mline.normal.y, mline.normal.z);
    (0..mline.style_element_count)
        .filter_map(|element| {
            let (a, b) = element_segment(mline, segment, element)?;
            let curve = segment_curve(a, b, normal)?;
            let fraction = curve.parameter_at(point.to_array())?.clamp(0.0, 1.0);
            let projected = KernelVec3::from(curve.point_at(fraction));
            Some((
                element,
                projected.distance_squared(KernelVec3::from(point.to_array())),
            ))
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(element, _)| element)
}

fn drawn_ranges(parameters: &[f64], length: f64) -> Vec<(f64, f64)> {
    crate::entities::mline::mline_drawn_ranges(parameters, length)
}

fn store_drawn_ranges(parameters: &mut Vec<f64>, length: f64, ranges: &[(f64, f64)]) {
    crate::entities::mline::store_mline_drawn_ranges(parameters, length, ranges);
}

fn remove_drawn_range(parameters: &mut Vec<f64>, length: f64, low: f64, high: f64) {
    if high - low <= 1.0e-9 {
        return;
    }
    let mut result = Vec::new();
    for (start, end) in drawn_ranges(parameters, length) {
        if low > start + 1.0e-9 {
            result.push((start, low.min(end)));
        }
        if high < end - 1.0e-9 {
            result.push((high.max(start), end));
        }
    }
    store_drawn_ranges(parameters, length, &result);
}

fn add_drawn_range(parameters: &mut Vec<f64>, length: f64, low: f64, high: f64) {
    let mut ranges = drawn_ranges(parameters, length);
    ranges.push((low, high));
    ranges.sort_by(|left, right| left.0.total_cmp(&right.0));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for range in ranges {
        if let Some(last) = merged.last_mut() {
            if range.0 <= last.1 + 1.0e-9 {
                last.1 = last.1.max(range.1);
                continue;
            }
        }
        merged.push(range);
    }
    store_drawn_ranges(parameters, length, &merged);
}

fn cut_ranges(parameters: &[f64], length: f64) -> Vec<(f64, f64)> {
    parameters
        .chunks(2)
        .filter_map(|pair| {
            let start = pair[0].clamp(0.0, length);
            let end = pair.get(1).copied().unwrap_or(length).clamp(0.0, length);
            (end - start > 1.0e-9).then_some((start, end))
        })
        .collect()
}

fn store_cut_ranges(parameters: &mut Vec<f64>, length: f64, ranges: &[(f64, f64)]) {
    parameters.clear();
    for (start, end) in ranges {
        parameters.push(*start);
        if *end < length - 1.0e-9 {
            parameters.push(*end);
        }
    }
}

fn add_cut_range(parameters: &mut Vec<f64>, length: f64, low: f64, high: f64) {
    if high - low <= 1.0e-9 {
        return;
    }
    let mut ranges = cut_ranges(parameters, length);
    ranges.push((low, high));
    ranges.sort_by(|left, right| left.0.total_cmp(&right.0));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for range in ranges {
        if let Some(last) = merged.last_mut() {
            if range.0 <= last.1 + 1.0e-9 {
                last.1 = last.1.max(range.1);
                continue;
            }
        }
        merged.push(range);
    }
    store_cut_ranges(parameters, length, &merged);
}

fn remove_cut_range(parameters: &mut Vec<f64>, length: f64, low: f64, high: f64) {
    if high - low <= 1.0e-9 {
        return;
    }
    let mut result = Vec::new();
    for (start, end) in cut_ranges(parameters, length) {
        if low > start + 1.0e-9 {
            result.push((start, low.min(end)));
        }
        if high < end - 1.0e-9 {
            result.push((high.max(start), end));
        }
    }
    store_cut_ranges(parameters, length, &result);
}

fn remap_ranges(
    ranges: &[(f64, f64)],
    source_start: f64,
    source_end: f64,
    target_length: f64,
) -> Vec<(f64, f64)> {
    let source_length = source_end - source_start;
    if source_length <= 1.0e-12 || target_length <= 1.0e-12 {
        return Vec::new();
    }
    ranges
        .iter()
        .filter_map(|range| {
            let start = range.0.max(source_start);
            let end = range.1.min(source_end);
            (end - start > 1.0e-9).then_some((
                (start - source_start) / source_length * target_length,
                (end - source_start) / source_length * target_length,
            ))
        })
        .collect()
}

fn merge_ranges(mut ranges: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    ranges.sort_by(|left, right| left.0.total_cmp(&right.0));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for range in ranges {
        if let Some(last) = merged.last_mut() {
            if range.0 <= last.1 + 1.0e-9 {
                last.1 = last.1.max(range.1);
                continue;
            }
        }
        merged.push(range);
    }
    merged
}

fn shift_ranges(
    ranges: &[(f64, f64)],
    source_length: f64,
    shift: f64,
    target_length: f64,
) -> Vec<(f64, f64)> {
    ranges
        .iter()
        .filter_map(|range| {
            let start = if range.0 <= 1.0e-9 {
                0.0
            } else {
                (range.0 + shift).clamp(0.0, target_length)
            };
            let end = if range.1 >= source_length - 1.0e-9 {
                target_length
            } else {
                (range.1 + shift).clamp(0.0, target_length)
            };
            (end - start > 1.0e-9).then_some((start, end))
        })
        .collect()
}

fn style_width(style: &MLineStyle, scale: f64) -> f64 {
    let low = style
        .elements
        .iter()
        .map(|element| element.offset)
        .fold(f64::INFINITY, f64::min);
    let high = style
        .elements
        .iter()
        .map(|element| element.offset)
        .fold(f64::NEG_INFINITY, f64::max);
    if low.is_finite() && high.is_finite() {
        (high - low).abs() * scale.abs()
    } else {
        scale.abs()
    }
}

fn outer_element_indices(style: &MLineStyle) -> Vec<usize> {
    if style.elements.is_empty() {
        return Vec::new();
    }
    let low = style
        .elements
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| left.offset.total_cmp(&right.offset))
        .map(|(index, _)| index)
        .unwrap_or(0);
    let high = style
        .elements
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| left.offset.total_cmp(&right.offset))
        .map(|(index, _)| index)
        .unwrap_or(low);
    if low == high {
        vec![low]
    } else {
        vec![low, high]
    }
}

fn inner_element_indices(style: &MLineStyle) -> Vec<usize> {
    let outer = outer_element_indices(style);
    let inner: Vec<usize> = (0..style.elements.len())
        .filter(|index| !outer.contains(index))
        .collect();
    if inner.is_empty() {
        outer.into_iter().take(1).collect()
    } else {
        inner
    }
}

fn gap_elements(
    mline: &mut MLine,
    segment: usize,
    intersection: DVec3,
    half_gap: f64,
    elements: Option<Vec<usize>>,
) {
    let elements = elements.unwrap_or_else(|| (0..mline.style_element_count).collect());
    for element in elements {
        let Some((a, b)) = element_segment(mline, segment, element) else {
            continue;
        };
        let Some(curve) = segment_curve(
            a,
            b,
            DVec3::new(mline.normal.x, mline.normal.y, mline.normal.z),
        ) else {
            continue;
        };
        let length = curve.length();
        if let Some(element) = mline.vertices[segment].segments.get_mut(element) {
            let Some(fraction) = curve.parameter_at(intersection.to_array()) else {
                continue;
            };
            let at = fraction.clamp(0.0, 1.0) * length;
            let low = (at - half_gap).max(0.0);
            let high = (at + half_gap).min(length);
            remove_drawn_range(&mut element.parameters, length, low, high);
            add_cut_range(&mut element.area_fill_parameters, length, low, high);
        }
    }
}

fn move_closest_end(
    mline: &mut MLine,
    style: &MLineStyle,
    index: usize,
    intersection: DVec3,
) {
    if mline.vertices.is_empty() || mline.is_closed() || index >= mline.vertices.len() {
        return;
    }
    let last_index = mline.vertices.len() - 1;
    if index != 0 && index != last_index {
        return;
    }
    let segment = if index == 0 { 0 } else { last_index - 1 };
    let normal = DVec3::new(mline.normal.x, mline.normal.y, mline.normal.z);
    let Some(source) = snapshot_segment_data(mline, segment, normal) else {
        return;
    };
    let peripheral_segment = if index == 0 {
        (mline.vertices.len() > 2).then_some(1)
    } else {
        (mline.vertices.len() > 2).then_some(last_index - 2)
    };
    let peripheral = peripheral_segment
        .and_then(|segment| snapshot_segment_data(mline, segment, normal).map(|data| (segment, data)));
    mline.vertices[index].position = acadrust::types::Vector3::new(
        intersection.x,
        intersection.y,
        intersection.z,
    );
    crate::entities::mline::rebuild_mline_geometry(mline);
    crate::modules::draw::draw::mline::sync_mline_element_parameters(mline, style);
    restore_shifted_segment_data(mline, segment, normal, source, index == 0);
    if let Some((segment, source)) = peripheral {
        restore_shifted_segment_data(mline, segment, normal, source, index == 0);
    }
}

fn selected_terminal_end(mline: &MLine, segment: usize, pick: DVec3) -> Option<usize> {
    if mline.is_closed() || mline.vertices.len() < 2 {
        return None;
    }
    let last_vertex = mline.vertices.len() - 1;
    let last_segment = last_vertex - 1;
    match (segment == 0, segment == last_segment) {
        (true, true) => {
            let pick = KernelVec3::from(pick.to_array());
            let first = &mline.vertices[0].position;
            let last = &mline.vertices[last_vertex].position;
            let first_distance = KernelVec3::new(first.x, first.y, first.z).distance_squared(pick);
            let last_distance = KernelVec3::new(last.x, last.y, last.z).distance_squared(pick);
            Some(if first_distance <= last_distance { 0 } else { last_vertex })
        }
        (true, false) => Some(0),
        (false, true) => Some(last_vertex),
        (false, false) => None,
    }
}

fn segment_intersection(
    first: (DVec3, DVec3),
    second: (DVec3, DVec3),
    normal: DVec3,
) -> Option<(DVec3, f64, f64, f64)> {
    let axis = (cadkernel::space::Vec3::from(first.1.to_array())
        - cadkernel::space::Vec3::from(first.0.to_array()))
    .normalize()?
    .to_array();
    let plane = cadkernel::space::Plane::orthonormal(
        first.0.to_array(),
        axis,
        normal.to_array(),
    )?;
    let points = [
        first.0.to_array(),
        first.1.to_array(),
        second.0.to_array(),
        second.1.to_array(),
    ];
    let tolerance = cadkernel::space::coplanarity_tolerance(&points);
    if !plane.contains(points[2], tolerance) || !plane.contains(points[3], tolerance) {
        return None;
    }
    let p = cadkernel::geom2d::Vec2::from(plane.project(points[0])?);
    let q = cadkernel::geom2d::Vec2::from(plane.project(points[2])?);
    let r = cadkernel::geom2d::Vec2::from(plane.project(points[1])?) - p;
    let s = cadkernel::geom2d::Vec2::from(plane.project(points[3])?) - q;
    let (t, u) = cadkernel::geom2d::intersect::line_line(
        p.to_array(),
        r.to_array(),
        q.to_array(),
        s.to_array(),
    )?;
    let intersection = plane.point_at((p + r * t).to_array());
    let sine = r.cross(s) / (r.length() * s.length()).max(f64::MIN_POSITIVE);
    Some((DVec3::from_array(intersection), t, u, sine))
}

inventory::submit!(crate::command::CommandRegistration { names: &["MLEDIT"] });
