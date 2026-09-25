// Codegen for `ocs`'s generic entity conversion functions (`dict_to_entity`
// / `entity_to_dict`), driven by `entity_manifest.json` plus the type
// registry embedded in `ocs_plugin_api` (`get_embedded_type_registry_json`,
// traced by `crates/ocs_plugin_api/build.rs` in the OCS fork).
//
// Adapted from the RustPython-flavored port of `schoeller/ocs_python_repl`'s
// `build/generate.rs` (a PyO3-based generator over the same registry +
// manifest shape) — see `project_opencad_python_generic_entity_crud_plan.md`
// in this session's memory for the full design rationale. The two crates
// target different Python runtimes (PyO3/real CPython vs. RustPython), so
// the leaf conversion code differs, but the manifest schema and the
// registry-driven-with-overrides approach carries over directly.
//
// `include!`'d directly into `build.rs` (not a separate crate) so it shares
// `build.rs`'s own dependency graph without adding a workspace member.

use std::collections::BTreeSet;

use ocs_plugin_api::{FieldInfo, TypeId, TypeInfo, TypeKind, TypeRegistry};
use serde::Deserialize;

// ════════════════════════════════════════════════════════════════════════════
// Manifest schema
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
pub struct Manifest {
    /// Kinds whose payload is a nested enum: converted by hand-written
    /// `<snake>_to_dict`/`<snake>_from_dict`/`<snake>_apply` functions.
    #[serde(default)]
    pub manual_kinds: Vec<String>,
    /// Entity kinds exposed to `ocs.add`/`ocs.update`/`entity_to_dict`. The
    /// central, easy-to-widen filter for Phase 2 (see the plan doc).
    pub type_filter: Vec<String>,
    /// `EntityCommon` fields promoted onto every entity dict. Only `handle`
    /// and `layer` are supported today — the rest of `EntityCommon` (color,
    /// line weight, XDATA, ...) has its own existing `ocs.*` accessors and is
    /// deliberately out of scope for this generic path.
    #[serde(default = "default_base_fields")]
    pub base_fields: Vec<String>,
    #[serde(default)]
    pub overrides: std::collections::BTreeMap<String, EntityOverride>,
    /// Fields of nested structs (a polyline vertex, a hatch edge) that must be
    /// present in a scripted dict. Everything else in a nested struct still
    /// defaults when absent, but an unknown key is always refused.
    #[serde(default)]
    pub required_struct_fields: std::collections::BTreeMap<String, Vec<String>>,
}

fn default_base_fields() -> Vec<String> {
    vec!["handle".to_string(), "layer".to_string()]
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct EntityOverride {
    /// Rust expression constructing a fresh value when this entity has no Default.
    #[serde(default)]
    pub rust_constructor: Option<String>,
    /// Existing entities can be inspected and patched, but `ocs.add` cannot
    /// create one without host-side reference setup.
    #[serde(default)]
    pub update_only: bool,
    /// Message returned when a script tries to create an update-only kind.
    #[serde(default)]
    pub update_only_message: Option<String>,
    #[serde(default)]
    pub fields: std::collections::BTreeMap<String, FieldOverride>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct FieldOverride {
    /// Emit a getter but reject patches to this field.
    #[serde(default)]
    pub read_only: bool,
    /// Rename the dict key for this field.
    #[serde(default)]
    pub python_name: Option<String>,
    /// Raw Rust *value* expression (not yet wrapped in `vm.new_pyobj`),
    /// with `VAR` standing in for the entity's local variable name.
    /// Requires `rust_setter` too.
    #[serde(default)]
    pub rust_getter: Option<String>,
    /// Raw Rust *statement* (semicolon-terminated), with `VAR` standing in
    /// for the entity's local variable name and `dict`/`vm` bound as usual.
    /// Requires `rust_getter` too.
    #[serde(default)]
    pub rust_setter: Option<String>,
    /// Drop this field from both directions entirely (e.g. a field whose
    /// type this generator doesn't support yet, such as a tagged enum).
    #[serde(default)]
    pub exclude: bool,
    /// `ocs.add()` errors if this field is missing rather than silently
    /// defaulting it to zero/empty (Phase 1 review feedback: "add should
    /// reject missing required geometry rather than defaulting to zero").
    /// Only meaningful on the generic (non-`rust_getter`/`rust_setter`)
    /// path — `ocs.update()` never requires a field (a merge onto an
    /// already-valid entity never needs it re-specified).
    #[serde(default)]
    pub required: bool,
    /// Still generate this field type's converters although a
    /// `rust_getter`/`rust_setter` override replaces the generic path (the
    /// override's hand-written code calls them).
    #[serde(default)]
    pub keep_types: bool,
}

impl Manifest {
    pub fn load(json: &str) -> Self {
        serde_json::from_str(json).expect("entity_manifest.json should be valid JSON")
    }
}

fn get<'a>(registry: &'a TypeRegistry, name: &str) -> Option<&'a TypeInfo> {
    registry.types.get(&TypeId::new(name))
}

// ════════════════════════════════════════════════════════════════════════════
// Type classification
// ════════════════════════════════════════════════════════════════════════════

const BUILTIN: &[&str] = &[
    "f64", "f32", "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "bool", "String", "str",
];
/// Types with their own hand-written conversion helpers in `ocs_module.rs`
/// (`vector3_to_py`/`py_to_vector3`, etc.) rather than generated ones.
const SPECIAL: &[&str] = &["Vector3", "Vector2", "Handle", "Color"];

enum Classify {
    /// A C-like enum: every variant is a unit variant. Represented in Python
    /// as its variant name (a plain string).
    UnitEnum,
    /// An enum whose variants carry one payload. Represented as
    /// `{ "kind": "Variant", "value": ... }` so nested geometry such as
    /// hatch boundary edges remains explicit and round-trippable.
    TaggedEnum,
    /// A struct whose only field is `bits` (an opencadcodec bitflags newtype,
    /// e.g. `PolylineFlags`/`VertexFlags`). Represented in Python as a plain
    /// integer via `.bits()`/`::from_bits(..)` — the named boolean accessors
    /// some of these types have (`is_closed`/`set_closed`, ...) are exposed
    /// per-field instead, via a manifest override, not generically.
    BitFlags,
    /// An ordinary struct: represented in Python as a nested dict.
    Struct,
}

fn classify<'a>(name: &str, info: &'a TypeInfo) -> Classify {
    match info.kind {
        TypeKind::Struct => {
            if info.fields.len() == 1 && info.fields[0].name == "bits" {
                Classify::BitFlags
            } else {
                Classify::Struct
            }
        }
        TypeKind::Enum => {
            if info.variants.iter().any(|v| !v.fields.is_empty()) {
                if info.variants.iter().all(|v| v.fields.len() <= 1) {
                    Classify::TaggedEnum
                } else {
                    panic!("entity_manifest.json: {name} has a tagged variant without exactly one payload");
                }
            } else {
                Classify::UnitEnum
            }
        }
        // opencadcodec `bitflags!` types serialize as a newtype over their integer
        // bits; they convert exactly like the hand-written `bits` structs.
        TypeKind::Newtype => Classify::BitFlags,
        other => panic!("entity_manifest.json: {name} has unsupported registry kind {other:?}"),
    }
}

fn bits_field_type(info: &TypeInfo) -> &str {
    info.fields[0].type_id.as_str()
}

// ════════════════════════════════════════════════════════════════════════════
// Helper-type closure (topologically ordered so each generated fn only calls
// ones already emitted above it)
// ════════════════════════════════════════════════════════════════════════════

fn helper_closure(manifest: &Manifest, registry: &TypeRegistry) -> Vec<String> {
    let mut needed: BTreeSet<String> = BTreeSet::new();
    let mut queue: Vec<String> = Vec::new();

    for kind in &manifest.type_filter {
        if manifest.manual_kinds.contains(kind) {
            continue;
        }
        let Some(info) = get(registry, kind) else {
            panic!("entity_manifest.json: unknown entity kind {kind}");
        };
        let override_def = manifest.overrides.get(kind);
        for f in &info.fields {
            if f.name == "common" {
                continue;
            }
            if override_def
                .and_then(|o| o.fields.get(&f.name))
                .map(|o| o.exclude || (o.rust_getter.is_some() && !o.keep_types))
                .unwrap_or(false)
            {
                // Excluded, or fully hand-supplied by the override — its
                // type (if any) never needs a generated conversion pair.
                continue;
            }
            queue.push(f.type_id.as_str().to_string());
        }
    }

    while let Some(name) = queue.pop() {
        if let Some(parts) = tuple_parts(&name) {
            queue.extend(parts.into_iter().map(str::to_owned));
            continue;
        }
        if BUILTIN.contains(&name.as_str())
            || SPECIAL.contains(&name.as_str())
            || f64_array_len(&name).is_some()
        {
            continue;
        }
        if !needed.insert(name.clone()) {
            continue;
        }
        let Some(info) = get(registry, &name) else {
            panic!("entity_manifest.json: type {name} (reached transitively) is not in the type registry");
        };
        for f in &info.fields {
            if f.type_id.as_str() != "EntityCommon" {
                queue.push(f.type_id.as_str().to_string());
            }
        }
        for v in &info.variants {
            for f in &v.fields {
                queue.push(f.type_id.as_str().to_string());
            }
        }
    }

    // Fixpoint topo sort: repeatedly emit any type whose remaining
    // dependencies (among `needed`) are already ordered.
    let mut ordered = Vec::new();
    let mut remaining: Vec<String> = needed.into_iter().collect();
    while !remaining.is_empty() {
        let mut progressed = false;
        for name in remaining.clone() {
            let info = get(registry, &name).unwrap();
            let deps_pending = info
                .fields
                .iter()
                .chain(info.variants.iter().flat_map(|v| v.fields.iter()))
                .any(|f| {
                    let t = f.type_id.as_str();
                    t != name && remaining.iter().any(|r| r == t)
                });
            if !deps_pending {
                ordered.push(name.clone());
                remaining.retain(|n| n != &name);
                progressed = true;
            }
        }
        if !progressed {
            // Shouldn't happen for our closed, cycle-free set of types, but
            // don't hang the build if it ever does.
            ordered.append(&mut remaining);
            break;
        }
    }
    ordered
}

// ════════════════════════════════════════════════════════════════════════════
// Leaf conversion expressions (single value, not sequence/optional wrapping)
// ════════════════════════════════════════════════════════════════════════════

fn snake_case(name: &str) -> String {
    let mut out = String::new();
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(ch.to_ascii_lowercase());
    }
    out
}

fn native_rust_type(type_id: &str) -> &'static str {
    match type_id {
        "f64" | "f32" => "f64",
        "i8" => "i8",
        "i16" => "i16",
        "i32" => "i32",
        "i64" => "i64",
        "u8" => "u8",
        "u16" => "u16",
        "u32" => "u32",
        "u64" => "u64",
        "bool" => "bool",
        "String" | "str" => "String",
        other => panic!("not a native type: {other}"),
    }
}

/// Rust path of a traced type; most live in `codec::entities`.
fn type_path(name: &str) -> String {
    match name {
        "LineWeight" => "codec::types::LineWeight".to_owned(),
        "LeaderLineBreakInfo" => "codec::entities::multileader::LeaderLineBreakInfo".to_owned(),
        _ => format!("codec::entities::{name}"),
    }
}

fn is_native(type_id: &str) -> bool {
    BUILTIN.contains(&type_id)
}

/// Split a tuple type id such as `(u32,CellBorder)` into its element types.
fn tuple_parts(type_id: &str) -> Option<Vec<&str>> {
    let inner = type_id.strip_prefix('(')?.strip_suffix(')')?;
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0i32, 0usize);
    for (i, ch) in inner.char_indices() {
        match ch {
            '(' | '[' | '<' => depth += 1,
            ')' | ']' | '>' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(inner[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(inner[start..].trim());
    (parts.len() >= 2).then_some(parts)
}

/// `[f64; N]` fixed arrays (e.g. a 4x4 transform) convert as flat float lists.
fn f64_array_len(type_id: &str) -> Option<usize> {
    type_id
        .strip_prefix("[f64; ")?
        .strip_suffix(']')?
        .parse()
        .ok()
}

fn default_expr(type_id: &str) -> String {
    if let Some(n) = f64_array_len(type_id) {
        return format!("[0.0f64; {n}]");
    }
    if let Some(parts) = tuple_parts(type_id) {
        let items: Vec<String> = parts.iter().map(|part| default_expr(part)).collect();
        return format!("({})", items.join(", "));
    }
    match type_id {
        "f64" | "f32" => "0.0".to_string(),
        "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" => "0".to_string(),
        "bool" => "false".to_string(),
        "String" | "str" => "String::new()".to_string(),
        "Vector3" => "codec::types::Vector3::ZERO".to_string(),
        "Vector2" => "codec::types::Vector2::ZERO".to_string(),
        "Handle" => "codec::Handle::new(0)".to_string(),
        "Color" => "codec::types::Color::ByLayer".to_string(),
        other => format!("default_{}()", snake_case(other)),
    }
}

/// `item_expr` must already be a `&T` expression (typically a bare place like
/// `&value.field`, sometimes already parenthesized). Result: a
/// `PyResult<PyObjectRef>` expression.
fn leaf_to_py(registry: &TypeRegistry, type_id: &str, item_expr: &str) -> String {
    // A plain function argument (`f(item_expr)`) never needs parens; a method
    // call or cast directly on `item_expr` (`item_expr.method()`,
    // `item_expr as T`) does, since `&` binds looser than both.
    let parenthesized = format!("({item_expr})");
    if f64_array_len(type_id).is_some() {
        return format!(
            "Ok(vm.ctx.new_list(({item_expr}).iter().map(|item| vm.new_pyobj(*item)).collect()).into())"
        );
    }
    if let Some(parts) = tuple_parts(type_id) {
        let items: Vec<String> = parts
            .iter()
            .enumerate()
            .map(|(i, part)| format!("{}?", leaf_to_py(registry, part, &format!("&({item_expr}).{i}"))))
            .collect();
        return format!("Ok(vm.ctx.new_list(vec![{}]).into())", items.join(", "));
    }
    match type_id {
        "f64" | "f32" | "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "bool" => {
            format!("Ok(vm.new_pyobj(*{parenthesized}))")
        }
        "String" | "str" => format!("Ok(vm.new_pyobj({parenthesized}.clone()))"),
        "Vector3" => format!("vector3_to_py_dict(vm, {item_expr})"),
        "Vector2" => format!("vector2_to_py(vm, {item_expr})"),
        "Handle" => format!("Ok(vm.new_pyobj({parenthesized}.value()))"),
        "Color" => format!("color_to_py_dict(vm, {item_expr})"),
        other => {
            let info = get(registry, other)
                .unwrap_or_else(|| panic!("type {other} missing from registry"));
            match classify(other, info) {
                Classify::UnitEnum => {
                    format!("Ok(vm.new_pyobj({}_name({item_expr}).to_owned()))", snake_case(other))
                }
                Classify::TaggedEnum => format!("{}_to_dict(vm, {item_expr})", snake_case(other)),
                Classify::BitFlags => format!("Ok(vm.new_pyobj({parenthesized}.bits() as i64))"),
                Classify::Struct => format!("{}_to_dict(vm, {item_expr})", snake_case(other)),
            }
        }
    }
}

/// `value_expr` must already be an owned `PyObjectRef` expression. Result: a `PyResult<T>` expression.
fn leaf_from_py(registry: &TypeRegistry, type_id: &str, value_expr: &str) -> String {
    if type_id == "f64" {
        // RustPython's f64 conversion rejects ints; scripts write `radius=5`.
        return format!("py_number_to_f64({value_expr}, vm)");
    }
    if type_id == "u64" {
        // serde reports Rust `usize` as `u64`; the inferred cast serves both.
        return format!("{value_expr}.try_into_value::<u64>(vm).map(|n| n as _)");
    }
    if is_native(type_id) {
        return format!("{value_expr}.try_into_value::<{}>(vm)", native_rust_type(type_id));
    }
    if let Some(n) = f64_array_len(type_id) {
        return format!("py_to_f64_array::<{n}>({value_expr}, vm)");
    }
    if let Some(parts) = tuple_parts(type_id) {
        let n = parts.len();
        let items: Vec<String> = parts
            .iter()
            .enumerate()
            .map(|(i, part)| format!("{}?", leaf_from_py(registry, part, &format!("items.remove(0 * {i})"))))
            .collect();
        return format!(
            "(|| -> PyResult<_> {{ let mut items: Vec<PyObjectRef> = ({value_expr}).try_into_value(vm)?; if items.len() != {n} {{ return Err(vm.new_value_error(\"expected a {n}-item list\".to_owned())); }} Ok(({})) }})()",
            items.join(", ")
        );
    }
    match type_id {
        "Vector3" => format!("py_to_vector3_dict({value_expr}, vm)"),
        "Vector2" => format!("py_to_vector2({value_expr}, vm)"),
        "Handle" => format!("{value_expr}.try_into_value::<u64>(vm).map(codec::Handle::new)"),
        "Color" => format!("py_to_color_dict({value_expr}, vm)"),
        other => {
            let info = get(registry, other)
                .unwrap_or_else(|| panic!("type {other} missing from registry"));
            match classify(other, info) {
                Classify::UnitEnum => format!(
                    "{value_expr}.try_into_value::<String>(vm).and_then(|s| str_to_{}(s, vm))",
                    snake_case(other)
                ),
                Classify::TaggedEnum => format!("dict_to_{}({value_expr}, vm)", snake_case(other)),
                Classify::BitFlags => {
                    let path = type_path(other);
                    let bits_ty = bits_field_type(info);
                    if matches!(info.kind, TypeKind::Newtype) {
                        // bitflags 2: `from_bits` is fallible and rejects unknown bits.
                        format!(
                            "{value_expr}.try_into_value::<i64>(vm).and_then(|n| <{bits_ty}>::try_from(n).ok().and_then({path}::from_bits).ok_or_else(|| vm.new_value_error(\"{other} has unknown or out-of-range bits\".to_owned())))"
                        )
                    } else {
                        format!(
                            "{value_expr}.try_into_value::<i64>(vm).and_then(|n| <{bits_ty}>::try_from(n).map({path}::from_bits).map_err(|_| vm.new_value_error(\"{other} bits out of range\".to_owned())))"
                        )
                    }
                }
                Classify::Struct => format!("dict_to_{}({value_expr}, vm)", snake_case(other)),
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Field-level (sequence/optional-aware) getter/setter expressions
// ════════════════════════════════════════════════════════════════════════════

/// `field_place` is a place expression for the field itself (e.g. `"value.radius"`),
/// reached through an already-borrowed parent. Result: a `PyResult<PyObjectRef>` expression.
fn getter_for_field(
    registry: &TypeRegistry,
    type_id: &str,
    optional: bool,
    is_sequence: bool,
    field_place: &str,
) -> String {
    // The registry reports a fixed array as a sequence of itself.
    let is_sequence = is_sequence && f64_array_len(type_id).is_none();
    if is_sequence {
        let inner = leaf_to_py(registry, type_id, "item");
        format!(
            "{{ let items = {field_place}.iter().map(|item| {inner}).collect::<PyResult<Vec<_>>>()?; Ok(vm.ctx.new_list(items).into()) }}"
        )
    } else if optional {
        let inner = leaf_to_py(registry, type_id, "v");
        format!("match &{field_place} {{ None => Ok(vm.ctx.none()), Some(v) => {inner} }}")
    } else {
        leaf_to_py(registry, type_id, &format!("&{field_place}"))
    }
}

/// `py_name` is the dict key to read; `keep_expr` is the expression to use
/// when the key is absent (or explicitly `None`, for non-`Option` fields) —
/// the fresh constructor's value for `ocs.add()`, the field's current value
/// (`{var}.{field}.clone()`) for `ocs.update()`, or a diverging `return Err(..)` block for
/// a required-but-missing `ocs.add()` field. Result: a plain (non-`PyResult`)
/// expression of the field's own Rust type (`T`, `Vec<T>`, or `Option<T>`).
fn setter_for_field(
    registry: &TypeRegistry,
    type_id: &str,
    optional: bool,
    is_sequence: bool,
    py_name: &str,
    keep_expr: &str,
) -> String {
    let is_sequence = is_sequence && f64_array_len(type_id).is_none();
    if is_sequence {
        let inner = leaf_from_py(registry, type_id, "item");
        format!(
            "match dict.get_item_opt(\"{py_name}\", vm)? {{ None => {keep_expr}, Some(v) => {{ let items: Vec<PyObjectRef> = v.try_into_value(vm)?; items.into_iter().map(|item| {inner}).collect::<PyResult<Vec<_>>>()? }} }}"
        )
    } else if optional {
        let inner = leaf_from_py(registry, type_id, "v");
        format!(
            "match dict.get_item_opt(\"{py_name}\", vm)? {{ None => {keep_expr}, Some(v) if vm.is_none(&v) => None, Some(v) => Some({inner}?) }}"
        )
    } else {
        let inner = leaf_from_py(registry, type_id, "v");
        format!(
            "match dict.get_item_opt(\"{py_name}\", vm)? {{ None => {keep_expr}, Some(v) if vm.is_none(&v) => {keep_expr}, Some(v) => {inner}? }}"
        )
    }
}

fn field_default_expr(f: &FieldInfo) -> String {
    if f.is_sequence && f64_array_len(f.type_id.as_str()).is_none() {
        "Vec::new()".to_string()
    } else if f.optional {
        "None".to_string()
    } else {
        default_expr(f.type_id.as_str())
    }
}

/// A diverging block expression: aborts `dict_to_entity` with a clear error
/// instead of producing a value. Valid anywhere an expression of any type is
/// expected (Rust's `!` coerces to everything), so it drops straight into a
/// `match` arm exactly like a real value would.
fn required_error_expr(entity_kind: &str, py_name: &str) -> String {
    format!(
        "{{ return Err(vm.new_value_error(\"ocs.add: {entity_kind} requires \\\"{py_name}\\\"\".to_owned())); }}"
    )
}

/// Wraps an already-built sequence-field setter expression with a
/// non-empty check, for a required sequence field (e.g. a polyline's
/// `vertices`) — covers both "key missing" and "key present but `[]`" in
/// one place, since both parse to the same empty `Vec`.
fn wrap_required_nonempty(setter_expr: &str, entity_kind: &str, py_name: &str) -> String {
    format!(
        "{{ let parsed = {setter_expr}; if parsed.is_empty() {{ return Err(vm.new_value_error(\"ocs.add: {entity_kind} requires at least one \\\"{py_name}\\\"\".to_owned())); }} parsed }}"
    )
}

// ════════════════════════════════════════════════════════════════════════════
// Per-helper-type generation
// ════════════════════════════════════════════════════════════════════════════

fn gen_unit_enum(name: &str, info: &TypeInfo) -> String {
    let path = type_path(name);
    let snake = snake_case(name);
    let mut name_arms = String::new();
    let mut parse_arms = String::new();
    for v in &info.variants {
        name_arms.push_str(&format!("        {path}::{v} => \"{v}\",\n", v = v.name));
        parse_arms.push_str(&format!("        \"{v}\" => Ok({path}::{v}),\n", v = v.name));
    }
    let first_variant = info
        .variants
        .first()
        .unwrap_or_else(|| panic!("{name}: unit enum has no variants"))
        .name
        .clone();
    format!(
        r#"fn {snake}_name(value: &{path}) -> &'static str {{
    match value {{
{name_arms}    }}
}}

#[allow(dead_code)]
fn str_to_{snake}(value: String, vm: &VirtualMachine) -> PyResult<{path}> {{
    match value.as_str() {{
{parse_arms}        other => Err(vm.new_value_error(format!("ocs: unsupported {name} value: {{other}}"))),
    }}
}}

#[allow(dead_code)]
fn default_{snake}() -> {path} {{
    {path}::{first_variant}
}}

"#
    )
}

fn gen_bitflags(name: &str, info: &TypeInfo) -> String {
    let path = type_path(name);
    let snake = snake_case(name);
    let ctor = if matches!(info.kind, TypeKind::Newtype) { "empty" } else { "new" };
    format!(
        r#"#[allow(dead_code)]
fn default_{snake}() -> {path} {{
    {path}::{ctor}()
}}

"#
    )
}

fn gen_tagged_enum(name: &str, info: &TypeInfo, registry: &TypeRegistry) -> String {
    let path = type_path(name);
    let snake = snake_case(name);
    let mut to_arms = String::new();
    let mut from_arms = String::new();
    for variant in &info.variants {
        if variant.fields.is_empty() {
            to_arms.push_str(&format!(
                "        {path}::{variant} => {{ let dict = vm.ctx.new_dict(); dict.set_item(\"kind\", vm.new_pyobj(\"{variant}\"), vm)?; Ok(dict.into()) }}\n",
                variant = variant.name,
            ));
            from_arms.push_str(&format!(
                "        \"{variant}\" => Ok({path}::{variant}),\n",
                variant = variant.name,
            ));
            continue;
        }
        let payload = &variant.fields[0];
        let getter = leaf_to_py(registry, payload.type_id.as_str(), "value");
        let setter = leaf_from_py(registry, payload.type_id.as_str(), "payload");
        to_arms.push_str(&format!(
            "        {path}::{variant}(value) => {{ let dict = vm.ctx.new_dict(); dict.set_item(\"kind\", vm.new_pyobj(\"{variant}\"), vm)?; dict.set_item(\"value\", {getter}?, vm)?; Ok(dict.into()) }}\n",
            variant = variant.name,
        ));
        from_arms.push_str(&format!(
            "        \"{variant}\" => {{ let payload = dict.get_item(\"value\", vm)?; Ok({path}::{variant}({setter}?)) }}\n",
            variant = variant.name,
        ));
    }
    let first = &info.variants[0];
    let default_value = match first.fields.first() {
        None => format!("{path}::{}", first.name),
        Some(payload) => format!("{path}::{}({})", first.name, default_expr(payload.type_id.as_str())),
    };
    format!(
        r#"fn {snake}_to_dict(vm: &VirtualMachine, value: &{path}) -> PyResult<PyObjectRef> {{
    match value {{
{to_arms}    }}
}}

#[allow(dead_code)]
fn dict_to_{snake}(value: PyObjectRef, vm: &VirtualMachine) -> PyResult<{path}> {{
    let dict = value.try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)?;
    let kind = dict.get_item("kind", vm)?.try_into_value::<String>(vm)?;
    match kind.as_str() {{
{from_arms}        other => Err(vm.new_value_error(format!("ocs: unsupported {name} kind: {{other}}"))),
    }}
}}

#[allow(dead_code)]
fn default_{snake}() -> {path} {{
    {default_value}
}}

"#
    )
}

fn gen_struct(name: &str, info: &TypeInfo, registry: &TypeRegistry, required: &[String]) -> String {
    let path = type_path(name);
    let snake = snake_case(name);
    let mut to_dict_body = String::new();
    let mut from_dict_fields = String::new();
    let mut default_fields = String::new();
    let mut allowed: Vec<String> = Vec::new();
    for f in &info.fields {
        if f.type_id.as_str() != "EntityCommon" {
            allowed.push(format!("\"{}\"", f.name));
        }
    }
    let allowed_list = allowed.join(", ");
    let required_list = required
        .iter()
        .map(|field| format!("\"{field}\""))
        .collect::<Vec<_>>()
        .join(", ");
    for f in &info.fields {
        if f.type_id.as_str() == "EntityCommon" {
            // A sub-record's own common data (layer, XDATA, handles) is not part
            // of the scripted model; it defaults on the way in.
            let default = "codec::entities::EntityCommon::default()";
            from_dict_fields.push_str(&format!("        {}: {default},\n", f.name));
            default_fields.push_str(&format!("        {}: {default},\n", f.name));
            continue;
        }
        let getter = getter_for_field(
            registry,
            f.type_id.as_str(),
            f.optional,
            f.is_sequence,
            &format!("value.{}", f.name),
        );
        to_dict_body.push_str(&format!(
            "    dict.set_item(\"{fname}\", {getter}?, vm)?;\n",
            fname = f.name
        ));
        let setter = setter_for_field(
            registry,
            f.type_id.as_str(),
            f.optional,
            f.is_sequence,
            &f.name,
            &field_default_expr(f),
        );
        from_dict_fields.push_str(&format!("        {fname}: {setter},\n", fname = f.name));
        default_fields.push_str(&format!(
            "        {fname}: {def},\n",
            fname = f.name,
            def = field_default_expr(f)
        ));
    }
    format!(
        r#"fn {snake}_to_dict(vm: &VirtualMachine, value: &{path}) -> PyResult<PyObjectRef> {{
    let dict = vm.ctx.new_dict();
{to_dict_body}    Ok(dict.into())
}}

#[allow(dead_code)]
fn dict_to_{snake}(value: PyObjectRef, vm: &VirtualMachine) -> PyResult<{path}> {{
    let dict = value.try_into_value::<rustpython_vm::builtins::PyDictRef>(vm)?;
    let allowed: &[&str] = &[{allowed_list}];
    for key in dict.keys_vec() {{
        let key = key.try_into_value::<String>(vm)?;
        if !allowed.contains(&key.as_str()) {{
            return Err(vm.new_value_error(format!("{name} has no property {{key:?}}; it takes: {{}}", allowed.join(", "))));
        }}
    }}
    let required_fields: &[&str] = &[{required_list}];
    for required in required_fields.iter().copied() {{
        if dict.get_item_opt(required, vm)?.map_or(true, |v| vm.is_none(&v)) {{
            return Err(vm.new_value_error(format!("{name} needs {{required}}")));
        }}
    }}
    Ok({path} {{
{from_dict_fields}    }})
}}

#[allow(dead_code)]
fn default_{snake}() -> {path} {{
    {path} {{
{default_fields}    }}
}}

"#
    )
}

// ════════════════════════════════════════════════════════════════════════════
// Top-level entity fields (registry + manifest overrides)
// ════════════════════════════════════════════════════════════════════════════

struct ResolvedEntityField {
    py_name: String,
    /// A `PyResult<PyObjectRef>` expression.
    getter: String,
    /// A full, semicolon-terminated Rust statement.
    setter: String,
}

/// `ocs.add()` builds an entity from scratch (missing fields fall back to a
/// type default, or error if `required`); `ocs.update()` merges onto an
/// already-`.clone()`d existing entity (missing fields simply keep whatever
/// `var` already holds — see [[project_opencad_python_generic_entity_crud_plan]]
/// in this session's memory for why: reconstructing from a partial dict on
/// update could silently reset properties this generator doesn't represent
/// at all, e.g. `EntityCommon`'s color/line-weight/XDATA).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Add,
    Update,
}

fn resolve_entity_fields(
    manifest: &Manifest,
    registry: &TypeRegistry,
    kind: &str,
    info: &TypeInfo,
    var: &str,
    mode: Mode,
) -> Vec<ResolvedEntityField> {
    let override_def = manifest.overrides.get(kind).cloned().unwrap_or_default();
    let mut out = Vec::new();
    for f in &info.fields {
        if f.name == "common" {
            continue;
        }
        let field_override = override_def.fields.get(&f.name).cloned();
        if let Some(ov) = &field_override {
            if ov.exclude {
                continue;
            }
            if let (Some(getter_expr), Some(setter_stmt)) = (&ov.rust_getter, &ov.rust_setter) {
                assert!(!ov.read_only, "{kind}.{} cannot be read_only with a rust_setter", f.name);
                assert!(
                    !ov.required,
                    "entity_manifest.json: {kind}.{}: \"required\" only applies to the generic \
                     field path, not a rust_getter/rust_setter override",
                    f.name
                );
                let py_name = ov.python_name.clone().unwrap_or_else(|| f.name.clone());
                let getter = format!("Ok(vm.new_pyobj({}))", getter_expr.replace("VAR", var));
                let setter = setter_stmt.replace("VAR", var);
                out.push(ResolvedEntityField { py_name, getter, setter });
                continue;
            }
        }
        let py_name = field_override
            .as_ref()
            .and_then(|o| o.python_name.clone())
            .unwrap_or_else(|| f.name.clone());
        let required = field_override.as_ref().map(|o| o.required).unwrap_or(false);
        let getter = getter_for_field(
            registry,
            f.type_id.as_str(),
            f.optional,
            f.is_sequence,
            &format!("{var}.{}", f.name),
        );
        if field_override.as_ref().is_some_and(|ov| ov.read_only) {
            let setter = format!("if dict.get_item_opt(\"{py_name}\", vm)?.is_some() {{ return Err(vm.new_value_error(\"{kind}.{py_name} is read-only\".to_owned())); }}");
            out.push(ResolvedEntityField { py_name, getter, setter });
            continue;
        }
        let keep_expr = match mode {
            Mode::Update => format!("{var}.{}.clone()", f.name),
            Mode::Add if required && !f.is_sequence => required_error_expr(kind, &py_name),
            Mode::Add => format!("{var}.{}.clone()", f.name),
        };
        let mut setter_expr =
            setter_for_field(registry, f.type_id.as_str(), f.optional, f.is_sequence, &py_name, &keep_expr);
        if mode == Mode::Add && required && f.is_sequence {
            setter_expr = wrap_required_nonempty(&setter_expr, kind, &py_name);
        }
        let setter = format!("{var}.{} = {setter_expr};", f.name);
        out.push(ResolvedEntityField { py_name, getter, setter });
    }
    out
}

fn gen_entity_to_dict(manifest: &Manifest, registry: &TypeRegistry) -> String {
    let mut arms = String::new();
    for kind in &manifest.type_filter {
        if manifest.manual_kinds.contains(kind) {
            arms.push_str(&format!(
                "        codec::EntityType::{kind}(value) => {}_to_dict(vm, value),\n",
                snake_case(kind)
            ));
            continue;
        }
        let info = get(registry, kind).unwrap();
        let var = "value";
        let fields = resolve_entity_fields(manifest, registry, kind, info, var, Mode::Add);
        let mut body = String::new();
        body.push_str("            let dict = vm.ctx.new_dict();\n");
        body.push_str(&format!("            dict.set_item(\"kind\", vm.new_pyobj(\"{kind}\"), vm)?;\n"));
        body.push_str(&format!(
            "            dict.set_item(\"handle\", vm.new_pyobj({var}.common.handle.value()), vm)?;\n"
        ));
        body.push_str(&format!(
            "            dict.set_item(\"layer\", vm.new_pyobj({var}.common.layer.clone()), vm)?;\n"
        ));
        if manifest.base_fields.iter().any(|field| field == "owner_handle") {
            body.push_str(&format!(
                "            dict.set_item(\"owner_handle\", vm.new_pyobj({var}.common.owner_handle.value()), vm)?;\n"
            ));
        }
        for f in &fields {
            body.push_str(&format!(
                "            dict.set_item(\"{}\", {}?, vm)?;\n",
                f.py_name, f.getter
            ));
        }
        body.push_str("            Ok(dict.into())\n");
        arms.push_str(&format!(
            "        codec::EntityType::{kind}({var}) => {{\n{body}        }}\n"
        ));
    }
    format!(
        r#"/// Convert any generic-CRUD-supported entity into a plain dict with a
/// `"kind"` key (matching the convention every other `ocs.*` dict already
/// uses, e.g. XDATA's `{{"kind": ..., "value": ...}}`), plus `"handle"` and
/// `"layer"` and every entity-specific field the type registry knows about.
pub(crate) fn entity_to_dict(vm: &VirtualMachine, entity: &codec::EntityType) -> PyResult<PyObjectRef> {{
    match entity {{
{arms}        _ => Err(vm.new_value_error(
            "ocs: this entity kind is not supported by the generic add/update API yet".to_owned(),
        )),
    }}
}}

"#
    )
}

fn gen_dict_to_entity(manifest: &Manifest, registry: &TypeRegistry) -> String {
    let mut arms = String::new();
    for kind in &manifest.type_filter {
        if let Some(override_def) = manifest.overrides.get(kind).filter(|o| o.update_only) {
            let message = override_def.update_only_message.clone().unwrap_or_else(|| {
                format!("{kind} cannot be created through the generic API; only existing {kind} entities can be edited")
            });
            arms.push_str(&format!(
                "        \"{kind}\" => {{ return Err(vm.new_value_error({message:?}.to_owned())); }}\n"
            ));
            continue;
        }
        if manifest.manual_kinds.contains(kind) {
            arms.push_str(&format!(
                "        \"{kind}\" => codec::EntityType::{kind}({}_from_dict(dict, vm)?),\n",
                snake_case(kind)
            ));
            continue;
        }
        let info = get(registry, kind).unwrap();
        let var = "value";
        let fields = resolve_entity_fields(manifest, registry, kind, info, var, Mode::Add);
        let mut body = String::new();
        let mut allowed = vec!["kind", "handle", "layer"];
        if manifest.base_fields.iter().any(|field| field == "owner_handle") {
            allowed.push("owner_handle");
        }
        allowed.extend(fields.iter().map(|field| field.py_name.as_str()));
        body.push_str(&format!(
            "            ensure_known_entity_keys(dict, \"{kind}\", &{:?}, vm)?;\n",
            allowed
        ));
        let constructor = manifest.overrides.get(kind)
            .and_then(|override_def| override_def.rust_constructor.as_deref())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("codec::entities::{kind}::default()"));
        body.push_str(&format!("            let mut {var} = {constructor};\n"));
        body.push_str(&format!(
            "            {var}.common.handle = codec::Handle::new(get_opt_u64(dict, \"handle\", vm)?);\n"
        ));
        body.push_str(&format!(
            "            {var}.common.layer = get_opt_string(dict, \"layer\", \"0\", vm)?;\n"
        ));
        if manifest.base_fields.iter().any(|field| field == "owner_handle") {
            body.push_str(&format!(
                "            {var}.common.owner_handle = codec::Handle::new(get_opt_u64(dict, \"owner_handle\", vm)?);\n"
            ));
        }
        for f in &fields {
            body.push_str(&format!("            {}\n", f.setter));
        }
        body.push_str(&format!("            codec::EntityType::{kind}({var})\n"));
        arms.push_str(&format!("        \"{kind}\" => {{\n{body}        }}\n"));
    }
    format!(
        r#"/// Build a brand-new entity from a dict with a `"kind"` key (see
/// `entity_to_dict`) — the `ocs.add()` direction. Fields the manifest marks
/// `"required"` (the geometry that actually defines the shape — a line's
/// endpoints, a circle's center/radius, ...) error if missing rather than
/// silently defaulting to zero; every other field defaults sensibly (what a
/// fresh `codec::entities::X::default()` would already hold).
pub(crate) fn dict_to_entity(dict: &rustpython_vm::builtins::PyDictRef, vm: &VirtualMachine) -> PyResult<codec::EntityType> {{
    let kind = dict.get_item("kind", vm)?.try_into_value::<String>(vm)?;
    Ok(match kind.as_str() {{
{arms}        other => {{
            return Err(vm.new_value_error(format!("ocs: unsupported entity kind: {{other}}")));
        }}
    }})
}}

"#
    )
}

fn gen_apply_dict_to_entity(manifest: &Manifest, registry: &TypeRegistry) -> String {
    let mut arms = String::new();
    for kind in &manifest.type_filter {
        if manifest.manual_kinds.contains(kind) {
            arms.push_str(&format!(
                "        codec::EntityType::{kind}(existing_value) => codec::EntityType::{kind}({}_apply(existing_value, dict, vm)?),\n",
                snake_case(kind)
            ));
            continue;
        }
        let info = get(registry, kind).unwrap();
        let var = "value";
        let fields = resolve_entity_fields(manifest, registry, kind, info, var, Mode::Update);
        let mut body = String::new();
        let mut allowed = vec!["kind", "handle", "layer"];
        if manifest.base_fields.iter().any(|field| field == "owner_handle") {
            allowed.push("owner_handle");
        }
        allowed.extend(fields.iter().map(|field| field.py_name.as_str()));
        body.push_str(&format!(
            "            ensure_known_entity_keys(dict, \"{kind}\", &{:?}, vm)?;\n",
            allowed
        ));
        body.push_str(&format!("            let mut {var} = existing_value.clone();\n"));
        // `handle` is never touched — an update can't rehome an entity's
        // identity. `layer` (unlike every other field) is promoted onto
        // every dict by `entity_to_dict`/read by `dict_to_entity` without a
        // manifest entry, so it's handled by hand here too, the same way
        // `dict_to_entity` hand-writes it above — merge semantics: keep the
        // existing layer unless the dict actually specifies one.
        body.push_str(&format!(
            "            {var}.common.layer = match dict.get_item_opt(\"layer\", vm)? {{ \
             None => {var}.common.layer.clone(), \
             Some(v) if vm.is_none(&v) => {var}.common.layer.clone(), \
             Some(v) => v.try_into_value::<String>(vm)? }};\n"
        ));
        if manifest.base_fields.iter().any(|field| field == "owner_handle") {
            body.push_str(&format!(
                "            if let Some(owner) = dict.get_item_opt(\"owner_handle\", vm)? {{ \
                 let owner = owner.try_into_value::<u64>(vm)?; \
                 if owner != {var}.common.owner_handle.value() {{ \
                 return Err(vm.new_value_error(\"owner_handle is read-only after creation\".to_owned())); }} }};\n"
            ));
        }
        for f in &fields {
            body.push_str(&format!("            {}\n", f.setter));
        }
        body.push_str(&format!("            codec::EntityType::{kind}({var})\n"));
        arms.push_str(&format!(
            "        codec::EntityType::{kind}(existing_value) => {{\n{body}        }}\n"
        ));
    }
    format!(
        r#"/// Apply a dict's keys onto a *clone* of `existing` — the `ocs.update()`
/// direction. Unlike `dict_to_entity`, a key missing from `dict` means
/// "leave this field as it already was", not "reset it to a type default":
/// reconstructing a fresh entity from a necessarily-partial dict would
/// silently drop whatever this generator can't represent at all (most of
/// `EntityCommon` — color, line weight, XDATA, ...) the moment a script
/// updates just one field. `existing`'s own kind is authoritative; a `dict`
/// `"kind"` (if present at all — round-tripped from a prior `get_entity()`)
/// is never consulted, since an update can't change an entity's shape.
pub(crate) fn apply_dict_to_entity(
    existing: &codec::EntityType,
    dict: &rustpython_vm::builtins::PyDictRef,
    vm: &VirtualMachine,
) -> PyResult<codec::EntityType> {{
    Ok(match existing {{
{arms}        _ => {{
            return Err(vm.new_value_error(
                "ocs: this entity kind is not supported by the generic add/update API yet"
                    .to_owned(),
            ));
        }}
    }})
}}
"#
    )
}

// ════════════════════════════════════════════════════════════════════════════
// Entry point
// ════════════════════════════════════════════════════════════════════════════

pub fn generate_entity_crud(manifest: &Manifest, registry: &TypeRegistry) -> String {
    let mut out = String::new();
    out.push_str(
        "// @generated by opencad-python/build.rs from entity_manifest.json + the\n\
         // ocs_plugin_api embedded type registry. Do not edit by hand — edit\n\
         // entity_manifest.json or build/generate.rs instead.\n\n",
    );

    for name in helper_closure(manifest, registry) {
        let info = get(registry, &name).unwrap();
        match classify(&name, info) {
            Classify::UnitEnum => out.push_str(&gen_unit_enum(&name, info)),
            Classify::TaggedEnum => out.push_str(&gen_tagged_enum(&name, info, registry)),
            Classify::BitFlags => out.push_str(&gen_bitflags(&name, info)),
            Classify::Struct => {
                let required = manifest.required_struct_fields.get(&name).cloned().unwrap_or_default();
                for field in &required {
                    assert!(
                        info.fields.iter().any(|f| &f.name == field),
                        "entity_manifest.json: required_struct_fields: {name} has no field {field}"
                    );
                }
                out.push_str(&gen_struct(&name, info, registry, &required))
            }
        }
    }

    out.push_str(&gen_entity_to_dict(manifest, registry));
    out.push_str(&gen_dict_to_entity(manifest, registry));
    out.push_str(&gen_apply_dict_to_entity(manifest, registry));
    out
}
