# Automation verification cut — 2026-09-22

This cut records the local evidence for the versioned export, audit, and
verified-save changes. It does not claim AutoCAD application compatibility or
full semantic equivalence beyond the checks listed here.

## Passed

- `cargo check --bin OpenCADStudio`
- `cargo test --lib`: 1,559 passed, 0 failed, 19 ignored.
- `cargo test --lib audit_rejects_an_undeclared_entity_layer -- --nocapture`
- `cargo test --lib save_verified_writes_reopens_hashes_and_matches_manifest -- --nocapture`
- `cargo test --lib explicit_target_version_parser_never_silently_defaults -- --nocapture`
- `cargo test --lib raw_dxf_audit_detects_dangling_handle_references -- --nocapture`
- `cargo test --lib raw_dxf_audit_marks_binary_input_as_skipped -- --nocapture`
- `cargo test --lib raw_dxf_audit_detects_objects_orphaned_from_the_root_dictionary -- --nocapture`
- `cargo test --lib advertises_the_shared_tools -- --nocapture`
- `python docs/automation/mcp_smoke.py target/debug/OpenCADStudio.exe`
- cad-core R12 and R2000 fixtures converted through OpenCADStudio to DWG 2000
  and back to DXF 2000; both reopened with the expected normalized semantic
  manifest and ezdxf reported zero audit errors and zero fixes.

## Deliberately rejected

- The observed R12 DXF -> R14 DWG -> R14 DXF path produced six object
  dictionaries orphaned from the root, leaving layout and placeholder owners
  invalid. The raw ASCII DXF graph audit now reports this as a failed audit
  rather than allowing a false-green delivery.

## Boundary

- The official signed installation was left unchanged. Development validation
  used `target/debug/OpenCADStudio.exe` from this checkout.
- `audit` checks model metadata, semantic entity counts, bounds, referenced
  layers and blocks, duplicate handles, serialization, and ASCII DXF raw handle
  references.
- `save_verified` additionally hashes, reopens, checks the exact requested
  target version, and compares the normalized semantic manifest.
- Binary DXF can still be read and written, but the raw ASCII group-code graph
  subcheck is explicitly reported as skipped for binary input/output.
- The repository-wide `cargo fmt --check` is not a green gate in this checkout:
  the current rustfmt reports pre-existing differences across many untouched
  files. `git diff --check` passes for this cut.
