//! Native DWG writers for database data/index helper objects.

use crate::io::dwg::dwg_reference_type::DwgReferenceType;
use crate::objects::{DataObject, DataObjectData};

use super::{common, DwgObjectWriter};

impl<'a> DwgObjectWriter<'a> {
    pub(super) fn write_data_object(&mut self, object: &DataObject) {
        let type_code = match &object.data {
            DataObjectData::Dummy => common::OBJ_DUMMY,
            DataObjectData::LongTransaction => common::OBJ_LONG_TRANSACTION,
            _ => self.class_type_code(object.dxf_name(), 500),
        };
        self.write_common_non_entity_data(
            type_code,
            object.handle,
            object.owner,
            &object.reactors,
            &object.xdictionary_handle,
        );
        if matches!(&object.data, DataObjectData::BreakData(_)) {
            let owner = self
                .owner_overrides
                .get(&object.handle)
                .copied()
                .unwrap_or(object.owner);
            let reactors = if object.reactors.is_empty() {
                self.document
                    .reactors_by_handle
                    .get(&object.handle)
                    .cloned()
                    .unwrap_or_default()
            } else {
                object.reactors.clone()
            };
            let xdictionary = object.xdictionary_handle.or_else(|| {
                self.document
                    .xdic_by_handle
                    .get(&object.handle)
                    .copied()
                    .filter(|handle| self.document.objects.contains_key(handle))
            });
            let handles = self.writer.handle_mut();
            handles.reset();
            if owner.value() < object.handle.value() {
                let delta = object.handle.value() - owner.value();
                let count = ((64 - delta.leading_zeros() + 7) / 8) as u8;
                handles.write_byte(0xC0 | count);
                let bytes = delta.to_be_bytes();
                handles.write_bytes(&bytes[8 - count as usize..]);
            } else {
                handles.write_handle(DwgReferenceType::SoftPointer, owner.value());
            }
            for reactor in reactors {
                handles.write_handle(DwgReferenceType::SoftPointer, reactor.value());
            }
            if let Some(xdictionary) = xdictionary {
                handles.write_handle(DwgReferenceType::HardOwnership, xdictionary.value());
            } else if !self.version.r2004_plus() {
                handles.write_handle(DwgReferenceType::HardOwnership, 0);
            }
        }
        match &object.data {
            DataObjectData::BreakData(value) => {
                self.writer.write_bit_short(value.version);
                self.writer
                    .write_bit_long(value.point_references.len() as i32);
                for reference in &value.point_references {
                    self.writer.write_bit_short(reference.version);
                    self.writer.write_bit(reference.reserved != 0);
                    self.writer.write_bit_long(reference.reference_type);
                    self.writer.write_bit_short(reference.flags);
                    self.writer.write_bit_long(reference.identifier);
                    self.writer.write_3bit_double(reference.first_point);
                    self.writer.write_3bit_double(reference.second_point);
                    self.writer.write_bit_short(reference.trailing_version);
                }
                self.writer.write_handle(
                    DwgReferenceType::SoftPointer,
                    value.dimension_reference.value(),
                );
                self.writer.write_handle(
                    DwgReferenceType::SoftPointer,
                    value.reserved_reference.value(),
                );
            }
            DataObjectData::BreakPointRef => {}
            DataObjectData::CellStyleMap(value) => {
                self.writer.write_bit_long(value.cells.len() as i32);
                for cell in &value.cells {
                    self.write_named_table_cell_style(cell);
                }
            }
            DataObjectData::AcDsRecord
            | DataObjectData::AcDsSchema
            | DataObjectData::Dummy
            | DataObjectData::LongTransaction
            | DataObjectData::ObjectPointer => {}
            DataObjectData::IdBuffer(value) => {
                self.writer.write_byte(value.flags);
                self.writer.write_bit_long(value.object_ids.len() as i32);
                for handle in &value.object_ids {
                    self.writer
                        .write_handle(DwgReferenceType::SoftPointer, handle.value());
                }
            }
            DataObjectData::Index(value) => {
                self.writer.write_datetime(
                    value.last_updated_julian_day,
                    value.last_updated_milliseconds,
                );
            }
            DataObjectData::LayerIndex(value) => {
                self.writer.write_datetime(
                    value.last_updated_julian_day,
                    value.last_updated_milliseconds,
                );
                self.writer.write_bit_long(value.entries.len() as i32);
                for entry in &value.entries {
                    self.writer.write_bit_long(entry.layer_count);
                    self.writer.write_variable_text(&entry.name);
                    self.writer
                        .write_handle(DwgReferenceType::HardPointer, entry.id_buffer.value());
                }
            }
            DataObjectData::PartialViewingFilter(_) => {}
            DataObjectData::TableGeometry(value) => {
                self.writer.write_bit_long(value.rows);
                self.writer.write_bit_long(value.columns);
                self.writer.write_bit_long(value.cells.len() as i32);
                for cell in &value.cells {
                    self.writer.write_bit_long(cell.geometry_data_flag);
                    self.writer.write_bit_double(cell.width_with_gap);
                    self.writer.write_bit_double(cell.height_with_gap);
                    self.writer
                        .write_handle(DwgReferenceType::SoftPointer, cell.table_geometry.value());
                    self.writer.write_bit_long(cell.geometry.len() as i32);
                    for geometry in &cell.geometry {
                        self.writer.write_3bit_double(geometry.distance_to_top_left);
                        self.writer.write_3bit_double(geometry.distance_to_center);
                        self.writer.write_bit_double(geometry.width);
                        self.writer.write_bit_double(geometry.height);
                        self.writer.write_bit_double(geometry.outer_width);
                        self.writer.write_bit_double(geometry.outer_height);
                        self.writer.write_bit_long(geometry.flags);
                    }
                }
            }
        }
        self.register_object(object.handle);
    }
}
