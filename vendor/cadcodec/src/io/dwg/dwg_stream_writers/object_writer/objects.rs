//! Non-graphical object serialization for DWG records.
//!
//! Handles dictionaries, layouts, plot-settings, XRecords, groups,
//! mline styles, image definitions, etc.
//!
//! Each writer:
//! 1. Calls `write_common_non_entity_data()` (type + handle + reactors)
//! 2. Writes type-specific fields
//! 3. Calls `register_object()` (CRC, output, handle map)
//!
//! Ported from the reference `DwgObjectWriter` objects module.

use crate::io::dwg::dwg_reference_type::DwgReferenceType;
use crate::objects::*;
use crate::types::{DxfVersion, Handle};

use super::common;
use super::DwgObjectWriter;

/// Value type of an XRECORD/xdata resbuf item, keyed by its group code.
/// Mirrors libredwg `dwg_resbuf_value_type`. Only `Str` is version-specific
/// (code page pre-R2007, UTF-16 since); every other type is a fixed byte run.
#[derive(PartialEq)]
enum XdVt {
    Str,
    Real,
    Int16,
    Int32,
    Int64,
    Int8,
    Point3d,
    Binary,
    Handle,
    Invalid,
}

fn xdata_value_type(gc: i16) -> XdVt {
    use XdVt::*;
    match gc {
        g if g < 0 => Handle,
        0..=4 => Str,
        5 => Handle,
        6..=9 => Str,
        10..=37 => Point3d,
        38..=59 => Real,
        60..=79 => Int16,
        80..=99 => Int32,
        100..=102 => Str,
        105 => Handle,
        110..=139 => Point3d,
        140..=149 => Real,
        150..=169 => Int64,
        170..=179 => Int16,
        210..=269 => Point3d,
        270..=279 => Int16,
        280..=289 => Int8,
        290..=299 => Int8, // bool, one byte
        300..=309 => Str,
        310..=319 => Binary,
        320..=329 => Handle,
        330..=369 => Handle,
        370..=389 => Int16,
        390..=399 => Handle,
        400..=409 => Int16,
        410..=419 => Str,
        420..=429 => Int32,
        430..=439 => Str,
        440..=459 => Int32,
        460..=469 => Real,
        470..=479 => Str,
        480..=481 => Handle,
        999 => Str,
        1004 => Binary,
        1000..=1003 => Str,
        1005 => Handle,
        1010..=1039 => Point3d,
        1040..=1042 => Real,
        1043..=1069 => Point3d,
        1070 => Int16,
        1071 => Int32,
        _ => Invalid,
    }
}

fn xrecord_reference_type(kind: ProxyReferenceKind) -> DwgReferenceType {
    match kind {
        ProxyReferenceKind::Undefined => DwgReferenceType::Undefined,
        ProxyReferenceKind::SoftOwnership => DwgReferenceType::SoftOwnership,
        ProxyReferenceKind::HardOwnership => DwgReferenceType::HardOwnership,
        ProxyReferenceKind::SoftPointer => DwgReferenceType::SoftPointer,
        ProxyReferenceKind::HardPointer => DwgReferenceType::HardPointer,
    }
}

/// Re-encode an XRECORD's xdata byte blob between the code-page (pre-R2007) and
/// UTF-16 (R2007+) string encodings, copying every non-string item verbatim.
///
/// XRECORD framing is already written version-correctly by the normal object
/// path; only the inline strings inside the xdata are version-specific. Without
/// this a cross-version save would emit the source version's strings and the
/// reader would mis-parse them ("Invalid xdata type"). Items are byte-aligned
/// (every value is a byte multiple). Each legacy string's embedded code-page
/// index is honored.
fn transcode_xrecord_xdata(
    raw: &[u8],
    src_unicode: bool,
    tgt_unicode: bool,
    target_encoding: &'static encoding_rs::Encoding,
    target_code_page: u8,
) -> Vec<u8> {
    if src_unicode == tgt_unicode {
        return raw.to_vec();
    }
    let rd_u16 = |b: &[u8], i: usize| (b[i] as u16) | ((b[i + 1] as u16) << 8);
    let mut out = Vec::with_capacity(raw.len() + raw.len() / 2);
    let mut p = 0usize;
    while p + 2 <= raw.len() {
        let item_start = p;
        let output_start = out.len();
        macro_rules! preserve_tail {
            () => {{
                out.truncate(output_start);
                out.extend_from_slice(&raw[item_start..]);
                return out;
            }};
        }
        let gc = rd_u16(raw, p) as i16;
        let vt = xdata_value_type(gc);
        if vt == XdVt::Invalid {
            preserve_tail!();
        }
        out.extend_from_slice(&raw[p..p + 2]); // group code
        p += 2;
        // Fixed-size values: copy the exact byte run verbatim.
        let fixed = match vt {
            XdVt::Real | XdVt::Int64 | XdVt::Handle => Some(8usize),
            XdVt::Point3d => Some(24),
            XdVt::Int32 => Some(4),
            XdVt::Int16 => Some(2),
            XdVt::Int8 => Some(1),
            _ => None,
        };
        if let Some(n) = fixed {
            if p + n > raw.len() {
                preserve_tail!();
            }
            out.extend_from_slice(&raw[p..p + n]);
            p += n;
            continue;
        }
        if vt == XdVt::Binary {
            if p >= raw.len() {
                preserve_tail!();
            }
            let size = raw[p] as usize;
            if p + 1 + size > raw.len() {
                preserve_tail!();
            }
            out.extend_from_slice(&raw[p..p + 1 + size]);
            p += 1 + size;
            continue;
        }
        // Str: decode source format, re-encode target format.
        if p + 2 > raw.len() {
            preserve_tail!();
        }
        let len = rd_u16(raw, p) as usize;
        p += 2;
        let text: String = if src_unicode {
            if p + len * 2 > raw.len() {
                preserve_tail!();
            }
            let units: Vec<u16> = (0..len).map(|i| rd_u16(raw, p + i * 2)).collect();
            p += len * 2;
            String::from_utf16_lossy(&units)
        } else {
            // [u8 codepage][len bytes]
            if p + 1 + len > raw.len() {
                preserve_tail!();
            }
            let source_code_page = raw[p] as u16;
            p += 1;
            let s = crate::io::dxf::code_page::encoding_from_dwg_code_page(source_code_page)
                .decode(&raw[p..p + len])
                .0
                .into_owned();
            p += len;
            crate::io::dxf::code_page::decode_mif_escapes(&s)
        };
        if tgt_unicode {
            let utf16: Vec<u16> = text.encode_utf16().take(u16::MAX as usize).collect();
            out.extend_from_slice(&(utf16.len() as u16).to_le_bytes());
            for u in utf16 {
                out.extend_from_slice(&u.to_le_bytes());
            }
        } else {
            let encoded = crate::io::dxf::code_page::encode_legacy_string(&text, target_encoding);
            let bytes = &encoded[..encoded.len().min(u16::MAX as usize)];
            out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
            out.push(target_code_page);
            out.extend_from_slice(&bytes);
        }
    }
    out.extend_from_slice(&raw[p..]);
    out
}

fn encode_xrecord_entries(
    entries: &[XRecordEntry],
    unicode: bool,
    encoding: &'static encoding_rs::Encoding,
    code_page: u8,
) -> Vec<u8> {
    let mut output = Vec::new();
    for entry in entries {
        output.extend_from_slice(&(entry.code as i16 as u16).to_le_bytes());
        match &entry.value {
            XRecordValue::String(value) => {
                if unicode {
                    let units: Vec<u16> = value.encode_utf16().collect();
                    output.extend_from_slice(
                        &(units.len().min(u16::MAX as usize) as u16).to_le_bytes(),
                    );
                    for unit in units.iter().take(u16::MAX as usize) {
                        output.extend_from_slice(&unit.to_le_bytes());
                    }
                } else {
                    let encoded = crate::io::dxf::code_page::encode_legacy_string(value, encoding);
                    let bytes: &[u8] = encoded.as_ref();
                    output.extend_from_slice(
                        &(bytes.len().min(u16::MAX as usize) as u16).to_le_bytes(),
                    );
                    output.push(code_page);
                    output.extend_from_slice(&bytes[..bytes.len().min(u16::MAX as usize)]);
                }
            }
            XRecordValue::Double(value) => {
                output.extend_from_slice(&value.to_le_bytes());
            }
            XRecordValue::Int16(value) => {
                output.extend_from_slice(&value.to_le_bytes());
            }
            XRecordValue::Int32(value) => {
                output.extend_from_slice(&value.to_le_bytes());
            }
            XRecordValue::Int64(value) => {
                output.extend_from_slice(&value.to_le_bytes());
            }
            XRecordValue::Byte(value) => output.push(*value),
            XRecordValue::Bool(value) => output.push(*value as u8),
            XRecordValue::Handle(value) => {
                output.extend_from_slice(&value.value().to_le_bytes());
            }
            XRecordValue::Point3D(x, y, z) => {
                output.extend_from_slice(&x.to_le_bytes());
                output.extend_from_slice(&y.to_le_bytes());
                output.extend_from_slice(&z.to_le_bytes());
            }
            XRecordValue::Chunk(value) => {
                let length = value.len().min(u8::MAX as usize);
                output.push(length as u8);
                output.extend_from_slice(&value[..length]);
            }
        }
    }
    output
}

/// Flatten a [`Matrix4`](crate::types::Matrix4) into 12 doubles holding its 3×4
/// part in row-major order (3 rows of 4); the bottom row is dropped. DWG stores
/// the spatial-filter transforms row-major.
fn matrix_to_row_major(m: &crate::types::Matrix4) -> [f64; 12] {
    let mut out = [0.0; 12];
    let mut i = 0;
    for row in 0..3 {
        for col in 0..4 {
            out[i] = m.m[row][col];
            i += 1;
        }
    }
    out
}

impl<'a> DwgObjectWriter<'a> {
    // ── Object dispatch ─────────────────────────────────────────────

    /// Write a single non-graphical object record.
    pub(super) fn write_object(&mut self, obj: &ObjectType) {
        match obj {
            ObjectType::Dictionary(d) => self.write_dictionary(d),
            ObjectType::Layout(l) => self.write_layout(l),
            ObjectType::XRecord(x) => {
                if !x.entries_complete {
                    if let Some(raw) = &x.raw_dwg_data {
                        if self.raw_passthrough_compatible(x.raw_dwg_version) {
                            for reference in &x.object_references {
                                if self.document.objects.contains_key(&reference.handle) {
                                    self.object_queue.push_back(reference.handle);
                                }
                            }
                            if let Some(xdictionary) = x.xdictionary_handle {
                                if self.document.objects.contains_key(&xdictionary) {
                                    self.object_queue.push_back(xdictionary);
                                }
                            }
                            self.register_raw_object(x.handle, raw, x.raw_dwg_handle_bits);
                            return;
                        }
                    }
                }
                self.write_xrecord(x)
            }
            ObjectType::Group(g) => self.write_group(g),
            ObjectType::MLineStyle(m) => self.write_mlinestyle(m),
            ObjectType::MultiLeaderStyle(m) => self.write_multileader_style(m),
            ObjectType::ImageDefinition(d) => self.write_image_definition(d),
            ObjectType::UnderlayDefinition(d) => self.write_underlay_definition(d),
            ObjectType::ImageDefinitionReactor(r) => self.write_image_definition_reactor(r),
            ObjectType::PlotSettings(p) => self.write_plot_settings_obj(p),
            ObjectType::Scale(s) => self.write_scale(s),
            ObjectType::ObjectContextData(c) => self.write_object_context_data(c),
            ObjectType::SortEntitiesTable(s) => self.write_sort_entities_table(s),
            ObjectType::DictionaryVariable(d) => self.write_dictionary_variable(d),
            ObjectType::RasterVariables(r) => self.write_raster_variables(r),
            ObjectType::DictionaryWithDefault(d) => self.write_dictionary_with_default(d),
            ObjectType::PlaceHolder(p) => self.write_placeholder(p),
            ObjectType::BookColor(b) => self.write_book_color(b),
            ObjectType::WipeoutVariables(w) => self.write_wipeout_variables(w),
            ObjectType::SpatialFilter(s) => self.write_spatial_filter(s),
            ObjectType::GeoData(g) => self.write_geodata(g),
            ObjectType::BlockVisibilityParameter(p) => self.write_block_visibility_parameter(p),
            ObjectType::DynamicBlock(value) => self.write_dynamic_block(value),
            ObjectType::Associative(value) => self.write_associative_object(value),
            ObjectType::ClassObject(value) => {
                if let ClassObjectData::CsacDocumentOptions(data) = &value.data {
                    if let Some(raw) = &data.raw_dwg_data {
                        if self.raw_passthrough_compatible(data.raw_dwg_version) {
                            self.register_raw_object(value.handle, raw, data.raw_dwg_handle_bits);
                            return;
                        }
                    }
                }
                self.write_class_object(value)
            }
            ObjectType::DataObject(value) => self.write_data_object(value),
            ObjectType::Field(value) => self.write_field_object(value),
            ObjectType::FieldList(value) => self.write_field_list(value),
            ObjectType::RegisteredClass(value) => {
                if value.properties.is_empty() {
                    if let Some(raw) = &value.raw_dwg_data {
                        if self.raw_passthrough_compatible(value.raw_dwg_version) {
                            self.register_raw_object(value.handle, raw, value.raw_dwg_handle_bits);
                            return;
                        }
                    }
                }
                self.write_registered_class_object(value)
            }
            ObjectType::DgnLineStyle(value) => self.write_dgn_line_style_object(value),
            ObjectType::ProxyObject(value) => self.write_proxy_object(value),
            ObjectType::VisualStyle(v) => self.write_visual_style(v),
            ObjectType::Material(m) => self.write_material(m),
            ObjectType::TableContent(t) => self.write_table_content_object(t),
            ObjectType::TableStyle(t) => self.write_table_style(t),
            ObjectType::Unknown {
                handle,
                raw_dwg_data,
                raw_dwg_handle_bits,
                raw_dwg_version,
                ..
            } => {
                if let Some(ref raw) = raw_dwg_data {
                    if self.raw_passthrough_compatible(*raw_dwg_version) {
                        self.register_raw_object(*handle, raw, *raw_dwg_handle_bits);
                    }
                }
            }
        }
    }

    fn write_registered_class_object(&mut self, value: &RegisteredClassObject) {
        if !value.properties.is_empty() {
            let payload = crate::objects::semantic_property::encode_registered_class_envelope(
                &value.dxf_name,
                &value.cpp_class_name,
                &value.properties,
                &value.payload,
            );
            self.write_common_non_entity_data(
                common::OBJ_PROXY_OBJECT,
                value.handle,
                value.owner,
                &value.reactors,
                &value.xdictionary_handle,
            );
            self.writer.write_bit_long(499);
            if self.dxf_version > crate::types::DxfVersion::AC1015 {
                self.writer.write_variable_text(&value.dxf_name);
            }
            if self.version.r2018_plus(self.dxf_version) {
                self.writer.write_bit_long(0);
                self.writer.write_bit_long(0);
            } else {
                self.writer.write_bit_long(0);
            }
            if self.version.r2000_plus() {
                self.writer.write_bit(true);
            }
            self.write_registered_payload(&payload, &value.object_ids);
            self.register_object(value.handle);
            return;
        }
        let type_code = self.class_type_code(&value.dxf_name, 0);
        self.write_common_non_entity_data(
            type_code,
            value.handle,
            value.owner,
            &value.reactors,
            &value.xdictionary_handle,
        );
        self.write_registered_payload(&value.payload, &value.object_ids);
        self.register_object(value.handle);
    }

    fn write_dgn_line_style_object(&mut self, value: &DgnLineStyleObject) {
        let type_code = self.class_type_code(value.dxf_name(), 0);
        self.write_common_non_entity_data(
            type_code,
            value.handle,
            value.owner,
            &value.reactors,
            &value.xdictionary_handle,
        );
        match &value.data {
            DgnLineStyleData::Definition {
                description,
                version,
                style_number,
                component_uid,
                is_continuous,
                unit_definition,
                unit_scale,
                units_type,
                is_element,
                is_physical,
                is_scale_independent,
                is_snappable,
                root_component,
                ..
            } => {
                self.writer.write_variable_text(description);
                self.writer.write_bit_long(*version);
                self.writer.write_bit_long(*style_number);
                self.writer.write_bytes(component_uid);
                self.writer.write_bit(*is_continuous);
                self.writer.write_bit_double(*unit_definition);
                self.writer.write_bit_double(*unit_scale);
                self.writer.write_bit_long(*units_type);
                self.writer.write_bit(*is_element);
                self.writer.write_bit(*is_physical);
                self.writer.write_bit(*is_scale_independent);
                self.writer.write_bit(*is_snappable);
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, root_component.value());
            }
            DgnLineStyleData::Component {
                kind,
                description,
                version,
                component_uid,
                scale,
                property_flags,
                component,
                ..
            } => {
                self.writer.write_variable_text(description);
                self.writer.write_bit_long(*version);
                self.writer.write_bit_long(kind.code());
                self.writer.write_bytes(component_uid);
                self.writer.write_bit_double(*scale);
                self.writer.write_bytes(&[*property_flags]);
                self.write_dgn_line_style_component(component);
            }
            DgnLineStyleData::Registered {
                payload,
                object_ids,
                ..
            } => self.write_registered_payload(payload, object_ids),
        }
        self.register_object(value.handle);
    }

    fn write_dgn_line_style_component(&mut self, component: &DgnLsComponentData) {
        match component {
            DgnLsComponentData::Symbol(value) => {
                self.writer.write_bit_double(value.stored_unit_scale);
                self.writer.write_bit_double(value.unit_scale);
                self.writer.write_bit(value.has_unit_scale);
                self.writer.write_bit(value.is_3d);
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, value.block.value());
            }
            DgnLsComponentData::Compound(value) => {
                self.writer.write_bit_long(value.entries.len() as i32);
                for entry in &value.entries {
                    self.writer.write_bit_double(entry.offset);
                }
                for entry in &value.entries {
                    self.writer
                        .write_handle(DwgReferenceType::HardPointer, entry.component.value());
                }
            }
            DgnLsComponentData::Stroke(value) => {
                self.write_dgn_stroke_pattern(value);
            }
            DgnLsComponentData::Point(value) => {
                self.writer.write_bit_long(value.symbols.len() as i32);
                for symbol in &value.symbols {
                    self.writer.write_bit(symbol.partial_strokes);
                    self.writer.write_bit(symbol.clip_partial);
                    self.writer.write_bit(symbol.allow_stretch);
                    self.writer.write_bit(symbol.partial_projected);
                    self.writer.write_bit(symbol.use_symbol_color);
                    self.writer.write_bit(symbol.use_symbol_lineweight);
                    self.writer.write_bit_long(symbol.justify);
                    self.writer.write_bit_long(symbol.rotation_type);
                    self.writer.write_bit_long(symbol.vertex_mask);
                    self.writer.write_bit_double(symbol.x_offset);
                    self.writer.write_bit_double(symbol.y_offset);
                    self.writer.write_bit_double(symbol.angle);
                    self.writer.write_bit_long(symbol.stroke_number);
                }
                self.writer.write_handle(
                    DwgReferenceType::HardPointer,
                    value.stroke_component.value(),
                );
                for symbol in &value.symbols {
                    self.writer.write_handle(
                        DwgReferenceType::HardPointer,
                        symbol.symbol_component.value(),
                    );
                }
            }
            DgnLsComponentData::Internal(value) => {
                self.write_dgn_stroke_pattern(&value.pattern);
                self.writer.write_bit_long(value.internal_version);
                self.writer.write_bit_long(value.hardware_style);
                self.writer.write_bit(value.is_hardware_style);
                self.writer.write_bit_long(value.line_code);
            }
        }
    }

    fn write_dgn_stroke_pattern(&mut self, pattern: &DgnLsStrokePattern) {
        self.writer.write_bit(pattern.has_iteration_limit);
        self.writer.write_bit(pattern.is_single_segment);
        self.writer.write_bit_long(pattern.iteration_limit);
        self.writer.write_bit_double(pattern.auto_phase);
        self.writer.write_bit_double(pattern.phase);
        let phase_mode = pattern.phase_mode.code();
        self.writer.write_bit((phase_mode & 2) != 0);
        self.writer.write_bit((phase_mode & 1) != 0);
        self.writer.write_bit_long(pattern.strokes.len() as i32);
        for stroke in &pattern.strokes {
            self.writer.write_bit(stroke.is_dash);
            self.writer.write_bit(stroke.bypass_corner);
            self.writer.write_bit(stroke.can_be_scaled);
            self.writer.write_bit(stroke.invert_at_origin);
            self.writer.write_bit(stroke.invert_at_end);
            self.writer.write_bit_double(stroke.length);
            self.writer.write_bit_double(stroke.start_width);
            self.writer.write_bit_double(stroke.end_width);
            self.writer.write_bit_long(stroke.width_mode);
            self.writer.write_bit_long(stroke.cap_mode);
        }
    }

    pub(super) fn write_registered_payload(
        &mut self,
        payload: &crate::objects::ProxyPayload,
        object_ids: &[crate::objects::ProxyObjectReference],
    ) {
        let data = payload.data();
        for bit_index in 0..payload.bit_count as usize {
            let byte = data.get(bit_index / 8).copied().unwrap_or(0);
            self.writer
                .write_bit((byte & (0x80 >> (bit_index % 8))) != 0);
        }
        for object_id in object_ids {
            let reference_type = match object_id.kind {
                crate::objects::ProxyReferenceKind::Undefined => DwgReferenceType::Undefined,
                crate::objects::ProxyReferenceKind::SoftOwnership => {
                    DwgReferenceType::SoftOwnership
                }
                crate::objects::ProxyReferenceKind::HardOwnership => {
                    DwgReferenceType::HardOwnership
                }
                crate::objects::ProxyReferenceKind::SoftPointer => DwgReferenceType::SoftPointer,
                crate::objects::ProxyReferenceKind::HardPointer => DwgReferenceType::HardPointer,
            };
            self.writer
                .write_handle(reference_type, object_id.handle.value());
        }
    }

    fn write_proxy_object(&mut self, value: &ProxyObject) {
        self.write_common_non_entity_data(
            common::OBJ_PROXY_OBJECT,
            value.handle,
            value.owner,
            &value.reactors,
            &value.xdictionary_handle,
        );
        self.writer.write_bit_long(value.class_id);
        if self.dxf_version > crate::types::DxfVersion::AC1015 {
            let dxf_subclass = if value.dxf_subclass.is_empty() {
                self.document
                    .classes
                    .iter()
                    .find(|class| i32::from(class.class_number) == value.class_id)
                    .map(|class| class.dxf_name.as_str())
                    .unwrap_or("")
            } else {
                &value.dxf_subclass
            };
            self.writer.write_variable_text(dxf_subclass);
        }
        if self.version.r2018_plus(self.dxf_version) {
            self.writer.write_bit_long(value.dwg_version);
            self.writer.write_bit_long(value.maintenance_version);
        } else {
            self.writer
                .write_bit_long((value.maintenance_version << 16) | (value.dwg_version & 0xffff));
        }
        if self.version.r2000_plus() {
            self.writer.write_bit(value.from_dxf);
        }
        let payload = value.payload.data();
        for bit_index in 0..value.payload.bit_count as usize {
            let byte = payload.get(bit_index / 8).copied().unwrap_or(0);
            self.writer
                .write_bit((byte & (0x80 >> (bit_index % 8))) != 0);
        }
        let text_payload = value.text_payload.data();
        for bit_index in 0..value.text_payload.bit_count as usize {
            let byte = text_payload.get(bit_index / 8).copied().unwrap_or(0);
            self.writer
                .write_text_bit((byte & (0x80 >> (bit_index % 8))) != 0);
        }
        for object_id in &value.object_ids {
            let reference_type = match object_id.kind {
                crate::objects::ProxyReferenceKind::Undefined => DwgReferenceType::Undefined,
                crate::objects::ProxyReferenceKind::SoftOwnership => {
                    DwgReferenceType::SoftOwnership
                }
                crate::objects::ProxyReferenceKind::HardOwnership => {
                    DwgReferenceType::HardOwnership
                }
                crate::objects::ProxyReferenceKind::SoftPointer => DwgReferenceType::SoftPointer,
                crate::objects::ProxyReferenceKind::HardPointer => DwgReferenceType::HardPointer,
            };
            self.writer
                .write_handle(reference_type, object_id.handle.value());
        }
        self.register_object(value.handle);
    }

    fn write_block_visibility_parameter(&mut self, value: &BlockVisibilityParameter) {
        let type_code = self.class_type_code(
            "BLOCKVISIBILITYPARAMETER",
            common::OBJ_BLOCKVISIBILITYPARAMETER,
        );
        self.write_common_non_entity_data(type_code, value.handle, value.owner, &[], &None);
        self.writer.write_bit_long(value.eval_parent_id);
        self.writer.write_bit_long(value.eval_major);
        self.writer.write_bit_long(value.eval_minor);
        self.writer.write_bit_short(value.eval_value_code);
        match &value.eval_value {
            BlockEvalValue::Real(v) => self.writer.write_bit_double(*v),
            BlockEvalValue::Point(v) => self
                .writer
                .write_2raw_double(crate::types::Vector2::new(v[0], v[1])),
            BlockEvalValue::Text(v) => self.writer.write_variable_text(v),
            BlockEvalValue::Long(v) => self.writer.write_bit_long(*v),
            BlockEvalValue::Handle(v) => self
                .writer
                .write_handle(DwgReferenceType::HardPointer, v.value()),
            BlockEvalValue::Short(v) => self.writer.write_bit_short(*v),
            BlockEvalValue::None => {}
        }
        self.writer.write_bit_long(value.eval_node_id);
        self.writer.write_variable_text(&value.element_name);
        self.writer.write_bit_long(value.element_major);
        self.writer.write_bit_long(value.element_minor);
        self.writer.write_bit_long(value.element_eed_1071);
        self.writer.write_bit(value.show_properties);
        self.writer.write_bit(value.chain_actions);
        self.writer.write_3bit_double(value.def_point);
        for property in &value.property_info {
            self.writer
                .write_bit_long(property.connections.len() as i32);
            for connection in &property.connections {
                self.writer.write_bit_long(connection.code);
                self.writer.write_variable_text(&connection.name);
            }
        }
        self.writer.write_bit_long(value.property_info_count);
        self.writer.write_bit(value.is_initialized);
        self.writer.write_variable_text(&value.name);
        self.writer.write_variable_text(&value.description);
        self.writer.write_bit(value.unknown_bool);
        self.writer.write_bit_long(value.all_blocks.len() as i32);
        for handle in &value.all_blocks {
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, handle.value());
        }
        self.writer.write_bit_long(value.states.len() as i32);
        for state in &value.states {
            self.writer.write_variable_text(&state.name);
            self.writer
                .write_bit_long(state.visible_blocks.len() as i32);
            for handle in &state.visible_blocks {
                self.writer
                    .write_handle(DwgReferenceType::SoftPointer, handle.value());
            }
            self.writer
                .write_bit_long(state.visible_params.len() as i32);
            for handle in &state.visible_params {
                self.writer
                    .write_handle(DwgReferenceType::SoftPointer, handle.value());
            }
        }
        self.register_object(value.handle);
    }

    fn write_visual_style(&mut self, value: &VisualStyle) {
        let Some(type_code) = self
            .document
            .classes
            .get_by_name("VISUALSTYLE")
            .map(|class| class.class_number)
        else {
            return;
        };
        self.write_common_non_entity_data(
            type_code,
            value.handle,
            value.owner,
            &value.reactors,
            &value.xdictionary_handle,
        );
        self.writer.write_variable_text(&value.description);
        self.writer.write_bit_long(value.style_type as i32);

        if !self.version.r2010_plus() {
            self.writer.write_bit_long(value.face_lighting_model as i32);
            self.writer
                .write_bit_long(value.face_lighting_quality as i32);
            self.writer.write_bit_long(value.face_color_mode as i32);
            let properties = value.legacy_properties();
            self.writer
                .write_bit_double(Self::visual_style_double(&properties[0]));
            self.writer
                .write_bit_double(Self::visual_style_double(&properties[1]));
            self.writer
                .write_cm_color(&Self::visual_style_color(&properties[2]));
            self.writer.write_bit_long(value.face_modifier);
            self.writer.write_bit_long(value.edge_model);
            self.writer.write_bit_long(value.edge_style);
            self.writer
                .write_cm_color(&Self::visual_style_color(&properties[3]));
            self.writer
                .write_cm_color(&Self::visual_style_color(&properties[4]));
            self.writer
                .write_bit_long(Self::visual_style_long(&properties[5]));
            self.writer
                .write_bit_double(Self::visual_style_double(&properties[6]));
            self.writer
                .write_bit_long(Self::visual_style_long(&properties[7]));
            self.writer
                .write_cm_color(&Self::visual_style_color(&properties[8]));
            self.writer
                .write_bit_double(Self::visual_style_double(&properties[9]));
            self.writer
                .write_bit_short(Self::visual_style_long(&properties[10]) as i16);
            self.writer
                .write_bit_short(Self::visual_style_long(&properties[11]) as i16);
            self.writer
                .write_bit_long(Self::visual_style_long(&properties[12]));
            self.writer
                .write_cm_color(&Self::visual_style_color(&properties[13]));
            self.writer
                .write_bit_short(Self::visual_style_long(&properties[14]) as i16);
            self.writer
                .write_byte(Self::visual_style_long(&properties[15]) as u8);
            self.writer
                .write_bit_short(Self::visual_style_long(&properties[16]) as i16);
            self.writer
                .write_bit(Self::visual_style_bool(&properties[17]));
            self.writer
                .write_bit_short(Self::visual_style_long(&properties[18]) as i16);
            self.writer
                .write_bit_short(Self::visual_style_long(&properties[19]) as i16);
            self.writer
                .write_bit_long(Self::visual_style_long(&properties[20]));
            self.writer
                .write_bit_long(Self::visual_style_long(&properties[21]));
            self.writer
                .write_bit_long(Self::visual_style_long(&properties[22]));
            self.writer
                .write_bit_double(Self::visual_style_double(&properties[23]));
            self.writer.write_bit(value.internal_use_only);
        } else {
            self.writer.write_bit_short(value.extended_lighting_model);
            self.writer.write_bit(value.internal_use_only);
            for property in value.core_properties() {
                self.write_visual_style_property(&property);
            }
            if self.version.r2013_plus(self.dxf_version) {
                for property in value.extended_properties() {
                    self.write_visual_style_property(&property);
                }
            }
        }

        self.register_object(value.handle);
    }

    fn visual_style_long(property: &VisualStyleProperty) -> i32 {
        match &property.value {
            VisualStylePropertyValue::Short(value) => *value as i32,
            VisualStylePropertyValue::Long(value) => *value,
            VisualStylePropertyValue::Double(value) => *value as i32,
            VisualStylePropertyValue::Bool(value) => *value as i32,
            _ => 0,
        }
    }

    fn visual_style_double(property: &VisualStyleProperty) -> f64 {
        match &property.value {
            VisualStylePropertyValue::Short(value) => *value as f64,
            VisualStylePropertyValue::Long(value) => *value as f64,
            VisualStylePropertyValue::Double(value) => *value,
            VisualStylePropertyValue::Bool(value) => *value as u8 as f64,
            _ => 0.0,
        }
    }

    fn visual_style_bool(property: &VisualStyleProperty) -> bool {
        match &property.value {
            VisualStylePropertyValue::Short(value) => *value != 0,
            VisualStylePropertyValue::Long(value) => *value != 0,
            VisualStylePropertyValue::Double(value) => *value != 0.0,
            VisualStylePropertyValue::Bool(value) => *value,
            _ => false,
        }
    }

    fn visual_style_color(property: &VisualStyleProperty) -> crate::types::Color {
        match &property.value {
            VisualStylePropertyValue::Color(value) => *value,
            _ => crate::types::Color::ByLayer,
        }
    }

    fn write_visual_style_value(&mut self, value: &VisualStylePropertyValue) {
        match value {
            VisualStylePropertyValue::Short(value) => {
                self.writer.write_bit_short(*value);
            }
            VisualStylePropertyValue::Long(value) => {
                self.writer.write_bit_long(*value);
            }
            VisualStylePropertyValue::Double(value) => {
                self.writer.write_bit_double(*value);
            }
            VisualStylePropertyValue::Bool(value) => {
                self.writer.write_bit(*value);
            }
            VisualStylePropertyValue::Color(value) => {
                self.writer.write_cm_color(value);
            }
            VisualStylePropertyValue::Text(value) => {
                self.writer.write_variable_text(value);
            }
        }
    }

    fn write_visual_style_property(&mut self, property: &VisualStyleProperty) {
        self.write_visual_style_value(&property.value);
        self.writer.write_bit_short(property.enabled);
    }

    fn write_material(&mut self, value: &Material) {
        let Some(type_code) = self
            .document
            .classes
            .get_by_name("MATERIAL")
            .map(|class| class.class_number)
        else {
            return;
        };
        self.write_common_non_entity_data(
            type_code,
            value.handle,
            value.owner,
            &value.reactors,
            &value.xdictionary_handle,
        );
        self.writer.write_variable_text(&value.name);
        self.writer.write_variable_text(&value.description);
        self.write_material_color(&value.ambient_color);
        self.write_material_color(&value.diffuse_color);
        self.write_material_map(&value.diffuse_map);
        self.write_material_color(&value.specular_color);
        self.write_material_map(&value.specular_map);
        self.writer.write_bit_double(value.specular_gloss_factor);
        self.write_material_map(&value.reflection_map);
        self.writer.write_bit_double(value.opacity_percent);
        self.write_material_map(&value.opacity_map);
        self.write_material_map(&value.bump_map);
        self.writer.write_bit_double(value.refraction_index);
        self.write_material_map(&value.refraction_map);
        if self.version.r2007_plus() {
            self.writer.write_bit_double(value.translucence);
            self.writer.write_bit_double(value.self_illumination);
            self.writer.write_bit_double(value.reflectivity);
            self.writer.write_bit_long(value.illumination_model);
            self.writer.write_bit_long(value.channel_flags);
            self.writer.write_bit_long(value.mode);
        }
        self.register_object(value.handle);
    }

    fn write_material_color(&mut self, value: &MaterialColor) {
        self.writer.write_byte(value.flag);
        self.writer.write_bit_double(value.factor);
        if value.flag == 1 {
            self.writer.write_bit_long(value.rgb.unwrap_or_default());
        }
    }

    fn write_material_texture(&mut self, value: &MaterialTexture, depth: usize) {
        self.writer.write_bit_short(value.mode);
        if value.mode == 0 || value.mode == 1 {
            self.write_material_color(&value.color1);
            self.write_material_color(&value.color2);
        } else if value.mode == 2 {
            match value.procedural.as_ref() {
                Some(MaterialProceduralValue::Bool(item)) => {
                    self.writer.write_bit_short(1);
                    self.writer.write_bit(*item);
                }
                Some(MaterialProceduralValue::Integer(item)) => {
                    self.writer.write_bit_short(2);
                    self.writer.write_bit_short(*item);
                }
                Some(MaterialProceduralValue::Real(item)) => {
                    self.writer.write_bit_short(3);
                    self.writer.write_bit_double(*item);
                }
                Some(MaterialProceduralValue::Color(item)) => {
                    self.writer.write_bit_short(4);
                    self.writer.write_cm_color(item);
                }
                Some(MaterialProceduralValue::Text(item)) => {
                    self.writer.write_bit_short(5);
                    self.writer.write_variable_text(item);
                }
                Some(MaterialProceduralValue::Table(items)) if depth < 8 => {
                    self.writer.write_bit_short(6);
                    self.writer
                        .write_bit_short(items.len().min(i16::MAX as usize) as i16);
                    for (name, texture) in items.iter().take(i16::MAX as usize) {
                        self.writer.write_variable_text(name);
                        self.write_material_texture(texture, depth + 1);
                    }
                    self.writer.write_bit(value.table_end);
                }
                _ => {
                    self.writer.write_bit_short(0);
                }
            }
        }
    }

    fn write_material_map(&mut self, value: &MaterialMap) {
        self.writer.write_bit_double(value.blend_factor);
        self.writer.write_byte(value.projection);
        self.writer.write_byte(value.tiling);
        self.writer.write_byte(value.auto_transform);
        for item in value.transform {
            self.writer.write_bit_double(item);
        }
        self.writer.write_byte(value.source);
        if value.source == 1 {
            self.writer.write_variable_text(&value.file_name);
        } else if value.source == 2 {
            if let Some(texture) = &value.texture {
                self.write_material_texture(texture, 0);
            } else {
                self.write_material_texture(&MaterialTexture::default(), 0);
            }
        }
    }

    fn write_table_content_object(&mut self, value: &crate::entities::Table) {
        let type_code = self.class_type_code("TABLECONTENT", common::OBJ_TABLECONTENT);
        self.write_common_non_entity_data(
            type_code,
            value.common.handle,
            value.common.owner_handle,
            &value.common.reactors,
            &value.common.xdictionary_handle,
        );
        self.write_table_content(value);
        self.register_object(value.common.handle);
    }

    fn write_table_style(&mut self, value: &TableStyle) {
        let Some(type_code) = self
            .document
            .classes
            .get_by_name("TABLESTYLE")
            .map(|class| class.class_number)
        else {
            return;
        };
        self.write_common_non_entity_data(type_code, value.handle, value.owner_handle, &[], &None);

        if !self.version.r2010_plus() {
            self.writer.write_variable_text(&value.name);
            self.writer.write_bit_short(value.flow_direction as i16);
            self.writer.write_bit_short(value.flags.bits());
            self.writer.write_bit_double(value.horizontal_margin);
            self.writer.write_bit_double(value.vertical_margin);
            self.writer.write_bit(value.title_suppressed);
            self.writer.write_bit(value.header_suppressed);
            self.write_legacy_table_row_style(&value.data_row_style);
            self.write_legacy_table_row_style(&value.title_row_style);
            self.write_legacy_table_row_style(&value.header_row_style);
        } else {
            self.writer.write_byte(value.modern_unknown_byte);
            self.writer.write_variable_text(&value.name);
            self.writer.write_bit_long(value.modern_unknown_long1);
            self.writer.write_bit_long(value.modern_unknown_long2);
            self.writer.write_handle(
                DwgReferenceType::HardOwnership,
                value.modern_cell_style_handle.value(),
            );
            if let Some(style) = &value.modern_style {
                self.write_named_table_cell_style(style);
            } else {
                self.write_default_modern_table_cell_style(value);
            }
            self.writer
                .write_bit_long(value.modern_overrides.len().min(i32::MAX as usize) as i32);
            for (key, style) in value.modern_overrides.iter().take(i32::MAX as usize) {
                self.writer.write_bit_long(*key);
                self.write_named_table_cell_style(style);
            }
        }

        self.register_object(value.handle);
    }

    fn write_legacy_table_row_style(&mut self, value: &RowCellStyle) {
        self.writer.write_handle(
            DwgReferenceType::HardPointer,
            value
                .text_style_handle
                .map(|handle| handle.value())
                .unwrap_or(0),
        );
        self.writer.write_bit_double(value.text_height);
        self.writer.write_bit_short(value.alignment as i16);
        self.writer.write_cm_true_color(&value.text_color);
        self.writer.write_cm_true_color(&value.fill_color);
        self.writer.write_bit(value.fill_enabled);
        for border in [
            &value.top_border,
            &value.horizontal_inside_border,
            &value.bottom_border,
            &value.left_border,
            &value.vertical_inside_border,
            &value.right_border,
        ] {
            self.writer.write_bit_short(border.line_weight.as_i16());
            self.writer.write_bit(!border.is_invisible);
            self.writer.write_cm_true_color(&border.color);
        }
        if self.version == crate::io::dwg::DwgVersion::AC21 {
            self.writer.write_bit_long(value.data_type);
            self.writer.write_bit_long(value.unit_type);
            self.writer.write_variable_text(&value.format_string);
        }
    }

    fn write_object_table_content_format(&mut self, value: &TableContentFormat) {
        self.writer.write_bit_long(value.property_override_flags);
        self.writer.write_bit_long(value.property_flags);
        self.writer.write_bit_long(value.value_data_type);
        self.writer.write_bit_long(value.value_unit_type);
        self.writer.write_variable_text(&value.value_format_string);
        self.writer.write_bit_double(value.rotation);
        self.writer.write_bit_double(value.block_scale);
        self.writer.write_bit_long(value.cell_alignment);
        self.writer.write_cm_true_color(&value.content_color);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, value.text_style.value());
        self.writer.write_bit_double(value.text_height);
    }

    pub(super) fn write_table_cell_style_data(&mut self, value: &TableCellStyleData) {
        self.writer.write_bit_long(value.style_type);
        self.writer.write_bit_short(value.data_flags);
        if value.data_flags == 0 {
            return;
        }
        self.writer.write_bit_long(value.property_override_flags);
        self.writer.write_bit_long(value.merge_flags);
        self.writer.write_cm_true_color(&value.background_color);
        self.writer.write_bit_long(value.content_layout);
        self.write_object_table_content_format(&value.content_format);
        self.writer.write_bit_short(value.margin_override_flags);
        if value.margin_override_flags != 0 {
            self.writer.write_bit_double(value.vertical_margin);
            self.writer.write_bit_double(value.horizontal_margin);
            self.writer.write_bit_double(value.bottom_margin);
            self.writer.write_bit_double(value.right_margin);
            self.writer.write_bit_double(value.horizontal_spacing);
            self.writer.write_bit_double(value.vertical_spacing);
        }
        self.writer
            .write_bit_long(value.borders.len().min(6) as i32);
        for grid in value.borders.iter().take(6) {
            self.writer.write_bit_long(grid.index_mask);
            if grid.index_mask == 0 {
                continue;
            }
            self.writer
                .write_bit_long(grid.border.property_flags.bits());
            self.writer.write_bit_long(grid.border.border_type as i32);
            self.writer.write_cm_true_color(&grid.border.color);
            self.writer
                .write_bit_long(grid.border.line_weight.as_i16() as i32);
            self.writer
                .write_handle(DwgReferenceType::HardPointer, grid.line_type.value());
            self.writer
                .write_bit_long((!grid.border.is_invisible) as i32);
            self.writer
                .write_bit_double(grid.border.double_line_spacing);
        }
    }

    pub(super) fn write_named_table_cell_style(&mut self, value: &NamedTableCellStyle) {
        self.write_table_cell_style_data(&value.cell_style);
        self.writer.write_bit_long(value.id);
        self.writer.write_bit_long(value.style_type);
        self.writer.write_variable_text(&value.name);
    }

    fn write_default_modern_table_cell_style(&mut self, value: &TableStyle) {
        let row = &value.data_row_style;
        let cell_style = TableCellStyleData {
            style_type: 5,
            data_flags: 1,
            background_color: row.fill_color,
            content_layout: 1,
            content_format: TableContentFormat {
                value_data_type: row.data_type,
                value_unit_type: row.unit_type,
                value_format_string: row.format_string.clone(),
                block_scale: 1.0,
                cell_alignment: row.alignment as i32,
                content_color: row.text_color,
                text_style: row.text_style_handle.unwrap_or(Handle::NULL),
                text_height: row.text_height,
                ..TableContentFormat::default()
            },
            margin_override_flags: 1,
            vertical_margin: value.vertical_margin,
            horizontal_margin: value.horizontal_margin,
            bottom_margin: value.vertical_margin,
            right_margin: value.horizontal_margin,
            horizontal_spacing: value.horizontal_margin * 3.0,
            vertical_spacing: value.vertical_margin * 3.0,
            borders: [
                (1, &row.top_border),
                (2, &row.right_border),
                (4, &row.bottom_border),
                (8, &row.left_border),
                (16, &row.horizontal_inside_border),
                (32, &row.vertical_inside_border),
            ]
            .into_iter()
            .map(|(index_mask, border)| TableGridFormat {
                index_mask,
                border: border.clone(),
                line_type: Handle::NULL,
            })
            .collect(),
            ..TableCellStyleData::default()
        };
        self.write_named_table_cell_style(&NamedTableCellStyle {
            cell_style,
            id: 4,
            style_type: 2,
            name: "Table".to_string(),
        });
    }

    // ── Helpers ──────────────────────────────────────────────────────

    /// Whether verbatim `raw_dwg_data` from `raw_dwg_version` can be re-emitted
    /// to the current target. Raw bytes encode the object using the source
    /// version's object-type encoding, stream layout and text encoding, which
    /// differ across the R2004/R2007 and R2007/R2010 boundaries and cannot be
    /// reframed without parsing the (unsupported) object — so passthrough is
    /// only valid for the exact source version. Object schemas change inside
    /// an encoding family too (notably R2013 common data and R2018 proxy
    /// bodies), so carrying raw bytes across those boundaries corrupts the
    /// following fields. `None` is treated as compatible.
    pub(super) fn raw_passthrough_compatible(&self, raw_dwg_version: Option<DxfVersion>) -> bool {
        match raw_dwg_version {
            None => true,
            Some(src) => src == self.dxf_version,
        }
    }

    /// Returns `true` when the record at `handle` will be serialized.
    pub(super) fn is_writable_object(&self, handle: &Handle) -> bool {
        match self.document.objects.get(handle) {
            None => match self.document.get_entity(*handle) {
                Some(crate::entities::EntityType::Unknown(entity)) => entity
                    .raw_dwg_data
                    .as_ref()
                    .is_some_and(|_| self.raw_passthrough_compatible(entity.dwg_source_version)),
                Some(_) => true,
                None => false,
            },
            Some(obj) => match obj {
                ObjectType::VisualStyle(_) => {
                    (self.version.r2007_plus()
                        || self.document.dwg_source_version == Some(self.dxf_version))
                        && self.document.classes.get_by_name("VISUALSTYLE").is_some()
                }
                ObjectType::Material(_) => {
                    (self.version.r2007_plus()
                        || self.document.dwg_source_version == Some(self.dxf_version))
                        && self.document.classes.get_by_name("MATERIAL").is_some()
                }
                ObjectType::TableStyle(_) => {
                    self.document.classes.get_by_name("TABLESTYLE").is_some()
                }
                ObjectType::Unknown {
                    type_name,
                    raw_dwg_data,
                    raw_dwg_version,
                    ..
                } => {
                    // Exclude types that would also be excluded if parsed into proper variants
                    if type_name.starts_with("DWG_OBJ_106") // TABLESTYLE
                        || type_name.starts_with("DWG_OBJ_105")
                    // TABLECONTENT
                    {
                        return false;
                    }
                    // Exclude raw objects dropped on incompatible cross-version save.
                    raw_dwg_data.is_some() && self.raw_passthrough_compatible(*raw_dwg_version)
                }
                _ => true,
            },
        }
    }

    // ── Dictionary ──────────────────────────────────────────────────

    fn write_dictionary(&mut self, dict: &Dictionary) {
        // A pre-R2000 source can legitimately carry newer class-based objects
        // and their NOD entries. Preserve them on a same-version round trip;
        // only strip the newer roots during an actual down-conversion.
        let preserve_source_schema = self.version.r13_14_only()
            && self.document.dwg_source_version == Some(self.dxf_version);
        let entries: Vec<&(String, Handle)> = if self.version.r2000_plus() || preserve_source_schema
        {
            dict.entries
                .iter()
                .filter(|(_, h)| !h.is_null() && self.is_writable_object(h))
                .collect()
        } else {
            dict.entries
                .iter()
                .filter(|(name, h)| {
                    !matches!(
                        name.as_str(),
                        "ACAD_PLOTSTYLENAME"
                            | "ACAD_LAYOUT"
                            | "ACAD_PLOTSETTINGS"
                            | "ACAD_MATERIAL"
                            | "ACAD_COLOR"
                            | "ACAD_VISUALSTYLE"
                    ) && !h.is_null()
                        && self.is_writable_object(h)
                })
                .collect()
        };

        self.write_common_non_entity_data(
            common::OBJ_DICTIONARY,
            dict.handle,
            dict.owner,
            &dict.reactors,
            &dict.xdictionary_handle,
        );

        // Number of entries (BL)
        self.writer.write_bit_long(entries.len() as i32);

        // R14 Only: Unknown byte (always 0)
        if self.dxf_version == DxfVersion::AC1014 {
            self.writer.write_byte(0);
        }

        // R2000+: Cloning flag (BS 281) + Hard-owner flag (RC)
        if self.version.r2000_plus() {
            self.writer.write_bit_short(dict.duplicate_cloning as i16);
            self.writer.write_byte(if dict.hard_owner { 1 } else { 0 });
        }

        // Entry names + handles
        for (name, handle) in &entries {
            self.writer.write_variable_text(name);
            // Dictionary item handles ALWAYS use reference code 2 (soft owner),
            // regardless of the hard-owner flag. The hard/soft distinction is
            // carried only by the is_hardowner byte (and the DXF 350/360 group),
            // NOT the DWG handle code. Writing code 3 here makes AutoCAD reject
            // every entry (eWrongObjectType) and discard the whole dictionary.
            // (libredwg dwg.spec: HANDLE_VECTOR_N(itemhandles, numitems, 2, 0).)
            self.writer
                .write_handle(DwgReferenceType::SoftOwnership, handle.value());

            // Enqueue referenced objects
            if !handle.is_null() {
                self.object_queue.push_back(*handle);
            }
        }

        self.register_object(dict.handle);
    }

    // ── Dictionary with default ─────────────────────────────────────

    fn write_dictionary_with_default(&mut self, dict: &DictionaryWithDefault) {
        // UNLISTED type — always use its class number when the source class
        // exists.
        let type_code = self.class_type_code("ACDBDICTIONARYWDFLT", common::OBJ_DICTIONARYWDFLT);

        self.write_common_non_entity_data(type_code, dict.handle, dict.owner, &[], &None);

        // Filter out entries referencing un-writable objects
        let entries: Vec<&(String, Handle)> = dict
            .entries
            .iter()
            .filter(|(_, h)| h.is_null() || self.is_writable_object(h))
            .collect();

        // Same as dictionary
        self.writer.write_bit_long(entries.len() as i32);

        // Unlike a plain DICTIONARY, these fields are present in R13/R14 too.
        self.writer.write_bit_short(dict.duplicate_cloning as i16);
        self.writer.write_byte(if dict.hard_owner { 1 } else { 0 });

        for (name, handle) in &entries {
            self.writer.write_variable_text(name);
            // Dictionary item handles always use reference code 2 (see
            // write_dictionary) — never code 3, which AutoCAD rejects.
            self.writer
                .write_handle(DwgReferenceType::SoftOwnership, handle.value());

            if !handle.is_null() {
                self.object_queue.push_back(*handle);
            }
        }

        // Default entry handle
        self.writer
            .write_handle(DwgReferenceType::HardPointer, dict.default_handle.value());

        self.register_object(dict.handle);
    }

    // ── Dictionary Variable ─────────────────────────────────────────

    fn write_dictionary_variable(&mut self, dv: &DictionaryVariable) {
        // UNLISTED type — always use DXF class number (500+)
        let type_code = self.class_type_code("DICTIONARYVAR", common::OBJ_DICTIONARYVAR);
        self.write_common_non_entity_data(type_code, dv.handle, dv.owner_handle, &[], &None);

        self.writer.write_byte(0); // object schema number
        self.writer.write_variable_text(&dv.value);

        self.register_object(dv.handle);
    }

    // ── Layout (extends PlotSettings) ───────────────────────────────
    //
    // Layout extends PlotSettings, so PlotSettings fields come first.

    fn write_layout(&mut self, layout: &Layout) {
        // For pre-R2004, LAYOUT is an UNLISTED type — must use the DXF
        // class number instead of the fixed type code 82.
        let type_code = if self.version.r2004_pre() {
            self.document
                .classes
                .get_by_name("LAYOUT")
                .map(|c| c.class_number)
                .unwrap_or(common::OBJ_LAYOUT)
        } else {
            common::OBJ_LAYOUT
        };

        self.write_common_non_entity_data(
            type_code,
            layout.handle,
            layout.owner,
            &layout.reactors,
            &layout.xdictionary_handle,
        );

        // ── PlotSettings preamble ──
        // ModelType flag (bit 0x400) must be set for model space layouts
        let mut plot_flags = layout.plot_flags.to_bits();
        if layout.name == "Model" {
            plot_flags |= 0x400;
        }
        self.write_plot_settings_data(plot_flags, layout);

        // ── Layout-specific data ──
        // Layout name (TV)
        self.writer.write_variable_text(&layout.name);
        // Tab order (BS 71)
        self.writer.write_bit_short(layout.tab_order);
        // Layout flags (BS 70)
        self.writer.write_bit_short(layout.flags);

        // Insertion base (3BD 12)
        self.writer.write_3bit_double(crate::types::Vector3::new(
            layout.insertion_base.0,
            layout.insertion_base.1,
            layout.insertion_base.2,
        ));

        // Min limits (2RD 10)
        self.writer.write_raw_double(layout.min_limits.0);
        self.writer.write_raw_double(layout.min_limits.1);
        // Max limits (2RD 11)
        self.writer.write_raw_double(layout.max_limits.0);
        self.writer.write_raw_double(layout.max_limits.1);

        // UCS origin (3BD 13)
        self.writer.write_3bit_double(crate::types::Vector3::new(
            layout.ucs_origin.0,
            layout.ucs_origin.1,
            layout.ucs_origin.2,
        ));

        // X axis direction (3BD)
        self.writer.write_3bit_double(crate::types::Vector3::new(
            layout.ucs_x_axis.0,
            layout.ucs_x_axis.1,
            layout.ucs_x_axis.2,
        ));
        // Y axis direction (3BD)
        self.writer.write_3bit_double(crate::types::Vector3::new(
            layout.ucs_y_axis.0,
            layout.ucs_y_axis.1,
            layout.ucs_y_axis.2,
        ));

        // Elevation (BD)
        self.writer.write_bit_double(layout.elevation);

        // UCS orthographic type (BS)
        self.writer.write_bit_short(layout.ucs_ortho_type);

        // Min extents (3BD)
        self.writer.write_3bit_double(crate::types::Vector3::new(
            layout.min_extents.0,
            layout.min_extents.1,
            layout.min_extents.2,
        ));
        // Max extents (3BD)
        self.writer.write_3bit_double(crate::types::Vector3::new(
            layout.max_extents.0,
            layout.max_extents.1,
            layout.max_extents.2,
        ));

        // R2004+: Viewport count (BL)
        if self.version.r2004_plus() {
            self.writer.write_bit_long(layout.viewports.len() as i32);
        }

        // ── Handle references ──
        // 330 Associated block record (soft pointer)
        self.writer
            .write_handle(DwgReferenceType::SoftPointer, layout.block_record.value());

        // 331 Last active viewport (soft pointer)
        self.writer
            .write_handle(DwgReferenceType::SoftPointer, layout.viewport.value());

        // 346 base UCS handle (hard pointer)
        self.writer
            .write_handle(DwgReferenceType::HardPointer, layout.base_ucs.value());
        // 345 named UCS handle (hard pointer)
        self.writer
            .write_handle(DwgReferenceType::HardPointer, layout.named_ucs.value());

        if self.version.r2004_plus() {
            for viewport in &layout.viewports {
                self.writer
                    .write_handle(DwgReferenceType::SoftPointer, viewport.value());
            }
        }

        self.register_object(layout.handle);
    }

    /// Write the PlotSettings portion of a Layout record.
    ///
    /// Field order must match C# DwgObjectWriter.Objects.cs writePlotSettings()
    /// exactly.
    fn write_plot_settings_data(&mut self, plot_flags: i32, layout: &crate::objects::Layout) {
        // Page setup name (TV 1)
        self.writer.write_variable_text(&layout.plot_page_name);
        // Printer / Config (TV 2)
        self.writer.write_variable_text(&layout.plot_printer_name);
        // Plot layout flags (BS 70)
        self.writer.write_bit_short(plot_flags as i16);

        // Margins (BD: left, bottom, right, top)
        self.writer.write_bit_double(layout.plot_margin_left);
        self.writer.write_bit_double(layout.plot_margin_bottom);
        self.writer.write_bit_double(layout.plot_margin_right);
        self.writer.write_bit_double(layout.plot_margin_top);

        // Paper width (BD 44), height (BD 45)
        self.writer.write_bit_double(layout.paper_width);
        self.writer.write_bit_double(layout.paper_height);

        // Paper size (TV 4)
        self.writer.write_variable_text(&layout.paper_size);

        // Plot origin (2BD 46,47)
        self.writer.write_bit_double(layout.plot_origin_x);
        self.writer.write_bit_double(layout.plot_origin_y);

        // Paper units (BS 72), Plot rotation (BS 73), Plot type (BS 74)
        self.writer.write_bit_short(layout.plot_paper_units);
        self.writer.write_bit_short(layout.plot_rotation);
        self.writer.write_bit_short(layout.plot_type);

        // Plot window (2BD min, 2BD max)
        self.writer.write_bit_double(layout.plot_window_min_x);
        self.writer.write_bit_double(layout.plot_window_min_y);
        self.writer.write_bit_double(layout.plot_window_max_x);
        self.writer.write_bit_double(layout.plot_window_max_y);

        // R13-R2000 only: Plot view name (TV 6)
        if self.version.r13_15_only() {
            self.writer.write_variable_text(&layout.plot_view_name);
        }

        // Real world units / numerator (BD 142)
        self.writer.write_bit_double(layout.plot_scale_numerator);
        // Drawing units / denominator (BD 143)
        self.writer.write_bit_double(layout.plot_scale_denominator);

        // Current style sheet (TV 7)
        self.writer.write_variable_text(&layout.plot_style_sheet);

        // Scale type (BS 75)
        self.writer.write_bit_short(layout.plot_scale_type);

        // Scale factor (BD 147) — standard scale value
        self.writer.write_bit_double(layout.plot_scale_factor);

        // Paper image origin (2BD 148,149)
        self.writer.write_bit_double(layout.paper_image_origin_x);
        self.writer.write_bit_double(layout.paper_image_origin_y);

        // R2004+: shade plot fields
        if self.version.r2004_plus() {
            self.writer.write_bit_short(layout.shade_plot_mode);
            self.writer.write_bit_short(layout.shade_plot_resolution);
            self.writer.write_bit_short(layout.shade_plot_dpi);

            // Plot view handle (soft pointer)
            let plot_view_handle = if !layout.plot_view_handle.is_null() {
                layout.plot_view_handle
            } else {
                self.document
                    .views
                    .get(&layout.plot_view_name)
                    .map(|view| view.handle)
                    .unwrap_or(Handle::NULL)
            };
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, plot_view_handle.value());
        }

        // R2007+: visual style handle
        if self.version.r2007_plus() {
            self.writer.write_handle(
                DwgReferenceType::SoftPointer,
                layout.visual_style_handle.value(),
            );
        }
    }

    /// Write a standalone PlotSettings object.
    fn write_plot_settings_obj(&mut self, ps: &PlotSettings) {
        // UNLISTED type — always use DXF class number (500+)
        let type_code = self.class_type_code("PLOTSETTINGS", common::OBJ_PLOTSETTINGS);

        self.write_common_non_entity_data(
            type_code,
            ps.handle,
            ps.owner,
            &ps.reactors,
            &ps.xdictionary_handle,
        );

        // Field order must match C# writePlotSettings() exactly
        // Page setup name (TV 1)
        self.writer.write_variable_text(&ps.page_name);
        // Printer / Config (TV 2)
        self.writer.write_variable_text(&ps.printer_name);
        // Plot layout flags (BS 70)
        self.writer.write_bit_short(ps.flags.to_bits() as i16);

        // Margins (BD: left, bottom, right, top)
        self.writer.write_bit_double(ps.margins.left);
        self.writer.write_bit_double(ps.margins.bottom);
        self.writer.write_bit_double(ps.margins.right);
        self.writer.write_bit_double(ps.margins.top);

        // Paper width (BD 44), height (BD 45)
        self.writer.write_bit_double(ps.paper_width);
        self.writer.write_bit_double(ps.paper_height);

        // Paper size (TV 4)
        self.writer.write_variable_text(&ps.paper_size);

        // Plot origin (2BD 46,47)
        self.writer.write_bit_double(ps.origin_x);
        self.writer.write_bit_double(ps.origin_y);

        // Paper units (BS 72), Plot rotation (BS 73), Plot type (BS 74)
        self.writer.write_bit_short(ps.paper_units as i16);
        self.writer.write_bit_short(ps.rotation as i16);
        self.writer.write_bit_short(ps.plot_type as i16);

        // Plot window (2BD min, 2BD max)
        self.writer.write_bit_double(ps.plot_window.lower_left_x);
        self.writer.write_bit_double(ps.plot_window.lower_left_y);
        self.writer.write_bit_double(ps.plot_window.upper_right_x);
        self.writer.write_bit_double(ps.plot_window.upper_right_y);

        // R13-R2000 only: Plot view name (TV 6)
        if self.version.r13_15_only() {
            self.writer.write_variable_text(&ps.plot_view_name);
        }

        // Real world units / numerator (BD 142)
        self.writer.write_bit_double(ps.scale_numerator);
        // Drawing units / denominator (BD 143)
        self.writer.write_bit_double(ps.scale_denominator);

        // Current style sheet (TV 7)
        self.writer.write_variable_text(&ps.current_style_sheet);

        // Scale type (BS 75)
        self.writer.write_bit_short(ps.scale_type as i16);

        // Scale factor (BD 147)
        self.writer.write_bit_double(ps.standard_scale_factor);

        // Paper image origin (2BD 148,149)
        self.writer.write_bit_double(ps.paper_image_origin_x);
        self.writer.write_bit_double(ps.paper_image_origin_y);

        // R2004+: shade plot fields
        if self.version.r2004_plus() {
            self.writer.write_bit_short(ps.shade_plot_mode as i16);
            self.writer.write_bit_short(ps.shade_plot_resolution as i16);
            self.writer.write_bit_short(ps.shade_plot_dpi);

            // Plot view handle (soft pointer)
            let plot_view_handle = if !ps.plot_view_handle.is_null() {
                ps.plot_view_handle
            } else {
                self.document
                    .views
                    .get(&ps.plot_view_name)
                    .map(|view| view.handle)
                    .unwrap_or(Handle::NULL)
            };
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, plot_view_handle.value());
        }

        // R2007+: visual style handle
        if self.version.r2007_plus() {
            self.writer.write_handle(
                DwgReferenceType::SoftPointer,
                ps.visual_style_handle.value(),
            );
        }

        self.register_object(ps.handle);
    }

    // ── Group ───────────────────────────────────────────────────────

    fn write_group(&mut self, group: &Group) {
        self.write_common_non_entity_data(common::OBJ_GROUP, group.handle, group.owner, &[], &None);

        self.writer.write_variable_text(&group.description);
        self.writer
            .write_bit_short(if group.unnamed { 1 } else { 0 });
        self.writer
            .write_bit_short(if group.selectable { 1 } else { 0 });

        // Entity handles
        self.writer.write_bit_long(group.entities.len() as i32);
        for h in &group.entities {
            self.writer
                .write_handle(DwgReferenceType::HardPointer, h.value());
        }

        self.register_object(group.handle);
    }

    // ── MLineStyle ──────────────────────────────────────────────────

    fn write_mlinestyle(&mut self, style: &MLineStyle) {
        self.write_common_non_entity_data(
            common::OBJ_MLINESTYLE,
            style.handle,
            style.owner,
            &[],
            &None,
        );

        self.writer.write_variable_text(&style.name);
        self.writer.write_variable_text(&style.description);

        // Flags — DWG binary format swaps some pairs vs the DXF enum:
        //   DWG bit 1 = DisplayJoints, bit 2 = FillOn
        //   (DXF enum: FillOn=1, DisplayJoints=2)
        //   DWG: StartRound=0x20, StartInner=0x40
        //   (DXF: StartInner=0x20, StartRound=0x40)
        //   DWG: EndRound=0x200, EndInner=0x400
        //   (DXF: EndInner=0x200, EndRound=0x400)
        let mut flags: i16 = 0;
        if style.flags.display_joints {
            flags |= 1;
        }
        if style.flags.fill_on {
            flags |= 2;
        }
        if style.flags.start_square_cap {
            flags |= 16;
        }
        if style.flags.start_round_cap {
            flags |= 0x20;
        }
        if style.flags.start_inner_arcs_cap {
            flags |= 0x40;
        }
        if style.flags.end_square_cap {
            flags |= 0x100;
        }
        if style.flags.end_round_cap {
            flags |= 0x200;
        }
        if style.flags.end_inner_arcs_cap {
            flags |= 0x400;
        }
        self.writer.write_bit_short(flags);

        self.writer.write_cm_color(&style.fill_color);
        self.writer.write_bit_double(style.start_angle);
        self.writer.write_bit_double(style.end_angle);

        // Elements
        self.writer.write_byte(style.elements.len() as u8);
        for elem in &style.elements {
            self.writer.write_bit_double(elem.offset);
            self.writer.write_cm_color(&elem.color);

            if self.version.r2018_plus(self.dxf_version) {
                // R2018+: Line type handle (hard pointer)
                let lt_handle = self
                    .document
                    .line_types
                    .get(&elem.linetype)
                    .map(|lt| lt.handle)
                    .unwrap_or(Handle::NULL);
                self.writer
                    .write_handle(DwgReferenceType::HardPointer, lt_handle.value());
            } else {
                // Before R2018: bit_short index into the linetypes in table
                // order, EXCLUDING ByBlock/ByLayer; 0x7FFF means ByLayer.
                let idx: i16 =
                    if elem.linetype.is_empty() || elem.linetype.eq_ignore_ascii_case("ByLayer") {
                        0x7FFF
                    } else {
                        self.document
                            .line_types
                            .iter()
                            .filter(|lt| {
                                !lt.name.eq_ignore_ascii_case("ByBlock")
                                    && !lt.name.eq_ignore_ascii_case("ByLayer")
                            })
                            .position(|lt| lt.name.eq_ignore_ascii_case(&elem.linetype))
                            .map(|p| p as i16)
                            .unwrap_or(0x7FFF)
                    };
                self.writer.write_bit_short(idx);
            }
        }

        self.register_object(style.handle);
    }

    // ── MultiLeaderStyle ────────────────────────────────────────────

    fn write_multileader_style(&mut self, style: &MultiLeaderStyle) {
        // UNLISTED type — always use DXF class number (500+)
        let type_code = self.class_type_code("MLEADERSTYLE", common::OBJ_MLEADERSTYLE);
        self.write_common_non_entity_data(type_code, style.handle, style.owner_handle, &[], &None);

        // R2010+: Version (BS, expected 2)
        if self.version.r2010_plus() {
            self.writer.write_bit_short(2);
        }

        // Content type
        self.writer.write_bit_short(style.content_type as i16);
        // Draw order
        self.writer
            .write_bit_short(style.multileader_draw_order as i16);
        self.writer.write_bit_short(style.leader_draw_order as i16);

        // Max leader points
        self.writer.write_bit_long(style.max_leader_points);
        // Segment angles
        self.writer.write_bit_double(style.first_segment_angle);
        self.writer.write_bit_double(style.second_segment_angle);

        // Leader
        self.writer.write_bit_short(style.path_type as i16);
        self.writer.write_cm_color(&style.line_color);

        let lt = style.line_type_handle.unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, lt.value());
        self.writer
            .write_bit_long(style.line_weight.as_i16() as i32);

        self.writer.write_bit(style.enable_landing);
        self.writer.write_bit_double(style.landing_gap);
        self.writer.write_bit(style.enable_dogleg);
        self.writer.write_bit_double(style.landing_distance);

        self.writer.write_variable_text(&style.description);

        // Arrowhead
        let ah = style.arrowhead_handle.unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, ah.value());
        self.writer.write_bit_double(style.arrowhead_size);

        // Default text
        self.writer.write_variable_text(&style.default_text);

        // Text style
        let ts = style.text_style_handle.unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, ts.value());

        // Text attachments
        self.writer
            .write_bit_short(style.text_left_attachment as i16);
        self.writer
            .write_bit_short(style.text_right_attachment as i16);
        self.writer.write_bit_short(style.text_angle_type as i16);
        self.writer.write_bit_short(style.text_alignment as i16);
        self.writer.write_cm_color(&style.text_color);
        self.writer.write_bit_double(style.text_height);
        self.writer.write_bit(style.text_frame);
        self.writer.write_bit(style.text_always_left);

        self.writer.write_bit_double(style.align_space);

        // Block
        let bc = style.block_content_handle.unwrap_or(Handle::NULL);
        self.writer
            .write_handle(DwgReferenceType::HardPointer, bc.value());
        self.writer.write_cm_color(&style.block_content_color);
        self.writer.write_bit_double(style.block_content_scale_x);
        self.writer.write_bit_double(style.block_content_scale_y);
        self.writer.write_bit_double(style.block_content_scale_z);
        self.writer.write_bit(style.enable_block_scale);
        self.writer.write_bit_double(style.block_content_rotation);
        self.writer.write_bit(style.enable_block_rotation);
        self.writer
            .write_bit_short(style.block_content_connection as i16);

        self.writer.write_bit_double(style.scale_factor);
        self.writer.write_bit(style.property_changed);
        self.writer.write_bit(style.is_annotative);
        self.writer.write_bit_double(style.break_gap_size);

        // R2010+ additional fields
        if self.version.r2010_plus() {
            self.writer
                .write_bit_short(style.text_attachment_direction as i16);
            self.writer
                .write_bit_short(style.text_top_attachment as i16);
            self.writer
                .write_bit_short(style.text_bottom_attachment as i16);
        }

        // R2013+ undocumented flag (DXF code 298)
        if self.version.r2013_plus(self.dxf_version) {
            self.writer.write_bit(style.unknown_flag_298);
        }

        self.register_object(style.handle);
    }

    // ── Image Definition ────────────────────────────────────────────

    fn write_image_definition(&mut self, def: &ImageDefinition) {
        // UNLISTED type — always use DXF class number (500+)
        let type_code = self.class_type_code("IMAGEDEF", common::OBJ_IMAGEDEF);
        self.write_common_non_entity_data(type_code, def.handle, def.owner, &[], &None);

        self.writer.write_bit_long(def.class_version);
        self.writer.write_2raw_double(crate::types::Vector2::new(
            def.size_in_pixels.0 as f64,
            def.size_in_pixels.1 as f64,
        ));
        self.writer.write_variable_text(&def.file_name);
        self.writer.write_bit(def.is_loaded);
        self.writer.write_byte(def.resolution_unit as u8);
        self.writer.write_2raw_double(crate::types::Vector2::new(
            def.pixel_size.0,
            def.pixel_size.1,
        ));

        self.register_object(def.handle);
    }

    // ── Underlay Definition (PDF / DWF / DGN) ───────────────────────

    /// Write an underlay definition object (AcDbUnderlayDefinition): the file
    /// path and page/sheet name, both variable text, after the common
    /// non-entity header.
    fn write_underlay_definition(&mut self, def: &UnderlayDefinition) {
        use crate::entities::underlay::UnderlayType;
        // UNLISTED type — resolve to the registered DXF class number (500+).
        let fallback = match def.underlay_type {
            UnderlayType::Dwf => common::OBJ_DWFDEFINITION,
            UnderlayType::Dgn => common::OBJ_DGNDEFINITION,
            UnderlayType::Pdf => common::OBJ_PDFDEFINITION,
        };
        let type_code = self.class_type_code(def.entity_name(), fallback);
        self.write_common_non_entity_data(type_code, def.handle, def.owner_handle, &[], &None);

        self.writer.write_variable_text(&def.file_path);
        self.writer.write_variable_text(&def.page_name);

        self.register_object(def.handle);
    }

    // ── Image Definition Reactor ────────────────────────────────────

    fn write_image_definition_reactor(&mut self, reactor: &ImageDefinitionReactor) {
        // UNLISTED type — always use DXF class number (500+)
        let type_code = self.class_type_code("IMAGEDEF_REACTOR", common::OBJ_IMAGEDEFREACTOR);
        self.write_common_non_entity_data(type_code, reactor.handle, reactor.owner, &[], &None);

        self.writer.write_bit_long(0); // class version

        // C# reference does NOT write an image_handle here
        // (the reader gets this from the reactor's owner relationship)

        self.register_object(reactor.handle);
    }

    // ── Scale ───────────────────────────────────────────────────────

    fn write_scale(&mut self, scale: &Scale) {
        // UNLISTED type — always use DXF class number (500+)
        let type_code = self.class_type_code("SCALE", common::OBJ_SCALE);
        self.write_common_non_entity_data(type_code, scale.handle, scale.owner_handle, &[], &None);

        self.writer.write_bit_short(0); // unknown BS
        self.writer.write_variable_text(&scale.name);
        self.writer.write_bit_double(scale.paper_units);
        self.writer.write_bit_double(scale.drawing_units);
        self.writer.write_bit(scale.is_unit_scale);

        self.register_object(scale.handle);
    }

    // ── Object Context Data (annotative per-scale leaf) ─────────────

    /// Write an `AcDb*ObjectContextData` leaf.
    ///
    /// The shared `AcDbObjectContextData` base and the type-specific placement
    /// payload are always encoded from the semantic model, including objects
    /// read from an existing file.
    fn write_object_context_data(&mut self, obj: &ObjectContextData) {
        // UNLISTED type — resolve the 500+ class number (registered in the
        // default class set, so this always resolves for a synthesized object).
        let type_code = self.class_type_code(obj.class_name(), 0);
        self.write_common_non_entity_data(
            type_code,
            obj.handle,
            obj.owner_handle,
            &obj.reactors,
            &obj.xdictionary_handle,
        );

        // AcDbObjectContextData base.
        self.writer.write_bit_short(obj.class_version);
        self.writer.write_bit(obj.is_default);

        // AcDbAnnotScaleObjectContextData: first object-specific handle.
        self.writer
            .write_handle(DwgReferenceType::HardPointer, obj.scale.value());

        // AcDb<Type>ObjectContextData placement payload (DATA stream).
        match &obj.kind {
            ObjectContextKind::AnnotScale => {}
            ObjectContextKind::MLeader(context) => {
                self.write_multileader_annotation_context(context, false);
            }
            ObjectContextKind::BlkRef {
                rotation,
                insertion,
                scale_factor,
            } => {
                self.writer.write_bit_double(*rotation);
                self.writer.write_bit_double(insertion.x);
                self.writer.write_bit_double(insertion.y);
                self.writer.write_bit_double(insertion.z);
                self.writer.write_bit_double(scale_factor.x);
                self.writer.write_bit_double(scale_factor.y);
                self.writer.write_bit_double(scale_factor.z);
            }
            ObjectContextKind::Text {
                horizontal_mode,
                rotation,
                insertion,
                alignment,
            } => {
                self.writer.write_bit_short(*horizontal_mode);
                self.writer.write_bit_double(*rotation);
                self.writer.write_raw_double(insertion.x);
                self.writer.write_raw_double(insertion.y);
                self.writer.write_raw_double(alignment.x);
                self.writer.write_raw_double(alignment.y);
            }
            ObjectContextKind::MText(m) => {
                self.writer.write_bit_long(m.attachment);
                // Binary stores x_axis_dir BEFORE ins_pt.
                self.writer.write_bit_double(m.x_axis_dir.x);
                self.writer.write_bit_double(m.x_axis_dir.y);
                self.writer.write_bit_double(m.x_axis_dir.z);
                self.writer.write_bit_double(m.insertion.x);
                self.writer.write_bit_double(m.insertion.y);
                self.writer.write_bit_double(m.insertion.z);
                self.writer.write_bit_double(m.rect_width);
                self.writer.write_bit_double(m.rect_height);
                self.writer.write_bit_double(m.extents_width);
                self.writer.write_bit_double(m.extents_height);
                self.writer.write_bit_long(m.column_type);
                if m.column_type != 0 {
                    if let Some(c) = &m.columns {
                        self.writer.write_bit_long(c.num_heights);
                        self.writer.write_bit_double(c.width);
                        self.writer.write_bit_double(c.gutter);
                        self.writer.write_bit(c.auto_height);
                        self.writer.write_bit(c.flow_reversed);
                        if !c.auto_height && m.column_type == 2 {
                            for h in &c.heights {
                                self.writer.write_bit_double(*h);
                            }
                        }
                    }
                }
            }
            ObjectContextKind::Dim(d) => {
                // AcDbDimensionObjectContextData base (data stream).
                self.writer.write_raw_double(d.def_pt.x);
                self.writer.write_raw_double(d.def_pt.y);
                self.writer.write_bit(d.is_def_textloc);
                self.writer.write_bit_double(d.text_rotation);
                self.writer.write_bit(d.b293);
                self.writer.write_bit(d.dimtofl);
                self.writer.write_bit(d.dimosxd);
                self.writer.write_bit(d.dimatfit);
                self.writer.write_bit(d.dimtix);
                self.writer.write_bit(d.dimtmove);
                self.writer.write_byte(d.override_code);
                self.writer.write_bit(d.has_arrow2);
                self.writer.write_bit(d.flip_arrow2);
                self.writer.write_bit(d.flip_arrow1);
                // Subtype-specific 3BD point(s).
                let mut pt = |p: &crate::types::Vector3| {
                    self.writer.write_bit_double(p.x);
                    self.writer.write_bit_double(p.y);
                    self.writer.write_bit_double(p.z);
                };
                match &d.subtype {
                    DimSubtype::Aligned { dimline_pt } => pt(dimline_pt),
                    DimSubtype::Angular { arc_pt } => pt(arc_pt),
                    DimSubtype::Diametric {
                        first_arc_pt,
                        def_pt,
                    } => {
                        pt(first_arc_pt);
                        pt(def_pt);
                    }
                    DimSubtype::Radial { first_arc_pt } => pt(first_arc_pt),
                    DimSubtype::RadialLarge {
                        ovr_center,
                        jog_point,
                    } => {
                        pt(ovr_center);
                        pt(jog_point);
                    }
                    DimSubtype::Ordinate {
                        feature_location_pt,
                        leader_endpt,
                    } => {
                        pt(feature_location_pt);
                        pt(leader_endpt);
                    }
                }
            }
            ObjectContextKind::MTextAttribute(value) => {
                self.writer.write_bit_short(value.horizontal_mode);
                self.writer.write_bit_double(value.rotation);
                self.writer.write_raw_double(value.insertion.x);
                self.writer.write_raw_double(value.insertion.y);
                let has_context = value.enable_context && value.context.is_some();
                self.writer.write_raw_double(value.alignment.x);
                self.writer.write_raw_double(value.alignment.y);
                self.writer.write_bit(has_context);
                if let Some(context) = value.context.as_ref().filter(|_| has_context) {
                    // Embedded non-entity common tail. Type, handle and EED
                    // are omitted because this object lives inline.
                    self.writer.write_bit_long(context.reactors.len() as i32);
                    self.writer.write_bit(context.xdictionary_handle.is_none());
                    if self.version.r2013_plus(self.dxf_version) {
                        self.writer.write_bit(context.has_binary_data);
                    }
                    self.writer.write_bit_short(context.class_version);
                    self.writer.write_bit(context.is_default);
                    self.writer.write_bit_long(context.mtext.attachment);
                    self.writer.write_bit_double(context.mtext.x_axis_dir.x);
                    self.writer.write_bit_double(context.mtext.x_axis_dir.y);
                    self.writer.write_bit_double(context.mtext.x_axis_dir.z);
                    self.writer.write_bit_double(context.mtext.insertion.x);
                    self.writer.write_bit_double(context.mtext.insertion.y);
                    self.writer.write_bit_double(context.mtext.insertion.z);
                    self.writer.write_bit_double(context.mtext.rect_width);
                    self.writer.write_bit_double(context.mtext.rect_height);
                    self.writer.write_bit_double(context.mtext.extents_width);
                    self.writer.write_bit_double(context.mtext.extents_height);
                    self.writer.write_bit_long(context.mtext.column_type);
                    if context.mtext.column_type != 0 {
                        if let Some(columns) = &context.mtext.columns {
                            self.writer.write_bit_long(columns.num_heights);
                            self.writer.write_bit_double(columns.width);
                            self.writer.write_bit_double(columns.gutter);
                            self.writer.write_bit(columns.auto_height);
                            self.writer.write_bit(columns.flow_reversed);
                            if !columns.auto_height && context.mtext.column_type == 2 {
                                for height in &columns.heights {
                                    self.writer.write_bit_double(*height);
                                }
                            }
                        }
                    }
                }
            }
            ObjectContextKind::Leader(value) => {
                self.writer.write_bit_long(value.points.len() as i32);
                for point in &value.points {
                    self.writer.write_3bit_double(*point);
                }
                self.writer.write_3bit_double(value.x_direction);
                self.writer.write_bit(value.annotation_enabled);
                self.writer.write_3bit_double(value.insertion_offset);
                self.writer.write_3bit_double(value.endpoint_projection);
            }
            ObjectContextKind::Fcf {
                location,
                horizontal_direction,
            } => {
                self.writer.write_bit_double(location.x);
                self.writer.write_bit_double(location.y);
                self.writer.write_bit_double(location.z);
                self.writer.write_bit_double(horizontal_direction.x);
                self.writer.write_bit_double(horizontal_direction.y);
                self.writer.write_bit_double(horizontal_direction.z);
            }
            ObjectContextKind::HatchScale(value) => {
                self.write_hatch_scale_context_data(value);
            }
            ObjectContextKind::HatchView(value) => {
                self.write_hatch_scale_context_data(&value.hatch);
                self.writer.write_bit_double(value.view_normal.x);
                self.writer.write_bit_double(value.view_normal.y);
                self.writer.write_bit_double(value.view_normal.z);
                self.writer.write_bit_double(value.view_rotation);
                self.writer.write_bit(value.evaluate_hatch);
            }
            ObjectContextKind::Opaque => {}
        }

        // Dimension context also carries a hard-pointer to its block (code 5),
        // emitted after the scale in the handle stream.
        if let ObjectContextKind::Dim(d) = &obj.kind {
            self.writer
                .write_handle(DwgReferenceType::HardPointer, d.block.value());
        }
        if let ObjectContextKind::HatchView(value) = &obj.kind {
            self.writer
                .write_handle(DwgReferenceType::SoftOwnership, value.view.value());
        }
        if let ObjectContextKind::MTextAttribute(value) = &obj.kind {
            if value.enable_context {
                if let Some(context) = &value.context {
                    self.writer
                        .write_handle(DwgReferenceType::SoftPointer, context.owner_handle.value());
                    for reactor in &context.reactors {
                        self.writer
                            .write_handle(DwgReferenceType::SoftPointer, reactor.value());
                    }
                    if let Some(xdictionary_handle) = context.xdictionary_handle {
                        self.writer.write_handle(
                            DwgReferenceType::HardOwnership,
                            xdictionary_handle.value(),
                        );
                    }
                    self.writer
                        .write_handle(DwgReferenceType::HardPointer, context.scale.value());
                }
            }
        }

        self.register_object(obj.handle);
    }

    fn write_hatch_scale_context_data(&mut self, value: &crate::objects::HatchScaleContext) {
        self.writer
            .write_bit_short(value.pattern_lines.len() as i16);
        for line in &value.pattern_lines {
            self.writer.write_bit_double(line.angle);
            self.writer.write_bit_double(line.base_point.x);
            self.writer.write_bit_double(line.base_point.y);
            self.writer.write_bit_double(line.offset.x);
            self.writer.write_bit_double(line.offset.y);
            self.writer.write_bit_short(line.dash_lengths.len() as i16);
            for dash in &line.dash_lengths {
                self.writer.write_bit_double(*dash);
            }
        }
        self.writer.write_bit_double(value.pattern_scale);
        self.writer.write_bit_double(value.pattern_base.x);
        self.writer.write_bit_double(value.pattern_base.y);
        self.writer.write_bit_double(value.pattern_base.z);
        self.writer.write_bit_long(value.loops.len() as i32);
        for loop_data in &value.loops {
            self.writer.write_bit_long(loop_data.loop_type);
            self.writer.write_bit(loop_data.supports_context);
            if !loop_data.supports_context {
                if let Some(boundary) = &loop_data.boundary {
                    self.write_hatch_boundary_path_contents(boundary, loop_data.loop_type, false);
                } else if loop_data.loop_type & 2 != 0 {
                    self.writer.write_bit(false);
                    self.writer.write_bit(false);
                    self.writer.write_bit_long(0);
                } else {
                    self.writer.write_bit_long(0);
                }
            }
        }
    }

    // ── Sort Entities Table ─────────────────────────────────────────

    fn write_sort_entities_table(&mut self, table: &SortEntitiesTable) {
        // UNLISTED type — always use DXF class number (500+)
        let type_code = self.class_type_code("SORTENTSTABLE", common::OBJ_SORTENTSTABLE);
        self.write_common_non_entity_data(type_code, table.handle, table.owner_handle, &[], &None);

        let entries: Vec<_> = table.entries().collect();
        self.writer.write_bit_long(entries.len() as i32);

        // Sort handles are stored inline in the DATA section (one per entry);
        // the owner block and the sorted entity handles follow in the handle
        // stream (owner first, then one per entry). Mirrors `read_sort_entities_
        // table`. (#146)
        for entry in &entries {
            // Sort handles are sort-order keys, NOT object references — they use
            // reference code 0 (Undefined/absolute), per the ODA spec
            // (FIELD_HANDLE(sort_ents, 0, 5)). Writing them as a resolvable
            // pointer makes AutoCAD's audit dereference them and mark the
            // SORTENTS object ePermanentlyErased when a key has no live object.
            self.writer
                .write_main_handle(DwgReferenceType::Undefined, entry.sort_handle.value());
        }
        // block_owner is a soft pointer (code 4), not hard — matches the ODA
        // spec FIELD_HANDLE(block_owner, 4, 0); a hard ref here makes AutoCAD
        // reject the table (eWrongObjectType).
        self.writer.write_handle(
            DwgReferenceType::SoftPointer,
            table.block_owner_handle.value(),
        );
        for entry in &entries {
            self.writer
                .write_handle(DwgReferenceType::SoftPointer, entry.entity_handle.value());
        }

        self.register_object(table.handle);
    }

    // ── XRecord ─────────────────────────────────────────────────────

    fn write_xrecord(&mut self, xrec: &XRecord) {
        let type_code = if self.dxf_version <= DxfVersion::AC1014 {
            self.class_type_code("XRECORD", common::OBJ_XRECORD)
        } else {
            common::OBJ_XRECORD
        };
        self.write_common_non_entity_data(
            type_code,
            xrec.handle,
            xrec.owner,
            &xrec.reactors,
            &xrec.xdictionary_handle,
        );

        let advanced_material_entries = match self.document.objects.get(&xrec.owner) {
            Some(ObjectType::Dictionary(dictionary))
                if dictionary.entries.iter().any(|(name, handle)| {
                    name.eq_ignore_ascii_case("ADVMATERIAL") && *handle == xrec.handle
                }) =>
            {
                self.document
                    .objects
                    .values()
                    .find_map(|object| match object {
                        ObjectType::Material(material)
                            if material.xdictionary_handle == Some(xrec.owner)
                                && material.has_advanced_data() =>
                        {
                            let values = vec![
                                XRecordEntry::double(460, material.color_bleed_scale * 100.0),
                                XRecordEntry::double(461, material.indirect_bump_scale * 100.0),
                                XRecordEntry::double(462, material.reflectance_scale * 100.0),
                                XRecordEntry::double(463, material.transmittance_scale * 100.0),
                                XRecordEntry::bool(290, material.two_sided_material),
                                XRecordEntry::int16(270, material.luminance_mode),
                                XRecordEntry::double(464, material.luminance),
                                XRecordEntry::bool(293, material.is_anonymous),
                                XRecordEntry::int16(272, material.global_illumination),
                                XRecordEntry::int16(273, material.final_gather),
                            ];
                            let mut merged = xrec.entries.clone();
                            for value in values {
                                if let Some(existing) =
                                    merged.iter_mut().find(|entry| entry.code == value.code)
                                {
                                    existing.value = value.value;
                                } else {
                                    merged.push(value);
                                }
                            }
                            Some(merged)
                        }
                        _ => None,
                    })
            }
            _ => None,
        };
        let xrecord_entries = advanced_material_entries
            .as_deref()
            .unwrap_or(&xrec.entries);
        let encoding =
            crate::io::dxf::code_page::encoding_from_code_page(&self.document.header.code_page)
                .unwrap_or(encoding_rs::WINDOWS_1252);
        let code_page =
            crate::io::dxf::code_page::dwg_code_page_index(&self.document.header.code_page)
                .min(u8::MAX as u16) as u8;

        // Write xdata bytes first (per spec: data before cloning flags). The
        // blob is captured verbatim from the source version; when saving to a
        // different string-encoding family (code page <-> UTF-16) re-encode the
        // inline strings so the xdata stays valid instead of being dropped.
        let xdata = if !xrec.entries_complete && !xrec.raw_data.is_empty() {
            let tgt_unicode = self.dxf_version >= DxfVersion::AC1021;
            match self.document.dwg_source_version {
                Some(src) if (src >= DxfVersion::AC1021) != tgt_unicode => transcode_xrecord_xdata(
                    &xrec.raw_data,
                    src >= DxfVersion::AC1021,
                    tgt_unicode,
                    encoding,
                    code_page,
                ),
                _ => xrec.raw_data.clone(),
            }
        } else if !xrecord_entries.is_empty() {
            encode_xrecord_entries(
                xrecord_entries,
                self.dxf_version >= DxfVersion::AC1021,
                encoding,
                code_page,
            )
        } else if xrec.raw_data.is_empty() {
            Vec::new()
        } else {
            let tgt_unicode = self.dxf_version >= DxfVersion::AC1021;
            match self.document.dwg_source_version {
                Some(src) if (src >= DxfVersion::AC1021) != tgt_unicode => transcode_xrecord_xdata(
                    &xrec.raw_data,
                    src >= DxfVersion::AC1021,
                    tgt_unicode,
                    encoding,
                    code_page,
                ),
                _ => xrec.raw_data.clone(),
            }
        };
        if !xdata.is_empty() {
            self.writer.write_bit_long(xdata.len() as i32);
            for &b in &xdata {
                self.writer.write_byte(b);
            }
        } else {
            self.writer.write_bit_long(0);
        }

        // R2000+: Cloning flags (valid range 0..5; enum already constrains to valid values)
        if self.dxf_version >= DxfVersion::AC1015 {
            self.writer.write_bit_short(xrec.cloning_flags.to_value());
        }

        if xrec.preserve_object_reference_stream {
            // A decoded DWG may deliberately omit the translation vector even
            // when its payload contains 330-369 values. Preserve that exact
            // representation; adding synthesized handles changes clone
            // semantics in otherwise valid third-party files.
            for reference in &xrec.object_references {
                self.writer.write_handle(
                    xrecord_reference_type(reference.kind),
                    reference.handle.value(),
                );
                if self.document.objects.contains_key(&reference.handle) {
                    self.object_queue.push_back(reference.handle);
                }
            }
        } else {
            // DXF-originated and newly edited XRecords need one translation
            // handle for every 330-369 value. Preserve a supplied kind when
            // available, otherwise derive it from the group-code range.
            let object_id_entries: Vec<_> = xrecord_entries
                .iter()
                .filter(|entry| (330..=369).contains(&entry.code))
                .collect();
            for (index, entry) in object_id_entries.iter().enumerate() {
                let decoded = xrec.object_references.get(index);
                let handle = entry
                    .value
                    .as_handle()
                    .or_else(|| decoded.map(|reference| reference.handle))
                    .unwrap_or(Handle::NULL);
                let reference_type = decoded
                    .map(|reference| xrecord_reference_type(reference.kind))
                    .unwrap_or_else(|| match entry.code {
                        330..=339 => DwgReferenceType::SoftPointer,
                        340..=349 => DwgReferenceType::HardPointer,
                        350..=359 => DwgReferenceType::SoftOwnership,
                        360..=369 => DwgReferenceType::HardOwnership,
                        _ => DwgReferenceType::SoftPointer,
                    });
                self.writer.write_handle(reference_type, handle.value());
                if self.document.objects.contains_key(&handle) {
                    self.object_queue.push_back(handle);
                }
            }
            for reference in xrec.object_references.iter().skip(object_id_entries.len()) {
                self.writer.write_handle(
                    xrecord_reference_type(reference.kind),
                    reference.handle.value(),
                );
                if self.document.objects.contains_key(&reference.handle) {
                    self.object_queue.push_back(reference.handle);
                }
            }
        }

        self.register_object(xrec.handle);
    }

    // ── Raster Variables ────────────────────────────────────────────

    fn write_raster_variables(&mut self, rv: &RasterVariables) {
        // UNLISTED type — always use DXF class number (500+)
        let type_code = self.class_type_code("RASTERVARIABLES", common::OBJ_RASTERVARIABLES);
        self.write_common_non_entity_data(type_code, rv.handle, rv.owner, &[], &None);

        self.writer.write_bit_long(rv.class_version);
        self.writer.write_bit_short(rv.display_image_frame);
        self.writer.write_bit_short(rv.image_quality);
        self.writer.write_bit_short(rv.units);

        self.register_object(rv.handle);
    }

    // ── Spatial Filter (XCLIP clip boundary) ────────────────────────

    fn write_spatial_filter(&mut self, sf: &SpatialFilter) {
        // UNLISTED type — always use DXF class number (500+)
        let type_code = self.class_type_code("SPATIAL_FILTER", common::OBJ_SPATIALFILTER);
        self.write_common_non_entity_data(type_code, sf.handle, sf.owner, &[], &None);

        self.writer.write_bit_short(sf.boundary_points.len() as i16);
        for p in &sf.boundary_points {
            self.writer.write_2raw_double(*p);
        }
        self.writer.write_3bit_double(sf.normal);
        self.writer.write_3bit_double(sf.origin);
        self.writer.write_bit_short(sf.display_enabled as i16);
        self.writer.write_bit_short(sf.front_clip.is_some() as i16);
        if let Some(d) = sf.front_clip {
            self.writer.write_bit_double(d);
        }
        self.writer.write_bit_short(sf.back_clip.is_some() as i16);
        if let Some(d) = sf.back_clip {
            self.writer.write_bit_double(d);
        }
        for v in matrix_to_row_major(&sf.inverse_block_transform) {
            self.writer.write_bit_double(v);
        }
        for v in matrix_to_row_major(&sf.clip_bound_transform) {
            self.writer.write_bit_double(v);
        }

        self.register_object(sf.handle);
    }

    // ── Geographic Data ─────────────────────────────────────────────

    fn write_geodata(&mut self, geo: &GeoData) {
        let type_code = self.class_type_code("GEODATA", common::OBJ_GEODATA);
        self.write_common_non_entity_data(
            type_code,
            geo.handle,
            geo.owner,
            &geo.reactors,
            &geo.xdictionary_handle,
        );

        self.writer.write_bit_long(geo.version);
        self.writer
            .write_handle(DwgReferenceType::SoftPointer, geo.host_block.value());
        self.writer.write_bit_short(geo.coordinate_type);

        if geo.version == 1 {
            self.writer.write_3bit_double(geo.reference_point);
            self.writer.write_bit_long(geo.horizontal_units);
            self.writer.write_3bit_double(geo.design_point);
            self.writer
                .write_3bit_double(geo.obsolete_observation_point);
            self.writer.write_3bit_double(geo.up_direction);
            let north_angle =
                std::f64::consts::FRAC_PI_2 - geo.north_direction.y.atan2(geo.north_direction.x);
            self.writer.write_bit_double(north_angle);
            self.writer.write_3bit_double(geo.obsolete_scale_vector);
            self.writer
                .write_variable_text(&geo.coordinate_system_definition);
            self.writer.write_variable_text(&geo.geo_rss_tag);
            self.writer.write_bit_double(geo.horizontal_unit_scale);
            self.writer
                .write_variable_text(&geo.coordinate_system_datum);
            self.writer.write_variable_text(&geo.coordinate_system_wkt);
        } else {
            self.writer.write_3bit_double(geo.design_point);
            self.writer.write_3bit_double(geo.reference_point);
            self.writer.write_bit_double(geo.horizontal_unit_scale);
            self.writer.write_bit_long(geo.horizontal_units);
            self.writer.write_bit_double(geo.vertical_unit_scale);
            self.writer.write_bit_long(geo.vertical_units);
            self.writer.write_3bit_double(geo.up_direction);
            self.writer.write_2raw_double(geo.north_direction);
            self.writer.write_bit_long(geo.scale_estimation_method);
            self.writer.write_bit_double(geo.user_scale_factor);
            self.writer.write_bit(geo.sea_level_correction);
            self.writer.write_bit_double(geo.sea_level_elevation);
            self.writer
                .write_bit_double(geo.coordinate_projection_radius);
            self.writer
                .write_variable_text(&geo.coordinate_system_definition);
            self.writer.write_variable_text(&geo.geo_rss_tag);
        }

        self.writer.write_variable_text(&geo.observation_from_tag);
        self.writer.write_variable_text(&geo.observation_to_tag);
        self.writer
            .write_variable_text(&geo.observation_coverage_tag);
        self.writer.write_bit_long(geo.mesh_points.len() as i32);
        for point in &geo.mesh_points {
            self.writer.write_2raw_double(point.source);
            self.writer.write_2raw_double(point.destination);
        }
        self.writer.write_bit_long(geo.mesh_faces.len() as i32);
        for face in &geo.mesh_faces {
            self.writer.write_bit_long(face.first);
            self.writer.write_bit_long(face.second);
            self.writer.write_bit_long(face.third);
        }
        if geo.version == 1 {
            self.writer.write_bit(geo.civil_data_present);
            self.writer.write_bit(geo.civil_obsolete_flag);
            self.writer.write_2raw_double(geo.civil_reference_point1);
            self.writer.write_2raw_double(geo.civil_reference_point2);
            self.writer.write_bit_long(geo.civil_unknown1);
            self.writer.write_bit_long(geo.civil_unknown2);
            self.writer.write_bit(geo.civil_unknown_flag1);
            self.writer.write_2raw_double(geo.civil_zero_point1);
            self.writer.write_2raw_double(geo.civil_zero_point2);
            self.writer.write_bit(geo.civil_unknown_flag2);
            self.writer.write_bit_double(geo.civil_north_angle_degrees);
            self.writer.write_bit_double(geo.civil_north_angle_radians);
            self.writer.write_bit_long(geo.scale_estimation_method);
            self.writer.write_bit_double(geo.user_scale_factor);
            self.writer.write_bit(geo.sea_level_correction);
            self.writer.write_bit_double(geo.sea_level_elevation);
            self.writer
                .write_bit_double(geo.coordinate_projection_radius);
        }

        self.register_object(geo.handle);
    }

    // ── PlaceHolder ─────────────────────────────────────────────────

    fn write_placeholder(&mut self, ph: &PlaceHolder) {
        self.write_common_non_entity_data(common::OBJ_PLACEHOLDER, ph.handle, ph.owner, &[], &None);

        self.register_object(ph.handle);
    }

    // ── BookColor ───────────────────────────────────────────────────

    fn write_book_color(&mut self, bc: &BookColor) {
        // UNLISTED type — always use DXF class number (500+)
        let type_code = self.class_type_code("DBCOLOR", common::OBJ_DBCOLOR);
        self.write_common_non_entity_data(type_code, bc.handle, bc.owner, &[], &None);

        if self.version.r2004_plus() {
            self.writer.write_bit_short(0);
            let (r, g, b) = bc.color.rgb().unwrap_or((0, 0, 0));
            let true_color = (0xC2u32 << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
            self.writer.write_bit_long(true_color as i32);

            let mut flags = 0u8;
            if !bc.color_name.is_empty() {
                flags |= 1;
            }
            if !bc.book_name.is_empty() {
                flags |= 2;
            }
            self.writer.write_byte(flags);
            if flags & 1 != 0 {
                self.writer.write_variable_text(&bc.color_name);
            }
            if flags & 2 != 0 {
                self.writer.write_variable_text(&bc.book_name);
            }
        } else {
            self.writer.write_bit_short(bc.color.approximate_index());
        }

        self.register_object(bc.handle);
    }

    // ── Wipeout Variables ───────────────────────────────────────────

    fn write_wipeout_variables(&mut self, wv: &WipeoutVariables) {
        // UNLISTED type — always use DXF class number (500+)
        let type_code = self.class_type_code("WIPEOUTVARIABLES", common::OBJ_WIPEOUTVARIABLES);
        self.write_common_non_entity_data(type_code, wv.handle, wv.owner, &[], &None);

        self.writer.write_bit_short(wv.display_frame);

        self.register_object(wv.handle);
    }
}
