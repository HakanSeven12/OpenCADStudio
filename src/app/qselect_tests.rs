//! Quick Select validation: one rule source for the live dialog and Apply.

use super::*;

fn text_property() -> QSelectPropertyChoice {
    QSelectPropertyChoice {
        field: "layer".into(),
        label: "Layer".into(),
        editor: QSelectValueEditor::Text,
    }
}

fn number_property() -> QSelectPropertyChoice {
    QSelectPropertyChoice {
        field: "start_x".into(),
        label: "Start X".into(),
        editor: QSelectValueEditor::Number,
    }
}

fn choice_property() -> QSelectPropertyChoice {
    QSelectPropertyChoice {
        field: "linetype".into(),
        label: "Linetype".into(),
        editor: QSelectValueEditor::Choice(vec!["Continuous".into(), "Dashed".into()]),
    }
}

fn condition(
    property: Option<QSelectPropertyChoice>,
    operator: QSelectOp,
    value: &str,
) -> QSelectCondition {
    QSelectCondition {
        type_filter: Some("Line".into()),
        property,
        operator,
        value: value.into(),
    }
}

fn state(
    conditions: Vec<QSelectCondition>,
    editing_condition: Option<usize>,
    property: Option<QSelectPropertyChoice>,
    operator: QSelectOp,
    value: &str,
) -> QSelectState {
    QSelectState {
        conditions,
        editing_condition,
        scope: QSelectScope::CurrentSpace,
        available_types: vec!["Line".into()],
        available_properties: Vec::new(),
        candidate_count: 5,
        type_filter: Some("Line".into()),
        property,
        operator,
        value: value.into(),
        mode: QSelectMode::Include,
        append: false,
        error: None,
    }
}

#[test]
fn empty_scope_is_reported_before_any_condition_error() {
    let state = state(Vec::new(), None, Some(number_property()), QSelectOp::Eq, "abc");
    assert_eq!(
        state.validation_error(0),
        Some(crate::t!("No objects are available in this scope.").into_owned())
    );
}

#[test]
fn apply_is_blocked_while_a_condition_edit_is_open() {
    let state = state(
        vec![condition(Some(number_property()), QSelectOp::Eq, "2")],
        Some(0),
        Some(number_property()),
        QSelectOp::Eq,
        "2",
    );
    assert_eq!(
        state.validation_error(5),
        Some(
            crate::t!("Update or cancel the condition edit before applying.").into_owned()
        )
    );
}

#[test]
fn empty_condition_list_validates_the_draft() {
    let invalid = state(Vec::new(), None, Some(number_property()), QSelectOp::Eq, "abc");
    assert_eq!(
        invalid.validation_error(5),
        Some(crate::t!("Enter a valid number.").into_owned())
    );

    let valid = state(Vec::new(), None, Some(number_property()), QSelectOp::Eq, "2");
    assert_eq!(valid.validation_error(5), None);
}

#[test]
fn committed_conditions_report_the_first_error() {
    let state = state(
        vec![
            condition(Some(number_property()), QSelectOp::Eq, "2"),
            condition(Some(choice_property()), QSelectOp::Eq, ""),
        ],
        None,
        Some(number_property()),
        QSelectOp::Eq,
        "2",
    );
    assert_eq!(
        state.validation_error(5),
        Some(crate::t!("Choose a value.").into_owned())
    );
}

#[test]
fn borrowed_rule_pins_each_branch() {
    let enter_number = Some(crate::t!("Enter a valid number.").into_owned());
    let choose_value = Some(crate::t!("Choose a value.").into_owned());
    let needs_number = Some(crate::t!("This operator requires a numeric property.").into_owned());
    let choose_filter = Some(crate::t!("Choose an object type or property.").into_owned());
    let text = text_property();
    let number = number_property();
    let choice = choice_property();

    let cases = [
        (Some("Line"), Some(&text), QSelectOp::Eq, "", None),
        (Some("Line"), Some(&text), QSelectOp::Any, "", None),
        (Some("Line"), Some(&number), QSelectOp::Eq, "abc", enter_number.clone()),
        (Some("Line"), Some(&number), QSelectOp::Eq, "2", None),
        (Some("Line"), Some(&number), QSelectOp::Gt, "2", None),
        (Some("Line"), Some(&text), QSelectOp::Gt, "x", needs_number.clone()),
        (Some("Line"), Some(&choice), QSelectOp::Eq, "", choose_value.clone()),
        (Some("Line"), Some(&choice), QSelectOp::Eq, "Dashed", None),
        (Some("Line"), None, QSelectOp::Eq, "", None),
        (None, None, QSelectOp::Eq, "", choose_filter.clone()),
    ];

    for (type_filter, property, operator, value, expected) in cases {
        assert_eq!(
            QSelectCondition::validation_error_for(type_filter, property, operator, value),
            expected,
            "case {property:?} {operator:?} {value:?}"
        );
    }
}
