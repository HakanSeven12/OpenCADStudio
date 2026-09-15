// print_to_printer — send the current layout to the system printer.
//
// Desktop strategy:
//   1. Render the drawing to a temporary PDF (reusing the PDF export pipeline).
//   2. Send that PDF to the system printer with `lp` (Linux/macOS) or
//      `ShellExecute PRINT` (Windows).
//
// The function is async so the UI remains responsive while the job is queued.
//
// The web build has no system printers: `preview_page_pdf` /
// `preview_pages_pdf` render the same PDF bytes and open them in a browser
// tab, where the browser's own print dialog takes over.

#[cfg(not(target_arch = "wasm32"))]
use crate::io::pdf_export;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn temp_pdf_path(kind: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "open_cad_studio_{kind}_{}_{stamp}_{id}.pdf",
        std::process::id()
    ))
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn temp_pdf_path(kind: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(format!("{kind}.pdf"))
}

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
    /// Print quality label selected in the plot dialog. Read only on the CUPS
    /// path (`lp -o print-quality=…`); on Windows the driver's own quality
    /// setting wins, so the field is legitimately unread there.
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    pub quality: Option<String>,
}

#[cfg(target_arch = "wasm32")]
pub fn list_printers() -> Vec<String> {
    Vec::new()
}

#[cfg(target_arch = "wasm32")]
pub async fn print_wires_with(
    _page: crate::io::pdf_export::PdfPageInput,
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
/// Windows queries the spooler's cached local and connected printer list.
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
        windows_printers().unwrap_or_else(|error| {
            eprintln!("Could not enumerate printers: {error}");
            Vec::new()
        })
    }
}

#[cfg(target_os = "windows")]
fn windows_printers() -> std::io::Result<Vec<String>> {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_INSUFFICIENT_BUFFER};
    use windows_sys::Win32::Graphics::Printing::{
        EnumPrintersW, PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL,
    };

    let mut bytes_needed = 0;
    let mut printer_count = 0;
    let mut buffer = Vec::<usize>::new();
    // The printer list can grow between the size query and the data query.
    for _ in 0..4 {
        let buffer_bytes = u32::try_from(std::mem::size_of_val(buffer.as_slice()))
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::OutOfMemory))?;
        let data = if buffer.is_empty() {
            std::ptr::null_mut()
        } else {
            buffer.as_mut_ptr().cast::<u8>()
        };
        // SAFETY: data is null for the size query, otherwise it points to an
        // aligned, writable allocation of buffer_bytes bytes.
        let success = unsafe {
            EnumPrintersW(
                PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS,
                std::ptr::null(),
                4,
                data,
                buffer_bytes,
                &mut bytes_needed,
                &mut printer_count,
            )
        };
        if success != 0 {
            return windows_printer_names(&buffer, printer_count as usize);
        }
        let error = unsafe { GetLastError() };
        if error != ERROR_INSUFFICIENT_BUFFER || bytes_needed <= buffer_bytes {
            return Err(std::io::Error::from_raw_os_error(error as i32));
        }
        buffer.resize(
            (bytes_needed as usize).div_ceil(std::mem::size_of::<usize>()),
            0,
        );
    }
    Err(std::io::Error::from_raw_os_error(
        ERROR_INSUFFICIENT_BUFFER as i32,
    ))
}

#[cfg(target_os = "windows")]
fn windows_printer_names(buffer: &[usize], printer_count: usize) -> std::io::Result<Vec<String>> {
    use std::io::{Error, ErrorKind};
    use windows_sys::Win32::Graphics::Printing::PRINTER_INFO_4W;

    const {
        assert!(std::mem::align_of::<usize>() >= std::mem::align_of::<PRINTER_INFO_4W>());
    }
    let buffer_bytes = std::mem::size_of_val(buffer);
    if printer_count > buffer_bytes / std::mem::size_of::<PRINTER_INFO_4W>() {
        return Err(Error::from(ErrorKind::InvalidData));
    }
    // SAFETY: the initialized buffer is aligned and large enough for these
    // records. Raw pointer fields are validated before reading any names.
    let records = unsafe {
        std::slice::from_raw_parts(buffer.as_ptr().cast::<PRINTER_INFO_4W>(), printer_count)
    };
    let mut names = Vec::with_capacity(printer_count);
    for record in records {
        if record.pPrinterName.is_null() {
            continue;
        }
        let offset = (record.pPrinterName as usize)
            .checked_sub(buffer.as_ptr() as usize)
            .filter(|offset| *offset < buffer_bytes && offset % 2 == 0)
            .ok_or(Error::from(ErrorKind::InvalidData))?;
        // SAFETY: offset is UTF-16 aligned and the slice ends within buffer.
        let utf16 = unsafe {
            std::slice::from_raw_parts(
                buffer.as_ptr().cast::<u8>().add(offset).cast::<u16>(),
                (buffer_bytes - offset) / 2,
            )
        };
        let length = utf16
            .iter()
            .position(|&unit| unit == 0)
            .ok_or(Error::from(ErrorKind::InvalidData))?;
        if length != 0 {
            names.push(String::from_utf16_lossy(&utf16[..length]));
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}

#[cfg(all(test, target_os = "windows"))]
mod printer_buffer_tests {
    use super::windows_printer_names;
    use windows_sys::Win32::Graphics::Printing::PRINTER_INFO_4W;

    #[test]
    fn printer_names_stay_within_the_returned_buffer() {
        assert!(windows_printer_names(&[], 0).unwrap().is_empty());
        assert!(windows_printer_names(&[], 1).is_err());
        let mut buffer = vec![0usize; 16];
        let name_offset = std::mem::size_of::<PRINTER_INFO_4W>();
        let name: Vec<u16> = "Printer \u{03b1}\0".encode_utf16().collect();
        // SAFETY: the aligned allocation holds the record and UTF-16 name.
        unsafe {
            let record = buffer.as_mut_ptr().cast::<PRINTER_INFO_4W>();
            let name_ptr = buffer
                .as_mut_ptr()
                .cast::<u8>()
                .add(name_offset)
                .cast::<u16>();
            std::ptr::copy_nonoverlapping(name.as_ptr(), name_ptr, name.len());
            (*record).pPrinterName = name_ptr;
            assert_eq!(
                windows_printer_names(&buffer, 1).unwrap(),
                ["Printer \u{03b1}"]
            );
            (*record).pPrinterName = buffer.as_mut_ptr().cast::<u16>().wrapping_sub(1);
            assert!(windows_printer_names(&buffer, 1).is_err());
            (*record).pPrinterName = name_ptr;
        }
        buffer[name_offset / std::mem::size_of::<usize>()..].fill(usize::MAX);
        assert!(windows_printer_names(&buffer, 1).is_err());
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

/// Render a page and send it to the selected printer.
#[cfg(not(target_arch = "wasm32"))]
pub async fn print_wires_with(
    page: crate::io::pdf_export::PdfPageInput,
    opts: PrintOptions,
) -> Result<String, String> {
    let tmp_path = temp_pdf_path("print");
    pdf_export::export_pdf(&page, &tmp_path)?;
    dispatch_to_printer_opts(&tmp_path, &opts)
}

/// Send an already-rendered PDF to the selected printer.
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

/// Render a page and show it the way each platform previews: the OS PDF
/// viewer on desktop, a new browser tab on the web.
pub fn preview_page_pdf(page: &crate::io::pdf_export::PdfPageInput) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let tmp = temp_pdf_path("preview");
        crate::io::pdf_export::export_pdf(page, &tmp)?;
        open_in_viewer(&tmp)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let bytes = crate::io::pdf_export::render_pdf_bytes(std::slice::from_ref(page), None)?;
        crate::sys::open_bytes_in_new_tab("plot_preview.pdf", &bytes);
        Ok(())
    }
}

/// Web-only multi-page variant used by print-all: render every layout into
/// one PDF and open it in a browser tab.
#[cfg(target_arch = "wasm32")]
pub fn preview_pages_pdf(pages: &[crate::io::pdf_export::PdfPageInput]) -> Result<(), String> {
    let bytes = crate::io::pdf_export::render_pdf_bytes(pages, None)?;
    crate::sys::open_bytes_in_new_tab("plot_preview.pdf", &bytes);
    Ok(())
}

/// Dispatch a PDF to a specific printer with [`PrintOptions`].
#[cfg(not(target_arch = "wasm32"))]
fn dispatch_to_printer_opts(
    path: &std::path::Path,
    opts: &PrintOptions,
) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Foundation::{GetLastError, ERROR_NO_ASSOCIATION};
        use windows_sys::Win32::UI::Shell::{
            ShellExecuteExW, SHELLEXECUTEINFOW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC,
            SE_ERR_NOASSOC,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

        let wide = |s: &str| -> Vec<u16> { OsStr::new(s).encode_wide().chain(Some(0)).collect() };
        let path_wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let (verb, params, label) = match opts.printer.as_deref() {
            Some(p) if !p.is_empty() => (wide("printto"), Some(wide(p)), p.to_string()),
            _ => (wide("print"), None, "default printer".to_string()),
        };
        let params_ptr = params.as_ref().map(|v| v.as_ptr()).unwrap_or(std::ptr::null());
        for _ in 0..opts.copies.max(1) {
            let mut info = SHELLEXECUTEINFOW {
                cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
                fMask: SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC,
                lpVerb: verb.as_ptr(),
                lpFile: path_wide.as_ptr(),
                lpParameters: params_ptr,
                nShow: SW_HIDE,
                ..Default::default()
            };
            if unsafe { ShellExecuteExW(&mut info) } == 0 {
                let shell_code = info.hInstApp as usize;
                let code = if (1..=32).contains(&shell_code) {
                    shell_code as u32
                } else {
                    unsafe { GetLastError() }
                };
                if code == SE_ERR_NOASSOC || code == ERROR_NO_ASSOCIATION {
                    return Err(
                        "Windows has no PDF application registered with Print support.".into(),
                    );
                }
                return Err(format!("Windows print dispatch failed (code {code})"));
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
