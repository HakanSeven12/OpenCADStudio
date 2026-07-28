//! Dimension Style Manager window — fills the entire OS window.

use crate::app::{ColorPickTarget, DsField, Message};
use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_input, Space,
};
use iced::{Background, Border, Element, Fill, Theme};

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
    pub dimfit: &'a str,
    pub dimdsep: &'a str,
    pub dimrnd: &'a str,
    pub dimzin: &'a str,
    pub dimfrac: &'a str,
    pub dimaunit: &'a str,
    pub dimadec: &'a str,
    pub dimunit: &'a str,
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
    /// Colour field whose expanded palette is currently open.
    pub color_open: Option<DsField>,
}

fn tab_btn_style(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, st| {
        let palette = theme.extended_palette();
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
    let palette = theme.extended_palette();
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

fn muted_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().background.base.text.scale_alpha(0.68)),
    }
}

fn primary_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().primary.base.color),
    }
}

fn hdivider<'a>() -> Element<'a, Message> {
    container(Space::new().width(Fill).height(1))
        .width(Fill)
        .height(1)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.neutral.color
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
) -> Element<'a, Message> {
    // ── Tab bar ───────────────────────────────────────────────────────────
    let tabs = row![
        button(text("Lines").size(11))
            .on_press(Message::DimStyleDialogTab(0))
            .style(tab_btn_style(tab == 0))
            .padding([4, 10]),
        button(text("Arrows").size(11))
            .on_press(Message::DimStyleDialogTab(1))
            .style(tab_btn_style(tab == 1))
            .padding([4, 10]),
        button(text("Text").size(11))
            .on_press(Message::DimStyleDialogTab(2))
            .style(tab_btn_style(tab == 2))
            .padding([4, 10]),
        button(text("Scale/Units").size(11))
            .on_press(Message::DimStyleDialogTab(3))
            .style(tab_btn_style(tab == 3))
            .padding([4, 10]),
        button(text("Tolerances").size(11))
            .on_press(Message::DimStyleDialogTab(4))
            .style(tab_btn_style(tab == 4))
            .padding([4, 10]),
        button(text("Alternate").size(11))
            .on_press(Message::DimStyleDialogTab(5))
            .style(tab_btn_style(tab == 5))
            .padding([4, 10]),
    ]
    .spacing(2);

    let lbl = |s: &'static str| text(s).size(11).style(muted_style).width(180);

    let mk_field = |fld: DsField, val: &'a str| -> Element<'a, Message> {
        text_input("", val)
            .on_input(move |s| Message::DsEdit(fld.clone(), s))
            .style(field_style)
            .size(11)
            .width(100)
            .into()
    };

    let chk = |label: &'static str, val: bool, fld: DsField| -> Element<'a, Message> {
        checkbox(val)
            .label(label)
            .on_toggle(move |_| Message::DsToggle(fld.clone()))
            .size(14)
            .text_size(11)
            .into()
    };

    // Enum dropdown: maps the stored integer code to a named option and back,
    // so the user picks "Above" rather than typing "1".
    let enum_field = move |label: &'static str,
                           fld: DsField,
                           val: &'a str,
                           opts: &'static [(&'static str, &'static str)]|
          -> Element<'a, Message> {
        let labels: Vec<String> = opts.iter().map(|(_, l)| (*l).to_string()).collect();
        let cur = opts
            .iter()
            .find(|(c, _)| *c == val.trim())
            .map(|(_, l)| (*l).to_string())
            .unwrap_or_else(|| val.to_string());
        row![
            lbl(label),
            pick_list(labels, Some(cur), move |chosen| {
                let code = opts
                    .iter()
                    .find(|(_, l)| *l == chosen.as_str())
                    .map(|(c, _)| (*c).to_string())
                    .unwrap_or(chosen);
                Message::DsEdit(fld.clone(), code)
            })
            .text_size(11)
            .width(150),
        ]
        .spacing(8)
        .align_y(iced::Center)
        .into()
    };

    // Zero-suppression codes shared by DIMZIN / DIMALTZ / DIMALTTZ / DIMTZIN.
    const OPT_ZIN: &[(&str, &str)] = &[
        ("0", "Suppress none"),
        ("1", "Suppress 0 ft & 0 in"),
        ("2", "Keep 0 ft, drop 0 in"),
        ("3", "Drop 0 ft, keep 0 in"),
        ("4", "Suppress leading"),
        ("8", "Suppress trailing"),
        ("12", "Suppress leading & trailing"),
    ];
    // Linear unit formats shared by DIMLUNIT / DIMALTU.
    const OPT_LUNIT: &[(&str, &str)] = &[
        ("1", "Scientific"),
        ("2", "Decimal"),
        ("3", "Engineering"),
        ("4", "Architectural"),
        ("5", "Fractional"),
        ("6", "Windows desktop"),
    ];

    // Shared colour selector (main dropdown + "more" palette), reusing the
    // existing DsEdit path (the chosen colour is sent as an ACI string).
    let color_open = vals.color_open.clone();
    let color_row = move |label: &'static str, fld: DsField, _val: &'a str| -> Element<'a, Message> {
        let cur = crate::ui::color_select::aci_string_to_color(_val);
        let open = color_open.as_ref() == Some(&fld);
        let f_sel = fld.clone();
        let selector = crate::ui::color_select::color_selector(
            cur,
            open,
            crate::ui::color_select::ColorExtras {
                by_layer: true,
                by_block: true,
            },
            move |c| Message::DsEdit(f_sel.clone(), crate::ui::color_select::color_to_aci_string(c)),
            Message::DsColorMore(fld.clone()),
            Message::OpenColorWindow(ColorPickTarget::DimStyle(fld.clone())),
        );
        row![lbl(label), selector]
            .spacing(8)
            .align_y(iced::Center)
            .into()
    };

    // Block / linetype Handle dropdown: pick a block-record (arrowheads) or a
    // linetype by name from the available records.
    let hrow = move |label: &'static str,
                     options: Vec<String>,
                     selected: String,
                     field: &'static str|
          -> Element<'a, Message> {
        row![
            lbl(label),
            pick_list(options, Some(selected), move |value| {
                Message::DsSetHandle { field, value }
            })
            .text_size(11)
            .width(150),
        ]
        .spacing(8)
        .align_y(iced::Center)
        .into()
    };

    let tab_content: Element<'_, Message> = match tab {
        0 => column![
            text("Dimension Line").size(11).style(primary_style),
            row![
                lbl("Extension (DIMDLE)"),
                mk_field(DsField::Dimdle, vals.dimdle)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl("Spacing (DIMDLI)"),
                mk_field(DsField::Dimdli, vals.dimdli)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl("Text gap (DIMGAP)"),
                mk_field(DsField::Dimgap, vals.dimgap)
            ]
            .spacing(8)
            .align_y(iced::Center),
            chk("Suppress 1st line (DIMSD1)", vals.dimsd1, DsField::Dimsd1),
            chk("Suppress 2nd line (DIMSD2)", vals.dimsd2, DsField::Dimsd2),
            text("Extension Line").size(11).style(primary_style),
            row![
                lbl("Extension (DIMEXE)"),
                mk_field(DsField::Dimexe, vals.dimexe)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl("Offset (DIMEXO)"),
                mk_field(DsField::Dimexo, vals.dimexo)
            ]
            .spacing(8)
            .align_y(iced::Center),
            chk("Suppress 1st line (DIMSE1)", vals.dimse1, DsField::Dimse1),
            chk("Suppress 2nd line (DIMSE2)", vals.dimse2, DsField::Dimse2),
            color_row("Dim line color ACI (DIMCLRD)", DsField::Dimclrd, vals.dimclrd),
            row![
                lbl("Dim line weight (DIMLWD)"),
                mk_field(DsField::Dimlwd, vals.dimlwd)
            ]
            .spacing(8)
            .align_y(iced::Center),
            color_row("Ext line color ACI (DIMCLRE)", DsField::Dimclre, vals.dimclre),
            row![
                lbl("Ext line weight (DIMLWE)"),
                mk_field(DsField::Dimlwe, vals.dimlwe)
            ]
            .spacing(8)
            .align_y(iced::Center),
            chk(
                "Fixed-length ext lines (DIMFXLON)",
                vals.dimfxlon,
                DsField::Dimfxlon
            ),
            row![
                lbl("Fixed length (DIMFXL)"),
                mk_field(DsField::Dimfxl, vals.dimfxl)
            ]
            .spacing(8)
            .align_y(iced::Center),
            hrow(
                "Dim line linetype (DIMLTYPE)",
                vals.lt_opts.clone(),
                vals.dimltex_name.clone(),
                "dimltex_handle"
            ),
            hrow(
                "Ext line 1 linetype (DIMLTEX1)",
                vals.lt_opts.clone(),
                vals.dimltex1_name.clone(),
                "dimltex1_handle"
            ),
            hrow(
                "Ext line 2 linetype (DIMLTEX2)",
                vals.lt_opts.clone(),
                vals.dimltex2_name.clone(),
                "dimltex2_handle"
            ),
        ]
        .spacing(7)
        .into(),
        1 => column![
            text("Arrows").size(11).style(primary_style),
            hrow(
                "Arrowhead (DIMBLK)",
                vals.block_opts.clone(),
                vals.dimblk_name.clone(),
                "dimblk"
            ),
            hrow(
                "1st arrowhead (DIMBLK1)",
                vals.block_opts.clone(),
                vals.dimblk1_name.clone(),
                "dimblk1"
            ),
            hrow(
                "2nd arrowhead (DIMBLK2)",
                vals.block_opts.clone(),
                vals.dimblk2_name.clone(),
                "dimblk2"
            ),
            hrow(
                "Leader arrowhead (DIMLDRBLK)",
                vals.block_opts.clone(),
                vals.dimldrblk_name.clone(),
                "dimldrblk"
            ),
            row![
                lbl("Arrow size (DIMASZ)"),
                mk_field(DsField::Dimasz, vals.dimasz)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl("Center mark (DIMCEN)"),
                mk_field(DsField::Dimcen, vals.dimcen)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl("Tick size (DIMTSZ)"),
                mk_field(DsField::Dimtsz, vals.dimtsz)
            ]
            .spacing(8)
            .align_y(iced::Center),
            chk(
                "Separate arrow blocks (DIMSAH)",
                vals.dimsah,
                DsField::Dimsah
            ),
            enum_field(
                "Arc length symbol (DIMARCSYM)",
                DsField::Dimarcsym,
                vals.dimarcsym,
                &[("0", "Before text"), ("1", "Above text"), ("2", "None")],
            ),
            row![
                lbl("Jog angle ° (DIMJOGANG)"),
                mk_field(DsField::Dimjogang, vals.dimjogang)
            ]
            .spacing(8)
            .align_y(iced::Center),
        ]
        .spacing(7)
        .into(),
        2 => column![
            text("Text").size(11).style(primary_style),
            row![
                lbl("Height (DIMTXT)"),
                mk_field(DsField::Dimtxt, vals.dimtxt)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl("Style (DIMTXSTY)"),
                mk_field(DsField::Dimtxsty, vals.dimtxsty)
            ]
            .spacing(8)
            .align_y(iced::Center),
            enum_field(
                "Vertical pos (DIMTAD)",
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
            chk("Horizontal inside (DIMTIH)", vals.dimtih, DsField::Dimtih),
            chk("Horizontal outside (DIMTOH)", vals.dimtoh, DsField::Dimtoh),
            color_row("Text color ACI (DIMCLRT)", DsField::Dimclrt, vals.dimclrt),
            enum_field(
                "Horizontal just (DIMJUST)",
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
                lbl("Vertical pos (DIMTVP)"),
                mk_field(DsField::Dimtvp, vals.dimtvp)
            ]
            .spacing(8)
            .align_y(iced::Center),
            enum_field(
                "Text fill mode (DIMTFILL)",
                DsField::Dimtfill,
                vals.dimtfill,
                &[
                    ("0", "None"),
                    ("1", "Drawing background"),
                    ("2", "Color"),
                ],
            ),
            color_row("Fill color ACI (DIMTFILLCLR)", DsField::Dimtfillclr, vals.dimtfillclr),
            chk(
                "Left-to-right (DIMTXTDIRECTION)",
                vals.dimtxtdirection,
                DsField::Dimtxtdirection
            ),
        ]
        .spacing(7)
        .into(),
        3 => column![
            text("Scale").size(11).style(primary_style),
            chk("Annotative", vals.annotative, DsField::Annotative),
            row![
                lbl("Overall scale (DIMSCALE)"),
                mk_field(DsField::Dimscale, vals.dimscale)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl("Linear factor (DIMLFAC)"),
                mk_field(DsField::Dimlfac, vals.dimlfac)
            ]
            .spacing(8)
            .align_y(iced::Center),
            text("Units").size(11).style(primary_style),
            enum_field("Format (DIMLUNIT)", DsField::Dimlunit, vals.dimlunit, OPT_LUNIT),
            row![
                lbl("Decimals (DIMDEC)"),
                mk_field(DsField::Dimdec, vals.dimdec)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl("Suffix (DIMPOST)"),
                mk_field(DsField::Dimpost, vals.dimpost)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl("Decimal sep ASCII (DIMDSEP)"),
                mk_field(DsField::Dimdsep, vals.dimdsep)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl("Round off (DIMRND)"),
                mk_field(DsField::Dimrnd, vals.dimrnd)
            ]
            .spacing(8)
            .align_y(iced::Center),
            enum_field("Zero suppress (DIMZIN)", DsField::Dimzin, vals.dimzin, OPT_ZIN),
            enum_field(
                "Fraction format (DIMFRAC)",
                DsField::Dimfrac,
                vals.dimfrac,
                &[
                    ("0", "Horizontal"),
                    ("1", "Diagonal"),
                    ("2", "Not stacked"),
                ],
            ),
            enum_field(
                "Angular unit (DIMAUNIT)",
                DsField::Dimaunit,
                vals.dimaunit,
                &[
                    ("0", "Decimal degrees"),
                    ("1", "Deg/min/sec"),
                    ("2", "Gradians"),
                    ("3", "Radians"),
                ],
            ),
            row![
                lbl("Angular decimals (DIMADEC)"),
                mk_field(DsField::Dimadec, vals.dimadec)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl("Unit format (DIMUNIT)"),
                mk_field(DsField::Dimunit, vals.dimunit)
            ]
            .spacing(8)
            .align_y(iced::Center),
            enum_field(
                "Angular zero supp (DIMAZIN)",
                DsField::Dimazin,
                vals.dimazin,
                &[
                    ("0", "None"),
                    ("1", "Leading"),
                    ("2", "Trailing"),
                    ("3", "Leading & trailing"),
                ],
            ),
            text("Fit").size(11).style(primary_style),
            enum_field(
                "Fit (DIMATFIT)",
                DsField::Dimatfit,
                vals.dimatfit,
                &[
                    ("0", "Move text & arrows out"),
                    ("1", "Arrows out first"),
                    ("2", "Text out first"),
                    ("3", "Best fit"),
                ],
            ),
            enum_field(
                "Text movement (DIMTMOVE)",
                DsField::Dimtmove,
                vals.dimtmove,
                &[
                    ("0", "Keep with dim line"),
                    ("1", "Add leader"),
                    ("2", "Move freely"),
                ],
            ),
            row![
                lbl("Fit (legacy DIMFIT)"),
                mk_field(DsField::Dimfit, vals.dimfit)
            ]
            .spacing(8)
            .align_y(iced::Center),
            chk("Force text inside (DIMTIX)", vals.dimtix, DsField::Dimtix),
            chk(
                "Suppress outside arrows (DIMSOXD)",
                vals.dimsoxd,
                DsField::Dimsoxd
            ),
            chk("Place text manually (DIMUPT)", vals.dimupt, DsField::Dimupt),
            chk(
                "Dim line between ext (DIMTOFL)",
                vals.dimtofl,
                DsField::Dimtofl
            ),
        ]
        .spacing(7)
        .into(),
        5 => column![
            text("Alternate Units").size(11).style(primary_style),
            chk(
                "Enable alternate units (DIMALT)",
                vals.dimalt,
                DsField::Dimalt
            ),
            row![
                lbl("Multiplier (DIMALTF)"),
                mk_field(DsField::Dimaltf, vals.dimaltf)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl("Decimals (DIMALTD)"),
                mk_field(DsField::Dimaltd, vals.dimaltd)
            ]
            .spacing(8)
            .align_y(iced::Center),
            enum_field("Unit format (DIMALTU)", DsField::Dimaltu, vals.dimaltu, OPT_LUNIT),
            row![
                lbl("Tol decimals (DIMALTTD)"),
                mk_field(DsField::Dimalttd, vals.dimalttd)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl("Round off (DIMALTRND)"),
                mk_field(DsField::Dimaltrnd, vals.dimaltrnd)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl("Suffix (DIMAPOST)"),
                mk_field(DsField::Dimapost, vals.dimapost)
            ]
            .spacing(8)
            .align_y(iced::Center),
            enum_field("Zero suppress (DIMALTZ)", DsField::Dimaltz, vals.dimaltz, OPT_ZIN),
            enum_field(
                "Tol zero supp (DIMALTTZ)",
                DsField::Dimalttz,
                vals.dimalttz,
                OPT_ZIN,
            ),
        ]
        .spacing(7)
        .into(),
        _ => column![
            text("Tolerances").size(11).style(primary_style),
            chk("Generate tolerances (DIMTOL)", vals.dimtol, DsField::Dimtol),
            chk("Limits generation (DIMLIM)", vals.dimlim, DsField::Dimlim),
            row![
                lbl("Plus tolerance (DIMTP)"),
                mk_field(DsField::Dimtp, vals.dimtp)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl("Minus tolerance (DIMTM)"),
                mk_field(DsField::Dimtm, vals.dimtm)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl("Tol. decimals (DIMTDEC)"),
                mk_field(DsField::Dimtdec, vals.dimtdec)
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                lbl("Tol. scale (DIMTFAC)"),
                mk_field(DsField::Dimtfac, vals.dimtfac)
            ]
            .spacing(8)
            .align_y(iced::Center),
            enum_field(
                "Tol. vert just (DIMTOLJ)",
                DsField::Dimtolj,
                vals.dimtolj,
                &[("0", "Bottom"), ("1", "Middle"), ("2", "Top")],
            ),
            enum_field("Tol. zero supp (DIMTZIN)", DsField::Dimtzin, vals.dimtzin, OPT_ZIN),
        ]
        .spacing(7)
        .into(),
    };

    // ── Right panel: tabs + scrollable content ────────────────────────────
    // The scrollable itself fills the full width so its scrollbar sits flush
    // against the window's right edge; horizontal insets live on the inner
    // content container instead of the panel padding.
    let right_panel = container(
        column![
            text(format!("Editing: {selected}")).size(11).style(muted_style),
            tabs,
            hdivider(),
            scrollable(container(tab_content).padding([12, 12]).width(Fill))
                .width(Fill)
                .height(Fill),
        ]
        .spacing(6)
        .height(Fill),
    )
    .height(Fill)
    .width(Fill)
    .padding(iced::Padding {
        top: 12.0,
        right: 0.0,
        bottom: 12.0,
        left: 0.0,
    });

    crate::ui::style::style_manager::view(crate::ui::style::style_manager::Scaffold {
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
        editor: right_panel.into(),
    })
}
