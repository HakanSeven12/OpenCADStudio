# Block Editor regression coverage

Updated: 2026-11-07. Integration reviewed through `b0ef6af8f`.

The current DWG control and persistence checks pass. Full-drawing DXF conversion remains open. This document records regression coverage and its limits; it does not certify completion of the entire Block Editor roadmap.

## Test requirements

Automated cases construct records and geometry in Rust and serialize to memory. They require no user drawing, fixture environment variable, personal path or network access. Supplementary application checks use disposable copies and preserve the original.

Use focused assertions for exact geometry, parameter frames, grip positions, isolation and history. The broad control audit requires applied edits and completed commands, but does not replace those geometry assertions. A successful command alone is insufficient evidence of correct evaluation.

Tests intended to cover opening a document must use the application load finalizer where applicable. Raw codec readers alone missed the evaluated-origin defect. Compare geometry independently of entity iteration order after serialization.

## Synthetic coverage map

| Scenario | Tests and assertions |
| --- | --- |
| Distance parameter batches and shared graphs | `distance_parameter_batch_native_graph_preserves_point_visibility_and_roundtrips`: existing objects and graph node/edge prefixes survive append and round trips. |
| Grip counts and graph preservation | `block_grip_counts_preserve_action_graphs_and_native_roundtrips`; `block_grip_counts_reloaded_two_grip_linear_parameter_cycles_and_roundtrips`: cycle counts from a decoded two-grip parameter while preserving actions and graph validity. |
| Orientation parameter batches | `orientation_parameter_native_batch_coexists_and_roundtrips_complete_objects`: native objects coexist and survive both formats. |
| Point graph append and metadata | `point_parameter_appends_to_visibility_and_keeps_existing_objects`; `point_parameter_native_fields_and_shared_graph_roundtrip`. |
| Visibility placement across definitions | `visibility_parameter_in_multiblock_document_roundtrips`: placement on one definition preserves the other definition's protected graph and existing objects. |
| Unassociated base grip suppression | `block_key_point_synthetic_suppresses_unassociated_base_grip`: configured grip locations remain distinct from eligible runtime roles. |
| Polar Stretch frames and parameter dependencies | `block_polar_stretch_base_and_end_bindings_evaluate_complete_reference_frames`; `block_polar_stretch_parameter_dependencies_remain_metadata_after_reload`: exact target geometry, reference isolation and retained parameter metadata. |
| Reversed Polar Stretch line | `block_polar_stretch_end_connection_parameter_dependency_and_reversed_line_roundtrip`: driven vertex at index zero, target filtering and native action-field preservation. |
| Swing thickness and transformed references | `synthetic_swing_door_thickness_grip_tracks_requested_position`: requested grip position, leaf geometry, unchanged arc and sibling isolation. |
| Window endpoint edits, history and persistence | `synthetic_window_endpoint_grips_track_requested_positions`; `synthetic_window_grip_edits_undo_and_redo_the_whole_drawing`; `synthetic_window_grip_edits_persist_through_dwg_and_dxf`: ordinary/mirrored references, exact geometry and another edit after reload. |
| Accumulated dependent Scale state | `test_synthetic_dependent_scale_preserves_geometry_refresh_and_roundtrip`; `dependent_scale_accumulates_base_edits_and_survives_refresh`: centers and radii survive repeated base edits and rebuilds. |
| Scale through application grips and reset | `synthetic_window_scale_grips_refresh_reload_reset_and_continue`: Scale with Stretch, ordinary/mirrored placement, geometry and cache history, refresh, both formats, reset and a fresh edit. |
| Source geometry changes and Test Block reset | `synthetic_scale_refresh_uses_changed_source_geometry`: rebuild from the edited source, continue editing and discard accumulated state on Test Block reset. |
| Nonzero dependent action offsets | `block_action_base_offsets_follow_frames_and_survive_native_rebuilds`; `block_action_base_offset_properties_refresh_references_and_keep_history`: preserve the authored pivot offset during frame changes and Properties refresh. |
| Coupled Flip geometry | `synthetic_coupled_flip_workflow_and_linear_stretch_roundtrip`: both activation orders, exact geometry, native-frame recovery, intermediate undo, refresh, finalized opens, subsequent Stretch, unflip and reset. Includes a transported Base Point. |
| Flip viewport interaction | `coupled_flip_viewport_click_all_masks_and_orders_recover_refresh_and_reset`: supported state masks/orders, direct arrow clicks, arrow reversal, no state menu, marker consistency, recovery, history and persistence. |
| Base Point through finalized opening | `synthetic_basepoint_stretch_grips_survive_finalized_open_and_continue`: world grip positions and Scale geometry survive finalized DWG/DXF loads, then another edit. |
| Alignment interaction | `block_alignment_runtime_grip_events_preview_history_and_native_roundtrips`: viewport input, snapping, preview, history and persistence. |
| Broad reference control audit | `synthetic_dynamic_and_static_references_audit_properties_and_grips`: static/dynamic filtering, plain and mirrored/scaled references, numeric/choice/visibility paths, applied edits and grip command completion. |

The application lifecycle tests are in [synthetic_reference_grips.rs](../src/app/block_editor_tests/synthetic_reference_grips.rs), [window_scale.rs](../src/app/block_editor_tests/window_scale.rs), [flip_frame_grips.rs](../src/app/block_editor_tests/flip_frame_grips.rs) and [visibility_properties.rs](../src/app/block_editor_tests/visibility_properties.rs). Scene coverage is under [block_actions](../src/scene/block_actions), the parameter modules and [grip configuration](../src/scene/block_grips/config.rs).

Deleting an external inventory test does not establish coverage for every imported topology. Reduce new failures to readable synthetic definitions and add focused assertions for the behavior that failed.

## Fixes and integrated audit milestones

| Commit | Result |
| --- | --- |
| `2117f5ccd` | Drawing and Test Block reset discard cached dependent Scale bases. |
| `2a3538bfd` | Added source-geometry refresh and Test Block reset regressions. |
| `ff9a933c4` | Added Polar Stretch parameter-dependency coverage and the initial coverage map. |
| `52294fb7e` | Strengthened exact coupled Flip lifecycle coverage. |
| `585b63dcf` | Preserve authored Scale offsets, migrate cached pivots and refresh against the preceding definition. |
| `bd84a75b8` | Preserve evaluated dynamic origins with Base Point parameters during load finalization. |
| `ad4506761` | Integrated reversed Polar Stretch, decoded grip-count and protected-graph audit coverage. |
| `659851f4d` | Integrated the coupled Flip viewport mask/order audit. |
| `3ecc870b1` | Integrated the synthetic multi-reference control audit. |
| `a0e25bf0a` | Require actual edits, completed grip commands and expected graph types in audit assertions. |

## Recorded validation

These are completed checks from 2026-11-07, not a claim that tests were rerun for this documentation update.

| Check | Recorded result |
| --- | --- |
| Block Editor application suite | 325 passed; no failures or ignored tests in that run. |
| Scene Block-related suite | 265 passed; no failures or ignored tests in that run. |
| Native build | Passed; delivery executable reported `884cb96a7c9e`. |
| Wasm library check | Passed for runtime fixes through `bd84a75b8`; subsequent commits added tests and documentation. |
| Application session used for final drawing checks | Debug executable `bd84a75b86e7`; confirmed from its process/session descriptor and executable version. |
| Original document integrity | Source hash unchanged after checks on separate copies. |

The connector stamped its own older build identity onto the newer GUI session. The application executable identity was checked separately; the connector's stamp alone was not used as evidence of the tested GUI revision.

Supplementary checks exercised eight dynamic references, all 20 exposed Properties rows and 22 grips. Property changes restored their previous values on undo. Grip edits changed evaluated geometry and restored it on undo. The checks included both alignment grips, direct Flip toggles, both Flip orders and repeated mirrored-window edits.

An edited state spanning all eight references and two active Flips passed same-version DWG audit and verified save with no dropped records. Reopening restored property values, world grip positions, reference positions and queried geometry. Another grip edit on every reference passed after reopening. An unflip/reflip cycle restored the saved geometry. These observations establish the tested workflows, not every retained graph or possible edit sequence.

## DXF reference integrity integration

Synthetic coverage now includes acknowledged lossy export with DWG-only unsupported objects. Full-drawing verification on the integrated build remains a separate gate:

- Same-version DWG preserves raw unsupported objects (including `DIMCONTAINER2`) and dynamic block graphs without loss.
- DXF export preserves the loss warning when unacknowledged (`allow_lossy: false`), reporting dropped record counts and refusing to write corrupt data.
- When lossy export is explicitly acknowledged (`allow_lossy: true`), reference-integrity pruning removes dangling handle references to omitted objects and transitively un-emitted child records across dictionary entries, reactors, extension dictionaries, proxy references, and handle fields. The synthetic acknowledged outputs pass structural verification; malformed essential references still cause refusal.
- Structural verification continues to reject broken essential references on surviving native objects (such as corrupt dynamic block evaluation graph node expressions).
- The application cleanup uses the pinned codec; it does not depend on an unpublished codec revision. Its omission policy matches that writer. Supported associative dependencies and their owned bodies are retained, as are image reactors whose targets are entities. A removed dictionary default becomes null rather than selecting another entry.
- `save_as_retained_constraints_clone_connections_runtime_and_history` checks both original and cloned constraint graphs before DXF writing and after DWG/DXF reopening. This caught an overly broad cleanup rule during integration.

Remaining limitations: unsupported DWG-only objects (`DIMCONTAINER2`) remain lossy on DXF export and are not synthesized into native DXF entities. Full semantic decoding of proprietary third-party container schemas remains outside scope.

## Tool Palettes integration

The dockable panel now supports tabs, search, capture from a selected reference or block definition, tool ordering/removal, insertion, persistence and undo/redo. Captured tools carry definition geometry and dependencies into another drawing, including supported dynamic action graphs. Insertion preserves signed per-axis scales and rotation. Synthetic tests evaluate imported dynamic target geometry, rather than checking only a dynamic flag.

Failed imports and recursive insertion are guarded. Dynamic block redefinition is refused before mutation until safe graph/reference migration is implemented. Insertion uses current drawing styling; source reference layer and color are not captured. Cancellation clears the active placement highlight. All 23 locales have Fluent labels and feedback; native-speaker review remains open.

Remaining palette scope: rename tabs, direct viewport drag/drop, editable tool properties, safe dynamic redefinition, optional source-style capture and shared catalogs. This integration does not close those items.

## Integration validation

After review corrections: 328 Block Editor application tests, 78 Dock tests, 265 scene Block-related tests, 108 automation tests, 16 i18n tests and 15 save-crate tests passed. These tests are self-contained; no external drawing dependency was introduced. The initial integrated Block Editor run exposed the constraint-dependency cleanup regression; the corrected full suite passes.

Native build and wasm library check passed for the integrated runtime. A separately launched GUI executable reported `b0ef6af8f689-dirty` (the pending change was this coverage document); its process/session descriptor confirmed the executable identity. The Tool Palettes panel opened and was inspected visually.

Full-drawing DXF verification remains open on that executable: audit reported 200 omitted records, export without acknowledgement refused, and acknowledged export failed with 194 dangling handles and 1,797 orphaned objects (no duplicates). One inspected case is a surviving dictionary whose entity owner is already absent from the loaded document. The output is diagnostic only. Next, reduce missing-owner dictionaries and cascading metadata to a synthetic regression, distinguish safe metadata cleanup from essential graph corruption, and retain loss acknowledgement and structural verification. The original source hash remained unchanged.

The earlier 325-test and all-grip application-session evidence above remains historical evidence for that executable. No new all-grip pass is claimed for the integrated GUI.

## Reproducing the relevant checks

Run from the repository root in PowerShell. No external fixture configuration is needed.

```powershell
$env:RUST_MIN_STACK = '67108864'
$env:ICED_TEST_BACKEND = 'tiny-skia'
cargo test --lib app::block_editor_tests
cargo test --lib scene::block_
cargo test --locked -p ocs_save
cargo test --lib app::dock::tests
cargo test --lib i18n
cargo build --locked --bin OpenCADStudio
cargo check --locked --target wasm32-unknown-unknown --lib
```

Broader roadmap gates remain: retained/native bindings and transform combinations, construction interoperability, Constraint Status, retained constraints in Save Block As, Lookup/property-table options and performance, remaining Tool Palettes scope above, and remaining GUI/localization/performance checks. Existing XY compatibility and provenance holds remain in force. This coverage record does not close those items.
