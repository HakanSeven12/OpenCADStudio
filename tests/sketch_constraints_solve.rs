// Stage 3 of docs/parametric_system_design.md's staged plan:
// `refresh_sketch_constraints` wired into `Scene::bump_entities` (§4.1),
// exercised against an in-memory-only `SketchConstraintSet` (no save/load —
// that's stage 4). Verifies: an edit to one constrained entity moves its
// constrained neighbors, produces the expected change list, and an
// unrelated edit doesn't touch anything (§4.3's "what does NOT trigger").

use OpenCADStudio::scene::named_parameters::DrivingValue;
use OpenCADStudio::scene::sketch_constraints::{ConstraintKind, SketchRef, SketchScope};
use OpenCADStudio::scene::{ChangeKind, Scene};
use acadrust::entities::EntityType;
use acadrust::types::{Handle, Vector3};

fn add_line(scene: &mut Scene, x1: f64, y1: f64, x2: f64, y2: f64) -> Handle {
    scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
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
    scene.add_entity(EntityType::Circle(acadrust::entities::Circle::from_center_radius(Vector3::new(cx, cy, 0.0), radius)))
}

fn circle_geom(scene: &Scene, handle: Handle) -> (Vector3, f64) {
    match scene.document.get_entity(handle).expect("entity exists") {
        EntityType::Circle(c) => (c.center, c.radius),
        other => panic!("expected a Circle, got {other:?}"),
    }
}

fn add_arc(scene: &mut Scene, cx: f64, cy: f64, radius: f64, start_angle: f64, end_angle: f64) -> Handle {
    scene.add_entity(EntityType::Arc(acadrust::entities::Arc::from_center_radius_angles(
        Vector3::new(cx, cy, 0.0),
        radius,
        start_angle,
        end_angle,
    )))
}

fn arc_geom(scene: &Scene, handle: Handle) -> (Vector3, f64) {
    match scene.document.get_entity(handle).expect("entity exists") {
        EntityType::Arc(a) => (a.center, a.radius),
        other => panic!("expected an Arc, got {other:?}"),
    }
}

fn arc_angles(scene: &Scene, handle: Handle) -> (f64, f64, f64) {
    match scene.document.get_entity(handle).expect("entity exists") {
        EntityType::Arc(a) => (a.radius, a.start_angle, a.end_angle),
        other => panic!("expected an Arc, got {other:?}"),
    }
}

fn add_ellipse(scene: &mut Scene, cx: f64, cy: f64, major_axis: (f64, f64), ratio: f64) -> Handle {
    scene.add_entity(EntityType::Ellipse(acadrust::entities::Ellipse::from_center_axes(
        Vector3::new(cx, cy, 0.0),
        Vector3::new(major_axis.0, major_axis.1, 0.0),
        ratio,
    )))
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

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(ConstraintKind::Horizontal, vec![SketchRef::whole(line)], None);

    // Drag the endpoint off-axis — a real edit, the kind of thing a Move
    // command or grip-drag commit would do.
    set_line_end(&mut scene, line, Vector3::new(10.0, 4.0, 0.0));
    scene.bump_entities(&[(line, ChangeKind::Modified)]);

    let (start, end) = line_endpoints(&scene, line);
    assert!((start.y - end.y).abs() < 1e-6, "line should have been re-leveled: start={start:?} end={end:?}");

    // A line is 4 raw coordinates; Horizontal removes exactly 1 (y1 == y2),
    // leaving 3 — not 1: the two X coordinates are registered but never
    // referenced by any constraint, so they must still count as free
    // (regression guard for the undercounting `solve_scope` bug found while
    // live-testing this stage: `System::partition`'s subsystems only see
    // constraint-referenced params, so an untouched one was invisible to
    // `diagnose` entirely before this was fixed).
    let dof = scene.sketch_constraint_set(SketchScope::ModelSpace).and_then(|s| s.dof);
    assert_eq!(dof, Some(3), "DOF should count the two untouched X coordinates too");
}

#[test]
fn parallel_constraint_rotates_the_other_line_when_one_moves() {
    let mut scene = Scene::new();
    let a = add_line(&mut scene, 0.0, 0.0, 10.0, 0.0);
    let b = add_line(&mut scene, 0.0, 5.0, 10.0, 5.0);

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(ConstraintKind::Parallel, vec![SketchRef::whole(a), SketchRef::whole(b)], None);

    // Rotate `a`; `b` (parallel to it) should follow.
    set_line_end(&mut scene, a, Vector3::new(10.0, 6.0, 0.0));
    scene.bump_entities(&[(a, ChangeKind::Modified)]);

    let (a1, a2) = line_endpoints(&scene, a);
    let (b1, b2) = line_endpoints(&scene, b);
    let dir_a = ((a2.x - a1.x), (a2.y - a1.y));
    let dir_b = ((b2.x - b1.x), (b2.y - b1.y));
    // Parallel: cross product of directions is ~0.
    let cross = dir_a.0 * dir_b.1 - dir_a.1 * dir_b.0;
    assert!(cross.abs() < 1e-6, "lines should be parallel after the solve: dir_a={dir_a:?} dir_b={dir_b:?}");
}

#[test]
fn distance_constraint_holds_the_target_length_after_an_unrelated_endpoint_edit() {
    let mut scene = Scene::new();
    let line = add_line(&mut scene, 0.0, 0.0, 10.0, 0.0);

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
        ConstraintKind::Distance,
        vec![SketchRef::point(line, 0), SketchRef::point(line, 1)],
        Some(DrivingValue::Literal(20.0)),
    );

    set_line_end(&mut scene, line, Vector3::new(5.0, 5.0, 0.0)); // arbitrary edit, not length=20
    scene.bump_entities(&[(line, ChangeKind::Modified)]);

    let (start, end) = line_endpoints(&scene, line);
    let len = ((end.x - start.x).powi(2) + (end.y - start.y).powi(2)).sqrt();
    assert!((len - 20.0).abs() < 1e-6, "length should have been solved to the driving value, got {len}");
}

/// `named_parameters_design.md` stage 3: a `Distance` constraint's driving
/// value can be a named-parameter reference instead of a literal, resolved
/// through `Scene::named_parameters` at solve time.
#[test]
fn distance_constraint_resolves_a_named_parameter_reference() {
    let mut scene = Scene::new();
    let line = add_line(&mut scene, 0.0, 0.0, 10.0, 0.0);
    scene.named_parameters_mut().set("target_len", "20").unwrap();

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
        ConstraintKind::Distance,
        vec![SketchRef::point(line, 0), SketchRef::point(line, 1)],
        Some(DrivingValue::Named("target_len".to_string())),
    );

    set_line_end(&mut scene, line, Vector3::new(5.0, 5.0, 0.0)); // arbitrary edit, not length=20
    scene.bump_entities(&[(line, ChangeKind::Modified)]);

    let (start, end) = line_endpoints(&scene, line);
    let len = ((end.x - start.x).powi(2) + (end.y - start.y).powi(2)).sqrt();
    assert!((len - 20.0).abs() < 1e-6, "length should have resolved the named parameter, got {len}");

    // Editing the parameter itself and touching the scope again should
    // ripple through, same as editing a literal driving value would.
    scene.named_parameters_mut().set("target_len", "8").unwrap();
    scene.bump_entities(&[(line, ChangeKind::Modified)]);
    let (start, end) = line_endpoints(&scene, line);
    let len = ((end.x - start.x).powi(2) + (end.y - start.y).powi(2)).sqrt();
    assert!((len - 8.0).abs() < 1e-6, "length should track the redefined parameter value, got {len}");
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

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
        ConstraintKind::Distance,
        vec![SketchRef::point(line, 0), SketchRef::point(line, 1)],
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

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(ConstraintKind::Tangent, vec![SketchRef::whole(circle), SketchRef::whole(line)], None);
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

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(ConstraintKind::Tangent, vec![SketchRef::whole(a), SketchRef::whole(b)], None);
    scene.bump_entities(&[(a, ChangeKind::Modified), (b, ChangeKind::Modified)]);

    let (ca, ra) = circle_geom(&scene, a);
    let (cb, rb) = circle_geom(&scene, b);
    let center_dist = ((ca.x - cb.x).powi(2) + (ca.y - cb.y).powi(2)).sqrt();
    assert!((center_dist - (ra + rb)).abs() < 1e-6, "circles should sit exactly (r1+r2) apart after solving: dist={center_dist} r1+r2={}", ra + rb);
}

#[test]
fn coincident_constraint_pulls_the_second_point_onto_the_first_when_it_moves() {
    let mut scene = Scene::new();
    let a = add_line(&mut scene, 0.0, 0.0, 5.0, 0.0);
    let b = add_line(&mut scene, 5.0, 0.0, 5.0, 5.0); // b.start coincident with a.end

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
        ConstraintKind::Coincident,
        vec![SketchRef::point(a, 1), SketchRef::point(b, 0)],
        None,
    );

    // Move a's end away — b's start should follow to stay coincident.
    set_line_end(&mut scene, a, Vector3::new(8.0, 3.0, 0.0));
    scene.bump_entities(&[(a, ChangeKind::Modified)]);

    let (_, a_end) = line_endpoints(&scene, a);
    let (b_start, _) = line_endpoints(&scene, b);
    assert!((a_end.x - b_start.x).abs() < 1e-6 && (a_end.y - b_start.y).abs() < 1e-6, "a.end={a_end:?} b.start={b_start:?}");
}

#[test]
fn unrelated_entity_edit_does_not_touch_constrained_geometry() {
    let mut scene = Scene::new();
    let line = add_line(&mut scene, 0.0, 0.0, 10.0, 3.0); // deliberately not horizontal
    let unrelated = add_line(&mut scene, 100.0, 100.0, 200.0, 100.0);

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(ConstraintKind::Horizontal, vec![SketchRef::whole(line)], None);

    let (start_before, end_before) = line_endpoints(&scene, line);
    set_line_end(&mut scene, unrelated, Vector3::new(200.0, 150.0, 0.0));
    scene.bump_entities(&[(unrelated, ChangeKind::Modified)]);

    let (start_after, end_after) = line_endpoints(&scene, line);
    assert_eq!(start_before, start_after, "unrelated edit must not trigger a re-solve of unrelated geometry");
    assert_eq!(end_before, end_after);
}

#[test]
fn erasing_a_constrained_entity_drops_its_constraints_instead_of_dangling() {
    // Design doc §5.3/§12 (open question 4): deletion policy is "silently
    // drop", matching FreeCAD, rather than leaving a `SketchRef` pointing
    // at a handle that no longer resolves.
    let mut scene = Scene::new();
    let a = add_line(&mut scene, 0.0, 0.0, 10.0, 0.0);
    let b = add_line(&mut scene, 0.0, 5.0, 10.0, 5.0);
    let c = add_line(&mut scene, 20.0, 20.0, 30.0, 20.0); // unrelated, own constraint

    let parallel_id =
        scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(ConstraintKind::Parallel, vec![SketchRef::whole(a), SketchRef::whole(b)], None);
    let horizontal_id = scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(ConstraintKind::Horizontal, vec![SketchRef::whole(c)], None);

    scene.erase_entities(&[a]);

    let set = scene.sketch_constraint_set(SketchScope::ModelSpace).expect("scope still exists");
    assert!(set.get(parallel_id).is_none(), "the Parallel constraint referencing the erased line must be gone");
    assert!(set.get(horizontal_id).is_some(), "an unrelated constraint on a different entity must survive");

    // And the surviving constraint set still functions normally afterward.
    set_line_end(&mut scene, c, Vector3::new(30.0, 25.0, 0.0));
    scene.bump_entities(&[(c, ChangeKind::Modified)]);
    let (start, end) = line_endpoints(&scene, c);
    assert!((start.y - end.y).abs() < 1e-6, "remaining Horizontal constraint should still solve: start={start:?} end={end:?}");
}

#[test]
fn copying_two_constrained_entities_carries_their_constraint_along() {
    // Design doc §5.3/§12 (open question 3, resolved): a constraint entirely
    // between the copied entities should follow the copy.
    let mut scene = Scene::new();
    let a = add_line(&mut scene, 0.0, 0.0, 10.0, 0.0);
    let b = add_line(&mut scene, 0.0, 5.0, 10.0, 3.0); // deliberately not parallel yet
    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(ConstraintKind::Parallel, vec![SketchRef::whole(a), SketchRef::whole(b)], None);
    // Solve once so the source pair is actually parallel before copying.
    scene.bump_entities(&[(a, ChangeKind::Modified), (b, ChangeKind::Modified)]);
    let constraints_before = scene.sketch_constraint_set(SketchScope::ModelSpace).unwrap().constraints.len();

    let transform = OpenCADStudio::command::EntityTransform::Translate(glam::DVec3::new(100.0, 100.0, 0.0));
    let new_handles = scene.copy_entities(&[a, b], &transform);
    assert_eq!(new_handles.len(), 2);
    let (new_a, new_b) = (new_handles[0], new_handles[1]);

    let constraints_after = scene.sketch_constraint_set(SketchScope::ModelSpace).unwrap().constraints.len();
    assert_eq!(constraints_after, constraints_before + 1, "the copy should have gained exactly one new Parallel constraint");

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
    assert!(ocross.abs() < 1e-6, "the original pair should remain parallel too");
}

// The "one edit that ripples through a constraint still records as one undo
// step" case needs `Scene::record_undo_before`, which is `pub(crate)` (an
// integration test crate can't reach it) — covered instead as an internal
// unit test in `src/scene/sketch_solve.rs`.

#[test]
fn a_duplicated_horizontal_constraint_is_reported_as_redundant() {
    // Design doc §6.4 (stage 11): `solve_scope` classifies the redundant row
    // `ocs_gcs::diagnosis::diagnose` flags and resolves it back to the
    // `ConstraintId` a `ConflictResolverPanel` would name.
    use OpenCADStudio::scene::sketch_constraints::ConstraintKind;
    use ocs_gcs::diagnosis::RedundancyKind;

    let mut scene = Scene::new();
    let a = add_line(&mut scene, 0.0, 0.0, 10.0, 3.0);
    let set = scene.sketch_constraint_set_mut(SketchScope::ModelSpace);
    let first = set.add(ConstraintKind::Horizontal, vec![SketchRef::whole(a)], None);
    let second = set.add(ConstraintKind::Horizontal, vec![SketchRef::whole(a)], None); // exact duplicate

    scene.bump_entities(&[(a, ChangeKind::Modified)]);

    let set = scene.sketch_constraint_set(SketchScope::ModelSpace).unwrap();
    assert_eq!(set.conflicts.len(), 1, "exactly one of the two identical constraints should be flagged");
    let (flagged_id, kind) = set.conflicts[0];
    assert!(flagged_id == first || flagged_id == second, "the flagged id should be one of the two duplicates");
    assert_eq!(kind, RedundancyKind::Redundant);
}

#[test]
fn two_conflicting_distance_targets_are_reported_as_conflicting() {
    use OpenCADStudio::scene::sketch_constraints::ConstraintKind;
    use ocs_gcs::diagnosis::RedundancyKind;

    let mut scene = Scene::new();
    let a = add_line(&mut scene, 0.0, 0.0, 10.0, 0.0);
    let set = scene.sketch_constraint_set_mut(SketchScope::ModelSpace);
    let first = set.add(ConstraintKind::Distance, vec![SketchRef::point(a, 0), SketchRef::point(a, 1)], Some(DrivingValue::Literal(20.0)));
    let second = set.add(ConstraintKind::Distance, vec![SketchRef::point(a, 0), SketchRef::point(a, 1)], Some(DrivingValue::Literal(50.0)));

    scene.bump_entities(&[(a, ChangeKind::Modified)]);

    let set = scene.sketch_constraint_set(SketchScope::ModelSpace).unwrap();
    assert_eq!(set.conflicts.len(), 1);
    let (flagged_id, kind) = set.conflicts[0];
    assert!(flagged_id == first || flagged_id == second, "the flagged id should be one of the two conflicting Distance constraints");
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

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
        ConstraintKind::Concentric,
        vec![SketchRef::center(a), SketchRef::center(b)],
        None,
    );

    set_circle_center(&mut scene, a, Vector3::new(2.0, -3.0, 0.0));
    scene.bump_entities(&[(a, ChangeKind::Modified)]);

    let (ca, ra) = circle_geom(&scene, a);
    let (cb, rb) = circle_geom(&scene, b);
    assert!((ca.x - cb.x).abs() < 1e-6 && (ca.y - cb.y).abs() < 1e-6, "centers should coincide: a={ca:?} b={cb:?}");
    // Radii are untouched by Concentric — only centers move.
    assert!((ra - 3.0).abs() < 1e-6 && (rb - 1.0).abs() < 1e-6, "radii must not change: ra={ra} rb={rb}");
}

#[test]
fn center_point_constraint_pulls_a_lines_endpoint_onto_a_circles_center() {
    let mut scene = Scene::new();
    let circle = add_circle(&mut scene, 0.0, 0.0, 4.0);
    let line = add_line(&mut scene, 10.0, 10.0, 20.0, 20.0);

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
        ConstraintKind::CenterPoint,
        vec![SketchRef::point(line, 0), SketchRef::center(circle)],
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

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
        ConstraintKind::Colinear,
        vec![SketchRef::whole(a), SketchRef::whole(b)],
        None,
    );

    scene.bump_entities(&[(b, ChangeKind::Modified)]);

    let (a1, a2) = line_endpoints(&scene, a);
    let (b1, b2) = line_endpoints(&scene, b);
    // Both of b's endpoints should land on a's infinite line: the signed
    // area of (a2-a1) × (bN-a1) is ~0 for a point exactly on that line.
    let area = |p: Vector3| (a2.x - a1.x) * (p.y - a1.y) - (a2.y - a1.y) * (p.x - a1.x);
    assert!(area(b1).abs() < 1e-5, "b.start should land on a's line, area={}", area(b1));
    assert!(area(b2).abs() < 1e-5, "b.end should land on a's line, area={}", area(b2));
}

#[test]
fn midpoint_constraint_pulls_a_point_onto_a_lines_midpoint() {
    let mut scene = Scene::new();
    let base = add_line(&mut scene, 0.0, 0.0, 10.0, 0.0);
    let marker = add_line(&mut scene, 20.0, 20.0, 21.0, 21.0); // marker.start is the tracked point

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
        ConstraintKind::Midpoint,
        vec![SketchRef::point(marker, 0), SketchRef::whole(base)],
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
fn fixed_constraint_holds_an_entity_in_place_despite_a_connected_edit() {
    let mut scene = Scene::new();
    let fixed_line = add_line(&mut scene, 0.0, 0.0, 10.0, 0.0);
    let moving_line = add_line(&mut scene, 10.0, 0.0, 10.0, 10.0);

    let set = scene.sketch_constraint_set_mut(SketchScope::ModelSpace);
    set.add(ConstraintKind::Fixed, vec![SketchRef::whole(fixed_line)], None);
    set.add(ConstraintKind::Coincident, vec![SketchRef::point(fixed_line, 1), SketchRef::point(moving_line, 0)], None);

    let (before_start, before_end) = line_endpoints(&scene, fixed_line);

    // Drag the shared point — without Fixed this would pull fixed_line's
    // end along with it; Fixed should hold fixed_line exactly in place and
    // let moving_line's start follow back to it instead.
    set_line_start(&mut scene, moving_line, Vector3::new(15.0, 5.0, 0.0));
    scene.bump_entities(&[(moving_line, ChangeKind::Modified)]);

    let (after_start, after_end) = line_endpoints(&scene, fixed_line);
    assert_eq!(before_start, after_start, "Fixed entity's start must not move");
    assert_eq!(before_end, after_end, "Fixed entity's end must not move");
    let (moving_start, _) = line_endpoints(&scene, moving_line);
    assert!(
        (moving_start.x - before_end.x).abs() < 1e-6 && (moving_start.y - before_end.y).abs() < 1e-6,
        "moving_line's start should have been pulled back to fixed_line's (unmoved) end"
    );
}

#[test]
fn point_on_curve_constraint_pulls_a_point_onto_a_circles_circumference() {
    let mut scene = Scene::new();
    let circle = add_circle(&mut scene, 0.0, 0.0, 5.0);
    let marker = add_line(&mut scene, 100.0, 100.0, 101.0, 101.0); // marker.start is the tracked point

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
        ConstraintKind::PointOnCurve,
        vec![SketchRef::point(marker, 0), SketchRef::whole(circle)],
        None,
    );

    scene.bump_entities(&[(marker, ChangeKind::Modified)]);

    let (center, radius) = circle_geom(&scene, circle);
    let (marker_start, _) = line_endpoints(&scene, marker);
    let dist = ((marker_start.x - center.x).powi(2) + (marker_start.y - center.y).powi(2)).sqrt();
    assert!((dist - radius).abs() < 1e-5, "point should land on the circumference: dist={dist} radius={radius}");
}

#[test]
fn equal_distance_constraint_matches_a_second_point_pairs_separation() {
    let mut scene = Scene::new();
    // Reference pair: fixed 6 units apart.
    let a = add_line(&mut scene, 0.0, 0.0, 6.0, 0.0);
    // Tracked pair: starts at some other separation, should be pulled to 6.
    let b = add_line(&mut scene, 20.0, 20.0, 25.0, 20.0);

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
        ConstraintKind::EqualDistance,
        vec![SketchRef::point(b, 0), SketchRef::point(b, 1), SketchRef::point(a, 0), SketchRef::point(a, 1)],
        None,
    );

    scene.bump_entities(&[(b, ChangeKind::Modified)]);

    let (a1, a2) = line_endpoints(&scene, a);
    let (b1, b2) = line_endpoints(&scene, b);
    let dist_a = ((a2.x - a1.x).powi(2) + (a2.y - a1.y).powi(2)).sqrt();
    let dist_b = ((b2.x - b1.x).powi(2) + (b2.y - b1.y).powi(2)).sqrt();
    assert!((dist_a - dist_b).abs() < 1e-6, "the two pairs' separations should match: dist_a={dist_a} dist_b={dist_b}");
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

    let set = scene.sketch_constraint_set_mut(SketchScope::ModelSpace);
    // Pin the axis and `a` in place — otherwise they're just as free to move
    // as `b`'s center, and with only 2 equations (MidpointOnLine +
    // Perpendicular) against that many more unknowns the solver is free to
    // converge on any of infinitely many valid configurations, not
    // necessarily "only b moves, to a's exact mirror" (which is the one
    // deterministic outcome this test actually wants to check).
    set.add(ConstraintKind::Fixed, vec![SketchRef::whole(axis)], None);
    set.add(ConstraintKind::Fixed, vec![SketchRef::whole(a)], None);
    set.add(ConstraintKind::Symmetric, vec![SketchRef::center(a), SketchRef::center(b), SketchRef::whole(axis)], None);

    scene.bump_entities(&[(b, ChangeKind::Modified)]);

    let (ca, _) = circle_geom(&scene, a);
    let (cb, _) = circle_geom(&scene, b);
    assert_eq!(ca, Vector3::new(3.0, 4.0, 0.0), "Fixed a's center must not have moved");
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

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
        ConstraintKind::Radius,
        vec![SketchRef::whole(arc)],
        Some(DrivingValue::Literal(7.5)),
    );
    scene.bump_entities(&[(arc, ChangeKind::Modified)]);

    let (_, radius) = arc_geom(&scene, arc);
    assert!((radius - 7.5).abs() < 1e-6, "arc radius should have solved to the driven target: got {radius}");
}

#[test]
fn tangent_constraint_between_a_line_and_an_arc_solves_to_touching() {
    let mut scene = Scene::new();
    let arc = add_arc(&mut scene, 0.0, 0.0, 5.0, 0.0, std::f64::consts::PI);
    let line = add_line(&mut scene, -10.0, 10.0, 10.0, 10.0); // clear of the arc's circle

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(ConstraintKind::Tangent, vec![SketchRef::whole(arc), SketchRef::whole(line)], None);
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

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
        ConstraintKind::Concentric,
        vec![SketchRef::center(arc), SketchRef::center(circle)],
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

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
        ConstraintKind::Coincident,
        vec![SketchRef::point(arc, 0), SketchRef::point(line, 0)],
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

    let set = scene.sketch_constraint_set_mut(SketchScope::ModelSpace);
    set.add(ConstraintKind::Fixed, vec![SketchRef::whole(anchor)], None);
    set.add(ConstraintKind::Coincident, vec![SketchRef::point(arc, 0), SketchRef::point(anchor, 0)], None);
    scene.bump_entities(&[(arc, ChangeKind::Modified)]);

    let (center, radius, start_angle, _) = {
        match scene.document.get_entity(arc).expect("entity exists") {
            EntityType::Arc(a) => (a.center, a.radius, a.start_angle, a.end_angle),
            other => panic!("expected an Arc, got {other:?}"),
        }
    };
    let recomputed_start = Vector3::new(center.x + radius * start_angle.cos(), center.y + radius * start_angle.sin(), 0.0);
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
fn point_on_curve_constraint_pulls_a_point_onto_an_arcs_underlying_circle() {
    let mut scene = Scene::new();
    let arc = add_arc(&mut scene, 0.0, 0.0, 5.0, 0.0, std::f64::consts::PI);
    let marker = add_line(&mut scene, 20.0, 20.0, 21.0, 20.0);

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
        ConstraintKind::PointOnCurve,
        vec![SketchRef::point(marker, 0), SketchRef::whole(arc)],
        None,
    );
    scene.bump_entities(&[(marker, ChangeKind::Modified)]);

    let (center, radius) = arc_geom(&scene, arc);
    let (p, _) = line_endpoints(&scene, marker);
    let dist = ((p.x - center.x).powi(2) + (p.y - center.y).powi(2)).sqrt();
    assert!((dist - radius).abs() < 1e-6, "point should land exactly on the arc's circle: dist={dist} radius={radius}");
}

#[test]
fn radius_and_endpoint_coincident_constraints_coexist_on_the_same_arc() {
    // Exercises both registration paths for the same arc in one solve: the
    // whole-circle path (`Radius`, via `.circle`) and the endpoint path
    // (`Coincident` on marker 0, via the arc-rules-anchored `start` point).
    let mut scene = Scene::new();
    let arc = add_arc(&mut scene, 0.0, 0.0, 3.0, 0.0, std::f64::consts::FRAC_PI_2);
    let anchor = add_line(&mut scene, 9.0, -4.0, 9.0, -4.0);

    let set = scene.sketch_constraint_set_mut(SketchScope::ModelSpace);
    set.add(ConstraintKind::Fixed, vec![SketchRef::whole(anchor)], None);
    set.add(ConstraintKind::Coincident, vec![SketchRef::point(arc, 0), SketchRef::point(anchor, 0)], None);
    set.add(ConstraintKind::Radius, vec![SketchRef::whole(arc)], Some(DrivingValue::Literal(6.0)));
    scene.bump_entities(&[(arc, ChangeKind::Modified)]);

    let (_, radius) = arc_geom(&scene, arc);
    assert!((radius - 6.0).abs() < 1e-6, "Radius constraint should still hold: got {radius}");
    let actual_start = arc_start_point(&scene, arc);
    assert!(
        (actual_start.x - 9.0).abs() < 1e-6 && (actual_start.y - -4.0).abs() < 1e-6,
        "the arc's start point should have reached the fixed anchor despite the simultaneous Radius constraint: {actual_start:?}"
    );
}

// Phase 2 of the same plan: `Diameter`/`DistanceX`/`DistanceY` reuse
// existing `ocs_gcs` primitives (`Equal`'s `ratio`, `Difference`) with no
// new solver-crate code.

#[test]
fn diameter_constraint_drives_the_radius_to_half_the_target() {
    let mut scene = Scene::new();
    let circle = add_circle(&mut scene, 0.0, 0.0, 3.0);

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
        ConstraintKind::Diameter,
        vec![SketchRef::whole(circle)],
        Some(DrivingValue::Literal(16.0)),
    );
    scene.bump_entities(&[(circle, ChangeKind::Modified)]);

    let (_, radius) = circle_geom(&scene, circle);
    assert!((radius - 8.0).abs() < 1e-6, "radius should be half the diameter target: got {radius}");
}

#[test]
fn distance_x_constraint_pins_only_the_x_component() {
    let mut scene = Scene::new();
    // Deliberately not axis-aligned so DistanceX and DistanceY are
    // distinguishable from a plain Distance/Horizontal outcome.
    let line = add_line(&mut scene, 0.0, 0.0, 6.0, 8.0);

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
        ConstraintKind::DistanceX,
        vec![SketchRef::point(line, 0), SketchRef::point(line, 1)],
        Some(DrivingValue::Literal(10.0)),
    );
    scene.bump_entities(&[(line, ChangeKind::Modified)]);

    let (start, end) = line_endpoints(&scene, line);
    assert!((end.x - start.x - 10.0).abs() < 1e-6, "x component should equal the target: dx={}", end.x - start.x);
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

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(ConstraintKind::Normal, vec![SketchRef::whole(circle), SketchRef::whole(line)], None);
    scene.bump_entities(&[(circle, ChangeKind::Modified), (line, ChangeKind::Modified)]);

    let (center, _) = circle_geom(&scene, circle);
    let (p1, p2) = line_endpoints(&scene, line);
    let line_dir = (p2.x - p1.x, p2.y - p1.y);
    let line_len = (line_dir.0 * line_dir.0 + line_dir.1 * line_dir.1).sqrt();
    let signed_dist = (line_dir.0 * (center.y - p1.y) - line_dir.1 * (center.x - p1.x)) / line_len;
    assert!(signed_dist.abs() < 1e-6, "the circle's center should lie on the (infinite) line after solving: dist={signed_dist}");
}

// Phase 4a of the same plan: `ArcLength` registers an arc's
// `start_angle`/`end_angle` separately from its center/radius (which still
// flow through the existing `EntityGeom::Circle` aliasing), so it can
// combine with a `Radius` constraint on the same arc in one solve.

#[test]
fn arc_length_constraint_drives_the_swept_length_to_the_target() {
    let mut scene = Scene::new();
    // radius 5, a 90° span (length ≈ 7.85) — deliberately not the target.
    let arc = add_arc(&mut scene, 0.0, 0.0, 5.0, 0.0, std::f64::consts::FRAC_PI_2);

    let set = scene.sketch_constraint_set_mut(SketchScope::ModelSpace);
    set.add(ConstraintKind::Radius, vec![SketchRef::whole(arc)], Some(DrivingValue::Literal(5.0)));
    set.add(ConstraintKind::ArcLength, vec![SketchRef::whole(arc)], Some(DrivingValue::Literal(10.0)));
    scene.bump_entities(&[(arc, ChangeKind::Modified)]);

    let (radius, start_angle, end_angle) = arc_angles(&scene, arc);
    assert!((radius - 5.0).abs() < 1e-6, "Radius constraint should still hold: got {radius}");
    let length = radius * (end_angle - start_angle);
    assert!((length - 10.0).abs() < 1e-5, "swept length should equal the ArcLength target: got {length}");
}

// Phase 5 of the same plan: an `Ellipse` registers center/focus1/minor-
// radius (converted from acadrust's center + major-axis-vector + ratio
// parametrization). Only center-based whole-entity constraints are wired
// up so far — Concentric, CenterPoint, Fixed — mirroring how Arc started
// at "center/radius only" in Phase 1. Axis-length constraints and
// `PointOnEllipse`-based Coincident/Tangent are a further follow-up.

#[test]
fn concentric_constraint_between_an_ellipse_and_a_circle_pulls_centers_together() {
    let mut scene = Scene::new();
    let ellipse = add_ellipse(&mut scene, 0.0, 0.0, (6.0, 0.0), 0.5);
    let circle = add_circle(&mut scene, 10.0, -6.0, 2.0);

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
        ConstraintKind::Concentric,
        vec![SketchRef::center(ellipse), SketchRef::center(circle)],
        None,
    );
    scene.bump_entities(&[(ellipse, ChangeKind::Modified), (circle, ChangeKind::Modified)]);

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

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
        ConstraintKind::CenterPoint,
        vec![SketchRef::point(line, 0), SketchRef::center(ellipse)],
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
    // The riskiest part of Ellipse support: converting acadrust's center +
    // major-axis-*vector* + minor/major ratio into `ocs_gcs`'s center +
    // focus + minor-radius, then back, for a major axis that ISN'T
    // axis-aligned (an axis-aligned one wouldn't exercise the direction
    // math at all). A solve with nothing actually pulling on it should
    // reproduce the same ellipse, not drift.
    let mut scene = Scene::new();
    let ellipse = add_ellipse(&mut scene, 2.0, -3.0, (4.0, 3.0), 0.6); // major radius 5, tilted
    let circle = add_circle(&mut scene, 2.0, -3.0, 1.0); // already concentric — no solving needed

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
        ConstraintKind::Concentric,
        vec![SketchRef::center(ellipse), SketchRef::center(circle)],
        None,
    );
    scene.bump_entities(&[(ellipse, ChangeKind::Modified)]);

    let (center, major_axis, ratio) = ellipse_geom(&scene, ellipse);
    assert!((center.x - 2.0).abs() < 1e-6 && (center.y - -3.0).abs() < 1e-6, "center should not have drifted: {center:?}");
    assert!((major_axis.x - 4.0).abs() < 1e-6 && (major_axis.y - 3.0).abs() < 1e-6, "major axis should not have drifted: {major_axis:?}");
    assert!((ratio - 0.6).abs() < 1e-6, "minor/major ratio should not have drifted: {ratio}");
}

#[test]
fn fixed_constraint_holds_an_ellipse_in_place_despite_a_connected_edit() {
    let mut scene = Scene::new();
    let ellipse = add_ellipse(&mut scene, 0.0, 0.0, (6.0, 0.0), 0.5);
    let line = add_line(&mut scene, 0.0, 0.0, 5.0, 5.0);

    let set = scene.sketch_constraint_set_mut(SketchScope::ModelSpace);
    set.add(ConstraintKind::Fixed, vec![SketchRef::whole(ellipse)], None);
    set.add(ConstraintKind::CenterPoint, vec![SketchRef::point(line, 0), SketchRef::center(ellipse)], None);

    let before = ellipse_geom(&scene, ellipse);

    // Drag the line's start away — without Fixed this would pull the
    // ellipse's center along with it; Fixed should hold the ellipse
    // exactly in place and let the line's start follow back to it instead.
    set_line_start(&mut scene, line, Vector3::new(20.0, -8.0, 0.0));
    scene.bump_entities(&[(line, ChangeKind::Modified)]);

    let after = ellipse_geom(&scene, ellipse);
    assert_eq!(before, after, "Fixed ellipse must not move at all (center, major axis, or ratio)");
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

    scene.sketch_constraint_set_mut(SketchScope::ModelSpace).add(
        ConstraintKind::DistanceY,
        vec![SketchRef::point(line, 0), SketchRef::point(line, 1)],
        Some(DrivingValue::Literal(-3.0)),
    );
    scene.bump_entities(&[(line, ChangeKind::Modified)]);

    let (start, end) = line_endpoints(&scene, line);
    assert!((end.y - start.y - -3.0).abs() < 1e-6, "y component should equal the (signed) target: dy={}", end.y - start.y);
}
