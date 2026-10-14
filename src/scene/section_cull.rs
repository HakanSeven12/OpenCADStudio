//! SECTIONCULL: keep only what lies inside a section object — the part a
//! live section shows — and remove the rest for good. Solids, surfaces and
//! closed meshes are cut to it, open meshes are clipped face by face, and
//! every other object wholly outside it is erased. Objects across its
//! boundary that cannot be cut stay whole.

use codec::{EntityType, Handle};
use glam::DVec3;
use rustc_hash::FxHashMap;

use super::Scene;

/// What culling to a section does to the space it lives in.
#[derive(Default)]
pub struct SectionCull {
    /// Bodies the section crosses, with what stays of each.
    pub cut: Vec<(Handle, kernel::brep::Body)>,
    /// Open meshes the section crosses, clipped to it.
    pub clipped: Vec<EntityType>,
    /// Objects wholly outside the section.
    pub outside: Vec<Handle>,
}

enum Place {
    Inside,
    Outside,
    Across,
}

const TOLERANCE: f64 = 1e-9;

fn place(planes: &[kernel::space::Plane], points: &[[f64; 3]]) -> Place {
    let mut inside = true;
    for plane in planes {
        let behind = points
            .iter()
            .filter(|point| plane.distance_to(**point).unwrap_or(0.0) < -TOLERANCE)
            .count();
        if behind == points.len() {
            return Place::Outside;
        }
        inside &= behind == 0;
    }
    if inside {
        Place::Inside
    } else {
        Place::Across
    }
}

fn same_extent(a: &kernel::brep::Body, b: &kernel::brep::Body) -> bool {
    match (kernel::brep::body_bounds(a), kernel::brep::body_bounds(b)) {
        (Some(a), Some(b)) => (0..3).all(|axis| {
            (a.min[axis] - b.min[axis]).abs() <= 1e-7 && (a.max[axis] - b.max[axis]).abs() <= 1e-7
        }),
        _ => false,
    }
}

impl Scene {
    /// The section SECTIONCULL works with: the selected section object, or
    /// else the live section of the current space.
    pub fn cull_section(&self) -> Option<Handle> {
        let is_section = |handle: &Handle| {
            matches!(
                self.document.get_entity(*handle),
                Some(EntityType::Extended(extended))
                    if matches!(
                        extended.data,
                        codec::entities::ExtendedEntityData::SectionObject(_)
                    )
            )
        };
        if let Some(handle) = self
            .selected_handles_in_order()
            .into_iter()
            .find(is_section)
        {
            return Some(handle);
        }
        let block = self.current_layout_block_handle();
        self.document
            .entities()
            .filter(|entity| match entity {
                EntityType::Extended(extended) => match &extended.data {
                    codec::entities::ExtendedEntityData::SectionObject(data) => {
                        let owner = extended.common.owner_handle;
                        data.flags & 1 != 0
                            && !extended.common.invisible
                            && (owner.is_null() || owner == block)
                    }
                    _ => false,
                },
                _ => false,
            })
            .map(|entity| entity.common().handle)
            .last()
    }

    /// Plan culling the current space to section object `section`; `None`
    /// when it is not a section object.
    pub fn section_cull(&self, section: Handle) -> Option<SectionCull> {
        let Some(EntityType::Extended(extended)) = self.document.get_entity(section) else {
            return None;
        };
        let codec::entities::ExtendedEntityData::SectionObject(data) = &extended.data else {
            return None;
        };
        let slice_depth = crate::entities::extended::section_is_slice(extended)
            .then(|| crate::entities::extended::section_slice_depth(extended).unwrap_or(0.0));
        let planes = Self::section_planes(data, slice_depth, None)?;

        // Every drawn point of every object in the space, from its wires and
        // its shaded mesh.
        let block = self.current_layout_block_handle();
        let mut points: FxHashMap<Handle, Vec<[f64; 3]>> = FxHashMap::default();
        let wires = self.wires_for_block_culled(block, None, None, None, None, None, true, None);
        for wire in &wires {
            let Some(handle) = Self::handle_from_wire_name(&wire.name) else {
                continue;
            };
            let entry = points.entry(handle).or_default();
            for (index, &[x, y, z]) in wire.points.iter().enumerate() {
                let [lx, ly, lz] = wire.points_low.get(index).copied().unwrap_or([0.0; 3]);
                let point = DVec3::new(
                    f64::from(x) + f64::from(lx),
                    f64::from(y) + f64::from(ly),
                    f64::from(z) + f64::from(lz),
                );
                if point.is_finite() {
                    entry.push(point.to_array());
                }
            }
        }
        for set in self.interaction_meshes_arc().iter() {
            let handle = set.entity_handle();
            let (Some(handle), Some(aabb)) = (handle, super::mesh_interaction_aabb(set)) else {
                continue;
            };
            let entry = points.entry(handle).or_default();
            for corner in 0..8 {
                entry.push([
                    aabb[if corner & 1 == 0 { 0 } else { 3 }],
                    aabb[if corner & 2 == 0 { 1 } else { 4 }],
                    aabb[if corner & 4 == 0 { 2 } else { 5 }],
                ]);
            }
        }

        let mut cull = SectionCull::default();
        // A live section hides what lies wholly outside it, so the space's
        // own objects are walked too.
        let mut handles: Vec<Handle> = points
            .keys()
            .copied()
            .chain(
                self.document
                    .entities()
                    .filter(|entity| entity.common().owner_handle == block)
                    .map(|entity| entity.common().handle),
            )
            .collect();
        handles.sort_unstable_by_key(|handle| handle.value());
        handles.dedup();
        for handle in handles {
            if handle == section || self.is_layer_locked(handle) {
                continue;
            }
            let Some(entity) = self.document.get_entity(handle) else {
                continue;
            };
            // Bodies are cut with the kernel: their drawn wires may already
            // show the live cut, so they say nothing about the source.
            if let Some(body) = self.section_source_body(handle) {
                match Self::section_body(&body, data, slice_depth, None) {
                    Ok(None) => cull.outside.push(handle),
                    Ok(Some(kept)) if !same_extent(&body, &kept) => cull.cut.push((handle, kept)),
                    _ => {}
                }
                continue;
            }
            if let Some((vertices, _)) = crate::entities::mesh::mesh_polygons(entity) {
                match place(&planes, &vertices) {
                    Place::Outside => cull.outside.push(handle),
                    Place::Inside => {}
                    Place::Across => {
                        let mut mesh = entity.clone();
                        for plane in &planes {
                            let Some((vertices, faces)) =
                                crate::entities::mesh::mesh_polygons(&mesh)
                            else {
                                break;
                            };
                            let side = |point: [f64; 3]| plane.distance_to(point).unwrap_or(0.0);
                            if vertices.iter().all(|point| side(*point) >= -TOLERANCE) {
                                continue;
                            }
                            let Some(mut clipped) = crate::entities::mesh::clipped_mesh(
                                &vertices,
                                &faces,
                                side,
                                true,
                                mesh.common(),
                            ) else {
                                break;
                            };
                            clipped.common.handle = handle;
                            mesh = EntityType::Mesh(clipped);
                        }
                        cull.clipped.push(mesh);
                    }
                }
                continue;
            }
            if let Some(points) = points.get(&handle) {
                if matches!(place(&planes, points), Place::Outside) {
                    cull.outside.push(handle);
                }
            }
        }
        Some(cull)
    }
}
