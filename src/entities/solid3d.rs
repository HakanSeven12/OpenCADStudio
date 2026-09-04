// Grippable + PropertyEditable for Solid3D, Region, Body.
//
// Shared grips and properties for modeler entities.

use acadrust::entities::{Body, Region, Solid3D, Surface};
use cadkernel::space::polygon;
use crate::t;
use crate::command::EntityTransform;
use crate::entities::common::{center_grip, edit_prop as edit, parse_f64, ro_prop as ro};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable};
use crate::scene::model::object::{GripApply, GripDef, PropSection};

/// Shared transform for the ACIS volume entities. Translate / rotate / scale
/// delegate to acadrust (which composes the move into the solid's ACIS
/// placement), and mirror delegates via a reflection transform. Without this
/// the entity dispatcher treated solids as non-transformable, so a moved or
/// pasted solid stayed at its original ACIS placement.
macro_rules! impl_acis_transformable {
    ($ty:ty) => {
        impl Transformable for $ty {
            fn apply_transform(&mut self, t: &EntityTransform) {
                crate::scene::view::transform::apply_standard_entity_transform(self, t, |e, p1, p2| {
                    let m = crate::scene::view::transform::reflection_about_xy_line(p1, p2);
                    acadrust::Entity::apply_transform(e, &m);
                });
            }
        }
    };
}
impl_acis_transformable!(Solid3D);
impl_acis_transformable!(Region);
impl_acis_transformable!(Body);
impl_acis_transformable!(Surface);

// ── shared helpers ────────────────────────────────────────────────────────────

fn dvec3(v: &acadrust::types::Vector3) -> glam::DVec3 {
    glam::DVec3::new(v.x, v.y, v.z)
}

fn translate_acis_entity<T: acadrust::Entity>(entity: &mut T, d: glam::DVec3) {
    acadrust::Entity::translate(
        entity,
        acadrust::types::Vector3::new(d.x, d.y, d.z),
    );
}

fn yes_no(value: bool) -> &'static str {
    if value { "Yes" } else { "No" }
}

fn handle_text(handle: Option<acadrust::Handle>) -> String {
    handle
        .filter(|handle| handle.is_valid())
        .map(|handle| format!("{:X}", handle.value()))
        .unwrap_or_else(|| "None".to_string())
}

fn acis_sections(
    acis: &acadrust::entities::AcisData,
    wires: &[acadrust::entities::Wire],
    silhouettes: &[acadrust::entities::Silhouette],
    history: Option<acadrust::Handle>,
) -> Vec<PropSection> {
    let mut wire_types = [0usize; 5];
    let mut transformed = 0usize;
    let mut points = 0usize;
    for wire in wires {
        let index = match wire.wire_type {
            acadrust::entities::WireType::Silhouette => 1,
            acadrust::entities::WireType::VisibleEdge => 2,
            acadrust::entities::WireType::HiddenEdge => 3,
            acadrust::entities::WireType::Isoline => 4,
            _ => 0,
        };
        wire_types[index] += 1;
        transformed += wire.has_transform as usize;
        points += wire.points.len();
    }
    let revision = if acis.revision.has_guid {
        format!(
            "{}-{}-{}-{:02X?}; end {}",
            acis.revision.major,
            acis.revision.minor1,
            acis.revision.minor2,
            acis.revision.bytes,
            acis.revision.end_marker
        )
    } else {
        "None".to_string()
    };
    let bindings = if acis.materials.is_empty() {
        "None".to_string()
    } else {
        acis.materials
            .iter()
            .map(|binding| {
                format!(
                    "{}:{}→{}",
                    binding.array_index,
                    binding.absolute_reference,
                    handle_text(binding.material_handle)
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let extra = acis.extra_acis_data.as_ref().map_or_else(
        || "None".to_string(),
        |data| {
            format!(
                "{}; {} bytes",
                if data.is_binary { "SAB" } else { "SAT" },
                data.size()
            )
        },
    );
    vec![
        PropSection {
            title: t!("Modeler Geometry").into_owned(),
            props: vec![
                ro(t!("Format").as_ref(),
                    "acis_format",
                    if acis.is_binary { "SAB" } else { "SAT" },
                ),
                ro(t!("Version").as_ref(), "acis_version", format!("{:?}", acis.version)),
                ro(t!("Data Size").as_ref(), "acis_size", format!("{} bytes", acis.size())),
                ro(t!("Revision").as_ref(), "acis_revision", revision),
                ro(t!("Material Bindings").as_ref(), "acis_materials", bindings),
                ro(t!("Extra Modeler Data").as_ref(), "acis_extra", extra),
                ro(t!("History").as_ref(), "acis_history", handle_text(history)),
            ],
        },
        PropSection {
            title: t!("Modeler Display Data").into_owned(),
            props: vec![
                ro(t!("Wireframe Cache").as_ref(),
                    "acis_wireframe",
                    yes_no(acis.wireframe_data_present),
                ),
                ro(t!("Reference Point").as_ref(),
                    "acis_wireframe_point",
                    yes_no(acis.wireframe_point_present),
                ),
                ro(t!("Isoline List").as_ref(),
                    "acis_wireframe_isolines",
                    yes_no(acis.wireframe_isoline_present),
                ),
                ro(t!("Isolines").as_ref(),
                    "acis_isolines",
                    acis.wireframe_isolines.to_string(),
                ),
                ro(t!("Wires").as_ref(),
                    "acis_wires",
                    format!(
                        "{} (unknown {}, silhouette {}, visible {}, hidden {}, isoline {}; {} points; {} transformed)",
                        wires.len(),
                        wire_types[0],
                        wire_types[1],
                        wire_types[2],
                        wire_types[3],
                        wire_types[4],
                        points,
                        transformed
                    ),
                ),
                ro(t!("View Silhouettes").as_ref(),
                    "acis_silhouettes",
                    format!(
                        "{} views; {} wires",
                        silhouettes.len(),
                        silhouettes.iter().map(|silhouette| silhouette.wires.len()).sum::<usize>()
                    ),
                ),
            ],
        },
    ]
}

fn position_section(prefix: &str, p: &acadrust::types::Vector3) -> PropSection {
    let fields = match prefix {
        "rgn" => ["rgn_px", "rgn_py", "rgn_pz"],
        "bdy" => ["bdy_px", "bdy_py", "bdy_pz"],
        "srf" => ["srf_px", "srf_py", "srf_pz"],
        _ => ["s3d_px", "s3d_py", "s3d_pz"],
    };
    PropSection {
        title: t!("Geometry").into_owned(),
        props: vec![
            edit(t!("Position X").as_ref(), fields[0], p.x),
            edit(t!("Position Y").as_ref(), fields[1], p.y),
            edit(t!("Position Z").as_ref(), fields[2], p.z),
        ],
    }
}

/// Approximate a region's enclosed area and boundary perimeter from its
/// wireframe loops. Perimeter is the total edge length across every wire.
/// Area accumulates the Newell area vector of each loop (opposite-wound
/// holes subtract) and halves its magnitude — exact for a single planar
/// loop, approximate for multi-loop or curved regions. Returns zeros when
/// there is nothing to measure.
/// The area and perimeter of a region's boundary wires.
///
/// Measured in space rather than in projection: a region need not lie in a
/// coordinate plane, and flattening it to XY first would report its shadow.
fn region_area_perimeter(wires: &[acadrust::entities::Wire]) -> (f64, f64) {
    let mut area_vector = [0.0f64; 3];
    let mut perimeter = 0.0;
    for wire in wires {
        let ring: Vec<[f64; 3]> = wire.points.iter().map(|p| [p.x, p.y, p.z]).collect();
        // Wires arrive as open chains that the region closes between them, so
        // the length is the chain's and the area vectors are summed before
        // being measured — one wire's contribution is not an area on its own.
        perimeter += polygon::chain_length(&ring);
        let piece = polygon::area_vector(&ring);
        for axis in 0..3 {
            area_vector[axis] += piece[axis];
        }
    }
    let area = (area_vector[0].powi(2) + area_vector[1].powi(2) + area_vector[2].powi(2)).sqrt();
    (area, perimeter)
}

// ── Solid3D ───────────────────────────────────────────────────────────────────

impl Grippable for Solid3D {
    fn grips(&self) -> Vec<GripDef> {
        vec![center_grip(0, dvec3(&self.point_of_reference))]
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if grip_id != 0 {
            return;
        }
        if let GripApply::Translate(d) = apply {
            translate_acis_entity(self, d);
        }
    }
}

impl PropertyEditable for Solid3D {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        let mut sections =
            acis_sections(&self.acis_data, &self.wires, &self.silhouettes, self.history_handle);
        sections[0]
            .props
            .insert(0, ro(t!("UID").as_ref(), "s3d_uid", self.uid.clone()));
        sections.push(position_section("s3d", &self.point_of_reference));
        sections
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        let Some(v) = parse_f64(value) else {
            return;
        };
        let delta = match field {
            "s3d_px" => acadrust::types::Vector3::new(v - self.point_of_reference.x, 0.0, 0.0),
            "s3d_py" => acadrust::types::Vector3::new(0.0, v - self.point_of_reference.y, 0.0),
            "s3d_pz" => acadrust::types::Vector3::new(0.0, 0.0, v - self.point_of_reference.z),
            _ => return,
        };
        if delta != acadrust::types::Vector3::ZERO {
            acadrust::Entity::translate(self, delta);
        }
    }
}

// ── Region ────────────────────────────────────────────────────────────────────

impl Grippable for Region {
    fn grips(&self) -> Vec<GripDef> {
        vec![center_grip(0, dvec3(&self.point_of_reference))]
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if grip_id != 0 {
            return;
        }
        if let GripApply::Translate(d) = apply {
            translate_acis_entity(self, d);
        }
    }
}

impl PropertyEditable for Region {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        let (area, perimeter) = region_area_perimeter(&self.wires);
        let mut sections =
            acis_sections(&self.acis_data, &self.wires, &self.silhouettes, self.history_handle);
        sections[0]
            .props
            .insert(0, ro(t!("UID").as_ref(), "rgn_uid", self.uid.clone()));
        let mut geometry = position_section("rgn", &self.point_of_reference);
        geometry
            .props
            .push(ro(t!("Area").as_ref(), "rgn_area", format!("{area:.4}")));
        geometry
            .props
            .push(ro(t!("Perimeter").as_ref(), "rgn_perimeter", format!("{perimeter:.4}")));
        sections.push(geometry);
        sections
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        let Some(v) = parse_f64(value) else {
            return;
        };
        let delta = match field {
            "rgn_px" => acadrust::types::Vector3::new(v - self.point_of_reference.x, 0.0, 0.0),
            "rgn_py" => acadrust::types::Vector3::new(0.0, v - self.point_of_reference.y, 0.0),
            "rgn_pz" => acadrust::types::Vector3::new(0.0, 0.0, v - self.point_of_reference.z),
            _ => return,
        };
        if delta != acadrust::types::Vector3::ZERO {
            acadrust::Entity::translate(self, delta);
        }
    }
}

// ── Body ──────────────────────────────────────────────────────────────────────

impl Grippable for Body {
    fn grips(&self) -> Vec<GripDef> {
        vec![center_grip(0, dvec3(&self.point_of_reference))]
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if grip_id != 0 {
            return;
        }
        if let GripApply::Translate(d) = apply {
            translate_acis_entity(self, d);
        }
    }
}

impl PropertyEditable for Body {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        let mut sections =
            acis_sections(&self.acis_data, &self.wires, &self.silhouettes, self.history_handle);
        sections[0]
            .props
            .insert(0, ro(t!("UID").as_ref(), "bdy_uid", self.uid.clone()));
        sections.push(position_section("bdy", &self.point_of_reference));
        sections
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        let Some(v) = parse_f64(value) else {
            return;
        };
        let delta = match field {
            "bdy_px" => acadrust::types::Vector3::new(v - self.point_of_reference.x, 0.0, 0.0),
            "bdy_py" => acadrust::types::Vector3::new(0.0, v - self.point_of_reference.y, 0.0),
            "bdy_pz" => acadrust::types::Vector3::new(0.0, 0.0, v - self.point_of_reference.z),
            _ => return,
        };
        if delta != acadrust::types::Vector3::ZERO {
            acadrust::Entity::translate(self, delta);
        }
    }
}

fn embedded_name(entity: Option<&acadrust::entities::EmbeddedEntity>) -> &'static str {
    match entity {
        Some(acadrust::entities::EmbeddedEntity::Point(_)) => "Point",
        Some(acadrust::entities::EmbeddedEntity::Line(_)) => "Line",
        Some(acadrust::entities::EmbeddedEntity::Arc(_)) => "Arc",
        Some(acadrust::entities::EmbeddedEntity::Circle(_)) => "Circle",
        Some(acadrust::entities::EmbeddedEntity::Ellipse(_)) => "Ellipse",
        Some(acadrust::entities::EmbeddedEntity::Spline(_)) => "Spline",
        Some(acadrust::entities::EmbeddedEntity::LwPolyline(_)) => "Polyline",
        Some(acadrust::entities::EmbeddedEntity::Region(_)) => "Region",
        Some(acadrust::entities::EmbeddedEntity::Ray(_)) => "Ray",
        Some(acadrust::entities::EmbeddedEntity::XLine(_)) => "XLine",
        Some(acadrust::entities::EmbeddedEntity::Unknown { .. }) => "Unknown",
        None => "None",
    }
}

fn vector_text(vector: &acadrust::types::Vector3) -> String {
    format!("{:.6}, {:.6}, {:.6}", vector.x, vector.y, vector.z)
}

fn matrix_text(matrix: &[f64; 16]) -> String {
    matrix
        .chunks_exact(4)
        .map(|row| {
            format!(
                "[{:.4}, {:.4}, {:.4}, {:.4}]",
                row[0], row[1], row[2], row[3]
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn sweep_options_text(options: &acadrust::entities::SurfaceSweepOptions) -> String {
    format!(
        "draft {:.6} ({:.6}→{:.6}); twist {:.6}; scale {:.6}; align {:.6}; solid {}; flags {}/{}; align-start {}; bank {}; base {}; sweep-xform {}; path-xform {}; ref {}",
        options.draft_angle,
        options.draft_start_distance,
        options.draft_end_distance,
        options.twist_angle,
        options.scale_factor,
        options.align_angle,
        yes_no(options.is_solid),
        options.sweep_alignment_flags,
        options.path_flags,
        yes_no(options.align_start),
        yes_no(options.bank),
        yes_no(options.base_point_set),
        yes_no(options.sweep_entity_transform_computed),
        yes_no(options.path_entity_transform_computed),
        vector_text(&options.reference_vector)
    )
}

fn surface_construction_section(surface: &Surface) -> PropSection {
    use acadrust::entities::SurfaceData;
    let mut props = vec![
        ro(t!("Kind").as_ref(), "srf_kind", format!("{:?}", surface.kind)),
        ro(t!("Modeler Format").as_ref(),
            "srf_modeler_version",
            surface.modeler_format_version.to_string(),
        ),
        edit(t!("U Isolines").as_ref(),
            "srf_u_isolines",
            surface.u_isolines as f64,
        ),
        edit(t!("V Isolines").as_ref(),
            "srf_v_isolines",
            surface.v_isolines as f64,
        ),
    ];
    match &surface.surface_data {
        SurfaceData::Generic => {}
        SurfaceData::Plane { class_version } => {
            props.push(ro(t!("Class Version").as_ref(),
                "srf_class_version",
                class_version.to_string(),
            ));
        }
        SurfaceData::Extruded {
            sweep_entity,
            options,
            sweep_vector,
            sweep_transform,
        } => {
            props.extend([
                ro(t!("Sweep Entity").as_ref(),
                    "srf_sweep_entity",
                    embedded_name(sweep_entity.as_ref()),
                ),
                ro(t!("Sweep Vector").as_ref(), "srf_sweep_vector", vector_text(sweep_vector)),
                ro(t!("Sweep Options").as_ref(), "srf_sweep_options", sweep_options_text(options)),
                ro(t!("Sweep Transform").as_ref(),
                    "srf_sweep_transform",
                    matrix_text(sweep_transform),
                ),
                ro(t!("Sweep Entity Transform").as_ref(),
                    "srf_sweep_entity_transform",
                    matrix_text(&options.sweep_entity_transform),
                ),
                ro(t!("Path Entity Transform").as_ref(),
                    "srf_path_entity_transform",
                    matrix_text(&options.path_entity_transform),
                ),
            ]);
        }
        SurfaceData::Lofted {
            loft_transform,
            cross_section_entities,
            guide_entities,
            path_entity,
            plane_normal_lofting_type,
            start_draft_angle,
            end_draft_angle,
            start_draft_magnitude,
            end_draft_magnitude,
            arc_length_parameterization,
            no_twist,
            align_direction,
            simple_surfaces,
            closed_surfaces,
            solid,
            ruled_surface,
            virtual_guide,
            cross_sections,
            guide_curves,
            path_curve,
        } => {
            props.extend([
                ro(t!("Embedded Curves").as_ref(),
                    "srf_loft_embedded",
                    format!(
                        "{} cross sections; {} guides; path {}",
                        cross_section_entities.len(),
                        guide_entities.len(),
                        embedded_name(path_entity.as_ref())
                    ),
                ),
                ro(t!("Database Curves").as_ref(),
                    "srf_loft_handles",
                    format!(
                        "{} cross sections; {} guides; path {}",
                        cross_sections.len(),
                        guide_curves.len(),
                        handle_text(*path_curve)
                    ),
                ),
                ro(t!("Draft").as_ref(),
                    "srf_loft_draft",
                    format!(
                        "type {}; angle {:.6}→{:.6}; magnitude {:.6}→{:.6}",
                        plane_normal_lofting_type,
                        start_draft_angle,
                        end_draft_angle,
                        start_draft_magnitude,
                        end_draft_magnitude
                    ),
                ),
                ro(t!("Loft Flags").as_ref(),
                    "srf_loft_flags",
                    format!(
                        "arc-length {}; no-twist {}; align {}; simple {}; closed {}; solid {}; ruled {}; virtual-guide {}",
                        yes_no(*arc_length_parameterization),
                        yes_no(*no_twist),
                        yes_no(*align_direction),
                        yes_no(*simple_surfaces),
                        yes_no(*closed_surfaces),
                        yes_no(*solid),
                        yes_no(*ruled_surface),
                        yes_no(*virtual_guide)
                    ),
                ),
                ro(t!("Loft Transform").as_ref(),
                    "srf_loft_transform",
                    matrix_text(loft_transform),
                ),
            ]);
        }
        SurfaceData::Revolved {
            revolve_entity,
            class_version,
            entity_id,
            axis_point,
            axis_vector,
            revolve_angle,
            start_angle,
            entity_transform,
            draft_angle,
            draft_start_distance,
            draft_end_distance,
            twist_angle,
            solid,
            close_to_axis,
        } => {
            props.extend([
                ro(t!("Revolve Entity").as_ref(),
                    "srf_revolve_entity",
                    embedded_name(revolve_entity.as_ref()),
                ),
                ro(t!("Class / Entity").as_ref(),
                    "srf_revolve_ids",
                    format!("{class_version} / {entity_id}"),
                ),
                ro(t!("Axis Point").as_ref(), "srf_axis_point", vector_text(axis_point)),
                ro(t!("Axis Vector").as_ref(), "srf_axis_vector", vector_text(axis_vector)),
                ro(t!("Angles").as_ref(),
                    "srf_revolve_angles",
                    format!(
                        "start {:.6}; revolve {:.6}; draft {:.6}; twist {:.6}",
                        start_angle, revolve_angle, draft_angle, twist_angle
                    ),
                ),
                ro(t!("Draft Distances").as_ref(),
                    "srf_revolve_draft_distances",
                    format!("{draft_start_distance:.6}→{draft_end_distance:.6}"),
                ),
                ro(t!("Revolve Flags").as_ref(),
                    "srf_revolve_flags",
                    format!(
                        "solid {}; close-to-axis {}",
                        yes_no(*solid),
                        yes_no(*close_to_axis)
                    ),
                ),
                ro(t!("Entity Transform").as_ref(),
                    "srf_revolve_transform",
                    matrix_text(entity_transform),
                ),
            ]);
        }
        SurfaceData::Swept {
            class_version,
            sweep_entity,
            path_entity,
            sweep_transform,
            path_transform,
            options,
        } => {
            props.extend([
                ro(t!("Class Version").as_ref(),
                    "srf_class_version",
                    class_version.to_string(),
                ),
                ro(t!("Sweep / Path").as_ref(),
                    "srf_swept_entities",
                    format!(
                        "{} / {}",
                        embedded_name(sweep_entity.as_ref()),
                        embedded_name(path_entity.as_ref())
                    ),
                ),
                ro(t!("Sweep Options").as_ref(), "srf_sweep_options", sweep_options_text(options)),
                ro(t!("Sweep Transform").as_ref(),
                    "srf_sweep_transform",
                    matrix_text(sweep_transform),
                ),
                ro(t!("Path Transform").as_ref(),
                    "srf_path_transform",
                    matrix_text(path_transform),
                ),
                ro(t!("Sweep Entity Transform").as_ref(),
                    "srf_sweep_entity_transform",
                    matrix_text(&options.sweep_entity_transform),
                ),
                ro(t!("Path Entity Transform").as_ref(),
                    "srf_path_entity_transform",
                    matrix_text(&options.path_entity_transform),
                ),
            ]);
        }
        SurfaceData::Nurb {
            short_170,
            cv_hull_display,
            u_vector1,
            v_vector1,
            u_vector2,
            v_vector2,
        } => {
            props.extend([
                ro(t!("NURB Version").as_ref(),
                    "srf_nurb_version",
                    short_170.to_string(),
                ),
                ro(t!("CV Hull").as_ref(),
                    "srf_nurb_cv_hull",
                    yes_no(*cv_hull_display),
                ),
                ro(t!("U Vector 1").as_ref(), "srf_nurb_u1", vector_text(u_vector1)),
                ro(t!("V Vector 1").as_ref(), "srf_nurb_v1", vector_text(v_vector1)),
                ro(t!("U Vector 2").as_ref(), "srf_nurb_u2", vector_text(u_vector2)),
                ro(t!("V Vector 2").as_ref(), "srf_nurb_v2", vector_text(v_vector2)),
            ]);
        }
    }
    PropSection {
        title: t!("Surface Construction").into_owned(),
        props,
    }
}

fn translate_surface(surface: &mut Surface, delta: acadrust::types::Vector3) {
    let before = surface.point_of_reference;
    acadrust::Entity::translate(surface, delta);
    if surface.point_of_reference == before {
        surface.point_of_reference = before + delta;
    }
}

impl Grippable for Surface {
    fn grips(&self) -> Vec<GripDef> {
        vec![center_grip(0, dvec3(&self.point_of_reference))]
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if grip_id != 0 {
            return;
        }
        if let GripApply::Translate(delta) = apply {
            translate_surface(
                self,
                acadrust::types::Vector3::new(delta.x, delta.y, delta.z),
            );
        }
    }
}

impl PropertyEditable for Surface {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        let mut sections =
            acis_sections(&self.acis_data, &self.wires, &self.silhouettes, self.history_handle);
        sections.push(surface_construction_section(self));
        sections.push(position_section("srf", &self.point_of_reference));
        sections
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        match field {
            "srf_u_isolines" => {
                if let Some(value) = parse_f64(value) {
                    self.u_isolines = (value.round() as i16).max(0);
                }
            }
            "srf_v_isolines" => {
                if let Some(value) = parse_f64(value) {
                    self.v_isolines = (value.round() as i16).max(0);
                }
            }
            _ => {
                let Some(value) = parse_f64(value) else {
                    return;
                };
                let delta = match field {
                    "srf_px" => acadrust::types::Vector3::new(
                        value - self.point_of_reference.x,
                        0.0,
                        0.0,
                    ),
                    "srf_py" => acadrust::types::Vector3::new(
                        0.0,
                        value - self.point_of_reference.y,
                        0.0,
                    ),
                    "srf_pz" => acadrust::types::Vector3::new(
                        0.0,
                        0.0,
                        value - self.point_of_reference.z,
                    ),
                    _ => return,
                };
                if delta != acadrust::types::Vector3::ZERO {
                    translate_surface(self, delta);
                }
            }
        }
    }
}

// ── Accessors for the Solid3D / Region / Body trio ─────────────────────────
//
// These entity types share ACIS data and a point of reference.

use crate::scene::model::mesh_model::MeshLodSet;
use crate::scene::convert::solid3d_tess;
use acadrust::{types::Vector3, EntityType};

const DISPLAY_DEFLECTION_COEFFICIENT: f64 = 2.5e-4;

/// Shared world-space chord tolerance for solid display.
pub fn display_deflection(
    header: &acadrust::document::HeaderVariables,
    facet_res: f64,
) -> Option<f64> {
    let low = header.model_space_extents_min;
    let high = header.model_space_extents_max;
    let spans = [high.x - low.x, high.y - low.y, high.z - low.z];
    let span = spans
        .into_iter()
        .filter(|value| value.is_finite() && *value > 0.0)
        .fold(0.0, f64::max);
    if !span.is_finite() || span <= 0.0 || span > 1.0e16 {
        return None;
    }
    let resolution = if facet_res.is_finite() && facet_res > 0.0 {
        facet_res.clamp(0.01, 10.0)
    } else {
        1.0
    };
    Some(
        (span * DISPLAY_DEFLECTION_COEFFICIENT / resolution.max(1.0).sqrt()).max(1e-9),
    )
}

/// `point_of_reference` of an ACIS-backed volume entity, if applicable.
pub fn point_of_reference(e: &EntityType) -> Option<&Vector3> {
    match e {
        EntityType::Solid3D(s) => Some(&s.point_of_reference),
        EntityType::Region(r) => Some(&r.point_of_reference),
        EntityType::Body(b) => Some(&b.point_of_reference),
        EntityType::Surface(s) => Some(&s.point_of_reference),
        _ => None,
    }
}

/// Build material-aware shaded geometry for every standard 3-D solid/surface
/// and mesh family, returning `None` when decoded geometry is unusable.
pub fn tessellate_volume(
    e: &EntityType,
    color: [f32; 4],
    facet_res: f64,
    chordal_deflection: Option<f64>,
    isolines: usize,
) -> Option<MeshLodSet> {
    match e {
        EntityType::Solid3D(s) => solid3d_tess::tessellate_solid3d(
            s,
            color,
            facet_res,
            chordal_deflection,
            isolines,
        ),
        EntityType::Region(r) => solid3d_tess::tessellate_region(
            r,
            color,
            facet_res,
            chordal_deflection,
            isolines,
        ),
        EntityType::Body(b) => solid3d_tess::tessellate_body(
            b,
            color,
            facet_res,
            chordal_deflection,
            isolines,
        ),
        EntityType::Surface(s) => solid3d_tess::tessellate_surface(
            s,
            color,
            facet_res,
            chordal_deflection,
            isolines,
        ),
        EntityType::Mesh(_) | EntityType::PolygonMesh(_) | EntityType::PolyfaceMesh(_) => {
            crate::entities::mesh::tessellate_shaded_mesh(e, color)
        }
        _ => None,
    }
}
