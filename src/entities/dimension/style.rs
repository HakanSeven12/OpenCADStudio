// Dimension style resolution and measurement display.

use super::*;

/// The style a dimension draws with: its style's family member for the
/// dimension's type when the drawing has one (`Name$0` linear and aligned,
/// `$2` angular, `$3` diameter, `$4` radius, `$6` ordinate), else the style
/// itself; a dimension without a style name uses Standard.
pub(crate) fn dimension_source_style<'a>(
    document: &'a Document,
    dim: &Dimension,
) -> Option<&'a DimStyle> {
    let name = dim.base().style_name.trim();
    let name = if name.is_empty() { "Standard" } else { name };
    let find = |wanted: &str| {
        document
            .dim_styles
            .iter()
            .find(|style| style.name.eq_ignore_ascii_case(wanted))
    };
    let suffix = match dim {
        Dimension::Linear(_) | Dimension::Aligned(_) => Some("$0"),
        Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_) => Some("$2"),
        Dimension::Diameter(_) => Some("$3"),
        Dimension::Radius(_) | Dimension::LargeRadial(_) => Some("$4"),
        Dimension::Ordinate(_) => Some("$6"),
        _ => None,
    };
    suffix
        .and_then(|suffix| find(&format!("{name}{suffix}")))
        .or_else(|| find(name))
}

pub(crate) fn resolved_dimension_style(
    source: &DimStyle,
    dimension: &Dimension,
    document: &Document,
) -> DimStyle {
    use crate::entities::dim_override as ov;

    let mut style = source.clone();
    let data = &dimension.base().common.extended_data;
    ov::apply_pairs(&mut style, &ov::pairs(data));
    if let Some(value) = ov::handle(data, ov::DIMTXSTY) {
        if let Some(record) = document
            .text_styles
            .iter()
            .find(|record| record.handle == value)
        {
            style.dimtxsty = record.name.clone();
        }
    }
    // A text style with a fixed height sizes the dimension text instead of
    // DIMTXT (#3116).
    if let Some(record) = document
        .text_styles
        .iter()
        .find(|record| record.name.eq_ignore_ascii_case(&style.dimtxsty))
        .filter(|record| record.has_fixed_height())
    {
        style.dimtxt = record.height;
    }

    // The dimension's own annotation context decides whether it scales with
    // the annotation scale; the style only seeds new dimensions. A dimension
    // turned non-annotative under an annotative style uses its DIMSCALE.
    // An arc length dimension carries no context, so its style decides.
    // (#1917)
    if !matches!(dimension, Dimension::Arc(_)) {
        style.annotative =
            crate::scene::annotative::object_is_annotative(document, &dimension.base().common);
    }
    // An annotative dimension sizes its features by the annotation scale; the
    // DIMSCALE it still stores is inert.
    if style.annotative {
        style.dimscale = 0.0;
    }

    // Negative DIMLFAC applies only in paper space, and there only to a
    // dimension that measures model space through a viewport. The associative
    // record stores the applied factor; zero marks paper-space geometry whose
    // dimension reads its sheet distance.
    if style.dimlfac < 0.0 {
        style.dimlfac = match calculated_dimlfac(dimension) {
            Some(calculated) if calculated.abs() < 1e-12 => 1.0,
            Some(calculated) => calculated.abs(),
            None => crate::scene::viewport_ref::MeasurementScale::user_lfac_for_space(
                style.dimlfac,
                dimension_in_paper_space(dimension, document),
            ),
        };
    }
    style
}

/// The linear factor recorded when the dimension was last regenerated.
pub(crate) fn calculated_dimlfac(dimension: &Dimension) -> Option<f64> {
    dimension
        .base()
        .common
        .extended_data
        .get_record("ACAD_DIMASSOC_CALC_DIMLFAC")
        .and_then(|record| {
            record.values.iter().find_map(|value| match value {
                document::xdata::XDataValue::Real(factor) if factor.is_finite() => Some(*factor),
                _ => None,
            })
        })
}

/// Whether the dimension's owner is paper space, even without a Layout object.
pub(crate) fn dimension_in_paper_space(dimension: &Dimension, document: &Document) -> bool {
    let owner = dimension.base().common.owner_handle;
    if !owner.is_valid() {
        return false;
    }
    document.objects.values().any(|object| {
        matches!(object,
            document::objects::ObjectType::Layout(layout)
                if layout.name != "Model" && layout.block_record == owner
        )
    }) || document.block_records.iter().any(|block| {
        block.handle == owner && block.name.to_ascii_uppercase().starts_with("*PAPER_SPACE")
    })
}

/// The value a dimension's text reports for its measurement: the raw
/// geometric measurement times the resolved linear factor. Angular dimensions
/// never scale.
///
/// `actual_measurement` (DXF group 42) stays **raw** — that is what the format
/// specifies and what export writes — so this is the one helper UI surfaces
/// use when they want the number the user reads.
pub(crate) fn displayed_measurement(dimension: &Dimension, style: Option<&DimStyle>) -> f64 {
    let raw = dimension.measurement();
    if matches!(
        dimension,
        Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_)
    ) {
        return raw;
    }
    let lfac = style.map(|s| s.dimlfac).unwrap_or(1.0);
    if lfac.abs() < 1e-12 {
        raw
    } else {
        raw * lfac
    }
}
