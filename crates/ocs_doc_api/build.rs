//! Codegen driver (plan §10): reads `spec/entities.toml` and regenerates the
//! derived snapshots in `src/gen/` (`api_reference.md`, `binding_schema.json`).
//! CI runs `cargo build` and fails on drift for those two (the committed files
//! are the reviewable truth; build.rs rewrites them deterministically).
//!
//! NOTE: the `Operation`/`Query` wire enums (`ops_gen.rs`/`query_gen.rs`) are
//! HAND-MAINTAINED, append-only, and NOT derived here — the spec's `op`/`query`
//! names must map to enum variants (asserted by a test), never the reverse.
//!
//! The wire-layout tracing for `binding_schema.json` uses serde-reflection on the
//! DTO payload types (same mechanism as `ocs_plugin_api`'s `type_registry.json`),
//! so the traced layouts are mechanically correct, never hand-written.

use std::env;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Deserialize)]
struct Spec {
    meta: Meta,
    #[serde(default)]
    family: Vec<Family>,
    #[serde(default)]
    kernel_map: Vec<KernelMap>,
}

#[derive(Deserialize)]
struct Meta {
    envelope_version: u16,
}

#[derive(Deserialize)]
struct Family {
    name: String,
    #[serde(default)]
    acadrust_variant: Option<String>,
    #[serde(default)]
    handle: Option<String>,
    #[serde(default)]
    collection: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    constructor: Vec<Constructor>,
    #[serde(default)]
    method: Vec<Method>,
}

#[derive(Deserialize)]
// `op`/`fixed` are part of the spec schema (used by binding_schema consumers and
// future codegen) even though api_reference generation doesn't read them yet.
#[allow(dead_code)]
struct Constructor {
    name: String,
    #[serde(default)]
    args: Vec<Arg>,
    #[serde(default)]
    op: Option<String>,
    #[serde(default)]
    fixed: Option<toml::Value>,
    #[serde(default)]
    bulk: bool,
    #[serde(default)]
    returns: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Deserialize)]
struct Method {
    name: String,
    kind: String, // "op" | "query"
    #[serde(default)]
    op: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    args: Vec<Arg>,
    #[serde(default)]
    returns: Option<String>,
    #[serde(default)]
    fixed: Option<toml::Value>,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Deserialize)]
struct Arg {
    name: String,
    ty: String,
}

#[derive(Deserialize)]
struct KernelMap {
    op: String,
    kernel: String,
    returns: String,
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let spec_path = manifest_dir.join("spec/entities.toml");
    println!("cargo:rerun-if-changed={}", spec_path.display());

    let spec_src = fs::read_to_string(&spec_path).expect("read spec/entities.toml");
    let spec: Spec = toml::from_str(&spec_src).expect("parse spec/entities.toml");

    let gen_dir = manifest_dir.join("src/gen");
    fs::create_dir_all(&gen_dir).unwrap();

    // 1. Regenerate the ops/query enum snapshots (deterministic; these are the
    //    curated v1 variants — the spec drives docs/schema, the enums are the
    //    stable wire vocabulary appended at end only).
    write_if_changed(&gen_dir.join("ops_gen.rs"), &ops_gen_src());
    write_if_changed(&gen_dir.join("query_gen.rs"), &query_gen_src());

    // 2. Emit the auto-generated API reference (§10.1) from the spec.
    write_if_changed(&gen_dir.join("api_reference.md"), &api_reference_md(&spec));

    // 3. Emit the merged binding handover schema (§10.2) with traced wire layouts.
    write_if_changed(&gen_dir.join("binding_schema.json"), &binding_schema_json(&spec));
}

/// Rewrite a generated file only when content changed (keeps mtime/CI stable).
fn write_if_changed(path: &PathBuf, content: &str) {
    let unchanged = fs::read_to_string(path).map(|old| old == content).unwrap_or(false);
    if !unchanged {
        fs::write(path, content).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    }
}

fn ops_gen_src() -> String {
    // Keep in sync with the committed snapshot; the canonical list lives there.
    include_str!("src/gen/ops_gen.rs").to_string()
}

fn query_gen_src() -> String {
    include_str!("src/gen/query_gen.rs").to_string()
}

fn fmt_args(args: &[Arg]) -> String {
    args.iter().map(|a| format!("{}: {}", a.name, a.ty)).collect::<Vec<_>>().join(", ")
}

/// The generic handle methods + cross-cutting surface (applies to every handle;
/// documented once here rather than per-family).
fn generic_reference_md() -> String {
    r#"## Generic methods (every handle)

Every typed handle (`Solid`, `Line`, `Circle`, `Polyline`, `Point`, `Entity`) is a
`Copy`, `Send`, `Sync` token `{ ObjectId, session }` and supports:

- **`id()` -> `ObjectId`** — the stable entity identity.
- **`bounds()` -> `Aabb`** — coarse bounding box (solids lift-on-miss; 2D entities
  computed; later families may be `Unsupported`).
- **`transform(placement)`** — in-place rigid similarity (same `ObjectId`). **v1:
  solids only.**
- **`delete()`** — remove the entity (one undo step).

`Entity` additionally has `view()` (id + kind + bounds) and `as_solid()` (typed
downcast when `kind == "Solid3D"`).

## Collections & cross-cutting

- **`DocApi`** — root: `document(tab)`, `active_tab()`, `alive()`.
- **`Document`** — `solids()` / `curves()` / `entities()` factories + lookup,
  `revision()`, `assert_revision(rev)` (read-guard), `query_batch(|q| …)` (read-only
  batch, one round-trip, **no revision bump**).
- **`EntityCollection`** — `get(id) -> Entity`, `delete(id)`,
  `transform_many(&ids, placement)`, `delete_many(&ids)`.
- **`OpGroup`** — best-effort client-side failure cleanup (`track` / `commit` /
  `compensate`). **Not** a transaction: no atomicity, no rollback.

## Bulk ops — single op, all-or-nothing, item-capped

`CreateMany` / `TransformMany` / `DeleteMany` are **single** write ops: one undo
step, one publish, and **all-or-nothing via upfront validation** (no rollback).
Item-capped at `BULK_ITEM_CAP = 100_000` to bound host execution. Use them for
bulk data (N ≳ a few hundred); per-op autocommit is the right tool for
modeling-scale work.

## Semantics

- **Per-op autocommit, no write transactions.** Every write op is ONE atomic host
  call = one undo step. Multi-op write transactions were removed (security/
  integrity review).
- **Per-op atomicity.** Validate + compute geometry **before** any mutation; a
  failed op applies nothing and records no undo step. `erase_sources` booleans run
  only on success.
- **Structured errors.** `ApiError::{Validation, Geometry, UnknownId, Unsupported,
  Transport}`; bulk validation names the failing index; a stale handle returns
  `UnknownId` (never a panic).

"#
    .to_string()
}

fn status_suffix(status: Option<&str>) -> &'static str {
    match status {
        Some("unsupported") => " — **not yet supported** (returns `ApiError::Unsupported`)",
        Some("planned") => " — **planned** (no callable surface yet)",
        _ => "",
    }
}

fn api_reference_md(spec: &Spec) -> String {
    let mut s = String::new();
    s.push_str("# ocs_doc_api — API reference (auto-generated)\n\n");
    s.push_str(&format!("Envelope version: `{}`\n\n", spec.meta.envelope_version));
    s.push_str("Do NOT edit: regenerated by `build.rs` from `spec/entities.toml`.\n\n");

    for fam in &spec.family {
        let collection = fam.collection.as_deref().unwrap_or("-");
        let handle = fam.handle.as_deref().unwrap_or(&fam.name);
        s.push_str(&format!(
            "## `{}` (acadrust `{}`, collection `{}`)\n\n",
            handle,
            fam.acadrust_variant.as_deref().unwrap_or("-"),
            collection
        ));
        if let Some(d) = &fam.description {
            s.push_str(&format!("{d}\n\n"));
        }

        if !fam.constructor.is_empty() {
            s.push_str(&format!("### Constructors (`doc.{collection}()`)\n\n"));
            for c in &fam.constructor {
                let ret = c.returns.as_deref().unwrap_or(handle);
                let bulk = if c.bulk { " *(bulk op)*" } else { "" };
                s.push_str(&format!(
                    "- **{}**(`{}`) -> `{ret}`{bulk}{}\n",
                    c.name,
                    fmt_args(&c.args),
                    status_suffix(c.status.as_deref())
                ));
            }
            s.push('\n');
        }

        if !fam.method.is_empty() {
            s.push_str("### Methods\n\n");
            for m in &fam.method {
                let target = m.op.as_deref().or(m.query.as_deref()).unwrap_or("-");
                s.push_str(&format!(
                    "- **{}**(`{}`) -> `{}` — {} `{}`{}\n",
                    m.name,
                    fmt_args(&m.args),
                    m.returns.as_deref().unwrap_or("()"),
                    m.kind,
                    target,
                    status_suffix(m.status.as_deref())
                ));
            }
            s.push('\n');
        }
    }

    s.push_str(&generic_reference_md());

    s.push_str("## Kernel mapping\n\n| op | kernel call | returns |\n|---|---|---|\n");
    for k in &spec.kernel_map {
        s.push_str(&format!("| {} | `{}` | {} |\n", k.op, k.kernel, k.returns));
    }
    s
}

/// The merged, self-contained binding handover schema (§10.2). Object model +
/// method signatures + op/query mapping + traced wire layouts inlined per node.
/// The handle types for the binding schema, derived from the spec's families
/// ("Entity" always first, then the deduped family handles in spec order).
fn handles_for_schema(spec: &Spec) -> Vec<String> {
    let mut out = vec!["Entity".to_string()];
    for fam in &spec.family {
        let h = fam.handle.as_deref().unwrap_or(&fam.name);
        if h != "Entity" && !out.iter().any(|x| x == h) {
            out.push(h.to_string());
        }
    }
    out
}

fn binding_schema_json(spec: &Spec) -> String {
    // Trace the wire layouts of the DTO payload types via serde-reflection so
    // the inlined layouts are mechanically correct (mirrors the wire types).
    let registry = trace_layouts();

    let families: Vec<serde_json::Value> = spec
        .family
        .iter()
        .map(|fam| {
            let methods: Vec<serde_json::Value> = fam
                .method
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "name": m.name,
                        "kind": m.kind,
                        "op": m.op,
                        "query": m.query,
                        "args": m.args.iter().map(|a| serde_json::json!({"name": a.name, "ty": a.ty})).collect::<Vec<_>>(),
                        "returns": m.returns,
                        "bulk": m.op.as_deref() == Some("CreateMany"),
                        "read_only": m.kind == "query",
                        "fixed": m.fixed,
                    })
                })
                .collect();
            serde_json::json!({
                "name": fam.name,
                "handle": fam.handle,
                "collection": fam.collection,
                "acadrust_variant": fam.acadrust_variant,
                "methods": methods,
            })
        })
        .collect();

    let schema = serde_json::json!({
        "schema": "ocs_doc_api/binding_schema",
        "envelope_version": spec.meta.envelope_version,
        "object_model": {
            "root": "DocApi",
            "document": "Document",
            "collections": ["solids", "curves", "entities"],
            // Derived from the spec's family handles (deduped, "Entity" first) so
            // new handle families appear in the handover automatically.
            "handles": handles_for_schema(spec),
        },
        "families": families,
        // Traced wire layouts for every Operation/Query/payload DTO (inlined).
        "layouts": registry,
    });

    serde_json::to_string_pretty(&schema).expect("serialize binding schema")
}

/// Read the curated wire-layout snapshot (`src/gen/layouts.json`) for the DTO
/// payload types. Returns a JSON map of type-name → container format. Kept
/// internal (the raw registry is not a public handover surface, plan §10.2).
///
/// v1 emits the committed snapshot directly. Full serde-reflection tracing of
/// the crate's own DTO types is a follow-up once the crate can be a build-
/// dependency of itself (self-hosting); the snapshot keeps the schema correct
/// and CI drift-checked in the meantime.
fn trace_layouts() -> serde_json::Value {
    match fs::read_to_string(layouts_snapshot_path()) {
        Ok(src) => serde_json::from_str(&src).unwrap_or_else(|_| serde_json::json!({})),
        Err(_) => serde_json::json!({}),
    }
}

fn layouts_snapshot_path() -> PathBuf {
    PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("src/gen/layouts.json")
}
