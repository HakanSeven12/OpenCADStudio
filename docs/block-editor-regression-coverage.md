# Block Editor regression coverage

Automated cases construct records and geometry in Rust and serialize to memory. User drawings are supplementary manual checks and are not test dependencies.

| Removed external scenario | Synthetic coverage |
| --- | --- |
| Distance parameter batches and shared graph | `distance_parameter_batch_native_graph_preserves_point_visibility_and_roundtrips` |
| Grip counts and graph preservation | `block_grip_counts_preserve_action_graphs_and_native_roundtrips` |
| Orientation parameter batches | `orientation_parameter_native_batch_coexists_and_roundtrips_complete_objects` |
| Point graph append and metadata | `point_parameter_native_fields_and_shared_graph_roundtrip` |
| Visibility placement and multiple definitions | `visibility_parameter_in_multiblock_document_roundtrips` |
| Unassociated base grip suppression | `block_key_point_synthetic_suppresses_unassociated_base_grip` |
| Polar Stretch key points, target geometry and reload | `block_polar_stretch_base_and_end_bindings_evaluate_complete_reference_frames`; `block_polar_stretch_parameter_dependencies_remain_metadata_after_reload` covers mixed parameter dependencies |
| Coupled Flip and Linear Stretch lifecycle | `synthetic_coupled_flip_workflow_and_linear_stretch_roundtrip` |
| Window endpoint positions, history and reload | `synthetic_window_endpoint_grips_track_requested_positions`, `synthetic_window_grip_edits_undo_and_redo_the_whole_drawing`, `synthetic_window_grip_edits_persist_through_dwg_and_dxf` |
| Dependent Scale with Stretch, mirrored placement and reset | `synthetic_window_scale_grips_refresh_reload_reset_and_continue` |
| Alignment snapping, history and reload | `block_alignment_runtime_grip_events_preview_history_and_native_roundtrips` |

Deleting a drawing inventory test does not establish equivalent coverage for every imported topology. New failures must be reduced to readable synthetic definitions with exact geometry and state assertions. A successful command or grip-position check alone cannot establish correct evaluated geometry.
