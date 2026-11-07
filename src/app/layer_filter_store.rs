//! Layer property filters kept in the drawing (#2610): one `AcLyLayerFilter`
//! XRECORD per filter under anonymous `*A<n>` keys in the layer table's
//! `ACLYDICTIONARY`, beside the layer groups. A record carries the filter's
//! name (`300`), its expression in the form other programs read (`301`,
//! `NAME=="A*" AND PLOTTABLE=="False"`), the filter it sits under (`302`),
//! and the exact filter as JSON in `3` chunks. A record without the JSON is
//! read from its expression; one that can't be read is left as it is.

use crate::filter::{Condition, Filter, Item, Match, Op};
use document::objects::{DictionaryCloningFlags, ObjectType, XRecord, XRecordEntry, XRecordValue};
use document::{Document, Handle};

const KIND: &str = "AcLyLayerFilter";
const VERSION: i32 = 1;
/// A string of an XRECORD stays under 255 bytes as an older DXF writes it.
const CHUNK: usize = 240;

/// A filter as the drawing keeps it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StoredLayerFilter {
    pub name: String,
    /// The filter it sits under, if any.
    pub parent: Option<String>,
    pub filter: Filter,
}

fn parse(xrecord: &XRecord) -> Option<StoredLayerFilter> {
    let mut entries = xrecord.entries.iter();
    let kind = entries.next()?;
    if kind.code != 1 || kind.value.as_string() != Some(KIND) {
        return None;
    }
    let (mut name, mut expression, mut parent, mut json) = (None, None, None, String::new());
    for entry in entries {
        match (entry.code, &entry.value) {
            (300, XRecordValue::String(v)) if name.is_none() => name = Some(v.clone()),
            (301, XRecordValue::String(v)) => expression = Some(v.clone()),
            (302, XRecordValue::String(v)) if !v.is_empty() => parent = Some(v.clone()),
            (3, XRecordValue::String(part)) => json.push_str(part),
            _ => {}
        }
    }
    let filter = if json.is_empty() {
        from_expression(expression.as_deref()?)?
    } else {
        serde_json::from_str(&json).ok()?
    };
    Some(StoredLayerFilter {
        name: name?,
        parent,
        filter,
    })
}

/// The filter records of the drawing, with their handles, in order.
fn records(document: &Document) -> Vec<(Handle, StoredLayerFilter)> {
    let Some(dictionary) = super::layer_groups::filter_dictionary(document) else {
        return Vec::new();
    };
    let Some(ObjectType::Dictionary(dictionary)) = document.objects.get(&dictionary) else {
        return Vec::new();
    };
    dictionary
        .entries
        .iter()
        .filter_map(|(_, handle)| match document.objects.get(handle) {
            Some(ObjectType::XRecord(xrecord)) => parse(xrecord).map(|f| (*handle, f)),
            _ => None,
        })
        .collect()
}

/// The drawing's layer property filters.
pub(crate) fn stored_layer_filters(document: &Document) -> Vec<StoredLayerFilter> {
    records(document).into_iter().map(|(_, f)| f).collect()
}

/// Put `filters` in the drawing in place of the ones it has; records it
/// can't read stay untouched.
pub(crate) fn write_layer_filters(document: &mut Document, filters: &[StoredLayerFilter]) {
    let old: Vec<Handle> = records(document).into_iter().map(|(h, _)| h).collect();
    if let Some(dictionary) = super::layer_groups::filter_dictionary(document) {
        if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&dictionary) {
            dictionary.entries.retain(|(_, h)| !old.contains(h));
        }
    }
    for handle in old {
        document.objects.remove(&handle);
    }
    if filters.is_empty() {
        return;
    }
    let dictionary = super::layer_groups::ensure_filter_dictionary(document);
    for stored in filters {
        let handle = document.allocate_handle();
        let mut xrecord = XRecord::new();
        xrecord.handle = handle;
        xrecord.owner = dictionary;
        xrecord.reactors = vec![dictionary];
        xrecord.cloning_flags = DictionaryCloningFlags::KeepExisting;
        xrecord.entries = entries(stored);
        document
            .objects
            .insert(handle, ObjectType::XRecord(xrecord));
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
    }
}

fn entries(stored: &StoredLayerFilter) -> Vec<XRecordEntry> {
    let mut entries = vec![
        XRecordEntry::string(1, KIND),
        XRecordEntry::new(90, XRecordValue::Int32(VERSION)),
        XRecordEntry::string(300, &stored.name),
        XRecordEntry::string(301, &cut(&to_expression(&stored.filter))),
        XRecordEntry::string(302, stored.parent.as_deref().unwrap_or("")),
    ];
    let json = serde_json::to_string(&stored.filter).unwrap_or_default();
    let mut part = String::new();
    let mut length = 0;
    for c in json.chars() {
        let width = c.len_utf8();
        if length + width > CHUNK {
            entries.push(XRecordEntry::string(3, &part));
            part.clear();
            length = 0;
        }
        part.push(c);
        length += width;
    }
    if !part.is_empty() {
        entries.push(XRecordEntry::string(3, &part));
    }
    entries
}

/// An expression longer than a record's string is cut; the JSON beside it
/// keeps the whole filter.
fn cut(expression: &str) -> String {
    let mut out = String::new();
    for c in expression.chars() {
        if out.len() + c.len_utf8() > CHUNK {
            break;
        }
        out.push(c);
    }
    out
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
    let body = format!("{key}{op}\"{value}\"");
    Some(if condition.negate {
        format!("NOT {body}")
    } else {
        body
    })
}

/// The filter in the expression form: `NAME=="A*" AND (COLOR=="1" OR …)`.
pub(crate) fn to_expression(filter: &Filter) -> String {
    let join = |mode: Match| if mode == Match::All { " AND " } else { " OR " };
    filter
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
                (!parts.is_empty()).then(|| format!("({})", parts.join(join(g.mode))))
            }
        })
        .collect::<Vec<_>>()
        .join(join(filter.root))
}

/// Read an expression another program wrote: conditions joined by AND or
/// OR (one kind at the top), each `KEY=="value"` or `KEY!="value"`,
/// optionally led by NOT. Anything else gives `None`.
pub(crate) fn from_expression(expression: &str) -> Option<Filter> {
    let text = expression.trim();
    if text.is_empty() {
        return Some(Filter::default());
    }
    let upper = text.to_ascii_uppercase();
    let (root, separator) = if upper.contains(" OR ") && !upper.contains(" AND ") {
        (Match::Any, " OR ")
    } else {
        (Match::All, " AND ")
    };
    let mut items = Vec::new();
    let mut rest = text;
    loop {
        let at = rest.to_ascii_uppercase().find(separator);
        let (part, next) = match at {
            Some(at) => (&rest[..at], Some(&rest[at + separator.len()..])),
            None => (rest, None),
        };
        items.push(Item::Condition(parse_condition(part.trim())?));
        match next {
            Some(next) => rest = next,
            None => break,
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
            "NAME==\"A-*\" AND PLOTTABLE==\"False\""
        );
        assert_eq!(from_expression(&to_expression(&filter)), Some(filter));
        let any = from_expression("COLOR==\"1\" OR NOT LOCKED==\"True\"").unwrap();
        assert_eq!(any.root, Match::Any);
        assert!(matches!(&any.items[1], Item::Condition(c) if c.negate && c.value == "1"));
        assert_eq!(from_expression("SOMETHING ELSE"), None);
    }
}
