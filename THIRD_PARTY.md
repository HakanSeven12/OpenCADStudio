# Third-party material

## Solidcore

Solidcore, github.com/NVIDIA/solidcore, Copyright NVIDIA Corporation,
Apache License 2.0 (http://www.apache.org/licenses/LICENSE-2.0). Taken at
commit `e261b7d`:

- `tests/fixtures/step/solidcore/*.step`: the STEP fixtures of
  `crates/solidcore-step/tests/fixtures/`, unchanged.
- `examples/step_eval.rs` writes the `eval FILE --json-out OUT` form that
  Solidcore's `bench/step/run.py` reads; no Solidcore code is copied.
