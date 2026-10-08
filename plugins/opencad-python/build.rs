//! Generates `OUT_DIR/entity_crud.rs` — the `dict_to_entity`/`entity_to_dict`
//! conversion functions `ocs_module.rs` `include!`s — from `entity_manifest.json`
//! plus the type registry `ocs_plugin_api` embeds at its own build time (see
//! `crates/ocs_plugin_api::get_embedded_type_registry_json`, always compiled,
//! not gated behind the `host` feature — see that crate's `src/lib.rs`). See
//! `build/generate.rs` for the actual codegen.

use std::env;
use std::path::PathBuf;

include!("build/generate.rs");

fn main() {
    println!("cargo:rerun-if-changed=entity_manifest.json");
    println!("cargo:rerun-if-changed=build/generate.rs");

    let registry_json = ocs_plugin_api::get_embedded_type_registry_json();
    let registry: TypeRegistry = serde_json::from_str(registry_json)
        .expect("ocs_plugin_api's embedded type registry should be valid JSON");

    let manifest = Manifest::load(include_str!("entity_manifest.json"));

    let generated = generate_entity_crud(&manifest, &registry);

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR set by cargo"));
    std::fs::write(out_dir.join("entity_crud.rs"), generated)
        .expect("failed to write generated entity_crud.rs");
}
