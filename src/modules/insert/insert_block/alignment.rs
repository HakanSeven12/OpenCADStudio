use super::*;
use crate::scene::block_alignment::{self, Alignment, Targets};

pub(super) struct InsertionAlignment {
    grips: Vec<Alignment>,
    targets: Targets,
}

impl InsertionAlignment {
    pub(super) fn from_scene(
        scene: &crate::scene::Scene,
        name: &str,
        plane: WorkingPlane,
    ) -> Option<Self> {
        let grips = block_alignment::insertion_grips(&scene.document, name);
        if grips.is_empty() {
            return None;
        }
        Some(Self {
            grips,
            targets: Targets::from_scene(scene, codec::Handle::NULL, plane),
        })
    }
}

impl InsertBlockCommand {
    /// Align only the active native insertion anchor. A custom Basepoint or an
    /// unrelated cycled grip must not silently turn into an Alignment anchor.
    pub(super) fn aligned_pose(
        &self,
        point: DVec3,
        scale: [f64; 3],
        rotation: f64,
    ) -> (DVec3, f64) {
        let fallback = (point, rotation);
        if self.mode == Mode::Paste || self.plane.to_local(point).z.abs() > 1e-7 {
            return fallback;
        }
        let Some(context) = &self.alignment else {
            return fallback;
        };
        let Some(grip) = context
            .grips
            .iter()
            .find(|g| g.anchor.distance(self.base) < 1e-8)
        else {
            return fallback;
        };
        let mut alignment = grip.clone();
        let direction = alignment.direction * DVec3::from_array(scale);
        let (sin, cos) = rotation.sin_cos();
        let direction = DVec3::new(
            direction.x * cos - direction.y * sin,
            direction.x * sin + direction.y * cos,
            direction.z,
        );
        let Some(direction) = self.plane.vector_to_world(direction).try_normalize() else {
            return fallback;
        };
        alignment.anchor = point;
        alignment.direction = direction;
        alignment.plane = self.plane;
        context
            .targets
            .pose(&alignment, point, self.pick_context, false)
            .map(|(destination, delta)| (destination, rotation + delta))
            .unwrap_or(fallback)
    }
}

#[cfg(test)]
mod tests;
