//! Microsoft Compound File Binary (CFB) storage used by OLE and VBA payloads.

const END_OF_CHAIN: u32 = 0xFFFF_FFFE;
const FREE_SECTOR: u32 = 0xFFFF_FFFF;
const FAT_SECTOR: u32 = 0xFFFF_FFFD;
const DIFAT_SECTOR: u32 = 0xFFFF_FFFC;
const CFB_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StructuredStoragePayload {
    pub leading_records: Vec<BinaryRecord>,
    pub compound_file: Option<CompoundFile>,
    pub trailing_records: Vec<BinaryRecord>,
}

impl StructuredStoragePayload {
    pub fn decode(data: &[u8]) -> Self {
        let Some(offset) = data
            .windows(CFB_MAGIC.len())
            .position(|window| window == CFB_MAGIC)
        else {
            return Self {
                leading_records: BinaryRecord::split(data, 4096),
                compound_file: None,
                trailing_records: Vec::new(),
            };
        };
        match CompoundFile::decode(&data[offset..]) {
            Some((compound_file, consumed)) => Self {
                leading_records: BinaryRecord::split(&data[..offset], 4096),
                compound_file: Some(compound_file),
                trailing_records: BinaryRecord::split(
                    data.get(offset + consumed..).unwrap_or_default(),
                    4096,
                ),
            },
            None => Self {
                leading_records: BinaryRecord::split(data, 4096),
                compound_file: None,
                trailing_records: Vec::new(),
            },
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut output = BinaryRecord::join(&self.leading_records);
        if let Some(compound_file) = &self.compound_file {
            output.extend_from_slice(&compound_file.encode());
        }
        output.extend_from_slice(&BinaryRecord::join(&self.trailing_records));
        output
    }

    pub fn encoded_len(&self) -> usize {
        self.encode().len()
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BinaryRecord {
    pub sequence: u32,
    pub data: Vec<u8>,
}

impl BinaryRecord {
    pub fn split(data: &[u8], chunk_size: usize) -> Vec<Self> {
        let size = chunk_size.max(1);
        data.chunks(size)
            .enumerate()
            .map(|(sequence, data)| Self {
                sequence: sequence as u32,
                data: data.to_vec(),
            })
            .collect()
    }

    pub fn join(records: &[Self]) -> Vec<u8> {
        let mut records: Vec<&Self> = records.iter().collect();
        records.sort_by_key(|record| record.sequence);
        records
            .into_iter()
            .flat_map(|record| record.data.iter().copied())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CompoundFile {
    pub minor_version: u16,
    pub major_version: u16,
    pub transaction_signature: u32,
    pub root: CompoundStorage,
}

impl Default for CompoundFile {
    fn default() -> Self {
        Self {
            minor_version: 0x003E,
            major_version: 3,
            transaction_signature: 0,
            root: CompoundStorage::root(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CompoundStorage {
    pub name: String,
    pub class_id: [u8; 16],
    pub state_bits: u32,
    pub creation_time: u64,
    pub modified_time: u64,
    pub entries: Vec<CompoundEntry>,
}

impl CompoundStorage {
    pub fn root() -> Self {
        Self {
            name: "Root Entry".to_string(),
            class_id: [0; 16],
            state_bits: 0,
            creation_time: 0,
            modified_time: 0,
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CompoundEntry {
    Storage(CompoundStorage),
    Stream(CompoundStream),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CompoundStream {
    pub name: String,
    pub class_id: [u8; 16],
    pub state_bits: u32,
    pub creation_time: u64,
    pub modified_time: u64,
    pub content: CompoundStreamContent,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CompoundStreamContent {
    PropertySet(CompoundPropertySetStream),
    VbaDirectory(crate::vba::VbaDirectoryStream),
    VbaModule(crate::vba::VbaModuleStream),
    Binary(Vec<BinaryRecord>),
}

impl CompoundStreamContent {
    pub fn decode(name: &str, data: &[u8]) -> Self {
        if name.eq_ignore_ascii_case("dir") {
            if let Some(directory) = crate::vba::VbaDirectoryStream::decode(data) {
                return Self::VbaDirectory(directory);
            }
        }
        if name.starts_with('\u{5}') || data.starts_with(&[0xFE, 0xFF]) {
            if let Some(properties) = CompoundPropertySetStream::decode(data) {
                return Self::PropertySet(properties);
            }
        }
        Self::Binary(BinaryRecord::split(data, 4096))
    }

    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::PropertySet(properties) => properties.encode(),
            Self::VbaDirectory(directory) => directory.encode(),
            Self::VbaModule(module) => module.encode(),
            Self::Binary(records) => BinaryRecord::join(records),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CompoundPropertySetStream {
    pub version: u16,
    pub system_identifier: u32,
    pub class_id: [u8; 16],
    pub sets: Vec<CompoundPropertySet>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CompoundPropertySet {
    pub format_id: [u8; 16],
    pub properties: Vec<CompoundProperty>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CompoundProperty {
    pub id: u32,
    pub value: CompoundPropertyValue,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CompoundPropertyValue {
    Empty,
    Null,
    I16(i16),
    I32(i32),
    I64(i64),
    U16(u16),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    Bool(bool),
    AnsiString {
        value: String,
        code_page: u16,
        original_value: String,
        original_code_page: u16,
        encoded: Vec<BinaryRecord>,
    },
    UnicodeString(String),
    FileTime(u64),
    ClassId([u8; 16]),
    Blob(Vec<BinaryRecord>),
    Clipboard {
        format: u32,
        data: Vec<BinaryRecord>,
    },
    Dictionary(Vec<CompoundDictionaryEntry>),
    Vector {
        element_type: u16,
        values: Vec<CompoundPropertyValue>,
    },
    Unsupported {
        variant_type: u32,
        records: Vec<BinaryRecord>,
    },
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CompoundDictionaryEntry {
    pub property_id: u32,
    pub name: String,
}

#[derive(Clone)]
struct DirectoryEntry {
    name: String,
    kind: u8,
    left: u32,
    right: u32,
    child: u32,
    class_id: [u8; 16],
    state_bits: u32,
    creation_time: u64,
    modified_time: u64,
    start: u32,
    size: u64,
}

struct CompoundDecoder<'a> {
    data: &'a [u8],
    sector_size: usize,
    mini_sector_size: usize,
    mini_cutoff: u64,
    fat: Vec<u32>,
    mini_fat: Vec<u32>,
    mini_stream: Vec<u8>,
    directory: Vec<DirectoryEntry>,
}

impl CompoundFile {
    fn decode(data: &[u8]) -> Option<(Self, usize)> {
        let decoder = CompoundDecoder::new(data)?;
        let root_index = decoder.directory.iter().position(|entry| entry.kind == 5)?;
        let root = decoder.storage(root_index, true, &mut std::collections::HashSet::new())?;
        let sector_count = decoder
            .fat
            .iter()
            .rposition(|entry| *entry != FREE_SECTOR)
            .map(|index| index + 1)
            .unwrap_or(0);
        let consumed = decoder
            .sector_size
            .checked_mul(1 + sector_count)?
            .min(data.len());
        Some((
            Self {
                minor_version: read_u16(data, 24)?,
                major_version: read_u16(data, 26)?,
                transaction_signature: read_u32(data, 52)?,
                root,
            },
            consumed,
        ))
    }

    pub fn encode(&self) -> Vec<u8> {
        CompoundEncoder::new(self).encode()
    }
}

impl<'a> CompoundDecoder<'a> {
    fn new(data: &'a [u8]) -> Option<Self> {
        if data.len() < 512 || data.get(..8)? != CFB_MAGIC {
            return None;
        }
        let major = read_u16(data, 26)?;
        let sector_shift = read_u16(data, 30)?;
        let mini_shift = read_u16(data, 32)?;
        if !(9..=12).contains(&sector_shift) || !(6..sector_shift).contains(&mini_shift) {
            return None;
        }
        let sector_size = 1usize << sector_shift;
        let mini_sector_size = 1usize << mini_shift;
        if data.len() < sector_size {
            return None;
        }
        let sector = |index: u32| -> Option<&'a [u8]> {
            let offset = (index as usize + 1).checked_mul(sector_size)?;
            data.get(offset..offset.checked_add(sector_size)?)
        };
        let mut difat: Vec<u32> = (0..109)
            .filter_map(|index| read_u32(data, 76 + index * 4))
            .filter(|value| *value < DIFAT_SECTOR)
            .collect();
        let mut difat_sector = read_u32(data, 68)?;
        let difat_count = read_u32(data, 72)? as usize;
        for _ in 0..difat_count.min(data.len() / sector_size) {
            if difat_sector >= DIFAT_SECTOR {
                break;
            }
            let bytes = sector(difat_sector)?;
            let entries = sector_size / 4;
            for index in 0..entries - 1 {
                let value = read_u32(bytes, index * 4)?;
                if value != FREE_SECTOR {
                    difat.push(value);
                }
            }
            difat_sector = read_u32(bytes, (entries - 1) * 4)?;
        }
        let fat_sector_count = read_u32(data, 44)? as usize;
        let mut fat = Vec::new();
        for sector_index in difat.into_iter().take(fat_sector_count) {
            let bytes = sector(sector_index)?;
            fat.extend(bytes.chunks_exact(4).filter_map(|chunk| read_u32(chunk, 0)));
        }
        if fat.is_empty() {
            return None;
        }
        let regular_chain = |start: u32| {
            read_chain(start, &fat, fat.len() + 1, |index| {
                sector(index).map(|bytes| bytes.to_vec())
            })
        };
        let directory_start = read_u32(data, 48)?;
        let directory_bytes = regular_chain(directory_start);
        let mut directory = Vec::new();
        for bytes in directory_bytes.chunks_exact(128) {
            let name_length = read_u16(bytes, 64)? as usize;
            let name = if (2..=64).contains(&name_length) {
                let units: Vec<u16> = bytes[..name_length - 2]
                    .chunks_exact(2)
                    .filter_map(|chunk| read_u16(chunk, 0))
                    .collect();
                String::from_utf16_lossy(&units)
            } else {
                String::new()
            };
            let size = if major >= 4 {
                read_u64(bytes, 120)?
            } else {
                read_u32(bytes, 120)? as u64
            };
            let mut class_id = [0u8; 16];
            class_id.copy_from_slice(bytes.get(80..96)?);
            directory.push(DirectoryEntry {
                name,
                kind: *bytes.get(66)?,
                left: read_u32(bytes, 68)?,
                right: read_u32(bytes, 72)?,
                child: read_u32(bytes, 76)?,
                class_id,
                state_bits: read_u32(bytes, 96)?,
                creation_time: read_u64(bytes, 100)?,
                modified_time: read_u64(bytes, 108)?,
                start: read_u32(bytes, 116)?,
                size,
            });
        }
        let mini_fat_start = read_u32(data, 60)?;
        let mini_fat_count = read_u32(data, 64)? as usize;
        let mini_fat_bytes = regular_chain(mini_fat_start);
        let mini_fat = mini_fat_bytes
            .chunks_exact(4)
            .take(mini_fat_count.saturating_mul(sector_size / 4))
            .filter_map(|chunk| read_u32(chunk, 0))
            .collect();
        let root = directory.iter().find(|entry| entry.kind == 5)?;
        let mut mini_stream = regular_chain(root.start);
        mini_stream.truncate(root.size as usize);
        Some(Self {
            data,
            sector_size,
            mini_sector_size,
            mini_cutoff: read_u32(data, 56)? as u64,
            fat,
            mini_fat,
            mini_stream,
            directory,
        })
    }

    fn storage(
        &self,
        index: usize,
        root: bool,
        visited: &mut std::collections::HashSet<usize>,
    ) -> Option<CompoundStorage> {
        if !visited.insert(index) {
            return None;
        }
        let entry = self.directory.get(index)?;
        let mut entries = Vec::new();
        let child_indices = self.sibling_tree(entry.child);
        let vba_directory = if entry.name.eq_ignore_ascii_case("VBA") {
            child_indices
                .iter()
                .filter_map(|index| self.directory.get(*index))
                .find(|child| child.kind == 2 && child.name.eq_ignore_ascii_case("dir"))
                .and_then(|child| crate::vba::VbaDirectoryStream::decode(&self.stream_data(child)))
        } else {
            None
        };
        let module_descriptors = vba_directory
            .as_ref()
            .map(|directory| directory.module_descriptors())
            .unwrap_or_default();
        for child_index in child_indices {
            let Some(child) = self.directory.get(child_index) else {
                continue;
            };
            match child.kind {
                1 => {
                    if let Some(storage) = self.storage(child_index, false, visited) {
                        entries.push(CompoundEntry::Storage(storage));
                    }
                }
                2 => {
                    let data = self.stream_data(child);
                    let content = if child.name.eq_ignore_ascii_case("dir") {
                        vba_directory
                            .clone()
                            .map(CompoundStreamContent::VbaDirectory)
                            .unwrap_or_else(|| CompoundStreamContent::decode(&child.name, &data))
                    } else if let Some(module) = module_descriptors
                        .iter()
                        .find(|module| module.stream_name.eq_ignore_ascii_case(&child.name))
                    {
                        crate::vba::VbaModuleStream::decode(
                            &data,
                            module.source_offset,
                            module.code_page,
                        )
                        .map(CompoundStreamContent::VbaModule)
                        .unwrap_or_else(|| CompoundStreamContent::decode(&child.name, &data))
                    } else {
                        CompoundStreamContent::decode(&child.name, &data)
                    };
                    entries.push(CompoundEntry::Stream(CompoundStream {
                        name: child.name.clone(),
                        class_id: child.class_id,
                        state_bits: child.state_bits,
                        creation_time: child.creation_time,
                        modified_time: child.modified_time,
                        content,
                    }));
                }
                _ => {}
            }
        }
        Some(CompoundStorage {
            name: if root && entry.name.is_empty() {
                "Root Entry".to_string()
            } else {
                entry.name.clone()
            },
            class_id: entry.class_id,
            state_bits: entry.state_bits,
            creation_time: entry.creation_time,
            modified_time: entry.modified_time,
            entries,
        })
    }

    fn sibling_tree(&self, root: u32) -> Vec<usize> {
        fn visit(
            directory: &[DirectoryEntry],
            index: u32,
            output: &mut Vec<usize>,
            visited: &mut std::collections::HashSet<u32>,
        ) {
            if index == FREE_SECTOR || !visited.insert(index) {
                return;
            }
            let Some(entry) = directory.get(index as usize) else {
                return;
            };
            visit(directory, entry.left, output, visited);
            output.push(index as usize);
            visit(directory, entry.right, output, visited);
        }
        let mut output = Vec::new();
        visit(
            &self.directory,
            root,
            &mut output,
            &mut std::collections::HashSet::new(),
        );
        output
    }

    fn stream_data(&self, entry: &DirectoryEntry) -> Vec<u8> {
        let size = entry.size as usize;
        if size == 0 {
            return Vec::new();
        }
        let mut output = if entry.size < self.mini_cutoff {
            read_chain(
                entry.start,
                &self.mini_fat,
                self.mini_fat.len() + 1,
                |index| {
                    let offset = index as usize * self.mini_sector_size;
                    self.mini_stream
                        .get(offset..offset + self.mini_sector_size)
                        .map(|bytes| bytes.to_vec())
                },
            )
        } else {
            read_chain(entry.start, &self.fat, self.fat.len() + 1, |index| {
                let offset = (index as usize + 1) * self.sector_size;
                self.data
                    .get(offset..offset + self.sector_size)
                    .map(|bytes| bytes.to_vec())
            })
        };
        output.truncate(size);
        output
    }
}

fn read_chain<F>(start: u32, allocation: &[u32], limit: usize, mut read: F) -> Vec<u8>
where
    F: FnMut(u32) -> Option<Vec<u8>>,
{
    let mut output = Vec::new();
    let mut current = start;
    let mut visited = std::collections::HashSet::new();
    while current < DIFAT_SECTOR && visited.len() < limit && visited.insert(current) {
        let Some(bytes) = read(current) else {
            break;
        };
        output.extend_from_slice(&bytes);
        current = *allocation.get(current as usize).unwrap_or(&END_OF_CHAIN);
    }
    output
}

impl CompoundPropertySetStream {
    pub fn decode(data: &[u8]) -> Option<Self> {
        if read_u16(data, 0)? != 0xFFFE {
            return None;
        }
        let version = read_u16(data, 2)?;
        let system_identifier = read_u32(data, 4)?;
        let mut class_id = [0u8; 16];
        class_id.copy_from_slice(data.get(8..24)?);
        let count = read_u32(data, 24)? as usize;
        if count > 64 {
            return None;
        }
        let mut descriptors = Vec::with_capacity(count);
        for index in 0..count {
            let offset = 28 + index * 20;
            let mut format_id = [0u8; 16];
            format_id.copy_from_slice(data.get(offset..offset + 16)?);
            descriptors.push((format_id, read_u32(data, offset + 16)? as usize));
        }
        let mut sets = Vec::with_capacity(count);
        for (format_id, offset) in descriptors {
            let size = read_u32(data, offset)? as usize;
            let section = data.get(offset..offset.checked_add(size)?.min(data.len()))?;
            let property_count = read_u32(section, 4)? as usize;
            if property_count > 1_000_000 {
                return None;
            }
            let mut table = Vec::with_capacity(property_count);
            for index in 0..property_count {
                let entry = 8 + index * 8;
                table.push((
                    read_u32(section, entry)?,
                    read_u32(section, entry + 4)? as usize,
                ));
            }
            table.sort_by_key(|(_, property_offset)| *property_offset);
            let mut properties = Vec::with_capacity(property_count);
            for index in 0..table.len() {
                let (id, property_offset) = table[index];
                let end = table
                    .get(index + 1)
                    .map(|(_, next)| *next)
                    .unwrap_or(section.len());
                let bytes = section.get(property_offset..end)?;
                let value = if id == 0 {
                    decode_dictionary(bytes)
                } else {
                    decode_property(bytes)
                };
                properties.push(CompoundProperty { id, value });
            }
            properties.sort_by_key(|property| property.id);
            let code_page = properties
                .iter()
                .find(|property| property.id == 1)
                .and_then(|property| property_code_page(&property.value))
                .unwrap_or(1252);
            for property in &mut properties {
                apply_property_code_page(&mut property.value, code_page);
            }
            sets.push(CompoundPropertySet {
                format_id,
                properties,
            });
        }
        Some(Self {
            version,
            system_identifier,
            class_id,
            sets,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let header_size = 28 + self.sets.len() * 20;
        let mut output = vec![0u8; header_size];
        write_u16(&mut output, 0, 0xFFFE);
        write_u16(&mut output, 2, self.version);
        write_u32(&mut output, 4, self.system_identifier);
        output[8..24].copy_from_slice(&self.class_id);
        write_u32(&mut output, 24, self.sets.len() as u32);
        for (index, set) in self.sets.iter().enumerate() {
            let descriptor = 28 + index * 20;
            output[descriptor..descriptor + 16].copy_from_slice(&set.format_id);
            let set_offset = output.len() as u32;
            write_u32(&mut output, descriptor + 16, set_offset);
            output.extend_from_slice(&encode_property_set(set));
        }
        output
    }
}

fn decode_dictionary(data: &[u8]) -> CompoundPropertyValue {
    let Some(count) = read_u32(data, 0) else {
        return CompoundPropertyValue::Dictionary(Vec::new());
    };
    let mut offset = 4usize;
    let mut entries = Vec::new();
    for _ in 0..count.min(1_000_000) {
        let (Some(property_id), Some(length)) =
            (read_u32(data, offset), read_u32(data, offset + 4))
        else {
            break;
        };
        offset += 8;
        let byte_length = length as usize;
        let Some(bytes) = data.get(offset..offset.saturating_add(byte_length)) else {
            break;
        };
        let name = String::from_utf8_lossy(bytes)
            .trim_end_matches('\0')
            .to_string();
        entries.push(CompoundDictionaryEntry { property_id, name });
        offset = align4(offset + byte_length);
    }
    CompoundPropertyValue::Dictionary(entries)
}

fn decode_property(data: &[u8]) -> CompoundPropertyValue {
    let Some(variant_type) = read_u32(data, 0) else {
        return CompoundPropertyValue::Unsupported {
            variant_type: 0,
            records: Vec::new(),
        };
    };
    if variant_type & 0x1000 != 0 {
        let element_type = (variant_type & 0x0FFF) as u16;
        let count = read_u32(data, 4).unwrap_or(0) as usize;
        let mut values = Vec::new();
        let mut offset = 8usize;
        for _ in 0..count.min(1_000_000) {
            let (value, consumed) =
                decode_scalar(element_type as u32, data.get(offset..).unwrap_or_default());
            values.push(value);
            offset = align4(offset.saturating_add(consumed));
        }
        return CompoundPropertyValue::Vector {
            element_type,
            values,
        };
    }
    decode_scalar(variant_type, data.get(4..).unwrap_or_default()).0
}

fn property_code_page(value: &CompoundPropertyValue) -> Option<u16> {
    match value {
        CompoundPropertyValue::I16(value) if *value >= 0 => Some(*value as u16),
        CompoundPropertyValue::I32(value) if (0..=u16::MAX as i32).contains(value) => {
            Some(*value as u16)
        }
        CompoundPropertyValue::U16(value) => Some(*value),
        CompoundPropertyValue::U32(value) if *value <= u16::MAX as u32 => Some(*value as u16),
        _ => None,
    }
}

fn apply_property_code_page(value: &mut CompoundPropertyValue, code_page: u16) {
    match value {
        CompoundPropertyValue::AnsiString {
            value,
            code_page: current_code_page,
            original_value,
            original_code_page,
            encoded,
        } => {
            let bytes = BinaryRecord::join(encoded);
            let bytes = bytes.strip_suffix(&[0]).unwrap_or(&bytes);
            let decoded = decode_code_page(bytes, code_page);
            *value = decoded.clone();
            *original_value = decoded;
            *current_code_page = code_page;
            *original_code_page = code_page;
        }
        CompoundPropertyValue::Vector { values, .. } => {
            for value in values {
                apply_property_code_page(value, code_page);
            }
        }
        _ => {}
    }
}

fn code_page_encoding(code_page: u16) -> &'static encoding_rs::Encoding {
    match code_page {
        65001 => encoding_rs::UTF_8,
        932 => encoding_rs::SHIFT_JIS,
        936 => encoding_rs::GBK,
        949 => encoding_rs::EUC_KR,
        950 => encoding_rs::BIG5,
        1200 => encoding_rs::UTF_16LE,
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

fn decode_code_page(bytes: &[u8], code_page: u16) -> String {
    code_page_encoding(code_page).decode(bytes).0.into_owned()
}

fn encode_code_page(value: &str, code_page: u16) -> Vec<u8> {
    code_page_encoding(code_page).encode(value).0.into_owned()
}

fn decode_scalar(variant_type: u32, data: &[u8]) -> (CompoundPropertyValue, usize) {
    let unsupported = || CompoundPropertyValue::Unsupported {
        variant_type,
        records: BinaryRecord::split(data, 4096),
    };
    match variant_type {
        0 => (CompoundPropertyValue::Empty, 0),
        1 => (CompoundPropertyValue::Null, 0),
        2 => read_u16(data, 0)
            .map(|value| (CompoundPropertyValue::I16(value as i16), 2))
            .unwrap_or_else(|| (unsupported(), data.len())),
        3 | 10 => read_u32(data, 0)
            .map(|value| (CompoundPropertyValue::I32(value as i32), 4))
            .unwrap_or_else(|| (unsupported(), data.len())),
        4 => read_u32(data, 0)
            .map(|value| (CompoundPropertyValue::F32(f32::from_bits(value)), 4))
            .unwrap_or_else(|| (unsupported(), data.len())),
        5 | 7 => read_u64(data, 0)
            .map(|value| (CompoundPropertyValue::F64(f64::from_bits(value)), 8))
            .unwrap_or_else(|| (unsupported(), data.len())),
        11 => read_u16(data, 0)
            .map(|value| (CompoundPropertyValue::Bool(value != 0), 2))
            .unwrap_or_else(|| (unsupported(), data.len())),
        18 => read_u16(data, 0)
            .map(|value| (CompoundPropertyValue::U16(value), 2))
            .unwrap_or_else(|| (unsupported(), data.len())),
        19 => read_u32(data, 0)
            .map(|value| (CompoundPropertyValue::U32(value), 4))
            .unwrap_or_else(|| (unsupported(), data.len())),
        20 => read_u64(data, 0)
            .map(|value| (CompoundPropertyValue::I64(value as i64), 8))
            .unwrap_or_else(|| (unsupported(), data.len())),
        21 => read_u64(data, 0)
            .map(|value| (CompoundPropertyValue::U64(value), 8))
            .unwrap_or_else(|| (unsupported(), data.len())),
        30 => {
            let length = read_u32(data, 0).unwrap_or(0) as usize;
            let bytes = data.get(4..4 + length.min(data.len().saturating_sub(4)));
            match bytes {
                Some(bytes) => (
                    CompoundPropertyValue::AnsiString {
                        value: decode_code_page(bytes.strip_suffix(&[0]).unwrap_or(bytes), 1252),
                        code_page: 1252,
                        original_value: decode_code_page(
                            bytes.strip_suffix(&[0]).unwrap_or(bytes),
                            1252,
                        ),
                        original_code_page: 1252,
                        encoded: BinaryRecord::split(bytes, 4096),
                    },
                    4 + length,
                ),
                None => (unsupported(), data.len()),
            }
        }
        31 => {
            let count = read_u32(data, 0).unwrap_or(0) as usize;
            let byte_length = count.saturating_mul(2);
            let bytes = data.get(4..4 + byte_length.min(data.len().saturating_sub(4)));
            match bytes {
                Some(bytes) => {
                    let units: Vec<u16> = bytes
                        .chunks_exact(2)
                        .filter_map(|chunk| read_u16(chunk, 0))
                        .take_while(|unit| *unit != 0)
                        .collect();
                    (
                        CompoundPropertyValue::UnicodeString(String::from_utf16_lossy(&units)),
                        4 + byte_length,
                    )
                }
                None => (unsupported(), data.len()),
            }
        }
        64 => read_u64(data, 0)
            .map(|value| (CompoundPropertyValue::FileTime(value), 8))
            .unwrap_or_else(|| (unsupported(), data.len())),
        65 => {
            let length = read_u32(data, 0).unwrap_or(0) as usize;
            let bytes = data
                .get(4..4 + length.min(data.len().saturating_sub(4)))
                .unwrap_or_default();
            (
                CompoundPropertyValue::Blob(BinaryRecord::split(bytes, 4096)),
                4 + length,
            )
        }
        71 => {
            let length = read_u32(data, 0).unwrap_or(0) as usize;
            let format = read_u32(data, 4).unwrap_or(0);
            let bytes = data
                .get(8..4 + length.min(data.len().saturating_sub(4)))
                .unwrap_or_default();
            (
                CompoundPropertyValue::Clipboard {
                    format,
                    data: BinaryRecord::split(bytes, 4096),
                },
                4 + length,
            )
        }
        72 => {
            let mut class_id = [0u8; 16];
            if let Some(bytes) = data.get(..16) {
                class_id.copy_from_slice(bytes);
                (CompoundPropertyValue::ClassId(class_id), 16)
            } else {
                (unsupported(), data.len())
            }
        }
        _ => (unsupported(), data.len()),
    }
}

fn encode_property_set(set: &CompoundPropertySet) -> Vec<u8> {
    let table_size = 8 + set.properties.len() * 8;
    let mut output = vec![0u8; table_size];
    write_u32(&mut output, 4, set.properties.len() as u32);
    for (index, property) in set.properties.iter().enumerate() {
        while output.len() % 4 != 0 {
            output.push(0);
        }
        let offset = output.len();
        let bytes = if property.id == 0 {
            match &property.value {
                CompoundPropertyValue::Dictionary(entries) => encode_dictionary(entries),
                value => encode_property(value),
            }
        } else {
            encode_property(&property.value)
        };
        let table_offset = 8 + index * 8;
        write_u32(&mut output, table_offset, property.id);
        write_u32(&mut output, table_offset + 4, offset as u32);
        output.extend_from_slice(&bytes);
    }
    while output.len() % 4 != 0 {
        output.push(0);
    }
    let size = output.len() as u32;
    write_u32(&mut output, 0, size);
    output
}

fn encode_dictionary(entries: &[CompoundDictionaryEntry]) -> Vec<u8> {
    let mut output = Vec::new();
    output.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for entry in entries {
        output.extend_from_slice(&entry.property_id.to_le_bytes());
        let mut bytes = entry.name.as_bytes().to_vec();
        bytes.push(0);
        output.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        output.extend_from_slice(&bytes);
        while output.len() % 4 != 0 {
            output.push(0);
        }
    }
    output
}

fn encode_property(value: &CompoundPropertyValue) -> Vec<u8> {
    let (variant_type, body) = encode_scalar(value);
    let mut output = variant_type.to_le_bytes().to_vec();
    output.extend_from_slice(&body);
    output
}

fn encode_scalar(value: &CompoundPropertyValue) -> (u32, Vec<u8>) {
    match value {
        CompoundPropertyValue::Empty => (0, Vec::new()),
        CompoundPropertyValue::Null => (1, Vec::new()),
        CompoundPropertyValue::I16(value) => (2, value.to_le_bytes().to_vec()),
        CompoundPropertyValue::I32(value) => (3, value.to_le_bytes().to_vec()),
        CompoundPropertyValue::I64(value) => (20, value.to_le_bytes().to_vec()),
        CompoundPropertyValue::U16(value) => (18, value.to_le_bytes().to_vec()),
        CompoundPropertyValue::U32(value) => (19, value.to_le_bytes().to_vec()),
        CompoundPropertyValue::U64(value) => (21, value.to_le_bytes().to_vec()),
        CompoundPropertyValue::F32(value) => (4, value.to_bits().to_le_bytes().to_vec()),
        CompoundPropertyValue::F64(value) => (5, value.to_bits().to_le_bytes().to_vec()),
        CompoundPropertyValue::Bool(value) => (
            11,
            if *value { 0xFFFFu16 } else { 0 }.to_le_bytes().to_vec(),
        ),
        CompoundPropertyValue::AnsiString {
            value,
            code_page,
            original_value,
            original_code_page,
            encoded,
        } => {
            let mut bytes = if value == original_value && code_page == original_code_page {
                BinaryRecord::join(encoded)
            } else {
                let mut bytes = encode_code_page(value, *code_page);
                bytes.push(0);
                bytes
            };
            if bytes.last().copied() != Some(0) {
                bytes.push(0);
            }
            let mut body = (bytes.len() as u32).to_le_bytes().to_vec();
            body.extend_from_slice(&bytes);
            (30, body)
        }
        CompoundPropertyValue::UnicodeString(value) => {
            let mut units: Vec<u16> = value.encode_utf16().collect();
            units.push(0);
            let mut body = (units.len() as u32).to_le_bytes().to_vec();
            for unit in units {
                body.extend_from_slice(&unit.to_le_bytes());
            }
            (31, body)
        }
        CompoundPropertyValue::FileTime(value) => (64, value.to_le_bytes().to_vec()),
        CompoundPropertyValue::ClassId(value) => (72, value.to_vec()),
        CompoundPropertyValue::Blob(records) => {
            let bytes = BinaryRecord::join(records);
            let mut body = (bytes.len() as u32).to_le_bytes().to_vec();
            body.extend_from_slice(&bytes);
            (65, body)
        }
        CompoundPropertyValue::Clipboard { format, data } => {
            let bytes = BinaryRecord::join(data);
            let mut body = ((bytes.len() + 4) as u32).to_le_bytes().to_vec();
            body.extend_from_slice(&format.to_le_bytes());
            body.extend_from_slice(&bytes);
            (71, body)
        }
        CompoundPropertyValue::Vector {
            element_type,
            values,
        } => {
            let mut body = (values.len() as u32).to_le_bytes().to_vec();
            for value in values {
                body.extend_from_slice(&encode_scalar(value).1);
                while body.len() % 4 != 0 {
                    body.push(0);
                }
            }
            (0x1000 | *element_type as u32, body)
        }
        CompoundPropertyValue::Unsupported {
            variant_type,
            records,
        } => (*variant_type, BinaryRecord::join(records)),
        CompoundPropertyValue::Dictionary(entries) => (0, encode_dictionary(entries)),
    }
}

#[derive(Clone)]
struct FlatEntry {
    name: String,
    kind: u8,
    class_id: [u8; 16],
    state_bits: u32,
    creation_time: u64,
    modified_time: u64,
    color: u8,
    left: u32,
    right: u32,
    child: u32,
    stream: Vec<u8>,
    start: u32,
    size: u64,
}

struct CompoundEncoder<'a> {
    file: &'a CompoundFile,
    flat: Vec<FlatEntry>,
}

impl<'a> CompoundEncoder<'a> {
    fn new(file: &'a CompoundFile) -> Self {
        Self {
            file,
            flat: Vec::new(),
        }
    }

    fn encode(mut self) -> Vec<u8> {
        self.add_storage(&self.file.root, true);
        let sector_size = 512usize;
        let mini_sector_size = 64usize;
        let mini_cutoff = 4096usize;
        let mut sectors: Vec<Vec<u8>> = Vec::new();
        let mut fat: Vec<u32> = Vec::new();
        let mut mini_stream = Vec::new();
        let mut mini_fat = Vec::new();

        for entry in &mut self.flat {
            if entry.kind != 2 || entry.stream.is_empty() {
                entry.start = END_OF_CHAIN;
                entry.size = entry.stream.len() as u64;
                continue;
            }
            entry.size = entry.stream.len() as u64;
            if entry.stream.len() < mini_cutoff {
                let count = entry.stream.len().div_ceil(mini_sector_size);
                entry.start = mini_fat.len() as u32;
                for index in 0..count {
                    let start = index * mini_sector_size;
                    let end = (start + mini_sector_size).min(entry.stream.len());
                    mini_stream.extend_from_slice(&entry.stream[start..end]);
                    mini_stream.resize(
                        mini_stream.len().div_ceil(mini_sector_size) * mini_sector_size,
                        0,
                    );
                    mini_fat.push(if index + 1 == count {
                        END_OF_CHAIN
                    } else {
                        entry.start + index as u32 + 1
                    });
                }
            } else {
                entry.start = allocate_chain(&entry.stream, sector_size, &mut sectors, &mut fat);
            }
        }
        let root_start = allocate_chain(&mini_stream, sector_size, &mut sectors, &mut fat);
        let mut mini_fat_bytes: Vec<u8> = mini_fat
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        if !mini_fat_bytes.is_empty() {
            let entry_count = mini_fat_bytes
                .len()
                .div_ceil(sector_size)
                .saturating_mul(sector_size / 4);
            for _ in mini_fat.len()..entry_count {
                mini_fat_bytes.extend_from_slice(&FREE_SECTOR.to_le_bytes());
            }
        }
        let mini_fat_start = allocate_chain(&mini_fat_bytes, sector_size, &mut sectors, &mut fat);
        let mini_fat_sectors = mini_fat_bytes.len().div_ceil(sector_size);
        let directory_bytes = self.directory_bytes(root_start, mini_stream.len() as u64);
        let directory_start = allocate_chain(&directory_bytes, sector_size, &mut sectors, &mut fat);

        let non_allocation_sectors = sectors.len();
        let entries_per_fat = sector_size / 4;
        let entries_per_difat = entries_per_fat - 1;
        let (fat_count, difat_count) =
            allocation_sector_counts(non_allocation_sectors, entries_per_fat, entries_per_difat);
        let difat_start = if difat_count == 0 {
            END_OF_CHAIN
        } else {
            sectors.len() as u32
        };
        let mut difat_sector_ids = Vec::new();
        for _ in 0..difat_count {
            difat_sector_ids.push(sectors.len() as u32);
            sectors.push(vec![0u8; sector_size]);
            fat.push(DIFAT_SECTOR);
        }
        let mut fat_sector_ids = Vec::new();
        for _ in 0..fat_count {
            fat_sector_ids.push(sectors.len() as u32);
            sectors.push(vec![0u8; sector_size]);
            fat.push(FAT_SECTOR);
        }
        fat.resize(fat_count * entries_per_fat, FREE_SECTOR);
        for (index, sector_id) in fat_sector_ids.iter().enumerate() {
            let start = index * entries_per_fat;
            let bytes: Vec<u8> = fat[start..start + entries_per_fat]
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect();
            sectors[*sector_id as usize].copy_from_slice(&bytes);
        }
        let extra_fat = fat_sector_ids.get(109..).unwrap_or_default();
        for (index, sector_id) in difat_sector_ids.iter().enumerate() {
            let start = index * entries_per_difat;
            let entries = &extra_fat
                [start.min(extra_fat.len())..(start + entries_per_difat).min(extra_fat.len())];
            let sector = &mut sectors[*sector_id as usize];
            for (entry_index, value) in entries.iter().enumerate() {
                write_u32(sector, entry_index * 4, *value);
            }
            for entry_index in entries.len()..entries_per_difat {
                write_u32(sector, entry_index * 4, FREE_SECTOR);
            }
            write_u32(
                sector,
                entries_per_difat * 4,
                difat_sector_ids
                    .get(index + 1)
                    .copied()
                    .unwrap_or(END_OF_CHAIN),
            );
        }

        let mut header = vec![0u8; sector_size];
        header[..8].copy_from_slice(&CFB_MAGIC);
        write_u16(&mut header, 24, self.file.minor_version);
        write_u16(&mut header, 26, 3);
        write_u16(&mut header, 28, 0xFFFE);
        write_u16(&mut header, 30, 9);
        write_u16(&mut header, 32, 6);
        write_u32(&mut header, 40, 0);
        write_u32(&mut header, 44, fat_count as u32);
        write_u32(&mut header, 48, directory_start);
        write_u32(&mut header, 52, self.file.transaction_signature);
        write_u32(&mut header, 56, mini_cutoff as u32);
        write_u32(&mut header, 60, mini_fat_start);
        write_u32(&mut header, 64, mini_fat_sectors as u32);
        write_u32(&mut header, 68, difat_start);
        write_u32(&mut header, 72, difat_count as u32);
        for index in 0..109 {
            write_u32(
                &mut header,
                76 + index * 4,
                fat_sector_ids.get(index).copied().unwrap_or(FREE_SECTOR),
            );
        }
        for sector in sectors {
            header.extend_from_slice(&sector);
        }
        header
    }

    fn add_storage(&mut self, storage: &CompoundStorage, root: bool) -> u32 {
        let index = self.flat.len() as u32;
        self.flat.push(FlatEntry {
            name: storage.name.clone(),
            kind: if root { 5 } else { 1 },
            class_id: storage.class_id,
            state_bits: storage.state_bits,
            creation_time: storage.creation_time,
            modified_time: storage.modified_time,
            color: 1,
            left: FREE_SECTOR,
            right: FREE_SECTOR,
            child: FREE_SECTOR,
            stream: Vec::new(),
            start: END_OF_CHAIN,
            size: 0,
        });
        let mut children = Vec::new();
        for entry in &storage.entries {
            let child = match entry {
                CompoundEntry::Storage(storage) => self.add_storage(storage, false),
                CompoundEntry::Stream(stream) => {
                    let child = self.flat.len() as u32;
                    self.flat.push(FlatEntry {
                        name: stream.name.clone(),
                        kind: 2,
                        class_id: stream.class_id,
                        state_bits: stream.state_bits,
                        creation_time: stream.creation_time,
                        modified_time: stream.modified_time,
                        color: 1,
                        left: FREE_SECTOR,
                        right: FREE_SECTOR,
                        child: FREE_SECTOR,
                        stream: stream.content.encode(),
                        start: END_OF_CHAIN,
                        size: 0,
                    });
                    child
                }
            };
            children.push(child);
        }
        children.sort_by(|left, right| {
            let left_name = &self.flat[*left as usize].name;
            let right_name = &self.flat[*right as usize].name;
            left_name
                .encode_utf16()
                .count()
                .cmp(&right_name.encode_utf16().count())
                .then_with(|| left_name.to_uppercase().cmp(&right_name.to_uppercase()))
        });
        self.flat[index as usize].child = build_directory_tree(&mut self.flat, &children);
        index
    }

    fn directory_bytes(&self, root_start: u32, root_size: u64) -> Vec<u8> {
        let entry_count = self.flat.len().max(1).div_ceil(4) * 4;
        let mut output = vec![0u8; entry_count * 128];
        for (index, entry) in self.flat.iter().enumerate() {
            let bytes = &mut output[index * 128..(index + 1) * 128];
            let mut name: Vec<u16> = entry.name.encode_utf16().take(31).collect();
            name.push(0);
            for (unit_index, unit) in name.iter().enumerate() {
                write_u16(bytes, unit_index * 2, *unit);
            }
            write_u16(bytes, 64, (name.len() * 2) as u16);
            bytes[66] = entry.kind;
            bytes[67] = entry.color;
            write_u32(bytes, 68, entry.left);
            write_u32(bytes, 72, entry.right);
            write_u32(bytes, 76, entry.child);
            bytes[80..96].copy_from_slice(&entry.class_id);
            write_u32(bytes, 96, entry.state_bits);
            write_u64(bytes, 100, entry.creation_time);
            write_u64(bytes, 108, entry.modified_time);
            let (start, size) = if entry.kind == 5 {
                (root_start, root_size)
            } else {
                (entry.start, entry.size)
            };
            write_u32(bytes, 116, start);
            write_u64(bytes, 120, size);
        }
        output
    }
}

fn build_directory_tree(flat: &mut [FlatEntry], entries: &[u32]) -> u32 {
    fn build(flat: &mut [FlatEntry], entries: &[u32], depth: usize) -> (u32, usize) {
        if entries.is_empty() {
            return (FREE_SECTOR, depth.saturating_sub(1));
        }
        let middle = entries.len() / 2;
        let index = entries[middle];
        let (left, left_depth) = build(flat, &entries[..middle], depth + 1);
        let (right, right_depth) = build(flat, &entries[middle + 1..], depth + 1);
        flat[index as usize].left = left;
        flat[index as usize].right = right;
        (index, depth.max(left_depth).max(right_depth))
    }

    fn color_deepest(flat: &mut [FlatEntry], index: u32, depth: usize, deepest: usize) {
        if index == FREE_SECTOR {
            return;
        }
        let left = flat[index as usize].left;
        let right = flat[index as usize].right;
        flat[index as usize].color = if depth == deepest && depth != 0 { 0 } else { 1 };
        color_deepest(flat, left, depth + 1, deepest);
        color_deepest(flat, right, depth + 1, deepest);
    }

    if entries.is_empty() {
        return FREE_SECTOR;
    }
    let (root, deepest) = build(flat, entries, 0);
    color_deepest(flat, root, 0, deepest);
    root
}

fn allocate_chain(
    data: &[u8],
    sector_size: usize,
    sectors: &mut Vec<Vec<u8>>,
    fat: &mut Vec<u32>,
) -> u32 {
    if data.is_empty() {
        return END_OF_CHAIN;
    }
    let start = sectors.len() as u32;
    let count = data.len().div_ceil(sector_size);
    for index in 0..count {
        let offset = index * sector_size;
        let end = (offset + sector_size).min(data.len());
        let mut sector = vec![0u8; sector_size];
        sector[..end - offset].copy_from_slice(&data[offset..end]);
        sectors.push(sector);
        fat.push(if index + 1 == count {
            END_OF_CHAIN
        } else {
            start + index as u32 + 1
        });
    }
    start
}

fn allocation_sector_counts(
    non_allocation: usize,
    fat_capacity: usize,
    difat_capacity: usize,
) -> (usize, usize) {
    let mut fat_count = non_allocation.div_ceil(fat_capacity).max(1);
    let mut difat_count = fat_count.saturating_sub(109).div_ceil(difat_capacity);
    loop {
        let total = non_allocation + fat_count + difat_count;
        let next_fat = total.div_ceil(fat_capacity).max(1);
        let next_difat = next_fat.saturating_sub(109).div_ceil(difat_capacity);
        if next_fat == fat_count && next_difat == difat_count {
            return (fat_count, difat_count);
        }
        fat_count = next_fat;
        difat_count = next_difat;
    }
}

fn align4(value: usize) -> usize {
    (value + 3) & !3
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

fn read_u64(data: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        data.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

fn write_u16(data: &mut [u8], offset: usize, value: u16) {
    data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(data: &mut [u8], offset: usize, value: u64) {
    data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}
