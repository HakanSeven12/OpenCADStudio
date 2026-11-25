//! BricsCAD's parameter objects as DWG records (2010 and later), decoded so
//! that encoding them again gives the same bits, and encoded after a change.
//!
//! A record holds the common object header (type, handle, extended data,
//! owner, reactors, extension dictionary) and a body. Most of BricsCAD's
//! classes hold a plain list of named properties; three hold a few values
//! around such a list:
//!
//! - `BDMDBCONSTRAINTMANAGER`: five counts, the objects and child managers it
//!   owns, then its settings;
//! - `BDMOBJECT`: a byte and a count, the object it stands for, its
//!   properties, a count;
//! - `BDMDBOBJECTDEPENDENCY`: its (empty) properties, a count, the object it
//!   depends on.
//!
//! In the data stream a property list is a count and, per property, a type
//! byte and the value. Names and text sit in the string stream and references
//! in the handle stream, both in the order the data stream names them.

use document::{DxfVersion, Handle};

/// A reference through the handle stream and the code it is written with:
/// a reference type (2 to 5) or one of the forms relative to the record's own
/// handle (6, 8, 10, 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ref {
    pub(crate) handle: Handle,
    pub(crate) code: u8,
}

impl Ref {
    pub(crate) fn new(handle: Handle, code: u8) -> Self {
        Self { handle, code }
    }
}

/// A property value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Text(String),
    Integer(i32),
    Real(f64),
    Flag(bool),
    Byte(u8),
    /// Bytes in the data stream; BricsCAD stores some handles this way.
    Raw(Vec<u8>),
    /// A reference: a byte in the data stream, the handle in the handle stream.
    Object {
        kind: u8,
        target: Ref,
    },
    List {
        element: u8,
        items: Vec<Value>,
    },
    Map {
        key: u8,
        value: u8,
        entries: Vec<(Value, Value)>,
    },
}

impl Value {
    fn kind(&self) -> u8 {
        match self {
            Value::Text(_) => 2,
            Value::Integer(_) => 3,
            Value::Real(_) => 4,
            Value::Flag(_) => 5,
            Value::Byte(_) => 6,
            Value::Raw(_) => 7,
            Value::Object { .. } => 8,
            Value::List { .. } => 9,
            Value::Map { .. } => 10,
        }
    }

    pub(crate) fn text(&self) -> Option<&str> {
        match self {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }

    pub(crate) fn object(&self) -> Option<Handle> {
        match self {
            Value::Object { target, .. } => Some(target.handle),
            _ => None,
        }
    }

    pub(crate) fn items(&self) -> Option<&[Value]> {
        match self {
            Value::List { items, .. } => Some(items),
            _ => None,
        }
    }

    pub(crate) fn entries(&self) -> Option<&[(Value, Value)]> {
        match self {
            Value::Map { entries, .. } => Some(entries),
            _ => None,
        }
    }

    /// Every reference in the value, in the order they are written.
    fn refs_mut(&mut self, visit: &mut impl FnMut(&mut Ref)) {
        match self {
            Value::Object { target, .. } => visit(target),
            Value::List { items, .. } => items.iter_mut().for_each(|item| item.refs_mut(visit)),
            Value::Map { entries, .. } => entries.iter_mut().for_each(|(key, value)| {
                key.refs_mut(visit);
                value.refs_mut(visit);
            }),
            _ => {}
        }
    }
}

/// The named properties of a record, in stored order.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Properties(pub(crate) Vec<(String, Value)>);

impl Properties {
    pub(crate) fn get(&self, name: &str) -> Option<&Value> {
        self.0
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    pub(crate) fn get_mut(&mut self, name: &str) -> Option<&mut Value> {
        self.0
            .iter_mut()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    pub(crate) fn text(&self, name: &str) -> Option<&str> {
        self.get(name)?.text()
    }

    /// Replace the value of an existing property; `false` when there is none.
    pub(crate) fn set(&mut self, name: &str, value: Value) -> bool {
        match self.get_mut(name) {
            Some(slot) => {
                *slot = value;
                true
            }
            None => false,
        }
    }

    fn refs_mut(&mut self, visit: &mut impl FnMut(&mut Ref)) {
        for (_, value) in &mut self.0 {
            value.refs_mut(visit);
        }
    }
}

/// What a record holds after the common header.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Body {
    Properties(Properties),
    /// `BDMDBCONSTRAINTMANAGER`. `counts[1]` objects and `counts[3]` child
    /// managers, in that order in `children`.
    ConstraintManager {
        counts: [i32; 5],
        children: Vec<Ref>,
        settings: Properties,
    },
    /// `BDMOBJECT`.
    Object {
        lead: u8,
        lead_count: i32,
        entity: Ref,
        properties: Properties,
        tail: i32,
    },
    /// `BDMDBOBJECTDEPENDENCY`.
    Dependency {
        properties: Properties,
        tail: i32,
        entity: Ref,
    },
}

impl Body {
    pub(crate) fn properties(&self) -> &Properties {
        match self {
            Body::Properties(properties)
            | Body::Object { properties, .. }
            | Body::Dependency { properties, .. } => properties,
            Body::ConstraintManager { settings, .. } => settings,
        }
    }

    pub(crate) fn properties_mut(&mut self) -> &mut Properties {
        match self {
            Body::Properties(properties)
            | Body::Object { properties, .. }
            | Body::Dependency { properties, .. } => properties,
            Body::ConstraintManager { settings, .. } => settings,
        }
    }

    /// Every reference of the body, in the order they are written.
    fn refs_mut(&mut self, visit: &mut impl FnMut(&mut Ref)) {
        match self {
            Body::Properties(properties) => properties.refs_mut(visit),
            Body::ConstraintManager {
                children, settings, ..
            } => {
                children.iter_mut().for_each(&mut *visit);
                settings.refs_mut(visit);
            }
            Body::Object {
                entity, properties, ..
            } => {
                visit(entity);
                properties.refs_mut(visit);
            }
            Body::Dependency {
                properties, entity, ..
            } => {
                properties.refs_mut(visit);
                visit(entity);
            }
        }
    }
}

/// One record of a BricsCAD parameter object.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Record {
    object_type: u16,
    pub(crate) handle: Handle,
    handle_code: u8,
    extended_data: Vec<(Ref, Vec<u8>)>,
    pub(crate) owner: Ref,
    pub(crate) reactors: Vec<Ref>,
    pub(crate) extension_dictionary: Option<Ref>,
    binary_data: bool,
    pub(crate) body: Body,
    pub(crate) version: DxfVersion,
}

impl Record {
    /// A new record of the class with DWG type number `object_type`; no
    /// extended data, reactors or extension dictionary.
    pub(crate) fn new(
        object_type: u16,
        handle: Handle,
        owner: Ref,
        body: Body,
        version: DxfVersion,
    ) -> Self {
        Self {
            object_type,
            handle,
            handle_code: 0,
            extended_data: Vec::new(),
            owner,
            reactors: Vec::new(),
            extension_dictionary: None,
            binary_data: false,
            body,
            version,
        }
    }

    /// A new record of the class of `template`, which gives the type number
    /// and the version; no extended data, reactors or extension dictionary.
    pub(crate) fn like(template: &Record, handle: Handle, owner: Ref, body: Body) -> Self {
        Self {
            object_type: template.object_type,
            handle,
            handle_code: template.handle_code,
            extended_data: Vec::new(),
            owner,
            reactors: Vec::new(),
            extension_dictionary: None,
            binary_data: false,
            body,
            version: template.version,
        }
    }

    /// Every reference of the record: owner, reactors, extension dictionary
    /// and those of the body. Extended data names applications, which are
    /// left alone.
    pub(crate) fn refs_mut(&mut self, mut visit: impl FnMut(&mut Ref)) {
        visit(&mut self.owner);
        self.reactors.iter_mut().for_each(&mut visit);
        if let Some(dictionary) = &mut self.extension_dictionary {
            visit(dictionary);
        }
        self.body.refs_mut(&mut visit);
    }
}

/// Decode the record of a `class` object: the merged data, string and handle
/// streams as the DWG reader keeps them, without the size prefixes and the
/// CRC. `None` for a DWG before 2010 and a record that does not decode in
/// full: any data or string bit left over, or more than padding left in the
/// handle stream.
pub(crate) fn decode(
    class: &str,
    data: &[u8],
    handle_bits: i64,
    version: DxfVersion,
) -> Option<Record> {
    if version < DxfVersion::AC1024 {
        return None;
    }
    let total = data.len().checked_mul(8)?;
    let handles_at = total.checked_sub(usize::try_from(handle_bits).ok()?)?;
    let mut main = Bits::new(data, 0, handles_at);
    let object_type = main.object_type()?;
    let (handle_code, own) = main.handle(0)?;
    let mut extended_data = Vec::new();
    loop {
        let size = main.bit_short()?;
        if size < 0 {
            return None;
        }
        if size == 0 {
            break;
        }
        let (code, application) = main.handle(0)?;
        let bytes = (0..size).map(|_| main.byte()).collect::<Option<Vec<_>>>()?;
        extended_data.push((Ref::new(Handle::from(application), code), bytes));
    }
    let reactor_count = usize::try_from(main.bit_long()?).ok()?;
    if reactor_count > total {
        return None;
    }
    let has_extension_dictionary = !main.bit()?;
    let binary_data = version >= DxfVersion::AC1027 && main.bit()?;

    let (strings_at, strings_end) = string_stream(data, handles_at)?;
    if main.pos > strings_at {
        return None;
    }
    main.end = strings_at;
    let mut handles = Bits::new(data, handles_at, total);
    let reference = |handles: &mut Bits| {
        handles
            .handle(own)
            .map(|(code, handle)| Ref::new(Handle::from(handle), code))
    };
    let owner = reference(&mut handles)?;
    let reactors = (0..reactor_count)
        .map(|_| reference(&mut handles))
        .collect::<Option<Vec<_>>>()?;
    let extension_dictionary = if has_extension_dictionary {
        Some(reference(&mut handles)?)
    } else {
        None
    };
    let mut reader = Reader {
        main,
        strings: Bits::new(data, strings_at, strings_end),
        handles,
        own,
    };
    let body = reader.body(class)?;
    let complete = reader.main.pos == reader.main.end
        && reader.strings.pos == reader.strings.end
        && reader.handles.end - reader.handles.pos < 8;
    complete.then_some(Record {
        object_type,
        handle: Handle::from(own),
        handle_code,
        extended_data,
        owner,
        reactors,
        extension_dictionary,
        binary_data,
        body,
        version,
    })
}

/// Encode `record`: the merged streams and the bit count of its handle
/// stream, as the DWG writer takes a raw record.
pub(crate) fn encode(record: &Record) -> (Vec<u8>, i64) {
    let own = record.handle.value();
    let mut main = Writer::default();
    let strings = Writer::default();
    let mut handles = Writer::default();
    main.object_type(record.object_type);
    main.handle(record.handle_code, own, 0);
    for (application, bytes) in &record.extended_data {
        main.bit_short(bytes.len() as i16);
        main.handle(application.code, application.handle.value(), 0);
        for byte in bytes {
            main.byte(*byte);
        }
    }
    main.bit_short(0);
    main.bit_long(record.reactors.len() as i32);
    main.bit(record.extension_dictionary.is_none());
    if record.version >= DxfVersion::AC1027 {
        main.bit(record.binary_data);
    }
    handles.reference(record.owner, own);
    for reactor in &record.reactors {
        handles.reference(*reactor, own);
    }
    if let Some(dictionary) = record.extension_dictionary {
        handles.reference(dictionary, own);
    }
    let mut encoder = Encoder {
        main,
        strings,
        handles,
        own,
    };
    encoder.body(&record.body);
    let Encoder {
        mut main,
        strings,
        handles,
        ..
    } = encoder;
    let string_bits = strings.len;
    if string_bits > 0 {
        main.append(&strings);
        if string_bits < 0x8000 {
            main.raw_short(string_bits as u16);
        } else {
            main.raw_short((string_bits >> 15) as u16);
            main.raw_short((string_bits & 0x7FFF) as u16 | 0x8000);
        }
        main.bit(true);
    } else {
        main.bit(false);
    }
    let main_bits = main.len;
    main.append(&handles);
    let bytes = main.into_bytes();
    let handle_bits = (bytes.len() * 8 - main_bits) as i64;
    (bytes, handle_bits)
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
    fn body(&mut self, class: &str) -> Option<Body> {
        Some(match class {
            "BDMDBCONSTRAINTMANAGER" => {
                let mut counts = [0; 5];
                for count in &mut counts {
                    *count = self.main.bit_long()?;
                }
                // Only the objects and child managers are understood.
                if counts[2] != 0 || counts[4] != 0 {
                    return None;
                }
                let children =
                    usize::try_from(counts[1]).ok()? + usize::try_from(counts[3]).ok()?;
                if children > self.handles.end {
                    return None;
                }
                let children = (0..children)
                    .map(|_| self.reference())
                    .collect::<Option<Vec<_>>>()?;
                Body::ConstraintManager {
                    counts,
                    children,
                    settings: self.properties()?,
                }
            }
            "BDMOBJECT" => Body::Object {
                lead: self.main.byte()?,
                lead_count: self.main.bit_long()?,
                entity: self.reference()?,
                properties: self.properties()?,
                tail: self.main.bit_long()?,
            },
            "BDMDBOBJECTDEPENDENCY" => Body::Dependency {
                properties: self.properties()?,
                tail: self.main.bit_long()?,
                entity: self.reference()?,
            },
            _ => Body::Properties(self.properties()?),
        })
    }

    fn reference(&mut self) -> Option<Ref> {
        let (code, handle) = self.handles.handle(self.own)?;
        Some(Ref::new(Handle::from(handle), code))
    }

    fn properties(&mut self) -> Option<Properties> {
        let count = self.count()?;
        let mut properties = Vec::with_capacity(count.min(64));
        for _ in 0..count {
            let name = self.text()?;
            let kind = self.main.byte()?;
            properties.push((name, self.value(kind, 0)?));
        }
        Some(Properties(properties))
    }

    fn value(&mut self, kind: u8, depth: u8) -> Option<Value> {
        // Lists and maps nest a level or two; a deeper one is not a property
        // list.
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
            8 => Value::Object {
                kind: self.main.byte()?,
                target: self.reference()?,
            },
            9 => {
                let element = self.main.byte()?;
                let count = self.count()?;
                Value::List {
                    element,
                    items: (0..count)
                        .map(|_| self.value(element, depth + 1))
                        .collect::<Option<_>>()?,
                }
            }
            10 => {
                let key = self.main.byte()?;
                let value = self.main.byte()?;
                let count = self.count()?;
                Value::Map {
                    key,
                    value,
                    entries: (0..count)
                        .map(|_| Some((self.value(key, depth + 1)?, self.value(value, depth + 1)?)))
                        .collect::<Option<_>>()?,
                }
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

struct Encoder {
    main: Writer,
    strings: Writer,
    handles: Writer,
    own: u64,
}

impl Encoder {
    fn body(&mut self, body: &Body) {
        match body {
            Body::Properties(properties) => self.properties(properties),
            Body::ConstraintManager {
                counts,
                children,
                settings,
            } => {
                for count in counts {
                    self.main.bit_long(*count);
                }
                for child in children {
                    self.handles.reference(*child, self.own);
                }
                self.properties(settings);
            }
            Body::Object {
                lead,
                lead_count,
                entity,
                properties,
                tail,
            } => {
                self.main.byte(*lead);
                self.main.bit_long(*lead_count);
                self.handles.reference(*entity, self.own);
                self.properties(properties);
                self.main.bit_long(*tail);
            }
            Body::Dependency {
                properties,
                tail,
                entity,
            } => {
                self.properties(properties);
                self.main.bit_long(*tail);
                self.handles.reference(*entity, self.own);
            }
        }
    }

    fn properties(&mut self, properties: &Properties) {
        self.main.bit_long(properties.0.len() as i32);
        for (name, value) in &properties.0 {
            self.text(name);
            self.main.byte(value.kind());
            self.value(value);
        }
    }

    fn value(&mut self, value: &Value) {
        match value {
            Value::Text(text) => self.text(text),
            Value::Integer(integer) => self.main.bit_long(*integer),
            Value::Real(real) => self.main.bit_double(*real),
            Value::Flag(flag) => self.main.bit(*flag),
            Value::Byte(byte) => self.main.byte(*byte),
            Value::Raw(bytes) => {
                self.main.byte(bytes.len() as u8);
                for byte in bytes {
                    self.main.byte(*byte);
                }
            }
            Value::Object { kind, target } => {
                self.main.byte(*kind);
                self.handles.reference(*target, self.own);
            }
            Value::List { element, items } => {
                self.main.byte(*element);
                self.main.bit_long(items.len() as i32);
                for item in items {
                    self.value(item);
                }
            }
            Value::Map {
                key,
                value,
                entries,
            } => {
                self.main.byte(*key);
                self.main.byte(*value);
                self.main.bit_long(entries.len() as i32);
                for (key, value) in entries {
                    self.value(key);
                    self.value(value);
                }
            }
        }
    }

    fn text(&mut self, text: &str) {
        let units: Vec<u16> = text.encode_utf16().collect();
        self.strings.bit_short(units.len() as i16);
        for unit in units {
            self.strings.raw_short(unit);
        }
    }
}

/// A bit range of a record, read most significant bit first.
pub(super) struct Bits<'a> {
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

    /// A handle reference as its code and the handle; the offset forms count
    /// from `reference`.
    fn handle(&mut self, reference: u64) -> Option<(u8, u64)> {
        let form = self.byte()?;
        let mut value = 0u64;
        for _ in 0..form & 0x0F {
            value = value << 8 | u64::from(self.byte()?);
        }
        let code = form >> 4;
        let handle = match code {
            0x6 => reference.checked_add(1)?,
            0x8 => reference.checked_sub(1)?,
            0xA => reference.checked_add(value)?,
            0xC => reference.checked_sub(value)?,
            _ => value,
        };
        Some((code, handle))
    }
}

/// The bits of a record being written, most significant bit first.
#[derive(Default)]
struct Writer {
    bytes: Vec<u8>,
    len: usize,
}

impl Writer {
    fn bit(&mut self, bit: bool) {
        if self.len.is_multiple_of(8) {
            self.bytes.push(0);
        }
        if bit {
            let last = self.bytes.len() - 1;
            self.bytes[last] |= 0x80 >> (self.len % 8);
        }
        self.len += 1;
    }

    fn bits(&mut self, value: u32, count: u32) {
        for shift in (0..count).rev() {
            self.bit(value >> shift & 1 == 1);
        }
    }

    fn byte(&mut self, byte: u8) {
        self.bits(u32::from(byte), 8);
    }

    fn raw_short(&mut self, value: u16) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }

    fn bit_short(&mut self, value: i16) {
        match value {
            0 => self.bits(2, 2),
            256 => self.bits(3, 2),
            1..=255 => {
                self.bits(1, 2);
                self.byte(value as u8);
            }
            _ => {
                self.bits(0, 2);
                self.raw_short(value as u16);
            }
        }
    }

    fn bit_long(&mut self, value: i32) {
        match value {
            0 => self.bits(2, 2),
            1..=255 => {
                self.bits(1, 2);
                self.byte(value as u8);
            }
            _ => {
                self.bits(0, 2);
                for byte in value.to_le_bytes() {
                    self.byte(byte);
                }
            }
        }
    }

    fn bit_double(&mut self, value: f64) {
        if value.to_bits() == 1.0f64.to_bits() {
            self.bits(1, 2);
        } else if value.to_bits() == 0.0f64.to_bits() {
            self.bits(2, 2);
        } else {
            self.bits(0, 2);
            for byte in value.to_le_bytes() {
                self.byte(byte);
            }
        }
    }

    fn object_type(&mut self, value: u16) {
        if value <= 0xFF {
            self.bits(0, 2);
            self.byte(value as u8);
        } else if (0x1F0..=0x2EF).contains(&value) {
            self.bits(1, 2);
            self.byte((value - 0x1F0) as u8);
        } else {
            self.bits(2, 2);
            self.raw_short(value);
        }
    }

    /// A handle with `code`; an offset form is recomputed from `own`, and
    /// falls back to the matching other offset form when the handle moved.
    fn handle(&mut self, code: u8, handle: u64, own: u64) {
        let (code, value) = match code {
            0x6 | 0x8 | 0xA | 0xC => {
                let difference = i128::from(handle) - i128::from(own);
                match (code, difference) {
                    (0x6, 1) => (0x6, 0),
                    (0x8, -1) => (0x8, 0),
                    (_, difference) if difference > 0 => (0xA, difference as u64),
                    (_, difference) if difference < 0 => (0xC, difference.unsigned_abs() as u64),
                    _ => (0x4, handle),
                }
            }
            code => (code, handle),
        };
        let bytes: Vec<u8> = value
            .to_be_bytes()
            .into_iter()
            .skip_while(|byte| *byte == 0)
            .collect();
        self.byte(code << 4 | bytes.len() as u8);
        for byte in bytes {
            self.byte(byte);
        }
    }

    fn reference(&mut self, reference: Ref, own: u64) {
        self.handle(reference.code, reference.handle.value(), own);
    }

    fn append(&mut self, other: &Writer) {
        for index in 0..other.len {
            self.bit(other.bytes[index / 8] >> (7 - index % 8) & 1 == 1);
        }
    }

    fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}
