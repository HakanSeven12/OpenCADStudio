# Deferred precision input — design note

## Deferred data model

`SnapResult` gains `deferred: Option<DeferredPick>`. A deferred pick is produced
only when the command has **no base point** (`from_point == None`) and the
running TAN or PER object snap finds a curve under the cursor:

```rust
enum DeferredMode   { Tangent, Perpendicular }
enum DeferredCurve  { Line{p1,p2}, Circle{center,axis_x,axis_y,radius},
                      Arc{..angles..}, Ellipse{..start/end..} }
struct DeferredPick { mode: DeferredMode, curve: DeferredCurve, at: DVec3 }
```

`at` is the nearest point on the picked curve to the cursor; it is a *hint* for
choosing among multiple solutions, not the committed point. `tangent_obj` is
kept unchanged for compatibility. `CadCommand` gains default hooks
`on_point_with_deferred` / `on_preview_wires_with_deferred`; their defaults
forward a Tangent deferred to the existing tangent hooks, so only LINE changes
behaviour.

## Pure solver

`src/snap/deferred.rs` has no `Scene`/UI dependency. Its entry points are
`resolve_deferred_line(LineEnd, LineEnd) -> Option<(DVec3, DVec3)>` and
`point_in_direction(curve, mode, dir, hint) -> Option<DVec3>`.
`LineEnd` is `Point(DVec3)` or `Deferred(DeferredPick)`.
A candidate line must be finite and have non-zero length. Among candidates the
one whose endpoints lie closest to the two `at` hints wins (total-order float
comparison). Degenerate, parallel and unsupported cases return `None`.

Supported curves: line (infinite), circle, bounded circular arc, ellipse and
elliptical arc. Splines are **not** supported: a deferred snap is exposed at
the single point under the cursor, and a spline has no closed-form common
tangent/normal, so it would silently produce an unstable answer.

## Plane handling / UCS

Circles, arcs and ellipses carry their own world-space plane; the solver
projects the other point onto that plane along the plane normal, solves in 2D
and lifts the result back (matching the existing planar tangent helpers). A
line target uses vector projection in world space. For two deferred curves the
planes must coincide (parallel normals, common origin plane); otherwise the
solver returns `None`. All coordinates stay `f64`.

## Solution-selection rule

For `Deferred + Point`, generate all points on the curve satisfying the mode
against the point, pair each with the point, and pick the pair with the smaller
`dist(solution, hint)` (ties resolved deterministically). For two deferred
ends, generate the bounded pair list (common tangents, tangent-to-centre,
normal-to-line, centre-to-centre, parallel-line normals) and apply the same
rule with both hints. Arc/elliptical-arc solutions outside the stored sweep are
discarded.

## Typed `@d<a` after a deferred pick

The host feeds typed points through `CadCommand::on_typed_point(pt, dir, dist)`.
LINE resolves them in the requested direction:

* deferred **TAN**: `point_in_direction` picks the curve point whose tangent is
  parallel to the typed direction; the end is that point plus `dir * dist`.
* deferred **PER** on a **line**: accepted only when the typed direction is
  perpendicular to the target line. A non-perpendicular direction is refused
  (the command repeats the prompt and commits nothing) rather than being
  silently projected onto the line, which would shorten the typed distance.
* deferred **PER** on a circle/ellipse: `point_in_direction` picks the curve
  point whose normal is parallel to the typed direction; the end is that point
  plus `dir * dist`.

A mouse pick with a deferred pick still resolves with the pure solver against
the picked point.

## Temporary tracking (TT)

`TT` at a point prompt suspends the command and starts a one-point modifier.
The picked point is added to the snapper as a *temporary* acquisition via
`acquire_temporary_tracking_point`, bypassing dwell and the endpoint filter,
then the parent command resumes. Temporary points are flagged in a parallel
`tracking_temp` vector, survive point commits, are drawn as a distinct marker,
and are cleared with all tracking when the command ends. The acquisition cap
is raised from 4 to 7; eviction of the oldest point is explicit and remembers
the evicted point (`tracking_evicted`) so the overlay can draw it faded.

## Constraints honoured

No existing prompt, option or behaviour is removed; host-owned commit/undo is
unchanged. Rust 2021, no `unsafe`, no new dependencies. All loops are bounded
(constant sample counts and the 7-point tracking cap).
