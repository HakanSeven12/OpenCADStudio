// Plugin-declared entity class labels (API v10).
//
// A plugin that stamps stable class-identity keys into entity XDATA (one
// string value per record, e.g. "CVIFC-CLASS:LINE") and declares an
// `EntityClasses` table through `BuiltinPlugin::entity_classes` gets those
// keys rendered as display labels while the plugin is loaded, e.g. the
// Properties panel title becomes "Linear Element (Line)". The identity keys
// round-trip the drawing; the labels never do — they are plugin-localizable
// display text resolved from the live manager state at query time.
//
// Gating is structural, not incidental: declarations are only ever read
// through the live plugin manager, so a plugin that is not installed,
// disabled, cap-excluded (`OCS_PLUGIN_MAX_API_VERSION`), failed at load, has
// been removed, or whose process has died contributes nothing — its entities
// display as their bare native type. No snapshot of declarations is cached.

use document::EntityType;
use rustc_hash::FxHashSet;

// The wire declaration type and the resolution core exist only where the
// out-of-process plugin runtime does; the web build has no plugins at all.
#[cfg(not(target_arch = "wasm32"))]
use document::xdata::ExtendedData;
#[cfg(not(target_arch = "wasm32"))]
use ocs_plugin_api::ribbon::owned::OwnedEntityClasses;

/// The display label a live, non-disabled plugin declares for `entity`'s
/// XDATA class identity, or `None` for the native fallback.
///
/// Scans the entity's XDATA records for one whose application name is claimed
/// by a live declaration and whose values carry a string starting with the
/// declared class prefix; the remainder is the class key. "Unknown key,
/// prefix-only, or empty label → `None`" is decided by whichever
/// class-identity value is found first: a real marker with no resolvable label
/// falls back to the native type rather than letting an unrelated record
/// win. Foreign-app records and records without a class-identity value are
/// skipped.
pub(crate) fn custom_entity_class_label(
    disabled: &FxHashSet<String>,
    entity: &EntityType,
) -> Option<String> {
    let xdata = &entity.common().extended_data;
    if xdata.records().is_empty() {
        return None;
    }
    #[cfg(target_arch = "wasm32")]
    {
        // No external plugins exist in the web build; the native type stays.
        let _ = disabled;
        None
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let declarations = crate::plugin::external::with_manager(|manager| {
            manager.entity_class_declarations(|id| disabled.contains(id))
        });
        resolve_class_label(&declarations, xdata)
    }
}

/// Pure resolution core over a set of live `(plugin id, declaration)` pairs,
/// in load order. Kept separate from the live-manager plumbing so the
/// semantics test without spawning plugin processes.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn resolve_class_label(
    declarations: &[(String, OwnedEntityClasses)],
    xdata: &ExtendedData,
) -> Option<String> {
    use document::xdata::XDataValue;

    for record in xdata.records() {
        let Some(declaration) = declaration_for(declarations, &record.application_name) else {
            continue; // Foreign application — ignored.
        };
        for value in &record.values {
            let XDataValue::String(text) = value else {
                continue;
            };
            let Some(key) = text.strip_prefix(&declaration.class_prefix) else {
                continue;
            };
            // First class-identity value decides; an empty remainder (prefix
            // only), unknown key, or empty label all mean the native type.
            if key.is_empty() {
                return None;
            }
            return match declaration.labels.get(key) {
                Some(label) if !label.is_empty() => Some(label.clone()),
                _ => None,
            };
        }
    }
    None
}

/// The first declaration (load order) claiming `app_id`. Later duplicates are
/// ignored and reported at debug level — an app id belongs to exactly one
/// loaded plugin.
#[cfg(not(target_arch = "wasm32"))]
fn declaration_for<'a>(
    declarations: &'a [(String, OwnedEntityClasses)],
    app_id: &str,
) -> Option<&'a OwnedEntityClasses> {
    let mut found = None;
    for (plugin_id, classes) in declarations {
        if classes.app_id != app_id {
            continue;
        }
        if found.is_some() {
            log::debug!(
                "[plugin] entity-class app id {app_id} is declared by more than one loaded \
                 plugin; keeping the first declaration, ignoring plugin {plugin_id}"
            );
            continue;
        }
        found = Some(classes);
    }
    found
}

/// Display form: `"<Label> (<Native>)"` when a plugin class label matched,
/// the native title unchanged otherwise. One rule for every surface — the
/// Properties panel title (native = translated `ui_name_or_class`), the
/// records API `display_type`, and the automation entity summary (native =
/// `ui_name`).
pub(crate) fn display_type_title(custom_label: Option<String>, native_title: String) -> String {
    match custom_label {
        Some(label) => format!("{label} ({native_title})"),
        None => native_title,
    }
}

#[cfg(test)]
mod tests {
    use document::entities::{Line, Point};
    use document::xdata::{ExtendedDataRecord, XDataValue};

    use super::*;

    fn declaration(plugin_id: &str, app_id: &str) -> (String, OwnedEntityClasses) {
        (
            plugin_id.to_string(),
            OwnedEntityClasses {
                app_id: app_id.to_string(),
                class_prefix: "CVIFC-CLASS:".to_string(),
                labels: [
                    ("LINE".to_string(), "Linear Element".to_string()),
                    (
                        "CIRCULARARC".to_string(),
                        "Circular Arc Element".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            },
        )
    }

    fn line_with(tag: &str) -> EntityType {
        let mut entity = EntityType::Line(Line::new());
        let mut record = ExtendedDataRecord::new("CVIFC");
        record.add_value(XDataValue::Integer32(1));
        if !tag.is_empty() {
            record.add_value(XDataValue::String(format!("CVIFC-CLASS:{tag}")));
        }
        entity.common_mut().extended_data.add_record(record);
        entity
    }

    #[test]
    fn declared_app_with_known_key_resolves_the_label() {
        let entity = line_with("LINE");
        let xdata = &entity.common().extended_data;
        assert_eq!(
            resolve_class_label(&[declaration("opencad.civil_ifc", "CVIFC")], xdata),
            Some("Linear Element".to_string())
        );
    }

    #[test]
    fn foreign_app_records_are_ignored() {
        let mut entity = EntityType::Point(Point::new());
        let mut record = ExtendedDataRecord::new("SOMEONE-ELSE");
        record.add_value(XDataValue::String("CVIFC-CLASS:LINE".to_string()));
        entity.common_mut().extended_data.add_record(record);

        assert_eq!(
            resolve_class_label(
                &[declaration("opencad.civil_ifc", "CVIFC")],
                &entity.common().extended_data
            ),
            None,
            "a record under a foreign app id must not resolve"
        );
    }

    #[test]
    fn record_without_identity_value_is_ignored_but_not_decisive() {
        // First record: declared app, no class-identity value → skipped.
        // Second record: declared app with a known key → resolves.
        let mut entity = EntityType::Line(Line::new());
        let mut markerless = ExtendedDataRecord::new("CVIFC");
        markerless.add_value(XDataValue::Integer32(7));
        entity.common_mut().extended_data.add_record(markerless);
        let mut keyed = ExtendedDataRecord::new("CVIFC");
        keyed.add_value(XDataValue::String("CVIFC-CLASS:LINE".to_string()));
        entity.common_mut().extended_data.add_record(keyed);

        assert_eq!(
            resolve_class_label(
                &[declaration("opencad.civil_ifc", "CVIFC")],
                &entity.common().extended_data
            ),
            Some("Linear Element".to_string())
        );

        // And with no key anywhere: native fallback.
        assert_eq!(
            resolve_class_label(
                &[declaration("opencad.civil_ifc", "CVIFC")],
                &line_with("").common().extended_data
            ),
            None
        );
    }

    #[test]
    fn prefix_only_value_falls_back_to_native() {
        let mut entity = EntityType::Line(Line::new());
        let mut record = ExtendedDataRecord::new("CVIFC");
        record.add_value(XDataValue::String("CVIFC-CLASS:".to_string()));
        entity.common_mut().extended_data.add_record(record);

        assert_eq!(
            resolve_class_label(
                &[declaration("opencad.civil_ifc", "CVIFC")],
                &entity.common().extended_data
            ),
            None
        );
    }

    #[test]
    fn unknown_key_falls_back_to_native() {
        let entity = line_with("TANGENT");
        let xdata = &entity.common().extended_data;
        assert_eq!(
            resolve_class_label(&[declaration("opencad.civil_ifc", "CVIFC")], xdata),
            None
        );
    }

    #[test]
    fn empty_label_falls_back_to_native() {
        let mut empty_label = declaration("opencad.civil_ifc", "CVIFC");
        empty_label
            .1
            .labels
            .insert("LINE".to_string(), String::new());
        let entity = line_with("LINE");
        let xdata = &entity.common().extended_data;
        assert_eq!(resolve_class_label(&[empty_label], xdata), None);
    }

    #[test]
    fn later_duplicate_app_id_declaration_is_ignored() {
        let mut second = declaration("other.plugin", "CVIFC");
        second.1.labels.clear();
        second
            .1
            .labels
            .insert("LINE".to_string(), "Should Not Win".to_string());
        let entity = line_with("LINE");
        let xdata = &entity.common().extended_data;
        assert_eq!(
            resolve_class_label(&[declaration("opencad.civil_ifc", "CVIFC"), second], xdata),
            Some("Linear Element".to_string()),
            "the first loaded declaration for an app id wins"
        );
    }

    #[test]
    fn no_loaded_plugin_falls_back_to_native() {
        // In the test build the live manager is empty (startup loading is
        // compiled out), so the full path must yield the native fallback for
        // an entity that carries a perfect identity record.
        let entity = line_with("LINE");
        assert_eq!(
            custom_entity_class_label(&FxHashSet::default(), &entity),
            None
        );
    }

    #[test]
    fn display_title_qualifies_native_with_label() {
        assert_eq!(
            display_type_title(Some("Linear Element".to_string()), "Line".to_string()),
            "Linear Element (Line)"
        );
        assert_eq!(display_type_title(None, "Line".to_string()), "Line");
    }
}
