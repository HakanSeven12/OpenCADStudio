# OCS canvas entity completion plan

## Goal and current state

Build a general Python document model over the OCS host API so a script can control OCS: read and change every canvas entity, manage the drawing's tables, and run OCS's own commands. Keep a property-level record of read/write, read-only, snapshot-only, and unsupported fields in the [43-kind coverage ledger](plugin-host-model-coverage-ledger.md). The architecture and guarantees are in [plugin-host-model.md](plugin-host-model.md); the progress log at the end of this file records each increment with its evidence. Do not resume the separate Lisp-to-Python conversion work.

**State as of 21 September 2026.** The feature is host API v7. An earlier draft pull request, [#1391](https://github.com/HakanSeven12/OpenCADStudio/pull/1391), covered the first slice of it and was closed in favour of this more complete change.

- **Entities:** explicit writes for all 43 canvas kinds (41 creatable; the legacy `Polyline` and `Body` are update-only). **31 kinds are `Complete`** (every C/R/E/D/U/I/W/V/P gate passes). The remaining kinds are integration-tested and held short of `Complete` by a named blocker in the ledger: seven by opencadcodec DXF bugs (fixes submitted as [opencadcodec#48](https://github.com/HakanSeven12/opencadcodec/pull/48), not yet merged, so OCS still pins the unfixed revision), Polyline3D by the DWG format, Polyline2D by a kind change on DXF save, RasterImage by an image-reactor structure that cannot be verified, and Body by having no creation path.
- **Drawing tables:** layers, text and dimension styles, blocks and their contents, linetypes and layouts, through the additive `TableOperation` request (see the log entries dated 2026-09-21).
- **Commands:** `HostApi::run_command` drives the real OCS commands step by step; `doc.command`, `doc.start_command` and `doc.modify` build on it.
- **Known open items:** the unfiled opencadcodec findings (`cadcodec-reader-gaps.md`), complex (text and shape) linetypes, plot devices and named page setups, and PEDIT's remaining options.

Repositories:

- Bundled Python adapter: `plugins/opencad-python`
- Host coverage policy: `crates/ocs_plugin_api/entity_coverage_policy.json`
- Host coverage/validation: `crates/ocs_plugin_api/src/entity_coverage.rs` and `build.rs`
- Adapter mapping: `plugins/opencad-python/entity_manifest.json`,
  `plugins/opencad-python/build/generate.rs`,
  `plugins/opencad-python/src/ocs_module.rs`, and
  `plugins/opencad-python/src/document_model.py`
- Host design notes: `docs/plugin-host-model.md`

Host and adapter changes live in the same repository and are committed together.

## First: complete the originally agreed Phase 8

The earlier Phase 8 was **integration and compatibility**, not a new entity kind. `Insert` was added afterward as a separate coverage increment. Before marking any kind complete, establish an executable integration harness that exercises the actual plugin runner and IPC, not only direct Rust conversion helpers:

1. Stage the in-tree v7 plugin with `bash plugins/opencad-python/tools/stage-bundled.sh <stage directory> [--debug]`. Verify the staged `plugin.toml` says API v7 and its opencadcodec source and Rust compiler match OCS. Keep the repository-relative `ocs_plugin_api` path; never check in a machine-local path.
2. Through real IPC, create, inspect, edit, and delete a representative entity. Verify an invalid multi-entity transaction changes nothing, a valid transaction produces one undo entry, and undo **and redo** restore the expected geometry and references.
3. In the running GUI, exercise a real point pick, entity pick, cancellation/Escape, and plugin command cancellation. Verify drawing/selection/command notifications, including overflow recovery.
4. Repeat an input request and event poll across two tabs; verify token and notification isolation and cleanup when a tab closes.
5. Save and reopen representative edits as **DWG and DXF**, verifying the actual geometry and preserved unmapped fields. Use a real viewport/canvas check where the entity has visible geometry; a converter round trip alone does not satisfy this.
6. Record platform, OCS/opencadcodec revisions, fixture files, commands, results, and any engine limitation. Keep repeatable tests in the relevant repositories.

Phase 8 is complete only when these checks pass through the bundled plugin and
real runner boundary. If a test is blocked by OCS or opencadcodec behavior, record
the specific failure and leave Phase 8 open.

## Per-kind completion gate

Use one checklist row per kind in a coverage ledger. Status values: `unstarted`, `mapped`, `integration-tested`, `complete`, or `blocked`; record the blocking dependency. **Do not use “complete” for read-only, update-only, or layer-only support.** For every kind:

1. **Declare coverage:** Every traced field has an accurate catalog status and validation rule. Distinguish ordinary properties from handles, owners, linked records, external assets, and opaque payloads. Snapshot-only fields remain explicitly unmapped. Assert adapter keys match the host catalog.
2. **Create:** A Python script creates a valid entity through the document model in a real document with required tables, styles, definitions, files, and references. Missing or invalid required inputs fail before mutation. Add document-model creation/deletion methods if needed; current `doc.entities` only indexes and iterates, while low-level `ocs.add`/remove APIs exist.
3. **Read and edit:** After creation, read the entity by handle through `doc.entities`; edit at least one defining geometry or placement field and one other meaningful writable field inside `doc.transaction`. Verify that all unmentioned fields, handles, ownership, attached records, and XDATA remain unchanged. Check invalid values and forbidden reference edits fail atomically.
4. **Delete and history:** Delete the created entity through the Python interface. Prove create, edit, and delete have defined undo/redo behavior in the host, including grouped writes and failure rollback. Record any transaction API extension and bump the host API version if its ABI changes; keep older plugin compatibility.
5. **Real IPC and canvas:** Run the above through the staged plugin and actual OCS IPC. Verify visibility/geometry in the canvas and selection or picking in the GUI. For nonvisual entities, verify their observable document or layout behavior and explain the oracle.
6. **Persistence:** Save/reopen DWG **and** DXF fixtures. Compare kind, geometry, important metadata, handles/references, and preserved fields after reopen; edit again after reopen. Test at least one valid edge case for that kind. Do not count a JSON or in-memory round trip as file persistence.
7. **Packaged build:** Run the in-tree adapter tests with `--locked`, stage the
   plugin with `stage-bundled.sh`, and load it from the macOS application
   resources through the separate runner process. The staged manifest must
   match the host's Rust compiler and opencadcodec source.

If OCS/opencadcodec cannot create or serialize a kind reliably, mark it **blocked**, add a regression test or fixture demonstrating the failure, and move to the next independent kind. An explicit unsupported status is honest progress, not completion.

## Existing mapped kinds: audit before claiming completion

Audit these against the same gate: `Point`, `Line`, `Circle`, `Arc`, `Ellipse`, `Polyline`, `Polyline2D`, `Polyline3D`, `LwPolyline`, `Spline`, `Text`, `MText`, `Ray`, `XLine`, `Solid`, `Face3D`, `Insert`. `Insert` currently has transform edits but **no Python creation**; its block name is read-only and attached attributes are snapshot-only. Earlier tests include some direct DWG round trips, but there is no evidence that every mapped kind passes real IPC, GUI, deletion, undo/redo, and both file formats. Mark each independently after testing.

## Ordered 26-kind expansion queue

The order below groups shared implementation work. Complete a kind only through the gate above. Within a group, move past a blocked kind after recording the exact dependency. “Minimum exercise” identifies a real entity-specific create/edit/delete case; it does not make other fields automatically writable.

| # | Kind | Minimum entity-specific exercise and dependency |
|---:|---|---|
| 1 | Tolerance | Create a feature-control frame with text/style; move and edit its text/direction; check style reference and display. |
| 2 | Shape | Create with an installed SHX shape/style; change insertion, size, rotation; verify the named glyph and style survive reopen. |
| 3 | AttributeDefinition | Create in a block definition; edit tag, default text, and placement; verify the block’s definition and linked inserts. |
| 4 | AttributeEntity | Create as a block reference attribute; edit value and placement; verify parent `Insert`, handles, and sequence linkage. |
| 5 | Hatch | Create a closed-boundary solid hatch and a patterned hatch; edit boundary/pattern; verify fill and boundary validity. |
| 6 | Leader | Create with vertices and an annotation; edit path and annotation placement; preserve annotation handle. |
| 7 | MLine | Create against a valid MLine style; edit vertices and scale; verify style element offsets and joins. |
| 8 | Dimension | Create at least one supported dimension subtype with style and reference points; edit defining geometry and displayed measurement. Track subtype coverage separately. |
| 9 | MultiLeader | Create text and leader paths using a valid style; edit landing/vertices and text; preserve nested context and annotation links. |
| 10 | Table | Create rows, columns, style, and cells; edit size and one cell value; preserve merged ranges and block/style links. |
| 11 | PolygonMesh | Create a grid with defined M/N counts and vertices; edit a vertex and density; reject inconsistent counts. |
| 12 | PolyfaceMesh | Create vertices/faces; edit a vertex and face indices; reject invalid indices and preserve sequence records. |
| 13 | Mesh | Create vertices/faces/edges; edit one vertex and topology; reject out-of-range indices and verify rendered faces. |
| 14 | Helix | Create with axis, radius, turns, and height; edit a defining parameter and verify the derived curve plus embedded spline data. |
| 15 | RasterImage | Create with a resolvable image definition/file; edit placement, vectors, and clipping; verify display and definition handles. |
| 16 | Wipeout | Create with a valid clip polygon; edit boundary and placement; verify masking, clip mode, and image-definition links. |
| 17 | Underlay | Create with a valid PDF/DWF/DGN definition; edit transform and clipping; verify external resource and definition handle. |
| 18 | Viewport | Create in the appropriate layout; edit center/size/view target; verify visible viewport and frozen-layer/clip references. |
| 19 | ViewBorder | Create with its required view/scale references; edit border geometry; verify linked viewport/view records. |
| 20 | SectionSymbol | Create with view style and point records; edit endpoints/label; preserve view-representation links. |
| 21 | Light | Create a light with position/target; edit intensity and direction; verify color/photometric state and visible glyph. |
| 22 | Region | Create/import a valid region payload; transform/edit supported geometry, then verify ACIS payload after both file formats. |
| 23 | Body | Create/import a valid body payload; apply a supported transform/edit; verify ACIS and history/reference preservation. |
| 24 | Solid3D | Create/import a valid solid; edit a supported geometric operation; verify ACIS, topology, and history after reopen. |
| 25 | Surface | Create/import a valid surface; edit a supported parameter/transform; verify surface payload, isolines, and history. |
| 26 | Ole2Frame | Create with a valid embedded object payload; edit frame geometry; verify storage, aspect behavior, and persistence. |

The last five may require new engine capabilities rather than only Python converters. Do not deserialize and rewrite their opaque payloads unless lossless behavior is proven. If a listed kind has no meaningful safe Python creation/edit path, keep it marked blocked or partial and report why; do not relabel it complete to reach 43/43.

## Work pattern for each coding increment

1. Inspect the opencadcodec entity struct, constructors, private fields, serializer, drawing path, and linked tables/objects. Record the intended editable/read-only/unmapped properties first.
2. Put generic validation and transaction behavior in the host. Keep the Python adapter thin; add a manifest override only when the generator cannot safely express a field.
3. Add focused host validation, adapter conversion, document-model, real transaction/undo/redo, IPC, GUI, and DWG/DXF fixture checks. Record test evidence in the ledger, including which checks were manual.
4. Run `cargo test -p ocs_plugin_api --features host --lib` and the relevant
   app host tests. Run
   `cargo test --locked --features experimental-host-model --manifest-path plugins/opencad-python/Cargo.toml`
   plus
   `python3 -m unittest discover -s plugins/opencad-python/tests -v`.
5. Check `git diff --check` and repository status, commit host and adapter changes
   together, and record the resulting commit and the next unfinished gate in the ledger.

## Phase 8 progress log

- Added a V4 local-socket nested-request test for `UpdateEntitiesTransaction` using a simulated runner peer and a recording host. This proves the request and response cross the V4 IPC framing and reach `HostApi`; it does **not** prove the actual Python plugin runs through IPC.
- Extended the OCS app-host Point batch test to assert redo after undo. This proves the real app history path for those edits; creation/deletion history remains untested by this case.
- Staged the actual debug Python cdylib with `stage-bundled.sh`. Its generated
  manifest declares API v7 and the exact OCS opencadcodec and Rust compiler
  fingerprints. The plugin now builds from `plugins/opencad-python` in this
  repository and ships from the macOS application resources.
- The opt-in `staged_python_plugin_line_lifecycle_over_real_ipc` app test passed with the staged v7 Python plugin and built OCS executable. It creates, edits, and deletes a Line through Python over the actual runner IPC; confirms three undo entries and undo/redo; and writes/reopens the edited Line as DWG and DXF. It does not exercise GUI picking, multiple tabs, or cancellation. Re-run with `OCS_TEST_PYTHON_PLUGIN=<staged dylib> OCS_PLUGIN_RUNNER_EXE=<built OpenCADStudio executable> cargo test app::plugin_host::tests::staged_python_plugin_line_lifecycle_over_real_ipc --lib -- --test-threads=1 --nocapture` from the OCS checkout.
- The same Line test now also sends a duplicate-handle batch through Python and confirms the host rejects it without changing geometry or creating an undo entry.
- The opt-in `staged_python_point_pick_and_cancel_over_real_ipc` test passes with the same runner. It requests point and entity picks through Python, drives the app's active-command point/object-pick callbacks, polls the returned tokens through a later Python run, and verifies Enter cancellation. The callbacks are invoked by the test harness, not by a physical GUI pointer event; live GUI verification remains open.
- The opt-in `staged_python_tabs_isolate_tokens_and_notifications` test passes with two real OCS document tabs and the staged runner. A token created in tab 0 cannot be observed or consumed in tab 1 before or after completion. Host-to-plugin DrawingChanged notifications for both tab ids are delivered over IPC and drained only by their owning tab. A 257-event burst reports overflow, and `DocumentTabClosed` discards queued events for that tab.
- Added `doc.create_entity(kind, **properties)` and `doc.delete_entity(entity_or_handle)` to the high-level Python document model. Unit tests cover forwarding and managed identity fields. The real staged-plugin Line lifecycle now uses these methods rather than low-level `ocs.add`/`remove_entity`.
- Closed the first new-kind increment for `Tolerance`. The host coverage catalog now maps insertion point, direction, normal, text, dimension style name, text height, and gap; validates geometry and document style references; keeps the style handle read-only; and leaves the undocumented DWG short unmapped. The thin adapter adds generated conversion plus stale style-handle clearing when the style name changes.
- `staged_python_tolerance_lifecycle_over_real_ipc` passes the full C/R/E/D/U/I/W/V/P gate with the actual runner: high-level create/read/transaction edit/delete, live canvas selection, undo/redo, invalid direction and missing style rollback, untouched-field preservation, and DWG/DXF reopen. Tolerance is recorded as **Complete** in the ledger. The queue now continues with `Shape`.
- Closed `Shape` with a generated SHX fixture and a real shape-file `TextStyle`. Host-owned binding resolves the Python style name to the stable table handle required by DWG; validation rejects missing/non-shape styles and invalid geometry. The adapter maps all stable shape fields and keeps the bound style handle read-only.
- `staged_python_shape_lifecycle_over_real_ipc` passes C/R/E/D/U/I/W/V/P through the actual runner, checks the real SHX linework before and after DWG/DXF reopen, and proves that invalid size/style edits remain atomic. The DXF renderer fallback now searches named `is_shape_file` styles as well as legacy unnamed styles. Shape is **Complete**; the queue now continues with `AttributeDefinition`.
- Added creation-time `owner_handle` to the generated document-model adapter. It is present in snapshots and accepted by `doc.create_entity`, but rejected after creation; host transactions independently reject owner changes. Generic host validation now requires a non-null canvas owner to resolve to a block record, except `AttributeEntity`, whose owner may be an `Insert`.
- Mapped stable AttributeDefinition fields and validated tag, placement, normal, height, width, angles, text style, line count and block ownership. `embedded_mtext` stays snapshot-readable but unmapped so partial Python edits preserve its linked R2018+ layout payload.
- `staged_python_attribute_definition_lifecycle_over_real_ipc` passes the real create/read/edit/delete/undo/selection/render/validation/packaged-build path using a structurally valid block and linked Insert. Full DWG state and core DXF geometry survive reopen; deletion stays absent after both formats. OCS also stopped double-converting ATTDEF/ATTRIB DXF rotation already converted by opencadcodec.
- AttributeDefinition remains **Integration-tested / DXF blocked**, not Complete: opencadcodec `568a12c`'s ATTDEF DXF reader did not restore optional AcDbText fields including width factor. Revalidate this recorded failure against the currently merged `7ea4247` revision before keeping or clearing the W-gate blocker.
- Closed `AttributeEntity` without flattening canonical drawing storage. V4 views and IPC snapshots expose each nested child by handle, while host create/update/delete and transactions rewrite the owning Insert so rendering, serialization and undo remain consistent.
- `staged_python_attribute_entity_lifecycle_over_real_ipc` passes C/R/E/D/U/I/W/V/P with the actual v7 adapter. It creates a linked attribute from its block's ATTDEF, reads and edits value/placement/rotation, selects the parent Insert, renders text, rejects invalid tag/height/owner edits atomically, reopens edited and deleted state in DWG and DXF, and proves three-step undo/redo. AttributeEntity is **Complete**; the queue now continues with `Hatch`.
- Completed `Hatch` with full solid/pattern definitions and typed nested boundary paths. The adapter generator now handles single-payload tagged enums as `{kind, value}` dictionaries, which covers every Hatch edge variant and is reusable by later nested entity models. Unknown adapter properties are rejected instead of being silently ignored.
- `staged_python_hatch_lifecycle_over_real_ipc` passes C/R/E/D/U/I/W/V/P with closed line-loop fixtures: solid creation, conversion to a stored-line pattern, boundary replacement, selection and live render-cache checks, atomic invalid scale/association/open-loop/unmapped-payload rejection, edited/deleted DWG and DXF reopen, post-reopen edits, and three-step undo/redo. Hatch is **Complete**; the queue now continues with `Leader`.
- Mapped `Leader` (host validation and reference binding in `entity_coverage.rs`, policy/build.rs/manifest entries, adapter count 23). `staged_python_leader_lifecycle_over_real_ipc` passes C/R/E/D/U/I/V/P through the real runner. W is blocked by opencadcodec: the DXF writer omits the annotation handle (340), DXF reads lose `annotation_offset` (213), and DWG R2010+ does not store text size or hookline flag. Each gap is pinned by a canary assertion so it fails loudly once fixed. The queue now continues with `MLine`.
- Completed `MLine`. Host validation and style binding live in `ocs_plugin_api`; derived vertex geometry and element parameters are recomputed by the host (`normalize_scripted_mline`) on scripted create/edit, so scripts supply positions only. The adapter maps `flags` as an integer through a manifest override because the generator cannot classify opencadcodec's serde-newtype `MLineFlags`. `staged_python_mline_lifecycle_over_real_ipc` passes C/R/E/D/U/I/W/V/P with DWG and DXF reopen. The queue now continues with `Dimension`.
- Completed `Dimension` for all nine subtypes. Because the payload is an enum, the adapter generator gained a `manual_kinds` hook (hand-written `<kind>_to_dict/_from_dict/_apply`) and the host catalog a `synthetic` policy section. The host derives `actual_measurement` and the base definition point on every scripted create/edit. Python ints are now accepted wherever a coordinate or angle is expected. `staged_python_dimension_lifecycle_over_real_ipc` records each subtype separately and all nine pass C/R/E/D/U/I/W/V/P. The queue now continues with `MultiLeader`.
- Completed `MultiLeader`. The adapter generator now converts `Color`, `LineWeight`, mixed unit/payload enums, fixed float arrays and bitflags-2 newtypes, and a `keep_types` override lets a hand-written setter reuse generated converters (used for the merge-style `context` patch). `Leader.override_color` became writable through the same `Color` form; neither file format stores it. `staged_python_multileader_lifecycle_over_real_ipc` passes the full gate with DWG and DXF reopen. The queue now continues with `Table`.
- Completed `Table`. The generator now handles tuples and `usize`; the host keeps DXF cell merge dimensions in step with `merged_ranges`. `staged_python_table_lifecycle_over_real_ipc` passes the full gate with DWG and DXF reopen. The queue now continues with `PolygonMesh`.
- Completed `PolygonMesh`, `PolyfaceMesh` and `Mesh` (queue items 6-8) with one shared `run_mesh_lifecycle` test driver and per-kind host validation. The generator now omits a sub-record's own `EntityCommon` from the scripted form. All three pass the full gate including DWG and DXF reopen with re-edit. The queue now continues with `Helix`.
- Mapped `Helix` with a host-rebuilt spline. `run_mesh_lifecycle` gained per-format DXF expectations so an engine gap is pinned by a canary instead of hidden. All gates pass except W for `handedness` in DXF (reader ignores boolean group 290). The queue now continues with `RasterImage`.
- Mapped `RasterImage`. The host reads the image file, creates and links the `ImageDefinition`, and derives `size` and the default clip boundary. The shared lifecycle driver passes every gate in OCS with a real PNG; `ACAD_IMAGE_DICT`/reactor creation is not implemented, so the kind stays integration-tested. The queue now continues with `Wipeout`.
- Completed `Wipeout` (queue item 11) with rectangular and polygonal clip masks through the shared lifecycle driver; scripted creation matches OCS's native command. The queue now continues with `Underlay`.
- Mapped `Underlay` against a fixture PDF `UnderlayDefinition`. All gates pass except DXF W: the reader does not convert the degree-valued rotation back to radians, and repeated DXF round trips compound the error (pinned by a canary). The queue now continues with `Viewport`.
- Mapped `Viewport` with a paper-space owner rule and frozen-layer reference validation. Every gate except a paper-space canvas oracle passes through the shared driver; the kind stays integration-tested. The queue now continues with `ViewBorder`.
- Mapped `ViewBorder` with viewport and scale reference rules. All gates pass in DWG; DXF loading does not restore the typed entity (pinned by a canary). The queue now continues with `SectionSymbol`.
- Recorded `SectionSymbol` (queue item 15) as **blocked**: it needs a section view style and view representation the host cannot create, and its raw point-count fields have no documented derivation. Mapped `Light` (item 16); every gate passes except DXF `cast_shadows`, which the reader drops (pinned by a canary). The queue now continues with `Region`.
- Recorded `Region`, `Body`, `Solid3D`, `Surface` (queue items 17-20) and `Ole2Frame` (item 21) as **blocked**. The ACIS kinds need a host-side kernel-backed create/transform API that regenerates the payload, wires and silhouettes; the OLE frame needs a valid embedded-storage fixture. No opaque payload is rewritten. With this the ordered queue is complete: 12 kinds are fully complete, Leader, Helix, Underlay, ViewBorder and Light are mapped but blocked at DWG/DXF persistence by opencadcodec gaps, RasterImage and Viewport lack an oracle or linkage, and six kinds are blocked. Next work is the engine fixes behind the canary assertions, the mapped-kind audit of the 17 pre-existing kinds, and merging `origin/main`.
- Audited the 17 pre-existing mapped kinds through the shared real-runner driver (`audit_python_*`). The audit found and fixed genuine validation gaps: Ellipse had none, Text and MText accepted a zero height, Arc accepted non-finite angles, Point and Line accepted infinite thickness, and most basic kinds had no creation-time validation. A shared `validate_basic_entity` now covers Point, Line, Circle, Arc, Ellipse, Text, MText, LwPolyline, Polyline, Polyline2D, Polyline3D and Spline, refusing only problems an edit introduces so already-invalid legacy values stay editable. Twelve of the 17 kinds now pass every gate (Point, Line, Circle, Arc, Ellipse, LwPolyline, Text, MText, Ray, XLine, Solid, Face3D). Four expose persistence findings pinned by canaries (Polyline reopens as Polyline3D; Polyline2D reopens as LwPolyline in DXF; Polyline3D loses elevation in DWG; Spline loses weights in DXF), and Insert is update-only with all applicable gates passing. The driver gained per-format DWG expectations and a host-made block fixture for Insert.
- Audited `AttributeDefinition` against opencadcodec `5b682ed` with a full-field comparison through both formats. Everything passes in DWG. DXF keeps only tag, prompt, value, insertion point, height and rotation because the reader handles eight group codes; alignment, width factor, oblique angle, attribute flags, field length, generation flags, lock and text style reopen as defaults. The kind stays integration-tested and the gap is pinned by canary expectations; the fix belongs in opencadcodec's `read_attdef`. The shared driver's block fixture now also serves `MAKEBLOCK` scripts.
- Decided the two open audit questions. **Insert is creatable** by naming an existing block (existence, ordinary-block and no-cycle checks; attribute records stay with the AttributeEntity flow), which completes the kind. **Legacy Polyline is update-only** with a hint pointing scripts at Polyline2D and Polyline3D, because both formats convert it on save. The generator gained an `update_only_message` for such kinds, integer coordinates are accepted in Insert scales, and the driver gained `MAKEPOLYLINE` and `MAKEBLOCK` fixtures.
- Wrote up the opencadcodec DXF reader gaps as draft issues in [cadcodec-reader-gaps.md](cadcodec-reader-gaps.md) (nothing filed): ATTDEF, boolean groups read through `as_i16()`, UNDERLAY rotation units, LEADER, DRAWINGVIEW/SECTIONLINE dispatch and TABLE merged ranges. Verifying them corrected an earlier attribution: the Spline "DXF drops weights" finding was an OCS bug (the writer emits weights only for a rational spline), now fixed by setting `flags.rational` from the weights, so Spline is complete.
- Mapped `Ole2Frame` as update-only (queue item 21, previously blocked). OCS already builds real embedded frames (`build_embedded_ole`), which supplied the fixture the blocker was waiting for. Frame corners and the aspect lock are scriptable; the opaque storage and envelope are excluded and refused on edit. The audit proves the embedded picture survives DWG and DXF unchanged. Creation from Python remains a follow-up. The remaining blocked kinds are SectionSymbol, Region, Body, Solid3D and Surface.
- Investigated the opencadkernel path for the ACIS kinds ([cadkernel-body-path.md](cadkernel-body-path.md)). The earlier "blocked" reasoning was overstated: OCS already builds primitives, transforms bodies and runs booleans through opencadkernel, and `solid_to_sat` plus `kernel_acis_body` verify each result lifts back with no loss. Two `#[ignore]` spike tests measured exact volumes and DWG and DXF round trips for a box, moved, rotated and mirrored copies, three booleans, four other primitives and a `Body` entity. The real gap is an operation channel from a script to that pipeline; the note recommends an additive `solid_operation` host method and a phased plan (Solid3D create and transform, then booleans and Body, then Region, then Surface). Booleans took about 23 s each in a debug build and need release timing first. Region, Body, Solid3D and Surface move from blocked to unblocked-by-design; SectionSymbol stays blocked.
- Built the opencadkernel operation channel and completed `Solid3D` (create and rigid move). `HostApi::solid_operation` is additive (default "unsupported"), so older hosts and plugins are unaffected; it travels as `PluginRequest::SolidOperation` and `PluginResponse::SolidResult`, and the host records its own undo step only after validation so a refused operation leaves nothing behind. The Python surface is `doc.solids.*`. Every result is verified lossless by the kernel path before it is accepted and a payload that will not lift is refused untouched. `Body` moves through the same channel and `Region` is accepted but untested. Booleans, Region creation from curves and Surface remain, in that order; booleans wait on release-build timing. Solid3D, Body and Region are now mapped read-only with the opaque payload excluded and preserved.
- Completed `Region`. `SolidOperation::RegionFromProfile` reuses the REGION command's kernel calls and `doc.solids.region` exposes it; the lifecycle test creates regions from real circle and polyline entities and covers deletion of the source, refusals, a move, both formats and undo. Along the way `create_entity` now accepts `(x, y, z)` tuples for point properties (decided from the schema's property types) and every generated float setter accepts Python ints, so `create_entity('Circle', center=(0, 0, 0), radius=5)` works. Left: booleans (release timing first), holes and open-curve merging for regions, and `Surface`.
- Completed `Surface` for planes, extruded open profiles, isolines and rigid moves. `SurfaceFromProfile` and `Extrude` join the kernel operations (`doc.solids.surface` and `doc.solids.extrude`); `Extrude` also gives Solid3D extrusions from closed profiles, checked against analytic volumes. The test found and fixed a real bug: Surface's `kind` field silently overwrote the entity-type `kind` key in Python dicts, so it is exposed as `surface_kind`, and a new uniqueness test guards the whole catalog. Transform now accepts surfaces (a plane stays a plane; other kinds turn generic). Left: booleans (release timing first), regions with holes, and lofted, revolved, swept and NURBS surfaces. SectionSymbol is the only unmapped kind.
- Timed booleans in a release build (details in [cadkernel-body-path.md](cadkernel-body-path.md)). A curved box-and-cylinder boolean takes about 1 to 2.2 s, overlapping spheres 0.2 s and planar operands 1 to 2 ms, against 23 s in a debug build. The kernel refuses many curved cases quickly with a reason (`Coincident`, `CutRefused`, `NoClosedForm`), including a second cut in an already drilled box. Recommendation: add a synchronous `SolidOperation::Boolean` that surfaces the refusal and leaves the operands untouched, test it with planar operands plus one ignored curved case, and note the 30 s per-script budget. No code changed in this step.
- Implemented `SolidOperation::Boolean` and `doc.solids.union`, `subtract` and `intersect`, as recommended by the timing study. It runs synchronously, surfaces the kernel's refusal (`Coincident`, `CutRefused`, `NoClosedForm`) with a plain-language reason and the words "nothing was changed", consumes the operands unless `keep_operands`, and records one undo step. The lifecycle test uses planar boxes (millisecond cost, exact volumes, debug-safe) and an `#[ignore]` test documents a curved refusal. With this the kernel operations in the opencadkernel note are all built. SectionSymbol is the only unmapped kind.
- Mapped `SectionSymbol`, the last unmapped kind, so all 43 canvas kinds now have explicit writes. The blocker recorded earlier was overstated: opencadcodec documents `points` as canonical, says the counts equal the point count in every verified export, and provides `sync_display_fields()`, which OCS's own edits already call; OCS also renders a symbol with no style or view link. The host derives the counts and projections when `points` changes and validates that references are existing `SectionViewStyle` and `ViewRep` class objects. Every gate passes except DXF, where opencadcodec reopens a top-level `SECTIONLINE` as an unknown entity (issue 5 in the opencadcodec write-up, pinned by a canary). Still unverifiable here: the meaning of `raw_flags_90` and AutoCAD's acceptance of a symbol with no parent view.
- Completed `Ole2Frame` by adding creation. `SolidOperation::EmbedPicture` and `doc.embed_picture` build the frame from a picture file with OCS's own embedding code, so a script never touches the opaque storage; PNG, JPEG and BMP are stored byte-for-byte and other formats are re-encoded as PNG. The lifecycle test now creates through Python and the new creation-rules test covers formats, aspect, origin, layer and seven refusals. 30 kinds are complete.
- Completed `Viewport` by closing its missing canvas oracle and its derived state. The shared driver now switches the scene to the layout that owns a viewport and requires a drawn frame. The host assigns a unique id (at least 2) and derives `custom_scale = height / view_height` on every scripted create or edit, as MVIEW does, so `custom_scale` became read-only. A control showed my earlier suspicion was wrong: the default id 0 still draws a frame in a layout with no sheet viewport, so the id matters for uniqueness, not visibility. 31 kinds are complete.
- Improved `RasterImage` linkage. A scripted image now reuses one definition per file and registers it in `ACAD_IMAGE_DICT`, which owns it; this persists through DWG and DXF. The `IMAGEDEF_REACTOR` is deliberately not written: its owner convention is ambiguous between opencadcodec's struct docs and its DXF writer and cannot be checked against AutoCAD here, so the kind stays integration-tested. An exploratory spike also showed that OCS's own IMAGE command creates bare images that lose their file path on save, an OCS bug outside this plugin work that is flagged separately.

### 2026-09-21: beyond entities, the layer table (`doc.layers`)

Entity CRUD is at its ceiling without the opencadcodec fixes (PR HakanSeven12/opencadcodec#48), so work moved to
drawing objects a script needs. New additive v7 request `TableOperation` (`LayerCreate`, `LayerModify`,
`LayerRename`, `LayerDelete { erase_objects }`, `LayerSetCurrent`) with `HostApi::table_operation`,
executed by `HostSession::table_operation`. Every refusal (invalid or duplicate name, bad color,
unknown linetype, out-of-range lineweight or transparency, protected layer `0`/`Defpoints`, current
layer, layer holding objects without `erase_objects`) is decided before the undo step is recorded, so
nothing changes. Python: `ocs.active_document.layers` (`create`, `modify`, `rename`, `delete`,
`set_current`, `current`, lookup and iteration). Evidence: `audit_python_layer_table_over_real_ipc`
(19 refusals, live/DWG/DXF persistence of colour, lineweight, transparency, flags, linetype,
description, rename following entities, `erase_objects` and one-step undo), an IPC round-trip test and a
Python unit test. Next: text and dimension styles, linetypes, blocks, layouts; then headless commands.

### 2026-09-21: text and dimension styles (`doc.text_styles`, `doc.dim_styles`)

`TableOperation` grew `TextStyleCreate/Modify`, `DimStyleCreate/Modify` (DimStyle fields travel as a JSON
object, so all 88 are settable with host validation; handles, xref fields and the name are refused),
`StyleRename`, `StyleDelete`, `StyleSetCurrent`. The host reuses OCS's own style machinery (in-use scan,
reference-following rename) by widening four `style_ops` methods to `pub(super)`, and additionally makes a
renamed text style's name follow into dimension styles. Style operations require the active tab.
Evidence: `audit_python_text_and_dim_styles_over_real_ipc` (26 refusals, live/DWG/DXF persistence, copy
semantics, rename following references, current/delete guards, five-step undo), IPC round-trip and Python unit
tests. Four opencadcodec findings (STYLE flags and oblique units, DIMSTYLE name, `true_type_font`) are pinned by
canaries and listed in `cadcodec-reader-gaps.md`. Next: linetypes, blocks, layouts; then headless commands.

### 2026-09-21: blocks (`doc.blocks`)

`TableOperation` gained `BlockCreate` (copy existing model/paper entities into a definition shifted by
`-base_point`, optionally erasing the originals), `BlockModify` (description, explodable, uniform scale),
`BlockRename` (inserts follow, via `Scene::rename_block`) and `BlockDelete` (refused while an insert, a
dimension-style arrow or a multileader uses it; removes the members, markers, record and orphaned draw-order
tables). Creation reuses `Scene::define_block_from_owned_entities`. Python: `blocks.create/modify/rename/delete`,
lookup and iteration, records with member handles and kinds. Evidence: `audit_python_blocks_over_real_ipc`
(18 refusals; shifted geometry and base point; inserts following a rename; original kept vs erased;
live/DWG/DXF persistence; guarded delete; three-step undo restoring the definitions), IPC round-trip and Python
unit tests. One more opencadcodec finding (DXF drops the block description) is pinned by a canary. Not covered:
editing a definition's contents in place, and attribute definitions inside a scripted block. Next: linetypes and
layouts, then headless commands.

### 2026-09-21: block contents (`create_entity(..., block=)`)

`TableOperation::BlockEntityAdd` adds a validated entity to a definition: the host sets the owner, runs the same
validation and reference binding as a new model-space entity, refuses viewports, raster images, block markers,
nested attributes and self-nesting inserts (direct or transitive), then reuses the scene's block-editor routing so
the entity gets normal preparation. Editing and deleting members already worked through the entity API; deleting a
member now also drops its handle from the block record, because the core document leaves a removed entity
listed (`entity_handles`). An entity-delta undo of an add still leaves the handle listed (documented scene behaviour),
so consumers must treat `entity_handles` as candidates and confirm with `get_entity`; the Python block records
already do. Evidence: `audit_python_block_contents_over_real_ipc` (9 refusals; create, edit, delete in a block;
attribute definition inside a block; nested insert; live/DWG/DXF persistence; undo), IPC and Python unit tests.
Next: linetypes and layouts, then headless commands.

### 2026-09-21: linetypes and layouts (`doc.linetypes`, `doc.layouts`)

`TableOperation` gained `LinetypeCreate/Modify/Rename/Delete` (simple patterns only, standard names taken, rename
follows layers and entities, delete refused while a layer, entity, dimension style or the current setting uses it) and
`LayoutCreate/Rename/Delete/SetCurrent/SetPage` (reusing `add_layout`, the default page setup, the sheet viewport,
`rename_layout` plus the layout dictionary key, `delete_layout`, and the application's own `on_layout_switch`; `Model` is
protected; `set_page` sets paper size, rotation and a custom scale and refreshes the limits). The host system variable
`CTAB` now reports the current layout. Evidence: `audit_python_linetypes_and_layouts_over_real_ipc` (34 refusals; patterns,
modify, rename following layers; page setup; an entity created on a sheet and not in model space; live/DWG/DXF
persistence; deleting the current layout; four-step undo restoring the layouts, their entity and the linetype), IPC
round-trip and Python unit tests. Not covered: complex (text/shape) linetypes, reordering layouts, plot devices and
styles, and page-setup names. Next: headless command execution.

### 2026-09-21: command runner and modify wrappers (`doc.command`, `doc.start_command`, `doc.modify`)

`HostApi::run_command(CommandRequest) -> CommandOutcome` (additive v7; `Run`, `Start`, `Point`, `Text`, `Token`, `Entity`,
`Selection`, `Enter`, `Cancel`) drives the real command through the same primitives the automation channel uses
(`run_command_line`, `dispatch_command`, `feed_command`, `feed_active_cmd`, `CommandEscape`) and finishes each step with
`drive_headless_task`, which is synchronous on the host thread. The earlier "nested command replay can hang" concern does not
apply to this path: no step waits on anything that must arrive from outside, the exploration harness ran every command
tried without a hang, and an editor or dialog a command opens is reported in `blocked_by` and closed by `Cancel`. It refuses
while another command is active, on a denylist (`QUIT`, `EXIT`, `CLOSE*`, `NEW`, `QNEW`, `OPEN`, `SAVE*`, `RECOVER`, `SCRIPT`,
`RUNSCRIPT`, `PY_*`), and off the active tab. It does not consult the user's automation on/off switch, which governs the
external MCP/serve channel; a script is already something the user chose to run. Python: `doc.command`, `doc.start_command`
(context manager that cancels a waiting command) and `doc.modify` (offset, trim, extend, fillet, move, copy, rotate, scale,
mirror, erase). The prompt sequences were observed with `spike_command_runner_prompts` (ignored harness). Evidence:
`audit_python_modify_wrappers_over_real_ipc` checks exact geometry for every wrapper (offset distance and side, trim and
extend end points, fillet arc centre and radius, move, copy keeping the original, rotate, scale, mirror keeping the source,
erase), `doc.command` and an interactive session, nine refusals (quit, new, `PY_RUN`, unknown, incomplete line, input with no
command, second command, run while active), no command left running, and a one-step undo. IPC round-trip and Python unit tests.
Not covered: chamfer, arrays, hatch, join, explode, break and the many other commands (reachable through `doc.command` and
`start_command`); reading prompts for commands with a text editor; an on/off setting for script-driven commands.

### 2026-09-21: more modify wrappers

`doc.modify` gained `chamfer`, `array_rect`, `array_polar`, `explode`, `join`, `break_entity`, `stretch` and `lengthen`, each
written against the prompt sequence observed in `spike_command_runner_prompts` (STRETCH needs an Enter after the crossing
window, LENGTHEN and STRETCH end on Enter, EXPLODE and JOIN use the pre-selection because a pick at their selection prompt
selects nothing). `audit_python_modify_wrappers_over_real_ipc` checks exact geometry for each: the chamfer line, all six
rectangular-array members, the four polar-array members (start and half turn), the exploded segments and the removed polyline,
the joined line without its parts, the two pieces after a break, the stretched and the lengthened end points. HATCH is left to
`create_entity("Hatch")`: its command needs an interactive boundary pick that no scripted step can supply. Not wrapped: PEDIT,
path and 3-D arrays, and the remaining commands, which stay reachable through `doc.command` and `start_command`.

### 2026-09-21: nested input is validated (vertex validation gap)

Found while writing the `explode` audit: an `LwPolyline` created from vertices that used the wrong key (`{'x': .., 'y': ..}` instead of
`{'location': ..}`) was accepted and became zero-coordinate vertices. The generated dict-to-struct conversions for nested records
defaulted every absent field and ignored unknown keys, so any typo inside a list of vertices, edges or faces vanished silently.
The generator now (1) refuses an unknown key in every nested record, naming the keys it takes, and (2) enforces a manifest list,
`required_struct_fields`, of the fields that define a nested record's geometry: `LwVertex.location`, `Vertex2D.location`,
`Vertex3D.location`, `Vertex3DPolyline.position`, `PolygonMeshVertex.location`, `PolyfaceVertex.location`, `MLineVertex.position`,
`LineEdge.start/end`, `CircularArcEdge.center/radius`, `EllipticArcEdge.center/major_axis_endpoint`, `PolylineEdge.vertices` and
`SectionSymbolPoint.point`. A `None` counts as missing. Everything else in a nested record still defaults when absent. Evidence:
`audit_python_nested_input_is_validated_over_real_ipc` (eight refusals with the right messages for LwPolyline, Polyline2D and
PolygonMesh; nothing invalid reaches the drawing; the valid forms still create), and every existing per-kind lifecycle audit still passes.

### 2026-09-21: PEDIT and the remaining arrays

`doc.modify` gained `array_path`, `array_3d`, `polyline_close`, `polyline_open`, `polyline_width`, `polyline_reverse` and `polyline_join`,
written against the prompts observed in `spike_command_runner_prompts` (PEDIT offers to turn a line or arc into a polyline and the wrapper
accepts; the join option takes a selection, and its Enter completes the whole command). The default pick point for a polyline is its first
vertex. `audit_python_modify_wrappers_over_real_ipc` checks: closed and opened flags, the widened polyline, the reversed vertex order, the
joined polyline's three vertices with its line consumed, a line turned into a widened polyline (and the line gone), all five path-array
positions (2600 to 2650 in 12.5 steps), and all eight members of the 3-D array (x 2700/2720, y 0/10, z 0/30). Finding: PEDIT's line-to-polyline
step needs between 2 and 4 MiB of stack in a debug build, more than a test thread's 2 MiB, so that audit runs on a 64 MiB thread; the
application's main thread has 8 MiB, and a release build uses far less. Not wrapped: PEDIT fit, spline, decurve, linetype generation and
vertex editing.

### 2026-09-21: guardrails (`SCRIPTCOMMANDS`, stack)

**On/off setting.** A saved user preference, `UserSettings::script_commands` (on by default, so an existing config keeps
scripts allowed), toggled by the `SCRIPTCOMMANDS [0|1]` command, gates `HostApi::run_command`: while it is off every `Run`,
`Start` and input request is refused (`Cancel` stays allowed so a waiting command is never stranded) with a message that says how to turn it back on. The runner refuses the command
itself (it joins `QUIT`, `NEW`, `PY_*` and the rest on the denylist), so a script cannot switch it. It is a command and a
persisted flag only; there is no Options-page control, which would need locale and layout work in files outside this feature.
Evidence: `script_commands_setting_gates_the_runner` (default on; a script cannot change it; the user turns it off and nothing
runs, `Run` and `Start` both refused, no entity added; back on and it runs) and `script_commands_default_on_and_survive_a_round_trip`.
Checked that the test did not write the setting into the real user config.

**Stack.** The planned large-stack thread for command steps was not built, on the evidence: the native builds already link with
at least 8 MiB (`.cargo/config.toml` raises Windows to 16 MiB), the deepest step found (PEDIT converting a line to a polyline)
needs 2 to 4 MiB in a debug build, and the nesting depth is fixed because `PY_*` is refused, so it cannot recurse. Moving the step
to a helper thread would be unsound here (the editor state is not `Send` and some of it has thread affinity), and the alternative,
a stack-growing dependency such as `stacker`, adds a crate to H7's tree to guard a case that cannot occur. The 64 MiB thread in the
PEDIT audit stays, because a test thread gets only 2 MiB.

### 2026-09-21: opencadcodec PR #51 (text and dimension style DXF fixes)

Second small upstream PR, [HakanSeven12/opencadcodec#51](https://github.com/HakanSeven12/opencadcodec/pull/51): STYLE generation
flags (group 71 was hard-coded to 0), the STYLE oblique angle (group 50 is degrees; it was written and read as raw radians)
and the DIMSTYLE text-style name (resolved from the group 340 handle). Three commits, three regression tests, independent of
#48 (merged locally with it; the combined suite passes). Not included: `true_type_font` and the block description, which need
an XDATA layout that cannot be verified here. OCS still pins the unfixed revision, so the canaries in
`audit_python_text_and_dim_styles_over_real_ipc` still assert the wrong result until #51 is merged and OCS repins.

### 2026-09-21: Linux and Windows verification

The change was built in release on Ubuntu and Windows runners with the Python plugin staged beside the executable
(`plugins/opencad.python`, where the host looks for it on both platforms), and the real-IPC plugin tests ran against
that build: **60 of 60 on each** (unsigned portable packages, built by a fork-only workflow that is not part of this change).
The first Windows run failed two tests because the harness substituted the Windows temp directory (`C:\Users\...`) into a
Python string literal, where `\U` is a unicode escape; that was a test-harness path problem, not a product one, and is fixed by
handing the scripts a forward-slash path. Not covered: the whole workspace suite on Windows, the macOS-only bundle path, and an
installer (AppImage or MSI) that includes the plugin.

### 2026-09-21: pull-request check (`python-host-check.yml`)

`.github/workflows/python-host-check.yml` runs on `pull_request` (path-filtered to the plugin API, the plugin, the host's plugin and command
files, settings and the lockfile), on a `pr-check-*` tag and by hand, on `ubuntu-22.04` and `windows-latest`. Steps: the plugin API tests
(`--features host`), staging the bundled plugin and building the runner, the plugin crate tests, the Python model unit tests, and the host's
real-IPC tests. It fails outright if the staged plugin or the runner is missing, because those tests otherwise skip and report success. First run
(`pr-check-1`, commit `1f8cbb81`, all green on both platforms): 115 plugin API tests, 20 plugin crate tests, 16 Python model tests and 60 real-IPC
host tests on each. Debug build, so a cold run is about 30 minutes and a cached one much less. The whole lib suite is deliberately not run by that workflow, because `main`'s own `Tests` workflow (`cargo test --workspace --locked`, added upstream on 21 September) runs it on every pull request. Against the merged tree that command gives 1,805 passed and 1 failed locally on macOS, `fonts_parse_and_resolve`, which also fails on pristine `main` there.

### 2026-09-21: merged upstream `main` (25 commits)

Merged `origin/main` at its head (no conflicts). Upstream added `.github/workflows/ci.yml` (`cargo test --workspace --locked` on Linux for every
pull request, `LC_ALL=C`), removed the Nix files, and added parametric-constraint and viewport-plot changes. Re-verified on the merged tree: the
plugin gate (60 real-IPC host tests, 115 plugin API, 20 plugin crate, 16 Python model), the wasm check, and the upstream workspace command with
`--no-fail-fast` (1,805 passed, 1 failed: `fonts_parse_and_resolve`, which fails on pristine `main` on this Mac too; `arc_grips_drive_center_start_and_end_but_not_midpoint`
now passes, fixed upstream). The change is 72 files, +30,705 / -178 against current `main` (fork-only workflow excluded).

