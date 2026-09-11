//! Dynamic text field objects (`AcDbField` / `AcDbFieldList`).

use crate::entities::CellValue;
use crate::types::Handle;

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FieldChildValue {
    pub key: String,
    pub value: CellValue,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Field {
    pub handle: Handle,
    pub owner: Handle,
    pub evaluator_id: String,
    pub code: String,
    pub format: String,
    pub child_fields: Vec<Handle>,
    pub referenced_objects: Vec<Handle>,
    pub evaluation_option: i32,
    pub filing_option: i32,
    pub state: i32,
    pub evaluation_status: i32,
    pub evaluation_error_code: i32,
    pub evaluation_error_message: String,
    pub value: CellValue,
    pub value_string: String,
    pub value_string_length: i32,
    pub child_values: Vec<FieldChildValue>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FieldList {
    pub handle: Handle,
    pub owner: Handle,
    pub unknown: bool,
    pub fields: Vec<Handle>,
}

impl Field {
    pub(crate) fn visit_handles_mut(&mut self, visit: &mut impl FnMut(&mut Handle)) {
        visit(&mut self.owner);
        for handle in &mut self.child_fields {
            visit(handle);
        }
        for handle in &mut self.referenced_objects {
            visit(handle);
        }
        if let Some(handle) = self.value.handle_value.as_mut() {
            visit(handle);
        }
        for child in &mut self.child_values {
            if let Some(handle) = child.value.handle_value.as_mut() {
                visit(handle);
            }
        }
    }
}

impl FieldList {
    pub(crate) fn visit_handles_mut(&mut self, visit: &mut impl FnMut(&mut Handle)) {
        visit(&mut self.owner);
        for handle in &mut self.fields {
            visit(handle);
        }
    }
}
