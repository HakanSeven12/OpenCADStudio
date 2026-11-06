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
                QSelectOp::Lt => Op::Lt,
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
}
