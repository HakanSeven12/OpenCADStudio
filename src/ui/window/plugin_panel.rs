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
use ocs_plugin_api::dialog::{DialogControl, DialogValue, HorizontalAlign, PanelDef, VerticalAlign};

use crate::app::Message;
use crate::ui::dock::{title_bar, Chrome, PanelId};
use crate::ui::style::form::{button_style, field_style};
use crate::ui::window::plugin_dialog::ChoiceOption;

const LABEL_WIDTH: f32 = 110.0;

/// State backing an active declarative plugin dockable side panel.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginPanelState {
    pub def: PanelDef,
    pub visible: bool,
    pub text_values: HashMap<String, String>,
    pub bool_values: HashMap<String, bool>,
    pub choice_values: HashMap<String, usize>,
    pub color_values: HashMap<String, [u8; 3]>,
    pub file_values: HashMap<String, String>,
    pub table_values: HashMap<String, Vec<Vec<String>>>,
    pub collapsed_sections: HashSet<String>,
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
                DialogControl::Choice { id, default_index, .. } => {
                    cv.insert(id.clone(), *default_index);
                }
                DialogControl::Color { id, default, .. } => {
                    clrv.insert(id.clone(), *default);
                }
                DialogControl::FilePicker { id, default_path, .. } => {
                    fv.insert(id.clone(), default_path.clone());
                }
                DialogControl::Fields { id, controls, collapsed, .. } => {
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
                DialogControl::Row { controls: sub_ctrls, .. }
                | DialogControl::Column { controls: sub_ctrls, .. } => {
                    for sub in sub_ctrls {
                        init_control(sub, tv, bv, cv, clrv, fv, tbv, cs);
                    }
                }
                DialogControl::InfoText { .. } | DialogControl::Button { .. } => {}
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

        Self {
            def,
            visible: false,
            text_values,
            bool_values,
            choice_values,
            color_values,
            file_values,
            table_values,
            collapsed_sections,
        }
    }

    pub fn update_values(&mut self, values: HashMap<String, DialogValue>) {
        for (k, v) in values {
            match v {
                DialogValue::Text(s) => {
                    self.text_values.insert(k, s);
                }
                DialogValue::Number(n) => {
                    self.text_values.insert(k, n.to_string());
                }
                DialogValue::Bool(b) => {
                    self.bool_values.insert(k, b);
                }
                DialogValue::Choice { index, .. } => {
                    self.choice_values.insert(k, index);
                }
                DialogValue::Color(c) => {
                    self.color_values.insert(k, c);
                }
                DialogValue::FilePath(f) => {
                    self.file_values.insert(k, f);
                }
                DialogValue::Table(rows) => {
                    self.table_values.insert(k, rows);
                }
                DialogValue::Fields(map) => {
                    self.update_values(map);
                }
            }
        }
    }

    pub fn view<'a>(&'a self, num_id: u32, width: f32, chrome: Chrome) -> Element<'a, Message> {
        let tbar = title_bar(
            PanelId::Plugin(num_id),
            self.def.title.clone(),
            chrome,
        );

        let mut body_col = column![].spacing(8).width(Fill);
        for control in &self.def.controls {
            body_col = body_col.push(self.view_control(control));
        }

        let scroll_body = scrollable(
            container(body_col)
                .padding([8, 10])
                .width(Fill)
        )
        .direction(scrollable::Direction::Vertical(scrollable::Scrollbar::default()))
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

    fn view_field_item<'a>(&'a self, control: &'a DialogControl) -> Element<'a, Message> {
        let panel_id = self.def.id.clone();
        match control {
            DialogControl::Text { id, label, .. } => {
                let current = self.text_values.get(id).map(|s| s.as_str()).unwrap_or("");
                let pid = panel_id.clone();
                let cid = id.clone();
                let input = text_input("", current)
                    .on_input(move |val| Message::PluginPanelTextChange(pid.clone(), cid.clone(), val))
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
            DialogControl::Number { id, label, .. } => {
                let current = self.text_values.get(id).map(|s| s.as_str()).unwrap_or("");
                let pid = panel_id.clone();
                let cid = id.clone();
                let input = text_input("", current)
                    .on_input(move |val| Message::PluginPanelTextChange(pid.clone(), cid.clone(), val))
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
                let pid = panel_id.clone();
                let cid = id.clone();
                let cb = checkbox(current)
                    .label(label)
                    .on_toggle(move |checked| Message::PluginPanelBoolToggle(pid.clone(), cid.clone(), checked))
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
                let pid = panel_id.clone();
                let cid = id.clone();

                let picker = pick_list(selected, choice_opts, |opt| opt.label.clone())
                    .on_select(move |opt| Message::PluginPanelChoiceSelect(pid.clone(), cid.clone(), opt.index, opt.label.clone()))
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
            _ => self.view_control(control),
        }
    }

    fn view_control<'a>(&'a self, control: &'a DialogControl) -> Element<'a, Message> {
        let panel_id = self.def.id.clone();
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
                    .on_press(Message::PluginPanelButtonClick(pid, cid, act))
                    .style(button_style(false))
                    .padding([5, 10]);
                if let Some(w) = width {
                    btn = btn.width(Length::Fixed(*w));
                } else {
                    btn = btn.width(Fill);
                }
                btn.into()
            }
            DialogControl::Text { id, label, .. } => {
                let current = self.text_values.get(id).map(|s| s.as_str()).unwrap_or("");
                let pid = panel_id.clone();
                let cid = id.clone();
                let input = text_input("", current)
                    .on_input(move |val| Message::PluginPanelTextChange(pid.clone(), cid.clone(), val))
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
                let pid = panel_id.clone();
                let cid = id.clone();
                let input = text_input("", current)
                    .on_input(move |val| Message::PluginPanelTextChange(pid.clone(), cid.clone(), val))
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
                let pid = panel_id.clone();
                let cid = id.clone();
                checkbox(current)
                    .label(label)
                    .on_toggle(move |checked| Message::PluginPanelBoolToggle(pid.clone(), cid.clone(), checked))
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
                let pid = panel_id.clone();
                let cid = id.clone();

                let picker = pick_list(selected, choice_opts, |opt| opt.label.clone())
                    .on_select(move |opt| {
                        Message::PluginPanelChoiceSelect(pid.clone(), cid.clone(), opt.index, opt.label.clone())
                    })
                    .padding(5);

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
                    row![color_preview, text(format!("RGB({}, {}, {})", rgb[0], rgb[1], rgb[2])).size(11)]
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

                let input = text_input("", current)
                    .on_input(move |val| {
                        Message::PluginPanelFilePicked(
                            pid.clone(),
                            cid.clone(),
                            Some(std::path::PathBuf::from(val)),
                        )
                    })
                    .padding(5)
                    .size(12)
                    .style(field_style);

                let pid_pick = panel_id.clone();
                let browse_btn = button(text("Browse...").size(11))
                    .on_press(Message::PluginPanelFilePick(pid_pick, cid_for_pick))
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
                            let pid = panel_id.clone();
                            let tid = id.clone();
                            let ti = text_input("", cell_text.as_str())
                                .on_input(move |val| {
                                    Message::PluginPanelTableCellChange(pid.clone(), tid.clone(), r_idx, c_idx, val)
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
                            let pid = panel_id.clone();
                            let tid = id.clone();
                            let del_btn = button(
                                crate::ui::icons::themed_secondary(crate::ui::icons::CLOSE, 10.0)
                            )
                            .on_press(Message::PluginPanelTableRemoveRow(pid, tid, r_idx))
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
                    let pid = panel_id.clone();
                    let tid = id.clone();
                    let add_btn = button(text(crate::t!("Add row").into_owned()).size(11))
                        .on_press(Message::PluginPanelTableAddRow(pid, tid))
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
