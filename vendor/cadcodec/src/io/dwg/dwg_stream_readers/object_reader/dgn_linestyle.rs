use crate::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader;
use crate::objects::{
    DgnLineStyleData, DgnLsComponentData, DgnLsComponentType, DgnLsCompoundComponent,
    DgnLsCompoundEntry, DgnLsInternalComponent, DgnLsPhaseMode, DgnLsPointComponent, DgnLsStroke,
    DgnLsStrokePattern, DgnLsSymbolComponent, DgnLsSymbolReference,
};
use crate::types::Handle;

pub fn read_dgn_line_style_data(
    reader: &mut DwgMergedReader,
    dxf_name: &str,
) -> Option<DgnLineStyleData> {
    let description = reader.read_variable_text();
    let version = reader.read_bit_long();
    let component_type = reader.read_bit_long();
    let component_uid = read_uid(reader);

    if dxf_name.eq_ignore_ascii_case("LSDEFINITION") {
        return Some(DgnLineStyleData::Definition {
            description,
            version,
            style_number: component_type,
            component_uid,
            is_continuous: reader.read_bit(),
            unit_definition: reader.read_bit_double(),
            unit_scale: reader.read_bit_double(),
            units_type: reader.read_bit_long(),
            is_element: reader.read_bit(),
            is_physical: reader.read_bit(),
            is_scale_independent: reader.read_bit(),
            is_snappable: reader.read_bit(),
            root_component: Handle::from(reader.read_handle()),
            properties: Vec::new(),
        });
    }

    let kind = DgnLsComponentType::from_code(component_type)?;
    let scale = reader.read_bit_double();
    let property_flags = read_byte(reader);
    let component = match kind {
        DgnLsComponentType::Symbol => DgnLsComponentData::Symbol(DgnLsSymbolComponent {
            stored_unit_scale: reader.read_bit_double(),
            unit_scale: reader.read_bit_double(),
            has_unit_scale: reader.read_bit(),
            is_3d: reader.read_bit(),
            block: Handle::from(reader.read_handle()),
        }),
        DgnLsComponentType::Compound => {
            let count = read_count(reader, 2)?;
            if count as i64 > reader.handle_remaining_bits() / 8 {
                return None;
            }
            let offsets = (0..count)
                .map(|_| reader.read_bit_double())
                .collect::<Vec<_>>();
            let entries = offsets
                .into_iter()
                .map(|offset| DgnLsCompoundEntry {
                    component: Handle::from(reader.read_handle()),
                    offset,
                })
                .collect();
            DgnLsComponentData::Compound(DgnLsCompoundComponent { entries })
        }
        DgnLsComponentType::Stroke => DgnLsComponentData::Stroke(read_stroke_pattern(reader)?),
        DgnLsComponentType::Point => {
            let count = read_count(reader, 20)?;
            if count as i64 + 1 > reader.handle_remaining_bits() / 8 {
                return None;
            }
            let mut symbols = Vec::with_capacity(count as usize);
            for _ in 0..count {
                symbols.push(DgnLsSymbolReference {
                    symbol_component: Handle::NULL,
                    partial_strokes: reader.read_bit(),
                    clip_partial: reader.read_bit(),
                    allow_stretch: reader.read_bit(),
                    partial_projected: reader.read_bit(),
                    use_symbol_color: reader.read_bit(),
                    use_symbol_lineweight: reader.read_bit(),
                    justify: reader.read_bit_long(),
                    rotation_type: reader.read_bit_long(),
                    vertex_mask: reader.read_bit_long(),
                    x_offset: reader.read_bit_double(),
                    y_offset: reader.read_bit_double(),
                    angle: reader.read_bit_double(),
                    stroke_number: reader.read_bit_long(),
                });
            }
            let stroke_component = Handle::from(reader.read_handle());
            for symbol in &mut symbols {
                symbol.symbol_component = Handle::from(reader.read_handle());
            }
            DgnLsComponentData::Point(DgnLsPointComponent {
                stroke_component,
                symbols,
            })
        }
        DgnLsComponentType::Internal => {
            let pattern = read_stroke_pattern(reader)?;
            DgnLsComponentData::Internal(DgnLsInternalComponent {
                pattern,
                internal_version: reader.read_bit_long(),
                hardware_style: reader.read_bit_long(),
                is_hardware_style: reader.read_bit(),
                line_code: reader.read_bit_long(),
            })
        }
    };

    Some(DgnLineStyleData::Component {
        kind,
        description,
        version,
        component_uid,
        scale,
        property_flags,
        component,
        properties: Vec::new(),
    })
}

fn read_uid(reader: &mut DwgMergedReader) -> [u8; 16] {
    let bytes = reader.read_bytes(16);
    let mut uid = [0; 16];
    uid[..bytes.len().min(16)].copy_from_slice(&bytes[..bytes.len().min(16)]);
    uid
}

fn read_byte(reader: &mut DwgMergedReader) -> u8 {
    reader.read_bytes(1).first().copied().unwrap_or(0)
}

// A clamped count can still fabricate 100,000 entries by reading zeros past
// the record boundary. Use the minimum encoded size instead, and let the
// caller preserve an unsupported layout as a raw record.
fn read_count(reader: &mut DwgMergedReader, minimum_bits: i64) -> Option<usize> {
    let count = reader.read_bit_long();
    (count >= 0 && i64::from(count) <= reader.main_remaining_bits() / minimum_bits)
        .then_some(count as usize)
}

fn read_stroke_pattern(reader: &mut DwgMergedReader) -> Option<DgnLsStrokePattern> {
    let has_iteration_limit = reader.read_bit();
    let is_single_segment = reader.read_bit();
    let iteration_limit = reader.read_bit_long();
    let auto_phase = reader.read_bit_double();
    let phase = reader.read_bit_double();
    let phase_mode =
        DgnLsPhaseMode::from_code(((reader.read_bit() as u8) << 1) | reader.read_bit() as u8);
    // Five flags, three BD values and two BL values consume at least 15 bits.
    let count = read_count(reader, 15)?;
    let mut strokes = Vec::with_capacity(count as usize);
    for _ in 0..count {
        strokes.push(DgnLsStroke {
            is_dash: reader.read_bit(),
            bypass_corner: reader.read_bit(),
            can_be_scaled: reader.read_bit(),
            invert_at_origin: reader.read_bit(),
            invert_at_end: reader.read_bit(),
            length: reader.read_bit_double(),
            start_width: reader.read_bit_double(),
            end_width: reader.read_bit_double(),
            width_mode: reader.read_bit_long(),
            cap_mode: reader.read_bit_long(),
        });
    }
    Some(DgnLsStrokePattern {
        has_iteration_limit,
        is_single_segment,
        iteration_limit,
        auto_phase,
        phase,
        phase_mode,
        strokes,
    })
}
