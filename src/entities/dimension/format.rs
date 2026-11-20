// Pure measurement formatting (linear, angular, arch, eng, limits, tolerance).

use super::*;

pub(crate) fn dimension_text_value(dim: &Dimension, style: Option<&DimStyle>) -> Option<String> {
    let (main, tol) = dimension_text_parts(dim, style)?;
    // Tolerance is appended inline for callers (e.g. fill rect width) that
    // don't render a separate tolerance entity. The visual pipeline that
    // does emit a separate tolerance text re-derives the parts itself.
    match tol {
        Some(t) => Some(format!("{} {}", main, t)),
        None => Some(main),
    }
}

/// Returns (primary_text, tolerance_suffix). The tolerance is emitted as a
/// separate Text entity so DIMTFAC can scale its height and DIMTOLJ can
/// align it vertically against the primary value.
pub(crate) fn dimension_text_parts(
    dim: &Dimension,
    style: Option<&DimStyle>,
) -> Option<(String, Option<String>)> {
    let base = dim.base();
    let is_angular = matches!(dim, Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_));

    // Auto-generated body used when the user did not override it. Built first
    // so user_text "<>" substitution can re-use it. With DIMLIM the limits
    // stack is the value itself: the source application prints the upper limit over the
    // lower one and no nominal, so the tolerance slot stays empty.
    let value = match limits_text(dim, style, is_angular) {
        Some(limits) => limits,
        None if is_angular => format_angular_value(display_measurement(dim), style),
        None => format_linear_value(display_measurement(dim), style),
    };
    let primary_raw = match dim {
        Dimension::Radius(_) | Dimension::LargeRadial(_) => format!("R{}", value),
        // U+2205 is the native diameter sign and what `%%c` resolves to; the
        // bundled dimension fonts carry it where Latin Ø is missing.
        Dimension::Diameter(_) => format!("∅{}", value),
        _ => value,
    };

    // Build the deviation suffix separately so the caller can render it as
    // its own Text entity at DIMTFAC × DIMTXT height.
    let tolerance_suffix = build_tolerance_suffix(dim, style, is_angular);
    let primary = apply_dimpost(&primary_raw, style);

    // Alternate units appended in brackets when DIMALT is on (linear only).
    let primary = if !is_angular {
        match alternate_units_text(display_measurement(dim) * effective_dimlfac(style), style) {
            Some(alt) => format!("{} [{}]", primary, alt),
            None => primary,
        }
    } else {
        primary
    };

    // Explicit user override (mtext-style "user_text") wins, but "<>" inside
    // it substitutes the measured value. " " (single space) suppresses text.
    if let Some(user_text) = &base.user_text {
        if user_text.is_empty() || user_text.trim().is_empty() {
            return None;
        }
        return Some((
            split_at_line(&user_text.replace("<>", &primary)),
            tolerance_suffix,
        ));
    }
    // The stored text the same way: empty is the measurement, blank (" ")
    // hides it.
    if !base.text.is_empty() {
        if base.text.trim().is_empty() {
            return None;
        }
        return Some((
            split_at_line(&base.text.replace("<>", &primary)),
            tolerance_suffix,
        ));
    }
    Some((primary, tolerance_suffix))
}

/// The text the user typed, or the stored text when there is none.
pub(crate) fn typed_text(dim: &Dimension) -> &str {
    let base = dim.base();
    base.user_text.as_deref().unwrap_or(&base.text)
}

/// Whether the text carries `\X`, which puts what follows below the
/// dimension line.
pub(crate) fn text_splits_at_line(dim: &Dimension) -> bool {
    typed_text(dim).contains(r"\X")
}

/// `\X` laid out as a line break; the text is then centred on the line.
pub(crate) fn split_at_line(text: &str) -> String {
    text.replacen(r"\X", r"\P", 1)
}

/// The value the text reports. An angular dimension whose extension point
/// sits on its vertex has no measurable angle; the source application then keeps showing the
/// angle it stored (group 42, radians) rather than 0°, and so do we.
pub(crate) fn display_measurement(dim: &Dimension) -> f64 {
    let measured = dim.measurement();
    let stored = dim.base().actual_measurement;
    match dim {
        Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_)
            if measured.abs() < 1e-9 && stored.abs() > 1e-9 =>
        {
            stored.to_degrees()
        }
        _ => measured,
    }
}

/// DIMLIM: the upper limit stacked over the lower one, replacing the nominal.
/// The offsets apply to the displayed number, so DIMLFAC and DIMRND come first.
pub(crate) fn limits_text(
    dim: &Dimension,
    style: Option<&DimStyle>,
    is_angular: bool,
) -> Option<String> {
    // the source application stacks the two limits even when both offsets are zero and the
    // halves read the same.
    let s = style.filter(|s| s.dimlim)?;
    let measurement = display_measurement(dim);
    if is_angular {
        return Some(format!(
            "\\S{}^{};",
            format_angular_value(measurement + s.dimtp, style),
            format_angular_value(measurement - s.dimtm, style)
        ));
    }
    let mut nominal = measurement * effective_dimlfac(style);
    if s.dimrnd > 1e-12 {
        nominal = (nominal / s.dimrnd).round() * s.dimrnd;
    }
    let (high, low) = limit_pair(nominal + s.dimtp, nominal - s.dimtm, s);
    Some(format!("\\S{}^{};", high, low))
}

/// The two limits keep the same number of decimals so the stack lines up:
/// DIMTZIN drops trailing zeros only as far as both values allow. the source application
/// writes `13.950` over `13.926` with DIMTZIN 8, not `13.95` over `13.926`.
pub(crate) fn limit_pair(high: f64, low: f64, s: &DimStyle) -> (String, String) {
    let dimtdec = s.dimtdec.max(0) as usize;
    let mut high = decimal_text(high, dimtdec);
    let mut low = decimal_text(low, dimtdec);
    if s.dimtzin & 8 != 0 {
        let droppable = |value: &str| match value.find('.') {
            Some(_) => value.len() - value.trim_end_matches('0').len(),
            None => 0,
        };
        let drop = droppable(&high).min(droppable(&low));
        for value in [&mut high, &mut low] {
            value.truncate(value.len() - drop);
            if value.ends_with('.') {
                value.pop();
            }
        }
    }
    let finish = |value: &str| {
        let value = if s.dimtzin & 4 != 0 {
            strip_leading_zero(value)
        } else {
            value.to_string()
        };
        swap_decimal_sep(&value, s.dimdsep)
    };
    (finish(&high), finish(&low))
}

/// Formats a tolerance number with DIMTDEC, DIMTZIN and DIMDSEP.
pub(crate) fn tolerance_formatter(s: &DimStyle) -> impl Fn(f64) -> String + '_ {
    let dimtdec = s.dimtdec.max(0) as usize;
    move |v: f64| {
        let raw = decimal_text(v, dimtdec);
        swap_decimal_sep(&apply_linear_zero_suppression(&raw, s.dimtzin), s.dimdsep)
    }
}

/// DIMTOL deviation suffix drawn beside the nominal. DIMLIM wins over DIMTOL,
/// as in the source application, and is handled by `limits_text`.
pub(crate) fn build_tolerance_suffix(
    dim: &Dimension,
    style: Option<&DimStyle>,
    is_angular: bool,
) -> Option<String> {
    let s = style.filter(|s| !s.dimlim)?;
    let fmt = tolerance_formatter(s);
    if s.dimtol {
        let unit = if is_angular { "°" } else { "" };
        if (s.dimtp - s.dimtm).abs() < 1e-12 && s.dimtp.abs() > 1e-12 {
            return Some(format!("±{}{}", fmt(s.dimtp), unit));
        }
        if s.dimtp.abs() > 1e-12 || s.dimtm.abs() > 1e-12 {
            let mut upper = fmt(s.dimtp);
            let mut lower = fmt(s.dimtm);
            let alignment = crate::entities::dim_override::int(
                &dim.base().common.extended_data,
                crate::entities::dim_override::DIMTALN,
            )
            .unwrap_or(0);
            if alignment == 0 {
                align_tolerance_decimals(&mut upper, &mut lower, s.dimdsep);
            }
            return Some(format!("\\S+{}{}^-{}{};", upper, unit, lower, unit));
        }
    }
    None
}

pub(crate) fn align_tolerance_decimals(upper: &mut String, lower: &mut String, separator: i16) {
    let separator = separator as u8 as char;
    let integer_width = |value: &str| {
        value
            .find(separator)
            .or_else(|| value.find('.'))
            .unwrap_or(value.len())
    };
    let upper_width = integer_width(upper);
    let lower_width = integer_width(lower);
    let target = upper_width.max(lower_width);
    if upper_width < target {
        upper.insert_str(0, &" ".repeat(target - upper_width));
    }
    if lower_width < target {
        lower.insert_str(0, &" ".repeat(target - lower_width));
    }
}

/// Build the bracketed alternate-units suffix when DIMALT is enabled.
/// When DIMTOL is also on, the bracketed text includes the tolerance
/// component formatted with DIMALTTD / DIMALTTZ.
pub(crate) fn alternate_units_text(measurement: f64, style: Option<&DimStyle>) -> Option<String> {
    let s = style?;
    if !s.dimalt {
        return None;
    }
    let scaled = measurement * s.dimaltf;
    let use_sub_units = s.dimaltz & 4 != 0 && scaled.abs() < 1.0 && s.dimaltmzf.abs() > 1e-12;
    // DIMALTMZF replaces the ordinary alternate-unit factor for sub-unit
    // values; it is not an additional multiplier.
    let mut v = if use_sub_units {
        measurement * s.dimaltmzf
    } else {
        scaled
    };
    if s.dimaltrnd > 1e-12 {
        v = (v / s.dimaltrnd).round() * s.dimaltrnd;
    }
    let dec = s.dimaltd.max(0) as usize;
    let raw = format_with_unit(v, s.dimaltu, dec, s.dimfrac, s.dimaltz, true);
    let suppressed = apply_linear_zero_suppression(&raw, s.dimaltz);
    let mut sep_swapped = swap_decimal_sep(&suppressed, s.dimdsep);
    if use_sub_units {
        sep_swapped.push_str(&s.dimaltmzs);
    }
    let tolerance_factor = if use_sub_units {
        s.dimaltmzf
    } else {
        s.dimaltf
    };
    // Alt-unit tolerance suffix using DIMALTTD / DIMALTTZ.
    let alt_value = if s.dimtol {
        let alttdec = s.dimalttd.max(0) as usize;
        let alttzin = s.dimalttz;
        let fmt = |x: f64| -> String {
            let raw = decimal_text(x * tolerance_factor, alttdec);
            swap_decimal_sep(&apply_linear_zero_suppression(&raw, alttzin), s.dimdsep)
        };
        if (s.dimtp - s.dimtm).abs() < 1e-12 && s.dimtp.abs() > 1e-12 {
            format!("{}±{}", sep_swapped, fmt(s.dimtp))
        } else if s.dimtp.abs() > 1e-12 || s.dimtm.abs() > 1e-12 {
            format!("{} +{} / -{}", sep_swapped, fmt(s.dimtp), fmt(s.dimtm))
        } else {
            sep_swapped
        }
    } else if s.dimlim {
        let alttdec = s.dimalttd.max(0) as usize;
        let alttzin = s.dimalttz;
        let fmt = |x: f64| -> String {
            let raw = decimal_text(x * tolerance_factor, alttdec);
            swap_decimal_sep(&apply_linear_zero_suppression(&raw, alttzin), s.dimdsep)
        };
        // Alternate limits stack like the primary ones.
        format!(
            "\\S{}^{};",
            fmt(measurement + s.dimtp),
            fmt(measurement - s.dimtm)
        )
    } else {
        sep_swapped
    };
    // DIMAPOST wraps the alt value (same "<>" convention as DIMPOST).
    let wrapped = if s.dimapost.is_empty() {
        alt_value
    } else if s.dimapost.contains("<>") {
        s.dimapost.replace("<>", &alt_value)
    } else {
        format!("{}{}", alt_value, s.dimapost)
    };
    Some(wrapped)
}

/// Build the secondary tolerance Text entity at `DIMTXT × DIMTFAC` height,
/// positioned to the right of the primary text and vertically aligned per
/// `DIMTOLJ` (0=bottom, 1=middle, 2=top). Returns None when DIMTOL/DIMLIM
/// produce no tolerance string (e.g. both DIMTP and DIMTM are zero).
pub(crate) fn dimension_tolerance_entity(
    dim: &Dimension,
    style: Option<&DimStyle>,
    primary: &EntityType,
    primary_height: f64,
) -> Option<EntityType> {
    let s = style?;
    let is_angular = matches!(dim, Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_));
    let tol = build_tolerance_suffix(dim, style, is_angular)?;
    let tol_height = primary_height * dimtfac_or_one(s);

    // Pull the geometry we need from the synthetic primary entity (Text or
    // MText — `dimension_text_entity` routes to MText when the dim value
    // carries inline format codes).
    let (primary_value_len, primary_insertion, primary_rotation, primary_style, primary_common) =
        match primary {
            EntityType::Text(t) => (
                t.value.chars().count(),
                t.insertion_point,
                t.rotation,
                t.style.clone(),
                t.common.clone(),
            ),
            EntityType::MText(m) => (
                m.value.chars().count(),
                m.insertion_point,
                m.rotation,
                m.style.clone(),
                m.common.clone(),
            ),
            _ => return None,
        };

    // Approximate widths from glyph counts (~0.6 × cell size per char).
    let primary_w = primary_value_len as f64 * primary_height * CELL_WIDTH;
    let tol_visible_chars = tol
        .strip_prefix("\\S")
        .and_then(|value| value.strip_suffix(';'))
        .map(|value| value.split('^').map(str::len).max().unwrap_or(0))
        .unwrap_or_else(|| tol.chars().count());
    let tol_w = tol_visible_chars as f64 * tol_height * CELL_WIDTH;
    let gap = primary_height * 0.2;
    let dx_local = primary_w * 0.5 + tol_w * 0.5 + gap;
    let dy_local = match s.dimtolj {
        0 => -primary_height * 0.5 + tol_height * 0.5, // bottom-aligned with primary baseline
        2 => primary_height * 0.5 - tol_height * 0.5,  // top-aligned with primary top
        _ => 0.0,                                      // centred (default for ±)
    };
    let rot = primary_rotation;
    let (sr, cr) = rot.sin_cos();
    let pos = Vector3::new(
        primary_insertion.x + dx_local * cr - dy_local * sr,
        primary_insertion.y + dx_local * sr + dy_local * cr,
        primary_insertion.z,
    );
    if value_has_mtext_codes(&tol) {
        let mut mtext = MText::with_value(tol, pos);
        mtext.height = tol_height;
        mtext.rotation = rot;
        mtext.style = primary_style;
        mtext.attachment_point = document::entities::AttachmentPoint::MiddleCenter;
        mtext.common = primary_common;
        return Some(EntityType::MText(Box::new(mtext)));
    }

    let mut t = Text::with_value(tol, pos)
        .with_height(tol_height)
        .with_rotation(rot);
    t.style = primary_style;
    t.common = primary_common;
    t.horizontal_alignment = document::entities::text::TextHorizontalAlignment::Center;
    t.vertical_alignment = document::entities::text::TextVerticalAlignment::Middle;
    Some(EntityType::Text(t))
}

/// Wrap a measured value with the style's DIMPOST prefix/suffix template.
/// "<>" inside DIMPOST is replaced by the value; absent "<>" appends.
pub(crate) fn apply_dimpost(value: &str, style: Option<&DimStyle>) -> String {
    let post = style.map(|s| s.dimpost.as_str()).unwrap_or("");
    if post.is_empty() {
        return value.to_string();
    }
    if post.contains("<>") {
        post.replace("<>", value)
    } else {
        format!("{}{}", value, post)
    }
}

/// `resolved_dimension_style` has already applied the DXF sign convention,
/// so a negative value here means the caller passed an unresolved style;
/// fall back to 1.0 rather than reporting a negative length.
pub(crate) fn effective_dimlfac(style: Option<&DimStyle>) -> f64 {
    let lfac = style.map(|s| s.dimlfac).unwrap_or(1.0);
    if lfac.abs() < 1e-12 || lfac < 0.0 {
        1.0
    } else {
        lfac
    }
}

/// Average advance of one character cell as a fraction of the text height.
/// Measured on the bundled stroke fonts by rendering the text of 106 saved
/// dimension pictures: median 0.66, where the old guess of 0.60 left every
/// fit and hook estimate ten percent narrow.
pub(crate) const CELL_WIDTH: f64 = 0.66;

/// DIMTFAC with the unset value 0 read as 1.
pub(crate) fn dimtfac_or_one(s: &DimStyle) -> f64 {
    if s.dimtfac.abs() < 1e-12 {
        1.0
    } else {
        s.dimtfac
    }
}

/// Vertical extent of a dimension's whole value: the first line (or limits
/// stack) plus any `\P` continuation lines at MText's default spacing.
pub(crate) fn dimension_text_extent_height(
    dim: &Dimension,
    style: Option<&DimStyle>,
    text_height: f64,
) -> f64 {
    let lines = dimension_text_value(dim, style)
        .map(|value| text_lines(&value))
        .unwrap_or(1);
    dimension_text_block_height(style, text_height) + (lines - 1) as f64 * text_height * 5.0 / 3.0
}

/// Vertical extent of the rendered value: one line, or a DIMLIM stack whose
/// halves each sit at DIMTFAC × DIMTXT.
pub(crate) fn dimension_text_block_height(style: Option<&DimStyle>, text_height: f64) -> f64 {
    match style {
        Some(s) if s.dimlim => {
            // From the lower half's baseline to the upper half's cap, as the
            // MText stack lays them out.
            text_height
                * dimtfac_or_one(s)
                * f64::from(
                    crate::entities::text_support::STACK_RAISE
                        + crate::entities::text_support::STACK_HALF_SCALE,
                )
        }
        _ => text_height,
    }
}

/// Approximate width of the rendered value in character cells of the nominal
/// text height. A `\S` stack is as wide as its wider half, drawn at DIMTFAC.
pub(crate) fn dimension_text_cells(dim: &Dimension, style: Option<&DimStyle>) -> Option<f64> {
    let value = dimension_text_value(dim, style)?;
    Some(text_cells(&value, style.map(dimtfac_or_one).unwrap_or(1.0)))
}

/// Width in cells of the widest line; `\P` starts a new line.
pub(crate) fn text_cells(value: &str, stack_scale: f64) -> f64 {
    value
        .split(r"\P")
        .map(|line| line_cells(line, stack_scale))
        .fold(0.0, f64::max)
}

/// Number of lines the value renders as.
pub(crate) fn text_lines(value: &str) -> usize {
    value.split(r"\P").count().max(1)
}

/// Visible cells of one line: MText format codes (`\\f...;`, `\\H...;`, `\\C...;`,
/// braces, underline toggles) take no room, a stack is as wide as its wider half.
pub(crate) fn line_cells(value: &str, stack_scale: f64) -> f64 {
    let mut cells = 0.0;
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' => {}
            '\\' => match chars.next() {
                Some('S') => {
                    let body: String = chars.by_ref().take_while(|c| *c != ';').collect();
                    let widest = body
                        .split(['^', '/', '#'])
                        .map(|half| half.chars().count())
                        .max()
                        .unwrap_or(0);
                    cells += widest as f64 * stack_scale;
                }
                Some('~') => cells += 1.0,
                // Underline, overline and strike toggles take no room; an
                // escaped backslash or brace is one visible character.
                Some('L' | 'l' | 'O' | 'o' | 'K' | 'k') => {}
                Some('\\' | '{' | '}') => cells += 1.0,
                Some(_) => {
                    for c in chars.by_ref() {
                        if c == ';' {
                            break;
                        }
                    }
                }
                None => {}
            },
            _ => cells += 1.0,
        }
    }
    cells
}

/// Format a linear measurement honouring DIMLFAC, DIMRND, DIMDEC, DIMZIN, DIMDSEP, DIMLUNIT.
pub(crate) fn format_linear_value(measurement: f64, style: Option<&DimStyle>) -> String {
    let (dec, zin, rnd, dsep, lunit, frac, sub_factor, sub_suffix) = style
        .map(|s| {
            (
                s.dimdec,
                s.dimzin,
                s.dimrnd,
                s.dimdsep,
                s.dimlunit,
                s.dimfrac,
                s.dimmzf,
                s.dimmzs.as_str(),
            )
        })
        .unwrap_or((4, 8, 0.0, 46, 2, 0, 1.0, ""));

    let lfac = effective_dimlfac(style);
    let scaled = measurement * lfac;
    let use_sub_units = zin & 4 != 0 && scaled.abs() < 1.0 && sub_factor.abs() > 1e-12;
    // For values below one unit, DIMMZF replaces DIMLFAC; applying it after
    // DIMLFAC multiplies both factors and reports the wrong sub-unit value.
    let mut v = if use_sub_units {
        measurement * sub_factor
    } else {
        scaled
    };
    if rnd > 1e-12 {
        v = (v / rnd).round() * rnd;
    }
    let dec = dec.max(0) as usize;
    let raw = format_with_unit(v, lunit, dec, frac, zin, false);
    let suppressed = apply_linear_zero_suppression(&raw, zin);
    let mut formatted = swap_decimal_sep(&suppressed, dsep);
    if use_sub_units {
        formatted.push_str(sub_suffix);
    }
    formatted
}

/// Dispatch on DIMLUNIT / DIMALTU.
///   1 = Scientific
///   2 = Decimal (default)
///   3 = Engineering   (feet + decimal inches; 1 unit = 1 inch)
///   4 = Architectural (feet + fractional inches)
///   5 = Fractional    (integer + fractional inches)
/// For alternate units, 4/5 are stacked and 6/7 are unstacked architectural
/// and fractional forms. For primary units DIMFRAC selects the stack form.
pub(crate) fn format_with_unit(
    value: f64,
    unit: i16,
    dec: usize,
    dimfrac: i16,
    zin: i16,
    alternate: bool,
) -> String {
    match unit {
        1 => format!("{:.*e}", dec, value),
        3 => format_engineering(value, dec),
        4 => format_architectural(value, dec, if alternate { 0 } else { dimfrac }, zin),
        5 => format_fractional(value, dec, if alternate { 0 } else { dimfrac }),
        6 if alternate => format_architectural(value, dec, 2, zin),
        7 if alternate => format_fractional(value, dec, 2),
        _ => decimal_text(value, dec),
    }
}

/// Fixed-point text rounded half away from zero. A few scaled ULPs settle
/// decimal ties without changing values at higher requested precision.
pub(crate) fn decimal_text(value: f64, decimals: usize) -> String {
    let scale = 10f64.powi(decimals.min(15) as i32);
    let scaled = value * scale;
    let tolerance = (scaled.abs().max(1.0) * f64::EPSILON * 4.0).min(0.25);
    format!(
        "{:.*}",
        decimals,
        (scaled + scaled.signum() * tolerance).round() / scale
    )
}

pub(crate) fn format_engineering(inches: f64, dec: usize) -> String {
    // Round to the printed precision *first*, then split, so inches that round
    // up to a full foot carry into the feet instead of printing an
    // out-of-range "2'-12.00"". Same reasoning as `format_architectural`,
    // where the grid is the fraction denominator rather than a decimal place.
    let scale = 10f64.powi(dec.min(15) as i32);
    let ticks = (inches.abs() * scale).round();
    let per_foot = 12.0 * scale;
    let sign = if inches < 0.0 && ticks != 0.0 {
        "-"
    } else {
        ""
    };
    let feet = (ticks / per_foot).trunc();
    let rem_in = (ticks - feet * per_foot) / scale;
    format!("{}{:.0}'-{:.*}\"", sign, feet, dec, rem_in)
}

pub(crate) fn format_architectural(
    inches: f64,
    precision: usize,
    dimfrac: i16,
    zin: i16,
) -> String {
    let denom = fractional_denominator(precision);
    // Round to the nearest 1/denom inch *first*, in integer ticks, so a fraction
    // that rounds up to a whole inch carries into the inches — and on into the
    // feet — instead of printing an un-reduced "11 1/1". A 3ft object that
    // measures 35.9999" (float) now reads 3'-0", not 2'-11 1".
    let ticks = (inches.abs() * denom as f64).round() as u64;
    let sign = if inches < 0.0 && ticks != 0 { "-" } else { "" };
    let per_foot = 12 * denom;
    let feet = ticks / per_foot;
    let rem = ticks % per_foot;
    let whole = rem / denom;
    let frac_str = format_fraction_component(&reduce_fraction(rem % denom, denom), dimfrac);

    // DIMZIN feet/inch suppression:
    //   0 suppress zero feet & zero inches, 1 include both,
    //   2 include zero feet / suppress zero inches,
    //   3 suppress zero feet / include zero inches.
    let suppress_zero_feet = zin == 0 || zin == 3;
    let suppress_zero_inches = zin == 0 || zin == 2;
    let feet_zero = feet == 0;
    let inches_zero = whole == 0 && frac_str.is_empty();
    let show_feet = !feet_zero || !suppress_zero_feet;
    let show_inches = !inches_zero || !suppress_zero_inches;

    let feet_part = if show_feet {
        format!("{}'", feet)
    } else {
        String::new()
    };
    let inch_part = if show_inches {
        if frac_str.is_empty() {
            format!("{}\"", whole)
        } else {
            format!("{} {}\"", whole, frac_str)
        }
    } else {
        String::new()
    };
    let body = match (feet_part.is_empty(), inch_part.is_empty()) {
        (false, false) => format!("{}-{}", feet_part, inch_part),
        (false, true) => feet_part,
        (true, false) => inch_part,
        (true, true) => "0\"".to_string(),
    };
    format!("{}{}", sign, body)
}

pub(crate) fn format_fractional(value: f64, precision: usize, dimfrac: i16) -> String {
    let denom = fractional_denominator(precision);
    // Round on the fraction grid first (same carry reasoning as architectural).
    let ticks = (value.abs() * denom as f64).round() as u64;
    let sign = if value < 0.0 && ticks != 0 { "-" } else { "" };
    let whole = ticks / denom;
    let frac_str = format_fraction_component(&reduce_fraction(ticks % denom, denom), dimfrac);
    if frac_str.is_empty() {
        format!("{}{}", sign, whole)
    } else if whole == 0 {
        format!("{}{}", sign, frac_str)
    } else {
        format!("{}{} {}", sign, whole, frac_str)
    }
}

/// Fraction precision maps 0..8 to whole units through 1/256. DIMFRAC controls
/// only the visual stack form; it must not change the numeric denominator.
pub(crate) fn fractional_denominator(precision: usize) -> u64 {
    1u64 << precision.min(8)
}

pub(crate) fn format_fraction_component(value: &str, dimfrac: i16) -> String {
    if value.is_empty() || dimfrac == 2 {
        return value.to_string();
    }
    let separator = if dimfrac == 1 { '#' } else { '/' };
    format!("\\S{};", value.replacen('/', &separator.to_string(), 1))
}

/// Reduce a power-of-two fraction numer/denom to display form. Empty for a zero
/// numerator. Callers strip the whole units first, so `numer < denom` and the
/// reduced denominator is always ≥ 2 — the carry that used to leak out as a bare
/// "1/1" is handled upstream now.
pub(crate) fn reduce_fraction(mut n: u64, mut d: u64) -> String {
    if n == 0 {
        return String::new();
    }
    while n.is_multiple_of(2) && d.is_multiple_of(2) {
        n /= 2;
        d /= 2;
    }
    if d == 1 {
        format!("{}", n)
    } else {
        format!("{}/{}", n, d)
    }
}

/// Format an angular measurement in degrees using the angular style settings.
pub(crate) fn format_angular_value(measurement_deg: f64, style: Option<&DimStyle>) -> String {
    let (aunit, adec, azin, decimal_separator) = style
        .map(|s| {
            let precision = if s.dimadec < 0 { s.dimdec } else { s.dimadec };
            (s.dimaunit, precision, s.dimazin, s.dimdsep)
        })
        .unwrap_or((0, 2, 0, b'.' as i16));
    let adec = adec.clamp(0, 8) as usize;

    match aunit {
        // 1 = Degrees / Minutes / Seconds
        1 => format_dms(measurement_deg, adec, azin, decimal_separator),
        // 2 = Gradians
        2 => {
            let g = measurement_deg / 0.9;
            let raw = format!("{:.*}", adec, g);
            format!(
                "{}g",
                swap_decimal_sep(
                    &apply_angular_zero_suppression(&raw, azin),
                    decimal_separator
                )
            )
        }
        // 3 = Radians
        3 => {
            let r = measurement_deg.to_radians();
            let raw = format!("{:.*}", adec, r);
            format!(
                "{}r",
                swap_decimal_sep(
                    &apply_angular_zero_suppression(&raw, azin),
                    decimal_separator
                )
            )
        }
        // 4 = Surveyor's units
        4 => format_surveyor(measurement_deg, adec, azin, decimal_separator),
        // 0 or unknown = Decimal Degrees
        _ => {
            let raw = format!("{:.*}", adec, measurement_deg);
            format!(
                "{}°",
                swap_decimal_sep(
                    &apply_angular_zero_suppression(&raw, azin),
                    decimal_separator
                )
            )
        }
    }
}

/// Surveyor's units (DIMAUNIT 4): a bearing away from north or south toward
/// east or west, so the quoted angle never exceeds a quarter turn. Due north,
/// south, east and west have no bearing to quote and are written as the single
/// letter, which is also what keeps a quarter turn from reading as a
/// contradictory `N 90d0'0" E`. The bearing is DMS, so it carries DIMADEC,
/// DIMAZIN and DIMDSEP the same way the DMS format does.
pub(crate) fn format_surveyor(
    deg: f64,
    sec_dec: usize,
    azin: i16,
    decimal_separator: i16,
) -> String {
    // Quantise before choosing a cardinal or a quadrant, so a value a hair
    // under a cardinal direction does not print a quarter-turn bearing.
    let scale = 3600.0 * 10f64.powi(sec_dec.min(8) as i32);
    let turn = 360.0 * scale;
    let ticks = (deg.rem_euclid(360.0) * scale).round().rem_euclid(turn);
    let at = |d: f64| (ticks - d * scale).abs() < 0.5;
    if at(0.0) {
        return "E".into();
    }
    if at(90.0) {
        return "N".into();
    }
    if at(180.0) {
        return "W".into();
    }
    if at(270.0) {
        return "S".into();
    }
    let d = ticks / scale;
    // Measured from the nearer pole, toward the side the angle falls on.
    let (pole, bearing, side) = if d < 90.0 {
        ("N", 90.0 - d, "E")
    } else if d < 180.0 {
        ("N", d - 90.0, "W")
    } else if d < 270.0 {
        ("S", 270.0 - d, "W")
    } else {
        ("S", d - 270.0, "E")
    };
    format!(
        "{pole} {} {side}",
        format_dms(bearing, sec_dec, azin, decimal_separator)
    )
}

pub(crate) fn format_dms(deg: f64, sec_dec: usize, azin: i16, decimal_separator: i16) -> String {
    let sign = if deg < 0.0 { "-" } else { "" };
    let scale = 10_u64.pow(sec_dec.min(8) as u32) as f64;
    let total_ticks = (deg.abs() * 3600.0 * scale).round();
    let d = (total_ticks / (3600.0 * scale)).floor();
    let remaining = total_ticks - d * 3600.0 * scale;
    let m = (remaining / (60.0 * scale)).floor();
    let s = (remaining - m * 60.0 * scale) / scale;
    let s_str = format!("{:.*}", sec_dec, s);
    let s_str = swap_decimal_sep(
        &apply_angular_zero_suppression(&s_str, azin),
        decimal_separator,
    );
    format!("{}{:.0}°{:.0}'{}\"", sign, d, m, s_str)
}

/// Apply DIMZIN bit flags to a formatted linear value.
///  bit 0 (1)  suppress 0' (imperial feet)        — not applicable for decimal
///  bit 1 (2)  suppress 0" (imperial inches)      — not applicable for decimal
///  bit 2 (4)  suppress leading zeros             (e.g. ".5" not "0.5")
///  bit 3 (8)  suppress trailing zeros            (e.g. "1.5" not "1.50")
/// Default = 8 (trailing-zero suppression on).
pub(crate) fn apply_linear_zero_suppression(s: &str, zin: i16) -> String {
    let mut out = s.to_string();
    if zin & 8 != 0 {
        out = strip_trailing_zeros(&out);
    }
    if zin & 4 != 0 {
        out = strip_leading_zero(&out);
    }
    out
}

pub(crate) fn apply_angular_zero_suppression(s: &str, azin: i16) -> String {
    // DIMAZIN: 0=neither, 1=leading, 2=trailing, 3=both.
    let mut out = s.to_string();
    if azin & 2 != 0 {
        out = strip_trailing_zeros(&out);
    }
    if azin & 1 != 0 {
        out = strip_leading_zero(&out);
    }
    out
}

pub(crate) fn strip_trailing_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    // Engineering units end in the inch mark: the inches lose their zeros.
    if let Some(number) = s.strip_suffix('"') {
        return format!("{}\"", strip_trailing_zeros(number));
    }
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn strip_leading_zero(s: &str) -> String {
    // "0.5" → ".5",  "-0.5" → "-.5",  "0" stays.
    if let Some(rest) = s.strip_prefix("-0.") {
        return format!("-.{rest}");
    }
    if let Some(rest) = s.strip_prefix("0.") {
        return format!(".{rest}");
    }
    s.to_string()
}

pub(crate) fn swap_decimal_sep(s: &str, dsep_code: i16) -> String {
    // DIMDSEP holds an ASCII code (0 means default '.'). 46='.', 44=',', etc.
    if dsep_code <= 0 || dsep_code == 46 {
        return s.to_string();
    }
    let ch = char::from_u32(dsep_code as u32).unwrap_or('.');
    s.replace('.', &ch.to_string())
}
