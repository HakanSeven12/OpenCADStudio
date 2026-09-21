//! Coordinates and feature identity shared by viewport acquisition and dimensions.

use acadrust::types::Handle;
use glam::{DMat3, DVec2, DVec3};

use crate::command::DimensionAssociationSource;
use crate::snap::SnapType;

/// Planar orthographic mapping between a layout viewport's paper rectangle and
/// the model plane it displays. Handles pan (`model_target` / `paper_center`),
/// scale (paper units per model unit) and in-plane rotation (`twist`,
/// radians, counter-clockwise, applied to model geometry before scaling).
///
/// Constructed by `Scene::viewport_frame`. All arithmetic is `f64`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewportFrame {
    /// Viewport entity handle.
    pub viewport: Handle,
    /// Paper-space centre of the viewport rectangle.
    pub paper_center: DVec2,
    /// Model-space point displayed at `paper_center`.
    pub model_target: DVec2,
    /// Paper units per model unit (e.g. 0.1 for a 1:10 viewport).
    pub scale: f64,
    /// In-plane rotation (DXF `twist_angle`), radians.
    pub twist: f64,
    /// `true` when the viewport is display-locked (snapping still allowed).
    pub locked: bool,
}

impl ViewportFrame {
    /// Model -> paper, as a 2-D affine matrix (homogeneous, column vectors).
    pub fn model_to_paper_matrix(&self) -> DMat3 {
        let (s, c) = self.twist.sin_cos();
        let rs = DMat3::from_cols(
            DVec3::new(c * self.scale, s * self.scale, 0.0),
            DVec3::new(-s * self.scale, c * self.scale, 0.0),
            DVec3::new(0.0, 0.0, 1.0),
        );
        let t_model = DMat3::from_translation(-self.model_target);
        let t_paper = DMat3::from_translation(self.paper_center);
        t_paper * rs * t_model
    }

    /// Paper -> model (inverse of [`Self::model_to_paper_matrix`]).
    pub fn paper_to_model_matrix(&self) -> DMat3 {
        self.model_to_paper_matrix().inverse()
    }

    /// Project a model-space point onto the paper sheet. `z` passes through.
    pub fn model_to_paper(&self, p: DVec3) -> DVec3 {
        let q = self.model_to_paper_matrix().transform_point2(p.truncate());
        DVec3::new(q.x, q.y, p.z)
    }

    /// Lift a paper-sheet point into model space. `z` passes through.
    pub fn paper_to_model(&self, p: DVec3) -> DVec3 {
        let q = self.paper_to_model_matrix().transform_point2(p.truncate());
        DVec3::new(q.x, q.y, p.z)
    }

    /// Model -> paper for a direction (rotation + scale, no translation).
    pub fn model_to_paper_dir(&self, d: DVec2) -> DVec2 {
        self.model_to_paper_matrix().transform_vector2(d)
    }

    /// Factor a paper-space length must be multiplied by to recover the model
    /// length it displays (`1 / scale`). This is the *viewport compensation*
    /// used by [`MeasurementScale`].
    pub fn paper_to_model_length_factor(&self) -> f64 {
        1.0 / self.scale
    }
}

/// Identity of the geometry a snap landed on, in model space, including the
/// block-instance path when the entity lives inside a block reference.
///
/// `source.handle` is the innermost entity (the one owning the feature).
/// `block_path` is the ordered list of INSERT handles from the outermost
/// instance in the space down to the instance that directly contains the
/// entity; empty for top-level geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct SnapSourceRef {
    pub source: DimensionAssociationSource,
    pub block_path: Vec<Handle>,
    pub snap_type: SnapType,
    /// The other entity/instance path for a true intersection.
    pub intersection: Option<Box<SnapSourceRef>>,
}

/// A snap accepted by a point-input command, with enough information for
/// dimension commands to distinguish *placement* (paper) from *measurement*
/// (model) and for association creation to record where it came from.
///
/// Invariant: when `viewport` is `Some`, `paper_point` is the point on the
/// sheet and `model_point` is that same point in model space through the
/// viewport frame. When `viewport` is `None` the snap was on paper-space (or
/// model-space) geometry directly and `paper_point == model_point`.
#[derive(Clone, Debug, PartialEq)]
pub struct AcceptedSnap {
    /// Point in the coordinate system of the current space (paper when a
    /// layout is active).
    pub paper_point: DVec3,
    /// Point in model space (identical to `paper_point` when `viewport` is
    /// `None`).
    pub model_point: DVec3,
    /// The viewport the snap looked through, if any.
    pub viewport: Option<Handle>,
    /// The frame in effect when the snap was accepted (so a later measurement
    /// uses the same scale even if the viewport is changed afterwards).
    pub frame: Option<ViewportFrame>,
    /// Geometry identity, when the snap landed on an entity feature (not a
    /// grid/free point).
    pub source: Option<SnapSourceRef>,
}

impl AcceptedSnap {
    /// A plain point in the current space with no viewport and no source.
    pub fn free(point: DVec3) -> Self {
        Self {
            paper_point: point,
            model_point: point,
            viewport: None,
            frame: None,
            source: None,
        }
    }

    pub fn through_viewport(&self) -> bool {
        self.viewport.is_some()
    }

    /// `snap.world` is always in the command's current space. A projected
    /// viewport hit also retains its original model coordinate explicitly.
    pub fn from_snap(snap: &crate::snap::SnapResult, frame: Option<ViewportFrame>) -> Self {
        let frame = frame.filter(|frame| snap.viewport == Some(frame.viewport));
        let source_ref = |source| SnapSourceRef {
            source,
            block_path: Vec::new(),
            snap_type: snap.snap_type,
            intersection: None,
        };
        Self {
            paper_point: snap.world,
            model_point: frame.map_or(snap.world, |frame| {
                snap.model_point
                    .unwrap_or_else(|| frame.paper_to_model(snap.world))
            }),
            viewport: frame.map(|f| f.viewport),
            frame,
            source: snap.source.map(|source| SnapSourceRef {
                intersection: snap
                    .secondary_source
                    .map(|source| Box::new(source_ref(source))),
                ..source_ref(source)
            }),
        }
    }

    /// The paper point with the viewport context of `self`, but no geometry
    /// identity — used when a command constrains a snapped point (ortho/polar,
    /// axis lock) so the recorded point matches what was committed.
    pub fn with_paper_point(mut self, paper_point: DVec3) -> Self {
        // Compare in the sheet plane only: paper space is flat and callers
        // clamp `z` to 0, which must not be mistaken for the point having been
        // moved off the snapped feature (and must not discard the snapped
        // model point's real elevation).
        let moved = self
            .paper_point
            .truncate()
            .distance_squared(paper_point.truncate())
            > 1e-18;
        if moved {
            // The committed point is no longer the feature that was snapped.
            self.source = None;
            self.model_point = match &self.frame {
                Some(frame) => frame.paper_to_model(paper_point),
                None => paper_point,
            };
        }
        self.paper_point = paper_point;
        self
    }
}

/// Length dimensions store raw distances in their owning space. The displayed
/// value uses `user_lfac * viewport_compensation`; angular dimensions are exempt.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasurementScale {
    pub user_lfac: f64,
    pub viewport_compensation: f64,
}

impl MeasurementScale {
    /// OCS bookkeeping keeps user scaling separate from the changing viewport
    /// scale. Standard DIMLFAC still carries the full factor for other readers.
    pub const APP_ID: &'static str = "OCS_VIEWPORT_MEASUREMENT";

    pub fn read(data: &acadrust::xdata::ExtendedData) -> Option<Self> {
        use acadrust::xdata::XDataValue;
        let record = data.get_record(Self::APP_ID)?;
        let [XDataValue::Integer16(1), XDataValue::Real(user), XDataValue::Real(compensation)] =
            record.values.as_slice()
        else {
            return None;
        };
        if !user.is_finite() || *user <= 0.0 || !compensation.is_finite() || *compensation <= 0.0 {
            return None;
        }
        Some(Self {
            user_lfac: *user,
            viewport_compensation: *compensation,
        })
    }

    pub fn write_to_entity(self, entity: &mut acadrust::EntityType) {
        use acadrust::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};
        crate::entities::dim_override::set_on_entity(
            entity,
            crate::entities::dim_override::DIMLFAC,
            Some(XDataValue::Real(-self.paper_factor())),
        );
        let common = entity.common_mut();
        let mut data = ExtendedData::new();
        for record in common
            .extended_data
            .records()
            .iter()
            .filter(|r| r.application_name != Self::APP_ID)
        {
            data.add_record(record.clone());
        }
        let mut record = ExtendedDataRecord::new(Self::APP_ID);
        record.add_value(XDataValue::Integer16(1));
        record.add_value(XDataValue::Real(self.user_lfac));
        record.add_value(XDataValue::Real(self.viewport_compensation));
        data.add_record(record);
        // Parsed records are authoritative after changing measurement data.
        common.extended_data = data;
    }

    /// Combined factor applied to a distance measured on the paper sheet.
    pub fn paper_factor(&self) -> f64 {
        self.user_lfac * self.viewport_compensation
    }

    /// Positive DIMLFAC applies everywhere; negative DIMLFAC applies only on
    /// paper. Zero means one. Saved viewport compensation is already in DIMLFAC.
    pub fn user_lfac_for_space(dimlfac: f64, paper_space: bool) -> f64 {
        if dimlfac == 0.0 {
            1.0
        } else if dimlfac > 0.0 {
            dimlfac
        } else if paper_space {
            -dimlfac
        } else {
            1.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(scale: f64, twist: f64) -> ViewportFrame {
        ViewportFrame {
            viewport: Handle::NULL,
            paper_center: DVec2::new(100.0, 50.0),
            model_target: DVec2::new(1000.0, 2000.0),
            scale,
            twist,
            locked: false,
        }
    }

    #[test]
    fn round_trips_with_scale_and_twist() {
        let f = frame(0.1, 0.3);
        let m = DVec3::new(1234.5, 1987.25, 0.0);
        let p = f.model_to_paper(m);
        let back = f.paper_to_model(p);
        assert!((back - m).length() < 1e-9);
    }

    #[test]
    fn scale_maps_lengths() {
        let f = frame(0.1, 0.0);
        let a = f.model_to_paper(DVec3::new(0.0, 0.0, 0.0));
        let b = f.model_to_paper(DVec3::new(100.0, 0.0, 0.0));
        assert!(((b - a).length() - 10.0).abs() < 1e-9);
        assert!((f.paper_to_model_length_factor() - 10.0).abs() < 1e-12);
        assert_eq!(frame(1e-14, 0.0).paper_to_model_length_factor(), 1e14);
    }

    #[test]
    fn dimlfac_sign_depends_on_dimension_space() {
        for (factor, paper, model) in [(-2.0, 2.0, 1.0), (0.0, 1.0, 1.0), (3.5, 3.5, 3.5)] {
            assert_eq!(MeasurementScale::user_lfac_for_space(factor, true), paper);
            assert_eq!(MeasurementScale::user_lfac_for_space(factor, false), model);
        }
    }
}
