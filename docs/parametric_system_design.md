# Persistent Parametric Constraint System — Design

Status: design/research only, no code written against this doc yet.
Scope: Section 5 (document-model integration) and Phase B/C of Section 4 (UI)
of `/Users/felix/.claude/plans/snoopy-finding-matsumoto.md`, made concrete
against the actual OpenCADStudio codebase as of this session.

The one-shot "Constraints" ribbon group already live in the app
(`src/modules/draw/constrain/{mod.rs,tools.rs,value.rs}`, dispatched from
`src/app/commands/draw.rs` lines 960–1029) is explicitly a Fillet/Trim-style
single operation — it snapshots coordinates into a fresh `ocs_gcs::ParamStore`,
solves once, writes back, and remembers nothing. This document designs the
real thing: constraints that attach to entities, persist through save/load,
and re-solve automatically when constrained geometry changes.

## 1. What the investigation found

### 1.1 Document/entity model and undo/redo

`Scene` (`src/scene/mod.rs:1501`) wraps `pub document: CadDocument`
(`acadrust::CadDocument`, an external crate — see §1.4). Entities live in a
flat store addressed by `acadrust::Handle`, which is a newtype over `u64`
(`.../cadcodec-.../src/types/handle.rs:13`) and is **the actual DWG/DXF
persistent handle**, not a session-local index. `CadDocument::allocate_handle`
(`.../document.rs:1965`) mints new ones; nothing in the codec reuses a handle
after an entity is erased (erase leaves a hole, undo/redo of an erase
restores the original handle via `restore_entity_arc`, `src/scene/mod.rs:2567`).
**Handles are stable across undo/redo and across save/load** — this is the
foundation the whole addressing scheme below relies on.

Undo/redo is a **delta model**, not full-document snapshots for ordinary
edits (`src/app/document.rs:642-812`, `src/app/history.rs`):

- `Scene::begin_undo_recording` (`src/scene/mod.rs:2480`) opens an
  `UndoRecording` that captures, per handle, the **first-touch before-image**
  (`record_undo_before`, `:2499`, no-op if not recording — callers guard with
  `is_recording_undo()`).
- The app-level wrapper is `OpenCADStudio::begin_undo`
  (`src/app/history.rs:372`) / `commit_undo_delta` (`:701`), which pairs each
  recorded before-image with the entity's current after-image and pushes one
  symmetric `DeltaSnapshot` (`src/app/document.rs:723`) onto the undo stack.
  Undo applies the before side, redo the after side — same allocation moves
  between stacks (`src/app/document.rs:717-720` doc comment).
- `Scene::bump_entities(&mut self, changes: &[(Handle, ChangeKind)])`
  (`:2612`) is the single choke point every mutation goes through: it bumps
  `geometry_epoch`, invalidates render/pick caches, and — critically for this
  design — **already runs a chain of derived-geometry recompute passes
  in-line**, before the epoch bump:
  - `refresh_associative_centerlines` / `refresh_associative_center_marks`
    (`src/scene/centerline.rs:327`, `src/scene/centermark.rs:353`)
  - `refresh_associative_dimensions` (`src/scene/dimension_assoc.rs:880`)
  - `refresh_associative_hatches` (`src/scene/boundary.rs:680`)

  Each of these takes the incoming `changes` slice, finds document objects
  that depend on the changed handles, recomputes their derived geometry, and
  **appends its own `(Handle, ChangeKind::Modified)` entries to the same
  `changes` vec** (`src/scene/mod.rs:2624-2643`) rather than calling
  `bump_entities` again — so there is no re-entrant bump and no infinite
  loop. The centerline/centermark passes additionally call
  `is_recording_undo()` + `record_undo_before()` before they mutate
  (`src/scene/centerline.rs:338-340`, `src/scene/centermark.rs:343-345`, etc.)
  so their writes fold into whatever undo delta the *triggering* edit is
  already recording, producing one undo step for the whole cascade. The
  dimension/hatch passes do not bother (their output is fully re-derivable
  from source geometry, so it doesn't need its own undo record).

  **This is precisely the mechanism a `SketchConstraintSet` re-solve needs,
  and the precedent already establishes the exact call contract to match.**

- `changes_touch_block_definition` (`:2698`) and the `layout_blocks` /
  `BlockRecord` scan there confirms **block records are the app's only
  existing sub-document/grouping concept**. `BlockEditSession`
  (`src/modules/draw/modify/block_edit.rs:47`) scopes an interactive edit
  session to one `br_handle: Handle` (a block definition), with its own
  entity snapshot and camera. There is no "Sketch" entity or block-scoped
  named group beyond ordinary block records — see §3.1.

### 1.2 Sub-element (point-level) identity — already exists, twice

The task's open question #2 ("can you address 'the start point of line
handle 7' independently of the whole line?") has a concrete answer: **yes,
two independent schemes already do this**, both usable as a template:

1. **Grip identity.** `GripDef` (`src/scene/model/object.rs:105`) has
   `pub id: usize`, documented as "Object-local identifier (stable index,
   unique per object instance)". A `GripEdit`/`GripTarget`
   (`src/scene/pick/grip.rs:29-56`) addresses a drag target as
   `(handle: Handle, grip_id: usize)`. For a `Line`, grip 0/1 are (by
   established convention in every entity's grip producer) the endpoints.
2. **Associative-dimension reference identity**, which is the closer match
   because it is a *persisted, serialized* sub-element reference, not a
   transient UI index. `AssocDimensionReference`
   (`.../cadcodec-.../src/objects/associative.rs:1253`) has
   `xrefs: Vec<Handle>` (the source entity) + `main_gs_marker: i32` (an
   AutoCAD "graphical subentity marker" — the same GsMarker concept OSNAP
   uses). `resolve_reference` (`src/scene/dimension_assoc.rs:342-409`) shows
   the resolution rule: negative markers are special cases (circle/arc
   center = `-3`, point-at-parameter = `-2`), and a non-negative marker
   indexes into `source_points(entity)` — an ordered `Vec<Vector3>` of named
   points per entity type (line start/end, etc.), i.e. **exactly the
   GsMarker-indexed point-list scheme `ocs_gcs` constraint endpoints need**,
   and it is literally what real DWG/DXF files already carry for OSNAP-based
   dimension associativity — this is not an invented scheme, it's the format's
   own.

Neither scheme currently exists as a reusable public API outside
`dimension_assoc.rs`/`grip.rs` — `source_points()` is private to that module
(confirm before depending on it; it may need to move to a shared location, or
the constraint layer defines its own equivalent enumeration function per
entity type — see open questions).

### 1.3 Re-solve trigger mechanics — no push-based observer exists

There is **no general "on change" callback/subscriber system** in `Scene`.
Change propagation is pull/epoch-based: `geometry_epoch` + `GeometryDelta`
journal (`src/scene/mod.rs:372-386`) that consumers replay via
`replay_since` (`:2307`), used for GPU buffer diffing and picking-index
maintenance — not a generic reactive hook a new module could subscribe to
independently. The only place that currently reacts synchronously to a
mutation *before* it's finalized is the in-line derived-geometry chain
inside `bump_entities` itself (§1.1). **This settles the trigger design: a
constraint re-solve must be another link in that same chain**, not a
separately-registered listener — see §4.

`ocs_gcs::system::System` (`crates/ocs_gcs/src/system.rs:22-58`) has no
incremental/dirty-tracking API: `add_param`/`add_constraint`, then
`partition()` rebuilds `SubSystem`s from scratch. There is no "touch this
constraint and re-partition only its component" operation. This matches
FreeCAD's own planegcs usage (a full rebuild-and-resolve per sketch
recompute) and settles the naive-vs-incremental question: **always rebuild
a fresh `System`/`ParamStore` from current document coordinates + the
persisted constraint records, every re-solve** (§4.2) — building an
incremental solver session is out of scope and not something the existing
crate API supports without new work.

Grip-drag preview state (`GripEdit`/`preview_wires`,
`src/scene/mod.rs:1601-1609`) is kept separate from the committed document —
the document (and therefore `bump_entities`) is only touched at drag
commit, not per mouse-move frame. This is the natural debounce point: a
live drag already doesn't call `bump_entities` per frame, so a constraint
re-solve hooked into `bump_entities` automatically only fires at commit,
with no extra debounce logic needed for the drag case. (It *will* fire once
per discrete edit — one click of a Move/Stretch/grip commit, one property
edit, one paste — which is the right granularity, matching how
`refresh_associative_dimensions` etc. already behave.)

### 1.4 File format and persistence

Default save format is DWG (AC1032/R2018+) via `acadrust`
(`src/io/mod.rs:4`, `save_as_version`/`save_to_bytes` at `:1546`/`:1717`);
DXF is also supported through the same codec. **There is no separate
"OpenCADStudio native" serde dump of the whole document** — `serde`/
`serde_json`/`bincode` in `Cargo.toml` are used elsewhere (plugin IPC,
`ocs_doc_api`, see `src/app/doc_api.rs`), not for saving drawings. This means
**any new Rust struct added to `Scene`/`DocumentTab` that isn't threaded
through `acadrust`'s object model will silently NOT survive a save/load
round-trip** — it would only live for the session. A `SketchConstraintSet`
must be represented as data the DWG/DXF codec actually writes and reads.

Two ways to make that true, found in `acadrust`:

- **Native option**: `ObjectType::Associative` with
  `AssociativeData::ConstraintGroup(Assoc2dConstraintGroup)`
  (`.../objects/associative.rs:74`, `:1069-1091`) is AutoCAD's own
  `AcDbAssoc2dConstraintGroup` — the real object AutoCAD's "Geometric
  Constraints" feature uses. It has a full, real writer (not a passthrough
  stub) in both DWG (`.../io/dwg/dwg_stream_writers/object_writer/associative.rs:791-852`)
  and DXF (`.../io/dxf/writer/section_writer/associative.rs:787`). Its node
  vocabulary (`AssocConstraintNodeData`, `:956-1065`) covers
  Point/Line/Circle/Arc/Ellipse geometry nodes and Angle/Distance/
  RadiusDiameter/Parallel dimensional/geometric nodes with
  `geometry_dependency: Handle` + `geometry_node_id: i32` — but it does
  **not** have explicit node kinds for Coincident, Horizontal, Vertical,
  Equal, Perpendicular, or Tangent, and its `connections: Vec<i32>` graph
  encodes AutoCAD's own internal action-dependency network, which is not
  publicly documented and was itself reverse-engineered into this crate.
  Faithfully mapping all ~30 `ocs_gcs` constraint types onto this vocabulary
  1:1, in a way a real AutoCAD build would also interpret correctly, is a
  substantial and separately-risky undertaking — **not recommended for v1**
  (see recommendation below).
- **Extension option**: `ObjectType::XRecord(XRecord)`
  (`.../objects/xrecord.rs:383-403`) is the standard AutoCAD ARX
  extensibility object third-party applications use to carry private data
  through a DWG/DXF file without needing format-level support for their
  specific feature. `XRecordEntry`/`XRecordValueType` includes a `Chunk`
  (binary blob) variant (`:57-82`). XRecords hang off a `Dictionary`
  (`.../objects/mod.rs:136`) via `xdictionary_handle`, and the document's
  root is `CadDocument.header.named_objects_dict_handle`
  (`.../document.rs:580`, initialized at `:1529`) — the exact place
  well-behaved third-party AutoCAD data already lives in real DWG files.

**Recommendation: use the XRecord/Dictionary extension mechanism, not
`Assoc2dConstraintGroup`.** Concretely: serialize the whole
`SketchConstraintSet` for one scope (§3.1) with a stable, versioned binary
format (the app already depends on `bincode`) into one `XRecord`'s `Chunk`
entry, owned by a per-scope `Dictionary` entry hung off
`named_objects_dict_handle` under an app-specific key (e.g.
`"OCS_SKETCH_CONSTRAINTS"`). This requires **zero changes to the external
`acadrust`/`cadkernel` crates**, rides an already-fully-implemented
read/write path, and only needs OpenCADStudio's own reader/writer for the
blob's *contents* to agree with itself — no AutoCAD-compatibility burden.
The tradeoff, called out explicitly as an open risk in §7, is that a DWG
saved this way will show no constraint data if opened in real AutoCAD or
another vendor's app (it'll just look like an inert XRecord); pursuing true
`AcDbAssoc2dConstraintGroup` interop is a valid future enhancement once v1
ships, not a blocker.

## 2. Core data structures

All in a **new module** `crates/ocs_gcs`-adjacent but living in the app
crate (not `ocs_gcs` itself, which must stay UI/document-free per the
existing port plan) — proposed at `src/modules/sketch/constraint_set.rs`,
alongside a `constraint_map.rs` mirroring the plan's Section 4 note.

```rust
/// One endpoint a constraint attaches to: an entity handle plus which
/// sub-element of it. Mirrors the existing AssocDimensionReference /
/// GsMarker scheme (§1.2) rather than inventing a new one.
pub struct SketchRef {
    pub entity: acadrust::Handle,
    /// None = the entity as a whole (radius/length/whole-curve constraints).
    /// Some(marker) = an indexed sub-point, same convention as
    /// `main_gs_marker` / `source_points()`: non-negative indexes a
    /// per-entity-type ordered point list (0/1 = line start/end, etc.);
    /// -3 = circle/arc center; -2 = point-at-parameter (osnap_distance
    /// carries the parameter). Reuses rather than reinvents.
    pub marker: Option<i32>,
}

/// One user-facing constraint record: the friendly type plus its
/// endpoints and (for dimensional constraints) a driving value.
pub struct SketchConstraint {
    pub id: ConstraintId,             // stable within one SketchConstraintSet
    pub kind: ConstraintKind,         // Coincident, Horizontal, Parallel, Distance{value}, ...
    pub refs: SmallVec<[SketchRef; 2]>,
    pub driving_param: Option<f64>,   // e.g. typed distance/angle/radius value
    pub enabled: bool,                // user can suppress without deleting
}

pub type ConstraintId = u32;

/// Everything ocs_gcs needs to solve one scope's sketch, plus enough to
/// re-derive it. Rebuilt-and-solved wholesale on every trigger (§4.2) — no
/// incremental state is retained here beyond the constraint records
/// themselves.
pub struct SketchConstraintSet {
    pub scope: SketchScope,                      // §3.1
    pub constraints: Vec<SketchConstraint>,
    next_id: ConstraintId,
}
```

`ConstraintKind` is the friendly-name enum from the plan's Section 4
(`Coincident`, `Horizontal`, `Vertical`, `Parallel`, `Perpendicular`,
`Equal`, `Distance`, `Angle`, `Radius`, `Tangent`, …) — the mapping layer
already sketched as `constraint_map.rs` in the plan's Phase A. Each variant
maps to one or more `ocs_gcs::constraints::*` primitive constructions,
exactly as `apply_horizontal`/`apply_parallel`/etc. in
`src/modules/draw/constrain/tools.rs:83-200` already do per-call — the new
code differs only in *persisting* the record and re-running that same
mapping on every solve instead of once.

### 2.1 Where it's stored at runtime

Not in `CadDocument` (external crate, don't touch) and not bolted onto
`Scene` as a bare field either — add it to `DocumentTab`
(`src/app/document.rs:106-218`), the per-tab session state, as:

```rust
pub(super) sketch_constraints: Vec<SketchConstraintSet>, // one per active scope
```

keyed by `SketchScope` (below), populated from the document's XRecord blob
on open (§5) and reserialized into it on save. This keeps `ocs_gcs`
dependency-free and keeps the design consistent with how `plugin_state`
(`:216`) already attaches session-scoped, non-`CadDocument` state to a tab.

## 3. Scoping and addressing

### 3.1 `SketchScope` — what "one sketch" means here

The app has no dedicated "Sketch" grouping (§1.1). Given block records are
the only existing sub-document concept, and given the task explicitly allows
"the whole flat document" as a fallback:

```rust
pub enum SketchScope {
    ModelSpace,
    Block(acadrust::Handle), // a BlockRecord handle, matching BlockEditSession::br_handle
}
```

v1 recommendation: **one `SketchConstraintSet` per space** — model space
gets one, and each block definition gets its own if/when it's edited in
BEDIT (`BlockEditSession::br_handle`, `src/modules/draw/modify/block_edit.rs:51`).
This is not "a real Sketch object" the way FreeCAD has one, but it matches
the only grouping boundary that already exists, needs no new document
concept, and is forward-compatible: if a real "Sketch" sub-entity concept is
added later, `SketchScope` gains a variant and existing `ModelSpace`/`Block`
sets are unaffected. Paper space layouts are out of scope (constraints on
annotation/layout geometry make little sense) — an edit there simply finds
no matching `SketchConstraintSet` and does nothing.

### 3.2 Endpoint addressing

Covered in §1.2/§2: `SketchRef { entity: Handle, marker: Option<i32> }`,
directly reusing the GsMarker/`source_points()` convention already present
for associative dimensions. **Action item this design depends on**:
`source_points()` (or an equivalent) needs to become a small `pub(crate)` or
`pub` function usable from both `dimension_assoc.rs` and the new constraint
module — likely promoted to a shared location (`src/scene/subelement.rs` or
similar) rather than duplicated. This is a small, mechanical refactor, not a
new design.

## 4. Re-solve trigger — concrete mechanism

### 4.1 Hook point

Add one more link to the existing in-line chain in `Scene::bump_entities`
(`src/scene/mod.rs:2612`), after the existing associative-geometry refreshes
and before the epoch bump:

```rust
// after refresh_associative_hatches, before `if !changes.is_empty() { refresh_dependency_index_for_changes }`
for change in self.refresh_sketch_constraints(&changes) {
    if !changes.iter().any(|(handle, _)| *handle == change.0) {
        changes.push(change);
    }
}
```

`refresh_sketch_constraints` lives in the new module (not `ocs_gcs`) and:

1. Filters `changes` to handles that are referenced by some
   `SketchConstraint::refs` in some `SketchConstraintSet` on this tab (a
   `Handle -> Vec<(scope, ConstraintId)>` reverse index, rebuilt lazily and
   invalidated the same way `SceneDependencyIndex` already is —
   `invalidate_dependency_index`, `src/scene/mod.rs:2582`/`:9882` — same
   pattern, separate cache).
2. For each affected scope, rebuilds a fresh `ocs_gcs::System` from that
   scope's current entity coordinates + `SketchConstraint` records (§4.2),
   solves, and — for every parameter that moved — calls
   `is_recording_undo()` + `record_undo_before()` (matching
   `centerline.rs:338-340`) **then** `document.get_entity_mut(...)` to
   write the solved coordinates back, and collects `(Handle,
   ChangeKind::Modified)` for the return vec.
3. Returns that vec; `bump_entities` folds it into `changes` exactly like
   the existing associative passes, so **one user edit that ripples through
   several constrained entities still produces one undo step and one epoch
   bump** — no special-casing needed, this falls out of the existing
   contract for free.

This directly answers "circular triggering": because the solve's writes are
folded into the *same* `changes` vec inside the *same* `bump_entities` call
rather than by calling `bump_entities` again, there is no re-entrancy to
guard against — same reason the three existing associative-refresh passes
don't loop either. A genuine constraint-graph cycle (e.g. A depends on B
depends on A, which can't happen in a well-formed `ocs_gcs::System` since
its partitioning is over undirected constraint-sharing, not a directed
dependency) is a different, solver-level concern handled by `ocs_gcs`
already converging or reporting `SolveStatus` failure — not an infinite
recompute loop.

### 4.2 Full rebuild per trigger, not incremental

Per §1.3, `ocs_gcs::System` has no incremental API, so `refresh_sketch_constraints`
rebuilds from scratch every time it fires: fresh `ParamStore`, one
`store.add()` per unique referenced point (deduplicated by `SketchRef`, so a
shared Coincident point becomes one parameter, not two independently-solved
ones), one `ocs_gcs::constraints::*` construction per `SketchConstraint`,
`system.partition()`, solve each `SubSystem`. This mirrors both FreeCAD's
own architecture and the existing one-shot commands' pattern
(`ParamStore::new()` fresh every call, `tools.rs:86/104/122/145/167/186`) —
consistent with how this port has approached fidelity to planegcs throughout.

Cost model: bounded by one scope's constraint count, not document size,
because the reverse index in step 1 above only rebuilds scopes actually
touched. A drawing with thousands of unconstrained entities and one 40-line
constrained sketch pays for the 40-line resolve, not the whole document —
same locality property `refresh_associative_dimensions` already has by
filtering on `changed`.

### 4.3 What does NOT trigger a re-solve

- Selection changes, hover, camera moves, layer visibility — none of these
  call `bump_entities`, so none reach this hook. Free.
- Live grip-drag preview (§1.3) — the document isn't touched until commit,
  so mid-drag frames never fire a resolve. The dragged-point's constrained
  neighbors will visibly "lag" behind during the drag (frozen at their
  pre-drag position) and snap to solved positions on release. This is an
  intentional, low-risk simplification for v1 — see §7 for the UX
  tradeoff this implies versus FreeCAD/Fusion 360's live-solve-during-drag
  feel, and a possible follow-on (§6 staged plan, stage 4).

## 5. Undo/redo and save/load — concrete integration

### 5.1 Undo/redo

No new undo mechanism needed. §4.1's `record_undo_before` calls inside
`refresh_sketch_constraints` make solved-geometry writes participate in
whatever `DeltaSnapshot` the *triggering* command already opened via
`begin_undo`/`commit_undo_delta`. A `Move` command that nudges one endpoint
of a Coincident-constrained line, causing three other entities to re-solve,
produces one `DeltaSnapshot` with four entity images and undoes/redoes as
one step — the same guarantee `refresh_associative_centerlines` already
gives centerline geometry today.

**Gap**: adding/deleting a `SketchConstraint` itself (not a geometry edit,
a constraint-set edit) needs its own undo path, since `SketchConstraintSet`
lives on `DocumentTab`, not in `CadDocument`'s entity/object store, so it's
outside the `DeltaSnapshot::entities` delta model entirely. Two options:
(a) give `HistorySnapshot` a new variant, `SketchConstraints(before, after)`
holding a full `SketchConstraintSet` clone (cheap — sketches are small,
unlike the whole-document `Full` structure fallback this codebase
deliberately avoids for entities), or (b) once §5.2's XRecord persistence
exists, route constraint-set edits through the *same* `ObjectEntryDelta` /
`StructureSnapshot::Objects` path already used for "Group erase and
RasterImage add" (`src/app/document.rs:790-794`, referenced at
`:749-751`) by writing the serialized blob into the XRecord's `entries` on
every add/remove and letting the existing object-delta undo cover it —
**this is the recommended option**, since it reuses an already-implemted
path instead of adding a new `HistorySnapshot` variant, at the cost of
needing the XRecord "live" (i.e. resident in `document.objects`, not just
serialized at save time) — see §5.2.

### 5.2 Save/load

Two implementation choices interact:

- **"Live" model**: `SketchConstraintSet` also exists as a resident
  `ObjectType::XRecord` in `document.objects` at all times (not just written
  out at save time), kept in sync on every constraint add/remove/re-solve.
  This makes §5.1(b) work directly (constraint-set edits ride the ordinary
  object-delta undo path) and makes save trivial (the XRecord is already
  correct, `save_as_version` just writes what's there). Cost: every
  constraint add/remove/re-solve re-serializes the whole scope's blob into
  the resident `XRecord.entries`, which is a Vec<XRecordEntry> — for a
  small sketch this is cheap (bytes, not megabytes), consistent with the
  scope-locality argument in §4.2.
- **"Lazy" model**: `SketchConstraintSet` lives only in `DocumentTab`
  (§2.1) and gets serialized into a fresh XRecord/Dictionary entry only at
  save time (`save_as_version`/`save_to_bytes`), and deserialized back on
  open. Simpler to build first, but forces §5.1(a) (a dedicated
  `HistorySnapshot` variant) since there's no resident object to hang the
  existing delta-undo path off of, and requires the save path to reach into
  `DocumentTab`-level state (currently `save_as_version` takes `&CadDocument`
  only, per `src/io/mod.rs:1546` — would need a new call site that also
  receives `&[SketchConstraintSet]`, or a pre-save step that materializes
  them into `document.objects` transiently before handing off to
  `save_as_version`, which converges back to the "live" model anyway at
  save time).

**Recommendation: build the "lazy" model first (stage 4 of §6) as the
simplest thing that actually round-trips through save/load, then migrate to
"live" once the constraint-set-edit undo gap (§5.1) is worth closing
properly** — the lazy model's pre-save materialization step is forward
compatible with later making it resident permanently.

Concretely, on open: after the document loads, scan
`document.objects` values under the dictionary named
`"OCS_SKETCH_CONSTRAINTS"` (if present) for `XRecord`s, deserialize each
`Chunk` entry into a `SketchConstraintSet`, populate
`DocumentTab::sketch_constraints`. On save: before calling
`save_as_version`, serialize each `SketchConstraintSet` and upsert the
corresponding `Dictionary`/`XRecord` pair into `document.objects` (creating
them via `CadDocument::allocate_handle` on first save). Versioning: prefix
the blob with a format version byte/u32 so a future schema change can be
detected and either migrated or rejected gracefully rather than corrupting
silently — there is no existing precedent in this codebase for a
constraint-schema migration path, so this needs to be designed carefully
when the schema first changes (flagged in §7, not solved here).

### 5.3 Copy/paste and entity deletion

Neither is addressed by the existing associative-geometry passes in a way
that generalizes cleanly, and this design does not have a settled answer —
flagged explicitly in §7 rather than papered over:

- **Deletion**: if an entity referenced by a `SketchConstraint` is erased,
  the constraint becomes dangling. `refresh_sketch_constraints` (§4.1) must
  detect `ChangeKind::Removed` for a referenced handle and either drop the
  constraint (silently, or with a command-line notice) or refuse to
  complete the delete (FreeCAD does the former; SolidWorks warns). Simple
  to implement (delete-on-dangling) but not designed against real usage
  data here.
- **Copy/paste**: `acadrust::Handle`s are not preserved across a
  copy/paste of a *subset* of entities (a pasted line gets a fresh handle
  from `allocate_handle`), so constraints between two copied entities need
  their `SketchRef.entity` handles remapped through whatever handle-mapping
  table the paste command already builds for other cross-entity references
  (e.g. block INSERT membership, dimension associativity — `AssociativeData`
  is presumably remapped somewhere already since dimensions survive
  copy/paste). This needs to reuse that existing remap table rather than
  invent a new one, but **which command owns that table was not located in
  this investigation** (out of budget — see §7).

## 6. UI implications

### 6.1 Phase A (already partially done) — extend to persistent commands

The existing `HCONSTRAINT`/`VCONSTRAINT`/`PCONSTRAINT`/`QCONSTRAINT`/
`ECONSTRAINT` dispatch block (`src/app/commands/draw.rs:960-1029`) becomes
the template for persistent variants: same selection UX, same
`begin_undo`/`commit_undo_delta` bracket, but instead of calling
`constrain::apply_horizontal` (which solves-and-forgets), call a
`sketch::add_constraint(scope, kind, refs)` that (a) appends a
`SketchConstraint` to the tab's `SketchConstraintSet` for the entities'
scope and (b) calls `scene.bump_entities(&touched)` once to trigger the
first solve through the normal §4 path — no separate "solve now" code path
needed, add-then-bump reuses everything. Coincident/Distance/Angle/Radius/
Tangent (not yet wired per the plan's progress notes) need sub-entity point
picking, which requires the promoted `source_points()`/marker-picking UI
(§3.2) — this is new interaction work, not just new dispatch.

### 6.2 Phase B — live inference

Hooks into `CadCommand::on_point` per the plan's Section 4, concretely at
the point a draw command returns `CmdResult::CommitEntity` (e.g.
`LineCommand::on_point`, `src/modules/draw/draw/line.rs:74-83`, returns
`CmdResult::CommitEntity(line)` on every click after the first). The
inference pass belongs at the layer that turns `CmdResult::CommitEntity`
into an actual document write (not inside each draw command — `line.rs`
itself must stay unaware of constraints, matching how it's unaware of undo
today), most likely alongside wherever `CmdResult::CommitEntity` is matched
in the app's command-result handler (not located precisely in this pass —
grep for `CmdResult::CommitEntity` at that dispatch site before
implementing). The inference itself: for the new entity's endpoints, query
nearby existing entities' `source_points()` within snap tolerance (reusing
the existing OSNAP infrastructure's tolerance/query rather than a new
spatial index), and if a match is found, auto-add a Coincident
`SketchConstraint` the same way §6.1's `add_constraint` does — suppressible
by a modifier key per the plan's UX research, which needs a new
`CadCommand::set_modifier`-style push analogous to the existing
`set_ctrl`/`set_shift` (`src/command.rs:1857-1862`).

### 6.3 Constraint glyphs, color state, DOF badge

- **Glyphs**: an overlay layer alongside `src/ui/overlay.rs` (confirmed to
  exist, currently handles crosshair/grip/snap-marker overlays via `iced::
  widget::canvas`, `src/ui/overlay.rs:1-40`) is the right place — glyphs are
  screen-space UI chrome anchored to world points, exactly what this module
  already draws for grips. A new canvas layer reading
  `DocumentTab::sketch_constraints` and projecting each constraint's anchor
  point(s) through the existing camera/UCS transform is additive, no
  changes to the existing overlay content.
- **Constraint-state color** (under/fully-constrained, conflict): the
  plan's original note pointed at `src/scene/render_graph.rs`'s ByLayer/
  ByBlock color resolution (confirmed present, `render_graph.rs:87-99,194-198`)
  and an `adapt_to_bg` in `src/scene/project.rs` — **this investigation
  could not locate `adapt_to_bg`** (it may have been renamed or moved since
  the plan was written); treat that specific hook as unconfirmed. Recoloring
  committed wire geometry based on constraint status also means threading
  per-entity DOF state into a color-resolution path built for
  layer/block color inheritance, which is a bigger, riskier change than it
  sounds. **Recommended alternative**: render constraint state as glyph
  color/badges in the overlay layer (§ above) rather than recoloring the
  entities themselves — same information, no `render_graph.rs` changes,
  and it's the more common convention among the referenced incumbents
  (Onshape/Fusion badge geometry, they don't usually recolor the model
  itself except for a subtle under-constrained tint SolidWorks does — treat
  wire recoloring as a stretch goal, not v1).
- **DOF badge**: `src/ui/statusbar/mod.rs` has `StatusBar::view`
  (`:66`) taking a data struct (`StatusMenuData`, `:41`) — a small addition
  there, summing `ocs_gcs::diagnosis::diagnose(...).dof` across all of the
  active tab's `SketchConstraintSet`s (only the ones touching the current
  space) is a cheap, self-contained addition. `diagnose` is documented as
  "cheap enough to call after every edit for a live DOF readout"
  (`crates/ocs_gcs/src/diagnosis.rs:58-61`) — call it right after
  `refresh_sketch_constraints`' solve in §4.1, cache the `Diagnosis` on the
  `SketchConstraintSet` alongside the solve rather than recomputing it again
  in the status bar's view code.

### 6.4 Phase C — conflict resolver

`ocs_gcs::diagnosis::Diagnosis` (`diagnosis.rs:41-56`) gives `dof: usize`
and `redundant: Vec<usize>` (row indices into one `SubSystem`), with
`redundant_constraints()` resolving those back to `&Rc<dyn Constraint>`.
The module doc (`:20-31`) is explicit that this **cannot yet distinguish
redundant-but-harmless from genuinely conflicting** — telling those apart
needs attempting a solve on the reduced system and checking whether the
residual reaches zero, which is *not implemented*. A `ConflictResolverPanel`
(the plan's SketchXpert-style feature) needs that distinction to propose
correct candidate fixes and currently cannot be built correctly on top of
`diagnosis.rs` as it stands — this is a `ocs_gcs` gap, not just a UI gap;
flagged in §7 and reflected as its own stage in §6.

## 7. Open questions / risks (not resolved by reading code alone)

1. **XRecord round-trip fidelity untested.** This design assumes writing a
   `Chunk`-typed `XRecordEntry` into a fresh `Dictionary`/`XRecord` pair and
   reading it back through `save_as_version`/document-open round-trips
   byte-for-byte. The writer paths exist and look complete
   (§1.4), but this was not exercised — no test was run. First implementation
   stage should be exactly this round-trip in isolation before any solver
   integration.
2. **`Assoc2dConstraintGroup` interop is explicitly deferred, not ruled
   out.** If real-AutoCAD interop for constraint data becomes a product
   requirement later, mapping onto that object's node vocabulary is a
   separate, substantial design effort (reverse-engineered format, missing
   node kinds for half of `ocs_gcs`'s constraint types) that this document
   does not attempt to solve.
3. **Copy/paste handle remapping table not located** (§5.3) — needs a
   follow-up investigation into whichever command implements paste before
   Phase A's persistent constraints can be considered safe against
   copy/paste, or this should be explicitly scoped out of v1 with a
   documented limitation (pasted entities lose their constraints).
4. **Entity-deletion policy for dangling constraints is a product decision,
   not a technical one** (§5.3) — silently drop vs. warn vs. block, needs a
   product call, not just an implementation.
5. **`source_points()` promotion scope unknown** — this function
   (`src/scene/dimension_assoc.rs`, exact line not confirmed beyond the call
   site at `:406-408`) needs to be read in full to confirm it covers every
   entity type `ocs_gcs` constraints care about (line, circle, arc, ellipse
   variants) before relying on it as the shared sub-element enumeration —
   this investigation confirmed its existence and call convention, not its
   full entity-type coverage.
6. **Live-drag re-solve UX gap** (§4.3) — constrained neighbors freeze
   during a drag and snap on release, rather than live-tracking like
   FreeCAD/Fusion 360/Onshape. Whether this is acceptable for v1 or a
   blocking UX regression against the plan's stated goal of matching those
   tools' feel is a product judgment call this document flags but does not
   make. A future stage could special-case a *cheap, direct* (non-`ocs_gcs`)
   local re-projection during drag frames (e.g. keep a coincident point
   glued without a full solve) as a partial mitigation, but that is
   meaningfully more work and is not in the staged plan below.
7. **`adapt_to_bg`/wire-recoloring hook not confirmed to exist under that
   name any more** (§6.3) — the original plan file cites it; this
   investigation could not find it under that name in the current tree and
   recommends the overlay-glyph approach specifically to sidestep needing
   it.
8. **No conflicting-vs-redundant distinction in `ocs_gcs::diagnosis`**
   (§6.4) — blocks a correct Phase C resolver until that solver-level gap
   is closed; this is tracked as its own implementation stage, not silently
   assumed away.
9. **Performance at scale untested.** The locality arguments in §4.2 (only
   touched scopes rebuild) are architecturally sound but no profiling was
   done against a large, heavily-constrained drawing; the original port
   plan's fixture-based testing strategy doesn't cover this either.

## 8. Staged implementation plan

Each stage independently testable, matching how the rest of this port has
proceeded (`ocs_gcs` itself untouched by any of this — all new code is
app-side, except one bug fix in `ocs_gcs::system::System::partition` found
while building stage 3 — see that stage's notes).

### Progress (as of this session)

**Stages 1–3 done and tested.**

- Stage 1: `src/scene/sketch_constraints.rs` — `SketchRef`, `ConstraintKind`
  (Coincident/Horizontal/Vertical/Parallel/Perpendicular/Equal/Distance/
  Angle/Radius/Tangent), `SketchConstraint`, `SketchScope`
  (ModelSpace/Block), `SketchConstraintSet`. `source_points()`
  (`src/scene/dimension_assoc.rs`) promoted from private to `pub(crate)` —
  the one-line mechanical refactor §3.2 anticipated. 8 unit tests
  (`cargo test --lib sketch_constraints`), all passing.
- Stage 2: `tests/sketch_constraints_xrecord_roundtrip.rs` — the XRecord
  round-trip spike, resolving open question 1. **Simpler than the design
  doc assumed**: `CadDocument` already has `ensure_xrecord`/`xrecord`/
  `xrecord_mut(owner, key)` convenience methods (discovered by reading
  `src/scene/centerline.rs`'s settings-persistence code, which uses exactly
  this pattern for `CenterLineSettings`) — no manual `Dictionary`/
  `named_objects_dict_handle` graph-walking needed. Verified: a
  `SketchConstraintSet` serialized into a `Chunk`-typed `XRecordEntry`
  survives a full save/reload round-trip, for both DXF and DWG, for both a
  `SketchScope::ModelSpace` owner and a real `SketchScope::Block(BlockRecord
  handle)` owner (a bare, entity-less `BlockRecord` did *not* round-trip —
  the writer prunes it as unreferenced; a real block created via
  `Scene::create_block_from_entities` does). 3 tests, all passing.
- Stage 3: `refresh_sketch_constraints` wired into `bump_entities`
  (`src/scene/sketch_solve.rs`), against an in-memory-only
  `SketchConstraintSet` per the plan (no save/load integration yet — that's
  still stage 4). Maps every `ConstraintKind` except `Coincident`'s Y-half
  (folded in separately — see the module's doc comment) and `Tangent` (no
  settled `ocs_gcs` mapping yet) onto `ocs_gcs` primitives, rebuilds a fresh
  `ocs_gcs::System` from live document geometry on every trigger (§4.2),
  solves via `solve_dl`, writes back through `record_undo_before`/
  `get_entity_mut` exactly as designed. 5 external integration tests
  (`tests/sketch_constraints_solve.rs`: Horizontal, Parallel, Distance,
  Coincident, and an unrelated-edit-doesn't-trigger check) + 1 internal
  unit test for the one-undo-step guarantee (needs `pub(crate)` undo APIs
  an external test crate can't reach).
  - **Deviation from this doc**: `sketch_constraints: Vec<SketchConstraintSet>`
    lives on `Scene` (via `Scene::sketch_constraint_set`/`_mut`, §2.1's
    field kept `pub(crate)` behind those two accessors so nothing can end up
    with two sets for one scope), not on `DocumentTab` as §2.1 originally
    proposed. Reason, discovered only by actually wiring the hook:
    `bump_entities` — which stage 3 must hook per §4.1 — is a `Scene`
    method, and `Scene` has no reference back to its owning `DocumentTab`.
    Putting the constraint sets on `Scene` (which already owns `document:
    CadDocument`, the only other thing this state is meaningfully
    co-located with) resolves that access problem directly rather than
    threading a callback or restructuring ownership.
  - **Real `ocs_gcs` bug found and fixed**: `System::partition()` built each
    `SubSystem`'s free-parameter list from *every* parameter its
    constraints referenced, including ones marked `driven` (a dimensional
    constraint's target value, or any point a caller deliberately wants
    fixed) — it never filtered by the `driven` flag at all. Every existing
    working call site (`crate::modules::draw::constrain`'s one-shot
    bridges, this stage's predecessor) sidesteps `System` entirely and
    calls `SubSystem::new(clist, &explicit_free_list)` directly, so this
    never got exercised until `sketch_solve.rs` became the first real
    `System::partition()` consumer. Symptom: a `Distance` constraint with
    both endpoints free solved to a wrong length (the driven target
    parameter itself was incorrectly free to move). Fixed with one filter
    (`crates/ocs_gcs/src/system.rs`), plus a regression test
    (`partition_excludes_driven_params_from_each_subsystems_free_list`) and
    a correction to the existing `a_two_point_distance_sketch_builds_
    partitions_and_round_trips` test, which had silently encoded the buggy
    5-free-param count as correct.

**Stages 5–7 done and tested** (skipping stage 4 — save/load — for now; the
one-shot ribbon commands and DOF/glyphs were the more visible unlock and
didn't depend on it).

- **Stage 5**: Horizontal/Vertical/Parallel/Perpendicular/Equal
  (`src/app/commands/draw.rs`) and Distance/Angle
  (`src/modules/draw/constrain/value.rs`) all now add a persistent
  `SketchConstraint` instead of solving once and forgetting. New
  `CmdResult::AddSketchConstraint { kind, refs, driving_param, label }`
  (`src/command.rs`), handled once in `command_driver.rs`: resolves the
  current scope (`DocumentTab::current_sketch_scope`, new — model space, or
  the open block definition during BEDIT), adds the record, and triggers the
  first solve via the same `bump_entities` path any later edit uses. The old
  one-shot bridge functions (`apply_horizontal` et al.,
  `src/modules/draw/constrain/mod.rs`) are deleted — nothing called them
  anymore once every dispatch arm moved over. Radius uses `DCONSTRAINT` on a
  circle (matching the one-shot version's existing dual-purpose UX; no
  separate ribbon button added). Coincident and Tangent are still not wired
  to any command (point-picking UI / no settled mapping, per the design doc's
  own scoping) — the constraint kind and solve path both already support
  Coincident, just nothing in the UI can author one yet.
  **Live-verified**: a slanted line (0,0)-(10,3), Horizontal applied via the
  ribbon — snapped level immediately, `DOF: 3` appeared in the status bar,
  and a "—" glyph pill rendered at its midpoint (stage 7). Then dragged just
  the right endpoint via its grip to an arbitrary off-axis point — the
  *left* endpoint's Y moved to match it, keeping the line exactly level
  (both ends landed at y = −6.0407) with `DOF: 3` unchanged throughout —
  confirming this is a real re-solved constraint, not a one-shot apply that
  happened to look similar.
- **Stage 6 (DOF badge)**: `solve_scope` (`sketch_solve.rs`) now also sums
  `ocs_gcs::diagnosis::diagnose(...).dof` across every independent partition
  in the scope and caches it on `SketchConstraintSet::dof` (new field,
  `#[serde(skip)]` — a derived cache, not persisted state). `StatusBar::view`
  takes a new `sketch_dof: Option<usize>` parameter and shows a plain
  (not yet user-hideable via `StatusPill`) "DOF: n" pill when the current
  scope has a constraint set at all; invisible otherwise, so an
  unconstrained drawing sees no new clutter.
  **Bug found live-testing this stage**: `diagnose` only sees params that
  appear in some constraint's own `params()` (via `SubSystem`'s plist, per
  the stage-3 `System::partition` fix above) — a registered entity
  coordinate nothing constrains *yet* (e.g. a line's two X coordinates,
  after only a Horizontal constraint pins its Ys) was invisible to it
  entirely, undercounting DOF (a fresh Horizontal-only line showed "DOF: 1"
  live, not the correct 3). Fixed in `solve_scope` by adding back
  `total_free − touched_free` (every registered free param not referenced
  by any constraint is trivially 1 DOF each on its own), with a regression
  test (`horizontal_...` in `tests/sketch_constraints_solve.rs` now also
  asserts `dof == Some(3)`).
- **Stage 7 (constraint glyphs)**: extended the *existing* `SelectionCanvas`
  (`src/ui/overlay.rs`) with a `constraint_glyphs: Vec<(Point, String)>`
  field rather than adding a second canvas layer — same "always-on UI
  chrome" category as the grips/crosshair/UCS icon it already draws. Each
  enabled constraint gets one small pill (symbol, plus the driving value for
  a dimensional kind — `sketch_constraints::glyph_label`) at a computed
  world anchor (`sketch_constraints::glyph_anchor`: a point ref resolves
  through `resolve_point`; a whole-entity ref falls back to a line's
  midpoint or a circle's rightmost point), projected via the same
  relative-to-eye math the grips use (`scene::pick::grip::project_rte`,
  promoted from private to `pub(crate)` for this second caller). Model space
  only — paper space shows none, matching constraints not applying there.
  Visual only: no click-to-select or delete yet.

**Stages 9, 10, and 12 done and tested** (skipping stage 4 — save/load —
still, for the same reason as stages 5-7 above; stage 9's "recommended"
approach explicitly depends on it, so a scoped-down alternative was used —
see below).

- **Stage 9 (constraint-set-edit undo)**: adding a persistent constraint is
  now itself undoable, not just the geometry moves it triggers. **Deviation
  from this doc**: §5.1(b)'s "live XRecord" approach (folding the
  constraint-set edit atomically into the same undo entry as the geometry
  delta) needs stage 4/save-load, which is still skipped this session — used
  §5.1(a)'s documented fallback instead: a new, independent
  `HistorySnapshot::SketchConstraints(SketchConstraintsSnapshot)` variant
  (`src/app/document.rs`) holding a whole-`SketchConstraintSet` before/after
  clone, pushed via a new `push_sketch_constraints_history`
  (`src/app/history.rs`) as its own entry *adjacent to*, not merged with,
  the `Delta` entry the triggering solve already produces via
  `commit_undo_delta`. `CmdResult::AddSketchConstraint`'s handler
  (`command_driver.rs`) now captures the scope's constraint set before
  adding, and calls the new push after. Cost of the scoped-down approach:
  undoing a constraint-add takes **two** presses, not one — the first
  (topmost entry, pushed last) removes the constraint record and leaves the
  already-solved geometry in place; the second reverts the geometry itself.
  Redo replays symmetrically. 1 internal test
  (`command_driver.rs::sketch_constraint_undo_tests::
  adding_a_constraint_is_undoable_and_redoable_in_two_steps`), passing,
  exercising exactly that two-step sequence in both directions.
- **Stage 10 (`ocs_gcs::diagnosis` conflict/redundant distinction)**: new
  `RedundancyKind { Redundant, Conflicting }` and
  `classify_redundant(sub, store, redundant) -> Vec<(usize, RedundancyKind)>`
  (`crates/ocs_gcs/src/diagnosis.rs`), answering the gap `diagnose` itself
  deliberately leaves open (per its module doc comment): for each row
  `diagnose` already flagged as linearly dependent, remove it, solve the
  reduced system from a cloned `ParamStore` (caller's real values untouched),
  and check whether the *removed* constraint's own error reaches ~0 there —
  redundant if so (the rest already implies it), conflicting if not (the
  rest disagrees with it). Kept as a separate, opt-in call rather than folded
  into `diagnose` (costs one extra solve per redundant row, which
  `diagnose`'s "cheap enough for every edit" contract doesn't afford — but
  the common zero-redundant case never pays for it). 2 new tests (a
  duplicated `Equal` classifies `Redundant`; two different fixed `Difference`
  targets on the same pair classify `Conflicting`), both passing; not yet
  consumed by any UI (that's stage 11).
- **Stage 12 (copy/paste handle remapping + deletion policy)**: resolves
  both of §7's remaining open questions.
  - **Open question 3** (which command owns the copy/paste handle-remap
    table): there isn't one shared table — `Scene::copy_entities`
    (`src/scene/modify.rs`, backing COPY/ARRAY/MIRROR) and
    `OpenCADStudio::finalize_paste` (`command_driver.rs`, clipboard paste)
    each already build their own local `handle_map: FxHashMap<Handle,
    Handle>` while duplicating entities. New `Scene::
    duplicate_sketch_constraints_for(&handle_map)`
    (`src/scene/sketch_constraints.rs`) is called from both, right after
    their existing `bump_entities`: for every enabled constraint whose refs
    are *all* covered by the map, adds a remapped copy touching the new
    handles. A constraint straddling a duplicated and a non-duplicated
    entity is left alone (can't sensibly follow).
  - **Open question 4** (entity-deletion policy): chose "silently drop",
    matching FreeCAD as this doc recommended. `refresh_sketch_constraints`
    (`sketch_solve.rs`) now runs a deletion pass first, before any resolve:
    for every `ChangeKind::Removed` handle, calls the already-existing
    `SketchConstraintSet::remove_all_touching` on every scope, unconditionally
    (not gated on a `touched` check), so a scope left empty by the deletion
    doesn't attempt a pointless resolve afterward.
  - 2 new external integration tests in `tests/sketch_constraints_solve.rs`
    (erasing a constrained line drops its constraint but leaves an unrelated
    one alone, and the survivor keeps solving; copying two Parallel-linked
    lines carries an independent copy of the constraint, verified by
    breaking the copy's parallelism and confirming its own constraint
    re-levels it while the original pair is untouched), both passing.

**Stages 8 and 11 done and tested** (stage 4/save-load still skipped, same
reasons as above).

- **Stage 8 (Phase B live inference)**: drawing a `LINE` whose endpoint
  already coincides with an existing entity's point in the current scope now
  auto-adds a Coincident constraint. New
  `sketch_constraints::infer_coincident_refs(document, scope, new_handle,
  new_entity)` (`src/scene/sketch_constraints.rs`) checks every one of the
  new entity's `source_points()` against every other entity's in the same
  scope for near-exact world-space coincidence (1e-6 epsilon), returning a
  `(new, existing)` `SketchRef` pair per match. Wired into
  `command_driver.rs`'s new `infer_and_add_coincident_constraints`, called
  from the `CmdResult::CommitEntity` dispatch — scoped to the active command
  being named `"LINE"` specifically (not every `CommitEntity` source; FILLET/
  OFFSET/MIRROR/etc. also route Lines through the same dispatch and
  shouldn't silently acquire constraints nobody asked for). Suppressible by
  holding Shift.
  - **Deviations from this doc**: §6.2 suggested reusing OSNAP's
    screen-space tolerance/query; this instead checks tight world-space
    coincidence, since the natural hook point (`CmdResult::CommitEntity`'s
    dispatch) has no cursor pixel position or camera by the time an entity
    becomes a document write — only its final world coordinates. If OSNAP's
    endpoint mode already grabbed the point while the user was drawing, the
    coordinates already match to floating-point precision, so this
    reproduces the same result without re-deriving OSNAP's screen-space math
    or threading camera state into this dispatch site. Separately, §6.2
    proposed a new `CadCommand::set_modifier` mechanism for the suppression
    key; turned out unnecessary — `command_driver.rs` already reads
    `self.shift_down` directly at this exact dispatch site (the app-level
    field the existing Ctrl/Shift push already tracks), so no new
    per-command plumbing was needed.
  - Mirrors `CmdResult::AddSketchConstraint`'s undo shape (stage 9): the
    add-and-solve is bracketed in its own `begin_undo`/`commit_undo_delta`,
    then the constraint-set edit is pushed as the separate, adjacent
    `SketchConstraints` entry.
  - 2 new internal tests (drawing onto an existing endpoint infers a
    Coincident referencing the pre-existing line; Shift suppresses it
    entirely), both passing.
- **Stage 11 (Phase C conflict resolver)**: `solve_scope`
  (`sketch_solve.rs`) now also tracks which system-level `ocs_gcs` constraint
  came from which `SketchConstraint` (an `owner: Vec<(Rc<dyn Constraint>,
  ConstraintId)>` built alongside registration — a `SubSystem`'s
  redundant-row indices are local to that partition's own list, not
  `set.constraints`' indices, and Coincident contributes two system-level
  constraints for one `SketchConstraint`). When a partition's `diagnose`
  flags any redundant rows, `classify_redundant` (stage 10) is called on
  just that partition and each result resolved back to a `ConstraintId` via
  `Rc::ptr_eq` against the owner list, cached on a new
  `SketchConstraintSet::conflicts: Vec<(ConstraintId, RedundancyKind)>`
  field (`#[serde(skip)]`, like `dof`). A new status-bar pill ("⚠ n
  conflicting", `src/ui/statusbar/mod.rs`) appears only when the current
  scope's `conflicts` is non-empty; clicking it (`Message::
  ResolveOneSketchConflict` → `OpenCADStudio::resolve_one_sketch_conflict`,
  `command_driver.rs`) removes the first flagged constraint and re-solves,
  using the same undo bracket as stage 8/9's handlers.
  - **Deviation from this doc**: §6.4's full `ConflictResolverPanel` — a
    window listing every flagged constraint by name with a cyclable live
    preview of each candidate fix before committing — is real, standalone UI
    work (new window state, `Message` plumbing for open/close/select/
    preview/commit) that this pass did not have live-UI-testing access to
    verify: the desktop session's app focus was blocked for the whole of
    this stage by an unrelated stuck System Settings window on a second
    monitor (a recurrence of the same class of issue noted during stages
    5-7's live verification). What's built instead is bounded but real: one
    click removes one flagged constraint, so a user with several conflicts
    clicks repeatedly and watches the DOF/conflict count drop — same
    end effect as the panel's "remove and re-solve" action, minus the
    browsing/naming/preview UI. The full panel remains open follow-up work.
  - 2 new external integration tests (`tests/sketch_constraints_solve.rs`:
    a duplicated Horizontal constraint classifies `Redundant` and resolves to
    one of the two ids; two conflicting `Distance` targets on the same pair
    classify `Conflicting`) + 1 internal test
    (`resolve_one_sketch_conflict_removes_a_flagged_constraint_and_is_undoable`
    in `command_driver.rs`, also confirming the removal is undoable), all
    passing.

Full regression check after stages 8/9/10/11/12: `cargo test --workspace`,
660 passed / 1 failed (`scene::text::lff::tests::fonts_parse_and_resolve`, a
pre-existing Turkish-glyph-fallback test wholly unrelated to this work —
`src/scene/text/lff.rs` untouched by any stage in this doc) / 13 ignored.

**Stages 8 and 11 live-verified** in a release build (`cargo build --release
--bin OpenCADStudio`, run from a scratch copy of the app bundle so
`/Applications`'s installed copy was never touched) once the stuck-focus
environment issue cleared. Stage 8: drew two separate `LINE` segments whose
shared endpoint coincided exactly — a "≡" glyph appeared at the junction
automatically and the DOF badge read 6 (8 raw coordinates − 2 for the
inferred constraint); dragging one line's endpoint grip pulled the other
line's endpoint along with it, confirming the inferred constraint re-solves
live rather than just having matched once. Stage 11: applied Horizontal to
the same line twice via the ribbon — a "⚠ 1 conflicting" pill appeared next
to the DOF badge; clicking it removed the duplicate and the pill vanished,
DOF unchanged at 5. Both matched the automated tests' expectations exactly.

**Stage 4 (save/load integration) done and tested.** Built the "lazy" model
§5.2 recommends as the first step: `src/scene/sketch_persist.rs`, new. No
new `HistorySnapshot` variant or resident-XRecord machinery — constraint
sets stay exactly where stage 3 put them (`Scene::sketch_constraints`,
in-memory only during editing) and are materialized into `document.objects`
only right before a save, read back right after a load.

- `Scene::materialize_sketch_constraints_for_save`: for each non-empty
  `SketchConstraintSet`, resolves its scope's owner handle
  (`SketchScope::owner_handle`), `ensure_xrecord`s a `"OCS_SKETCH_
  CONSTRAINTS"`-keyed record there, and overwrites its `entries` with one
  version-prefixed `Chunk` (`bincode::serialize`, prefixed with a
  `FORMAT_VERSION: u8` — §5.2's own call-out: a future schema change bumps
  this, and `decode` rejects a mismatched version rather than misreading
  bytes, though the migration path itself is still undesigned, per §7).
  Empty sets are skipped, so an unconstrained drawing gains no persisted
  bookkeeping. Called from `OpenCADStudio::prepare_native_save` (covers
  every native save trigger: Save, Save As, autosave, recovery-save) and
  from the wasm save path's own manual prep block (`src/app/update/
  file.rs`) — both confirmed to be the actual save entry points by tracing
  every caller, not assumed.
- `Scene::load_sketch_constraints_from_document`: clears
  `sketch_constraints`, then checks every `BlockRecord` in `document.
  block_records` (model space, paper space, and every named block
  definition — the table already enumerates all of them, so no separate
  "which scopes exist" bookkeeping was needed) for that XRecord key,
  decoding and pushing whatever it finds. Called from
  `OpenCADStudio::on_file_opened` (native/web open) and the automation
  `"open"` op (`src/app/automation.rs`) — the two places a `CadDocument`
  actually gets installed into a tab's `Scene`; the automation `"new"` op
  now also clears `sketch_constraints` for the same reason `"open"` needs
  the load call (a reused tab slot must not keep a previous document's
  constraints).
- **Deliberate scope-down, not yet done**: no "resolve once on load" step.
  A freshly opened file's `SketchConstraintSet.dof`/`.conflicts` stay at
  their post-deserialize defaults (`None`/empty) until the user's next edit
  touches that scope — already anticipated by stage 6's own `dof` field doc
  comment, not a new gap this stage introduces. Considered forcing an
  initial resolve right after load (would populate the DOF badge
  immediately and self-heal any float drift from serialization) but
  `on_file_opened` installs a `DerivedCaches` bundle computed on a
  background thread *before* this hook runs — mutating entity geometry via
  a resolve here could desync from tessellation/derived state that bundle
  already reflects, and this pass didn't have budget to verify that
  interaction is safe. Left as `None`/empty rather than risk it; the §5.2
  "live" model (constraint sets resident in `document.objects` at all
  times, closing this and the stage-9 undo-shape gap together) remains the
  documented next step if this is worth revisiting.
- 6 new internal tests (`src/scene/sketch_persist.rs`): materialize+reload
  agreement for both a ModelSpace and a `SketchScope::Block` set, an empty
  set not persisting, format-version rejection, and — going beyond the
  in-memory-only checks — two **real** `save_to_bytes`/`load_bytes` round
  trips (DXF and DWG) through the actual `io` primitives the app's open/save
  path uses, not just `materialize` and `load` agreeing with each other on
  the same document.

Full regression check after stage 4: `cargo test --workspace`, 666 passed /
1 failed (the same pre-existing, unrelated font test noted above) / 13
ignored. Not live-verified in the running app this pass (no UI change to
click through — this stage is data-layer only, and its correctness is a
byte-round-trip property the automated tests already cover directly).

Every stage in the §8 plan below is now built and tested. Three documented
scope-downs remain against this doc's original recommendations: stage 9's
two-press undo, stage 11's one-at-a-time resolver pill, and stage 4's
no-resolve-on-load — all explained at their own stage above.

### Post-plan follow-ups: Coincident/Tangent manual UI, live-drag re-solve, color coding

Three items from the earlier "older open items" list, done on user request after
all twelve staged-plan stages landed.

- **Tangent mapping + manual UI**: `sketch_solve.rs`'s `build_constraint`
  now maps `ConstraintKind::Tangent` for Line-Circle (`ocs_gcs::constraints::
  circle_arc::C2LDistance`, with a driven zero-distance target and
  `internal: false` — this makes its target exactly the circle's own
  radius, i.e. plain tangency) and Circle-Circle (`TangentCircumf`).
  `ccw`/`internal` are picked from the pair's *current* geometry (which
  side of the line the circle already sits on; whether the circles are
  already nested) so the first solve doesn't have to cross the sign-flip
  singularity between the two tangency configurations. Line-Line and
  anything involving an Arc build nothing (Arc isn't a supported
  `EntityGeom` yet, matching that type's own documented scope) — skipped,
  not panicked, same contract every other unbuildable constraint gets.
  Wired to a new `TCONSTRAINT` ribbon button
  (`src/modules/draw/constrain/tools.rs`), reusing the existing
  two-whole-entity-selection dispatch (`src/app/commands/draw.rs`) Parallel/
  Perpendicular/Equal already use. 2 new integration tests (line-circle and
  circle-circle, both converging to within 1e-6 of exact tangency from a
  deliberately-not-tangent start).
- **Coincident manual UI**: new `CoincidentConstraintCommand`
  (`src/modules/draw/constrain/coincident.rs`, `CCONSTRAINT`) — unlike the
  rest of this module, Coincident addresses a *point* on an entity, not the
  whole thing, so it needs two point picks instead of a selection. A
  `CadCommand` has no document access (same constraint `value.rs`'s
  `DCONSTRAINT`/`ACONSTRAINT` already work around), so it only accumulates
  the two raw points and hands them back as a new `CmdResult::
  AddCoincidentConstraint { point_a, point_b, label }` for the host to
  resolve — mirroring `ReassociateCenterMark`'s existing split. New
  `sketch_constraints::nearest_sketch_point(document, scope, world_point,
  exclude)` finds the closest addressable point (an ordinary
  `source_points()` index, or a circle/arc center) within the same
  world-space epsilon stage 8's `infer_coincident_refs` already
  established — same trust that the point arriving here is already
  OSNAP-snapped, for the same reason (no camera/pixel radius available this
  far from the click). A pick that doesn't resolve reports a command-line
  error ("enable an Endpoint/Center object snap and try again") rather than
  silently constraining the wrong thing or the nearest-anything. 2 new
  internal tests (a successful two-point resolve referencing both lines; a
  missed pick adding nothing).
- **Color coding** (§6.3's own recommendation — glyph/badge color, not wire
  recoloring): a constraint's glyph pill now renders in the theme's
  `danger` palette instead of `primary` when `SketchConstraintSet.conflicts`
  (stage 10/11) flags it as redundant/conflicting
  (`src/ui/overlay.rs`/`src/app/view/mod.rs`, `constraint_glyphs` tuple grew
  an `is_conflicting: bool`). Separately, the DOF status-bar pill now uses a
  new `success_pill` (theme's `success` palette) instead of the plain
  `status_pill` when `dof == 0` — "fully constrained" is visible at a
  glance, matching the UX research bullet in §1 ("a simple color-coded
  constraint state"). No `render_graph.rs`/entity-recoloring changes, per
  this doc's own recommendation to avoid that larger, riskier surface.
- **Live-drag re-solve** (§7 open question 6 — the largest of the three):
  constrained neighbors previously froze during a grip drag and only
  snapped into place on release. Investigation found the assumption behind
  that open question ("the document is unmutated during a live-preview
  drag") was wrong for *this* codebase: `Scene::apply_grip` already writes
  the dragged handle's geometry straight into `self.document` on every
  mouse-move frame — the "preview" system (`set_preview_wires`) is a
  tessellated-wire rendering optimization, not a way of keeping the
  document clean during the drag. That meant the existing `solve_scope`/
  `refresh_sketch_constraints` machinery could run mid-drag with no
  architectural mismatch; the missing piece was purely that nothing called
  it per-frame (only once, at grip release, via
  `bump_entities(&changes)` in `viewport.rs`'s release handler).
  - New `Scene::solve_sketch_constraints_preview(&self, touched: &[Handle])
    -> Vec<(Handle, EntityType)>` (`sketch_solve.rs`) — a lighter sibling of
    `refresh_sketch_constraints`: same rebuild-and-solve core, but reports
    what moved instead of writing it back, and skips `record_undo_before`
    entirely (a grip drag tracks its before/after through its own
    `grip_originals`/`grip_preview_handles` arrays, committed as one
    `push_entity_group_history` group on release — not through the
    recording session `refresh_sketch_constraints` otherwise participates
    in; calling `record_undo_before` here would assume a recording context
    that doesn't exist for this call path).
  - Wired into the per-mouse-move drag update in `src/app/update/
    viewport.rs`, right after `Scene::apply_grip` writes the directly-dragged
    handle's new position, before this frame's mesh/hatch/wire-preview
    refresh. A constraint-solved neighbor's first touch this gesture
    captures its original state into `grip_originals` (so the eventual
    release-time undo group covers it correctly) and hides it from the
    resident tessellation (`preview_hidden`), then every frame folds it
    into this frame's `edited_handles` so the rest of the function's
    existing preview/mesh/hatch refresh already covers it for free — no
    changes needed to that downstream code.
  - Cheap no-op when there are no sketch constraints at all (the common
    case) or nothing touched resolves to a constrained handle. Per this
    doc's own §4.2 architecture this still does a full rebuild-and-solve
    per touched scope on every call — i.e. every mouse-move frame during a
    drag that touches constrained geometry; §7 open question 9
    ("performance at scale untested") already flags this as an accepted,
    unprofiled risk, not a new one.
  - 1 new internal test proving the preview solve reports the correct
    result without mutating the document at all.

Full regression check: `cargo test --workspace`, 669 passed / 1 failed (same
pre-existing unrelated font test) / 13 ignored.

**Live-verified** in a release build (same scratch-copy-of-the-app-bundle
technique as stages 8/11): live-drag re-solve was the main target, since
it's the one genuinely new architectural mechanism here, and it's
conclusively confirmed — dragging one line's shared-endpoint grip visibly
pulled a second, separately-constrained line's endpoint along with it
*during* the drag (captured via a manual mouse-down / mouse-move /
screenshot / mouse-up sequence, not just before-and-after), and undo/redo
correctly treated the whole gesture (including the constraint-followed
neighbor) as one step. Tangent was confirmed live to apply via the ribbon,
correctly update the DOF badge, and undo/redo cleanly; live pixel-checking
the *exact* resulting tangency point on a deliberately extreme (very
far-apart) test configuration was inconclusive from screenshots alone, but
the underlying formula is independently proven by the automated tests
above at 1e-6 tolerance from a similarly-unfavorable starting
configuration. Coincident's command dispatch, prompting, and
missed-pick error path were all confirmed live; the success path needs a
real OSNAP-precision click this session's synthetic input couldn't
reliably reproduce (the epsilon is deliberately tight — see
`nearest_sketch_point`'s doc comment — well below what manual
screenshot-and-click pixel estimation can hit), so it's verified by the
automated tests only. Color coding was not separately live-clicked this
pass (small, low-risk code reusing an established theme-palette pattern
already used elsewhere in this codebase).

1. **`SketchRef`/`SketchConstraint`/`SketchConstraintSet` data types** +
   promote/confirm `source_points()`-equivalent sub-element enumeration
   (§3.2, resolves open question 5). Pure data + unit tests, no document
   integration yet.
2. **XRecord round-trip spike** (resolves open question 1): write a
   throwaway blob into a fresh `Dictionary`/`XRecord` under
   `named_objects_dict_handle`, save via `save_as_version`, reload, confirm
   byte-identical readback. Gate everything after this stage on it passing.
3. **`refresh_sketch_constraints` wired into `bump_entities`** (§4.1) against
   an **in-memory-only** `SketchConstraintSet` (no save/load yet — populate
   it only via a test harness or a temporary debug command). Verify: one
   edit to a constrained entity moves its constrained neighbors, produces
   one undo step, doesn't infinite-loop, doesn't fire on unrelated edits.
4. **Save/load integration** (§5.2, "lazy" model): serialize on save,
   deserialize on open, using stage 2's confirmed round-trip.
   Constraint-set-edit undo (§5.1) can stay a known gap at this stage
   (document it as a TODO, not silently broken — e.g. adding a constraint
   without an accompanying geometry change simply isn't undoable yet).
5. **Phase A commands**: persistent Coincident/Horizontal/Vertical/
   Parallel/Perpendicular/Equal/Distance/Angle/Radius/Tangent, replacing the
   one-shot `constrain::apply_*` call sites in `src/app/commands/draw.rs`
   with `add_constraint` + `bump_entities` (§6.1). Sub-entity point picking
   UI for Coincident/Distance/Angle/Radius/Tangent is new work here, not
   reused from the one-shot commands (which never needed it).
6. **DOF badge** (§6.3) — cheap, additive, ships as soon as stage 3's solve
   produces a `Diagnosis` to read.
7. **Constraint glyphs overlay** (§6.3).
8. **Phase B live inference** (§6.2), including the modifier-key escape
   hatch.
9. **Constraint-set-edit undo migration to the "live" XRecord model**
   (§5.1(b)/§5.2), closing stage 4's known gap.
10. **`ocs_gcs::diagnosis` conflicting-vs-redundant distinction** (open
    question 8) — a `ocs_gcs`-side change, tested the same way the rest of
    that crate is (`cargo test -p ocs_gcs`), independent of everything else
    in this plan.
11. **Phase C conflict resolver UI**, gated on stage 10.
12. Copy/paste handle remapping (open question 3) and deletion policy (open
    question 4) — investigate and resolve before calling persistent
    constraints "done"; both can technically ship without this (with
    documented limitations) if timeline pressure requires it, but should not
    be silently forgotten.
