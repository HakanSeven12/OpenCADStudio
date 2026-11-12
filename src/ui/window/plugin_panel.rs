//! Native dock panel renderer for declarative plugin palettes.
//!
//! Renders persistent dockable side panels defined by plugins into native iced
//! widget trees with title bar, collapse pins, inputs, buttons, collapsible sections,
//! and data tables, integrated with OpenCADStudio's docking system.

use std::collections::{HashMap, HashSet};
use iced::widget::{
    button, checkbox, column, container, pick_list, row, rule, scrollable, text, text_input, Space,
};
use iced::{Element, Fill, Length, Theme};
use ocs_plugin_api::dialog::{
    DialogControl, DialogValue, HorizontalAlign, PanelDef, PanelSnapshot, VerticalAlign,
};

use crate::app::Message;
use crate::ui::dock::{title_bar, Chrome, PanelId};
use crate::ui::style::form::{button_style, field_style};
use crate::ui::window::plugin_dialog::{
    apply_control_value_updates, collect_svg_handles, find_choice_option_in_controls, styled_text,
    svg_image, ChoiceOption,
};

const LABEL_WIDTH: f32 = 110.0;

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
                            raw_text.insert(id.clone(), raw.clone());
                            values.insert(id.clone(), DialogValue::Text(raw.clone()));
                        } else {
                            errors.push(format!("missing text value for control id '{id}'"));
                        }
                    }
                    DialogControl::Number { id, .. } => {
                        declare_control(id, Some(SnapshotValueKind::Number), ids, expected, errors);
                        if let Some(raw) = state.text_values.get(id) {
                            raw_text.insert(id.clone(), raw.clone());
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
                            raw_text.insert(id.clone(), raw.clone());
                            values.insert(id.clone(), DialogValue::FilePath(raw.clone()));
                        } else {
                            errors.push(format!("missing file value for control id '{id}'"));
                        }
                    }
                    DialogControl::Fields { id, controls, .. } => {
                        declare_control(id, None, ids, expected, errors);
                        field_ids.insert(id.clone());
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
                        );
                        values.insert(id.clone(), DialogValue::Fields(field_values));
                    }
                    DialogControl::Table { id, .. } => {
                        declare_control(id, Some(SnapshotValueKind::Table), ids, expected, errors);
                        if let Some(rows) = state.table_values.get(id) {
                            values.insert(id.clone(), DialogValue::Table(rows.clone()));
                        } else {
                            errors.push(format!("missing table value for control id '{id}'"));
                        }
                    }
                    DialogControl::Row { controls, .. }
                    | DialogControl::Column { controls, .. } => {
                        snapshot_controls(
                            state, controls, values, raw_text, ids, expected, field_ids, errors,
                        );
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

        snapshot_controls(
            self,
            &self.def.controls,
            &mut values,
            &mut raw_text,
            &mut ids,
            &mut expected,
            &mut field_ids,
            &mut errors,
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

        let mut body_col = column![].spacing(8).width(Fill);
        for control in &self.def.controls {
            if control.id().is_some_and(|id| self.hidden_controls.contains(id)) {
                continue;
            }
            body_col = body_col.push(self.view_control_inner(control, false));
        }

        let scroll_body = scrollable(container(body_col).padding([8, 10]).width(Fill))
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::default(),
            ))
            .height(Fill)
            .width(Fill);

        container(column![tbar, scroll_body].width(Fill).height(Fill))
            .width(Length::Fixed(width))
            .height(Fill)
            .style(|theme: &Theme| {
                let palette = theme.palette();
                container::Style {
                    background: Some(iced::Background::Color(palette.background.base.color)),
                    border: iced::Border {
                        color: palette.background.neutral.color,
                        width: 1.0,
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                }
            })
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
        let panel_id = self.def.id.clone();
        match control {
            DialogControl::Text { id, label, .. } | DialogControl::Number { id, label, .. } => {
                let current = self.text_values.get(id).map(|s| s.as_str()).unwrap_or("");
                let pid = panel_id.clone();
                let cid = id.clone();
                let mut input = text_input("", current)
                    .padding([3, 6])
                    .size(12)
                    .style(field_style);
                if !is_disabled {
                    input = input.on_input(move |val| {
                        Message::PluginPanelTextChange(pid.clone(), cid.clone(), val)
                    });
                }

                row![
                    container(text(label).size(11))
                        .width(Length::Fixed(LABEL_WIDTH)),
                    input
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .width(Fill)
                .into()
            }
            DialogControl::Checkbox { id, label, .. } => {
                let current = self.bool_values.get(id).copied().unwrap_or(false);
                let pid = panel_id.clone();
                let cid = id.clone();
                let mut cb = checkbox(current)
                    .label(label)
                    .size(14);
                if !is_disabled {
                    cb = cb.on_toggle(move |checked| {
                        Message::PluginPanelBoolToggle(pid.clone(), cid.clone(), checked)
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
                let pid = panel_id.clone();
                let cid = id.clone();

                let mut picker = pick_list(selected, choice_opts, |opt| opt.label.clone())
                    .padding([3, 6])
                    .text_size(12);
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
                    container(text(label).size(11))
                        .width(Length::Fixed(LABEL_WIDTH)),
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
                let pid = panel_id.clone();
                let cid = id.clone();
                let cid_for_pick = id.clone();

                let mut input = text_input("", current)
                    .padding([3, 6])
                    .size(12)
                    .style(field_style);
                if !is_disabled {
                    input = input.on_input(move |val| {
                        Message::PluginPanelFileTextChange(pid.clone(), cid.clone(), val)
                    });
                }

                let pid_pick = panel_id.clone();
                let mut browse_btn = button(text("Browse...").size(11))
                    .style(button_style(false))
                    .padding([3, 6]);
                if !is_disabled {
                    browse_btn =
                        browse_btn.on_press(Message::PluginPanelFilePick(pid_pick, cid_for_pick));
                }

                row![
                    container(text(label).size(11))
                        .width(Length::Fixed(LABEL_WIDTH)),
                    row![input, browse_btn].spacing(4).align_y(iced::Alignment::Center)
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
                let pid = panel_id.clone();
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
                } else {
                    label_txt = label_txt.align_x(iced::Center);
                }
                let mut btn = button(label_txt)
                    .style(button_style(false))
                    .padding([5, 10]);
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
            DialogControl::Text { id, label, .. } | DialogControl::Number { id, label, .. } => {
                let current = self.text_values.get(id).map(|s| s.as_str()).unwrap_or("");
                let pid = panel_id.clone();
                let cid = id.clone();
                let mut input = text_input("", current)
                    .padding(5)
                    .size(12)
                    .style(field_style);
                if !is_disabled {
                    input = input.on_input(move |val| {
                        Message::PluginPanelTextChange(pid.clone(), cid.clone(), val)
                    });
                }

                column![text(label).size(11), input]
                    .spacing(4)
                    .width(Fill)
                    .into()
            }
            DialogControl::Checkbox { id, label, .. } => {
                let current = self.bool_values.get(id).copied().unwrap_or(false);
                let pid = panel_id.clone();
                let cid = id.clone();
                let mut cb = checkbox(current)
                    .label(label)
                    .size(14);
                if !is_disabled {
                    cb = cb.on_toggle(move |checked| {
                        Message::PluginPanelBoolToggle(pid.clone(), cid.clone(), checked)
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
                let pid = panel_id.clone();
                let cid = id.clone();

                let mut picker = pick_list(selected, choice_opts, |opt| opt.label.clone())
                    .padding(5);
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

                column![text(label).size(11), picker]
                    .spacing(4)
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

                column![
                    text(label).size(11),
                    row![
                        color_preview,
                        text(format!("RGB({}, {}, {})", rgb[0], rgb[1], rgb[2])).size(11)
                    ]
                    .spacing(6)
                    .align_y(iced::Alignment::Center)
                ]
                .spacing(4)
                .width(Fill)
                .into()
            }
            DialogControl::FilePicker { id, label, .. } => {
                let current = self.file_values.get(id).map(|s| s.as_str()).unwrap_or("");
                let pid = panel_id.clone();
                let cid = id.clone();
                let cid_for_pick = id.clone();

                let mut input = text_input("", current)
                    .padding(5)
                    .size(12)
                    .style(field_style);
                if !is_disabled {
                    input = input.on_input(move |val| {
                        Message::PluginPanelFileTextChange(pid.clone(), cid.clone(), val)
                    });
                }

                let pid_pick = panel_id.clone();
                let mut browse_btn = button(text("Browse...").size(11))
                    .style(button_style(false))
                    .padding([4, 8]);
                if !is_disabled {
                    browse_btn =
                        browse_btn.on_press(Message::PluginPanelFilePick(pid_pick, cid_for_pick));
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
                    let pid = panel_id.clone();
                    let cid = id.clone();

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
                    .on_press(Message::PluginPanelSectionToggle(pid, cid))
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
                            let pid = panel_id.clone();
                            let tid = id.clone();
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
                                    Message::PluginPanelTableCellChange(pid.clone(), tid.clone(), r_idx, c_idx, val)
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
                            let pid = panel_id.clone();
                            let tid = id.clone();
                            let mut del_btn = button(
                                crate::ui::icons::themed_secondary(crate::ui::icons::CLOSE, 10.0)
                            )
                            .style(button::text)
                            .padding([3, 8]);
                            if !is_disabled {
                                del_btn = del_btn.on_press(Message::PluginPanelTableRemoveRow(pid, tid, r_idx));
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
                    let pid = panel_id.clone();
                    let tid = id.clone();
                    let mut add_btn = button(text(crate::t!("Add row").into_owned()).size(11))
                        .style(button_style(false))
                        .padding([4, 10]);
                    if !is_disabled {
                        add_btn = add_btn.on_press(Message::PluginPanelTableAddRow(pid, tid));
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
            (
                "length".to_string(),
                DialogValue::Text("2.5".to_string()),
            ),
        ]));
        assert_eq!(state.def.title, "IFC properties - Alignment-Axis-h-seg-001");
        assert_eq!(state.def.controls, original_controls);
        assert_eq!(state.text_values.get("length").map(String::as_str), Some("2.5"));
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
            Some(&iced::widget::svg::Handle::from_memory(svg.as_bytes().to_vec()))
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
        assert_ne!(updated.svg_handles.get("chart"), state.svg_handles.get("chart"));
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
}
