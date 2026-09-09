# DWG-Native Constraint Compatibility — Design

## 1. What this is, and why

Hakan (upstream maintainer) reviewed the sketch-constraint system
(`docs/parametric_system_design.md`) and asked for it to persist using the
constraint object model already exposed in `acadrust`/`cadcodec` (the pinned
CAD kernel), so a file saved by OpenCADStudio carries constraints AutoCAD or
BricsCAD would recognize as their own native objects — not just an opaque
custom blob only this app understands.

This is purely a **persistence-format** question, not a solver question.
`ocs_gcs` (the FreeCAD `planegcs` port: `crates/ocs_gcs/src/solvers/{bfgs,dogleg,lm}.rs`)
is the numerical engine that computes geometry from a constraint system —
that stays exactly as it is regardless of what this document decides. What
changes is only how `SketchConstraintSet` gets written to and read from a
`CadDocument`, in `src/scene/sketch_persist.rs`, currently a custom
`bincode`-in-XRecord blob keyed `OCS_SKETCH_CONSTRAINTS`.

## 2. What `acadrust` already provides (confirmed from source)

All paths below are relative to the pinned checkout
(`~/.cargo/git/checkouts/cadcodec-*/*/src/`, rev pinned in `Cargo.toml`).

- `objects/associative.rs` defines AutoCAD's real 2D geometric-constraint
  object graph as plain Rust types, reachable generically through
  `ObjectType::Associative(AssociativeObject { data: AssociativeData::ConstraintGroup(Assoc2dConstraintGroup), .. })`
  — the same `document.objects: HashMap<Handle, ObjectType>` map
  `sketch_persist.rs` already inserts `XRecord`/`Dictionary` entries into.
- Both DWG (`io/dwg/dwg_stream_{readers,writers}/object_{reader,writer}/associative.rs`,
  ~1240/~870 lines) and DXF (`io/dxf/{reader,writer}/section_{reader,writer}/associative.rs`)
  have **complete read and write** implementations for every type below —
  confirmed by reading the actual field-by-field (de)serialization code, not
  inferred from the type definitions alone.
- `classes/mod.rs` registers the DXF class names every one of these needs
  when written (`(DXF_NAME, "AcDbClassName", ...)` tuples):
  `AcDbAssocDependency`, `AcDbAssocGeomDependency`, `AcDbAssocAction`,
  `AcDbAssocNetwork`, `AcDbAssoc2dConstraintGroup`, `AcDbAssocVariable`,
  `AcDbAssocPersSubentManager`.

### The graph shape

Not a flat list — a small object graph, matching AutoCAD's real associative
framework:

```
AssocNetwork (per block/space that owns constrained geometry)
  └─ owned_actions: Vec<Handle> ─→ AssocAction
                                     ├─ action_body: Handle ─→ Assoc2dConstraintGroup
                                     │    ├─ work_plane: [Vector3; 3]
                                     │    └─ nodes: Vec<AssocConstraintNode>
                                     │         (both the *constraint* nodes
                                     │          and the *geometry* nodes live
                                     │          in this one Vec — see §3/§4)
                                     └─ dependencies: Vec<AssocActionDependency>
                                          ─→ AssocGeomDependency
                                               ├─ dependency: AssocDependency
                                               │    └─ dependent_on: Handle
                                               │         (the real Line/Circle/... entity)
                                               └─ persistent_subent: AssocPersistentSubentId
```

`AssocConstraintNode { node_id: i32, status: u8, connections: Vec<i32>,
class_name: String, registry_flag: bool, data: AssocConstraintNodeData }` —
`node_id`/`connections` link nodes to each other *within* the group (a
constraint node's `owner_id` fields point at a geometry node's `node_id`);
`class_name` picks which `AssocConstraintNodeData` variant applies on
read (`read_constraint_node_data(reader, class_name)`,
`dwg_stream_readers/object_reader/associative.rs:663`).

### 2b. Primary sources beyond `cadcodec`'s own code

`cadcodec` ships no constraint-bearing test fixture and its own source only
shows *what bytes get read*, not *what they mean* semantically — §7 (now
mostly resolved) needed Autodesk's own interface documentation to answer.
Two real, first-party sources turned out to be publicly available with no
sample DWG file needed at all:

- **The ObjectARX 2012 SDK's own C++ headers**, mirrored on GitHub at
  [Stalso/CadPlugin](https://github.com/Stalso/CadPlugin/tree/master/ObjectARX%202012/inc)
  — Autodesk's own copyright header on each file explicitly grants "permission
  to use, copy, modify, and distribute this software... for any purpose and
  without fee", and this is the SDK Autodesk itself publishes specifically so
  third parties can build interoperable tools — not leaked or decompiled
  source. `AcDbAssoc2dConstraintGroup.h`, `AcDbAssocNetwork.h`,
  `AcDbAssocManager.h`, `AcDbAssocVariable.h`, `AcConstrainedGeometry.h` were
  all read directly; their class-level doc comments describe the exact
  same object graph `cadcodec` implements, in Autodesk's own words.
- **Autodesk's live public ObjectARX reference**
  (`help.autodesk.com/view/OARXMAC/2025/...`) — confirmed the
  `AcDb::ImplicitPointType` enum's exact values and per-member descriptions
  (used for §4's `point_type` mapping) and `AcDbAssocNetwork`'s
  `getInstanceFromObject`/`getInstanceFromDatabase` dictionary-key default
  (used for §7 question 1). No login or SDK download needed — the reference
  guide is indexed and publicly browsable.

## 3. Constraint-kind mapping

Our `ConstraintKind` (`src/scene/sketch_constraints.rs`) against
`AssocConstraintNodeData` (`objects/associative.rs:885`). The "refs shape"
column is copied from `sketch_solve::build_constraint`'s actual `match`
(`src/scene/sketch_solve.rs:151-208`) — the ground truth for what each kind
expects, not the doc comment.

| `ConstraintKind` | refs shape (from `build_constraint`) | `AssocConstraintNodeData` | Notes |
|---|---|---|---|
| `Coincident` | `[point, point]` | `Geometrical` + `class_name = "ACPOINTCOINCIDENCECONSTRAINT"` | |
| `Horizontal` | `[whole_line]` | `Geometrical` + `"ACHORIZONTALCONSTRAINT"` | |
| `Vertical` | `[whole_line]` | `Geometrical` + `"ACVERTICALCONSTRAINT"` | |
| `Parallel` | `[whole_line, whole_line]` | `Parallel { datum_line_index: None, .. }` | dedicated variant, not `Geometrical` |
| `Perpendicular` | `[whole_line, whole_line]` | `Geometrical` + `"ACPERPENDICULARCONSTRAINT"` | |
| `Equal` (line case) | `[whole_line, whole_line]` | `Geometrical` + `"ACEQUALLENGTHCONSTRAINT"` | |
| `Equal` (circle/arc case) | `[whole_circle, whole_circle]` | `Geometrical` + `"ACEQUALRADIUSCONSTRAINT"` | **our `Equal` doesn't distinguish these — see §5** |
| `Distance` | `[point, point]` | `Distance { direction_type, distance: Option<Vector3>, value_dependency, dimension_dependency, .. }` | needs an `AssocVariable`/value dependency for the driving value — see §5 |
| `Angle` | `[whole_line, whole_line]` | `Angle { sector_type, value_dependency, dimension_dependency, .. }` | same value-dependency need |
| `Radius` | `[whole_circle]` | `RadiusDiameter { mode, value_dependency, dimension_dependency, .. }` | `mode` presumably distinguishes radius vs. diameter — we're always radius |
| `Tangent` | `[circle, circle]` or `[circle, line]`/`[line, circle]` — Line-Line and anything involving an `Arc` are explicitly unsupported today (`sketch_solve.rs:239-241`, `Arc` isn't a supported `EntityGeom` variant yet) | `Geometrical` + `"ACTANGENTCONSTRAINT"` | our own `Arc`-tangent gap predates this document, unrelated to DWG compatibility |

AutoCAD's `is_plain_geometrical_constraint` list also includes several kinds
we have no `ConstraintKind` for at all: `ACCENTERPOINTCONSTRAINT`,
`ACCOLINEARCONSTRAINT`, `ACCONCENTRICCONSTRAINT`, `ACEQUALCURVATURECONSTRAINT`,
`ACEQUALDISTANCECONSTRAINT`, `ACEQUALHELPPARAMETERCONSTRAINT`,
`ACFIXEDCONSTRAINT`, `ACMIDPOINTCONSTRAINT`, `ACNORMALCONSTRAINT`,
`ACPOINTCURVECONSTRAINT`, `ACSYMMETRICCONSTRAINT` — genuinely nothing to map
to yet, not a mapping gap (§5).

## 4. Geometry-node mapping

Every entity a constraint references also needs its own node in the same
`nodes: Vec<AssocConstraintNode>`, so constraint nodes have something to
point `owner_id` at, *and* a paired `AssocGeomDependency` (in the owning
`AssocAction`'s `dependencies`) so the graph — not just the constraint data
— actually references the real `Line`/`Circle`/... entity by `Handle`.

| Our reference | `AssocConstraintNodeData` geometry variant |
|---|---|
| `SketchRef::whole(line)` | `BoundedLine { point, direction, is_ray: false, start_point, end_point, .. }` — confirmed a *bounded* line is the right choice: `Line`/`BoundedLine`'s split exists because AutoCAD's own constrained-geometry set includes true infinite construction lines (`ACCONSTRAINEDCONSTRUCTIONLINE`/`ACCONSTRAINEDDATUMLINE`, `read_constraint_node_data`, `associative.rs:711-714`) as a *separate* case from an ordinary bounded segment; nothing we draw is an infinite line |
| `SketchRef::point(line, 0\|1)` | `ImplicitPoint { point_type: kStartImplicit(0)\|kEndImplicit(1), point_index: -1, curve_id: <the Line/BoundedLine node's node_id>, .. }` — **confirmed**, `AcDb::ImplicitPointType` enum, `kStartImplicit`/`kEndImplicit`/`kMidImplicit`/`kCenterImplicit`/`kDefineImplicit` (0-4), Autodesk's own live ObjectARX reference (§2b). `point_index` is `-1` for every case except `kDefineImplicit` (a spline control point, which we don't reference); `curve_id` is presumably the owning geometry node's own `node_id` (not yet cross-checked against a real file — the *values* are confirmed, this one wiring detail isn't) |
| `SketchRef::whole(circle)` | `Circle { center, normal, direction, radius, start_parameter: 0.0, end_parameter: TAU, .. }` |
| `SketchRef::whole(arc)` | `Arc { .. same fields .. , start_point, end_point }` |
| `SketchRef::center(circle\|arc)` | `ImplicitPoint { point_type: kCenterImplicit(3), point_index: -1, curve_id: <the Circle/Arc node's node_id>, .. }` — confirmed by the same enum (`kCenterImplicit`: "Center point of a circle, arc, ellipse or bounded ellipse") — **not** the separate `Point`/`ACCONSTRAINEDPOINT` variant, which the ObjectARX docs' own class hierarchy (§2b) show is for a *free-standing* constrained point, not one implicitly defined by another curve |
| `SketchRef::point(_, -2)` (reserved, unused today) | would be `kMidImplicit` if ever used — "Mid point of a bounded line ... or arc" is exactly what marker `-2`'s doc comment already reserved this for |

`SketchRef` and this graph already agree on one thing directly: our own doc
comment (`src/scene/sketch_constraints.rs:20-22`) says the `marker`
convention was deliberately borrowed from
`AssocDimensionReference::main_gs_marker` (`src/scene/dimension_assoc.rs`,
already shipping, already reading/writing real `Assoc*` objects for
associative dimensions). That's a different, lighter mechanism than the
constraint graph (an `AcDbOsnapPointRef` chain attached directly to a
`Dimension` entity, not the `AssocNetwork`/`AssocAction` framework — see
`AssocDimensionAssociation`, `objects/associative.rs:1200`), so it's a
precedent for *how this codebase already talks to `Assoc*` types*, not a
template for the constraint graph's own wiring.

## 5. Known gaps (not addressed by this document)

- **`Equal` is one `ConstraintKind`, AutoCAD has (at least) two**:
  `ACEQUALLENGTHCONSTRAINT` (lines) vs. `ACEQUALRADIUSCONSTRAINT`
  (circles/arcs), plus `ACEQUALCURVATURECONSTRAINT`/
  `ACEQUALDISTANCECONSTRAINT` we don't model at all. `build_constraint`
  already branches on `refs`' resolved shape at solve time
  (`sketch_solve.rs:181-188`), so the same branch can pick the write-side
  `class_name` — no data-model change needed for the length/radius split;
  the other two AutoCAD sub-kinds are a real, separate gap.
- **`DrivingValue::Named` — confirmed, has a real native equivalent, and the
  full wiring is now traced end to end.**
  `AcDbAssocVariable` (`objects/associative.rs:1024`: `name`, `expression`,
  `evaluator`, `description`, cached `value`) is fully read *and* written on
  both DWG and DXF (confirmed in
  `dwg_stream_readers/object_reader/associative.rs:1031-1063` and the DXF
  reader/writer equivalents) — it's the same associative-action graph a
  constraint group lives in (`AssocVariable` wraps an `AssocAction`, exactly
  like `Assoc2dConstraintGroup` does), not a separate table.

  The two handles on `Distance`/`Angle`/`RadiusDiameter`
  (`read_explicit_constraint`, `associative.rs:652-661` — reads them in this
  order: `value_dependency` first, `dimension_dependency` second) resolve
  to two different, unrelated things:

  - **`value_dependency`** → an `AssocValueDependency` object
    (`{ dependency: AssocDependency, name, value: AssocEvalVariant }`,
    `objects/associative.rs:554`). Its own `value: AssocEvalVariant`
    (`{ code: i16 /* DXF resbuf type */, value: AssocEvalValue::Real(f64) }`,
    confirmed `associative.rs:579-596`) is the **cached, already-resolved
    number** — always present, read fast without evaluating anything. The
    embedded `AssocDependency.dependent_on: Handle`
    (`read_dependency`, `associative.rs:83-109`) is what actually links to a
    named parameter: `Handle::NULL` when the driving value is a bare
    literal, or an `AssocVariable`'s handle when it's driven by one. This
    gives a direct, confirmed mapping:
    - `DrivingValue::Literal(v)` → `AssocValueDependency` with
      `value = Real(v)`, `dependency.dependent_on = Handle::NULL`.
    - `DrivingValue::Named(name)` → the same, but `dependency.dependent_on`
      points at an `AssocVariable{ name, expression: <our formula>, .. }`
      object (create once per distinct name, reused by every constraint
      referencing it — `value` still carries the current resolved number,
      kept in sync with `ParameterTable`'s last evaluation, same duty our
      own `dof`/`conflicts` caches already have).
  - **`dimension_dependency`** → a *separate* dependency chain
    (`AssocAnnotationDependency`/`AssocDimDependencyBody`,
    `associative.rs:744-751` and `:1077-1082`) linking to the actual
    `Dimension` entity AutoCAD draws next to a constraint to show its
    current value on screen — nothing to do with named parameters at all.
    Not required for our purposes unless we also want the constraint to
    carry a visible on-canvas dimension the way AutoCAD's does (open
    question, not yet decided — §7).

  Remaining gap before mapping directly: AutoCAD's `expression` is
  evaluated by a named, extensible `evaluator` (not fixed) — real-file
  survey needed to know what evaluator name(s) actually appear in practice
  (§7 open question 4), ours has one hardcoded evaluator.
- **Missing `ConstraintKind`s**: Concentric, Colinear, Symmetric, Fixed,
  Midpoint, Normal, CenterPoint, PointOnCurve — real AutoCAD constraint
  types with no `ConstraintKind` variant to map from at all. Out of scope
  until/unless the sketch-constraint system itself grows them.
- **`Tangent` involving an `Arc`** has no native mapping to design, since
  `build_constraint` doesn't support it at all yet (`sketch_solve.rs:239-241`
  — `Arc` isn't a supported `EntityGeom` variant in that function). A
  pre-existing gap in the constraint system itself, unrelated to and not
  blocking this document.

## 6. Persistence architecture: additive, not a replacement

**Proposal: write both.** Keep `OCS_SKETCH_CONSTRAINTS` as the authoritative
round-trip format OpenCADStudio itself reads back
(`sketch_persist::load_sketch_constraints_from_document`), and *additionally*
materialize the `AssocNetwork`/.../`Assoc2dConstraintGroup` graph alongside
it on save, for other applications to read.

Reasoning:
- Our own model carries information the native graph can't represent
  losslessly today — `DrivingValue::Named` chief among them (§5), also
  `enabled` (a suppressed-but-kept constraint) which may or may not have a
  clean native equivalent (`AssocConstraintNodeData`'s `is_active` field on
  several variants looks like the right fit, but unconfirmed, §7).
  Round-tripping through the native format only and reconstructing our
  model from it risks silent data loss on every save/reopen cycle.
- The native graph's own geometry-node fields (`center`, `radius`, `point`,
  ...) are *snapshots* of the current solved geometry, not derived at load
  time — so they need to be kept in sync with whatever `ocs_gcs` last
  solved, same as we already do for our own tessellated wire geometry. Two
  representations of "the same" constraint, kept consistent by one save
  path, is a smaller invariant to maintain than trying to make the native
  graph the *only* source of truth for a system it can't fully express.
- This is reversible: if the native graph turns out to work well enough
  (once tested against real AutoCAD/BricsCAD opens, §7) to *become* the
  primary format later, dropping the redundant `OCS_SKETCH_CONSTRAINTS`
  write is a small follow-up, not a rewrite.

## 7. Open questions

Three of the original five are now resolved from Autodesk's own primary
sources (§2b), not just inferred from `cadcodec`'s struct shapes — kept
here with their answers so the reasoning stays visible, not deleted.
`cadcodec` itself still ships no constraint-bearing DWG/DXF fixture and no
constraint-specific test (checked: no hits for `Assoc2dConstraintGroup`/
`AssocConstraintNode` under any `tests/` directory or fixture path in the
dependency), so even the resolved ones haven't been checked against a real
written-and-reopened file yet — that step still matters (§8 stage 1) for
confirming implementation correctness, just not for the *design* questions
below anymore.

1. **Resolved.** Where the top-level `AssocNetwork` attaches:
   `AcDbAssocNetwork::getInstanceFromDatabase`/`getInstanceFromObject`'s own
   doc comments (§2b) state the default sub-dictionary key directly —
   `"ACAD_ASSOCNETWORK"` — used both for the one top-level network (owned by
   the database's Named Object Dictionary) and for each per-block-record
   network (owned by that `AcDbBlockTableRecord`'s own extension
   dictionary — confirming the parallel to `OCS_SKETCH_CONSTRAINTS`'s own
   attachment was right). The two are linked: creating a per-block network
   with `addToTopLevelNetwork = true` (the default) also registers it as an
   action inside the top-level network, so evaluating the top-level network
   cascades through every block's sub-network.
2. **Resolved.** Sub-entity addressing for `ImplicitPoint`: confirmed the
   real addressing lives entirely in the constraint-node's own
   `point_type`/`point_index`/`curve_id` fields, not in
   `AssocPersistentSubentId` (that type turned out to be an abstract C++
   base class for a *different*, more general subentity-tracking need
   elsewhere in the associative framework — irrelevant to 2D constraint
   geometry). `AcDb::ImplicitPointType`'s exact enum (§4) settles the
   mapping.
3. **Still open.** Whether a partially-populated graph (e.g. omitting
   `AssocPersSubentManager`, or leaving some `AssocAction` bookkeeping
   fields at their `Default`) still opens cleanly in real AutoCAD, or
   whether it requires every field correctly populated to avoid a repair
   prompt or silent constraint loss. Genuinely needs a real write-then-open
   test, not documentation — this is a robustness/tolerance question, not a
   structural one.
4. **Narrowed to almost nothing.** `AssocVariable`'s `evaluator` field:
   `AcDbAssocVariable.h`'s own doc comment (§2b) says plainly — "If empty,
   the default evaluator for current acad version will be used." Leaving it
   as an empty string is officially-supported behavior per the SDK itself,
   not a guess. No further investigation needed unless a real file later
   shows this doesn't round-trip as expected.
5. **Still open.** DWG version gating — `AssocConstraintNode`'s own reader
   already branches on `version.r2013_plus(dxf_version)` for one field
   (`dwg_stream_readers/object_reader/associative.rs:604`); unclear whether
   other fields need per-version handling this document hasn't looked for.
   Likely answerable the same way as questions 1/2/4 were (Autodesk's own
   docs usually note version-specific members) — just not done yet.
6. **Still open, low priority.** Whether to bother wiring
   `dimension_dependency` at all (§5) — it drives the small on-canvas
   dimension label AutoCAD shows next to a constrained value, cosmetic
   rather than structural. Leaving it `Handle::NULL` is presumably safe (the
   constraint itself is still fully defined without it) but unconfirmed;
   deferring this to a later stage either way (§8).

## 8. Suggested staged approach

1. **Proof of concept, narrowest possible slice**: `Horizontal`/`Vertical`
   on a single `Line`, no dimensional constraints (sidesteps the
   `AssocVariable`/value-dependency wiring entirely, though that's now fully
   designed too — §5). Write the graph — `AssocNetwork` at
   `"ACAD_ASSOCNETWORK"` on the scope owner's extension dictionary (§7.1),
   one `Assoc2dConstraintGroup` action, one `BoundedLine` geometry node, one
   `Geometrical`/`"ACHORIZONTALCONSTRAINT"` (or vertical) constraint node —
   and open the file in whatever real-DWG-reading tool is available, to
   confirm AutoCAD/BricsCAD actually shows a constraint icon on the line and
   doesn't flag the file as corrupt. This is what answers open question 3
   (partial-graph tolerance) — the one remaining question documentation
   alone can't settle.
2. **Geometric constraints, still no driving value**: add `Coincident`
   (needs `ImplicitPoint`/`kStartImplicit`/`kEndImplicit`, §4, now
   confirmed), `Parallel`, `Perpendicular`, `Equal` (both sub-kinds) —
   verify against the same real-file test from stage 1.
3. **Dimensional constraints**: `Distance`, `Angle`, `Radius`, using the now
   fully-traced `value_dependency → AssocValueDependency → AssocVariable`
   chain (§5) — including deciding whether `DrivingValue::Named` gets the
   real native mapping designed there, or stays a resolved-literal degrade
   for a first pass.
4. **Everything else**: `Tangent`'s non-circle-circle cases, the missing
   `ConstraintKind`s (§5) if/when the sketch-constraint system grows them,
   `dimension_dependency` (§7.6) if a visible on-canvas dimension label
   turns out to matter.

Each stage's file should be hand-inspected (or opened in real software)
before moving to the next — the graph's correctness genuinely can't be
verified by this codebase's own round-trip tests alone, since a bug that's
symmetric (write it wrong, read the same wrongness back) would pass here and
still fail to open correctly elsewhere.

## 9. Progress

**Implemented** (`src/scene/dwg_native_constraints.rs`, wired into both save
paths in `src/app/update/file.rs` alongside the existing
`materialize_sketch_constraints_for_save`/`materialize_named_parameters_for_save`
calls). All 4 stages from §8 landed together rather than incrementally, since
`ConstraintKind` only has the 10 variants already covered — there was no
narrower slice left once Horizontal/Vertical's plumbing (network/dictionary
creation, the two-pass builder, class-table registration) was in place.
Covers: every `ConstraintKind` (`Coincident`, `Horizontal`, `Vertical`,
`Parallel`, `Perpendicular`, `Equal`'s line/circle split, `Distance`,
`Angle`, `Radius`, `Tangent`), `AssocGeomDependency` for whole-geometry nodes,
`AssocValueDependency`/`AssocVariable` for dimensional constraints including
`DrivingValue::Named`, and a resave-clears-and-rebuilds cleanup pass
(`remove_owned_recursive`) so materializing twice doesn't accumulate orphans.
Verified via real `save_to_bytes`/`load_bytes` round trips through
`acadrust`'s own reader/writer (5 tests in that module) — this confirms
self-consistency with `acadrust`'s implementation of the documented format,
**not** real-world AutoCAD/BricsCAD compatibility (§7.3/§7.5 below remain
unverified; no such software is available in this environment).

Two implementation-time findings not anticipated by the design above:

- **The top-level NOD-anchored network was dropped from scope.**
  `named_parameters_persist.rs`'s own doc comment already documents a real
  `acadrust` bug: a `Dictionary`-type object (like the database's Named
  Object Dictionary) used as an XRecord/dictionary owner loses its extension
  data through a DXF save specifically (`CadDocument::ensure_xrecord` only
  patches the owner's `xdictionary_handle` when `get_entity_mut(owner)`
  succeeds — a Dictionary isn't an entity, so it silently falls back to a
  DWG-only side map). Rather than hit that bug, this module only creates a
  network per `SketchScope::owner_handle()` (a real `BlockRecord` entity) —
  the same already-proven-safe attachment point `sketch_persist.rs` uses. No
  top-level network is created at all.
- **DXF cannot carry the dependency chain — DWG is the only fully-faithful
  format.** Confirmed by reading `acadrust`'s DXF writer
  (`io/dxf/writer/section_writer.rs`'s `is_unrestorable_assoc_object`): it
  deliberately skips writing `AssocDependency`/`AssocValueDependency`/
  `AssocGeomDependency`/`AssocVariable` objects to DXF at all
  ("Unrestorable associative-framework objects are not written") — DWG has
  no equivalent filter. A DXF save keeps the `Assoc2dConstraintGroup`/
  `AssocNetwork` shell and every node's own embedded geometry, but loses live
  geometry/value linkage and every `AssocVariable`. This module still
  materializes the same graph for both formats (the shell that survives DXF
  is strictly more than nothing, and dangling handles are an ordinary,
  tolerated pattern in both formats) rather than special-casing DXF to write
  less. Demonstrated directly by
  `a_dwg_save_keeps_the_full_dependency_chain_including_the_named_variable`
  vs. `a_dxf_save_keeps_only_the_constraint_group_shell` in that module's
  test suite.

Also confirmed while implementing (not previously documented): the DWG
writer's `Assoc2dConstraintGroup` arm only emits node data at all when
`nodes.first()` is `Some` — an empty `nodes` `Vec` skips node serialization
entirely — so `nodes[0]` must always be a synthetic root pseudo-node (empty
`class_name`, connected to every real node) even for a single-constraint
graph; and a multi-geometry constraint (e.g. `Parallel`) references its
second geometry via the generic `connections: Vec<i32>` field on
`AssocConstraintNode`, not a second dedicated field on
`AssocConstraintNodeData::Parallel` — confirmed from `AcConstraintGroupNode`'s
own doc comment ("the connection between two nodes is not directed").

Of the original five open questions, three were resolved from Autodesk's own
primary documentation (§7.1, 7.2, 7.4) before implementation started — the
network's dictionary key, `ImplicitPoint`'s addressing enum, and the
`AssocVariable` evaluator default were confirmed, not guessed. Two remain
open (§7.3 partial-graph tolerance, §7.5 version gating) and still genuinely
need a real AutoCAD/BricsCAD open, not just more reading — self-consistency
through `acadrust`'s own round trip can't settle either one. Next action is
opening a saved DWG in real software (or at minimum a third-party DWG
inspector) to make progress on those two, and posting the GitHub Discussion
Hakan asked for once that's done.
