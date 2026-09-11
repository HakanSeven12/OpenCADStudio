use crate::io::dwg::dwg_reference_type::DwgReferenceType;
use crate::objects::{Field, FieldList};

use super::DwgObjectWriter;

impl<'a> DwgObjectWriter<'a> {
    pub(super) fn write_field_object(&mut self, value: &Field) {
        let type_code = self.class_type_code("FIELD", 500);
        self.write_common_non_entity_data(type_code, value.handle, value.owner, &[], &None);
        self.writer.write_variable_text(&value.evaluator_id);
        self.writer.write_variable_text(&value.code);
        self.writer.write_bit_long(value.child_fields.len() as i32);
        self.writer
            .write_bit_long(value.referenced_objects.len() as i32);
        if self.version.r2007_pre() {
            self.writer.write_variable_text(&value.format);
        }
        self.writer.write_bit_long(value.evaluation_option);
        self.writer.write_bit_long(value.filing_option);
        self.writer.write_bit_long(value.state);
        self.writer.write_bit_long(value.evaluation_status);
        self.writer.write_bit_long(value.evaluation_error_code);
        self.writer
            .write_variable_text(&value.evaluation_error_message);
        self.write_table_cad_value(&value.value);
        self.writer.write_variable_text(&value.value_string);
        self.writer.write_bit_long(value.value_string_length);
        self.writer.write_bit_long(value.child_values.len() as i32);
        for item in &value.child_values {
            self.writer.write_variable_text(&item.key);
            self.write_table_cad_value(&item.value);
        }
        for handle in &value.child_fields {
            self.writer
                .write_handle(DwgReferenceType::HardOwnership, handle.value());
        }
        for handle in &value.referenced_objects {
            self.writer
                .write_handle(DwgReferenceType::HardPointer, handle.value());
        }
        self.register_object(value.handle);
    }

    pub(super) fn write_field_list(&mut self, value: &FieldList) {
        let type_code = self.class_type_code("FIELDLIST", 500);
        self.write_common_non_entity_data(type_code, value.handle, value.owner, &[], &None);
        self.writer.write_bit_long(value.fields.len() as i32);
        self.writer.write_bit(value.unknown);
        for handle in &value.fields {
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, handle.value());
        }
        self.register_object(value.handle);
    }
}
