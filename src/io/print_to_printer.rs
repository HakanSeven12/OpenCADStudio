// print_to_printer — send the current layout to the system printer.
//
// Strategy:
//   1. Render the drawing to a temporary PDF (reusing the PDF export pipeline).
//   2. Send that PDF to the system printer with `lp` (Linux/macOS) or
//      `ShellExecute PRINT` (Windows).
//
// The function is async so the UI remains responsive while the job is queued.

#[cfg(not(target_arch = "wasm32"))]
use crate::io::pdf_export;
use crate::io::plot_style::PlotStyleTable;
use crate::scene::model::hatch_model::HatchModel;
use crate::io::pdf_export::PlotWire;

/// Extra options for a print job. On CUPS (Linux/macOS) these map to `lp`
/// flags / `-o` options. On Windows the generated PDF already carries render
/// options. Windows queues repeated jobs when more than one copy is requested;
/// driver quality remains managed by the selected printer.
#[derive(Debug, Clone, Default)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub struct PrintOptions {
    /// Target printer name, or `None` for the system default.
    pub printer: Option<String>,
    /// Number of copies (treated as at least 1).
    pub copies: u32,
    /// Print quality label selected in the plot dialog.
    pub quality: Option<String>,
    /// Controls applied while building the intermediate PDF.
    pub render: crate::io::pdf_export::PdfPlotOptions,
}

#[cfg(target_arch = "wasm32")]
pub fn list_printers() -> Vec<String> {
    Vec::new()
}

#[cfg(target_arch = "wasm32")]
#[allow(clippy::too_many_arguments)]
pub async fn print_wires_with(
    _wires: std::sync::Arc<Vec<PlotWire>>,
    _hatches: Vec<HatchModel>,
    _wipeouts: Vec<HatchModel>,
    _paper_w: f64,
    _paper_h: f64,
    _offset_x: f64,
    _offset_y: f64,
    _rotation_deg: i32,
    _scale: f32,
    _clip: Option<(f32, f32, f32, f32)>,
    _plot_style: Option<PlotStyleTable>,
    _opts: PrintOptions,
) -> Result<String, String> {
    Err("Printing is not available in the web version.".into())
}

#[cfg(target_arch = "wasm32")]
pub fn open_in_viewer(_path: &std::path::Path) -> Result<(), String> {
    Err("Preview is not available in the web version.".into())
}

#[cfg(target_arch = "wasm32")]
pub fn print_existing_pdf(_path: &std::path::Path, _opts: &PrintOptions) -> Result<String, String> {
    Err("Printing is not available in the web version.".into())
}

/// Enumerate installed printers. Linux/macOS query CUPS via `lpstat -e`;
/// Windows returns an empty list (the "printto" dispatch targets a named
/// printer directly and the system default is always available).
#[cfg(not(target_arch = "wasm32"))]
pub fn list_printers() -> Vec<String> {
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("lpstat")
            .arg("-e")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }
    #[cfg(target_os = "windows")]
    {
        Vec::new()
    }
}

/// Build the platform printer-properties command.
#[cfg(not(target_arch = "wasm32"))]
fn printer_properties_command(printer: Option<&str>) -> (&'static str, Vec<String>) {
    let named = printer
        .map(str::trim)
        .filter(|name| !name.is_empty());

    #[cfg(target_os = "windows")]
    let command = match named {
        Some(name) => (
            "rundll32.exe",
            vec![
                "printui.dll,PrintUIEntry".to_string(),
                "/p".to_string(),
                "/n".to_string(),
                name.to_string(),
            ],
        ),
        None => ("control.exe", vec!["printers".to_string()]),
    };

    #[cfg(target_os = "macos")]
    let command = {
        let _ = named;
        (
            "open",
            vec!["x-apple.systempreferences:com.apple.Print-Scan-Settings.extension".to_string()],
        )
    };

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let command = {
        let target = named
            .map(|name| format!("http://localhost:631/printers/{name}"))
            .unwrap_or_else(|| "http://localhost:631/printers".to_string());
        ("xdg-open", vec![target])
    };

    command
}

/// Open the operating system's printer configuration surface.
#[cfg(not(target_arch = "wasm32"))]
pub fn open_printer_properties(printer: Option<&str>) -> Result<(), String> {
    let (program, args) = printer_properties_command(printer);
    std::process::Command::new(program)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open printer properties: {error}"))
}

#[cfg(target_arch = "wasm32")]
pub fn open_printer_properties(_printer: Option<&str>) -> Result<(), String> {
    Err("Printer properties are not available in the web version.".into())
}

/// Like [`print_wires`] but honours a [`PrintOptions`] bundle (printer, copies,
/// grayscale, quality, DPI).
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
pub async fn print_wires_with(
    wires: std::sync::Arc<Vec<PlotWire>>,
    hatches: Vec<HatchModel>,
    wipeouts: Vec<HatchModel>,
    paper_w: f64,
    paper_h: f64,
    offset_x: f64,
    offset_y: f64,
    rotation_deg: i32,
    scale: f32,
    clip: Option<(f32, f32, f32, f32)>,
    plot_style: Option<PlotStyleTable>,
    opts: PrintOptions,
) -> Result<String, String> {
    let tmp_path = std::env::temp_dir().join("open_cad_studio_print.pdf");
    pdf_export::export_pdf(
        &wires,
        &hatches,
        &wipeouts,
        paper_w,
        paper_h,
        offset_x,
        offset_y,
        rotation_deg,
        scale,
        clip,
        &tmp_path,
        plot_style.as_ref(),
        opts.render,
    )?;
    dispatch_to_printer_opts(&tmp_path, &opts)
}

/// Send an already-rendered PDF to a printer with [`PrintOptions`]. Used for
/// clipped window plots, whose PDF is built with a scale + clip the plain
/// `print_wires_with` path doesn't expose.
#[cfg(not(target_arch = "wasm32"))]
pub fn print_existing_pdf(path: &std::path::Path, opts: &PrintOptions) -> Result<String, String> {
    dispatch_to_printer_opts(path, opts)
}

/// Open a file with the OS default application (used for print preview).
#[cfg(not(target_arch = "wasm32"))]
pub fn open_in_viewer(path: &std::path::Path) -> Result<(), String> {
    let p = path.to_string_lossy().to_string();
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", "", &p]);
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(&p);
        c
    };
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(&p);
        c
    };
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open preview: {e}"))
}

/// Dispatch a PDF to a specific printer with [`PrintOptions`].
#[cfg(not(target_arch = "wasm32"))]
fn dispatch_to_printer_opts(
    path: &std::path::Path,
    opts: &PrintOptions,
) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        // Target a named printer via the "printto" verb; fall back to the
        // default-printer "print" verb.
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        let wide = |s: &str| -> Vec<u16> { OsStr::new(s).encode_wide().chain(Some(0)).collect() };
        let path_wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let (verb, params, label) = match opts.printer.as_deref() {
            Some(p) if !p.is_empty() => (wide("printto"), Some(wide(p)), p.to_string()),
            _ => (wide("print"), None, "default printer".to_string()),
        };
        let params_ptr = params.as_ref().map(|v| v.as_ptr()).unwrap_or(std::ptr::null());
        for _ in 0..opts.copies.max(1) {
            let result = unsafe {
                windows_sys::Win32::UI::Shell::ShellExecuteW(
                    std::ptr::null_mut(),
                    verb.as_ptr(),
                    path_wide.as_ptr(),
                    params_ptr,
                    std::ptr::null(),
                    windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE,
                ) as usize
            };
            if result <= 32 {
                return Err(format!("ShellExecute failed (code {result})"));
            }
        }
        Ok(label)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let path_str = path.to_string_lossy();
        let mut cmd = std::process::Command::new("lp");
        if let Some(p) = opts.printer.as_deref() {
            if !p.is_empty() {
                cmd.arg("-d").arg(p);
            }
        }
        let copies = opts.copies.max(1);
        if copies > 1 {
            cmd.arg("-n").arg(copies.to_string());
        }
        if let Some(q) = opts.quality.as_deref() {
            // CUPS print-quality: 3 = draft, 4 = normal, 5 = high / best.
            let pq = match q {
                "Low" => "3",
                "High" => "5",
                _ => "4",
            };
            cmd.arg("-o").arg(format!("print-quality={pq}"));
        }
        let lp_result = cmd
            .arg("--")
            .arg(path_str.as_ref())
            .output();
        if let Ok(out) = &lp_result {
            if !out.status.success() {
                // Continue to the lpr fallback below.
            } else {
            let msg = String::from_utf8_lossy(&out.stdout);
            let printer = msg
                .split_whitespace()
                .find(|w| w.contains('-'))
                .unwrap_or("printer")
                .to_string();
                return Ok(printer);
            }
        }

        let mut fallback = std::process::Command::new("lpr");
        if let Some(printer) = opts.printer.as_deref().filter(|name| !name.is_empty()) {
            fallback.arg("-P").arg(printer);
        }
        if copies > 1 {
            fallback.arg(format!("-#{copies}"));
        }
        let out = fallback
            .arg(path_str.as_ref())
            .output()
            .map_err(|error| match lp_result {
                Ok(ref lp) => format!(
                    "lp failed: {}; lpr could not launch: {error}",
                    String::from_utf8_lossy(&lp.stderr)
                ),
                Err(ref lp) => format!("lp could not launch: {lp}; lpr could not launch: {error}"),
            })?;
        if out.status.success() {
            Ok(opts
                .printer
                .clone()
                .unwrap_or_else(|| "default printer".into()))
        } else {
            Err(format!(
                "lpr failed: {}",
                String::from_utf8_lossy(&out.stderr)
            ))
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod printer_properties_tests {
    use super::printer_properties_command;

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_opens_selected_printer_or_printer_list() {
        assert_eq!(
            printer_properties_command(Some("  Office LaserJet  ")),
            (
                "rundll32.exe",
                vec![
                    "printui.dll,PrintUIEntry".to_string(),
                    "/p".to_string(),
                    "/n".to_string(),
                    "Office LaserJet".to_string(),
                ],
            ),
        );
        assert_eq!(
            printer_properties_command(None),
            ("control.exe", vec!["printers".to_string()]),
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_opens_print_settings() {
        let expected = (
            "open",
            vec![
                "x-apple.systempreferences:com.apple.Print-Scan-Settings.extension".to_string(),
            ],
        );
        assert_eq!(
            printer_properties_command(Some("  Office LaserJet  ")),
            expected.clone(),
        );
        assert_eq!(printer_properties_command(None), expected);
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    #[test]
    fn unix_opens_selected_cups_printer_or_printer_list() {
        assert_eq!(
            printer_properties_command(Some("  Office LaserJet  ")),
            (
                "xdg-open",
                vec!["http://localhost:631/printers/Office LaserJet".to_string()],
            ),
        );
        assert_eq!(
            printer_properties_command(None),
            (
                "xdg-open",
                vec!["http://localhost:631/printers".to_string()],
            ),
        );
    }

    #[test]
    fn a_blank_selection_is_treated_as_no_selection() {
        assert_eq!(
            printer_properties_command(Some("   ")),
            printer_properties_command(None),
        );
    }
}
