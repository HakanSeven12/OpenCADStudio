//! Offline validation of application-owned JSON Schema contracts.
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

const VALIDATOR_CACHE_CAP: usize = 128;

fn format_validation_error(error: jsonschema::ValidationError<'_>, path: &str) -> String {
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
}

pub fn compile_validator(schema: &Value) -> Result<jsonschema::Validator, String> {
    jsonschema::validator_for(schema).map_err(|e| format!("invalid JSON schema: {e}"))
}

pub fn validate_with_validator(
    validator: &jsonschema::Validator,
    value: &Value,
    path: &str,
) -> Result<(), String> {
    validator
        .validate(value)
        .map_err(|error| format_validation_error(error, path))
}

/// Cache validators by a static schema identifier so fixed inline and entity
/// contracts never re-serialize their JSON schema on every validation call.
pub fn validate_static_schema(
    key: &'static str,
    build: impl FnOnce() -> Result<Value, String>,
    value: &Value,
    path: &str,
) -> Result<(), String> {
    static STATIC_VALIDATORS: OnceLock<Mutex<HashMap<&'static str, Arc<jsonschema::Validator>>>> =
        OnceLock::new();
    let cache_mutex = STATIC_VALIDATORS.get_or_init(Default::default);
    let cached = {
        let cache = cache_mutex.lock().unwrap_or_else(|e| e.into_inner());
        cache.get(key).map(Arc::clone)
    };
    let validator = match cached {
        Some(validator) => validator,
        None => {
            let schema = build()?;
            let compiled = Arc::new(compile_validator(&schema)?);
            let mut cache = cache_mutex.lock().unwrap_or_else(|e| e.into_inner());
            Arc::clone(cache.entry(key).or_insert_with(|| Arc::clone(&compiled)))
        }
    };
    validate_with_validator(&validator, value, path)
}

/// Only application-owned schemas enter this cache; client payloads are instances,
/// never schemas. Disable JSON Schema HTTP/file resolution at the dependency level.
pub fn validate_shape(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    static VALIDATORS: OnceLock<Mutex<HashMap<String, Arc<jsonschema::Validator>>>> =
        OnceLock::new();
    let key = schema.to_string();
    let cache_mutex = VALIDATORS.get_or_init(Default::default);
    let cached = {
        let cache = cache_mutex.lock().unwrap_or_else(|e| e.into_inner());
        cache.get(&key).map(Arc::clone)
    };
    let validator = match cached {
        Some(validator) => validator,
        None => {
            let compiled = Arc::new(compile_validator(schema)?);
            let mut cache = cache_mutex.lock().unwrap_or_else(|e| e.into_inner());
            if cache.len() >= VALIDATOR_CACHE_CAP {
                cache.clear();
            }
            cache.insert(key, Arc::clone(&compiled));
            compiled
        }
    };
    validate_with_validator(&validator, value, path)
}

#[cfg(test)]
mod tests {
    use super::{validate_shape, validate_static_schema};
    use serde_json::json;

    #[test]
    fn a_schema_that_does_not_compile_does_not_stop_later_validations() {
        let bad = json!({"type": "no-such-type"});
        assert!(
            validate_shape(&bad, &json!(1), "").is_err(),
            "the bad schema must return Err instead of panicking"
        );

        let good = json!({"type": "integer"});
        assert_eq!(validate_shape(&good, &json!(1), ""), Ok(()));
        assert!(validate_shape(&good, &json!("text"), "").is_err());

        assert!(
            validate_static_schema("test-good-int", || Ok(json!({"type": "integer"})), &json!(42), "").is_ok()
        );
        assert!(
            validate_static_schema("test-good-int", || Ok(json!({"type": "integer"})), &json!("no"), "").is_err()
        );
    }
}
