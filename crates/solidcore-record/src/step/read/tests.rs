//! STEP geometry, one entity at a time: what the reader stores for each kind, in file units and
//! in inches and degrees, and what it refuses. Expected values are the file's numbers times the
//! unit factors, written as the same products, so every comparison is exact.
//!
//! Origin: SolidCraft's `crates/step/src/geom_tests.rs` at `a5b71ca2`, without the parts that
//! test evaluation (`Curve::point`, `Surface::point`) or the writer, which are not in the record;
//! a case that checked a value by evaluating checks the data the value comes from instead.

use std::f64::consts::{FRAC_1_SQRT_2, PI};
use std::fmt::Write as _;
use std::sync::Arc;

use super::{MAX_GEOM_DEPTH, Reader};
use crate::step::geom::{
    Associated, Axis1, Curve, CurveForm, KnotSpec, Logical, Placement, PreferredRep, Surface,
    SurfaceCurveKind, SurfaceForm, Trim, TrimPreference, Vector,
};
use crate::step::p21::{self, Exchange};
use crate::step::units::Units;

const MM: Units = Units { len: 1.0, ang: 1.0 };
const DEG: f64 = PI / 180.0;
/// Inches and degrees.
const INCH_DEG: Units = Units {
    len: 25.4,
    ang: DEG,
};

fn file(data: &str) -> Exchange {
    let t = format!(
        "ISO-10303-21;\nHEADER;FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));ENDSEC;\nDATA;\n{data}\nENDSEC;\nEND-ISO-10303-21;\n"
    );
    match p21::parse(&t) {
        Ok(ex) => ex,
        Err(e) => panic!("{e}\n{t}"),
    }
}

/// Common entities: #1 origin, #2 +Z, #3 +X, #4 the world placement, #5 +Y.
const BASE: &str = "#1=CARTESIAN_POINT('',(0.,0.,0.));#2=DIRECTION('',(0.,0.,1.));\
#3=DIRECTION('',(1.,0.,0.));#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);#5=DIRECTION('',(0.,1.,0.));";

/// The world placement `#4` as the file writes it.
const WORLD: Placement = Placement {
    origin: [0.0, 0.0, 0.0],
    axis: Some([0.0, 0.0, 1.0]),
    ref_direction: Some([1.0, 0.0, 0.0]),
};

fn curve_err(data: &str, id: u64, units: Units) -> Result<Arc<Curve>, String> {
    let ex = file(&format!("{BASE}{data}"));
    Reader::new(&ex, units).curve(id)
}

fn curve(data: &str, id: u64, units: Units) -> Arc<Curve> {
    curve_err(data, id, units).unwrap_or_else(|e| panic!("{e}"))
}

fn surface_err(data: &str, id: u64, units: Units) -> Result<Arc<Surface>, String> {
    let ex = file(&format!("{BASE}{data}"));
    Reader::new(&ex, units).surface(id)
}

fn surface(data: &str, id: u64, units: Units) -> Arc<Surface> {
    surface_err(data, id, units).unwrap_or_else(|e| panic!("{e}"))
}

// Curves.

#[test]
fn line() {
    let c = curve(
        "#10=CARTESIAN_POINT('',(1.,2.,3.));#11=VECTOR('',#3,5.);#12=LINE('',#10,#11);",
        12,
        MM,
    );
    assert_eq!(
        *c,
        Curve::Line {
            origin: [1.0, 2.0, 3.0],
            vector: Vector {
                orientation: [1.0, 0.0, 0.0],
                magnitude: 5.0,
                scale: [1.0; 3],
            },
        }
    );
}

#[test]
fn circle_and_ellipse() {
    let c = curve("#10=CIRCLE('',#4,10.);", 10, MM);
    assert_eq!(
        *c,
        Curve::Circle {
            position: WORLD,
            radius: 10.0
        }
    );
    let e = curve("#10=ELLIPSE('',#4,10.,4.);", 10, MM);
    assert_eq!(
        *e,
        Curve::Ellipse {
            position: WORLD,
            semi_axis_1: 10.0,
            semi_axis_2: 4.0
        }
    );
}

#[test]
fn hyperbola_and_parabola() {
    let h = curve("#10=HYPERBOLA('',#4,2.,3.);", 10, MM);
    assert_eq!(
        *h,
        Curve::Hyperbola {
            position: WORLD,
            semi_axis: 2.0,
            semi_imag_axis: 3.0
        }
    );
    let p = curve("#10=PARABOLA('',#4,0.5);", 10, MM);
    assert_eq!(
        *p,
        Curve::Parabola {
            position: WORLD,
            focal_dist: 0.5
        }
    );
    // A negative focal distance is the parabola opening the other way; zero is refused.
    let p = curve("#10=PARABOLA('',#4,-0.5);", 10, MM);
    assert!(matches!(*p, Curve::Parabola { focal_dist, .. } if focal_dist == -0.5));
    assert_eq!(
        curve_err("#10=PARABOLA('',#4,0.);", 10, MM),
        Err("parabola focal distance must not be zero".into())
    );
}

#[test]
fn bspline_curve_with_knots() {
    // A quadratic Bézier from (0,0) through control (1,2) to (2,0).
    let c = curve(
        "#10=CARTESIAN_POINT('',(0.,0.,0.));#11=CARTESIAN_POINT('',(1.,2.,0.));\
         #12=CARTESIAN_POINT('',(2.,0.,0.));#13=B_SPLINE_CURVE_WITH_KNOTS('',2,(#10,#11,#12),\
         .UNSPECIFIED.,.F.,.F.,(3,3),(0.,1.),.PIECEWISE_BEZIER_KNOTS.);",
        13,
        MM,
    );
    let Curve::BSpline(b) = &*c else {
        panic!("{c:?}")
    };
    assert_eq!(b.degree, 2);
    assert_eq!(
        b.points,
        vec![[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [2.0, 0.0, 0.0]]
    );
    assert_eq!(
        (b.knots.clone(), b.multiplicities.clone()),
        (vec![0.0, 1.0], vec![3, 3])
    );
    assert_eq!(b.knot_spec, KnotSpec::PiecewiseBezierKnots);
    assert_eq!(b.weights, None);
    assert_eq!(b.form, CurveForm::Unspecified);
    assert_eq!(
        (b.closed, b.self_intersect),
        (Logical::False, Logical::False)
    );
}

#[test]
fn rational_bspline_curve_keeps_its_weights() {
    // The quarter circle as a rational quadratic: weights 1, 1/√2, 1 (written as the shortest
    // decimal of FRAC_1_SQRT_2, so it reads as exactly that double).
    let w = FRAC_1_SQRT_2;
    let c = curve(
        &format!(
            "#10=CARTESIAN_POINT('',(1.,0.,0.));#11=CARTESIAN_POINT('',(1.,1.,0.));\
             #12=CARTESIAN_POINT('',(0.,1.,0.));#13=(BOUNDED_CURVE()B_SPLINE_CURVE(2,(#10,#11,#12),\
             .CIRCULAR_ARC.,.F.,.U.)B_SPLINE_CURVE_WITH_KNOTS((3,3),(0.,1.),.UNSPECIFIED.)CURVE()\
             GEOMETRIC_REPRESENTATION_ITEM()RATIONAL_B_SPLINE_CURVE((1.,{w:?},1.))\
             REPRESENTATION_ITEM(''));"
        ),
        13,
        MM,
    );
    let Curve::BSpline(b) = &*c else {
        panic!("{c:?}")
    };
    assert_eq!(b.form, CurveForm::CircularArc);
    assert_eq!(b.weights, Some(vec![1.0, w, 1.0]));
    assert_eq!(
        b.points,
        vec![[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]]
    );
    assert_eq!(
        (b.knots.clone(), b.multiplicities.clone()),
        (vec![0.0, 1.0], vec![3, 3])
    );
    assert_eq!(
        (b.knot_spec, b.self_intersect),
        (KnotSpec::Unspecified, Logical::Unknown)
    );
}

#[test]
fn implied_knots_and_repeated_knots() {
    let pts = "#10=CARTESIAN_POINT('',(0.,0.,0.));#11=CARTESIAN_POINT('',(1.,1.,0.));\
               #12=CARTESIAN_POINT('',(2.,0.,0.));#13=CARTESIAN_POINT('',(3.,1.,0.));";
    let knots = |c: &Curve| {
        let Curve::BSpline(b) = c else {
            panic!("{c:?}")
        };
        (b.knots.clone(), b.multiplicities.clone(), b.knot_spec)
    };
    let q = curve(
        &format!("{pts}#14=QUASI_UNIFORM_CURVE('',2,(#10,#11,#12,#13),.UNSPECIFIED.,.F.,.F.);"),
        14,
        MM,
    );
    assert_eq!(
        knots(&q),
        (
            vec![0.0, 1.0, 2.0],
            vec![3, 1, 3],
            KnotSpec::QuasiUniformKnots
        )
    );
    let bez = curve(
        &format!("{pts}#14=BEZIER_CURVE('',3,(#10,#11,#12,#13),.UNSPECIFIED.,.F.,.F.);"),
        14,
        MM,
    );
    assert_eq!(
        knots(&bez),
        (vec![0.0, 1.0], vec![4, 4], KnotSpec::PiecewiseBezierKnots)
    );
    // Uniform: points + degree + 1 knots of multiplicity 1, from -degree.
    let uni = curve(
        &format!("{pts}#14=UNIFORM_CURVE('',2,(#10,#11,#12,#13),.UNSPECIFIED.,.F.,.F.);"),
        14,
        MM,
    );
    assert_eq!(
        knots(&uni),
        (
            vec![-2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0],
            vec![1; 7],
            KnotSpec::UniformKnots
        )
    );
    // A Bézier needs degree·k + 1 points.
    assert_eq!(
        curve_err(
            &format!("{pts}#14=BEZIER_CURVE('',2,(#10,#11,#12,#13),.UNSPECIFIED.,.F.,.F.);"),
            14,
            MM
        ),
        Err("Bezier control point count".into())
    );
    // A knot written twice is one knot.
    let rep = curve(
        &format!(
            "{pts}#14=B_SPLINE_CURVE_WITH_KNOTS('',2,(#10,#11,#12,#13),.UNSPECIFIED.,.F.,.F.,\
             (3,1,1,2),(0.,1.,1.,2.),.UNSPECIFIED.);"
        ),
        14,
        MM,
    );
    assert_eq!(
        knots(&rep),
        (vec![0.0, 1.0, 2.0], vec![3, 2, 2], KnotSpec::Unspecified)
    );
}

#[test]
fn polyline_and_trimmed_curve() {
    let pl = curve(
        "#10=CARTESIAN_POINT('',(0.,0.,0.));#11=CARTESIAN_POINT('',(4.,0.,0.));\
         #12=CARTESIAN_POINT('',(4.,2.,0.));#13=POLYLINE('',(#10,#11,#12));",
        13,
        MM,
    );
    assert_eq!(
        *pl,
        Curve::Polyline(vec![[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [4.0, 2.0, 0.0]])
    );
    // A circle trimmed in degrees: the parameters become radians, the point stays a point.
    let deg = Units { len: 1.0, ang: DEG };
    let t = curve(
        "#10=CIRCLE('',#4,10.);#11=CARTESIAN_POINT('',(10.,0.,0.));\
         #12=TRIMMED_CURVE('',#10,(#11,PARAMETER_VALUE(0.)),(PARAMETER_VALUE(90.)),.T.,.PARAMETER.);",
        12,
        deg,
    );
    let Curve::Trimmed {
        basis,
        trim_1,
        trim_2,
        sense,
        preference,
    } = &*t
    else {
        panic!("{t:?}")
    };
    assert_eq!(
        **basis,
        Curve::Circle {
            position: WORLD,
            radius: 10.0
        }
    );
    assert_eq!(
        *trim_1,
        vec![Trim::Point([10.0, 0.0, 0.0]), Trim::Parameter(0.0)]
    );
    assert_eq!(*trim_2, vec![Trim::Parameter(90.0 * DEG)]);
    assert!(*sense);
    assert_eq!(*preference, TrimPreference::Parameter);
    // A line's parameters are plain numbers: not scaled by the angle unit.
    let t = curve(
        "#10=VECTOR('',#3,1.);#11=LINE('',#1,#10);\
         #12=TRIMMED_CURVE('',#11,(2.),(PARAMETER_VALUE(5.)),.F.,.UNSPECIFIED.);",
        12,
        deg,
    );
    let Curve::Trimmed {
        trim_1,
        trim_2,
        sense,
        preference,
        ..
    } = &*t
    else {
        panic!("{t:?}")
    };
    assert_eq!(
        (trim_1.clone(), trim_2.clone()),
        (vec![Trim::Parameter(2.0)], vec![Trim::Parameter(5.0)])
    );
    assert!(!*sense);
    assert_eq!(*preference, TrimPreference::Unspecified);
}

/// A trimmed curve's parameters are its basis's: through an offset, a trimmed or a surface
/// curve of a conic they are still angles, so a file in degrees gives radians (a review on
/// #2592). SolidCraft's reader looked at the direct basis only and kept 90 unscaled.
#[test]
fn trims_on_a_curve_over_a_conic_are_angles() {
    let deg = Units { len: 1.0, ang: DEG };
    let trims = |data: &str, id: u64| {
        let t = curve(&format!("#10=CIRCLE('',#4,10.);{data}"), id, deg);
        let Curve::Trimmed { trim_1, trim_2, .. } = &*t else {
            panic!("{t:?}")
        };
        (trim_1.clone(), trim_2.clone())
    };
    let expected = (
        vec![Trim::Parameter(0.0)],
        vec![Trim::Parameter(90.0 * DEG)],
    );
    // An offset of the circle.
    assert_eq!(
        trims(
            "#11=OFFSET_CURVE_3D('',#10,1.,.F.,#2);\
             #12=TRIMMED_CURVE('',#11,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(90.)),.T.,.PARAMETER.);",
            12
        ),
        expected
    );
    // A trimmed curve of a trimmed circle.
    assert_eq!(
        trims(
            "#11=TRIMMED_CURVE('',#10,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(180.)),.T.,.PARAMETER.);\
             #12=TRIMMED_CURVE('',#11,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(90.)),.T.,.PARAMETER.);",
            12
        ),
        expected
    );
    // A surface curve whose 3D curve is the circle.
    assert_eq!(
        trims(
            "#11=PLANE('',#4);#13=SURFACE_CURVE('',#10,(#11),.CURVE_3D.);\
             #12=TRIMMED_CURVE('',#13,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(90.)),.T.,.PARAMETER.);",
            12
        ),
        expected
    );
}

#[test]
fn offset_curve() {
    let c = curve(
        "#10=CIRCLE('',#4,10.);#11=OFFSET_CURVE_3D('',#10,1.,.F.,#2);",
        11,
        MM,
    );
    let Curve::OffsetCurve3d {
        basis,
        distance,
        self_intersect,
        ref_direction,
    } = &*c
    else {
        panic!("{c:?}")
    };
    assert!(matches!(**basis, Curve::Circle { radius, .. } if radius == 10.0));
    assert_eq!(
        (*distance, *self_intersect, *ref_direction),
        (1.0, Logical::False, [0.0, 0.0, 1.0])
    );
    // In inches the distance is a length; the direction keeps its ratios.
    let c = curve(
        "#10=CIRCLE('',#4,10.);#11=DIRECTION('',(0.,0.,3.));#12=OFFSET_CURVE_3D('',#10,1.,.U.,#11);",
        12,
        INCH_DEG,
    );
    assert!(
        matches!(*c, Curve::OffsetCurve3d { distance, ref_direction, .. }
        if distance == 25.4 && ref_direction == [0.0, 0.0, 3.0])
    );
}

#[test]
fn units_scale_lengths_not_directions() {
    let c = curve(
        "#10=CARTESIAN_POINT('',(1.,0.,0.));#11=VECTOR('',#3,2.);#12=LINE('',#10,#11);",
        12,
        INCH_DEG,
    );
    assert_eq!(
        *c,
        Curve::Line {
            origin: [25.4, 0.0, 0.0],
            vector: Vector {
                orientation: [1.0, 0.0, 0.0],
                magnitude: 2.0,
                scale: [25.4; 3],
            },
        }
    );
    let c = curve(
        "#10=CARTESIAN_POINT('',(1.,2.,3.));#11=AXIS2_PLACEMENT_3D('',#10,#2,#3);#12=CIRCLE('',#11,2.);",
        12,
        INCH_DEG,
    );
    assert_eq!(
        *c,
        Curve::Circle {
            position: Placement {
                origin: [25.4, 2.0 * 25.4, 3.0 * 25.4],
                axis: Some([0.0, 0.0, 1.0]),
                ref_direction: Some([1.0, 0.0, 0.0]),
            },
            radius: 2.0 * 25.4,
        }
    );
}

#[test]
fn placements_keep_the_directions_the_file_gives() {
    // Neither normalised nor made orthogonal (was: axis (0,0,1), ref_direction (1,0,0)).
    let ex = file(&format!(
        "{BASE}#10=DIRECTION('',(0.,0.,2.));#11=DIRECTION('',(1.,0.,1.));\
         #12=AXIS2_PLACEMENT_3D('',#1,#10,#11);#13=AXIS2_PLACEMENT_3D('',#1,$,$);\
         #14=CARTESIAN_POINT('',(1.,2.));#15=DIRECTION('',(0.,3.));#16=AXIS2_PLACEMENT_2D('',#14,#15);\
         #17=DIRECTION('',(1.E-13,0.,0.));#18=AXIS2_PLACEMENT_3D('',#1,#2,#17);\
         #19=AXIS1_PLACEMENT('',#14,$);#20=AXIS1_PLACEMENT('',#1,#10);"
    ));
    let r = Reader::new(&ex, MM);
    assert_eq!(
        r.placement(12),
        Ok(Placement {
            origin: [0.0; 3],
            axis: Some([0.0, 0.0, 2.0]),
            ref_direction: Some([1.0, 0.0, 1.0]),
        })
    );
    // Both omitted: ISO 10303-42's +Z and its chosen X are the kernel's to fill in.
    assert_eq!(
        r.placement(13),
        Ok(Placement {
            origin: [0.0; 3],
            axis: None,
            ref_direction: None,
        })
    );
    // A 2D placement as a frame in the XY plane: no axis.
    assert_eq!(
        r.frame(16),
        Ok(Placement {
            origin: [1.0, 2.0, 0.0],
            axis: None,
            ref_direction: Some([0.0, 3.0, 0.0]),
        })
    );
    assert_eq!(r.frame(12), r.placement(12));
    // … but not where a 3D placement is asked for.
    assert_eq!(
        r.placement(16),
        Err("#16: unexpected placement AXIS2_PLACEMENT_2D in 3D".into())
    );
    // A direction too short to normalise is still data (was refused below a length of 1e-12);
    // the kernel decides when it builds.
    assert_eq!(
        r.placement(18).map(|p| p.ref_direction),
        Ok(Some([1e-13, 0.0, 0.0]))
    );
    assert_eq!(r.direction(17), Ok([1e-13, 0.0, 0.0]));
    assert_eq!(
        r.axis1(19),
        Ok(Axis1 {
            origin: [1.0, 2.0, 0.0],
            direction: None
        })
    );
    assert_eq!(
        r.axis1(20),
        Ok(Axis1 {
            origin: [0.0; 3],
            direction: Some([0.0, 0.0, 2.0])
        })
    );
    // A point with three coordinates where a 2D placement is read.
    assert_eq!(
        r.frame(14).map(|_| ()),
        Err("placement location: expected a reference".into())
    );
}

#[test]
fn shared_curves_are_read_once() {
    let ex = file(&format!(
        "{BASE}#10=CIRCLE('',#4,1.);#11=TRIMMED_CURVE('',#10,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(1.)),.T.,.PARAMETER.);"
    ));
    let mut r = Reader::new(&ex, MM);
    let t = r.curve(11).unwrap();
    let c = r.curve(10).unwrap();
    let Curve::Trimmed { basis, .. } = &*t else {
        panic!("{t:?}")
    };
    assert!(Arc::ptr_eq(basis, &c));
    assert!(Arc::ptr_eq(&r.curve(10).unwrap(), &c));
}

/// A chain of `n` trimmed curves `#20, #21, …`, each the basis of the one before, ending in a
/// line.
fn trimmed_chain(n: u64) -> String {
    let mut d = String::from("#10=VECTOR('',#3,1.);#11=LINE('',#1,#10);");
    for k in 0..n {
        let basis = if k + 1 == n { 11 } else { 21 + k };
        write!(
            d,
            "#{}=TRIMMED_CURVE('',#{basis},(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(1.)),.T.,.PARAMETER.);",
            20 + k
        )
        .unwrap();
    }
    d
}

#[test]
fn nesting_is_bounded() {
    // The line at depth MAX_GEOM_DEPTH is read; one deeper is refused.
    let n = MAX_GEOM_DEPTH as u64;
    assert!(curve_err(&trimmed_chain(n), 20, MM).is_ok());
    assert_eq!(
        curve_err(&trimmed_chain(n + 1), 20, MM),
        Err("curve definitions nested too deeply".into())
    );
    // A curve that is its own basis.
    assert_eq!(
        curve_err(
            "#10=TRIMMED_CURVE('',#10,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(1.)),.T.,.PARAMETER.);",
            10,
            MM
        ),
        Err("curve definitions nested too deeply".into())
    );
}

#[test]
fn refusals_say_why() {
    for (data, id, why) in [
        // A missing reference, and a reference to the wrong kind.
        ("#10=CIRCLE('',#99,1.);", 10, "missing entity #99"),
        (
            "#10=LINE('',#1,#1);",
            10,
            "#1: expected VECTOR, found CARTESIAN_POINT",
        ),
        (
            "#10=LINE('',#1,$);",
            10,
            "line vector: expected a reference",
        ),
        // Signs and sizes.
        (
            "#10=CIRCLE('',#4,-1.);",
            10,
            "circle radius must be positive",
        ),
        (
            "#10=ELLIPSE('',#4,2.,0.);",
            10,
            "ellipse axis must be positive",
        ),
        (
            "#10=CIRCLE('',#4,1.E10);",
            10,
            "radius: 10000000000 out of range",
        ),
        (
            "#10=VECTOR('',#3,-1.);#11=LINE('',#1,#10);",
            11,
            "negative vector magnitude",
        ),
        (
            "#10=VECTOR('',#3,0.);#11=LINE('',#1,#10);",
            11,
            "zero vector",
        ),
        (
            "#10=DIRECTION('',(0.,0.,0.));#11=VECTOR('',#10,1.);#12=LINE('',#1,#11);",
            12,
            "#10: zero direction",
        ),
        // Knots.
        (
            "#10=B_SPLINE_CURVE_WITH_KNOTS('',1,(#1,#1,#1),.UNSPECIFIED.,.F.,.F.,(2,2),(0.,1.),.UNSPECIFIED.);",
            10,
            "knot multiplicities do not match the control points",
        ),
        (
            "#10=B_SPLINE_CURVE_WITH_KNOTS('',2,(#1,#1),.UNSPECIFIED.,.F.,.F.,(3,3),(0.,1.),.UNSPECIFIED.);",
            10,
            "knot multiplicities do not match the control points",
        ),
        (
            "#10=B_SPLINE_CURVE_WITH_KNOTS('',99,(#1,#1),.UNSPECIFIED.,.F.,.F.,(1,1),(0.,1.),.UNSPECIFIED.);",
            10,
            "bad B-spline degree",
        ),
        (
            "#10=B_SPLINE_CURVE_WITH_KNOTS('',1,(#1,#1),.UNSPECIFIED.,.F.,.F.,(4000000000000,1),(0.,1.),.UNSPECIFIED.);",
            10,
            "bad knot multiplicity",
        ),
        (
            "#10=B_SPLINE_CURVE_WITH_KNOTS('',1,(#1,#1),.UNSPECIFIED.,.F.,.F.,(2,2),(1.,0.),.UNSPECIFIED.);",
            10,
            "knots must not decrease, one multiplicity each",
        ),
        (
            "#10=(B_SPLINE_CURVE(1,(#1,#1),.UNSPECIFIED.,.F.,.F.)B_SPLINE_CURVE_WITH_KNOTS((2,2),(0.,1.),.UNSPECIFIED.)RATIONAL_B_SPLINE_CURVE((1.,-1.)));",
            10,
            "non-positive weight",
        ),
        (
            "#10=(B_SPLINE_CURVE(1,(#1,#1),.UNSPECIFIED.,.F.,.F.)B_SPLINE_CURVE_WITH_KNOTS((2,2),(0.,1.),.UNSPECIFIED.)RATIONAL_B_SPLINE_CURVE((1.)));",
            10,
            "weights do not match the control points",
        ),
        // Shapes.
        ("#10=POLYLINE('',(#1));", 10, "polyline point count"),
        (
            "#10=TRIMMED_CURVE('',#4,(),(PARAMETER_VALUE(1.)),.T.,.PARAMETER.);",
            10,
            "not supported yet: curve AXIS2_PLACEMENT_3D",
        ),
        (
            "#10=CIRCLE('',#4,1.);#11=TRIMMED_CURVE('',#10,(),(PARAMETER_VALUE(1.)),.T.,.PARAMETER.);",
            11,
            "trimmed curve trim count",
        ),
        (
            "#10=UNKNOWN_CURVE('');",
            10,
            "not supported yet: curve UNKNOWN_CURVE",
        ),
        (
            "#10=(BOUNDED_CURVE()CURVE()GEOMETRIC_REPRESENTATION_ITEM()REPRESENTATION_ITEM(''));",
            10,
            "complex entity without B_SPLINE_CURVE",
        ),
    ] {
        assert_eq!(curve_err(data, id, MM), Err(why.to_string()), "{data}");
    }
}

// Surfaces.

#[test]
// The values the file writes, which are near π/4 and 2π, not those constants.
#[allow(clippy::approx_constant)]
fn analytic_surfaces() {
    assert_eq!(
        *surface("#10=PLANE('',#4);", 10, MM),
        Surface::Plane { position: WORLD }
    );
    assert_eq!(
        *surface("#10=CYLINDRICAL_SURFACE('',#4,5.);", 10, MM),
        Surface::Cylinder {
            position: WORLD,
            radius: 5.0
        }
    );
    assert_eq!(
        *surface("#10=CONICAL_SURFACE('',#4,5.,0.785398163397448);", 10, MM),
        Surface::Cone {
            position: WORLD,
            radius: 5.0,
            semi_angle: 0.785398163397448
        }
    );
    // A cone may start at its apex (radius 0).
    assert!(matches!(
        *surface("#10=CONICAL_SURFACE('',#4,0.,0.5);", 10, MM),
        Surface::Cone { radius, .. } if radius == 0.0
    ));
    assert_eq!(
        *surface("#10=SPHERICAL_SURFACE('',#4,3.);", 10, MM),
        Surface::Sphere {
            position: WORLD,
            radius: 3.0
        }
    );
    assert_eq!(
        *surface("#10=TOROIDAL_SURFACE('',#4,10.,2.);", 10, MM),
        Surface::Torus {
            position: WORLD,
            major_radius: 10.0,
            minor_radius: 2.0
        }
    );
    // A negative major radius (a spindle torus some writers write) is read as written.
    assert!(matches!(
        *surface("#10=TOROIDAL_SURFACE('',#4,-1.,2.);", 10, MM),
        Surface::Torus { major_radius, .. } if major_radius == -1.0
    ));
    assert_eq!(
        *surface("#10=DEGENERATE_TOROIDAL_SURFACE('',#4,1.,2.,.T.);", 10, MM),
        Surface::DegenerateTorus {
            position: WORLD,
            major_radius: 1.0,
            minor_radius: 2.0,
            select_outer: true
        }
    );
}

#[test]
fn units_scale_surface_lengths_and_angles() {
    let k = surface("#10=CONICAL_SURFACE('',#4,1.,45.);", 10, INCH_DEG);
    assert!(matches!(*k, Surface::Cone { radius, semi_angle, .. }
        if radius == 25.4 && semi_angle == 45.0 * DEG));
    let t = surface("#10=TOROIDAL_SURFACE('',#4,4.,1.);", 10, INCH_DEG);
    assert!(
        matches!(*t, Surface::Torus { major_radius, minor_radius, .. }
        if major_radius == 4.0 * 25.4 && minor_radius == 25.4)
    );
}

#[test]
fn bspline_surfaces() {
    // A bilinear patch over the unit square, then the same as a rational complex entity.
    let pts = "#10=CARTESIAN_POINT('',(0.,0.,0.));#11=CARTESIAN_POINT('',(0.,1.,0.));\
               #12=CARTESIAN_POINT('',(1.,0.,0.));#13=CARTESIAN_POINT('',(1.,1.,1.));";
    let net = vec![
        vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        vec![[1.0, 0.0, 0.0], [1.0, 1.0, 1.0]],
    ];
    let s = surface(
        &format!(
            "{pts}#14=B_SPLINE_SURFACE_WITH_KNOTS('',1,1,((#10,#11),(#12,#13)),.PLANE_SURF.,.F.,.T.,\
             .F.,(2,2),(2,2),(0.,1.),(0.,2.),.UNSPECIFIED.);"
        ),
        14,
        MM,
    );
    let Surface::BSpline(b) = &*s else {
        panic!("{s:?}")
    };
    assert_eq!((b.u_degree, b.v_degree), (1, 1));
    assert_eq!(b.points, net);
    assert_eq!(b.weights, None);
    assert_eq!(
        (b.u_knots.clone(), b.u_multiplicities.clone()),
        (vec![0.0, 1.0], vec![2, 2])
    );
    assert_eq!(
        (b.v_knots.clone(), b.v_multiplicities.clone()),
        (vec![0.0, 2.0], vec![2, 2])
    );
    assert_eq!(b.form, SurfaceForm::PlaneSurf);
    assert_eq!(
        (b.u_closed, b.v_closed, b.self_intersect),
        (Logical::False, Logical::True, Logical::False)
    );
    let r = surface(
        &format!(
            "{pts}#14=(BOUNDED_SURFACE()B_SPLINE_SURFACE(1,1,((#10,#11),(#12,#13)),.UNSPECIFIED.,.F.,.F.,.F.)\
             B_SPLINE_SURFACE_WITH_KNOTS((2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.)GEOMETRIC_REPRESENTATION_ITEM()\
             RATIONAL_B_SPLINE_SURFACE(((1.,2.),(3.,4.)))REPRESENTATION_ITEM('')SURFACE());"
        ),
        14,
        MM,
    );
    let Surface::BSpline(b) = &*r else {
        panic!("{r:?}")
    };
    assert_eq!(b.points, net);
    assert_eq!(b.weights, Some(vec![vec![1.0, 2.0], vec![3.0, 4.0]]));
    // Implied knots in both directions.
    let q = surface(
        &format!(
            "{pts}#14=BEZIER_SURFACE('',1,1,((#10,#11),(#12,#13)),.UNSPECIFIED.,.F.,.F.,.F.);"
        ),
        14,
        MM,
    );
    let Surface::BSpline(b) = &*q else {
        panic!("{q:?}")
    };
    assert_eq!(b.knot_spec, KnotSpec::PiecewiseBezierKnots);
    assert_eq!(
        (b.u_knots.clone(), b.u_multiplicities.clone()),
        (vec![0.0, 1.0], vec![2, 2])
    );
    assert_eq!(
        (b.v_knots.clone(), b.v_multiplicities.clone()),
        (vec![0.0, 1.0], vec![2, 2])
    );
}

#[test]
fn swept_surfaces() {
    // A circle of radius 2 about the Z axis, centred at x = 10.
    let rev = surface(
        "#10=CARTESIAN_POINT('',(10.,0.,0.));#11=AXIS2_PLACEMENT_3D('',#10,#5,#3);#12=CIRCLE('',#11,2.);\
         #13=AXIS1_PLACEMENT('',#1,#2);#14=SURFACE_OF_REVOLUTION('',#12,#13);",
        14,
        MM,
    );
    let Surface::Revolution { profile, axis } = &*rev else {
        panic!("{rev:?}")
    };
    assert_eq!(
        **profile,
        Curve::Circle {
            position: Placement {
                origin: [10.0, 0.0, 0.0],
                axis: Some([0.0, 1.0, 0.0]),
                ref_direction: Some([1.0, 0.0, 0.0]),
            },
            radius: 2.0,
        }
    );
    assert_eq!(
        *axis,
        Axis1 {
            origin: [0.0; 3],
            direction: Some([0.0, 0.0, 1.0])
        }
    );
    let ext = surface(
        "#10=CIRCLE('',#4,3.);#11=VECTOR('',#2,4.);#12=SURFACE_OF_LINEAR_EXTRUSION('',#10,#11);",
        12,
        INCH_DEG,
    );
    let Surface::Extrusion { profile, vector } = &*ext else {
        panic!("{ext:?}")
    };
    assert!(matches!(**profile, Curve::Circle { radius, .. } if radius == 3.0 * 25.4));
    assert_eq!(
        *vector,
        Vector {
            orientation: [0.0, 0.0, 1.0],
            magnitude: 4.0,
            scale: [25.4; 3]
        }
    );
}

#[test]
fn offset_and_trimmed_surfaces() {
    let o = surface(
        "#10=CYLINDRICAL_SURFACE('',#4,5.);#11=OFFSET_SURFACE('',#10,1.5,.F.);",
        11,
        MM,
    );
    let Surface::Offset {
        basis,
        distance,
        self_intersect,
    } = &*o
    else {
        panic!("{o:?}")
    };
    assert!(matches!(**basis, Surface::Cylinder { radius, .. } if radius == 5.0));
    assert_eq!((*distance, *self_intersect), (1.5, Logical::False));
    // A cylinder's u is an angle and its v a length.
    let t = surface(
        "#10=CYLINDRICAL_SURFACE('',#4,5.);#11=RECTANGULAR_TRIMMED_SURFACE('',#10,0.,90.,0.,10.,.T.,.F.);",
        11,
        INCH_DEG,
    );
    let Surface::RectangularTrimmed {
        u1,
        u2,
        v1,
        v2,
        u_sense,
        v_sense,
        ..
    } = *t
    else {
        panic!("{t:?}")
    };
    assert_eq!((u1, u2, v1, v2), (0.0, 90.0 * DEG, 0.0, 10.0 * 25.4));
    assert_eq!((u_sense, v_sense), (true, false));
    // A B-spline's parameters are plain numbers.
    let t = surface(
        "#10=CARTESIAN_POINT('',(1.,1.,0.));#11=B_SPLINE_SURFACE_WITH_KNOTS('',1,1,((#1,#1),(#10,#10)),\
         .UNSPECIFIED.,.F.,.F.,.F.,(2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.);\
         #12=RECTANGULAR_TRIMMED_SURFACE('',#11,0.,0.5,0.25,1.,.T.,.T.);",
        12,
        INCH_DEG,
    );
    assert!(matches!(*t, Surface::RectangularTrimmed { u2, v1, .. } if u2 == 0.5 && v1 == 0.25));
}

/// A cylinder `#10` of radius 5 with a seam: the line x = 5, with pcurves u = 0 and u = 2π.
const SEAM: &str = "#10=CYLINDRICAL_SURFACE('',#4,5.);#11=CARTESIAN_POINT('',(5.,0.,0.));\
#12=VECTOR('',#2,1.);#13=LINE('',#11,#12);#20=CARTESIAN_POINT('',(0.,0.));#21=DIRECTION('',(0.,1.));\
#22=VECTOR('',#21,1.);#23=LINE('',#20,#22);\
#24=(GEOMETRIC_REPRESENTATION_CONTEXT(2)PARAMETRIC_REPRESENTATION_CONTEXT()REPRESENTATION_CONTEXT('2D SPACE',''));\
#25=DEFINITIONAL_REPRESENTATION('',(#23),#24);#26=PCURVE('',#10,#25);\
#27=CARTESIAN_POINT('',(6.28318530717959,0.));#28=LINE('',#27,#22);\
#29=DEFINITIONAL_REPRESENTATION('',(#28),#24);#30=PCURVE('',#10,#29);\
#31=SEAM_CURVE('',#13,(#26,#30),.PCURVE_S1.);";

#[test]
// The values the file writes, which are near π/4 and 2π, not those constants.
#[allow(clippy::approx_constant)]
fn surface_curve_with_pcurves_shares_its_surface() {
    let c = curve(SEAM, 31, MM);
    let Curve::SurfaceCurve {
        kind,
        associated,
        master,
        curve_3d,
    } = &*c
    else {
        panic!("{c:?}")
    };
    assert_eq!(
        (*kind, *master),
        (SurfaceCurveKind::Seam, PreferredRep::PcurveS1)
    );
    assert!(matches!(**curve_3d, Curve::Line { origin, .. } if origin == [5.0, 0.0, 0.0]));
    let [Associated::Pcurve(a), Associated::Pcurve(b)] = associated.as_slice() else {
        panic!("{associated:?}")
    };
    assert!(
        Arc::ptr_eq(&a.surface, &b.surface),
        "one surface entity, read once"
    );
    let up = Vector {
        orientation: [0.0, 1.0, 0.0],
        magnitude: 1.0,
        scale: [1.0; 3],
    };
    assert_eq!(
        *a.curve,
        Curve::Line {
            origin: [0.0, 0.0, 0.0],
            vector: up
        }
    );
    assert_eq!(
        *b.curve,
        Curve::Line {
            origin: [6.28318530717959, 0.0, 0.0],
            vector: up
        }
    );
    // A PCURVE read on its own is the same data, sharing the surface read before.
    let ex = file(&format!("{BASE}{SEAM}"));
    let mut r = Reader::new(&ex, MM);
    let seam = r.curve(31).unwrap();
    let p = r.pcurve(26).unwrap();
    let Curve::SurfaceCurve { associated, .. } = &*seam else {
        panic!("{seam:?}")
    };
    assert_eq!(associated.first(), Some(&Associated::Pcurve(p.clone())));
    assert!(
        matches!(associated.first(), Some(Associated::Pcurve(q)) if Arc::ptr_eq(&q.surface, &p.surface))
    );
    assert_eq!(r.pcurve(13), Err("#13: expected PCURVE, found LINE".into()));
}

#[test]
fn pcurves_scale_with_the_surface_parameters() {
    // Inches and degrees: a cylinder pcurve's u is an angle, v a length.
    let c = curve(
        "#10=CYLINDRICAL_SURFACE('',#4,1.);#11=CARTESIAN_POINT('',(1.,0.,0.));#12=VECTOR('',#2,1.);#13=LINE('',#11,#12);\
         #20=CARTESIAN_POINT('',(90.,0.));#21=DIRECTION('',(0.,1.));#22=VECTOR('',#21,1.);#23=LINE('',#20,#22);\
         #24=(GEOMETRIC_REPRESENTATION_CONTEXT(2)PARAMETRIC_REPRESENTATION_CONTEXT()REPRESENTATION_CONTEXT('2D SPACE',''));\
         #25=DEFINITIONAL_REPRESENTATION('',(#23),#24);#26=PCURVE('',#10,#25);#27=SURFACE_CURVE('',#13,(#26),.CURVE_3D.);",
        27,
        INCH_DEG,
    );
    let Curve::SurfaceCurve {
        kind,
        associated,
        master,
        ..
    } = &*c
    else {
        panic!("{c:?}")
    };
    assert_eq!(
        (*kind, *master),
        (SurfaceCurveKind::Surface, PreferredRep::Curve3d)
    );
    let Some(Associated::Pcurve(pc)) = associated.first() else {
        panic!("{associated:?}")
    };
    assert!(matches!(*pc.surface, Surface::Cylinder { radius, .. } if radius == 25.4));
    assert_eq!(
        *pc.curve,
        Curve::Line {
            origin: [90.0 * DEG, 0.0 * 25.4, 0.0],
            vector: Vector {
                orientation: [0.0, 1.0, 0.0],
                magnitude: 1.0,
                scale: [DEG, 25.4, 1.0],
            },
        }
    );
}

/// A pcurve `#26` on `surface` (`#10`) whose curve is `#23`, defined in `curve`.
fn pcurve_on(surface: &str, curve: &str) -> String {
    format!(
        "{surface}{curve}\
         #24=(GEOMETRIC_REPRESENTATION_CONTEXT(2)PARAMETRIC_REPRESENTATION_CONTEXT()REPRESENTATION_CONTEXT('2D SPACE',''));\
         #25=DEFINITIONAL_REPRESENTATION('',(#23),#24);#26=PCURVE('',#10,#25);"
    )
}

#[test]
fn conics_in_parameter_space() {
    // A circle in a plane's parameter space: both parameters lengths, so the radius is too, and
    // a 2D placement's direction ratios take the parameters' units.
    let circle = "#20=CARTESIAN_POINT('',(1.,2.));#21=DIRECTION('',(1.,1.));\
                  #22=AXIS2_PLACEMENT_2D('',#20,#21);#23=CIRCLE('',#22,3.);";
    let ex = file(&format!("{BASE}{}", pcurve_on("#10=PLANE('',#4);", circle)));
    let p = Reader::new(&ex, INCH_DEG).pcurve(26).unwrap();
    assert_eq!(
        *p.curve,
        Curve::Circle {
            position: Placement {
                origin: [25.4, 2.0 * 25.4, 0.0],
                axis: None,
                ref_direction: Some([25.4, 25.4, 0.0]),
            },
            radius: 3.0 * 25.4,
        }
    );
    // On a cylinder in degrees, u is an angle and v a length: not a conic in mm and rad.
    let ex = file(&format!(
        "{BASE}{}",
        pcurve_on("#10=CYLINDRICAL_SURFACE('',#4,5.);", circle)
    ));
    assert_eq!(
        Reader::new(&ex, INCH_DEG).pcurve(26),
        Err("not supported yet: a conic in a parameter space with mixed units".into())
    );
    // In millimetres and radians both factors are 1.
    assert!(matches!(*Reader::new(&ex, MM).pcurve(26).unwrap().curve,
        Curve::Circle { radius, .. } if radius == 3.0));
    // A 3D-only curve in parameter space.
    let ex = file(&format!(
        "{BASE}{}",
        pcurve_on(
            "#10=PLANE('',#4);",
            "#20=CIRCLE('',#4,1.);#23=OFFSET_CURVE_3D('',#20,1.,.F.,#2);"
        )
    ));
    assert_eq!(
        Reader::new(&ex, MM).pcurve(26),
        Err("not supported yet: curve OFFSET_CURVE_3D in parameter space".into())
    );
}

#[test]
fn nesting_in_parameter_space_is_bounded() {
    // A 2D trimmed curve that is its own basis: refused at the depth bound (was a recursion
    // without bound: the bound was checked only where the 2D curve was entered).
    let ex = file(&format!(
        "{BASE}{}",
        pcurve_on(
            "#10=PLANE('',#4);",
            "#23=TRIMMED_CURVE('',#23,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(1.)),.T.,.PARAMETER.);"
        )
    ));
    assert_eq!(
        Reader::new(&ex, MM).pcurve(26),
        Err("curve definitions nested too deeply".into())
    );
    // A surface that is its own basis.
    assert_eq!(
        surface_err("#10=OFFSET_SURFACE('',#10,1.,.F.);", 10, MM),
        Err("surface definitions nested too deeply".into())
    );
}

#[test]
fn surface_refusals_say_why() {
    for (data, why) in [
        (
            "#10=CYLINDRICAL_SURFACE('',#4,0.);",
            "cylinder radius must be positive",
        ),
        (
            "#10=SPHERICAL_SURFACE('',#4,-3.);",
            "sphere radius must be positive",
        ),
        (
            "#10=CONICAL_SURFACE('',#4,1.,2.);",
            "cone with a negative radius or a semi angle outside (0, 90°)",
        ),
        (
            "#10=CONICAL_SURFACE('',#4,-1.,0.5);",
            "cone with a negative radius or a semi angle outside (0, 90°)",
        ),
        (
            "#10=CONICAL_SURFACE('',#4,1.,0.);",
            "cone with a negative radius or a semi angle outside (0, 90°)",
        ),
        (
            "#10=TOROIDAL_SURFACE('',#4,1.,-2.);",
            "torus minor radius must be positive",
        ),
        (
            "#10=B_SPLINE_SURFACE_WITH_KNOTS('',1,1,((#1,#1),(#1)),.UNSPECIFIED.,.F.,.F.,.F.,(2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.);",
            "ragged B-spline control net",
        ),
        (
            "#10=B_SPLINE_SURFACE_WITH_KNOTS('',1,1,((#1,#1),(#1,#1)),.UNSPECIFIED.,.F.,.F.,.F.,(2,2),(2,1),(0.,1.),(0.,1.),.UNSPECIFIED.);",
            "knot multiplicities do not match the control points",
        ),
        (
            "#10=(B_SPLINE_SURFACE(1,1,((#1,#1),(#1,#1)),.UNSPECIFIED.,.F.,.F.,.F.)B_SPLINE_SURFACE_WITH_KNOTS((2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.)RATIONAL_B_SPLINE_SURFACE(((1.,1.))));",
            "weights do not match the control net",
        ),
        (
            "#10=SURFACE_OF_REVOLUTION('',#10,#4);",
            "not supported yet: curve SURFACE_OF_REVOLUTION",
        ),
        (
            "#10=PLANE('',#3);",
            "placement location: expected a reference",
        ),
        ("#10=PLANE('',#99);", "missing entity #99"),
        (
            "#10=CURVE_BOUNDED_SURFACE('',#4,(),.F.);",
            "not supported yet: surface CURVE_BOUNDED_SURFACE",
        ),
    ] {
        assert_eq!(
            surface_err(data, 10, MM).map(|_| ()),
            Err(why.to_string()),
            "{data}"
        );
    }
    // A revolution whose axis is the wrong kind of placement.
    assert_eq!(
        surface_err(
            "#10=CIRCLE('',#4,1.);#11=SURFACE_OF_REVOLUTION('',#10,#4);",
            11,
            MM
        ),
        Err("#4: expected AXIS1_PLACEMENT, found AXIS2_PLACEMENT_3D".into())
    );
}
