//! Typed access to entity proxy-graphics metafiles.
//!
//! Unknown records stay opaque so decoding and re-encoding a metafile does not
//! discard primitives or traits that this module does not model yet.

use crate::types::Vector3;

const HEADER_SIZE: usize = 8;
const RECORD_HEADER_SIZE: usize = 8;
const UNICODE_TEXT_FIXED_SIZE: usize = 96;

/// An entity proxy-graphics metafile.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ProxyGraphics {
    pub records: Vec<ProxyGraphicRecord>,
}

/// A typed proxy-graphics record.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ProxyGraphicRecord {
    /// Type 21: an empty record that disables filling for later primitives.
    FillOff,
    /// Type 36: a single-line UTF-16 text primitive.
    UnicodeText(ProxyUnicodeText),
    /// A record whose payload is not interpreted by this version of acadrust.
    Unknown { record_type: u32, data: Vec<u8> },
}

/// Type-36 Unicode text data.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ProxyUnicodeText {
    pub position: Vector3,
    pub normal: Vector3,
    pub direction: Vector3,
    pub height: f64,
    pub width_factor: f64,
    pub oblique_angle: f64,
    pub text: String,
}

impl ProxyGraphics {
    /// Decode a complete proxy-graphics metafile.
    pub fn decode(data: &[u8]) -> Option<Self> {
        let total_size = read_u32(data, 0)? as usize;
        let record_count = read_u32(data, 4)? as usize;
        if total_size < HEADER_SIZE || total_size > data.len() {
            return None;
        }

        let mut records = Vec::with_capacity(record_count.min(1_000_000));
        let mut offset = HEADER_SIZE;
        for _ in 0..record_count {
            let record_size = read_u32(data, offset)? as usize;
            let record_type = read_u32(data, offset + 4)?;
            if record_size < RECORD_HEADER_SIZE {
                return None;
            }
            let record_end = offset.checked_add(record_size)?;
            if record_end > total_size {
                return None;
            }
            let payload = &data[offset + RECORD_HEADER_SIZE..record_end];
            records.push(decode_record(record_type, payload));
            offset = record_end;
        }
        if offset != total_size {
            return None;
        }
        Some(Self { records })
    }

    /// Encode the records as a complete proxy-graphics metafile.
    pub fn encode(&self) -> Option<Vec<u8>> {
        let record_count = u32::try_from(self.records.len()).ok()?;
        let mut output = vec![0u8; HEADER_SIZE];
        output[4..8].copy_from_slice(&record_count.to_le_bytes());
        for record in &self.records {
            let (record_type, payload) = encode_record(record)?;
            let record_size = u32::try_from(RECORD_HEADER_SIZE.checked_add(payload.len())?).ok()?;
            output.extend_from_slice(&record_size.to_le_bytes());
            output.extend_from_slice(&record_type.to_le_bytes());
            output.extend_from_slice(&payload);
        }
        let total_size = u32::try_from(output.len()).ok()?;
        output[0..4].copy_from_slice(&total_size.to_le_bytes());
        Some(output)
    }
}

impl super::EntityCommon {
    /// Decode this entity's cached proxy graphics.
    pub fn proxy_graphics(&self) -> Option<ProxyGraphics> {
        ProxyGraphics::decode(self.graphic_data.as_deref()?)
    }

    /// Replace this entity's cached proxy graphics.
    pub fn set_proxy_graphics(&mut self, graphics: &ProxyGraphics) -> bool {
        let Some(data) = graphics.encode() else {
            return false;
        };
        self.graphic_data = Some(data);
        true
    }
}

fn decode_record(record_type: u32, payload: &[u8]) -> ProxyGraphicRecord {
    match record_type {
        21 if payload.is_empty() => ProxyGraphicRecord::FillOff,
        36 => decode_unicode_text(payload)
            .map(ProxyGraphicRecord::UnicodeText)
            .unwrap_or_else(|| ProxyGraphicRecord::Unknown {
                record_type,
                data: payload.to_vec(),
            }),
        _ => ProxyGraphicRecord::Unknown {
            record_type,
            data: payload.to_vec(),
        },
    }
}

fn encode_record(record: &ProxyGraphicRecord) -> Option<(u32, Vec<u8>)> {
    match record {
        ProxyGraphicRecord::FillOff => Some((21, Vec::new())),
        ProxyGraphicRecord::UnicodeText(text) => {
            let mut data = Vec::with_capacity(UNICODE_TEXT_FIXED_SIZE + text.text.len() * 2 + 4);
            write_vector(&mut data, text.position);
            write_vector(&mut data, text.normal);
            write_vector(&mut data, text.direction);
            data.extend_from_slice(&text.height.to_le_bytes());
            data.extend_from_slice(&text.width_factor.to_le_bytes());
            data.extend_from_slice(&text.oblique_angle.to_le_bytes());
            for value in text.text.encode_utf16() {
                data.extend_from_slice(&value.to_le_bytes());
            }
            data.extend_from_slice(&0u16.to_le_bytes());
            while (RECORD_HEADER_SIZE + data.len()) % 4 != 0 {
                data.push(0);
            }
            Some((36, data))
        }
        ProxyGraphicRecord::Unknown { record_type, data } => Some((*record_type, data.clone())),
    }
}

fn decode_unicode_text(data: &[u8]) -> Option<ProxyUnicodeText> {
    if data.len() < UNICODE_TEXT_FIXED_SIZE + 2 {
        return None;
    }
    let position = read_vector(data, 0)?;
    let normal = read_vector(data, 24)?;
    let direction = read_vector(data, 48)?;
    let height = read_f64(data, 72)?;
    let width_factor = read_f64(data, 80)?;
    let oblique_angle = read_f64(data, 88)?;
    let tail = &data[UNICODE_TEXT_FIXED_SIZE..];
    let terminator = tail.chunks_exact(2).position(|value| value == [0, 0])?;
    let units = tail[..terminator * 2]
        .chunks_exact(2)
        .map(|value| u16::from_le_bytes([value[0], value[1]]))
        .collect::<Vec<_>>();
    let text = String::from_utf16(&units).ok()?;
    Some(ProxyUnicodeText {
        position,
        normal,
        direction,
        height,
        width_factor,
        oblique_angle,
        text,
    })
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    let value = data.get(offset..offset + 4)?;
    Some(u32::from_le_bytes(value.try_into().ok()?))
}

fn read_f64(data: &[u8], offset: usize) -> Option<f64> {
    let value = data.get(offset..offset + 8)?;
    Some(f64::from_le_bytes(value.try_into().ok()?))
}

fn read_vector(data: &[u8], offset: usize) -> Option<Vector3> {
    Some(Vector3::new(
        read_f64(data, offset)?,
        read_f64(data, offset + 8)?,
        read_f64(data, offset + 16)?,
    ))
}

fn write_vector(output: &mut Vec<u8>, value: Vector3) {
    output.extend_from_slice(&value.x.to_le_bytes());
    output.extend_from_slice(&value.y.to_le_bytes());
    output.extend_from_slice(&value.z.to_le_bytes());
}
