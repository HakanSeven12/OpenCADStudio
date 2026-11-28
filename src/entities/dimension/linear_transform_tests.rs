// Tests for linear_transform.
use super::*;

use std::f64::consts::FRAC_PI_2;

/// A horizontal linear dimension measuring 10 units along X.
fn horizontal() -> Dimension {
    let mut d =
        DimensionLinear::horizontal(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
    d.definition_point = Vector3::new(0.0, 5.0, 0.0);
    Dimension::Linear(d)
}

fn rotation(dim: &Dimension) -> f64 {
    match dim {
        Dimension::Linear(d) => d.rotation,
        _ => unreachable!("linear dimension expected"),
    }
}

/// The stored and the live value both still read 10.
fn assert_measures_ten(dim: &Dimension) {
    let stored = dim.base().actual_measurement;
    assert!(
        (stored - 10.0).abs() < 1e-9,
        "stored measurement must stay 10, got {stored}"
    );
    let live = dim.measurement();
    assert!(
        (live - 10.0).abs() < 1e-9,
        "measurement must stay 10, got {live}"
    );
}

/// ROTATE in the drawing plane turns the measured axis with the points.
/// Left at 0, a horizontal dimension turned 90° measured its now-vertical
/// points along X and dropped to 0.
#[test]
fn rotate_turns_the_measured_axis() {
    let mut dim = horizontal();
    apply_transform(
        &mut dim,
        &EntityTransform::Rotate {
            center: DVec3::ZERO,
            axis: DVec3::Z,
            angle_rad: FRAC_PI_2,
        },
    );
    assert_measures_ten(&dim);
    let turned = rotation(&dim);
    assert!(
        (turned - FRAC_PI_2).abs() < 1e-9,
        "rotation must follow the rotate, got {turned}"
    );
}

/// MIRROR in the drawing plane reflects the measured axis. Mirrored about
/// y = x, the horizontal dimension becomes a vertical one of the same value.
#[test]
fn mirror_reflects_the_measured_axis() {
    let mut dim = horizontal();
    apply_transform(
        &mut dim,
        &EntityTransform::Mirror {
            p1: DVec3::ZERO,
            p2: DVec3::new(1.0, 1.0, 0.0),
            working_normal: DVec3::Z,
        },
    );
    assert_measures_ten(&dim);
    let reflected = rotation(&dim);
    assert!(
        (reflected - FRAC_PI_2).abs() < 1e-9,
        "rotation must be reflected, got {reflected}"
    );
}

/// A zero-length mirror line leaves the points alone, so the axis too.
#[test]
fn degenerate_mirror_keeps_the_measured_axis() {
    let mut dim = horizontal();
    apply_transform(
        &mut dim,
        &EntityTransform::Mirror {
            p1: DVec3::new(3.0, 3.0, 0.0),
            p2: DVec3::new(3.0, 3.0, 0.0),
            working_normal: DVec3::Z,
        },
    );
    assert_eq!(rotation(&dim), 0.0);
    assert_measures_ten(&dim);
}

/// The point the dimension line passes through.
fn definition_point(dim: &Dimension) -> DVec3 {
    let p = dimension_definition_point(dim);
    DVec3::new(p.x, p.y, p.z)
}

fn assert_near(actual: DVec3, expected: DVec3, what: &str) {
    assert!(
        actual.abs_diff_eq(expected, 1e-9),
        "{what}: expected {expected:?}, got {actual:?}"
    );
}

/// ROTATE about an axis off world Z (a working plane other than world XY)
/// turns the dimension like the geometry it measures. It used to only move
/// by where the origin went, leaving the dimension line and plane unturned.
#[test]
fn rotate_off_world_z_turns_the_dimension() {
    let center = DVec3::new(0.0, 10.0, 0.0);
    let turn = glam::DQuat::from_axis_angle(DVec3::X, FRAC_PI_2);
    let mut dim = horizontal();
    apply_transform(
        &mut dim,
        &EntityTransform::Rotate {
            center,
            axis: DVec3::X,
            angle_rad: FRAC_PI_2,
        },
    );
    assert_near(
        definition_point(&dim),
        turn * (DVec3::new(0.0, 5.0, 0.0) - center) + center,
        "definition point",
    );
    let normal = dim.base().normal;
    assert_near(
        DVec3::new(normal.x, normal.y, normal.z),
        turn * DVec3::Z,
        "normal",
    );
    assert_measures_ten(&dim);
}

/// MIRROR in a working plane off world Z reflects the dimension through
/// the mirror plane instead of only moving it.
#[test]
fn mirror_off_world_z_reflects_the_dimension() {
    // Working plane YZ; the line runs along Z at y = 2, so the mirror
    // plane is y = 2.
    let mut dim = horizontal();
    apply_transform(
        &mut dim,
        &EntityTransform::Mirror {
            p1: DVec3::new(0.0, 2.0, 0.0),
            p2: DVec3::new(0.0, 2.0, 1.0),
            working_normal: DVec3::X,
        },
    );
    assert_near(
        definition_point(&dim),
        DVec3::new(0.0, -1.0, 0.0),
        "definition point",
    );
    assert_measures_ten(&dim);
}
