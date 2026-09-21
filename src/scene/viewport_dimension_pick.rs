//! Pick dimension geometry through a layout viewport. Resolve block instances
//! in model space, then project the picked entity onto the paper sheet.

use super::*;

use crate::command::EntityTransform;
use crate::scene::viewport_ref::ViewportFrame;
use acadrust::types::{Matrix4, Transform};
#[cfg(test)]
use glam::DVec2;
use glam::DVec3;

/// A model entity resolved by clicking inside a paper-space content viewport.
#[derive(Clone, Debug)]
pub struct ViewportDimensionPick {
    /// The viewport the click looked through.
    pub frame: ViewportFrame,
    /// Handle of the innermost model entity that owns the picked geometry.
    pub entity_handle: Handle,
    /// INSERT handles from the outermost instance down to the instance that
    /// directly contains `entity_handle`; empty for top-level model geometry.
    pub block_path: Vec<Handle>,
    /// The picked geometry with the block-instance transform *and* the
    /// viewport model→paper transform applied, so paper-space dimension
    /// commands can consume it unchanged.
    pub paper_entity: EntityType,
    /// The click position on the sheet.
    pub paper_point: DVec3,
    /// The click position in model space.
    pub model_point: DVec3,
}

impl Scene {
    /// Resolve an explicit dimension object pick made on the sheet but landing
    /// inside a content viewport, against the *model* geometry that viewport
    /// displays.
    ///
    /// `aperture_paper` is the pick aperture in paper units.
    pub fn dimension_pick_through_viewport(
        &self,
        paper: DVec3,
        aperture_paper: f64,
    ) -> Option<ViewportDimensionPick> {
        for frame in self.viewport_frames_at_paper_point(paper) {
            let model_point = frame.paper_to_model(paper);
            let aperture_model =
                (aperture_paper.max(0.0) * frame.paper_to_model_length_factor()).max(1e-9);

            let Some(top) = self.nearest_model_wire_handle(&frame, model_point, aperture_model)
            else {
                continue;
            };
            let Some((entity, block_path)) =
                self.resolve_measurable_entity(top, model_point, Some(frame.viewport))
            else {
                continue;
            };

            if !planar_pick_distance(&entity, model_point).is_some_and(|d| d <= aperture_model) {
                continue;
            }
            let mut paper_entity = entity;
            crate::scene::view::dispatch::apply_transform(
                &mut paper_entity,
                &EntityTransform::Affine(viewport_model_to_paper_transform(&frame)),
            );
            let entity_handle = paper_entity.as_entity().handle();
            return Some(ViewportDimensionPick {
                frame,
                entity_handle: if entity_handle.is_valid() {
                    entity_handle
                } else {
                    top
                },
                block_path,
                paper_entity,
                paper_point: paper,
                model_point,
            });
        }
        None
    }

    /// Nearest resident model wire of `viewport` to `model_point`, within
    /// `aperture` model units. Wires are the same tessellation the viewport
    /// draws, so this matches what the user sees.
    fn nearest_model_wire_handle(
        &self,
        frame: &ViewportFrame,
        model_point: DVec3,
        aperture: f64,
    ) -> Option<Handle> {
        let viewport = frame.viewport;
        let camera = self.camera_for_viewport(viewport)?;
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 1000.0,
        };
        let radius_px =
            (aperture / (2.0 * camera.ortho_size() as f64) * bounds.height as f64) as f32;
        let wires = self.interaction_pick_candidates_near(
            self.model_wires_for_viewport_arc(viewport, 0.0),
            model_point,
            camera.view_proj_rte(bounds),
            camera.eye(),
            bounds,
            radius_px.max(1.0),
        );
        let mut best: Option<(f64, Handle)> = None;
        for wire in wires.iter() {
            if !wire.display_visible {
                continue;
            }
            let Some(handle) = Self::handle_from_wire_name(&wire.name) else {
                continue;
            };
            let offset = wire
                .render_instance
                .as_ref()
                .map(|inst| DVec3::from_array(inst.translation))
                .unwrap_or(DVec3::ZERO);
            let distance = wire_polyline_nearest(wire, offset, model_point);
            if let Some((distance, nearest)) = distance {
                if !self.viewport_displays_paper_point(
                    viewport,
                    frame.model_to_paper(nearest).truncate(),
                ) {
                    continue;
                }
                if distance <= aperture && best.as_ref().is_none_or(|(d, _)| distance < *d) {
                    best = Some((distance, handle));
                }
            }
        }
        best.map(|(_, handle)| handle)
    }

    /// Turn a picked handle into a measurable planar entity in model (WCS)
    /// coordinates, descending through block instances and baking the
    /// instance transform into the returned clone.
    pub(crate) fn resolve_measurable_entity(
        &self,
        handle: Handle,
        model_point: DVec3,
        viewport: Option<Handle>,
    ) -> Option<(EntityType, Vec<Handle>)> {
        self.resolve_measurable_feature(handle, model_point, viewport, None)
    }

    pub(crate) fn resolve_measurable_feature(
        &self,
        handle: Handle,
        model_point: DVec3,
        viewport: Option<Handle>,
        kind: Option<crate::snap::SnapType>,
    ) -> Option<(EntityType, Vec<Handle>)> {
        let entity = self.document.get_entity(handle)?;
        match entity {
            EntityType::Insert(insert) => self.descend_block_instance(
                insert,
                Transform::identity(),
                model_point,
                0,
                viewport,
                kind,
            ),
            other => {
                feature_pick_distance(other, model_point, kind)?;
                Some((other.clone(), Vec::new()))
            }
        }
    }

    /// Depth-first search for the nearest measurable entity inside an INSERT.
    /// Returns the entity already transformed into WCS plus the INSERT path.
    fn descend_block_instance(
        &self,
        insert: &acadrust::entities::Insert,
        outer: Transform,
        model_point: DVec3,
        depth: usize,
        viewport: Option<Handle>,
        kind: Option<crate::snap::SnapType>,
    ) -> Option<(EntityType, Vec<Handle>)> {
        const MAX_DEPTH: usize = 8;
        if depth >= MAX_DEPTH {
            return None;
        }
        if insert.row_count > 1 || insert.column_count > 1 {
            return None;
        }
        let insert_handle = insert.common.handle;
        let local = crate::scene::render_graph::insert_transform(&self.document, insert);
        let combined = local.then(&outer);
        let block = self.document.block_records.get(&insert.block_name)?;

        let mut best: Option<(f64, EntityType, Vec<Handle>)> = None;
        for &child in &block.entity_handles {
            let Some(child_entity) = self.document.get_entity(child) else {
                continue;
            };
            let common = child_entity.common();
            let layer = self.document.layers.get(&common.layer);
            let frozen = viewport.and_then(|h| self.document.get_entity(h)).is_some_and(|entity| {
                matches!(entity, EntityType::Viewport(vp) if layer.is_some_and(|l| vp.frozen_layers.contains(&l.handle)))
            });
            if common.invisible || layer.is_some_and(|l| l.flags.off || l.flags.frozen) || frozen {
                continue;
            }
            if let EntityType::Insert(child_insert) = child_entity {
                if let Some((entity, mut nested_path)) = self.descend_block_instance(
                    child_insert,
                    combined,
                    model_point,
                    depth + 1,
                    viewport,
                    kind,
                ) {
                    if let Some(distance) = feature_pick_distance(&entity, model_point, kind) {
                        if best.as_ref().is_none_or(|(d, _, _)| distance < *d) {
                            let mut full = vec![insert_handle];
                            full.append(&mut nested_path);
                            best = Some((distance, entity, full));
                        }
                    }
                }
                continue;
            }
            // A nonuniform block transform turns circular geometry into an
            // ellipse. The entity transform API retains the circle type, so
            // decline this object pick instead of manufacturing a radius.
            let curved = matches!(child_entity, EntityType::Circle(_) | EntityType::Arc(_))
                || matches!(child_entity, EntityType::LwPolyline(p) if p.vertices.iter().any(|v| v.bulge != 0.0));
            let m = &combined.matrix.m;
            let x = DVec3::new(m[0][0], m[1][0], m[2][0]);
            let y = DVec3::new(m[0][1], m[1][1], m[2][1]);
            let magnitude = x.length_squared().max(y.length_squared());
            if curved
                && kind != Some(crate::snap::SnapType::Center)
                && (magnitude < 1e-24
                    || (x.length_squared() - y.length_squared()).abs() > magnitude * 1e-10
                    || x.dot(y).abs() > magnitude * 1e-10)
            {
                continue;
            }
            let mut placed = child_entity.clone();
            crate::scene::view::dispatch::apply_transform(
                &mut placed,
                &EntityTransform::Affine(combined),
            );
            let Some(distance) = feature_pick_distance(&placed, model_point, kind) else {
                continue;
            };
            if best.as_ref().is_none_or(|(d, _, _)| distance < *d) {
                best = Some((distance, placed, vec![insert_handle]));
            }
        }
        let (_, entity, found_path) = best?;
        Some((entity, found_path))
    }
}

/// Model→paper for a viewport, as an affine entity transform. Z passes
/// through unchanged so planar geometry keeps its elevation.
pub fn viewport_model_to_paper_transform(frame: &ViewportFrame) -> Transform {
    let (sin, cos) = frame.twist.sin_cos();
    let s = frame.scale;
    let a = cos * s;
    let b = sin * s;
    // t = paper_center - R*s*model_target
    let tx = frame.paper_center.x - (a * frame.model_target.x - b * frame.model_target.y);
    let ty = frame.paper_center.y - (b * frame.model_target.x + a * frame.model_target.y);
    Transform::from_matrix(Matrix4 {
        m: [
            [a, -b, 0.0, tx],
            [b, a, 0.0, ty],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    })
}

/// Distance from `point` to a wire's polyline, in XY. `None` when the wire has
/// no usable segment.
fn wire_polyline_nearest(
    wire: &crate::scene::WireModel,
    offset: DVec3,
    point: DVec3,
) -> Option<(f64, DVec3)> {
    let count = wire.points.len();
    if count == 0 {
        return None;
    }
    let at = |i: usize| -> DVec3 {
        let hi = wire.points[i];
        let lo = wire.points_low.get(i).copied().unwrap_or([0.0; 3]);
        DVec3::new(
            hi[0] as f64 + lo[0] as f64,
            hi[1] as f64 + lo[1] as f64,
            hi[2] as f64 + lo[2] as f64,
        ) + offset
    };
    let mut best = f64::INFINITY;
    let mut best_point = DVec3::ZERO;
    let mut previous: Option<DVec3> = None;
    for i in 0..count {
        let current = at(i);
        if !current.x.is_finite() || !current.y.is_finite() {
            previous = None;
            continue;
        }
        let nearest = previous.map_or(current, |start| {
            let segment = cadkernel::geom2d::Curve::Line(cadkernel::geom2d::Line {
                start: start.truncate().to_array(),
                end: current.truncate().to_array(),
            });
            let near = cadkernel::geom2d::closest_point(&segment, point.truncate().to_array());
            start.lerp(current, near.t)
        });
        let distance = (nearest - point).truncate().length();
        if distance < best {
            best = distance;
            best_point = nearest;
        }
        previous = Some(current);
    }
    best.is_finite().then_some((best, best_point))
}

/// Distance from `point` to the planar geometry object-pick dimensioning
/// supports. `None` for entity types that cannot be dimensioned by picking.
pub fn planar_pick_distance(entity: &EntityType, point: DVec3) -> Option<f64> {
    let planar = crate::entities::curve::entity_curve(entity)?;
    let normal = DVec3::from_array(planar.plane.normal()?);
    if normal.cross(DVec3::Z).length_squared() > 1e-12 {
        return None;
    }
    let uv = planar.plane.project(point.to_array())?;
    let nearest = cadkernel::geom2d::closest_point(&planar.curve, uv);
    Some(
        (DVec3::from_array(planar.plane.point_at(nearest.point)) - point)
            .truncate()
            .length(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> ViewportFrame {
        ViewportFrame {
            viewport: Handle::NULL,
            paper_center: DVec2::new(100.0, 50.0),
            model_target: DVec2::new(1000.0, 2000.0),
            scale: 0.1,
            twist: 0.4,
            locked: false,
        }
    }

    #[test]
    fn affine_transform_matches_the_frame_mapping() {
        let f = frame();
        let t = viewport_model_to_paper_transform(&f);
        for model in [
            DVec3::new(1000.0, 2000.0, 0.0),
            DVec3::new(1100.0, 2000.0, 0.0),
            DVec3::new(940.5, 2113.25, 0.0),
        ] {
            let expected = f.model_to_paper(model);
            let actual = t.apply(acadrust::types::Vector3::new(model.x, model.y, model.z));
            assert!(
                (actual.x - expected.x).abs() < 1e-9 && (actual.y - expected.y).abs() < 1e-9,
                "{actual:?} != {expected:?}"
            );
        }
    }
}

/// Distance to the particular feature accepted by a snap, rather than to the
/// nearest curve. A circle's center is deliberately not on its circumference.
pub(crate) fn feature_pick_distance(
    entity: &EntityType,
    point: DVec3,
    kind: Option<crate::snap::SnapType>,
) -> Option<f64> {
    use crate::snap::SnapType as S;
    let points: Vec<acadrust::types::Vector3> = match kind {
        Some(S::Center) => match entity {
            EntityType::Circle(c) => vec![c.center_wcs()],
            EntityType::Arc(a) => vec![a.center_wcs()],
            EntityType::Ellipse(e) => vec![e.center],
            _ => return None,
        },
        Some(S::Endpoint) => crate::scene::dimension_assoc::source_points(entity),
        Some(S::Node) => match entity {
            EntityType::Point(p) => vec![p.location],
            _ => return None,
        },
        Some(S::Midpoint | S::Quadrant) => {
            let planar = crate::entities::curve::entity_curve(entity)?;
            cadkernel::geom2d::snap::characteristic_points(&planar.curve)
                .into_iter()
                .filter(|p| match kind {
                    Some(S::Midpoint) => p.kind == cadkernel::geom2d::snap::SnapKind::Midpoint,
                    _ => p.kind == cadkernel::geom2d::snap::SnapKind::Quadrant,
                })
                .map(|p| {
                    let p = planar.plane.point_at(p.point);
                    acadrust::types::Vector3::new(p[0], p[1], p[2])
                })
                .collect()
        }
        _ => return planar_pick_distance(entity, point),
    };
    points
        .into_iter()
        .map(|p| (DVec3::new(p.x, p.y, p.z) - point).truncate().length())
        .min_by(f64::total_cmp)
}
