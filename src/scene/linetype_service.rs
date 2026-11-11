//! The shared linetype-definition service: create, modify, rename, delete and
//! the "is it in use" scan.
//!
//! The command line (`-LINETYPE`), the plugin API
//! (`TableOperation::Linetype*`) and `PURGE` route through this module so the
//! same linetype gets the same answer everywhere.
//!
//! Each change has two halves. `plan_*` validates against a shared document
//! and changes nothing, so the caller can refuse before it takes an undo
//! snapshot; `apply_*` makes the change. Callers own the app-level concerns
//! (undo snapshots, command-line output, cache invalidation).

use document::objects::ObjectType;
use document::tables::linetype::LineTypeElement;
use document::tables::LineType;
use document::types::Handle;
use document::{Document, EntityType};
use rustc_hash::FxHashMap;
use std::fmt;

/// `.lin` files and DXF readers allow this many dash-length specifications.
const MAX_ELEMENTS: usize = 12;
const MIN_ELEMENTS: usize = 2;
const MAX_LENGTH: f64 = 1.0e6;
const MAX_TEXT: usize = 255;

/// Why a linetype change was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum LinetypeError {
    InvalidName,
    Exists(String),
    Catalog(String),
    NotFound(String),
    /// A built-in (ByLayer, ByBlock, Continuous) or xref-dependent linetype.
    Protected(String),
    /// Text and shape elements have no representation in a plain pattern.
    Complex(String),
    NothingToChange,
    ElementCount,
    LengthRange,
    LeadingGap,
    NoGap,
    InvalidDescription,
    InUse(String),
    Table(String),
}

impl fmt::Display for LinetypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::InvalidName => crate::t!(
                "Linetype names must be 1 to 255 characters and cannot contain < > / \\ \" : ; ? * | , = or a backtick."
            ),
            Self::Exists(name) => crate::tf!("Linetype \"{name}\" already exists."),
            Self::Catalog(name) => {
                crate::tf!("\"{name}\" is a catalog linetype. Use LINETYPE SET to load it.")
            }
            Self::NotFound(name) => crate::tf!("Linetype \"{name}\" does not exist."),
            Self::Protected(name) => crate::tf!("Linetype \"{name}\" cannot be changed."),
            Self::Complex(name) => {
                crate::tf!("Linetype \"{name}\" has text or shape elements and cannot be changed.")
            }
            Self::NothingToChange => crate::t!("No linetype properties to change."),
            Self::ElementCount => crate::t!("A linetype pattern needs 2 to 12 elements."),
            Self::LengthRange => {
                crate::t!("Linetype pattern lengths must be finite and within 1000000.")
            }
            Self::LeadingGap => crate::t!("A linetype pattern must start with a dash or dot."),
            Self::NoGap => crate::t!("A linetype pattern needs at least one gap."),
            Self::InvalidDescription => {
                crate::t!("A linetype description must be text of at most 255 characters.")
            }
            Self::InUse(name) => crate::tf!(
                "Linetype \"{name}\" is current or still in use. PURGE removes unused linetypes."
            ),
            Self::Table(reason) => crate::tf!("The linetype table refused the change: {reason}"),
        };
        f.write_str(&text)
    }
}

impl std::error::Error for LinetypeError {}

/// Every linetype a drawing still needs.
///
/// Names are compared without regard to case. Handles cover the references
/// that do not carry a name, so they keep working through a rename.
#[derive(Default)]
pub struct LinetypeUsage {
    names: FxHashMap<String, usize>,
    handles: FxHashMap<Handle, usize>,
    current: String,
}

impl LinetypeUsage {
    pub fn of(doc: &Document) -> Self {
        let mut usage = Self::default();
        for entity in doc.entities() {
            usage.name(&entity.common().linetype);
            match entity {
                EntityType::Insert(insert) => {
                    for attribute in &insert.attributes {
                        usage.name(&attribute.common.linetype);
                    }
                }
                EntityType::MultiLeader(leader) => {
                    usage.handle(leader.line_type_handle);
                    for root in &leader.context.leader_roots {
                        for line in &root.lines {
                            usage.handle(line.line_type_handle);
                        }
                    }
                }
                EntityType::Table(table) => {
                    let styles = table.rows.iter().flat_map(|row| {
                        row.style
                            .iter()
                            .chain(row.cells.iter().filter_map(|cell| cell.style.as_ref()))
                    });
                    for style in styles {
                        for border in [
                            &style.top_border,
                            &style.right_border,
                            &style.bottom_border,
                            &style.left_border,
                        ]
                        .into_iter()
                        .chain(style.additional_borders.iter().map(|(_, border)| border))
                        {
                            usage.handle(border.line_type_handle);
                        }
                    }
                }
                _ => {}
            }
        }
        for layer in doc.layers.iter() {
            usage.name(&layer.line_type);
        }
        usage.current = doc.header.current_linetype_name.to_uppercase();
        for style in doc.dim_styles.iter() {
            usage.handle(Some(style.dimltex_handle));
            usage.handle(Some(style.dimltex1_handle));
            usage.handle(Some(style.dimltex2_handle));
        }
        for object in doc.objects.values() {
            match object {
                ObjectType::MLineStyle(style) => {
                    for element in &style.elements {
                        usage.name(&element.linetype);
                    }
                }
                ObjectType::MultiLeaderStyle(style) => usage.handle(style.line_type_handle),
                _ => {}
            }
        }
        // Restoring a saved layer state writes its linetype back onto the
        // layer, so the state needs the definition to still be there.
        for state in doc.layer_states() {
            for layer in &state.layers {
                usage.name(&layer.line_type);
            }
        }
        usage
    }

    fn name(&mut self, name: &str) {
        if !name.is_empty() {
            *self.names.entry(name.to_uppercase()).or_default() += 1;
        }
    }

    fn handle(&mut self, handle: Option<Handle>) {
        if let Some(handle) = handle.filter(|handle| !handle.is_null()) {
            *self.handles.entry(handle).or_default() += 1;
        }
    }

    /// Whether the drawing needs `linetype`, the current one included.
    pub fn contains(&self, linetype: &LineType) -> bool {
        self.current == linetype.name.to_uppercase() || self.count(linetype) > 0
    }

    /// How many objects, layers and styles refer to `linetype`.
    pub fn count(&self, linetype: &LineType) -> usize {
        let named = self
            .names
            .get(&linetype.name.to_uppercase())
            .copied()
            .unwrap_or(0);
        named + self.handles.get(&linetype.handle).copied().unwrap_or(0)
    }
}

/// The three names every drawing relies on.
pub fn is_builtin(name: &str) -> bool {
    ["CONTINUOUS", "BYLAYER", "BYBLOCK"].contains(&name.trim().to_uppercase().as_str())
}

fn valid_name(raw: &str) -> Result<String, LinetypeError> {
    let name = raw.trim();
    if name.chars().count() > MAX_TEXT || !crate::scene::valid_block_name(name) {
        return Err(LinetypeError::InvalidName);
    }
    Ok(name.to_owned())
}

/// A name a new or renamed linetype may take: legal, not a built-in, not in
/// the drawing and not one the catalog would load.
fn available_name(doc: &Document, name: &str) -> Result<(), LinetypeError> {
    if is_builtin(name) {
        return Err(LinetypeError::Protected(name.to_owned()));
    }
    if doc.line_types.contains(name) {
        return Err(LinetypeError::Exists(name.to_owned()));
    }
    if crate::io::linetypes::catalog_has(name) {
        return Err(LinetypeError::Catalog(name.to_owned()));
    }
    Ok(())
}

fn check_description(text: &str) -> Result<(), LinetypeError> {
    if text.chars().any(char::is_control) || text.chars().count() > MAX_TEXT {
        return Err(LinetypeError::InvalidDescription);
    }
    Ok(())
}

/// A pattern as `.lin` stores it: positive is a dash, negative a gap, zero a
/// dot. "A" alignment needs a pen-down first element.
fn check_pattern(pattern: &[f64]) -> Result<(), LinetypeError> {
    if !(MIN_ELEMENTS..=MAX_ELEMENTS).contains(&pattern.len()) {
        return Err(LinetypeError::ElementCount);
    }
    if pattern
        .iter()
        .any(|length| !length.is_finite() || length.abs() > MAX_LENGTH)
    {
        return Err(LinetypeError::LengthRange);
    }
    if pattern[0] < 0.0 {
        return Err(LinetypeError::LeadingGap);
    }
    if !pattern.iter().any(|length| *length < 0.0) {
        return Err(LinetypeError::NoGap);
    }
    Ok(())
}

fn set_pattern(linetype: &mut LineType, pattern: &[f64]) {
    linetype.elements = pattern
        .iter()
        .map(|length| LineTypeElement {
            length: *length,
            complex: None,
        })
        .collect();
    linetype.pattern_length = pattern.iter().map(|length| length.abs()).sum();
}

/// The linetype `name` when it may be changed at all.
fn editable(doc: &Document, name: &str) -> Result<LineType, LinetypeError> {
    let name = name.trim();
    let existing = doc
        .line_types
        .get(name)
        .ok_or_else(|| LinetypeError::NotFound(name.to_owned()))?;
    if is_builtin(&existing.name) || existing.xref_dependent {
        return Err(LinetypeError::Protected(existing.name.clone()));
    }
    Ok(existing.clone())
}

/// The new, not yet added definition.
pub fn plan_create(
    doc: &Document,
    name: &str,
    description: &str,
    pattern: &[f64],
) -> Result<LineType, LinetypeError> {
    let name = valid_name(name)?;
    available_name(doc, &name)?;
    check_pattern(pattern)?;
    check_description(description)?;
    let mut linetype = LineType::new(name);
    linetype.description = description.to_owned();
    set_pattern(&mut linetype, pattern);
    Ok(linetype)
}

pub fn apply_create(doc: &mut Document, mut linetype: LineType) -> Result<Handle, LinetypeError> {
    let handle = doc.allocate_handle();
    linetype.handle = handle;
    doc.line_types
        .add(linetype)
        .map_err(|error| LinetypeError::Table(error.to_string()))?;
    Ok(handle)
}

/// The definition as it will be after the change.
pub fn plan_modify(
    doc: &Document,
    name: &str,
    description: Option<&str>,
    pattern: Option<&[f64]>,
) -> Result<LineType, LinetypeError> {
    let mut linetype = editable(doc, name)?;
    if linetype.is_complex() {
        return Err(LinetypeError::Complex(linetype.name));
    }
    if description.is_none() && pattern.is_none() {
        return Err(LinetypeError::NothingToChange);
    }
    if let Some(text) = description {
        check_description(text)?;
        linetype.description = text.to_owned();
    }
    if let Some(pattern) = pattern {
        check_pattern(pattern)?;
        set_pattern(&mut linetype, pattern);
    }
    Ok(linetype)
}

pub fn apply_modify(doc: &mut Document, linetype: LineType) -> Handle {
    let handle = linetype.handle;
    if let Some(slot) = doc.line_types.get_mut(&linetype.name) {
        *slot = linetype;
    }
    handle
}

/// A validated rename.
#[derive(Clone, Debug, PartialEq)]
pub struct LinetypeRename {
    /// The stored spelling of the old name.
    pub old: String,
    pub new: String,
    pub handle: Handle,
}

pub fn plan_rename(doc: &Document, from: &str, to: &str) -> Result<LinetypeRename, LinetypeError> {
    let existing = editable(doc, from)?;
    let new = valid_name(to)?;
    if existing.name == new {
        return Err(LinetypeError::Exists(new));
    }
    // A change of case only keeps the entry, so its own name is not taken.
    if !existing.name.eq_ignore_ascii_case(&new) {
        available_name(doc, &new)?;
    }
    Ok(LinetypeRename {
        old: existing.name,
        new,
        handle: existing.handle,
    })
}

pub fn apply_rename(doc: &mut Document, rename: &LinetypeRename) -> Result<Handle, LinetypeError> {
    doc.line_types
        .rename(&rename.old, rename.new.clone())
        .map_err(|error| LinetypeError::Table(error.to_string()))?;
    rename_references(doc, &rename.old, &rename.new);
    Ok(rename.handle)
}

/// Point everything that stores linetype `old` by name at `new`. References
/// by handle follow the table entry on their own.
pub fn rename_references(doc: &mut Document, old: &str, new: &str) {
    for handle in entities_naming(doc, old) {
        if let Some(entity) = doc.get_entity_mut(handle) {
            retarget_entity(entity, old, new);
        }
    }
    rename_table_references(doc, old, new);
}

/// The entities that store linetype `name`, on themselves or an attribute.
pub fn entities_naming(doc: &Document, name: &str) -> Vec<Handle> {
    let names = |linetype: &str| linetype.eq_ignore_ascii_case(name);
    doc.entities()
        .filter(|entity| {
            names(&entity.common().linetype)
                || matches!(entity, EntityType::Insert(insert)
                    if insert.attributes.iter().any(|a| names(&a.common.linetype)))
        })
        .map(|entity| entity.common().handle)
        .collect()
}

pub fn retarget_entity(entity: &mut EntityType, old: &str, new: &str) {
    let retarget = |slot: &mut String| {
        if slot.eq_ignore_ascii_case(old) {
            *slot = new.to_owned();
        }
    };
    retarget(&mut entity.common_mut().linetype);
    if let EntityType::Insert(insert) = entity {
        for attribute in &mut insert.attributes {
            retarget(&mut attribute.common.linetype);
        }
    }
}

/// Everything but entities: layers, the current linetype, multiline styles
/// and saved layer states.
pub fn rename_table_references(doc: &mut Document, old: &str, new: &str) {
    let retarget = |slot: &mut String| {
        if slot.eq_ignore_ascii_case(old) {
            *slot = new.to_owned();
        }
    };
    for layer in doc.layers.iter_mut() {
        retarget(&mut layer.line_type);
    }
    retarget(&mut doc.header.current_linetype_name);
    for object in doc.objects.values_mut() {
        if let ObjectType::MLineStyle(style) = object {
            for element in &mut style.elements {
                retarget(&mut element.linetype);
            }
        }
    }
    for mut state in doc.layer_states() {
        let mut changed = false;
        for layer in &mut state.layers {
            if layer.line_type.eq_ignore_ascii_case(old) {
                layer.line_type = new.to_owned();
                changed = true;
            }
        }
        if changed {
            doc.store_layer_state(state);
        }
    }
}

/// The stored name of a linetype that may be deleted.
pub fn plan_delete(doc: &Document, name: &str) -> Result<LineType, LinetypeError> {
    let existing = editable(doc, name)?;
    if LinetypeUsage::of(doc).contains(&existing) {
        return Err(LinetypeError::InUse(existing.name));
    }
    Ok(existing)
}

pub fn apply_delete(doc: &mut Document, linetype: &LineType) -> Handle {
    doc.line_types.remove(&linetype.name);
    linetype.handle
}
