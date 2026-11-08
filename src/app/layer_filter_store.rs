//! Layer property filters kept in the drawing (#2610), the way DWG keeps
//! them: one `AcLyLayerFilter` XRECORD per filter (`300` name, `301`
//! expression such as `( NAME== "A*" AND PLOTTABLE== "False" )`) under
//! anonymous `*A<n>` keys in the layer table's `ACLYDICTIONARY`, beside the
//! layer groups. A nested filter sits in the `ACLYDICTIONARY` of its parent
//! record's extension dictionary. A record whose expression can't be read is
//! left as it is.

use crate::filter::{Condition, Filter, Group, Item, Match, Op};
use document::objects::{
    Dictionary, DictionaryCloningFlags, ObjectType, XRecord, XRecordEntry, XRecordValue,
};
use document::{Document, Handle};

const KIND: &str = "AcLyLayerFilter";
const VERSION: i32 = 1;
const DICTIONARY: &str = "ACLYDICTIONARY";
/// Nesting deeper than this is taken as a broken (cyclic) drawing.
const MAX_DEPTH: usize = 16;

/// A filter as the drawing keeps it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StoredLayerFilter {
    pub name: String,
    /// The filter it sits under, if any.
    pub parent: Option<String>,
    pub filter: Filter,
}

fn parse(xrecord: &XRecord) -> Option<(String, Filter)> {
    let mut entries = xrecord.entries.iter();
    let kind = entries.next()?;
    if kind.code != 1 || kind.value.as_string() != Some(KIND) {
        return None;
    }
    let (mut name, mut expression) = (None, None);
    for entry in entries {
        match (entry.code, &entry.value) {
            (300, XRecordValue::String(v)) if name.is_none() => name = Some(v.clone()),
            (301, XRecordValue::String(v)) if expression.is_none() => expression = Some(v.clone()),
            _ => {}
        }
    }
    Some((name?, from_expression(expression.as_deref().unwrap_or(""))?))
}

/// The `ACLYDICTIONARY` of a record's extension dictionary: its nested filters.
fn nested_dictionary(document: &Document, xrecord: &XRecord) -> Option<Handle> {
    let Some(ObjectType::Dictionary(extension)) = document.objects.get(&xrecord.xdictionary_handle?)
    else {
        return None;
    };
    extension
        .entries
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(DICTIONARY))
        .map(|(_, handle)| *handle)
        .filter(|handle| matches!(document.objects.get(handle), Some(ObjectType::Dictionary(_))))
}

/// The filter records under `dictionary`, each before the ones nested in it.
fn walk(
    document: &Document,
    dictionary: Handle,
    parent: Option<&str>,
    depth: usize,
    out: &mut Vec<(Handle, StoredLayerFilter)>,
) {
    let Some(ObjectType::Dictionary(entries)) = document.objects.get(&dictionary) else {
        return;
    };
    if depth > MAX_DEPTH {
        return;
    }
    for (_, handle) in &entries.entries {
        let Some(ObjectType::XRecord(xrecord)) = document.objects.get(handle) else {
            continue;
        };
        let Some((name, filter)) = parse(xrecord) else {
            continue;
        };
        out.push((
            *handle,
            StoredLayerFilter {
                name: name.clone(),
                parent: parent.map(str::to_owned),
                filter,
            },
        ));
        if let Some(nested) = nested_dictionary(document, xrecord) {
            walk(document, nested, Some(&name), depth + 1, out);
        }
    }
}

/// The filter records of the drawing, with their handles, in order.
fn records(document: &Document) -> Vec<(Handle, StoredLayerFilter)> {
    let mut out = Vec::new();
    if let Some(dictionary) = super::layer_groups::filter_dictionary(document) {
        walk(document, dictionary, None, 0, &mut out);
    }
    out
}

/// The drawing's layer property filters.
pub(crate) fn stored_layer_filters(document: &Document) -> Vec<StoredLayerFilter> {
    records(document).into_iter().map(|(_, f)| f).collect()
}

/// Take a filter record out of its dictionary, with the extension
/// dictionary that held its nested filters.
fn remove_record(document: &mut Document, handle: Handle) {
    let Some(ObjectType::XRecord(xrecord)) = document.objects.remove(&handle) else {
        return;
    };
    if let Some(ObjectType::Dictionary(owner)) = document.objects.get_mut(&xrecord.owner) {
        owner.entries.retain(|(_, h)| *h != handle);
    }
    if let Some(extension) = xrecord.xdictionary_handle {
        if let Some(ObjectType::Dictionary(extension)) = document.objects.remove(&extension) {
            for (_, nested) in extension.entries {
                document.objects.remove(&nested);
            }
        }
    }
}

fn new_dictionary(document: &mut Document, owner: Handle, hard_owner: bool) -> Handle {
    let handle = document.allocate_handle();
    let mut dictionary = Dictionary::new();
    dictionary.handle = handle;
    dictionary.owner = owner;
    dictionary.hard_owner = hard_owner;
    if !hard_owner {
        dictionary.reactors = vec![owner];
        dictionary.duplicate_cloning = 1;
    }
    document.objects.insert(handle, ObjectType::Dictionary(dictionary));
    handle
}

/// The dictionary that holds the filters nested in record `parent`, made
/// when it is missing.
fn ensure_nested_dictionary(document: &mut Document, parent: Handle) -> Handle {
    if let Some(ObjectType::XRecord(xrecord)) = document.objects.get(&parent) {
        if let Some(found) = nested_dictionary(document, xrecord) {
            return found;
        }
    }
    let extension = new_dictionary(document, parent, true);
    if let Some(ObjectType::XRecord(xrecord)) = document.objects.get_mut(&parent) {
        xrecord.xdictionary_handle = Some(extension);
    }
    let nested = new_dictionary(document, extension, false);
    if let Some(ObjectType::Dictionary(extension)) = document.objects.get_mut(&extension) {
        extension.entries.push((DICTIONARY.to_string(), nested));
    }
    nested
}

/// Put `filters` in the drawing in place of the ones it has; records it
/// can't read stay untouched.
pub(crate) fn write_layer_filters(document: &mut Document, filters: &[StoredLayerFilter]) {
    for (handle, _) in records(document).into_iter().rev() {
        remove_record(document, handle);
    }
    if filters.is_empty() {
        return;
    }
    let root = super::layer_groups::ensure_filter_dictionary(document);
    let mut placed: Vec<(String, Handle)> = Vec::new();
    for stored in filters {
        let parent = stored.parent.as_deref().and_then(|parent| {
            placed
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(parent))
                .map(|(_, handle)| *handle)
        });
        let dictionary = match parent {
            Some(parent) => ensure_nested_dictionary(document, parent),
            None => root,
        };
        let handle = document.allocate_handle();
        let mut xrecord = XRecord::new();
        xrecord.handle = handle;
        xrecord.owner = dictionary;
        xrecord.reactors = vec![dictionary];
        xrecord.cloning_flags = DictionaryCloningFlags::KeepExisting;
        xrecord.entries = vec![
            XRecordEntry::string(1, KIND),
            XRecordEntry::new(90, XRecordValue::Int32(VERSION)),
            XRecordEntry::string(300, &stored.name),
            XRecordEntry::string(301, &to_expression(&stored.filter)),
        ];
        document.objects.insert(handle, ObjectType::XRecord(xrecord));
        if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&dictionary) {
            let next = (1..)
                .map(|n| format!("*A{n}"))
                .find(|key| {
                    !dictionary
                        .entries
                        .iter()
                        .any(|(taken, _)| taken.eq_ignore_ascii_case(key))
                })
                .unwrap_or_default();
            dictionary.entries.push((next, handle));
        }
        placed.push((stored.name.clone(), handle));
    }
}

/// Our property ids and the keys of the expression other programs write.
const KEYS: [(&str, &str); 8] = [
    ("name", "NAME"),
    ("description", "DESCRIPTION"),
    ("color", "COLOR"),
    ("linetype", "LINETYPE"),
    ("on", "ON"),
    ("frozen", "FROZEN"),
    ("locked", "LOCKED"),
    ("plot", "PLOTTABLE"),
];

fn flag_key(field: &str) -> bool {
    matches!(field, "on" | "frozen" | "locked" | "plot" | "xref")
}

fn condition_expression(condition: &Condition) -> Option<String> {
    let field = condition.property.as_deref()?;
    let key = KEYS.iter().find(|(f, _)| *f == field).map(|(_, k)| *k)?;
    let op = match condition.op {
        Op::Eq => "==",
        Op::Ne => "!=",
        Op::Gt => ">",
        Op::Ge => ">=",
        Op::Lt => "<",
        Op::Le => "<=",
        Op::Any => return None,
    };
    let value = if flag_key(field) {
        if condition.value == "1" {
            "True"
        } else {
            "False"
        }
        .to_string()
    } else {
        condition.value.replace('"', "")
    };
    let body = format!("{key}{op} \"{value}\"");
    Some(if condition.negate {
        format!("NOT {body}")
    } else {
        body
    })
}

/// The filter in the expression form: `( NAME== "A*" AND ( COLOR== "1" OR … ) )`.
pub(crate) fn to_expression(filter: &Filter) -> String {
    let join = |mode: Match| if mode == Match::All { " AND " } else { " OR " };
    let body = filter
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Condition(c) => condition_expression(c),
            Item::Group(g) => {
                let parts: Vec<String> = g
                    .conditions
                    .iter()
                    .filter_map(condition_expression)
                    .collect();
                (!parts.is_empty()).then(|| format!("( {} )", parts.join(join(g.mode))))
            }
        })
        .collect::<Vec<_>>()
        .join(join(filter.root));
    if body.is_empty() {
        body
    } else {
        format!("( {body} )")
    }
}

/// `text` without parentheses that enclose all of it.
fn strip_parens(mut text: &str) -> &str {
    loop {
        text = text.trim();
        let Some(inner) = text.strip_prefix('(').and_then(|t| t.strip_suffix(')')) else {
            return text;
        };
        // `(a) AND (b)` starts and ends with parentheses that are not a pair.
        let mut depth = 0i32;
        let mut quoted = false;
        for c in inner.chars() {
            match c {
                '"' => quoted = !quoted,
                '(' if !quoted => depth += 1,
                ')' if !quoted => {
                    depth -= 1;
                    if depth < 0 {
                        return text;
                    }
                }
                _ => {}
            }
        }
        text = inner;
    }
}

/// `text` split at its outermost AND or OR (outside quotes and
/// parentheses), with which of them joins it; `None` when both do.
fn split_top(text: &str) -> Option<(Match, Vec<&str>)> {
    let (bytes, upper) = (text.as_bytes(), text.to_ascii_uppercase());
    let (mut depth, mut quoted, mut mode) = (0i32, false, None);
    let (mut parts, mut from, mut at) = (Vec::new(), 0, 0);
    while at < bytes.len() {
        match bytes[at] {
            b'"' => quoted = !quoted,
            b'(' if !quoted => depth += 1,
            b')' if !quoted => depth -= 1,
            _ => {}
        }
        if depth == 0 && !quoted {
            let joiner = [(" AND ", Match::All), (" OR ", Match::Any)]
                .into_iter()
                .find(|(word, _)| upper.as_bytes()[at..].starts_with(word.as_bytes()));
            if let Some((word, kind)) = joiner {
                if mode.is_some_and(|m| m != kind) {
                    return None;
                }
                mode = Some(kind);
                parts.push(&text[from..at]);
                at += word.len();
                from = at;
                continue;
            }
        }
        at += 1;
    }
    parts.push(&text[from..]);
    Some((mode.unwrap_or(Match::All), parts))
}

/// Read an expression another program wrote: conditions `KEY== "value"` or
/// `KEY!= "value"`, optionally led by NOT, joined by AND or OR, one level of
/// parentheses grouping them. Anything else gives `None`.
pub(crate) fn from_expression(expression: &str) -> Option<Filter> {
    let text = strip_parens(expression);
    if text.is_empty() {
        return Some(Filter::default());
    }
    let (root, parts) = split_top(text)?;
    let mut items = Vec::new();
    for part in parts {
        let part = strip_parens(part);
        let (mode, inner) = split_top(part)?;
        if inner.len() > 1 {
            let conditions = inner
                .into_iter()
                .map(|c| parse_condition(strip_parens(c)))
                .collect::<Option<Vec<_>>>()?;
            items.push(Item::Group(Group { mode, conditions }));
        } else {
            items.push(Item::Condition(parse_condition(part)?));
        }
    }
    Some(Filter { root, items })
}

fn parse_condition(part: &str) -> Option<Condition> {
    let (negate, part) = match part.get(..4) {
        Some(head) if head.eq_ignore_ascii_case("NOT ") => (true, part[4..].trim()),
        _ => (false, part),
    };
    let (key, op, value) = ["==", "!=", ">=", "<=", ">", "<", "="]
        .iter()
        .find_map(|op| part.split_once(op).map(|(k, v)| (k.trim(), *op, v.trim())))?;
    let field = KEYS
        .iter()
        .find(|(_, k)| k.eq_ignore_ascii_case(key))
        .map(|(f, _)| *f)?;
    let value = value.trim_matches('"').to_string();
    let value = if flag_key(field) {
        if value.eq_ignore_ascii_case("true") {
            "1"
        } else {
            "0"
        }
        .to_string()
    } else {
        value
    };
    Some(Condition {
        negate,
        type_filter: None,
        property: Some(field.to_string()),
        op: match op {
            "!=" => Op::Ne,
            ">" => Op::Gt,
            ">=" => Op::Ge,
            "<" => Op::Lt,
            "<=" => Op::Le,
            _ => Op::Eq,
        },
        value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(field: &str, value: &str) -> Item {
        Item::Condition(Condition {
            property: Some(field.into()),
            value: value.into(),
            ..Default::default()
        })
    }

    #[test]
    fn filters_round_trip_through_the_layer_dictionary_and_dxf() {
        let mut document = Document::new();
        let filters = vec![
            StoredLayerFilter {
                name: "Hidden".into(),
                parent: None,
                filter: Filter {
                    root: Match::All,
                    items: vec![row("name", "A-*"), row("plot", "0")],
                },
            },
            StoredLayerFilter {
                name: "Hidden walls".into(),
                parent: Some("Hidden".into()),
                filter: Filter {
                    root: Match::All,
                    items: vec![row("name", "A-WALL*")],
                },
            },
        ];
        write_layer_filters(&mut document, &filters);
        assert_eq!(stored_layer_filters(&document), filters);
        // A group beside them is left alone.
        super::super::layer_groups::store_layer_group(&mut document, "Walls", &[]);
        write_layer_filters(&mut document, &filters[..1]);
        assert_eq!(stored_layer_filters(&document).len(), 1);
        assert_eq!(super::super::layer_groups::layer_groups(&document).len(), 1);
        // Through a DXF file.
        let path =
            std::env::temp_dir().join(format!("ocs-layer-filters-{}.dxf", std::process::id()));
        crate::io::save(&document, &path).expect("save");
        let back = crate::io::load_file(&path).expect("load");
        let _ = std::fs::remove_file(&path);
        assert_eq!(stored_layer_filters(&back), filters[..1]);
    }

    #[test]
    fn expressions_read_and_written_the_way_other_programs_do() {
        let filter = Filter {
            root: Match::All,
            items: vec![row("name", "A-*"), row("plot", "0")],
        };
        assert_eq!(
            to_expression(&filter),
            "( NAME== \"A-*\" AND PLOTTABLE== \"False\" )"
        );
        // As the reference writes one, and with a group.
        let read = from_expression("( NAME== \"*1-*\" )").unwrap();
        assert!(matches!(&read.items[..], [Item::Condition(c)] if c.value == "*1-*"));
        let grouped = from_expression("NAME==\"A*\" AND (COLOR==\"1\" OR COLOR==\"2\")").unwrap();
        assert!(matches!(&grouped.items[1], Item::Group(g) if g.mode == Match::Any && g.conditions.len() == 2));
        assert_eq!(from_expression(&to_expression(&grouped)), Some(grouped));
        assert_eq!(from_expression(&to_expression(&filter)), Some(filter));
        let any = from_expression("COLOR==\"1\" OR NOT LOCKED==\"True\"").unwrap();
        assert_eq!(any.root, Match::Any);
        assert!(matches!(&any.items[1], Item::Condition(c) if c.negate && c.value == "1"));
        assert_eq!(from_expression("SOMETHING ELSE"), None);
    }
}
