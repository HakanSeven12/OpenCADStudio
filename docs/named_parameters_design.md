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

**Stage 1 done and tested.** `src/scene/named_parameters.rs`, new module
(registered `pub mod named_parameters;` in `src/scene/mod.rs`, alongside
`sketch_constraints`/`sketch_persist`/`sketch_solve`). Pure data + parser,
no document/UI integration yet, per this stage's own scope.

- **`Expr`**: a small AST (`Number`, `Ref`, `Neg`, `Add`/`Sub`/`Mul`/`Div`/
  `Pow`, `Call`) plus a hand-rolled tokenizer/recursive-descent parser
  (`parse`) — ordinary precedence (`+`/`-` < `*`/`/` < unary `-` < `^`,
  right-associative `^`), parens, and scientific-notation numeric literals
  (`1.5e-2`). **Deviation from this doc's own §2 suggestion** to check
  `evalexpr`/`meval` first: hand-rolled anyway, because cycle detection
  needs the set of names an expression references *before* it's evaluated
  (`Expr::refs`), and owning the parse tree gives that for free — treating
  evaluation as a black box the way a generic crate is normally used would
  just mean re-deriving the same dependency information some other way. No
  new external dependency added.
- **§5 open question 2 (expression language surface)**: resolved for v1 as
  arithmetic + a small, deliberately conservative builtin function set
  (`sqrt`, `abs`, `pow`, `min`, `max` — `call_builtin`), chosen as low-risk,
  commonly-needed CAD primitives rather than a considered product decision
  on the full surface. The grammar's `Call(String, Vec<Expr>)` node means
  adding more later needs no parser rewrite, matching this doc's own
  call-out. Builtin names are reserved (`is_reserved_name`) so a parameter
  can never collide with one — resolves part of open question 5 (naming
  collisions) alongside `is_valid_name`'s identifier rule (leading
  letter/underscore, then alphanumeric/underscore).
- **Cycle detection happens at `ParameterTable::set` time, not at
  resolve/evaluate time** — the design doc's "must be rejected" read as
  real-time rejection (matching a spreadsheet's usual UX), not a
  resolve-time error. `set` walks the dependency graph from the candidate
  formula's own references, through every already-stored formula, looking
  for a path back to the name being defined; a reference to a name that
  doesn't exist *yet* just terminates that branch rather than counting as a
  cycle (order of definition doesn't matter, same as a spreadsheet formula
  referencing an empty cell) — it only becomes
  `ParamError::UndefinedReference` if `resolve`/`resolve_all` actually needs
  that value later and it's still missing. A `resolve`-time cycle guard
  exists too (`resolve_inner`'s `stack` check) but is defense-in-depth only
  — unreachable through the public API, since `set` already refuses to
  create one; the code comment is explicit that this is a safety net (e.g.
  against a foreign/hand-edited file deserialized straight into a table
  without going through `set`), not a path the design expects to hit.
- **`resolve_all` isolates a failure to just the parameters it actually
  affects**, not the whole table — matching this codebase's established
  don't-let-one-bad-item-take-down-everything-else pattern from the
  constraint system's own dangling-reference handling.
- `ParameterTable::set` is a single upsert (define-or-redefine), not two
  separate "must not exist" / "must already exist" entry points — a later UI
  stage that wants add-vs-rename semantics can check `contains` itself
  first; not a decision this data-layer stage needed to make.
- 17 unit tests: precedence/associativity, scientific notation, malformed
  formulas, builtin arity/unknown-function errors, division by zero, name
  validation, literal and chained-reference resolution, undefined-reference
  reporting, direct and indirect (closes-later) cycle rejection, redefining
  a parameter to *break* a would-be cycle, `resolve_all` isolation,
  remove-then-resolve leaving a dependent gracefully undefined (not
  panicking), upsert semantics, insertion-order stability across
  redefinitions, and a `bincode` round-trip (both `Expr` and `Parameter`
  derive `Serialize`/`Deserialize` already, ready for stage 2's XRecord
  persistence — not exercised through the real save/load path yet, just
  in-memory `bincode::serialize`/`deserialize`, the same distinction stage 4
  of the sketch-constraint project drew between its own in-memory-agreement
  test and its real-save-path tests).

Full regression check: `cargo test --workspace`, 686 passed / 1 failed (the
same pre-existing, unrelated Turkish-glyph-fallback font test noted
throughout `docs/parametric_system_design.md`) / 13 ignored. `cargo clippy
--lib` shows no new warnings in `named_parameters.rs` (pre-existing warnings
elsewhere untouched).

**Still open after stage 1** (per §5): open question 1 (solid-history
scope), 3 (undo granularity), and 4 (per-document vs per-block-record table
scope) — none needed answering yet since nothing here touches the document
model or undo. Open question 2 and part of 5 got a working v1 answer above,
not a final product ruling.

**Stage 2 done and tested.** `src/scene/named_parameters_persist.rs`, new
module (registered `mod named_parameters_persist;` in `src/scene/mod.rs`),
following `sketch_persist.rs`'s exact precedent: the "lazy" model, a
version-prefixed `bincode` blob in one `Chunk`-typed `XRecordEntry`,
materialized right before save and read back right after load. Also went
ahead with the "wire it into open/save" half of stage 2 (not just the
spike) since it's the same four call sites `sketch_persist` already touches
and there was no reason to leave it half-done:
`OpenCADStudio::prepare_native_save` and the wasm save path
(`src/app/update/file.rs`), `OpenCADStudio::on_file_opened`'s native/web
open path (`src/app/update/file.rs:1393`), and the automation `"open"`/
`"new"` ops (`src/app/automation.rs`) — `named_parameters` lives on `Scene`
(`Scene::named_parameters: named_parameters::ParameterTable`, alongside
`sketch_constraints`, for the same co-location-with-`document` reason
stage 3 of the constraint-solver project gave), with `Scene::
named_parameters`/`named_parameters_mut` accessors.

- **A real round-trip gap found and worked around, not patched**: the
  obvious owner handle for a document-wide (not per-scope) XRecord is
  `document.header.named_objects_dict_handle` — the vendored `cadcodec`
  source's own `extension_dictionary_handle` explicitly resolves an
  extension dictionary for a `Dictionary`-type owner, and that crate's own
  test suite (`tests/issue51.rs`) uses exactly that handle as an XRecord
  owner. In practice this round-tripped through DWG but **silently dropped
  the XRecord through DXF** — caught by this stage's own real-bytes save/
  load tests, not assumed to work from reading the source alone (the same
  discipline `sketch_persist`'s stage 4 applied). Root cause, traced into
  the vendored source: `CadDocument::ensure_xrecord` only writes the
  *owner's own* `xdictionary_handle` field when `get_entity_mut(owner)`
  succeeds (i.e. `owner` is a real "entity"); a `Dictionary` owner like the
  root NOD isn't one, so `ensure_xrecord` falls back to a runtime-only side
  map (`xdic_by_handle`) instead. The DWG writer consults that side map;
  grepping `io/dxf/writer/` for it turns up nothing — the DXF writer only
  ever reads the owner's own `xdictionary_handle` field, never the side map,
  so a Dictionary-owned extension dictionary is simply invisible to a DXF
  save. This is a gap in the vendored crate for this specific owner-type
  combination. Per this project's own stated preference (§1.4 of the sibling
  design doc: "requires zero changes to the external `acadrust`/`cadkernel`
  crates") the fix taken was to pick a different owner rather than patch
  the dependency: **`document.header.model_space_block_handle`** — a real
  `BlockRecord` entity, the exact anchor `sketch_persist`'s own
  `SketchScope::ModelSpace` case already proved clean through both formats.
  Not a perfect semantic fit (a parameter table has nothing to do with model
  space specifically) but it's guaranteed to exist in every `CadDocument`
  and is a real entity, which is the property that actually matters. Worth
  revisiting if a later need for a true document-root-anchored XRecord comes
  up — the fix would be in `cadcodec` itself (teach `ensure_xrecord` to also
  set a `Dictionary`/`Layout`/`XRecord`/`PlotSettings`/`VisualStyle`/
  `Material`/`ProxyObject` owner's own `xdictionary_handle` field, not just
  entities'), not something to route around a second time.
- 5 new tests (`named_parameters_persist.rs`): in-memory materialize+reload
  agreement, real `save_to_bytes`/`load_bytes` round trips for both DXF and
  DWG (the two that caught the gap above), an empty table not persisting,
  and format-version rejection — same shape as `sketch_persist`'s own test
  suite.

Full regression check: `cargo test --workspace`, 691 passed / 1 failed (same
pre-existing, unrelated font test) / 13 ignored. `cargo clippy --lib` shows
no new warnings in either `named_parameters.rs` or
`named_parameters_persist.rs`.

**Stage 3 done and tested.** `SketchConstraint::driving_param` is now
`Option<named_parameters::DrivingValue>` (`sketch_constraints.rs`) instead
of `Option<f64>` — exactly this doc's own §2 suggested shape:

```rust
pub enum DrivingValue {
    Literal(f64),
    Named(String),
}
```

with `DrivingValue::resolve(&self, table: &ParameterTable) -> Result<f64,
ParamError>` (`named_parameters.rs`) doing the lookup — a literal resolves
to itself, a named reference resolves through the table.

- **Solve-side wiring**: `sketch_solve.rs`'s `build_constraint`/`solve_scope`
  now take a `&ParameterTable` (threaded from `Scene::named_parameters` at
  both call sites, `refresh_sketch_constraints` and
  `solve_sketch_constraints_preview`) and resolve `driving_param` at the
  three existing call sites (Distance/Angle/Radius) via `c.driving_param.
  as_ref()?.resolve(params).ok()?` — a resolve failure (an undefined
  reference, a division by zero; a cycle should be unreachable since `set`
  already refuses to create one) is treated exactly like every other
  unbuildable-constraint case `build_constraint`'s own doc comment already
  documents: the constraint is skipped for this solve, not an error.
- **Judgment call, not dictated by this doc**: how a resolve failure should
  surface wasn't an open question this doc raised explicitly, so the
  existing precedent decided it — matching a dangling-reference / unbuildable
  constraint rather than inventing a new "parameter error" state. A richer
  surfacing (e.g. a distinct glyph color, a status-bar warning naming the
  broken reference) is possible future work once stage 4's UI exists to
  make it discoverable, not something this data/solver-layer stage needed
  to add speculatively.
- **`CmdResult::AddSketchConstraint` intentionally left as `Option<f64>`**
  (`src/command.rs`) rather than also switching it to `DrivingValue` — no UI
  can author a `Named` reference yet (that's stage 4's picker), so every
  existing call site (`DistanceConstraintCommand`/`AngleConstraintCommand`
  in `src/modules/draw/constrain/value.rs`, the Horizontal/Vertical/etc.
  dispatch in `src/app/commands/draw.rs`) needed no changes at all; the one
  conversion point is `command_driver.rs`'s `CmdResult::AddSketchConstraint`
  handler, which now wraps the literal into `DrivingValue::Literal` right
  before calling `SketchConstraintSet::add`. Keeps this stage's diff to the
  data/solver layer plus one narrow seam, not a ripple through every typed-
  value UI command.
- **`glyph_label`** (`sketch_constraints.rs`) now matches on `&DrivingValue`
  instead of a bare `f64`: a literal renders exactly as before, a named
  reference renders its name (e.g. "↔ hole_dia") rather than a resolved
  number — showing the resolved value too would need threading
  `ParameterTable` into the glyph-render call site
  (`src/ui/overlay.rs`), which is UI-layer work for stage 4, not this one;
  this arm exists only so the match stays exhaustive ahead of that UI (no
  command can produce a `Named` driving_param yet, so it isn't reachable
  today).
- **`sketch_persist::FORMAT_VERSION` bumped 1 → 2**: `SketchConstraint`'s
  serialized shape changed (a bare `f64` became an enum with its own
  discriminant), so a version-1 blob's bytes no longer line up with the
  current struct — exactly the "detected and gracefully skipped" case that
  version byte exists for (§5.2's own call-out: no migration path exists,
  bumping the version is the correct response, not a substitute for one). A
  previously-saved file's sketch constraints will not reload after this
  change; acceptable for personal-use, pre-release local work, not something
  a real migration was owed here.
- 2 new tests (`tests/sketch_constraints_solve.rs`): a `Distance`
  constraint driven by a named reference solves correctly and re-solves
  when the parameter's own value is redefined; a `Distance` constraint
  referencing an undefined name is skipped without panicking, leaving the
  triggering edit untouched (the same contract the existing "unrelated edit
  doesn't trigger a resolve" test already established for dangling
  refs). Every existing `driving_param`-touching test (in
  `sketch_constraints.rs`, `sketch_persist.rs`,
  `tests/sketch_constraints_xrecord_roundtrip.rs`,
  `tests/sketch_constraints_solve.rs`) updated to construct
  `DrivingValue::Literal(...)` instead of a bare float; all still pass.
- **Testing-methodology note, not a code defect**: discovered while
  re-verifying this stage that plain `cargo test --workspace` stops after
  the first *crate's* test failures and silently skips the remaining
  integration-test binaries in the workspace — the pre-existing, unrelated
  font-fallback failure was silently truncating every regression check this
  whole project has run with a bare `cargo test --workspace`. Re-ran with
  `--no-fail-fast` for this stage's check and confirmed every binary (not
  just the lib crate) passes except that one known-unrelated test. Worth
  using `--no-fail-fast` for any future full-workspace check in this repo.

Full regression check: `cargo test --workspace --no-fail-fast`, every test
binary passing except the same pre-existing, unrelated font test noted
throughout this document and its sibling. `cargo clippy --lib --tests`
shows no new warnings on any line this stage touched (the two warnings that
do appear in `sketch_solve.rs`/`scene/mod.rs` predate this stage and are on
code this stage didn't modify).

**Stage 4 done and tested (not live-verified in the running app — see the
note below).**

- **The parameters panel** (`src/ui/window/named_parameters.rs`, new): an
  in-canvas modal opened by a new `PARAMETERS` command (AutoCAD's own name
  for the equivalent feature — `src/app/commands/view.rs`, dispatched
  alongside `ALIASEDIT`; registered in the autocomplete/MCP command list
  from the start, unlike the six pre-existing constraint commands
  `docs/backlog.md` flags as missing that registration). Built by mirroring
  `alias_editor.rs`'s buffered-rows pattern as closely as possible rather
  than inventing new UI conventions: rows are `(name, formula)`, edited in a
  working buffer (`OpenCADStudio::named_parameter_editor_rows`,
  `ModalKind::NamedParameters`) and only committed to `Scene::
  named_parameters` on Apply — a better fit here than for aliases, even:
  `ParameterTable::set` validates (and can reject) a formula immediately, so
  writing through to the live table on every keystroke would fight a user
  mid-type.
- **Live preview column, beyond what the alias editor has**: each row also
  shows its resolved value or its error, recomputed from a scratch
  `ParameterTable` built from the *whole buffer's current text* on every
  render (`preview`, private to the UI module, unit-tested directly) — not
  cached, matching the "cheap enough to rebuild from scratch" philosophy
  `ParameterTable`/`sketch_solve.rs` already use throughout. This means a
  cross-row problem (a forward reference, a cycle that would close between
  two draft rows) shows up immediately, before Apply, not just after a
  failed commit.
- **A real gap found while building the preview, not by design**: two rows
  typed with the same name would otherwise silently collapse — `set`
  upserts by name, so the buffer's later row would just overwrite the
  earlier one with no indication anything was discarded. Added
  `duplicate_name_rows` (checked first, before any `set` call, by both the
  preview and the real Apply) to flag every row sharing a duplicated name as
  its own error instead of silently picking one arbitrarily.
- **The other half of stage 4 — "a way to pick a named parameter instead of
  a literal"**: resolved as letting the *same* value prompt accept either,
  rather than building a separate picker UI. `DistanceConstraintCommand`/
  `AngleConstraintCommand` (`src/modules/draw/constrain/value.rs`) now take
  a one-time snapshot of the active tab's parameter names at construction
  (`known_param_names` — the same `default_value`-is-a-one-time-snapshot
  precedent these commands already relied on, since `on_text_input` has no
  document access); `parse_driving_value` tries a numeric literal first,
  falls back to an exact match against that snapshot, and returns neither if
  the typed token is unrecognized (the prompt's existing "invalid input"
  behavior, unchanged). This mirrors how AutoCAD's own dimensional-
  constraint prompts already accept an expression in place of a bare value,
  and needed no new interaction design. `CmdResult::AddSketchConstraint`'s
  `driving_param` (left as a bare `Option<f64>` in stage 3, deliberately,
  exactly so this stage could revisit it) is now `Option<DrivingValue>`.
- **Apply re-solves what it affects**: because editing a named parameter is
  the entire point of the feature ("editing the named value ripples to
  every constraint that references it," §1), Apply doesn't just rebuild the
  table — it also gathers every entity handle touched by an enabled
  constraint whose `driving_param` is `DrivingValue::Named` (across every
  scope; the whole table changed, not one isolated value, so there's no
  cheaper "which handles actually changed" test worth doing) and runs them
  through the same `begin_undo`/`bump_entities`/`commit_undo_delta` bracket
  `resolve_one_sketch_conflict` already established
  (`OpenCADStudio::apply_named_parameter_editor_rows`, `command_driver.rs`).
- **Judgment call on undo granularity (§5 open question 3), not fully
  resolved**: the geometry Apply moves is one undo step (bracketed as
  above), matching how every other solve-triggering action in this codebase
  behaves. The parameter *table* edit itself has no undo entry of its own
  yet — that would need a new `HistorySnapshot` variant mirroring
  `SketchConstraints` (the sibling project's own stage 9), which this pass
  didn't build. Documented, not silently dropped, exactly like stage 9's own
  "two presses, not one" scope-down: undoing after an Apply undoes the
  geometry it moved, not the parameter values.
- 15 new tests: 3 for `parse_driving_value` (literal-first, name match,
  unrecognized token), 6 for the panel's `preview` (cross-row reference,
  forward reference regardless of buffer order, a cycle flagged on the row
  that closes it, a blank row left unflagged, a malformed formula, the
  duplicate-name fix), and 6 integration tests in `command_driver.rs`
  (applying rows defines the parameter *and* re-solves a constraint that
  already referenced it by name — including re-applying after redefining the
  parameter; a malformed row is reported as an error and doesn't block a
  good row alongside it; the duplicate-name fix exercised end to end through
  `apply_named_parameter_editor_rows`, not just the UI-layer preview).
- **Testing-methodology note carried over from stage 3**: verified with
  `cargo test --workspace --no-fail-fast` throughout this stage too, for the
  same reason noted there.

Full regression check: `cargo test --workspace --no-fail-fast`, 703 tests
passing, only the same pre-existing, unrelated font-fallback failure noted
throughout this document and its sibling. `cargo clippy --lib --tests` shows
no new warnings on any line this stage touched.

**Not live-verified in the running app this pass** — a real gap, not a
formality skipped by habit: this is genuinely new UI surface (a whole modal
panel, plus a new branch in an existing text-input prompt), and static
review/unit tests can't catch every real-app integration issue (layout,
widget-API misuse that only manifests at runtime, focus/keyboard handling).
Per `docs/backlog.md`'s own live-testing notes, exercising it for real needs
a release build swapped into a scratch copy of the installed app bundle
(`open_application`/`request_access` always resolve to the `/Applications`
copy, and the debug binary doesn't reliably present a controllable window in
this environment) — a real time investment this pass didn't spend, flagged
explicitly rather than silently assumed to work. First things to check in a
future live-testing pass: the panel actually opens via `PARAMETERS`, the
value column updates live while typing, the duplicate/cycle/malformed-
formula error text renders legibly, Apply visibly moves geometry for a
constraint driven by a named parameter, and typing a parameter's name at a
`DCONSTRAINT`/`ACONSTRAINT` prompt is actually accepted.

## Follow-up work not yet done

- **Solid-history `PROP_*` values** (§2's own flagged scope decision, open
  question 1) — named parameters currently only reach `SketchConstraint::
  driving_param` (Distance/Angle/Radius), not `solid_history.rs`'s box
  length/cylinder radius/etc. Not started; a decision to make, not assumed.
- **Constraint-table-edit undo** (open question 3, only partially resolved —
  see above).
- **Reserved-word/name-collision UX polish**: `ParameterTable::set` already
  rejects an invalid or builtin-colliding name (stage 1); the panel surfaces
  that rejection as the row's error text via `preview`/Apply, but there's no
  live per-keystroke nudge beyond that (e.g. graying out `Apply` while any
  row has an error) — a small, optional polish, not a functional gap.
- **Live UI verification**, per the note above.
