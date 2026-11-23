use super::*;

#[test]
fn audit_python_point_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: "doc.create_entity('Point', location=P(1.0, 2.0, 3.0))",
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.location = (4.0, 5.0, 6.0)\n",
            "    e.thickness = 2.0\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            ("'location':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
            ("'thickness':float('inf')", "finite"),
            ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Point(_)),
        digest: |entity| match entity {
            EntityType::Point(v) => format!(
                "{:.1},{:.1},{:.1} th{:.1}",
                v.location.x, v.location.y, v.location.z, v.thickness
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Point(v) = entity {
                v.location.z = 9.0;
            }
        },
        expect_created: "1.0,2.0,3.0 th0.0",
        expect_edited: "4.0,5.0,6.0 th2.0",
        expect_reedited: "4.0,5.0,9.0 th2.0",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn audit_python_line_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: "doc.create_entity('Line', start=P(0.0, 0.0, 0.0), end=P(10.0, 0.0, 0.0))",
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.end = (10.0, 5.0, 0.0)\n",
            "    e.thickness = 1.5\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            ("'start':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
            ("'thickness':float('inf')", "finite"),
            ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Line(_)),
        digest: |entity| match entity {
            EntityType::Line(v) => {
                format!("{:.1},{:.1} th{:.1}", v.end.x, v.end.y, v.thickness)
            }
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Line(v) = entity {
                v.start.x = 1.0;
            }
        },
        expect_created: "10.0,0.0 th0.0",
        expect_edited: "10.0,5.0 th1.5",
        expect_reedited: "10.0,5.0 th1.5",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn audit_python_circle_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: "doc.create_entity('Circle', center=P(0.0, 0.0, 0.0), radius=5.0)",
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.radius = 7.0\n",
            "    e.center = (1.0, 1.0, 0.0)\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            ("'radius':0.0", "radius"),
            ("'radius':-3.0", "radius"),
            ("'center':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Circle(_)),
        digest: |entity| match entity {
            EntityType::Circle(v) => {
                format!("{:.1},{:.1} r{:.1}", v.center.x, v.center.y, v.radius)
            }
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Circle(v) = entity {
                v.radius = 8.0;
            }
        },
        expect_created: "0.0,0.0 r5.0",
        expect_edited: "1.0,1.0 r7.0",
        expect_reedited: "1.0,1.0 r8.0",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn audit_python_arc_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: "doc.create_entity('Arc', center=P(0.0, 0.0, 0.0), radius=5.0, start_angle=0.0, end_angle=1.5)",
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.end_angle = 2.0\n",
            "    e.radius = 6.0\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            ("'radius':0.0", "radius"),
            ("'end_angle':float('nan')", "finite"),
            ("'center':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Arc(_)),
        digest: |entity| match entity {
            EntityType::Arc(v) => {
                format!("r{:.1} {:.1}..{:.1}", v.radius, v.start_angle, v.end_angle)
            }
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Arc(v) = entity {
                v.start_angle = 0.5;
            }
        },
        expect_created: "r5.0 0.0..1.5",
        expect_edited: "r6.0 0.0..2.0",
        expect_reedited: "r6.0 0.5..2.0",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn audit_python_ellipse_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: "doc.create_entity('Ellipse', center=P(0.0, 0.0, 0.0), major_axis=P(10.0, 0.0, 0.0), minor_axis_ratio=0.5)",
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.minor_axis_ratio = 0.25\n",
            "    e.end_parameter = 3.0\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            ("'minor_axis_ratio':0.0", "ratio"),
            ("'minor_axis_ratio':2.0", "ratio"),
            ("'major_axis':{'x':0.0,'y':0.0,'z':0.0}", "axis"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Ellipse(_)),
        digest: |entity| match entity {
            EntityType::Ellipse(v) => {
                format!("q{:.2} ..{:.1}", v.minor_axis_ratio, v.end_parameter)
            }
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Ellipse(v) = entity {
                v.minor_axis_ratio = 0.75;
            }
        },
        expect_created: "q0.50 ..6.3",
        expect_edited: "q0.25 ..3.0",
        expect_reedited: "q0.75 ..3.0",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn audit_python_ray_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: "doc.create_entity('Ray', base_point=P(0.0, 0.0, 0.0), direction=P(1.0, 0.0, 0.0))",
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.base_point = (2.0, 2.0, 0.0)\n",
            "    e.direction = (0.0, 1.0, 0.0)\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            ("'direction':{'x':2.0,'y':0.0,'z':0.0}", "unit"),
            ("'base_point':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Ray(_)),
        digest: |entity| match entity {
            EntityType::Ray(v) => format!(
                "{:.1},{:.1} d{:.1},{:.1}",
                v.base_point.x, v.base_point.y, v.direction.x, v.direction.y
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Ray(v) = entity {
                v.base_point.x = 3.0;
            }
        },
        expect_created: "0.0,0.0 d1.0,0.0",
        expect_edited: "2.0,2.0 d0.0,1.0",
        expect_reedited: "3.0,2.0 d0.0,1.0",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn audit_python_xline_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: "doc.create_entity('XLine', base_point=P(0.0, 0.0, 0.0), direction=P(1.0, 0.0, 0.0))",
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.base_point = (2.0, 2.0, 0.0)\n",
            "    e.direction = (0.0, 1.0, 0.0)\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            ("'direction':{'x':2.0,'y':0.0,'z':0.0}", "unit"),
            ("'base_point':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
        ],
        is_kind: |entity| matches!(entity, EntityType::XLine(_)),
        digest: |entity| match entity {
            EntityType::XLine(v) => format!(
                "{:.1},{:.1} d{:.1},{:.1}",
                v.base_point.x, v.base_point.y, v.direction.x, v.direction.y
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::XLine(v) = entity {
                v.base_point.x = 3.0;
            }
        },
        expect_created: "0.0,0.0 d1.0,0.0",
        expect_edited: "2.0,2.0 d0.0,1.0",
        expect_reedited: "3.0,2.0 d0.0,1.0",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn audit_python_solid_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: "doc.create_entity('Solid', first_corner=P(0.0, 0.0, 0.0), second_corner=P(4.0, 0.0, 0.0), third_corner=P(0.0, 4.0, 0.0), fourth_corner=P(4.0, 4.0, 0.0))",
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.fourth_corner = (5.0, 5.0, 0.0)\n",
            "    e.thickness = 2.0\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            (
                "'first_corner':{'x':float('nan'),'y':0.0,'z':0.0}",
                "finite",
            ),
            ("'thickness':float('inf')", "finite"),
            ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Solid(_)),
        digest: |entity| match entity {
            EntityType::Solid(v) => format!(
                "{:.1},{:.1} th{:.1}",
                v.fourth_corner.x, v.fourth_corner.y, v.thickness
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Solid(v) = entity {
                v.first_corner.x = 1.0;
            }
        },
        expect_created: "4.0,4.0 th0.0",
        expect_edited: "5.0,5.0 th2.0",
        expect_reedited: "5.0,5.0 th2.0",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn audit_python_face3d_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: "doc.create_entity('Face3D', first_corner=P(0.0, 0.0, 0.0), second_corner=P(4.0, 0.0, 0.0), third_corner=P(4.0, 4.0, 1.0), fourth_corner=P(0.0, 4.0, 1.0))",
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.third_corner = (4.0, 4.0, 2.0)\n",
            "    e.invisible_edges = 3\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            (
                "'first_corner':{'x':float('nan'),'y':0.0,'z':0.0}",
                "finite",
            ),
            ("'invisible_edges':64", "bits"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Face3D(_)),
        digest: |entity| match entity {
            EntityType::Face3D(v) => {
                format!("z{:.1} inv{}", v.third_corner.z, v.invisible_edges.bits())
            }
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Face3D(v) = entity {
                v.first_corner.x = 1.0;
            }
        },
        expect_created: "z1.0 inv0",
        expect_edited: "z2.0 inv3",
        expect_reedited: "z2.0 inv3",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn audit_python_text_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: "doc.create_entity('Text', text='Hello', insertion=P(1.0, 2.0, 0.0), height=2.5)",
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.text = 'World'\n",
            "    e.rotation = 0.5\n",
            "    e.height = 3.0\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            ("'height':0.0", "height"),
            ("'height':-1.0", "height"),
            ("'insertion':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Text(_)),
        digest: |entity| match entity {
            EntityType::Text(v) => format!("{} h{:.1} r{:.1}", v.value, v.height, v.rotation),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Text(v) = entity {
                v.height = 4.0;
            }
        },
        expect_created: "Hello h2.5 r0.0",
        expect_edited: "World h3.0 r0.5",
        expect_reedited: "World h4.0 r0.5",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn audit_python_mtext_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: "doc.create_entity('MText', text='Line one', insertion=P(1.0, 2.0, 0.0), height=2.5, rectangle_width=30.0)",
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.text = 'Line two'\n",
            "    e.rectangle_width = 40.0\n",
            "    e.rotation = 0.5\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            ("'height':0.0", "height"),
            ("'rectangle_width':-1.0", "width"),
            ("'insertion':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
        ],
        is_kind: |entity| matches!(entity, EntityType::MText(_)),
        digest: |entity| match entity {
            EntityType::MText(v) => {
                format!("{} w{:.1} r{:.1}", v.value, v.rectangle_width, v.rotation)
            }
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::MText(v) = entity {
                v.rectangle_width = 50.0;
            }
        },
        expect_created: "Line one w30.0 r0.0",
        expect_edited: "Line two w40.0 r0.5",
        expect_reedited: "Line two w50.0 r0.5",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn audit_python_lwpolyline_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: concat!(
            "doc.create_entity('LwPolyline', vertices=[{'location': {'x': 0.0, 'y': 0.0}}, {'location': {'x': 10.0, 'y': 0.0}}, {'location': {'x': 10.0, 'y': 5.0}}])\n"
        ),
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.vertices = [{'location': {'x': 0.0, 'y': 0.0}}, {'location': {'x': 10.0, 'y': 0.0}}, {'location': {'x': 10.0, 'y': 8.0}, 'bulge': 0.5}, {'location': {'x': 0.0, 'y': 8.0}}]\n",
            "    e.is_closed = True\n",
            "    e.constant_width = 0.5\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            ("'vertices':[{'location':{'x':0.0,'y':0.0}}]", "at least 2"),
            ("'constant_width':-1.0", "non-negative"),
            (
                "'vertices':[{'location':{'x':float('nan'),'y':0.0}},{'location':{'x':1.0,'y':0.0}}]",
                "finite",
            ),
            ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
        ],
        is_kind: |entity| matches!(entity, EntityType::LwPolyline(_)),
        digest: |entity| match entity {
            EntityType::LwPolyline(v) => format!(
                "n{} closed{} w{:.1} bulge{:.1}",
                v.vertices.len(),
                v.is_closed,
                v.constant_width,
                v.vertices.get(2).map_or(0.0, |x| x.bulge)
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::LwPolyline(v) = entity {
                v.constant_width = 1.0;
            }
        },
        expect_created: "n3 closedfalse w0.0 bulge0.0",
        expect_edited: "n4 closedtrue w0.5 bulge0.5",
        expect_reedited: "n4 closedtrue w1.0 bulge0.5",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn audit_python_polyline2d_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: concat!(
            "doc.create_entity('Polyline2D', vertices=[{'location': {'x': 0.0, 'y': 0.0, 'z': 0.0}}, {'location': {'x': 10.0, 'y': 0.0, 'z': 0.0}}, {'location': {'x': 10.0, 'y': 5.0, 'z': 0.0}}])\n"
        ),
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.vertices = [{'location': {'x': 0.0, 'y': 0.0, 'z': 0.0}}, {'location': {'x': 10.0, 'y': 0.0, 'z': 0.0}}, {'location': {'x': 10.0, 'y': 8.0, 'z': 0.0}}, {'location': {'x': 0.0, 'y': 8.0, 'z': 0.0}}]\n",
            "    e.closed = True\n",
            "    e.thickness = 1.0\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            (
                "'vertices':[{'location':{'x':0.0,'y':0.0,'z':0.0}}]",
                "at least 2",
            ),
            ("'thickness':float('inf')", "finite"),
            (
                "'vertices':[{'location':{'x':float('nan'),'y':0.0,'z':0.0}},{'location':{'x':1.0,'y':0.0,'z':0.0}}]",
                "finite",
            ),
            ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Polyline2D(_)),
        digest: |entity| match entity {
            EntityType::Polyline2D(v) => format!(
                "n{} closed{} th{:.1}",
                v.vertices.len(),
                v.flags.is_closed(),
                v.thickness
            ),
            other => format!(
                "wrong kind {}",
                format!("{other:?}").split('(').next().unwrap()
            ),
        },
        reedit: |entity| {
            if let EntityType::Polyline2D(v) = entity {
                v.thickness = 2.0;
            }
        },
        expect_created: "n3 closedfalse th0.0",
        expect_edited: "n4 closedtrue th1.0",
        expect_reedited: "n4 closedtrue th2.0",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "wrong kind LwPolyline",
        expect_reedited_dxf: "wrong kind LwPolyline",
    });
}

#[test]
fn audit_python_polyline_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: "# MAKEPOLYLINE
",
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.vertices = [{'location': {'x': 0.0, 'y': 0.0, 'z': 0.0}}, {'location': {'x': 10.0, 'y': 0.0, 'z': 2.0}}, {'location': {'x': 10.0, 'y': 8.0, 'z': 6.0}}, {'location': {'x': 0.0, 'y': 8.0, 'z': 6.0}}]\n",
            "    e.closed = True\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            (
                "'vertices':[{'location':{'x':0.0,'y':0.0,'z':0.0}}]",
                "at least 2",
            ),
            (
                "'vertices':[{'location':{'x':float('nan'),'y':0.0,'z':0.0}},{'location':{'x':1.0,'y':0.0,'z':0.0}}]",
                "finite",
            ),
        ],
        is_kind: |entity| matches!(entity, EntityType::Polyline(_)),
        digest: |entity| match entity {
            EntityType::Polyline(v) => format!(
                "n{} closed{} lastz{:.1}",
                v.vertices.len(),
                v.flags.is_closed(),
                v.vertices.last().map_or(0.0, |x| x.location.z)
            ),
            other => format!(
                "wrong kind {}",
                format!("{other:?}").split('(').next().unwrap()
            ),
        },
        reedit: |entity| {
            if let EntityType::Polyline(v) = entity {
                v.vertices[0].location.z = 1.0;
            }
        },
        expect_created: "n3 closedfalse lastz4.0",
        expect_edited: "n4 closedtrue lastz6.0",
        expect_reedited: "n4 closedtrue lastz6.0",
        expect_edited_dwg: "wrong kind Polyline3D",
        expect_reedited_dwg: "wrong kind Polyline3D",
        expect_edited_dxf: "wrong kind Polyline3D",
        expect_reedited_dxf: "wrong kind Polyline3D",
    });
}

#[test]
fn audit_python_polyline3d_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: concat!(
            "doc.create_entity('Polyline3D', vertices=[{'position': {'x': 0.0, 'y': 0.0, 'z': 0.0}}, {'position': {'x': 10.0, 'y': 0.0, 'z': 2.0}}, {'position': {'x': 10.0, 'y': 5.0, 'z': 4.0}}])\n"
        ),
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.vertices = [{'position': {'x': 0.0, 'y': 0.0, 'z': 0.0}}, {'position': {'x': 10.0, 'y': 0.0, 'z': 2.0}}, {'position': {'x': 10.0, 'y': 8.0, 'z': 6.0}}, {'position': {'x': 0.0, 'y': 8.0, 'z': 6.0}}]\n",
            "    e.elevation = 1.5\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            (
                "'vertices':[{'position':{'x':0.0,'y':0.0,'z':0.0}}]",
                "at least 2",
            ),
            ("'elevation':float('inf')", "finite"),
            (
                "'vertices':[{'position':{'x':float('nan'),'y':0.0,'z':0.0}},{'position':{'x':1.0,'y':0.0,'z':0.0}}]",
                "finite",
            ),
            ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Polyline3D(_)),
        digest: |entity| match entity {
            EntityType::Polyline3D(v) => format!(
                "n{} el{:.1} lastz{:.1}",
                v.vertices.len(),
                v.elevation,
                v.vertices.last().map_or(0.0, |x| x.position.z)
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Polyline3D(v) = entity {
                v.elevation = 2.5;
            }
        },
        expect_created: "n3 el0.0 lastz4.0",
        expect_edited: "n4 el1.5 lastz6.0",
        expect_reedited: "n4 el2.5 lastz6.0",
        expect_edited_dwg: "n4 el0.0 lastz6.0",
        expect_reedited_dwg: "n4 el0.0 lastz6.0",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn audit_python_spline_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: concat!(
            "doc.create_entity('Spline', degree=2, knots=[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], control_points=[{'x': 0.0, 'y': 0.0, 'z': 0.0}, {'x': 5.0, 'y': 5.0, 'z': 0.0}, {'x': 10.0, 'y': 0.0, 'z': 0.0}])\n"
        ),
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.control_points = [{'x': 0.0, 'y': 0.0, 'z': 0.0}, {'x': 5.0, 'y': 8.0, 'z': 0.0}, {'x': 10.0, 'y': 0.0, 'z': 0.0}]\n",
            "    e.weights = [1.0, 2.0, 1.0]\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            ("'degree':0", "at least 1"),
            ("'knots':[0.0,1.0]", "must hold"),
            ("'weights':[1.0,0.0,1.0]", "greater than zero"),
            ("'weights':[1.0]", "match"),
            (
                "'control_points':[{'x':float('nan'),'y':0.0,'z':0.0},{'x':1.0,'y':0.0,'z':0.0},{'x':2.0,'y':0.0,'z':0.0}]",
                "finite",
            ),
            ("'knots':[0.0,0.0,1.0,0.5,1.0,1.0]", "nondecreasing"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Spline(_)),
        digest: |entity| match entity {
            EntityType::Spline(v) => format!(
                "deg{} cp{} y{:.1} w{:?}",
                v.degree,
                v.control_points.len(),
                v.control_points[1].y,
                v.weights
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Spline(v) = entity {
                v.control_points[1].y = 9.0;
            }
        },
        expect_created: "deg2 cp3 y5.0 w[]",
        expect_edited: "deg2 cp3 y8.0 w[1.0, 2.0, 1.0]",
        expect_reedited: "deg2 cp3 y9.0 w[1.0, 2.0, 1.0]",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
    });
}

#[test]
fn audit_python_insert_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: concat!(
            "# MAKEBLOCK\n",
            "doc.create_entity('Insert', block_name='AUDITBLK', insert_point=P(1.0, 2.0, 0.0))\n"
        ),
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.insert_point = (4.0, 5.0, 0.0)\n",
            "    e.x_scale = 2.0\n",
            "    e.y_scale = 3.0\n",
            "    e.rotation = 0.5\n",
            "    e.column_count = 2\n",
            "    e.column_spacing = 10.0\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            ("'x_scale':0.0", "nonzero"),
            ("'rotation':float('nan')", "finite"),
            ("'column_count':0", "greater than zero"),
            (
                "'insert_point':{'x':float('nan'),'y':0.0,'z':0.0}",
                "finite",
            ),
            ("'block_name':'OTHER'", "does not exist"),
            ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Insert(_)),
        digest: |entity| match entity {
            EntityType::Insert(v) => format!(
                "{} at{:.1},{:.1} s{:.1},{:.1} r{:.1} cols{}x{:.1}",
                v.block_name,
                v.insert_point.x,
                v.insert_point.y,
                v.x_scale(),
                v.y_scale(),
                v.rotation,
                v.column_count,
                v.column_spacing
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Insert(v) = entity {
                v.rotation = 1.0;
            }
        },
        expect_created: "AUDITBLK at1.0,2.0 s1.0,1.0 r0.0 cols1x0.0",
        expect_edited: "AUDITBLK at4.0,5.0 s2.0,3.0 r0.5 cols2x10.0",
        expect_reedited: "AUDITBLK at4.0,5.0 s2.0,3.0 r1.0 cols2x10.0",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
    });
}

#[test]
fn audit_python_attribute_definition_lifecycle_over_real_ipc() {
    run_mesh_lifecycle(&MeshCase {
        create: concat!(
            "# MAKEBLOCK\n",
            "doc.create_entity('AttributeDefinition', owner_handle=BLOCKRECORD, tag='PART_NO', prompt='Part number',\n",
            "    default_value='PN-001', insertion_point=P(1.0, 2.0, 0.0), height=2.5, width_factor=1.25, oblique_angle=0.1,\n",
            "    rotation=0.25, horizontal_alignment='Center', vertical_alignment='Top', alignment_point=P(3.0, 4.0, 0.0),\n",
            "    flags={'invisible': True, 'constant': False, 'verify': True, 'preset': False, 'locked_position': False, 'annotative': False},\n",
            "    field_length=12, text_generation_flags=2, lock_position=True)\n"
        ),
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.insertion_point = (4.0, 5.0, 0.0)\n",
            "    e.default_value = 'PN-002'\n",
            "    e.prompt = 'Serial'\n",
            "    e.height = 3.0\n",
            "    e.width_factor = 0.8\n",
            "    e.rotation = 0.5\n",
            "    e.oblique_angle = 0.2\n",
            "    e.horizontal_alignment = 'Right'\n",
            "    e.vertical_alignment = 'Middle'\n",
            "    e.flags = {'invisible': False, 'constant': True, 'verify': False, 'preset': True, 'locked_position': False, 'annotative': False}\n",
            "    e.field_length = 20\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            ("'tag':'BAD TAG'", "whitespace"),
            ("'height':0.0", "greater than zero"),
            ("'width_factor':0.0", "width_factor"),
            ("'text_style':'Missing'", "does not exist"),
            ("'owner_handle':999999", "read-only"),
            ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
            ("'rotation':float('nan')", "finite"),
            ("'horizontal_alignment':'Nowhere'", "unsupported"),
        ],
        is_kind: |entity| matches!(entity, EntityType::AttributeDefinition(_)),
        digest: |entity| match entity {
            EntityType::AttributeDefinition(v) => format!(
                "{}|{}|{} ins{:.1},{:.1} al{:.1},{:.1} h{:.1} r{:.2} wf{:.2} ob{:.2} {:?}/{:?} f{}{}{}{} fl{} tg{} lock{}",
                v.tag,
                v.prompt,
                v.default_value,
                v.insertion_point.x,
                v.insertion_point.y,
                v.alignment_point.x,
                v.alignment_point.y,
                v.height,
                v.rotation,
                v.width_factor,
                v.oblique_angle,
                v.horizontal_alignment,
                v.vertical_alignment,
                u8::from(v.flags.invisible),
                u8::from(v.flags.constant),
                u8::from(v.flags.verify),
                u8::from(v.flags.preset),
                v.field_length,
                v.text_generation_flags,
                u8::from(v.lock_position)
            ),
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::AttributeDefinition(v) = entity {
                v.default_value = "PN-003".into();
            }
        },
        expect_created: "PART_NO|Part number|PN-001 ins1.0,2.0 al3.0,4.0 h2.5 r0.25 wf1.25 ob0.10 Center/Top f1010 fl12 tg2 lock1",
        expect_edited: "PART_NO|Serial|PN-002 ins4.0,5.0 al3.0,4.0 h3.0 r0.50 wf0.80 ob0.20 Right/Middle f0101 fl20 tg2 lock1",
        expect_reedited: "PART_NO|Serial|PN-003 ins4.0,5.0 al3.0,4.0 h3.0 r0.50 wf0.80 ob0.20 Right/Middle f0101 fl20 tg2 lock1",
        // Reading a DXF derives a justified attribute's insertion point
        // from its alignment point (#2728).
        expect_edited_dxf: "PART_NO|Serial|PN-002 ins-11.0,-5.4 al3.0,4.0 h3.0 r0.50 wf0.80 ob0.20 Right/Middle f0101 fl20 tg2 lock1",
        expect_reedited_dxf: "PART_NO|Serial|PN-003 ins-11.0,-5.4 al3.0,4.0 h3.0 r0.50 wf0.80 ob0.20 Right/Middle f0101 fl20 tg2 lock1",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
    });
}

#[test]
fn audit_python_insert_creation_rules_over_real_ipc() {
    let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
        return;
    };
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let mut records = std::collections::BTreeMap::new();
    for name in ["OUTER", "INNER"] {
        let next = host.document().next_handle();
        let (record_handle, block_handle, end_handle) = (
            Handle::new(next),
            Handle::new(next + 1),
            Handle::new(next + 2),
        );
        let mut record = document::tables::BlockRecord::new(name);
        record.handle = record_handle;
        record.block_entity_handle = block_handle;
        record.block_end_handle = end_handle;
        host.document_mut().block_records.add(record).unwrap();
        let mut block = document::entities::Block::new(name, document::types::Vector3::ZERO);
        block.common.handle = block_handle;
        block.common.owner_handle = record_handle;
        host.document_mut()
            .add_entity(EntityType::Block(block))
            .unwrap();
        let mut end = document::entities::BlockEnd::new();
        end.common.handle = end_handle;
        end.common.owner_handle = record_handle;
        host.document_mut()
            .add_entity(EntityType::BlockEnd(end))
            .unwrap();
        records.insert(name, record_handle);
    }
    let process = ocs_plugin_api::process::PluginProcess::spawn(
        std::path::Path::new(&plugin_path),
        &mut host,
        crate::plugin::v4_support::notification_handler(),
        None,
    )
    .unwrap();
    let dispatch = |host: &mut HostSession<'_>, command: &str| {
        assert!(
            process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch")
        );
    };
    let last = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();
    let count = |host: &HostSession<'_>| {
        host.document()
            .entities()
            .filter(|e| matches!(e, EntityType::Insert(_)))
            .count()
    };
    let create = |name: &str, extra: &str| {
        format!(
            "PY_EVAL ocs.active_document.create_entity('Insert', block_name='{name}', insert_point={{'x':0.0,'y':0.0,'z':0.0}}{extra})"
        )
    };

    // A valid insert of INNER into the drawing works.
    dispatch(&mut host, &create("INNER", ""));
    assert_eq!(count(&host), 1, "{}", last(&host));
    // Refused: unknown block, model space, bad scale, zero array count.
    for (command, message) in [
        (create("MISSING", ""), "does not exist"),
        (create("*Model_Space", ""), "model-space or paper-space"),
        (create("INNER", ", x_scale=0.0"), "nonzero"),
        (create("INNER", ", column_count=0"), "greater than zero"),
        (create("", ""), "empty"),
    ] {
        dispatch(&mut host, &command);
        assert_eq!(count(&host), 1, "{command}");
        assert!(last(&host).contains(message), "{command}: {}", last(&host));
    }
    // A block cannot contain itself, directly or through another block.
    let owner = |name: &str| format!(", owner_handle={}", records[name].value());
    dispatch(&mut host, &create("OUTER", &owner("OUTER")));
    assert_eq!(count(&host), 1);
    assert!(last(&host).contains("contain itself"), "{}", last(&host));
    dispatch(&mut host, &create("INNER", &owner("OUTER")));
    assert_eq!(count(&host), 2, "{}", last(&host));
    dispatch(&mut host, &create("OUTER", &owner("INNER")));
    assert_eq!(count(&host), 2);
    assert!(last(&host).contains("contain itself"), "{}", last(&host));
    // The legacy Polyline cannot be created and says what to use instead.
    dispatch(
        &mut host,
        "PY_EVAL ocs.active_document.create_entity('Polyline', vertices=[])",
    );
    assert!(
        last(&host).contains("Polyline2D or Polyline3D"),
        "{}",
        last(&host)
    );
}

#[test]
fn audit_python_ole2frame_lifecycle_over_real_ipc() {
    image::RgbaImage::from_pixel(4, 3, image::Rgba([10, 120, 200, 255]))
        .save(std::env::temp_dir().join("ocs_ole_audit.png"))
        .unwrap();
    run_mesh_lifecycle(&MeshCase {
        create: "doc.embed_picture('TMPDIR/ocs_ole_audit.png', origin=(10, 10, 0), width=20)\n",
        edit: concat!(
            "e = doc.entities[HANDLE]\n",
            "with doc.transaction('Edit'):\n",
            "    e.upper_left_corner = (12.0, 40.0, 0.0)\n",
            "    e.lower_right_corner = (42.0, 10.0, 0.0)\n",
            "    e.lock_aspect = 1\n",
            "doc.selection = [e]\n"
        ),
        rejects: &[
            (
                "'upper_left_corner':{'x':42.0,'y':40.0,'z':0.0}",
                "nonzero width and height",
            ),
            ("'lock_aspect':2", "0 or 1"),
            (
                "'lower_right_corner':{'x':float('nan'),'y':0.0,'z':0.0}",
                "finite",
            ),
            ("'version':3", "read-only"),
            ("'storage':{}", "outside the editable schema"),
        ],
        is_kind: |entity| matches!(entity, EntityType::Ole2Frame(_)),
        digest: |entity| match entity {
            EntityType::Ole2Frame(v) => {
                let picture = match document::entities::extract_presentation(&v.encoded_payload()) {
                    Some(document::entities::OlePresentation::Raster(bytes)) => bytes.len(),
                    _ => 0,
                };
                format!(
                    "ul{:.1},{:.1} lr{:.1},{:.1} lock{} picture{}",
                    v.upper_left_corner.x,
                    v.upper_left_corner.y,
                    v.lower_right_corner.x,
                    v.lower_right_corner.y,
                    v.lock_aspect,
                    picture
                )
            }
            _ => "wrong kind".into(),
        },
        reedit: |entity| {
            if let EntityType::Ole2Frame(v) = entity {
                v.lower_right_corner.x = 50.0;
            }
        },
        expect_created: "ul10.0,25.0 lr30.0,10.0 lock0 picture119",
        expect_edited: "ul12.0,40.0 lr42.0,10.0 lock1 picture119",
        expect_reedited: "ul12.0,40.0 lr50.0,10.0 lock1 picture119",
        expect_edited_dxf: "",
        expect_reedited_dxf: "",
        expect_edited_dwg: "",
        expect_reedited_dwg: "",
    });
}
