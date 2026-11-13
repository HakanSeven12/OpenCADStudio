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

use document::xdata::{ExtendedDataRecord, XDataValue};
use document::EntityType;
use rustc_hash::FxHashSet;

const OWNER_STAMP_FLAGS_INDEX: usize = 2;
const SELECTABLE_CUSTOM_FLAG: i32 = 1;

/// A resolved custom type label and the façade requested by its owner stamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CustomEntityClassLabel {
    pub label: String,
    pub class_only: bool,
}

fn selectable_custom_record(record: &ExtendedDataRecord) -> bool {
    matches!(
        record.values.get(OWNER_STAMP_FLAGS_INDEX),
        Some(XDataValue::Integer32(flags)) if flags & SELECTABLE_CUSTOM_FLAG != 0
    )
}

/// Native geometry editing is disabled by the persisted owner-stamp flag,
/// independently of plugin liveness and the legacy reference-only set.
pub(crate) fn native_edit_disabled(entity: &EntityType) -> bool {
    entity
        .common()
        .extended_data
        .records()
        .iter()
        .any(|record| {
            let owner_stamp_shape = matches!(record.values.first(), Some(XDataValue::Integer32(_)))
                || record.values.iter().any(
                    |value| matches!(value, XDataValue::String(text) if text.contains("-CLASS:")),
                );
            owner_stamp_shape && selectable_custom_record(record)
        })
}

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
) -> Option<CustomEntityClassLabel> {
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
        resolve_class_facade(&declarations, xdata)
    }
}

/// Pure resolution core over a set of live `(plugin id, declaration)` pairs,
/// in load order. Civil writes `CVIFC` → `CVIFC-CLASS:<key>` directly; staged
/// owner stamps carry `<prefix><key>` as the application name. Only the fixed
/// third flags value opts into the class-only façade.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn resolve_class_facade(
    declarations: &[(String, OwnedEntityClasses)],
    xdata: &ExtendedData,
) -> Option<CustomEntityClassLabel> {
    fn labelled(
        declaration: &OwnedEntityClasses,
        key: &str,
        record: &ExtendedDataRecord,
    ) -> Option<CustomEntityClassLabel> {
        if key.is_empty() {
            return None;
        }
        match declaration.labels.get(key) {
            Some(label) if !label.is_empty() => Some(CustomEntityClassLabel {
                label: label.clone(),
                class_only: selectable_custom_record(record),
            }),
            _ => None,
        }
    }

    for record in xdata.records() {
        if let Some(declaration) = declaration_for(declarations, &record.application_name) {
            for value in &record.values {
                let XDataValue::String(text) = value else {
                    continue;
                };
                let Some(key) = text.strip_prefix(&declaration.class_prefix) else {
                    continue;
                };
                // First class-identity value decides; an empty remainder,
                // unknown key, or empty label all mean the native type.
                return labelled(declaration, key, record);
            }
        }
        if let Some((declaration, key)) = declarations.iter().find_map(|(_, declaration)| {
            record
                .application_name
                .strip_prefix(&declaration.class_prefix)
                .map(|key| (declaration, key))
        }) {
            return labelled(declaration, key, record);
        }
    }
    None
}

#[cfg(all(test, not(target_arch = "wasm32")))]
fn resolve_class_label(
    declarations: &[(String, OwnedEntityClasses)],
    xdata: &ExtendedData,
) -> Option<String> {
    resolve_class_facade(declarations, xdata).map(|class| class.label)
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

/// Display form: a selectable-custom class label alone, otherwise
/// `"<Label> (<Native>)"`, with the native title unchanged when no class
/// matched. One rule for Properties, records, and automation surfaces.
pub(crate) fn display_type_title(
    custom_label: Option<CustomEntityClassLabel>,
    native_title: String,
) -> String {
    match custom_label {
        Some(class) if class.class_only => class.label,
        Some(class) => format!("{} ({native_title})", class.label),
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
            display_type_title(
                Some(CustomEntityClassLabel {
                    label: "Linear Element".to_string(),
                    class_only: false,
                }),
                "Line".to_string(),
            ),
            "Linear Element (Line)"
        );
        assert_eq!(display_type_title(None, "Line".to_string()), "Line");
    }

    #[test]
    fn selectable_custom_stamps_resolve_class_only_for_both_stamp_forms() {
        let declarations = [declaration("opencad.civil_ifc", "CVIFC")];
        let mut direct = ExtendedDataRecord::new("CVIFC");
        direct.add_value(XDataValue::String("CVIFC-CLASS:LINE".to_string()));
        direct.add_value(XDataValue::String(String::new()));
        direct.add_value(XDataValue::Integer32(1));
        let mut direct_entity = EntityType::Line(Line::new());
        direct_entity.common_mut().extended_data.add_record(direct);

        let direct_facade =
            resolve_class_facade(&declarations, &direct_entity.common().extended_data)
                .expect("the direct CVIFC class stamp resolves");
        assert!(direct_facade.class_only);
        assert!(native_edit_disabled(&direct_entity));
        assert_eq!(
            display_type_title(Some(direct_facade), "Line".to_string()),
            "Linear Element"
        );

        let mut staged = ExtendedDataRecord::new("CVIFC-CLASS:LINE");
        staged.add_value(XDataValue::Integer32(1));
        staged.add_value(XDataValue::String(String::new()));
        staged.add_value(XDataValue::Integer32(1));
        let mut staged_entity = EntityType::Line(Line::new());
        staged_entity.common_mut().extended_data.add_record(staged);
        let staged_facade =
            resolve_class_facade(&declarations, &staged_entity.common().extended_data)
                .expect("the application-name class stamp resolves");
        assert!(staged_facade.class_only);
        assert!(native_edit_disabled(&staged_entity));

        let mut legacy = ExtendedDataRecord::new("CVIFC-CLASS:LINE");
        legacy.add_value(XDataValue::Integer32(1));
        let mut legacy_entity = EntityType::Line(Line::new());
        legacy_entity.common_mut().extended_data.add_record(legacy);
        let legacy_facade =
            resolve_class_facade(&declarations, &legacy_entity.common().extended_data)
                .expect("an unflagged application-name stamp still resolves");
        assert!(!legacy_facade.class_only);
        assert!(!native_edit_disabled(&legacy_entity));
        assert_eq!(
            display_type_title(Some(legacy_facade), "Line".to_string()),
            "Linear Element (Line)"
        );
    }
}
