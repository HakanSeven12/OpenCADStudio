//! Dimension Style Manager window — fills the entire OS window.

use crate::app::{ColorPickTarget, DsField, Message};
use iced::widget::{
    button, canvas, checkbox, column, container, row, scrollable, text, text_input, Space,
};
use iced::{mouse, Background, Border, Element, Length, Point, Rectangle, Theme};
use crate::ui::style::common::muted_style;
use crate::t;
use std::borrow::Cow;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
struct DimEnumChoice {
    code: String,
    label: String,
}

impl fmt::Display for DimEnumChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

/// All DimStyle field values needed by the view.
pub struct DimStyleValues<'a> {
    pub dimdle: &'a str,
    pub dimdli: &'a str,
    pub dimgap: &'a str,
    pub dimexe: &'a str,
    pub dimexo: &'a str,
    pub dimsd1: bool,
    pub dimsd2: bool,
    pub dimse1: bool,
    pub dimse2: bool,
    pub dimasz: &'a str,
    pub dimcen: &'a str,
    pub dimtsz: &'a str,
    pub dimtxt: &'a str,
    pub dimtxsty: &'a str,
    pub dimtad: &'a str,
    pub dimtih: bool,
    pub dimtoh: bool,
    pub dimscale: &'a str,
    pub dimlfac: &'a str,
    pub dimlunit: &'a str,
    pub dimdec: &'a str,
    pub dimpost: &'a str,
    pub dimtol: bool,
    pub dimlim: bool,
    pub dimtp: &'a str,
    pub dimtm: &'a str,
    pub dimtdec: &'a str,
    pub dimtfac: &'a str,
    pub annotative: bool,
    pub dimclrd: &'a str,
    pub dimlwd: &'a str,
    pub dimclre: &'a str,
    pub dimlwe: &'a str,
    pub dimfxl: &'a str,
    pub dimfxlon: bool,
    pub dimsah: bool,
    pub dimarcsym: &'a str,
    pub dimjogang: &'a str,
    pub dimclrt: &'a str,
    pub dimjust: &'a str,
    pub dimtvp: &'a str,
    pub dimtfill: &'a str,
    pub dimtfillclr: &'a str,
    pub dimtxtdirection: bool,
    pub dimatfit: &'a str,
    pub dimtix: bool,
    pub dimsoxd: bool,
    pub dimtmove: &'a str,
    pub dimupt: bool,
    pub dimtofl: bool,
    pub dimdsep: &'a str,
    pub dimrnd: &'a str,
    pub dimzin: &'a str,
    pub dimfrac: &'a str,
    pub dimaunit: &'a str,
    pub dimadec: &'a str,
    pub dimazin: &'a str,
    pub dimalt: bool,
    pub dimaltf: &'a str,
    pub dimaltd: &'a str,
    pub dimaltu: &'a str,
    pub dimalttd: &'a str,
    pub dimaltrnd: &'a str,
    pub dimapost: &'a str,
    pub dimaltz: &'a str,
    pub dimalttz: &'a str,
    pub dimtolj: &'a str,
    pub dimtzin: &'a str,
    // Resolved selected names for the block/linetype Handle fields.
    pub dimblk_name: String,
    pub dimblk1_name: String,
    pub dimblk2_name: String,
    pub dimldrblk_name: String,
    pub dimltex_name: String,
    pub dimltex1_name: String,
    pub dimltex2_name: String,
    // Dropdown option lists shared by the arrowhead / linetype fields.
    pub block_opts: Vec<String>,
    pub lt_opts: Vec<String>,
    pub text_style_opts: Vec<String>,
    pub text_style_fixed_height: Option<f64>,
    pub compare_name: String,
    pub compare_opts: Vec<String>,
    pub comparison_sections: Vec<String>,
    pub read_only: bool,
    pub in_use: bool,
    /// Colour field whose expanded palette is currently open.
    pub color_open: Option<DsField>,
}

fn tab_btn_style(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, st| {
        let palette = theme.palette();
        let pair = match (active, st) {
            (true, _) => palette.primary.strong,
            (false, button::Status::Hovered | button::Status::Pressed) => {
                palette.background.strong
            }
            _ => palette.background.weak,
        };
        button::Style {
        background: Some(Background::Color(pair.color)),
        text_color: pair.text,
        border: Border {
            color: palette.background.neutral.color,
            width: 1.0,
            radius: 3.0.into(),
        },
        ..Default::default()
        }
    }
}

fn field_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let palette = theme.palette();
    let border = match status {
        text_input::Status::Focused { .. } => palette.primary.base.color,
        _ => palette.background.neutral.color,
    };
    text_input::Style {
        background: Background::Color(palette.background.base.color),
        border: Border {
            color: border,
            width: 1.0,
            radius: 3.0.into(),
        },
        icon: palette.background.base.text,
        placeholder: palette.background.base.text.scale_alpha(0.48),
        value: palette.background.base.text,
        selection: palette.primary.base.color.scale_alpha(0.5),
    }
}

fn primary_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().primary.base.color),
    }
}

struct DimensionPreview {
    ext1: bool,
    ext2: bool,
    dim1: bool,
    dim2: bool,
    tick: bool,
    arrow_size: f32,
    text_above: bool,
    basic: bool,
    text: String,
}

impl canvas::Program<Message> for DimensionPreview {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let palette = theme.palette();
        let ink = palette.background.base.text.scale_alpha(0.88);
        let guide = palette.primary.base.color;
        let stroke = canvas::Stroke::default().with_color(ink).with_width(1.2);
        let x1 = 52.0;
        let x2 = (bounds.width - 52.0).max(x1 + 40.0);
        let y = bounds.height * 0.58;
        let object_y = bounds.height - 18.0;

        let line = |a: Point, b: Point| canvas::Path::line(a, b);
        frame.stroke(
            &line(Point::new(x1 - 18.0, object_y), Point::new(x2 + 18.0, object_y)),
            canvas::Stroke::default().with_color(ink.scale_alpha(0.38)).with_width(1.0),
        );
        if self.ext1 {
            frame.stroke(&line(Point::new(x1, object_y), Point::new(x1, 22.0)), stroke.clone());
        }
        if self.ext2 {
            frame.stroke(&line(Point::new(x2, object_y), Point::new(x2, 22.0)), stroke.clone());
        }

        let text_half = (self.text.chars().count() as f32 * 3.4 + 8.0).min((x2 - x1) * 0.34);
        if self.dim1 {
            frame.stroke(&line(Point::new(x1, y), Point::new((bounds.width * 0.5 - text_half).max(x1), y)), stroke.clone());
        }
        if self.dim2 {
            frame.stroke(&line(Point::new((bounds.width * 0.5 + text_half).min(x2), y), Point::new(x2, y)), stroke.clone());
        }

        let size = self.arrow_size.clamp(5.0, 14.0);
        for (tip_x, sign) in [(x1, 1.0_f32), (x2, -1.0_f32)] {
            if self.tick {
                frame.stroke(
                    &line(
                        Point::new(tip_x - size * 0.55, y + size * 0.75),
                        Point::new(tip_x + size * 0.55, y - size * 0.75),
                    ),
                    canvas::Stroke::default().with_color(guide).with_width(1.6),
                );
            } else {
                let arrow = canvas::Path::new(|path| {
                    path.move_to(Point::new(tip_x, y));
                    path.line_to(Point::new(tip_x + sign * size, y - size * 0.42));
                    path.line_to(Point::new(tip_x + sign * size, y + size * 0.42));
                    path.close();
                });
                frame.fill(&arrow, guide);
            }
        }

        let text_y = if self.text_above { y - 16.0 } else { y };
        if self.basic {
            let frame_width = (self.text.chars().count() as f32 * 7.0 + 12.0)
                .min((x2 - x1) * 0.8);
            frame.stroke(
                &canvas::Path::rectangle(
                    Point::new(bounds.width * 0.5 - frame_width * 0.5, text_y - 9.0),
                    iced::Size::new(frame_width, 18.0),
                ),
                canvas::Stroke::default().with_color(ink).with_width(1.0),
            );
        }
        frame.fill_text(canvas::Text {
            content: self.text.clone(),
            position: Point::new(bounds.width * 0.5, text_y),
            color: ink,
            size: iced::Pixels(12.0),
            align_x: iced::advanced::text::Alignment::Center,
            align_y: iced::alignment::Vertical::Center,
            shaping: iced::advanced::text::Shaping::Advanced,
            ..Default::default()
        });
        vec![frame.into_geometry()]
    }
}

fn hdivider<'a>(width: iced::Length) -> Element<'a, Message> {
    container(Space::new().width(width).height(1))
        .width(width)
        .height(1)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.palette().background.neutral.color
            )),
            ..Default::default()
        })
        .into()
}

pub fn view_window<'a>(
    styles: Vec<String>,
    selected: &'a str,
    current: &'a str,
    tab: u8,
    vals: DimStyleValues<'a>,
    rename_active: Option<&'a str>,
    rename_buf: &'a str,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    // ── Tab bar ───────────────────────────────────────────────────────────
    let tabs = row![
        button(text(t!("Lines")).size(11))
            .on_press(Message::DimStyleDialogTab(0))
            .style(tab_btn_style(tab == 0))
            .padding([4, 10]),
        button(text(t!("Symbols and Arrows")).size(11))
            .on_press(Message::DimStyleDialogTab(1))
            .style(tab_btn_style(tab == 1))
            .padding([4, 10]),
        button(text(t!("Text")).size(11))
            .on_press(Message::DimStyleDialogTab(2))
            .style(tab_btn_style(tab == 2))
            .padding([4, 10]),
        button(text(t!("Fit")).size(11))
            .on_press(Message::DimStyleDialogTab(3))
            .style(tab_btn_style(tab == 3))
            .padding([4, 10]),
        button(text(t!("Primary Units")).size(11))
            .on_press(Message::DimStyleDialogTab(4))
            .style(tab_btn_style(tab == 4))
            .padding([4, 10]),
        button(text(t!("Alternate Units")).size(11))
            .on_press(Message::DimStyleDialogTab(5))
            .style(tab_btn_style(tab == 5))
            .padding([4, 10]),
        button(text(t!("Tolerances")).size(11))
            .on_press(Message::DimStyleDialogTab(6))
            .style(tab_btn_style(tab == 6))
            .padding([4, 10]),
    ]
    .spacing(2);

    let lbl = |s: Cow<'static, str>| text(s).size(11).style(muted_style).width(180);

    let mk_field = |fld: DsField, val: &'a str| -> Element<'a, Message> {
        if vals.read_only {
            crate::ui::read_only::field(val, 11.0, Length::Fixed(100.0))
        } else {
            text_input("", val)
                .on_input(move |s| Message::DsEdit(fld.clone(), s))
                .style(field_style)
                .size(11)
                .width(100)
                .into()
        }
    };

    let mk_field_enabled = |fld: DsField, val: &'a str, enabled: bool| -> Element<'a, Message> {
        if enabled && !vals.read_only {
            text_input("", val)
                .on_input(move |value| Message::DsEdit(fld.clone(), value))
                .style(field_style)
                .size(11)
                .width(100)
                .into()
        } else {
            crate::ui::read_only::field(val, 11.0, Length::Fixed(100.0))
        }
    };

    let chk = |label: Cow<'static, str>, val: bool, fld: DsField| -> Element<'a, Message> {
        let item = checkbox(val).label(label).size(14).text_size(11);
        if vals.read_only {
            item.into()
        } else {
            item.on_toggle(move |_| Message::DsToggle(fld.clone())).into()
        }
    };

    // Enum dropdown: maps the stored integer code to a named option and back,
    // so the user picks "Above" rather than typing "1".
    let enum_field = move |label: Cow<'static, str>,
                           fld: DsField,
                           val: &'a str,
                           opts: &'static [(&'static str, &'static str)]|
          -> Element<'a, Message> {
        let options: Vec<DimEnumChoice> = opts
            .iter()
            .map(|(code, label)| DimEnumChoice {
                code: (*code).to_string(),
                label: crate::i18n::translate(label).into_owned(),
            })
            .collect();
        let cur = options
            .iter()
            .find(|choice| choice.code == val.trim())
            .cloned()
            .unwrap_or_else(|| DimEnumChoice {
                code: val.to_string(),
                label: val.to_string(),
            });
        let choice: Element<'a, Message> = if vals.read_only {
            crate::ui::read_only::field(cur.to_string().as_str(), 11.0, Length::Fixed(150.0))
        } else {
            iced::widget::pick_list(Some(cur), options, |value| value.to_string())
                .on_select(move |chosen: DimEnumChoice| {
                    Message::DsEdit(fld.clone(), chosen.code)
                })
                .text_size(11)
                .width(150)
                .into()
        };
        row![lbl(label), choice]
        .spacing(8)
        .align_y(iced::Center)
        .into()
    };

    // Linear unit formats shared by DIMLUNIT / DIMALTU.
    const OPT_LUNIT: &[(&str, &str)] = &[
        ("1", "Scientific"),
        ("2", "Decimal"),
        ("3", "Engineering"),
        ("4", "Architectural"),
        ("5", "Fractional"),
        ("6", "Windows desktop"),
    ];
    const OPT_LINEWEIGHT: &[(&str, &str)] = &[
        ("-1", "By layer"),
        ("-2", "By block"),
        ("-3", "Default"),
        ("0", "0.00 mm"),
        ("5", "0.05 mm"),
        ("9", "0.09 mm"),
        ("13", "0.13 mm"),
        ("15", "0.15 mm"),
        ("18", "0.18 mm"),
        ("20", "0.20 mm"),
        ("25", "0.25 mm"),
        ("30", "0.30 mm"),
        ("35", "0.35 mm"),
        ("40", "0.40 mm"),
        ("50", "0.50 mm"),
        ("53", "0.53 mm"),
        ("60", "0.60 mm"),
        ("70", "0.70 mm"),
        ("80", "0.80 mm"),
        ("90", "0.90 mm"),
        ("100", "1.00 mm"),
        ("106", "1.06 mm"),
        ("120", "1.20 mm"),
        ("140", "1.40 mm"),
        ("158", "1.58 mm"),
        ("200", "2.00 mm"),
        ("211", "2.11 mm"),
    ];
    let alternate_unit_name = OPT_LUNIT
        .iter()
        .find(|(code, _)| *code == vals.dimaltu.trim())
        .map(|(_, label)| crate::i18n::translate(label).into_owned())
        .unwrap_or_else(|| vals.dimaltu.to_string());

    let zero_controls = |field: DsField,
                         value: &'a str,
                         show_feet_inches: bool,
                         enabled: bool|
     -> Element<'a, Message> {
        let raw = value.trim().parse::<i16>().unwrap_or(0);
        let feet_modes = vec![
            DimEnumChoice { code: "0".into(), label: t!("Suppress zero feet and zero inches").into_owned() },
            DimEnumChoice { code: "1".into(), label: t!("Show zero feet and zero inches").into_owned() },
            DimEnumChoice { code: "2".into(), label: t!("Show zero feet; suppress zero inches").into_owned() },
            DimEnumChoice { code: "3".into(), label: t!("Suppress zero feet; show zero inches").into_owned() },
        ];
        let feet_current = feet_modes[(raw & 3) as usize].clone();
        let feet: Element<'a, Message> = if show_feet_inches {
            let list = iced::widget::pick_list(Some(feet_current.clone()), feet_modes, |choice| choice.to_string())
                .text_size(11)
                .width(210);
            let list: Element<'a, Message> = if enabled && !vals.read_only {
                let target = field.clone();
                list.on_select(move |choice| {
                    Message::DsZeroBase(target.clone(), choice.code.parse().unwrap_or(0))
                }).into()
            } else {
                crate::ui::read_only::field(feet_current.to_string().as_str(), 11.0, Length::Fixed(210.0))
            };
            row![lbl(t!("Feet and inches")), list]
                .spacing(8).align_y(iced::Center).into()
        } else {
            Space::new().height(0).into()
        };
        let leading = checkbox(raw & 4 != 0)
            .label(t!("Suppress leading zeros"))
            .size(14)
            .text_size(11);
        let trailing = checkbox(raw & 8 != 0)
            .label(t!("Suppress trailing zeros"))
            .size(14)
            .text_size(11);
        let leading: Element<'a, Message> = if enabled && !vals.read_only {
            let target = field.clone();
            leading.on_toggle(move |_| Message::DsZeroFlag(target.clone(), 4)).into()
        } else {
            leading.into()
        };
        let trailing: Element<'a, Message> = if enabled && !vals.read_only {
            trailing.on_toggle(move |_| Message::DsZeroFlag(field.clone(), 8)).into()
        } else {
            trailing.into()
        };
        column![feet, leading, trailing].spacing(6).into()
    };

    // Shared colour selector (main dropdown + "more" palette), reusing the
    // existing DsEdit path (the chosen colour is sent as an ACI string).
    let color_open = vals.color_open.clone();
    let color_row = move |label: Cow<'static, str>, fld: DsField, _val: &'a str| -> Element<'a, Message> {
        if vals.read_only {
            return row![
                lbl(label),
                crate::ui::read_only::field(_val, 11.0, Length::Fixed(150.0)),
            ]
            .spacing(8)
            .align_y(iced::Center)
            .into();
        }
        let cur = crate::ui::color_select::aci_string_to_color(_val);
        let open = color_open.as_ref() == Some(&fld);
        let f_sel = fld.clone();
        let selector = crate::ui::color_select::color_selector(
            cur,
            open,
            crate::ui::color_select::ColorExtras {
                by_layer: true,
                by_block: true,
                ..Default::default()
            },
            move |c| Message::DsEdit(f_sel.clone(), crate::ui::color_select::color_to_aci_string(c)),
            Message::DsColorMore(fld.clone()),
            Message::OpenColorWindow(
                ColorPickTarget::DimStyle(fld.clone()),
                cur,
            ),
        );
        row![lbl(label), container(selector).width(150)]
            .spacing(8)
            .align_y(iced::Center)
            .into()
    };

    // Block / linetype Handle dropdown: pick a block-record (arrowheads) or a
    // linetype by name from the available records.
    let hrow = move |label: Cow<'static, str>,
                     options: Vec<String>,
                     selected: String,
                     field: &'static str|
          -> Element<'a, Message> {
        let list: Element<'a, Message> = if vals.read_only {
            crate::ui::read_only::field(&selected, 11.0, Length::Fixed(150.0))
        } else {
            iced::widget::pick_list(Some(selected), options, |value| value.to_string())
                .on_select(move |value| Message::DsSetHandle { field, value })
                .text_size(11)
                .width(150)
                .into()
        };
        row![lbl(label), list]
        .spacing(8)
        .align_y(iced::Center)
        .into()
    };

    let hrow_enabled = move |label: Cow<'static, str>,
                              options: Vec<String>,
                              selected: String,
                              field: &'static str,
                              enabled: bool|
          -> Element<'a, Message> {
        let list = iced::widget::pick_list(Some(selected.clone()), options, |value| value.to_string())
            .text_size(11)
            .width(150);
        let list: Element<'a, Message> = if enabled && !vals.read_only {
            list.on_select(move |value| Message::DsSetHandle { field, value }).into()
        } else {
            crate::ui::read_only::field(&selected, 11.0, Length::Fixed(150.0))
        };
        row![lbl(label), list]
            .spacing(8)
            .align_y(iced::Center)
            .into()
    };

    let text_style_field: Element<'a, Message> = if vals.read_only {
        crate::ui::read_only::field(&vals.dimtxsty, 11.0, Length::Fixed(150.0))
    } else {
        iced::widget::pick_list(
            Some(vals.dimtxsty.to_string()),
            vals.text_style_opts.clone(),
            |value| value.to_string(),
        )
        .on_select(|value| Message::DsEdit(DsField::Dimtxsty, value))
        .text_size(11)
        .width(150)
        .into()
    };
    let text_height_note: Element<'a, Message> = if let Some(height) = vals.text_style_fixed_height {
        text(t!("The selected text style fixes the height at %{height}.", height = height))
            .size(10)
            .style(muted_style)
            .into()
    } else {
        Space::new().height(0).into()
    };
    let center_mode = vals.dimcen.trim().parse::<f64>().unwrap_or(0.0);
    let tick_on = vals.dimtsz.trim().parse::<f64>().unwrap_or(0.0) > 0.0;
    let center_choices = vec![
        DimEnumChoice { code: "none".into(), label: t!("None").into_owned() },
        DimEnumChoice { code: "mark".into(), label: t!("Center mark").into_owned() },
        DimEnumChoice { code: "lines".into(), label: t!("Centerlines").into_owned() },
    ];
    let center_current = if center_mode < 0.0 {
        center_choices[2].clone()
    } else if center_mode > 0.0 {
        center_choices[1].clone()
    } else {
        center_choices[0].clone()
    };
    let center_method_field: Element<'a, Message> = if vals.read_only {
        crate::ui::read_only::field(center_current.to_string().as_str(), 11.0, Length::Fixed(150.0))
    } else {
        iced::widget::pick_list(Some(center_current), center_choices, |choice| choice.to_string())
            .on_select(|choice| Message::DsCenterMarkMode(choice.code))
            .text_size(11)
            .width(150)
            .into()
    };
    let tolerance_choices = vec![
        DimEnumChoice { code: "none".into(), label: t!("None").into_owned() },
        DimEnumChoice { code: "symmetrical".into(), label: t!("Symmetrical").into_owned() },
        DimEnumChoice { code: "deviation".into(), label: t!("Deviation").into_owned() },
        DimEnumChoice { code: "limits".into(), label: t!("Limits").into_owned() },
        DimEnumChoice { code: "basic".into(), label: t!("Basic").into_owned() },
    ];
    let tolerance_current = if vals.dimgap.trim().starts_with('-') {
        tolerance_choices[4].clone()
    } else if vals.dimlim {
        tolerance_choices[3].clone()
    } else if vals.dimtol && vals.dimtp.trim() == vals.dimtm.trim() {
        tolerance_choices[1].clone()
    } else if vals.dimtol {
        tolerance_choices[2].clone()
    } else {
        tolerance_choices[0].clone()
    };
    let tolerance_method_field: Element<'a, Message> = if vals.read_only {
        crate::ui::read_only::field(tolerance_current.to_string().as_str(), 11.0, Length::Fixed(150.0))
    } else {
        iced::widget::pick_list(
            Some(tolerance_current),
            tolerance_choices,
            |choice| choice.to_string(),
        )
        .on_select(|choice| Message::DsToleranceMode(choice.code))
        .text_size(11)
        .width(150)
        .into()
    };

    let tab_content: Element<'_, Message> = match tab {
        0 => column![
            text(t!("Dimension Line")).size(11).style(primary_style),
            row![
                lbl(t!("Extend beyond ticks")),
                mk_field(DsField::Dimdle, vals.dimdle)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl(t!("Baseline spacing")),
                mk_field(DsField::Dimdli, vals.dimdli)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl(t!("Text gap")),
                mk_field(DsField::Dimgap, vals.dimgap)
            ]
            .spacing(8)
            .align_y(iced::Center),
            chk(t!("Suppress first dimension line"), vals.dimsd1, DsField::Dimsd1),
            chk(t!("Suppress second dimension line"), vals.dimsd2, DsField::Dimsd2),
            text(t!("Extension Line")).size(11).style(primary_style),
            row![
                lbl(t!("Extend beyond dimension line")),
                mk_field(DsField::Dimexe, vals.dimexe)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl(t!("Offset from measured point")),
                mk_field(DsField::Dimexo, vals.dimexo)
            ]
            .spacing(8)
            .align_y(iced::Center),
            chk(t!("Suppress first extension line"), vals.dimse1, DsField::Dimse1),
            chk(t!("Suppress second extension line"), vals.dimse2, DsField::Dimse2),
            color_row(t!("Dimension line color"), DsField::Dimclrd, vals.dimclrd),
            enum_field(t!("Dimension lineweight"), DsField::Dimlwd, vals.dimlwd, OPT_LINEWEIGHT),
            color_row(t!("Extension line color"), DsField::Dimclre, vals.dimclre),
            enum_field(t!("Extension lineweight"), DsField::Dimlwe, vals.dimlwe, OPT_LINEWEIGHT),
            chk(
                t!("Use fixed-length extension lines"),
                vals.dimfxlon,
                DsField::Dimfxlon
            ),
            row![
                lbl(t!("Fixed length")),
                mk_field_enabled(DsField::Dimfxl, vals.dimfxl, vals.dimfxlon)
            ]
            .spacing(8)
            .align_y(iced::Center),
            hrow(
                t!("Dimension linetype"),
                vals.lt_opts.clone(),
                vals.dimltex_name.clone(),
                "dimltex_handle"
            ),
            hrow(
                t!("First extension linetype"),
                vals.lt_opts.clone(),
                vals.dimltex1_name.clone(),
                "dimltex1_handle"
            ),
            hrow(
                t!("Second extension linetype"),
                vals.lt_opts.clone(),
                vals.dimltex2_name.clone(),
                "dimltex2_handle"
            ),
        ]
        .spacing(7)
        .into(),
        1 => column![
            text(t!("Arrowheads")).size(11).style(primary_style),
            hrow_enabled(
                t!("Both arrowheads"),
                vals.block_opts.clone(),
                vals.dimblk_name.clone(),
                "dimblk",
                !vals.dimsah && !tick_on
            ),
            hrow_enabled(
                t!("First arrowhead"),
                vals.block_opts.clone(),
                vals.dimblk1_name.clone(),
                "dimblk1",
                vals.dimsah && !tick_on
            ),
            hrow_enabled(
                t!("Second arrowhead"),
                vals.block_opts.clone(),
                vals.dimblk2_name.clone(),
                "dimblk2",
                vals.dimsah && !tick_on
            ),
            hrow(
                t!("Leader arrowhead"),
                vals.block_opts.clone(),
                vals.dimldrblk_name.clone(),
                "dimldrblk"
            ),
            row![
                lbl(t!("Arrow size")),
                mk_field_enabled(DsField::Dimasz, vals.dimasz, !tick_on)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![lbl(t!("Center marks")), center_method_field]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl(t!("Center mark size")),
                mk_field_enabled(DsField::Dimcen, vals.dimcen, center_mode != 0.0)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl(t!("Tick size")),
                mk_field(DsField::Dimtsz, vals.dimtsz)
            ]
            .spacing(8)
            .align_y(iced::Center),
            chk(
                t!("Use different first and second arrowheads"),
                vals.dimsah,
                DsField::Dimsah
            ),
            enum_field(
                t!("Arc length symbol"),
                DsField::Dimarcsym,
                vals.dimarcsym,
                &[("0", "Before text"), ("1", "Above text"), ("2", "None")],
            ),
            row![
                lbl(t!("Radius jog angle (°)")),
                mk_field(DsField::Dimjogang, vals.dimjogang)
            ]
            .spacing(8)
            .align_y(iced::Center),
        ]
        .spacing(7)
        .into(),
        2 => column![
            text(t!("Text")).size(11).style(primary_style),
            row![
                lbl(t!("Text height")),
                mk_field_enabled(DsField::Dimtxt, vals.dimtxt, vals.text_style_fixed_height.is_none())
            ]
            .spacing(8)
            .align_y(iced::Center),
            text_height_note,
            row![
                lbl(t!("Text style")),
                text_style_field
            ]
            .spacing(8)
            .align_y(iced::Center),
            enum_field(
                t!("Vertical placement"),
                DsField::Dimtad,
                vals.dimtad,
                &[
                    ("0", "Centered"),
                    ("1", "Above"),
                    ("2", "Outside"),
                    ("3", "JIS"),
                    ("4", "Below"),
                ],
            ),
            chk(t!("Keep text horizontal when inside"), vals.dimtih, DsField::Dimtih),
            chk(t!("Keep text horizontal when outside"), vals.dimtoh, DsField::Dimtoh),
            color_row(t!("Text color"), DsField::Dimclrt, vals.dimclrt),
            enum_field(
                t!("Horizontal placement"),
                DsField::Dimjust,
                vals.dimjust,
                &[
                    ("0", "Centered"),
                    ("1", "At first ext"),
                    ("2", "At second ext"),
                    ("3", "Over first ext"),
                    ("4", "Over second ext"),
                ],
            ),
            row![
                lbl(t!("Vertical offset")),
                mk_field(DsField::Dimtvp, vals.dimtvp)
            ]
            .spacing(8)
            .align_y(iced::Center),
            enum_field(
                t!("Text background"),
                DsField::Dimtfill,
                vals.dimtfill,
                &[
                    ("0", "None"),
                    ("1", "Drawing background"),
                    ("2", "Color"),
                ],
            ),
            if vals.dimtfill.trim() == "2" {
                color_row(t!("Background color"), DsField::Dimtfillclr, vals.dimtfillclr)
            } else {
                row![lbl(t!("Background color")), container(text(t!("Not applicable")).size(11).style(muted_style)).padding([4, 7]).width(150)]
                    .spacing(8).align_y(iced::Center).into()
            },
            chk(
                t!("Read left to right"),
                vals.dimtxtdirection,
                DsField::Dimtxtdirection
            ),
        ]
        .spacing(7)
        .into(),
        3 => column![
            text(t!("Fit options")).size(11).style(primary_style),
            enum_field(
                t!("When space is limited"),
                DsField::Dimatfit,
                vals.dimatfit,
                &[
                    ("0", "Move text and arrows outside"),
                    ("1", "Move arrows outside first"),
                    ("2", "Move text outside first"),
                    ("3", "Best fit"),
                ],
            ),
            chk(t!("Always keep text between extension lines"), vals.dimtix, DsField::Dimtix),
            chk(t!("Suppress arrows when they do not fit"), vals.dimsoxd, DsField::Dimsoxd),
            text(t!("Text movement")).size(11).style(primary_style),
            enum_field(
                t!("When text is moved"),
                DsField::Dimtmove,
                vals.dimtmove,
                &[
                    ("0", "Keep dimension line with text"),
                    ("1", "Add a leader"),
                    ("2", "Move text freely"),
                ],
            ),
            text(t!("Scale")).size(11).style(primary_style),
            chk(t!("Annotative"), vals.annotative, DsField::Annotative),
            row![
                lbl(t!("Overall scale")),
                if vals.annotative {
                    container(text(t!("Automatic")).size(11).style(muted_style))
                        .padding([4, 7])
                        .width(100)
                        .into()
                } else {
                    mk_field(DsField::Dimscale, vals.dimscale)
                }
            ]
            .spacing(8)
            .align_y(iced::Center),
            text(t!("Fine tuning")).size(11).style(primary_style),
            chk(t!("Place text manually while creating dimensions"), vals.dimupt, DsField::Dimupt),
            chk(t!("Draw dimension line between extension lines"), vals.dimtofl, DsField::Dimtofl),
        ]
        .spacing(8)
        .into(),
        4 => column![
            text(t!("Linear dimensions")).size(11).style(primary_style),
            enum_field(t!("Unit format"), DsField::Dimlunit, vals.dimlunit, OPT_LUNIT),
            row![lbl(t!("Precision")), mk_field(DsField::Dimdec, vals.dimdec)]
                .spacing(8).align_y(iced::Center),
            if matches!(vals.dimlunit.trim(), "4" | "5") {
                enum_field(
                    t!("Fraction format"),
                    DsField::Dimfrac,
                    vals.dimfrac,
                    &[("0", "Horizontal"), ("1", "Diagonal"), ("2", "Not stacked")],
                )
            } else {
                row![lbl(t!("Fraction format")), container(text(t!("Not applicable")).size(11).style(muted_style)).padding([4, 7]).width(150)]
                    .spacing(8).align_y(iced::Center).into()
            },
            if vals.dimlunit.trim() == "2" {
                enum_field(
                    t!("Decimal separator"),
                    DsField::Dimdsep,
                    vals.dimdsep,
                    &[("46", "Period"), ("44", "Comma"), ("32", "Space")],
                )
            } else {
                row![lbl(t!("Decimal separator")), container(text(t!("Not applicable")).size(11).style(muted_style)).padding([4, 7]).width(150)]
                    .spacing(8).align_y(iced::Center).into()
            },
            row![lbl(t!("Round off")), mk_field(DsField::Dimrnd, vals.dimrnd)]
                .spacing(8).align_y(iced::Center),
            row![lbl(t!("Measurement template")), mk_field(DsField::Dimpost, vals.dimpost)]
                .spacing(8).align_y(iced::Center),
            text(t!("Use <> as the measured value; text before and after it becomes the prefix and suffix."))
                .size(10).style(muted_style),
            row![lbl(t!("Measurement scale")), mk_field(DsField::Dimlfac, vals.dimlfac)]
                .spacing(8).align_y(iced::Center),
            text(t!("Zero suppression")).size(11).style(primary_style),
            zero_controls(
                DsField::Dimzin,
                vals.dimzin,
                matches!(vals.dimlunit.trim(), "3" | "4"),
                true,
            ),
            text(t!("Angular dimensions")).size(11).style(primary_style),
            enum_field(
                t!("Unit format"),
                DsField::Dimaunit,
                vals.dimaunit,
                &[("0", "Decimal degrees"), ("1", "Degrees, minutes, seconds"), ("2", "Gradians"), ("3", "Radians")],
            ),
            row![lbl(t!("Precision")), mk_field(DsField::Dimadec, vals.dimadec)]
                .spacing(8).align_y(iced::Center),
            enum_field(
                t!("Zero suppression"),
                DsField::Dimazin,
                vals.dimazin,
                &[("0", "None"), ("1", "Leading"), ("2", "Trailing"), ("3", "Leading and trailing")],
            ),
        ]
        .spacing(8)
        .into(),
        5 => column![
            text(t!("Alternate Units")).size(11).style(primary_style),
            chk(
                t!("Display alternate units"),
                vals.dimalt,
                DsField::Dimalt
            ),
            row![
                lbl(t!("Multiplier")),
                mk_field_enabled(DsField::Dimaltf, vals.dimaltf, vals.dimalt)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl(t!("Precision")),
                mk_field_enabled(DsField::Dimaltd, vals.dimaltd, vals.dimalt)
            ]
            .spacing(8)
            .align_y(iced::Center),
            if vals.dimalt {
                enum_field(t!("Unit format"), DsField::Dimaltu, vals.dimaltu, OPT_LUNIT)
            } else {
                row![lbl(t!("Unit format")), container(text(alternate_unit_name).size(11).style(muted_style)).padding([4, 7]).width(150)]
                    .spacing(8).align_y(iced::Center).into()
            },
            row![
                lbl(t!("Tolerance precision")),
                mk_field_enabled(DsField::Dimalttd, vals.dimalttd, vals.dimalt)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl(t!("Round off")),
                mk_field_enabled(DsField::Dimaltrnd, vals.dimaltrnd, vals.dimalt)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl(t!("Measurement template")),
                mk_field_enabled(DsField::Dimapost, vals.dimapost, vals.dimalt)
            ]
            .spacing(8)
            .align_y(iced::Center),
            text(t!("Zero suppression")).size(11).style(primary_style),
            zero_controls(
                DsField::Dimaltz,
                vals.dimaltz,
                matches!(vals.dimaltu.trim(), "3" | "4"),
                vals.dimalt,
            ),
            text(t!("Tolerance zero suppression")).size(11).style(primary_style),
            zero_controls(
                DsField::Dimalttz,
                vals.dimalttz,
                matches!(vals.dimaltu.trim(), "3" | "4"),
                vals.dimalt,
            ),
        ]
        .spacing(7)
        .into(),
        _ => column![
            text(t!("Tolerances")).size(11).style(primary_style),
            row![
                lbl(t!("Method")),
                tolerance_method_field
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl(t!("Upper value")),
                mk_field_enabled(DsField::Dimtp, vals.dimtp, vals.dimtol || vals.dimlim)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl(t!("Lower value")),
                mk_field_enabled(
                    DsField::Dimtm,
                    vals.dimtm,
                    (vals.dimtol || vals.dimlim)
                        && !(vals.dimtol && vals.dimtp.trim() == vals.dimtm.trim()),
                )
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl(t!("Precision")),
                mk_field_enabled(DsField::Dimtdec, vals.dimtdec, vals.dimtol || vals.dimlim)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl(t!("Height scale")),
                mk_field_enabled(DsField::Dimtfac, vals.dimtfac, vals.dimtol || vals.dimlim)
            ]
            .spacing(8)
            .align_y(iced::Center),
            enum_field(
                t!("Vertical position"),
                DsField::Dimtolj,
                vals.dimtolj,
                &[("0", "Bottom"), ("1", "Middle"), ("2", "Top")],
            ),
            text(t!("Zero suppression")).size(11).style(primary_style),
            zero_controls(
                DsField::Dimtzin,
                vals.dimtzin,
                matches!(vals.dimlunit.trim(), "3" | "4"),
                vals.dimtol || vals.dimlim,
            ),
        ]
        .spacing(7)
        .into(),
    };

    let precision = vals.dimdec.trim().parse::<usize>().unwrap_or(2).min(8);
    let factor = vals.dimlfac.trim().parse::<f64>().unwrap_or(1.0);
    let mut measured = format!("{:.*}", precision, 125.0 * factor);
    let zero_flags = vals.dimzin.trim().parse::<i16>().unwrap_or(0);
    if zero_flags & 8 != 0 && measured.contains('.') {
        while measured.ends_with('0') {
            measured.pop();
        }
        if measured.ends_with('.') {
            measured.pop();
        }
    }
    if zero_flags & 4 != 0 && measured.starts_with("0.") {
        measured.remove(0);
    }
    if vals.dimdsep.trim() == "44" {
        measured = measured.replace('.', ",");
    } else if vals.dimdsep.trim() == "32" {
        measured = measured.replace('.', " ");
    }
    let mut preview_text = if vals.dimpost.contains("<>") {
        vals.dimpost.replace("<>", &measured)
    } else {
        format!("{measured}{}", vals.dimpost)
    };
    if vals.dimalt {
        let alternate = 125.0 * vals.dimaltf.trim().parse::<f64>().unwrap_or(1.0);
        let alt_precision = vals.dimaltd.trim().parse::<usize>().unwrap_or(2).min(8);
        let alt_value = format!("{:.*}", alt_precision, alternate);
        let alternate_text = if vals.dimapost.contains("<>") {
            vals.dimapost.replace("<>", &alt_value)
        } else {
            format!("{alt_value}{}", vals.dimapost)
        };
        preview_text.push_str(&format!("  [{alternate_text}]"));
    }
    if vals.dimlim {
        preview_text = format!(
            "{} / {}",
            preview_text,
            vals.dimtm.trim()
        );
    } else if vals.dimtol {
        if vals.dimtp.trim() == vals.dimtm.trim() {
            preview_text.push_str(&format!(" ±{}", vals.dimtp.trim()));
        } else {
            preview_text.push_str(&format!(" +{} −{}", vals.dimtp.trim(), vals.dimtm.trim()));
        }
    }
    let preview = canvas(DimensionPreview {
        ext1: !vals.dimse1,
        ext2: !vals.dimse2,
        dim1: !vals.dimsd1,
        dim2: !vals.dimsd2,
        tick: vals.dimtsz.trim().parse::<f32>().unwrap_or(0.0) > 0.0,
        arrow_size: vals.dimasz.trim().parse::<f32>().unwrap_or(1.0).abs() * 5.0 + 5.0,
        text_above: vals.dimtad.trim() != "0",
        basic: vals.dimgap.trim().starts_with('-'),
        text: preview_text,
    })
    .width(Length::Fill)
    .height(Length::Fixed(112.0));
    let preview_panel = container(preview)
        .width(Length::Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.weak.color)),
            border: Border {
                color: theme.palette().background.neutral.color,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        });
    let status = if vals.read_only {
        t!("Referenced style · Read only")
    } else if selected == current {
        t!("Current style")
    } else if vals.in_use {
        t!("Used in drawing")
    } else {
        t!("Available style")
    };
    let comparison: Element<'a, Message> = if vals.compare_opts.is_empty() {
        Space::new().height(0).into()
    } else {
        let summary = if vals.comparison_sections.is_empty() {
            t!("No differences").into_owned()
        } else {
            format!("{}: {}", t!("Different sections"), vals.comparison_sections.join(", "))
        };
        row![
            text(t!("Compare with")).size(10).style(muted_style),
            iced::widget::pick_list(
                Some(vals.compare_name.clone()),
                vals.compare_opts.clone(),
                |value| value.to_string(),
            )
            .on_select(Message::DimStyleDialogCompare)
            .text_size(11)
            .width(150),
            text(summary).size(10).style(muted_style),
        ]
        .spacing(8)
        .align_y(iced::Center)
        .into()
    };

    let right_panel = container(
        column![
            row![
                text(selected).size(13).style(primary_style),
                Space::new().width(Length::Fill),
                text(status).size(10).style(muted_style),
            ]
            .align_y(iced::Center),
            preview_panel,
            comparison,
            tabs,
            hdivider(Length::Fill),
            scrollable(container(tab_content).padding([12, 12]).width(Length::Fill))
                .width(Length::Fill)
                .height(sizing.height),
        ]
        .spacing(6)
        .height(sizing.height),
    )
    .height(sizing.height)
    .width(Length::Fill)
    .padding(iced::Padding {
        top: 12.0,
        right: 0.0,
        bottom: 12.0,
        left: 0.0,
    });

    crate::ui::style::style_manager::view(crate::ui::style::style_manager::Scaffold {
        sizing,
        kind: crate::app::StyleKind::Dim,
        styles: &styles,
        selected,
        current: Some(current),
        rename_active,
        rename_buf,
        on_new: Message::DimStyleDialogNew,
        on_copy: Message::DimStyleDialogCopy,
        on_delete: Message::DimStyleDialogDelete,
        on_select: Message::DimStyleDialogSelect,
        on_set_current: Message::DimStyleDialogSetCurrent,
        on_apply: Message::DimStyleDialogApply,
        read_only: vals.read_only,
        editor: right_panel.into(),
    })
}
