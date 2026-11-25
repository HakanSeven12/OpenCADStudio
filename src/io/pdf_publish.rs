//! One PDF from many drawings, written as it goes: the sheet-list PUBLISH
//! appends each drawing's rendered pages and forgets them, so a set of
//! thousands of sheets never sits in memory at once.
//!
//! Each appended document's pages and everything they use are renumbered
//! into the output file at once. The page tree, the bookmarks — one per
//! sheet, in sheet order, each opening its whole page — and the layer list
//! are written when the set is finished. Layers of the same name share one
//! optional-content group across all drawings, so a viewer's layer panel
//! lists each layer once and hides it on every sheet.

use lopdf::{Dictionary, Object, ObjectId, Stream, StringFormat};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// A multi-sheet PDF being written.
pub struct PublishPdf {
    out: std::io::BufWriter<std::fs::File>,
    temp_path: PathBuf,
    path: PathBuf,
    finished: bool,
    /// Bytes written so far: the offset the next object starts at.
    offset: u64,
    /// File offset of each object, by object number - 1.
    xref: Vec<u64>,
    /// The page tree root, numbered up front so pages can name their parent.
    pages_id: u32,
    pages: Vec<u32>,
    /// Bookmark title and page of each sheet.
    bookmarks: Vec<(String, u32)>,
    /// The pages links reach by name, and their names.
    destinations: Vec<(String, u32)>,
    /// Optional-content group of each layer name, in first-seen order.
    layers: Vec<u32>,
    layer_ids: HashMap<Vec<u8>, u32>,
    /// Streams already written that reference nothing - font programs,
    /// images - by their bytes' hash and length: another drawing's identical
    /// stream points there instead of being written again.
    shared: HashMap<(u64, usize), u32>,
}

impl PublishPdf {
    /// Start a PDF that replaces `path` once it is finished.
    pub fn create(path: &Path) -> Result<Self, String> {
        let temp_path = super::save_temp_path(path);
        let file = std::fs::File::create(&temp_path)
            .map_err(|error| format!("Cannot create {}: {error}", temp_path.display()))?;
        let mut pdf = Self {
            out: std::io::BufWriter::with_capacity(1 << 20, file),
            temp_path,
            path: path.to_path_buf(),
            finished: false,
            offset: 0,
            xref: Vec::new(),
            pages_id: 0,
            pages: Vec::new(),
            bookmarks: Vec::new(),
            destinations: Vec::new(),
            layers: Vec::new(),
            layer_ids: HashMap::new(),
            shared: HashMap::new(),
        };
        // A binary comment tells transfer tools the file is not text.
        pdf.write(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n")?;
        pdf.pages_id = pdf.reserve();
        Ok(pdf)
    }

    /// Pages written so far.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Append every page of `doc` in order, the page at index `i` bookmarked
    /// with `titles[i]` and named `destinations[i]` for links that open it.
    /// Returns the number of pages appended.
    pub fn append(
        &mut self,
        mut doc: lopdf::Document,
        titles: &[String],
        destinations: &[Option<String>],
    ) -> Result<usize, String> {
        doc.compress();
        let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
        // The drawing's layers: its optional-content groups by name.
        let mut layer_of: HashMap<ObjectId, Vec<u8>> = HashMap::new();
        if let Ok(groups) = doc
            .catalog()
            .and_then(|catalog| catalog.get(b"OCProperties"))
            .and_then(|properties| match properties {
                Object::Reference(id) => doc.get_dictionary(*id),
                other => other.as_dict(),
            })
            .and_then(|properties| properties.get(b"OCGs"))
            .and_then(Object::as_array)
        {
            for group in groups {
                let Ok(id) = group.as_reference() else {
                    continue;
                };
                let name = doc
                    .get_dictionary(id)
                    .and_then(|group| group.get(b"Name"))
                    .and_then(Object::as_str)
                    .map(<[u8]>::to_vec)
                    .unwrap_or_default();
                layer_of.insert(id, name);
            }
        }
        // Each page as it leaves its page tree: with what it inherits, and
        // the output tree as its parent.
        let mut page_dicts: HashMap<ObjectId, Dictionary> = HashMap::new();
        for &id in &pages {
            let Ok(page) = doc.get_dictionary(id) else {
                continue;
            };
            let mut page = page.clone();
            inherit_page_attributes(&doc, id, &mut page);
            page.remove(b"Parent");
            page_dicts.insert(id, page);
        }
        // Number everything the pages use.
        let mut renumber: HashMap<ObjectId, u32> = HashMap::new();
        let mut order: Vec<ObjectId> = Vec::new();
        let mut stack: Vec<ObjectId> = pages.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            if renumber.contains_key(&id) {
                continue;
            }
            if let Some(name) = layer_of.get(&id) {
                if let Some(&shared) = self.layer_ids.get(name) {
                    renumber.insert(id, shared);
                    continue;
                }
                let shared = self.reserve();
                self.layer_ids.insert(name.clone(), shared);
                self.layers.push(shared);
                renumber.insert(id, shared);
            } else if let Some(key) = doc.get_object(id).ok().and_then(leaf_stream_key) {
                if let Some(&shared) = self.shared.get(&key) {
                    renumber.insert(id, shared);
                    continue;
                }
                let number = self.reserve();
                self.shared.insert(key, number);
                renumber.insert(id, number);
            } else {
                let number = self.reserve();
                renumber.insert(id, number);
            }
            order.push(id);
            if let Some(page) = page_dicts.get(&id) {
                for (_, value) in page.iter().rev() {
                    collect_references(value, &mut stack);
                }
            } else if let Ok(object) = doc.get_object(id) {
                collect_references(object, &mut stack);
            }
        }
        let first_page = self.pages.len();
        for &id in &order {
            let mut object = match page_dicts.remove(&id) {
                Some(page) => Object::Dictionary(page),
                // A dangling reference still gets an object, so the
                // renumbered file stays consistent.
                None => doc.get_object(id).cloned().unwrap_or(Object::Null),
            };
            remap(&mut object, &renumber);
            if pages.contains(&id) {
                if let Object::Dictionary(page) = &mut object {
                    page.set("Parent", Object::Reference((self.pages_id, 0)));
                }
            }
            self.write_object(renumber[&id], &object)?;
        }
        for (index, id) in pages.iter().enumerate() {
            let number = renumber[id];
            self.pages.push(number);
            if let Some(title) = titles.get(index) {
                self.bookmarks.push((title.clone(), number));
            }
            if let Some(Some(name)) = destinations.get(index) {
                self.destinations.push((name.clone(), number));
            }
        }
        Ok(self.pages.len() - first_page)
    }

    /// Write the page tree, the bookmarks (unless `bookmarks` is false), the
    /// layer list and the cross-reference table, then move the file into
    /// place. Returns its size in bytes.
    pub fn finish(mut self, title: &str, bookmarks: bool) -> Result<u64, String> {
        if self.pages.is_empty() {
            return Err("No sheet could be published.".into());
        }
        let kids: Vec<Object> = self
            .pages
            .iter()
            .map(|&page| Object::Reference((page, 0)))
            .collect();
        let count = kids.len() as i64;
        let pages = Dictionary::from_iter([
            ("Type", Object::Name(b"Pages".to_vec())),
            ("Kids", Object::Array(kids)),
            ("Count", Object::Integer(count)),
        ]);
        self.write_object(self.pages_id, &Object::Dictionary(pages))?;

        let mut catalog = Dictionary::from_iter([
            ("Type", Object::Name(b"Catalog".to_vec())),
            ("Pages", Object::Reference((self.pages_id, 0))),
        ]);
        if bookmarks && !self.bookmarks.is_empty() {
            let outlines = self.write_bookmarks()?;
            catalog.set("Outlines", Object::Reference((outlines, 0)));
            catalog.set("PageMode", Object::Name(b"UseOutlines".to_vec()));
        }
        if !self.destinations.is_empty() {
            // A name tree lists its keys in order; a name links to its first page.
            let mut names = std::mem::take(&mut self.destinations);
            names.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            names.dedup_by(|a, b| a.0 == b.0);
            let mut list = Vec::with_capacity(names.len() * 2);
            for (name, page) in names {
                list.push(Object::String(name.into_bytes(), StringFormat::Literal));
                list.push(Object::Array(vec![
                    Object::Reference((page, 0)),
                    Object::Name(b"Fit".to_vec()),
                ]));
            }
            let tree = Dictionary::from_iter([("Names", Object::Array(list))]);
            catalog.set(
                "Names",
                Dictionary::from_iter([("Dests", Object::Dictionary(tree))]),
            );
        }
        if !self.layers.is_empty() {
            let groups: Vec<Object> = self
                .layers
                .iter()
                .map(|&group| Object::Reference((group, 0)))
                .collect();
            let defaults = Dictionary::from_iter([
                ("Order", Object::Array(groups.clone())),
                ("ON", Object::Array(groups.clone())),
                ("RBGroups", Object::Array(Vec::new())),
            ]);
            catalog.set(
                "OCProperties",
                Dictionary::from_iter([
                    ("OCGs", Object::Array(groups)),
                    ("D", Object::Dictionary(defaults)),
                ]),
            );
        }
        let catalog_id = self.reserve();
        self.write_object(catalog_id, &Object::Dictionary(catalog))?;
        let info = Dictionary::from_iter([
            ("Title", text_string(title)),
            ("Producer", Object::string_literal("Open CAD Studio")),
        ]);
        let info_id = self.reserve();
        self.write_object(info_id, &Object::Dictionary(info))?;

        let xref_offset = self.offset;
        let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", self.xref.len() + 1);
        for offset in &self.xref {
            table.push_str(&format!("{offset:010} 00000 n \n"));
        }
        let id = file_id(&self.path, self.pages.len(), xref_offset);
        table.push_str(&format!(
            "trailer\n<</Size {}/Root {catalog_id} 0 R/Info {info_id} 0 R/ID[<{id}><{id}>]>>\n\
             startxref\n{xref_offset}\n%%EOF\n",
            self.xref.len() + 1
        ));
        self.write(table.as_bytes())?;
        self.out
            .flush()
            .map_err(|error| format!("Failed to write PDF data: {error}"))?;
        self.out
            .get_ref()
            .sync_all()
            .map_err(|error| format!("Failed to write PDF data: {error}"))?;
        super::replace_save_file(&self.temp_path, &self.path, false)
            .map_err(|error| format!("Failed to replace PDF file: {error}"))?;
        self.finished = true;
        super::remove_stale_save_temps(&self.path);
        Ok(self.offset)
    }

    /// The outline root and one item per sheet, flat and in sheet order.
    fn write_bookmarks(&mut self) -> Result<u32, String> {
        let root = self.reserve();
        let items: Vec<u32> = (0..self.bookmarks.len()).map(|_| self.reserve()).collect();
        let bookmarks = std::mem::take(&mut self.bookmarks);
        for (index, (title, page)) in bookmarks.iter().enumerate() {
            let mut item = Dictionary::from_iter([
                ("Title", text_string(title)),
                ("Parent", Object::Reference((root, 0))),
                (
                    "Dest",
                    Object::Array(vec![
                        Object::Reference((*page, 0)),
                        Object::Name(b"Fit".to_vec()),
                    ]),
                ),
            ]);
            if index > 0 {
                item.set("Prev", Object::Reference((items[index - 1], 0)));
            }
            if let Some(next) = items.get(index + 1) {
                item.set("Next", Object::Reference((*next, 0)));
            }
            self.write_object(items[index], &Object::Dictionary(item))?;
        }
        let outlines = Dictionary::from_iter([
            ("Type", Object::Name(b"Outlines".to_vec())),
            ("First", Object::Reference((items[0], 0))),
            ("Last", Object::Reference((items[items.len() - 1], 0))),
            ("Count", Object::Integer(items.len() as i64)),
        ]);
        self.write_object(root, &Object::Dictionary(outlines))?;
        Ok(root)
    }

    /// The next object number; its offset is recorded when it is written.
    fn reserve(&mut self) -> u32 {
        self.xref.push(0);
        self.xref.len() as u32
    }

    fn write_object(&mut self, number: u32, object: &Object) -> Result<(), String> {
        self.xref[number as usize - 1] = self.offset;
        let mut bytes = format!("{number} 0 obj\n").into_bytes();
        serialize(&mut bytes, object);
        bytes.extend_from_slice(b"\nendobj\n");
        self.write(&bytes)
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.out
            .write_all(bytes)
            .map_err(|error| format!("Failed to write PDF data: {error}"))?;
        self.offset += bytes.len() as u64;
        Ok(())
    }
}

impl Drop for PublishPdf {
    /// A set that is dropped unfinished leaves no partial file behind.
    fn drop(&mut self) {
        if !self.finished {
            let _ = std::fs::remove_file(&self.temp_path);
        }
    }
}

/// The identity of a stream worth sharing: one of at least 1 KiB that
/// references no other object, by the hash and length of its bytes.
fn leaf_stream_key(object: &Object) -> Option<(u64, usize)> {
    use std::hash::{Hash, Hasher};
    let Object::Stream(stream) = object else {
        return None;
    };
    if stream.content.len() < 1024 {
        return None;
    }
    let mut references = Vec::new();
    collect_references(&Object::Dictionary(stream.dict.clone()), &mut references);
    if !references.is_empty() {
        return None;
    }
    let mut bytes = Vec::with_capacity(stream.content.len() + 64);
    serialize(&mut bytes, object);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some((hasher.finish(), bytes.len()))
}

/// Queue the objects `object` references.
fn collect_references(object: &Object, stack: &mut Vec<ObjectId>) {
    match object {
        Object::Reference(id) => stack.push(*id),
        Object::Array(items) => {
            for item in items.iter().rev() {
                collect_references(item, stack);
            }
        }
        // Reversed onto the stack, so objects are numbered in the order
        // they are named.
        Object::Dictionary(dict) => {
            for (_, value) in dict.iter().rev() {
                collect_references(value, stack);
            }
        }
        Object::Stream(stream) => {
            for (_, value) in stream.dict.iter().rev() {
                collect_references(value, stack);
            }
        }
        _ => {}
    }
}

/// Copy the attributes a page inherits from its page tree onto the page, as
/// it leaves that tree.
fn inherit_page_attributes(doc: &lopdf::Document, page_id: ObjectId, page: &mut Dictionary) {
    let mut parent = page.get(b"Parent").and_then(Object::as_reference).ok();
    let mut seen = vec![page_id];
    while let Some(id) = parent.filter(|id| !seen.contains(id)) {
        seen.push(id);
        let Ok(node) = doc.get_dictionary(id) else {
            break;
        };
        for key in [&b"Resources"[..], b"MediaBox", b"CropBox", b"Rotate"] {
            if !page.has(key) {
                if let Ok(value) = node.get(key) {
                    page.set(key.to_vec(), value.clone());
                }
            }
        }
        parent = node.get(b"Parent").and_then(Object::as_reference).ok();
    }
}

/// Point every reference in `object` at its number in the output.
fn remap(object: &mut Object, renumber: &HashMap<ObjectId, u32>) {
    match object {
        Object::Reference(id) => {
            // Everything reachable from the pages was numbered.
            match renumber.get(id) {
                Some(&number) => *id = (number, 0),
                None => *object = Object::Null,
            }
        }
        Object::Array(items) => {
            for item in items {
                remap(item, renumber);
            }
        }
        Object::Dictionary(dict) => {
            for (_, value) in dict.iter_mut() {
                remap(value, renumber);
            }
        }
        Object::Stream(stream) => {
            for (_, value) in stream.dict.iter_mut() {
                remap(value, renumber);
            }
        }
        _ => {}
    }
}

/// A PDF text string: plain ASCII as it is, anything else as UTF-16BE with
/// its byte-order mark.
fn text_string(text: &str) -> Object {
    if text.bytes().all(|byte| (0x20..0x7F).contains(&byte)) {
        Object::String(text.as_bytes().to_vec(), StringFormat::Literal)
    } else {
        let mut bytes = vec![0xFE, 0xFF];
        bytes.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
        Object::String(bytes, StringFormat::Hexadecimal)
    }
}

/// A file identifier from the output path, its size and its page count.
fn file_id(path: &Path, pages: usize, size: u64) -> String {
    use std::hash::{Hash, Hasher};
    let mut first = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut first);
    pages.hash(&mut first);
    size.hash(&mut first);
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| time.as_nanos())
        .unwrap_or_default()
        .hash(&mut first);
    let a = first.finish();
    a.hash(&mut first);
    let b = first.finish();
    format!("{a:016X}{b:016X}")
}

/// Write `object` in PDF syntax.
fn serialize(out: &mut Vec<u8>, object: &Object) {
    match object {
        Object::Null => out.extend_from_slice(b"null"),
        Object::Boolean(value) => out.extend_from_slice(if *value { b"true" } else { b"false" }),
        Object::Integer(value) => out.extend_from_slice(value.to_string().as_bytes()),
        // Rust prints floats without exponents, as PDF requires.
        Object::Real(value) if value.is_finite() => {
            out.extend_from_slice(value.to_string().as_bytes())
        }
        Object::Real(_) => out.push(b'0'),
        Object::Name(name) => serialize_name(out, name),
        Object::String(text, StringFormat::Literal) => {
            out.push(b'(');
            for &byte in text {
                match byte {
                    b'(' | b')' | b'\\' => out.extend_from_slice(&[b'\\', byte]),
                    b'\r' => out.extend_from_slice(b"\\r"),
                    _ => out.push(byte),
                }
            }
            out.push(b')');
        }
        Object::String(text, StringFormat::Hexadecimal) => {
            out.push(b'<');
            for byte in text {
                out.extend_from_slice(format!("{byte:02X}").as_bytes());
            }
            out.push(b'>');
        }
        Object::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b' ');
                }
                serialize(out, item);
            }
            out.push(b']');
        }
        Object::Dictionary(dict) => serialize_dictionary(out, dict),
        Object::Stream(Stream { dict, content, .. }) => {
            let mut dict = dict.clone();
            dict.set("Length", Object::Integer(content.len() as i64));
            serialize_dictionary(out, &dict);
            out.extend_from_slice(b"\nstream\n");
            out.extend_from_slice(content);
            out.extend_from_slice(b"\nendstream");
        }
        Object::Reference((number, generation)) => {
            out.extend_from_slice(format!("{number} {generation} R").as_bytes())
        }
    }
}

fn serialize_dictionary(out: &mut Vec<u8>, dict: &Dictionary) {
    out.extend_from_slice(b"<<");
    for (key, value) in dict.iter() {
        serialize_name(out, key);
        out.push(b' ');
        serialize(out, value);
    }
    out.extend_from_slice(b">>");
}

/// A name with white space, delimiters and bytes outside `!`..`~` written as
/// `#xx` escapes.
fn serialize_name(out: &mut Vec<u8>, name: &[u8]) {
    out.push(b'/');
    for &byte in name {
        if b" \t\n\r\x0C()<>[]{}/%#".contains(&byte) || !(33..=126).contains(&byte) {
            out.extend_from_slice(format!("#{byte:02X}").as_bytes());
        } else {
            out.push(byte);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-page document of `width` × `height` points whose page uses the
    /// layers `layers`.
    fn sheet(width: i64, height: i64, layers: &[&str]) -> lopdf::Document {
        let mut doc = lopdf::Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let mut groups = Vec::new();
        let mut properties = Dictionary::new();
        for (index, name) in layers.iter().enumerate() {
            let group = doc.add_object(Dictionary::from_iter([
                ("Type", Object::Name(b"OCG".to_vec())),
                ("Name", Object::string_literal(*name)),
            ]));
            groups.push(Object::Reference(group));
            properties.set(format!("L{index}"), Object::Reference(group));
        }
        let content = doc.add_object(Stream::new(
            Dictionary::new(),
            b"/OC /L0 BDC 0 0 m 10 10 l S EMC".to_vec(),
        ));
        // Resources and the sheet size come from the page tree, as a page may
        // inherit them.
        let page = doc.add_object(Dictionary::from_iter([
            ("Type", Object::Name(b"Page".to_vec())),
            ("Parent", Object::Reference(pages_id)),
            ("Contents", Object::Reference(content)),
        ]));
        doc.objects.insert(
            pages_id,
            Object::Dictionary(Dictionary::from_iter([
                ("Type", Object::Name(b"Pages".to_vec())),
                ("Kids", Object::Array(vec![Object::Reference(page)])),
                ("Count", Object::Integer(1)),
                (
                    "MediaBox",
                    Object::Array(vec![0.into(), 0.into(), width.into(), height.into()]),
                ),
                (
                    "Resources",
                    Object::Dictionary(Dictionary::from_iter([(
                        "Properties",
                        Object::Dictionary(properties),
                    )])),
                ),
            ])),
        );
        let catalog = doc.add_object(Dictionary::from_iter([
            ("Type", Object::Name(b"Catalog".to_vec())),
            ("Pages", Object::Reference(pages_id)),
            (
                "OCProperties",
                Object::Dictionary(Dictionary::from_iter([("OCGs", Object::Array(groups))])),
            ),
        ]));
        doc.trailer.set("Root", Object::Reference(catalog));
        doc
    }

    fn title(item: &Dictionary) -> String {
        let bytes = item.get(b"Title").and_then(Object::as_str).unwrap();
        match bytes.strip_prefix(&[0xFE, 0xFF]) {
            Some(utf16) => String::from_utf16(
                &utf16
                    .chunks(2)
                    .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
            None => String::from_utf8(bytes.to_vec()).unwrap(),
        }
    }

    #[test]
    fn drawings_become_one_pdf_with_a_bookmark_per_sheet_and_shared_layers() {
        let dir = std::env::temp_dir().join(format!("ocs-publish-pdf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("set.pdf");
        let mut pdf = PublishPdf::create(&path).unwrap();
        assert_eq!(
            pdf.append(
                sheet(1191, 842, &["0", "Walls"]),
                &["0001 - A3".into()],
                &[]
            )
            .unwrap(),
            1
        );
        assert_eq!(
            pdf.append(
                sheet(842, 595, &["Walls", "Doors"]),
                &["0002 - Übersicht".into()],
                &[Some("Sheet 2".into())]
            )
            .unwrap(),
            1
        );
        assert!(!path.exists(), "the PDF appears only when finished");
        let size = pdf.finish("Set", true).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), size);

        let doc = lopdf::Document::load(&path).unwrap();
        let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
        assert_eq!(pages.len(), 2);
        let boxes: Vec<Vec<i64>> = pages
            .iter()
            .map(|&id| {
                let page = doc.get_dictionary(id).unwrap();
                page.get(b"MediaBox")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_i64().unwrap())
                    .collect()
            })
            .collect();
        assert_eq!(boxes, vec![vec![0, 0, 1191, 842], vec![0, 0, 842, 595]]);

        let catalog = doc.catalog().unwrap();
        assert_eq!(
            catalog.get(b"PageMode").unwrap().as_name().unwrap(),
            b"UseOutlines"
        );
        let outlines = doc
            .get_dictionary(catalog.get(b"Outlines").unwrap().as_reference().unwrap())
            .unwrap();
        assert_eq!(outlines.get(b"Count").unwrap().as_i64().unwrap(), 2);
        let mut item_id = outlines.get(b"First").unwrap().as_reference().ok();
        let mut seen = Vec::new();
        while let Some(id) = item_id {
            let item = doc.get_dictionary(id).unwrap();
            let dest = item.get(b"Dest").unwrap().as_array().unwrap();
            seen.push((title(item), dest[0].as_reference().unwrap()));
            item_id = item.get(b"Next").and_then(Object::as_reference).ok();
        }
        assert_eq!(
            seen,
            vec![
                ("0001 - A3".to_string(), pages[0]),
                ("0002 - Übersicht".to_string(), pages[1])
            ]
        );

        // Links open the second sheet by its name.
        let names = catalog.get(b"Names").unwrap().as_dict().unwrap();
        let dests = names.get(b"Dests").unwrap().as_dict().unwrap();
        let list = dests.get(b"Names").unwrap().as_array().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].as_str().unwrap(), b"Sheet 2");
        assert_eq!(
            list[1].as_array().unwrap()[0].as_reference().unwrap(),
            pages[1]
        );

        // "Walls" is one group for both sheets; each page's properties name
        // groups the catalog lists.
        let properties = catalog.get(b"OCProperties").unwrap().as_dict().unwrap();
        let groups: Vec<ObjectId> = properties
            .get(b"OCGs")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|g| g.as_reference().unwrap())
            .collect();
        let names: Vec<Vec<u8>> = groups
            .iter()
            .map(|&g| {
                doc.get_dictionary(g)
                    .unwrap()
                    .get(b"Name")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_vec()
            })
            .collect();
        assert_eq!(
            names,
            vec![b"0".to_vec(), b"Walls".to_vec(), b"Doors".to_vec()]
        );
        for &page in &pages {
            let page = doc.get_dictionary(page).unwrap();
            let resources = page.get(b"Resources").unwrap().as_dict().unwrap();
            for (_, group) in resources
                .get(b"Properties")
                .unwrap()
                .as_dict()
                .unwrap()
                .iter()
            {
                assert!(groups.contains(&group.as_reference().unwrap()));
            }
            assert_eq!(
                page.get(b"Parent").unwrap().as_reference().unwrap(),
                catalog.get(b"Pages").unwrap().as_reference().unwrap()
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A stream that references nothing and comes again in another drawing -
    /// the same font program in every sheet - is written once.
    #[test]
    fn identical_streams_of_several_drawings_are_written_once() {
        let dir = std::env::temp_dir().join(format!("ocs-publish-shared-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("set.pdf");
        let program: Vec<u8> = (0..4096u32).map(|i| (i * 7 % 251) as u8).collect();
        let with_program = |bytes: &[u8]| {
            let mut doc = sheet(100, 100, &["0"]);
            let stream = doc
                .add_object(Stream::new(Dictionary::new(), bytes.to_vec()).with_compression(false));
            let page = doc.get_pages().into_values().next().unwrap();
            doc.get_dictionary_mut(page)
                .unwrap()
                .set("Thumb", Object::Reference(stream));
            doc
        };
        let mut pdf = PublishPdf::create(&path).unwrap();
        pdf.append(with_program(&program), &[], &[]).unwrap();
        pdf.append(with_program(&program), &[], &[]).unwrap();
        let mut other = program.clone();
        other[0] ^= 1;
        pdf.append(with_program(&other), &[], &[]).unwrap();
        pdf.finish("Set", false).unwrap();
        let doc = lopdf::Document::load(&path).unwrap();
        let thumbs: Vec<ObjectId> = doc
            .get_pages()
            .into_values()
            .map(|page| {
                doc.get_dictionary(page)
                    .unwrap()
                    .get(b"Thumb")
                    .unwrap()
                    .as_reference()
                    .unwrap()
            })
            .collect();
        assert_eq!(thumbs[0], thumbs[1], "the same program is shared");
        assert_ne!(thumbs[0], thumbs[2], "a different one is not");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_bookmarks_and_unfinished_sets_leave_no_file() {
        let dir = std::env::temp_dir().join(format!("ocs-publish-pdf2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("set.pdf");
        let mut pdf = PublishPdf::create(&path).unwrap();
        pdf.append(sheet(100, 100, &[]), &[], &[]).unwrap();
        drop(pdf);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);

        let mut pdf = PublishPdf::create(&path).unwrap();
        pdf.append(sheet(100, 100, &["0"]), &["x".into()], &[])
            .unwrap();
        pdf.finish("Set", false).unwrap();
        let doc = lopdf::Document::load(&path).unwrap();
        assert!(doc.catalog().unwrap().get(b"Outlines").is_err());
        assert_eq!(doc.get_pages().len(), 1);
        assert!(PublishPdf::create(&path)
            .unwrap()
            .finish("Set", true)
            .is_err());
        assert!(path.exists(), "a failed set keeps the previous file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_published_set_removes_the_temporary_file_of_a_stopped_publish() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("set.pdf");
        let stopped = crate::io::stopped_save_temp(&path);

        let mut pdf = PublishPdf::create(&path).unwrap();
        pdf.append(sheet(100, 100, &["0"]), &[], &[]).unwrap();
        pdf.finish("Set", false).unwrap();

        assert!(path.exists());
        assert!(!stopped.exists());
    }

    #[test]
    fn names_and_strings_are_escaped() {
        let mut out = Vec::new();
        serialize(
            &mut out,
            &Object::Array(vec![
                Object::Name(b"A B#".to_vec()),
                Object::string_literal("a(b)\\c\r"),
                Object::Real(0.5),
                Object::Real(f32::NAN),
            ]),
        );
        assert_eq!(out, b"[/A#20B#23 (a\\(b\\)\\\\c\\r) 0.5 0]".to_vec());
    }
}
