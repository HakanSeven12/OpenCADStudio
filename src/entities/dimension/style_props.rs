// Properties-panel style sections for dimensions.

use super::*;

/// Build the linear-dimension property groups from the assigned style plus
/// any entity-level DSTYLE overrides. Conditional rows remain visible but are
/// read-only while their controlling option is disabled.
pub fn style_sections(
    style: &DimStyle,
    dimension: &Dimension,
    document: &Document,
) -> Vec<PropSection> {
    use crate::entities::dim_override as ov;

    let effective_style = resolved_dimension_style(style, dimension, document);
    let data = &dimension.base().common.extended_data;
    let real = |code, inherited| ov::real(data, code).unwrap_or(inherited);
    let int = |code, inherited| ov::int(data, code).unwrap_or(inherited);
    let string =
        |code, inherited: &str| ov::string(data, code).unwrap_or_else(|| inherited.to_string());
    let on = |value: bool| if value { "On" } else { "Off" };
    let yes = |value: bool| if value { "Yes" } else { "No" };
    let number = |label: &str, field: &'static str, value: f64, editable: bool| {
        property(
            label,
            field,
            if editable {
                PropValue::EditText(crate::entities::common::format_decimal(value))
            } else {
                PropValue::ReadOnly(crate::entities::common::format_decimal(value))
            },
        )
    };
    let text = |label: &str, field: &'static str, value: String, editable: bool| {
        property(
            label,
            field,
            if editable {
                PropValue::PlainText(value)
            } else {
                PropValue::ReadOnly(value)
            },
        )
    };
    let choice =
        |label: &str, field: &'static str, selected: &str, options: &[&str], editable: bool| {
            property(
                label,
                field,
                if editable {
                    choice_value(selected, options)
                } else {
                    PropValue::ReadOnly(selected.to_string())
                },
            )
        };

    let s = &effective_style;
    let dimfxlon = int(ov::DIMFXLON, s.dimfxlon as i16) != 0;
    let dimtix = int(ov::DIMTIX, s.dimtix as i16) != 0;
    let dimlunit = int(ov::DIMLUNIT, s.dimlunit);
    let dimfrac = int(ov::DIMFRAC, s.dimfrac);
    let dimalt = int(ov::DIMALT, s.dimalt as i16) != 0;
    let dimtol = int(ov::DIMTOL, s.dimtol as i16) != 0;
    let dimlim = int(ov::DIMLIM, s.dimlim as i16) != 0;
    let dimtp = real(ov::DIMTP, s.dimtp);
    let dimtm = real(ov::DIMTM, s.dimtm);
    let dimgap = real(ov::DIMGAP, s.dimgap);
    let tolerance_display = if dimgap < 0.0 {
        "Basic"
    } else if dimlim {
        "Limits"
    } else if dimtol && (dimtp - dimtm).abs() <= 1e-12 {
        "Symmetrical"
    } else if dimtol {
        "Deviation"
    } else {
        "None"
    };
    let tolerance_enabled = matches!(tolerance_display, "Symmetrical" | "Deviation" | "Limits");
    let alternate_tolerance_enabled = tolerance_enabled && dimalt;
    let dimzin = int(ov::DIMZIN, s.dimzin);
    let dimaltz = int(ov::DIMALTZ, s.dimaltz);
    let dimtzin = int(ov::DIMTZIN, s.dimtzin);
    let dimalttz = int(ov::DIMALTTZ, s.dimalttz);
    let annotative = s.annotative;

    let overridden_text_style = ov::handle(data, ov::DIMTXSTY);
    let text_style_name = overridden_text_style
        .and_then(|handle| {
            document
                .text_styles
                .iter()
                .find(|record| record.handle == handle)
                .map(|record| record.name.clone())
        })
        .unwrap_or_else(|| s.dimtxsty.clone());
    let text_height_editable = document
        .text_styles
        .iter()
        .find(|record| {
            overridden_text_style.is_some_and(|handle| record.handle == handle)
                || (overridden_text_style.is_none()
                    && record.name.eq_ignore_ascii_case(&text_style_name))
        })
        .is_none_or(|record| !record.has_fixed_height());

    let (dim_prefix, dim_suffix) = split_measurement_template(&string(ov::DIMPOST, &s.dimpost));
    let (alt_prefix, alt_suffix) = split_measurement_template(&string(ov::DIMAPOST, &s.dimapost));
    let decimal_separator = {
        let value = int(ov::DIMDSEP, s.dimdsep);
        let character = value as u8 as char;
        if value > 0 && !character.is_control() {
            character.to_string()
        } else {
            value.to_string()
        }
    };

    let mut linetype_options: Vec<String> = document
        .line_types
        .iter()
        .map(|line_type| line_type.name.clone())
        .filter(|name| !name.is_empty())
        .collect();
    let text_style_options: Vec<String> = document
        .text_styles
        .iter()
        .map(|record| record.name.clone())
        .filter(|name| !name.is_empty())
        .collect();

    let arrow_name = |code, inherited_handle, inherited_name: &str| {
        ov::handle(data, code)
            .map(|handle| block_name(document, handle, inherited_name))
            .unwrap_or_else(|| block_name(document, inherited_handle, inherited_name))
    };
    let linetype =
        |code, inherited| linetype_name(document, ov::handle(data, code).unwrap_or(inherited));
    let arrow_1 = if matches!(dimension, Dimension::Radius(_) | Dimension::LargeRadial(_)) {
        block_name(
            document,
            ov::handle(data, ov::DIMLDRBLK).unwrap_or(s.dimldrblk),
            "Closed filled",
        )
    } else if s.dimsah {
        arrow_name(ov::DIMBLK1, s.dimblk1, &s.dimblk1_name)
    } else {
        // Separate arrows off: both ends draw DIMBLK, as the renderer does.
        block_name(document, s.dimblk, &s.dimblk_name)
    };
    let arrow_2 = if s.dimsah {
        arrow_name(ov::DIMBLK2, s.dimblk2, &s.dimblk2_name)
    } else {
        block_name(document, s.dimblk, &s.dimblk_name)
    };
    // The standard arrowheads, then any user arrow block in use. (#1820)
    let arrow_options =
        crate::entities::arrowheads::options([&arrow_1, &arrow_2].map(String::as_str));
    for current in [
        linetype(ov::DIMLTYPE, s.dimltex_handle),
        linetype(ov::DIMLTEX1, s.dimltex1_handle),
        linetype(ov::DIMLTEX2, s.dimltex2_handle),
    ] {
        if !linetype_options.contains(&current) {
            linetype_options.push(current);
        }
    }

    let precision_options: Vec<String> = (0..=8).map(precision_label).collect();
    let linear_units = [
        "Scientific",
        "Decimal",
        "Engineering",
        "Architectural",
        "Fractional",
        "Desktop",
    ];
    let alternate_unit_options = [
        "Scientific",
        "Decimal",
        "Engineering",
        "Architectural stacked",
        "Fractional stacked",
        "Architectural",
        "Fractional",
        "Desktop",
    ];

    let fill_mode = int(ov::DIMTFILL, s.dimtfill);
    let fill_color =
        ov::color(data, ov::DIMTFILLCLR).unwrap_or_else(|| AcadColor::from_index(s.dimtfillclr));
    let fill_value = match fill_mode {
        1 => choice_value("Background", &["None", "Background", "Color"]),
        2 => PropValue::ColorChoice(fill_color),
        _ => choice_value("None", &["None", "Background", "Color"]),
    };

    let mut sections = vec![
        PropSection {
            title: t!("Lines & Arrows").into_owned(),
            props: vec![
                property(
                    t!("Arrow 1").as_ref(),
                    "dim_arrowhead_1",
                    PropValue::Choice {
                        selected: arrow_1,
                        options: arrow_options.clone(),
                    },
                ),
                property(
                    t!("Arrow 2").as_ref(),
                    "dim_arrowhead_2",
                    PropValue::Choice {
                        selected: arrow_2,
                        options: arrow_options,
                    },
                ),
                number(
                    t!("Arrow size").as_ref(),
                    "dim_arrow_size",
                    real(ov::DIMASZ, s.dimasz),
                    true,
                ),
                property(
                    t!("Dim line lineweight").as_ref(),
                    "dim_line_lineweight",
                    PropValue::Choice {
                        selected: lineweight_label(int(ov::DIMLWD, s.dimlwd)),
                        options: lineweight_options(),
                    },
                ),
                property(
                    t!("Ext line lineweight").as_ref(),
                    "dim_ext_line_lineweight",
                    PropValue::Choice {
                        selected: lineweight_label(int(ov::DIMLWE, s.dimlwe)),
                        options: lineweight_options(),
                    },
                ),
                choice(
                    t!("Dim line 1").as_ref(),
                    "dim_line_1",
                    on(int(ov::DIMSD1, s.dimsd1 as i16) == 0),
                    &["On", "Off"],
                    true,
                ),
                choice(
                    t!("Dim line 2").as_ref(),
                    "dim_line_2",
                    on(int(ov::DIMSD2, s.dimsd2 as i16) == 0),
                    &["On", "Off"],
                    true,
                ),
                property(
                    t!("Dim line color").as_ref(),
                    "dim_line_color",
                    PropValue::ColorChoice(
                        ov::color(data, ov::DIMCLRD)
                            .unwrap_or_else(|| AcadColor::from_index(s.dimclrd)),
                    ),
                ),
                property(
                    t!("Dim line linetype").as_ref(),
                    "dim_linetype",
                    PropValue::Choice {
                        selected: linetype(ov::DIMLTYPE, s.dimltex_handle),
                        options: linetype_options.clone(),
                    },
                ),
                number(
                    t!("Dim line ext").as_ref(),
                    "dim_line_ext",
                    real(ov::DIMDLE, s.dimdle),
                    true,
                ),
                property(
                    t!("Ext line 1 linetype").as_ref(),
                    "dim_ext_linetype_1",
                    PropValue::Choice {
                        selected: linetype(ov::DIMLTEX1, s.dimltex1_handle),
                        options: linetype_options.clone(),
                    },
                ),
                property(
                    t!("Ext line 2 linetype").as_ref(),
                    "dim_ext_linetype_2",
                    PropValue::Choice {
                        selected: linetype(ov::DIMLTEX2, s.dimltex2_handle),
                        options: linetype_options,
                    },
                ),
                choice(
                    t!("Ext line 1").as_ref(),
                    "dim_ext_line_1",
                    on(int(ov::DIMSE1, s.dimse1 as i16) == 0),
                    &["On", "Off"],
                    true,
                ),
                choice(
                    t!("Ext line 2").as_ref(),
                    "dim_ext_line_2",
                    on(int(ov::DIMSE2, s.dimse2 as i16) == 0),
                    &["On", "Off"],
                    true,
                ),
                choice(
                    t!("Ext line fixed").as_ref(),
                    "dim_ext_line_fixed",
                    on(dimfxlon),
                    &["On", "Off"],
                    true,
                ),
                number(
                    t!("Ext line fixed length").as_ref(),
                    "dim_ext_line_fixed_length",
                    real(ov::DIMFXL, s.dimfxl),
                    dimfxlon,
                ),
                property(
                    t!("Ext line color").as_ref(),
                    "dim_ext_line_color",
                    PropValue::ColorChoice(
                        ov::color(data, ov::DIMCLRE)
                            .unwrap_or_else(|| AcadColor::from_index(s.dimclre)),
                    ),
                ),
                number(
                    t!("Ext line ext").as_ref(),
                    "dim_ext_line_ext",
                    real(ov::DIMEXE, s.dimexe),
                    true,
                ),
                number(
                    t!("Ext line offset").as_ref(),
                    "dim_ext_line_offset",
                    real(ov::DIMEXO, s.dimexo),
                    true,
                ),
            ],
        },
        PropSection {
            title: t!("Text").into_owned(),
            props: vec![
                property(t!("Fill color").as_ref(), "dim_text_fill_color", fill_value),
                choice(
                    t!("Fractional type").as_ref(),
                    "dim_fractional_type",
                    fraction_type_label(dimfrac),
                    &["Horizontal", "Diagonal", "Not stacked"],
                    matches!(dimlunit, 4 | 5),
                ),
                property(
                    t!("Text color").as_ref(),
                    "dim_text_color",
                    PropValue::ColorChoice(
                        ov::color(data, ov::DIMCLRT)
                            .unwrap_or_else(|| AcadColor::from_index(s.dimclrt)),
                    ),
                ),
                number(
                    t!("Text height").as_ref(),
                    "dim_text_height",
                    real(ov::DIMTXT, s.dimtxt),
                    text_height_editable,
                ),
                number(
                    t!("Text offset").as_ref(),
                    "dim_text_offset",
                    dimgap.abs(),
                    true,
                ),
                choice(
                    t!("Text outside align").as_ref(),
                    "dim_text_outside_align",
                    on(int(ov::DIMTOH, s.dimtoh as i16) == 0),
                    &["On", "Off"],
                    true,
                ),
                choice(
                    t!("Text pos hor").as_ref(),
                    "dim_text_pos_hor",
                    text_horizontal_label(int(ov::DIMJUST, s.dimjust)),
                    &[
                        "Centered",
                        "At extension line 1",
                        "At extension line 2",
                        "Over extension line 1",
                        "Over extension line 2",
                    ],
                    true,
                ),
                choice(
                    t!("Text pos vert").as_ref(),
                    "dim_text_pos_vert",
                    text_vertical_label(int(ov::DIMTAD, s.dimtad)),
                    &["Centered", "Above", "Outside", "JIS", "Below"],
                    true,
                ),
                property(
                    t!("Text style").as_ref(),
                    "dim_text_style",
                    PropValue::Choice {
                        selected: text_style_name,
                        options: text_style_options,
                    },
                ),
                choice(
                    t!("Text inside align").as_ref(),
                    "dim_text_inside_align",
                    on(int(ov::DIMTIH, s.dimtih as i16) == 0),
                    &["On", "Off"],
                    // Editable whether or not text is forced inside
                    // (measured on a linear dimension with Text inside Off).
                    true,
                ),
                property(
                    t!("Text position X").as_ref(),
                    "text_x",
                    PropValue::EditText(crate::entities::common::format_decimal(
                        dimension.base().text_middle_point.x,
                    )),
                ),
                property(
                    t!("Text position Y").as_ref(),
                    "text_y",
                    PropValue::EditText(crate::entities::common::format_decimal(
                        dimension.base().text_middle_point.y,
                    )),
                ),
                property(
                    t!("Text rotation").as_ref(),
                    "text_rotation",
                    PropValue::EditText(crate::entities::common::format_decimal(
                        dimension.base().text_rotation.to_degrees(),
                    )),
                ),
                choice(
                    t!("Text view direction").as_ref(),
                    "dim_text_view_direction",
                    if int(ov::DIMTXTDIRECTION, s.dimtxtdirection as i16) != 0 {
                        "Right-to-Left"
                    } else {
                        "Left-to-Right"
                    },
                    &["Left-to-Right", "Right-to-Left"],
                    true,
                ),
                property(
                    t!("Measurement").as_ref(),
                    "measurement",
                    // The value the dimension text reports (DIMLFAC applied,
                    // including viewport compensation) — not the raw paper
                    // distance, which would disagree with what is drawn.
                    PropValue::ReadOnly(crate::entities::common::format_decimal(
                        displayed_measurement(dimension, Some(s)),
                    )),
                ),
                property(
                    t!("Text override").as_ref(),
                    "text_override",
                    PropValue::PlainText(
                        dimension_text_override(dimension.base())
                            .unwrap_or("")
                            .to_string(),
                    ),
                ),
            ],
        },
        PropSection {
            title: t!("Fit").into_owned(),
            props: vec![
                choice(
                    t!("Dim line forced").as_ref(),
                    "dim_line_forced",
                    on(int(ov::DIMTOFL, s.dimtofl as i16) != 0),
                    &["On", "Off"],
                    true,
                ),
                choice(
                    t!("Dim line inside").as_ref(),
                    "dim_line_inside",
                    on(int(ov::DIMSOXD, s.dimsoxd as i16) == 0),
                    &["On", "Off"],
                    true,
                ),
                number(
                    t!("Dim scale overall").as_ref(),
                    "dim_scale_overall",
                    real(ov::DIMSCALE, s.dimscale),
                    !annotative,
                ),
                choice(
                    t!("Fit").as_ref(),
                    "dim_fit",
                    fit_label(int(ov::DIMATFIT, s.dimatfit)),
                    &["Both text and arrows", "Arrows", "Text", "Best fit"],
                    true,
                ),
                choice(
                    t!("Text inside").as_ref(),
                    "dim_text_inside",
                    on(dimtix),
                    &["On", "Off"],
                    true,
                ),
                choice(
                    t!("Text movement").as_ref(),
                    "dim_text_movement",
                    text_movement_label(int(ov::DIMTMOVE, s.dimtmove)),
                    &[
                        "Keep dim line with text",
                        "Move text, add leader",
                        "Move text, no leader",
                    ],
                    true,
                ),
            ],
        },
        PropSection {
            title: t!("Primary Units").into_owned(),
            props: vec![
                text(
                    t!("Decimal separator").as_ref(),
                    "dim_decimal_separator",
                    decimal_separator.clone(),
                    true,
                ),
                text(
                    t!("Dim prefix").as_ref(),
                    "dim_prefix",
                    dim_prefix.clone(),
                    true,
                ),
                text(
                    t!("Dim suffix").as_ref(),
                    "dim_suffix",
                    dim_suffix.clone(),
                    true,
                ),
                text(
                    t!("Dim sub-units suffix").as_ref(),
                    "dim_sub_units_suffix",
                    string(ov::DIMMZS, &s.dimmzs),
                    dimzin & 4 != 0,
                ),
                number(
                    t!("Dim roundoff").as_ref(),
                    "dim_roundoff",
                    real(ov::DIMRND, s.dimrnd),
                    true,
                ),
                number(
                    t!("Dim scale linear").as_ref(),
                    "dim_scale_linear",
                    // The *raw* signed DIMLFAC: a negative value is the
                    // paper-space / viewport-compensation convention, and
                    // hiding its sign would make the row uneditable in
                    // practice. `s.dimlfac` is sign-resolved, so fall back to
                    // the unresolved source style instead.
                    real(ov::DIMLFAC, style.dimlfac),
                    true,
                ),
                number(
                    t!("Dim sub-units scale").as_ref(),
                    "dim_sub_units_scale",
                    real(ov::DIMMZF, s.dimmzf),
                    dimzin & 4 != 0,
                ),
                choice(
                    t!("Dim units").as_ref(),
                    "dim_units",
                    linear_unit_label(dimlunit),
                    &linear_units,
                    true,
                ),
                choice(
                    t!("Suppress leading zeros").as_ref(),
                    "dim_suppress_leading_zeros",
                    yes(dimzin & 4 != 0),
                    &["Yes", "No"],
                    true,
                ),
                choice(
                    t!("Suppress trailing zeros").as_ref(),
                    "dim_suppress_trailing_zeros",
                    yes(dimzin & 8 != 0),
                    &["Yes", "No"],
                    true,
                ),
                choice(
                    t!("Suppress zero feet").as_ref(),
                    "dim_suppress_zero_feet",
                    yes(suppresses_zero_feet(dimzin)),
                    &["Yes", "No"],
                    matches!(dimlunit, 3 | 4),
                ),
                choice(
                    t!("Suppress zero inches").as_ref(),
                    "dim_suppress_zero_inches",
                    yes(suppresses_zero_inches(dimzin)),
                    &["Yes", "No"],
                    matches!(dimlunit, 3 | 4),
                ),
                property(
                    t!("Precision").as_ref(),
                    "dim_precision",
                    PropValue::Choice {
                        selected: precision_label(int(ov::DIMDEC, s.dimdec)),
                        options: precision_options.clone(),
                    },
                ),
            ],
        },
        PropSection {
            title: t!("Alternate Units").into_owned(),
            props: vec![
                choice(
                    t!("Alt enabled").as_ref(),
                    "dim_alt_enabled",
                    on(dimalt),
                    &["On", "Off"],
                    true,
                ),
                choice(
                    t!("Alt format").as_ref(),
                    "dim_alt_format",
                    alternate_unit_label(int(ov::DIMALTU, s.dimaltu)),
                    &alternate_unit_options,
                    dimalt,
                ),
                property(
                    t!("Alt precision").as_ref(),
                    "dim_alt_precision",
                    if dimalt {
                        PropValue::Choice {
                            selected: precision_label(int(ov::DIMALTD, s.dimaltd)),
                            options: precision_options.clone(),
                        }
                    } else {
                        PropValue::ReadOnly(precision_label(int(ov::DIMALTD, s.dimaltd)))
                    },
                ),
                number(
                    t!("Alt scale factor").as_ref(),
                    "dim_alt_scale_factor",
                    real(ov::DIMALTF, s.dimaltf),
                    dimalt,
                ),
                number(
                    t!("Alt sub-units scale").as_ref(),
                    "dim_alt_sub_units_scale",
                    real(ov::DIMALTMZF, s.dimaltmzf),
                    dimalt && dimaltz & 4 != 0,
                ),
                number(
                    t!("Alt round").as_ref(),
                    "dim_alt_roundoff",
                    real(ov::DIMALTRND, s.dimaltrnd),
                    dimalt,
                ),
                text(
                    t!("Alt prefix").as_ref(),
                    "dim_alt_prefix",
                    alt_prefix,
                    dimalt,
                ),
                text(
                    t!("Alt suffix").as_ref(),
                    "dim_alt_suffix",
                    alt_suffix,
                    dimalt,
                ),
                text(
                    t!("Alt sub-units suffix").as_ref(),
                    "dim_alt_sub_units_suffix",
                    string(ov::DIMALTMZS, &s.dimaltmzs),
                    dimalt && dimaltz & 4 != 0,
                ),
                choice(
                    t!("Alt suppress leading zeros").as_ref(),
                    "dim_alt_suppress_leading_zeros",
                    yes(dimaltz & 4 != 0),
                    &["Yes", "No"],
                    dimalt,
                ),
                choice(
                    t!("Alt suppress trailing zeros").as_ref(),
                    "dim_alt_suppress_trailing_zeros",
                    yes(dimaltz & 8 != 0),
                    &["Yes", "No"],
                    dimalt,
                ),
                choice(
                    t!("Alt suppress zero feet").as_ref(),
                    "dim_alt_suppress_zero_feet",
                    yes(suppresses_zero_feet(dimaltz)),
                    &["Yes", "No"],
                    dimalt,
                ),
                choice(
                    t!("Alt suppress zero inches").as_ref(),
                    "dim_alt_suppress_zero_inches",
                    yes(suppresses_zero_inches(dimaltz)),
                    &["Yes", "No"],
                    dimalt,
                ),
            ],
        },
        PropSection {
            title: t!("Tolerances").into_owned(),
            props: vec![
                choice(
                    t!("Tolerance display").as_ref(),
                    "dim_tolerance_display",
                    tolerance_display,
                    &["None", "Symmetrical", "Deviation", "Limits", "Basic"],
                    true,
                ),
                property(
                    t!("Tolerance precision").as_ref(),
                    "dim_tolerance_precision",
                    if tolerance_enabled {
                        PropValue::Choice {
                            selected: precision_label(int(ov::DIMTDEC, s.dimtdec)),
                            options: precision_options.clone(),
                        }
                    } else {
                        PropValue::ReadOnly(precision_label(int(ov::DIMTDEC, s.dimtdec)))
                    },
                ),
                number(
                    t!("Tolerance limit lower").as_ref(),
                    "dim_tolerance_limit_lower",
                    dimtm,
                    matches!(tolerance_display, "Deviation" | "Limits"),
                ),
                number(
                    t!("Tolerance limit upper").as_ref(),
                    "dim_tolerance_limit_upper",
                    dimtp,
                    tolerance_enabled,
                ),
                number(
                    t!("Tolerance text height").as_ref(),
                    "dim_tolerance_text_height",
                    real(ov::DIMTFAC, s.dimtfac),
                    tolerance_enabled,
                ),
                choice(
                    t!("Tolerance pos vert").as_ref(),
                    "dim_tolerance_pos_vert",
                    tolerance_vertical_label(int(ov::DIMTOLJ, s.dimtolj)),
                    &["Bottom", "Middle", "Top"],
                    true,
                ),
                choice(
                    t!("Tolerance alignment").as_ref(),
                    "dim_tolerance_alignment",
                    tolerance_alignment_label(int(ov::DIMTALN, 0)),
                    &["Decimal Separator", "Operational Symbols"],
                    true,
                ),
                choice(
                    t!("Tolerance suppress leading zeros").as_ref(),
                    "dim_tolerance_suppress_leading_zeros",
                    yes(dimtzin & 4 != 0),
                    &["Yes", "No"],
                    tolerance_enabled,
                ),
                choice(
                    t!("Tolerance suppress trailing zeros").as_ref(),
                    "dim_tolerance_suppress_trailing_zeros",
                    yes(dimtzin & 8 != 0),
                    &["Yes", "No"],
                    tolerance_enabled,
                ),
                choice(
                    t!("Tolerance suppress zero feet").as_ref(),
                    "dim_tolerance_suppress_zero_feet",
                    yes(suppresses_zero_feet(dimtzin)),
                    &["Yes", "No"],
                    tolerance_enabled,
                ),
                choice(
                    t!("Tolerance suppress zero inches").as_ref(),
                    "dim_tolerance_suppress_zero_inches",
                    yes(suppresses_zero_inches(dimtzin)),
                    &["Yes", "No"],
                    tolerance_enabled,
                ),
                property(
                    t!("Alt tolerance precision").as_ref(),
                    "dim_alt_tolerance_precision",
                    if alternate_tolerance_enabled {
                        PropValue::Choice {
                            selected: precision_label(int(ov::DIMALTTD, s.dimalttd)),
                            options: precision_options,
                        }
                    } else {
                        PropValue::ReadOnly(precision_label(int(ov::DIMALTTD, s.dimalttd)))
                    },
                ),
                choice(
                    t!("Alt tolerance suppress leading zeros").as_ref(),
                    "dim_alt_tolerance_suppress_leading_zeros",
                    yes(dimalttz & 4 != 0),
                    &["Yes", "No"],
                    alternate_tolerance_enabled,
                ),
                choice(
                    t!("Alt tolerance suppress trailing zeros").as_ref(),
                    "dim_alt_tolerance_suppress_trailing_zeros",
                    yes(dimalttz & 8 != 0),
                    &["Yes", "No"],
                    alternate_tolerance_enabled,
                ),
                choice(
                    t!("Alt tolerance suppress zero feet").as_ref(),
                    "dim_alt_tolerance_suppress_zero_feet",
                    yes(suppresses_zero_feet(dimalttz)),
                    &["Yes", "No"],
                    alternate_tolerance_enabled,
                ),
                choice(
                    t!("Alt tolerance suppress zero inches").as_ref(),
                    "dim_alt_tolerance_suppress_zero_inches",
                    yes(suppresses_zero_inches(dimalttz)),
                    &["Yes", "No"],
                    alternate_tolerance_enabled,
                ),
            ],
        },
    ];
    if matches!(
        dimension,
        Dimension::Radius(_) | Dimension::Diameter(_) | Dimension::LargeRadial(_)
    ) {
        let dimcen = real(ov::DIMCEN, s.dimcen);
        let center_type = if dimcen > 1e-12 {
            "Mark"
        } else if dimcen < -1e-12 {
            "Line"
        } else {
            "None"
        };
        if let Some(lines) = sections
            .iter_mut()
            .find(|section| section.title == t!("Lines & Arrows").as_ref())
        {
            const RADIUS_LINE_FIELDS: &[&str] = &[
                "dim_arrowhead_1",
                "dim_arrow_size",
                "dim_line_lineweight",
                "dim_ext_line_lineweight",
                "dim_line_2",
                "dim_line_color",
                "dim_linetype",
                "dim_ext_linetype_1",
                "dim_ext_line_1",
                "dim_ext_line_color",
                "dim_ext_line_ext",
                "dim_ext_line_offset",
            ];
            const DIAMETER_LINE_FIELDS: &[&str] = &[
                "dim_arrowhead_1",
                "dim_arrowhead_2",
                "dim_arrow_size",
                "dim_line_lineweight",
                "dim_ext_line_lineweight",
                "dim_line_1",
                "dim_line_2",
                "dim_line_color",
                "dim_linetype",
                "dim_ext_linetype_1",
                "dim_ext_line_1",
                "dim_ext_line_color",
                "dim_ext_line_ext",
                "dim_ext_line_offset",
            ];
            const LARGE_RADIAL_LINE_FIELDS: &[&str] = &[
                "dim_arrowhead_1",
                "dim_arrow_size",
                "dim_line_lineweight",
                "dim_ext_line_lineweight",
                "dim_line_2",
                "dim_line_color",
                "dim_linetype",
                "dim_ext_linetype_1",
                "dim_ext_line_1",
                "dim_ext_line_color",
                "dim_ext_line_ext",
                "dim_ext_line_offset",
            ];
            let retained = if matches!(dimension, Dimension::Diameter(_)) {
                DIAMETER_LINE_FIELDS
            } else if matches!(dimension, Dimension::LargeRadial(_)) {
                LARGE_RADIAL_LINE_FIELDS
            } else {
                RADIUS_LINE_FIELDS
            };
            lines
                .props
                .retain(|property| retained.contains(&property.field));
            lines.props.push(choice(
                t!("Center mark").as_ref(),
                "dim_center_type",
                center_type,
                &["None", "Mark", "Line"],
                true,
            ));
            lines.props.push(number(
                t!("Center mark size").as_ref(),
                "dim_center_size",
                dimcen.abs(),
                center_type != "None",
            ));
            if let Dimension::LargeRadial(radial) = dimension {
                lines.props.push(edit(
                    t!("Center location override X").as_ref(),
                    "override_x",
                    radial.override_center.x,
                ));
                lines.props.push(edit(
                    t!("Center location override Y").as_ref(),
                    "override_y",
                    radial.override_center.y,
                ));
                lines.props.push(edit(
                    t!("Jog location X").as_ref(),
                    "jog_x",
                    radial.jog_point.x,
                ));
                lines.props.push(edit(
                    t!("Jog location Y").as_ref(),
                    "jog_y",
                    radial.jog_point.y,
                ));
                lines.props.push(edit_angle(
                    t!("Jog angle").as_ref(),
                    "jog_angle",
                    radial.jog_angle.to_degrees(),
                ));
            }
            if matches!(dimension, Dimension::Radius(_) | Dimension::LargeRadial(_)) {
                if let Some(arrow) = lines
                    .props
                    .iter_mut()
                    .find(|property| property.field == "dim_arrowhead_1")
                {
                    arrow.label = t!("Arrow").into_owned();
                    arrow.field = "dim_radial_arrow";
                }
                if let Some(dim_line) = lines
                    .props
                    .iter_mut()
                    .find(|property| property.field == "dim_line_2")
                {
                    dim_line.label = t!("Dim line").into_owned();
                }
            }
            if matches!(dimension, Dimension::LargeRadial(_)) {
                for property in &mut lines.props {
                    property.label = match property.field {
                        "dim_ext_line_1" => t!("Ext line").into_owned(),
                        "dim_ext_line_lineweight" => t!("Ext line weight").into_owned(),
                        "dim_ext_linetype_1" => t!("Ext line type").into_owned(),
                        _ => property.label.clone(),
                    };
                }
                const LARGE_RADIAL_ORDER: &[&str] = &[
                    "dim_radial_arrow",
                    "dim_arrow_size",
                    "dim_center_type",
                    "dim_center_size",
                    "dim_line_lineweight",
                    "dim_line_2",
                    "dim_linetype",
                    "dim_line_color",
                    "override_x",
                    "override_y",
                    "jog_x",
                    "jog_y",
                    "dim_ext_line_1",
                    "jog_angle",
                    "dim_ext_line_lineweight",
                    "dim_ext_linetype_1",
                    "dim_ext_line_ext",
                    "dim_ext_line_color",
                    "dim_ext_line_offset",
                ];
                lines.props.sort_by_key(|property| {
                    LARGE_RADIAL_ORDER
                        .iter()
                        .position(|field| *field == property.field)
                        .unwrap_or(LARGE_RADIAL_ORDER.len())
                });
            }
        }
        if let Some(primary_units) = sections
            .iter_mut()
            .find(|section| section.title == t!("Primary Units").as_ref())
        {
            primary_units.props.retain(|property| {
                !matches!(
                    property.field,
                    "dim_sub_units_suffix" | "dim_sub_units_scale"
                )
            });
        }
        if matches!(dimension, Dimension::LargeRadial(_)) {
            for (section_name, excluded) in [
                (t!("Text"), &["dim_text_pos_hor"][..]),
                (t!("Fit"), &["dim_line_inside"][..]),
                (
                    t!("Alternate Units"),
                    &["dim_alt_sub_units_scale", "dim_alt_sub_units_suffix"][..],
                ),
            ] {
                if let Some(section) = sections
                    .iter_mut()
                    .find(|section| section.title == section_name.as_ref())
                {
                    section
                        .props
                        .retain(|property| !excluded.contains(&property.field));
                }
            }
            for (section_name, fields) in [
                (
                    t!("Text"),
                    &["dim_text_outside_align", "dim_text_pos_vert"][..],
                ),
                (t!("Fit"), &["dim_fit", "dim_text_inside"][..]),
            ] {
                if let Some(section) = sections
                    .iter_mut()
                    .find(|section| section.title == section_name.as_ref())
                {
                    for property in &mut section.props {
                        if fields.contains(&property.field) {
                            if let PropValue::Choice { selected, .. } = &property.value {
                                property.value = PropValue::ReadOnly(selected.clone());
                            }
                        }
                    }
                }
            }
            if let Some(fit) = sections
                .iter_mut()
                .find(|section| section.title == t!("Fit").as_ref())
            {
                const LARGE_RADIAL_FIT_ORDER: &[&str] = &[
                    "dim_line_forced",
                    "dim_scale_overall",
                    "dim_fit",
                    "dim_text_inside",
                    "dim_text_movement",
                ];
                fit.props.sort_by_key(|property| {
                    LARGE_RADIAL_FIT_ORDER
                        .iter()
                        .position(|field| *field == property.field)
                        .unwrap_or(LARGE_RADIAL_FIT_ORDER.len())
                });
            }
            if let Some(alternate_units) = sections
                .iter_mut()
                .find(|section| section.title == t!("Alternate Units").as_ref())
            {
                const LARGE_RADIAL_ALT_UNITS_ORDER: &[&str] = &[
                    "dim_alt_enabled",
                    "dim_alt_format",
                    "dim_alt_precision",
                    "dim_alt_roundoff",
                    "dim_alt_scale_factor",
                    "dim_alt_suppress_leading_zeros",
                    "dim_alt_suppress_trailing_zeros",
                    "dim_alt_suppress_zero_feet",
                    "dim_alt_suppress_zero_inches",
                    "dim_alt_prefix",
                    "dim_alt_suffix",
                ];
                alternate_units.props.sort_by_key(|property| {
                    LARGE_RADIAL_ALT_UNITS_ORDER
                        .iter()
                        .position(|field| *field == property.field)
                        .unwrap_or(LARGE_RADIAL_ALT_UNITS_ORDER.len())
                });
            }
            if let Some(tolerances) = sections
                .iter_mut()
                .find(|section| section.title == t!("Tolerances").as_ref())
            {
                const LARGE_RADIAL_TOLERANCE_ORDER: &[&str] = &[
                    "dim_alt_tolerance_suppress_zero_inches",
                    "dim_tolerance_alignment",
                    "dim_tolerance_display",
                    "dim_tolerance_limit_lower",
                    "dim_tolerance_limit_upper",
                    "dim_tolerance_pos_vert",
                    "dim_tolerance_precision",
                    "dim_tolerance_suppress_leading_zeros",
                    "dim_tolerance_suppress_trailing_zeros",
                    "dim_tolerance_suppress_zero_feet",
                    "dim_tolerance_suppress_zero_inches",
                    "dim_tolerance_text_height",
                    "dim_alt_tolerance_precision",
                    "dim_alt_tolerance_suppress_leading_zeros",
                    "dim_alt_tolerance_suppress_trailing_zeros",
                    "dim_alt_tolerance_suppress_zero_feet",
                ];
                tolerances.props.sort_by_key(|property| {
                    LARGE_RADIAL_TOLERANCE_ORDER
                        .iter()
                        .position(|field| *field == property.field)
                        .unwrap_or(LARGE_RADIAL_TOLERANCE_ORDER.len())
                });
            }
            if !dimension.base().text_user_positioned {
                let text_position =
                    styled_dimension_text_position(dimension, s, 1.0, Some(document));
                for section in &mut sections {
                    for property in &mut section.props {
                        match property.field {
                            "text_x" => {
                                property.value =
                                    PropValue::EditText(format!("{:.4}", text_position.x));
                            }
                            "text_y" => {
                                property.value =
                                    PropValue::EditText(format!("{:.4}", text_position.y));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    if matches!(dimension, Dimension::Arc(_)) {
        if let Some(lines) = sections
            .iter_mut()
            .find(|section| section.title == t!("Lines & Arrows").as_ref())
        {
            let symbol = match int(ov::DIMARCSYM, s.dimarcsym) {
                1 => "Above dimension text",
                2 => "None",
                _ => "Preceding dimension text",
            };
            lines.props.insert(
                3,
                choice(
                    t!("Arc length symbol").as_ref(),
                    "dim_arc_symbol",
                    symbol,
                    &["Preceding dimension text", "Above dimension text", "None"],
                    true,
                ),
            );
        }
    }

    if matches!(
        dimension,
        Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_)
    ) {
        if let Some(text_section) = sections
            .iter_mut()
            .find(|section| section.title == t!("Text").as_ref())
        {
            text_section
                .props
                .retain(|property| property.field != "dim_fractional_type");
        }

        let angle_zero_suppression = int(ov::DIMAZIN, s.dimazin);
        let angle_unit = int(ov::DIMAUNIT, s.dimaunit);
        if let Some(primary_units) = sections
            .iter_mut()
            .find(|section| section.title == t!("Primary Units").as_ref())
        {
            primary_units.props = vec![
                text(
                    t!("Decimal separator").as_ref(),
                    "dim_decimal_separator",
                    decimal_separator,
                    true,
                ),
                text(t!("Prefix").as_ref(), "dim_prefix", dim_prefix, true),
                text(t!("Suffix").as_ref(), "dim_suffix", dim_suffix, true),
                choice(
                    t!("Angle format").as_ref(),
                    "dim_angle_units",
                    angular_unit_label(angle_unit),
                    &[
                        "Decimal degrees",
                        "Degrees/minutes/seconds",
                        "Gradians",
                        "Radians",
                    ],
                    true,
                ),
                choice(
                    t!("Suppress leading zeros").as_ref(),
                    "dim_angle_suppress_leading_zeros",
                    yes(angle_zero_suppression & 1 != 0),
                    &["Yes", "No"],
                    true,
                ),
                choice(
                    t!("Suppress trailing zeros").as_ref(),
                    "dim_angle_suppress_trailing_zeros",
                    yes(angle_zero_suppression & 2 != 0),
                    &["Yes", "No"],
                    true,
                ),
                property(
                    t!("Precision").as_ref(),
                    "dim_angle_precision",
                    PropValue::Choice {
                        selected: int(ov::DIMADEC, s.dimadec).to_string(),
                        options: (-1..=8).map(|value| value.to_string()).collect(),
                    },
                ),
            ];
        }
        sections.retain(|section| section.title != t!("Alternate Units").as_ref());
        let by_order = |order: &'static [&'static str]| {
            move |property: &Property| {
                order
                    .iter()
                    .position(|field| *field == property.field)
                    .unwrap_or(order.len())
            }
        };
        if let Some(tolerances) = sections
            .iter_mut()
            .find(|section| section.title == t!("Tolerances").as_ref())
        {
            tolerances.props.retain(|property| {
                !property.field.starts_with("dim_alt_tolerance_")
                    && !matches!(
                        property.field,
                        "dim_tolerance_suppress_zero_feet" | "dim_tolerance_suppress_zero_inches"
                    )
            });
            tolerances.props.sort_by_key(by_order(&[
                "dim_tolerance_alignment",
                "dim_tolerance_display",
                "dim_tolerance_limit_lower",
                "dim_tolerance_limit_upper",
                "dim_tolerance_pos_vert",
                "dim_tolerance_precision",
                "dim_tolerance_suppress_leading_zeros",
                "dim_tolerance_suppress_trailing_zeros",
                "dim_tolerance_text_height",
            ]));
        }
        // An arc has no dimension line extension.
        if let Some(lines) = sections
            .iter_mut()
            .find(|section| section.title == t!("Lines & Arrows").as_ref())
        {
            lines
                .props
                .retain(|property| property.field != "dim_line_ext");
        }
        if let Some(fit) = sections
            .iter_mut()
            .find(|section| section.title == t!("Fit").as_ref())
        {
            fit.props.sort_by_key(by_order(&[
                "dim_line_forced",
                "dim_line_inside",
                "dim_scale_overall",
                "dim_fit",
                "dim_text_inside",
                "dim_text_movement",
            ]));
        }
        if let Some(units) = sections
            .iter_mut()
            .find(|section| section.title == t!("Primary Units").as_ref())
        {
            units.props.sort_by_key(by_order(&[
                "dim_decimal_separator",
                "dim_prefix",
                "dim_suffix",
                "dim_angle_suppress_leading_zeros",
                "dim_angle_suppress_trailing_zeros",
                "dim_angle_precision",
                "dim_angle_units",
            ]));
        }
        // The measurement reads at the angular precision, and automatic text
        // reports where it is drawn rather than an unset point.
        if let Some(text_section) = sections
            .iter_mut()
            .find(|section| section.title == t!("Text").as_ref())
        {
            let automatic = stored_text_point(dimension)
                .is_none()
                .then(|| styled_dimension_text_position(dimension, s, 1.0, Some(document)));
            for property in &mut text_section.props {
                match property.field {
                    "measurement" => {
                        // The row shows the number at the angular precision,
                        // without the text's degree sign.
                        let value = format_angular_value(
                            displayed_measurement(dimension, Some(s)),
                            Some(s),
                        );
                        property.value =
                            PropValue::ReadOnly(value.trim_end_matches('°').to_string());
                    }
                    "text_x" => {
                        if let Some(position) = automatic {
                            property.value = PropValue::EditText(format!("{:.4}", position.x));
                        }
                    }
                    "text_y" => {
                        if let Some(position) = automatic {
                            property.value = PropValue::EditText(format!("{:.4}", position.y));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    if matches!(dimension, Dimension::Ordinate(_)) {
        for section in &mut sections {
            section
                .props
                .retain(|property| match section.title.as_str() {
                    title if title == t!("Lines & Arrows").as_ref() => matches!(
                        property.field,
                        "dim_arrow_size"
                            | "dim_ext_line_lineweight"
                            | "dim_ext_linetype_1"
                            | "dim_ext_line_1"
                            | "dim_ext_line_fixed"
                            | "dim_ext_line_fixed_length"
                            | "dim_ext_line_color"
                            | "dim_ext_line_offset"
                    ),
                    title if title == t!("Text").as_ref() => !matches!(
                        property.field,
                        "dim_text_outside_align" | "dim_text_pos_hor" | "dim_text_inside_align"
                    ),
                    title if title == t!("Fit").as_ref() => {
                        matches!(property.field, "dim_text_movement" | "dim_scale_overall")
                    }
                    _ => true,
                });
        }
        let dim_scale = if s.dimscale > 1e-6 { s.dimscale } else { 1.0 };
        let text_position = dimension_text_pos_f64(
            dimension,
            Some(s),
            real(ov::DIMTXT, s.dimtxt) * dim_scale,
            dim_scale,
            Some(document),
        );
        for section in &mut sections {
            for property in &mut section.props {
                match property.field {
                    "text_x" => {
                        property.value = PropValue::EditText(format!("{:.4}", text_position.x));
                    }
                    "text_y" => {
                        property.value = PropValue::EditText(format!("{:.4}", text_position.y));
                    }
                    _ => {}
                }
            }
        }
    }
    sections
}

pub(crate) fn dimension_style_scale(style: &DimStyle, fallback: f64) -> f64 {
    if style.dimscale > 1.0e-6 {
        style.dimscale
    } else {
        fallback
    }
}

pub(crate) fn styled_dimension_text_position(
    dimension: &Dimension,
    style: &DimStyle,
    fallback_scale: f64,
    document: Option<&Document>,
) -> Vector3 {
    let scale = dimension_style_scale(style, fallback_scale);
    dimension_text_pos_f64(
        dimension,
        Some(style),
        style.dimtxt * scale,
        scale,
        document,
    )
}

pub(crate) fn materialize_large_radial_text_position(
    document: &mut Document,
    handle: Handle,
    value: &str,
) {
    if parse_f64(value).is_none() {
        return;
    }
    let Some(EntityType::Dimension(dimension)) = document.get_entity(handle) else {
        return;
    };
    if !matches!(&**dimension, Dimension::LargeRadial(_)) || dimension.base().text_user_positioned {
        return;
    }
    let source = dimension_source_style(document, dimension);
    let position = source
        .map(|style| resolved_dimension_style(style, dimension, document))
        .map(|style| styled_dimension_text_position(dimension, &style, 1.0, Some(&*document)))
        .unwrap_or_else(|| dimension_text_pos_f64(dimension, None, 2.5, 1.0, None));
    if let Some(Dimension::LargeRadial(radial)) =
        document.get_entity_mut(handle).and_then(as_dimension_mut)
    {
        radial.base.text_middle_point = position;
    }
}

pub(crate) fn property(label: &str, field: &'static str, value: PropValue) -> Property {
    Property {
        label: label.to_string(),
        field,
        value,
    }
}

pub(crate) fn split_measurement_template(value: &str) -> (String, String) {
    value
        .split_once("<>")
        .map(|(prefix, suffix)| (prefix.to_string(), suffix.to_string()))
        .unwrap_or_else(|| (String::new(), value.to_string()))
}

pub(crate) fn fraction_type_label(value: i16) -> &'static str {
    match value {
        1 => "Diagonal",
        2 => "Not stacked",
        _ => "Horizontal",
    }
}

pub(crate) fn suppresses_zero_feet(value: i16) -> bool {
    matches!(value & 3, 0 | 3)
}

pub(crate) fn suppresses_zero_inches(value: i16) -> bool {
    matches!(value & 3, 0 | 2)
}

pub(crate) fn text_horizontal_label(value: i16) -> &'static str {
    match value {
        1 => "At extension line 1",
        2 => "At extension line 2",
        3 => "Over extension line 1",
        4 => "Over extension line 2",
        _ => "Centered",
    }
}

pub(crate) fn text_vertical_label(value: i16) -> &'static str {
    match value {
        1 => "Above",
        2 => "Outside",
        3 => "JIS",
        4 => "Below",
        _ => "Centered",
    }
}

pub(crate) fn fit_label(value: i16) -> &'static str {
    match value {
        1 => "Arrows",
        2 => "Text",
        3 => "Best fit",
        _ => "Both text and arrows",
    }
}

pub(crate) fn text_movement_label(value: i16) -> &'static str {
    match value {
        1 => "Move text, add leader",
        2 => "Move text, no leader",
        _ => "Keep dim line with text",
    }
}

pub(crate) fn alternate_unit_label(value: i16) -> &'static str {
    match value {
        1 => "Scientific",
        3 => "Engineering",
        4 => "Architectural stacked",
        5 => "Fractional stacked",
        6 => "Architectural",
        7 => "Fractional",
        8 => "Desktop",
        _ => "Decimal",
    }
}

pub(crate) fn angular_unit_label(value: i16) -> &'static str {
    match value {
        1 => "Degrees/minutes/seconds",
        2 => "Gradians",
        3 => "Radians",
        _ => "Decimal degrees",
    }
}

pub(crate) fn tolerance_vertical_label(value: i16) -> &'static str {
    match value {
        0 => "Bottom",
        2 => "Top",
        _ => "Middle",
    }
}

pub(crate) fn precision_label(value: i16) -> String {
    let decimals = value.clamp(0, 8) as usize;
    if decimals == 0 {
        "0".to_string()
    } else {
        format!("0.{}", "0".repeat(decimals))
    }
}

pub(crate) fn tolerance_alignment_label(value: i16) -> &'static str {
    if value == 0 {
        "Decimal Separator"
    } else {
        "Operational Symbols"
    }
}

pub(crate) fn choice_value(
    selected: &str,
    options: &[&str],
) -> crate::scene::model::object::PropValue {
    crate::scene::model::object::PropValue::Choice {
        selected: selected.to_string(),
        options: options.iter().map(|value| (*value).to_string()).collect(),
    }
}

/// The arrowhead picker's name for `handle`; `fallback` names a block the
/// drawing does not hold.
pub(crate) fn block_name(document: &Document, handle: document::Handle, fallback: &str) -> String {
    if !handle.is_null() && !document.block_records.iter().any(|b| b.handle == handle) {
        return crate::entities::arrowheads::label_for_block(fallback);
    }
    crate::entities::arrowheads::label(document, handle)
}

pub(crate) fn linetype_name(document: &Document, handle: document::Handle) -> String {
    document
        .line_types
        .iter()
        .find(|line_type| line_type.handle == handle)
        .map(|line_type| line_type.name.clone())
        .unwrap_or_else(|| "ByBlock".to_string())
}

pub(crate) fn linear_unit_label(value: i16) -> &'static str {
    match value {
        1 => "Scientific",
        3 => "Engineering",
        4 => "Architectural",
        5 => "Fractional",
        6 => "Desktop",
        _ => "Decimal",
    }
}
