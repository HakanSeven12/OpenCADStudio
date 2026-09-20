//! What a Windows printer driver says about its sheets, turned into the plot
//! dialog's [`PrinterCapabilities`] — the counterpart of the CUPS media
//! parser in `plot_device.rs`.
//!
//! The driver answers three `DeviceCapabilities` questions with parallel
//! arrays (a paper id, a size in tenths of a millimetre and a 64-character
//! name per sheet) and, through its default DEVMODE, which sheet it starts
//! with; the unprintable margins come from a printer DC. Everything here
//! works on those raw arrays so it compiles and is tested on every platform;
//! the calls that fill them live in `print_to_printer.rs`.

use crate::io::paper_catalog::{self, Margins, PaperSize, PaperUnits};
use crate::io::plot_device::{PrinterCapabilities, PrinterMedia};

/// `DC_PAPERNAMES` returns names in fixed-width slots of this many UTF-16
/// units, NUL-padded (and not NUL-terminated when exactly full).
pub const PAPER_NAME_CHARS: usize = 64;

/// `dmPaperSize` of a user-defined sheet: its size is in `dmPaperWidth` /
/// `dmPaperLength` rather than in the driver's table.
pub const DMPAPER_USER: u16 = 256;

/// Sheets a driver would list but cannot mean: a "user defined" slot is
/// reported as 0 × 0, and some drivers pad with absurd extents.
const MAX_SHEET_MM: f64 = 10_000.0;

/// The driver's answers, as the Win32 calls hand them over.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RawPrinterMedia {
    /// `DC_PAPERS`: one `DMPAPER_*` id per sheet.
    pub ids: Vec<u16>,
    /// `DC_PAPERSIZE`: one `(width, height)` per sheet, tenths of a millimetre.
    pub sizes_tenths_mm: Vec<(i32, i32)>,
    /// `DC_PAPERNAMES`: `count × PAPER_NAME_CHARS` UTF-16 units, flat.
    pub names_utf16: Vec<u16>,
    /// The default DEVMODE's `dmPaperSize` when `DM_PAPERSIZE` is set.
    pub default_id: Option<u16>,
    /// The default DEVMODE's `dmPaperWidth` / `dmPaperLength` (tenths of a
    /// millimetre) for a `DMPAPER_USER` default.
    pub default_tenths_mm: Option<(i32, i32)>,
    /// Unprintable margins measured on the printer DC, portrait, mm.
    pub margins: Option<Margins>,
}

/// The `GetDeviceCaps` values a printable-area calculation needs, taken
/// from one printer DC in portrait.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeviceCapsSample {
    /// `PHYSICALWIDTH` / `PHYSICALHEIGHT`: the sheet, in device pixels.
    pub physical_width: i32,
    pub physical_height: i32,
    /// `PHYSICALOFFSETX` / `PHYSICALOFFSETY`: the printable area's corner.
    pub offset_x: i32,
    pub offset_y: i32,
    /// `HORZRES` / `VERTRES`: the printable area, in device pixels.
    pub horz_res: i32,
    pub vert_res: i32,
    /// `LOGPIXELSX` / `LOGPIXELSY`: device pixels per inch.
    pub dpi_x: i32,
    pub dpi_y: i32,
}

/// The sheet names out of a `DC_PAPERNAMES` buffer: each slot is trimmed at
/// its first NUL (a name that fills its slot has none) and decoded lossily.
pub fn paper_names(names_utf16: &[u16], count: usize) -> Vec<String> {
    names_utf16
        .chunks(PAPER_NAME_CHARS)
        .take(count)
        .map(|slot| {
            let end = slot.iter().position(|&unit| unit == 0).unwrap_or(slot.len());
            String::from_utf16_lossy(&slot[..end]).trim().to_string()
        })
        .collect()
}

/// The unprintable margins a DC sample describes, in millimetres — `None`
/// when the driver reported no usable resolution or extents.
pub fn device_margins_mm(sample: &DeviceCapsSample) -> Option<Margins> {
    if sample.dpi_x <= 0
        || sample.dpi_y <= 0
        || sample.physical_width <= 0
        || sample.physical_height <= 0
        || sample.horz_res <= 0
        || sample.vert_res <= 0
    {
        return None;
    }
    let mm_x = |pixels: i32| f64::from(pixels) / f64::from(sample.dpi_x) * 25.4;
    let mm_y = |pixels: i32| f64::from(pixels) / f64::from(sample.dpi_y) * 25.4;
    let margins = Margins {
        left: mm_x(sample.offset_x),
        top: mm_y(sample.offset_y),
        right: mm_x(sample.physical_width - sample.offset_x - sample.horz_res),
        bottom: mm_y(sample.physical_height - sample.offset_y - sample.vert_res),
    };
    let sane = |value: f64| value.is_finite() && value >= 0.0;
    (sane(margins.left) && sane(margins.top) && sane(margins.right) && sane(margins.bottom))
        .then_some(margins)
}

/// The driver's sheets as the plot dialog lists them: one entry per distinct
/// size (a rotated or borderless twin folds onto its sheet), catalogue
/// sheets under their own name, unknown sizes under the driver's name, the
/// device's margins on every entry, and the driver's default sheet when it
/// is one of them. `None` when the driver listed nothing usable, so the
/// dialog falls back to the catalogue.
pub fn printer_media(raw: &RawPrinterMedia) -> Option<PrinterCapabilities> {
    let count = raw.ids.len().min(raw.sizes_tenths_mm.len());
    let names = paper_names(&raw.names_utf16, count);
    let margins = raw.margins.unwrap_or(Margins::ZERO);
    let mut media: Vec<PrinterMedia> = Vec::new();
    // Which listed sheet each id resolves to, so the default can be found by
    // id even when its own entry folded onto an earlier twin.
    let mut sheet_of_id: Vec<(u16, usize)> = Vec::new();
    for index in 0..count {
        let (w, h) = raw.sizes_tenths_mm[index];
        let (w, h) = (f64::from(w) / 10.0, f64::from(h) / 10.0);
        if !(w > 0.0 && h > 0.0 && w <= MAX_SHEET_MM && h <= MAX_SHEET_MM) {
            continue;
        }
        let (pw, ph) = if w <= h { (w, h) } else { (h, w) };
        let name = names.get(index).cloned().unwrap_or_default();
        if let Some(position) = media.iter().position(|m| same_size(&m.paper, pw, ph)) {
            if name.to_ascii_lowercase().contains("borderless") {
                media[position].borderless = true;
            }
            sheet_of_id.push((raw.ids[index], position));
            continue;
        }
        let paper = paper_catalog::match_dimensions_mm(pw, ph).cloned().unwrap_or_else(|| {
            if name.is_empty() {
                PaperSize::custom(pw, ph, PaperUnits::Millimeters)
            } else {
                PaperSize::custom_named(&name, pw, ph, PaperUnits::Millimeters)
            }
        });
        sheet_of_id.push((raw.ids[index], media.len()));
        media.push(PrinterMedia {
            paper,
            margins,
            borderless: name.to_ascii_lowercase().contains("borderless"),
        });
    }
    if media.is_empty() {
        return None;
    }
    let default_paper = match raw.default_id {
        Some(DMPAPER_USER) => raw.default_tenths_mm.and_then(|(w, h)| {
            let (w, h) = (f64::from(w) / 10.0, f64::from(h) / 10.0);
            let (pw, ph) = if w <= h { (w, h) } else { (h, w) };
            media
                .iter()
                .find(|m| same_size(&m.paper, pw, ph))
                .map(|m| m.paper.clone())
        }),
        Some(id) => sheet_of_id
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, position)| media[*position].paper.clone()),
        None => None,
    };
    Some(PrinterCapabilities {
        media,
        default_paper,
    })
}

/// The driver's paper id for a sheet of this size in either orientation —
/// what a print job's DEVMODE needs so the sheet is printed at its own size
/// rather than on the driver's default.
pub fn paper_id_for_sheet(raw: &RawPrinterMedia, width_mm: f64, height_mm: f64) -> Option<u16> {
    let (pw, ph) = if width_mm <= height_mm {
        (width_mm, height_mm)
    } else {
        (height_mm, width_mm)
    };
    raw.ids
        .iter()
        .zip(&raw.sizes_tenths_mm)
        .find(|(_, (w, h))| {
            let (w, h) = (f64::from(*w) / 10.0, f64::from(*h) / 10.0);
            let (sw, sh) = if w <= h { (w, h) } else { (h, w) };
            (sw - pw).abs() <= 0.5 && (sh - ph).abs() <= 0.5
        })
        .map(|(id, _)| *id)
}

fn same_size(paper: &PaperSize, portrait_w: f64, portrait_h: f64) -> bool {
    let (w, h) = paper.portrait_mm();
    (w - portrait_w).abs() <= 0.5 && (h - portrait_h).abs() <= 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(name: &str) -> Vec<u16> {
        let mut units: Vec<u16> = name.encode_utf16().collect();
        units.resize(PAPER_NAME_CHARS, 0);
        units
    }

    fn raw(entries: &[(u16, (i32, i32), &str)]) -> RawPrinterMedia {
        RawPrinterMedia {
            ids: entries.iter().map(|(id, _, _)| *id).collect(),
            sizes_tenths_mm: entries.iter().map(|(_, size, _)| *size).collect(),
            names_utf16: entries.iter().flat_map(|(_, _, name)| slot(name)).collect(),
            default_id: None,
            default_tenths_mm: None,
            margins: Some(Margins {
                left: 3.0,
                bottom: 3.0,
                right: 3.0,
                top: 3.0,
            }),
        }
    }

    #[test]
    fn names_are_trimmed_at_the_first_nul_and_survive_a_full_slot() {
        let mut buffer = slot("A4");
        buffer.extend(std::iter::repeat_n(b'x' as u16, PAPER_NAME_CHARS));
        // A lone surrogate decodes lossily instead of failing.
        let mut broken = slot("Letter");
        broken[6] = 0xD800;
        broken[7] = 0;
        buffer.extend(broken);
        let names = paper_names(&buffer, 3);
        assert_eq!(names[0], "A4");
        assert_eq!(names[1].len(), PAPER_NAME_CHARS);
        assert!(names[2].starts_with("Letter"));
        assert_eq!(paper_names(&buffer, 1).len(), 1, "count caps the slots read");
    }

    #[test]
    fn twins_fold_onto_their_sheet_and_nonsense_sizes_are_dropped() {
        let caps = printer_media(&raw(&[
            (9, (2100, 2970), "A4"),
            (77, (2970, 2100), "A4 Rotated"),
            (1, (2159, 2794), "Letter"),
            (256, (0, 0), "User Defined"),
            (300, (2100, 2970), "A4 Borderless"),
            (301, (1000, 1500), "Photo 10x15"),
        ]))
        .expect("usable sheets");
        assert_eq!(caps.media.len(), 3);
        assert_eq!(caps.media[0].paper.canonical, "ISO_A4_(210.00_x_297.00_MM)");
        assert!(caps.media[0].borderless, "the borderless twin marks the sheet");
        assert_eq!(caps.media[1].paper.canonical, "ANSI_A_(8.50_x_11.00_Inches)");
        assert_eq!(caps.media[2].paper.label, "Photo 10x15", "the driver's name is kept");
        assert_eq!(caps.media[2].margins.left, 3.0, "the device margins go on every sheet");
        assert!(caps.default_paper.is_none());
    }

    #[test]
    fn the_default_sheet_is_found_by_id_or_by_user_size() {
        let mut media = raw(&[
            (9, (2100, 2970), "A4"),
            (77, (2970, 2100), "A4 Rotated"),
            (8, (2970, 4200), "A3"),
        ]);
        media.default_id = Some(77);
        let caps = printer_media(&media).unwrap();
        assert_eq!(
            caps.default_paper.as_ref().map(|p| p.canonical.as_ref()),
            Some("ISO_A4_(210.00_x_297.00_MM)"),
            "the rotated twin's id still names A4"
        );
        media.default_id = Some(DMPAPER_USER);
        media.default_tenths_mm = Some((4200, 2970));
        let caps = printer_media(&media).unwrap();
        assert_eq!(
            caps.default_paper.as_ref().map(|p| p.canonical.as_ref()),
            Some("ISO_A3_(297.00_x_420.00_MM)")
        );
        media.default_id = Some(999);
        assert!(printer_media(&media).unwrap().default_paper.is_none());
        assert!(printer_media(&RawPrinterMedia::default()).is_none(), "nothing listed → catalogue");
    }

    #[test]
    fn device_margins_come_from_the_dc_geometry() {
        let sample = DeviceCapsSample {
            physical_width: 4960,
            physical_height: 7016,
            offset_x: 100,
            offset_y: 100,
            horz_res: 4760,
            vert_res: 6716,
            dpi_x: 600,
            dpi_y: 600,
        };
        let margins = device_margins_mm(&sample).unwrap();
        let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
        assert!(close(margins.left, 100.0 / 600.0 * 25.4));
        assert!(close(margins.right, 100.0 / 600.0 * 25.4));
        assert!(close(margins.top, 100.0 / 600.0 * 25.4));
        assert!(close(margins.bottom, 200.0 / 600.0 * 25.4));
        assert!(device_margins_mm(&DeviceCapsSample::default()).is_none());
        let inverted = DeviceCapsSample { horz_res: 9000, ..sample };
        assert!(device_margins_mm(&inverted).is_none(), "printable area wider than the sheet");
    }

    #[test]
    fn the_paper_id_matches_either_orientation_within_half_a_millimetre() {
        let media = raw(&[(9, (2100, 2970), "A4"), (8, (2970, 4200), "A3")]);
        assert_eq!(paper_id_for_sheet(&media, 297.0, 210.0), Some(9));
        assert_eq!(paper_id_for_sheet(&media, 210.3, 297.4), Some(9));
        assert_eq!(paper_id_for_sheet(&media, 420.0, 297.0), Some(8));
        assert_eq!(paper_id_for_sheet(&media, 216.0, 279.0), None);
    }
}
