//! The dimension style variables: one row per variable with its value kind,
//! factory default and a short description. Shared by the `-DIMSTYLE`
//! listing and write forms and by the style comparison so every surface
//! prints the same columns.

use document::tables::DimStyle;

/// How the variable validates on write.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Any finite float.
    Float,
    /// Float > 0.
    Positive,
    /// Float >= 0.
    NonNegative,
    /// Float != 0.
    NonZero,
    /// Integer in `[min, max]`.
    Int { min: i16, max: i16 },
    /// Colour index: 0 = ByBlock, 256 = ByLayer.
    Aci,
    /// Line weight in 1/100 mm: -3 = Default, -2 = ByBlock, -1 = ByLayer.
    LineWeight,
    /// On/off boolean.
    Bool,
    /// Free-form text.
    Str,
    /// Arrow block by name; an empty name clears the override.
    BlockName,
    /// Text style by name; must exist in the drawing.
    TextStyleName,
}

pub(crate) struct DimVariable {
    pub(crate) name: &'static str,
    pub(crate) kind: Kind,
    pub(crate) description: &'static str,
}

const fn v(name: &'static str, kind: Kind, description: &'static str) -> DimVariable {
    DimVariable {
        name,
        kind,
        description,
    }
}

/// Every scalar dimension variable the style table holds.
pub(crate) const DIM_VARIABLES: &[DimVariable] = &[
    // Dimension lines
    v("DIMCLRD", Kind::Aci, "Dimension line colour."),
    v("DIMDLE", Kind::Float, "Dimension line extension beyond the extension lines."),
    v("DIMDLI", Kind::NonNegative, "Dimension line spacing for continue and baseline."),
    v("DIMGAP", Kind::Float, "Gap between line and text; negative suppresses the line."),
    v("DIMLWD", Kind::LineWeight, "Dimension line weight."),
    v("DIMSD1", Kind::Bool, "Suppress the first dimension line."),
    v("DIMSD2", Kind::Bool, "Suppress the second dimension line."),
    // Extension lines
    v("DIMCLRE", Kind::Aci, "Extension line colour."),
    v("DIMEXE", Kind::NonNegative, "Extension line extension."),
    v("DIMEXO", Kind::NonNegative, "Extension line offset from origin."),
    v("DIMLWE", Kind::LineWeight, "Extension line weight."),
    v("DIMSE1", Kind::Bool, "Suppress the first extension line."),
    v("DIMSE2", Kind::Bool, "Suppress the second extension line."),
    v("DIMFXL", Kind::NonNegative, "Fixed extension line length."),
    v("DIMFXLON", Kind::Bool, "Use a fixed extension line length."),
    // Arrowheads
    v("DIMASZ", Kind::Positive, "Arrowhead size."),
    v("DIMBLK", Kind::BlockName, "Arrowhead block; empty uses arrowheads."),
    v("DIMBLK1", Kind::BlockName, "First arrowhead block."),
    v("DIMBLK2", Kind::BlockName, "Second arrowhead block."),
    v("DIMSAH", Kind::Bool, "Separate arrowheads for the two endpoints."),
    v("DIMCEN", Kind::Float, "Centre mark size; 0 none, negative centrelines."),
    v("DIMTSZ", Kind::Float, "Oblique tick size; 0 uses arrowheads."),
    // Text
    v("DIMCLRT", Kind::Aci, "Dimension text colour."),
    v("DIMTXT", Kind::Positive, "Dimension text height."),
    v("DIMJUST", Kind::Int { min: 0, max: 6 }, "Horizontal justification."),
    v("DIMTAD", Kind::Int { min: 0, max: 4 }, "Vertical text position."),
    v("DIMTVP", Kind::Float, "Text vertical position as a fraction of height."),
    v("DIMTIH", Kind::Bool, "Text inside extension lines is horizontal."),
    v("DIMTOH", Kind::Bool, "Text outside extension lines is horizontal."),
    v("DIMTIX", Kind::Bool, "Force text inside the extension lines."),
    v("DIMSOXD", Kind::Bool, "Suppress the outside extension lines."),
    v("DIMTFAC", Kind::Positive, "Tolerance text height factor."),
    v("DIMTFILL", Kind::Int { min: 0, max: 2 }, "Text background fill mode."),
    v("DIMTFILLCLR", Kind::Aci, "Text background fill colour."),
    v("DIMTMOVE", Kind::Int { min: 0, max: 2 }, "Text movement mode."),
    v("DIMTXTDIRECTION", Kind::Bool, "Right-to-left text direction."),
    v("DIMTXSTY", Kind::TextStyleName, "Dimension text style."),
    // Scale and units
    v("DIMSCALE", Kind::Positive, "Overall scale factor for the style."),
    v("DIMLFAC", Kind::NonZero, "Linear measurement scale factor."),
    v("DIMLUNIT", Kind::Int { min: 1, max: 6 }, "Primary units format."),
    v("DIMDEC", Kind::Int { min: 0, max: 8 }, "Primary units decimal places."),
    v("DIMRND", Kind::Float, "Rounding value for primary units."),
    v("DIMDSEP", Kind::Int { min: 0, max: 127 }, "Decimal separator character code."),
    v("DIMZIN", Kind::Int { min: 0, max: 15 }, "Primary units zero suppression."),
    // Angular
    v("DIMAUNIT", Kind::Int { min: 0, max: 4 }, "Angular units format."),
    v("DIMADEC", Kind::Int { min: 0, max: 8 }, "Angular decimal places."),
    v("DIMAZIN", Kind::Int { min: 0, max: 3 }, "Angular zero suppression."),
    // Alternate units
    v("DIMALT", Kind::Bool, "Enable alternate units."),
    v("DIMALTF", Kind::NonZero, "Alternate unit multiplier."),
    v("DIMALTD", Kind::Int { min: 0, max: 8 }, "Alternate unit decimal places."),
    v("DIMALTU", Kind::Int { min: 1, max: 6 }, "Alternate units format."),
    v("DIMALTTD", Kind::Int { min: 0, max: 8 }, "Alternate tolerance decimal places."),
    v("DIMALTRND", Kind::Float, "Alternate unit rounding value."),
    v("DIMAPOST", Kind::Str, "Alternate units suffix."),
    v("DIMALTZ", Kind::Int { min: 0, max: 15 }, "Alternate units zero suppression."),
    v("DIMALTTZ", Kind::Int { min: 0, max: 15 }, "Alternate tolerance zero suppression."),
    v("DIMALTMZF", Kind::Float, "Alternate measurement zero scale factor."),
    v("DIMALTMZS", Kind::Str, "Alternate measurement zero suffix."),
    v("DIMMZF", Kind::Float, "Measurement zero scale factor."),
    v("DIMMZS", Kind::Str, "Measurement zero suffix."),
    // Tolerances
    v("DIMTOL", Kind::Bool, "Show tolerance values."),
    v("DIMLIM", Kind::Bool, "Show limits as the measured dimension."),
    v("DIMTP", Kind::Float, "Plus tolerance value."),
    v("DIMTM", Kind::Float, "Minus tolerance value."),
    v("DIMTDEC", Kind::Int { min: 0, max: 8 }, "Tolerance decimal places."),
    v("DIMTFAC", Kind::Positive, "Tolerance text height factor."),
    v("DIMTOLJ", Kind::Int { min: 0, max: 2 }, "Tolerance vertical justification."),
    v("DIMTZIN", Kind::Int { min: 0, max: 15 }, "Tolerance zero suppression."),
    // Fit
    v("DIMATFIT", Kind::Int { min: 0, max: 3 }, "Fit order when text and arrows do not fit."),
    v("DIMTOFL", Kind::Bool, "Draw the dimension line when text and arrows are outside."),
    v("DIMUPT", Kind::Bool, "Cursor position controls the text position."),
    v("DIMFIT", Kind::Int { min: 0, max: 3 }, "Legacy fit control (obsolete)."),
    // Formatting
    v("DIMPOST", Kind::Str, "Measurement prefix and suffix string."),
    v("DIMFRAC", Kind::Int { min: 0, max: 2 }, "Fraction format."),
    v("DIMARCSYM", Kind::Int { min: 0, max: 2 }, "Arc length symbol position."),
    v("DIMJOGANG", Kind::Float, "Jog angle for radius dimensions, radians."),
    v("DIMUNIT", Kind::Int { min: 1, max: 6 }, "Legacy primary units format (obsolete)."),
];

/// Format a float for display: trims trailing zeros.
fn fmt_float(value: f64) -> String {
    let trimmed = format!("{value:.4}");
    trimmed
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

/// Format a colour value as the command line accepts it.
fn fmt_color(value: i16) -> String {
    match value {
        0 => "ByBlock".to_string(),
        256 => "ByLayer".to_string(),
        other => other.to_string(),
    }
}

/// Format a line weight value as the command line accepts it.
fn fmt_line_weight(value: i16) -> String {
    match value {
        -3 => "Default".to_string(),
        -2 => "ByBlock".to_string(),
        -1 => "ByLayer".to_string(),
        other => format!("{:.2} mm", f64::from(other) / 100.0),
    }
}

/// Format a boolean for display.
fn fmt_bool(value: bool) -> String {
    if value {
        "On".to_string()
    } else {
        "Off".to_string()
    }
}

/// The name of a dimension variable, if it is one we expose.
pub(crate) fn find(name: &str) -> Option<&'static DimVariable> {
    DIM_VARIABLES
        .iter()
        .find(|variable| variable.name.eq_ignore_ascii_case(name))
}

/// Read the current value of a variable from `style`, or `None` when the
/// name is not a dimension variable.
pub(crate) fn get(style: &DimStyle, name: &str) -> Option<String> {
    let value = match name.to_ascii_uppercase().as_str() {
        "DIMCLRD" => fmt_color(style.dimclrd),
        "DIMDLE" => fmt_float(style.dimdle),
        "DIMDLI" => fmt_float(style.dimdli),
        "DIMGAP" => fmt_float(style.dimgap),
        "DIMLWD" => fmt_line_weight(style.dimlwd),
        "DIMSD1" => fmt_bool(style.dimsd1),
        "DIMSD2" => fmt_bool(style.dimsd2),
        "DIMCLRE" => fmt_color(style.dimclre),
        "DIMEXE" => fmt_float(style.dimexe),
        "DIMEXO" => fmt_float(style.dimexo),
        "DIMLWE" => fmt_line_weight(style.dimlwe),
        "DIMSE1" => fmt_bool(style.dimse1),
        "DIMSE2" => fmt_bool(style.dimse2),
        "DIMFXL" => fmt_float(style.dimfxl),
        "DIMFXLON" => fmt_bool(style.dimfxlon),
        "DIMASZ" => fmt_float(style.dimasz),
        "DIMBLK" => style.dimblk_name.clone(),
        "DIMBLK1" => style.dimblk1_name.clone(),
        "DIMBLK2" => style.dimblk2_name.clone(),
        "DIMSAH" => fmt_bool(style.dimsah),
        "DIMCEN" => fmt_float(style.dimcen),
        "DIMTSZ" => fmt_float(style.dimtsz),
        "DIMCLRT" => fmt_color(style.dimclrt),
        "DIMTXT" => fmt_float(style.dimtxt),
        "DIMJUST" => style.dimjust.to_string(),
        "DIMTAD" => style.dimtad.to_string(),
        "DIMTVP" => fmt_float(style.dimtvp),
        "DIMTIH" => fmt_bool(style.dimtih),
        "DIMTOH" => fmt_bool(style.dimtoh),
        "DIMTIX" => fmt_bool(style.dimtix),
        "DIMSOXD" => fmt_bool(style.dimsoxd),
        "DIMTFILL" => style.dimtfill.to_string(),
        "DIMTFILLCLR" => fmt_color(style.dimtfillclr),
        "DIMTMOVE" => style.dimtmove.to_string(),
        "DIMTXTDIRECTION" => fmt_bool(style.dimtxtdirection),
        "DIMTXSTY" => style.dimtxsty.clone(),
        "DIMSCALE" => fmt_float(style.dimscale),
        "DIMLFAC" => fmt_float(style.dimlfac),
        "DIMLUNIT" => style.dimlunit.to_string(),
        "DIMDEC" => style.dimdec.to_string(),
        "DIMRND" => fmt_float(style.dimrnd),
        "DIMDSEP" => style.dimdsep.to_string(),
        "DIMZIN" => style.dimzin.to_string(),
        "DIMAUNIT" => style.dimaunit.to_string(),
        "DIMADEC" => style.dimadec.to_string(),
        "DIMAZIN" => style.dimazin.to_string(),
        "DIMALT" => fmt_bool(style.dimalt),
        "DIMALTF" => fmt_float(style.dimaltf),
        "DIMALTD" => style.dimaltd.to_string(),
        "DIMALTU" => style.dimaltu.to_string(),
        "DIMALTTD" => style.dimalttd.to_string(),
        "DIMALTRND" => fmt_float(style.dimaltrnd),
        "DIMAPOST" => style.dimapost.clone(),
        "DIMALTZ" => style.dimaltz.to_string(),
        "DIMALTTZ" => style.dimalttz.to_string(),
        "DIMALTMZF" => fmt_float(style.dimaltmzf),
        "DIMALTMZS" => style.dimaltmzs.clone(),
        "DIMMZF" => fmt_float(style.dimmzf),
        "DIMMZS" => style.dimmzs.clone(),
        "DIMTOL" => fmt_bool(style.dimtol),
        "DIMLIM" => fmt_bool(style.dimlim),
        "DIMTP" => fmt_float(style.dimtp),
        "DIMTM" => fmt_float(style.dimtm),
        "DIMTDEC" => style.dimtdec.to_string(),
        "DIMTFAC" => fmt_float(style.dimtfac),
        "DIMTOLJ" => style.dimtolj.to_string(),
        "DIMTZIN" => style.dimtzin.to_string(),
        "DIMATFIT" => style.dimatfit.to_string(),
        "DIMTOFL" => fmt_bool(style.dimtofl),
        "DIMUPT" => fmt_bool(style.dimupt),
        "DIMFIT" => style.dimfit.to_string(),
        "DIMPOST" => style.dimpost.clone(),
        "DIMFRAC" => style.dimfrac.to_string(),
        "DIMARCSYM" => style.dimarcsym.to_string(),
        "DIMJOGANG" => fmt_float(style.dimjogang),
        "DIMUNIT" => style.dimunit.to_string(),
        _ => return None,
    };
    Some(value)
}

/// The factory default for a variable, from a fresh style.
pub(crate) fn factory_default(name: &str) -> Option<String> {
    get(&DimStyle::new("default"), name)
}

/// The rows for a full variable dump: a header row followed by one
/// `<name><TAB><current><TAB><factory default><TAB><description>` row per
/// variable.
pub(crate) fn dump(style: &DimStyle) -> Vec<String> {
    let mut rows = vec![format!(
        "{:<16}{:<12}{:<12}{}",
        "Variable", "Value", "Default", "Description"
    )];
    rows.extend(DIM_VARIABLES.iter().filter_map(|variable| {
        let current = get(style, variable.name)?;
        let default = factory_default(variable.name).unwrap_or_default();
        Some(format!(
            "{:<16}{:<12}{:<12}{}",
            variable.name, current, default, variable.description
        ))
    }));
    rows
}

/// Parse a float for write, returning a reason on failure.
fn parse_float(raw: &str) -> Result<f64, String> {
    raw.trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("\"{raw}\" is not a number"))
}

/// Parse a boolean for write, returning a reason on failure.
fn parse_bool(raw: &str) -> Result<bool, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "on" | "yes" | "true" => Ok(true),
        "0" | "off" | "no" | "false" => Ok(false),
        other => Err(format!("\"{other}\" is not on or off")),
    }
}

/// Parse an integer in `[min, max]` for write.
fn parse_int(raw: &str, min: i16, max: i16) -> Result<i16, String> {
    let value: i16 = raw
        .trim()
        .parse()
        .map_err(|_| format!("\"{raw}\" is not an integer"))?;
    if (min..=max).contains(&value) {
        Ok(value)
    } else {
        Err(format!("{value} is outside the range {min}-{max}"))
    }
}

/// Parse a colour index: ByBlock / ByLayer or 0..=256.
fn parse_color(raw: &str) -> Result<i16, String> {
    let text = raw.trim();
    match text.to_ascii_lowercase().as_str() {
        "byblock" => return Ok(0),
        "bylayer" => return Ok(256),
        _ => {}
    }
    let value: i16 = text
        .parse()
        .map_err(|_| format!("\"{raw}\" is not a colour index"))?;
    if (0..=256).contains(&value) {
        Ok(value)
    } else {
        Err(format!("{value} is outside the range 0-256 (ByBlock..ByLayer)"))
    }
}

/// Parse a line weight: ByLayer / ByBlock / Default or millimetres.
fn parse_line_weight(raw: &str) -> Result<i16, String> {
    let text = raw.trim();
    match text.to_ascii_lowercase().as_str() {
        "bylayer" => return Ok(-1),
        "byblock" => return Ok(-2),
        "default" => return Ok(-3),
        _ => {}
    }
    let mm: f64 = text
        .trim_end_matches("mm")
        .trim()
        .parse()
        .map_err(|_| format!("\"{raw}\" is not a line weight in mm"))?;
    let value = (mm * 100.0).round() as i16;
    if (0..=211).contains(&value) {
        Ok(value)
    } else {
        Err(format!("{mm} mm is outside the range 0-2.11 mm"))
    }
}

/// Set one variable on `style`, with the variable's validation. Block and
/// text-style names resolve through the caller's tables (the caller holds
/// the document's borrows).
/// `resolve_block` / `resolve_style` map a name to `(handle, resolved name)`,
/// or `None` when it does not exist.
/// Returns the old and new values as the command line prints them.
pub(crate) fn set(
    style: &mut DimStyle,
    name: &str,
    raw: &str,
    resolve_block: &dyn Fn(&str) -> Option<(document::Handle, String)>,
    resolve_style: &dyn Fn(&str) -> Option<(document::Handle, String)>,
) -> Result<(String, String), String> {
    let variable = find(name).ok_or_else(|| format!("'{}' is not a dimension variable", name))?;
    let old = get(style, variable.name).unwrap_or_default();
    let upper = variable.name;
    match variable.kind {
        Kind::Float | Kind::Positive | Kind::NonNegative | Kind::NonZero => {
            let value = parse_float(raw)?;
            match variable.kind {
                Kind::Positive if value <= 0.0 => {
                    return Err(format!("{value} must be greater than 0 for {upper}"))
                }
                Kind::NonNegative if value < 0.0 => {
                    return Err(format!("{value} must not be negative for {upper}"))
                }
                Kind::NonZero if value == 0.0 => {
                    return Err(format!("{upper} must not be 0"))
                }
                _ => {}
            }
            match upper {
                "DIMDLE" => style.dimdle = value,
                "DIMDLI" => style.dimdli = value,
                "DIMGAP" => style.dimgap = value,
                "DIMEXE" => style.dimexe = value,
                "DIMEXO" => style.dimexo = value,
                "DIMFXL" => style.dimfxl = value,
                "DIMASZ" => style.dimasz = value,
                "DIMCEN" => style.dimcen = value,
                "DIMTSZ" => style.dimtsz = value,
                "DIMTXT" => style.dimtxt = value,
                "DIMTVP" => style.dimtvp = value,
                "DIMTFAC" => style.dimtfac = value,
                "DIMSCALE" => style.dimscale = value,
                "DIMLFAC" => style.dimlfac = value,
                "DIMRND" => style.dimrnd = value,
                "DIMJOGANG" => style.dimjogang = value,
                "DIMALTF" => style.dimaltf = value,
                "DIMALTRND" => style.dimaltrnd = value,
                "DIMALTMZF" => style.dimaltmzf = value,
                "DIMMZF" => style.dimmzf = value,
                "DIMTP" => style.dimtp = value,
                "DIMTM" => style.dimtm = value,
                _ => return Err(format!("{upper} is not writable as a number")),
            }
        }
        Kind::Int { min, max } => {
            let value = parse_int(raw, min, max)?;
            match upper {
                "DIMJUST" => style.dimjust = value,
                "DIMTAD" => style.dimtad = value,
                "DIMTFILL" => style.dimtfill = value,
                "DIMTMOVE" => style.dimtmove = value,
                "DIMLUNIT" => style.dimlunit = value,
                "DIMDEC" => style.dimdec = value,
                "DIMDSEP" => style.dimdsep = value,
                "DIMZIN" => style.dimzin = value,
                "DIMAUNIT" => style.dimaunit = value,
                "DIMADEC" => style.dimadec = value,
                "DIMAZIN" => style.dimazin = value,
                "DIMALTD" => style.dimaltd = value,
                "DIMALTU" => style.dimaltu = value,
                "DIMALTTD" => style.dimalttd = value,
                "DIMALTZ" => style.dimaltz = value,
                "DIMALTTZ" => style.dimalttz = value,
                "DIMTDEC" => style.dimtdec = value,
                "DIMTOLJ" => style.dimtolj = value,
                "DIMTZIN" => style.dimtzin = value,
                "DIMATFIT" => style.dimatfit = value,
                "DIMFIT" => style.dimfit = value,
                "DIMFRAC" => style.dimfrac = value,
                "DIMARCSYM" => style.dimarcsym = value,
                "DIMUNIT" => style.dimunit = value,
                _ => return Err(format!("{upper} is not writable as an integer")),
            }
        }
        Kind::Aci => {
            let value = parse_color(raw)?;
            match upper {
                "DIMCLRD" => style.dimclrd = value,
                "DIMCLRE" => style.dimclre = value,
                "DIMCLRT" => style.dimclrt = value,
                "DIMTFILLCLR" => style.dimtfillclr = value,
                _ => return Err(format!("{upper} is not a colour variable")),
            }
        }
        Kind::LineWeight => {
            let value = parse_line_weight(raw)?;
            match upper {
                "DIMLWD" => style.dimlwd = value,
                "DIMLWE" => style.dimlwe = value,
                _ => return Err(format!("{upper} is not a line weight variable")),
            }
        }
        Kind::Bool => {
            let value = parse_bool(raw)?;
            match upper {
                "DIMSD1" => style.dimsd1 = value,
                "DIMSD2" => style.dimsd2 = value,
                "DIMSE1" => style.dimse1 = value,
                "DIMSE2" => style.dimse2 = value,
                "DIMFXLON" => style.dimfxlon = value,
                "DIMSAH" => style.dimsah = value,
                "DIMTIH" => style.dimtih = value,
                "DIMTOH" => style.dimtoh = value,
                "DIMTIX" => style.dimtix = value,
                "DIMSOXD" => style.dimsoxd = value,
                "DIMTXTDIRECTION" => style.dimtxtdirection = value,
                "DIMALT" => style.dimalt = value,
                "DIMTOL" => style.dimtol = value,
                "DIMLIM" => style.dimlim = value,
                "DIMTOFL" => style.dimtofl = value,
                "DIMUPT" => style.dimupt = value,
                _ => return Err(format!("{upper} is not a boolean variable")),
            }
        }
        Kind::Str => match upper {
            "DIMAPOST" => style.dimapost = raw.to_string(),
            "DIMALTMZS" => style.dimaltmzs = raw.to_string(),
            "DIMMZS" => style.dimmzs = raw.to_string(),
            "DIMPOST" => style.dimpost = raw.to_string(),
            _ => return Err(format!("{upper} is not a text variable")),
        },
        Kind::BlockName => {
            let text = raw.trim();
            if text.is_empty() {
                match upper {
                    "DIMBLK" => {
                        style.dimblk = document::Handle::NULL;
                        style.dimblk_name.clear();
                    }
                    "DIMBLK1" => {
                        style.dimblk1 = document::Handle::NULL;
                        style.dimblk1_name.clear();
                    }
                    "DIMBLK2" => {
                        style.dimblk2 = document::Handle::NULL;
                        style.dimblk2_name.clear();
                    }
                    _ => unreachable!(),
                }
            } else {
                let (handle, name) = resolve_block(text)
                    .ok_or_else(|| format!("no block named \"{text}\""))?;
                match upper {
                    "DIMBLK" => {
                        style.dimblk = handle;
                        style.dimblk_name = name;
                    }
                    "DIMBLK1" => {
                        style.dimblk1 = handle;
                        style.dimblk1_name = name;
                    }
                    "DIMBLK2" => {
                        style.dimblk2 = handle;
                        style.dimblk2_name = name;
                    }
                    _ => unreachable!(),
                }
            }
        }
        Kind::TextStyleName => {
            let text = raw.trim();
            let (handle, name) = resolve_style(text)
                .ok_or_else(|| format!("no text style named \"{text}\""))?;
            style.dimtxsty_handle = handle;
            style.dimtxsty = name;
        }
    }
    Ok((old, get(style, variable.name).unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_blocks(_: &str) -> Option<(document::Handle, String)> {
        None
    }

    fn no_styles(_: &str) -> Option<(document::Handle, String)> {
        None
    }

    #[test]
    fn dump_prints_every_variable_with_value_default_and_description() {
        let style = DimStyle::new("ISO-25");
        let rows = dump(&style);
        assert_eq!(rows.len(), DIM_VARIABLES.len() + 1);
        assert!(rows[0].contains("Variable"));
        assert!(rows[0].contains("Value"));
        assert!(rows[0].contains("Default"));
        assert!(rows[0].contains("Description"));
        for variable in DIM_VARIABLES {
            let row = rows
                .iter()
                .find(|row| row.split_whitespace().next() == Some(variable.name))
                .unwrap_or_else(|| panic!("row for {}", variable.name));
            assert!(!row.trim_end().ends_with(' '), "{row}");
            assert!(row.len() > variable.name.len() + 10, "{row}");
            assert!(row.contains(variable.description), "{row}");
        }
        // The fresh metric style reports its values.
        let text = rows.join("\n");
        assert!(text.contains("DIMTXT"));
        let dimtxt = rows
            .iter()
            .find(|row| row.split_whitespace().next() == Some("DIMTXT"))
            .unwrap();
        assert!(dimtxt.contains("0.18"), "{dimtxt}");
    }

    #[test]
    fn set_validates_and_applies_every_kind() {
        let mut style = DimStyle::new("ISO-25");
        // Positive floats refuse zero and negatives.
        assert!(set(&mut style, "DIMTXT", "2.5", &no_blocks, &no_styles).is_ok());
        assert_eq!(style.dimtxt, 2.5);
        assert!(set(&mut style, "DIMTXT", "0", &no_blocks, &no_styles).is_err());
        assert!(set(&mut style, "DIMTXT", "-1", &no_blocks, &no_styles).is_err());
        assert!(set(&mut style, "DIMTXT", "banana", &no_blocks, &no_styles).is_err());
        // Non-zero scale factor.
        assert!(set(&mut style, "DIMLFAC", "0", &no_blocks, &no_styles).is_err());
        assert!(set(&mut style, "DIMLFAC", "2.5", &no_blocks, &no_styles).is_ok());
        // Int-ranged precisions refuse out-of-range values.
        assert!(set(&mut style, "DIMDEC", "9", &no_blocks, &no_styles).is_err());
        assert!(set(&mut style, "DIMDEC", "2", &no_blocks, &no_styles).is_ok());
        // Colours accept ByBlock/ByLayer and refuse 300.
        assert_eq!(
            set(&mut style, "DIMCLRD", "ByBlock", &no_blocks, &no_styles)
                .map(|(_, new)| new)
                .unwrap(),
            "ByBlock"
        );
        assert!(set(&mut style, "DIMCLRD", "300", &no_blocks, &no_styles).is_err());
        assert!(set(&mut style, "DIMCLRD", "3", &no_blocks, &no_styles).is_ok());
        assert_eq!(style.dimclrd, 3);
        // Line weights accept mm values and keyword forms.
        assert!(set(&mut style, "DIMLWE", "0.53", &no_blocks, &no_styles).is_ok());
        assert_eq!(style.dimlwe, 53);
        assert!(set(&mut style, "DIMLWE", "ByLayer", &no_blocks, &no_styles).is_ok());
        assert_eq!(style.dimlwe, -1);
        assert!(set(&mut style, "DIMLWE", "99", &no_blocks, &no_styles).is_err());
        // Booleans accept the keyword forms.
        assert!(set(&mut style, "DIMTOL", "on", &no_blocks, &no_styles).is_ok());
        assert_eq!(style.dimtol, true);
        assert!(set(&mut style, "DIMTOL", "banana", &no_blocks, &no_styles).is_err());
        // Strings are free-form.
        assert!(set(&mut style, "DIMPOST", "mm", &no_blocks, &no_styles).is_ok());
        assert_eq!(style.dimpost, "mm");
        // Block names clear on empty input and resolve unknown names to an error.
        assert!(set(&mut style, "DIMBLK", "", &no_blocks, &no_styles).is_ok());
        assert!(set(&mut style, "DIMBLK", "NO_SUCH_BLOCK", &no_blocks, &no_styles).is_err());
        let arrow = |_: &str| Some((document::Handle::new(41), "ARROW".to_string()));
        assert!(set(&mut style, "DIMBLK", "arrow", &arrow, &no_styles).is_ok());
        assert_eq!(style.dimblk, document::Handle::new(41));
        assert_eq!(style.dimblk_name, "ARROW");
        // Text style names must resolve.
        assert!(set(&mut style, "DIMTXSTY", "NO_SUCH_STYLE", &no_blocks, &no_styles).is_err());
        let style_lookup = |_: &str| Some((document::Handle::new(42), "Standard".to_string()));
        assert!(set(&mut style, "DIMTXSTY", "standard", &no_blocks, &style_lookup).is_ok());
        assert_eq!(style.dimtxsty_handle, document::Handle::new(42));
        // Ints in range apply.
        assert!(set(&mut style, "DIMTOLJ", "2", &no_blocks, &no_styles).is_ok());
        assert_eq!(style.dimtolj, 2);
        assert!(set(&mut style, "DIMTOLJ", "3", &no_blocks, &no_styles).is_err());
    }

    #[test]
    fn every_variable_round_trips_through_get_and_set() {
        let mut style = DimStyle::new("ISO-25");
        for variable in DIM_VARIABLES {
            let current = get(&style, variable.name);
            assert!(current.is_some(), "{} reads", variable.name);
            match variable.kind {
                Kind::Bool => {
                    let _ = set(&mut style, variable.name, "On", &no_blocks, &no_styles)
                        .unwrap_or_else(|reason| panic!("{}: {reason}", variable.name));
                    let _ = set(&mut style, variable.name, "Off", &no_blocks, &no_styles)
                        .unwrap_or_else(|reason| panic!("{}: {reason}", variable.name));
                }
                Kind::Str => {
                    let _ = set(&mut style, variable.name, "probe", &no_blocks, &no_styles)
                        .unwrap_or_else(|reason| panic!("{}: {reason}", variable.name));
                }
                Kind::Aci => {
                    let _ = set(&mut style, variable.name, "7", &no_blocks, &no_styles)
                        .unwrap_or_else(|reason| panic!("{}: {reason}", variable.name));
                }
                Kind::LineWeight => {
                    let _ = set(&mut style, variable.name, "0.25", &no_blocks, &no_styles)
                        .unwrap_or_else(|reason| panic!("{}: {reason}", variable.name));
                }
                Kind::Int { .. } => {
                    let min = if let Kind::Int { min, .. } = variable.kind { min } else { unreachable!() };
                    let _ = set(&mut style, variable.name, &min.to_string(), &no_blocks, &no_styles)
                        .unwrap_or_else(|reason| panic!("{}: {reason}", variable.name));
                }
                Kind::Float | Kind::Positive | Kind::NonNegative | Kind::NonZero => {
                    let _ = set(&mut style, variable.name, "1", &no_blocks, &no_styles)
                        .unwrap_or_else(|reason| panic!("{}: {reason}", variable.name));
                }
                Kind::BlockName | Kind::TextStyleName => {
                    let resolve = |_: &str| Some((document::Handle::new(7), "probe".to_string()));
                    let _ = set(&mut style, variable.name, "probe", &resolve, &resolve)
                        .unwrap_or_else(|reason| panic!("{}: {reason}", variable.name));
                }
            }
        }
    }
}
