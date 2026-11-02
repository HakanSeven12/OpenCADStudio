//! The texts that use a text style, and what Apply changes on them: a value
//! that follows the style takes the new one, a value set on the text itself
//! stays (#2413).
//!
//! Font, bold/italic and the backward/upside-down flags come from the style
//! when a text is drawn, so they follow by themselves. TEXT, ATTRIB and
//! ATTDEF store their own width and oblique angle, and every text stores its
//! height; those are written here when they equal the style's old value. An
//! MTEXT draws with the style's width and oblique unless it carries a
//! whole-text `\W` / `\Q` code, and keeps its height under a whole-text `\H`.

use codec::entities::EntityType;
use codec::tables::TextStyle;
use codec::types::Handle;
use codec::Document;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;

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
    /// Stored on the entity.
    Stored(f64),
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
    /// The text, or the INSERT that holds the attribute.
    handle: Handle,
    /// The attribute's place in the INSERT.
    attribute: Option<usize>,
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

    /// Whether the text shows the style's `old` value, so it follows the
    /// style. A height follows only a fixed (non-zero) style height.
    fn follows(&self, value: StyleValue, old: f64) -> bool {
        match self.held(value) {
            Held::Stored(_) if value == StyleValue::Height && old <= 0.0 => false,
            Held::Stored(stored) => same(stored, old),
            Held::FromStyle => true,
            Held::Own => false,
        }
    }
}

/// Equal up to the last bits a save and load may round.
fn same(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
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

/// The texts in the drawing that use `style`: model space, layouts, block
/// definitions and the attributes of inserts. Texts in external references
/// are left out; Apply does not change them.
pub(super) fn text_users(doc: &Document, style: &str) -> Vec<TextUser> {
    users_by_style(doc)
        .remove(&style.to_ascii_lowercase())
        .unwrap_or_default()
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
        let mut push = |style: &str, kind, attribute, height, width, oblique| {
            users
                .entry(style.to_ascii_lowercase())
                .or_default()
                .push(TextUser {
                    handle,
                    attribute,
                    kind,
                    block,
                    in_block: block.is_some(),
                    height,
                    width,
                    oblique,
                })
        };
        let stored = |height: f64, width: f64, oblique: f64| {
            (
                Held::Stored(height),
                Held::Stored(width),
                Held::Stored(oblique),
            )
        };
        match entity {
            EntityType::Text(t) => {
                let (h, w, o) = stored(t.height, t.width_factor, t.oblique_angle);
                push(&t.style, TextKind::Text, None, h, w, o)
            }
            EntityType::AttributeDefinition(a) => {
                let (h, w, o) = stored(a.height, a.width_factor, a.oblique_angle);
                push(&a.text_style, TextKind::AttributeDefinition, None, h, w, o)
            }
            EntityType::Insert(insert) => {
                for (index, a) in insert.attributes.iter().enumerate() {
                    let (h, w, o) = stored(a.height, a.width_factor, a.oblique_angle);
                    push(&a.text_style, TextKind::Attribute, Some(index), h, w, o)
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
                // A whole-text `\H` is the text's own height, whatever the
                // stored one says.
                let height = match code('H') {
                    Held::Own => Held::Own,
                    _ => Held::Stored(t.height),
                };
                push(
                    &t.style,
                    TextKind::MText,
                    None,
                    height,
                    code('W'),
                    code('Q'),
                )
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
    /// Texts that follow the style and take the new value.
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
            let following: Vec<_> = users.iter().filter(|u| u.follows(value, from)).collect();
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

/// Write the new style values into the texts that follow `old`, each through
/// `get_entity_mut`. The image of each text before its first change goes into
/// `before`, so the style manager can undo it with the style.
pub(super) fn update_texts(
    doc: &mut Document,
    old: &TextStyle,
    new: &TextStyle,
    before: &mut FxHashMap<Handle, Arc<EntityType>>,
) {
    let users = text_users(doc, &new.name);
    for change in value_changes(&users, old, new) {
        for user in users
            .iter()
            .filter(|u| u.follows(change.value, change.from))
        {
            if !matches!(user.held(change.value), Held::Stored(_)) {
                continue;
            }
            if let Some(image) = doc.get_entity_arc(user.handle) {
                before.entry(user.handle).or_insert(image);
            }
            let fields = match (doc.get_entity_mut(user.handle), user.attribute) {
                (Some(EntityType::Text(t)), None) => {
                    Some((&mut t.height, &mut t.width_factor, &mut t.oblique_angle))
                }
                (Some(EntityType::AttributeDefinition(a)), None) => {
                    Some((&mut a.height, &mut a.width_factor, &mut a.oblique_angle))
                }
                (Some(EntityType::Insert(insert)), Some(index)) => insert
                    .attributes
                    .get_mut(index)
                    .map(|a| (&mut a.height, &mut a.width_factor, &mut a.oblique_angle)),
                (Some(EntityType::MText(t)), None) if change.value == StyleValue::Height => {
                    t.height = change.to;
                    None
                }
                _ => None,
            };
            if let Some((height, width, oblique)) = fields {
                *match change.value {
                    StyleValue::Height => height,
                    StyleValue::Width => width,
                    StyleValue::Oblique => oblique,
                } = change.to;
            }
        }
    }
}

/// A number without trailing zeros (`0.8`, `1`).
fn trimmed(value: f64) -> String {
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    crate::entities::common::without_negative_zero(text.to_string())
}

/// The lines the style manager shows before Apply: per changed value, how
/// many texts follow the style and take it, and how many keep their own.
pub(super) fn summary_lines(changes: &[ValueChange]) -> Vec<String> {
    if changes.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    for change in changes {
        let (value, from, to) = match change.value {
            StyleValue::Height => (
                crate::t!("Height"),
                crate::entities::common::format_length(change.from),
                crate::entities::common::format_length(change.to),
            ),
            StyleValue::Width => (
                crate::t!("Width factor"),
                trimmed(change.from),
                trimmed(change.to),
            ),
            StyleValue::Oblique => (
                crate::t!("Oblique angle"),
                format!("{}°", trimmed(change.from.to_degrees())),
                format!("{}°", trimmed(change.to.to_degrees())),
            ),
        };
        let count = change.updated;
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
        lines.push(line);
    }
    lines.push(
        crate::t!(
            "Texts in external references are not changed. One undo step with the style change."
        )
        .into_owned(),
    );
    lines
}
