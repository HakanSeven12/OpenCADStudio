/// Operator the Quick Select filter applies between an entity's
/// property value and the user-typed test value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QSelectOp {
    /// `*Any value` — the entity matches as long as the type filter
    /// passes; the value column is ignored.
    Any,
    Eq,
    Neq,
    Gt,
    Ge,
    Lt,
    Le,
}

impl QSelectOp {
    /// Operators that order numbers, only offered for numeric properties.
    pub fn needs_number(self) -> bool {
        matches!(self, Self::Gt | Self::Ge | Self::Lt | Self::Le)
    }
}

/// Candidate set searched by Quick Select.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QSelectScope {
    CurrentSpace,
    CurrentSelection,
}

impl std::fmt::Display for QSelectScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let source = match self {
            QSelectScope::CurrentSpace => "Current space",
            QSelectScope::CurrentSelection => "Current selection",
        };
        f.write_str(crate::t!(source).as_ref())
    }
}

/// Whether matching candidates are kept or removed from the result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QSelectMode {
    Include,
    Exclude,
}

/// What Quick Select does with the objects it finds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum QSelectResult {
    /// They become the selection.
    #[default]
    New,
    /// They join the selection.
    Add,
    /// They leave the selection.
    Remove,
}

/// A picked object's properties ticked from the start: what describes it,
/// not where it sits (#2611).
pub(crate) const SAMPLE_TICKED_FIELDS: &[&str] = &["layer", "color", "linetype"];

/// The properties of a picked object, each ticked to become a condition.
#[derive(Clone, Debug)]
pub struct QSelectSample {
    pub type_name: String,
    pub rows: Vec<(QSelectCondition, bool)>,
}

/// Editor used by the Quick Select value field.
#[derive(Clone, Debug)]
pub enum QSelectValueEditor {
    Text,
    Number,
    Choice(Vec<String>),
}

impl std::fmt::Display for QSelectOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            QSelectOp::Any => "* Any value",
            QSelectOp::Eq => "= Equals",
            QSelectOp::Neq => "!= Not equal",
            QSelectOp::Gt => "> Greater than",
            QSelectOp::Ge => ">= Greater or equal",
            QSelectOp::Lt => "< Less than",
            QSelectOp::Le => "<= Less or equal",
        };
        f.write_str(crate::t!(s).as_ref())
    }
}

/// One row in the Quick Select Properties pick_list. `field` is the
/// stable identifier (`"layer"`, `"start_x"`, …) used to look up the
/// value on each candidate entity; `label` is the human label rendered
/// in the dropdown. Equality only compares `field` so the pick_list
/// round-trips selection correctly even when labels are duplicated.
#[derive(Clone, Debug)]
pub struct QSelectPropertyChoice {
    pub field: String,
    pub label: String,
    pub editor: QSelectValueEditor,
}

impl PartialEq for QSelectPropertyChoice {
    fn eq(&self, other: &Self) -> bool {
        self.field == other.field
    }
}

impl Eq for QSelectPropertyChoice {}

impl std::fmt::Display for QSelectPropertyChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// One committed condition in the Quick Select filter list.
#[derive(Clone, Debug)]
pub struct QSelectCondition {
    pub type_filter: Option<String>,
    pub property: Option<QSelectPropertyChoice>,
    pub operator: QSelectOp,
    pub value: String,
}

impl QSelectCondition {
    /// Condition rules over borrowed fields. The live dialog validates its
    /// draft through this form so it shares one rule with Apply without
    /// cloning the property choice (and its option list) every frame.
    pub fn validation_error_for(
        type_filter: Option<&str>,
        property: Option<&QSelectPropertyChoice>,
        operator: QSelectOp,
        value: &str,
    ) -> Option<String> {
        if type_filter.is_none() && property.is_none() {
            return Some(crate::t!("Choose an object type or property.").into_owned());
        }
        let Some(property) = property else {
            return None;
        };
        if matches!(operator, QSelectOp::Any) {
            return None;
        }
        if operator.needs_number() && !matches!(property.editor, QSelectValueEditor::Number) {
            return Some(crate::t!("This operator requires a numeric property.").into_owned());
        }
        match &property.editor {
            QSelectValueEditor::Number
                if crate::entities::common::parse_f64(value)
                    .is_none_or(|value| !value.is_finite()) =>
            {
                Some(crate::t!("Enter a valid number.").into_owned())
            }
            QSelectValueEditor::Choice(_) if value.is_empty() => {
                Some(crate::t!("Choose a value.").into_owned())
            }
            _ => None,
        }
    }

    pub fn validation_error(&self) -> Option<String> {
        Self::validation_error_for(
            self.type_filter.as_deref(),
            self.property.as_ref(),
            self.operator,
            &self.value,
        )
    }
}

/// Quick Select editor and committed conditions (combined with AND).
#[derive(Clone, Debug)]
pub struct QSelectState {
    pub conditions: Vec<QSelectCondition>,
    pub editing_condition: Option<usize>,
    pub scope: QSelectScope,
    /// Object types in the scope with how many of each.
    pub available_types: Vec<(String, usize)>,
    pub available_properties: Vec<QSelectPropertyChoice>,
    pub candidate_count: usize,
    /// `None` = "(Any type)".
    pub type_filter: Option<String>,
    /// `None` = no property filter; the type filter alone applies.
    pub property: Option<QSelectPropertyChoice>,
    pub operator: QSelectOp,
    pub value: String,
    pub mode: QSelectMode,
    pub result: QSelectResult,
    pub error: Option<String>,
    /// Objects the conditions find right now.
    pub live_count: usize,
    /// What the conditions find, by object type, most first.
    pub live_by_type: Vec<(String, usize)>,
    /// The matches are highlighted in the drawing.
    pub show: bool,
    /// Values of the chosen property in the scope, most frequent first.
    pub value_counts: Vec<(String, usize)>,
    /// A picked object's properties waiting to be ticked.
    pub sample: Option<QSelectSample>,
}

impl QSelectState {
    /// The conditions Select would apply: the list, or the draft while the
    /// list is empty and the draft is complete.
    pub fn effective_conditions(&self) -> Option<Vec<QSelectCondition>> {
        if !self.conditions.is_empty() {
            return Some(self.conditions.clone());
        }
        let draft = self.draft_condition();
        ((draft.type_filter.is_some() || draft.property.is_some())
            && draft.validation_error().is_none())
        .then(|| vec![draft])
    }

    pub fn normalize_operator(&mut self) {
        if self.operator.needs_number()
            && !self
                .property
                .as_ref()
                .is_some_and(|property| matches!(property.editor, QSelectValueEditor::Number))
        {
            self.operator = QSelectOp::Eq;
        }
    }

    pub fn draft_condition(&self) -> QSelectCondition {
        QSelectCondition {
            type_filter: self.type_filter.clone(),
            property: self.property.clone(),
            operator: self.operator,
            value: self.value.clone(),
        }
    }

    /// Error Apply would report. Apply passes a freshly counted scope size;
    /// the live dialog uses the stored count so it stays cheap per frame.
    pub fn validation_error(&self, candidate_count: usize) -> Option<String> {
        if candidate_count == 0 {
            return Some(crate::t!("No objects are available in this scope.").into_owned());
        }
        if self.editing_condition.is_some() {
            return Some(
                crate::t!("Update or cancel the condition edit before applying.").into_owned(),
            );
        }
        if self.conditions.is_empty() {
            self.draft_condition().validation_error()
        } else {
            self.conditions
                .iter()
                .find_map(QSelectCondition::validation_error)
        }
    }
}

#[derive(Clone, Debug)]
pub struct QSelectSettings {
    pub conditions: Vec<QSelectCondition>,
    pub scope: QSelectScope,
    pub type_filter: Option<String>,
    pub property_field: Option<String>,
    pub operator: QSelectOp,
    pub value: String,
    pub mode: QSelectMode,
    pub result: QSelectResult,
    pub show: bool,
}

impl From<&QSelectState> for QSelectSettings {
    fn from(state: &QSelectState) -> Self {
        Self {
            conditions: state.conditions.clone(),
            scope: state.scope,
            type_filter: state.type_filter.clone(),
            property_field: state
                .property
                .as_ref()
                .map(|property| property.field.clone()),
            operator: state.operator,
            value: state.value.clone(),
            mode: state.mode,
            result: state.result,
            show: state.show,
        }
    }
}
