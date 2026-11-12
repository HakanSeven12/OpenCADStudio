//! Native modal dialog renderer for declarative plugin dialogs.
//!
//! Renders arbitrary `DialogDef` structures declared by plugins into native iced
//! widget trees with rows, columns, inputs, checkboxes, pick lists, and buttons,
//! matching the OpenCADStudio design system and theme.

use std::collections::{HashMap, HashSet};
use iced::widget::{
    button, checkbox, column, container, pick_list, row, rule, scrollable, text, text_input, Space,
};
use iced::{Element, Fill, Length, Theme};
use ocs_plugin_api::dialog::{
    DialogControl, DialogDef, DialogResponse, DialogValue, HorizontalAlign, TextStyle,
    VerticalAlign,
};

use crate::app::Message;
use crate::ui::style::form::{button_style, dialog_button, field_style};

/// State backing an active declarative plugin modal dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginDialogState {
    pub def: DialogDef,
    pub tab_id: Option<u64>,
    pub plugin_id: Option<String>,
    pub command_id: Option<u64>,
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
    /// Parsed-once handles of the dialog's [`DialogControl::Svg`] images,
    /// by control id (see [`collect_svg_handles`]).
    pub svg_handles: HashMap<String, iced::widget::svg::Handle>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceOption {
    pub index: usize,
    pub label: String,
}

impl std::fmt::Display for ChoiceOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

/// Shared helper to apply `DialogValue` updates to dialog or panel control state maps.
pub fn apply_control_value_updates(
    values: HashMap<String, DialogValue>,
    text_values: &mut HashMap<String, String>,
    bool_values: &mut HashMap<String, bool>,
    choice_values: &mut HashMap<String, usize>,
    color_values: &mut HashMap<String, [u8; 3]>,
    file_values: &mut HashMap<String, String>,
    table_values: &mut HashMap<String, Vec<Vec<String>>>,
    disabled_controls: &mut HashSet<String>,
    hidden_controls: &mut HashSet<String>,
    dynamic_choice_options: &mut HashMap<String, Vec<String>>,
) {
    for (k, v) in values {
        match v {
            DialogValue::Text(s) => {
                text_values.insert(k, s);
            }
            DialogValue::Number(n) => {
                text_values.insert(k, n.to_string());
            }
            DialogValue::Bool(b) => {
                if !b && table_values.contains_key(&k) {
                    table_values.insert(k.clone(), Vec::new());
                }
                bool_values.insert(k, b);
            }
            DialogValue::Choice { index, .. } => {
                choice_values.insert(k, index);
            }
            DialogValue::ChoiceOptions {
                options,
                selected_index,
            } => {
                choice_values.insert(k.clone(), selected_index);
                dynamic_choice_options.insert(k, options);
            }
            DialogValue::Color(c) => {
                color_values.insert(k, c);
            }
            DialogValue::FilePath(f) => {
                file_values.insert(k, f);
            }
            DialogValue::Table(rows) => {
                table_values.insert(k, rows);
            }
            DialogValue::Enabled(en) => {
                let id = k.strip_suffix(":enabled").unwrap_or(&k);
                if en {
                    disabled_controls.remove(id);
                } else {
                    disabled_controls.insert(id.to_string());
                }
            }
            DialogValue::Visible(vis) => {
                let id = k.strip_suffix(":visible").unwrap_or(&k);
                if vis {
                    hidden_controls.remove(id);
                } else {
                    hidden_controls.insert(id.to_string());
                }
            }
            DialogValue::Fields(map) => {
                apply_control_value_updates(
                    map,
                    text_values,
                    bool_values,
                    choice_values,
                    color_values,
                    file_values,
                    table_values,
                    disabled_controls,
                    hidden_controls,
                    dynamic_choice_options,
                );
            }
        }
    }
}

/// Recursively look up a static choice option label by control ID and option index.
pub fn find_choice_option_in_controls(
    controls: &[DialogControl],
    control_id: &str,
    idx: usize,
) -> Option<String> {
    for c in controls {
        match c {
            DialogControl::Choice { id, options, .. } if id == control_id => {
                return options.get(idx).cloned();
            }
            DialogControl::Row { controls: sub, .. }
            | DialogControl::Column { controls: sub, .. }
            | DialogControl::Fields { controls: sub, .. } => {
                if let Some(found) = find_choice_option_in_controls(sub, control_id, idx) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

impl PluginDialogState {
    pub fn title(&self) -> String {
        self.def.title.clone()
    }

    pub fn new(def: DialogDef) -> Self {
        let mut text_values = HashMap::new();
        let mut bool_values = HashMap::new();
        let mut choice_values = HashMap::new();
        let mut color_values = HashMap::new();
        let mut file_values = HashMap::new();
        let mut table_values = HashMap::new();
        let mut collapsed_sections = HashSet::new();

        fn init_control(
            control: &DialogControl,
            text_values: &mut HashMap<String, String>,
            bool_values: &mut HashMap<String, bool>,
            choice_values: &mut HashMap<String, usize>,
            color_values: &mut HashMap<String, [u8; 3]>,
            file_values: &mut HashMap<String, String>,
            table_values: &mut HashMap<String, Vec<Vec<String>>>,
            collapsed_sections: &mut HashSet<String>,
        ) {
            match control {
                DialogControl::InfoText { .. }
                | DialogControl::StyledText { .. }
                | DialogControl::Svg { .. } => {}
                DialogControl::Label { id, text, .. } => {
                    text_values.insert(id.clone(), text.clone());
                }
                DialogControl::Button { .. } => {}
                DialogControl::Text { id, default, .. } => {
                    text_values.insert(id.clone(), default.clone());
                }
                DialogControl::Number { id, default, .. } => {
                    text_values.insert(id.clone(), default.to_string());
                }
                DialogControl::Checkbox { id, default, .. } => {
                    bool_values.insert(id.clone(), *default);
                }
                DialogControl::Choice {
                    id, default_index, ..
                } => {
                    choice_values.insert(id.clone(), *default_index);
                }
                DialogControl::Color { id, default, .. } => {
                    color_values.insert(id.clone(), *default);
                }
                DialogControl::FilePicker {
                    id, default_path, ..
                } => {
                    file_values.insert(id.clone(), default_path.clone());
                }
                DialogControl::Table { id, rows, .. } => {
                    table_values.insert(id.clone(), rows.clone());
                }
                DialogControl::Row { controls, .. } | DialogControl::Column { controls, .. } => {
                    for c in controls {
                        init_control(
                            c,
                            text_values,
                            bool_values,
                            choice_values,
                            color_values,
                            file_values,
                            table_values,
                            collapsed_sections,
                        );
                    }
                }
                DialogControl::Fields {
                    id,
                    controls,
                    collapsed,
                    ..
                } => {
                    if *collapsed {
                        collapsed_sections.insert(id.clone());
                    }
                    for c in controls {
                        init_control(
                            c,
                            text_values,
                            bool_values,
                            choice_values,
                            color_values,
                            file_values,
                            table_values,
                            collapsed_sections,
                        );
                    }
                }
            }
        }

        for row in &def.rows {
            for control in &row.controls {
                init_control(
                    control,
                    &mut text_values,
                    &mut bool_values,
                    &mut choice_values,
                    &mut color_values,
                    &mut file_values,
                    &mut table_values,
                    &mut collapsed_sections,
                );
            }
        }

        let mut svg_handles = HashMap::new();
        for row in &def.rows {
            collect_svg_handles(&row.controls, &mut svg_handles);
        }

        Self {
            def,
            tab_id: None,
            plugin_id: None,
            command_id: None,
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

    pub fn with_owner(
        mut self,
        tab_id: Option<u64>,
        plugin_id: Option<String>,
        command_id: Option<u64>,
    ) -> Self {
        self.tab_id = tab_id;
        self.plugin_id = plugin_id;
        self.command_id = command_id;
        self
    }

    pub fn reset_values(&mut self) {
        let tab_id = self.tab_id;
        let plugin_id = self.plugin_id.clone();
        let command_id = self.command_id;
        *self = Self::new(self.def.clone()).with_owner(tab_id, plugin_id, command_id);
    }

    pub fn choice_option_text(&self, control_id: &str, idx: usize) -> String {
        if let Some(opts) = self.dynamic_choice_options.get(control_id) {
            return opts.get(idx).cloned().unwrap_or_default();
        }
        for row in &self.def.rows {
            if let Some(found) = find_choice_option_in_controls(&row.controls, control_id, idx) {
                return found;
            }
        }
        String::new()
    }

    pub fn update_values(&mut self, values: HashMap<String, DialogValue>) {
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

    pub fn collect_response_map(&self) -> HashMap<String, DialogValue> {
        match self.collect_response() {
            DialogResponse::Submitted(map) => map,
            DialogResponse::Cancelled => HashMap::new(),
        }
    }

    fn collect_control_value(&self, control: &DialogControl) -> Option<(String, DialogValue)> {
        match control {
            DialogControl::Label { id, text, .. } => {
                let val = self.text_values.get(id).unwrap_or(text).clone();
                Some((id.clone(), DialogValue::Text(val)))
            }
            DialogControl::InfoText { .. }
            | DialogControl::StyledText { .. }
            | DialogControl::Svg { .. }
            | DialogControl::Button { .. }
            | DialogControl::Row { .. }
            | DialogControl::Column { .. } => None,
            DialogControl::Text { id, default, .. } => {
                let val = self
                    .text_values
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| default.clone());
                Some((id.clone(), DialogValue::Text(val)))
            }
            DialogControl::Number {
                id,
                default,
                min,
                max,
                ..
            } => {
                let parsed = self
                    .text_values
                    .get(id)
                    .and_then(|s| s.trim().parse::<f64>().ok())
                    .unwrap_or(*default);
                let clamped = match (*min, *max) {
                    (Some(lo), Some(hi)) => parsed.clamp(lo, hi),
                    (Some(lo), None) => parsed.max(lo),
                    (None, Some(hi)) => parsed.min(hi),
                    (None, None) => parsed,
                };
                Some((id.clone(), DialogValue::Number(clamped)))
            }
            DialogControl::Checkbox { id, default, .. } => {
                let val = self.bool_values.get(id).copied().unwrap_or(*default);
                Some((id.clone(), DialogValue::Bool(val)))
            }
            DialogControl::Choice { id, options, default_index, .. } => {
                let idx = self.choice_values.get(id).copied().unwrap_or(*default_index);
                let effective_options = self.dynamic_choice_options.get(id).unwrap_or(options);
                let text = effective_options.get(idx).cloned().unwrap_or_default();
                Some((id.clone(), DialogValue::Choice { index: idx, text }))
            }
            DialogControl::Color { id, default, .. } => {
                let val = self.color_values.get(id).copied().unwrap_or(*default);
                Some((id.clone(), DialogValue::Color(val)))
            }
            DialogControl::FilePicker {
                id, default_path, ..
            } => {
                let val = self
                    .file_values
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| default_path.clone());
                Some((id.clone(), DialogValue::FilePath(val)))
            }
            DialogControl::Table { id, rows, .. } => {
                let val = self
                    .table_values
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| rows.clone());
                Some((id.clone(), DialogValue::Table(val)))
            }
            DialogControl::Fields { id, controls, .. } => {
                let mut field_map = HashMap::new();
                for c in controls {
                    if let Some((cid, cval)) = self.collect_control_value(c) {
                        field_map.insert(cid, cval);
                    }
                }
                Some((id.clone(), DialogValue::Fields(field_map)))
            }
        }
    }

    /// Collect all current values into a submitted `DialogResponse`.
    pub fn collect_response(&self) -> DialogResponse {
        let mut map = HashMap::new();

        fn collect_recursive(
            state: &PluginDialogState,
            ctrls: &[DialogControl],
            map: &mut HashMap<String, DialogValue>,
        ) {
            for control in ctrls {
                match control {
                    DialogControl::Row { controls, .. }
                    | DialogControl::Column { controls, .. } => {
                        collect_recursive(state, controls, map);
                    }
                    _ => {
                        if let Some((id, val)) = state.collect_control_value(control) {
                            map.insert(id, val);
                        }
                    }
                }
            }
        }

        for row in &self.def.rows {
            collect_recursive(self, &row.controls, &mut map);
        }

        DialogResponse::Submitted(map)
    }

    pub fn find_table_col_count(&self, table_id: &str) -> usize {
        fn find_cols(ctrls: &[DialogControl], tid: &str) -> Option<usize> {
            for c in ctrls {
                match c {
                    DialogControl::Table { id, headers, .. } if id == tid => {
                        return Some(headers.len());
                    }
                    DialogControl::Fields { controls, .. }
                    | DialogControl::Row { controls, .. }
                    | DialogControl::Column { controls, .. } => {
                        if let Some(n) = find_cols(controls, tid) {
                            return Some(n);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        for r in &self.def.rows {
            if let Some(n) = find_cols(&r.controls, table_id) {
                return n;
            }
        }
        1
    }

    fn has_table_control(&self) -> bool {
        fn check_ctrls(ctrls: &[DialogControl]) -> bool {
            ctrls.iter().any(|c| match c {
                DialogControl::Table { .. } => true,
                DialogControl::Fields { controls, .. }
                | DialogControl::Row { controls, .. }
                | DialogControl::Column { controls, .. } => check_ctrls(controls),
                _ => false,
            })
        }
        self.def.rows.iter().any(|r| check_ctrls(&r.controls))
    }

    /// Render the dialog into native iced widgets.
    pub fn view<'a>(&'a self) -> Element<'a, Message> {
        let mut body = column![].spacing(10);

        for r in &self.def.rows {
            let mut row_widget = row![].spacing(12);
            match r.align_v {
                Some(VerticalAlign::Top) => {
                    row_widget = row_widget.align_y(iced::alignment::Vertical::Top);
                }
                Some(VerticalAlign::Middle) => {
                    row_widget = row_widget.align_y(iced::alignment::Vertical::Center);
                }
                Some(VerticalAlign::Bottom) => {
                    row_widget = row_widget.align_y(iced::alignment::Vertical::Bottom);
                }
                None => {
                    row_widget = row_widget.align_y(iced::Alignment::Center);
                }
            }

            let mut any_visible = false;
            for control in &r.controls {
                if control.id().is_some_and(|id| self.hidden_controls.contains(id)) {
                    continue;
                }
                any_visible = true;
                let control_element = self.view_control_inner(control, false);
                row_widget = row_widget.push(control_element);
            }
            if !any_visible && !r.controls.is_empty() {
                continue;
            }

            let row_container: Element<'a, Message> = match r.align_h {
                Some(HorizontalAlign::Left) => {
                    container(row_widget).width(Fill).align_x(iced::alignment::Horizontal::Left).into()
                }
                Some(HorizontalAlign::Center) => {
                    container(row_widget).width(Fill).align_x(iced::alignment::Horizontal::Center).into()
                }
                Some(HorizontalAlign::Right) => {
                    container(row_widget).width(Fill).align_x(iced::alignment::Horizontal::Right).into()
                }
                None => row_widget.into(),
            };

            body = body.push(row_container);
        }

        let mut actions = row![
            Space::new().width(Fill),
            dialog_button(&self.def.submit_label, Message::PluginDialogSubmit, true),
        ];

        if !self.def.cancel_label.trim().is_empty() {
            actions = actions.push(dialog_button(
                &self.def.cancel_label,
                Message::PluginDialogCancel,
                false,
            ));
        }

        let actions = actions.spacing(8).align_y(iced::Alignment::Center);

        let default_width = if self.has_table_control() {
            560.0
        } else {
            460.0
        };
        let dialog_w = self.def.width.unwrap_or(default_width);

        column![body, Space::new().height(6), actions]
            .spacing(12)
            .padding(14)
            .width(Length::Fixed(dialog_w))
            .into()
    }

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
        let is_disabled =
            parent_disabled || control.id().is_some_and(|id| self.disabled_controls.contains(id));
        const LABEL_WIDTH: f32 = 110.0;
        match control {
            DialogControl::Text { id, label, .. } | DialogControl::Number { id, label, .. } => {
                let current = self.text_values.get(id).map(|s| s.as_str()).unwrap_or("");
                let id_clone = id.clone();
                let mut input = text_input("", current)
                    .padding([3, 6])
                    .size(12)
                    .style(field_style);
                if !is_disabled {
                    input = input
                        .on_input(move |val| Message::PluginDialogTextChange(id_clone.clone(), val));
                }

                row![
                    container(text(label).size(11)).width(Length::Fixed(LABEL_WIDTH)),
                    input
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .width(Fill)
                .into()
            }
            DialogControl::Checkbox { id, label, .. } => {
                let current = self.bool_values.get(id).copied().unwrap_or(false);
                let id_clone = id.clone();
                let mut cb = checkbox(current)
                    .label(label)
                    .size(14);
                if !is_disabled {
                    cb = cb.on_toggle(move |checked| {
                        Message::PluginDialogBoolToggle(id_clone.clone(), checked)
                    });
                }

                row![Space::new().width(Length::Fixed(LABEL_WIDTH)), cb]
                    .spacing(8)
                    .align_y(iced::Alignment::Center)
                    .width(Fill)
                    .into()
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
                let id_clone = id.clone();

                let mut picker = pick_list(selected, choice_opts, |opt| opt.label.clone())
                    .padding([3, 6])
                    .text_size(12);
                if !is_disabled {
                    picker = picker.on_select(move |opt| {
                        Message::PluginDialogChoiceSelect(id_clone.clone(), opt.index)
                    });
                }

                row![
                    container(text(label).size(11)).width(Length::Fixed(LABEL_WIDTH)),
                    picker
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .width(Fill)
                .into()
            }
            DialogControl::Color { id, label, .. } => {
                let rgb = self.color_values.get(id).copied().unwrap_or([255, 0, 0]);
                let color_preview = container(Space::new().width(24).height(16))
                    .style(move |theme: &Theme| container::Style {
                        background: Some(iced::Background::Color(iced::Color::from_rgb8(rgb[0], rgb[1], rgb[2]))),
                        border: iced::Border {
                            color: theme.palette().background.neutral.color,
                            width: 1.0,
                            radius: 2.0.into(),
                        },
                        ..Default::default()
                    });

                row![
                    container(text(label).size(11)).width(Length::Fixed(LABEL_WIDTH)),
                    row![
                        color_preview,
                        text(format!("RGB({}, {}, {})", rgb[0], rgb[1], rgb[2])).size(11)
                    ]
                    .spacing(6)
                    .align_y(iced::Alignment::Center)
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .width(Fill)
                .into()
            }
            DialogControl::FilePicker { id, label, .. } => {
                let current = self.file_values.get(id).map(|s| s.as_str()).unwrap_or("");
                let id_clone = id.clone();
                let id_for_pick = id.clone();

                let mut input = text_input("", current)
                    .padding([3, 6])
                    .size(12)
                    .style(field_style);
                if !is_disabled {
                    input = input.on_input(move |val| {
                        Message::PluginDialogFileTextChange(id_clone.clone(), val)
                    });
                }

                let mut browse_btn = button(text("Browse...").size(11))
                    .style(button_style(false))
                    .padding([3, 6]);
                if !is_disabled {
                    browse_btn = browse_btn.on_press(Message::PluginDialogFilePick(id_for_pick));
                }

                row![
                    container(text(label).size(11)).width(Length::Fixed(LABEL_WIDTH)),
                    row![input, browse_btn]
                        .spacing(4)
                        .align_y(iced::Alignment::Center)
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .width(Fill)
                .into()
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
        let is_disabled =
            parent_disabled || control.id().is_some_and(|id| self.disabled_controls.contains(id));
        match control {
            DialogControl::InfoText { text: msg, align_h }
            | DialogControl::Label {
                text: msg, align_h, ..
            } => {
                let display_msg = control
                    .id()
                    .and_then(|id| self.text_values.get(id))
                    .map_or(msg.as_str(), String::as_str);
                let mut t = text(display_msg).size(12).style(|theme: &Theme| text::Style {
                    color: Some(crate::ui::style::readable::muted_text(theme)),
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
                let mut row_el = row![].spacing(8);
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
                    Some(HorizontalAlign::Left) => {
                        container(row_el).width(Fill).align_x(iced::alignment::Horizontal::Left).into()
                    }
                    Some(HorizontalAlign::Center) => {
                        container(row_el).width(Fill).align_x(iced::alignment::Horizontal::Center).into()
                    }
                    Some(HorizontalAlign::Right) => {
                        container(row_el).width(Fill).align_x(iced::alignment::Horizontal::Right).into()
                    }
                    None => row_el.width(Fill).into(),
                }
            }
            DialogControl::Column {
                controls,
                align_h,
                align_v,
            } => {
                let mut col_el = column![].spacing(8);
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
                    Some(VerticalAlign::Top) => {
                        container(col_el).height(Fill).align_y(iced::alignment::Vertical::Top).into()
                    }
                    Some(VerticalAlign::Middle) => {
                        container(col_el).height(Fill).align_y(iced::alignment::Vertical::Center).into()
                    }
                    Some(VerticalAlign::Bottom) => {
                        container(col_el).height(Fill).align_y(iced::alignment::Vertical::Bottom).into()
                    }
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
                let cid = id.clone();
                let act = action.clone();
                let mut label_txt = text(label).size(12);
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
                }
                let mut btn = button(label_txt)
                    .style(button_style(false))
                    .padding([5, 10]);
                if !is_disabled {
                    btn = btn.on_press(Message::PluginDialogButtonClick(cid, act));
                }
                if let Some(w) = width {
                    btn = btn.width(Length::Fixed(*w));
                }
                btn.into()
            }
            DialogControl::Text { id, label, .. } | DialogControl::Number { id, label, .. } => {
                let current = self.text_values.get(id).map(|s| s.as_str()).unwrap_or("");
                let id_clone = id.clone();
                let mut input = text_input("", current)
                    .padding(5)
                    .size(12)
                    .style(field_style);
                if !is_disabled {
                    input = input
                        .on_input(move |val| Message::PluginDialogTextChange(id_clone.clone(), val));
                }

                column![text(label).size(11), input]
                    .spacing(4)
                    .width(Fill)
                    .into()
            }
            DialogControl::Checkbox { id, label, .. } => {
                let current = self.bool_values.get(id).copied().unwrap_or(false);
                let id_clone = id.clone();
                let mut cb = checkbox(current)
                    .label(label)
                    .size(14);
                if !is_disabled {
                    cb = cb.on_toggle(move |checked| {
                        Message::PluginDialogBoolToggle(id_clone.clone(), checked)
                    });
                }
                cb.into()
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
                let id_clone = id.clone();

                let mut picker = pick_list(selected, choice_opts, |opt| opt.label.clone())
                    .padding([3, 6])
                    .text_size(12);
                if !is_disabled {
                    picker = picker.on_select(move |opt| {
                        Message::PluginDialogChoiceSelect(id_clone.clone(), opt.index)
                    });
                }

                column![text(label).size(11), picker]
                    .spacing(4)
                    .width(Fill)
                    .into()
            }
            DialogControl::Color { id, label, .. } => {
                let rgb = self.color_values.get(id).copied().unwrap_or([255, 0, 0]);
                let color_preview = container(Space::new().width(24).height(18))
                    .style(move |theme: &Theme| container::Style {
                        background: Some(iced::Background::Color(iced::Color::from_rgb8(rgb[0], rgb[1], rgb[2]))),
                        border: iced::Border {
                            color: theme.palette().background.neutral.color,
                            width: 1.0,
                            radius: 2.0.into(),
                        },
                        ..Default::default()
                    });

                column![
                    text(label).size(11),
                    row![
                        color_preview,
                        text(format!("RGB({}, {}, {})", rgb[0], rgb[1], rgb[2])).size(11)
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center)
                ]
                .spacing(4)
                .into()
            }
            DialogControl::FilePicker { id, label, .. } => {
                let current = self.file_values.get(id).map(|s| s.as_str()).unwrap_or("");
                let id_clone = id.clone();
                let id_for_pick = id.clone();

                let mut input = text_input("", current)
                    .padding(5)
                    .size(12)
                    .style(field_style);
                if !is_disabled {
                    input = input.on_input(move |val| {
                        Message::PluginDialogFileTextChange(id_clone.clone(), val)
                    });
                }

                let mut browse_btn = button(text("Browse...").size(11))
                    .style(button_style(false))
                    .padding([4, 8]);
                if !is_disabled {
                    browse_btn = browse_btn.on_press(Message::PluginDialogFilePick(id_for_pick));
                }

                column![
                    text(label).size(11),
                    row![input, browse_btn]
                        .spacing(6)
                        .align_y(iced::Alignment::Center)
                ]
                .spacing(4)
                .width(Fill)
                .into()
            }
            DialogControl::Fields {
                id,
                title,
                controls,
                ..
            } => {
                let mut group_col = column![].spacing(6).width(Fill);
                let is_collapsed = self.collapsed_sections.contains(id);

                if let Some(t) = title {
                    let id_for_toggle = id.clone();

                    let header_btn = button(
                        row![
                            if is_collapsed {
                                crate::ui::icons::themed_arrow_right(10.0)
                            } else {
                                crate::ui::icons::themed_arrow_down(10.0)
                            },
                            text(t).size(12),
                        ]
                        .spacing(6)
                        .align_y(iced::Alignment::Center)
                    )
                    .on_press(Message::PluginDialogSectionToggle(id_for_toggle))
                    .style(button::text)
                    .padding([2, 4]);

                    group_col = group_col.push(header_btn);
                    if !is_collapsed {
                        group_col = group_col.push(rule::horizontal(1));
                    }
                }

                if !is_collapsed {
                    let mut inner_items = column![].spacing(6).width(Fill);
                    for c in controls {
                        if c.id().is_some_and(|cid| self.hidden_controls.contains(cid)) {
                            continue;
                        }
                        inner_items = inner_items.push(self.view_field_item(c, is_disabled));
                    }
                    group_col = group_col.push(inner_items);
                }

                container(group_col)
                    .padding([8, 10])
                    .style(|theme: &Theme| {
                        let palette = theme.palette();
                        container::Style {
                            background: Some(iced::Background::Color(
                                palette.background.weak.color,
                            )),
                            border: iced::Border {
                                color: palette.background.neutral.color,
                                width: 1.0,
                                radius: 4.0.into(),
                            },
                            ..Default::default()
                        }
                    })
                    .width(Fill)
                    .into()
            }
            DialogControl::Table {
                id,
                label,
                headers,
                allow_add,
                allow_delete,
                ..
            } => {
                let mut table_col = column![].spacing(6).width(Fill);

                if let Some(lbl) = label {
                    table_col = table_col.push(text(lbl).size(12));
                }

                let mut grid_col = column![].spacing(0).width(Fill);

                // Table headers
                let mut header_row = row![]
                    .spacing(0)
                    .align_y(iced::Alignment::Center)
                    .width(Fill);
                for h in headers {
                    header_row = header_row.push(
                        container(text(h).size(11))
                            .padding([5, 8])
                            .width(Fill)
                            .style(|theme: &Theme| container::Style {
                                border: iced::Border {
                                    color: theme
                                        .palette()
                                        .background
                                        .neutral
                                        .color
                                        .scale_alpha(0.3),
                                    width: 1.0,
                                    radius: 0.0.into(),
                                },
                                ..Default::default()
                            })
                    );
                }
                if *allow_delete {
                    header_row = header_row.push(Space::new().width(Length::Fixed(28.0)));
                }
                grid_col = grid_col.push(
                    container(header_row)
                        .style(|theme: &Theme| {
                            let palette = theme.palette();
                            container::Style {
                                background: Some(iced::Background::Color(
                                    palette.background.weakest.color,
                                )),
                                border: iced::Border {
                                    color: palette.background.neutral.color,
                                    width: 1.0,
                                    radius: 0.0.into(),
                                },
                                ..Default::default()
                            }
                        })
                        .width(Fill)
                );

                // Table data rows
                let current_rows = self.table_values.get(id);
                let mut rows_list = column![].spacing(0).width(Fill);

                if let Some(rows) = current_rows {
                    for (r_idx, row_cells) in rows.iter().enumerate() {
                        let mut r_widget = row![]
                            .spacing(0)
                            .align_y(iced::Alignment::Center)
                            .width(Fill);
                        for (c_idx, cell_text) in row_cells.iter().enumerate() {
                            let id_clone = id.clone();
                            let mut ti = text_input("", cell_text.as_str())
                                .style(|theme: &Theme, status| {
                                    let palette = theme.palette();
                                    match status {
                                        text_input::Status::Focused { .. } => text_input::Style {
                                            background: iced::Background::Color(
                                                palette.background.base.color,
                                            ),
                                            border: iced::Border {
                                                color: palette.primary.base.color,
                                                width: 1.0,
                                                radius: 2.0.into(),
                                            },
                                            icon: palette.background.base.text,
                                            placeholder: crate::ui::style::readable::hint_text(
                                                theme,
                                            ),
                                            value: palette.background.base.text,
                                            selection: palette.primary.base.color.scale_alpha(0.5),
                                        },
                                        text_input::Status::Hovered => text_input::Style {
                                            background: iced::Background::Color(
                                                palette.background.weak.color,
                                            ),
                                            border: iced::Border {
                                                color: palette.background.neutral.color,
                                                width: 1.0,
                                                radius: 2.0.into(),
                                            },
                                            icon: palette.background.base.text,
                                            placeholder: crate::ui::style::readable::hint_text(
                                                theme,
                                            ),
                                            value: palette.background.base.text,
                                            selection: palette.primary.base.color.scale_alpha(0.5),
                                        },
                                        _ => text_input::Style {
                                            background: iced::Background::Color(
                                                iced::Color::TRANSPARENT,
                                            ),
                                            border: iced::Border {
                                                color: iced::Color::TRANSPARENT,
                                                width: 1.0,
                                                radius: 0.0.into(),
                                            },
                                            icon: palette.background.base.text,
                                            placeholder: crate::ui::style::readable::hint_text(
                                                theme,
                                            ),
                                            value: palette.background.base.text,
                                            selection: palette.primary.base.color.scale_alpha(0.5),
                                        },
                                    }
                                })
                                .padding([4, 6])
                                .size(12)
                                .width(Fill);
                            if !is_disabled {
                                ti = ti.on_input(move |val| {
                                    Message::PluginDialogTableCellChange(id_clone.clone(), r_idx, c_idx, val)
                                });
                            }

                            let cell_container =
                                container(ti)
                                    .width(Fill)
                                    .style(|theme: &Theme| container::Style {
                                        border: iced::Border {
                                            color: theme
                                                .palette()
                                                .background
                                                .neutral
                                                .color
                                                .scale_alpha(0.2),
                                            width: 1.0,
                                            radius: 0.0.into(),
                                        },
                                        ..Default::default()
                                    });
                            r_widget = r_widget.push(cell_container);
                        }
                        if *allow_delete {
                            let id_del = id.clone();
                            let mut del_btn = button(
                                crate::ui::icons::themed_secondary(crate::ui::icons::CLOSE, 10.0)
                            )
                            .style(button::text)
                            .padding([3, 8]);
                            if !is_disabled {
                                del_btn = del_btn.on_press(Message::PluginDialogTableRemoveRow(id_del, r_idx));
                            }
                            r_widget = r_widget.push(del_btn);
                        }

                        let row_container =
                            container(r_widget).width(Fill).style(move |theme: &Theme| {
                                let palette = theme.palette();
                                let bg = if r_idx % 2 == 0 {
                                    palette.background.base.color
                                } else {
                                    palette.background.weak.color
                                };
                                container::Style {
                                    background: Some(iced::Background::Color(bg)),
                                    border: iced::Border {
                                        color: palette.background.neutral.color.scale_alpha(0.25),
                                        width: 1.0,
                                        radius: 0.0.into(),
                                    },
                                    ..Default::default()
                                }
                            });

                        rows_list = rows_list.push(row_container);
                    }
                }

                let scrollable_rows = scrollable(rows_list)
                    .direction(scrollable::Direction::Vertical(
                        scrollable::Scrollbar::default(),
                    ))
                    .height(Length::Shrink)
                    .width(Fill);

                grid_col = grid_col.push(scrollable_rows);

                let grid_container = container(grid_col)
                    .style(|theme: &Theme| {
                        let palette = theme.palette();
                        container::Style {
                            background: Some(iced::Background::Color(
                                palette.background.base.color,
                            )),
                            border: iced::Border {
                                color: palette.background.neutral.color,
                                width: 1.0,
                                radius: 4.0.into(),
                            },
                            ..Default::default()
                        }
                    })
                    .width(Fill);

                table_col = table_col.push(grid_container);

                if *allow_add {
                    let id_add = id.clone();
                    let mut add_btn = button(text(crate::t!("Add row").into_owned()).size(11))
                        .style(button_style(false))
                        .padding([4, 10]);
                    if !is_disabled {
                        add_btn = add_btn.on_press(Message::PluginDialogTableAddRow(id_add));
                    }
                    table_col = table_col.push(
                        row![Space::new().width(Fill), add_btn].width(Fill)
                    );
                }

                container(table_col)
                    .padding([8, 10])
                    .style(|theme: &Theme| {
                        let palette = theme.palette();
                        container::Style {
                            background: Some(iced::Background::Color(
                                palette.background.weak.color,
                            )),
                            border: iced::Border {
                                color: palette.background.neutral.color,
                                width: 1.0,
                                radius: 4.0.into(),
                            },
                            ..Default::default()
                        }
                    })
                    .width(Fill)
                    .into()
            }
        }
    }
}

/// Parses every [`DialogControl::Svg`] in `controls` (recursively) once,
/// keyed by control id, so a view pass reuses the handle instead of copying
/// and hashing the document on every frame.
pub(crate) fn collect_svg_handles(
    controls: &[DialogControl],
    out: &mut HashMap<String, iced::widget::svg::Handle>,
) {
    for control in controls {
        match control {
            DialogControl::Svg { id, svg, .. } => {
                out.insert(
                    id.clone(),
                    iced::widget::svg::Handle::from_memory(svg.clone().into_bytes()),
                );
            }
            DialogControl::Fields { controls, .. }
            | DialogControl::Row { controls, .. }
            | DialogControl::Column { controls, .. } => collect_svg_handles(controls, out),
            _ => {}
        }
    }
}

/// A [`DialogControl::StyledText`] (API v19), shared by dialogs and dock
/// panels. Status styles take the theme's own success/warning/danger colour,
/// adjusted in hue only as far as it needs to read on the panel surface.
pub(crate) fn styled_text<'a>(
    msg: &'a str,
    style: TextStyle,
    align_h: Option<HorizontalAlign>,
) -> Element<'a, Message> {
    let bold = iced::Font::DEFAULT.weight(iced::font::Weight::Bold);
    let mut t = match style {
        TextStyle::Normal => text(msg).size(12),
        TextStyle::Muted => text(msg).size(12).style(|theme: &Theme| text::Style {
            color: Some(crate::ui::style::readable::muted_text(theme)),
        }),
        TextStyle::Strong => text(msg).size(12).font(bold),
        TextStyle::Heading => text(msg).size(15).font(bold),
        TextStyle::Success | TextStyle::Warning | TextStyle::Danger => text(msg)
            .size(12)
            .font(bold)
            .style(move |theme: &Theme| {
                let palette = theme.palette();
                let accent = match style {
                    TextStyle::Success => palette.success.base.color,
                    TextStyle::Warning => palette.warning.base.color,
                    _ => palette.danger.base.color,
                };
                text::Style {
                    color: Some(crate::ui::style::readable::icon_accent(accent, theme)),
                }
            }),
    };
    match align_h {
        Some(h) => {
            t = t.align_x(match h {
                HorizontalAlign::Left => iced::alignment::Horizontal::Left,
                HorizontalAlign::Center => iced::alignment::Horizontal::Center,
                HorizontalAlign::Right => iced::alignment::Horizontal::Right,
            });
            container(t).width(Fill).into()
        }
        None => t.into(),
    }
}

/// A [`DialogControl::Svg`] (API v19): the full width, keeping the aspect
/// ratio, or `height` logical pixels tall when given. Nothing when the handle
/// is missing.
pub(crate) fn svg_image<'a>(
    handle: Option<&iced::widget::svg::Handle>,
    height: Option<f32>,
) -> Element<'a, Message> {
    let Some(handle) = handle else {
        return Space::new().width(0).height(0).into();
    };
    let image = iced::widget::svg(handle.clone()).width(Fill);
    match height {
        Some(height) => image.height(Length::Fixed(height)).into(),
        None => image.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocs_plugin_api::dialog::{DialogControl, DialogDef, DialogValue};

    #[test]
    fn test_dialog_state_update_values_inplace() {
        let def = DialogDef::new("Test Dialog")
            .with_control(DialogControl::Text {
                id: "status".to_string(),
                label: "Status:".to_string(),
                default: "Initial".to_string(),
            })
            .with_control(DialogControl::Table {
                id: "scripts".to_string(),
                label: Some("Startup Scripts".to_string()),
                headers: vec!["Path".to_string()],
                rows: vec![],
                allow_add: true,
                allow_delete: true,
            })
            .with_control(DialogControl::Choice {
                id: "category".to_string(),
                label: "Category:".to_string(),
                options: vec!["Cat 1".to_string(), "Cat 2".to_string()],
                default_index: 0,
            })
            .with_control(DialogControl::Fields {
                id: "nested_group".to_string(),
                title: Some("Group".to_string()),
                controls: vec![DialogControl::Choice {
                    id: "nested_choice".to_string(),
                    label: "Nested:".to_string(),
                    options: vec!["Static A".to_string(), "Static B".to_string()],
                    default_index: 0,
                }],
                collapsed: false,
            })
            .with_control(DialogControl::Button {
                id: "apply_btn".to_string(),
                label: "Apply".to_string(),
                action: Some("apply".to_string()),
                align_h: None,
                width: None,
            });

        let mut state = PluginDialogState::new(def);
        assert_eq!(state.text_values.get("status").unwrap(), "Initial");
        assert_eq!(state.table_values.get("scripts").unwrap().len(), 0);
        assert_eq!(*state.choice_values.get("category").unwrap(), 0);
        assert!(!state.disabled_controls.contains("apply_btn"));
        assert_eq!(state.choice_option_text("category", 1), "Cat 2");
        assert_eq!(state.choice_option_text("nested_choice", 1), "Static B");

        let mut updates = HashMap::new();
        updates.insert("status".to_string(), DialogValue::Text("Updated Status".to_string()));
        updates.insert(
            "scripts".to_string(),
            DialogValue::Table(vec![vec!["C:/path/script1.lsp".to_string()]]),
        );
        updates.insert(
            "category".to_string(),
            DialogValue::ChoiceOptions {
                options: vec!["Opt A".to_string(), "Opt B".to_string(), "Opt C".to_string()],
                selected_index: 2,
            },
        );
        updates.insert("apply_btn".to_string(), DialogValue::Enabled(false));
        updates.insert("scripts:visible".to_string(), DialogValue::Visible(true));
        updates.insert("advanced_section".to_string(), DialogValue::Visible(false));

        state.update_values(updates);

        assert_eq!(state.text_values.get("status").unwrap(), "Updated Status");
        let table = state.table_values.get("scripts").unwrap();
        assert_eq!(table.len(), 1);
        assert_eq!(table[0][0], "C:/path/script1.lsp");
        assert_eq!(*state.choice_values.get("category").unwrap(), 2);
        assert_eq!(
            state.dynamic_choice_options.get("category").unwrap(),
            &vec!["Opt A".to_string(), "Opt B".to_string(), "Opt C".to_string()]
        );
        assert_eq!(state.choice_option_text("category", 2), "Opt C");
        assert!(state.disabled_controls.contains("apply_btn"));
        assert!(!state.hidden_controls.contains("scripts"));
        assert!(state.hidden_controls.contains("advanced_section"));

        let res_map = state.collect_response_map();
        assert_eq!(
            res_map.get("status"),
            Some(&DialogValue::Text("Updated Status".to_string()))
        );
        assert_eq!(
            res_map.get("category"),
            Some(&DialogValue::Choice {
                index: 2,
                text: "Opt C".to_string()
            })
        );
    }

    #[test]
    fn test_panel_state_shares_update_values_and_choice_lookup() {
        use crate::ui::window::plugin_panel::PluginPanelState;
        use ocs_plugin_api::dialog::PanelDef;

        let def = PanelDef::new("my_panel", "My Panel")
            .with_control(DialogControl::Fields {
                id: "sec1".to_string(),
                title: Some("Section".to_string()),
                controls: vec![
                    DialogControl::Choice {
                        id: "p_choice".to_string(),
                        label: "Choice".to_string(),
                        options: vec!["One".to_string(), "Two".to_string()],
                        default_index: 0,
                    },
                    DialogControl::Button {
                        id: "p_btn".to_string(),
                        label: "Click".to_string(),
                        action: None,
                        align_h: None,
                        width: None,
                    },
                ],
                collapsed: false,
            });

        let mut panel = PluginPanelState::from_def(def);
        assert_eq!(panel.choice_option_text("p_choice", 1), "Two");
        assert!(!panel.disabled_controls.contains("p_btn"));
        assert!(!panel.hidden_controls.contains("p_choice"));

        let mut updates = HashMap::new();
        updates.insert(
            "p_choice".to_string(),
            DialogValue::ChoiceOptions {
                options: vec!["Alpha".to_string(), "Beta".to_string()],
                selected_index: 1,
            },
        );
        updates.insert("p_btn:enabled".to_string(), DialogValue::Enabled(false));
        updates.insert("p_choice:visible".to_string(), DialogValue::Visible(false));
        panel.update_values(updates);

        assert_eq!(panel.choice_option_text("p_choice", 1), "Beta");
        assert!(panel.disabled_controls.contains("p_btn"));
        assert!(panel.hidden_controls.contains("p_choice"));
    }
}
