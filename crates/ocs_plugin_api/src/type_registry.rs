//! Embedded type registry generated at build time.
//!
//! The JSON embedded here is produced by tracing a curated allow-list of
//! `acadrust` types with `serde-reflection` and mapping the result into a
//! stable, language-binding-friendly schema defined in
//! [`crate::type_registry_types`].

pub use crate::type_registry_types::*;

/// The embedded type registry as a JSON string.
pub const EMBEDDED_TYPE_REGISTRY_JSON: &str =
    include_str!(concat!(env!("OUT_DIR"), "/type_registry.json"));

/// Convenience accessor for the embedded type registry JSON.
pub fn get_embedded_type_registry_json() -> &'static str {
    EMBEDDED_TYPE_REGISTRY_JSON
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_type_registry_is_valid_json() {
        let json = get_embedded_type_registry_json();
        assert!(!json.is_empty());
        let registry: TypeRegistry = serde_json::from_str(json).expect("valid TypeRegistry JSON");
        assert!(!registry.types.is_empty());
    }

    #[test]
    fn embedded_type_registry_contains_expected_acadrust_types() {
        let registry: TypeRegistry =
            serde_json::from_str(get_embedded_type_registry_json()).expect("valid TypeRegistry JSON");

        for name in ["Point", "Line", "Circle", "MText", "EntityCommon"] {
            assert!(
                registry.types.contains_key(&TypeId::new(name)),
                "registry should contain {}",
                name
            );
        }
    }

    #[test]
    fn embedded_type_registry_mtext_is_struct_with_expected_fields() {
        let registry: TypeRegistry =
            serde_json::from_str(get_embedded_type_registry_json()).expect("valid TypeRegistry JSON");

        let mtext = registry
            .types
            .get(&TypeId::new("MText"))
            .expect("MText in registry");
        assert_eq!(mtext.kind, TypeKind::Struct);

        let field_names: Vec<&str> = mtext.fields.iter().map(|f| f.name.as_str()).collect();
        for expected in [
            "common",
            "value",
            "insertion_point",
            "height",
            "attachment_point",
        ] {
            assert!(
                field_names.contains(&expected),
                "MText should have field {}",
                expected
            );
        }
    }

    #[test]
    fn embedded_type_registry_point_is_struct_with_expected_fields() {
        let registry: TypeRegistry =
            serde_json::from_str(get_embedded_type_registry_json()).expect("valid TypeRegistry JSON");

        let point = registry
            .types
            .get(&TypeId::new("Point"))
            .expect("Point in registry");
        assert_eq!(point.kind, TypeKind::Struct);

        let field_names: Vec<&str> = point.fields.iter().map(|f| f.name.as_str()).collect();
        for expected in ["common", "location", "thickness", "normal"] {
            assert!(
                field_names.contains(&expected),
                "Point should have field {}",
                expected
            );
        }

        let common = point
            .fields
            .iter()
            .find(|f| f.name == "common")
            .expect("common field");
        assert_eq!(common.type_id.as_str(), "EntityCommon");
    }

    #[test]
    fn embedded_type_registry_does_not_contain_entity_type() {
        let registry: TypeRegistry =
            serde_json::from_str(get_embedded_type_registry_json()).expect("valid TypeRegistry JSON");
        assert!(
            !registry.types.contains_key(&TypeId::new("EntityType")),
            "EntityType should not be in the allow-list registry"
        );
    }

    #[test]
    fn type_id_as_str_returns_inner_value() {
        let id = TypeId::new("Point");
        assert_eq!(id.as_str(), "Point");
    }
}
