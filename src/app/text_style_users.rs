//! The texts that use a text style, and what Apply does to them.
//!
//! Apply writes the style only; no text is changed. Font, bold/italic and the
//! backward/upside-down flags come from the style when a text is drawn, so
//! they follow by themselves. TEXT, ATTRIB and ATTDEF store their own height,
//! width and oblique angle, and every MTEXT its height: those stay as they
//! are, a new value reaches only texts placed afterwards. An MTEXT draws with
//! the style's width and oblique unless it carries a whole-text `\W` / `\Q`
//! code, so it redraws with the new ones.
//!
//! Before Apply the manager lists every changed style this way.

use document::entities::EntityType;
use document::tables::TextStyle;
use document::types::Handle;
use document::Document;
use rustc_hash::{FxHashMap, FxHashSet};

/// A style value an Apply can carry over to the texts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StyleValue {
    Height,
    Width,
    Oblique,
}

impl StyleValue {
    const ALL: [StyleValue; 3] = [StyleValue::Height, StyleValue::Width, StyleValue::Oblique];

    fn of(self, style: &TextStyle) -> f64 {
        match self {
            StyleValue::Height => style.height,
            StyleValue::Width => style.width_factor,
            StyleValue::Oblique => style.oblique_angle,
        }
    }
}

/// How a text holds one value.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Held {
    /// Stored on the entity, so it stays when the style changes.
    Stored,
    /// Not stored: drawn with the style's value (MTEXT width and oblique).
    FromStyle,
    /// Not stored: the text carries its own (an MTEXT `\W` / `\Q` code).
    Own,
}

/// What kind of text uses the style, as "Where used" counts them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKind {
    Text,
    MText,
    AttributeDefinition,
    Attribute,
}

/// One text that uses the style.
#[derive(Debug, Clone)]
pub(super) struct TextUser {
    kind: TextKind,
    /// The block definition that holds the text.
    block: Option<Handle>,
    in_block: bool,
    height: Held,
    width: Held,
    oblique: Held,
}

/// How many texts of each kind use a style, and how many of them sit inside
/// how many block definitions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Usage {
    pub kinds: Vec<(TextKind, usize)>,
    pub in_blocks: usize,
    pub blocks: usize,
}

impl Usage {
    pub(super) fn of(users: &[TextUser]) -> Self {
        let kinds = [
            TextKind::Text,
            TextKind::MText,
            TextKind::AttributeDefinition,
            TextKind::Attribute,
        ]
        .into_iter()
        .map(|kind| (kind, users.iter().filter(|u| u.kind == kind).count()))
        .filter(|(_, count)| *count > 0)
        .collect();
        let blocks: FxHashSet<Handle> = users.iter().filter_map(|u| u.block).collect();
        Usage {
            kinds,
            in_blocks: users.iter().filter(|u| u.in_block).count(),
            blocks: blocks.len(),
        }
    }
}

impl TextUser {
    fn held(&self, value: StyleValue) -> Held {
        match value {
            StyleValue::Height => self.height,
            StyleValue::Width => self.width,
            StyleValue::Oblique => self.oblique,
        }
    }

    /// Whether the text draws with the style's value, so a new one shows on
    /// it. A stored value stays on the text whatever the style says.
    fn follows(&self, value: StyleValue) -> bool {
        matches!(self.held(value), Held::FromStyle)
    }
}

/// The entity handles in external references, and those in block
/// definitions with the definition that holds each.
fn xref_and_block_handles(doc: &Document) -> (FxHashSet<Handle>, FxHashMap<Handle, Handle>) {
    let mut xref = FxHashSet::default();
    let mut block = FxHashMap::default();
    for record in doc.block_records.iter() {
        if record.flags.is_xref || record.flags.is_xref_overlay {
            xref.extend(record.entity_handles.iter().copied());
        } else if !(record.is_model_space() || record.is_paper_space()) {
            block.extend(record.entity_handles.iter().map(|h| (*h, record.handle)));
        }
    }
    (xref, block)
}

/// The texts of every style, keyed by the style name in ASCII lowercase, in one walk
/// of the drawing.
pub(super) fn users_by_style(doc: &Document) -> FxHashMap<String, Vec<TextUser>> {
    let (xref, block) = xref_and_block_handles(doc);
    let mut users: FxHashMap<String, Vec<TextUser>> = FxHashMap::default();
    for entity in doc.entities() {
        let handle = entity.common().handle;
        if xref.contains(&handle) {
            continue;
        }
        let block = block.get(&handle).copied();
        let mut push = |style: &str, kind, height, width, oblique| {
            users
                .entry(style.to_ascii_lowercase())
                .or_default()
                .push(TextUser {
                    kind,
                    block,
                    in_block: block.is_some(),
                    height,
                    width,
                    oblique,
                })
        };
        let stored = Held::Stored;
        match entity {
            EntityType::Text(t) => push(&t.style, TextKind::Text, stored, stored, stored),
            EntityType::AttributeDefinition(a) => push(
                &a.text_style,
                TextKind::AttributeDefinition,
                stored,
                stored,
                stored,
            ),
            EntityType::Insert(insert) => {
                for a in &insert.attributes {
                    push(&a.text_style, TextKind::Attribute, stored, stored, stored)
                }
            }
            EntityType::MText(t) => {
                let code = |code| {
                    if super::mtext_editor::sets_whole_text(&t.value, code) {
                        Held::Own
                    } else {
                        Held::FromStyle
                    }
                };
                // The height is stored (or a whole-text `\H`): it stays.
                push(&t.style, TextKind::MText, stored, code('W'), code('Q'))
            }
            _ => {}
        }
    }
    users
}

/// What an Apply does to one value on the texts.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ValueChange {
    pub value: StyleValue,
    pub from: f64,
    pub to: f64,
    /// Texts that draw with the style's value and show the new one.
    pub updated: usize,
    /// How many of those are inside block definitions.
    pub in_blocks: usize,
    /// Texts with a value of their own, which keep it.
    pub kept: usize,
}

/// The values that change from `old` to `new` and what that does to the
/// texts. A fixed height that was or becomes 0 (asked when placing) changes
/// no existing text.
pub(super) fn value_changes(
    users: &[TextUser],
    old: &TextStyle,
    new: &TextStyle,
) -> Vec<ValueChange> {
    StyleValue::ALL
        .into_iter()
        .filter_map(|value| {
            let (from, to) = (value.of(old), value.of(new));
            if from == to || value == StyleValue::Height && (from <= 0.0 || to <= 0.0) {
                return None;
            }
            let following: Vec<_> = users.iter().filter(|u| u.follows(value)).collect();
            Some(ValueChange {
                value,
                from,
                to,
                updated: following.len(),
                in_blocks: following.iter().filter(|u| u.in_block).count(),
                kept: users.len() - following.len(),
            })
        })
        .filter(|change| change.updated + change.kept > 0)
        .collect()
}

/// A number without trailing zeros (`0.8`, `1`).
fn trimmed(value: f64) -> String {
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    crate::entities::common::without_negative_zero(text.to_string())
}

/// A style property the summary names when Apply changes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Property {
    Name,
    Font,
    FontStyle,
    BigFont,
    Height,
    Width,
    Oblique,
    Annotative,
    Backward,
    UpsideDown,
    Vertical,
    /// A stored setting the manager does not show (character set, last
    /// height): the style still changes.
    Other,
}

impl Property {
    /// Texts draw with it from the style; they store nothing of it.
    fn is_drawn(self) -> bool {
        matches!(
            self,
            Property::Font
                | Property::FontStyle
                | Property::BigFont
                | Property::Backward
                | Property::UpsideDown
                | Property::Vertical
        )
    }
}

/// One property Apply changes, its stored and new values as shown.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PropertyChange {
    pub property: Property,
    pub from: String,
    pub to: String,
}

/// Everything Apply changes on one style and its texts.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct StyleChange {
    /// The name the style has after Apply.
    pub name: String,
    pub properties: Vec<PropertyChange>,
    /// What the changed height, width and oblique do to the texts.
    pub values: Vec<ValueChange>,
    /// The texts that use the style.
    pub users: usize,
}

fn on_off(on: bool) -> String {
    if on {
        crate::t!("On")
    } else {
        crate::t!("Off")
    }
    .into_owned()
}

fn or_none(value: &str) -> String {
    if value.trim().is_empty() {
        crate::t!("None").into_owned()
    } else {
        value.to_string()
    }
}

/// The properties that differ between `old` and `new`, in the manager's
/// order. A style that differs only in what the manager does not show gets
/// `Other`, so no change goes unlisted.
pub(super) fn property_changes(old: &TextStyle, new: &TextStyle) -> Vec<PropertyChange> {
    use crate::entities::text_support::{TTF_FLAG_BOLD, TTF_FLAG_ITALIC};
    let font = |s: &TextStyle| {
        or_none(if s.true_type_font.trim().is_empty() {
            &s.font_file
        } else {
            &s.true_type_font
        })
    };
    let face = |s: &TextStyle| {
        crate::ui::font_picker::FaceStyle::of(
            s.true_type_font_flags & TTF_FLAG_BOLD != 0,
            s.true_type_font_flags & TTF_FLAG_ITALIC != 0,
        )
        .to_string()
    };
    let length = crate::entities::common::format_length;
    let degrees = |radians: f64| format!("{}°", trimmed(radians.to_degrees()));
    let shown: [(Property, String, String); 11] = [
        (Property::Name, old.name.clone(), new.name.clone()),
        (Property::Font, font(old), font(new)),
        (Property::FontStyle, face(old), face(new)),
        (
            Property::BigFont,
            or_none(&old.big_font_file),
            or_none(&new.big_font_file),
        ),
        (Property::Height, length(old.height), length(new.height)),
        (
            Property::Width,
            trimmed(old.width_factor),
            trimmed(new.width_factor),
        ),
        (
            Property::Oblique,
            degrees(old.oblique_angle),
            degrees(new.oblique_angle),
        ),
        (
            Property::Annotative,
            on_off(old.annotative),
            on_off(new.annotative),
        ),
        (
            Property::Backward,
            on_off(old.flags.backward),
            on_off(new.flags.backward),
        ),
        (
            Property::UpsideDown,
            on_off(old.flags.upside_down),
            on_off(new.flags.upside_down),
        ),
        (
            Property::Vertical,
            on_off(old.is_vertical),
            on_off(new.is_vertical),
        ),
    ];
    // Decided on the stored values: a change below the shown precision still
    // counts, as Apply writes it.
    let differs = |property: Property| match property {
        Property::Height => old.height != new.height,
        Property::Width => old.width_factor != new.width_factor,
        Property::Oblique => old.oblique_angle != new.oblique_angle,
        Property::Font => {
            (&old.font_file, &old.true_type_font) != (&new.font_file, &new.true_type_font)
        }
        Property::FontStyle => {
            (old.true_type_font_flags ^ new.true_type_font_flags)
                & (TTF_FLAG_BOLD | TTF_FLAG_ITALIC)
                != 0
        }
        _ => false,
    };
    let mut changes: Vec<PropertyChange> = shown
        .into_iter()
        .filter(|(property, from, to)| from != to || differs(*property))
        .map(|(property, from, to)| PropertyChange { property, from, to })
        .collect();
    if changes.is_empty() && old != new {
        changes.push(PropertyChange {
            property: Property::Other,
            from: String::new(),
            to: String::new(),
        });
    }
    changes
}

/// What Apply does to every style changed since the manager opened (or since
/// the last Apply), matched by handle. `styles` are the styles as Apply will see them, `baseline` gives
/// each one's stored state, `users` the texts of each style keyed by the name
/// in ASCII lowercase.
pub(super) fn pending_changes<'s, 'b>(
    baseline: impl Fn(Handle) -> Option<&'b TextStyle>,
    styles: impl IntoIterator<Item = &'s TextStyle>,
    users: &FxHashMap<String, Vec<TextUser>>,
) -> Vec<StyleChange> {
    styles
        .into_iter()
        .filter_map(|new| {
            let old = baseline(new.handle).filter(|_| new.handle.is_valid())?;
            if old == new {
                return None;
            }
            let texts = users
                .get(&new.name.to_ascii_lowercase())
                .map(Vec::as_slice)
                .unwrap_or_default();
            Some(StyleChange {
                name: new.name.clone(),
                properties: property_changes(old, new),
                values: value_changes(texts, old, new),
                users: texts.len(),
            })
        })
        .collect()
}

/// "Height: 2.5 → 3.5. 4 texts that follow the style are updated. …"
fn value_line(label: &str, from: &str, to: &str, change: &ValueChange) -> String {
    let (value, count) = (label, change.updated);
    let mut line = if count == 1 {
        crate::tf!("{value}: {from} → {to}. 1 text that follows the style is updated.")
    } else {
        crate::tf!("{value}: {from} → {to}. {count} texts that follow the style are updated.")
    }
    .into_owned();
    if change.in_blocks > 0 {
        let count = change.in_blocks;
        line.push(' ');
        line.push_str(&crate::tf!("Inside block definitions: {count}."));
    }
    let count = change.kept;
    match count {
        0 => {}
        1 => {
            line.push(' ');
            line.push_str(&crate::t!("1 text with its own value keeps it."));
        }
        _ => {
            line.push(' ');
            line.push_str(&crate::tf!("{count} texts with their own value keep them."));
        }
    }
    line
}

/// The lines the style manager shows before Apply for one changed style: per
/// property its stored and new value; for a height, width or oblique also how
/// many texts follow the style and take it and how many keep their own.
pub(super) fn summary_lines(change: &StyleChange) -> Vec<String> {
    let mut lines = Vec::new();
    for property in &change.properties {
        let label = match property.property {
            Property::Name => crate::t!("Name"),
            Property::Font => crate::t!("Font"),
            Property::FontStyle => crate::t!("Font style"),
            Property::BigFont => crate::t!("Big Font file"),
            Property::Height => crate::t!("Height"),
            Property::Width => crate::t!("Width factor"),
            Property::Oblique => crate::t!("Oblique angle"),
            Property::Annotative => crate::t!("Annotative"),
            Property::Backward => crate::t!("Backward"),
            Property::UpsideDown => crate::t!("Upside down"),
            Property::Vertical => crate::t!("Vertical"),
            Property::Other => {
                lines.push(crate::t!("Other stored settings of the style change.").into_owned());
                continue;
            }
        };
        let (from, to) = (&property.from, &property.to);
        let value = match property.property {
            Property::Height => Some(StyleValue::Height),
            Property::Width => Some(StyleValue::Width),
            Property::Oblique => Some(StyleValue::Oblique),
            _ => None,
        };
        let line = match value.and_then(|value| change.values.iter().find(|c| c.value == value)) {
            Some(values) => value_line(&label, from, to, values),
            None => {
                let value = label;
                let mut line = crate::tf!("{value}: {from} → {to}.").into_owned();
                if property.property == Property::Annotative {
                    line.push(' ');
                    line.push_str(&crate::t!(
                        "Existing texts keep their own setting; new texts take this one."
                    ));
                }
                line
            }
        };
        lines.push(line);
    }
    let count = change.users;
    if count > 0 && change.properties.iter().any(|p| p.property.is_drawn()) {
        lines.push(
            if count == 1 {
                crate::t!("1 text that uses the style is redrawn; formatting set inside MTEXT stays.")
            } else {
                crate::tf!(
                    "{count} texts that use the style are redrawn; formatting set inside MTEXT stays."
                )
            }
            .into_owned(),
        );
    }
    lines
}
