//! Native dock panel renderer for declarative plugin palettes.
//!
//! Renders persistent dockable side panels defined by plugins into native iced
//! widget trees with title bar, collapse pins, inputs, buttons, collapsible sections,
//! and data tables, integrated with OpenCADStudio's docking system.

use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_input, Space,
};
use iced::{Element, Fill, Length, Padding, Theme};
use ocs_plugin_api::dialog::{
    DialogControl, DialogValue, HorizontalAlign, PanelDef, PanelSnapshot, PanelStyle,
    VerticalAlign,
};
use std::collections::{HashMap, HashSet};

use crate::app::Message;
use crate::ui::dock::{title_bar, Chrome, PanelId};
use crate::ui::kit;
use crate::ui::style::{chrome, kit_metrics as km, text as ui_text};
use crate::ui::window::plugin_dialog::{
    apply_control_value_updates, collect_svg_handles, find_choice_option_in_controls, styled_text,
    svg_image, ChoiceOption, FormControlView,
};

/// Per-snapshot construction budget (P1-21): a panel snapshot was a complete,
/// unrestricted clone of every typed value, raw text and full table, and a
/// plugin could grow (or register) a table whose copy dominated the runner
/// read loop on every poll. These caps bound one snapshot's form walk; each
/// truncation is reported in the snapshot's `errors`, so a truncated snapshot
/// is honestly unsuitable for submission instead of silently lying.
const MAX_SNAPSHOT_TABLE_ROWS: usize = 4096;
const MAX_SNAPSHOT_TABLE_CELLS: usize = 65_536;
const MAX_SNAPSHOT_FIELDS_DEPTH: usize = 64;
const MAX_SNAPSHOT_VALUE_LEN: usize = 64 * 1024;

/// State backing an active declarative plugin dockable side panel.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginPanelState {
    pub def: PanelDef,
    /// Transport-authenticated owner. `None` is reserved for host-side/test
    /// registrations, which cannot be read back by an IPC plugin.
    pub owner_plugin_id: Option<String>,
    pub visible: bool,
    pub text_values: HashMap<String, String>,
    pub bool_values: HashMap<String, bool>,
    pub choice_values: HashMap<String, usize>,
    pub color_values: HashMap<String, [u8; 3]>,
    pub file_values: HashMap<String, String>,
    pub table_values: HashMap<String, Vec<Vec<String>>>,
    pub collapsed_sections: HashSet<String>,
    pub disabled_controls: HashSet<String>,
    pub hidden_controls: HashSet<String>,
    pub dynamic_choice_options: HashMap<String, Vec<String>>,
    /// Parsed-once handles of the panel's [`DialogControl::Svg`] images, by
    /// control id; rebuilt with the state whenever the plugin re-registers or
    /// updates the panel definition.
    pub svg_handles: HashMap<String, iced::widget::svg::Handle>,
}

impl PluginPanelState {
    pub fn from_def(def: PanelDef) -> Self {
        let mut text_values = HashMap::new();
        let mut bool_values = HashMap::new();
        let mut choice_values = HashMap::new();
        let mut color_values = HashMap::new();
        let mut file_values = HashMap::new();
        let mut table_values = HashMap::new();
        let mut collapsed_sections = HashSet::new();

        fn init_control(
            c: &DialogControl,
            tv: &mut HashMap<String, String>,
            bv: &mut HashMap<String, bool>,
            cv: &mut HashMap<String, usize>,
            clrv: &mut HashMap<String, [u8; 3]>,
            fv: &mut HashMap<String, String>,
            tbv: &mut HashMap<String, Vec<Vec<String>>>,
            cs: &mut HashSet<String>,
        ) {
            match c {
                DialogControl::Text { id, default, .. } => {
                    tv.insert(id.clone(), default.clone());
                }
                DialogControl::Number { id, default, .. } => {
                    tv.insert(id.clone(), default.to_string());
                }
                DialogControl::Checkbox { id, default, .. } => {
                    bv.insert(id.clone(), *default);
                }
                DialogControl::Choice {
                    id, default_index, ..
                } => {
                    cv.insert(id.clone(), *default_index);
                }
                DialogControl::Color { id, default, .. } => {
                    clrv.insert(id.clone(), *default);
                }
                DialogControl::FilePicker {
                    id, default_path, ..
                } => {
                    fv.insert(id.clone(), default_path.clone());
                }
                DialogControl::Fields {
                    id,
                    controls,
                    collapsed,
                    ..
                } => {
                    if *collapsed {
                        cs.insert(id.clone());
                    }
                    for sub in controls {
                        init_control(sub, tv, bv, cv, clrv, fv, tbv, cs);
                    }
                }
                DialogControl::Table { id, rows, .. } => {
                    tbv.insert(id.clone(), rows.clone());
                }
                DialogControl::Row {
                    controls: sub_ctrls,
                    ..
                }
                | DialogControl::Column {
                    controls: sub_ctrls,
                    ..
                } => {
                    for sub in sub_ctrls {
                        init_control(sub, tv, bv, cv, clrv, fv, tbv, cs);
                    }
                }
                DialogControl::InfoText { .. } | DialogControl::StyledText { .. } => {}
                DialogControl::Svg { .. } => {}
                DialogControl::Label { id, text, .. } => {
                    tv.insert(id.clone(), text.clone());
                }
                DialogControl::Button { .. } => {}
            }
        }

        for c in &def.controls {
            init_control(
                c,
                &mut text_values,
                &mut bool_values,
                &mut choice_values,
                &mut color_values,
                &mut file_values,
                &mut table_values,
                &mut collapsed_sections,
            );
        }

        let mut svg_handles = HashMap::new();
        collect_svg_handles(&def.controls, &mut svg_handles);

        Self {
            def,
            owner_plugin_id: None,
            visible: false,
            text_values,
            bool_values,
            choice_values,
            color_values,
            file_values,
            table_values,
            collapsed_sections,
            disabled_controls: HashSet::new(),
            hidden_controls: HashSet::new(),
            dynamic_choice_options: HashMap::new(),
            svg_handles,
        }
    }

    pub fn choice_option_text(&self, control_id: &str, idx: usize) -> String {
        if let Some(opts) = self.dynamic_choice_options.get(control_id) {
            return opts.get(idx).cloned().unwrap_or_default();
        }
        find_choice_option_in_controls(&self.def.controls, control_id, idx).unwrap_or_default()
    }

    pub fn update_values(&mut self, mut values: HashMap<String, DialogValue>) {
        // A "title" value renames the panel, unless a control owns that id.
        if !declares_control_id(&self.def.controls, "title") {
            if let Some(DialogValue::Text(title)) = values.remove("title") {
                self.def.title = title;
            }
        }
        apply_control_value_updates(
            values,
            &mut self.text_values,
            &mut self.bool_values,
            &mut self.choice_values,
            &mut self.color_values,
            &mut self.file_values,
            &mut self.table_values,
            &mut self.disabled_controls,
            &mut self.hidden_controls,
            &mut self.dynamic_choice_options,
        );
    }

    /// Capture the complete current form state without changing the panel or document.
    ///
    /// Raw input is retained alongside typed values so partially-entered numbers are
    /// never replaced by a default or a lossy parse. Any validation error makes the
    /// snapshot unsuitable for submission; the plugin-facing submit path must reject it.
    pub fn snapshot(&self, tab_id: u64) -> PanelSnapshot {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum SnapshotValueKind {
            Text,
            Number,
            Bool,
            Choice,
            Color,
            FilePath,
            Table,
        }

        /// Per-snapshot counters threaded through the recursive form walk.
        #[derive(Default)]
        struct SnapshotBudget {
            /// Current `Fields`/`Row`/`Column` nesting depth.
            depth: usize,
            /// Total table cells copied into this snapshot so far.
            cells: usize,
        }

        /// Length-cap one string value for the snapshot (the panel state
        /// keeps the real value; the copy in the snapshot is truncated and
        /// honestly reported).
        fn capped_text(raw: &str, id: &str, errors: &mut Vec<String>) -> String {
            if raw.chars().count() <= MAX_SNAPSHOT_VALUE_LEN {
                raw.to_string()
            } else {
                errors.push(format!(
                    "value for control id '{id}' exceeds the snapshot length budget; truncated"
                ));
                raw.chars().take(MAX_SNAPSHOT_VALUE_LEN).collect()
            }
        }

        fn declare_control(
            id: &str,
            value_kind: Option<SnapshotValueKind>,
            ids: &mut HashSet<String>,
            expected: &mut HashMap<String, SnapshotValueKind>,
            errors: &mut Vec<String>,
        ) {
            if id.trim().is_empty() {
                errors.push("control has a missing id".to_string());
            }
            if !ids.insert(id.to_string()) {
                errors.push(format!("duplicate control id '{id}'"));
            }
            if let Some(kind) = value_kind {
                expected.entry(id.to_string()).or_insert(kind);
            }
        }

        #[allow(clippy::too_many_arguments)]
        fn snapshot_controls(
            state: &PluginPanelState,
            controls: &[DialogControl],
            values: &mut HashMap<String, DialogValue>,
            raw_text: &mut HashMap<String, String>,
            ids: &mut HashSet<String>,
            expected: &mut HashMap<String, SnapshotValueKind>,
            field_ids: &mut HashSet<String>,
            errors: &mut Vec<String>,
            budget: &mut SnapshotBudget,
        ) {
            for control in controls {
                match control {
                    DialogControl::InfoText { .. } | DialogControl::StyledText { .. } => {}
                    // An image is display, not a form value.
                    DialogControl::Svg { id, .. } => {
                        declare_control(id, None, ids, expected, errors);
                    }
                    // A label's text may be rewritten through `text_values`;
                    // it is display, not a form value.
                    DialogControl::Label { id, .. } => {
                        declare_control(id, Some(SnapshotValueKind::Text), ids, expected, errors);
                    }
                    DialogControl::Button { id, .. } => {
                        declare_control(id, None, ids, expected, errors);
                    }
                    DialogControl::Text { id, .. } => {
                        declare_control(id, Some(SnapshotValueKind::Text), ids, expected, errors);
                        if let Some(raw) = state.text_values.get(id) {
                            raw_text.insert(id.clone(), capped_text(raw, id, errors));
                            values.insert(id.clone(), DialogValue::Text(raw.clone()));
                        } else {
                            errors.push(format!("missing text value for control id '{id}'"));
                        }
                    }
                    DialogControl::Number { id, .. } => {
                        declare_control(id, Some(SnapshotValueKind::Number), ids, expected, errors);
                        if let Some(raw) = state.text_values.get(id) {
                            raw_text.insert(id.clone(), capped_text(raw, id, errors));
                            match raw.trim().parse::<f64>() {
                                Ok(number) if number.is_finite() => {
                                    values.insert(id.clone(), DialogValue::Number(number));
                                }
                                _ => errors.push(format!(
                                    "invalid finite number for control id '{id}': '{raw}'"
                                )),
                            }
                        } else {
                            errors.push(format!("missing number value for control id '{id}'"));
                        }
                    }
                    DialogControl::Checkbox { id, .. } => {
                        declare_control(id, Some(SnapshotValueKind::Bool), ids, expected, errors);
                        if let Some(value) = state.bool_values.get(id) {
                            values.insert(id.clone(), DialogValue::Bool(*value));
                        } else {
                            errors.push(format!("missing checkbox value for control id '{id}'"));
                        }
                    }
                    DialogControl::Choice { id, options, .. } => {
                        declare_control(id, Some(SnapshotValueKind::Choice), ids, expected, errors);
                        // Options replaced at run time are the ones shown.
                        let options = state.dynamic_choice_options.get(id).unwrap_or(options);
                        if let Some(index) = state.choice_values.get(id).copied() {
                            if let Some(text) = options.get(index) {
                                values.insert(
                                    id.clone(),
                                    DialogValue::Choice {
                                        index,
                                        text: text.clone(),
                                    },
                                );
                            } else {
                                errors.push(format!(
                                    "invalid choice index {index} for control id '{id}' with {} options",
                                    options.len()
                                ));
                            }
                        } else {
                            errors.push(format!("missing choice value for control id '{id}'"));
                        }
                    }
                    DialogControl::Color { id, .. } => {
                        declare_control(id, Some(SnapshotValueKind::Color), ids, expected, errors);
                        if let Some(value) = state.color_values.get(id) {
                            values.insert(id.clone(), DialogValue::Color(*value));
                        } else {
                            errors.push(format!("missing color value for control id '{id}'"));
                        }
                    }
                    DialogControl::FilePicker { id, .. } => {
                        declare_control(
                            id,
                            Some(SnapshotValueKind::FilePath),
                            ids,
                            expected,
                            errors,
                        );
                        if let Some(raw) = state.file_values.get(id) {
                            raw_text.insert(id.clone(), capped_text(raw, id, errors));
                            values.insert(id.clone(), DialogValue::FilePath(raw.clone()));
                        } else {
                            errors.push(format!("missing file value for control id '{id}'"));
                        }
                    }
                    DialogControl::Fields { id, controls, .. } => {
                        declare_control(id, None, ids, expected, errors);
                        field_ids.insert(id.clone());
                        if budget.depth >= MAX_SNAPSHOT_FIELDS_DEPTH {
                            errors.push(format!(
                                "fields '{id}' exceed the snapshot depth budget; skipped"
                            ));
                        } else {
                            budget.depth += 1;
                            let mut field_values = HashMap::new();
                            snapshot_controls(
                                state,
                                controls,
                                &mut field_values,
                                raw_text,
                                ids,
                                expected,
                                field_ids,
                                errors,
                                budget,
                            );
                            budget.depth -= 1;
                            values.insert(id.clone(), DialogValue::Fields(field_values));
                        }
                    }
                    DialogControl::Table { id, .. } => {
                        declare_control(id, Some(SnapshotValueKind::Table), ids, expected, errors);
                        if let Some(rows) = state.table_values.get(id) {
                            let total: usize = rows.iter().map(|row| row.len()).sum();
                            let over_rows = rows.len() > MAX_SNAPSHOT_TABLE_ROWS;
                            let over_cells = budget.cells + total > MAX_SNAPSHOT_TABLE_CELLS;
                            if !over_rows && !over_cells {
                                budget.cells += total;
                                values.insert(id.clone(), DialogValue::Table(rows.clone()));
                            } else {
                                // Keep whole rows until one of the caps
                                // lands; the plugin sees which rows exist
                                // via the error entry.
                                let mut kept_rows = Vec::new();
                                let mut kept_cells = 0usize;
                                for row in rows {
                                    if kept_rows.len() >= MAX_SNAPSHOT_TABLE_ROWS
                                        || budget.cells + kept_cells + row.len()
                                            > MAX_SNAPSHOT_TABLE_CELLS
                                    {
                                        break;
                                    }
                                    kept_cells += row.len();
                                    kept_rows.push(row.clone());
                                }
                                budget.cells += kept_cells;
                                errors.push(format!(
                                    "table '{id}' exceeds the snapshot budget ({} rows, {total} \
                                     cells); kept {} rows",
                                    rows.len(),
                                    kept_rows.len()
                                ));
                                values.insert(id.clone(), DialogValue::Table(kept_rows));
                            }
                        } else {
                            errors.push(format!("missing table value for control id '{id}'"));
                        }
                    }
                    DialogControl::Row { controls, .. }
                    | DialogControl::Column { controls, .. } => {
                        if budget.depth >= MAX_SNAPSHOT_FIELDS_DEPTH {
                            errors.push(
                                "controls exceed the snapshot depth budget; skipped".to_string(),
                            );
                        } else {
                            budget.depth += 1;
                            snapshot_controls(
                                state, controls, values, raw_text, ids, expected, field_ids,
                                errors, budget,
                            );
                            budget.depth -= 1;
                        }
                    }
                }
            }
        }

        fn validate_state_ids<'a>(
            state_ids: impl IntoIterator<Item = &'a String>,
            state_name: &str,
            allowed: &[SnapshotValueKind],
            ids: &HashSet<String>,
            expected: &HashMap<String, SnapshotValueKind>,
            errors: &mut Vec<String>,
        ) {
            for id in state_ids {
                match expected.get(id) {
                    Some(kind) if allowed.contains(kind) => {}
                    Some(_) => errors.push(format!(
                        "invalid {state_name} state type for control id '{id}'"
                    )),
                    None if ids.contains(id) => errors.push(format!(
                        "invalid {state_name} state type for control id '{id}'"
                    )),
                    None => errors.push(format!("unknown or stale {state_name} state id '{id}'")),
                }
            }
        }

        let mut values = HashMap::new();
        let mut raw_text = HashMap::new();
        let mut ids = HashSet::new();
        let mut expected = HashMap::new();
        let mut field_ids = HashSet::new();
        let mut errors = Vec::new();
        let mut budget = SnapshotBudget::default();

        snapshot_controls(
            self,
            &self.def.controls,
            &mut values,
            &mut raw_text,
            &mut ids,
            &mut expected,
            &mut field_ids,
            &mut errors,
            &mut budget,
        );

        validate_state_ids(
            self.text_values.keys(),
            "text_values",
            &[SnapshotValueKind::Text, SnapshotValueKind::Number],
            &ids,
            &expected,
            &mut errors,
        );
        validate_state_ids(
            self.bool_values.keys(),
            "bool_values",
            &[SnapshotValueKind::Bool],
            &ids,
            &expected,
            &mut errors,
        );
        validate_state_ids(
            self.choice_values.keys(),
            "choice_values",
            &[SnapshotValueKind::Choice],
            &ids,
            &expected,
            &mut errors,
        );
        validate_state_ids(
            self.color_values.keys(),
            "color_values",
            &[SnapshotValueKind::Color],
            &ids,
            &expected,
            &mut errors,
        );
        validate_state_ids(
            self.file_values.keys(),
            "file_values",
            &[SnapshotValueKind::FilePath],
            &ids,
            &expected,
            &mut errors,
        );
        validate_state_ids(
            self.table_values.keys(),
            "table_values",
            &[SnapshotValueKind::Table],
            &ids,
            &expected,
            &mut errors,
        );
        for id in &self.collapsed_sections {
            if !field_ids.contains(id) {
                if ids.contains(id) {
                    errors.push(format!(
                        "invalid collapsed_sections state type for control id '{id}'"
                    ));
                } else {
                    errors.push(format!(
                        "unknown or stale collapsed_sections state id '{id}'"
                    ));
                }
            }
        }

        errors.sort_unstable();
        errors.dedup();

        PanelSnapshot {
            panel_id: self.def.id.clone(),
            plugin_id: self.owner_plugin_id.clone().unwrap_or_default(),
            tab_id,
            values,
            raw_text,
            errors,
        }
    }

    pub fn view<'a>(&'a self, num_id: u32, width: f32, chrome: Chrome) -> Element<'a, Message> {
        let tbar = title_bar(PanelId::Plugin(num_id), self.def.title.clone(), chrome);

        if self.def.style == PanelStyle::Form {
            const FORM_SPACING: f32 = 8.0;
            const FORM_BODY_PAD: [u16; 2] = [8, 10];
            let form_view = FormControlView {
                panel_id: Some(&self.def.id),
                text_values: &self.text_values,
                bool_values: &self.bool_values,
                choice_values: &self.choice_values,
                color_values: &self.color_values,
                file_values: &self.file_values,
                table_values: &self.table_values,
                collapsed_sections: &self.collapsed_sections,
                disabled_controls: &self.disabled_controls,
                hidden_controls: &self.hidden_controls,
                dynamic_choice_options: &self.dynamic_choice_options,
                svg_handles: &self.svg_handles,
            };
            let mut body_col = column![].spacing(FORM_SPACING).width(Fill);
            for control in &self.def.controls {
                if control
                    .id()
                    .is_some_and(|id| self.hidden_controls.contains(id))
                {
                    continue;
                }
                body_col = body_col.push(form_view.view_control_inner(control, false));
            }
            return kit::palette(
                tbar,
                kit::scroll_area(container(body_col).padding(FORM_BODY_PAD).width(Fill)),
                width,
            );
        }

        let mut body_col = column![].spacing(km::TOOL_GAP).width(Fill);
        for control in &self.def.controls {
            if control
                .id()
                .is_some_and(|id| self.hidden_controls.contains(id))
            {
                continue;
            }
            body_col = body_col.push(self.view_control_inner(control, false));
        }

        kit::palette(
            tbar,
            kit::scroll_area(container(body_col).padding(Padding::ZERO.top(km::TOOL_GAP))),
            width,
        )
    }

    /// A value control as a row of the Properties grid: its label in the
    /// name cell, the field, list, check box, swatch or path in the value
    /// cell. Anything else is laid out by [`Self::view_control_inner`].
    fn view_field_item<'a>(
        &'a self,
        control: &'a DialogControl,
        parent_disabled: bool,
    ) -> Element<'a, Message> {
        if let Some(id) = control.id() {
            if self.hidden_controls.contains(id) {
                return Space::new().width(0).height(0).into();
            }
        }
        let is_disabled = parent_disabled
            || control
                .id()
                .is_some_and(|id| self.disabled_controls.contains(id));
        let panel_id = self.def.id.clone();
        match control {
            DialogControl::Text { id, label, .. } | DialogControl::Number { id, label, .. } => {
                let current = self.text_values.get(id).map(|s| s.as_str()).unwrap_or("");
                let pid = panel_id.clone();
                let cid = id.clone();
                grid_row(
                    label,
                    grid_field(current, is_disabled, move |val| {
                        Message::PluginPanelTextChange(pid.clone(), cid.clone(), val)
                    }),
                )
            }
            DialogControl::Checkbox { id, label, .. } => {
                let current = self.bool_values.get(id).copied().unwrap_or(false);
                let pid = panel_id.clone();
                let cid = id.clone();
                let mut cb = checkbox(current).size(km::CHECK);
                if !is_disabled {
                    cb = cb.on_toggle(move |checked| {
                        Message::PluginPanelBoolToggle(pid.clone(), cid.clone(), checked)
                    });
                }
                grid_row(label, value_box(cb))
            }
            DialogControl::Choice {
                id, label, options, ..
            } => {
                let current_idx = self.choice_values.get(id).copied().unwrap_or(0);
                let effective_options = self.dynamic_choice_options.get(id).unwrap_or(options);
                let choice_opts: Vec<ChoiceOption> = effective_options
                    .iter()
                    .enumerate()
                    .map(|(i, opt)| ChoiceOption {
                        index: i,
                        label: opt.clone(),
                    })
                    .collect();
                let selected = choice_opts.get(current_idx).cloned();
                let pid = panel_id.clone();
                let cid = id.clone();

                let mut picker = pick_list(selected, choice_opts, |opt| opt.label.clone())
                    .text_size(ui_text::UI)
                    .padding(kit::FIELD_PAD)
                    .style(kit::pick_list_style)
                    .menu_style(kit::pick_menu_style)
                    .width(Fill);
                if !is_disabled {
                    picker = picker.on_select(move |opt| {
                        Message::PluginPanelChoiceSelect(
                            pid.clone(),
                            cid.clone(),
                            opt.index,
                            opt.label.clone(),
                        )
                    });
                }
                grid_row(label, picker)
            }
            DialogControl::Color { id, label, .. } => {
                let rgb = self.color_values.get(id).copied().unwrap_or([255, 0, 0]);
                let color_preview = container(Space::new().width(km::ROW_ICON).height(km::CHECK))
                    .style(move |theme: &Theme| container::Style {
                        background: Some(iced::Background::Color(iced::Color::from_rgb8(
                            rgb[0], rgb[1], rgb[2],
                        ))),
                        border: iced::Border {
                            color: chrome::border(theme),
                            width: km::line(),
                            radius: 0.0.into(),
                        },
                        ..Default::default()
                    });
                grid_row(
                    label,
                    value_box(
                        row![
                            color_preview,
                            kit::value_text(
                                format!("RGB({}, {}, {})", rgb[0], rgb[1], rgb[2]),
                                false
                            )
                        ]
                        .spacing(km::TOOL_GAP)
                        .align_y(iced::Alignment::Center),
                    ),
                )
            }
            DialogControl::FilePicker { id, label, .. } => {
                let current = self.file_values.get(id).map(|s| s.as_str()).unwrap_or("");
                let pid = panel_id.clone();
                let cid = id.clone();
                let cid_for_pick = id.clone();
                let input = grid_field(current, is_disabled, move |val| {
                    Message::PluginPanelFileTextChange(pid.clone(), cid.clone(), val)
                });
                let browse_btn = button(
                    container(
                        text("Browse...")
                            .size(ui_text::UI)
                            .line_height(ui_text::line()),
                    )
                    .center_y(Fill),
                )
                .on_press_maybe(
                    (!is_disabled)
                        .then(|| Message::PluginPanelFilePick(panel_id.clone(), cid_for_pick)),
                )
                .style(kit::cell_button_style)
                .height(Length::Fixed(km::ROW_H))
                .padding([0.0, km::CELL_PAD_X]);
                grid_row(
                    label,
                    row![input, browse_btn].align_y(iced::Alignment::Center),
                )
            }
            _ => self.view_control_inner(control, parent_disabled),
        }
    }

    fn view_control_inner<'a>(
        &'a self,
        control: &'a DialogControl,
        parent_disabled: bool,
    ) -> Element<'a, Message> {
        if let Some(id) = control.id() {
            if self.hidden_controls.contains(id) {
                return Space::new().width(0).height(0).into();
            }
        }
        let is_disabled = parent_disabled
            || control
                .id()
                .is_some_and(|id| self.disabled_controls.contains(id));
        let panel_id = self.def.id.clone();
        match control {
            DialogControl::InfoText { text: msg, align_h }
            | DialogControl::Label {
                text: msg, align_h, ..
            } => {
                let display_msg = control
                    .id()
                    .and_then(|id| self.text_values.get(id))
                    .map_or(msg.as_str(), String::as_str);
                let mut t = text(display_msg)
                    .size(ui_text::UI)
                    .line_height(ui_text::line())
                    .style(|theme: &Theme| text::Style {
                        color: Some(chrome::muted(theme)),
                    });
                if let Some(h) = align_h {
                    match h {
                        HorizontalAlign::Left => {
                            t = t.align_x(iced::alignment::Horizontal::Left);
                        }
                        HorizontalAlign::Center => {
                            t = t.align_x(iced::alignment::Horizontal::Center);
                        }
                        HorizontalAlign::Right => {
                            t = t.align_x(iced::alignment::Horizontal::Right);
                        }
                    }
                    container(t).width(Fill).into()
                } else {
                    t.into()
                }
            }
            DialogControl::StyledText {
                text: msg,
                style,
                align_h,
            } => styled_text(msg, *style, *align_h),
            DialogControl::Svg { id, height, .. } => svg_image(self.svg_handles.get(id), *height),
            DialogControl::Row {
                controls,
                align_h,
                align_v,
            } => {
                let mut row_el = row![].spacing(km::TOOL_GAP);
                match align_v {
                    Some(VerticalAlign::Top) => {
                        row_el = row_el.align_y(iced::alignment::Vertical::Top);
                    }
                    Some(VerticalAlign::Middle) => {
                        row_el = row_el.align_y(iced::alignment::Vertical::Center);
                    }
                    Some(VerticalAlign::Bottom) => {
                        row_el = row_el.align_y(iced::alignment::Vertical::Bottom);
                    }
                    None => {
                        row_el = row_el.align_y(iced::Alignment::Center);
                    }
                }
                for c in controls {
                    if c.id().is_some_and(|id| self.hidden_controls.contains(id)) {
                        continue;
                    }
                    row_el = row_el.push(self.view_control_inner(c, is_disabled));
                }
                match align_h {
                    Some(HorizontalAlign::Left) => container(row_el)
                        .width(Fill)
                        .align_x(iced::alignment::Horizontal::Left)
                        .into(),
                    Some(HorizontalAlign::Center) => container(row_el)
                        .width(Fill)
                        .align_x(iced::alignment::Horizontal::Center)
                        .into(),
                    Some(HorizontalAlign::Right) => container(row_el)
                        .width(Fill)
                        .align_x(iced::alignment::Horizontal::Right)
                        .into(),
                    None => row_el.width(Fill).into(),
                }
            }
            DialogControl::Column {
                controls,
                align_h,
                align_v,
            } => {
                let mut col_el = column![].spacing(km::TOOL_GAP);
                match align_h {
                    Some(HorizontalAlign::Left) => {
                        col_el = col_el.align_x(iced::alignment::Horizontal::Left);
                    }
                    Some(HorizontalAlign::Center) => {
                        col_el = col_el.align_x(iced::alignment::Horizontal::Center);
                    }
                    Some(HorizontalAlign::Right) => {
                        col_el = col_el.align_x(iced::alignment::Horizontal::Right);
                    }
                    None => {}
                }
                for c in controls {
                    if c.id().is_some_and(|id| self.hidden_controls.contains(id)) {
                        continue;
                    }
                    col_el = col_el.push(self.view_control_inner(c, is_disabled));
                }
                match align_v {
                    Some(VerticalAlign::Top) => container(col_el)
                        .height(Fill)
                        .align_y(iced::alignment::Vertical::Top)
                        .into(),
                    Some(VerticalAlign::Middle) => container(col_el)
                        .height(Fill)
                        .align_y(iced::alignment::Vertical::Center)
                        .into(),
                    Some(VerticalAlign::Bottom) => container(col_el)
                        .height(Fill)
                        .align_y(iced::alignment::Vertical::Bottom)
                        .into(),
                    None => col_el.into(),
                }
            }
            DialogControl::Button {
                id,
                label,
                action,
                align_h,
                width,
            } => {
                let pid = panel_id.clone();
                let cid = id.clone();
                let act = action.clone();
                let mut label_txt = text(label).size(ui_text::UI).line_height(ui_text::line());
                if let Some(h) = align_h {
                    match h {
                        HorizontalAlign::Left => {
                            label_txt = label_txt.align_x(iced::alignment::Horizontal::Left);
                        }
                        HorizontalAlign::Center => {
                            label_txt = label_txt.align_x(iced::alignment::Horizontal::Center);
                        }
                        HorizontalAlign::Right => {
                            label_txt = label_txt.align_x(iced::alignment::Horizontal::Right);
                        }
                    }
                } else {
                    label_txt = label_txt.align_x(iced::Center);
                }
                let mut btn = button(container(label_txt.width(Fill)).center_y(Fill))
                    .style(kit::text_button_style(false))
                    .height(Length::Fixed(km::TOOL_H))
                    .padding([0.0, km::MENU_PAD_X]);
                if !is_disabled {
                    btn = btn.on_press(Message::PluginPanelButtonClick(pid, cid, act));
                }
                if let Some(w) = width {
                    btn = btn.width(Length::Fixed(*w));
                } else {
                    btn = btn.width(Fill);
                }
                btn.into()
            }
            DialogControl::Text { .. }
            | DialogControl::Number { .. }
            | DialogControl::Checkbox { .. }
            | DialogControl::Choice { .. }
            | DialogControl::Color { .. }
            | DialogControl::FilePicker { .. } => self.view_field_item(control, parent_disabled),
            DialogControl::Fields {
                id,
                title,
                controls,
                ..
            } => {
                let mut group_col = column![].width(Fill);
                let is_collapsed = self.collapsed_sections.contains(id);
                if let Some(t) = title {
                    group_col = group_col.push(kit::category_bar(
                        t.clone(),
                        is_collapsed,
                        Message::PluginPanelSectionToggle(panel_id.clone(), id.clone()),
                    ));
                }
                if !is_collapsed {
                    for c in controls {
                        if c.id().is_some_and(|cid| self.hidden_controls.contains(cid)) {
                            continue;
                        }
                        group_col = group_col.push(self.view_field_item(c, is_disabled));
                    }
                }
                group_col.into()
            }
            DialogControl::Table {
                id,
                label,
                headers,
                allow_add,
                allow_delete,
                ..
            } => {
                let mut table_col = column![].spacing(km::TOOL_GAP).width(Fill);

                if let Some(lbl) = label {
                    table_col = table_col.push(
                        text(lbl)
                            .size(ui_text::UI)
                            .line_height(ui_text::line())
                            .font(kit::SEMIBOLD),
                    );
                }

                // Table headers, as the kit's column header: surface_raised,
                // semibold titles, 1 px palette-body lines between them.
                let mut header_row = row![]
                    .spacing(km::line())
                    .align_y(iced::Alignment::Center)
                    .width(Fill);
                for h in headers {
                    header_row = header_row.push(
                        container(
                            text(h)
                                .size(ui_text::UI)
                                .line_height(ui_text::line())
                                .font(kit::SEMIBOLD)
                                .wrapping(iced::widget::text::Wrapping::None),
                        )
                        .padding([0.0, km::CELL_PAD_X])
                        .height(Length::Fixed(km::ROW_H))
                        .align_y(iced::Alignment::Center)
                        .width(Fill)
                        .clip(true)
                        .style(|theme: &Theme| container::Style {
                            background: Some(iced::Background::Color(chrome::surface_raised(
                                theme,
                            ))),
                            ..Default::default()
                        }),
                    );
                }
                if *allow_delete {
                    header_row = header_row.push(
                        container(Space::new().width(km::BAR_BUTTON).height(km::ROW_H)).style(
                            |theme: &Theme| container::Style {
                                background: Some(iced::Background::Color(chrome::surface_raised(
                                    theme,
                                ))),
                                ..Default::default()
                            },
                        ),
                    );
                }

                // Table data rows: plain text until hovered or typed in,
                // 1 px lines between rows and cells.
                let current_rows = self.table_values.get(id);
                let mut rows_list = column![header_row].spacing(km::line()).width(Fill);

                if let Some(rows) = current_rows {
                    for (r_idx, row_cells) in rows.iter().enumerate() {
                        let mut r_widget = row![]
                            .spacing(km::line())
                            .align_y(iced::Alignment::Center)
                            .width(Fill);
                        for (c_idx, cell_text) in row_cells.iter().enumerate() {
                            let pid = panel_id.clone();
                            let tid = id.clone();
                            let mut ti = text_input("", cell_text.as_str())
                                .style(cell_field_style)
                                .padding(kit::FIELD_PAD)
                                .size(ui_text::UI)
                                .line_height(ui_text::line())
                                .width(Fill);
                            if !is_disabled {
                                ti = ti.on_input(move |val| {
                                    Message::PluginPanelTableCellChange(
                                        pid.clone(),
                                        tid.clone(),
                                        r_idx,
                                        c_idx,
                                        val,
                                    )
                                });
                            }
                            r_widget = r_widget.push(body_cell(ti, Fill));
                        }
                        if *allow_delete {
                            let pid = panel_id.clone();
                            let tid = id.clone();
                            let del_btn = button(
                                container(crate::ui::icons::tinted(
                                    crate::ui::icons::CLOSE,
                                    km::BAR_ICON,
                                    chrome::text,
                                ))
                                .center(Fill),
                            )
                            .on_press_maybe(
                                (!is_disabled)
                                    .then(|| Message::PluginPanelTableRemoveRow(pid, tid, r_idx)),
                            )
                            .width(Length::Fixed(km::BAR_BUTTON))
                            .height(Length::Fixed(km::ROW_H))
                            .padding(0)
                            .style(kit::flat_button_style(false));
                            r_widget = r_widget.push(body_cell(del_btn, Length::Shrink));
                        }
                        rows_list = rows_list.push(r_widget);
                    }
                }

                let grid_container = container(
                    scrollable(rows_list)
                        .direction(scrollable::Direction::Vertical(
                            scrollable::Scrollbar::new()
                                .width(km::SCROLLBAR_W)
                                .scroller_width(km::SCROLL_THUMB_W),
                        ))
                        .style(kit::scroll_style)
                        .height(Length::Shrink)
                        .width(Fill),
                )
                .padding(km::line())
                .style(|theme: &Theme| container::Style {
                    background: Some(iced::Background::Color(chrome::surface_raised(theme))),
                    ..Default::default()
                })
                .width(Fill);

                table_col = table_col.push(grid_container);

                if *allow_add {
                    let pid = panel_id.clone();
                    let tid = id.clone();
                    table_col = table_col.push(
                        row![
                            Space::new().width(Fill),
                            kit::text_button(
                                crate::t!("Add row").into_owned(),
                                (!is_disabled).then(|| Message::PluginPanelTableAddRow(pid, tid)),
                                false,
                            )
                        ]
                        .width(Fill),
                    );
                }

                table_col.into()
            }
        }
    }
}

// ── Kit mapping ──────────────────────────────────────────────────────────────

/// A control as a row of the Properties grid: `label` in the name cell,
/// `value` in the value cell.
fn grid_row<'a>(label: &str, value: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    kit::row_view(kit::name_text(label.to_string()), value.into(), false, None)
}

/// A text field filling a grid value cell; read-only while `disabled`.
fn grid_field<'a>(
    current: &str,
    disabled: bool,
    on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    let mut input = text_input("", current)
        .size(ui_text::UI)
        .line_height(ui_text::line())
        .padding(kit::FIELD_PAD)
        .style(kit::field_style)
        .width(Fill);
    if !disabled {
        input = input.on_input(on_input);
    }
    input.into()
}

/// A value cell's content inset like the grid's text.
fn value_box<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .padding([0.0, km::CELL_PAD_X])
        .height(Length::Fixed(km::ROW_H))
        .align_y(iced::Alignment::Center)
        .width(Fill)
        .into()
}

/// A table cell on the surface, one row high.
fn body_cell<'a>(content: impl Into<Element<'a, Message>>, width: Length) -> Element<'a, Message> {
    container(content)
        .height(Length::Fixed(km::ROW_H))
        .width(width)
        .style(|theme: &Theme| container::Style {
            background: Some(iced::Background::Color(chrome::surface(theme))),
            ..Default::default()
        })
        .into()
}

/// A table cell's field: plain text until hovered or typed in.
fn cell_field_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let mut style = kit::field_style(theme, status);
    if matches!(
        status,
        text_input::Status::Active | text_input::Status::Disabled
    ) {
        style.background = iced::Background::Color(iced::Color::TRANSPARENT);
        style.border.color = iced::Color::TRANSPARENT;
    }
    style
}

/// Whether a control in `controls`, at any depth, has id `id`.
fn declares_control_id(controls: &[DialogControl], id: &str) -> bool {
    controls.iter().any(|control| {
        control.id() == Some(id)
            || match control {
                DialogControl::Row { controls, .. }
                | DialogControl::Column { controls, .. }
                | DialogControl::Fields { controls, .. } => declares_control_id(controls, id),
                _ => false,
            }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_control(id: &str, default: &str) -> DialogControl {
        DialogControl::Text {
            id: id.to_string(),
            label: id.to_string(),
            default: default.to_string(),
        }
    }

    fn number_control(id: &str, default: f64) -> DialogControl {
        DialogControl::Number {
            id: id.to_string(),
            label: id.to_string(),
            default,
            min: None,
            max: None,
            step: None,
        }
    }

    fn choice_control(id: &str, options: &[&str]) -> DialogControl {
        DialogControl::Choice {
            id: id.to_string(),
            label: id.to_string(),
            options: options.iter().map(|option| option.to_string()).collect(),
            default_index: 0,
        }
    }

    fn file_control(id: &str, default_path: &str) -> DialogControl {
        DialogControl::FilePicker {
            id: id.to_string(),
            label: id.to_string(),
            default_path: default_path.to_string(),
            filter_name: "Drawings".to_string(),
            extensions: vec!["dwg".to_string()],
            save: false,
        }
    }

    fn table_control(id: &str) -> DialogControl {
        DialogControl::Table {
            id: id.to_string(),
            label: Some(id.to_string()),
            headers: vec!["Name".to_string(), "Value".to_string()],
            rows: vec![vec!["default".to_string(), "1".to_string()]],
            allow_add: true,
            allow_delete: true,
        }
    }

    fn panel_state(controls: Vec<DialogControl>) -> PluginPanelState {
        let mut def = PanelDef::new("test_panel", "Test Panel");
        def.controls = controls;
        PluginPanelState::from_def(def)
    }

    #[test]
    fn snapshot_captures_nested_rows_columns_collapsed_fields_and_current_values() {
        let mut state = panel_state(vec![
            DialogControl::InfoText {
                text: "Information only".to_string(),
                align_h: None,
            },
            DialogControl::Button {
                id: "action".to_string(),
                label: "Action".to_string(),
                action: None,
                align_h: None,
                width: None,
            },
            DialogControl::Fields {
                id: "outer".to_string(),
                title: Some("Outer".to_string()),
                collapsed: true,
                controls: vec![
                    text_control("name", "default name"),
                    number_control("count", 1.0),
                    DialogControl::Checkbox {
                        id: "enabled".to_string(),
                        label: "Enabled".to_string(),
                        default: false,
                    },
                    DialogControl::row(vec![
                        choice_control("mode", &["First", "Second"]),
                        DialogControl::Color {
                            id: "accent".to_string(),
                            label: "Accent".to_string(),
                            default: [1, 2, 3],
                        },
                        DialogControl::column(vec![
                            file_control("path", "default.dwg"),
                            DialogControl::Fields {
                                id: "inner".to_string(),
                                title: Some("Inner".to_string()),
                                collapsed: true,
                                controls: vec![table_control("grid")],
                            },
                        ]),
                    ]),
                ],
            },
        ]);
        state
            .text_values
            .insert("name".to_string(), "  current name\t".to_string());
        state
            .text_values
            .insert("count".to_string(), " 0042.500 ".to_string());
        state.bool_values.insert("enabled".to_string(), true);
        state.choice_values.insert("mode".to_string(), 1);
        state.color_values.insert("accent".to_string(), [9, 8, 7]);
        state
            .file_values
            .insert("path".to_string(), r" C:\drafts\site plan.dwg ".to_string());
        state.table_values.insert(
            "grid".to_string(),
            vec![
                vec!["edited".to_string(), "2e".to_string()],
                vec![String::new(), "  raw  ".to_string()],
            ],
        );
        let before = state.clone();

        let snapshot = state.snapshot(73);

        assert_eq!(state, before, "snapshot must be read-only");
        assert_eq!(snapshot.panel_id, "test_panel");
        assert_eq!(snapshot.tab_id, 73);
        assert!(snapshot.errors.is_empty(), "{:?}", snapshot.errors);
        assert_eq!(snapshot.values.len(), 1);
        assert!(!snapshot.values.contains_key("action"));
        assert_eq!(snapshot.raw_text.len(), 3);
        assert_eq!(
            snapshot.raw_text.get("name").map(String::as_str),
            Some("  current name\t")
        );
        assert_eq!(
            snapshot.raw_text.get("count").map(String::as_str),
            Some(" 0042.500 ")
        );
        assert_eq!(
            snapshot.raw_text.get("path").map(String::as_str),
            Some(r" C:\drafts\site plan.dwg ")
        );

        let Some(DialogValue::Fields(outer)) = snapshot.values.get("outer") else {
            panic!("outer fields missing: {:?}", snapshot.values);
        };
        assert_eq!(outer.len(), 7);
        assert_eq!(
            outer.get("name"),
            Some(&DialogValue::Text("  current name\t".to_string()))
        );
        assert_eq!(outer.get("count"), Some(&DialogValue::Number(42.5)));
        assert_eq!(outer.get("enabled"), Some(&DialogValue::Bool(true)));
        assert_eq!(
            outer.get("mode"),
            Some(&DialogValue::Choice {
                index: 1,
                text: "Second".to_string(),
            })
        );
        assert_eq!(outer.get("accent"), Some(&DialogValue::Color([9, 8, 7])));
        assert_eq!(
            outer.get("path"),
            Some(&DialogValue::FilePath(
                r" C:\drafts\site plan.dwg ".to_string()
            ))
        );

        let Some(DialogValue::Fields(inner)) = outer.get("inner") else {
            panic!("inner fields missing: {outer:?}");
        };
        assert_eq!(
            inner.get("grid"),
            Some(&DialogValue::Table(vec![
                vec!["edited".to_string(), "2e".to_string()],
                vec![String::new(), "  raw  ".to_string()],
            ]))
        );
    }

    #[test]
    fn snapshot_preserves_invalid_number_drafts_and_rejects_non_finite_values() {
        let mut state = panel_state(
            ["blank", "partial", "sign", "nan", "infinite", "valid"]
                .iter()
                .map(|id| number_control(id, 99.0))
                .collect(),
        );
        for (id, raw) in [
            ("blank", ""),
            ("partial", "1e+"),
            ("sign", "-"),
            ("nan", "NaN"),
            ("infinite", "inf"),
            ("valid", "-0.0250"),
        ] {
            state.text_values.insert(id.to_string(), raw.to_string());
        }

        let snapshot = state.snapshot(4);

        assert_eq!(snapshot.errors.len(), 5, "{:?}", snapshot.errors);
        assert!(snapshot.errors.windows(2).all(|pair| pair[0] <= pair[1]));
        for id in ["blank", "partial", "sign", "nan", "infinite"] {
            assert!(
                !snapshot.values.contains_key(id),
                "unexpected value for {id}"
            );
            assert!(snapshot
                .errors
                .iter()
                .any(|error| error.contains(&format!("control id '{id}'"))));
        }
        assert_eq!(
            snapshot.values.get("valid"),
            Some(&DialogValue::Number(-0.025))
        );
        assert_eq!(snapshot.raw_text.len(), 6);
        for (id, raw) in [
            ("blank", ""),
            ("partial", "1e+"),
            ("sign", "-"),
            ("nan", "NaN"),
            ("infinite", "inf"),
            ("valid", "-0.0250"),
        ] {
            assert_eq!(snapshot.raw_text.get(id).map(String::as_str), Some(raw));
        }
    }

    #[test]
    fn title_updates_preserve_live_form_values_and_control_schema() {
        let mut state = panel_state(vec![text_control("length", "1.0")]);
        let original_controls = state.def.controls.clone();
        state.update_values(HashMap::from([
            (
                "title".to_string(),
                DialogValue::Text("IFC properties - Alignment-Axis-h-seg-001".to_string()),
            ),
            ("length".to_string(), DialogValue::Text("2.5".to_string())),
        ]));
        assert_eq!(state.def.title, "IFC properties - Alignment-Axis-h-seg-001");
        assert_eq!(state.def.controls, original_controls);
        assert_eq!(
            state.text_values.get("length").map(String::as_str),
            Some("2.5")
        );
        assert!(!state.text_values.contains_key("title"));
    }

    #[test]
    fn styled_text_and_svg_are_presentation_only_and_parse_once() {
        // API v19: the SVG handle is built at registration (nested in a
        // collapsed card), and neither control contributes a form value.
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\">\
                   <rect width=\"10\" height=\"10\" fill=\"#fff\"/></svg>";
        let state = panel_state(vec![
            DialogControl::StyledText {
                text: "Check passed".to_string(),
                style: ocs_plugin_api::dialog::TextStyle::Success,
                align_h: None,
            },
            DialogControl::Fields {
                id: "card".to_string(),
                title: Some("Card".to_string()),
                collapsed: true,
                controls: vec![DialogControl::Svg {
                    id: "chart".to_string(),
                    svg: svg.to_string(),
                    height: Some(120.0),
                }],
            },
        ]);
        assert_eq!(
            state.svg_handles.get("chart"),
            Some(&iced::widget::svg::Handle::from_memory(
                svg.as_bytes().to_vec()
            ))
        );
        let snapshot = state.snapshot(0);
        assert!(snapshot.errors.is_empty(), "{:?}", snapshot.errors);
        assert!(!snapshot.values.contains_key("chart"));
        // Re-registering an updated definition rebuilds the handle.
        let mut def = state.def.clone();
        if let DialogControl::Fields { controls, .. } = &mut def.controls[1] {
            controls[0] = DialogControl::Svg {
                id: "chart".to_string(),
                svg: svg.replace("#fff", "#000"),
                height: None,
            };
        }
        let updated = PluginPanelState::from_def(def);
        assert_ne!(
            updated.svg_handles.get("chart"),
            state.svg_handles.get("chart")
        );
    }

    #[test]
    fn snapshot_reports_duplicate_and_missing_ids_deterministically() {
        let mut state = panel_state(vec![DialogControl::Fields {
            id: "group".to_string(),
            title: None,
            collapsed: false,
            controls: vec![
                text_control("duplicate", "first"),
                DialogControl::column(vec![
                    number_control("duplicate", 2.0),
                    DialogControl::Button {
                        id: "duplicate".to_string(),
                        label: "Duplicate".to_string(),
                        action: None,
                        align_h: None,
                        width: None,
                    },
                ]),
                text_control("", "unnamed"),
            ],
        }]);
        state
            .text_values
            .insert(String::new(), "current unnamed".to_string());

        let first = state.snapshot(0);
        let second = state.snapshot(0);

        assert_eq!(first.errors, second.errors);
        assert!(first.errors.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(first
            .errors
            .iter()
            .any(|error| error == "duplicate control id 'duplicate'"));
        assert!(first
            .errors
            .iter()
            .any(|error| error == "control has a missing id"));
        assert_eq!(
            first.raw_text.get("").map(String::as_str),
            Some("current unnamed")
        );
    }

    #[test]
    fn snapshot_rejects_missing_values_invalid_choices_and_stale_state_keys() {
        let mut state = panel_state(vec![
            text_control("known", "current"),
            number_control("missing", 12.0),
            choice_control("mode", &["Only"]),
            DialogControl::Fields {
                id: "group".to_string(),
                title: None,
                collapsed: false,
                controls: vec![table_control("grid")],
            },
        ]);
        state.text_values.remove("missing");
        state.choice_values.insert("mode".to_string(), 9);
        state.bool_values.insert("known".to_string(), true);
        state.bool_values.insert("missing".to_string(), true);
        state
            .file_values
            .insert("known".to_string(), "wrong".to_string());
        state.table_values.insert("ghost".to_string(), Vec::new());
        state
            .text_values
            .insert("ghost_text".to_string(), "stale".to_string());
        state.collapsed_sections.insert("known".to_string());
        state.collapsed_sections.insert("ghost_section".to_string());

        let snapshot = state.snapshot(8);

        assert!(!snapshot.values.contains_key("missing"));
        assert!(!snapshot.values.contains_key("mode"));
        assert!(!snapshot.raw_text.contains_key("missing"));
        assert!(snapshot.errors.windows(2).all(|pair| pair[0] <= pair[1]));
        for expected in [
            "missing number value for control id 'missing'",
            "invalid choice index 9 for control id 'mode'",
            "invalid bool_values state type for control id 'known'",
            "invalid bool_values state type for control id 'missing'",
            "invalid file_values state type for control id 'known'",
            "unknown or stale table_values state id 'ghost'",
            "unknown or stale text_values state id 'ghost_text'",
            "invalid collapsed_sections state type for control id 'known'",
            "unknown or stale collapsed_sections state id 'ghost_section'",
        ] {
            assert!(
                snapshot.errors.iter().any(|error| error.contains(expected)),
                "missing {expected:?} in {:?}",
                snapshot.errors
            );
        }
    }

    #[test]
    fn form_and_palette_styles_render_controls_and_preserve_snapshots() {
        const TEST_PANEL_W: f32 = 280.0;
        let controls = vec![
            text_control("name", "Beam"),
            number_control("len", 10.0),
            choice_control("kind", &["Line", "Circle"]),
            DialogControl::Checkbox {
                id: "active".to_string(),
                label: "Active".to_string(),
                default: true,
            },
            DialogControl::Fields {
                id: "sec".to_string(),
                title: Some("Section".to_string()),
                collapsed: false,
                controls: vec![text_control("sub", "v")],
            },
            table_control("pts"),
            DialogControl::Button {
                id: "run".to_string(),
                label: "Run".to_string(),
                action: Some("(c:run)".to_string()),
                align_h: None,
                width: None,
            },
        ];

        let mut form_def = PanelDef::new("form_panel", "Form Panel").with_style(PanelStyle::Form);
        form_def.controls = controls.clone();
        let form_state = PluginPanelState::from_def(form_def);
        let _ = form_state.view(1, TEST_PANEL_W, Chrome::default());

        let mut palette_def =
            PanelDef::new("palette_panel", "Palette Panel").with_style(PanelStyle::Palette);
        palette_def.controls = controls;
        let palette_state = PluginPanelState::from_def(palette_def);
        let _ = palette_state.view(2, TEST_PANEL_W, Chrome::default());

        assert_eq!(form_state.snapshot(1).values, palette_state.snapshot(1).values);
    }
}
