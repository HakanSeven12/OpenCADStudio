//! The product structure, one case at a time: a part, an assembly with a shared sub-assembly,
//! mapped items, colours, names with escapes, and what is refused. Lengths are the file's
//! numbers times the context's unit, written as the same products, so every comparison is
//! exact.
//!
//! Origin: the data cases of SolidCraft's `crates/kernel/src/step_in/tests.rs` at `a5b71ca2`
//! (`hand_written_part_in_inches_with_colour`, `two_instance_assembly`,
//! `assembly_cycles_are_bounded`, `exponential_assemblies_are_bounded`), checking the data the
//! import builds from instead of the bodies it builds.

use std::fmt::Write as _;

use super::{
    BodyKind, Colour, Definition, Frame, Instance, Item, ItemKind, Link, MAX_ASSEMBLY_DEPTH,
    MAX_VISITS, Operator, Product, ProductStructure, Representation, RepresentationMap, Side,
    StyledItem, SurfaceStyle, Transformation, read,
};
use crate::step::geom::Placement;
use crate::step::p21::{self, Exchange};
use crate::step::units::{Context, Units};

fn file(data: &str) -> Exchange {
    let t = format!(
        "ISO-10303-21;\nHEADER;FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));ENDSEC;\nDATA;\n{data}\nENDSEC;\nEND-ISO-10303-21;\n"
    );
    match p21::parse(&t) {
        Ok(ex) if ex.duplicates.is_empty() => ex,
        Ok(ex) => panic!("ids defined twice: {:?}\n{t}", ex.duplicates),
        Err(e) => panic!("{e}\n{t}"),
    }
}

fn structure(data: &str) -> ProductStructure {
    read(&file(data)).unwrap_or_else(|e| panic!("{e}"))
}

fn refused(data: &str) -> String {
    match read(&file(data)) {
        Ok(s) => panic!("read: {s:?}"),
        Err(e) => e,
    }
}

/// The application contexts and a body: #8, #9, the solid #101 'Cube' (its shell #100 empty:
/// the product structure does not read topology).
const BASE: &str = "#8=PRODUCT_DEFINITION_CONTEXT('part definition',#9,'design');\
#9=APPLICATION_CONTEXT('');#100=CLOSED_SHELL('',());#101=MANIFOLD_SOLID_BREP('Cube',#100);\n";

/// A geometric context `#id` in the length unit `unit` (#id+2), radians (#id+3) and an
/// uncertainty of 1e-6 in that unit (#id+1).
fn context(id: u64, unit: &str) -> String {
    format!(
        "#{id}=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{}))GLOBAL_UNIT_ASSIGNED_CONTEXT((#{},#{}))REPRESENTATION_CONTEXT('',''));\n#{}=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-6),#{},'','');\n#{}={unit};\n#{}=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));\n",
        id + 1,
        id + 2,
        id + 3,
        id + 1,
        id + 2,
        id + 2,
        id + 3
    )
}

const MM: &str = "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))";
const CM: &str = "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.CENTI.,.METRE.))";
/// Millimetres per centimetre as `units` forms it (prefix times metre).
const CM_MM: f64 = 1e-2 * 1000.0;

/// A product `name` and its definition `#pd`: `PRODUCT` #pd+1, `PRODUCT_CONTEXT` #pd+2, formation
/// #pd+3, `PRODUCT_DEFINITION_SHAPE` #pd+4.
fn product(pd: u64, name: &str) -> String {
    format!(
        "#{}=PRODUCT('{name}','{name}','',(#{}));\n#{}=PRODUCT_CONTEXT('',#9,'mechanical');\n#{}=PRODUCT_DEFINITION_FORMATION('','',#{});\n#{pd}=PRODUCT_DEFINITION('design','',#{},#8);\n#{}=PRODUCT_DEFINITION_SHAPE('','',#{pd});\n",
        pd + 1,
        pd + 2,
        pd + 2,
        pd + 3,
        pd + 1,
        pd + 3,
        pd + 4
    )
}

fn mm(len: f64) -> Context {
    Context {
        units: Units { len, ang: 1.0 },
        uncertainty: Some(1.0E-6 * len),
    }
}

fn product_of(entity: u64, name: &str) -> Product {
    Product {
        entity,
        id: name.into(),
        name: name.into(),
        description: String::new(),
    }
}

fn placement(origin: [f64; 3], axis: Option<[f64; 3]>, ref_direction: Option<[f64; 3]>) -> Frame {
    Frame::Placement(Placement {
        origin,
        axis,
        ref_direction,
    })
}

/// A part in inches with a colour: SolidCraft's `hand_written_part_in_inches_with_colour`. The
/// definition's shape is an empty representation linked to the one with the solid.
#[test]
fn a_part() {
    let mut t = String::from(BASE);
    t += &context(
        20,
        "(CONVERSION_BASED_UNIT('INCH',#30)LENGTH_UNIT()NAMED_UNIT(*));\n#30=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#31);\n#31=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))",
    );
    t += &product(50, "Widget");
    t += "#60=ADVANCED_BREP_SHAPE_REPRESENTATION('brep',(#101,#65),#20);\n#61=SHAPE_DEFINITION_REPRESENTATION(#54,#62);\n#62=SHAPE_REPRESENTATION('',(),#20);\n#63=SHAPE_REPRESENTATION_RELATIONSHIP('','',#62,#60);\n";
    t += "#65=AXIS2_PLACEMENT_3D('',#66,$,$);\n#66=CARTESIAN_POINT('',(0.,0.,0.));\n";
    t += "#70=STYLED_ITEM('',(#71),#101);\n#71=PRESENTATION_STYLE_ASSIGNMENT((#72));\n#72=SURFACE_STYLE_USAGE(.BOTH.,#73);\n#73=SURFACE_SIDE_STYLE('',(#74));\n#74=SURFACE_STYLE_FILL_AREA(#75);\n#75=FILL_AREA_STYLE('',(#76));\n#76=FILL_AREA_STYLE_COLOUR('',#77);\n#77=COLOUR_RGB('',1.,0.5,0.);\n";
    let inch = Context {
        units: Units {
            len: 25.4 * 1.0,
            ang: 1.0,
        },
        uncertainty: Some(1.0E-6 * (25.4 * 1.0)),
    };
    assert_eq!(
        structure(&t),
        ProductStructure {
            products: vec![product_of(51, "Widget")],
            definitions: vec![Definition {
                entity: 50,
                id: "design".into(),
                description: String::new(),
                formation: 53,
                product: 0,
                shapes: vec![1],
            }],
            representations: vec![
                Representation {
                    entity: 60,
                    kind: "ADVANCED_BREP_SHAPE_REPRESENTATION".into(),
                    name: "brep".into(),
                    items: vec![
                        Item {
                            entity: 101,
                            name: "Cube".into(),
                            kind: ItemKind::Body(BodyKind::ManifoldSolidBrep),
                        },
                        Item {
                            entity: 65,
                            name: String::new(),
                            kind: ItemKind::Other("AXIS2_PLACEMENT_3D".into()),
                        },
                    ],
                    context: Some(20),
                    units: inch,
                },
                Representation {
                    entity: 62,
                    kind: "SHAPE_REPRESENTATION".into(),
                    name: String::new(),
                    items: Vec::new(),
                    context: Some(20),
                    units: inch,
                },
            ],
            maps: Vec::new(),
            links: vec![Link {
                entity: 63,
                rep_1: 1,
                rep_2: 0,
                transformation: None,
            }],
            occurrences: Vec::new(),
            roots: vec![0],
            styles: vec![StyledItem {
                entity: 70,
                name: String::new(),
                item: 101,
                overrides: None,
                surface: vec![SurfaceStyle {
                    side: Side::Both,
                    fill: Some(Colour::Rgb {
                        name: String::new(),
                        rgb: [1.0, 0.5, 0.0],
                    }),
                    rendering: None,
                    transparency: None,
                }],
                curve: Vec::new(),
            }],
        }
    );
}

/// A solid outside any product: its representation is still read, and nothing else.
#[test]
fn a_loose_solid() {
    let mut t = String::from(BASE);
    t += &context(20, MM);
    t += "#60=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#101),#20);\n";
    let s = structure(&t);
    assert_eq!(
        (s.products.len(), s.definitions.len(), s.roots.len()),
        (0, 0, 0)
    );
    assert_eq!(s.representations.len(), 1);
    assert_eq!(s.representations[0].entity, 60);
    assert_eq!(s.representations[0].units, mm(1e-3 * 1000.0));
}

/// The occurrence `#n` of `child` in `parent` placed by an `ITEM_DEFINED_TRANSFORMATION` of
/// `item_1` (in `rep_1`) onto `item_2` (in `rep_2`): NAUO #n, its PDS #n+1, the transformation
/// #n+2, the relationship #n+3, the CDSR #n+4.
fn occurrence(
    n: u64,
    name: &str,
    parent: u64,
    child: u64,
    reps: (u64, u64),
    items: (u64, u64),
) -> String {
    let mut t = String::new();
    let _ = write!(
        t,
        "#{n}=NEXT_ASSEMBLY_USAGE_OCCURRENCE('{n}','{name}','',#{parent},#{child},$);\n\
#{}=PRODUCT_DEFINITION_SHAPE('','',#{n});\n\
#{}=ITEM_DEFINED_TRANSFORMATION('','',#{},#{});\n\
#{}=(REPRESENTATION_RELATIONSHIP('','',#{},#{})REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{})SHAPE_REPRESENTATION_RELATIONSHIP());\n\
#{}=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{},#{});\n",
        n + 1,
        n + 2,
        items.0,
        items.1,
        n + 3,
        reps.0,
        reps.1,
        n + 2,
        n + 4,
        n + 3,
        n + 1
    );
    t
}

/// Top uses Sub twice and Part once; Sub uses Part once. Sub's placements are in centimetres,
/// Top's and Part's in millimetres. Four occurrences, six instances; the transformation is the
/// two placements as written, each in its representation's unit.
#[test]
fn an_assembly_with_a_shared_sub_assembly() {
    let mut t = String::from(BASE);
    t += &context(20, CM);
    t += &context(30, MM);
    t += &product(50, "Top");
    t += &product(70, "Sub");
    t += &product(90, "Part");
    // Top (mm): its own frame #161 and the places of its three components #163, #165, #167.
    t += "#160=SHAPE_REPRESENTATION('Top',(#161,#163,#165,#167),#30);\n#162=SHAPE_DEFINITION_REPRESENTATION(#54,#160);\n";
    t += "#161=AXIS2_PLACEMENT_3D('',#1610,$,$);\n#1610=CARTESIAN_POINT('',(0.,0.,0.));\n";
    t += "#163=AXIS2_PLACEMENT_3D('',#1630,#1631,#1632);\n#1630=CARTESIAN_POINT('',(100.,0.,0.));\n#1631=DIRECTION('',(0.,0.,1.));\n#1632=DIRECTION('',(0.,2.,0.));\n";
    t += "#165=AXIS2_PLACEMENT_3D('',#1650,$,$);\n#1650=CARTESIAN_POINT('',(0.,100.,0.));\n";
    t += "#167=AXIS2_PLACEMENT_3D('',#1670,$,$);\n#1670=CARTESIAN_POINT('',(0.,0.,100.));\n";
    // Sub (cm): its frame #171 and the place of its part #173.
    t += "#170=SHAPE_REPRESENTATION('Sub',(#171,#173),#20);\n#172=SHAPE_DEFINITION_REPRESENTATION(#74,#170);\n";
    t += "#171=AXIS2_PLACEMENT_3D('',#1710,$,$);\n#1710=CARTESIAN_POINT('',(1.,2.,3.));\n";
    t += "#173=AXIS2_PLACEMENT_3D('',#1730,$,$);\n#1730=CARTESIAN_POINT('',(5.,0.,0.));\n";
    // Part (mm): the solid and its frame.
    t += "#180=ADVANCED_BREP_SHAPE_REPRESENTATION('Part',(#101,#181),#30);\n#182=SHAPE_DEFINITION_REPRESENTATION(#94,#180);\n";
    t += "#181=AXIS2_PLACEMENT_3D('',#1810,$,$);\n#1810=CARTESIAN_POINT('',(0.,0.,0.));\n";
    t += &occurrence(600, "Sub:1", 50, 70, (170, 160), (171, 163));
    t += &occurrence(610, "Sub:2", 50, 70, (170, 160), (171, 165));
    t += &occurrence(620, "Part:1", 70, 90, (180, 170), (181, 173));
    t += &occurrence(630, "Part:2", 50, 90, (180, 160), (181, 167));
    let s = structure(&t);
    assert_eq!(
        s.products,
        [
            product_of(51, "Top"),
            product_of(71, "Sub"),
            product_of(91, "Part")
        ]
    );
    // Representations in id order: #160 Top, #170 Sub, #180 Part.
    let shapes: Vec<_> = s
        .definitions
        .iter()
        .map(|d| (d.entity, d.product, d.shapes.clone()))
        .collect();
    assert_eq!(
        shapes,
        [(50, 0, vec![0]), (70, 1, vec![1]), (90, 2, vec![2])]
    );
    assert_eq!(s.roots, [0]);
    let occ: Vec<_> = s
        .occurrences
        .iter()
        .map(|o| {
            (
                o.entity,
                o.id.as_str(),
                o.name.as_str(),
                o.parent,
                o.child,
                o.placements.clone(),
            )
        })
        .collect();
    assert_eq!(
        occ,
        [
            (600, "600", "Sub:1", 0, 1, vec![0]),
            (610, "610", "Sub:2", 0, 1, vec![1]),
            (620, "620", "Part:1", 1, 2, vec![2]),
            (630, "630", "Part:2", 0, 2, vec![3]),
        ]
    );
    assert_eq!(s.occurrences[0].reference_designator, None);
    // Sub:1: Sub's frame (cm: 1, 2, 3 → 10, 20, 30 mm) onto Top's place for it, as written.
    assert_eq!(
        s.links[0],
        Link {
            entity: 603,
            rep_1: 1,
            rep_2: 0,
            transformation: Some(Transformation::Items {
                entity: 602,
                item_1: placement([1.0 * CM_MM, 2.0 * CM_MM, 3.0 * CM_MM], None, None),
                item_2: placement(
                    [100.0, 0.0, 0.0],
                    Some([0.0, 0.0, 1.0]),
                    Some([0.0, 2.0, 0.0])
                ),
            }),
        }
    );
    // Part:1: Part's frame (mm) onto Sub's place for it (cm: 5 → 50 mm).
    assert_eq!(
        s.links[2].transformation,
        Some(Transformation::Items {
            entity: 622,
            item_1: placement([0.0; 3], None, None),
            item_2: placement([5.0 * CM_MM, 0.0, 0.0], None, None),
        })
    );
    // Expanded: Top; Sub:1 with its Part:1; Sub:2 with its Part:1; Part:2.
    let inst = s.instances().unwrap();
    assert_eq!(
        inst,
        [
            Instance {
                definition: 0,
                occurrence: None,
                parent: None
            },
            Instance {
                definition: 1,
                occurrence: Some(0),
                parent: Some(0)
            },
            Instance {
                definition: 2,
                occurrence: Some(2),
                parent: Some(1)
            },
            Instance {
                definition: 1,
                occurrence: Some(1),
                parent: Some(0)
            },
            Instance {
                definition: 2,
                occurrence: Some(2),
                parent: Some(3)
            },
            Instance {
                definition: 2,
                occurrence: Some(3),
                parent: Some(0)
            },
        ]
    );
}

/// SolidCraft's `exponential_assemblies_are_bounded`: P0 uses P1 twice, P1 uses P2 twice, … 30
/// levels. Reading is linear (31 definitions, 60 occurrences); expanding stops at
/// [`MAX_VISITS`]. Three levels expand to 1 + 2 + 4 + 8 instances.
#[test]
fn a_sub_assembly_shared_at_every_level_is_read_once() {
    let levels = |n: u64| {
        let mut t = String::from(BASE);
        for l in 0..=n {
            t += &product(1000 + l * 10, &format!("P{l}"));
            if l < n {
                for k in 0..2 {
                    let _ = writeln!(
                        t,
                        "#{}=NEXT_ASSEMBLY_USAGE_OCCURRENCE('','','',#{},#{},$);",
                        2000 + l * 10 + k,
                        1000 + l * 10,
                        1000 + (l + 1) * 10
                    );
                }
            }
        }
        t
    };
    let s = structure(&levels(30));
    assert_eq!(
        (s.definitions.len(), s.occurrences.len(), s.roots.clone()),
        (31, 60, vec![0])
    );
    assert_eq!(
        s.instances(),
        Err(format!("more than {MAX_VISITS} assembly instances"))
    );
    assert_eq!(structure(&levels(3)).instances().map(|i| i.len()), Ok(15));
}

/// A representation placed twice by `MAPPED_ITEM`s: once by a placement, once by a
/// non-uniform operator. The map's origin is in the mapped representation's unit (inches), each
/// target in the unit of the representation that lists it (millimetres).
#[test]
fn mapped_items() {
    let mut t = String::from(BASE);
    t += &context(20, MM);
    t += &context(
        30,
        "(CONVERSION_BASED_UNIT('INCH',#40)LENGTH_UNIT()NAMED_UNIT(*));\n#40=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#41);\n#41=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))",
    );
    t += &product(50, "Holder");
    t += "#60=SHAPE_REPRESENTATION('',(#110,#115),#20);\n#61=SHAPE_DEFINITION_REPRESENTATION(#54,#60);\n";
    t += "#110=MAPPED_ITEM('first',#111,#112);\n#111=REPRESENTATION_MAP(#113,#120);\n";
    t += "#112=AXIS2_PLACEMENT_3D('',#1120,#1121,$);\n#1120=CARTESIAN_POINT('',(5.,0.,0.));\n#1121=DIRECTION('',(0.,0.,-1.));\n";
    t += "#113=AXIS2_PLACEMENT_3D('',#1130,$,$);\n#1130=CARTESIAN_POINT('',(1.,0.,0.));\n";
    t += "#115=MAPPED_ITEM('second',#111,#116);\n";
    t += "#116=CARTESIAN_TRANSFORMATION_OPERATOR_3D_NON_UNIFORM('','',#1161,$,#1160,2.,$,$,3.);\n#1160=CARTESIAN_POINT('',(0.,7.,0.));\n#1161=DIRECTION('',(0.,1.,0.));\n";
    t += "#120=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#101,#113),#30);\n";
    let s = structure(&t);
    assert_eq!(
        s.representations[0].items,
        [
            Item {
                entity: 110,
                name: "first".into(),
                kind: ItemKind::Mapped {
                    map: 0,
                    target: placement([5.0, 0.0, 0.0], Some([0.0, 0.0, -1.0]), None),
                },
            },
            Item {
                entity: 115,
                name: "second".into(),
                kind: ItemKind::Mapped {
                    map: 0,
                    target: Frame::Operator(Operator {
                        axis1: Some([0.0, 1.0, 0.0]),
                        axis2: None,
                        axis3: None,
                        origin: [0.0, 7.0, 0.0],
                        scale: Some(2.0),
                        scale2: None,
                        scale3: Some(3.0),
                    }),
                },
            },
        ]
    );
    assert_eq!(
        s.maps,
        [RepresentationMap {
            entity: 111,
            origin: placement([1.0 * (25.4 * 1.0), 0.0, 0.0], None, None),
            representation: 1,
        }]
    );
    assert_eq!(s.representations[1].entity, 120);
    assert_eq!(
        s.representations[1].items[0].kind,
        ItemKind::Body(BodyKind::ManifoldSolidBrep)
    );
}

/// Colours on a body and on a face: a fill colour, a rendering colour with a transparency, a
/// pre-defined colour, one ISO 10303-46 does not name, a curve style's colour, and an override.
#[test]
fn colours_of_bodies_and_faces() {
    let mut t = String::from(BASE);
    t += "#102=ADVANCED_FACE('',(),#103,.T.);\n#103=PLANE('',$);\n";
    // The body: fill colour and the curve colour of its edges.
    t += "#70=STYLED_ITEM('body',(#71,#78),#101);\n#71=PRESENTATION_STYLE_ASSIGNMENT((#72));\n#72=SURFACE_STYLE_USAGE(.POSITIVE.,#73);\n#73=SURFACE_SIDE_STYLE('',(#74));\n#74=SURFACE_STYLE_FILL_AREA(#75);\n#75=FILL_AREA_STYLE('',(#76));\n#76=FILL_AREA_STYLE_COLOUR('',#77);\n#77=COLOUR_RGB('steel',0.2,0.25,1.5);\n";
    t += "#78=PRESENTATION_STYLE_ASSIGNMENT((#79));\n#79=CURVE_STYLE('',#80,POSITIVE_LENGTH_MEASURE(0.1),#81);\n#80=DRAUGHTING_PRE_DEFINED_CURVE_FONT('continuous');\n#81=DRAUGHTING_PRE_DEFINED_COLOUR('black');\n";
    // The face, over the body's style: rendered red, a quarter transparent, filled 'mauve'.
    t += "#90=OVER_RIDING_STYLED_ITEM('face',(#91),#102,#70);\n#91=PRESENTATION_STYLE_ASSIGNMENT((#92));\n#92=SURFACE_STYLE_USAGE(.BOTH.,#93);\n#93=SURFACE_SIDE_STYLE('',(#94,#97));\n";
    t += "#94=SURFACE_STYLE_RENDERING_WITH_PROPERTIES(.NORMAL_SHADING.,#95,(#96));\n#95=DRAUGHTING_PRE_DEFINED_COLOUR('red');\n#96=SURFACE_STYLE_TRANSPARENT(0.25);\n";
    t += "#97=SURFACE_STYLE_FILL_AREA(#98);\n#98=FILL_AREA_STYLE('',(#99));\n#99=FILL_AREA_STYLE_COLOUR('',#89);\n#89=DRAUGHTING_PRE_DEFINED_COLOUR('mauve');\n";
    let s = structure(&t);
    assert_eq!(
        s.styles,
        [
            StyledItem {
                entity: 70,
                name: "body".into(),
                item: 101,
                overrides: None,
                surface: vec![SurfaceStyle {
                    side: Side::Positive,
                    fill: Some(Colour::Rgb {
                        name: "steel".into(),
                        rgb: [0.2, 0.25, 1.5],
                    }),
                    rendering: None,
                    transparency: None,
                }],
                curve: vec![Colour::PreDefined {
                    name: "black".into(),
                    rgb: Some([0.0, 0.0, 0.0]),
                }],
            },
            StyledItem {
                entity: 90,
                name: "face".into(),
                item: 102,
                overrides: Some(70),
                surface: vec![SurfaceStyle {
                    side: Side::Both,
                    fill: Some(Colour::PreDefined {
                        name: "mauve".into(),
                        rgb: None,
                    }),
                    rendering: Some(Colour::PreDefined {
                        name: "red".into(),
                        rgb: Some([1.0, 0.0, 0.0]),
                    }),
                    transparency: Some(0.25),
                }],
                curve: Vec::new(),
            },
        ]
    );
}

/// ISO 10303-46's eight pre-defined colours.
#[test]
fn pre_defined_colours() {
    let names = [
        "red", "green", "blue", "yellow", "magenta", "cyan", "black", "white",
    ];
    let rgb: Vec<_> = names.iter().map(|n| super::pre_defined_rgb(n)).collect();
    assert_eq!(
        rgb,
        [
            Some([1.0, 0.0, 0.0]),
            Some([0.0, 1.0, 0.0]),
            Some([0.0, 0.0, 1.0]),
            Some([1.0, 1.0, 0.0]),
            Some([1.0, 0.0, 1.0]),
            Some([0.0, 1.0, 1.0]),
            Some([0.0, 0.0, 0.0]),
            Some([1.0, 1.0, 1.0]),
        ]
    );
    assert_eq!(super::pre_defined_rgb("Red"), None);
}

/// Names keep their escapes decoded, and are not trimmed.
#[test]
fn names_with_escapes() {
    let mut t = String::from(BASE);
    t += "#51=PRODUCT('P-\\X2\\00E9\\X0\\',' B\\X\\E4r ''7'' ','\\X2\\5706\\X0\\',(#52));\n#52=PRODUCT_CONTEXT('',#9,'mechanical');\n#53=PRODUCT_DEFINITION_FORMATION('','',#51);\n#50=PRODUCT_DEFINITION('d\\\\1',$,#53,#8);\n";
    t += &product(70, "Child");
    t += "#600=NEXT_ASSEMBLY_USAGE_OCCURRENCE('NAUO1','Gr\\X\\FC\\X\\DFe:1','',#50,#70,'R\\X2\\00E9\\X0\\f');\n";
    let s = structure(&t);
    assert_eq!(
        s.products[0],
        Product {
            entity: 51,
            id: "P-\u{e9}".into(),
            name: " B\u{e4}r '7' ".into(),
            description: "\u{5706}".into(),
        }
    );
    assert_eq!(
        (
            s.definitions[0].id.as_str(),
            s.definitions[0].description.as_str()
        ),
        ("d\\1", "")
    );
    let o = &s.occurrences[0];
    assert_eq!(
        (
            o.id.as_str(),
            o.name.as_str(),
            o.reference_designator.as_deref()
        ),
        ("NAUO1", "Gr\u{fc}\u{df}e:1", Some("R\u{e9}f"))
    );
}

/// A chain of `n` + 1 definitions, each the only component of the one before.
fn chain(n: u64) -> String {
    let mut t = String::from(BASE);
    for l in 0..=n {
        t += &product(1000 + l * 10, &format!("P{l}"));
        if l < n {
            let _ = writeln!(
                t,
                "#{}=NEXT_ASSEMBLY_USAGE_OCCURRENCE('','','',#{},#{},$);",
                5000 + l,
                1000 + l * 10,
                1000 + (l + 1) * 10
            );
        }
    }
    t
}

#[test]
fn nesting_is_bounded() {
    let depth = MAX_ASSEMBLY_DEPTH as u64;
    let s = structure(&chain(depth));
    assert_eq!(s.instances().map(|i| i.len()), Ok(MAX_ASSEMBLY_DEPTH + 1));
    assert_eq!(refused(&chain(depth + 1)), "assembly nested too deeply");
}

/// What is refused, with its message.
#[test]
fn refusals_say_why() {
    let two = || {
        let mut t = String::from(BASE);
        t += &product(50, "A");
        t += &product(70, "B");
        t
    };
    // A contains B contains A (SolidCraft's `assembly_cycles_are_bounded`).
    let cycle = two()
        + "#90=NEXT_ASSEMBLY_USAGE_OCCURRENCE('1','','',#50,#70,$);\n#91=NEXT_ASSEMBLY_USAGE_OCCURRENCE('2','','',#70,#50,$);\n";
    assert_eq!(refused(&cycle), "assembly cycle");
    let itself = two() + "#90=NEXT_ASSEMBLY_USAGE_OCCURRENCE('1','','',#50,#50,$);\n";
    assert_eq!(refused(&itself), "assembly cycle");
    let missing_child = two() + "#90=NEXT_ASSEMBLY_USAGE_OCCURRENCE('1','','',#50,#999,$);\n";
    assert_eq!(
        refused(&missing_child),
        "occurrence #90: #999: expected a PRODUCT_DEFINITION"
    );
    let not_a_definition = two() + "#90=NEXT_ASSEMBLY_USAGE_OCCURRENCE('1','','',#50,#51,$);\n";
    assert_eq!(
        refused(&not_a_definition),
        "occurrence #90: #51: expected a PRODUCT_DEFINITION"
    );
    let missing_product = String::from(BASE)
        + "#53=PRODUCT_DEFINITION_FORMATION('','',#51);\n#50=PRODUCT_DEFINITION('design','',#53,#8);\n";
    assert_eq!(refused(&missing_product), "#51: expected a PRODUCT");
    let missing_rep = two() + "#61=SHAPE_DEFINITION_REPRESENTATION(#54,#999);\n";
    assert_eq!(refused(&missing_rep), "missing entity #999");
    let not_a_rep = two() + "#61=SHAPE_DEFINITION_REPRESENTATION(#54,#101);\n";
    assert_eq!(
        refused(&not_a_rep),
        "#101: expected a representation, found MANIFOLD_SOLID_BREP"
    );
    let missing_item = two()
        + "#60=SHAPE_REPRESENTATION('',(#101,#999),$);\n#61=SHAPE_DEFINITION_REPRESENTATION(#54,#60);\n";
    assert_eq!(refused(&missing_item), "missing entity #999");
    let mapped = |rest: &str| {
        two()
            + "#60=SHAPE_REPRESENTATION('',(#110),$);\n#61=SHAPE_DEFINITION_REPRESENTATION(#54,#60);\n#110=MAPPED_ITEM('',#111,#112);\n#112=AXIS2_PLACEMENT_3D('',#113,$,$);\n#113=CARTESIAN_POINT('',(0.,0.,0.));\n"
            + rest
    };
    assert_eq!(
        refused(&mapped("#111=REPRESENTATION_MAP(#112,#60);\n")),
        "mapped item cycle"
    );
    assert_eq!(
        refused(&mapped("#111=SHAPE_REPRESENTATION('',(),$);\n")),
        "#110: mapping source #111 is a SHAPE_REPRESENTATION, not a REPRESENTATION_MAP"
    );
    assert_eq!(
        refused(&mapped("#111=REPRESENTATION_MAP(#112,#999);\n")),
        "missing entity #999"
    );
    assert_eq!(
        refused(&mapped(
            "#111=REPRESENTATION_MAP(#112,#120);\n#120=SHAPE_REPRESENTATION('',(#101),$);\n"
        )
        .replace(
            "#110=MAPPED_ITEM('',#111,#112);",
            "#110=MAPPED_ITEM('',#111,#114);#114=CARTESIAN_TRANSFORMATION_OPERATOR_3D('','',$,$,#113,0.,$);"
        )),
        "mapped item #110: operator scale: expected a number other than zero"
    );
    assert_eq!(
        refused(&mapped(
            "#111=REPRESENTATION_MAP(#113,#120);\n#120=SHAPE_REPRESENTATION('',(#101),$);\n"
        )),
        "representation map #111: #113: expected a placement, found CARTESIAN_POINT"
    );
    let styled = |rest: &str| two() + rest;
    assert_eq!(
        refused(&styled("#170=STYLED_ITEM('',(),#999);\n")),
        "styled item #170: missing entity #999"
    );
    assert_eq!(
        refused(&styled(
            "#170=STYLED_ITEM('',(#171),#101);\n#171=PRESENTATION_STYLE_ASSIGNMENT((#172));\n#172=SURFACE_STYLE_USAGE(.BOTH.,#173);\n#173=SURFACE_SIDE_STYLE('',(#174));\n#174=SURFACE_STYLE_RENDERING(.NORMAL_SHADING.,#175);\n#175=COLOUR_RGB('',1.,$,0.);\n"
        )),
        "styled item #170: green: expected a number"
    );
    let placed = two()
        + "#60=SHAPE_REPRESENTATION('',(),$);\n#80=SHAPE_REPRESENTATION('',(),$);\n#90=NEXT_ASSEMBLY_USAGE_OCCURRENCE('1','','',#50,#70,$);\n#91=PRODUCT_DEFINITION_SHAPE('','',#90);\n#93=(REPRESENTATION_RELATIONSHIP('','',#60,#80)REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#92)SHAPE_REPRESENTATION_RELATIONSHIP());\n#94=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#93,#91);\n";
    assert_eq!(refused(&placed), "relationship #93: missing entity #92");
}
