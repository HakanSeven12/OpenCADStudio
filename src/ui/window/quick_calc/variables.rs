//! QuickCalc's variables: every one is global
//! (kept in the user settings, shared by all drawings), named with a `$`,
//! and either a constant (calculated once, stored as a value) or a function
//! (stored as an expression, recalculated each time it is used).

use std::collections::{HashMap, HashSet};

use glam::DVec3;
use serde::{Deserialize, Serialize};

use super::QuickCalcPalette;
use crate::calc::{self, CalcContext, CalcError, Definition, SysVars, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VarKind {
    Constant,
    Function,
}

/// A constant's stored value.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredValue {
    Number(f64),
    Point([f64; 3]),
}

impl From<Value> for StoredValue {
    fn from(value: Value) -> Self {
        match value {
            Value::Number(n) => StoredValue::Number(n),
            Value::Point(p) => StoredValue::Point(p.to_array()),
        }
    }
}

impl From<StoredValue> for Value {
    fn from(value: StoredValue) -> Self {
        match value {
            StoredValue::Number(n) => Value::Number(n),
            StoredValue::Point(p) => Value::Point(DVec3::from_array(p)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Variable {
    /// Lowercase, without the `$`.
    pub name: String,
    pub kind: VarKind,
    /// A constant's value; functions have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<StoredValue>,
    /// A function's expression, or the text a constant was calculated from.
    #[serde(default)]
    pub expr: String,
    pub category: String,
    #[serde(default)]
    pub description: String,
}

/// The inline Variable Definition form.
#[derive(Debug, Clone, PartialEq)]
pub struct VarForm {
    /// The variable being edited (its name before any rename); `None` for a
    /// new one.
    pub editing: Option<String>,
    pub kind: VarKind,
    pub name: String,
    pub category: String,
    /// `Some` once "+ New" is chosen: the new category's name.
    pub new_category: Option<String>,
    pub expr: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub enum FormMsg {
    Kind(VarKind),
    Name(String),
    Category(String),
    NewCategory,
    NewCategoryName(String),
    Expr(String),
    Description(String),
    Save,
    Cancel,
}

#[derive(Debug, Clone)]
pub enum VarMsg {
    New,
    Edit,
    Delete,
    /// Return the selected variable to the input.
    Return,
    Select(String),
    /// Double-click: insert `$name` into the input.
    Insert(String),
    ToggleCategory(String),
    Form(FormMsg),
}

/// The category a variable gets when nothing else says.
pub fn general() -> String {
    crate::t!("General").into_owned()
}

impl QuickCalcPalette {
    pub fn variable(&self, name: &str) -> Option<&Variable> {
        self.variables.iter().find(|v| v.name == name)
    }

    /// Categories in the order their first variable appears.
    pub fn categories(&self) -> Vec<String> {
        let mut seen = Vec::new();
        for v in &self.variables {
            if !seen.contains(&v.category) {
                seen.push(v.category.clone());
            }
        }
        seen
    }

    /// The evaluation context with every variable defined, and the error of
    /// each function that can't be calculated.
    pub fn context_and_errors(
        &self,
        base_unit: f64,
        sysvars: Option<SysVars>,
    ) -> (CalcContext, HashMap<String, CalcError>) {
        self.context_without(None, base_unit, sysvars)
    }

    /// The context with every variable but `skip` (the one being edited).
    fn context_without(
        &self,
        skip: Option<&str>,
        base_unit: f64,
        sysvars: Option<SysVars>,
    ) -> (CalcContext, HashMap<String, CalcError>) {
        let mut ctx = CalcContext::new(self.mode, base_unit);
        ctx.sysvars = sysvars;
        let definitions = self
            .variables
            .iter()
            .filter(move |v| Some(v.name.as_str()) != skip)
            .filter_map(|v| {
                let definition = match v.kind {
                    VarKind::Constant => Definition::Constant(v.value?.into()),
                    VarKind::Function => Definition::Function(&v.expr),
                };
                Some((v.name.as_str(), definition))
            });
        let errors = calc::define_variables(&mut ctx, definitions);
        ctx.ans = self.ans;
        (ctx, errors)
    }

    /// The error typing `$name = …` gets when `$name` is a function: a
    /// function changes only through Variables.
    pub fn assignment_error(&self, input: &str) -> Option<CalcError> {
        let (name, span) = calc::assigned_variable(input)?;
        (self.variable(&name)?.kind == VarKind::Function).then(|| CalcError {
            span,
            message: crate::tf!("${} is a function. Edit it in Variables", name).into_owned(),
        })
    }

    /// Keep what `$name = rhs` set: a new constant in General, or a new value
    /// (and source text) for an existing one. The variable becomes selected.
    pub fn assign(&mut self, name: &str, value: Value, input: &str) {
        let source = input
            .split_once('=')
            .map_or("", |(_, rhs)| rhs)
            .trim()
            .to_string();
        match self.variables.iter_mut().find(|v| v.name == name) {
            Some(v) => {
                v.value = Some(value.into());
                v.expr = source;
            }
            None => self.variables.push(Variable {
                name: name.to_string(),
                kind: VarKind::Constant,
                value: Some(value.into()),
                expr: source,
                category: general(),
                description: String::new(),
            }),
        }
        self.selected_var = Some(name.to_string());
    }

    /// Delete a variable at once. Returns its name for the status message.
    pub fn delete_selected(&mut self) -> Option<String> {
        let name = self.selected_var.take()?;
        self.variables.retain(|v| v.name != name);
        if self
            .form
            .as_ref()
            .is_some_and(|f| f.editing.as_deref() == Some(name.as_str()))
        {
            self.form = None;
        }
        Some(name)
    }

    /// New: an empty form in the selected variable's category, holding the
    /// input's result when the input has one.
    pub fn open_new_form(&mut self, input_value: Option<Value>) {
        let category = self
            .selected_var
            .as_deref()
            .and_then(|n| self.variable(n))
            .map_or_else(general, |v| v.category.clone());
        self.form = Some(VarForm {
            editing: None,
            kind: VarKind::Constant,
            name: String::new(),
            category,
            new_category: None,
            expr: input_value.map(|v| v.display()).unwrap_or_default(),
            description: String::new(),
        });
    }

    /// Edit: the form filled from the selected variable.
    pub fn open_edit_form(&mut self) {
        let Some(v) = self.selected_var.as_deref().and_then(|n| self.variable(n)) else {
            return;
        };
        let expr = match (&v.kind, v.value) {
            (VarKind::Constant, Some(value)) if v.expr.trim().is_empty() => {
                Value::from(value).display()
            }
            _ => v.expr.clone(),
        };
        self.form = Some(VarForm {
            editing: Some(v.name.clone()),
            kind: v.kind,
            name: v.name.clone(),
            category: v.category.clone(),
            new_category: None,
            expr,
            description: v.description.clone(),
        });
    }

    /// The form's value, or the first thing wrong with it.
    pub fn check_form(&self, base_unit: f64, sysvars: Option<SysVars>) -> Result<Value, String> {
        let Some(form) = &self.form else {
            return Err(String::new());
        };
        let name = form.name.trim().to_ascii_lowercase();
        if !calc::is_valid_name(&name) {
            return Err(
                crate::t!("Start the name with a letter; use letters, digits and _").into_owned(),
            );
        }
        if form.editing.as_deref() != Some(name.as_str()) && self.variable(&name).is_some() {
            return Err(crate::tf!("${} already exists", name).into_owned());
        }
        if form
            .new_category
            .as_deref()
            .is_some_and(|c| c.trim().is_empty())
        {
            return Err(crate::t!("Name the new category").into_owned());
        }
        let expr = form.expr.trim();
        if expr.is_empty() {
            return Err(match form.kind {
                VarKind::Constant => crate::t!("Type a value"),
                VarKind::Function => crate::t!("Type an expression"),
            }
            .into_owned());
        }
        if calc::assigned_variable(expr).is_some() {
            return Err(crate::t!("Leave out the $name = part").into_owned());
        }
        if form.kind == VarKind::Function && calc::uses_variable(expr, &name) {
            return Err(crate::t!("A function can't use itself").into_owned());
        }
        let (ctx, _) = self.context_without(form.editing.as_deref(), base_unit, sysvars);
        calc::value_of(expr, &ctx).map_err(|e| e.message)
    }

    /// Save the form if it is valid: a constant keeps its calculated value, a
    /// function its expression. Renaming is allowed. Returns the saved name.
    pub fn save_form(&mut self, base_unit: f64, sysvars: Option<SysVars>) -> Option<String> {
        let value = self.check_form(base_unit, sysvars).ok()?;
        let form = self.form.take()?;
        let name = form.name.trim().to_ascii_lowercase();
        let category = form
            .new_category
            .map(|c| c.trim().to_string())
            .unwrap_or(form.category);
        let variable = Variable {
            name: name.clone(),
            kind: form.kind,
            value: (form.kind == VarKind::Constant).then(|| value.into()),
            expr: form.expr.trim().to_string(),
            category,
            description: form.description.trim().to_string(),
        };
        let slot = form
            .editing
            .as_deref()
            .and_then(|old| self.variables.iter().position(|v| v.name == old));
        match slot {
            Some(i) => self.variables[i] = variable,
            None => self.variables.push(variable),
        }
        self.selected_var = Some(name.clone());
        Some(name)
    }

    pub fn form_msg(&mut self, msg: FormMsg) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        match msg {
            FormMsg::Kind(kind) => form.kind = kind,
            FormMsg::Name(name) => form.name = name.replace('$', ""),
            FormMsg::Category(category) => {
                form.category = category;
                form.new_category = None;
            }
            FormMsg::NewCategory => form.new_category = Some(String::new()),
            FormMsg::NewCategoryName(name) => form.new_category = Some(name),
            FormMsg::Expr(expr) => form.expr = expr,
            FormMsg::Description(text) => form.description = text,
            FormMsg::Save | FormMsg::Cancel => {}
        }
    }

    pub fn toggle_category(&mut self, category: String) {
        if !self.collapsed.remove(&category) {
            self.collapsed.insert(category);
        }
    }
}

/// Variables from the settings: valid, lowercase, unique names.
pub fn clean(variables: &[Variable]) -> Vec<Variable> {
    let mut seen = HashSet::new();
    variables
        .iter()
        .filter_map(|v| {
            let name = v.name.trim_start_matches('$').to_ascii_lowercase();
            let usable = match v.kind {
                VarKind::Constant => v.value.is_some(),
                VarKind::Function => !v.expr.trim().is_empty(),
            };
            (calc::is_valid_name(&name) && usable && seen.insert(name.clone())).then(|| Variable {
                name,
                category: if v.category.trim().is_empty() {
                    general()
                } else {
                    v.category.clone()
                },
                ..v.clone()
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constant(name: &str, n: f64, category: &str) -> Variable {
        Variable {
            name: name.into(),
            kind: VarKind::Constant,
            value: Some(StoredValue::Number(n)),
            expr: n.to_string(),
            category: category.into(),
            description: String::new(),
        }
    }

    fn function(name: &str, expr: &str) -> Variable {
        Variable {
            name: name.into(),
            kind: VarKind::Function,
            value: None,
            expr: expr.into(),
            category: "Walls".into(),
            description: String::new(),
        }
    }

    fn palette() -> QuickCalcPalette {
        QuickCalcPalette {
            variables: vec![
                constant("brick", 210.0, "Walls"),
                constant("joint", 10.0, "Walls"),
                function("module", "$brick + $joint"),
                function("broken", "$nope"),
            ],
            ..Default::default()
        }
    }

    fn submit(p: &mut QuickCalcPalette, input: &str) -> Option<Value> {
        p.input = input.into();
        let (ctx, _) = p.context_and_errors(1.0, None);
        p.submit(&ctx).map(|s| s.value)
    }

    #[test]
    fn typing_an_assignment() {
        let mut p = palette();
        // A new name is a constant in General, keeping its source text.
        assert_eq!(
            submit(&mut p, "$Wall = 2 * 150"),
            Some(Value::Number(300.0))
        );
        let wall = p.variable("wall").unwrap();
        assert_eq!(
            (
                wall.kind,
                wall.value,
                wall.expr.as_str(),
                wall.category.as_str()
            ),
            (
                VarKind::Constant,
                Some(StoredValue::Number(300.0)),
                "2 * 150",
                "General"
            )
        );
        assert_eq!(p.selected_var.as_deref(), Some("wall"));
        // An existing constant gets a new value, keeping its category.
        submit(&mut p, "$brick = 215");
        let brick = p.variable("brick").unwrap();
        assert_eq!(brick.value, Some(StoredValue::Number(215.0)));
        assert_eq!(brick.category, "Walls");
        // A function follows the new value; it can't be assigned.
        let (ctx, errors) = p.context_and_errors(1.0, None);
        assert_eq!(ctx.vars["module"], Value::Number(225.0));
        assert_eq!(errors["broken"].message, "Unknown variable $nope");
        assert_eq!(submit(&mut p, "$module = 1"), None);
        assert_eq!(
            p.preview(&ctx),
            super::super::Preview::Err(CalcError {
                span: 0..7,
                message: "$module is a function. Edit it in Variables".into()
            })
        );
        assert_eq!(p.variable("module").unwrap().expr, "$brick + $joint");
    }

    fn form(p: &mut QuickCalcPalette, kind: VarKind, name: &str, expr: &str) {
        p.form_msg(FormMsg::Kind(kind));
        p.form_msg(FormMsg::Name(name.into()));
        p.form_msg(FormMsg::Expr(expr.into()));
    }

    #[test]
    fn form_validation() {
        let mut p = palette();
        p.selected_var = Some("joint".into());
        p.open_new_form(Some(Value::Number(42.0)));
        assert_eq!(p.form.as_ref().unwrap().category, "Walls");
        assert_eq!(p.form.as_ref().unwrap().expr, "42");
        let problem = |p: &QuickCalcPalette| p.check_form(1.0, None).err();
        for (kind, name, expr, wrong) in [
            (VarKind::Constant, "1x", "1", "Start the name"),
            (VarKind::Constant, "brick", "1", "$brick already exists"),
            (VarKind::Constant, "x", "", "Type a value"),
            (
                VarKind::Constant,
                "x",
                "$x = 2",
                "Leave out the $name = part",
            ),
            (
                VarKind::Function,
                "x",
                "$x + 1",
                "A function can't use itself",
            ),
            (VarKind::Constant, "x", "$nope", "Unknown variable $nope"),
        ] {
            form(&mut p, kind, name, expr);
            assert!(
                problem(&p).is_some_and(|e| e.starts_with(wrong)),
                "{name} {expr}"
            );
        }
        form(&mut p, VarKind::Constant, "$Course", "$module * 2");
        assert_eq!(p.form.as_ref().unwrap().name, "Course");
        p.form_msg(FormMsg::NewCategory);
        assert!(problem(&p).is_some());
        p.form_msg(FormMsg::NewCategoryName("Stairs".into()));
        assert_eq!(p.save_form(1.0, None).as_deref(), Some("course"));
        let course = p.variable("course").unwrap();
        assert_eq!(course.value, Some(StoredValue::Number(440.0)));
        assert_eq!(course.category, "Stairs");
        assert_eq!(p.categories(), vec!["Walls", "Stairs"]);
    }

    #[test]
    fn edit_rename_and_delete() {
        let mut p = palette();
        p.selected_var = Some("module".into());
        p.open_edit_form();
        assert_eq!(p.form.as_ref().unwrap().expr, "$brick + $joint");
        // Renaming keeps its place in the list; functions that used the old
        // name break until they are fixed.
        form(&mut p, VarKind::Function, "unit", "$brick + 2 * $joint");
        assert_eq!(p.save_form(1.0, None).as_deref(), Some("unit"));
        assert_eq!(p.variables[2].name, "unit");
        let (ctx, _) = p.context_and_errors(1.0, None);
        assert_eq!(ctx.vars["unit"], Value::Number(230.0));
        assert_eq!(p.delete_selected().as_deref(), Some("unit"));
        assert!(p.variable("unit").is_none());
        assert_eq!(p.selected_var, None);
    }

    #[test]
    fn settings_round_trip() {
        let mut p = palette();
        p.variables.push(Variable {
            value: Some(StoredValue::Point([1.0, 2.0, 0.0])),
            ..constant("origin", 0.0, "")
        });
        p.toggle_angle_mode();
        p.set_keypad_mode(super::super::KeypadMode::Scientific);
        p.toggle(super::super::Section::Variables);
        let json = serde_json::to_string(&p.settings()).unwrap();
        let mut q = QuickCalcPalette::default();
        q.apply_settings(&serde_json::from_str(&json).unwrap());
        assert_eq!(q.variables.len(), 5);
        assert_eq!(q.mode, p.mode);
        assert_eq!(q.mode, crate::calc::AngleMode::Radians);
        assert_eq!(q.keypad_mode, p.keypad_mode);
        assert_eq!(q.open, [false, true]);
        assert_eq!(q.variables[4].category, "General");
        // Broken entries from a hand-edited file are dropped.
        let mut s = p.settings();
        s.variables.push(constant("1bad", 1.0, "X"));
        s.variables.push(constant("BRICK", 1.0, "X"));
        q.apply_settings(&s);
        assert_eq!(q.variables.len(), 5);
    }
}
