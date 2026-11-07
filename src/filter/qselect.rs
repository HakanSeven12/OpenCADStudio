//! Quick Select's condition list as a [`Filter`].
//!
//! Quick Select's conditions apply per object type (#2611): an object must
//! pass every condition of its own type and every condition without a type;
//! when some conditions name a type, an object of a type none of them names
//! is left out. As a filter that is one all-group per named type, each also
//! holding the untyped conditions, under an any-root. With a single type (or
//! none) the group is the root itself.

use super::{Condition, Filter, Group, Item, Match, Op};
use crate::app::{QSelectCondition, QSelectOp};

impl From<&QSelectCondition> for Condition {
    fn from(condition: &QSelectCondition) -> Self {
        Condition {
            negate: false,
            type_filter: condition.type_filter.clone(),
            property: condition
                .property
                .as_ref()
                .map(|property| property.field.clone()),
            op: match condition.operator {
                QSelectOp::Any => Op::Any,
                QSelectOp::Eq => Op::Eq,
                QSelectOp::Neq => Op::Ne,
                QSelectOp::Gt => Op::Gt,
                QSelectOp::Ge => Op::Ge,
                QSelectOp::Lt => Op::Lt,
                QSelectOp::Le => Op::Le,
            },
            value: condition.value.clone(),
        }
    }
}

pub fn filter_from_conditions(conditions: &[QSelectCondition]) -> Filter {
    let untyped: Vec<Condition> = conditions
        .iter()
        .filter(|condition| condition.type_filter.is_none())
        .map(Condition::from)
        .collect();
    // Types in the order they first appear, so the filter reads like the list.
    let mut groups: Vec<(&str, Vec<Condition>)> = Vec::new();
    for condition in conditions {
        let Some(type_name) = condition.type_filter.as_deref() else {
            continue;
        };
        match groups.iter_mut().find(|(name, _)| *name == type_name) {
            Some((_, group)) => group.push(condition.into()),
            None => groups.push((type_name, vec![condition.into()])),
        }
    }
    let with_untyped = |mut group: Vec<Condition>| {
        group.extend(untyped.iter().cloned());
        group
    };
    match groups.len() {
        0 | 1 => Filter {
            root: Match::All,
            items: with_untyped(groups.pop().map(|(_, group)| group).unwrap_or_default())
                .into_iter()
                .map(Item::Condition)
                .collect(),
        },
        _ => Filter {
            root: Match::Any,
            items: groups
                .into_iter()
                .map(|(_, group)| {
                    Item::Group(Group {
                        mode: Match::All,
                        conditions: with_untyped(group),
                    })
                })
                .collect(),
        },
    }
}

/// First whitespace-delimited word and the trimmed remainder.
fn split_word(text: &str) -> (&str, &str) {
    let text = text.trim();
    match text.find(char::is_whitespace) {
        Some(at) => (&text[..at], text[at..].trim()),
        None => (text, ""),
    }
}

/// Property names as typed on the command line to the engine's ids.
fn property_id(name: &str) -> String {
    match name.to_ascii_uppercase().as_str() {
        "TYPE" => "type",
        "LAYER" => "layer",
        "COLOR" => "color",
        "LINETYPE" => "linetype",
        "LINEWEIGHT" => "lineweight",
        "LINETYPESCALE" | "LTSCALE" => "linetype_scale",
        "PLOTSTYLE" => "plot_style",
        "TRANSPARENCY" => "transparency",
        "MATERIAL" => "material",
        "HYPERLINK" => "hyperlink",
        "HANDLE" => "handle",
        other => return other.to_ascii_lowercase(),
    }
    .to_string()
}

fn parse_op(word: &str) -> Option<Op> {
    Some(match word {
        "=" | "==" => Op::Eq,
        "!=" | "<>" => Op::Ne,
        "<" => Op::Lt,
        "<=" => Op::Le,
        ">" => Op::Gt,
        ">=" => Op::Ge,
        _ => return None,
    })
}

/// The command line's Quick Select: `PROPERTY [OP] VALUE`, optionally led by
/// `NOT`. Without an operator the value is matched with `=`, so wildcards
/// work everywhere; `NOT` turns the whole test around.
pub fn parse_command_line(rest: &str) -> Result<Filter, &'static str> {
    let rest = rest.trim();
    if rest.is_empty() {
        return Err("Usage: QSELECT [NOT] <PROPERTY> [=|!=|<|<=|>|>=] <VALUE>");
    }
    let mut negate = false;
    let (mut property, mut tail) = split_word(rest);
    if property.eq_ignore_ascii_case("NOT") {
        negate = true;
        (property, tail) = split_word(tail);
        if property.is_empty() {
            return Err("QSELECT: NOT needs a property and a value.");
        }
    }
    let (op, value) = match parse_op(split_word(tail).0) {
        Some(op) => (op, split_word(tail).1),
        None => (Op::Eq, tail),
    };
    let value = value.trim().trim_matches('"').to_string();
    if value.is_empty() {
        return Err("QSELECT: a value is needed.");
    }
    Ok(Filter {
        root: Match::All,
        items: vec![Item::Condition(Condition {
            negate,
            type_filter: None,
            property: Some(property_id(property)),
            op,
            value,
        })],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qs(type_filter: Option<&str>, field: Option<&str>, value: &str) -> QSelectCondition {
        QSelectCondition {
            type_filter: type_filter.map(str::to_owned),
            property: field.map(|field| crate::app::QSelectPropertyChoice {
                field: field.to_owned(),
                label: field.to_owned(),
                editor: crate::app::QSelectValueEditor::Text,
            }),
            operator: QSelectOp::Eq,
            value: value.to_owned(),
        }
    }

    #[test]
    fn untyped_conditions_stay_one_all_root() {
        let filter =
            filter_from_conditions(&[qs(None, Some("layer"), "A"), qs(None, Some("color"), "1")]);
        assert_eq!(filter.root, Match::All);
        assert_eq!(filter.items.len(), 2);
        assert!(filter_from_conditions(&[]).items.is_empty());
    }

    #[test]
    fn two_types_become_two_groups_under_any_each_with_the_untyped_ones() {
        let filter = filter_from_conditions(&[
            qs(Some("Line"), None, ""),
            qs(Some("Line"), Some("layer"), "FAL"),
            qs(None, Some("color"), "ByLayer"),
            qs(Some("Circle"), Some("radius"), "3"),
        ]);
        assert_eq!(filter.root, Match::Any);
        let groups: Vec<(Option<&str>, usize)> = filter
            .items
            .iter()
            .map(|item| match item {
                Item::Group(group) => {
                    assert_eq!(group.mode, Match::All);
                    (
                        group.conditions[0].type_filter.as_deref(),
                        group.conditions.len(),
                    )
                }
                Item::Condition(_) => panic!("only groups at the root"),
            })
            .collect();
        assert_eq!(groups, [(Some("Line"), 3), (Some("Circle"), 2)]);
    }

    fn parsed(rest: &str) -> Condition {
        let filter = parse_command_line(rest).expect(rest);
        assert_eq!(filter.root, Match::All);
        match filter.items.as_slice() {
            [Item::Condition(condition)] => condition.clone(),
            other => panic!("one condition expected: {other:?}"),
        }
    }

    #[test]
    fn command_line_reads_equality_wildcards_and_comparisons() {
        let equals = parsed("LAYER FAL*");
        assert_eq!(equals.property.as_deref(), Some("layer"));
        assert_eq!(equals.op, Op::Eq);
        assert_eq!(equals.value, "FAL*");
        assert!(!equals.negate);

        let greater = parsed("RADIUS >= 10");
        assert_eq!(greater.property.as_deref(), Some("radius"));
        assert_eq!(greater.op, Op::Ge);
        assert_eq!(greater.value, "10");

        let not_equal = parsed("LINETYPE <> HIDDEN");
        assert_eq!(not_equal.property.as_deref(), Some("linetype"));
        assert_eq!(not_equal.op, Op::Ne);
        assert_eq!(not_equal.value, "HIDDEN");
    }

    #[test]
    fn command_line_reads_not_and_quoted_values() {
        let negated = parsed("NOT TYPE LINE");
        assert!(negated.negate);
        assert_eq!(negated.property.as_deref(), Some("type"));
        assert_eq!(negated.value, "LINE");

        let quoted = parsed("LAYER \"My Layer\"");
        assert_eq!(quoted.property.as_deref(), Some("layer"));
        assert_eq!(quoted.value, "My Layer");
    }

    #[test]
    fn command_line_refuses_an_empty_property_or_value() {
        assert!(parse_command_line("").is_err());
        assert!(parse_command_line("NOT").is_err());
        assert!(parse_command_line("LAYER").is_err());
        assert!(parse_command_line("RADIUS >").is_err());
    }
}
