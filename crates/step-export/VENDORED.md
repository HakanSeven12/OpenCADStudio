# Vendored from solidcore

`crates/step-export` and `crates/solidcore-record` are copied from
[solidcore](https://github.com/tangency-labs/solidcore) at commit `e261b7d`
(Apache-2.0; see `LICENSE` and `NOTICE` in each crate). solidcore is a Rust
port of NVIDIA's USD BRep SMLib and SM_API (Apache-2.0).

## What was copied

- `crates/step-export/src/{lib.rs, geom.rs, map.rs, validate.rs, import_tests.rs, tetra_disk.stp}`
  from solidcore's `crates/step-export/src`.
- `crates/solidcore-record/src/step.rs` and `src/step/**` from solidcore's
  `crates/solidcore-record/src`.

## Changes from upstream

- `step-export`: `src/brep_to_model.rs` (and its test) are not copied, and the
  two lines of `lib.rs` that declared and re-exported it are removed; it maps
  solidcore's own kernel B-rep, which OpenCADStudio does not use (OCS's
  adapter is `src/io/step.rs`). With it gone the crate needs only
  `solidcore-record`.
- `step-export/src/lib.rs`, marked `OCS:` in the source:
  - shell models (`BodyKind::ShellModel`) are written as
    `SHELL_BASED_SURFACE_MODEL`s of `OPEN_SHELL`s (or `CLOSED_SHELL`s) in a
    `MANIFOLD_SURFACE_SHAPE_REPRESENTATION`, where upstream refuses them;
  - `ExportMetadata::body_styles` gives each body its own layer and colour
    (one `PRESENTATION_LAYER_ASSIGNMENT` per layer, one style per colour), and
    the styled items are collected in a
    `MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION`.
- `solidcore-record`: only the `step` module is vendored; `src/lib.rs` is
  OCS's own (it declares that module alone), so the crate needs only
  `thiserror`. The files under `src/step` are unchanged.
- `Cargo.toml` files are OCS's own (no solidcore workspace inheritance or
  lints).
