//! A filter read back as one sentence, so it needs no explanation:
//! "Layer = FAL* and (Line or Polyline) and not Linetype = HIDDEN".

use super::{Condition, Filter, Item, Match, Op};

/// How the sentence names types and properties; the caller knows the labels.
pub struct Labels<'a> {
    pub type_label: &'a dyn Fn(&str) -> String,
    pub property_label: &'a dyn Fn(&str) -> String,
}

pub fn op_symbol(op: Op) -> &'static str {
    match op {
        Op::Any => "",
        Op::Eq => "=",
        Op::Ne => "!=",
        Op::Gt => ">",
        Op::Ge => ">=",
        Op::Lt => "<",
        Op::Le => "<=",
    }
}

fn joiner(mode: Match) -> String {
    let word = match mode {
        Match::All => crate::t!("and"),
        Match::Any => crate::t!("or"),
    };
    format!(" {word} ")
}

/// One condition: "Line · Layer = FAL*", "Line", "Layer = FAL*", with "not"
/// in front when it is negated.
pub fn describe_condition(condition: &Condition, labels: &Labels) -> String {
    let type_part = condition
        .type_filter
        .as_deref()
        .map(|t| (labels.type_label)(t));
    let property_part = condition
        .property
        .as_deref()
        .filter(|_| condition.op != Op::Any)
        .map(|property| {
            format!(
                "{} {} {}",
                (labels.property_label)(property),
                op_symbol(condition.op),
                condition.value
            )
        });
    let body = match (type_part, property_part) {
        (Some(t), Some(p)) => format!("{t} · {p}"),
        (Some(t), None) => t,
        (None, Some(p)) => p,
        (None, None) => crate::t!("(Any type)").into_owned(),
    };
    if condition.negate {
        format!("{} {body}", crate::t!("not"))
    } else {
        body
    }
}

pub fn describe(filter: &Filter, labels: &Labels) -> String {
    let parts: Vec<String> = filter
        .items
        .iter()
        .map(|item| match item {
            Item::Condition(condition) => describe_condition(condition, labels),
            Item::Group(group) => {
                let inner: Vec<String> = group
                    .conditions
                    .iter()
                    .map(|condition| describe_condition(condition, labels))
                    .collect();
                if inner.len() > 1 && filter.items.len() > 1 {
                    format!("({})", inner.join(&joiner(group.mode)))
                } else {
                    inner.join(&joiner(group.mode))
                }
            }
        })
        .filter(|part| !part.is_empty())
        .collect();
    parts.join(&joiner(filter.root))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::Group;

    fn cond(t: Option<&str>, p: Option<&str>, op: Op, v: &str, negate: bool) -> Condition {
        Condition {
            negate,
            type_filter: t.map(str::to_owned),
            property: p.map(str::to_owned),
            op,
            value: v.into(),
        }
    }

    #[test]
    fn the_mockup_example_reads_as_one_sentence() {
        let filter = Filter {
            root: Match::All,
            items: vec![
                Item::Condition(cond(None, Some("layer"), Op::Eq, "FAL*", false)),
                Item::Group(Group {
                    mode: Match::Any,
                    conditions: vec![
                        cond(Some("Line"), None, Op::Eq, "", false),
                        cond(Some("Polyline"), None, Op::Eq, "", false),
                    ],
                }),
                Item::Condition(cond(None, Some("linetype"), Op::Eq, "HIDDEN", true)),
            ],
        };
        let type_label = |t: &str| t.to_string();
        let property_label = |p: &str| match p {
            "layer" => "Layer".to_string(),
            "linetype" => "Linetype".to_string(),
            other => other.to_string(),
        };
        let labels = Labels {
            type_label: &type_label,
            property_label: &property_label,
        };
        assert_eq!(
            describe(&filter, &labels),
            "Layer = FAL* and (Line or Polyline) and not Linetype = HIDDEN"
        );
    }
}
