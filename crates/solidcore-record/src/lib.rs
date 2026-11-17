//! STEP (ISO 10303) as data, vendored from solidcore's `solidcore-record` crate.
//!
//! Only the `step` module is vendored: OpenCADStudio reads and writes STEP
//! through it and needs none of the record crate's other modules (the native
//! B-rep record, profiles, vectors). The files under `src/step` are copied
//! unchanged; see `VENDORED.md`.
#![forbid(unsafe_code)]

pub mod step;
