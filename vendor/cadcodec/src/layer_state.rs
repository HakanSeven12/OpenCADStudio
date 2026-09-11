//! Native AutoCAD layer-state storage.
//!
//! AutoCAD keeps named layer states as XRecords in the
//! `ACAD_LAYERSTATES` dictionary attached to the layer table's extension
//! dictionary.  This module exposes that object graph as semantic Rust data
//! and provides capture, restore, rename, and delete operations.

use std::ops::{BitOr, BitOrAssign};

use crate::objects::{
    Dictionary, DictionaryCloningFlags, ObjectType, XRecord, XRecordEntry, XRecordValue,
};
use crate::tables::normalize_name;
use crate::types::{Color, Handle, LineWeight, Transparency};
use crate::CadDocument;

const LAYER_STATES_DICTIONARY: &str = "ACAD_LAYERSTATES";

/// Layer properties selected for restoration from a named state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LayerStateMask(u32);

impl LayerStateMask {
    pub const NONE: Self = Self(0);
    pub const ON: Self = Self(0x0001);
    pub const FROZEN: Self = Self(0x0002);
    pub const LOCKED: Self = Self(0x0004);
    pub const PLOT: Self = Self(0x0008);
    pub const NEW_VIEWPORT: Self = Self(0x0010);
    pub const COLOR: Self = Self(0x0020);
    pub const LINE_TYPE: Self = Self(0x0040);
    pub const LINE_WEIGHT: Self = Self(0x0080);
    pub const PLOT_STYLE: Self = Self(0x0100);
    pub const CURRENT_VIEWPORT: Self = Self(0x0200);
    pub const TRANSPARENCY: Self = Self(0x0400);
    pub const LAST_RESTORED: Self = Self(0x1_0000);

    /// Properties represented by [`crate::tables::Layer`].
    pub const SUPPORTED: Self = Self(
        Self::ON.0
            | Self::FROZEN.0
            | Self::LOCKED.0
            | Self::PLOT.0
            | Self::NEW_VIEWPORT.0
            | Self::COLOR.0
            | Self::LINE_TYPE.0
            | Self::LINE_WEIGHT.0
            | Self::PLOT_STYLE.0
            | Self::TRANSPARENCY.0,
    );

    /// All standard AutoCAD layer-state properties.
    pub const ALL: Self = Self(0x07ff);

    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

impl BitOr for LayerStateMask {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for LayerStateMask {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Saved properties for one layer.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LayerStateLayer {
    pub layer_handle: Handle,
    pub layer_name: String,
    pub off: bool,
    pub frozen: bool,
    pub locked: bool,
    pub plottable: bool,
    pub new_viewport_frozen: bool,
    pub color: Color,
    #[cfg_attr(feature = "serde", serde(default))]
    pub color_name: Option<String>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub book_name: Option<String>,
    pub line_type: String,
    pub line_weight: LineWeight,
    pub plot_style: String,
    pub transparency: Option<Transparency>,
}

/// A named layer state stored in a drawing.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LayerState {
    pub name: String,
    pub description: String,
    pub mask: LayerStateMask,
    pub hidden: bool,
    pub current_layer: String,
    pub layers: Vec<LayerStateLayer>,
}

impl CadDocument {
    pub(crate) fn rename_layer_state_references(&mut self, old_key: &str, new_name: &str) {
        let Some(dictionary_handle) = self.layer_states_dictionary_handle() else {
            return;
        };
        let handles = match self.objects.get(&dictionary_handle) {
            Some(ObjectType::Dictionary(dictionary)) => dictionary
                .entries
                .iter()
                .map(|(_, handle)| *handle)
                .collect::<Vec<_>>(),
            _ => return,
        };
        for handle in handles {
            let Some(ObjectType::XRecord(xrecord)) = self.objects.get_mut(&handle) else {
                continue;
            };
            let header_end = xrecord
                .entries
                .iter()
                .position(|entry| entry.code == 330)
                .unwrap_or(xrecord.entries.len());
            let Some(entry) = xrecord.entries[..header_end].iter_mut().find(|entry| {
                entry.code == 302
                    && entry
                        .value
                        .as_string()
                        .is_some_and(|name| normalize_name(name) == old_key)
            }) else {
                continue;
            };
            entry.value = XRecordValue::String(new_name.to_string());
            xrecord.raw_data.clear();
            xrecord.raw_dwg_data = None;
        }
    }

    /// Return every native layer state stored in the drawing.
    pub fn layer_states(&self) -> Vec<LayerState> {
        let Some(dictionary_handle) = self.layer_states_dictionary_handle() else {
            return Vec::new();
        };
        let Some(ObjectType::Dictionary(dictionary)) = self.objects.get(&dictionary_handle) else {
            return Vec::new();
        };

        dictionary
            .entries
            .iter()
            .filter_map(|(name, handle)| {
                let ObjectType::XRecord(xrecord) = self.objects.get(handle)? else {
                    return None;
                };
                Some(self.decode_layer_state(name, xrecord))
            })
            .collect()
    }

    /// Return one native layer state by name (case-insensitive).
    pub fn layer_state(&self, name: &str) -> Option<LayerState> {
        self.layer_states()
            .into_iter()
            .find(|state| state.name.eq_ignore_ascii_case(name))
    }

    /// Capture the current layer table and store it under `name`.
    ///
    /// Existing states with the same name are updated in place.
    pub fn capture_layer_state(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Handle {
        let name = name.into();
        let current_layer = if self.header.current_layer_name.is_empty() {
            self.layers
                .iter()
                .find(|layer| layer.handle == self.header.current_layer_handle)
                .map(|layer| layer.name.clone())
                .unwrap_or_else(|| "0".to_string())
        } else {
            self.header.current_layer_name.clone()
        };
        let layers = self
            .layers
            .iter()
            .map(|layer| LayerStateLayer {
                layer_handle: layer.handle,
                layer_name: layer.name.clone(),
                off: layer.flags.off,
                frozen: layer.flags.frozen,
                locked: layer.flags.locked,
                plottable: layer.is_plottable,
                new_viewport_frozen: layer.flags.frozen_in_new_viewport,
                color: layer.color,
                color_name: layer.color_name.clone(),
                book_name: layer.book_name.clone(),
                line_type: layer.line_type.clone(),
                line_weight: layer.line_weight,
                plot_style: if layer.plot_style.is_empty() {
                    format!("Color_{}", layer.color.approximate_index())
                } else {
                    layer.plot_style.clone()
                },
                transparency: Some(layer.transparency),
            })
            .collect();

        self.store_layer_state(LayerState {
            name,
            description: description.into(),
            mask: LayerStateMask::SUPPORTED,
            hidden: false,
            current_layer,
            layers,
        })
    }

    /// Store a semantic layer state in the native AutoCAD dictionary/XRecord form.
    ///
    /// Existing states with the same name are updated in place.
    pub fn store_layer_state(&mut self, state: LayerState) -> Handle {
        let dictionary_handle = self.ensure_layer_states_dictionary();
        let existing_handle = self
            .dictionary_entry(dictionary_handle, &state.name)
            .map(|(_, handle)| handle);
        let xrecord_handle = existing_handle.unwrap_or_else(|| self.allocate_handle());
        let entries = self.encode_layer_state(&state);

        let mut xrecord = match self.objects.remove(&xrecord_handle) {
            Some(ObjectType::XRecord(xrecord)) => xrecord,
            _ => XRecord::named(&state.name),
        };
        xrecord.handle = xrecord_handle;
        xrecord.owner = dictionary_handle;
        xrecord.reactors = vec![dictionary_handle];
        xrecord.name = state.name.clone();
        xrecord.cloning_flags = DictionaryCloningFlags::KeepExisting;
        xrecord.entries = entries;
        xrecord.raw_data.clear();
        xrecord.raw_dwg_data = None;
        xrecord.raw_dwg_handle_bits = 0;
        xrecord.raw_dwg_version = None;
        self.objects
            .insert(xrecord_handle, ObjectType::XRecord(xrecord));

        if let Some(ObjectType::Dictionary(dictionary)) = self.objects.get_mut(&dictionary_handle) {
            if let Some((key, handle)) = dictionary
                .entries
                .iter_mut()
                .find(|(key, _)| key.eq_ignore_ascii_case(&state.name))
            {
                *key = state.name;
                *handle = xrecord_handle;
            } else {
                dictionary.entries.push((state.name, xrecord_handle));
            }
        }

        xrecord_handle
    }

    /// Restore the selected properties of a named state.
    pub fn restore_layer_state(&mut self, name: &str) -> Option<usize> {
        let state = self.layer_state(name)?;
        let mut restored = 0usize;

        for saved in &state.layers {
            let layer_name = if !saved.layer_handle.is_null() {
                self.layers
                    .iter()
                    .find(|layer| layer.handle == saved.layer_handle)
                    .map(|layer| layer.name.clone())
            } else {
                None
            }
            .unwrap_or_else(|| saved.layer_name.clone());
            let layer = self.layers.get_mut(&layer_name);

            let Some(layer) = layer else {
                continue;
            };
            if state.mask.contains(LayerStateMask::ON) {
                layer.flags.off = saved.off;
            }
            if state.mask.contains(LayerStateMask::FROZEN) {
                layer.flags.frozen = saved.frozen;
            }
            if state.mask.contains(LayerStateMask::LOCKED) {
                layer.flags.locked = saved.locked;
            }
            if state.mask.contains(LayerStateMask::PLOT) {
                layer.is_plottable = saved.plottable;
            }
            if state.mask.contains(LayerStateMask::NEW_VIEWPORT) {
                layer.flags.frozen_in_new_viewport = saved.new_viewport_frozen;
            }
            if state.mask.contains(LayerStateMask::COLOR) {
                layer.color = saved.color;
                layer.color_name.clone_from(&saved.color_name);
                layer.book_name.clone_from(&saved.book_name);
            }
            if state.mask.contains(LayerStateMask::LINE_TYPE) && !saved.line_type.is_empty() {
                layer.line_type.clone_from(&saved.line_type);
            }
            if state.mask.contains(LayerStateMask::LINE_WEIGHT) {
                layer.line_weight = saved.line_weight;
            }
            if state.mask.contains(LayerStateMask::PLOT_STYLE) {
                layer.plot_style.clone_from(&saved.plot_style);
            }
            if state.mask.contains(LayerStateMask::TRANSPARENCY) {
                if let Some(transparency) = saved.transparency {
                    layer.transparency = transparency;
                }
            }
            restored += 1;
        }

        if let Some(layer) = self.layers.get(&state.current_layer) {
            self.header.current_layer_handle = layer.handle;
            self.header.current_layer_name.clone_from(&layer.name);
        }

        Some(restored)
    }

    /// Rename a native layer state. Returns `false` when the source is missing
    /// or the destination already exists.
    pub fn rename_layer_state(&mut self, old_name: &str, new_name: &str) -> bool {
        let Some(dictionary_handle) = self.layer_states_dictionary_handle() else {
            return false;
        };
        if self.dictionary_entry(dictionary_handle, new_name).is_some()
            && !old_name.eq_ignore_ascii_case(new_name)
        {
            return false;
        }
        let Some(ObjectType::Dictionary(dictionary)) = self.objects.get_mut(&dictionary_handle)
        else {
            return false;
        };
        let Some((key, handle)) = dictionary
            .entries
            .iter_mut()
            .find(|(key, _)| key.eq_ignore_ascii_case(old_name))
        else {
            return false;
        };
        *key = new_name.to_string();
        let xrecord_handle = *handle;
        if let Some(ObjectType::XRecord(xrecord)) = self.objects.get_mut(&xrecord_handle) {
            xrecord.name = new_name.to_string();
        }
        true
    }

    /// Delete a native layer state.
    pub fn delete_layer_state(&mut self, name: &str) -> bool {
        let Some(dictionary_handle) = self.layer_states_dictionary_handle() else {
            return false;
        };
        let Some(ObjectType::Dictionary(dictionary)) = self.objects.get_mut(&dictionary_handle)
        else {
            return false;
        };
        let Some(index) = dictionary
            .entries
            .iter()
            .position(|(key, _)| key.eq_ignore_ascii_case(name))
        else {
            return false;
        };
        let (_, handle) = dictionary.entries.remove(index);
        self.objects.remove(&handle);
        true
    }

    fn layer_extension_dictionary_handle(&self) -> Option<Handle> {
        let table_handle = self.layers.handle();
        self.xdic_by_handle
            .get(&table_handle)
            .copied()
            .filter(|handle| self.objects.contains_key(handle))
            .or_else(|| {
                self.objects
                    .iter()
                    .find_map(|(handle, object)| match object {
                        ObjectType::Dictionary(dictionary) if dictionary.owner == table_handle => {
                            Some(*handle)
                        }
                        _ => None,
                    })
            })
    }

    fn layer_states_dictionary_handle(&self) -> Option<Handle> {
        let extension_handle = self.layer_extension_dictionary_handle()?;
        let Some(ObjectType::Dictionary(extension)) = self.objects.get(&extension_handle) else {
            return None;
        };
        extension
            .entries
            .iter()
            .find(|(key, _)| {
                key.eq_ignore_ascii_case(LAYER_STATES_DICTIONARY)
                    || key.eq_ignore_ascii_case("ACAD_LAYERSTATE")
            })
            .map(|(_, handle)| *handle)
    }

    fn ensure_layer_states_dictionary(&mut self) -> Handle {
        let table_handle = self.layers.handle();
        let extension_handle = self.layer_extension_dictionary_handle().unwrap_or_else(|| {
            let handle = self.allocate_handle();
            let mut dictionary = Dictionary::new();
            dictionary.handle = handle;
            dictionary.owner = table_handle;
            dictionary.hard_owner = true;
            self.objects
                .insert(handle, ObjectType::Dictionary(dictionary));
            handle
        });
        self.xdic_by_handle.insert(table_handle, extension_handle);

        if let Some(handle) = self.layer_states_dictionary_handle() {
            if matches!(self.objects.get(&handle), Some(ObjectType::Dictionary(_))) {
                return handle;
            }
        }

        let dictionary_handle = self.allocate_handle();
        let mut dictionary = Dictionary::new();
        dictionary.handle = dictionary_handle;
        dictionary.owner = extension_handle;
        dictionary.reactors = vec![extension_handle];
        self.objects
            .insert(dictionary_handle, ObjectType::Dictionary(dictionary));

        if let Some(ObjectType::Dictionary(extension)) = self.objects.get_mut(&extension_handle) {
            if let Some((key, handle)) = extension.entries.iter_mut().find(|(key, _)| {
                key.eq_ignore_ascii_case(LAYER_STATES_DICTIONARY)
                    || key.eq_ignore_ascii_case("ACAD_LAYERSTATE")
            }) {
                *key = LAYER_STATES_DICTIONARY.to_string();
                *handle = dictionary_handle;
            } else {
                extension
                    .entries
                    .push((LAYER_STATES_DICTIONARY.to_string(), dictionary_handle));
            }
        }
        dictionary_handle
    }

    fn dictionary_entry(&self, dictionary_handle: Handle, name: &str) -> Option<(String, Handle)> {
        let Some(ObjectType::Dictionary(dictionary)) = self.objects.get(&dictionary_handle) else {
            return None;
        };
        dictionary
            .entries
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .cloned()
    }

    fn decode_layer_state(&self, name: &str, xrecord: &XRecord) -> LayerState {
        let first_layer = xrecord
            .entries
            .iter()
            .position(|entry| entry.code == 330)
            .unwrap_or(xrecord.entries.len());
        let header = &xrecord.entries[..first_layer];
        let mask = header
            .iter()
            .find(|entry| entry.code == 91)
            .and_then(|entry| entry.value.as_i32())
            .map(|value| LayerStateMask::from_bits(value as u32))
            .unwrap_or(LayerStateMask::ALL);
        let description = header
            .iter()
            .find(|entry| entry.code == 301)
            .and_then(|entry| entry.value.as_string())
            .unwrap_or_default()
            .to_string();
        let hidden = header
            .iter()
            .find(|entry| entry.code == 290)
            .and_then(|entry| entry.value.as_bool())
            .unwrap_or(false);
        let current_layer = header
            .iter()
            .find(|entry| entry.code == 302)
            .and_then(|entry| entry.value.as_string())
            .unwrap_or_default()
            .to_string();

        let mut layers = Vec::new();
        let mut start = first_layer;
        while start < xrecord.entries.len() {
            let end = xrecord.entries[start + 1..]
                .iter()
                .position(|entry| entry.code == 330)
                .map(|offset| start + 1 + offset)
                .unwrap_or(xrecord.entries.len());
            let group = &xrecord.entries[start..end];
            if let Some(layer) = self.decode_layer_state_layer(group) {
                layers.push(layer);
            }
            start = end;
        }

        LayerState {
            name: name.to_string(),
            description,
            mask,
            hidden,
            current_layer,
            layers,
        }
    }

    fn decode_layer_state_layer(&self, entries: &[XRecordEntry]) -> Option<LayerStateLayer> {
        let layer_handle = entries
            .first()
            .filter(|entry| entry.code == 330)?
            .value
            .as_handle()?;
        let flags = entry_i32(entries, 90).unwrap_or(0);
        let color = entry_i32(entries, 92)
            .or_else(|| entry_i32(entries, 420))
            .map(Color::from_true_color_value)
            .unwrap_or_else(|| Color::from_index(entry_i32(entries, 62).unwrap_or(7) as i16));
        let (book_name, color_name) = entry_string(entries, 300)
            .map(crate::io::dxf::split_color_book_name)
            .unwrap_or((None, None));
        let line_type_handle = entry_handle(entries, 331).unwrap_or(Handle::NULL);
        let layer_name = self
            .layers
            .iter()
            .find(|layer| layer.handle == layer_handle)
            .map(|layer| layer.name.clone())
            .unwrap_or_default();
        let line_type = self
            .line_types
            .iter()
            .find(|line_type| line_type.handle == line_type_handle)
            .map(|line_type| line_type.name.clone())
            .unwrap_or_default();

        Some(LayerStateLayer {
            layer_handle,
            layer_name,
            off: flags & 0x01 != 0,
            frozen: flags & 0x02 != 0,
            locked: flags & 0x04 != 0,
            plottable: flags & 0x08 != 0,
            new_viewport_frozen: flags & 0x10 != 0,
            color,
            color_name,
            book_name,
            line_type,
            line_weight: LineWeight::from_value(entry_i32(entries, 370).unwrap_or(-3) as i16),
            plot_style: entry_string(entries, 1).unwrap_or_default().to_string(),
            transparency: entry_i32(entries, 440)
                .map(|value| Transparency::from_alpha_value(value as u32)),
        })
    }

    fn encode_layer_state(&self, state: &LayerState) -> Vec<XRecordEntry> {
        let mut entries = vec![
            XRecordEntry::int32(91, state.mask.bits() as i32),
            XRecordEntry::string(301, &state.description),
            XRecordEntry::bool(290, state.hidden),
            XRecordEntry::string(302, &state.current_layer),
        ];

        for layer in &state.layers {
            let layer_handle = if layer.layer_handle.is_null() {
                self.layers
                    .get(&layer.layer_name)
                    .map(|layer| layer.handle)
                    .unwrap_or(Handle::NULL)
            } else {
                layer.layer_handle
            };
            if layer_handle.is_null() {
                continue;
            }

            let mut flags = 0i32;
            if layer.off {
                flags |= 0x01;
            }
            if layer.frozen {
                flags |= 0x02;
            }
            if layer.locked {
                flags |= 0x04;
            }
            if layer.plottable {
                flags |= 0x08;
            }
            if layer.new_viewport_frozen {
                flags |= 0x10;
            }

            let line_type_handle = self
                .line_types
                .iter()
                .find(|line_type| line_type.name.eq_ignore_ascii_case(&layer.line_type))
                .map(|line_type| line_type.handle)
                .or_else(|| {
                    self.line_types
                        .get("Continuous")
                        .map(|line_type| line_type.handle)
                })
                .unwrap_or(Handle::NULL);
            entries.push(XRecordEntry::handle(330, layer_handle));
            entries.push(XRecordEntry::int32(90, flags));
            entries.push(XRecordEntry::int16(62, layer.color.approximate_index()));
            if let Some(true_color) = layer.color.to_true_color_value() {
                entries.push(XRecordEntry::int32(
                    92,
                    (0xC200_0000u32 | true_color as u32) as i32,
                ));
            }
            entries.push(XRecordEntry::int16(370, layer.line_weight.value()));
            entries.push(XRecordEntry::handle(331, line_type_handle));
            entries.push(XRecordEntry::string(1, &layer.plot_style));
            if let Some(transparency) = layer.transparency {
                entries.push(XRecordEntry::int32(440, transparency.to_dxf_value()));
            }
            if let Some(name) = crate::io::dxf::join_color_book_name(
                layer.book_name.as_deref(),
                layer.color_name.as_deref(),
            ) {
                entries.push(XRecordEntry::string(300, &name));
            }
        }
        entries
    }
}

fn entry_i32(entries: &[XRecordEntry], code: i32) -> Option<i32> {
    entries
        .iter()
        .find(|entry| entry.code == code)
        .and_then(|entry| entry.value.as_i32())
}

fn entry_handle(entries: &[XRecordEntry], code: i32) -> Option<Handle> {
    entries
        .iter()
        .find(|entry| entry.code == code)
        .and_then(|entry| entry.value.as_handle())
}

fn entry_string(entries: &[XRecordEntry], code: i32) -> Option<&str> {
    entries
        .iter()
        .find(|entry| entry.code == code)
        .and_then(|entry| entry.value.as_string())
}
