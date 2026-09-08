# Named Parameters — Design & Handoff

**Status: not started.** This document is the starting context for a new
coding session (mirrors how `docs/parametric_system_design.md` was used for
the sketch-constraint-solver project — read that document's own "Progress"
section for the sibling feature this one builds alongside). The plan file at
`/Users/felix/.claude/plans/snoopy-finding-matsumoto.md` indexes both.

## 1. What this is, and why

OpenCADStudio now has real sketch-level parametric constraints (Coincident,
Horizontal/Vertical, Parallel/Perpendicular, Equal, Tangent, Distance, Angle,
Radius — see `docs/parametric_system_design.md`, fully built and tested).
What it does **not** have is a way to drive several of those dimensional
values from one shared, named, formula-capable source — FreeCAD calls this
its Spreadsheet workbench; AutoCAD calls it the **Parameters Manager**
(`PARAMETERS` command, since AutoCAD 2010) — a table of named variables
(`hole_dia`, `plate_len`, …) that a dimensional constraint's value can
reference instead of a bare literal, so editing the named value ripples to
every constraint that references it. This is explicitly the AutoCAD-parity
gap identified in the "AutoCAD equivalent" discussion this session (see
§7 for that framing) — we are not inventing a new concept, we're filling in
a specific, well-precedented one.

This is a genuinely separate feature from the constraint solver itself: it
needs a new document-level object, an expression parser, and a change to how
driving values are stored — not new `ocs_gcs` solver math.

## 2. Prior architecture decisions (from design discussion, not yet coded)

- **New document-level table object**, one per document (not per-scope/sketch
  — parameters are meant to be shared across a whole drawing), holding
  `name -> literal-or-formula`. Likely home: mirror
  `src/scene/sketch_persist.rs`'s pattern exactly — an `XRecord`/`Dictionary`
  blob on a well-known owner (that file's `XRECORD_KEY` /
  `materialize_*_for_save` / `load_*_from_document` shape is the direct
  precedent to copy, including the version-prefixed bincode envelope and the
  hook points into `on_file_opened` / `prepare_native_save` / the wasm save
  path / the automation `"new"`/`"open"` ops).
- **Expression parser/evaluator** — a small formula language (`2 * hole_dia +
  1.5`), with dependency-cycle detection (A referencing B referencing A must
  be rejected, not silently infinite-loop or panic). No existing dependency
  in the workspace does this — check crates.io options (e.g. `evalexpr`,
  `meval`) against the license stack (GPLv3 app; anything permissive/LGPL is
  fine) before hand-rolling one.
- **Driving-value call sites become "literal or named reference"**. The
  concrete, known change: `SketchConstraint::driving_param` is currently
  `Option<f64>` (`src/scene/sketch_constraints.rs:97`), consumed directly in
  `sketch_solve.rs` (`build_constraint`, lines ~184/190/195, one per
  Distance/Angle/Radius) via `sys.add_param(c.driving_param?, ...)`. This
  would become something like `DrivingValue::Literal(f64) |
  DrivingValue::Named(String)`, resolved against the parameter table at
  solve time (same rebuild-from-scratch-per-trigger model the constraint
  solver already uses — no incremental re-evaluation needed, consistent with
  `refresh_sketch_constraints`'s existing "full rebuild per trigger, not
  incremental" design choice).
- **Solid-history properties are a second, separate call-site family** —
  `src/scene/model/solid_history.rs`'s `PROP_LENGTH`/`PROP_HEIGHT`/etc. are
  plain `f64` fields on structs like `SolidHistoryBox` (e.g. `value.length`,
  set via the generic property-editing dispatch at `solid_history.rs:2239`).
  Whether named parameters should reach these too (so a box's height can
  reference the same `plate_thickness` a sketch dimension uses) is a scope
  decision for this session to make explicitly, not assume — see open
  questions below. If yes, it's the same "literal or reference" change
  repeated across a different, larger set of call sites (many more `PROP_*`
  constants than there are `ConstraintKind`s).

## 3. Explicitly out of scope for this feature (confirmed in prior discussion)

- **Live sketch-to-3D linking** (extrude/sweep/loft tracking a sketch's
  constraint solve) is a separate, larger project. Confirmed this session:
  `SolidHistorySweep`/`Revolve`/etc. hold `sweep_entity: Option<EmbeddedEntity>`
  — a **snapshotted copy** of the profile geometry, not a live `Handle` back
  into the sketch (`src/scene/model/solid_history.rs`, grep `sweep_entity`).
  Named parameters can reference solid-history properties (per §2's open
  question) without needing this link — that's a narrower, additive step.
  Do not conflate the two.

## 4. Suggested staged approach

Mirroring what worked well for the constraint-solver project (`docs/
parametric_system_design.md`'s own staged plan): small, independently
testable stages, each with its own automated tests, progress notes appended
to this document (not a separate changelog) after each stage, live UI
verification where the stage has new UI surface.

1. Data model: the parameter table type + expression parser, unit-tested in
   isolation (parse, evaluate, cycle detection) with no document/UI
   integration yet.
2. Persistence spike: XRecord round-trip for the new table, following
   `sketch_persist.rs`'s exact precedent — prove save/load byte-fidelity
   before wiring anything else to it (this is the same order stage 2 of the
   constraint-solver plan used, and it caught real assumptions early there).
3. Wire `SketchConstraint::driving_param` to resolve through the table
   (Distance/Angle/Radius constraints only, the three existing call sites).
4. UI: a parameters panel (add/rename/delete a named value, edit its
   formula, see its resolved number and any error) — likely the
   `src/ui/style/dimstyle.rs`-pattern panel already used elsewhere in this
   codebase, and a way to pick "use a named parameter" instead of typing a
   literal wherever a Distance/Angle/Radius constraint's value is entered
   today (`src/modules/draw/constrain/value.rs`'s `DistanceConstraintCommand`
   / `AngleConstraintCommand`).
5. (If scoped in per §2/§3) extend to `solid_history.rs`'s `PROP_*` values.

## 5. Open questions to resolve before/while coding (not resolved by this doc)

1. **Solid-history scope** — does v1 include primitive solid dimensions
   (box length, cylinder radius, …), or sketch dimensional constraints only?
   Affects how much of §4's stage 5 is in scope.
2. **Expression language surface** — arithmetic only, or also functions
   (`min`, `sqrt`, trig)? AutoCAD's Parameters Manager supports a fairly rich
   set; matching all of it is not required for v1 but the parser's grammar
   should be chosen so functions can be added later without a rewrite.
3. **Undo granularity for a parameter edit** — a named-parameter edit can
   ripple through many constraints across many scopes in one edit, similar
   to (bigger than) a single constraint's own resolve. Should probably reuse
   the same `record_undo_before`/one-history-entry-per-trigger model
   `refresh_sketch_constraints` already established, but confirm rather than
   assume.
4. **Where does the parameter table live for multi-document workflows?**
   Per-document (confirmed in §2) — but does a block/xref carry its own
   independent table, or only the top-level document? `sketch_persist.rs`'s
   per-`BlockRecord` XRecord scanning pattern suggests per-block-record is
   natural to copy, but sketch scopes and a shared parameter table are not
   the same kind of thing (scopes are naturally per-sketch; parameters are
   naturally document-wide) — don't copy the pattern mechanically without
   checking this distinction holds.
5. **Naming collisions / reserved words** — needs a validation rule (valid
   identifier characters, no collision with another parameter, sensible
   error on a malformed formula) before the UI stage.

## 6. Progress

Nothing built yet. Update this section (not a separate file) after each
stage, matching the reporting pattern used throughout
`docs/parametric_system_design.md`'s own "Progress" section.
