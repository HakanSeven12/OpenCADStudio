//! Owner stamps: the XDATA record a plugin writes on an entity it owns. Its
//! flags say whether the entity is a selectable custom object, whose native
//! geometry editing is then disabled.

use document::xdata::{ExtendedDataRecord, XDataValue};
use document::EntityType;

const OWNER_STAMP_FLAGS_INDEX: usize = 2;
const SELECTABLE_CUSTOM_FLAG: i32 = 1;

pub(crate) fn selectable_custom_record(record: &ExtendedDataRecord) -> bool {
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
