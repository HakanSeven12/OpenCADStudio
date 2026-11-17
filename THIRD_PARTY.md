# Third-party code

## solidcore (Apache-2.0)

[solidcore](https://github.com/tangency-labs/solidcore), a Rust port of
NVIDIA's USD BRep SMLib and SM_API (Copyright (c) 2020-2026 NVIDIA
CORPORATION & AFFILIATES, Apache-2.0).

- Vendored crates `crates/step-export` and `crates/solidcore-record` (the
  Part 21 parser, STEP geometry and units readers, the neutral B-rep model and
  its ISO 10303-21 writer), with their `LICENSE` and `NOTICE`. What was copied
  and changed is in `crates/step-export/VENDORED.md`.
- `src/io/step_import.rs`: the readers for `SURFACE_OF_REVOLUTION`,
  `SURFACE_OF_LINEAR_EXTRUSION`, `OFFSET_SURFACE`, `RECTANGULAR_TRIMMED_SURFACE`,
  the Bezier, uniform and quasi-uniform B-splines (their implied knots),
  `PARABOLA`, `HYPERBOLA`, `PCURVE`, `POLY_LOOP`/`FACETED_BREP`, `VERTEX_LOOP`
  and uncertainty units follow solidcore's `solidcore-record/src/step/read.rs`
  and `units.rs`; the reader's caps follow its `p21.rs`, and the seeded panic
  fuzz test its `solidcore-step/tests/panic_fuzz.rs`.
- `src/io/step.rs`: the kernel body to STEP model adapter follows
  solidcore's `step-export/src/brep_to_model.rs`.
- `tests/fixtures/step/solidcore/*.step`: the STEP fixtures of
  `crates/solidcore-step/tests/fixtures/`, unchanged.
- `examples/step_eval.rs` writes the `eval FILE --json-out OUT` form that
  Solidcore's `bench/step/run.py` reads; no Solidcore code is copied.
