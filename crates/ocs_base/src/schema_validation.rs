//! Offline validation of application-owned JSON Schema contracts.
use serde_json::Value;

/// Only application-owned schemas enter this cache; client payloads are instances,
/// never schemas. Disable JSON Schema HTTP/file resolution at the dependency level.
pub fn validate_shape(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex, OnceLock},
    };
    static VALIDATORS: OnceLock<Mutex<HashMap<String, Arc<jsonschema::Validator>>>> =
        OnceLock::new();
    let key = schema.to_string();
    let validator = {
        // A schema that does not compile panics below with the lock held. The
        // map is not changed then, so the cache stays good.
        let mut cache = VALIDATORS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(validator) = cache.get(&key) {
            Arc::clone(validator)
        } else {
            let validator = Arc::new(
                jsonschema::validator_for(schema).expect("application-owned schema must compile"),
            );
            cache.insert(key, Arc::clone(&validator));
            validator
        }
    };
    validator.validate(value).map_err(|error| {
        let field = match error.kind() {
            jsonschema::error::ValidationErrorKind::Required { property } => property.as_str(),
            jsonschema::error::ValidationErrorKind::AdditionalProperties { unexpected } => {
                unexpected.first().map(String::as_str)
            }
            _ => None,
        };
        let suffix = field
            .map(|key| format!("/{}", key.replace('~', "~0").replace('/', "~1")))
            .unwrap_or_default();
        format!("{path}{}{suffix}: {error}", error.instance_path())
    })
}

#[cfg(test)]
mod tests {
    use super::validate_shape;
    use serde_json::json;

    #[test]
    fn a_schema_that_does_not_compile_does_not_stop_later_validations() {
        let bad = json!({"type": "no-such-type"});
        let failed = std::panic::catch_unwind(|| validate_shape(&bad, &json!(1), ""));
        assert!(failed.is_err(), "the bad schema must panic");

        let good = json!({"type": "integer"});
        assert_eq!(validate_shape(&good, &json!(1), ""), Ok(()));
        assert!(validate_shape(&good, &json!("text"), "").is_err());
    }
}
