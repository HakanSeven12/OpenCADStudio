//! Typed DXF properties for registered application classes without a published
//! fixed field schema.

use crate::types::Handle;

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SemanticProperty {
    pub subclass: String,
    pub code: i32,
    pub value: SemanticPropertyValue,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SemanticPropertyValue {
    Text(String),
    Bool(bool),
    Byte(u8),
    Int16(i16),
    Int32(i32),
    Int64(i64),
    Double(f64),
    Handle(Handle),
    Binary(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RegisteredClassObject {
    pub handle: Handle,
    pub owner: Handle,
    pub reactors: Vec<Handle>,
    pub xdictionary_handle: Option<Handle>,
    pub dxf_name: String,
    pub cpp_class_name: String,
    pub properties: Vec<SemanticProperty>,
    pub payload: ProxyPayload,
    pub object_ids: Vec<ProxyObjectReference>,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub raw_dwg_data: Option<Vec<u8>>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub raw_dwg_handle_bits: i64,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub raw_dwg_version: Option<crate::types::DxfVersion>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ProxyObject {
    pub handle: Handle,
    pub owner: Handle,
    pub reactors: Vec<Handle>,
    pub xdictionary_handle: Option<Handle>,
    pub proxy_id: i32,
    pub class_id: i32,
    /// R2004+ binary text-stream name of the proxied DXF class.
    pub dxf_subclass: String,
    pub version: i32,
    pub dwg_version: i32,
    pub maintenance_version: i32,
    pub from_dxf: bool,
    pub payload: ProxyPayload,
    /// Opaque custom strings following `dxf_subclass` in the R2007+ text
    /// stream. Kept separate because proxy payloads may address both streams.
    #[cfg_attr(feature = "serde", serde(default))]
    pub text_payload: ProxyPayload,
    pub object_ids: Vec<ProxyObjectReference>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ProxyPayloadEncoding {
    Bytes,
    #[default]
    Bits,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ProxyPayload {
    pub encoding: ProxyPayloadEncoding,
    pub bit_count: u32,
    pub records: Vec<ProxyPayloadRecord>,
}

impl ProxyPayload {
    pub fn from_bits(data: &[u8], bit_count: u32) -> Self {
        Self::from_data(data, bit_count, ProxyPayloadEncoding::Bits)
    }

    pub fn from_bytes(data: &[u8]) -> Self {
        Self::from_data(
            data,
            data.len().saturating_mul(8).min(u32::MAX as usize) as u32,
            ProxyPayloadEncoding::Bytes,
        )
    }

    fn from_data(data: &[u8], bit_count: u32, encoding: ProxyPayloadEncoding) -> Self {
        let mut records = Vec::new();
        let mut bit_offset = 0u32;
        for (sequence, bytes) in data.chunks(127).enumerate() {
            let available = bit_count.saturating_sub(bit_offset);
            let record_bits = available.min((bytes.len() * 8) as u32);
            records.push(ProxyPayloadRecord {
                sequence: sequence as u32,
                bit_offset,
                bit_count: record_bits,
                data: bytes.to_vec(),
            });
            bit_offset = bit_offset.saturating_add(record_bits);
        }
        Self {
            encoding,
            bit_count,
            records,
        }
    }

    pub fn data(&self) -> Vec<u8> {
        let mut records: Vec<&ProxyPayloadRecord> = self.records.iter().collect();
        records.sort_by_key(|record| record.sequence);
        let byte_count = self.bit_count.div_ceil(8) as usize;
        let mut output = vec![0u8; byte_count];
        for record in records {
            let byte_offset = record.bit_offset as usize / 8;
            let byte_length = record.bit_count.div_ceil(8) as usize;
            if byte_offset >= output.len() {
                continue;
            }
            let end = (byte_offset + byte_length)
                .min(output.len())
                .min(byte_offset + record.data.len());
            output[byte_offset..end].copy_from_slice(&record.data[..end - byte_offset]);
        }
        if let Some(last) = output.last_mut() {
            let remainder = self.bit_count % 8;
            if remainder != 0 {
                *last &= 0xFF << (8 - remainder);
            }
        }
        output
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ProxyPayloadRecord {
    pub sequence: u32,
    pub bit_offset: u32,
    pub bit_count: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ProxyReferenceKind {
    Undefined,
    SoftOwnership,
    HardOwnership,
    SoftPointer,
    HardPointer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ProxyObjectReference {
    pub handle: Handle,
    pub kind: ProxyReferenceKind,
}

/// Lossless application payload used when a registered class must be emitted
/// as a standard proxy object/entity.  The wrapper is private to acadrust; CAD
/// hosts only need to preserve the ordinary proxy payload and references.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RegisteredClassEnvelope {
    pub dxf_name: String,
    pub cpp_class_name: String,
    pub properties: Vec<SemanticProperty>,
    pub payload: ProxyPayload,
}

const REGISTERED_CLASS_MAGIC: &[u8] = b"ACADRUST-REGISTERED-CLASS\0";
const REGISTERED_CLASS_ENVELOPE_VERSION: u8 = 1;

pub(crate) fn encode_registered_class_envelope(
    dxf_name: &str,
    cpp_class_name: &str,
    properties: &[SemanticProperty],
    payload: &ProxyPayload,
) -> ProxyPayload {
    let mut data = Vec::new();
    data.extend_from_slice(REGISTERED_CLASS_MAGIC);
    data.push(REGISTERED_CLASS_ENVELOPE_VERSION);
    write_envelope_string(&mut data, dxf_name);
    write_envelope_string(&mut data, cpp_class_name);
    data.push(match payload.encoding {
        ProxyPayloadEncoding::Bytes => 0,
        ProxyPayloadEncoding::Bits => 1,
    });
    data.extend_from_slice(&payload.bit_count.to_le_bytes());
    let payload_data = payload.data();
    write_envelope_bytes(&mut data, &payload_data);
    data.extend_from_slice(&(properties.len().min(u32::MAX as usize) as u32).to_le_bytes());
    for property in properties.iter().take(u32::MAX as usize) {
        write_envelope_string(&mut data, &property.subclass);
        data.extend_from_slice(&property.code.to_le_bytes());
        match &property.value {
            SemanticPropertyValue::Text(value) => {
                data.push(0);
                write_envelope_string(&mut data, value);
            }
            SemanticPropertyValue::Bool(value) => {
                data.push(1);
                data.push(u8::from(*value));
            }
            SemanticPropertyValue::Byte(value) => {
                data.push(2);
                data.push(*value);
            }
            SemanticPropertyValue::Int16(value) => {
                data.push(3);
                data.extend_from_slice(&value.to_le_bytes());
            }
            SemanticPropertyValue::Int32(value) => {
                data.push(4);
                data.extend_from_slice(&value.to_le_bytes());
            }
            SemanticPropertyValue::Int64(value) => {
                data.push(5);
                data.extend_from_slice(&value.to_le_bytes());
            }
            SemanticPropertyValue::Double(value) => {
                data.push(6);
                data.extend_from_slice(&value.to_bits().to_le_bytes());
            }
            SemanticPropertyValue::Handle(value) => {
                data.push(7);
                data.extend_from_slice(&value.value().to_le_bytes());
            }
            SemanticPropertyValue::Binary(value) => {
                data.push(8);
                write_envelope_bytes(&mut data, value);
            }
        }
    }
    ProxyPayload::from_bytes(&data)
}

pub(crate) fn decode_registered_class_envelope(
    payload: &ProxyPayload,
) -> Option<RegisteredClassEnvelope> {
    let data = payload.data();
    if !data.starts_with(REGISTERED_CLASS_MAGIC) {
        return None;
    }
    let mut reader = EnvelopeReader::new(&data[REGISTERED_CLASS_MAGIC.len()..]);
    if reader.byte()? != REGISTERED_CLASS_ENVELOPE_VERSION {
        return None;
    }
    let dxf_name = reader.string()?;
    let cpp_class_name = reader.string()?;
    let payload_encoding = reader.byte()?;
    let payload_bit_count = reader.u32()?;
    let payload_data = reader.bytes()?;
    if payload_bit_count as usize > payload_data.len().saturating_mul(8) {
        return None;
    }
    let original_payload = match payload_encoding {
        0 if payload_bit_count as usize == payload_data.len() * 8 => {
            ProxyPayload::from_bytes(payload_data)
        }
        1 => ProxyPayload::from_bits(payload_data, payload_bit_count),
        _ => return None,
    };
    let property_count = reader.u32()? as usize;
    if property_count > 1_000_000 {
        return None;
    }
    let mut properties = Vec::with_capacity(property_count);
    for _ in 0..property_count {
        let subclass = reader.string()?;
        let code = reader.i32()?;
        let value = match reader.byte()? {
            0 => SemanticPropertyValue::Text(reader.string()?),
            1 => SemanticPropertyValue::Bool(reader.byte()? != 0),
            2 => SemanticPropertyValue::Byte(reader.byte()?),
            3 => SemanticPropertyValue::Int16(reader.i16()?),
            4 => SemanticPropertyValue::Int32(reader.i32()?),
            5 => SemanticPropertyValue::Int64(reader.i64()?),
            6 => SemanticPropertyValue::Double(f64::from_bits(reader.u64()?)),
            7 => SemanticPropertyValue::Handle(Handle::from(reader.u64()?)),
            8 => SemanticPropertyValue::Binary(reader.bytes()?.to_vec()),
            _ => return None,
        };
        properties.push(SemanticProperty {
            subclass,
            code,
            value,
        });
    }
    if !reader.is_empty() {
        return None;
    }
    Some(RegisteredClassEnvelope {
        dxf_name,
        cpp_class_name,
        properties,
        payload: original_payload,
    })
}

fn write_envelope_string(output: &mut Vec<u8>, value: &str) {
    write_envelope_bytes(output, value.as_bytes());
}

fn write_envelope_bytes(output: &mut Vec<u8>, value: &[u8]) {
    let length = value.len().min(u32::MAX as usize);
    output.extend_from_slice(&(length as u32).to_le_bytes());
    output.extend_from_slice(&value[..length]);
}

struct EnvelopeReader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> EnvelopeReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        let end = self.position.checked_add(length)?;
        let value = self.data.get(self.position..end)?;
        self.position = end;
        Some(value)
    }

    fn byte(&mut self) -> Option<u8> {
        Some(*self.take(1)?.first()?)
    }

    fn i16(&mut self) -> Option<i16> {
        Some(i16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    fn i64(&mut self) -> Option<i64> {
        Some(i64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    fn bytes(&mut self) -> Option<&'a [u8]> {
        let length = self.u32()? as usize;
        self.take(length)
    }

    fn string(&mut self) -> Option<String> {
        String::from_utf8(self.bytes()?.to_vec()).ok()
    }

    fn is_empty(&self) -> bool {
        self.position == self.data.len()
    }
}
