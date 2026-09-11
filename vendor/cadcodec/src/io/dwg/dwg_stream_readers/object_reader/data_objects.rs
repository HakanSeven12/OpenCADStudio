//! Native DWG readers for database data/index helper objects.

use crate::entities::CellContentGeometry;
use crate::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader;
use crate::objects::{
    BreakData, BreakPointReference, CellStyleMap, DataObjectData, IdBuffer, Index, LayerIndex,
    LayerIndexEntry, PartialViewingFilter, TableGeometry, TableGeometryCell,
};
use crate::types::Handle;

fn count(value: i32, limit: i32) -> usize {
    value.max(0).min(limit) as usize
}

pub fn read_data_object_data(
    reader: &mut DwgMergedReader,
    dxf_name: &str,
) -> Option<DataObjectData> {
    let data = match dxf_name.to_uppercase().as_str() {
        "BREAKDATA" => {
            let version = reader.read_bit_short();
            let mut point_references = Vec::new();
            for _ in 0..count(reader.read_bit_long(), 100_000) {
                point_references.push(BreakPointReference {
                    version: reader.read_bit_short(),
                    reserved: reader.read_bit() as i16,
                    reference_type: reader.read_bit_long(),
                    flags: reader.read_bit_short(),
                    identifier: reader.read_bit_long(),
                    first_point: reader.read_3bit_double(),
                    second_point: reader.read_3bit_double(),
                    trailing_version: reader.read_bit_short(),
                });
            }
            let dimension_reference = Handle::from(reader.read_handle());
            let reserved_reference = Handle::from(reader.read_handle());
            DataObjectData::BreakData(BreakData {
                version,
                dimension_reference,
                reserved_reference,
                point_references,
            })
        }
        "BREAKPOINTREF" => DataObjectData::BreakPointRef,
        "CELLSTYLEMAP" => {
            let mut cells = Vec::new();
            for _ in 0..count(reader.read_bit_long(), 10_000) {
                cells.push(super::objects::read_named_table_cell_style(reader));
            }
            DataObjectData::CellStyleMap(CellStyleMap { cells })
        }
        "ACDSRECORD" => DataObjectData::AcDsRecord,
        "ACDSSCHEMA" => DataObjectData::AcDsSchema,
        "IDBUFFER" => {
            let flags = reader.read_byte();
            let mut object_ids = Vec::new();
            for _ in 0..count(reader.read_bit_long(), 10_000) {
                object_ids.push(Handle::from(reader.read_handle()));
            }
            DataObjectData::IdBuffer(IdBuffer { flags, object_ids })
        }
        "INDEX" => DataObjectData::Index(Index {
            last_updated_julian_day: reader.read_bit_long(),
            last_updated_milliseconds: reader.read_bit_long(),
        }),
        "LAYER_INDEX" => {
            let last_updated_julian_day = reader.read_bit_long();
            let last_updated_milliseconds = reader.read_bit_long();
            let mut entries = Vec::new();
            for _ in 0..count(reader.read_bit_long(), 20_000) {
                entries.push(LayerIndexEntry {
                    layer_count: reader.read_bit_long(),
                    name: reader.read_variable_text(),
                    id_buffer: Handle::from(reader.read_handle()),
                });
            }
            DataObjectData::LayerIndex(LayerIndex {
                last_updated_julian_day,
                last_updated_milliseconds,
                entries,
            })
        }
        "PARTIAL_VIEWING_FILTER" => DataObjectData::PartialViewingFilter(PartialViewingFilter),
        "LONG_TRANSACTION" => DataObjectData::LongTransaction,
        "TABLEGEOMETRY" => {
            let rows = reader.read_bit_long();
            let columns = reader.read_bit_long();
            let mut cells = Vec::new();
            for _ in 0..count(reader.read_bit_long(), 10_000) {
                let geometry_data_flag = reader.read_bit_long();
                let width_with_gap = reader.read_bit_double();
                let height_with_gap = reader.read_bit_double();
                let table_geometry = Handle::from(reader.read_handle());
                let mut geometry = Vec::new();
                for _ in 0..count(reader.read_bit_long(), 10_000) {
                    geometry.push(CellContentGeometry {
                        distance_to_top_left: reader.read_3bit_double(),
                        distance_to_center: reader.read_3bit_double(),
                        width: reader.read_bit_double(),
                        height: reader.read_bit_double(),
                        outer_width: reader.read_bit_double(),
                        outer_height: reader.read_bit_double(),
                        flags: reader.read_bit_long(),
                    });
                }
                cells.push(TableGeometryCell {
                    geometry_data_flag,
                    width_with_gap,
                    height_with_gap,
                    table_geometry,
                    geometry,
                });
            }
            DataObjectData::TableGeometry(TableGeometry {
                rows,
                columns,
                cells,
            })
        }
        _ => return None,
    };
    Some(data)
}
