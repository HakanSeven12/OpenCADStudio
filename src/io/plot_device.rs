//! Plot devices: what a page setup names as its plotter, and what that device
//! can print.
//!
//! the source application stores the device in `PlotSettings::printer_name` as the plotter
//! configuration's name — `DWG To PDF.pc3` for its PDF driver, the Windows
//! printer's name for a system printer, `None` when nothing is assigned. A
//! drawing saved here has to use the same spelling so the source application attaches the
//! layout to its own PDF driver instead of reporting a missing device; and a
//! device name that arrives from the source application (an HP plotter's `.pc3`, `DWF6
//! ePlot.pc3`) must go back out untouched even though we cannot drive it.
//!
//! The device also decides the *printable area*: the sheet inset by the
//! driver's unprintable margins, which the source application writes into the layout and
//! draws as the dashed rectangle in paper space. The PDF driver's margins are
//! a fixed table; a CUPS printer reports its media and margins over IPP
//! (`media-col-database`), which is queried once per printer and cached.

pub use crate::io::paper_catalog::Margins;
use crate::io::paper_catalog::{self, CustomPaper, PaperSize, PaperUnits, PaperVariant};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// the source application's plotter configuration name for PDF output.
pub const PDF_DEVICE_NAME: &str = "DWG To PDF.pc3";
/// the source application's spelling for "no plotter assigned" in the plot dialog…
pub const NO_DEVICE_NAME: &str = "None";
/// …and the name it actually stores in the drawing for that choice.
const NO_DEVICE_STORED_NAME: &str = "none_device";
/// The label older Open CAD Studio builds wrote into drawings for PDF output.
/// Recognised on read so those drawings keep plotting to PDF, and always
/// rewritten with [`PDF_DEVICE_NAME`] on save.
const LEGACY_PDF_LABEL: &str = "Save to PDF file…";
const WINDOWS_PDF_PRINTER: &str = "Microsoft Print to PDF";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlotDevice {
    /// No plotter assigned (the source application `None`). We plot to the system default
    /// printer in that case, which is what the plot dialog offers.
    None,
    /// PDF file output, the source application's `DWG To PDF.pc3`.
    Pdf,
    /// A system printer by its queue / spooler name.
    Printer(String),
}

impl PlotDevice {
    /// The name the source application would store for this device.
    pub fn canonical_name(&self) -> String {
        match self {
            PlotDevice::None => NO_DEVICE_STORED_NAME.to_string(),
            PlotDevice::Pdf => PDF_DEVICE_NAME.to_string(),
            PlotDevice::Printer(name) => name.clone(),
        }
    }

    /// Interpret a stored `printer_name`. Returns the device and whether the
    /// stored spelling should be preserved on save: an the source application plotter
    /// configuration we cannot drive (`DWF6 ePlot.pc3`, an HP `.pc3`) is
    /// treated as PDF output but keeps its name, so the drawing still attaches
    /// to that plotter in the source application; our own legacy label and an empty name are
    /// not worth preserving.
    pub fn from_stored_name(name: &str) -> (Self, bool) {
        let name = name.trim();
        if name.is_empty() || name.eq_ignore_ascii_case(NO_DEVICE_NAME) {
            return (PlotDevice::None, false);
        }
        if name.eq_ignore_ascii_case(NO_DEVICE_STORED_NAME) {
            // the source application's own spelling: keep it rather than rewriting it.
            return (PlotDevice::None, true);
        }
        if name == LEGACY_PDF_LABEL || name.eq_ignore_ascii_case(WINDOWS_PDF_PRINTER) {
            return (PlotDevice::Pdf, false);
        }
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".pc3") {
            // Any the source application plotter configuration: PDF ones are ours to drive,
            // the rest plot to PDF here but keep their name for the source application.
            return (PlotDevice::Pdf, true);
        }
        (PlotDevice::Printer(name.to_string()), true)
    }

    /// Unprintable margins the device imposes on `paper`, in millimetres.
    /// A sheet the user defined carries its own margins for every device,
    /// the way a custom size in a plotter configuration does; a printer that
    /// reports the same size still wins, since it knows its hardware.
    pub fn margins_mm(&self, paper: &PaperSize, custom: &[CustomPaper]) -> Margins {
        let user_defined = custom
            .iter()
            .find(|c| c.canonical() == paper.canonical)
            .map(CustomPaper::margins_mm);
        match self {
            PlotDevice::Pdf => user_defined.unwrap_or_else(|| pdf_margins_mm(paper)),
            PlotDevice::Printer(name) => cached_printer_capabilities(name)
                .and_then(|caps| caps.margins_for(paper))
                .or(user_defined)
                .unwrap_or(Margins::ZERO),
            // No device: the source application keeps the margins of the last device or its
            // defaults; without one we show the full sheet as printable.
            PlotDevice::None => user_defined.unwrap_or(Margins::ZERO),
        }
    }
}

// ── PDF driver ────────────────────────────────────────────────────────────

/// Unprintable margins of the source application's `DWG To PDF.pc3` sheets, as its Plotter
/// Configuration Editor lists them (Modify Standard Paper Sizes). They depend
/// on the sheet family and variant, not on the individual size: every ISO
/// sheet shares one set in millimetres, every ANSI / ARCH sheet one set in
/// inches. Order inside each set is left, bottom, right, top.
const PDF_MM_STANDARD: Margins = Margins {
    left: 5.0,
    bottom: 17.0,
    right: 6.0,
    top: 18.0,
};
const PDF_MM_EXPAND: Margins = Margins {
    left: 5.0,
    bottom: 10.0,
    right: 6.0,
    top: 11.0,
};
const PDF_MM_FULL_BLEED: Margins = Margins {
    left: 0.0,
    bottom: 1.0,
    right: 0.0,
    top: 1.0,
};
const PDF_IN_STANDARD: Margins = Margins {
    left: 0.23,
    bottom: 0.70,
    right: 0.23,
    top: 0.70,
};
const PDF_IN_EXPAND: Margins = Margins {
    left: 0.23,
    bottom: 0.42,
    right: 0.23,
    top: 0.42,
};
const PDF_IN_FULL_BLEED: Margins = Margins::uniform(0.03);

fn pdf_margins_mm(paper: &PaperSize) -> Margins {
    let set = match (paper.units, paper.variant) {
        (PaperUnits::Millimeters, PaperVariant::Standard) => PDF_MM_STANDARD,
        (PaperUnits::Millimeters, PaperVariant::Expand) => PDF_MM_EXPAND,
        (PaperUnits::Millimeters, PaperVariant::FullBleed) => PDF_MM_FULL_BLEED,
        (PaperUnits::Inches, PaperVariant::Standard) => PDF_IN_STANDARD,
        (PaperUnits::Inches, PaperVariant::Expand) => PDF_IN_EXPAND,
        (PaperUnits::Inches, PaperVariant::FullBleed) => PDF_IN_FULL_BLEED,
    };
    set.to_mm(paper.units)
}

// ── System printers ───────────────────────────────────────────────────────

/// One sheet a printer reports it can load, with its unprintable margins.
#[derive(Clone, Debug, PartialEq)]
pub struct PrinterMedia {
    /// The sheet as our catalogue knows it (matched by size), or a custom
    /// sheet built from the reported dimensions.
    pub paper: PaperSize,
    pub margins: Margins,
    /// `true` when the printer also reports a borderless variant of this size.
    pub borderless: bool,
}

/// What a printer reported about its media, cached for the session.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrinterCapabilities {
    /// Sheets in the printer's own order, one entry per distinct size.
    pub media: Vec<PrinterMedia>,
    /// The printer's default sheet, when reported.
    pub default_paper: Option<PaperSize>,
}

impl PrinterCapabilities {
    /// Margins the printer reports for `paper`, matched by size. `None` when
    /// the printer never mentioned a sheet of that size.
    pub fn margins_for(&self, paper: &PaperSize) -> Option<Margins> {
        let (w, h) = paper.portrait_mm();
        self.media
            .iter()
            .find(|media| {
                let (mw, mh) = media.paper.portrait_mm();
                (mw - w).abs() <= 0.5 && (mh - h).abs() <= 0.5
            })
            .map(|media| media.margins)
    }
}

/// Answers per printer for the session; a printer that could not be asked
/// is remembered as `None` too, so a slow failing query (an offline network
/// queue) runs once, not on every dialog open.
type CapabilityCache = HashMap<String, Option<Arc<PrinterCapabilities>>>;

fn capability_cache() -> &'static Mutex<CapabilityCache> {
    static CACHE: OnceLock<Mutex<CapabilityCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The cached capabilities of `printer`, if they were queried already.
pub fn cached_printer_capabilities(printer: &str) -> Option<Arc<PrinterCapabilities>> {
    capability_cache().lock().ok()?.get(printer).cloned().flatten()
}

/// Query `printer` for its media (blocking: a process spawn or a driver
/// round trip) and cache the answer. Returns `None` when the platform or
/// the printer cannot answer — the plot dialog then falls back to the paper
/// catalogue with unverified margins.
pub fn printer_capabilities(printer: &str) -> Option<Arc<PrinterCapabilities>> {
    if let Ok(cache) = capability_cache().lock() {
        if let Some(answer) = cache.get(printer) {
            return answer.clone();
        }
    }
    let caps = query_printer_capabilities(printer).map(Arc::new);
    if let Ok(mut cache) = capability_cache().lock() {
        cache.insert(printer.to_string(), caps.clone());
    }
    caps
}

/// Name of the system default printer, when the platform reports one.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "windows")))]
pub fn default_printer_name() -> Option<String> {
    // `lpstat -d` → "system default destination: NAME" (the prefix is
    // localized, the name follows the last colon).
    let output = std::process::Command::new("lpstat")
        .arg("-d")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().find(|line| line.contains(':'))?;
    let name = line.rsplit(':').next()?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// The spooler's default printer, by name — the same lookup every direct
/// print job resolves "Default" through, so the dialog names what will print.
#[cfg(target_os = "windows")]
pub fn default_printer_name() -> Option<String> {
    crate::io::print_to_printer::windows_default_printer().ok()
}

#[cfg(target_arch = "wasm32")]
pub fn default_printer_name() -> Option<String> {
    None
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "windows")))]
fn query_printer_capabilities(printer: &str) -> Option<PrinterCapabilities> {
    // cupsd answers for the queue itself, so this is quick even when the
    // printer is off; `ipptool` is part of every CUPS install that has
    // `lpstat`. The standard test file asks for every attribute; the two we
    // read are `media-col-database` and `media-default`.
    let uri = format!("ipp://localhost/printers/{}", printer.replace(' ', "%20"));
    let output = std::process::Command::new("ipptool")
        .args(["-tv", &uri, "get-printer-attributes.test"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_ipp_media(&String::from_utf8_lossy(&output.stdout))
}

/// The driver's sheet tables and a printer DC's margins (see
/// `windows_media`); `None` for a queue that lists no sheet.
#[cfg(target_os = "windows")]
fn query_printer_capabilities(printer: &str) -> Option<PrinterCapabilities> {
    let raw = crate::io::print_to_printer::windows_printer_media(printer)?;
    crate::io::windows_media::printer_media(&raw)
}

#[cfg(target_arch = "wasm32")]
fn query_printer_capabilities(_printer: &str) -> Option<PrinterCapabilities> {
    None
}

/// Read the media a printer reports from `ipptool -tv` output.
///
/// The relevant lines look like
/// `media-col-database (1setOf collection) = {media-size={x-dimension=21000
/// y-dimension=29700} media-bottom-margin=300 media-left-margin=300 …},{…}`
/// with sizes and margins in hundredths of a millimetre, and
/// `media-default (keyword) = iso_a4_210x297mm`. Borderless variants repeat a
/// size with zero margins; they collapse onto the bordered entry.
#[cfg_attr(any(target_arch = "wasm32", target_os = "windows"), allow(dead_code))]
fn parse_ipp_media(text: &str) -> Option<PrinterCapabilities> {
    let database = attribute_value(text, "media-col-database")?;
    let mut media: Vec<PrinterMedia> = Vec::new();
    for entry in database.split("},{") {
        let Some((w, h)) = collection_size_mm(entry) else {
            continue;
        };
        let margins = Margins {
            left: hundredths_mm(entry, "media-left-margin"),
            bottom: hundredths_mm(entry, "media-bottom-margin"),
            right: hundredths_mm(entry, "media-right-margin"),
            top: hundredths_mm(entry, "media-top-margin"),
        };
        let borderless = margins == Margins::ZERO;
        if let Some(existing) = media.iter_mut().find(|m| {
            let (mw, mh) = m.paper.portrait_mm();
            let (pw, ph) = portrait(w, h);
            (mw - pw).abs() <= 0.5 && (mh - ph).abs() <= 0.5
        }) {
            // Keep the bordered margins as the sheet's printable area and
            // remember that edge-to-edge is possible too.
            if borderless {
                existing.borderless = true;
            } else if existing.margins == Margins::ZERO {
                existing.margins = margins;
                existing.borderless = true;
            }
            continue;
        }
        let (pw, ph) = portrait(w, h);
        let paper = paper_catalog::match_dimensions_mm(pw, ph)
            .cloned()
            .unwrap_or_else(|| PaperSize::custom(pw, ph, PaperUnits::Millimeters));
        media.push(PrinterMedia {
            paper,
            margins,
            borderless,
        });
    }
    if media.is_empty() {
        return None;
    }
    let default_paper = attribute_value(text, "media-default")
        .and_then(|name| pwg_size_mm(name.trim()))
        .and_then(|(w, h)| {
            let (w, h) = portrait(w, h);
            media
                .iter()
                .find(|m| {
                    let (mw, mh) = m.paper.portrait_mm();
                    (mw - w).abs() <= 0.5 && (mh - h).abs() <= 0.5
                })
                .map(|m| m.paper.clone())
        });
    Some(PrinterCapabilities {
        media,
        default_paper,
    })
}

/// The text after `name (…) = ` on the line that declares `name`.
#[cfg_attr(any(target_arch = "wasm32", target_os = "windows"), allow(dead_code))]
fn attribute_value<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let line = line.trim_start();
        let rest = line.strip_prefix(name)?;
        // The attribute name is followed by its syntax in parentheses.
        if !rest.starts_with(' ') && !rest.starts_with('(') {
            return None;
        }
        let (_, value) = rest.split_once('=')?;
        Some(value.trim())
    })
}

#[cfg_attr(any(target_arch = "wasm32", target_os = "windows"), allow(dead_code))]
fn collection_size_mm(entry: &str) -> Option<(f64, f64)> {
    let x = hundredths(entry, "x-dimension")?;
    let y = hundredths(entry, "y-dimension")?;
    (x > 0.0 && y > 0.0).then_some((x, y))
}

#[cfg_attr(any(target_arch = "wasm32", target_os = "windows"), allow(dead_code))]
fn hundredths(entry: &str, key: &str) -> Option<f64> {
    let start = entry.find(key)? + key.len();
    let rest = entry[start..].trim_start_matches('=');
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<f64>().ok().map(|v| v / 100.0)
}

#[cfg_attr(any(target_arch = "wasm32", target_os = "windows"), allow(dead_code))]
fn hundredths_mm(entry: &str, key: &str) -> f64 {
    hundredths(entry, key).unwrap_or(0.0)
}

#[cfg_attr(any(target_arch = "wasm32", target_os = "windows"), allow(dead_code))]
fn portrait(w: f64, h: f64) -> (f64, f64) {
    if w <= h {
        (w, h)
    } else {
        (h, w)
    }
}

/// Dimensions from a PWG self-describing media name such as
/// `iso_a4_210x297mm` or `na_letter_8.5x11in`.
#[cfg_attr(any(target_arch = "wasm32", target_os = "windows"), allow(dead_code))]
fn pwg_size_mm(name: &str) -> Option<(f64, f64)> {
    let dims = name.rsplit('_').next()?;
    let (dims, units) = dims
        .strip_suffix("mm")
        .map(|d| (d, PaperUnits::Millimeters))
        .or_else(|| dims.strip_suffix("in").map(|d| (d, PaperUnits::Inches)))?;
    let (w, h) = dims.split_once('x')?;
    let w: f64 = w.parse().ok()?;
    let h: f64 = h.parse().ok()?;
    Some((units.to_mm(w), units.to_mm(h)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const IPP_SAMPLE: &str = "        media-default (keyword) = iso_a4_210x297mm\n\
        media-supported (1setOf keyword) = iso_a4_210x297mm,om_a-4.borderless_210x297mm,na_letter_8.5x11in,custom_215x345mm_215x345mm\n\
        media-col-database (1setOf collection) = {media-size={x-dimension=21000 y-dimension=29700} media-bottom-margin=300 media-left-margin=300 media-right-margin=300 media-top-margin=300},{media-size={x-dimension=21000 y-dimension=29700} media-bottom-margin=0 media-left-margin=0 media-right-margin=0 media-top-margin=0},{media-size={x-dimension=21590 y-dimension=27940} media-bottom-margin=635 media-left-margin=635 media-right-margin=635 media-top-margin=635},{media-size={x-dimension=21500 y-dimension=34500} media-bottom-margin=300 media-left-margin=300 media-right-margin=300 media-top-margin=300}\n";

    #[test]
    fn canonical_names_follow_source_app() {
        assert_eq!(PlotDevice::Pdf.canonical_name(), "DWG To PDF.pc3");
        // What the source application writes for "None" in its plot dialog.
        assert_eq!(PlotDevice::None.canonical_name(), "none_device");
        assert_eq!(
            PlotDevice::Printer("L3270".into()).canonical_name(),
            "L3270"
        );
    }

    #[test]
    fn stored_names_resolve_and_say_whether_to_keep_them() {
        assert_eq!(PlotDevice::from_stored_name(""), (PlotDevice::None, false));
        assert_eq!(
            PlotDevice::from_stored_name("None"),
            (PlotDevice::None, false)
        );
        assert_eq!(
            PlotDevice::from_stored_name("none_device"),
            (PlotDevice::None, true)
        );
        assert_eq!(
            PlotDevice::from_stored_name("NONE_DEVICE"),
            (PlotDevice::None, true)
        );
        assert_eq!(
            PlotDevice::from_stored_name("Save to PDF file…"),
            (PlotDevice::Pdf, false)
        );
        assert_eq!(
            PlotDevice::from_stored_name("Microsoft Print to PDF"),
            (PlotDevice::Pdf, false)
        );
        assert_eq!(
            PlotDevice::from_stored_name("DWG To PDF.pc3"),
            (PlotDevice::Pdf, true)
        );
        assert_eq!(
            PlotDevice::from_stored_name("the source application PDF (High Quality Print).pc3"),
            (PlotDevice::Pdf, true)
        );
        // A plotter we cannot drive plots to PDF here but keeps its name.
        assert_eq!(
            PlotDevice::from_stored_name("DWF6 ePlot.pc3"),
            (PlotDevice::Pdf, true)
        );
        assert_eq!(
            PlotDevice::from_stored_name("HP LaserJet 4"),
            (PlotDevice::Printer("HP LaserJet 4".into()), true)
        );
    }

    #[test]
    fn pdf_margins_follow_source_app_per_family_and_variant() {
        let m =
            |name: &str| PlotDevice::Pdf.margins_mm(&paper_catalog::resolve(name).unwrap(), &[]);
        // ISO sheets share one millimetre set regardless of size.
        let iso_standard = Margins {
            left: 5.0,
            bottom: 17.0,
            right: 6.0,
            top: 18.0,
        };
        assert_eq!(m("ISO_A4_(210.00_x_297.00_MM)"), iso_standard);
        assert_eq!(m("ISO_A0_(841.00_x_1189.00_MM)"), iso_standard);
        assert_eq!(
            m("ISO_expand_A4_(210.00_x_297.00_MM)"),
            Margins {
                left: 5.0,
                bottom: 10.0,
                right: 6.0,
                top: 11.0
            }
        );
        assert_eq!(
            m("ISO_full_bleed_A4_(210.00_x_297.00_MM)"),
            Margins {
                left: 0.0,
                bottom: 1.0,
                right: 0.0,
                top: 1.0
            }
        );
        // Inch sheets are listed in inches and stored in millimetres.
        let close = |a: Margins, b: Margins| {
            (a.left - b.left).abs() < 1e-9
                && (a.bottom - b.bottom).abs() < 1e-9
                && (a.right - b.right).abs() < 1e-9
                && (a.top - b.top).abs() < 1e-9
        };
        let inch = |l: f64, b: f64, r: f64, t: f64| Margins {
            left: l * 25.4,
            bottom: b * 25.4,
            right: r * 25.4,
            top: t * 25.4,
        };
        assert!(close(
            m("ANSI_A_(8.50_x_11.00_Inches)"),
            inch(0.23, 0.70, 0.23, 0.70)
        ));
        assert!(close(
            m("ARCH_D_(24.00_x_36.00_Inches)"),
            inch(0.23, 0.70, 0.23, 0.70)
        ));
        assert!(close(
            m("ARCH_expand_D_(24.00_x_36.00_Inches)"),
            inch(0.23, 0.42, 0.23, 0.42)
        ));
        assert!(close(
            m("ANSI_full_bleed_B_(11.00_x_17.00_Inches)"),
            inch(0.03, 0.03, 0.03, 0.03)
        ));
        assert_eq!(
            PlotDevice::None.margins_mm(
                &paper_catalog::resolve("ISO_A4_(210.00_x_297.00_MM)").unwrap(),
                &[]
            ),
            Margins::ZERO
        );
    }

    #[test]
    fn user_defined_sheets_bring_their_own_margins() {
        let roll = CustomPaper {
            name: "Roll".into(),
            width: 900.0,
            height: 1200.0,
            units: PaperUnits::Millimeters,
            margins: Margins::uniform(3.0),
        };
        let paper = roll.paper();
        assert_eq!(
            PlotDevice::Pdf.margins_mm(&paper, &[roll.clone()]),
            Margins::uniform(3.0)
        );
        assert_eq!(
            PlotDevice::None.margins_mm(&paper, &[roll.clone()]),
            Margins::uniform(3.0)
        );
        // An unknown printer falls back to the user's margins too.
        assert_eq!(
            PlotDevice::Printer("Nowhere".into()).margins_mm(&paper, &[roll]),
            Margins::uniform(3.0)
        );
        // Without a definition a custom-looking sheet gets the family set.
        assert_eq!(
            PlotDevice::Pdf.margins_mm(&paper, &[]),
            Margins {
                left: 5.0,
                bottom: 17.0,
                right: 6.0,
                top: 18.0
            }
        );
    }

    #[test]
    fn ipp_media_parses_sizes_margins_borderless_and_default() {
        let caps = parse_ipp_media(IPP_SAMPLE).expect("media parsed");
        assert_eq!(
            caps.media.len(),
            3,
            "borderless A4 folds into the bordered entry"
        );
        let a4 = &caps.media[0];
        assert_eq!(a4.paper.canonical, "ISO_A4_(210.00_x_297.00_MM)");
        assert_eq!(a4.margins, Margins::uniform(3.0));
        assert!(a4.borderless);
        let letter = &caps.media[1];
        assert_eq!(letter.paper.canonical, "ANSI_A_(8.50_x_11.00_Inches)");
        assert_eq!(letter.margins, Margins::uniform(6.35));
        assert!(!letter.borderless);
        let custom = &caps.media[2];
        assert_eq!(
            custom.paper.canonical,
            "UserDefinedMetric_(215.00_x_345.00_MM)"
        );
        assert_eq!(
            caps.default_paper.as_ref().map(|p| p.canonical.as_ref()),
            Some("ISO_A4_(210.00_x_297.00_MM)")
        );
        // Margin lookup by sheet size, either orientation, tolerant of the
        // catalogue's inch → mm rounding.
        let ansi_a = paper_catalog::resolve("Letter_(8.50_x_11.00_Inches)").unwrap();
        assert_eq!(caps.margins_for(&ansi_a), Some(Margins::uniform(6.35)));
        let a3 = paper_catalog::resolve("ISO_A3_(297.00_x_420.00_MM)").unwrap();
        assert_eq!(caps.margins_for(&a3), None);
    }

    #[test]
    fn ipp_output_without_media_is_rejected() {
        assert!(parse_ipp_media("printer-state (enum) = 3\n").is_none());
        assert!(parse_ipp_media(
            "media-col-database (1setOf collection) = {media-size={x-dimension=0 y-dimension=0}}\n"
        )
        .is_none());
    }

    #[test]
    fn pwg_names_carry_their_size() {
        assert_eq!(pwg_size_mm("iso_a4_210x297mm"), Some((210.0, 297.0)));
        let (w, h) = pwg_size_mm("na_letter_8.5x11in").unwrap();
        assert!((w - 215.9).abs() < 1e-9 && (h - 279.4).abs() < 1e-9);
        assert_eq!(
            pwg_size_mm("custom_215x345mm_215x345mm"),
            Some((215.0, 345.0))
        );
        assert_eq!(pwg_size_mm("iso_a4"), None);
    }
}
