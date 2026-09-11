use crate::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader;
use crate::io::dwg::dwg_version::DwgVersion;
use crate::objects::{Field, FieldChildValue, FieldList};
use crate::types::Handle;

fn safe_count(value: i32) -> usize {
    value.clamp(0, 20_000) as usize
}

pub fn read_field_object(reader: &mut DwgMergedReader, version: DwgVersion) -> Field {
    let evaluator_id = reader.read_variable_text();
    let code = reader.read_variable_text();
    let child_count = safe_count(reader.read_bit_long());
    let object_count = safe_count(reader.read_bit_long());
    let format = if version.r2007_pre() {
        reader.read_variable_text()
    } else {
        String::new()
    };
    let evaluation_option = reader.read_bit_long();
    let filing_option = reader.read_bit_long();
    let state = reader.read_bit_long();
    let evaluation_status = reader.read_bit_long();
    let evaluation_error_code = reader.read_bit_long();
    let evaluation_error_message = reader.read_variable_text();
    let value = super::entities::read_cad_value(reader, version);
    let value_string = reader.read_variable_text();
    let value_string_length = reader.read_bit_long();
    let child_value_count = safe_count(reader.read_bit_long());
    let mut child_values = Vec::with_capacity(child_value_count);
    for _ in 0..child_value_count {
        child_values.push(FieldChildValue {
            key: reader.read_variable_text(),
            value: super::entities::read_cad_value(reader, version),
        });
    }
    let mut child_fields = Vec::with_capacity(child_count);
    for _ in 0..child_count {
        child_fields.push(Handle::from(reader.read_handle()));
    }
    let mut referenced_objects = Vec::with_capacity(object_count);
    for _ in 0..object_count {
        referenced_objects.push(Handle::from(reader.read_handle()));
    }
    Field {
        evaluator_id,
        code,
        format,
        child_fields,
        referenced_objects,
        evaluation_option,
        filing_option,
        state,
        evaluation_status,
        evaluation_error_code,
        evaluation_error_message,
        value,
        value_string,
        value_string_length,
        child_values,
        ..Field::default()
    }
}

pub fn read_field_list(reader: &mut DwgMergedReader) -> FieldList {
    let count = safe_count(reader.read_bit_long());
    let unknown = reader.read_bit();
    let mut fields = Vec::with_capacity(count);
    for _ in 0..count {
        fields.push(Handle::from(reader.read_handle()));
    }
    FieldList {
        unknown,
        fields,
        ..FieldList::default()
    }
}
