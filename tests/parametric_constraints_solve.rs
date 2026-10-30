use codec::entities::EntityType;
use codec::types::{Handle, Vector3};
use OpenCADStudio::scene::named_parameters::DrivingValue;
use OpenCADStudio::scene::parametric_constraints::{
    ConstraintKind, ParametricRef, ParametricScope,
};
use OpenCADStudio::scene::{ChangeKind, Scene};

fn add_line(scene: &mut Scene, x1: f64, y1: f64, x2: f64, y2: f64) -> Handle {
    scene.add_entity(EntityType::Line(codec::entities::Line::from_points(
        Vector3::new(x1, y1, 0.0),
        Vector3::new(x2, y2, 0.0),
    )))
}

fn line_endpoints(scene: &Scene, handle: Handle) -> (Vector3, Vector3) {
    match scene.document.get_entity(handle).expect("entity exists") {
        EntityType::Line(l) => (l.start, l.end),
        other => panic!("expected a Line, got {other:?}"),
    }
}

fn set_line_end(scene: &mut Scene, handle: Handle, end: Vector3) {
    if let Some(EntityType::Line(l)) = scene.document.get_entity_mut(handle) {
        l.end = end;
    }
}

fn add_circle(scene: &mut Scene, cx: f64, cy: f64, radius: f64) -> Handle {
    scene.add_entity(EntityType::Circle(
        codec::entities::Circle::from_center_radius(Vector3::new(cx, cy, 0.0), radius),
    ))
}

fn circle_geom(scene: &Scene, handle: Handle) -> (Vector3, f64) {
    match scene.document.get_entity(handle).expect("entity exists") {
        EntityType::Circle(c) => (c.center, c.radius),
        other => panic!("expected a Circle, got {other:?}"),
    }
}

/// A 90-degree left-turn corner chain bonded exactly the way the civil
/// plugin's BOND bonds it: per joint a Coincident between the two element
/// endpoints plus a whole-curve Tangent (no tangent-point pick), and the PI
/// marker circles pinned with Coincident (marker center <-> element
/// endpoint). No radius driver, no Fixed on the implied corner PI: it stays
/// refresh-positioned.
///
/// Geometry: line 1 runs in +X from (-45, 0) to the tangency point
/// (-15, 0); the arc (R = 15, center (-15, 15)) sweeps from (-15, 0) to
/// (0, 15); line 2 runs in +Y from (0, 15) to (0, 45). Tangent length
/// T = R*tan(d/2) = 15 on both legs of the PI at the origin. Returns
/// `(line1, arc, line2, start_marker, end_marker)`.
#[allow(clippy::type_complexity)]
fn add_bonded_chain_graph(scene: &mut Scene) -> (Handle, Handle, Handle, Handle, Handle) {
    let line1 = add_line(scene, -45.0, 0.0, -15.0, 0.0);
    let arc = add_arc(scene, -15.0, 15.0, 15.0, -std::f64::consts::FRAC_PI_2, 0.0);
    let line2 = add_line(scene, 0.0, 15.0, 0.0, 45.0);
    let start_marker = add_circle(scene, -45.0, 0.0, 2.0);
    let end_marker = add_circle(scene, 0.0, 45.0, 2.0);

    let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
    set.add(
        ConstraintKind::Coincident,
        vec![
            ParametricRef::center(start_marker),
            ParametricRef::point(line1, 0),
        ],
        None,
    );
    set.add(
        ConstraintKind::Coincident,
        vec![ParametricRef::point(line1, 1), ParametricRef::point(arc, 0)],
        None,
    );
    set.add(
        ConstraintKind::Tangent,
        vec![ParametricRef::whole(line1), ParametricRef::whole(arc)],
        None,
    );
    set.add(
        ConstraintKind::Coincident,
        vec![ParametricRef::point(arc, 1), ParametricRef::point(line2, 0)],
        None,
    );
    set.add(
        ConstraintKind::Tangent,
        vec![ParametricRef::whole(arc), ParametricRef::whole(line2)],
        None,
    );
    set.add(
        ConstraintKind::Coincident,
        vec![
            ParametricRef::center(end_marker),
            ParametricRef::point(line2, 1),
        ],
        None,
    );
    (line1, arc, line2, start_marker, end_marker)
}

fn add_arc(
    scene: &mut Scene,
    cx: f64,
    cy: f64,
    radius: f64,
    start_angle: f64,
    end_angle: f64,
) -> Handle {
    scene.add_entity(EntityType::Arc(
        codec::entities::Arc::from_center_radius_angles(
            Vector3::new(cx, cy, 0.0),
            radius,
            start_angle,
            end_angle,
        ),
    ))
}

fn arc_geom(scene: &Scene, handle: Handle) -> (Vector3, f64) {
    match scene.document.get_entity(handle).expect("entity exists") {
        EntityType::Arc(a) => (a.center, a.radius),
        other => panic!("expected an Arc, got {other:?}"),
    }
}

fn add_ellipse(scene: &mut Scene, cx: f64, cy: f64, major_axis: (f64, f64), ratio: f64) -> Handle {
    scene.add_entity(EntityType::Ellipse(
        codec::entities::Ellipse::from_center_axes(
            Vector3::new(cx, cy, 0.0),
            Vector3::new(major_axis.0, major_axis.1, 0.0),
            ratio,
        ),
    ))
}

fn ellipse_geom(scene: &Scene, handle: Handle) -> (Vector3, Vector3, f64) {
    match scene.document.get_entity(handle).expect("entity exists") {
        EntityType::Ellipse(e) => (e.center, e.major_axis, e.minor_axis_ratio),
        other => panic!("expected an Ellipse, got {other:?}"),
    }
}

#[test]
fn horizontal_constraint_levels_the_line_when_an_endpoint_moves() {
    let mut scene = Scene::new();
    let line = add_line(&mut scene, 0.0, 0.0, 10.0, 0.0);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(line)],
            None,
        );

    // Drag the endpoint off-axis — a real edit, the kind of thing a Move
    // command or grip-drag commit would do.
    set_line_end(&mut scene, line, Vector3::new(10.0, 4.0, 0.0));
    scene.bump_entities(&[(line, ChangeKind::Modified)]);

    let (start, end) = line_endpoints(&scene, line);
    assert!(
        (start.y - end.y).abs() < 1e-6,
        "line should have been re-leveled: start={start:?} end={end:?}"
    );

    // A line is 4 raw coordinates; Horizontal removes exactly 1 (y1 == y2),
    // leaving 3 — not 1: the two X coordinates are registered but never
    // referenced by any constraint, so they must still count as free
    // (regression guard for the undercounting `solve_scope` bug found while
    // live-testing this stage: `System::partition`'s subsystems only see
    // constraint-referenced params, so an untouched one was invisible to
    // `diagnose` entirely before this was fixed).
    let dof = scene
        .parametric_constraint_set(ParametricScope::ModelSpace)
        .and_then(|s| s.dof);
    assert_eq!(
        dof,
        Some(3),
        "DOF should count the two untouched X coordinates too"
    );
}

#[test]
fn horizontal_constraint_levels_a_polyline_segment() {
    let mut scene = Scene::new();
    let polyline = scene.add_entity(EntityType::LwPolyline(
        codec::entities::LwPolyline::from_points(vec![
            codec::types::Vector2::new(0.0, 0.0),
            codec::types::Vector2::new(5.0, 2.0),
            codec::types::Vector2::new(10.0, 7.0),
        ]),
    ));
    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::segment(polyline, 1)],
            None,
        );

    scene.bump_entities(&[(polyline, ChangeKind::Modified)]);
    let EntityType::LwPolyline(polyline) = scene.document.get_entity(polyline).unwrap() else {
        panic!("expected a lightweight polyline");
    };
    assert!(
        (polyline.vertices[1].location.y - polyline.vertices[2].location.y).abs() < 1e-6,
        "the selected segment should be horizontal"
    );
}

#[test]
fn horizontal_constraint_does_not_flatten_a_polyline_arc_segment() {
    let mut source = codec::entities::LwPolyline::new();
    source.add_point_with_bulge(codec::types::Vector2::new(0.0, 0.0), 0.5);
    source.add_point(codec::types::Vector2::new(5.0, 2.0));
    let mut scene = Scene::new();
    let polyline = scene.add_entity(EntityType::LwPolyline(source));
    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::segment(polyline, 0)],
            None,
        );

    scene.bump_entities(&[(polyline, ChangeKind::Modified)]);
    let EntityType::LwPolyline(polyline) = scene.document.get_entity(polyline).unwrap() else {
        panic!("expected a lightweight polyline");
    };
    assert_eq!(polyline.vertices[0].location.y, 0.0);
    assert_eq!(polyline.vertices[1].location.y, 2.0);
    assert_eq!(polyline.vertices[0].bulge, 0.5);
}

#[test]
fn horizontal_constraint_aligns_two_selected_points() {
    let mut scene = Scene::new();
    let a = add_line(&mut scene, 0.0, 1.0, 4.0, 3.0);
    let b = add_line(&mut scene, 8.0, 7.0, 12.0, 9.0);
    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::point(a, 0), ParametricRef::point(b, 0)],
            None,
        );

    scene.bump_entities(&[(b, ChangeKind::Modified)]);
    let (a_start, _) = line_endpoints(&scene, a);
    let (b_start, _) = line_endpoints(&scene, b);
    assert!((a_start.y - b_start.y).abs() < 1e-6);
}

#[test]
fn parallel_constraint_rotates_the_other_line_when_one_moves() {
    let mut scene = Scene::new();
    let a = add_line(&mut scene, 0.0, 0.0, 10.0, 0.0);
    let b = add_line(&mut scene, 0.0, 5.0, 10.0, 5.0);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Parallel,
            vec![ParametricRef::whole(a), ParametricRef::whole(b)],
            None,
        );

    // Rotate `a`; `b` (parallel to it) should follow.
    set_line_end(&mut scene, a, Vector3::new(10.0, 6.0, 0.0));
    scene.bump_entities(&[(a, ChangeKind::Modified)]);

    let (a1, a2) = line_endpoints(&scene, a);
    let (b1, b2) = line_endpoints(&scene, b);
    let dir_a = ((a2.x - a1.x), (a2.y - a1.y));
    let dir_b = ((b2.x - b1.x), (b2.y - b1.y));
    // Parallel: cross product of directions is ~0.
    let cross = dir_a.0 * dir_b.1 - dir_a.1 * dir_b.0;
    assert!(
        cross.abs() < 1e-6,
        "lines should be parallel after the solve: dir_a={dir_a:?} dir_b={dir_b:?}"
    );
}

#[test]
fn distance_constraint_holds_the_target_length_after_an_unrelated_endpoint_edit() {
    let mut scene = Scene::new();
    let line = add_line(&mut scene, 0.0, 0.0, 10.0, 0.0);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Distance,
            vec![ParametricRef::point(line, 0), ParametricRef::point(line, 1)],
            Some(DrivingValue::Literal(20.0)),
        );

    set_line_end(&mut scene, line, Vector3::new(5.0, 5.0, 0.0)); // arbitrary edit, not length=20
    scene.bump_entities(&[(line, ChangeKind::Modified)]);

    let (start, end) = line_endpoints(&scene, line);
    let len = ((end.x - start.x).powi(2) + (end.y - start.y).powi(2)).sqrt();
    assert!(
        (len - 20.0).abs() < 1e-6,
        "length should have been solved to the driving value, got {len}"
    );
}

/// `named_parameters_design.md` stage 3: a `Distance` constraint's driving
/// value can be a named-parameter reference instead of a literal, resolved
/// through `Scene::named_parameters` at solve time.
#[test]
fn distance_constraint_resolves_a_named_parameter_reference() {
    let mut scene = Scene::new();
    let line = add_line(&mut scene, 0.0, 0.0, 10.0, 0.0);
    scene
        .named_parameters_mut()
        .set("target_len", "20")
        .unwrap();

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Distance,
            vec![ParametricRef::point(line, 0), ParametricRef::point(line, 1)],
            Some(DrivingValue::Named("target_len".to_string())),
        );

    set_line_end(&mut scene, line, Vector3::new(5.0, 5.0, 0.0)); // arbitrary edit, not length=20
    scene.bump_entities(&[(line, ChangeKind::Modified)]);

    let (start, end) = line_endpoints(&scene, line);
    let len = ((end.x - start.x).powi(2) + (end.y - start.y).powi(2)).sqrt();
    assert!(
        (len - 20.0).abs() < 1e-6,
        "length should have resolved the named parameter, got {len}"
    );

    // Editing the parameter itself and touching the scope again should
    // ripple through, same as editing a literal driving value would.
    scene.named_parameters_mut().set("target_len", "8").unwrap();
    scene.bump_entities(&[(line, ChangeKind::Modified)]);
    let (start, end) = line_endpoints(&scene, line);
    let len = ((end.x - start.x).powi(2) + (end.y - start.y).powi(2)).sqrt();
    assert!(
        (len - 8.0).abs() < 1e-6,
        "length should track the redefined parameter value, got {len}"
    );
}

/// A `driving_param` referencing a named parameter that doesn't (or no
/// longer) exists must not panic the solve — same "skip, don't panic"
/// contract `build_constraint`'s doc comment gives every other unbuildable
/// constraint. The referenced line's two endpoint coordinates are still
/// registered (via `point_ref`) even though the constraint itself can't be
/// built, so they're just left free — the edit that triggered this solve is
/// not undone or altered.
#[test]
fn distance_constraint_with_an_undefined_named_reference_is_skipped_not_panicked() {
    let mut scene = Scene::new();
    let line = add_line(&mut scene, 0.0, 0.0, 10.0, 0.0);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Distance,
            vec![ParametricRef::point(line, 0), ParametricRef::point(line, 1)],
            Some(DrivingValue::Named("does_not_exist".to_string())),
        );

    let edited_end = Vector3::new(5.0, 5.0, 0.0);
    set_line_end(&mut scene, line, edited_end);
    scene.bump_entities(&[(line, ChangeKind::Modified)]); // must not panic

    let (_, end) = line_endpoints(&scene, line);
    assert_eq!(end, edited_end, "an unresolvable driving reference must leave the edit alone, not move it toward a phantom target");
}

#[test]
fn tangent_constraint_between_a_line_and_a_circle_solves_to_touching() {
    let mut scene = Scene::new();
    let circle = add_circle(&mut scene, 0.0, 0.0, 5.0);
    // A horizontal line well clear of the circle (distance 10, radius 5) —
    // deliberately not tangent yet.
    let line = add_line(&mut scene, -10.0, 10.0, 10.0, 10.0);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Tangent,
            vec![ParametricRef::whole(circle), ParametricRef::whole(line)],
            None,
        );
    scene.bump_entities(&[(circle, ChangeKind::Modified), (line, ChangeKind::Modified)]);

    let (center, radius) = circle_geom(&scene, circle);
    let (p1, p2) = line_endpoints(&scene, line);
    let line_dir = (p2.x - p1.x, p2.y - p1.y);
    let line_len = (line_dir.0 * line_dir.0 + line_dir.1 * line_dir.1).sqrt();
    // Signed distance from the circle's center to the (infinite) line.
    let signed_dist = (line_dir.0 * (center.y - p1.y) - line_dir.1 * (center.x - p1.x)) / line_len;
    assert!((signed_dist.abs() - radius).abs() < 1e-6, "center-to-line distance should equal the radius after solving: dist={signed_dist} radius={radius}");
}

#[test]
fn tangent_constraint_between_two_circles_solves_to_external_tangency() {
    let mut scene = Scene::new();
    let a = add_circle(&mut scene, 0.0, 0.0, 3.0);
    let b = add_circle(&mut scene, 20.0, 0.0, 2.0); // far apart: distance 20, r1+r2 = 5

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Tangent,
            vec![ParametricRef::whole(a), ParametricRef::whole(b)],
            None,
        );
    scene.bump_entities(&[(a, ChangeKind::Modified), (b, ChangeKind::Modified)]);

    let (ca, ra) = circle_geom(&scene, a);
    let (cb, rb) = circle_geom(&scene, b);
    let center_dist = ((ca.x - cb.x).powi(2) + (ca.y - cb.y).powi(2)).sqrt();
    assert!(
        (center_dist - (ra + rb)).abs() < 1e-6,
        "circles should sit exactly (r1+r2) apart after solving: dist={center_dist} r1+r2={}",
        ra + rb
    );
}

#[test]
fn coincident_constraint_pulls_the_second_point_onto_the_first_when_it_moves() {
    let mut scene = Scene::new();
    let a = add_line(&mut scene, 0.0, 0.0, 5.0, 0.0);
    let b = add_line(&mut scene, 5.0, 0.0, 5.0, 5.0); // b.start coincident with a.end

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Coincident,
            vec![ParametricRef::point(a, 1), ParametricRef::point(b, 0)],
            None,
        );

    // Move a's end away — b's start should follow to stay coincident.
    set_line_end(&mut scene, a, Vector3::new(8.0, 3.0, 0.0));
    scene.bump_entities(&[(a, ChangeKind::Modified)]);

    let (_, a_end) = line_endpoints(&scene, a);
    let (b_start, _) = line_endpoints(&scene, b);
    assert!(
        (a_end.x - b_start.x).abs() < 1e-6 && (a_end.y - b_start.y).abs() < 1e-6,
        "a.end={a_end:?} b.start={b_start:?}"
    );
}

#[test]
fn unrelated_entity_edit_does_not_touch_constrained_geometry() {
    let mut scene = Scene::new();
    let line = add_line(&mut scene, 0.0, 0.0, 10.0, 3.0); // deliberately not horizontal
    let unrelated = add_line(&mut scene, 100.0, 100.0, 200.0, 100.0);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(line)],
            None,
        );

    let (start_before, end_before) = line_endpoints(&scene, line);
    set_line_end(&mut scene, unrelated, Vector3::new(200.0, 150.0, 0.0));
    scene.bump_entities(&[(unrelated, ChangeKind::Modified)]);

    let (start_after, end_after) = line_endpoints(&scene, line);
    assert_eq!(
        start_before, start_after,
        "unrelated edit must not trigger a re-solve of unrelated geometry"
    );
    assert_eq!(end_before, end_after);
}

#[test]
fn erasing_a_constrained_entity_drops_its_constraints_instead_of_dangling() {
    // Deleting an entity drops its constraints instead of leaving dangling refs.
    let mut scene = Scene::new();
    let a = add_line(&mut scene, 0.0, 0.0, 10.0, 0.0);
    let b = add_line(&mut scene, 0.0, 5.0, 10.0, 5.0);
    let c = add_line(&mut scene, 20.0, 20.0, 30.0, 20.0); // unrelated, own constraint

    let parallel_id = scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Parallel,
            vec![ParametricRef::whole(a), ParametricRef::whole(b)],
            None,
        );
    let horizontal_id = scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(c)],
            None,
        );

    scene.erase_entities(&[a]);

    let set = scene
        .parametric_constraint_set(ParametricScope::ModelSpace)
        .expect("scope still exists");
    assert!(
        set.get(parallel_id).is_none(),
        "the Parallel constraint referencing the erased line must be gone"
    );
    assert!(
        set.get(horizontal_id).is_some(),
        "an unrelated constraint on a different entity must survive"
    );

    // And the surviving constraint set still functions normally afterward.
    set_line_end(&mut scene, c, Vector3::new(30.0, 25.0, 0.0));
    scene.bump_entities(&[(c, ChangeKind::Modified)]);
    let (start, end) = line_endpoints(&scene, c);
    assert!(
        (start.y - end.y).abs() < 1e-6,
        "remaining Horizontal constraint should still solve: start={start:?} end={end:?}"
    );
}

#[test]
fn copying_two_constrained_entities_carries_their_constraint_along() {
    // A constraint entirely between copied entities follows the copy.
    let mut scene = Scene::new();
    let a = add_line(&mut scene, 0.0, 0.0, 10.0, 0.0);
    let b = add_line(&mut scene, 0.0, 5.0, 10.0, 3.0); // deliberately not parallel yet
    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Parallel,
            vec![ParametricRef::whole(a), ParametricRef::whole(b)],
            None,
        );
    // Solve once so the source pair is actually parallel before copying.
    scene.bump_entities(&[(a, ChangeKind::Modified), (b, ChangeKind::Modified)]);
    let constraints_before = scene
        .parametric_constraint_set(ParametricScope::ModelSpace)
        .unwrap()
        .constraints
        .len();

    let transform =
        OpenCADStudio::command::EntityTransform::Translate(glam::DVec3::new(100.0, 100.0, 0.0));
    let new_handles = scene.copy_entities(&[a, b], &transform);
    assert_eq!(new_handles.len(), 2);
    let (new_a, new_b) = (new_handles[0], new_handles[1]);

    let constraints_after = scene
        .parametric_constraint_set(ParametricScope::ModelSpace)
        .unwrap()
        .constraints
        .len();
    assert_eq!(
        constraints_after,
        constraints_before + 1,
        "the copy should have gained exactly one new Parallel constraint"
    );

    // Break the copy's parallelism, then confirm its own (copied) constraint
    // re-solves it — proof this is a real, independent constraint on the
    // copy, not just coincidentally-parallel geometry from the translate.
    set_line_end(&mut scene, new_b, Vector3::new(110.0, 108.0, 0.0));
    scene.bump_entities(&[(new_b, ChangeKind::Modified)]);
    let (a1, a2) = line_endpoints(&scene, new_a);
    let (b1, b2) = line_endpoints(&scene, new_b);
    let dir_a = (a2.x - a1.x, a2.y - a1.y);
    let dir_b = (b2.x - b1.x, b2.y - b1.y);
    let cross = dir_a.0 * dir_b.1 - dir_a.1 * dir_b.0;
    assert!(cross.abs() < 1e-6, "the copied pair should still be held parallel by its own constraint: dir_a={dir_a:?} dir_b={dir_b:?}");

    // The original pair's own constraint must be untouched (still its own
    // record, independently referencing the original handles).
    let (oa1, oa2) = line_endpoints(&scene, a);
    let (ob1, ob2) = line_endpoints(&scene, b);
    let odir_a = (oa2.x - oa1.x, oa2.y - oa1.y);
    let odir_b = (ob2.x - ob1.x, ob2.y - ob1.y);
    let ocross = odir_a.0 * odir_b.1 - odir_a.1 * odir_b.0;
    assert!(
        ocross.abs() < 1e-6,
        "the original pair should remain parallel too"
    );
}

// The "one edit that ripples through a constraint still records as one undo
// step" case needs `Scene::record_undo_before`, which is `pub(crate)` (an
// integration test crate can't reach it) — covered instead as an internal
// unit test in `src/scene/parametric_solve.rs`.

#[test]
fn a_duplicated_horizontal_constraint_is_reported_as_redundant() {
    // `solve_scope` resolves the redundant row back to the
    // `ConstraintId` a `ConflictResolverPanel` would name.
    use kernel_constraints::diagnosis::RedundancyKind;
    use OpenCADStudio::scene::parametric_constraints::ConstraintKind;

    let mut scene = Scene::new();
    let a = add_line(&mut scene, 0.0, 0.0, 10.0, 3.0);
    let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
    let first = set.add(
        ConstraintKind::Horizontal,
        vec![ParametricRef::whole(a)],
        None,
    );
    let second = set.add(
        ConstraintKind::Horizontal,
        vec![ParametricRef::whole(a)],
        None,
    ); // exact duplicate

    scene.bump_entities(&[(a, ChangeKind::Modified)]);

    let set = scene
        .parametric_constraint_set(ParametricScope::ModelSpace)
        .unwrap();
    assert_eq!(
        set.conflicts.len(),
        1,
        "exactly one of the two identical constraints should be flagged"
    );
    let (flagged_id, kind) = set.conflicts[0];
    assert!(
        flagged_id == first || flagged_id == second,
        "the flagged id should be one of the two duplicates"
    );
    assert_eq!(kind, RedundancyKind::Redundant);
}

#[test]
fn two_conflicting_distance_targets_are_reported_as_conflicting() {
    use kernel_constraints::diagnosis::RedundancyKind;
    use OpenCADStudio::scene::parametric_constraints::ConstraintKind;

    let mut scene = Scene::new();
    let a = add_line(&mut scene, 0.0, 0.0, 10.0, 0.0);
    let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
    let first = set.add(
        ConstraintKind::Distance,
        vec![ParametricRef::point(a, 0), ParametricRef::point(a, 1)],
        Some(DrivingValue::Literal(20.0)),
    );
    let second = set.add(
        ConstraintKind::Distance,
        vec![ParametricRef::point(a, 0), ParametricRef::point(a, 1)],
        Some(DrivingValue::Literal(50.0)),
    );

    scene.bump_entities(&[(a, ChangeKind::Modified)]);

    let set = scene
        .parametric_constraint_set(ParametricScope::ModelSpace)
        .unwrap();
    assert_eq!(set.conflicts.len(), 1);
    let (flagged_id, kind) = set.conflicts[0];
    assert!(
        flagged_id == first || flagged_id == second,
        "the flagged id should be one of the two conflicting Distance constraints"
    );
    assert_eq!(kind, RedundancyKind::Conflicting);
}

fn set_circle_center(scene: &mut Scene, handle: Handle, center: Vector3) {
    if let Some(EntityType::Circle(c)) = scene.document.get_entity_mut(handle) {
        c.center = center;
    }
}

fn set_line_start(scene: &mut Scene, handle: Handle, start: Vector3) {
    if let Some(EntityType::Line(l)) = scene.document.get_entity_mut(handle) {
        l.start = start;
    }
}

#[test]
fn concentric_constraint_pulls_the_second_circles_center_onto_the_firsts() {
    let mut scene = Scene::new();
    let a = add_circle(&mut scene, 0.0, 0.0, 3.0);
    let b = add_circle(&mut scene, 5.0, 5.0, 1.0);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Concentric,
            vec![ParametricRef::center(a), ParametricRef::center(b)],
            None,
        );

    set_circle_center(&mut scene, a, Vector3::new(2.0, -3.0, 0.0));
    scene.bump_entities(&[(a, ChangeKind::Modified)]);

    let (ca, ra) = circle_geom(&scene, a);
    let (cb, rb) = circle_geom(&scene, b);
    assert!(
        (ca.x - cb.x).abs() < 1e-6 && (ca.y - cb.y).abs() < 1e-6,
        "centers should coincide: a={ca:?} b={cb:?}"
    );
    // Radii are untouched by Concentric — only centers move.
    assert!(
        (ra - 3.0).abs() < 1e-6 && (rb - 1.0).abs() < 1e-6,
        "radii must not change: ra={ra} rb={rb}"
    );
}

#[test]
fn center_point_constraint_pulls_a_lines_endpoint_onto_a_circles_center() {
    let mut scene = Scene::new();
    let circle = add_circle(&mut scene, 0.0, 0.0, 4.0);
    let line = add_line(&mut scene, 10.0, 10.0, 20.0, 20.0);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::CenterPoint,
            vec![ParametricRef::point(line, 0), ParametricRef::center(circle)],
            None,
        );

    set_circle_center(&mut scene, circle, Vector3::new(-6.0, 9.0, 0.0));
    scene.bump_entities(&[(circle, ChangeKind::Modified)]);

    let (center, _) = circle_geom(&scene, circle);
    let (line_start, _) = line_endpoints(&scene, line);
    assert!(
        (center.x - line_start.x).abs() < 1e-6 && (center.y - line_start.y).abs() < 1e-6,
        "the line's start should sit exactly at the circle's center: center={center:?} start={line_start:?}"
    );
}

#[test]
fn colinear_constraint_pulls_the_second_line_onto_the_firsts_infinite_line() {
    let mut scene = Scene::new();
    let a = add_line(&mut scene, 0.0, 0.0, 10.0, 0.0);
    let b = add_line(&mut scene, 3.0, 4.0, 7.0, 6.0); // off-axis, not on a's line

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Colinear,
            vec![ParametricRef::whole(a), ParametricRef::whole(b)],
            None,
        );

    scene.bump_entities(&[(b, ChangeKind::Modified)]);

    let (a1, a2) = line_endpoints(&scene, a);
    let (b1, b2) = line_endpoints(&scene, b);
    // Both of b's endpoints should land on a's infinite line: the signed
    // area of (a2-a1) × (bN-a1) is ~0 for a point exactly on that line.
    let area = |p: Vector3| (a2.x - a1.x) * (p.y - a1.y) - (a2.y - a1.y) * (p.x - a1.x);
    assert!(
        area(b1).abs() < 1e-5,
        "b.start should land on a's line, area={}",
        area(b1)
    );
    assert!(
        area(b2).abs() < 1e-5,
        "b.end should land on a's line, area={}",
        area(b2)
    );
}

#[test]
fn midpoint_constraint_pulls_a_point_onto_a_lines_midpoint() {
    let mut scene = Scene::new();
    let base = add_line(&mut scene, 0.0, 0.0, 10.0, 0.0);
    let marker = add_line(&mut scene, 20.0, 20.0, 21.0, 21.0); // marker.start is the tracked point

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Midpoint,
            vec![ParametricRef::point(marker, 0), ParametricRef::whole(base)],
            None,
        );

    set_line_end(&mut scene, base, Vector3::new(10.0, 8.0, 0.0));
    scene.bump_entities(&[(base, ChangeKind::Modified)]);

    let (b1, b2) = line_endpoints(&scene, base);
    let (marker_start, _) = line_endpoints(&scene, marker);
    let expected = Vector3::new((b1.x + b2.x) / 2.0, (b1.y + b2.y) / 2.0, 0.0);
    assert!(
        (marker_start.x - expected.x).abs() < 1e-6 && (marker_start.y - expected.y).abs() < 1e-6,
        "marker point should sit at base's midpoint: expected={expected:?} got={marker_start:?}"
    );
}

#[test]
fn midpoint_constraint_supports_a_point_entity() {
    let mut scene = Scene::new();
    let base = add_line(&mut scene, 0.0, 0.0, 10.0, 6.0);
    let point = scene.add_entity(EntityType::Point(codec::entities::Point::at(Vector3::new(
        20.0, 20.0, 0.0,
    ))));
    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Midpoint,
            vec![ParametricRef::point(point, 0), ParametricRef::whole(base)],
            None,
        );

    scene.bump_entities(&[(base, ChangeKind::Modified)]);
    let (start, end) = line_endpoints(&scene, base);
    let EntityType::Point(point) = scene.document.get_entity(point).unwrap() else {
        panic!("expected a point");
    };
    assert!((point.location.x - (start.x + end.x) / 2.0).abs() < 1e-6);
    assert!((point.location.y - (start.y + end.y) / 2.0).abs() < 1e-6);
}

#[test]
fn coincident_constraint_supports_a_block_insertion_point() {
    let mut scene = Scene::new();
    let line = add_line(&mut scene, 0.0, 0.0, 10.0, 4.0);
    let insert = scene.add_entity(EntityType::Insert(codec::entities::Insert::new(
        "fixture",
        Vector3::new(30.0, 20.0, 0.0),
    )));
    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Coincident,
            vec![
                ParametricRef::point(line, 1),
                ParametricRef::point(insert, 0),
            ],
            None,
        );

    scene.bump_entities(&[(line, ChangeKind::Modified)]);
    let (_, end) = line_endpoints(&scene, line);
    let EntityType::Insert(insert) = scene.document.get_entity(insert).unwrap() else {
        panic!("expected a block reference");
    };
    assert!((insert.insert_point.x - end.x).abs() < 1e-6);
    assert!((insert.insert_point.y - end.y).abs() < 1e-6);
}

#[test]
fn fixed_constraint_holds_an_entity_in_place_despite_a_connected_edit() {
    let mut scene = Scene::new();
    let fixed_line = add_line(&mut scene, 0.0, 0.0, 10.0, 0.0);
    let moving_line = add_line(&mut scene, 10.0, 0.0, 10.0, 10.0);

    let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
    set.add(
        ConstraintKind::Fixed,
        vec![ParametricRef::whole(fixed_line)],
        None,
    );
    set.add(
        ConstraintKind::Coincident,
        vec![
            ParametricRef::point(fixed_line, 1),
            ParametricRef::point(moving_line, 0),
        ],
        None,
    );

    let (before_start, before_end) = line_endpoints(&scene, fixed_line);

    // Drag the shared point — without Fixed this would pull fixed_line's
    // end along with it; Fixed should hold fixed_line exactly in place and
    // let moving_line's start follow back to it instead.
    set_line_start(&mut scene, moving_line, Vector3::new(15.0, 5.0, 0.0));
    scene.bump_entities(&[(moving_line, ChangeKind::Modified)]);

    let (after_start, after_end) = line_endpoints(&scene, fixed_line);
    assert_eq!(
        before_start, after_start,
        "Fixed entity's start must not move"
    );
    assert_eq!(before_end, after_end, "Fixed entity's end must not move");
    let (moving_start, _) = line_endpoints(&scene, moving_line);
    assert!(
        (moving_start.x - before_end.x).abs() < 1e-6
            && (moving_start.y - before_end.y).abs() < 1e-6,
        "moving_line's start should have been pulled back to fixed_line's (unmoved) end"
    );
}

#[test]
fn point_on_curve_constraint_pulls_a_point_onto_a_circles_circumference() {
    let mut scene = Scene::new();
    let circle = add_circle(&mut scene, 0.0, 0.0, 5.0);
    let marker = add_line(&mut scene, 100.0, 100.0, 101.0, 101.0); // marker.start is the tracked point

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::PointOnCurve,
            vec![
                ParametricRef::point(marker, 0),
                ParametricRef::whole(circle),
            ],
            None,
        );

    scene.bump_entities(&[(marker, ChangeKind::Modified)]);

    let (center, radius) = circle_geom(&scene, circle);
    let (marker_start, _) = line_endpoints(&scene, marker);
    let dist = ((marker_start.x - center.x).powi(2) + (marker_start.y - center.y).powi(2)).sqrt();
    assert!(
        (dist - radius).abs() < 1e-5,
        "point should land on the circumference: dist={dist} radius={radius}"
    );
}

#[test]
fn point_on_curve_constraint_pulls_a_point_onto_an_ellipse() {
    let mut scene = Scene::new();
    let ellipse = add_ellipse(&mut scene, 0.0, 0.0, (5.0, 0.0), 0.6);
    let marker = add_line(&mut scene, 12.0, 8.0, 13.0, 8.0);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::PointOnCurve,
            vec![
                ParametricRef::point(marker, 0),
                ParametricRef::whole(ellipse),
            ],
            None,
        );
    scene.bump_entities(&[(marker, ChangeKind::Modified)]);

    let (center, major, ratio) = ellipse_geom(&scene, ellipse);
    let (point, _) = line_endpoints(&scene, marker);
    let a = major.length();
    let b = a * ratio;
    let u = major / a;
    let v = Vector3::new(-u.y, u.x, 0.0);
    let offset = point - center;
    let equation = (offset.dot(&u) / a).powi(2) + (offset.dot(&v) / b).powi(2);
    assert!(
        (equation - 1.0).abs() < 1e-5,
        "point should satisfy the solved ellipse equation: {equation}"
    );
}

#[test]
fn tangent_constraint_solves_an_ellipse_and_line() {
    let mut scene = Scene::new();
    let ellipse = add_ellipse(&mut scene, 0.0, 0.0, (5.0, 0.0), 0.6);
    let line = add_line(&mut scene, -8.0, 8.0, 8.0, 8.0);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Tangent,
            vec![ParametricRef::whole(ellipse), ParametricRef::whole(line)],
            None,
        );
    scene.bump_entities(&[(line, ChangeKind::Modified)]);

    let (center, major, ratio) = ellipse_geom(&scene, ellipse);
    let (start, end) = line_endpoints(&scene, line);
    let direction = (end - start).normalize();
    let normal = Vector3::new(-direction.y, direction.x, 0.0);
    let unit_major = major.normalize();
    let unit_minor = Vector3::new(-unit_major.y, unit_major.x, 0.0);
    let a = major.length();
    let b = a * ratio;
    let support =
        ((a * normal.dot(&unit_major)).powi(2) + (b * normal.dot(&unit_minor)).powi(2)).sqrt();
    let distance = (center - start).dot(&normal).abs();
    assert!(
        (distance - support).abs() < 1e-5,
        "line should be tangent to the solved ellipse: distance={distance} support={support}"
    );
}

#[test]
fn equal_constraint_matches_ellipse_major_axes() {
    let mut scene = Scene::new();
    let a = add_ellipse(&mut scene, 0.0, 0.0, (5.0, 0.0), 0.6);
    let b = add_ellipse(&mut scene, 20.0, 0.0, (2.0, 0.0), 0.5);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Equal,
            vec![ParametricRef::whole(a), ParametricRef::whole(b)],
            None,
        );
    scene.bump_entities(&[(b, ChangeKind::Modified)]);

    let (_, major_a, _) = ellipse_geom(&scene, a);
    let (_, major_b, _) = ellipse_geom(&scene, b);
    assert!(
        (major_a.length() - major_b.length()).abs() < 1e-5,
        "ellipse major radii should match: {} vs {}",
        major_a.length(),
        major_b.length()
    );
}

#[test]
fn equal_distance_constraint_matches_a_second_point_pairs_separation() {
    let mut scene = Scene::new();
    // Reference pair: fixed 6 units apart.
    let a = add_line(&mut scene, 0.0, 0.0, 6.0, 0.0);
    // Tracked pair: starts at some other separation, should be pulled to 6.
    let b = add_line(&mut scene, 20.0, 20.0, 25.0, 20.0);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::EqualDistance,
            vec![
                ParametricRef::point(b, 0),
                ParametricRef::point(b, 1),
                ParametricRef::point(a, 0),
                ParametricRef::point(a, 1),
            ],
            None,
        );

    scene.bump_entities(&[(b, ChangeKind::Modified)]);

    let (a1, a2) = line_endpoints(&scene, a);
    let (b1, b2) = line_endpoints(&scene, b);
    let dist_a = ((a2.x - a1.x).powi(2) + (a2.y - a1.y).powi(2)).sqrt();
    let dist_b = ((b2.x - b1.x).powi(2) + (b2.y - b1.y).powi(2)).sqrt();
    assert!(
        (dist_a - dist_b).abs() < 1e-6,
        "the two pairs' separations should match: dist_a={dist_a} dist_b={dist_b}"
    );
}

#[test]
fn symmetric_constraint_mirrors_one_circles_center_across_the_axis_line() {
    let mut scene = Scene::new();
    let axis = add_line(&mut scene, 0.0, 0.0, 0.0, 10.0); // the Y axis
    let a = add_circle(&mut scene, 3.0, 4.0, 1.0);
    // Starts well off the mirrored position (and not coincident with `a` —
    // a zero-length a/b segment would leave `Perpendicular`'s precomputed
    // scale dividing by zero at the very first solve iteration).
    let b = add_circle(&mut scene, 8.0, 9.0, 1.0);

    let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
    // Pin the axis and `a` in place — otherwise they're just as free to move
    // as `b`'s center, and with only 2 equations (MidpointOnLine +
    // Perpendicular) against that many more unknowns the solver is free to
    // converge on any of infinitely many valid configurations, not
    // necessarily "only b moves, to a's exact mirror" (which is the one
    // deterministic outcome this test actually wants to check).
    set.add(
        ConstraintKind::Fixed,
        vec![ParametricRef::whole(axis)],
        None,
    );
    set.add(ConstraintKind::Fixed, vec![ParametricRef::whole(a)], None);
    set.add(
        ConstraintKind::Symmetric,
        vec![
            ParametricRef::center(a),
            ParametricRef::center(b),
            ParametricRef::whole(axis),
        ],
        None,
    );

    scene.bump_entities(&[(b, ChangeKind::Modified)]);

    let (ca, _) = circle_geom(&scene, a);
    let (cb, _) = circle_geom(&scene, b);
    assert_eq!(
        ca,
        Vector3::new(3.0, 4.0, 0.0),
        "Fixed a's center must not have moved"
    );
    // With both a and the axis pinned, b's center has a unique valid
    // position left: a's exact mirror across the Y axis, (-3, 4).
    assert!(
        (cb.x - -3.0).abs() < 1e-5 && (cb.y - 4.0).abs() < 1e-5,
        "b's center should have been pulled to a's mirror image across the axis: expected (-3, 4), got {cb:?}"
    );
}

// Phase 1 of docs' constraint-parity plan: an Arc registers in the solver
// as its own center/radius (an `EntityGeom::Circle`), so every whole-circle
// constraint kind — Radius, Tangent, Concentric among them — already works
// on it. These mirror the equivalent Circle tests above one-for-one.

#[test]
fn radius_constraint_on_an_arc_solves_to_the_target() {
    let mut scene = Scene::new();
    let arc = add_arc(&mut scene, 0.0, 0.0, 3.0, 0.0, std::f64::consts::FRAC_PI_2);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Radius,
            vec![ParametricRef::whole(arc)],
            Some(DrivingValue::Literal(7.5)),
        );
    scene.bump_entities(&[(arc, ChangeKind::Modified)]);

    let (_, radius) = arc_geom(&scene, arc);
    assert!(
        (radius - 7.5).abs() < 1e-6,
        "arc radius should have solved to the driven target: got {radius}"
    );
}

#[test]
fn tangent_constraint_between_a_line_and_an_arc_solves_to_touching() {
    let mut scene = Scene::new();
    let arc = add_arc(&mut scene, 0.0, 0.0, 5.0, 0.0, std::f64::consts::PI);
    let line = add_line(&mut scene, -10.0, 10.0, 10.0, 10.0); // clear of the arc's circle

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Tangent,
            vec![ParametricRef::whole(arc), ParametricRef::whole(line)],
            None,
        );
    scene.bump_entities(&[(arc, ChangeKind::Modified), (line, ChangeKind::Modified)]);

    let (center, radius) = arc_geom(&scene, arc);
    let (p1, p2) = line_endpoints(&scene, line);
    let line_dir = (p2.x - p1.x, p2.y - p1.y);
    let line_len = (line_dir.0 * line_dir.0 + line_dir.1 * line_dir.1).sqrt();
    let signed_dist = (line_dir.0 * (center.y - p1.y) - line_dir.1 * (center.x - p1.x)) / line_len;
    assert!((signed_dist.abs() - radius).abs() < 1e-6, "center-to-line distance should equal the arc's radius after solving: dist={signed_dist} radius={radius}");
}

#[test]
fn concentric_constraint_between_an_arc_and_a_circle_pulls_centers_together() {
    let mut scene = Scene::new();
    let arc = add_arc(&mut scene, 0.0, 0.0, 4.0, 0.0, std::f64::consts::PI);
    let circle = add_circle(&mut scene, 10.0, -6.0, 2.0);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Concentric,
            vec![ParametricRef::center(arc), ParametricRef::center(circle)],
            None,
        );
    scene.bump_entities(&[(arc, ChangeKind::Modified), (circle, ChangeKind::Modified)]);

    let (arc_center, _) = arc_geom(&scene, arc);
    let (circle_center, _) = circle_geom(&scene, circle);
    assert!(
        (arc_center.x - circle_center.x).abs() < 1e-6 && (arc_center.y - circle_center.y).abs() < 1e-6,
        "arc and circle centers should coincide after solving: arc={arc_center:?} circle={circle_center:?}"
    );
}

// Phase 4b of the same plan: an arc's actual endpoints (marker 0/1, not
// just its center or whole curve) are now real, solvable points — kept
// consistent with center/radius/angle by the "arc rules" `CurveValue`
// constraints `solve_scope` adds for every registered arc. Coincident/
// PointOnCurve/etc. touching an arc endpoint need no kind-specific code at
// all: they resolve through the same generic `point_ref`/`point_for_marker`
// path every other point-ref constraint already uses.

fn arc_start_point(scene: &Scene, handle: Handle) -> Vector3 {
    match scene.document.get_entity(handle).expect("entity exists") {
        EntityType::Arc(a) => a.start_point(),
        other => panic!("expected an Arc, got {other:?}"),
    }
}

#[test]
fn coincident_constraint_pulls_a_lines_endpoint_onto_an_arcs_start_point() {
    let mut scene = Scene::new();
    let arc = add_arc(&mut scene, 0.0, 0.0, 5.0, 0.0, std::f64::consts::FRAC_PI_2);
    let line = add_line(&mut scene, 20.0, 20.0, 30.0, 30.0);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Coincident,
            vec![ParametricRef::point(arc, 0), ParametricRef::point(line, 0)],
            None,
        );

    // Move the arc directly (bypassing the solver), same pattern as the
    // equivalent Circle/Ellipse CenterPoint tests — the constraint is a
    // symmetric point-equality, so what's checked is that both sides end
    // up equal to *each other*, not at some hardcoded absolute position.
    if let Some(EntityType::Arc(a)) = scene.document.get_entity_mut(arc) {
        a.center = Vector3::new(-8.0, 3.0, 0.0);
    }
    scene.bump_entities(&[(arc, ChangeKind::Modified)]);

    let arc_start = arc_start_point(&scene, arc);
    let (line_start, _) = line_endpoints(&scene, line);
    assert!(
        (arc_start.x - line_start.x).abs() < 1e-6 && (arc_start.y - line_start.y).abs() < 1e-6,
        "the line's start should sit exactly at the arc's start point: arc_start={arc_start:?} line_start={line_start:?}"
    );
}

#[test]
fn an_arcs_endpoint_stays_consistent_with_its_center_radius_and_angle_after_solving() {
    // The actual "arc rules" guarantee: after a solve that moved a
    // constrained endpoint, recomputing the arc's start point from its
    // (possibly also moved) center/radius/start_angle must land exactly on
    // the same point the constraint pulled the endpoint to — not just "some
    // point near it". This is what would drift if the `CurveValue` ties
    // were missing or wrong.
    let mut scene = Scene::new();
    let arc = add_arc(&mut scene, 0.0, 0.0, 5.0, 0.0, std::f64::consts::PI);
    let anchor = add_line(&mut scene, 12.0, 7.0, 12.0, 7.0);

    let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
    set.add(
        ConstraintKind::Fixed,
        vec![ParametricRef::whole(anchor)],
        None,
    );
    set.add(
        ConstraintKind::Coincident,
        vec![
            ParametricRef::point(arc, 0),
            ParametricRef::point(anchor, 0),
        ],
        None,
    );
    scene.bump_entities(&[(arc, ChangeKind::Modified)]);

    let (center, radius, start_angle, _) = {
        match scene.document.get_entity(arc).expect("entity exists") {
            EntityType::Arc(a) => (a.center, a.radius, a.start_angle, a.end_angle),
            other => panic!("expected an Arc, got {other:?}"),
        }
    };
    let recomputed_start = Vector3::new(
        center.x + radius * start_angle.cos(),
        center.y + radius * start_angle.sin(),
        0.0,
    );
    let actual_start = arc_start_point(&scene, arc);
    assert!(
        (recomputed_start.x - actual_start.x).abs() < 1e-6 && (recomputed_start.y - actual_start.y).abs() < 1e-6,
        "start_point() must match center+radius*angle exactly: recomputed={recomputed_start:?} actual={actual_start:?}"
    );
    assert!(
        (actual_start.x - 12.0).abs() < 1e-6 && (actual_start.y - 7.0).abs() < 1e-6,
        "the constrained endpoint itself should have reached the fixed anchor: {actual_start:?}"
    );
}

#[test]
fn point_on_curve_constraint_keeps_a_point_within_an_arcs_sweep() {
    let mut scene = Scene::new();
    let arc = add_arc(&mut scene, 0.0, 0.0, 5.0, 0.0, std::f64::consts::PI);
    let marker = add_line(&mut scene, 0.0, -20.0, 1.0, -20.0);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::PointOnCurve,
            vec![ParametricRef::point(marker, 0), ParametricRef::whole(arc)],
            None,
        );
    scene.bump_entities(&[(marker, ChangeKind::Modified)]);

    let (center, radius, start_angle, end_angle) =
        match scene.document.get_entity(arc).expect("entity exists") {
            EntityType::Arc(arc) => (arc.center, arc.radius, arc.start_angle, arc.end_angle),
            other => panic!("expected an Arc, got {other:?}"),
        };
    let (p, _) = line_endpoints(&scene, marker);
    let dist = ((p.x - center.x).powi(2) + (p.y - center.y).powi(2)).sqrt();
    assert!(
        (dist - radius).abs() < 1e-6,
        "point should land exactly on the arc: dist={dist} radius={radius}"
    );
    assert!(
        kernel::geom2d::angle_within_arc(
            (p.y - center.y).atan2(p.x - center.x),
            start_angle,
            end_angle,
        ),
        "point must stay within the solved arc sweep: {p:?}"
    );
}

#[test]
fn point_on_curve_constraint_keeps_a_point_within_a_polyline_arc_segment() {
    let mut source = codec::entities::LwPolyline::new();
    source.add_point_with_bulge(codec::types::Vector2::new(0.0, 0.0), 1.0);
    source.add_point(codec::types::Vector2::new(10.0, 0.0));
    let mut scene = Scene::new();
    let polyline = scene.add_entity(EntityType::LwPolyline(source));
    let marker = add_line(&mut scene, 5.0, 20.0, 6.0, 20.0);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::PointOnCurve,
            vec![
                ParametricRef::point(marker, 0),
                ParametricRef::segment(polyline, 0),
            ],
            None,
        );
    scene.bump_entities(&[(marker, ChangeKind::Modified)]);

    let (point, _) = line_endpoints(&scene, marker);
    let EntityType::LwPolyline(polyline) = scene.document.get_entity(polyline).unwrap() else {
        panic!("expected a lightweight polyline");
    };
    let arc = kernel::geom2d::BulgeArc::from_bulge(
        [
            polyline.vertices[0].location.x,
            polyline.vertices[0].location.y,
        ],
        [
            polyline.vertices[1].location.x,
            polyline.vertices[1].location.y,
        ],
        polyline.vertices[0].bulge,
    )
    .unwrap();
    let distance = (point.x - arc.center[0]).hypot(point.y - arc.center[1]);
    assert!((distance - arc.radius).abs() < 1e-6);
    assert!(
        kernel::geom2d::angle_within_arc(
            (point.y - arc.center[1]).atan2(point.x - arc.center[0]),
            arc.start_angle,
            arc.start_angle + arc.sweep,
        ),
        "point must stay within the solved polyline arc segment: {point:?}"
    );
}

#[test]
fn radius_and_endpoint_coincident_constraints_coexist_on_the_same_arc() {
    // Exercises both registration paths for the same arc in one solve: the
    // whole-circle path (`Radius`, via `.circle`) and the endpoint path
    // (`Coincident` on marker 0, via the arc-rules-anchored `start` point).
    let mut scene = Scene::new();
    let arc = add_arc(&mut scene, 0.0, 0.0, 3.0, 0.0, std::f64::consts::FRAC_PI_2);
    let anchor = add_line(&mut scene, 9.0, -4.0, 9.0, -4.0);

    let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
    set.add(
        ConstraintKind::Fixed,
        vec![ParametricRef::whole(anchor)],
        None,
    );
    set.add(
        ConstraintKind::Coincident,
        vec![
            ParametricRef::point(arc, 0),
            ParametricRef::point(anchor, 0),
        ],
        None,
    );
    set.add(
        ConstraintKind::Radius,
        vec![ParametricRef::whole(arc)],
        Some(DrivingValue::Literal(6.0)),
    );
    scene.bump_entities(&[(arc, ChangeKind::Modified)]);

    let (_, radius) = arc_geom(&scene, arc);
    assert!(
        (radius - 6.0).abs() < 1e-6,
        "Radius constraint should still hold: got {radius}"
    );
    let actual_start = arc_start_point(&scene, arc);
    assert!(
        (actual_start.x - 9.0).abs() < 1e-6 && (actual_start.y - -4.0).abs() < 1e-6,
        "the arc's start point should have reached the fixed anchor despite the simultaneous Radius constraint: {actual_start:?}"
    );
}

// Phase 2 of the same plan: `Diameter`/`DistanceX`/`DistanceY` reuse
// existing `opencadkernel_constraints` primitives (`Equal`'s `ratio`, `Difference`) with no
// new solver-crate code.

#[test]
fn diameter_constraint_drives_the_radius_to_half_the_target() {
    let mut scene = Scene::new();
    let circle = add_circle(&mut scene, 0.0, 0.0, 3.0);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Diameter,
            vec![ParametricRef::whole(circle)],
            Some(DrivingValue::Literal(16.0)),
        );
    scene.bump_entities(&[(circle, ChangeKind::Modified)]);

    let (_, radius) = circle_geom(&scene, circle);
    assert!(
        (radius - 8.0).abs() < 1e-6,
        "radius should be half the diameter target: got {radius}"
    );
}

#[test]
fn distance_x_constraint_pins_only_the_x_component() {
    let mut scene = Scene::new();
    // Deliberately not axis-aligned so DistanceX and DistanceY are
    // distinguishable from a plain Distance/Horizontal outcome.
    let line = add_line(&mut scene, 0.0, 0.0, 6.0, 8.0);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::DistanceX,
            vec![ParametricRef::point(line, 0), ParametricRef::point(line, 1)],
            Some(DrivingValue::Literal(10.0)),
        );
    scene.bump_entities(&[(line, ChangeKind::Modified)]);

    let (start, end) = line_endpoints(&scene, line);
    assert!(
        (end.x - start.x - 10.0).abs() < 1e-6,
        "x component should equal the target: dx={}",
        end.x - start.x
    );
}

// Phase 3 of the same plan: `Normal` reduces to "line passes through the
// circle's center" for the Line/Circle-only entity model, reusing
// `PointOnLine` (the same primitive `PointOnCurve` already uses).

#[test]
fn normal_constraint_pulls_the_line_through_the_circles_center() {
    let mut scene = Scene::new();
    let circle = add_circle(&mut scene, 5.0, 5.0, 2.0);
    // A horizontal line well clear of the circle's center.
    let line = add_line(&mut scene, -10.0, 0.0, 10.0, 0.0);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Normal,
            vec![ParametricRef::whole(circle), ParametricRef::whole(line)],
            None,
        );
    scene.bump_entities(&[(circle, ChangeKind::Modified), (line, ChangeKind::Modified)]);

    let (center, _) = circle_geom(&scene, circle);
    let (p1, p2) = line_endpoints(&scene, line);
    let line_dir = (p2.x - p1.x, p2.y - p1.y);
    let line_len = (line_dir.0 * line_dir.0 + line_dir.1 * line_dir.1).sqrt();
    let signed_dist = (line_dir.0 * (center.y - p1.y) - line_dir.1 * (center.x - p1.x)) / line_len;
    assert!(
        signed_dist.abs() < 1e-6,
        "the circle's center should lie on the (infinite) line after solving: dist={signed_dist}"
    );
}

// Phase 5 of the same plan: an `Ellipse` registers center/focus1/minor-
// radius (converted from opencadcodec's center + major-axis-vector + ratio
// parametrization). Only center-based whole-entity constraints are wired
// up so far — Concentric, CenterPoint, Fixed — mirroring how Arc started
// at "center/radius only" in Phase 1. Axis-length constraints and
// `PointOnEllipse`-based Coincident/Tangent are a further follow-up.

#[test]
fn concentric_constraint_between_an_ellipse_and_a_circle_pulls_centers_together() {
    let mut scene = Scene::new();
    let ellipse = add_ellipse(&mut scene, 0.0, 0.0, (6.0, 0.0), 0.5);
    let circle = add_circle(&mut scene, 10.0, -6.0, 2.0);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Concentric,
            vec![
                ParametricRef::center(ellipse),
                ParametricRef::center(circle),
            ],
            None,
        );
    scene.bump_entities(&[
        (ellipse, ChangeKind::Modified),
        (circle, ChangeKind::Modified),
    ]);

    let (ellipse_center, _, _) = ellipse_geom(&scene, ellipse);
    let (circle_center, _) = circle_geom(&scene, circle);
    assert!(
        (ellipse_center.x - circle_center.x).abs() < 1e-6 && (ellipse_center.y - circle_center.y).abs() < 1e-6,
        "ellipse and circle centers should coincide after solving: ellipse={ellipse_center:?} circle={circle_center:?}"
    );
}

#[test]
fn center_point_constraint_pulls_a_lines_endpoint_onto_an_ellipses_center() {
    let mut scene = Scene::new();
    let ellipse = add_ellipse(&mut scene, 0.0, 0.0, (5.0, 0.0), 0.6);
    let line = add_line(&mut scene, 10.0, 10.0, 20.0, 20.0);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::CenterPoint,
            vec![
                ParametricRef::point(line, 0),
                ParametricRef::center(ellipse),
            ],
            None,
        );

    // Move the ellipse directly (bypassing the solver) before the first
    // solve, same as the equivalent Circle test — the constraint is a
    // symmetric point-equality, so what's checked is that both sides end
    // up equal to *each other*, not at some hardcoded absolute position.
    if let Some(EntityType::Ellipse(e)) = scene.document.get_entity_mut(ellipse) {
        e.center = Vector3::new(-6.0, 9.0, 0.0);
    }
    scene.bump_entities(&[(ellipse, ChangeKind::Modified)]);

    let (center, _, _) = ellipse_geom(&scene, ellipse);
    let (line_start, _) = line_endpoints(&scene, line);
    assert!(
        (center.x - line_start.x).abs() < 1e-6 && (center.y - line_start.y).abs() < 1e-6,
        "the line's start should sit exactly at the ellipse's center: center={center:?} start={line_start:?}"
    );
}

#[test]
fn a_tilted_ellipse_round_trips_through_the_solver_without_drifting() {
    // The riskiest part of Ellipse support: converting opencadcodec's center +
    // major-axis-*vector* + minor/major ratio into `opencadkernel_constraints`'s center +
    // focus + minor-radius, then back, for a major axis that ISN'T
    // axis-aligned (an axis-aligned one wouldn't exercise the direction
    // math at all). A solve with nothing actually pulling on it should
    // reproduce the same ellipse, not drift.
    let mut scene = Scene::new();
    let ellipse = add_ellipse(&mut scene, 2.0, -3.0, (4.0, 3.0), 0.6); // major radius 5, tilted
    let circle = add_circle(&mut scene, 2.0, -3.0, 1.0); // already concentric — no solving needed

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Concentric,
            vec![
                ParametricRef::center(ellipse),
                ParametricRef::center(circle),
            ],
            None,
        );
    scene.bump_entities(&[(ellipse, ChangeKind::Modified)]);

    let (center, major_axis, ratio) = ellipse_geom(&scene, ellipse);
    assert!(
        (center.x - 2.0).abs() < 1e-6 && (center.y - -3.0).abs() < 1e-6,
        "center should not have drifted: {center:?}"
    );
    assert!(
        (major_axis.x - 4.0).abs() < 1e-6 && (major_axis.y - 3.0).abs() < 1e-6,
        "major axis should not have drifted: {major_axis:?}"
    );
    assert!(
        (ratio - 0.6).abs() < 1e-6,
        "minor/major ratio should not have drifted: {ratio}"
    );
}

#[test]
fn concentric_constraint_does_not_reshape_the_ellipse_when_its_center_actually_moves() {
    // Regression test for a real solver bug: `a_tilted_ellipse_round_trips_
    // through_the_solver_without_drifting` (above) uses a circle that's
    // ALREADY concentric with the ellipse, so Concentric's equations start
    // out satisfied and the ellipse's center never actually has to move —
    // it doesn't exercise the bug at all. Here the circle starts elsewhere,
    // forcing a real center move, which is exactly when `kernel_constraints::geo::
    // Ellipse`'s absolute `focus1` point used to get left behind: `center`
    // moved to meet the circle, `focus1` didn't (nothing else referenced
    // it), so the derived major-axis direction/length and minor/major
    // ratio changed even though only a center-to-center constraint was
    // ever applied. The fix ties `focus1`'s offset from `center` to its
    // seeded value, so it translates along with `center` instead.
    let mut scene = Scene::new();
    let ellipse = add_ellipse(&mut scene, 0.0, 0.0, (4.0, 0.0), 0.5);
    let circle = add_circle(&mut scene, 10.0, 7.0, 1.5);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Concentric,
            vec![
                ParametricRef::center(ellipse),
                ParametricRef::center(circle),
            ],
            None,
        );
    scene.bump_entities(&[
        (ellipse, ChangeKind::Modified),
        (circle, ChangeKind::Modified),
    ]);

    let (ellipse_center, major_axis, ratio) = ellipse_geom(&scene, ellipse);
    let (circle_center, _) = circle_geom(&scene, circle);

    assert!(
        (ellipse_center.x - circle_center.x).abs() < 1e-6 && (ellipse_center.y - circle_center.y).abs() < 1e-6,
        "concentric constraint should still hold: ellipse={ellipse_center:?} circle={circle_center:?}"
    );
    // The whole point: the center was free to move (nothing else pinned
    // either entity), so it's not expected to still sit at (0,0) — but the
    // ellipse's *shape* must be exactly what it was before solving.
    assert!(
        (major_axis.x - 4.0).abs() < 1e-6 && (major_axis.y - 0.0).abs() < 1e-6,
        "major axis must not rotate or resize just because the center moved: {major_axis:?}"
    );
    assert!(
        (ratio - 0.5).abs() < 1e-6,
        "minor/major ratio must not change just because the center moved: {ratio}"
    );
}

#[test]
fn fixed_constraint_holds_an_ellipse_in_place_despite_a_connected_edit() {
    let mut scene = Scene::new();
    let ellipse = add_ellipse(&mut scene, 0.0, 0.0, (6.0, 0.0), 0.5);
    let line = add_line(&mut scene, 0.0, 0.0, 5.0, 5.0);

    let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
    set.add(
        ConstraintKind::Fixed,
        vec![ParametricRef::whole(ellipse)],
        None,
    );
    set.add(
        ConstraintKind::CenterPoint,
        vec![
            ParametricRef::point(line, 0),
            ParametricRef::center(ellipse),
        ],
        None,
    );

    let before = ellipse_geom(&scene, ellipse);

    // Drag the line's start away — without Fixed this would pull the
    // ellipse's center along with it; Fixed should hold the ellipse
    // exactly in place and let the line's start follow back to it instead.
    set_line_start(&mut scene, line, Vector3::new(20.0, -8.0, 0.0));
    scene.bump_entities(&[(line, ChangeKind::Modified)]);

    let after = ellipse_geom(&scene, ellipse);
    assert_eq!(
        before, after,
        "Fixed ellipse must not move at all (center, major axis, or ratio)"
    );
    let (start, _) = line_endpoints(&scene, line);
    assert!(
        (start.x - before.0.x).abs() < 1e-6 && (start.y - before.0.y).abs() < 1e-6,
        "line's start should have been pulled back to the (unmoved) ellipse center"
    );
}

#[test]
fn distance_y_constraint_pins_only_the_y_component() {
    let mut scene = Scene::new();
    let line = add_line(&mut scene, 0.0, 0.0, 6.0, 8.0);

    scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::DistanceY,
            vec![ParametricRef::point(line, 0), ParametricRef::point(line, 1)],
            Some(DrivingValue::Literal(-3.0)),
        );
    scene.bump_entities(&[(line, ChangeKind::Modified)]);

    let (start, end) = line_endpoints(&scene, line);
    assert!(
        (end.y - start.y - -3.0).abs() < 1e-6,
        "y component should equal the (signed) target: dy={}",
        end.y - start.y
    );
}

// The bonded-chain side flip: a line-arc-line chain bonded the way the
// civil plugin's BOND does is two-valued - the same constraint graph is
// satisfied identically by the arc bulging on either side of the legs (the
// corner's deflection read on the near side or the far side through the
// complement sweep). When a transaction writes the OTHER valid
// configuration - a fully consistent, zero-residual state - the solve the
// write triggers must recognize it as already converged and keep the
// written values exactly: it must never re-project the written state back
// onto the previous side, and neither may a following interactive frame's
// solve carrying the stale pre-write retention images.
#[test]
fn a_zero_residual_write_to_the_other_tangency_side_is_not_displaced() {
    let mut scene = Scene::new();
    // LINE, CIRCULARARC, LINE bonded exactly the way the civil plugin's
    // BOND bonds a chain (see add_bonded_chain_graph for the graph).
    let (line1, arc, line2, start_marker, end_marker) = add_bonded_chain_graph(&mut scene);

    // The written short configuration already satisfies every constraint
    // of the scope, so the first bump must hold it exactly as written.
    scene.bump_entities(&[
        (line1, ChangeKind::Modified),
        (arc, ChangeKind::Modified),
        (line2, ChangeKind::Modified),
        (start_marker, ChangeKind::Modified),
        (end_marker, ChangeKind::Modified),
    ]);
    let (arc_center, _) = arc_geom(&scene, arc);
    assert!(
        (arc_center.y - 15.0).abs() < 1e-6,
        "baseline: the short-arc write should hold exactly: {arc_center:?}"
    );

    // The far-side write: the same constraint graph with the arc re-seated
    // on the OTHER valid tangency configuration (the turn-band duality the
    // records model keeps with corner_long). The joint points on line 1
    // and both markers are untouched; the arc bulges to the other side
    // (center (-15, -15), start (-15, 0) at angle pi/2, end (0, -15) at
    // angle 2*pi through the complement sweep) and line 2's start follows
    // the joint. Every constraint above is satisfied EXACTLY by this state
    // too: zero residual across the whole scope.
    let mut far_arc = match scene.document.get_entity(arc) {
        Some(EntityType::Arc(a)) => a.clone(),
        other => panic!("expected an Arc, got {other:?}"),
    };
    far_arc.center = Vector3::new(-15.0, -15.0, 0.0);
    far_arc.start_angle = std::f64::consts::FRAC_PI_2;
    far_arc.end_angle = std::f64::consts::TAU;
    let mut far_line2 = match scene.document.get_entity(line2) {
        Some(EntityType::Line(l)) => l.clone(),
        other => panic!("expected a Line, got {other:?}"),
    };
    far_line2.start = Vector3::new(0.0, -15.0, 0.0);
    // The plugin's write route: one batched scene write whose single bump
    // solves once over the final state (update_entities_transaction's
    // batching comment: the solver must see every entity at once). The
    // retained pre-write images are captured first: the interactive drag
    // frames that keep firing around a plugin write carry the chain's
    // pre-write state as their retained originals (an open drag's undo
    // recording does exactly that).
    let frame_images: Vec<(Handle, EntityType)> = [line1, arc, line2, start_marker, end_marker]
        .into_iter()
        .filter_map(|handle| {
            scene
                .document
                .get_entity(handle)
                .cloned()
                .map(|entity| (handle, entity))
        })
        .collect();
    let moved =
        scene.update_entities_plugin(vec![EntityType::Arc(far_arc), EntityType::Line(far_line2)]);
    assert_eq!(moved, 2);

    // The interactive leg that surfaced the flip in the wild-grip QA: a
    // drag frame whose bump runs under the transform policy (retain_size =
    // true) while the dragged PI marker itself rewrites where it already
    // sits (its whole-entity pin follows the frame; the marker did not
    // move between the two configurations: it is pinned to line2's end,
    // which stays (0, 45) on both sides). The written far-side state
    // satisfies every constraint of the scope, so this frame is also a
    // fixpoint: the stale retention images (line2's pre-write length 30
    // against the written 60) must never re-project the chain onto the
    // other tangency side - that re-projection is the R = 15 flip the QA
    // measured.
    scene.bump_entities_with_parametric_transform_policy(
        &[(end_marker, ChangeKind::Modified)],
        &[],
        &[ParametricRef::whole(end_marker)],
        &frame_images,
    );

    // R1: a write whose result satisfies every constraint in the scope
    // must keep the written values exactly - the solve it triggers is a
    // no-op, never a re-projection onto the other tangency side.
    let (arc_center, arc_radius) = arc_geom(&scene, arc);
    let (line2_start, line2_end) = line_endpoints(&scene, line2);
    assert!(
        (arc_center.x - -15.0).abs() < 1e-6 && (arc_center.y - -15.0).abs() < 1e-6,
        "the solver must not displace a constraint-consistent write: the arc re-projected to {arc_center:?}"
    );
    assert!(
        (arc_radius - 15.0).abs() < 1e-6,
        "written radius must hold: {arc_radius}"
    );
    assert!(
        (line2_start.x - 0.0).abs() < 1e-6 && (line2_start.y - -15.0).abs() < 1e-6,
        "line2's start must keep the written joint: {line2_start:?}"
    );
    assert!(
        (line2_end.y - 45.0).abs() < 1e-6,
        "line2's untouched end must hold: {line2_end:?}"
    );
    let (_, start_marker_radius) = circle_geom(&scene, start_marker);
    let (_, end_marker_radius) = circle_geom(&scene, end_marker);
    assert!((start_marker_radius - 2.0).abs() < 1e-9);
    assert!((end_marker_radius - 2.0).abs() < 1e-9);
    // Nothing else drifted either: line 1 and both marker centers are the
    // same on both configurations, and the arc's end point (0, -15) is the
    // sharp flip detector - the short configuration ends the arc at (0, 15).
    let (line1_start, line1_end) = line_endpoints(&scene, line1);
    assert!((line1_start.x - -45.0).abs() < 1e-6 && (line1_start.y - 0.0).abs() < 1e-6);
    assert!((line1_end.x - -15.0).abs() < 1e-6 && (line1_end.y - 0.0).abs() < 1e-6);
    let (start_marker_center, _) = circle_geom(&scene, start_marker);
    let (end_marker_center, _) = circle_geom(&scene, end_marker);
    assert!(
        (start_marker_center.x - -45.0).abs() < 1e-6 && (start_marker_center.y - 0.0).abs() < 1e-6
    );
    assert!((end_marker_center.x - 0.0).abs() < 1e-6 && (end_marker_center.y - 45.0).abs() < 1e-6);
    let arc_end = match scene.document.get_entity(arc) {
        Some(EntityType::Arc(a)) => a.end_point(),
        other => panic!("expected an Arc, got {other:?}"),
    };
    assert!(
        (arc_end.x - 0.0).abs() < 1e-6 && (arc_end.y - -15.0).abs() < 1e-6,
        "the arc must still end at the written far-side joint: {arc_end:?}"
    );
}

// The fixpoint guarantee is not a veto on solving: a write whose state is
// genuinely violated (residual above the solver's own convergence
// tolerance) is still reconciled by the solve it triggers - the
// nearest-configuration doctrine for unsatisfied writes stays intact.
#[test]
fn a_constraint_violating_write_still_solves_back_to_tangency() {
    let mut scene = Scene::new();
    let (line1, arc, line2, start_marker, end_marker) = add_bonded_chain_graph(&mut scene);

    scene.bump_entities(&[
        (line1, ChangeKind::Modified),
        (arc, ChangeKind::Modified),
        (line2, ChangeKind::Modified),
        (start_marker, ChangeKind::Modified),
        (end_marker, ChangeKind::Modified),
    ]);

    // The plugin's write route, with the arc written 3 units off: its start
    // point (center + R at angle -pi/2) lands at (-15, 3), off line 1's end
    // (-15, 0) - the joint's Coincident row carries a real residual now.
    let mut bumped_arc = match scene.document.get_entity(arc) {
        Some(EntityType::Arc(a)) => a.clone(),
        other => panic!("expected an Arc, got {other:?}"),
    };
    bumped_arc.center = Vector3::new(-15.0, 18.0, 0.0);
    let moved = scene.update_entities_plugin(vec![EntityType::Arc(bumped_arc)]);
    assert_eq!(moved, 1);

    // Not a fixpoint: the solve ran and reconciled the violating write.
    let (arc_center, arc_radius) = arc_geom(&scene, arc);
    assert!(
        (arc_center.y - 18.0).abs() > 0.1,
        "a violated write must actually be solved, not held: {arc_center:?}"
    );

    // And the reconciliation leaves the graph satisfied again.
    let line1_end = line_endpoints(&scene, line1).1;
    let arc_start = {
        match scene.document.get_entity(arc).expect("entity exists") {
            EntityType::Arc(a) => a.start_point(),
            other => panic!("expected an Arc, got {other:?}"),
        }
    };
    assert!(
        (arc_start.x - line1_end.x).abs() < 1e-6 && (arc_start.y - line1_end.y).abs() < 1e-6,
        "the joint must be re-made: line1_end={line1_end:?} arc_start={arc_start:?}"
    );
    let line2_start = line_endpoints(&scene, line2).0;
    let arc_end = {
        match scene.document.get_entity(arc).expect("entity exists") {
            EntityType::Arc(a) => a.end_point(),
            other => panic!("expected an Arc, got {other:?}"),
        }
    };
    assert!(
        (arc_end.x - line2_start.x).abs() < 1e-6 && (arc_end.y - line2_start.y).abs() < 1e-6,
        "the second joint must hold: line2_start={line2_start:?} arc_end={arc_end:?}"
    );
    // Perpendicular radius to each leg at the shared joint means the
    // infinite line's distance from the center equals the radius again.
    for (leg, (p1, p2)) in [
        ("line1", line_endpoints(&scene, line1)),
        ("line2", line_endpoints(&scene, line2)),
    ] {
        let dx = p2.x - p1.x;
        let dy = p2.y - p1.y;
        let length = (dx * dx + dy * dy).sqrt();
        let signed = (dx * (arc_center.y - p1.y) - dy * (arc_center.x - p1.x)) / length;
        assert!(
            (signed.abs() - arc_radius).abs() < 1e-6,
            "{leg} must be tangent to the arc's radius again: dist={signed} radius={arc_radius}"
        );
    }
    // The marker pins re-hold too.
    let (start_marker_center, _) = circle_geom(&scene, start_marker);
    let (end_marker_center, _) = circle_geom(&scene, end_marker);
    let line1_start = line_endpoints(&scene, line1).0;
    let line2_end = line_endpoints(&scene, line2).1;
    assert!(
        (start_marker_center.x - line1_start.x).abs() < 1e-6
            && (start_marker_center.y - line1_start.y).abs() < 1e-6,
        "the start marker pin re-holds: marker={start_marker_center:?} line1_start={line1_start:?}"
    );
    assert!(
        (end_marker_center.x - line2_end.x).abs() < 1e-6
            && (end_marker_center.y - line2_end.y).abs() < 1e-6,
        "the end marker pin re-holds: marker={end_marker_center:?} line2_end={line2_end:?}"
    );
}

/// The fully consistent bonded-chain layout for a corner at `b` (the outer
/// PIs `a`/`c` stay the chain's endpoints, `radius` the curve R) on one of
/// the two tangency branches: the wedge-inside (near) reading, or the far
/// side. Both readings satisfy the same constraints - the flip above wrote
/// one branch of a 90-degree corner (mirrored across leg 1, whose legs are
/// perpendicular, so the mirror keeps both leg lines); a CORNER TURNED
/// through a general move (the plugin's crossing write: the PI dragged
/// across the turn-sense line, every element re-derived from the moved
/// corner) needs the far branch derived from the new legs instead. The far
/// branch = the excenter across the wedge bisector: both tangent points
/// land beyond the corner and the arc sweeps the complement 2*pi - |delta|,
/// the same corner_long reading the records model carries.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ChainLayout {
    line1: [(f64, f64); 2],
    arc_center: (f64, f64),
    arc_start_angle: f64,
    arc_end_angle: f64,
    line2: [(f64, f64); 2],
}

fn chain_layout(
    a: (f64, f64),
    c: (f64, f64),
    b: (f64, f64),
    radius: f64,
    far: bool,
) -> ChainLayout {
    let (dx1, dy1) = (a.0 - b.0, a.1 - b.1);
    let l1 = (dx1 * dx1 + dy1 * dy1).sqrt();
    let u1 = (dx1 / l1, dy1 / l1);
    let (dx2, dy2) = (c.0 - b.0, c.1 - b.1);
    let l2 = (dx2 * dx2 + dy2 * dy2).sqrt();
    let u2 = (dx2 / l2, dy2 / l2);
    let wedge_cos = u1.0 * u2.0 + u1.1 * u2.1;
    let cos_half = ((1.0 + wedge_cos) / 2.0).sqrt();
    let sin_half = ((1.0 - wedge_cos) / 2.0).sqrt();
    // T = R*tan(half-wedge) is the same R*tan(|delta|/2) the consumer's
    // derivation uses (half-wedge = (pi - |delta|)/2).
    let t = radius * cos_half / sin_half;
    let d = radius / sin_half;
    let (wx, wy) = (u1.0 + u2.0, u1.1 + u2.1);
    let wl = (wx * wx + wy * wy).sqrt();
    let w = (wx / wl, wy / wl);
    let sign = if far { -1.0 } else { 1.0 };
    let p1 = (b.0 + sign * t * u1.0, b.1 + sign * t * u1.1);
    let p2 = (b.0 + sign * t * u2.0, b.1 + sign * t * u2.1);
    let center = (b.0 + sign * d * w.0, b.1 + sign * d * w.1);
    // Signed deflection from the incoming (b - a) direction to the outgoing
    // (c - b) one.
    let u_in = (-u1.0, -u1.1);
    let mut deflection =
        (u2.1.atan2(u2.0)) - (u_in.1.atan2(u_in.0));
    while deflection > std::f64::consts::PI {
        deflection -= std::f64::consts::TAU;
    }
    while deflection < -std::f64::consts::PI {
        deflection += std::f64::consts::TAU;
    }
    let start_angle = (p1.1 - center.1).atan2(p1.0 - center.0);
    // The arc body must sit on the branch's side: the wedge-inside short
    // sweep reading signed, or the complement sweep through the far bulge.
    let sweep = if far {
        std::f64::consts::TAU - deflection.abs()
    } else {
        deflection
    };
    ChainLayout {
        line1: [(a.0, a.1), (p1.0, p1.1)],
        arc_center: center,
        arc_start_angle: start_angle,
        arc_end_angle: start_angle + sweep,
        line2: [(p2.0, p2.1), (c.0, c.1)],
    }
}

// The transaction-route crossing write: the civil plugin moves a bonded PI
// ACROSS the corner's turn-sense line and rewrites the WHOLE chain in ONE
// batch through update_entities_plugin - legs, arc body, markers - all
// derived from the moved corner (ZIG: PIs (0,0)/(60,0)/(60,40), R=15 at the
// middle corner; the live crossing case, with the corner at (-11.63, 69.84),
// R=15). The written state satisfies every constraint of the chain in its
// own far-side reading, so the bump it triggers must hold it exactly. The
// corner PI marker is unwritten-pinned (refresh-positioned) exactly as in
// add_bonded_chain_graph; its written position is asserted to hold.
#[test]
fn a_crossing_far_side_write_through_the_plugin_route_is_not_displaced() {
    let mut scene = Scene::new();
    let a = (0.0f64, 0.0f64);
    let c = (60.0f64, 40.0f64);
    let b0 = (60.0f64, 0.0f64);
    let b1 = (-11.63f64, 69.84f64);
    let radius = 15.0f64;
    const HOLD: f64 = 1.0e-6; // the flip pin's own no-op scale; a re-projection lands at 2*R ~ 30

    // Baseline: the near-side graph, held by its own first bump like the
    // flip pin's first leg.
    let near = chain_layout(a, c, b0, radius, false);
    let line1 = add_line(&mut scene, near.line1[0].0, near.line1[0].1, near.line1[1].0, near.line1[1].1);
    let arc = add_arc(
        &mut scene,
        near.arc_center.0,
        near.arc_center.1,
        radius,
        near.arc_start_angle,
        near.arc_end_angle,
    );
    let line2 = add_line(&mut scene, near.line2[0].0, near.line2[0].1, near.line2[1].0, near.line2[1].1);
    let start_marker = add_circle(&mut scene, a.0, a.1, 2.0);
    let middle_marker = add_circle(&mut scene, b0.0, b0.1, 2.0);
    let end_marker = add_circle(&mut scene, c.0, c.1, 2.0);
    // The exact constraint set add_bonded_chain_graph builds.
    let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
    set.add(
        ConstraintKind::Coincident,
        vec![ParametricRef::center(start_marker), ParametricRef::point(line1, 0)],
        None,
    );
    set.add(
        ConstraintKind::Coincident,
        vec![ParametricRef::point(line1, 1), ParametricRef::point(arc, 0)],
        None,
    );
    set.add(
        ConstraintKind::Tangent,
        vec![ParametricRef::whole(line1), ParametricRef::whole(arc)],
        None,
    );
    set.add(
        ConstraintKind::Coincident,
        vec![ParametricRef::point(arc, 1), ParametricRef::point(line2, 0)],
        None,
    );
    set.add(
        ConstraintKind::Tangent,
        vec![ParametricRef::whole(arc), ParametricRef::whole(line2)],
        None,
    );
    set.add(
        ConstraintKind::Coincident,
        vec![ParametricRef::center(end_marker), ParametricRef::point(line2, 1)],
        None,
    );
    let _ = set;
    scene.bump_entities(&[
        (line1, ChangeKind::Modified),
        (arc, ChangeKind::Modified),
        (line2, ChangeKind::Modified),
        (start_marker, ChangeKind::Modified),
        (middle_marker, ChangeKind::Modified),
        (end_marker, ChangeKind::Modified),
    ]);
    let (arc_center, _) = arc_geom(&scene, arc);
    assert!(
        (arc_center.x - near.arc_center.0).abs() < HOLD
            && (arc_center.y - near.arc_center.1).abs() < HOLD,
        "baseline: the near-side graph should hold exactly: {arc_center:?} vs {:?}",
        near.arc_center
    );

    // The crossing write: the same graph re-derived for the corner at b1 on
    // the FAR branch, all two joints consistent, both tangencies exact in
    // their own (far-side) reading. Construction residuals are verified
    // BEFORE any solver behavior is asserted, so a failure here is a
    // construction bug, never a solver verdict.
    let far = chain_layout(a, c, b1, radius, true);
    let (mut written_arc, mut written_line2) = match (
        scene.document.get_entity(arc),
        scene.document.get_entity(line2),
    ) {
        (Some(EntityType::Arc(a)), Some(EntityType::Line(l))) => (a.clone(), l.clone()),
        _ => panic!("expected an Arc and a Line"),
    };
    written_arc.center = Vector3::new(far.arc_center.0, far.arc_center.1, 0.0);
    written_arc.start_angle = far.arc_start_angle;
    written_arc.end_angle = far.arc_end_angle;
    written_line2.start = Vector3::new(far.line2[0].0, far.line2[0].1, 0.0);
    written_line2.end = Vector3::new(far.line2[1].0, far.line2[1].1, 0.0);
    let mut written_line1 = match scene.document.get_entity(line1) {
        Some(EntityType::Line(l)) => l.clone(),
        other => panic!("expected a Line, got {other:?}"),
    };
    written_line1.end = Vector3::new(far.line1[1].0, far.line1[1].1, 0.0);
    let mut written_middle = match scene.document.get_entity(middle_marker) {
        Some(EntityType::Circle(m)) => m.clone(),
        other => panic!("expected a Circle, got {other:?}"),
    };
    written_middle.center = Vector3::new(b1.0, b1.1, 0.0);
    // construction checks: the far branch must be exact on its own reading.
    {
        let arc_start = written_arc.start_point();
        let arc_end = written_arc.end_point();
        assert!(
            (arc_start.x - far.line1[1].0).abs() < 1.0e-9
                && (arc_start.y - far.line1[1].1).abs() < 1.0e-9,
            "construction: arc start = leg 1's end: {arc_start:?} vs {:?}",
            far.line1[1]
        );
        assert!(
            (arc_end.x - far.line2[0].0).abs() < 1.0e-9 && (arc_end.y - far.line2[0].1).abs() < 1.0e-9,
            "construction: arc end = leg 2's start: {arc_end:?} vs {:?}",
            far.line2[0]
        );
        let (ux, uy) = (
            far.line1[1].0 - far.line1[0].0,
            far.line1[1].1 - far.line1[0].1,
        );
        let leg1_len = (ux * ux + uy * uy).sqrt();
        let h1 = (ux * (far.arc_center.1 - far.line1[0].1) - uy * (far.arc_center.0 - far.line1[0].0))
            / leg1_len;
        assert!(
            (h1.abs() - radius).abs() < 1.0e-9,
            "construction: leg 1's line must carry the arc's radius: h1={h1}"
        );
        let (vx, vy) = (
            far.line2[1].0 - far.line2[0].0,
            far.line2[1].1 - far.line2[0].1,
        );
        let leg2_len = (vx * vx + vy * vy).sqrt();
        let h2 = (vx * (far.arc_center.1 - far.line2[0].1) - vy * (far.arc_center.0 - far.line2[0].0))
            / leg2_len;
        assert!(
            (h2.abs() - radius).abs() < 1.0e-9,
            "construction: leg 2's line must carry the arc's radius: h2={h2}"
        );
        // THE CROSSING, per the plugin's own op definition: the moved corner
        // and the written arc's body sit on the OPPOSITE side of the chord
        // (the line through the neighbour PIs a..c = the corner's turn-sense
        // line) from the initial corner and its near-side body. The bodies
        // are each arc's sweep midpoint; the legs reorient, so per-leg
        // crosses do not compare - the chord side does.
        let chord_side = |point: (f64, f64)| {
            let (dx, dy) = (c.0 - a.0, c.1 - a.1);
            let l = (dx * dx + dy * dy).sqrt();
            (dx * (point.1 - a.1) - dy * (point.0 - a.0)) / l
        };
        assert!(
            chord_side(b0) * chord_side(b1) < 0.0,
            "construction: the corner must cross the turn-sense line: {} vs {}",
            chord_side(b0),
            chord_side(b1)
        );
        let body_mid = |branch: &ChainLayout| {
            // the sweep midpoint's polar direction from the arc center
            let mid_angle = branch.arc_start_angle + (branch.arc_end_angle - branch.arc_start_angle) / 2.0;
            (
                branch.arc_center.0 + radius * mid_angle.cos(),
                branch.arc_center.1 + radius * mid_angle.sin(),
            )
        };
        assert!(
            chord_side(body_mid(&near)) * chord_side(b0) > 0.0
                && chord_side(body_mid(&far)) * chord_side(b1) > 0.0,
            "construction: the two bodies must straddle the turn-sense line: near-vs-b0 {} far-vs-b1 {}",
            chord_side(body_mid(&near)) * chord_side(b0),
            chord_side(body_mid(&far)) * chord_side(b1)
        );
        // the arc's complement sweep (> pi) is the far-body encoding.
        let mut sweep = far.arc_end_angle - far.arc_start_angle;
        sweep = sweep.rem_euclid(std::f64::consts::TAU);
        assert!(
            sweep > std::f64::consts::PI,
            "construction: the far write's sweep must be the complement: {sweep}"
        );
    }
    // The plugin's write route: ONE batched write, one bump over the final
    // state (update_entities_transaction batching comment: the solver sees
    // every entity at once).
    let moved = scene.update_entities_plugin(vec![
        EntityType::Line(written_line1),
        EntityType::Arc(written_arc),
        EntityType::Line(written_line2),
        EntityType::Circle(written_middle),
    ]);
    assert_eq!(moved, 4);

    // R1: the crossing write is a fixpoint of the scope - zero residual in
    // its own far-side reading - so the bump must keep the written values
    // exactly and keep the bulge side: never a re-projection between the two
    // readings (that one lands the arc within the 2*R(1-cos(delta/2)) band,
    // the mirrored-arc-body signature the live crossing run measured).
    let check_held = |name: &str, actually: (f64, f64), wanted: (f64, f64)| {
        assert!(
            (actually.0 - wanted.0).abs() < HOLD && (actually.1 - wanted.1).abs() < HOLD,
            "{name} must keep the written far-side value {wanted:?}, displaced to {actually:?}"
        );
    };
    let (line1_start, line1_end) = line_endpoints(&scene, line1);
    check_held("line 1 start", (line1_start.x, line1_start.y), far.line1[0]);
    check_held("line 1 end (the written joint)", (line1_end.x, line1_end.y), far.line1[1]);
    let (arc_center, arc_radius) = arc_geom(&scene, arc);
    check_held("arc center", (arc_center.x, arc_center.y), far.arc_center);
    assert!(
        (arc_radius - radius).abs() < HOLD,
        "written radius must hold: {arc_radius}"
    );
    let (line2_start, line2_end) = line_endpoints(&scene, line2);
    check_held("line 2 start (the written joint)", (line2_start.x, line2_start.y), far.line2[0]);
    check_held("line 2 end", (line2_end.x, line2_end.y), far.line2[1]);
    // The written sweep (the complement, the far-body ordering) survives:
    // the angles are held modulo the run's 2*pi lane drifts.
    {
        let angles = match scene.document.get_entity(arc) {
            Some(EntityType::Arc(a)) => (a.start_angle, a.end_angle),
            other => panic!("expected an Arc, got {other:?}"),
        };
        let (mut written, mut read) = (far.arc_end_angle - far.arc_start_angle, angles.1 - angles.0);
        written = written.rem_euclid(std::f64::consts::TAU);
        read = read.rem_euclid(std::f64::consts::TAU);
        assert!(
            (written - read).abs() < HOLD,
            "the far sweep ordering must survive the bump: written {written} read-back {read}"
        );
    }
    let (start_marker_center, _) = circle_geom(&scene, start_marker);
    check_held("start marker", (start_marker_center.x, start_marker_center.y), a);
    let (end_marker_center, _) = circle_geom(&scene, end_marker);
    check_held("end marker", (end_marker_center.x, end_marker_center.y), c);
    let (middle_center, _) = circle_geom(&scene, middle_marker);
    check_held("corner PI marker (refresh-positioned, unwritten)", (middle_center.x, middle_center.y), b1);
    // The bulge side: same signed cross as the construction check.
    {
        let (sx, sy) = (
            line1_end.x - line1_start.x,
            line1_end.y - line1_start.y,
        );
        let side_now = sx * (arc_center.y - line1_start.y) - sy * (arc_center.x - line1_start.x);
        assert!(
            (side_now - {
                let (sx0, sy0) = (
                    far.line1[1].0 - far.line1[0].0,
                    far.line1[1].1 - far.line1[0].1,
                );
                sx0 * (far.arc_center.1 - far.line1[0].1) - sy0 * (far.arc_center.0 - far.line1[0].0)
            })
            .abs()
                < 1.0e-6,
            "the arc's bulge side must stay on the written far reading: cross={side_now}"
        );
    }
}
// A consistent state nudged by an adversarially small amount (deep inside
// the solver's own 1e-10 convergence tolerance) still short-circuits: no
// iteration, no drift - the nudged coordinate survives bit-for-bit instead
// of being massaged, and nothing else creeps.
#[test]
fn a_within_tolerance_nudge_of_a_consistent_write_accumulates_no_drift() {
    let mut scene = Scene::new();
    let (line1, arc, line2, start_marker, end_marker) = add_bonded_chain_graph(&mut scene);

    scene.bump_entities(&[
        (line1, ChangeKind::Modified),
        (arc, ChangeKind::Modified),
        (line2, ChangeKind::Modified),
        (start_marker, ChangeKind::Modified),
        (end_marker, ChangeKind::Modified),
    ]);

    // A 1e-12 shift on one coordinate of the consistent chain - far inside
    // the solver's convergence tolerance (every row stays below 1e-10).
    if let Some(EntityType::Arc(a)) = scene.document.get_entity_mut(arc) {
        a.center.x = -15.0 + 1.0e-12;
    }
    scene.bump_entities(&[(arc, ChangeKind::Modified)]);

    // The nudged coordinate kept bit-exactly: the solve was a no-op, so no
    // iteration could massage the low bits or creep anywhere else.
    match scene.document.get_entity(arc).expect("entity exists") {
        EntityType::Arc(a) => {
            assert_eq!(
                a.center.x,
                -15.0 + 1.0e-12,
                "the nudge must survive bit-for-bit"
            );
            assert!(
                (a.center.y - 15.0).abs() < 1e-15,
                "no drift on the other coordinates"
            );
        }
        other => panic!("expected an Arc, got {other:?}"),
    }
    let (line2_start, line2_end) = line_endpoints(&scene, line2);
    assert!(
        (line2_start.x - 0.0).abs() < 1e-15,
        "no drift: {line2_start:?}"
    );
    assert!(
        (line2_start.y - 15.0).abs() < 1e-15,
        "no drift: {line2_start:?}"
    );
    assert!(
        (line2_end.y - 45.0).abs() < 1e-15,
        "no drift: {line2_end:?}"
    );
}
