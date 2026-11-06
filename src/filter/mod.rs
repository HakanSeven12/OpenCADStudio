//! The one filter engine behind Quick Select, Filter and the Layer manager's
//! property filters (#2611, #2610).
//!
//! A [`Filter`] is a root that matches all or any of its items; an item is a
//! condition or a group one level deep with its own all / any. A condition
//! names an optional object type, an optional property, an operator and a
//! value, and can be negated. What a "property" is comes from a
//! [`PropertySource`], so the same filter runs over drawing objects or over
//! layers.

use serde::{Deserialize, Serialize};

pub mod compare;
pub mod describe;
pub mod qselect;

/// How a root or a group combines its items.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Match {
    #[default]
    All,
    Any,
}

/// Operator between an object's property value and the condition's value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op {
    /// Any value: only the type test applies.
    Any,
    #[default]
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

/// One test: "an object of `type_filter` whose `property` `op` `value`".
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    /// The test's result is turned around.
    pub negate: bool,
    /// `None` = any type. An object of another type does not match.
    pub type_filter: Option<String>,
    /// Property id (`"layer"`, `"radius"` …); `None` = only the type test.
    pub property: Option<String>,
    pub op: Op,
    pub value: String,
}

/// Conditions combined by their own all / any. Groups don't nest.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub mode: Match,
    pub conditions: Vec<Condition>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Item {
    Condition(Condition),
    Group(Group),
}

/// A whole filter. An empty one matches everything.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    pub root: Match,
    pub items: Vec<Item>,
}

/// Where a filter reads an item's type and properties.
pub trait PropertySource {
    type Item: ?Sized;
    fn type_name<'a>(&'a self, item: &'a Self::Item) -> &'a str;
    /// The property's value as it is shown, `None` when the item has none.
    fn value(&self, item: &Self::Item, property: &str) -> Option<String>;
}

impl Condition {
    pub fn matches<S: PropertySource + ?Sized>(&self, source: &S, item: &S::Item) -> bool {
        self.test(source, item) != self.negate
    }

    fn test<S: PropertySource + ?Sized>(&self, source: &S, item: &S::Item) -> bool {
        if let Some(wanted) = &self.type_filter {
            if source.type_name(item) != wanted {
                return false;
            }
        }
        let Some(property) = &self.property else {
            return true;
        };
        if self.op == Op::Any {
            return true;
        }
        // An object without the property matches no comparison, `!=` too;
        // NOT asks for "doesn't have it".
        source
            .value(item, property)
            .is_some_and(|actual| compare::test(self.op, &actual, &self.value))
    }
}

fn combine(mode: Match, mut results: impl Iterator<Item = bool>) -> bool {
    match mode {
        Match::All => results.all(|matched| matched),
        Match::Any => results.any(|matched| matched),
    }
}

impl Group {
    pub fn matches<S: PropertySource + ?Sized>(&self, source: &S, item: &S::Item) -> bool {
        self.conditions.is_empty()
            || combine(
                self.mode,
                self.conditions.iter().map(|c| c.matches(source, item)),
            )
    }
}

impl Item {
    pub fn matches<S: PropertySource + ?Sized>(&self, source: &S, item: &S::Item) -> bool {
        match self {
            Item::Condition(condition) => condition.matches(source, item),
            Item::Group(group) => group.matches(source, item),
        }
    }
}

impl Filter {
    pub fn matches<S: PropertySource + ?Sized>(&self, source: &S, item: &S::Item) -> bool {
        self.items.is_empty()
            || combine(
                self.root,
                self.items.iter().map(|i| i.matches(source, item)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test objects: (type, [(property, value)]).
    struct Objects;
    type Object = (&'static str, &'static [(&'static str, &'static str)]);

    impl PropertySource for Objects {
        type Item = Object;
        fn type_name<'a>(&'a self, item: &'a Object) -> &'a str {
            item.0
        }
        fn value(&self, item: &Object, property: &str) -> Option<String> {
            item.1
                .iter()
                .find(|(name, _)| *name == property)
                .map(|(_, value)| value.to_string())
        }
    }

    const WALL: Object = ("Line", &[("layer", "FAL-KULSO"), ("linetype", "ByLayer")]);
    const HIDDEN: Object = ("Line", &[("layer", "FAL-BELSO"), ("linetype", "HIDDEN")]);
    const OUTLINE: Object = (
        "Polyline",
        &[("layer", "FAL-KULSO"), ("linetype", "ByLayer")],
    );
    const DOOR: Object = ("Circle", &[("layer", "AJTO"), ("radius", "3.0000")]);

    fn cond(type_filter: Option<&str>, property: Option<&str>, op: Op, value: &str) -> Condition {
        Condition {
            negate: false,
            type_filter: type_filter.map(str::to_owned),
            property: property.map(str::to_owned),
            op,
            value: value.to_owned(),
        }
    }

    fn found(filter: &Filter) -> Vec<&'static str> {
        [
            ("wall", WALL),
            ("hidden", HIDDEN),
            ("outline", OUTLINE),
            ("door", DOOR),
        ]
        .into_iter()
        .filter(|(_, object)| filter.matches(&Objects, object))
        .map(|(name, _)| name)
        .collect()
    }

    #[test]
    fn empty_filter_and_empty_group_match_everything() {
        assert_eq!(found(&Filter::default()).len(), 4);
        let filter = Filter {
            root: Match::Any,
            items: vec![Item::Group(Group::default())],
        };
        assert_eq!(found(&filter).len(), 4);
    }

    /// The mockup's example: Layer = FAL* and (Line or Polyline) and not
    /// Linetype = HIDDEN.
    #[test]
    fn root_all_with_an_any_group_and_a_negated_row() {
        let mut hidden = cond(None, Some("linetype"), Op::Eq, "HIDDEN");
        hidden.negate = true;
        let filter = Filter {
            root: Match::All,
            items: vec![
                Item::Condition(cond(None, Some("layer"), Op::Eq, "FAL*")),
                Item::Group(Group {
                    mode: Match::Any,
                    conditions: vec![
                        cond(Some("Line"), None, Op::Eq, ""),
                        cond(Some("Polyline"), None, Op::Eq, ""),
                    ],
                }),
                Item::Condition(hidden),
            ],
        };
        assert_eq!(found(&filter), ["wall", "outline"]);
    }

    #[test]
    fn a_typed_condition_does_not_match_another_type() {
        let filter = Filter {
            root: Match::All,
            items: vec![Item::Condition(cond(
                Some("Circle"),
                Some("layer"),
                Op::Ne,
                "X",
            ))],
        };
        assert_eq!(found(&filter), ["door"]);
    }

    #[test]
    fn a_missing_property_matches_no_comparison_but_not_turns_it_around() {
        let mut radius = cond(None, Some("radius"), Op::Ne, "1");
        assert_eq!(
            found(&Filter {
                root: Match::All,
                items: vec![Item::Condition(radius.clone())]
            }),
            ["door"]
        );
        radius.op = Op::Gt;
        radius.value = "2".into();
        radius.negate = true;
        assert_eq!(
            found(&Filter {
                root: Match::All,
                items: vec![Item::Condition(radius)]
            }),
            ["wall", "hidden", "outline"]
        );
    }

    #[test]
    fn any_operator_is_a_type_test() {
        let filter = Filter {
            root: Match::All,
            items: vec![Item::Condition(cond(
                Some("Line"),
                Some("layer"),
                Op::Any,
                "",
            ))],
        };
        assert_eq!(found(&filter), ["wall", "hidden"]);
    }
}
