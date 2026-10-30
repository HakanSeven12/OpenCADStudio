//! Object hyperlinks, kept in the `PE_URL` extended data the way the format
//! groups them: `url { [text [location]] { flags } }`. The text comes before
//! the named location, and an empty text is written only when a location
//! follows it. The inner group's integer is 1 while DWG links are converted
//! to DWF.

use codec::xdata::XDataValue;
use codec::{Document, EntityType, Handle};

pub(crate) const APP: &str = "PE_URL";
/// Flag of the inner group: links to drawings point at their DWF.
pub(crate) const CONVERT_TO_DWF: i32 = 1;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Link {
    pub url: String,
    /// The text to display (the description).
    pub text: String,
    /// A named location: a view or a layout of the linked drawing, or of this
    /// one when `url` is empty.
    pub location: String,
    pub flags: i32,
}

impl Link {
    /// Reads the grouped record, and the flat `url, text` form older files
    /// carry.
    pub fn from_values(values: &[XDataValue]) -> Option<Self> {
        let mut link = Link::default();
        let mut strings = Vec::new();
        let mut depth = 0;
        let mut url = None;
        for value in values {
            match value {
                XDataValue::ControlString(s) if s.trim() == "{" => depth += 1,
                XDataValue::ControlString(s) if s.trim() == "}" => depth -= 1,
                XDataValue::String(s) if url.is_none() => url = Some(s.clone()),
                XDataValue::String(s) if depth <= 1 => strings.push(s.clone()),
                XDataValue::Integer32(n) if depth == 2 => link.flags = *n,
                XDataValue::Integer16(n) if depth == 2 => link.flags = i32::from(*n),
                _ => {}
            }
        }
        link.url = url?;
        let mut strings = strings.into_iter();
        link.text = strings.next().unwrap_or_default();
        link.location = strings.next().unwrap_or_default();
        Some(link)
    }

    pub fn of(entity: &EntityType) -> Option<Self> {
        Self::from_values(&entity.common().extended_data.get_record(APP)?.values)
    }

    pub fn values(&self) -> Vec<XDataValue> {
        let mut values = vec![
            XDataValue::String(self.url.clone()),
            XDataValue::ControlString("{".into()),
        ];
        if !self.location.is_empty() {
            values.push(XDataValue::String(self.text.clone()));
            values.push(XDataValue::String(self.location.clone()));
        } else if !self.text.is_empty() {
            values.push(XDataValue::String(self.text.clone()));
        }
        values.extend([
            XDataValue::ControlString("{".into()),
            XDataValue::Integer32(self.flags),
            XDataValue::ControlString("}".into()),
            XDataValue::ControlString("}".into()),
        ]);
        values
    }

    /// Neither an address nor a location: no link at all.
    pub fn is_empty(&self) -> bool {
        self.url.trim().is_empty() && self.location.trim().is_empty()
    }

    /// The address and named location as one entry: `url#location`.
    pub fn address(&self) -> String {
        if self.location.is_empty() {
            self.url.clone()
        } else {
            format!("{}#{}", self.url, self.location)
        }
    }

    /// Splits `url#location` back into its parts.
    pub fn split_address(address: &str) -> (String, String) {
        match address.trim().split_once('#') {
            Some((url, location)) => (url.trim().to_string(), location.trim().to_string()),
            None => (address.trim().to_string(), String::new()),
        }
    }

    /// What a Properties row shows: the text, else the address.
    pub fn label(&self) -> String {
        if !self.text.trim().is_empty() {
            self.text.clone()
        } else {
            self.address()
        }
    }
}

/// Writes `link` onto the object, or removes its hyperlink for `None`.
pub(crate) fn set(document: &mut Document, handle: Handle, link: Option<&Link>) {
    crate::scene::view::dispatch::set_entity_xdata(document, handle, APP, link.map(Link::values));
}

/// Replaces only the address of an object's hyperlink; its text, named
/// location and flags stay.
pub(crate) fn set_url(document: &mut Document, handle: Handle, url: &str) {
    let mut link = document
        .get_entity(handle)
        .and_then(Link::of)
        .unwrap_or_default();
    link.url = url.trim().to_string();
    if link.is_empty() {
        set(document, handle, None);
    } else {
        set(document, handle, Some(&link));
    }
}

