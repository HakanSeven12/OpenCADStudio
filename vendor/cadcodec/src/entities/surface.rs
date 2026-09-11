//! Surface entities (ACAD_SURFACE family).
//!
//! Lofted / swept / extruded / revolved / plane / NURB surfaces share the
//! `AcDbSurface` base, which stores its geometry in ACIS format just like
//! [`Body`](super::solid3d::Body). They are kept as a distinct entity type so
//! the original surface kind survives a DWG round-trip.

use crate::entities::solid3d::{AcisData, Silhouette, Wire};
use crate::entities::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};

/// Which ACAD_SURFACE subtype a [`Surface`] represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SurfaceKind {
    /// Generic `SURFACE` / `AcDbSurface`.
    #[default]
    Generic,
    /// `PLANESURFACE`.
    Plane,
    /// `EXTRUDEDSURFACE`.
    Extruded,
    /// `LOFTEDSURFACE`.
    Lofted,
    /// `REVOLVEDSURFACE`.
    Revolved,
    /// `SWEPTSURFACE`.
    Swept,
    /// `NURBSURFACE`.
    Nurb,
}

impl SurfaceKind {
    /// Map a DXF class name to a surface kind.
    pub fn from_dxf_name(name: &str) -> Self {
        match name.to_uppercase().as_str() {
            "PLANESURFACE" => SurfaceKind::Plane,
            "EXTRUDEDSURFACE" => SurfaceKind::Extruded,
            "LOFTEDSURFACE" => SurfaceKind::Lofted,
            "REVOLVEDSURFACE" => SurfaceKind::Revolved,
            "SWEPTSURFACE" => SurfaceKind::Swept,
            "NURBSURFACE" => SurfaceKind::Nurb,
            _ => SurfaceKind::Generic,
        }
    }

    /// The DXF class name for this kind.
    pub fn dxf_name(self) -> &'static str {
        match self {
            SurfaceKind::Generic => "SURFACE",
            SurfaceKind::Plane => "PLANESURFACE",
            SurfaceKind::Extruded => "EXTRUDEDSURFACE",
            SurfaceKind::Lofted => "LOFTEDSURFACE",
            SurfaceKind::Revolved => "REVOLVEDSURFACE",
            SurfaceKind::Swept => "SWEPTSURFACE",
            SurfaceKind::Nurb => "NURBSURFACE",
        }
    }
}

/// Sweep options shared by extruded and swept surfaces.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SurfaceSweepOptions {
    pub draft_angle: f64,
    pub draft_start_distance: f64,
    pub draft_end_distance: f64,
    pub twist_angle: f64,
    pub scale_factor: f64,
    pub align_angle: f64,
    pub sweep_entity_transform: [f64; 16],
    pub path_entity_transform: [f64; 16],
    pub is_solid: bool,
    pub sweep_alignment_flags: i16,
    pub path_flags: i16,
    pub align_start: bool,
    pub bank: bool,
    pub base_point_set: bool,
    pub sweep_entity_transform_computed: bool,
    pub path_entity_transform_computed: bool,
    pub reference_vector: Vector3,
}

impl Default for SurfaceSweepOptions {
    fn default() -> Self {
        Self {
            draft_angle: 0.0,
            draft_start_distance: 0.0,
            draft_end_distance: 0.0,
            twist_angle: 0.0,
            scale_factor: 1.0,
            align_angle: 0.0,
            sweep_entity_transform: identity_matrix(),
            path_entity_transform: identity_matrix(),
            is_solid: false,
            sweep_alignment_flags: 0,
            path_flags: 0,
            align_start: false,
            bank: false,
            base_point_set: false,
            sweep_entity_transform_computed: false,
            path_entity_transform_computed: false,
            reference_vector: Vector3::UNIT_Z,
        }
    }
}

/// Subtype-specific native DWG surface data.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SurfaceData {
    #[default]
    Generic,
    Plane {
        class_version: i32,
    },
    Extruded {
        sweep_entity: Option<super::EmbeddedEntity>,
        options: SurfaceSweepOptions,
        sweep_vector: Vector3,
        sweep_transform: [f64; 16],
    },
    Lofted {
        loft_transform: [f64; 16],
        cross_section_entities: Vec<super::EmbeddedEntity>,
        guide_entities: Vec<super::EmbeddedEntity>,
        path_entity: Option<super::EmbeddedEntity>,
        plane_normal_lofting_type: i32,
        start_draft_angle: f64,
        end_draft_angle: f64,
        start_draft_magnitude: f64,
        end_draft_magnitude: f64,
        arc_length_parameterization: bool,
        no_twist: bool,
        align_direction: bool,
        simple_surfaces: bool,
        closed_surfaces: bool,
        solid: bool,
        ruled_surface: bool,
        virtual_guide: bool,
        cross_sections: Vec<Handle>,
        guide_curves: Vec<Handle>,
        path_curve: Option<Handle>,
    },
    Revolved {
        revolve_entity: Option<super::EmbeddedEntity>,
        class_version: i32,
        entity_id: i32,
        axis_point: Vector3,
        axis_vector: Vector3,
        revolve_angle: f64,
        start_angle: f64,
        entity_transform: [f64; 16],
        draft_angle: f64,
        draft_start_distance: f64,
        draft_end_distance: f64,
        twist_angle: f64,
        solid: bool,
        close_to_axis: bool,
    },
    Swept {
        class_version: i32,
        sweep_entity: Option<super::EmbeddedEntity>,
        path_entity: Option<super::EmbeddedEntity>,
        sweep_transform: [f64; 16],
        path_transform: [f64; 16],
        options: SurfaceSweepOptions,
    },
    Nurb {
        short_170: i16,
        cv_hull_display: bool,
        u_vector1: Vector3,
        v_vector1: Vector3,
        u_vector2: Vector3,
        v_vector2: Vector3,
    },
}

pub(crate) fn identity_matrix() -> [f64; 16] {
    let mut value = [0.0; 16];
    value[0] = 1.0;
    value[5] = 1.0;
    value[10] = 1.0;
    value[15] = 1.0;
    value
}

/// A 3D surface entity (ACAD_SURFACE family), backed by ACIS geometry.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Surface {
    /// Common entity data.
    pub common: EntityCommon,
    /// Which surface subtype this is.
    pub kind: SurfaceKind,
    /// ACIS/SAT surface geometry.
    pub acis_data: AcisData,
    /// Native modeler format version for surface subtypes that store it.
    pub modeler_format_version: i16,
    /// U and V isoline display counts.
    pub u_isolines: i16,
    pub v_isolines: i16,
    /// Subtype-specific construction data.
    pub surface_data: SurfaceData,
    /// Wireframe reference point.
    pub point_of_reference: Vector3,
    /// Modeler history object.
    pub history_handle: Option<Handle>,
    /// Wireframe edges for visualization.
    pub wires: Vec<Wire>,
    /// Silhouette data for viewports.
    pub silhouettes: Vec<Silhouette>,
}

impl Surface {
    /// Creates a new empty surface of the given kind.
    pub fn new(kind: SurfaceKind) -> Self {
        Self {
            common: EntityCommon::default(),
            kind,
            acis_data: AcisData::new(),
            modeler_format_version: 1,
            u_isolines: 4,
            v_isolines: 4,
            surface_data: match kind {
                SurfaceKind::Generic => SurfaceData::Generic,
                SurfaceKind::Plane => SurfaceData::Plane { class_version: 0 },
                SurfaceKind::Extruded => SurfaceData::Extruded {
                    sweep_entity: None,
                    options: SurfaceSweepOptions::default(),
                    sweep_vector: Vector3::ZERO,
                    sweep_transform: identity_matrix(),
                },
                SurfaceKind::Lofted => SurfaceData::Lofted {
                    loft_transform: identity_matrix(),
                    cross_section_entities: Vec::new(),
                    guide_entities: Vec::new(),
                    path_entity: None,
                    plane_normal_lofting_type: 0,
                    start_draft_angle: 0.0,
                    end_draft_angle: 0.0,
                    start_draft_magnitude: 0.0,
                    end_draft_magnitude: 0.0,
                    arc_length_parameterization: false,
                    no_twist: false,
                    align_direction: false,
                    simple_surfaces: false,
                    closed_surfaces: false,
                    solid: false,
                    ruled_surface: false,
                    virtual_guide: false,
                    cross_sections: Vec::new(),
                    guide_curves: Vec::new(),
                    path_curve: None,
                },
                SurfaceKind::Revolved => SurfaceData::Revolved {
                    revolve_entity: None,
                    class_version: 0,
                    entity_id: 0,
                    axis_point: Vector3::ZERO,
                    axis_vector: Vector3::UNIT_Z,
                    revolve_angle: 0.0,
                    start_angle: 0.0,
                    entity_transform: identity_matrix(),
                    draft_angle: 0.0,
                    draft_start_distance: 0.0,
                    draft_end_distance: 0.0,
                    twist_angle: 0.0,
                    solid: false,
                    close_to_axis: false,
                },
                SurfaceKind::Swept => SurfaceData::Swept {
                    class_version: 0,
                    sweep_entity: None,
                    path_entity: None,
                    sweep_transform: identity_matrix(),
                    path_transform: identity_matrix(),
                    options: SurfaceSweepOptions::default(),
                },
                SurfaceKind::Nurb => SurfaceData::Nurb {
                    short_170: 0,
                    cv_hull_display: false,
                    u_vector1: Vector3::ZERO,
                    v_vector1: Vector3::ZERO,
                    u_vector2: Vector3::ZERO,
                    v_vector2: Vector3::ZERO,
                },
            },
            point_of_reference: Vector3::ZERO,
            history_handle: None,
            wires: Vec::new(),
            silhouettes: Vec::new(),
        }
    }

    /// Returns true if this surface has valid ACIS data.
    pub fn has_acis_data(&self) -> bool {
        self.acis_data.has_data()
    }

    /// Parses the raw SAT text data into a structured [`SatDocument`].
    ///
    /// Returns `None` if the ACIS data is empty or binary (SAB).
    pub fn parse_sat(&self) -> Option<crate::entities::acis::SatDocument> {
        if self.acis_data.is_binary || self.acis_data.sat_data.is_empty() {
            return None;
        }
        crate::entities::acis::SatDocument::parse(&self.acis_data.sat_data).ok()
    }
}

impl Entity for Surface {
    fn handle(&self) -> Handle {
        self.common.handle
    }
    fn set_handle(&mut self, handle: Handle) {
        self.common.handle = handle;
    }
    fn layer(&self) -> &str {
        &self.common.layer
    }
    fn set_layer(&mut self, layer: String) {
        self.common.layer = layer;
    }
    fn color(&self) -> Color {
        self.common.color
    }
    fn set_color(&mut self, color: Color) {
        self.common.color = color;
    }
    fn line_weight(&self) -> LineWeight {
        self.common.line_weight
    }
    fn set_line_weight(&mut self, line_weight: LineWeight) {
        self.common.line_weight = line_weight;
    }
    fn transparency(&self) -> Transparency {
        self.common.transparency
    }
    fn set_transparency(&mut self, transparency: Transparency) {
        self.common.transparency = transparency;
    }
    fn is_invisible(&self) -> bool {
        self.common.invisible
    }
    fn set_invisible(&mut self, invisible: bool) {
        self.common.invisible = invisible;
    }
    fn bounding_box(&self) -> BoundingBox3D {
        if self.wires.is_empty() {
            return BoundingBox3D::default();
        }
        let mut min = Vector3::new(f64::MAX, f64::MAX, f64::MAX);
        let mut max = Vector3::new(f64::MIN, f64::MIN, f64::MIN);
        for wire in &self.wires {
            for pt in &wire.points {
                min.x = min.x.min(pt.x);
                min.y = min.y.min(pt.y);
                min.z = min.z.min(pt.z);
                max.x = max.x.max(pt.x);
                max.y = max.y.max(pt.y);
                max.z = max.z.max(pt.z);
            }
        }
        BoundingBox3D::new(min, max)
    }
    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_surface(self, offset);
    }
    fn entity_type(&self) -> &'static str {
        "SURFACE"
    }
    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_surface(self, transform);
    }
}
