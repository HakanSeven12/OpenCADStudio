//! Semantic Microsoft Office VBA project streams stored in OLE compound files.

use crate::compound_file::BinaryRecord;

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VbaDirectoryStream {
    pub code_page: u16,
    pub records: Vec<VbaDirectoryRecord>,
    original_code_page: u16,
    original_records: Vec<VbaDirectoryRecord>,
    compressed: Vec<BinaryRecord>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VbaDirectoryRecord {
    pub id: u16,
    pub value: VbaDirectoryValue,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VbaDirectoryValue {
    Marker,
    U16(u16),
    U32(u32),
    ProjectVersion {
        major: u32,
        minor: u16,
    },
    AnsiString {
        value: String,
        original_value: String,
        original_code_page: u16,
        encoded: Vec<BinaryRecord>,
    },
    UnicodeString {
        value: String,
        original_value: String,
        encoded: Vec<BinaryRecord>,
    },
    Binary(Vec<BinaryRecord>),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VbaModuleDescriptor {
    pub name: String,
    pub stream_name: String,
    pub source_offset: u32,
    pub code_page: u16,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VbaModuleStream {
    pub source_offset: u32,
    pub code_page: u16,
    pub performance_cache: Vec<BinaryRecord>,
    pub source: String,
    original_source_offset: u32,
    original_code_page: u16,
    original_source: String,
    compressed_source: Vec<BinaryRecord>,
}

impl VbaDirectoryStream {
    pub fn decode(data: &[u8]) -> Option<Self> {
        let decompressed = decompress_container(data)?;
        let raw = raw_directory_records(&decompressed)?;
        let code_page = raw
            .iter()
            .find(|record| record.id == 0x0003)
            .and_then(|record| read_u16(&record.data, 0))
            .unwrap_or(1252);
        let records: Vec<VbaDirectoryRecord> = raw
            .into_iter()
            .map(|record| decode_directory_record(record, code_page))
            .collect();
        Some(Self {
            code_page,
            original_code_page: code_page,
            original_records: records.clone(),
            records,
            compressed: BinaryRecord::split(data, 4096),
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        if self.code_page == self.original_code_page && self.records == self.original_records {
            return BinaryRecord::join(&self.compressed);
        }
        let mut bytes = Vec::new();
        for record in &self.records {
            encode_directory_record(record, self.code_page, &mut bytes);
        }
        compress_container(&bytes)
    }

    pub fn module_descriptors(&self) -> Vec<VbaModuleDescriptor> {
        let mut output = Vec::new();
        let mut name = String::new();
        let mut stream_name = String::new();
        let mut source_offset = 0;
        for record in &self.records {
            match record.id {
                0x0019 | 0x0047 => {
                    if let Some(value) = directory_string(&record.value) {
                        name = value.to_string();
                    }
                }
                0x001A | 0x0032 => {
                    if let Some(value) = directory_string(&record.value) {
                        stream_name = value.to_string();
                    }
                }
                0x0031 => {
                    if let VbaDirectoryValue::U32(value) = record.value {
                        source_offset = value;
                    }
                }
                0x002B => {
                    if !stream_name.is_empty() {
                        output.push(VbaModuleDescriptor {
                            name: name.clone(),
                            stream_name: stream_name.clone(),
                            source_offset,
                            code_page: self.code_page,
                        });
                    }
                    name.clear();
                    stream_name.clear();
                    source_offset = 0;
                }
                _ => {}
            }
        }
        output
    }
}

impl VbaModuleStream {
    pub fn decode(data: &[u8], source_offset: u32, code_page: u16) -> Option<Self> {
        let offset = source_offset as usize;
        let compressed = data.get(offset..)?;
        let source_bytes = decompress_container(compressed)?;
        let source = decode_code_page(&source_bytes, code_page);
        Some(Self {
            source_offset,
            code_page,
            performance_cache: BinaryRecord::split(&data[..offset], 4096),
            original_source_offset: source_offset,
            original_code_page: code_page,
            original_source: source.clone(),
            source,
            compressed_source: BinaryRecord::split(compressed, 4096),
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut output = BinaryRecord::join(&self.performance_cache);
        let source_offset = self.source_offset as usize;
        if output.len() < source_offset {
            output.resize(source_offset, 0);
        } else if output.len() > source_offset {
            output.truncate(source_offset);
        }
        if self.source == self.original_source
            && self.code_page == self.original_code_page
            && self.source_offset == self.original_source_offset
        {
            output.extend_from_slice(&BinaryRecord::join(&self.compressed_source));
        } else {
            let source = encode_code_page(&self.source, self.code_page);
            output.extend_from_slice(&compress_container(&source));
        }
        output
    }
}

struct RawDirectoryRecord {
    id: u16,
    data: Vec<u8>,
}

fn raw_directory_records(data: &[u8]) -> Option<Vec<RawDirectoryRecord>> {
    let mut records = Vec::new();
    let mut offset = 0usize;
    while offset < data.len() {
        if data
            .get(offset..)
            .unwrap_or_default()
            .iter()
            .all(|byte| *byte == 0)
        {
            break;
        }
        let id = read_u16(data, offset)?;
        let size = read_u32(data, offset + 2)? as usize;
        offset += 6;
        let actual_size = if id == 0x0009 {
            size.checked_add(2)?
        } else {
            size
        };
        let end = offset.checked_add(actual_size)?;
        records.push(RawDirectoryRecord {
            id,
            data: data.get(offset..end)?.to_vec(),
        });
        offset = end;
    }
    Some(records)
}

fn decode_directory_record(record: RawDirectoryRecord, code_page: u16) -> VbaDirectoryRecord {
    let id = record.id;
    let value = if id == 0x0009 && record.data.len() >= 6 {
        VbaDirectoryValue::ProjectVersion {
            major: read_u32(&record.data, 0).unwrap_or(0),
            minor: read_u16(&record.data, 4).unwrap_or(0),
        }
    } else if is_ansi_record(id) {
        let encoded = record.data;
        let value = decode_code_page(encoded.strip_suffix(&[0]).unwrap_or(&encoded), code_page);
        VbaDirectoryValue::AnsiString {
            original_value: value.clone(),
            value,
            original_code_page: code_page,
            encoded: BinaryRecord::split(&encoded, 4096),
        }
    } else if is_unicode_record(id) {
        let encoded = record.data;
        let value = decode_utf16(&encoded);
        VbaDirectoryValue::UnicodeString {
            original_value: value.clone(),
            value,
            encoded: BinaryRecord::split(&encoded, 4096),
        }
    } else if is_u16_record(id) && record.data.len() == 2 {
        VbaDirectoryValue::U16(read_u16(&record.data, 0).unwrap_or(0))
    } else if is_u32_record(id) && record.data.len() == 4 {
        VbaDirectoryValue::U32(read_u32(&record.data, 0).unwrap_or(0))
    } else if is_marker_record(id) && record.data.is_empty() {
        VbaDirectoryValue::Marker
    } else {
        VbaDirectoryValue::Binary(BinaryRecord::split(&record.data, 4096))
    };
    VbaDirectoryRecord { id, value }
}

fn encode_directory_record(record: &VbaDirectoryRecord, code_page: u16, output: &mut Vec<u8>) {
    output.extend_from_slice(&record.id.to_le_bytes());
    match &record.value {
        VbaDirectoryValue::ProjectVersion { major, minor } => {
            output.extend_from_slice(&4u32.to_le_bytes());
            output.extend_from_slice(&major.to_le_bytes());
            output.extend_from_slice(&minor.to_le_bytes());
        }
        value => {
            let data = encode_directory_value(value, code_page);
            output.extend_from_slice(&(data.len() as u32).to_le_bytes());
            output.extend_from_slice(&data);
        }
    }
}

fn encode_directory_value(value: &VbaDirectoryValue, code_page: u16) -> Vec<u8> {
    match value {
        VbaDirectoryValue::Marker => Vec::new(),
        VbaDirectoryValue::U16(value) => value.to_le_bytes().to_vec(),
        VbaDirectoryValue::U32(value) => value.to_le_bytes().to_vec(),
        VbaDirectoryValue::ProjectVersion { major, minor } => {
            let mut output = major.to_le_bytes().to_vec();
            output.extend_from_slice(&minor.to_le_bytes());
            output
        }
        VbaDirectoryValue::AnsiString {
            value,
            original_value,
            original_code_page,
            encoded,
        } => {
            if value == original_value && code_page == *original_code_page {
                BinaryRecord::join(encoded)
            } else {
                let mut output = encode_code_page(value, code_page);
                output.push(0);
                output
            }
        }
        VbaDirectoryValue::UnicodeString {
            value,
            original_value,
            encoded,
        } => {
            if value == original_value {
                BinaryRecord::join(encoded)
            } else {
                let mut output = Vec::new();
                for unit in value.encode_utf16().chain(std::iter::once(0)) {
                    output.extend_from_slice(&unit.to_le_bytes());
                }
                output
            }
        }
        VbaDirectoryValue::Binary(records) => BinaryRecord::join(records),
    }
}

fn directory_string(value: &VbaDirectoryValue) -> Option<&str> {
    match value {
        VbaDirectoryValue::AnsiString { value, .. }
        | VbaDirectoryValue::UnicodeString { value, .. } => Some(value),
        _ => None,
    }
}

fn is_ansi_record(id: u16) -> bool {
    matches!(
        id,
        0x0004 | 0x0005 | 0x0006 | 0x000C | 0x0016 | 0x0019 | 0x001A | 0x001C | 0x003D
    )
}

fn is_unicode_record(id: u16) -> bool {
    matches!(id, 0x0032 | 0x003C | 0x003E | 0x0040 | 0x0047 | 0x0048)
}

fn is_u16_record(id: u16) -> bool {
    matches!(id, 0x0003 | 0x000F | 0x0013 | 0x002C)
}

fn is_u32_record(id: u16) -> bool {
    matches!(
        id,
        0x0001 | 0x0002 | 0x0007 | 0x0008 | 0x0014 | 0x001E | 0x0031
    )
}

fn is_marker_record(id: u16) -> bool {
    matches!(id, 0x0010 | 0x0021 | 0x0022 | 0x0025 | 0x0028 | 0x002B)
}

fn decompress_container(data: &[u8]) -> Option<Vec<u8>> {
    if data.first().copied()? != 1 {
        return None;
    }
    let mut offset = 1usize;
    let mut output = Vec::new();
    while offset < data.len() {
        let chunk_start = offset;
        let header = read_u16(data, offset)?;
        offset += 2;
        if (header >> 12) & 0x7 != 0x3 {
            return None;
        }
        let chunk_size = ((header & 0x0FFF) as usize).checked_add(3)?;
        if !(3..=4098).contains(&chunk_size) {
            return None;
        }
        let chunk_end = chunk_start.checked_add(chunk_size)?;
        let chunk = data.get(offset..chunk_end)?;
        if header & 0x8000 == 0 {
            output.extend_from_slice(chunk);
        } else {
            output.extend_from_slice(&decompress_chunk(chunk)?);
        }
        offset = chunk_end;
    }
    Some(output)
}

fn decompress_chunk(data: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::new();
    let mut offset = 0usize;
    while offset < data.len() && output.len() < 4096 {
        let flags = *data.get(offset)?;
        offset += 1;
        for bit in 0..8 {
            if offset >= data.len() || output.len() >= 4096 {
                break;
            }
            if flags & (1 << bit) == 0 {
                output.push(*data.get(offset)?);
                offset += 1;
                continue;
            }
            let token = read_u16(data, offset)? as usize;
            offset += 2;
            let current = output.len();
            let offset_bits = if current <= 16 {
                4
            } else {
                (usize::BITS - (current - 1).leading_zeros()) as usize
            };
            let length_mask = 0xFFFFusize >> offset_bits;
            let length = (token & length_mask) + 3;
            let distance = (token >> (16 - offset_bits)) + 1;
            if distance > output.len() {
                return None;
            }
            for _ in 0..length {
                if output.len() >= 4096 {
                    break;
                }
                let value = output[output.len() - distance];
                output.push(value);
            }
        }
    }
    Some(output)
}

fn compress_container(data: &[u8]) -> Vec<u8> {
    let mut output = vec![1];
    for chunk in data.chunks(4096) {
        let header = 0x3000u16 | (chunk.len().saturating_sub(1) as u16);
        output.extend_from_slice(&header.to_le_bytes());
        output.extend_from_slice(chunk);
    }
    output
}

fn decode_code_page(data: &[u8], code_page: u16) -> String {
    code_page_encoding(code_page).decode(data).0.into_owned()
}

fn encode_code_page(value: &str, code_page: u16) -> Vec<u8> {
    code_page_encoding(code_page).encode(value).0.into_owned()
}

fn code_page_encoding(code_page: u16) -> &'static encoding_rs::Encoding {
    match code_page {
        65001 => encoding_rs::UTF_8,
        932 => encoding_rs::SHIFT_JIS,
        936 => encoding_rs::GBK,
        949 => encoding_rs::EUC_KR,
        950 => encoding_rs::BIG5,
        874 => encoding_rs::WINDOWS_874,
        1250 => encoding_rs::WINDOWS_1250,
        1251 => encoding_rs::WINDOWS_1251,
        1253 => encoding_rs::WINDOWS_1253,
        1254 => encoding_rs::WINDOWS_1254,
        1255 => encoding_rs::WINDOWS_1255,
        1256 => encoding_rs::WINDOWS_1256,
        1257 => encoding_rs::WINDOWS_1257,
        1258 => encoding_rs::WINDOWS_1258,
        866 => encoding_rs::IBM866,
        _ => encoding_rs::WINDOWS_1252,
    }
}

fn decode_utf16(data: &[u8]) -> String {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        data.get(offset..offset + 4)?.try_into().ok()?,
    ))
}
