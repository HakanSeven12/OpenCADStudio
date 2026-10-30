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
    DialogControl, DialogDef, DialogResponse, DialogValue, HorizontalAlign, VerticalAlign,
};

use crate::app::Message;
use crate::ui::style::form::{button_style, dialog_button, field_style};

/// State backing an active declarative plugin modal dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginDialogState {
    pub def: DialogDef,
    pub text_values: HashMap<String, String>,
    pub bool_values: HashMap<String, bool>,
    pub choice_values: HashMap<String, usize>,
    pub color_values: HashMap<String, [u8; 3]>,
    pub file_values: HashMap<String, String>,
    pub table_values: HashMap<String, Vec<Vec<String>>>,
    pub collapsed_sections: HashSet<String>,
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
                DialogControl::InfoText { .. } | DialogControl::Button { .. } => {}
                DialogControl::Text { id, default, .. } => {
                    text_values.insert(id.clone(), default.clone());
                }
                DialogControl::Number { id, default, .. } => {
                    text_values.insert(id.clone(), default.to_string());
                }
                DialogControl::Checkbox { id, default, .. } => {
                    bool_values.insert(id.clone(), *default);
                }
                DialogControl::Choice { id, default_index, .. } => {
                    choice_values.insert(id.clone(), *default_index);
                }
                DialogControl::Color { id, default, .. } => {
                    color_values.insert(id.clone(), *default);
                }
                DialogControl::FilePicker { id, default_path, .. } => {
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
                DialogControl::Fields { id, controls, collapsed, .. } => {
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

        Self {
            def,
            text_values,
            bool_values,
            choice_values,
            color_values,
            file_values,
            table_values,
            collapsed_sections,
        }
    }

    fn collect_control_value(&self, control: &DialogControl) -> Option<(String, DialogValue)> {
        match control {
            DialogControl::InfoText { .. }
            | DialogControl::Button { .. }
            | DialogControl::Row { .. }
            | DialogControl::Column { .. } => None,
            DialogControl::Text { id, default, .. } => {
                let val = self.text_values.get(id).cloned().unwrap_or_else(|| default.clone());
                Some((id.clone(), DialogValue::Text(val)))
            }
            DialogControl::Number { id, default, min, max, .. } => {
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
                let text = options.get(idx).cloned().unwrap_or_default();
                Some((id.clone(), DialogValue::Choice { index: idx, text }))
            }
            DialogControl::Color { id, default, .. } => {
                let val = self.color_values.get(id).copied().unwrap_or(*default);
                Some((id.clone(), DialogValue::Color(val)))
            }
            DialogControl::FilePicker { id, default_path, .. } => {
                let val = self.file_values.get(id).cloned().unwrap_or_else(|| default_path.clone());
                Some((id.clone(), DialogValue::FilePath(val)))
            }
            DialogControl::Table { id, rows, .. } => {
                let val = self.table_values.get(id).cloned().unwrap_or_else(|| rows.clone());
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

            for control in &r.controls {
                let control_element = self.view_control(control);
                row_widget = row_widget.push(control_element);
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
            actions = actions.push(dialog_button(&self.def.cancel_label, Message::PluginDialogCancel, false));
        }

        let actions = actions
            .spacing(8)
            .align_y(iced::Alignment::Center);

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

    fn view_field_item<'a>(&'a self, control: &'a DialogControl) -> Element<'a, Message> {
        const LABEL_WIDTH: f32 = 110.0;
        match control {
            DialogControl::Text { id, label, .. } | DialogControl::Number { id, label, .. } => {
                let current = self.text_values.get(id).map(|s| s.as_str()).unwrap_or("");
                let id_clone = id.clone();
                let input = text_input("", current)
                    .on_input(move |val| Message::PluginDialogTextChange(id_clone.clone(), val))
                    .padding([3, 6])
                    .size(12)
                    .style(field_style);

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
                let id_clone = id.clone();
                let cb = checkbox(current)
                    .label(label)
                    .on_toggle(move |checked| Message::PluginDialogBoolToggle(id_clone.clone(), checked))
                    .size(14);

                row![
                    Space::new().width(Length::Fixed(LABEL_WIDTH)),
                    cb
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .width(Fill)
                .into()
            }
            DialogControl::Choice { id, label, options, .. } => {
                let current_idx = self.choice_values.get(id).copied().unwrap_or(0);
                let choice_opts: Vec<ChoiceOption> = options
                    .iter()
                    .enumerate()
                    .map(|(i, opt)| ChoiceOption { index: i, label: opt.clone() })
                    .collect();
                let selected = choice_opts.get(current_idx).cloned();
                let id_clone = id.clone();

                let picker = pick_list(selected, choice_opts, |opt| opt.label.clone())
                    .on_select(move |opt| Message::PluginDialogChoiceSelect(id_clone.clone(), opt.index))
                    .padding([3, 6])
                    .text_size(12);

                row![
                    container(text(label).size(11))
                        .width(Length::Fixed(LABEL_WIDTH)),
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
                let id_clone = id.clone();
                let id_for_pick = id.clone();

                let input = text_input("", current)
                    .on_input(move |val| Message::PluginDialogFilePicked(id_clone.clone(), Some(std::path::PathBuf::from(val))))
                    .padding([3, 6])
                    .size(12)
                    .style(field_style);

                let browse_btn = button(text("Browse...").size(11))
                    .on_press(Message::PluginDialogFilePick(id_for_pick))
                    .style(button_style(false))
                    .padding([3, 6]);

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
            _ => self.view_control(control),
        }
    }

    fn view_control<'a>(&'a self, control: &'a DialogControl) -> Element<'a, Message> {
        match control {
            DialogControl::InfoText { text: msg, align_h } => {
                let mut t = text(msg)
                    .size(12)
                    .style(|theme: &Theme| text::Style {
                        color: Some(theme.palette().background.base.text.scale_alpha(0.75)),
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
            DialogControl::Row { controls, align_h, align_v } => {
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
                    row_el = row_el.push(self.view_control(c));
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
            DialogControl::Column { controls, align_h, align_v } => {
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
                    col_el = col_el.push(self.view_control(c));
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
            DialogControl::Button { id, label, action, align_h, width } => {
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
                    .on_press(Message::PluginPanelButtonClick(String::new(), cid, act))
                    .style(button_style(false))
                    .padding([5, 10]);
                if let Some(w) = width {
                    btn = btn.width(Length::Fixed(*w));
                }
                btn.into()
            }
            DialogControl::Text { id, label, .. } => {
                let current = self.text_values.get(id).map(|s| s.as_str()).unwrap_or("");
                let id_clone = id.clone();
                let input = text_input("", current)
                    .on_input(move |val| Message::PluginDialogTextChange(id_clone.clone(), val))
                    .padding(5)
                    .size(12)
                    .style(field_style);

                column![text(label).size(11), input]
                    .spacing(4)
                    .width(Fill)
                    .into()
            }
            DialogControl::Number { id, label, .. } => {
                let current = self.text_values.get(id).map(|s| s.as_str()).unwrap_or("");
                let id_clone = id.clone();
                let input = text_input("", current)
                    .on_input(move |val| Message::PluginDialogTextChange(id_clone.clone(), val))
                    .padding(5)
                    .size(12)
                    .style(field_style);

                column![text(label).size(11), input]
                    .spacing(4)
                    .width(Fill)
                    .into()
            }
            DialogControl::Checkbox { id, label, .. } => {
                let current = self.bool_values.get(id).copied().unwrap_or(false);
                let id_clone = id.clone();
                checkbox(current)
                    .label(label)
                    .on_toggle(move |checked| Message::PluginDialogBoolToggle(id_clone.clone(), checked))
                    .size(14)
                    .into()
            }
            DialogControl::Choice { id, label, options, .. } => {
                let current_idx = self.choice_values.get(id).copied().unwrap_or(0);
                let choice_opts: Vec<ChoiceOption> = options
                    .iter()
                    .enumerate()
                    .map(|(i, opt)| ChoiceOption {
                        index: i,
                        label: opt.clone(),
                    })
                    .collect();
                let selected = choice_opts.get(current_idx).cloned();
                let id_clone = id.clone();

                let picker = pick_list(selected, choice_opts, |opt| opt.label.clone())
                    .on_select(move |opt| {
                        Message::PluginDialogChoiceSelect(id_clone.clone(), opt.index)
                    })
                    .padding([3, 6])
                    .text_size(12);

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

                let input = text_input("", current)
                    .on_input(move |val| Message::PluginDialogFilePicked(id_clone.clone(), Some(std::path::PathBuf::from(val))))
                    .padding(5)
                    .size(12)
                    .style(field_style);

                let browse_btn = button(text("Browse...").size(11))
                    .on_press(Message::PluginDialogFilePick(id_for_pick))
                    .style(button_style(false))
                    .padding([4, 8]);

                column![
                    text(label).size(11),
                    row![input, browse_btn].spacing(6).align_y(iced::Alignment::Center)
                ]
                .spacing(4)
                .width(Fill)
                .into()
            }
            DialogControl::Fields { id, title, controls, .. } => {
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
                        inner_items = inner_items.push(self.view_field_item(c));
                    }
                    group_col = group_col.push(inner_items);
                }

                container(group_col)
                    .padding([8, 10])
                    .style(|theme: &Theme| {
                        let palette = theme.palette();
                        container::Style {
                            background: Some(iced::Background::Color(palette.background.weak.color)),
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
            DialogControl::Table { id, label, headers, allow_add, allow_delete, .. } => {
                let mut table_col = column![].spacing(6).width(Fill);

                if let Some(lbl) = label {
                    table_col = table_col.push(
                        text(lbl).size(12)
                    );
                }

                let mut grid_col = column![].spacing(0).width(Fill);

                // Table headers
                let mut header_row = row![].spacing(0).align_y(iced::Alignment::Center).width(Fill);
                for h in headers {
                    header_row = header_row.push(
                        container(text(h).size(11))
                            .padding([5, 8])
                            .width(Fill)
                            .style(|theme: &Theme| container::Style {
                                border: iced::Border {
                                    color: theme.palette().background.neutral.color.scale_alpha(0.3),
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
                                background: Some(iced::Background::Color(palette.background.weakest.color)),
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
                        let mut r_widget = row![].spacing(0).align_y(iced::Alignment::Center).width(Fill);
                        for (c_idx, cell_text) in row_cells.iter().enumerate() {
                            let id_clone = id.clone();
                            let ti = text_input("", cell_text.as_str())
                                .on_input(move |val| {
                                    Message::PluginDialogTableCellChange(id_clone.clone(), r_idx, c_idx, val)
                                })
                                .style(|theme: &Theme, status| {
                                    let palette = theme.palette();
                                    match status {
                                        text_input::Status::Focused { .. } => text_input::Style {
                                            background: iced::Background::Color(palette.background.base.color),
                                            border: iced::Border {
                                                color: palette.primary.base.color,
                                                width: 1.0,
                                                radius: 2.0.into(),
                                            },
                                            icon: palette.background.base.text,
                                            placeholder: palette.background.base.text.scale_alpha(0.4),
                                            value: palette.background.base.text,
                                            selection: palette.primary.base.color.scale_alpha(0.5),
                                        },
                                        text_input::Status::Hovered => text_input::Style {
                                            background: iced::Background::Color(palette.background.weak.color),
                                            border: iced::Border {
                                                color: palette.background.neutral.color,
                                                width: 1.0,
                                                radius: 2.0.into(),
                                            },
                                            icon: palette.background.base.text,
                                            placeholder: palette.background.base.text.scale_alpha(0.4),
                                            value: palette.background.base.text,
                                            selection: palette.primary.base.color.scale_alpha(0.5),
                                        },
                                        _ => text_input::Style {
                                            background: iced::Background::Color(iced::Color::TRANSPARENT),
                                            border: iced::Border {
                                                color: iced::Color::TRANSPARENT,
                                                width: 1.0,
                                                radius: 0.0.into(),
                                            },
                                            icon: palette.background.base.text,
                                            placeholder: palette.background.base.text.scale_alpha(0.4),
                                            value: palette.background.base.text,
                                            selection: palette.primary.base.color.scale_alpha(0.5),
                                        },
                                    }
                                })
                                .padding([4, 6])
                                .size(12)
                                .width(Fill);

                            let cell_container = container(ti)
                                .width(Fill)
                                .style(|theme: &Theme| container::Style {
                                    border: iced::Border {
                                        color: theme.palette().background.neutral.color.scale_alpha(0.2),
                                        width: 1.0,
                                        radius: 0.0.into(),
                                    },
                                    ..Default::default()
                                });
                            r_widget = r_widget.push(cell_container);
                        }
                        if *allow_delete {
                            let id_del = id.clone();
                            let del_btn = button(
                                crate::ui::icons::themed_secondary(crate::ui::icons::CLOSE, 10.0)
                            )
                            .on_press(Message::PluginDialogTableRemoveRow(id_del, r_idx))
                            .style(button::text)
                            .padding([3, 8]);
                            r_widget = r_widget.push(del_btn);
                        }

                        let row_container = container(r_widget)
                            .width(Fill)
                            .style(move |theme: &Theme| {
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
                            background: Some(iced::Background::Color(palette.background.base.color)),
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
                    let add_btn = button(text(crate::t!("Add row").into_owned()).size(11))
                        .on_press(Message::PluginDialogTableAddRow(id_add))
                        .style(button_style(false))
                        .padding([4, 10]);
                    table_col = table_col.push(
                        row![Space::new().width(Fill), add_btn].width(Fill)
                    );
                }

                container(table_col)
                    .padding([8, 10])
                    .style(|theme: &Theme| {
                        let palette = theme.palette();
                        container::Style {
                            background: Some(iced::Background::Color(palette.background.weak.color)),
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
