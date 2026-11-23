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
