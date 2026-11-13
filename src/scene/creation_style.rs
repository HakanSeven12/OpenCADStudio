use document::objects::ObjectType;
use document::tables::{DimStyle, TextStyle};
use document::{Document, EntityType, Handle};

#[derive(Debug, Clone)]
pub struct TextCreationDefaults {
    pub style_name: String,
    pub height: f64,
    pub width_factor: f64,
    pub oblique_angle: f64,
}

#[derive(Debug, Clone)]
pub struct DimensionCreationDefaults {
    pub style_name: String,
    pub style_handle: Handle,
    pub text_style_name: String,
    pub text_height: f64,
    pub gap: f64,
    pub arrow_size: f64,
    pub jog_angle: f64,
    pub scale: f64,
    pub annotative: bool,
}

fn named_text_style(doc: &Document, name: &str) -> Option<TextStyle> {
    doc.text_styles
        .iter()
        .find(|style| style.name.eq_ignore_ascii_case(name))
        .or_else(|| {
            doc.text_styles
                .iter()
                .find(|style| style.name.eq_ignore_ascii_case("Standard"))
        })
        .cloned()
}

/// Adds the reference's ISO-25 dimension style (the current style of its
/// metric template) when the drawing has none of that name, and hands back its
/// handle: the header records the current dimension style by handle as well as
/// by name, and a DWG carries only the handle.
pub fn ensure_iso_dim_style(doc: &mut Document) -> Handle {
    if let Some(style) = doc
        .dim_styles
        .iter()
        .find(|style| style.name.eq_ignore_ascii_case("ISO-25"))
    {
        return style.handle;
    }
    let mut style = DimStyle::new("ISO-25");
    style.handle = doc.allocate_handle();
    // Separate arrowheads off, as in the reference's ISO-25: DIMBLK draws
    // both ends.
    style.dimsah = false;
    style.dimtxt = 2.5;
    style.dimasz = 2.5;
    style.dimcen = 2.5;
    style.dimexe = 1.25;
    style.dimexo = 0.625;
    style.dimgap = 0.625;
    style.dimdli = 3.75;
    style.dimtad = 1;
    style.dimtih = false;
    style.dimtoh = false;
    style.dimtofl = true;
    style.dimtix = false;
    style.dimsoxd = false;
    style.dimdsep = i16::from(b',');
    style.dimlunit = 2;
    style.dimdec = 2;
    style.dimtdec = 2;
    style.dimaunit = 0;
    style.dimadec = 0;
    style.dimazin = 0;
    style.dimzin = 8;
    style.dimtzin = 8;
    style.dimatfit = 3;
    style.dimtmove = 0;
    style.dimscale = 1.0;
    style.dimlfac = 1.0;
    style.dimtfac = 1.0;
    style.dimaltf = 0.03937007874016;
    style.dimaltd = 3;
    style.dimalttd = 3;
    style.dimaltu = 2;
    style.dimfxl = 1.0;
    style.dimjogang = std::f64::consts::FRAC_PI_2;
    style.dimmzf = 100.0;
    style.dimaltmzf = 100.0;
    let handle = style.handle;
    let _ = doc.dim_styles.add(style);
    set_dim_style_extras(
        doc,
        handle,
        DimStyleExtras {
            jog_height: 1.5,
            break_size: 3.75,
            dimtaln: 0,
        },
    );
    handle
}

/// The metric template's text styles: Standard draws with Arial, and an
/// annotative Annotative style (Arial too) sits beside it. Both keep 2.5 as
/// the last height used and the Arial face's character set and pitch (34).
pub fn ensure_metric_text_styles(doc: &mut Document) {
    fn arial(style: &mut TextStyle) {
        style.font_file = "arial.ttf".to_string();
        style.true_type_font = "Arial".to_string();
        style.true_type_font_flags = 34;
        style.last_height = 2.5;
    }
    if let Some(standard) = doc.text_styles.get_mut("Standard") {
        arial(standard);
    }
    if !doc
        .text_styles
        .iter()
        .any(|style| style.name.eq_ignore_ascii_case("Annotative"))
    {
        let mut style = TextStyle::new("Annotative");
        style.handle = doc.allocate_handle();
        arial(&mut style);
        style.annotative = true;
        let _ = doc.text_styles.add(style);
    }
}

/// The metric template's Annotative dimension style: ISO-25's settings,
/// annotative, with the overall scale 0 (the annotation scale sizes it).
/// Made from ISO-25, which is made first when the drawing lacks it.
pub fn ensure_annotative_dim_style(doc: &mut Document) -> Handle {
    if let Some(style) = doc
        .dim_styles
        .iter()
        .find(|style| style.name.eq_ignore_ascii_case("Annotative"))
    {
        return style.handle;
    }
    let iso = ensure_iso_dim_style(doc);
    let Some(mut style) = doc.dim_styles.iter().find(|s| s.handle == iso).cloned() else {
        return Handle::NULL;
    };
    style.name = "Annotative".to_string();
    style.handle = doc.allocate_handle();
    style.annotative = true;
    style.dimscale = 0.0;
    let handle = style.handle;
    let _ = doc.dim_styles.add(style);
    set_dim_style_extras(
        doc,
        handle,
        DimStyleExtras {
            jog_height: 1.5,
            break_size: 3.75,
            dimtaln: 0,
        },
    );
    handle
}

/// The extended data a dimension style keeps beside its variables: the
/// linear jog height factor (`ACAD_DSTYLE_DIMJAG`, group 388), the
/// dimension break size (`ACAD_DSTYLE_DIMBREAK`, 391) and DIMTALN
/// (`ACAD_DSTYLE_DIMTALN`, 392).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DimStyleExtras {
    pub jog_height: f64,
    pub break_size: f64,
    pub dimtaln: i16,
}

const DIMJAG_APP: &str = "ACAD_DSTYLE_DIMJAG";
const DIMBREAK_APP: &str = "ACAD_DSTYLE_DIMBREAK";
const DIMTALN_APP: &str = "ACAD_DSTYLE_DIMTALN";

/// `handle`'s extras; a style without them has the defaults of the
/// drawing's measurement system.
pub fn dim_style_extras(doc: &Document, handle: Handle) -> DimStyleExtras {
    use document::xdata::XDataValue;
    let value = |app: &str| {
        doc.object_xdata(handle)
            .and_then(|data| data.get_record(app))
            .and_then(|record| record.values.get(1).cloned())
    };
    let real = |app: &str, default: f64| match value(app) {
        Some(XDataValue::Real(v) | XDataValue::Distance(v) | XDataValue::ScaleFactor(v)) => v,
        _ => default,
    };
    DimStyleExtras {
        jog_height: real(DIMJAG_APP, 1.5),
        break_size: real(
            DIMBREAK_APP,
            if doc.header.measurement == 1 { 3.75 } else { 0.125 },
        ),
        dimtaln: match value(DIMTALN_APP) {
            Some(XDataValue::Integer16(v)) => v,
            _ => 0,
        },
    }
}

/// Store `extras` on the style `handle`, registering their applications.
pub fn set_dim_style_extras(doc: &mut Document, handle: Handle, extras: DimStyleExtras) {
    use document::xdata::{ExtendedDataRecord, XDataValue};
    for (app, code, value) in [
        (DIMJAG_APP, 388, XDataValue::Real(extras.jog_height)),
        (DIMBREAK_APP, 391, XDataValue::Real(extras.break_size)),
        (DIMTALN_APP, 392, XDataValue::Integer16(extras.dimtaln)),
    ] {
        if !doc.app_ids.contains(app) {
            let mut entry = document::tables::AppId::new(app);
            entry.handle = doc.allocate_handle();
            let _ = doc.app_ids.add(entry);
        }
        let mut record = ExtendedDataRecord::new(app);
        record.add_value(XDataValue::Integer16(code));
        record.add_value(value);
        doc.set_object_xdata_record(handle, record);
    }
}

fn named_dim_style(doc: &Document, name: &str) -> Option<DimStyle> {
    doc.dim_styles
        .iter()
        .find(|style| style.name.eq_ignore_ascii_case(name))
        .or_else(|| {
            doc.dim_styles
                .iter()
                .find(|style| style.name.eq_ignore_ascii_case("Standard"))
        })
        .cloned()
}

pub fn current_text_defaults(doc: &Document) -> TextCreationDefaults {
    let requested = doc.header.current_text_style_name.trim();
    let style = named_text_style(doc, requested);
    let variable_height = if doc.header.text_height > 1.0e-9 {
        doc.header.text_height
    } else {
        style
            .as_ref()
            .map(|style| style.last_height)
            .filter(|height| *height > 1.0e-9)
            .unwrap_or(2.5)
    };
    TextCreationDefaults {
        style_name: style
            .as_ref()
            .map(|style| style.name.clone())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "Standard".to_string()),
        height: style
            .as_ref()
            .map(|style| style.height)
            .filter(|height| *height > 1.0e-9)
            .unwrap_or(variable_height),
        width_factor: style.as_ref().map_or(1.0, |style| style.width_factor),
        oblique_angle: style.as_ref().map_or(0.0, |style| style.oblique_angle),
    }
}

pub fn current_dimension_defaults(doc: &Document) -> DimensionCreationDefaults {
    let requested = doc.header.current_dimstyle_name.trim();
    let style = named_dim_style(doc, requested);
    DimensionCreationDefaults {
        style_name: style
            .as_ref()
            .map(|style| style.name.clone())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "Standard".to_string()),
        style_handle: style.as_ref().map_or(Handle::NULL, |style| style.handle),
        text_style_name: style
            .as_ref()
            .map(|style| style.dimtxsty.clone())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| current_text_defaults(doc).style_name),
        text_height: style.as_ref().map_or(2.5, |style| style.dimtxt),
        gap: style.as_ref().map_or(0.625, |style| style.dimgap),
        arrow_size: style.as_ref().map_or(2.5, |style| style.dimasz),
        jog_angle: style
            .as_ref()
            .map_or(std::f64::consts::FRAC_PI_4, |style| style.dimjogang),
        scale: style.as_ref().map_or(1.0, |style| {
            if style.annotative {
                1.0
            } else if style.dimscale > 1.0e-9 {
                style.dimscale
            } else {
                1.0
            }
        }),
        annotative: style.as_ref().is_some_and(|style| style.annotative),
    }
}

fn entity_uses_default_style(name: &str) -> bool {
    name.trim().is_empty() || name.eq_ignore_ascii_case("Standard")
}

fn apply_text_defaults(doc: &Document, entity: &mut EntityType) {
    let active = current_text_defaults(doc);
    let style_name = match entity {
        EntityType::Text(text) => &mut text.style,
        EntityType::MText(text) => &mut text.style,
        EntityType::AttributeEntity(attribute) => &mut attribute.text_style,
        EntityType::AttributeDefinition(attribute) => &mut attribute.text_style,
        _ => return,
    };
    if style_name.trim().is_empty() {
        style_name.clone_from(&active.style_name);
    }
    let mut resolved =
        named_text_style(doc, style_name).unwrap_or_else(|| TextStyle::new(style_name.clone()));
    // An annotative style's height is on paper; the creating command already
    // gave the entity its model height at the current annotation scale.
    if resolved.annotative {
        resolved.height = 0.0;
    }
    // Backward and upside down are not copied: the drawn state is the style
    // flag XOR the entity's own (group 71), so a copy would cancel the style.
    match entity {
        EntityType::Text(text) => {
            if resolved.height > 1.0e-9 {
                text.height = resolved.height;
            }
            if (text.width_factor - 1.0).abs() <= 1.0e-9 {
                text.width_factor = resolved.width_factor;
            }
            if text.oblique_angle.abs() <= 1.0e-9 {
                text.oblique_angle = resolved.oblique_angle;
            }
        }
        EntityType::MText(text) => {
            if resolved.height > 1.0e-9 {
                text.height = resolved.height;
            }
        }
        EntityType::AttributeEntity(attribute) => {
            if resolved.height > 1.0e-9 {
                attribute.height = resolved.height;
            }
            if (attribute.width_factor - 1.0).abs() <= 1.0e-9 {
                attribute.width_factor = resolved.width_factor;
            }
            if attribute.oblique_angle.abs() <= 1.0e-9 {
                attribute.oblique_angle = resolved.oblique_angle;
            }
        }
        EntityType::AttributeDefinition(attribute) => {
            if resolved.height > 1.0e-9 {
                attribute.height = resolved.height;
            }
            if (attribute.width_factor - 1.0).abs() <= 1.0e-9 {
                attribute.width_factor = resolved.width_factor;
            }
            if attribute.oblique_angle.abs() <= 1.0e-9 {
                attribute.oblique_angle = resolved.oblique_angle;
            }
        }
        _ => {}
    }
}

fn apply_dimension_defaults(doc: &Document, entity: &mut EntityType) {
    let active = current_dimension_defaults(doc);
    match entity {
        EntityType::Dimension(dimension) => {
            if entity_uses_default_style(&dimension.base().style_name) {
                dimension
                    .base_mut()
                    .style_name
                    .clone_from(&active.style_name);
            }
        }
        EntityType::Leader(leader) => {
            if entity_uses_default_style(&leader.dimension_style) {
                leader.dimension_style.clone_from(&active.style_name);
                leader.text_height = active.text_height;
                leader.dimension_gap = active.gap;
                leader.arrow_size = active.arrow_size;
            }
        }
        EntityType::Tolerance(tolerance) => {
            if entity_uses_default_style(&tolerance.dimension_style_name) {
                tolerance
                    .dimension_style_name
                    .clone_from(&active.style_name);
                tolerance.dimension_style_handle =
                    (!active.style_handle.is_null()).then_some(active.style_handle);
                tolerance.text_height = active.text_height;
                tolerance.dimension_gap = active.gap;
            }
        }
        _ => {}
    }
}

fn apply_object_defaults(doc: &Document, entity: &mut EntityType) {
    match entity {
        EntityType::MultiLeader(leader) if leader.style_handle.is_none() => {
            let name = doc.header.current_mleader_style_name.trim();
            if let Some(style) = doc
                .objects
                .iter()
                .find_map(|(handle, object)| match object {
                    ObjectType::MultiLeaderStyle(style)
                        if style.name.eq_ignore_ascii_case(name) =>
                    {
                        let mut style = style.clone();
                        style.handle = *handle;
                        Some(style)
                    }
                    _ => None,
                })
            {
                crate::scene::annotative::apply_mleader_style(leader, &style);
            }
        }
        EntityType::Table(table) if table.table_style_handle.is_none() => {
            let name = doc.header.current_table_style_name.trim();
            table.table_style_handle =
                doc.objects
                    .iter()
                    .find_map(|(handle, object)| match object {
                        ObjectType::TableStyle(style) if style.name.eq_ignore_ascii_case(name) => {
                            Some(*handle)
                        }
                        _ => None,
                    });
        }
        EntityType::MLine(mline) if mline.style_handle.is_none() => {
            let name = doc.header.multiline_style.trim();
            if let Some((handle, style_name)) =
                doc.objects
                    .iter()
                    .find_map(|(handle, object)| match object {
                        ObjectType::MLineStyle(style) if style.name.eq_ignore_ascii_case(name) => {
                            Some((*handle, style.name.clone()))
                        }
                        _ => None,
                    })
            {
                mline.style_handle = Some(handle);
                mline.style_name = style_name;
            }
        }
        _ => {}
    }
}

/// The drawing's DIMDLI takes the current dimension style's value, as
/// making a style current does; DIM's Offset changes it afterwards.
pub fn sync_header_dim_vars(doc: &mut Document) {
    let current = doc.header.current_dimstyle_name.clone();
    let style = doc
        .dim_styles
        .iter()
        .find(|style| style.name.eq_ignore_ascii_case(&current))
        .cloned();
    if let Some(style) = style {
        // Every dimension variable of the header is the current style's
        // (the header holds the current style plus its overrides); the
        // header's DIMZIN drops zeros in the numbers the drawing writes.
        write_header_dim_vars(doc, &style);
        doc.header.dim_line_increment = style.dimdli;
    }
}

fn header_color(index: i16, true_color: Option<document::types::Color>) -> document::types::Color {
    use document::types::Color;
    if let Some(rgb @ Color::Rgb { .. }) = true_color {
        return rgb;
    }
    match index {
        0 => Color::ByBlock,
        256 => Color::ByLayer,
        1..=255 => Color::Index(index as u8),
        _ => Color::ByBlock,
    }
}

fn style_color(color: document::types::Color) -> (i16, Option<document::types::Color>) {
    use document::types::Color;
    match color {
        Color::Rgb { .. } => (color.approximate_index(), Some(color)),
        other => (other.approximate_index(), None),
    }
}

/// Writes `style`'s dimension variables into the drawing header, which holds
/// the current style with its overrides. DIMDLI is the drawing's own (DIM's
/// Offset sets it) and is left to the caller.
pub fn write_header_dim_vars(doc: &mut Document, style: &DimStyle) {
    let text_style = doc
        .text_styles
        .iter()
        .find(|t| t.handle == style.dimtxsty_handle)
        .map(|t| t.name.clone())
        .unwrap_or_else(|| style.dimtxsty.clone());
    let linetype = |handle: Handle| {
        doc.line_types
            .iter()
            .find(|lt| lt.handle == handle)
            .map(|lt| lt.name.clone())
            .unwrap_or_default()
    };
    let (ltype, ltex1, ltex2) = (
        linetype(style.dimltex_handle),
        linetype(style.dimltex1_handle),
        linetype(style.dimltex2_handle),
    );
    let h = &mut doc.header;
    h.dim_scale = style.dimscale;
    h.dim_arrow_size = style.dimasz;
    h.dim_ext_line_offset = style.dimexo;
    h.dim_ext_line_extension = style.dimexe;
    h.dim_rounding = style.dimrnd;
    h.dim_line_extension = style.dimdle;
    h.dim_tolerance_plus = style.dimtp;
    h.dim_tolerance_minus = style.dimtm;
    h.dim_text_height = style.dimtxt;
    h.dim_center_mark = style.dimcen;
    h.dim_tick_size = style.dimtsz;
    h.dim_alt_scale = style.dimaltf;
    h.dim_linear_scale = style.dimlfac;
    h.dim_text_vertical_pos = style.dimtvp;
    h.dim_tolerance_scale = style.dimtfac;
    h.dim_line_gap = style.dimgap;
    h.dim_alt_rounding = style.dimaltrnd;
    h.dim_tolerance = style.dimtol;
    h.dim_limits = style.dimlim;
    h.dim_text_inside_horizontal = style.dimtih;
    h.dim_text_outside_horizontal = style.dimtoh;
    h.dim_suppress_ext1 = style.dimse1;
    h.dim_suppress_ext2 = style.dimse2;
    h.dim_text_above = style.dimtad;
    h.dim_zero_suppression = style.dimzin;
    h.dim_alt_zero_suppression = style.dimazin;
    h.dim_alternate_units = style.dimalt;
    h.dim_alt_decimal_places = style.dimaltd;
    h.dim_force_line_inside = style.dimtofl;
    h.dim_separate_arrows = style.dimsah;
    h.dim_force_text_inside = style.dimtix;
    h.dim_suppress_outside_ext = style.dimsoxd;
    h.dim_line_color = header_color(style.dimclrd, style.dimclrd_true_color);
    h.dim_ext_line_color = header_color(style.dimclre, style.dimclre_true_color);
    h.dim_text_color = header_color(style.dimclrt, style.dimclrt_true_color);
    h.dim_angular_decimal_places = style.dimadec;
    h.dim_decimal_places = style.dimdec;
    h.dim_tolerance_decimal_places = style.dimtdec;
    h.dim_alt_units_format = style.dimaltu;
    h.dim_alt_tolerance_decimal_places = style.dimalttd;
    h.dim_angular_units = style.dimaunit;
    h.dim_fraction_format = style.dimfrac;
    h.dim_linear_unit_format = style.dimlunit;
    h.dim_decimal_separator = char::from_u32(style.dimdsep.max(0) as u32).unwrap_or('.');
    h.dim_text_movement = style.dimtmove;
    h.dim_horizontal_justification = style.dimjust;
    h.dim_suppress_line1 = style.dimsd1;
    h.dim_suppress_line2 = style.dimsd2;
    h.dim_tolerance_justification = style.dimtolj;
    h.dim_tolerance_zero_suppression = style.dimtzin;
    h.dim_alt_tolerance_zero_suppression = style.dimaltz;
    h.dim_alt_tolerance_zero_tight = style.dimalttz;
    h.dim_fit = style.dimatfit;
    h.dim_user_positioned_text = style.dimupt;
    h.dim_post = style.dimpost.clone();
    h.dim_alt_post = style.dimapost.clone();
    h.dim_fixed_ext_line_length = style.dimfxl;
    h.dim_fixed_ext_line_on = style.dimfxlon;
    h.dim_arc_symbol = style.dimarcsym;
    h.dim_jog_angle = style.dimjogang;
    h.dim_text_fill = style.dimtfill;
    h.dim_text_fill_color = header_color(style.dimtfillclr, style.dimtfillclr_true_color);
    h.dim_text_direction = style.dimtxtdirection;
    h.dim_alt_mzf = style.dimaltmzf;
    h.dim_alt_mzs = style.dimaltmzs.clone();
    h.dim_mzf = style.dimmzf;
    h.dim_mzs = style.dimmzs.clone();
    h.dim_line_weight = style.dimlwd;
    h.dim_ext_line_weight = style.dimlwe;
    h.dim_text_style_handle = style.dimtxsty_handle;
    h.dim_text_style_name = text_style;
    h.dim_linetype_handle = style.dimltex_handle;
    h.dim_linetype1_handle = style.dimltex1_handle;
    h.dim_linetype2_handle = style.dimltex2_handle;
    h.dim_linetype_name = ltype;
    h.dim_linetype1_name = ltex1;
    h.dim_linetype2_name = ltex2;
    h.dim_arrow_block_handle = style.dimblk;
    h.dim_arrow_block1_handle = style.dimblk1;
    h.dim_arrow_block2_handle = style.dimblk2;
    h.dim_leader_arrow_block_handle = style.dimldrblk;
    // The arrow names follow from the handles when the drawing is written.
    h.dim_arrow_block.clear();
    h.dim_arrow_block1.clear();
    h.dim_arrow_block2.clear();
    h.dim_leader_arrow_block.clear();
}

/// The current style as the drawing header holds it: `base` with the
/// header's dimension variables laid over it. The header carries the
/// current style's overrides, so the differences from `base` are those.
pub fn header_dim_style(doc: &Document, base: &DimStyle) -> DimStyle {
    let h = &doc.header;
    let mut s = base.clone();
    // A value the file wrote with less precision is still the style's.
    let near = |value: f64, style: f64| {
        if (value - style).abs() <= 1e-9 * (1.0 + style.abs()) {
            style
        } else {
            value
        }
    };
    s.dimscale = near(h.dim_scale, s.dimscale);
    s.dimasz = near(h.dim_arrow_size, s.dimasz);
    s.dimexo = near(h.dim_ext_line_offset, s.dimexo);
    s.dimdli = near(h.dim_line_increment, s.dimdli);
    s.dimexe = near(h.dim_ext_line_extension, s.dimexe);
    s.dimrnd = near(h.dim_rounding, s.dimrnd);
    s.dimdle = near(h.dim_line_extension, s.dimdle);
    s.dimtp = near(h.dim_tolerance_plus, s.dimtp);
    s.dimtm = near(h.dim_tolerance_minus, s.dimtm);
    s.dimtxt = near(h.dim_text_height, s.dimtxt);
    s.dimcen = near(h.dim_center_mark, s.dimcen);
    s.dimtsz = near(h.dim_tick_size, s.dimtsz);
    s.dimaltf = near(h.dim_alt_scale, s.dimaltf);
    s.dimlfac = near(h.dim_linear_scale, s.dimlfac);
    s.dimtvp = near(h.dim_text_vertical_pos, s.dimtvp);
    s.dimtfac = near(h.dim_tolerance_scale, s.dimtfac);
    s.dimgap = near(h.dim_line_gap, s.dimgap);
    s.dimaltrnd = near(h.dim_alt_rounding, s.dimaltrnd);
    s.dimtol = h.dim_tolerance;
    s.dimlim = h.dim_limits;
    s.dimtih = h.dim_text_inside_horizontal;
    s.dimtoh = h.dim_text_outside_horizontal;
    s.dimse1 = h.dim_suppress_ext1;
    s.dimse2 = h.dim_suppress_ext2;
    s.dimtad = h.dim_text_above;
    s.dimzin = h.dim_zero_suppression;
    s.dimazin = h.dim_alt_zero_suppression;
    s.dimalt = h.dim_alternate_units;
    s.dimaltd = h.dim_alt_decimal_places;
    s.dimtofl = h.dim_force_line_inside;
    s.dimsah = h.dim_separate_arrows;
    s.dimtix = h.dim_force_text_inside;
    s.dimsoxd = h.dim_suppress_outside_ext;
    (s.dimclrd, s.dimclrd_true_color) = style_color(h.dim_line_color);
    (s.dimclre, s.dimclre_true_color) = style_color(h.dim_ext_line_color);
    (s.dimclrt, s.dimclrt_true_color) = style_color(h.dim_text_color);
    s.dimadec = h.dim_angular_decimal_places;
    s.dimdec = h.dim_decimal_places;
    s.dimtdec = h.dim_tolerance_decimal_places;
    s.dimaltu = h.dim_alt_units_format;
    s.dimalttd = h.dim_alt_tolerance_decimal_places;
    s.dimaunit = h.dim_angular_units;
    s.dimfrac = h.dim_fraction_format;
    s.dimlunit = h.dim_linear_unit_format;
    s.dimdsep = h.dim_decimal_separator as i16;
    s.dimtmove = h.dim_text_movement;
    s.dimjust = h.dim_horizontal_justification;
    s.dimsd1 = h.dim_suppress_line1;
    s.dimsd2 = h.dim_suppress_line2;
    s.dimtolj = h.dim_tolerance_justification;
    s.dimtzin = h.dim_tolerance_zero_suppression;
    s.dimaltz = h.dim_alt_tolerance_zero_suppression;
    s.dimalttz = h.dim_alt_tolerance_zero_tight;
    s.dimatfit = h.dim_fit;
    s.dimupt = h.dim_user_positioned_text;
    s.dimpost = h.dim_post.clone();
    s.dimapost = h.dim_alt_post.clone();
    s.dimfxl = near(h.dim_fixed_ext_line_length, s.dimfxl);
    s.dimfxlon = h.dim_fixed_ext_line_on;
    s.dimarcsym = h.dim_arc_symbol;
    s.dimjogang = near(h.dim_jog_angle, s.dimjogang);
    s.dimtfill = h.dim_text_fill;
    (s.dimtfillclr, s.dimtfillclr_true_color) = style_color(h.dim_text_fill_color);
    s.dimtxtdirection = h.dim_text_direction;
    s.dimaltmzf = near(h.dim_alt_mzf, s.dimaltmzf);
    s.dimaltmzs = h.dim_alt_mzs.clone();
    s.dimmzf = near(h.dim_mzf, s.dimmzf);
    s.dimmzs = h.dim_mzs.clone();
    s.dimlwd = h.dim_line_weight;
    s.dimlwe = h.dim_ext_line_weight;
    // A DXF header names these; a DWG gives their handles.
    let named = |handle: Handle, name: &str, find: &dyn Fn(&str) -> Option<Handle>| {
        if !handle.is_null() {
            Some(handle)
        } else if name.is_empty() {
            None
        } else {
            find(name)
        }
    };
    let text_style = |name: &str| {
        doc.text_styles
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
            .map(|t| t.handle)
    };
    let linetype = |name: &str| {
        doc.line_types
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
            .map(|t| t.handle)
    };
    let block = |name: &str| {
        doc.block_records
            .iter()
            .find(|b| b.name.eq_ignore_ascii_case(name))
            .map(|b| b.handle)
    };
    if let Some(handle) = named(h.dim_text_style_handle, &h.dim_text_style_name, &text_style) {
        s.dimtxsty_handle = handle;
        if let Some(t) = doc.text_styles.iter().find(|t| t.handle == handle) {
            s.dimtxsty = t.name.clone();
        }
    }
    for (slot, handle, name) in [
        (&mut s.dimltex_handle, h.dim_linetype_handle, &h.dim_linetype_name),
        (&mut s.dimltex1_handle, h.dim_linetype1_handle, &h.dim_linetype1_name),
        (&mut s.dimltex2_handle, h.dim_linetype2_handle, &h.dim_linetype2_name),
    ] {
        if let Some(found) = named(handle, name, &linetype) {
            *slot = found;
        }
    }
    // An empty arrow name is the default closed filled arrowhead.
    for (slot, handle, name) in [
        (&mut s.dimblk, h.dim_arrow_block_handle, &h.dim_arrow_block),
        (&mut s.dimblk1, h.dim_arrow_block1_handle, &h.dim_arrow_block1),
        (&mut s.dimblk2, h.dim_arrow_block2_handle, &h.dim_arrow_block2),
        (&mut s.dimldrblk, h.dim_leader_arrow_block_handle, &h.dim_leader_arrow_block),
    ] {
        *slot = named(handle, name, &block).unwrap_or(Handle::NULL);
    }
    s
}

/// The overrides the drawing header holds over the current dimension style
/// (the header is the current style with its overrides).
pub fn header_dim_overrides(
    doc: &Document,
) -> Option<(String, Vec<(i16, document::xdata::XDataValue)>)> {
    let current = doc.header.current_dimstyle_name.clone();
    let style = named_dim_style(doc, &current)?;
    let mut pairs =
        crate::entities::dim_override::diff_pairs(&style, &header_dim_style(doc, &style));
    // A variable a DXF header left out keeps the codec's default, which
    // stands for the current style's value, not an override.
    if let Some(carried) = &doc.header.dxf_dim_variables {
        use crate::entities::dim_override as o;
        pairs.retain(|(code, _)| {
            let name = match *code {
                o::DIMTXSTY => Some("DIMTXSTY"),
                o::DIMBLK => Some("DIMBLK"),
                o::DIMBLK1 => Some("DIMBLK1"),
                o::DIMBLK2 => Some("DIMBLK2"),
                o::DIMLDRBLK => Some("DIMLDRBLK"),
                o::DIMLTYPE => Some("DIMLTYPE"),
                o::DIMLTEX1 => Some("DIMLTEX1"),
                o::DIMLTEX2 => Some("DIMLTEX2"),
                code => o::variable_for_code(code).map(|(name, _)| name),
            };
            name.is_none_or(|name| carried.contains(&format!("${name}")))
        });
    }
    (!pairs.is_empty()).then(|| (style.name.clone(), pairs))
}

/// Writes the current dimension style with `overrides` laid over it into the
/// header, so the overrides are kept with the drawing. The drawing's DIMDLI
/// stays unless an override sets it.
pub fn stamp_header_dim_overrides(
    doc: &mut Document,
    overrides: Option<&(String, Vec<(i16, document::xdata::XDataValue)>)>,
) {
    let current = doc.header.current_dimstyle_name.clone();
    let Some(mut style) = named_dim_style(doc, &current) else {
        return;
    };
    let mut dimdli = doc.header.dim_line_increment;
    if let Some((_, pairs)) = overrides.filter(|(owner, _)| owner.eq_ignore_ascii_case(&current)) {
        crate::entities::dim_override::apply_pairs(&mut style, pairs);
        if pairs.iter().any(|(code, _)| *code == crate::entities::dim_override::DIMDLI) {
            dimdli = style.dimdli;
        }
    }
    write_header_dim_vars(doc, &style);
    doc.header.dim_line_increment = dimdli;
}

/// The drawing's DIMDLI (the header's; the current style's when the header
/// has none).
pub fn drawing_dimdli(doc: &Document) -> f64 {
    let header = doc.header.dim_line_increment;
    if header > 0.0 {
        return header;
    }
    let current = &doc.header.current_dimstyle_name;
    doc.dim_styles
        .iter()
        .find(|style| style.name.eq_ignore_ascii_case(current))
        .map_or(header, |style| style.dimdli)
}

/// A DIMDLI set for the drawing that differs from the current dimension
/// style goes on a new dimension as an override. Written before the
/// dimension is committed, so the override is part of the added object and
/// its undo step.
pub fn apply_drawing_dimdli(doc: &Document, entity: &mut EntityType) {
    if !matches!(entity, EntityType::Dimension(_)) {
        return;
    }
    let current = &doc.header.current_dimstyle_name;
    let Some(style_value) = doc
        .dim_styles
        .iter()
        .find(|style| style.name.eq_ignore_ascii_case(current))
        .map(|style| style.dimdli)
    else {
        return;
    };
    let drawing = drawing_dimdli(doc);
    if (drawing - style_value).abs() > 1.0e-9 {
        crate::entities::dim_override::set_on_entity(
            entity,
            crate::entities::dim_override::DIMDLI,
            Some(document::xdata::XDataValue::Real(drawing)),
        );
    }
}

/// The command-line dimension variables of `overrides` go on a new dimension
/// or leader as its overrides, while their style is still the current one.
pub fn apply_dim_overrides(
    doc: &Document,
    overrides: Option<&(String, Vec<(i16, document::xdata::XDataValue)>)>,
    entity: &mut EntityType,
) {
    let Some((style, values)) = overrides else {
        return;
    };
    if !matches!(entity, EntityType::Dimension(_) | EntityType::Leader(_))
        || !style.eq_ignore_ascii_case(&doc.header.current_dimstyle_name)
    {
        return;
    }
    for (code, value) in values {
        crate::entities::dim_override::set_on_entity(entity, *code, Some(value.clone()));
    }
}

pub fn apply_current_creation_styles(doc: &Document, entity: &mut EntityType) {
    entity.common_mut().transparency = doc.current_entity_transparency();
    apply_text_defaults(doc, entity);
    apply_dimension_defaults(doc, entity);
    apply_object_defaults(doc, entity);
}

pub(crate) fn transparency_options() -> Vec<String> {
    ["ByLayer".to_string(), "ByBlock".to_string()]
        .into_iter()
        .chain((0..=90).step_by(10).map(|value| value.to_string()))
        .collect()
}

pub(crate) fn parse_current_transparency(value: &str) -> Option<document::types::Transparency> {
    use document::types::Transparency;
    match value.trim().to_ascii_uppercase().as_str() {
        "BYLAYER" | "-1" => Some(Transparency::ByLayer),
        "BYBLOCK" | "-2" => Some(Transparency::ByBlock),
        value => value
            .parse::<u8>()
            .ok()
            .filter(|value| *value <= 90)
            .map(|value| Transparency::from_percent(value as f64 / 100.0)),
    }
}

pub(crate) fn current_transparency_label(value: document::types::Transparency) -> String {
    use document::types::Transparency;
    match value {
        Transparency::ByLayer => "ByLayer".into(),
        Transparency::ByBlock => "ByBlock".into(),
        Transparency::Explicit(_) => format!("{:.0}", value.as_percent() * 100.0),
    }
}

#[cfg(test)]
mod transparency_tests {
    use super::*;
    use document::types::Transparency;

    #[test]
    fn parses_supported_current_transparency_values() {
        assert_eq!(
            parse_current_transparency("ByLayer"),
            Some(Transparency::ByLayer)
        );
        assert_eq!(
            parse_current_transparency("-1"),
            Some(Transparency::ByLayer)
        );
        assert_eq!(
            parse_current_transparency("ByBlock"),
            Some(Transparency::ByBlock)
        );
        assert_eq!(
            parse_current_transparency("-2"),
            Some(Transparency::ByBlock)
        );
        assert_eq!(
            parse_current_transparency("25"),
            Some(Transparency::Explicit(64))
        );
        assert!(parse_current_transparency("91").is_none());
        assert!(parse_current_transparency("25.5").is_none());
        assert!(parse_current_transparency("invalid").is_none());
    }

    #[test]
    fn labels_transparency_for_command_and_properties_inputs() {
        assert_eq!(current_transparency_label(Transparency::ByLayer), "ByLayer");
        assert_eq!(current_transparency_label(Transparency::ByBlock), "ByBlock");
        assert_eq!(current_transparency_label(Transparency::Explicit(64)), "25");
    }
}
