//! Native Alignment grips and reference-plane curve placement.
use super::{
    block_actions as actions, block_grips, block_orientation_parameter as orientation,
    render_graph, Scene,
};
use crate::command::{EntityTransform, PointPickContext, WorkingPlane};
use codec::{
    entities::Insert,
    objects::{DynamicBlockData, ObjectType},
    Document, EntityType, Handle,
};
use glam::DVec3;
use kernel::geom2d::{self, Curve, Extent, Line, Tolerance};

#[derive(Clone)]
pub struct Alignment {
    pub parameter: Handle,
    pub id: usize,
    pub anchor: DVec3,
    pub direction: DVec3,
    pub plane: WorkingPlane,
    pub perpendicular: bool,
    pub reference: Insert,
    pub native: DynamicBlockData,
}

#[cfg(test)]
mod tests;

pub fn grips(doc: &Document, definition: Handle, insert: Handle) -> Vec<Alignment> {
    let Some(EntityType::Insert(reference)) = doc.get_entity(insert) else {
        return vec![];
    };
    reference_grips(doc, definition, reference)
}

/// Definition-space insertion grips, measured from the native block base.
pub(crate) fn insertion_grips(doc: &Document, name: &str) -> Vec<Alignment> {
    let Some(definition) = doc.block_records.get(name) else {
        return vec![];
    };
    reference_grips(
        doc,
        definition.handle,
        &Insert::new(name, codec::types::Vector3::ZERO),
    )
}

fn reference_grips(doc: &Document, definition: Handle, reference: &Insert) -> Vec<Alignment> {
    let transform = render_graph::insert_transform(doc, reference);
    let x = actions::v(transform.apply_rotation(actions::native(DVec3::X)));
    let y = actions::v(transform.apply_rotation(actions::native(DVec3::Y)));
    if !x.is_finite() || !y.is_finite() || x.cross(y).length() < 1e-12 {
        return vec![];
    }
    orientation::parameters(doc, definition)
        .into_iter()
        .filter_map(|p| {
            if p.kind != orientation::Kind::Alignment || p.native.parameter.chain_actions {
                return None;
            }
            let ObjectType::DynamicBlock(object) = doc.objects.get(&p.handle)? else {
                return None;
            };
            let DynamicBlockData::AlignmentParameter(native) = &object.data else {
                return None;
            };
            let native_grips = block_grips::config::locations(doc, definition, p.handle).ok()?;
            if native_grips.len() != 1 {
                return None;
            }
            let grip = &native_grips[0];
            if !actions::v(grip.point).is_finite()
                || (grip.point.z - p.native.definition_base_point.z).abs() > 1e-8
            {
                return None;
            }
            let anchor = actions::v(transform.apply(grip.point));
            let direction =
                actions::v(transform.apply_rotation(grip.direction?)).try_normalize()?;
            let plane = WorkingPlane::new(anchor, x, y);
            if !anchor.is_finite() || direction.dot(plane.z).abs() > 1e-8 {
                return None;
            }
            Some(Alignment {
                parameter: p.handle,
                id: block_grips::runtime::id(p.native.parameter.element.eval.node_id, 0)?,
                anchor,
                direction,
                plane,
                perpendicular: native.align_perpendicular,
                reference: reference.clone(),
                native: object.data.clone(),
            })
        })
        .collect()
}

impl Alignment {
    pub fn transformed(&self, destination: DVec3, angle: f64) -> Result<EntityType, &'static str> {
        if !destination.is_finite()
            || !angle.is_finite()
            || self.plane.to_local(destination).z.abs() > 1e-7
        {
            return Err("Specify a finite alignment point in the reference plane.");
        }
        let mut entity = EntityType::Insert(self.reference.clone());
        super::view::dispatch::apply_transform(
            &mut entity,
            &EntityTransform::Rotate {
                center: self.anchor,
                axis: self.plane.z,
                angle_rad: angle,
            },
        );
        super::view::dispatch::apply_transform(
            &mut entity,
            &EntityTransform::Translate(destination - self.anchor),
        );
        let EntityType::Insert(reference) = &entity else {
            unreachable!()
        };
        if !actions::v(reference.insert_point).is_finite() || !reference.rotation.is_finite() {
            return Err("Alignment produces an invalid reference transform.");
        }
        Ok(entity)
    }
}

impl Scene {
    pub(crate) fn alignment_grips(&self, insert: Handle) -> Vec<Alignment> {
        if self.block_edit_block.is_some() {
            return vec![];
        }
        let Some(EntityType::Insert(reference)) = self.document.get_entity(insert) else {
            return vec![];
        };
        if reference.common.owner_handle != self.interaction_block_handle() {
            return vec![];
        }
        self.document
            .dynamic_definition_for_insert(insert)
            .map(|definition| grips(&self.document, definition, insert))
            .unwrap_or_default()
    }
}

pub struct Targets {
    pub plane: WorkingPlane,
    curves: Vec<Curve>,
    bounds: Vec<Option<([f64; 2], [f64; 2])>>,
}

fn fragment(curve: &Curve, from: f64, to: f64) -> Option<Curve> {
    if !from.is_finite() || !to.is_finite() || to <= from {
        return None;
    }
    if curve.extent() == Extent::Bounded && from == 0. && to == 1. {
        return Some(curve.clone());
    }
    Some(match curve {
        Curve::Line(_) | Curve::Ray(_) | Curve::XLine(_) => Curve::Line(Line {
            start: curve.point_at(from),
            end: curve.point_at(to),
        }),
        Curve::Circle(circle) => Curve::Arc(geom2d::Arc {
            centre: circle.centre,
            radius: circle.radius,
            start_angle: std::f64::consts::TAU * from,
            end_angle: std::f64::consts::TAU * to,
        }),
        Curve::Arc(arc) => Curve::Arc(geom2d::Arc {
            start_angle: arc.start_angle + arc.sweep() * from,
            end_angle: arc.start_angle + arc.sweep() * to,
            ..*arc
        }),
        Curve::Ellipse(arc) => Curve::Ellipse(geom2d::EllipseArc {
            ellipse: arc.ellipse,
            start_parameter: arc.start_parameter + arc.sweep() * from,
            end_parameter: arc.start_parameter + arc.sweep() * to,
        }),
        Curve::Nurbs(curve) => Curve::Nurbs(curve.trimmed(from, to)?),
        Curve::Polyline(_) => return None, // Polylines are expanded into exact segments first.
    })
}

fn clipped(curve: Curve, boundary: &[Curve]) -> Vec<Curve> {
    let curve = if curve.extent() != Extent::Bounded {
        let (origin, direction) = curve.as_ray().unwrap();
        let denominator = direction[0] * direction[0] + direction[1] * direction[1];
        if denominator < 1e-24 {
            return vec![];
        }
        let parameters = boundary
            .iter()
            .flat_map(|edge| [edge.point_at(0.), edge.point_at(1.)])
            .map(|point| {
                ((point[0] - origin[0]) * direction[0] + (point[1] - origin[1]) * direction[1])
                    / denominator
            })
            .collect::<Vec<_>>();
        let mut from = parameters.iter().copied().fold(f64::INFINITY, f64::min);
        let to = parameters.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if curve.extent() == Extent::Forward {
            from = from.max(0.);
        }
        let Some(piece) = fragment(&curve, from, to) else {
            return vec![];
        };
        piece
    } else {
        curve
    };
    geom2d::inside_spans(boundary, &curve, Tolerance::new(1e-8))
        .into_iter()
        .filter_map(|span| fragment(&curve, span[0], span[1]))
        .collect()
}

impl Targets {
    pub fn from_scene(scene: &Scene, moving: Handle, plane: WorkingPlane) -> Self {
        let mut curves = Vec::new();
        let depths = scene.draw_depth_map();
        let frozen = scene
            .interaction_viewport_frozen_layers()
            .map(|layers| layers.iter().copied().collect());
        let annotation = if scene.active_viewport.is_some() || scene.current_layout == "Model" {
            scene.displayed_annotation_scale_handle()
        } else {
            scene.paper_annotation_scale_handle()
        };
        let graph = render_graph::RenderSceneGraph::new(
            &scene.document,
            frozen.as_ref(),
            annotation,
            scene.annotation_all_visible(),
            depths.as_ref(),
        )
        .with_annotation_scale(scene.annotation_scale)
        .with_viewport(scene.active_viewport);
        graph.walk_root(
            scene.render_scene_root(scene.interaction_block_handle()),
            |entity, context| {
                entity.common().handle != moving
                    && (context.is_instanced()
                        || !scene.entity_temporarily_hidden(entity.common().handle))
            },
            |entity, context| {
                let Some(native) = crate::entities::curve::entity_curve(entity) else {
                    return;
                };
                let origin = plane.to_local(actions::v(
                    context
                        .transform
                        .apply(actions::native(DVec3::from_array(native.plane.origin))),
                ));
                let x = plane.vector_to_local(actions::v(
                    context
                        .transform
                        .apply_rotation(actions::native(DVec3::from_array(native.plane.x_axis))),
                ));
                let y = plane.vector_to_local(actions::v(
                    context
                        .transform
                        .apply_rotation(actions::native(DVec3::from_array(native.plane.y_axis))),
                ));
                let segments = native.curve.segments();
                let segments = if segments.is_empty() {
                    vec![native.curve]
                } else {
                    segments
                };
                for curve in segments {
                    let planar = if curve.is_straight() {
                        [curve.point_at(0.), curve.point_at(1.)]
                            .iter()
                            .all(|p| (origin.z + x.z * p[0] + y.z * p[1]).abs() < 1e-7)
                    } else if let Some((lo, hi)) =
                        geom2d::analytic_curve_bounds(std::slice::from_ref(&curve))
                    {
                        [
                            [lo[0], lo[1]],
                            [lo[0], hi[1]],
                            [hi[0], lo[1]],
                            [hi[0], hi[1]],
                        ]
                        .iter()
                        .all(|p| (origin.z + x.z * p[0] + y.z * p[1]).abs() < 1e-7)
                    } else {
                        origin.z.abs() < 1e-7 && x.z.abs() < 1e-9 && y.z.abs() < 1e-9
                    };
                    if !planar {
                        continue;
                    }
                    let Some(curve) = curve.transformed(&geom2d::Transform {
                        origin: [origin.x, origin.y].into(),
                        x_axis: [x.x, x.y].into(),
                        y_axis: [y.x, y.y].into(),
                    }) else {
                        continue;
                    };
                    let mut pieces = vec![curve];
                    for clip in &context.clips {
                        let determinant = plane.x.x * plane.y.y - plane.x.y * plane.y.x;
                        if determinant.abs() < 1e-12 {
                            pieces.clear();
                            break;
                        }
                        let points = clip
                            .iter()
                            .map(|point| {
                                let dx = point[0] - plane.origin.x;
                                let dy = point[1] - plane.origin.y;
                                [
                                    (dx * plane.y.y - dy * plane.y.x) / determinant,
                                    (plane.x.x * dy - plane.x.y * dx) / determinant,
                                ]
                            })
                            .collect::<Vec<_>>();
                        let boundary = points
                            .iter()
                            .zip(points.iter().cycle().skip(1))
                            .take(points.len())
                            .map(|(a, b)| Curve::Line(Line { start: *a, end: *b }))
                            .collect::<Vec<_>>();
                        pieces = pieces
                            .into_iter()
                            .flat_map(|curve| clipped(curve, &boundary))
                            .collect();
                    }
                    curves.extend(pieces);
                }
            },
        );
        let bounds = curves
            .iter()
            .map(|curve| geom2d::analytic_curve_bounds(std::slice::from_ref(curve)))
            .collect();
        Self {
            plane,
            curves,
            bounds,
        }
    }

    pub fn pose(
        &self,
        alignment: &Alignment,
        cursor: DVec3,
        context: Option<PointPickContext>,
        flipped: bool,
    ) -> Result<(DVec3, f64), &'static str> {
        if !cursor.is_finite() {
            return Err("Specify a finite alignment point.");
        }
        let local = self.plane.to_local(cursor);
        let screen = |point: [f64; 2], context: PointPickContext| {
            crate::scene::pick::hit_test::world_to_screen(
                self.plane.to_world(DVec3::new(point[0], point[1], 0.)),
                context.view,
                context.eye,
                context.bounds,
            )
        };
        let mut best: Option<(f64, [f64; 2], [f64; 2])> = None;
        for (curve, bounds) in self.curves.iter().zip(&self.bounds) {
            if let (Some(context), Some((lo, hi))) = (context, bounds) {
                let points = [
                    [lo[0], lo[1]],
                    [lo[0], hi[1]],
                    [hi[0], lo[1]],
                    [hi[0], hi[1]],
                ]
                .map(|p| screen(p, context));
                let cursor = screen([local.x, local.y], context);
                let aperture = context.aperture_px.max(1.);
                if points.iter().all(|p| p.x.is_finite() && p.y.is_finite()) {
                    let min_x = points.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
                    let max_x = points.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max);
                    let min_y = points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
                    let max_y = points.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
                    if cursor.x < min_x - aperture
                        || cursor.x > max_x + aperture
                        || cursor.y < min_y - aperture
                        || cursor.y > max_y + aperture
                    {
                        continue;
                    }
                }
            }
            let nearest = geom2d::closest_point(curve, [local.x, local.y]);
            let distance = if let Some(context) = context {
                f64::from(
                    screen(nearest.point, context).distance(screen([local.x, local.y], context)),
                )
            } else {
                nearest.distance
            };
            let limit = context.map_or(1e-7, |context| f64::from(context.aperture_px.max(1.)));
            if !distance.is_finite()
                || distance > limit
                || best.as_ref().is_some_and(|best| best.0 <= distance)
            {
                continue;
            }
            let tangent = curve.tangent_at(nearest.t);
            if tangent[0].hypot(tangent[1]) < 1e-12 {
                continue;
            }
            best = Some((distance, nearest.point, tangent));
        }
        let Some((_, point, tangent)) = best else {
            // Off-target placement is free translation in the reference plane;
            // Flip still reverses the reference direction, without a target tangent.
            let angle = if flipped { std::f64::consts::PI } else { 0. };
            return Ok((self.plane.to_world(DVec3::new(local.x, local.y, 0.)), angle));
        };
        let target = if alignment.perpendicular {
            [-tangent[1], tangent[0]]
        } else {
            tangent
        };
        let axis = self.plane.vector_to_local(alignment.direction);
        let raw = (axis.x * target[1] - axis.y * target[0])
            .atan2(axis.x * target[0] + axis.y * target[1]);
        let mut angle = raw.rem_euclid(std::f64::consts::PI);
        if angle > std::f64::consts::FRAC_PI_2 {
            angle -= std::f64::consts::PI;
        }
        if flipped {
            angle += std::f64::consts::PI;
        }
        Ok((
            self.plane.to_world(DVec3::new(point[0], point[1], 0.)),
            angle,
        ))
    }
}
