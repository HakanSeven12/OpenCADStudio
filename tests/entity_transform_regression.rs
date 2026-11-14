// Pins what every `EntityTransform` does to every common entity type.
//
// `snapshot_matches_golden` applies each transform (and
// `WorkingPlane::place_entity`) to a fixed set of entities and compares the
// resulting geometry with `tests/fixtures/entity_transform/golden.txt`. Code
// that only moves or re-exports transform code must leave that file
// unchanged. A change that is meant to alter transform behaviour regenerates
// it with
//
//     OCS_UPDATE_GOLDEN=1 cargo test --locked --test integration entity_transform_regression
//
// and the diff of the golden file shows exactly what changed. Numbers are
// compared with a small tolerance, so last-bit differences between the
// platforms' maths libraries don't fail the test.
//
// The other tests check results against independent math: every point an
// entity stores in world coordinates, plus circle and arc in their own plane.
// For those, the golden file can't quietly record a wrong answer.

use std::f64::consts::FRAC_PI_2;
use std::fmt::Write as _;

use document::entities::{
    Arc, AttributeDefinition, BoundaryEdge, BoundaryPath, Circle, CircularArcEdge, Dimension,
    DimensionAligned, DimensionAngular2Ln, DimensionAngular3Pt, DimensionArc, DimensionBase,
    DimensionDiameter, DimensionLargeRadial, DimensionLinear, DimensionOrdinate, DimensionRadius,
    Ellipse, Face3D, Hatch, HatchPatternLine, Helix, Insert, Leader, Light, Line, LineEdge,
    LwPolyline, LwVertex, MText, Mesh, Point, PolyfaceMesh, PolygonMeshEntity, PolygonMeshVertex,
    Polyline2D, Polyline3D, PolylineEdge, Ray, Shape, Solid, Spline, Text, Tolerance, Vertex2D,
    XLine,
};
use document::types::{Vector2, Vector3};
use document::EntityType;
use glam::{DQuat, DVec3};
use OpenCADStudio::entities::transform::{EntityTransform, WorkingPlane};
use OpenCADStudio::scene::view::dispatch::apply_transform;

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/entity_transform/golden.txt"
);

// ── Inputs ────────────────────────────────────────────────────────────────

fn v3(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x, y, z)
}

/// A frame tilted out of the XY plane and far from the origin, so a
/// transform that ignores the origin or the tilt shows up in the numbers.
fn tilted_plane() -> WorkingPlane {
    WorkingPlane::new(
        DVec3::new(100.0, -50.0, 25.0),
        DVec3::new(0.0, 1.0, 0.0),
        DVec3::new(0.0, 0.0, 1.0),
    )
}

fn transforms() -> Vec<(&'static str, EntityTransform)> {
    vec![
        (
            "translate",
            EntityTransform::Translate(DVec3::new(12.5, -7.25, 3.0)),
        ),
        (
            "rotate_z",
            EntityTransform::Rotate {
                center: DVec3::new(5.0, 5.0, 0.0),
                axis: DVec3::Z,
                angle_rad: 0.6,
            },
        ),
        (
            "rotate_x",
            EntityTransform::Rotate {
                center: DVec3::new(0.0, 2.0, 1.0),
                axis: DVec3::X,
                angle_rad: FRAC_PI_2,
            },
        ),
        (
            "scale",
            EntityTransform::Scale {
                center: DVec3::new(2.0, -3.0, 0.0),
                factor: 2.5,
            },
        ),
        (
            "mirror",
            EntityTransform::Mirror {
                p1: DVec3::new(0.0, 0.0, 0.0),
                p2: DVec3::new(1.0, 2.0, 0.0),
                working_normal: DVec3::Z,
            },
        ),
        (
            "affine_tilted",
            EntityTransform::Affine(tilted_plane().to_world_transform()),
        ),
        (
            // The mirror above passes through the origin, where reflecting a
            // point and reflecting a direction give the same answer. This one
            // doesn't, so a field treated as the wrong kind shows up.
            "mirror_offset",
            EntityTransform::Mirror {
                p1: DVec3::new(3.0, 1.0, 0.0),
                p2: DVec3::new(5.0, 4.0, 0.0),
                working_normal: DVec3::Z,
            },
        ),
        (
            // A mirror drawn in a UCS that isn't parallel to world XY takes a
            // different code path from the Z-normal mirror above.
            "mirror_tilted",
            EntityTransform::Mirror {
                p1: DVec3::new(100.0, -50.0, 25.0),
                p2: DVec3::new(100.0, -49.0, 27.0),
                working_normal: tilted_plane().z,
            },
        ),
    ]
}

fn entities() -> Vec<(&'static str, EntityType)> {
    let mut ellipse = Ellipse::from_center_axes(v3(6.0, -2.0, 0.0), v3(4.0, 1.0, 0.0), 0.4);
    ellipse.start_parameter = 0.5;
    ellipse.end_parameter = 4.0;

    let mut pline = LwPolyline::new();
    for (x, y, bulge, width) in [
        (0.0, 0.0, 0.0, 0.0),
        (5.0, 0.0, 0.5, 0.25),
        (5.0, 4.0, -0.3, 0.0),
        (1.0, 3.0, 0.0, 0.0),
    ] {
        let mut vertex = LwVertex::new(Vector2::new(x, y));
        vertex.bulge = bulge;
        vertex.start_width = width;
        vertex.end_width = width;
        pline.vertices.push(vertex);
    }
    pline.is_closed = true;
    pline.elevation = 1.25;

    let mut text = Text::with_value("ABC", v3(2.0, 3.0, 0.0));
    text.height = 1.5;
    text.rotation = 0.25;
    text.width_factor = 0.8;
    text.oblique_angle = 0.1;

    let mut mtext = MText::with_value("Line one\\PLine two", v3(-4.0, 6.0, 0.0));
    mtext.height = 2.0;
    mtext.rectangle_width = 12.0;
    mtext.rotation = 0.4;

    let mut insert = Insert::new("BLOCK_A", v3(10.0, 5.0, 0.0));
    insert.set_x_scale(2.0);
    insert.set_y_scale(3.0);
    insert.set_z_scale(1.0);
    insert.rotation = 0.35;

    let mut spline = Spline::from_control_points(
        3,
        vec![
            v3(0.0, 0.0, 0.0),
            v3(2.0, 4.0, 0.0),
            v3(5.0, -1.0, 0.0),
            v3(8.0, 3.0, 0.0),
        ],
    );
    spline.fit_points = vec![v3(0.0, 0.0, 0.0), v3(4.0, 2.0, 0.0), v3(8.0, 3.0, 0.0)];

    let mut polyline = Polyline2D::new();
    polyline.add_vertex(Vertex2D::new(v3(0.0, 0.0, 0.0)).with_width(0.2, 0.4));
    polyline.add_vertex(Vertex2D::new(v3(6.0, 0.0, 0.0)).with_bulge(0.4));
    polyline.add_vertex(Vertex2D::new(v3(6.0, 5.0, 0.0)).with_bulge(-0.2));
    polyline.add_vertex(Vertex2D::new(v3(2.0, 5.0, 0.0)));
    polyline.elevation = 0.75;

    let mut hatch = Hatch::new();
    hatch.is_solid = false;
    hatch.pattern.name = "ANSI31".into();
    hatch.pattern.lines = vec![HatchPatternLine {
        angle: std::f64::consts::FRAC_PI_4,
        base_point: Vector2::new(0.5, 0.25),
        offset: Vector2::new(-1.0, 1.0),
        dash_lengths: vec![2.0, -0.5],
    }];
    hatch.pattern_angle = std::f64::consts::FRAC_PI_4;
    hatch.pattern_scale = 2.0;
    hatch.elevation = 0.5;
    let mut outer = BoundaryPath::new();
    for (a, b) in [((0.0, 0.0), (8.0, 0.0)), ((8.0, 0.0), (8.0, 6.0))] {
        outer.add_edge(BoundaryEdge::Line(LineEdge {
            start: Vector2::new(a.0, a.1),
            end: Vector2::new(b.0, b.1),
        }));
    }
    outer.add_edge(BoundaryEdge::CircularArc(CircularArcEdge {
        center: Vector2::new(4.0, 6.0),
        radius: 4.0,
        start_angle: 0.0,
        end_angle: std::f64::consts::PI,
        counter_clockwise: true,
    }));
    outer.add_edge(BoundaryEdge::Line(LineEdge {
        start: Vector2::new(0.0, 6.0),
        end: Vector2::new(0.0, 0.0),
    }));
    hatch.add_path(outer);
    let mut island = PolylineEdge::new(Vec::new(), true);
    island.add_vertex(Vector2::new(2.0, 2.0), 0.0);
    island.add_vertex(Vector2::new(5.0, 2.0), 0.5);
    island.add_vertex(Vector2::new(5.0, 4.0), 0.0);
    let mut inner = BoundaryPath::new();
    inner.add_edge(BoundaryEdge::Polyline(island));
    hatch.add_path(inner);
    hatch.add_seed_point(Vector2::new(1.0, 1.0));

    let mut linear = DimensionLinear::new(v3(0.0, 0.0, 0.0), v3(10.0, 0.0, 0.0));
    linear.definition_point = v3(10.0, 4.0, 0.0);
    linear.base.definition_point = linear.definition_point;
    linear.base.text_middle_point = v3(5.0, 4.5, 0.0);

    let mut aligned = DimensionAligned::new(v3(1.0, 1.0, 0.0), v3(7.0, 5.0, 0.0));
    aligned.definition_point = v3(5.0, 8.0, 0.0);
    aligned.base.definition_point = aligned.definition_point;
    aligned.base.text_middle_point = v3(3.5, 6.0, 0.0);

    let mut radius = DimensionRadius::new(v3(3.0, 3.0, 0.0), v3(6.0, 7.0, 0.0));
    radius.base.definition_point = radius.angle_vertex;
    radius.base.text_middle_point = v3(7.0, 8.5, 0.0);

    let mut angular =
        DimensionAngular3Pt::new(v3(0.0, 0.0, 0.0), v3(6.0, 0.0, 0.0), v3(3.0, 5.0, 0.0));
    angular.definition_point = v3(3.0, 2.0, 0.0);
    angular.base.definition_point = angular.definition_point;
    angular.base.text_middle_point = v3(4.0, 2.5, 0.0);

    let mut diameter = DimensionDiameter::new(v3(2.0, 2.0, 0.0), v3(8.0, 6.0, 0.0));
    diameter.base.text_middle_point = v3(9.0, 7.0, 0.0);
    diameter.base.text_rotation = 0.2;
    diameter.leader_length = 1.5;

    let mut ordinate = DimensionOrdinate::new(v3(4.0, 3.0, 0.0), v3(4.0, 9.0, 0.0), true);
    ordinate.base.definition_point = v3(0.0, 0.0, 0.0);
    ordinate.base.text_middle_point = v3(4.0, 9.5, 0.0);

    let mut arc_length = DimensionArc {
        center_point: v3(1.0, 1.0, 0.0),
        first_extension_point: v3(5.0, 1.0, 0.0),
        second_extension_point: v3(1.0, 5.0, 0.0),
        definition_point: v3(4.5, 4.5, 0.0),
        arc_start_parameter: 0.0,
        arc_end_parameter: FRAC_PI_2,
        ..Default::default()
    };
    arc_length.base.definition_point = arc_length.definition_point;
    arc_length.base.text_middle_point = v3(4.8, 4.8, 0.0);

    let mut angular2 =
        DimensionAngular2Ln::new(v3(0.0, 0.0, 0.0), v3(5.0, 1.0, 0.0), v3(1.0, 5.0, 0.0));
    angular2.dimension_arc = v3(3.0, 3.0, 0.0);
    angular2.base.definition_point = angular2.definition_point;
    angular2.base.text_middle_point = v3(3.2, 3.2, 0.0);
    angular2.base.text_rotation = 0.35;

    let mut jogged = DimensionLargeRadial {
        chord_point: v3(10.0, 0.0, 0.0),
        override_center: v3(4.0, -6.0, 0.0),
        jog_point: v3(6.0, -3.0, 0.0),
        jog_angle: 0.8,
        definition_point: v3(0.0, 0.0, 0.0),
        ..Default::default()
    };
    jogged.base.definition_point = jogged.definition_point;
    jogged.base.text_middle_point = v3(5.0, -2.0, 0.0);

    let dimension = |d: Dimension| EntityType::Dimension(Box::new(d));

    let polyline3d = Polyline3D::from_points(vec![
        v3(0.0, 0.0, 0.0),
        v3(3.0, 1.0, 2.0),
        v3(6.0, -1.0, 4.0),
        v3(8.0, 2.0, 1.0),
    ]);

    let mut polyface = PolyfaceMesh::new();
    for (x, y, z) in [
        (0.0, 0.0, 0.0),
        (4.0, 0.0, 0.0),
        (4.0, 3.0, 1.0),
        (0.0, 3.0, 1.0),
    ] {
        polyface.add_vertex_xyz(x, y, z);
    }

    let mut polygon_mesh = PolygonMeshEntity::new();
    polygon_mesh.m_vertex_count = 2;
    polygon_mesh.n_vertex_count = 2;
    for (x, y, z) in [
        (0.0, 0.0, 0.0),
        (0.0, 4.0, 1.0),
        (5.0, 0.0, 2.0),
        (5.0, 4.0, 3.0),
    ] {
        polygon_mesh
            .vertices
            .push(PolygonMeshVertex::at(v3(x, y, z)));
    }

    let mesh = Mesh::from_triangles(
        vec![
            v3(0.0, 0.0, 0.0),
            v3(3.0, 0.0, 0.0),
            v3(3.0, 2.0, 1.0),
            v3(0.0, 2.0, 1.0),
        ],
        &[(0, 1, 2), (0, 2, 3)],
    );

    let mut helix = Helix::new();
    helix.axis_base_point = v3(2.0, 3.0, 0.0);
    helix.start_point = v3(5.0, 3.0, 0.0);
    helix.axis_vector = v3(0.0, 0.0, 1.0);
    helix.radius = 3.0;
    helix.turns = 2.5;
    helix.turn_height = 1.5;
    // The HELIX command builds the curve with the geometry kernel. Any control
    // points will do here: they must move with the helix like everything else.
    helix.spline = Spline::from_control_points(
        3,
        vec![
            v3(5.0, 3.0, 0.0),
            v3(2.0, 6.0, 0.5),
            v3(-1.0, 3.0, 1.0),
            v3(2.0, 0.0, 1.5),
        ],
    );

    let mut leader = Leader::from_vertices(vec![
        v3(0.0, 0.0, 0.0),
        v3(3.0, 2.0, 0.0),
        v3(6.0, 2.0, 0.0),
    ]);
    leader.text_height = 1.2;
    leader.text_width = 5.0;
    leader.horizontal_direction = v3(1.0, 0.0, 0.0);
    leader.block_offset = v3(0.5, 0.25, 0.0);
    leader.annotation_offset = v3(0.75, -0.5, 0.0);

    let mut tolerance = Tolerance::new();
    tolerance.insertion_point = v3(3.0, 4.0, 0.0);
    tolerance.direction = v3(0.8, 0.6, 0.0);
    tolerance.text = "{\\Fgdt;j}%%v0.5".into();
    tolerance.text_height = 0.5;

    let mut shape = Shape::new();
    shape.insertion_point = v3(-2.0, 3.0, 0.0);
    shape.size = 2.0;
    shape.rotation = 0.3;
    shape.relative_x_scale = 1.5;
    shape.oblique_angle = 0.2;

    let mut attdef = AttributeDefinition::new("TAG".into(), "Prompt".into(), "Value".into());
    attdef.insertion_point = v3(1.0, -2.0, 0.0);
    attdef.alignment_point = v3(4.0, -2.0, 0.0);
    attdef.height = 0.8;
    attdef.rotation = 0.45;
    attdef.width_factor = 0.9;
    attdef.oblique_angle = 0.15;

    let mut light = Light::new();
    light.position = v3(4.0, 4.0, 6.0);
    light.target = v3(4.0, 2.0, 0.0);
    light.hotspot_angle = 0.5;
    light.falloff_angle = 0.7;

    vec![
        (
            "line",
            EntityType::Line(Line::from_points(v3(1.0, 2.0, 0.0), v3(7.0, 4.0, 1.5))),
        ),
        ("point", EntityType::Point(Point::at(v3(3.0, -1.0, 2.0)))),
        (
            "circle",
            EntityType::Circle(Circle::from_center_radius(v3(4.0, 3.0, 0.0), 2.5)),
        ),
        (
            "arc",
            EntityType::Arc(Arc::from_center_radius_angles(
                v3(-2.0, 1.0, 0.0),
                3.0,
                0.3,
                2.2,
            )),
        ),
        ("ellipse", EntityType::Ellipse(ellipse)),
        ("lwpolyline", EntityType::LwPolyline(pline)),
        ("text", EntityType::Text(text)),
        ("mtext", EntityType::MText(Box::new(mtext))),
        ("insert", EntityType::Insert(insert)),
        (
            "ray",
            EntityType::Ray(Ray::new(v3(1.0, 1.0, 0.0), v3(0.6, 0.8, 0.0))),
        ),
        (
            "xline",
            EntityType::XLine(XLine::new(v3(-3.0, 2.0, 0.0), v3(0.0, 1.0, 0.0))),
        ),
        (
            "solid",
            EntityType::Solid(Solid::new(
                v3(0.0, 0.0, 0.0),
                v3(4.0, 0.0, 0.0),
                v3(0.0, 3.0, 0.0),
                v3(4.0, 3.0, 0.0),
            )),
        ),
        ("spline", EntityType::Spline(spline)),
        ("polyline2d", EntityType::Polyline2D(polyline)),
        ("hatch", EntityType::Hatch(Box::new(hatch))),
        ("dim_linear", dimension(Dimension::Linear(linear))),
        ("dim_aligned", dimension(Dimension::Aligned(aligned))),
        ("dim_radius", dimension(Dimension::Radius(radius))),
        ("dim_angular3pt", dimension(Dimension::Angular3Pt(angular))),
        ("dim_diameter", dimension(Dimension::Diameter(diameter))),
        ("dim_ordinate", dimension(Dimension::Ordinate(ordinate))),
        ("dim_arc_length", dimension(Dimension::Arc(arc_length))),
        ("dim_angular2ln", dimension(Dimension::Angular2Ln(angular2))),
        (
            "dim_jogged_radius",
            dimension(Dimension::LargeRadial(jogged)),
        ),
        ("polyline3d", EntityType::Polyline3D(polyline3d)),
        ("polyface_mesh", EntityType::PolyfaceMesh(polyface)),
        ("polygon_mesh", EntityType::PolygonMesh(polygon_mesh)),
        ("mesh", EntityType::Mesh(mesh)),
        (
            "face3d",
            EntityType::Face3D(Face3D::new(
                v3(0.0, 0.0, 0.0),
                v3(4.0, 0.0, 1.0),
                v3(4.0, 3.0, 2.0),
                v3(0.0, 3.0, 1.0),
            )),
        ),
        ("helix", EntityType::Helix(Box::new(helix))),
        ("leader", EntityType::Leader(leader)),
        ("tolerance", EntityType::Tolerance(tolerance)),
        ("shape", EntityType::Shape(shape)),
        (
            "attribute_definition",
            EntityType::AttributeDefinition(attdef),
        ),
        ("light", EntityType::Light(Box::new(light))),
    ]
}

// ── Description ───────────────────────────────────────────────────────────

/// Fixed precision, with negative zero folded into zero, so the snapshot
/// doesn't flicker on rounding noise.
fn num(value: f64) -> String {
    let text = format!("{value:.9}");
    if text
        .trim_start_matches('-')
        .bytes()
        .all(|b| b == b'0' || b == b'.')
    {
        text.trim_start_matches('-').to_string()
    } else {
        text
    }
}

fn pt(v: Vector3) -> String {
    format!("({}, {}, {})", num(v.x), num(v.y), num(v.z))
}

fn pts(list: &[Vector3]) -> String {
    list.iter().map(|v| pt(*v)).collect::<Vec<_>>().join(" ")
}

/// The geometric fields of an entity, one per line.
fn describe(entity: &EntityType) -> String {
    let mut s = String::new();
    let mut field = |name: &str, value: String| {
        let _ = writeln!(s, "  {name}: {value}");
    };
    match entity {
        EntityType::Line(e) => {
            field("start", pt(e.start));
            field("end", pt(e.end));
            field("normal", pt(e.normal));
        }
        EntityType::Point(e) => {
            field("location", pt(e.location));
            field("normal", pt(e.normal));
            field("x_axis_angle", num(e.x_axis_angle));
        }
        EntityType::Circle(e) => {
            field("center", pt(e.center));
            field("radius", num(e.radius));
            field("normal", pt(e.normal));
        }
        EntityType::Arc(e) => {
            field("center", pt(e.center));
            field("radius", num(e.radius));
            field("start_angle", num(e.start_angle));
            field("end_angle", num(e.end_angle));
            field("normal", pt(e.normal));
        }
        EntityType::Ellipse(e) => {
            field("center", pt(e.center));
            field("major_axis", pt(e.major_axis));
            field("minor_axis_ratio", num(e.minor_axis_ratio));
            field("start_parameter", num(e.start_parameter));
            field("end_parameter", num(e.end_parameter));
            field("normal", pt(e.normal));
        }
        EntityType::LwPolyline(e) => {
            for (i, v) in e.vertices.iter().enumerate() {
                field(
                    &format!("vertex[{i}]"),
                    format!(
                        "({}, {}) bulge {} width {} {}",
                        num(v.location.x),
                        num(v.location.y),
                        num(v.bulge),
                        num(v.start_width),
                        num(v.end_width)
                    ),
                );
            }
            field("closed", e.is_closed.to_string());
            field("elevation", num(e.elevation));
            field("constant_width", num(e.constant_width));
            field("normal", pt(e.normal));
        }
        EntityType::Text(e) => {
            field("insertion_point", pt(e.insertion_point));
            field(
                "alignment_point",
                e.alignment_point.map_or("none".to_string(), pt),
            );
            field("height", num(e.height));
            field("rotation", num(e.rotation));
            field("width_factor", num(e.width_factor));
            field("oblique_angle", num(e.oblique_angle));
            field("normal", pt(e.normal));
        }
        EntityType::MText(e) => {
            field("insertion_point", pt(e.insertion_point));
            field("height", num(e.height));
            field("rectangle_width", num(e.rectangle_width));
            field("rotation", num(e.rotation));
            field(
                "dwg_x_direction",
                e.dwg_x_direction.map_or("none".to_string(), pt),
            );
            field("normal", pt(e.normal));
        }
        EntityType::Insert(e) => {
            field("insert_point", pt(e.insert_point));
            field(
                "scale",
                format!(
                    "{} {} {}",
                    num(e.x_scale()),
                    num(e.y_scale()),
                    num(e.z_scale())
                ),
            );
            field("rotation", num(e.rotation));
            field("normal", pt(e.normal));
        }
        EntityType::Ray(e) => {
            field("base_point", pt(e.base_point));
            field("direction", pt(e.direction));
        }
        EntityType::XLine(e) => {
            field("base_point", pt(e.base_point));
            field("direction", pt(e.direction));
        }
        EntityType::Solid(e) => {
            field(
                "corners",
                pts(&[
                    e.first_corner,
                    e.second_corner,
                    e.third_corner,
                    e.fourth_corner,
                ]),
            );
            field("normal", pt(e.normal));
        }
        EntityType::Spline(e) => {
            field("control_points", pts(&e.control_points));
            field("fit_points", pts(&e.fit_points));
            field("normal", pt(e.normal));
        }
        EntityType::Polyline2D(e) => {
            for (i, v) in e.vertices.iter().enumerate() {
                field(
                    &format!("vertex[{i}]"),
                    format!(
                        "{} bulge {} width {} {}",
                        pt(v.location),
                        num(v.bulge),
                        num(v.start_width),
                        num(v.end_width)
                    ),
                );
            }
            field("elevation", num(e.elevation));
            field("normal", pt(e.normal));
        }
        EntityType::Hatch(e) => {
            field("elevation", num(e.elevation));
            field("normal", pt(e.normal));
            field("pattern_angle", num(e.pattern_angle));
            field("pattern_scale", num(e.pattern_scale));
            for (i, l) in e.pattern.lines.iter().enumerate() {
                field(
                    &format!("pattern_line[{i}]"),
                    format!(
                        "angle {} base ({}, {}) offset ({}, {}) dashes {}",
                        num(l.angle),
                        num(l.base_point.x),
                        num(l.base_point.y),
                        num(l.offset.x),
                        num(l.offset.y),
                        l.dash_lengths
                            .iter()
                            .map(|d| num(*d))
                            .collect::<Vec<_>>()
                            .join(" ")
                    ),
                );
            }
            for (i, path) in e.paths.iter().enumerate() {
                for (j, edge) in path.edges.iter().enumerate() {
                    field(&format!("path[{i}].edge[{j}]"), describe_edge(edge));
                }
            }
            for (i, p) in e.seed_points.iter().enumerate() {
                field(
                    &format!("seed[{i}]"),
                    format!("({}, {})", num(p.x), num(p.y)),
                );
            }
        }
        EntityType::Dimension(d) => {
            describe_dimension_base(d.base(), &mut field);
            match d.as_ref() {
                Dimension::Linear(e) => {
                    field("first_point", pt(e.first_point));
                    field("second_point", pt(e.second_point));
                    field("definition_point", pt(e.definition_point));
                    field("rotation", num(e.rotation));
                    field("ext_line_rotation", num(e.ext_line_rotation));
                }
                Dimension::Aligned(e) => {
                    field("first_point", pt(e.first_point));
                    field("second_point", pt(e.second_point));
                    field("definition_point", pt(e.definition_point));
                    field("ext_line_rotation", num(e.ext_line_rotation));
                }
                Dimension::Radius(e) => {
                    field("definition_point", pt(e.definition_point));
                    field("angle_vertex", pt(e.angle_vertex));
                    field("leader_length", num(e.leader_length));
                }
                Dimension::Angular3Pt(e) => {
                    field("definition_point", pt(e.definition_point));
                    field("first_point", pt(e.first_point));
                    field("second_point", pt(e.second_point));
                    field("angle_vertex", pt(e.angle_vertex));
                }
                Dimension::Diameter(e) => {
                    field("definition_point", pt(e.definition_point));
                    field("angle_vertex", pt(e.angle_vertex));
                    field("leader_length", num(e.leader_length));
                }
                Dimension::Ordinate(e) => {
                    field("definition_point", pt(e.definition_point));
                    field("feature_location", pt(e.feature_location));
                    field("leader_endpoint", pt(e.leader_endpoint));
                    field("is_ordinate_type_x", e.is_ordinate_type_x.to_string());
                }
                Dimension::Arc(e) => {
                    field("definition_point", pt(e.definition_point));
                    field("first_extension_point", pt(e.first_extension_point));
                    field("second_extension_point", pt(e.second_extension_point));
                    field("center_point", pt(e.center_point));
                    field(
                        "arc_parameters",
                        format!(
                            "{} {}",
                            num(e.arc_start_parameter),
                            num(e.arc_end_parameter)
                        ),
                    );
                }
                Dimension::Angular2Ln(e) => {
                    field("dimension_arc", pt(e.dimension_arc));
                    field("first_point", pt(e.first_point));
                    field("second_point", pt(e.second_point));
                    field("angle_vertex", pt(e.angle_vertex));
                    field("definition_point", pt(e.definition_point));
                }
                Dimension::LargeRadial(e) => {
                    field("definition_point", pt(e.definition_point));
                    field("chord_point", pt(e.chord_point));
                    field("override_center", pt(e.override_center));
                    field("jog_point", pt(e.jog_point));
                    field("jog_angle", num(e.jog_angle));
                }
            }
        }
        EntityType::Polyline3D(e) => {
            let points: Vec<Vector3> = e.vertices.iter().map(|v| v.position).collect();
            field("vertices", pts(&points));
            field("elevation", num(e.elevation));
            field("normal", pt(e.normal));
        }
        EntityType::PolyfaceMesh(e) => {
            let points: Vec<Vector3> = e.vertices.iter().map(|v| v.location).collect();
            field("vertices", pts(&points));
            field("elevation", num(e.elevation));
            field("normal", pt(e.normal));
        }
        EntityType::PolygonMesh(e) => {
            let points: Vec<Vector3> = e.vertices.iter().map(|v| v.location).collect();
            field("vertices", pts(&points));
            field("elevation", num(e.elevation));
            field("normal", pt(e.normal));
        }
        EntityType::Mesh(e) => {
            field("vertices", pts(&e.vertices));
        }
        EntityType::Face3D(e) => {
            field(
                "corners",
                pts(&[
                    e.first_corner,
                    e.second_corner,
                    e.third_corner,
                    e.fourth_corner,
                ]),
            );
        }
        EntityType::Helix(e) => {
            field("axis_base_point", pt(e.axis_base_point));
            field("start_point", pt(e.start_point));
            field("axis_vector", pt(e.axis_vector));
            field("radius", num(e.radius));
            field("turns", num(e.turns));
            field("turn_height", num(e.turn_height));
            field("handedness", e.handedness.to_string());
            field("spline.control_points", pts(&e.spline.control_points));
        }
        EntityType::Leader(e) => {
            field("vertices", pts(&e.vertices));
            field("normal", pt(e.normal));
            field("horizontal_direction", pt(e.horizontal_direction));
            field("block_offset", pt(e.block_offset));
            field("annotation_offset", pt(e.annotation_offset));
            field("text_height", num(e.text_height));
            field("text_width", num(e.text_width));
        }
        EntityType::Tolerance(e) => {
            field("insertion_point", pt(e.insertion_point));
            field("direction", pt(e.direction));
            field("normal", pt(e.normal));
            field("text_height", num(e.text_height));
        }
        EntityType::Shape(e) => {
            field("insertion_point", pt(e.insertion_point));
            field("size", num(e.size));
            field("rotation", num(e.rotation));
            field("relative_x_scale", num(e.relative_x_scale));
            field("oblique_angle", num(e.oblique_angle));
            field("normal", pt(e.normal));
        }
        EntityType::AttributeDefinition(e) => {
            field("insertion_point", pt(e.insertion_point));
            field("alignment_point", pt(e.alignment_point));
            field("height", num(e.height));
            field("rotation", num(e.rotation));
            field("width_factor", num(e.width_factor));
            field("oblique_angle", num(e.oblique_angle));
            field("normal", pt(e.normal));
        }
        EntityType::Light(e) => {
            field("position", pt(e.position));
            field("target", pt(e.target));
            field("hotspot_angle", num(e.hotspot_angle));
            field("falloff_angle", num(e.falloff_angle));
        }
        other => panic!("describe() has no case for {other:?}"),
    }
    s
}

fn describe_dimension_base(b: &DimensionBase, field: &mut impl FnMut(&str, String)) {
    field("base.definition_point", pt(b.definition_point));
    field("base.text_middle_point", pt(b.text_middle_point));
    field("base.insertion_point", pt(b.insertion_point));
    field("base.normal", pt(b.normal));
    field("base.text_rotation", num(b.text_rotation));
    field("base.horizontal_direction", num(b.horizontal_direction));
    field("base.insertion_scale", pt(b.insertion_scale));
    field("base.insertion_rotation", num(b.insertion_rotation));
}

fn describe_edge(edge: &BoundaryEdge) -> String {
    let p2 = |v: Vector2| format!("({}, {})", num(v.x), num(v.y));
    match edge {
        BoundaryEdge::Line(e) => format!("line {} {}", p2(e.start), p2(e.end)),
        BoundaryEdge::CircularArc(e) => format!(
            "arc center {} radius {} angles {} {} ccw {}",
            p2(e.center),
            num(e.radius),
            num(e.start_angle),
            num(e.end_angle),
            e.counter_clockwise
        ),
        // Polyline edge vertices carry the bulge in z.
        BoundaryEdge::Polyline(e) => {
            format!("polyline closed {} {}", e.is_closed, pts(&e.vertices))
        }
        other => panic!("describe_edge() has no case for {other:?}"),
    }
}

fn transformed(entity: &EntityType, transform: &EntityTransform) -> EntityType {
    let mut copy = entity.clone();
    apply_transform(&mut copy, transform);
    copy
}

fn snapshot() -> String {
    let mut out = String::new();
    for (entity_name, entity) in entities() {
        let _ = writeln!(out, "== {entity_name} / original");
        out.push_str(&describe(&entity));
        for (transform_name, transform) in transforms() {
            let _ = writeln!(out, "== {entity_name} / {transform_name}");
            out.push_str(&describe(&transformed(&entity, &transform)));
        }
        let _ = writeln!(out, "== {entity_name} / place_entity_tilted");
        out.push_str(&describe(&tilted_plane().place_entity(entity.clone())));
    }
    out
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[test]
fn snapshot_matches_golden() {
    let actual = snapshot();
    if std::env::var_os("OCS_UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(std::path::Path::new(GOLDEN).parent().unwrap()).unwrap();
        std::fs::write(GOLDEN, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(GOLDEN)
        .unwrap_or_else(|e| panic!("can't read {GOLDEN}: {e}. Run with OCS_UPDATE_GOLDEN=1."));
    if let Some(first_diff) = first_difference(&expected, &actual) {
        panic!(
            "entity transform output no longer matches {GOLDEN}\n{first_diff}\n\
             If the change is intended, rerun with OCS_UPDATE_GOLDEN=1 and review the diff."
        );
    }
}

/// The golden text with its numbers pulled out, so numbers can be compared
/// with a tolerance and everything else exactly.
fn split_numbers(line: &str) -> (String, Vec<f64>) {
    let mut text = String::new();
    let mut numbers = Vec::new();
    let mut chars = line.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let starts_number = c.is_ascii_digit()
            || (c == '-' && chars.peek().is_some_and(|(_, n)| n.is_ascii_digit()));
        if !starts_number {
            text.push(c);
            continue;
        }
        let mut end = i + c.len_utf8();
        while let Some(&(j, n)) = chars.peek() {
            if n.is_ascii_digit() || n == '.' {
                end = j + n.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
        numbers.push(line[i..end].parse().expect("number in golden text"));
        text.push('#');
    }
    (text, numbers)
}

/// The first line where `actual` differs from `expected`, ignoring number
/// differences small enough to be platform rounding. The golden prints nine
/// decimals, so a last-bit difference can move a printed value by 1e-9.
fn first_difference(expected: &str, actual: &str) -> Option<String> {
    const TOLERANCE: f64 = 2e-9;
    let (expected_lines, actual_lines): (Vec<_>, Vec<_>) =
        (expected.lines().collect(), actual.lines().collect());
    for (i, (e, a)) in expected_lines.iter().zip(&actual_lines).enumerate() {
        let (e_text, e_nums) = split_numbers(e);
        let (a_text, a_nums) = split_numbers(a);
        let same = e_text == a_text
            && e_nums.len() == a_nums.len()
            && e_nums
                .iter()
                .zip(&a_nums)
                .all(|(x, y)| (x - y).abs() <= TOLERANCE + 1e-12 * x.abs());
        if !same {
            return Some(format!("line {}:\n  expected: {e}\n  actual:   {a}", i + 1));
        }
    }
    (expected_lines.len() != actual_lines.len()).then(|| {
        format!(
            "expected {} lines, got {}",
            expected_lines.len(),
            actual_lines.len()
        )
    })
}

#[test]
fn golden_comparison_tolerates_rounding_but_not_real_changes() {
    let golden = "== line / scale\n  start: (4.500000000, -0.750000000, 0.000000000)\n";
    let rounding = "== line / scale\n  start: (4.500000001, -0.749999999, 0.000000000)\n";
    let real = "== line / scale\n  start: (4.500002500, -0.750000000, 0.000000000)\n";
    let renamed = "== line / scaled\n  start: (4.500000000, -0.750000000, 0.000000000)\n";
    assert_eq!(first_difference(golden, golden), None);
    assert_eq!(first_difference(golden, rounding), None);
    assert!(first_difference(golden, real).is_some());
    assert!(first_difference(golden, renamed).is_some());
    assert!(first_difference(golden, "== line / scale\n").is_some());
}

/// `place_entity` is defined as applying the plane's world transform, so
/// the two must agree for every entity.
#[test]
fn place_entity_matches_affine_world_transform() {
    let plane = tilted_plane();
    let affine = EntityTransform::Affine(plane.to_world_transform());
    for (name, entity) in entities() {
        assert_eq!(
            describe(&plane.place_entity(entity.clone())),
            describe(&transformed(&entity, &affine)),
            "{name}"
        );
    }
}

/// Where a point should land, worked out without the code under test.
fn expected_point(transform: &EntityTransform, p: DVec3) -> DVec3 {
    match transform {
        EntityTransform::Translate(delta) => p + *delta,
        EntityTransform::Rotate {
            center,
            axis,
            angle_rad,
        } => *center + DQuat::from_axis_angle(axis.normalize(), *angle_rad) * (p - *center),
        EntityTransform::Scale { center, factor } => *center + (p - *center) * *factor,
        EntityTransform::Mirror {
            p1,
            p2,
            working_normal,
        } => {
            let n = (*p2 - *p1).cross(*working_normal).normalize();
            p - 2.0 * (p - *p1).dot(n) * n
        }
        EntityTransform::Affine(_) => tilted_plane().to_world(p),
    }
}

fn dv(v: Vector3) -> DVec3 {
    DVec3::new(v.x, v.y, v.z)
}

fn assert_close(actual: Vector3, expected: DVec3, what: &str) {
    assert!(
        dv(actual).abs_diff_eq(expected, 1e-9),
        "{what}: got {actual:?}, expected {expected:?}"
    );
}

/// Line endpoints and point locations are stored in world coordinates, so
/// each transform must move them exactly as plain point math says.
#[test]
fn line_and_point_follow_point_math() {
    let start = DVec3::new(1.0, 2.0, 0.0);
    let end = DVec3::new(7.0, 4.0, 1.5);
    let location = DVec3::new(3.0, -1.0, 2.0);
    for (name, transform) in transforms() {
        let EntityType::Line(line) = transformed(
            &EntityType::Line(Line::from_points(
                v3(start.x, start.y, start.z),
                v3(end.x, end.y, end.z),
            )),
            &transform,
        ) else {
            unreachable!()
        };
        assert_close(
            line.start,
            expected_point(&transform, start),
            &format!("{name} start"),
        );
        assert_close(
            line.end,
            expected_point(&transform, end),
            &format!("{name} end"),
        );

        let EntityType::Point(point) = transformed(
            &EntityType::Point(Point::at(v3(location.x, location.y, location.z))),
            &transform,
        ) else {
            unreachable!()
        };
        assert_close(
            point.location,
            expected_point(&transform, location),
            &format!("{name} point"),
        );
    }
}

/// The points an entity stores in world coordinates. Empty for entities that
/// store their geometry in their own plane (circle, arc, text, insert, ...):
/// those are pinned by the golden file.
fn world_points(entity: &EntityType) -> Vec<Vector3> {
    match entity {
        EntityType::Line(e) => vec![e.start, e.end],
        EntityType::Ray(e) => vec![e.base_point],
        EntityType::XLine(e) => vec![e.base_point],
        EntityType::Ellipse(e) => vec![e.center],
        EntityType::MText(e) => vec![e.insertion_point],
        EntityType::Tolerance(e) => vec![e.insertion_point],
        EntityType::Light(e) => vec![e.position, e.target],
        EntityType::Spline(e) => [e.control_points.clone(), e.fit_points.clone()].concat(),
        EntityType::Polyline3D(e) => e.vertices.iter().map(|v| v.position).collect(),
        EntityType::PolyfaceMesh(e) => e.vertices.iter().map(|v| v.location).collect(),
        EntityType::PolygonMesh(e) => e.vertices.iter().map(|v| v.location).collect(),
        EntityType::Mesh(e) => e.vertices.clone(),
        EntityType::Face3D(e) => vec![
            e.first_corner,
            e.second_corner,
            e.third_corner,
            e.fourth_corner,
        ],
        EntityType::Helix(e) => {
            let mut points = vec![e.axis_base_point, e.start_point];
            points.extend(&e.spline.control_points);
            points
        }
        EntityType::Leader(e) => e.vertices.clone(),
        EntityType::Dimension(d) => {
            let mut points = vec![d.base().text_middle_point];
            match d.as_ref() {
                Dimension::Linear(e) => {
                    points.extend([e.first_point, e.second_point, e.definition_point])
                }
                Dimension::Aligned(e) => {
                    points.extend([e.first_point, e.second_point, e.definition_point])
                }
                Dimension::Radius(e) => points.extend([e.definition_point, e.angle_vertex]),
                Dimension::Diameter(e) => points.extend([e.definition_point, e.angle_vertex]),
                Dimension::Angular3Pt(e) => points.extend([
                    e.definition_point,
                    e.first_point,
                    e.second_point,
                    e.angle_vertex,
                ]),
                Dimension::Angular2Ln(e) => points.extend([
                    e.dimension_arc,
                    e.first_point,
                    e.second_point,
                    e.angle_vertex,
                    e.definition_point,
                ]),
                Dimension::Ordinate(e) => {
                    points.extend([e.definition_point, e.feature_location, e.leader_endpoint])
                }
                Dimension::Arc(e) => points.extend([
                    e.definition_point,
                    e.first_extension_point,
                    e.second_extension_point,
                    e.center_point,
                ]),
                Dimension::LargeRadial(e) => points.extend([
                    e.definition_point,
                    e.chord_point,
                    e.override_center,
                    e.jog_point,
                ]),
            }
            points
        }
        _ => Vec::new(),
    }
}

/// Every point an entity stores in world coordinates must land where plain
/// point math puts it, for every transform. This holds whatever the golden
/// file says, so regenerating the file can't record a wrong position.
#[test]
fn world_stored_points_follow_point_math() {
    let mut checked = 0;
    for (entity_name, entity) in entities() {
        let before = world_points(&entity);
        for (transform_name, transform) in transforms() {
            let after = world_points(&transformed(&entity, &transform));
            assert_eq!(
                before.len(),
                after.len(),
                "{entity_name} / {transform_name}"
            );
            for (i, (b, a)) in before.iter().zip(&after).enumerate() {
                assert_close(
                    *a,
                    expected_point(&transform, dv(*b)),
                    &format!("{entity_name} / {transform_name} / point {i}"),
                );
                checked += 1;
            }
        }
    }
    // Guards against the list above silently matching nothing.
    assert!(checked > 500, "only {checked} points were checked");
}

/// For transforms that keep the drawing plane, a circle's centre moves like a
/// point and its radius scales with the transform.
#[test]
fn circle_center_and_radius_follow_point_math() {
    let center = DVec3::new(4.0, 3.0, 0.0);
    for (name, transform) in transforms() {
        let factor = match &transform {
            EntityTransform::Scale { factor, .. } => *factor,
            EntityTransform::Translate(_) => 1.0,
            EntityTransform::Mirror { working_normal, .. } if *working_normal == DVec3::Z => 1.0,
            EntityTransform::Rotate { axis, .. } if *axis == DVec3::Z => 1.0,
            // Out-of-plane cases are pinned by the golden file instead.
            _ => continue,
        };
        let EntityType::Circle(circle) = transformed(
            &EntityType::Circle(Circle::from_center_radius(
                v3(center.x, center.y, center.z),
                2.5,
            )),
            &transform,
        ) else {
            unreachable!()
        };
        assert_close(
            circle.center,
            expected_point(&transform, center),
            &format!("{name} center"),
        );
        assert!(
            (circle.radius - 2.5 * factor).abs() < 1e-9,
            "{name} radius: got {}",
            circle.radius
        );
    }
}

/// Every transform keeps an arc's end points on the transformed arc. A mirror
/// swaps which end is which, so the ends are compared as a set.
#[test]
fn arc_end_points_follow_point_math_in_plane() {
    let (center, radius, start, end) = (DVec3::new(-2.0, 1.0, 0.0), 3.0, 0.3_f64, 2.2_f64);
    let at = |c: DVec3, r: f64, a: f64| c + DVec3::new(r * a.cos(), r * a.sin(), 0.0);
    for (name, transform) in transforms() {
        let in_plane = match &transform {
            EntityTransform::Translate(_) | EntityTransform::Scale { .. } => true,
            EntityTransform::Mirror { working_normal, .. } => *working_normal == DVec3::Z,
            EntityTransform::Rotate { axis, .. } => *axis == DVec3::Z,
            EntityTransform::Affine(_) => false,
        };
        if !in_plane {
            continue;
        }
        let EntityType::Arc(arc) = transformed(
            &EntityType::Arc(Arc::from_center_radius_angles(
                v3(center.x, center.y, center.z),
                radius,
                start,
                end,
            )),
            &transform,
        ) else {
            unreachable!()
        };
        let got_center = dv(arc.center);
        let mut got = [
            at(got_center, arc.radius, arc.start_angle),
            at(got_center, arc.radius, arc.end_angle),
        ];
        let mut want = [
            expected_point(&transform, at(center, radius, start)),
            expected_point(&transform, at(center, radius, end)),
        ];
        let key = |v: &DVec3| (v.x * 1e6).round() as i64;
        got.sort_by_key(key);
        want.sort_by_key(key);
        for (g, w) in got.iter().zip(want.iter()) {
            assert!(g.abs_diff_eq(*w, 1e-9), "{name}: end {g:?}, expected {w:?}");
        }
    }
}

/// The old and new paths must name the same types: plugins, benches and
/// tests outside `src/` still use `command::`. A later PR that drops the
/// re-export from `command` updates this test on purpose.
#[test]
fn command_reexports_the_transform_types() {
    let from_command: OpenCADStudio::command::EntityTransform =
        EntityTransform::Translate(DVec3::ONE);
    let _: EntityTransform = from_command;
    let plane: OpenCADStudio::command::WorkingPlane = WorkingPlane::default();
    let _: WorkingPlane = plane;
}
