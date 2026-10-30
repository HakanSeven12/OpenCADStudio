//! Definition-level annotation metadata shared by block creation and editing.

use codec::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};
use codec::{Document, EntityType, Handle};

fn flag(data: Option<&ExtendedData>, app: &str) -> Option<bool> {
    data?
        .get_record(app)?
        .values
        .iter()
        .filter_map(|value| match value {
            XDataValue::Integer16(value) => Some(*value != 0),
            _ => None,
        })
        .last()
}

fn link(data: Option<&ExtendedData>) -> Option<(String, String)> {
    let link = super::hyperlink::Link::from_values(&data?.get_record(super::hyperlink::APP)?.values)?;
    Some((link.url.trim().to_string(), link.text.trim().to_string()))
}

pub(crate) fn hyperlink(document: &Document, name: &str) -> (String, String) {
    let Some(record) = document.block_records.get(name) else {
        return Default::default();
    };
    link(document.object_xdata(record.handle))
        .or_else(|| {
            link(
                document
                    .get_entity(record.block_entity_handle)
                    .map(|entity| &entity.common().extended_data),
            )
        })
        .unwrap_or_default()
}

fn ensure_application(document: &mut Document, app: &str) {
    if !document.app_ids.contains(app) {
        let mut entry = codec::tables::AppId::new(app);
        entry.handle = document.allocate_handle();
        document
            .app_ids
            .add(entry)
            .expect("new metadata application");
    }
}

pub(crate) fn set_hyperlink(
    document: &mut Document,
    definition: Handle,
    url: &str,
    description: &str,
) {
    if url.trim().is_empty() {
        document.remove_object_xdata_record(definition, "PE_URL");
    } else {
        ensure_application(document, "PE_URL");
        // A named location the record already had stays.
        let current = document
            .object_xdata(definition)
            .and_then(|data| data.get_record(super::hyperlink::APP))
            .and_then(|record| super::hyperlink::Link::from_values(&record.values))
            .unwrap_or_default();
        // `file#location` names the location too.
        let (url, location) = super::hyperlink::Link::split_address(url);
        let link = super::hyperlink::Link {
            url,
            text: description.trim().into(),
            location: if location.is_empty() { current.location.clone() } else { location },
            ..current
        };
        let mut record = ExtendedDataRecord::new("PE_URL");
        record.values = link.values();
        document.set_object_xdata_record(definition, record);
    }
    let marker = document
        .block_records
        .iter()
        .find(|record| record.handle == definition)
        .map(|record| record.block_entity_handle);
    if let Some(marker) = marker {
        super::view::dispatch::set_entity_xdata(document, marker, "PE_URL", None);
    }
}

pub(crate) fn annotation_flags(document: &Document, name: &str) -> (bool, bool) {
    let Some(record) = document.block_records.get(name) else {
        return (false, false);
    };
    let data = document.object_xdata(record.handle);
    let marker = document.get_entity(record.block_entity_handle);
    let legacy = marker.map(|entity| &entity.common().extended_data);
    let annotative = flag(data, "AcadAnnotative")
        .or_else(|| flag(legacy, "AcadAnnotative"))
        .unwrap_or(false);
    let orientation = flag(data, "AcadAnnoPO")
        .or_else(|| flag(legacy, "AcadAnnoPO"))
        .unwrap_or(false);
    (annotative, annotative && orientation)
}

pub(crate) fn set_annotation_flags(
    document: &mut Document,
    definition: Handle,
    annotative: bool,
    orientation: bool,
) {
    for app in ["AcadAnnotative", "AcadAnnoPO"] {
        ensure_application(document, app);
    }
    let mut annotation = ExtendedDataRecord::new("AcadAnnotative");
    annotation.values = vec![
        XDataValue::String("AnnotativeData".into()),
        XDataValue::ControlString("{".into()),
        XDataValue::Integer16(1),
        XDataValue::Integer16(i16::from(annotative)),
        XDataValue::ControlString("}".into()),
    ];
    document.set_object_xdata_record(definition, annotation);
    let mut paper = ExtendedDataRecord::new("AcadAnnoPO");
    paper.values = vec![XDataValue::Integer16(i16::from(annotative && orientation))];
    document.set_object_xdata_record(definition, paper);
    // Older definitions stored the flag on the BLOCK marker. Keep one owner.
    let marker = document
        .block_records
        .iter()
        .find(|record| record.handle == definition)
        .map(|record| record.block_entity_handle);
    if let Some(marker) = marker {
        if matches!(document.get_entity(marker), Some(EntityType::Block(_))) {
            for app in ["AcadAnnotative", "AcadAnnoPO"] {
                super::view::dispatch::set_entity_xdata(document, marker, app, None);
            }
        }
    }
}
