//! PDF layers (optional-content groups): what a file declares, and a source
//! whose layer visibility is overridden for an underlay.
//!
//! The override is an incremental update that replaces the catalog's default
//! optional-content configuration, so the rasteriser and the vector reader
//! both skip the hidden layers without any change to how they read a page.

use std::sync::Arc;

use hayro::hayro_syntax::object::{Array, Dict, Name, ObjectIdentifier};
use hayro::hayro_syntax::Pdf;

use super::pdf_raster::{register_derived_source, source_bytes};

/// An optional-content group (layer) of a PDF.
#[derive(Clone, Debug, PartialEq)]
pub struct PdfLayer {
    pub name: String,
    /// Shown by the file's default configuration.
    pub on_by_default: bool,
    object: (i32, i32),
}

fn text_string(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    bytes.iter().map(|&b| b as char).collect()
}

/// The layers a PDF declares, in file order.
pub fn layers(path: &str) -> Vec<PdfLayer> {
    let Some(bytes) = source_bytes(path) else {
        return Vec::new();
    };
    let Ok(pdf) = Pdf::new(bytes) else {
        return Vec::new();
    };
    let xref = pdf.xref();
    let Some(catalog) = xref.get::<Dict<'_>>(xref.root_id()) else {
        return Vec::new();
    };
    let Some(props) = catalog.get::<Dict<'_>>(b"OCProperties") else {
        return Vec::new();
    };
    let config = props.get::<Dict<'_>>(b"D").unwrap_or_default();
    let ids = |key: &[u8]| -> Vec<(i32, i32)> {
        config
            .get::<Array<'_>>(key)
            .map(|a| {
                a.raw_iter()
                    .filter_map(|item| item.as_obj_ref())
                    .map(|r| (r.obj_number, r.gen_number))
                    .collect()
            })
            .unwrap_or_default()
    };
    let base_off = config
        .get::<Name<'_>>(b"BaseState")
        .is_some_and(|n| n.as_ref() == b"OFF");
    let (on, off) = (ids(b"ON"), ids(b"OFF"));
    let Some(ocgs) = props.get::<Array<'_>>(b"OCGs") else {
        return Vec::new();
    };
    ocgs.raw_iter()
        .filter_map(|item| item.as_obj_ref())
        .filter_map(|r| {
            let id = (r.obj_number, r.gen_number);
            let dict = xref.get::<Dict<'_>>(ObjectIdentifier::from(r))?;
            let name = dict
                .get::<hayro::hayro_syntax::object::String<'_>>(b"Name")
                .map(|s| text_string(s.as_bytes()))
                .unwrap_or_default();
            let on_by_default = if base_off {
                on.contains(&id)
            } else {
                !off.contains(&id)
            };
            Some(PdfLayer {
                name,
                on_by_default,
                object: id,
            })
        })
        .collect()
}

/// A source whose `hidden` layers are off and every other layer on: the
/// original path when that is the file's own state, else a derived source
/// name registered with the updated bytes. Falls back to the original path
/// when the file cannot be updated (encrypted, unreadable catalog).
pub fn source_with_layers(path: &str, hidden: &[String]) -> String {
    let all = layers(path);
    if all.is_empty() {
        return path.to_string();
    }
    let off: Vec<&PdfLayer> = all
        .iter()
        .filter(|l| hidden.iter().any(|h| *h == l.name))
        .collect();
    if off.is_empty() && all.iter().all(|l| l.on_by_default) {
        return path.to_string();
    }
    let mut names: Vec<&str> = off.iter().map(|l| l.name.as_str()).collect();
    names.sort_unstable();
    let key = format!("{path}#layers-off={}", names.join("|"));
    if source_bytes(&key).is_some() {
        return key;
    }
    let Some(updated) = source_bytes(path).and_then(|bytes| with_layer_config(&bytes, &all, &off))
    else {
        return path.to_string();
    };
    register_derived_source(&key, Arc::new(updated));
    key
}

/// The bytes with an incremental update whose catalog lists `off` layers
/// as off in the default configuration and all others on.
fn with_layer_config(bytes: &[u8], all: &[PdfLayer], off: &[&PdfLayer]) -> Option<Vec<u8>> {
    let pdf = Pdf::new(Arc::new(bytes.to_vec())).ok()?;
    let xref = pdf.xref();
    let root = xref.root_id();
    let catalog = xref.get::<Dict<'_>>(root)?;
    let raw = std::str::from_utf8(catalog.data()).ok()?.trim().to_string();
    let tail = String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(4096)..]).into_owned();
    if tail.contains("/Encrypt") {
        return None;
    }
    let prev: usize = tail
        .rfind("startxref")
        .and_then(|i| tail[i + 9..].split_whitespace().next()?.parse().ok())?;
    let size: usize = {
        let region = String::from_utf8_lossy(&bytes[prev.min(bytes.len())..]).into_owned();
        let region = if region.contains("/Size") { region } else { tail.clone() };
        let i = region.find("/Size")?;
        region[i + 5..]
            .trim_start()
            .split(|c: char| !c.is_ascii_digit())
            .next()?
            .parse()
            .ok()?
    };
    let body = without_key(&raw, "/OCProperties")?;
    let refs = |layers: Vec<&PdfLayer>| {
        layers
            .iter()
            .map(|l| format!("{} {} R", l.object.0, l.object.1))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let props = format!(
        "/OCProperties << /OCGs [{}] /D << /BaseState /ON /OFF [{}] >> >>",
        refs(all.iter().collect()),
        refs(off.to_vec()),
    );
    let open = body.trim_end().strip_suffix(">>")?.trim_end();
    let catalog_text = format!("{open} {props} >>");
    let mut out = bytes.to_vec();
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    let object_offset = out.len();
    out.extend_from_slice(
        format!(
            "{} {} obj\n{catalog_text}\nendobj\n",
            root.obj_number, root.gen_number
        )
        .as_bytes(),
    );
    let xref_offset = out.len();
    out.extend_from_slice(
        format!(
            "xref\n0 1\n0000000000 65535 f \n{} 1\n{:010} {:05} n \ntrailer\n<< /Size {size} /Root {} {} R /Prev {prev} >>\nstartxref\n{xref_offset}\n%%EOF\n",
            root.obj_number, object_offset, root.gen_number, root.obj_number, root.gen_number,
        )
        .as_bytes(),
    );
    Some(out)
}

/// A dictionary's text without one key and its value (a dictionary, an
/// array, a reference or a single token).
fn without_key(dict: &str, key: &str) -> Option<String> {
    let Some(start) = dict.find(key) else {
        return Some(dict.to_string());
    };
    let rest = &dict[start + key.len()..];
    let value = rest.trim_start();
    let lead = rest.len() - value.len();
    let length = if value.starts_with("<<") || value.starts_with('[') {
        let (open, close) = if value.starts_with('[') { ("[", "]") } else { ("<<", ">>") };
        let mut depth = 0i32;
        let mut i = 0;
        loop {
            if i >= value.len() {
                return None;
            }
            if value[i..].starts_with(open) {
                depth += 1;
                i += open.len();
            } else if value[i..].starts_with(close) {
                depth -= 1;
                i += close.len();
                if depth == 0 {
                    break i;
                }
            } else {
                i += value[i..].chars().next()?.len_utf8();
            }
        }
    } else {
        let parts: Vec<&str> = value.split_whitespace().take(3).collect();
        if parts.len() == 3 && parts[2].starts_with('R') {
            value.find('R')? + 1
        } else {
            value
                .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
                .unwrap_or(value.len())
        }
    };
    Some(format!("{}{}", &dict[..start], &rest[lead + length..]))
}

/// Layer names an underlay has switched off: the strings of its
/// `AdeskUnderlayLayerOverrideData` extended data, as the reference stores
/// them.
pub const LAYER_OVERRIDE_APP: &str = "AdeskUnderlayLayerOverrideData";

pub fn hidden_layers(u: &codec::entities::Underlay) -> Vec<String> {
    u.common
        .extended_data
        .records()
        .iter()
        .filter(|r| r.application_name.eq_ignore_ascii_case(LAYER_OVERRIDE_APP))
        .flat_map(|r| r.values.iter())
        .filter_map(|v| match v {
            codec::xdata::XDataValue::String(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

/// The source an underlay's page is read from, with its layer overrides.
pub fn underlay_source(u: &codec::entities::Underlay, file_path: &str) -> String {
    let hidden = hidden_layers(u);
    if hidden.is_empty() {
        return file_path.to_string();
    }
    source_with_layers(file_path, &hidden)
}
