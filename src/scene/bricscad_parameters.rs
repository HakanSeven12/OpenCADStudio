//! Visibility states of BricsCAD parametric blocks, read from the objects
//! BricsCAD keeps them in.
//!
//! BricsCAD stores a parametric block's parameters in objects of its own
//! classes under `BDM_DATABASE` in the block's extension dictionary. They are
//! read as raw records (`ObjectType::Unknown`); this module decodes the few it
//! needs from their DWG bytes (DWG 2010 and later):
//!
//! - `BDMDBFEATUREMANAGER` (`FEATURE_MANAGER`) lists the block's features.
//! - A `BDMDBFEATUREDATA` of type `BdmConfigurationFeature` whose table type
//!   is `VisibilityStates` is one visibility parameter: its name and the state
//!   the block shows (`LastKeyValue`).
//! - Its `BDMDBCONFIGURATIONTABLE` names the states (`KeyValues`).
//! - The `BdmDbPersIdManager` (`PERSIDMANAGER`) of an anonymous copy names the
//!   block it was made from (`original.block`).
//!
//! A reference in a state other than the definition's shows such a copy, whose
//! own features hold the state it shows; the members a state hides sit on the
//! layer `*SUPPRESS` there.
//!
//! Each of these objects is a list of named properties. The data stream holds
//! the count and, per property, a type byte and the value. Names and text
//! values are in the string stream and object references in the handle
//! stream, in the order the properties name them.

use document::objects::ObjectType;
use document::{Document, DxfVersion, EntityType, Handle};

/// A visibility parameter of a BricsCAD parametric block.
#[derive(Debug, Clone, PartialEq)]
pub struct VisibilityParameter {
    pub name: String,
    pub states: Vec<String>,
    /// The state the block shows, by index into `states`.
    pub current: Option<usize>,
}

/// The visibility parameters of block record `block`, in the order its feature
/// list holds them. Empty for a block BricsCAD did not make parametric, and
/// for one whose objects cannot be read.
pub fn visibility_parameters(document: &Document, block: Handle) -> Vec<VisibilityParameter> {
    let Some(manager) = database_entry(document, block, "FEATURE_MANAGER") else {
        return Vec::new();
    };
    let Some(Value::List(features)) =
        properties(document, manager).and_then(|p| p.take("Features"))
    else {
        return Vec::new();
    };
    features
        .iter()
        .filter_map(|feature| match feature {
            Value::Object(handle) => visibility_parameter(document, *handle),
            _ => None,
        })
        .collect()
}

/// The visibility parameters of the block reference `insert`, in the states
/// it shows.
pub fn reference_parameters(document: &Document, insert: Handle) -> Vec<VisibilityParameter> {
    let Some(EntityType::Insert(reference)) = document.get_entity(insert) else {
        return Vec::new();
    };
    document
        .block_records
        .get(&reference.block_name)
        .map(|record| visibility_parameters(document, record.handle))
        .unwrap_or_default()
}

/// The block an anonymous copy of a BricsCAD parametric block was made from.
pub fn original_block(document: &Document, block: Handle) -> Option<Handle> {
    let manager = database_entry(document, block, "PERSIDMANAGER")?;
    match properties(document, manager)?.take("original.block")? {
        Value::Object(original) if !original.is_null() && original != block => Some(original),
        _ => None,
    }
}

fn visibility_parameter(document: &Document, feature: Handle) -> Option<VisibilityParameter> {
    let feature = properties(document, feature)?;
    if feature.text("Feature_type")? != "BdmConfigurationFeature"
        || feature.text("TableType")? != "VisibilityStates"
    {
        return None;
    }
    let Value::Object(table) = feature.get("ConfigTable")? else {
        return None;
    };
    let table = properties(document, *table)?;
    let Value::List(keys) = table.get("KeyValues")? else {
        return None;
    };
    let states: Vec<String> = keys
        .iter()
        .map(|key| match key {
            Value::Text(state) => Some(state.clone()),
            _ => None,
        })
        .collect::<Option<_>>()?;
    let name = table
        .text("KeyName")
        .or_else(|| feature.text("Name"))?
        .to_string();
    let current = feature
        .text("LastKeyValue")
        .and_then(|shown| states.iter().position(|state| state == shown));
    Some(VisibilityParameter {
        name,
        states,
        current,
    })
}

/// Entry `key` of the `BDM_DATABASE` dictionary in `block`'s extension
/// dictionary.
fn database_entry(document: &Document, block: Handle, key: &str) -> Option<Handle> {
    let dictionary = |handle: Handle| match document.objects.get(&handle)? {
        ObjectType::Dictionary(dictionary) => Some(dictionary),
        _ => None,
    };
    let extension = dictionary(document.extension_dictionary_handle(block)?)?;
    dictionary(extension.get("BDM_DATABASE")?)?.get(key)
}

/// A property value of a BricsCAD object.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Text(String),
    Integer(i32),
    Real(f64),
    Flag(bool),
    Byte(u8),
    /// A handle written as its bytes in the data stream.
    Raw(Vec<u8>),
    /// A reference through the handle stream.
    Object(Handle),
    List(Vec<Value>),
    Map(Vec<(Value, Value)>),
}

/// The named properties of one BricsCAD object, in stored order.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Properties(pub(crate) Vec<(String, Value)>);

impl Properties {
    pub(crate) fn get(&self, name: &str) -> Option<&Value> {
        self.0
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    fn take(mut self, name: &str) -> Option<Value> {
        let index = self.0.iter().position(|(key, _)| key == name)?;
        Some(self.0.swap_remove(index).1)
    }

    fn text(&self, name: &str) -> Option<&str> {
        match self.get(name)? {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }
}

/// The properties of the BricsCAD object `handle`, decoded from the DWG
/// record it was read from. `None` for any other object, a record from a DXF
/// file or a DWG before 2010, and a record that does not decode in full.
pub(crate) fn properties(document: &Document, handle: Handle) -> Option<Properties> {
    let ObjectType::Unknown {
        raw_dwg_data: Some(data),
        raw_dwg_handle_bits,
        raw_dwg_version: Some(version),
        ..
    } = document.objects.get(&handle)?
    else {
        return None;
    };
    decode(data, *raw_dwg_handle_bits, *version)
}

/// Decode one object record: the merged data, string and handle streams as
/// the DWG reader keeps them, without the size prefixes and the CRC.
pub(crate) fn decode(data: &[u8], handle_bits: i64, version: DxfVersion) -> Option<Properties> {
    if version < DxfVersion::AC1024 {
        return None;
    }
    let total = data.len().checked_mul(8)?;
    let handles_at = total.checked_sub(usize::try_from(handle_bits).ok()?)?;
    let mut main = Bits::new(data, 0, handles_at);
    main.object_type()?;
    let own = main.handle(0)?;
    // Extended data: blocks of a size, an application handle and the bytes,
    // until a size of 0.
    loop {
        let size = main.bit_short()?;
        if size <= 0 {
            break;
        }
        main.handle(0)?;
        for _ in 0..size {
            main.byte()?;
        }
    }
    let reactors = main.bit_long()?;
    let has_extension_dictionary = !main.bit()?;
    if version >= DxfVersion::AC1027 {
        main.bit()?;
    }

    let (strings_at, strings_end) = string_stream(data, handles_at)?;
    if main.pos > strings_at {
        return None;
    }
    main.end = strings_at;
    let mut handles = Bits::new(data, handles_at, total);
    handles.handle(own)?; // owner
    for _ in 0..reactors {
        handles.handle(own)?;
    }
    if has_extension_dictionary {
        handles.handle(own)?;
    }
    let mut reader = Reader {
        main,
        strings: Bits::new(data, strings_at, strings_end),
        handles,
        own,
    };
    let properties = reader.properties()?;
    // A record that leaves data unread was not one of these property lists.
    (reader.main.pos == reader.main.end).then_some(properties)
}

/// The bit range of the string stream, which ends where the data stream's
/// last bit says whether there is one.
fn string_stream(data: &[u8], handles_at: usize) -> Option<(usize, usize)> {
    let flag = handles_at.checked_sub(1)?;
    if !Bits::new(data, flag, handles_at).bit()? {
        return Some((flag, flag));
    }
    let size_at = flag.checked_sub(16)?;
    let mut size = usize::from(Bits::new(data, size_at, flag).raw_short()?);
    let mut start = size_at;
    if size & 0x8000 != 0 {
        let high_at = size_at.checked_sub(16)?;
        let high = usize::from(Bits::new(data, high_at, size_at).raw_short()?);
        size = (size & 0x7FFF) | (high << 15);
        start = high_at;
    }
    Some((start.checked_sub(size)?, start))
}

struct Reader<'a> {
    main: Bits<'a>,
    strings: Bits<'a>,
    handles: Bits<'a>,
    own: u64,
}

impl Reader<'_> {
    fn properties(&mut self) -> Option<Properties> {
        let count = self.main.bit_long()?;
        let mut properties = Vec::new();
        for _ in 0..count {
            let name = self.text()?;
            let kind = self.main.byte()?;
            properties.push((name, self.value(kind, 0)?));
        }
        Some(Properties(properties))
    }

    fn value(&mut self, kind: u8, depth: u8) -> Option<Value> {
        // Lists and maps nest at most a level or two; a deeper one is not
        // a property list.
        if depth > 4 {
            return None;
        }
        Some(match kind {
            2 => Value::Text(self.text()?),
            3 => Value::Integer(self.main.bit_long()?),
            4 => Value::Real(self.main.bit_double()?),
            5 => Value::Flag(self.main.bit()?),
            6 => Value::Byte(self.main.byte()?),
            7 => {
                let length = self.main.byte()?;
                Value::Raw(
                    (0..length)
                        .map(|_| self.main.byte())
                        .collect::<Option<_>>()?,
                )
            }
            8 => {
                self.main.byte()?; // the kind of reference
                Value::Object(Handle::from(self.handles.handle(self.own)?))
            }
            9 => {
                let element = self.main.byte()?;
                let count = self.count()?;
                Value::List(
                    (0..count)
                        .map(|_| self.value(element, depth + 1))
                        .collect::<Option<_>>()?,
                )
            }
            10 => {
                let key = self.main.byte()?;
                let value = self.main.byte()?;
                let count = self.count()?;
                Value::Map(
                    (0..count)
                        .map(|_| Some((self.value(key, depth + 1)?, self.value(value, depth + 1)?)))
                        .collect::<Option<_>>()?,
                )
            }
            _ => return None,
        })
    }

    /// An element count. Every element takes at least one bit of one of the
    /// streams, so a count beyond the record's bits is not one.
    fn count(&mut self) -> Option<usize> {
        let count = usize::try_from(self.main.bit_long()?).ok()?;
        (count <= self.main.data.len() * 8).then_some(count)
    }

    /// A string from the string stream: a length, then UTF-16 code units.
    fn text(&mut self) -> Option<String> {
        let length = usize::try_from(self.strings.bit_short()?).ok()?;
        if length * 16 > self.strings.end - self.strings.pos {
            return None;
        }
        let units: Vec<u16> = (0..length)
            .map(|_| self.strings.raw_short())
            .collect::<Option<_>>()?;
        Some(String::from_utf16_lossy(&units))
    }
}

/// A bit range of a record, read most significant bit first.
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    end: usize,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8], pos: usize, end: usize) -> Self {
        Self {
            data,
            pos,
            end: end.min(data.len() * 8),
        }
    }

    fn bit(&mut self) -> Option<bool> {
        if self.pos >= self.end {
            return None;
        }
        let bit = self.data[self.pos / 8] >> (7 - self.pos % 8) & 1;
        self.pos += 1;
        Some(bit == 1)
    }

    fn bits(&mut self, count: u32) -> Option<u32> {
        (0..count).try_fold(0, |value, _| Some(value << 1 | u32::from(self.bit()?)))
    }

    fn byte(&mut self) -> Option<u8> {
        self.bits(8).map(|value| value as u8)
    }

    fn raw_short(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes([self.byte()?, self.byte()?]))
    }

    fn raw_long(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes([
            self.byte()?,
            self.byte()?,
            self.byte()?,
            self.byte()?,
        ]))
    }

    fn bit_short(&mut self) -> Option<i16> {
        Some(match self.bits(2)? {
            0 => self.raw_short()? as i16,
            1 => i16::from(self.byte()?),
            2 => 0,
            _ => 256,
        })
    }

    fn bit_long(&mut self) -> Option<i32> {
        match self.bits(2)? {
            0 => Some(self.raw_long()? as i32),
            1 => Some(i32::from(self.byte()?)),
            2 => Some(0),
            _ => None,
        }
    }

    fn bit_double(&mut self) -> Option<f64> {
        match self.bits(2)? {
            0 => {
                let mut bytes = [0; 8];
                for byte in &mut bytes {
                    *byte = self.byte()?;
                }
                Some(f64::from_le_bytes(bytes))
            }
            1 => Some(1.0),
            2 => Some(0.0),
            _ => None,
        }
    }

    /// The object type of a DWG 2010 or later record.
    fn object_type(&mut self) -> Option<u16> {
        match self.bits(2)? {
            0 => self.byte().map(u16::from),
            1 => self.byte().map(|byte| 0x1F0 + u16::from(byte)),
            _ => self.raw_short(),
        }
    }

    /// A handle reference; the offset codes count from `reference`.
    fn handle(&mut self, reference: u64) -> Option<u64> {
        let form = self.byte()?;
        let mut value = 0u64;
        for _ in 0..form & 0x0F {
            value = value << 8 | u64::from(self.byte()?);
        }
        Some(match form >> 4 {
            0x6 => reference.checked_add(1)?,
            0x8 => reference.checked_sub(1)?,
            0xA => reference.checked_add(value)?,
            0xC => reference.checked_sub(value)?,
            _ => value,
        })
    }
}

#[cfg(test)]
mod tests;
