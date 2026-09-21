//! Paper catalogue with the source application-compatible media names.
//!
//! the source application stores a layout's paper as the plotter driver's *canonical media
//! name* — `ISO_A4_(210.00_x_297.00_MM)`, `ANSI_B_(11.00_x_17.00_Inches)`,
//! `ARCH_full_bleed_D_(24.00_x_36.00_Inches)` — always with the portrait
//! dimensions; landscape is expressed through the plot rotation. A drawing
//! saved here has to carry those exact names so the source application finds the media in its
//! `DWG To PDF.pc3` list instead of silently falling back to a default sheet,
//! and a name that arrives from the source application (including a custom size this catalogue
//! has never seen) must still resolve to its dimensions and go back out
//! untouched. The name itself encodes the dimensions, which is what makes the
//! second half possible: [`PaperSize::parse_canonical`] never needs a table.

use std::borrow::Cow;
use std::sync::OnceLock;

/// Unit the media is defined in. the source application keeps ISO/JIS sheets in millimetres
/// and ANSI/ARCH sheets in inches, and the unit is part of the media name.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash, serde::Serialize, serde::Deserialize)]
pub enum PaperUnits {
    Millimeters,
    Inches,
}

impl PaperUnits {
    pub const MM_PER_INCH: f64 = 25.4;

    pub fn to_mm(self, value: f64) -> f64 {
        match self {
            PaperUnits::Millimeters => value,
            PaperUnits::Inches => value * Self::MM_PER_INCH,
        }
    }

    pub fn from_mm(self, value: f64) -> f64 {
        match self {
            PaperUnits::Millimeters => value,
            PaperUnits::Inches => value / Self::MM_PER_INCH,
        }
    }

    /// The unit token the source application writes inside the media name.
    fn canonical_suffix(self) -> &'static str {
        match self {
            PaperUnits::Millimeters => "MM",
            PaperUnits::Inches => "Inches",
        }
    }

    fn from_canonical_suffix(token: &str) -> Option<Self> {
        if token.eq_ignore_ascii_case("MM") {
            Some(PaperUnits::Millimeters)
        } else if token.eq_ignore_ascii_case("Inches") || token.eq_ignore_ascii_case("Inch") {
            Some(PaperUnits::Inches)
        } else {
            None
        }
    }

    /// Short unit label for display (`mm` / `in`).
    pub fn short_label(self) -> &'static str {
        match self {
            PaperUnits::Millimeters => "mm",
            PaperUnits::Inches => "in",
        }
    }
}

/// The sheet family, used to group the picker and to prefer a standard sheet
/// over its full-bleed twin when matching by dimensions.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum PaperSeries {
    IsoA,
    IsoB,
    Ansi,
    Arch,
    Jis,
    /// Letter / Legal / Tabloid under their US names, and any user-defined sheet.
    Other,
}

/// the source application offers three printable-area variants of most sheets: the standard
/// one with driver margins, *full bleed* with zero margins, and *expand*
/// (the driver's maximum printable area). The sheet dimensions are identical;
/// only the media name and the margins differ.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum PaperVariant {
    Standard,
    FullBleed,
    Expand,
}

impl PaperVariant {
    /// The infix the source application places between the series and the sheet letter.
    fn canonical_infix(self) -> &'static str {
        match self {
            PaperVariant::Standard => "",
            PaperVariant::FullBleed => "full_bleed_",
            PaperVariant::Expand => "expand_",
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Orientation {
    Portrait,
    Landscape,
}

/// Unprintable margins of a sheet, in the order the source application stores them. The unit
/// is whatever the context says — millimetres in a drawing, the sheet's own
/// unit in a user-defined size.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Margins {
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub top: f64,
}

impl Margins {
    pub const ZERO: Margins = Margins {
        left: 0.0,
        bottom: 0.0,
        right: 0.0,
        top: 0.0,
    };

    pub const fn uniform(value: f64) -> Self {
        Margins {
            left: value,
            bottom: value,
            right: value,
            top: value,
        }
    }

    /// The same margins expressed in millimetres when they were given in
    /// `units`.
    pub fn to_mm(self, units: PaperUnits) -> Self {
        Margins {
            left: units.to_mm(self.left),
            bottom: units.to_mm(self.bottom),
            right: units.to_mm(self.right),
            top: units.to_mm(self.top),
        }
    }
}

/// A sheet the user defined, kept in the application settings the way
/// the source application keeps custom sizes in a plotter configuration: a name, the
/// dimensions in the sheet's own unit, and the printable margins.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct CustomPaper {
    /// Human name, e.g. `Roll 24`; becomes `Roll_24_(…)` in the media name.
    pub name: String,
    pub width: f64,
    pub height: f64,
    pub units: PaperUnits,
    /// Unprintable margins in `units`.
    pub margins: Margins,
}

impl Default for CustomPaper {
    fn default() -> Self {
        Self {
            name: String::new(),
            width: 0.0,
            height: 0.0,
            units: PaperUnits::Millimeters,
            margins: Margins::ZERO,
        }
    }
}

impl CustomPaper {
    /// The catalogue sheet this definition describes.
    pub fn paper(&self) -> PaperSize {
        PaperSize::custom_named(&self.name, self.width, self.height, self.units)
    }

    pub fn canonical(&self) -> String {
        self.paper().canonical.into_owned()
    }

    pub fn margins_mm(&self) -> Margins {
        self.margins.to_mm(self.units)
    }
}

/// One sheet of the catalogue, or a sheet reconstructed from a media name.
#[derive(Clone, PartialEq, Debug)]
pub struct PaperSize {
    /// the source application canonical media name, e.g. `ISO_A4_(210.00_x_297.00_MM)`. This is
    /// the value written to `PlotSettings::paper_size` and the key the plot
    /// dialog stores.
    pub canonical: Cow<'static, str>,
    /// Human name without dimensions: `ISO A4`, `ANSI full bleed B`.
    pub label: Cow<'static, str>,
    pub series: PaperSeries,
    pub variant: PaperVariant,
    /// Portrait width in `units` (width ≤ height).
    pub width: f64,
    /// Portrait height in `units`.
    pub height: f64,
    pub units: PaperUnits,
}

impl PaperSize {
    /// Portrait dimensions in millimetres.
    pub fn portrait_mm(&self) -> (f64, f64) {
        (self.units.to_mm(self.width), self.units.to_mm(self.height))
    }

    /// Sheet dimensions in millimetres for the given orientation.
    pub fn sheet_mm(&self, orientation: Orientation) -> (f64, f64) {
        let (w, h) = self.portrait_mm();
        match orientation {
            Orientation::Portrait => (w, h),
            Orientation::Landscape => (h, w),
        }
    }

    /// Picker text: the human name followed by the dimensions in the sheet's
    /// own unit — `ISO A4 (210 × 297 mm)`, `ANSI B (11.00 × 17.00 in)`.
    pub fn display(&self) -> String {
        format!(
            "{} ({} × {} {})",
            self.label,
            format_dimension(self.width, self.units),
            format_dimension(self.height, self.units),
            self.units.short_label()
        )
    }

    /// A user-defined sheet, named the way the source application names custom media created
    /// in the plot dialog (`UserDefinedMetric_(420.00_x_297.00_MM)`). The
    /// dimensions are normalised to portrait so the name stays canonical.
    pub fn custom(width: f64, height: f64, units: PaperUnits) -> Self {
        let label = match units {
            PaperUnits::Millimeters => "UserDefinedMetric",
            PaperUnits::Inches => "UserDefinedInches",
        };
        Self::custom_named(label, width, height, units)
    }

    /// A user-defined sheet under its own name, the way a plotter
    /// configuration's custom sizes are named (`Roll_24_(609.60_x_1500.00_MM)`).
    /// A blank name falls back to the source application's generic one; spaces become
    /// underscores in the media name and parentheses are dropped so the name
    /// stays parseable.
    pub fn custom_named(name: &str, width: f64, height: f64, units: PaperUnits) -> Self {
        let (width, height) = if width <= height {
            (width, height)
        } else {
            (height, width)
        };
        let cleaned: String = name
            .trim()
            .chars()
            .filter(|c| !matches!(c, '(' | ')'))
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if cleaned.is_empty() {
            return Self::custom(width, height, units);
        }
        Self {
            canonical: Cow::Owned(canonical_name(
                &cleaned.replace(' ', "_"),
                width,
                height,
                units,
            )),
            label: Cow::Owned(cleaned),
            series: PaperSeries::Other,
            variant: PaperVariant::Standard,
            width,
            height,
            units,
        }
    }

    /// Rebuild a sheet from any media name that follows the source application's
    /// `<name>_(<width>_x_<height>_<MM|Inches>)` convention, whether or not the
    /// catalogue knows it. The dialog form with spaces (`ISO A4 (210.00 x
    /// 297.00 MM)`) is accepted too. Returns `None` for names without an
    /// embedded dimension group (a bare printer paper name such as `A4`, or a
    /// raster `Pixels` size).
    pub fn parse_canonical(name: &str) -> Option<Self> {
        let name = name.trim();
        let open = name.rfind('(')?;
        let close = name[open..].find(')')? + open;
        let dims = name[open + 1..close].trim_matches(['_', ' ']);
        let raw_label = name[..open].trim_matches(['_', ' ']);
        if raw_label.is_empty() {
            return None;
        }
        // `210.00_x_297.00_MM`, or `210.00 x 297.00 MM` from a dialog string.
        let normalized: String = dims.replace(' ', "_");
        let mut parts = normalized.split('_').filter(|part| !part.is_empty());
        let width: f64 = parts.next()?.parse().ok()?;
        if !parts.next()?.eq_ignore_ascii_case("x") {
            return None;
        }
        let height: f64 = parts.next()?.parse().ok()?;
        let units = PaperUnits::from_canonical_suffix(parts.next()?)?;
        if parts.next().is_some()
            || width <= 0.0
            || height <= 0.0
            || !width.is_finite()
            || !height.is_finite()
        {
            return None;
        }
        let label = raw_label.replace('_', " ");
        let (series, variant) = classify_label(&label);
        let (portrait_width, portrait_height) = if width <= height {
            (width, height)
        } else {
            (height, width)
        };
        Some(Self {
            canonical: Cow::Owned(canonical_name(
                &raw_label.replace(' ', "_"),
                width,
                height,
                units,
            )),
            label: Cow::Owned(label),
            series,
            variant,
            width: portrait_width,
            height: portrait_height,
            units,
        })
    }
}

fn format_dimension(value: f64, units: PaperUnits) -> String {
    match units {
        // Millimetre sheets are whole numbers in every standard; keep the
        // fraction only when a custom size needs it.
        PaperUnits::Millimeters if (value - value.round()).abs() < 1e-6 => {
            format!("{}", value.round() as i64)
        }
        PaperUnits::Millimeters => format!("{value:.1}"),
        PaperUnits::Inches => format!("{value:.2}"),
    }
}

/// `<prefix>_(<w>_x_<h>_<units>)` exactly as the source application spells it: two decimals,
/// portrait order.
fn canonical_name(prefix: &str, width: f64, height: f64, units: PaperUnits) -> String {
    format!(
        "{prefix}_({width:.2}_x_{height:.2}_{})",
        units.canonical_suffix()
    )
}

/// Series and variant from a human label (`ISO full bleed A4`, `ARCH expand D`).
fn classify_label(label: &str) -> (PaperSeries, PaperVariant) {
    let lower = label.to_ascii_lowercase();
    let variant = if lower.contains("full bleed") {
        PaperVariant::FullBleed
    } else if lower.contains("expand") {
        PaperVariant::Expand
    } else {
        PaperVariant::Standard
    };
    let series = if lower.starts_with("iso b") {
        PaperSeries::IsoB
    } else if lower.starts_with("iso") {
        PaperSeries::IsoA
    } else if lower.starts_with("ansi") {
        PaperSeries::Ansi
    } else if lower.starts_with("arch") {
        PaperSeries::Arch
    } else if lower.starts_with("jis") {
        PaperSeries::Jis
    } else {
        PaperSeries::Other
    };
    (series, variant)
}

// ── Catalogue ─────────────────────────────────────────────────────────────

/// Compact source row: series prefix as the source application writes it, sheet letter,
/// portrait width/height, units, and whether the source application ships full-bleed /
/// expand twins of it in `DWG To PDF.pc3`.
struct Row {
    series: PaperSeries,
    prefix: &'static str,
    sheet: &'static str,
    width: f64,
    height: f64,
    units: PaperUnits,
    twins: bool,
}

const fn mm(
    series: PaperSeries,
    prefix: &'static str,
    sheet: &'static str,
    w: f64,
    h: f64,
    twins: bool,
) -> Row {
    Row {
        series,
        prefix,
        sheet,
        width: w,
        height: h,
        units: PaperUnits::Millimeters,
        twins,
    }
}

const fn inch(
    series: PaperSeries,
    prefix: &'static str,
    sheet: &'static str,
    w: f64,
    h: f64,
    twins: bool,
) -> Row {
    Row {
        series,
        prefix,
        sheet,
        width: w,
        height: h,
        units: PaperUnits::Inches,
        twins,
    }
}

/// The sheets the source application's `DWG To PDF.pc3` offers, in its display order, plus
/// the ISO B / JIS B series that other drivers add and the US names system
/// printers report.
const ROWS: &[Row] = &[
    mm(PaperSeries::IsoA, "ISO", "A0", 841.0, 1189.0, true),
    mm(PaperSeries::IsoA, "ISO", "A1", 594.0, 841.0, true),
    mm(PaperSeries::IsoA, "ISO", "A2", 420.0, 594.0, true),
    mm(PaperSeries::IsoA, "ISO", "A3", 297.0, 420.0, true),
    mm(PaperSeries::IsoA, "ISO", "A4", 210.0, 297.0, true),
    mm(PaperSeries::IsoA, "ISO", "A5", 148.0, 210.0, false),
    mm(PaperSeries::IsoB, "ISO", "B0", 1000.0, 1414.0, false),
    mm(PaperSeries::IsoB, "ISO", "B1", 707.0, 1000.0, false),
    mm(PaperSeries::IsoB, "ISO", "B2", 500.0, 707.0, false),
    mm(PaperSeries::IsoB, "ISO", "B3", 353.0, 500.0, false),
    mm(PaperSeries::IsoB, "ISO", "B4", 250.0, 353.0, false),
    mm(PaperSeries::IsoB, "ISO", "B5", 176.0, 250.0, false),
    inch(PaperSeries::Ansi, "ANSI", "A", 8.5, 11.0, true),
    inch(PaperSeries::Ansi, "ANSI", "B", 11.0, 17.0, true),
    inch(PaperSeries::Ansi, "ANSI", "C", 17.0, 22.0, true),
    inch(PaperSeries::Ansi, "ANSI", "D", 22.0, 34.0, true),
    inch(PaperSeries::Ansi, "ANSI", "E", 34.0, 44.0, true),
    inch(PaperSeries::Arch, "ARCH", "A", 9.0, 12.0, true),
    inch(PaperSeries::Arch, "ARCH", "B", 12.0, 18.0, true),
    inch(PaperSeries::Arch, "ARCH", "C", 18.0, 24.0, true),
    inch(PaperSeries::Arch, "ARCH", "D", 24.0, 36.0, true),
    inch(PaperSeries::Arch, "ARCH", "E", 36.0, 48.0, true),
    inch(PaperSeries::Arch, "ARCH", "E1", 30.0, 42.0, true),
    mm(PaperSeries::Jis, "JIS", "B0", 1030.0, 1456.0, false),
    mm(PaperSeries::Jis, "JIS", "B1", 728.0, 1030.0, false),
    mm(PaperSeries::Jis, "JIS", "B2", 515.0, 728.0, false),
    mm(PaperSeries::Jis, "JIS", "B3", 364.0, 515.0, false),
    mm(PaperSeries::Jis, "JIS", "B4", 257.0, 364.0, false),
    mm(PaperSeries::Jis, "JIS", "B5", 182.0, 257.0, false),
    inch(PaperSeries::Other, "Letter", "", 8.5, 11.0, false),
    inch(PaperSeries::Other, "Legal", "", 8.5, 14.0, false),
    inch(PaperSeries::Other, "Tabloid", "", 11.0, 17.0, false),
];

fn build_catalog() -> Vec<PaperSize> {
    let mut sizes = Vec::with_capacity(ROWS.len() * 3);
    for row in ROWS {
        let variants: &[PaperVariant] = if row.twins {
            &[
                PaperVariant::Standard,
                PaperVariant::FullBleed,
                PaperVariant::Expand,
            ]
        } else {
            &[PaperVariant::Standard]
        };
        for &variant in variants {
            // `ISO_full_bleed_A4`, `ANSI_expand_B`, `Letter`.
            let prefix = if row.sheet.is_empty() {
                row.prefix.to_string()
            } else {
                format!("{}_{}{}", row.prefix, variant.canonical_infix(), row.sheet)
            };
            sizes.push(PaperSize {
                canonical: Cow::Owned(canonical_name(&prefix, row.width, row.height, row.units)),
                label: Cow::Owned(prefix.replace('_', " ")),
                series: row.series,
                variant,
                width: row.width,
                height: row.height,
                units: row.units,
            });
        }
    }
    sizes
}

/// Every catalogued sheet in picker order.
pub fn catalog() -> &'static [PaperSize] {
    static CATALOG: OnceLock<Vec<PaperSize>> = OnceLock::new();
    CATALOG.get_or_init(build_catalog)
}

/// The sheet a fresh drawing or a page setup without one falls back to.
pub fn default_paper() -> &'static PaperSize {
    catalog()
        .iter()
        .find(|paper| paper.canonical == "ISO_A4_(210.00_x_297.00_MM)")
        .expect("catalogue always carries ISO A4")
}

/// Short names a system printer driver, an older Open CAD Studio config or a
/// user at the command line may spell a sheet with, mapped onto the canonical
/// sheet. `Ledger` is the landscape name of Tabloid; the sheet is the same.
const ALIASES: &[(&str, &str)] = &[
    ("A0", "ISO_A0_(841.00_x_1189.00_MM)"),
    ("A1", "ISO_A1_(594.00_x_841.00_MM)"),
    ("A2", "ISO_A2_(420.00_x_594.00_MM)"),
    ("A3", "ISO_A3_(297.00_x_420.00_MM)"),
    ("A4", "ISO_A4_(210.00_x_297.00_MM)"),
    ("A5", "ISO_A5_(148.00_x_210.00_MM)"),
    ("B4", "ISO_B4_(250.00_x_353.00_MM)"),
    ("B5", "ISO_B5_(176.00_x_250.00_MM)"),
    ("Letter", "Letter_(8.50_x_11.00_Inches)"),
    ("US Letter", "Letter_(8.50_x_11.00_Inches)"),
    ("Legal", "Legal_(8.50_x_14.00_Inches)"),
    ("US Legal", "Legal_(8.50_x_14.00_Inches)"),
    ("Tabloid", "Tabloid_(11.00_x_17.00_Inches)"),
    ("Ledger", "Tabloid_(11.00_x_17.00_Inches)"),
];

/// Resolve any spelling of a sheet: an exact canonical name (case-insensitive,
/// underscores or spaces), a driver/config alias such as `A4` or `Letter`, or
/// an unknown but well-formed the source application name, which is reconstructed from its
/// embedded dimensions. `None` only for names that carry no size at all.
pub fn resolve(name: &str) -> Option<PaperSize> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let spaced = name.replace('_', " ");
    if let Some(paper) = catalog().iter().find(|paper| {
        paper
            .canonical
            .replace('_', " ")
            .eq_ignore_ascii_case(&spaced)
    }) {
        return Some(paper.clone());
    }
    if let Some((_, canonical)) = ALIASES
        .iter()
        .find(|(alias, _)| alias.eq_ignore_ascii_case(name))
    {
        return catalog()
            .iter()
            .find(|paper| paper.canonical == *canonical)
            .cloned();
    }
    PaperSize::parse_canonical(name)
}

/// The catalogued sheet whose portrait size matches `(w, h)` in millimetres
/// (either order) within half a millimetre. Standard variants win over their
/// full-bleed / expand twins so a size read from a drawing without a usable
/// name lands on the plain sheet.
pub fn match_dimensions_mm(w: f64, h: f64) -> Option<&'static PaperSize> {
    const TOLERANCE_MM: f64 = 0.5;
    let (short, long) = if w <= h { (w, h) } else { (h, w) };
    catalog()
        .iter()
        .filter(|paper| paper.variant == PaperVariant::Standard)
        .filter(|paper| {
            let (pw, ph) = paper.portrait_mm();
            (pw - short).abs() <= TOLERANCE_MM && (ph - long).abs() <= TOLERANCE_MM
        })
        // ANSI A and Letter share a size; prefer the named series.
        .min_by_key(|paper| paper.series)
}

/// Sheet for a name as read from a drawing: the catalogued or reconstructed
/// sheet when the name carries one, otherwise the catalogued sheet with the
/// given dimensions, otherwise a custom sheet built from the dimensions alone.
pub fn from_drawing(name: &str, width_mm: f64, height_mm: f64) -> PaperSize {
    if let Some(paper) = resolve(name) {
        return paper;
    }
    if let Some(paper) = match_dimensions_mm(width_mm, height_mm) {
        return paper.clone();
    }
    if !width_mm.is_finite() || !height_mm.is_finite() || width_mm <= 0.0 || height_mm <= 0.0 {
        return default_paper().clone();
    }
    PaperSize::custom(width_mm, height_mm, PaperUnits::Millimeters)
}

// ── Plot geometry helpers ─────────────────────────────────────────────────

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum PlotScale {
    /// Scale so the window fills the sheet minus a 5% margin.
    Fit,
    /// Exact scale factor (mm per drawing unit).
    Ratio(f64),
}

/// Map a world-space window onto a sheet. Returns (scale, offset_x, offset_y),
/// where the scaled window is centered on the sheet and (offset_x, offset_y) is
/// the sheet-mm position of the window's min corner. The caller turns that into
/// its own coordinate offset.
pub fn window_to_sheet(
    window_wh: (f64, f64),
    sheet_mm: (f64, f64),
    scale: PlotScale,
) -> (f64, f64, f64) {
    let (ww, wh) = (window_wh.0.max(1e-9), window_wh.1.max(1e-9));
    let scale = match scale {
        PlotScale::Ratio(r) => r.max(1e-9),
        PlotScale::Fit => {
            const MARGIN: f64 = 1.05;
            let sx = (sheet_mm.0 / MARGIN) / ww;
            let sy = (sheet_mm.1 / MARGIN) / wh;
            sx.min(sy)
        }
    };
    let scaled_w = ww * scale;
    let scaled_h = wh * scale;
    let offset_x = (sheet_mm.0 - scaled_w) / 2.0;
    let offset_y = (sheet_mm.1 - scaled_h) / 2.0;
    (scale, offset_x, offset_y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a4() -> PaperSize {
        resolve("ISO_A4_(210.00_x_297.00_MM)").unwrap()
    }

    #[test]
    fn catalogue_names_follow_source_app_spelling() {
        let names: Vec<&str> = catalog().iter().map(|p| p.canonical.as_ref()).collect();
        for expected in [
            "ISO_A4_(210.00_x_297.00_MM)",
            "ISO_full_bleed_A4_(210.00_x_297.00_MM)",
            "ISO_expand_A0_(841.00_x_1189.00_MM)",
            "ISO_B4_(250.00_x_353.00_MM)",
            "ANSI_A_(8.50_x_11.00_Inches)",
            "ANSI_full_bleed_B_(11.00_x_17.00_Inches)",
            "ARCH_D_(24.00_x_36.00_Inches)",
            "ARCH_expand_E1_(30.00_x_42.00_Inches)",
            "JIS_B4_(257.00_x_364.00_MM)",
            "Letter_(8.50_x_11.00_Inches)",
            "Legal_(8.50_x_14.00_Inches)",
            "Tabloid_(11.00_x_17.00_Inches)",
        ] {
            assert!(names.contains(&expected), "missing {expected}");
        }
        // Every name is portrait and unique.
        let mut seen = std::collections::HashSet::new();
        for paper in catalog() {
            assert!(
                paper.width <= paper.height,
                "{} is not portrait",
                paper.canonical
            );
            assert!(
                seen.insert(paper.canonical.clone()),
                "duplicate {}",
                paper.canonical
            );
        }
    }

    #[test]
    fn every_catalogue_name_round_trips_through_the_parser() {
        for paper in catalog() {
            let parsed = PaperSize::parse_canonical(&paper.canonical)
                .unwrap_or_else(|| panic!("cannot parse {}", paper.canonical));
            assert_eq!(parsed.canonical, paper.canonical);
            assert_eq!(parsed.label, paper.label);
            assert_eq!(parsed.series, paper.series);
            assert_eq!(parsed.variant, paper.variant);
            assert_eq!(parsed.units, paper.units);
            assert!((parsed.width - paper.width).abs() < 1e-9);
            assert!((parsed.height - paper.height).abs() < 1e-9);
        }
    }

    #[test]
    fn unknown_canonical_names_are_reconstructed_and_preserved() {
        // A custom size created in a plot dialog.
        let custom = resolve("UserDefinedMetric_(420.00_x_297.00_MM)").unwrap();
        assert_eq!(custom.canonical, "UserDefinedMetric_(420.00_x_297.00_MM)");
        assert_eq!(custom.label, "UserDefinedMetric");
        assert_eq!(custom.series, PaperSeries::Other);
        assert_eq!(custom.portrait_mm(), (297.0, 420.0));
        // A driver sheet the catalogue does not list, in inches.
        let oversize = resolve("Oversize_A1_(625.00_x_880.00_MM)").unwrap();
        assert_eq!(oversize.portrait_mm(), (625.0, 880.0));
        // The dialog spelling with spaces resolves to the same canonical name.
        let spaced = resolve("ISO full bleed A3 (297.00 x 420.00 MM)").unwrap();
        assert_eq!(spaced.canonical, "ISO_full_bleed_A3_(297.00_x_420.00_MM)");
        assert_eq!(spaced.variant, PaperVariant::FullBleed);
        assert_eq!(spaced.series, PaperSeries::IsoA);
        // A raster size or a bare driver name carries no usable dimensions.
        assert!(resolve("Sun_Hi-Res_(1600.00_x_1280.00_Pixels)").is_none());
        assert!(resolve("A4 Plain").is_none());
        assert!(resolve("").is_none());
    }

    #[test]
    fn aliases_and_case_insensitive_names_resolve() {
        assert_eq!(resolve("A4").unwrap().canonical, a4().canonical);
        assert_eq!(
            resolve("a3").unwrap().canonical,
            "ISO_A3_(297.00_x_420.00_MM)"
        );
        assert_eq!(
            resolve("Letter").unwrap().canonical,
            "Letter_(8.50_x_11.00_Inches)"
        );
        assert_eq!(
            resolve("Ledger").unwrap().canonical,
            "Tabloid_(11.00_x_17.00_Inches)"
        );
        assert_eq!(
            resolve("iso_a4_(210.00_x_297.00_mm)").unwrap().canonical,
            a4().canonical
        );
    }

    #[test]
    fn dimensions_match_the_plain_sheet_in_either_orientation() {
        assert_eq!(
            match_dimensions_mm(297.0, 210.0).unwrap().canonical,
            a4().canonical
        );
        assert_eq!(
            match_dimensions_mm(210.0, 297.0).unwrap().canonical,
            a4().canonical
        );
        assert_eq!(
            match_dimensions_mm(210.3, 297.4).unwrap().canonical,
            a4().canonical
        );
        assert!(match_dimensions_mm(215.0, 297.0).is_none());
        // 8.5 × 11 in prefers ANSI A over the Letter alias sheet.
        assert_eq!(
            match_dimensions_mm(215.9, 279.4).unwrap().canonical,
            "ANSI_A_(8.50_x_11.00_Inches)"
        );
        assert_eq!(
            match_dimensions_mm(609.6, 914.4).unwrap().canonical,
            "ARCH_D_(24.00_x_36.00_Inches)"
        );
    }

    #[test]
    fn from_drawing_falls_back_from_name_to_size_to_custom() {
        assert_eq!(
            from_drawing("ISO_A3_(297.00_x_420.00_MM)", 0.0, 0.0).label,
            "ISO A3"
        );
        // A legacy drawing with a bare driver name and A4 dimensions.
        assert_eq!(
            from_drawing("Plain paper", 297.0, 210.0).canonical,
            a4().canonical
        );
        let custom = from_drawing("", 300.0, 500.0);
        assert_eq!(custom.canonical, "UserDefinedMetric_(300.00_x_500.00_MM)");
        assert_eq!(custom.portrait_mm(), (300.0, 500.0));
        assert_eq!(from_drawing("", 0.0, f64::INFINITY), *default_paper());
    }

    #[test]
    fn parser_rejects_non_finite_dimensions() {
        assert!(PaperSize::parse_canonical("Custom_(inf_x_297.00_MM)").is_none());
        assert!(PaperSize::parse_canonical("Custom_(210.00_x_NaN_MM)").is_none());
    }

    #[test]
    fn sheet_and_display_formatting() {
        assert_eq!(a4().sheet_mm(Orientation::Portrait), (210.0, 297.0));
        assert_eq!(a4().sheet_mm(Orientation::Landscape), (297.0, 210.0));
        assert_eq!(a4().display(), "ISO A4 (210 × 297 mm)");
        let ansi_b = resolve("ANSI_B_(11.00_x_17.00_Inches)").unwrap();
        assert_eq!(ansi_b.display(), "ANSI B (11.00 × 17.00 in)");
        let (w, h) = ansi_b.portrait_mm();
        assert!((w - 279.4).abs() < 1e-9 && (h - 431.8).abs() < 1e-9);
        assert_eq!(
            PaperSize::custom(297.5, 210.0, PaperUnits::Millimeters).display(),
            "UserDefinedMetric (210 × 297.5 mm)"
        );
        assert_eq!(default_paper().canonical, a4().canonical);
    }

    #[test]
    fn user_defined_sheets_carry_their_name_and_margins() {
        let roll = CustomPaper {
            name: "Roll 24 (long)".into(),
            width: 1500.0,
            height: 609.6,
            units: PaperUnits::Millimeters,
            margins: Margins {
                left: 5.0,
                bottom: 17.0,
                right: 6.0,
                top: 18.0,
            },
        };
        assert_eq!(roll.canonical(), "Roll_24_long_(609.60_x_1500.00_MM)");
        assert_eq!(roll.paper().label, "Roll 24 long");
        assert_eq!(roll.paper().display(), "Roll 24 long (609.6 × 1500 mm)");
        // Round-trips through the parser like any the source application name.
        assert_eq!(
            resolve(&roll.canonical()).unwrap().portrait_mm(),
            (609.6, 1500.0)
        );
        let arch = CustomPaper {
            name: String::new(),
            width: 30.0,
            height: 42.0,
            units: PaperUnits::Inches,
            margins: Margins::uniform(0.25),
        };
        assert_eq!(arch.canonical(), "UserDefinedInches_(30.00_x_42.00_Inches)");
        assert_eq!(arch.margins_mm(), Margins::uniform(6.35));
    }

    #[test]
    fn fit_centers_and_scales_within_margin() {
        // 100×100 window onto a 210×297 sheet, Fit. Limiting axis is width:
        // usable = 210/1.05 = 200; scale = 200/100 = 2.0.
        let (s, ox, oy) = window_to_sheet((100.0, 100.0), (210.0, 297.0), PlotScale::Fit);
        assert!((s - 2.0).abs() < 1e-9, "scale {s}");
        // Window is 100*2 = 200 wide/tall; centered on 210×297.
        assert!((ox - (210.0 - 200.0) / 2.0).abs() < 1e-9, "ox {ox}");
        assert!((oy - (297.0 - 200.0) / 2.0).abs() < 1e-9, "oy {oy}");
    }

    #[test]
    fn ratio_applies_exact_scale_and_centers() {
        // Ratio(0.5): scale is exactly 0.5; the scaled window is centered and
        // (ox, oy) is the sheet-mm position of its min corner.
        let (s, ox, oy) = window_to_sheet((100.0, 80.0), (210.0, 297.0), PlotScale::Ratio(0.5));
        assert!((s - 0.5).abs() < 1e-9);
        // scaled window = 50×40; centered box origin = ((210-50)/2,(297-40)/2).
        assert!((ox - (210.0 - 50.0) / 2.0).abs() < 1e-9, "ox {ox}");
        assert!((oy - (297.0 - 40.0) / 2.0).abs() < 1e-9, "oy {oy}");
    }
}
