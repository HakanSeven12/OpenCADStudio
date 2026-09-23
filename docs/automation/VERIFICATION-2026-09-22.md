# Automation verification cut — 2026-09-22

This cut records the local evidence for the versioned export, audit, and
verified-save changes. It does not claim AutoCAD application compatibility or
full semantic equivalence beyond the checks listed here.

## Passed

- `cargo check --bin OpenCADStudio`
- `cargo test --lib`: 1,560 passed, 0 failed, 19 ignored.
- `cargo test --lib audit_rejects_an_undeclared_entity_layer -- --nocapture`
- `cargo test --lib save_verified_writes_reopens_hashes_and_matches_manifest -- --nocapture`
- `cargo test --lib explicit_target_version_parser_never_silently_defaults -- --nocapture`
- `cargo test --lib raw_dxf_audit_detects_dangling_handle_references -- --nocapture`
- `cargo test --lib raw_dxf_audit_marks_binary_input_as_skipped -- --nocapture`
- `cargo test --lib raw_dxf_audit_detects_objects_orphaned_from_the_root_dictionary -- --nocapture`
- `cargo test --lib advertises_the_shared_tools -- --nocapture`
- `python docs/automation/mcp_smoke.py target/debug/OpenCADStudio.exe`
- `python docs/automation/mcp_eval.py target/debug/OpenCADStudio.exe`: passed
  with 7 tool calls, 1 asynchronous task and 13 RPC calls.
- `python docs/automation/mcp_acceptance.py target/debug/OpenCADStudio.exe`:
  created five entities through the live GUI/MCP session, captured a 1600x791
  viewport PNG, and verified DWG 2000/2013/2018 plus DXF 2000.
- Follow-up acceptance `20260922-175512` fixed the visual observation below:
  `ZOOM EXTENTS` now includes SDF glyph vertices, the TEXT is visible in the
  1600x791 capture, and all four versioned outputs passed `save_verified` again.
  The portable hashes and exact result boundary are recorded in
  [`evidence/2026-09-22-mcp-acceptance.json`](evidence/2026-09-22-mcp-acceptance.json).
- AutoCAD 2025 Core Console `AUDIT` reported `Total errors found 0 fixed 0`
  for all three DWG outputs. It used isolated profiles after Controlled Folder
  Access blocked Autodesk's normal profile path; no Windows security setting
  was changed.
- cad-core R12 and R2000 fixtures converted through OpenCADStudio to DWG 2000
  and back to DXF 2000; both reopened with the expected normalized semantic
  manifest and ezdxf reported zero audit errors and zero fixes.

## Deliberately rejected

- The observed R12 DXF -> R14 DWG -> R14 DXF path produced six object
  dictionaries orphaned from the root, leaving layout and placeholder owners
  invalid. The raw ASCII DXF graph audit now reports this as a failed audit
  rather than allowing a false-green delivery.

## Resolved observations

- The live MCP acceptance had created and serialized its `TEXT` entity, but
  `ZOOM EXTENTS` considered only ordinary wire points and clipped the SDF text
  above the viewport. It now includes SDF glyph vertices in both the robust
  center filter and fitted bounds. The command-history cancellation was the
  intentional Escape that ends TEXT's repeat mode after a successful batch
  commit, not a lost entity.

## Open observations

- AutoCAD Core Console emitted its final zero-error audit result but did not
  exit after scripted `QUIT` on this workstation. The acceptance runner killed
  only its own isolated process after 25 seconds; therefore the audit is passed
  while process shutdown is recorded as abnormal, not silently reclassified.

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
