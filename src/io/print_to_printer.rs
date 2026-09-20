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
    /// Driver options chosen in the printer-properties editor, as CUPS
    /// `key=value` pairs (`ColorModel=Gray`, `MediaType=Photographic`). Read
    /// on the CUPS path (`lp -o`); Windows keeps such choices in the
    /// printer's own preferences instead.
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    pub driver_options: Vec<(String, String)>,
}

#[cfg(target_arch = "wasm32")]
pub fn list_printers() -> Result<Vec<String>, String> {
    Ok(Vec::new())
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
/// An error is the system's own explanation (a stopped spooler, a failing
/// RPC); a desktop without CUPS at all is not an error but an empty list.
#[cfg(not(target_arch = "wasm32"))]
pub fn list_printers() -> Result<Vec<String>, String> {
    #[cfg(not(target_os = "windows"))]
    {
        let output = match std::process::Command::new("lpstat").arg("-e").output() {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.to_string()),
        };
        // `lpstat -e` exits non-zero with "No destinations added" when CUPS
        // runs but knows no queue — an empty list, not a failure.
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect())
    }
    #[cfg(target_os = "windows")]
    {
        windows_printers().map_err(|error| error.to_string())
    }
}

/// The printing situation as the system reports it, one line per fact:
/// the default printer, the printers listed (or the reason none were), and
/// on Windows the route a plot takes. Shown by the `PRINTERS` command.
#[cfg(not(target_arch = "wasm32"))]
pub fn printer_report() -> Vec<String> {
    let mut lines = Vec::new();
    match crate::io::plot_device::default_printer_name() {
        Some(name) => lines.push(crate::tf!("Default printer: {name}").into_owned()),
        None => lines.push(crate::t!("Default printer: none reported by the system").into_owned()),
    }
    match list_printers() {
        Ok(printers) if printers.is_empty() => {
            lines.push(crate::t!("Printers: none listed by the system").into_owned());
        }
        Ok(printers) => {
            let count = printers.len();
            lines.push(crate::tf!("Printers ({count}):").into_owned());
            lines.extend(printers.into_iter().map(|name| format!("  {name}")));
        }
        Err(error) => lines.push(crate::tf!("Could not list printers: {error}").into_owned()),
    }
    #[cfg(target_os = "windows")]
    {
        match pdf_printto_command() {
            Some(command) => lines.push(
                crate::tf!("Plots go through the PDF application: {command}").into_owned(),
            ),
            None => lines.push(
                crate::t!(
                    "Plots go straight to the printer (no PDF application registers a print verb)."
                )
                .into_owned(),
            ),
        }
    }
    lines
}

/// What `printer` reports about its sheets, one line per sheet plus the
/// default and the margins — `PRINTERS <name>`. Asks the driver (and caches
/// the answer like the plot dialog does).
#[cfg(not(target_arch = "wasm32"))]
pub fn printer_media_report(printer: &str) -> Vec<String> {
    let Some(caps) = crate::io::plot_device::printer_capabilities(printer) else {
        return vec![crate::tf!(
            "{printer}: the printer reports no sheets; the paper catalogue is used."
        )
        .into_owned()];
    };
    let mut lines = Vec::new();
    let count = caps.media.len();
    lines.push(crate::tf!("{printer}: {count} sheet(s) reported").into_owned());
    for media in &caps.media {
        let m = media.margins;
        let borderless = if media.borderless { " (borderless available)" } else { "" };
        lines.push(format!(
            "  {} — margins L {:.1} B {:.1} R {:.1} T {:.1} mm{borderless}",
            media.paper.display(),
            m.left,
            m.bottom,
            m.right,
            m.top
        ));
    }
    match &caps.default_paper {
        Some(paper) => {
            let sheet = paper.display();
            lines.push(crate::tf!("Default sheet: {sheet}").into_owned());
        }
        None => lines.push(crate::t!("Default sheet: not reported").into_owned()),
    }
    lines
}

#[cfg(target_arch = "wasm32")]
pub fn printer_media_report(_printer: &str) -> Vec<String> {
    vec![crate::t!("Printing is not available in the web version.").into_owned()]
}

#[cfg(target_arch = "wasm32")]
pub fn printer_report() -> Vec<String> {
    vec![crate::t!("Printing is not available in the web version.").into_owned()]
}

/// The Windows default printer, as the spooler names it.
#[cfg(target_os = "windows")]
pub(crate) fn windows_default_printer() -> std::io::Result<String> {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_FILE_NOT_FOUND};
    use windows_sys::Win32::Graphics::Printing::GetDefaultPrinterW;

    let mut needed = 0u32;
    // SAFETY: the first call only asks for the size; the second writes into
    // a buffer of exactly that size.
    unsafe { GetDefaultPrinterW(std::ptr::null_mut(), &mut needed) };
    if needed == 0 {
        // Windows answers ERROR_FILE_NOT_FOUND when no printer is the default.
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No default printer is configured on Windows.",
        ));
    }
    let mut buf = vec![0u16; needed as usize];
    if unsafe { GetDefaultPrinterW(buf.as_mut_ptr(), &mut needed) } == 0 {
        let code = unsafe { GetLastError() };
        if code == ERROR_FILE_NOT_FOUND {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No default printer is configured on Windows.",
            ));
        }
        return Err(std::io::Error::from_raw_os_error(code as i32));
    }
    let end = buf.iter().position(|&u| u == 0).unwrap_or(0);
    Ok(String::from_utf16_lossy(&buf[..end]))
}

/// The command Windows runs for the `printto` verb on `.pdf` files — the
/// registered PDF application's "print to a named printer" entry — or
/// `None` when no application registered one (Edge and store readers only
/// register `open`), in which case a plot goes straight to the spooler.
#[cfg(target_os = "windows")]
pub(crate) fn pdf_printto_command() -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{AssocQueryStringW, ASSOCF_NOTRUNCATE, ASSOCSTR_COMMAND};

    let wide =
        |s: &str| -> Vec<u16> { std::ffi::OsStr::new(s).encode_wide().chain(Some(0)).collect() };
    let extension = wide(".pdf");
    let verb = wide("printto");
    let mut len = 0u32;
    // SAFETY: strings are NUL-terminated; a null buffer asks for the size,
    // which the call reports with S_FALSE rather than S_OK.
    let sized = unsafe {
        AssocQueryStringW(
            ASSOCF_NOTRUNCATE,
            ASSOCSTR_COMMAND,
            extension.as_ptr(),
            verb.as_ptr(),
            std::ptr::null_mut(),
            &mut len,
        )
    };
    if sized < 0 || len == 0 {
        return None;
    }
    let mut buf = vec![0u16; len as usize];
    let filled = unsafe {
        AssocQueryStringW(
            ASSOCF_NOTRUNCATE,
            ASSOCSTR_COMMAND,
            extension.as_ptr(),
            verb.as_ptr(),
            buf.as_mut_ptr(),
            &mut len,
        )
    };
    if filled < 0 {
        return None;
    }
    let end = buf.iter().position(|&u| u == 0).unwrap_or(buf.len());
    let command = String::from_utf16_lossy(&buf[..end]);
    (!command.trim().is_empty()).then_some(command)
}

/// Where a Windows plot goes and how. The route is decided before any
/// system call so the decision itself can be tested anywhere.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) enum PrintRoute {
    /// Hand the PDF to the registered PDF application's `printto` verb,
    /// naming the printer explicitly.
    PdfApplication { printer: String },
    /// Rasterise in-process and draw to the printer through the spooler.
    Direct { printer: String },
}

impl PrintRoute {
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(crate) fn printer(&self) -> &str {
        match self {
            PrintRoute::PdfApplication { printer } | PrintRoute::Direct { printer } => printer,
        }
    }
}

/// Resolve the dialog's choice to a route. "Default" (no name) becomes the
/// system default printer *by name*, so the job can never drift to whatever
/// the PDF application considers its default; the PDF application is used
/// only when it registered a `printto` verb.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn plan_print_route(
    requested: Option<&str>,
    default_printer: Option<&str>,
    printto_registered: bool,
) -> Result<PrintRoute, String> {
    let requested = requested.map(str::trim).filter(|name| !name.is_empty());
    let printer = match requested.or(default_printer.map(str::trim).filter(|n| !n.is_empty())) {
        Some(name) => name.to_string(),
        None => return Err("No default printer is configured on Windows.".into()),
    };
    Ok(if printto_registered {
        PrintRoute::PdfApplication { printer }
    } else {
        PrintRoute::Direct { printer }
    })
}

#[cfg(test)]
mod print_route_tests {
    use super::{plan_print_route, PrintRoute};

    #[test]
    fn a_named_printer_is_kept_and_blank_means_the_default() {
        assert_eq!(
            plan_print_route(Some("Office Laser"), Some("Home Inkjet"), false),
            Ok(PrintRoute::Direct { printer: "Office Laser".into() })
        );
        assert_eq!(
            plan_print_route(Some("  "), Some("Home Inkjet"), false),
            Ok(PrintRoute::Direct { printer: "Home Inkjet".into() })
        );
        assert_eq!(
            plan_print_route(None, Some(" Home Inkjet "), true),
            Ok(PrintRoute::PdfApplication { printer: "Home Inkjet".into() })
        );
    }

    #[test]
    fn no_printer_at_all_is_an_error_not_a_guess() {
        assert!(plan_print_route(None, None, true).is_err());
        assert!(plan_print_route(Some(""), Some(""), false).is_err());
    }

    #[test]
    fn the_pdf_application_is_used_only_with_a_printto_verb() {
        let direct = plan_print_route(Some("X"), None, false).unwrap();
        let via_app = plan_print_route(Some("X"), None, true).unwrap();
        assert!(matches!(direct, PrintRoute::Direct { .. }));
        assert!(matches!(via_app, PrintRoute::PdfApplication { .. }));
        assert_eq!(direct.printer(), "X");
    }
}

#[cfg(target_os = "windows")]
fn windows_printers() -> std::io::Result<Vec<String>> {
    use windows_sys::Win32::Graphics::Printing::{
        PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL, PRINTER_ENUM_NETWORK,
    };

    let mut names = enumerate_printers(PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS)?;
    // LOCAL|CONNECTIONS misses network-discovered printers (spooler-shared and
    // per-machine connections), which is what "the plot dialog lost printers"
    // reports look like. A second NETWORK-augmented pass can only widen the
    // list: when the spooler rejects the extra flag it just yields nothing.
    if let Ok(extra) =
        enumerate_printers(PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS | PRINTER_ENUM_NETWORK)
    {
        names.extend(extra);
        names.sort();
        names.dedup();
    }
    Ok(names)
}

#[cfg(target_os = "windows")]
fn enumerate_printers(flags: u32) -> std::io::Result<Vec<String>> {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_INSUFFICIENT_BUFFER};
    use windows_sys::Win32::Graphics::Printing::EnumPrintersW;

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
                flags,
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
    let named = printer.map(str::trim).filter(|name| !name.is_empty());

    // `/e` opens the driver's *printing preferences* (paper, tray, duplex,
    // quality — what a print job honours), not the device's admin properties
    // sheet that `/p` shows.
    #[cfg(target_os = "windows")]
    let command = match named {
        Some(name) => (
            "rundll32.exe",
            vec![
                "printui.dll,PrintUIEntry".to_string(),
                "/e".to_string(),
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
        // Reached only outside Linux; Linux opens the desktop's own panel.
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
    #[cfg(target_os = "linux")]
    {
        return open_desktop_printer_settings(printer);
    }
    #[allow(unreachable_code)]
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

/// Raster resolution for the GDI fallback print. 300 DPI balances spool
/// memory (a 297mm-wide page is ~3.5k pixels) against readable linework; the
/// on-screen rasteriser runs at 150 DPI, which looks soft on paper.
#[cfg(target_os = "windows")]
const GDI_PRINT_DPI: f32 = 300.0;

/// Print by rasterising the PDF in-process (hayro) and drawing the bitmap to
/// the printer DC with GDI. Needs no PDF application — the ShellExecute verb
/// path only works when the registered PDF viewer exposes `print`/`printto`,
/// and many (Edge, store readers like ASC.Pdf) register only `open`.
/// Rotate a top-down RGBA raster 90° clockwise (width and height swap).
#[cfg(target_os = "windows")]
fn rotate_rgba90(pixels: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut rotated = vec![0u8; pixels.len()];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let src = (y * width as usize + x) * 4;
            // Clockwise: destination column = height-1-y, row = x.
            let dst = (x * height as usize + (height as usize - 1 - y)) * 4;
            rotated[dst..dst + 4].copy_from_slice(&pixels[src..src + 4]);
        }
    }
    rotated
}

/// Open `device`, fetch the driver's default DEVMODE (public struct plus the
/// driver's private data behind it) and let `edit` change it while the
/// printer is still open — a change has to be reconciled by the driver
/// through `DocumentPropertiesW` with that handle. `edit` returns `false`
/// to reject the buffer. `None` when the driver could not be queried.
#[cfg(target_os = "windows")]
fn with_printer_devmode(
    device_wide: &[u16],
    edit: impl FnOnce(windows_sys::Win32::Graphics::Printing::PRINTER_HANDLE, &mut Vec<u8>) -> bool,
) -> Option<Vec<u8>> {
    use windows_sys::Win32::Graphics::Gdi::{DEVMODEW, DM_OUT_BUFFER};
    use windows_sys::Win32::Graphics::Printing::{
        ClosePrinter, DocumentPropertiesW, OpenPrinterW, PRINTER_HANDLE,
    };
    unsafe {
        let mut handle = PRINTER_HANDLE::default();
        if OpenPrinterW(device_wide.as_ptr(), &mut handle, std::ptr::null()) == 0 {
            return None;
        }
        let result = (|| {
            let needed = DocumentPropertiesW(
                std::ptr::null_mut(),
                handle,
                device_wide.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null(),
                DM_OUT_BUFFER,
            );
            if needed <= 0 {
                return None;
            }
            let mut buf = vec![0u8; needed as usize];
            let dm = buf.as_mut_ptr() as *mut DEVMODEW;
            if DocumentPropertiesW(
                std::ptr::null_mut(),
                handle,
                device_wide.as_ptr(),
                dm,
                std::ptr::null(),
                DM_OUT_BUFFER,
            ) <= 0
            {
                return None;
            }
            // The driver's own dmSize locates its private data
            // (dmDriverExtra) behind the public struct; only a driver that
            // left it zero gets the public size.
            if (*dm).dmSize == 0 {
                (*dm).dmSize = std::mem::size_of::<DEVMODEW>() as u16;
            }
            edit(handle, &mut buf).then_some(buf)
        })();
        ClosePrinter(handle);
        result
    }
}

/// The sheet a print job asks the driver for: one of the driver's own
/// sheets by id, or a user-defined size in tenths of a millimetre.
#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DevmodeSheet {
    Id(u16),
    UserMm { width_tenths: i16, length_tenths: i16 },
}

/// Fetch the printer driver's DEVMODE, flip its orientation to match the
/// plot's aspect (`landscape` = image wider than tall) and, when a sheet is
/// given, select it. The merged DEVMODE is returned as raw bytes (drivers
/// append private data after the struct). A driver that rejects the sheet
/// is asked again for the orientation alone; `None` when the driver could
/// not be queried — CreateDC then falls back to the driver default.
#[cfg(target_os = "windows")]
fn oriented_printer_devmode(
    device_wide: &[u16],
    landscape: bool,
    sheet: Option<DevmodeSheet>,
) -> Option<Vec<u8>> {
    use windows_sys::Win32::Graphics::Gdi::{
        DEVMODEW, DM_IN_BUFFER, DM_ORIENTATION, DM_OUT_BUFFER, DM_PAPERLENGTH, DM_PAPERSIZE,
        DM_PAPERWIDTH, DMORIENT_LANDSCAPE, DMORIENT_PORTRAIT,
    };
    use windows_sys::Win32::Graphics::Printing::DocumentPropertiesW;
    let attempt = |sheet: Option<DevmodeSheet>| {
        with_printer_devmode(device_wide, |handle, buf| unsafe {
            let dm = buf.as_mut_ptr() as *mut DEVMODEW;
            (*dm).dmFields |= DM_ORIENTATION;
            (*dm).Anonymous1.Anonymous1.dmOrientation = if landscape {
                DMORIENT_LANDSCAPE as i16
            } else {
                DMORIENT_PORTRAIT as i16
            };
            match sheet {
                Some(DevmodeSheet::Id(id)) => {
                    (*dm).dmFields |= DM_PAPERSIZE;
                    (*dm).dmFields &= !(DM_PAPERWIDTH | DM_PAPERLENGTH);
                    (*dm).Anonymous1.Anonymous1.dmPaperSize = id as i16;
                }
                Some(DevmodeSheet::UserMm { width_tenths, length_tenths }) => {
                    (*dm).dmFields |= DM_PAPERSIZE | DM_PAPERWIDTH | DM_PAPERLENGTH;
                    (*dm).Anonymous1.Anonymous1.dmPaperSize =
                        crate::io::windows_media::DMPAPER_USER as i16;
                    (*dm).Anonymous1.Anonymous1.dmPaperWidth = width_tenths;
                    (*dm).Anonymous1.Anonymous1.dmPaperLength = length_tenths;
                }
                None => {}
            }
            // Let the driver reconcile fields that depend on orientation and
            // sheet (paper dimensions, imageable area) before we print with it.
            DocumentPropertiesW(
                std::ptr::null_mut(),
                handle,
                device_wide.as_ptr(),
                dm,
                dm,
                DM_IN_BUFFER | DM_OUT_BUFFER,
            ) > 0
        })
    };
    match sheet {
        Some(sheet) => attempt(Some(sheet)).or_else(|| attempt(None)),
        None => attempt(None),
    }
}

/// The DEVMODE sheet for a plot of `width_mm` × `height_mm`: the driver's
/// own id when it lists that size, else a user-defined size (which the
/// driver may still refuse — the caller then prints on the default sheet).
#[cfg(target_os = "windows")]
fn devmode_sheet_for(printer: &str, width_mm: f64, height_mm: f64) -> Option<DevmodeSheet> {
    let tenths = |mm: f64| -> Option<i16> { i16::try_from((mm * 10.0).round() as i64).ok() };
    match windows_printer_media(printer) {
        Some(raw) => {
            let listed = crate::io::windows_media::paper_id_for_sheet(&raw, width_mm, height_mm);
            if let Some(id) = listed {
                return Some(DevmodeSheet::Id(id));
            }
            // A driver that lists sheets but not this one gets it as a
            // user-defined size, portrait (the orientation flag turns it).
            let (w, h) = if width_mm <= height_mm {
                (width_mm, height_mm)
            } else {
                (height_mm, width_mm)
            };
            Some(DevmodeSheet::UserMm {
                width_tenths: tenths(w)?,
                length_tenths: tenths(h)?,
            })
        }
        None => None,
    }
}

/// What the driver of `printer` says about its sheets: the three parallel
/// `DeviceCapabilities` tables, the default DEVMODE's sheet and the margins
/// of a portrait printer DC. `None` when the driver lists no sheet (a fax or
/// virtual queue) or cannot be asked at all — the dialog keeps the catalogue.
/// Slow for an offline network queue (the spooler may fetch the driver), so
/// it is only ever called from a background task.
#[cfg(target_os = "windows")]
pub(crate) fn windows_printer_media(
    printer: &str,
) -> Option<crate::io::windows_media::RawPrinterMedia> {
    use crate::io::windows_media::{DeviceCapsSample, RawPrinterMedia, PAPER_NAME_CHARS};
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateDCW, DeleteDC, GetDeviceCaps, DEVMODEW, DM_PAPERLENGTH, DM_PAPERSIZE,
        DM_PAPERWIDTH, HORZRES, LOGPIXELSX, LOGPIXELSY, PHYSICALHEIGHT, PHYSICALOFFSETX,
        PHYSICALOFFSETY, PHYSICALWIDTH, VERTRES,
    };
    use windows_sys::Win32::Storage::Xps::{
        DeviceCapabilitiesW, DC_PAPERNAMES, DC_PAPERS, DC_PAPERSIZE,
    };

    let wide =
        |s: &str| -> Vec<u16> { std::ffi::OsStr::new(s).encode_wide().chain(Some(0)).collect() };
    let device_wide = wide(printer.trim());
    // SAFETY: the device name is NUL-terminated; a null output buffer asks
    // for the count only.
    let count = |capability: u16| -> Option<usize> {
        let n = unsafe {
            DeviceCapabilitiesW(
                device_wide.as_ptr(),
                std::ptr::null(),
                capability,
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        (n > 0).then_some(n as usize)
    };
    // The three tables are parallel; a driver that disagrees on their
    // lengths is read up to the shortest.
    let count = count(DC_PAPERS)?
        .min(count(DC_PAPERSIZE)?)
        .min(count(DC_PAPERNAMES)?);
    let mut ids = vec![0u16; count];
    let mut sizes = vec![POINT { x: 0, y: 0 }; count];
    let mut names = vec![0u16; count * PAPER_NAME_CHARS];
    // SAFETY: each buffer holds `count` entries of the type the capability
    // writes (WORD, POINT, 64 WCHARs); the calls fill at most that many.
    let filled = unsafe {
        DeviceCapabilitiesW(
            device_wide.as_ptr(),
            std::ptr::null(),
            DC_PAPERS,
            ids.as_mut_ptr(),
            std::ptr::null(),
        ) > 0
            && DeviceCapabilitiesW(
                device_wide.as_ptr(),
                std::ptr::null(),
                DC_PAPERSIZE,
                sizes.as_mut_ptr().cast::<u16>(),
                std::ptr::null(),
            ) > 0
            && DeviceCapabilitiesW(
                device_wide.as_ptr(),
                std::ptr::null(),
                DC_PAPERNAMES,
                names.as_mut_ptr(),
                std::ptr::null(),
            ) > 0
    };
    if !filled {
        return None;
    }

    // The driver's default sheet, from its default DEVMODE — which also
    // makes the margin DC start portrait and on that sheet.
    let devmode = with_printer_devmode(&device_wide, |_, _| true);
    let (default_id, default_tenths_mm) = devmode
        .as_ref()
        .map(|buf| {
            // SAFETY: the buffer holds at least a public DEVMODEW.
            let dm = unsafe { &*(buf.as_ptr() as *const DEVMODEW) };
            let fields = dm.dmFields;
            let paper = unsafe { dm.Anonymous1.Anonymous1 };
            let id = (fields & DM_PAPERSIZE != 0).then_some(paper.dmPaperSize as u16);
            let size = (fields & DM_PAPERWIDTH != 0 && fields & DM_PAPERLENGTH != 0)
                .then_some((i32::from(paper.dmPaperWidth), i32::from(paper.dmPaperLength)));
            (id, size)
        })
        .unwrap_or((None, None));

    let winspool = wide("WINSPOOL");
    let init = devmode
        .as_ref()
        .map(|b| b.as_ptr().cast::<DEVMODEW>())
        .unwrap_or(std::ptr::null());
    // SAFETY: the strings are NUL-terminated and `devmode` outlives the DC.
    let hdc = unsafe { CreateDCW(winspool.as_ptr(), device_wide.as_ptr(), std::ptr::null(), init) };
    let margins = if hdc.is_null() {
        None
    } else {
        // SAFETY: hdc is a valid printer DC; each index is documented.
        let caps = |index: u32| unsafe { GetDeviceCaps(hdc, index as i32) };
        let sample = DeviceCapsSample {
            physical_width: caps(PHYSICALWIDTH),
            physical_height: caps(PHYSICALHEIGHT),
            offset_x: caps(PHYSICALOFFSETX),
            offset_y: caps(PHYSICALOFFSETY),
            horz_res: caps(HORZRES),
            vert_res: caps(VERTRES),
            dpi_x: caps(LOGPIXELSX),
            dpi_y: caps(LOGPIXELSY),
        };
        unsafe { DeleteDC(hdc) };
        crate::io::windows_media::device_margins_mm(&sample)
    };

    Some(RawPrinterMedia {
        ids,
        sizes_tenths_mm: sizes.into_iter().map(|p| (p.x, p.y)).collect(),
        names_utf16: names,
        default_id,
        default_tenths_mm,
        margins,
    })
}

#[cfg(target_os = "windows")]
fn gdi_raster_print(
    path: &std::path::Path,
    printer: &str,
    copies: u32,
    output_file: Option<&std::path::Path>,
) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateDCW, DeleteDC, GetDeviceCaps, SetStretchBltMode, StretchDIBits, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, COLORONCOLOR, DEVMODEW, DIB_RGB_COLORS, HORZRES, SRCCOPY,
        VERTRES,
    };
    // windows-sys 0.61 groups the spooler-document calls (StartDoc/StartPage/
    // EndPage/EndDoc/DOCINFOW) under Storage::Xps, not Graphics::Gdi.
    use windows_sys::Win32::Storage::Xps::{EndDoc, EndPage, StartDocW, StartPage, DOCINFOW};

    let wide = |s: &str| -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(Some(0))
            .collect()
    };

    // The device is resolved by the caller (`plan_print_route`): a name the
    // dialog chose or the system default looked up by name.
    let device = printer.trim();
    if device.is_empty() {
        return Err("No printer was chosen for direct printing.".into());
    }
    let device = device.to_string();

    // A plot is printed once; the on-screen memo of PDF pages must not keep
    // a 300 DPI raster of every job for the rest of the session.
    let page = crate::scene::model::pdf_raster::rasterize_page_at_dpi_uncached(
        &path.to_string_lossy(),
        "1",
        GDI_PRINT_DPI,
    )
    .ok_or_else(|| "Could not rasterise the plot for direct printing.".to_string())?;

    let winspool = wide("WINSPOOL");
    let device_wide = wide(&device);
    // The dialog's sheet and Landscape/Portrait choice ride in through the
    // DEVMODE: the raster's size decides both, and the driver's default must
    // not override them (a landscape A3 plot into the driver's portrait A4
    // used to shrink and misorient the output).
    let sheet_w_mm = f64::from(page.width) / f64::from(GDI_PRINT_DPI) * 25.4;
    let sheet_h_mm = f64::from(page.height) / f64::from(GDI_PRINT_DPI) * 25.4;
    let sheet = devmode_sheet_for(&device, sheet_w_mm, sheet_h_mm);
    let dm_buf = oriented_printer_devmode(&device_wide, page.width > page.height, sheet);
    let init = dm_buf
        .as_ref()
        .map(|b| b.as_ptr().cast::<DEVMODEW>())
        .unwrap_or(std::ptr::null());
    // SAFETY: all pointers are NUL-terminated / valid for the call; the DC is
    // released below and dm_buf outlives it.
    let hdc = unsafe {
        CreateDCW(
            winspool.as_ptr(),
            device_wide.as_ptr(),
            std::ptr::null(),
            init,
        )
    };
    if hdc.is_null() {
        return Err(format!("Could not open printer \"{device}\" for printing."));
    }

    let result = (|| -> Result<(), String> {
        // SAFETY: hdc is a valid printer DC; each capability is a documented index.
        let (page_w, page_h) = unsafe {
            (
                GetDeviceCaps(hdc, HORZRES as i32),
                GetDeviceCaps(hdc, VERTRES as i32),
            )
        };
        if page_w <= 0 || page_h <= 0 {
            return Err("Printer reported no printable area.".into());
        }

        // BGRA, top-down — what StretchDIBits expects for BI_RGB 32bpp.
        let mut bgra = page.pixels.as_ref().clone();
        for px in bgra.chunks_exact_mut(4) {
            px.swap(0, 2);
        }

        let mut src_w = page.width;
        let mut src_h = page.height;
        // Drivers whose DEVMODE could not be set (v4 class drivers may fail
        // the DocumentProperties query) keep their default orientation. When
        // that mismatches the plot sheet, rotating the raster 90° keeps the
        // plot at full size on the page instead of shrinking it to fit.
        if (page_w > page_h) != (src_w > src_h) {
            bgra = rotate_rgba90(&bgra, src_w, src_h);
            std::mem::swap(&mut src_w, &mut src_h);
        }

        // Fit inside the printable area, preserving the page's aspect ratio.
        let fit_w = f64::from(page_w) / f64::from(src_w);
        let fit_h = f64::from(page_h) / f64::from(src_h);
        let scale = fit_w.min(fit_h);
        let draw_w = (f64::from(src_w) * scale).max(1.0) as i32;
        let draw_h = (f64::from(src_h) * scale).max(1.0) as i32;
        let x = (page_w - draw_w) / 2;
        let y = (page_h - draw_h) / 2;

        let doc_name = wide("OpenCADStudio Plot");
        // An optional redirect writes the spooled output to a file instead of
        // the physical tray — how "Microsoft Print to PDF" prints silently,
        // and how the fallback proves itself in tests without wasting paper.
        let redirect = output_file.map(|p| wide(&p.to_string_lossy()));
        let info = DOCINFOW {
            cbSize: std::mem::size_of::<DOCINFOW>() as i32,
            lpszDocName: doc_name.as_ptr(),
            lpszOutput: redirect
                .as_ref()
                .map(|w| w.as_ptr())
                .unwrap_or(std::ptr::null()),
            lpszDatatype: std::ptr::null(),
            fwType: 0,
        };
        // SAFETY: info and its strings outlive every StartDoc/EndDoc call here.
        if unsafe { StartDocW(hdc, &info) } <= 0 {
            return Err(format!("Print job could not start (code {}).", unsafe {
                windows_sys::Win32::Foundation::GetLastError()
            }));
        }
        unsafe { SetStretchBltMode(hdc, COLORONCOLOR) };
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: src_w as i32,
                // Negative height: top-down rows, no vertical flip needed.
                biHeight: -(src_h as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: bgra.len() as u32,
                ..Default::default()
            },
            bmiColors: [Default::default()],
        };
        for _ in 0..copies.max(1) {
            if unsafe { StartPage(hdc) } <= 0 {
                return Err(format!("Could not start a print page (code {}).", unsafe {
                    windows_sys::Win32::Foundation::GetLastError()
                }));
            }
            // StartPage resets DC attributes, so the stretch mode is set per page.
            unsafe { SetStretchBltMode(hdc, COLORONCOLOR) };
            let drawn = unsafe {
                StretchDIBits(
                    hdc,
                    x,
                    y,
                    draw_w,
                    draw_h,
                    0,
                    0,
                    src_w as i32,
                    src_h as i32,
                    bgra.as_ptr().cast(),
                    &mut bmi,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                )
            };
            // Failure returns GDI_ERROR (-1); a successful draw returns the
            // source height it copied (>= 0, 0 only for zero-height pages).
            if drawn < 0 {
                return Err(format!(
                    "Drawing to the printer failed (code {}).",
                    unsafe { windows_sys::Win32::Foundation::GetLastError() }
                ));
            }
            if unsafe { EndPage(hdc) } <= 0 {
                return Err(format!(
                    "Could not finish a print page (code {}).",
                    unsafe { windows_sys::Win32::Foundation::GetLastError() }
                ));
            }
        }
        if unsafe { EndDoc(hdc) } <= 0 {
            return Err(format!("Print job could not complete (code {}).", unsafe {
                windows_sys::Win32::Foundation::GetLastError()
            }));
        }
        Ok(())
    })();

    // SAFETY: the DC was created above and used only on this thread.
    unsafe { DeleteDC(hdc) };
    result
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

/// Ask the registered PDF application to print `path` on `printer` through
/// its `printto` verb, once per copy. The printer name is quoted: some
/// `printto` command templates do not quote their `%2`.
#[cfg(target_os = "windows")]
fn shell_print_to(path: &std::path::Path, printer: &str, copies: u32) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NO_ASSOCIATION};
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SHELLEXECUTEINFOW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SE_ERR_NOASSOC,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let wide = |s: &str| -> Vec<u16> { OsStr::new(s).encode_wide().chain(Some(0)).collect() };
    let path_wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let verb = wide("printto");
    let params = wide(&format!("\"{printer}\""));
    for _ in 0..copies.max(1) {
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC,
            lpVerb: verb.as_ptr(),
            lpFile: path_wide.as_ptr(),
            lpParameters: params.as_ptr(),
            nShow: SW_HIDE,
            ..Default::default()
        };
        // SAFETY: every pointer in `info` outlives the call.
        if unsafe { ShellExecuteExW(&mut info) } == 0 {
            let shell_code = info.hInstApp as usize;
            let code = if (1..=32).contains(&shell_code) {
                shell_code as u32
            } else {
                unsafe { GetLastError() }
            };
            if code == SE_ERR_NOASSOC || code == ERROR_NO_ASSOCIATION {
                return Err("Windows has no PDF application registered with Print support.".into());
            }
            return Err(format!("Windows print dispatch failed (code {code})"));
        }
    }
    Ok(())
}

/// Dispatch a PDF to a specific printer with [`PrintOptions`].
#[cfg(not(target_arch = "wasm32"))]
fn dispatch_to_printer_opts(path: &std::path::Path, opts: &PrintOptions) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        let default_printer = windows_default_printer().ok();
        let route = plan_print_route(
            opts.printer.as_deref(),
            default_printer.as_deref(),
            pdf_printto_command().is_some(),
        )?;
        let printer = route.printer().to_string();
        let via_application = match &route {
            PrintRoute::PdfApplication { .. } => {
                match shell_print_to(path, &printer, opts.copies) {
                    Ok(()) => true,
                    // The PDF application refused the job: the spooler route
                    // still prints it, and the report says so.
                    Err(_) => {
                        gdi_raster_print(path, &printer, opts.copies, None)?;
                        false
                    }
                }
            }
            PrintRoute::Direct { .. } => {
                gdi_raster_print(path, &printer, opts.copies, None)?;
                false
            }
        };
        // A direct job is fully spooled when the calls return; the temporary
        // PDF behind it has no reader left. The PDF application reads its
        // file after ShellExecute returns, so that copy is left to the
        // temp folder.
        if !via_application && path.starts_with(std::env::temp_dir()) {
            let _ = std::fs::remove_file(path);
        }
        let how = if via_application {
            crate::t!("via the PDF application")
        } else {
            crate::t!("direct print")
        };
        Ok(format!("{printer} ({how})"))
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
        // The driver's own options, exactly as the PPD names them.
        for (key, value) in &opts.driver_options {
            cmd.arg("-o").arg(format!("{key}={value}"));
        }
        let lp_result = cmd.arg("--").arg(path_str.as_ref()).output();
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

    /// The GDI fallback rasterises the plot PDF in-process; prove the 300 DPI
    /// path yields a page sized for print rather than the screen's 150 DPI.
    #[cfg(target_os = "windows")]
    #[test]
    fn gdi_fallback_rasterises_the_plot_pdf_at_print_dpi() {
        use printpdf::{Mm, PdfDocument, PdfPage as OutputPage, PdfSaveOptions};

        let mut document = PdfDocument::new("gdi print test");
        document
            .pages
            .push(OutputPage::new(Mm(210.0), Mm(148.0), Vec::new()));
        let bytes = document.save(&PdfSaveOptions::default(), &mut Vec::new());
        let path = std::env::temp_dir().join("ocs_gdi_print_probe.pdf");
        std::fs::write(&path, &bytes).unwrap();

        let page = crate::scene::model::pdf_raster::rasterize_page_at_dpi(
            &path.to_string_lossy(),
            "1",
            super::GDI_PRINT_DPI,
        )
        .expect("plot PDF should rasterise");
        let _ = std::fs::remove_file(&path);

        assert_eq!(page.dpi, super::GDI_PRINT_DPI);
        // 210mm at 300 DPI ≈ 2480 px, aspect 210:148.
        assert!((page.width as f64 - 210.0 / 25.4 * 300.0).abs() < 2.0);
        assert!((page.height as f64 - 148.0 / 25.4 * 300.0).abs() < 2.0);
        assert_eq!(
            page.pixels.len(),
            page.width as usize * page.height as usize * 4
        );
    }

    /// Opening the printer DC and reading its printable area must work with
    /// the device-name resolution the GDI fallback uses. No job is spooled.
    #[cfg(target_os = "windows")]
    #[test]
    fn gdi_fallback_opens_a_printer_dc() {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Graphics::Gdi::{
            CreateDCW, DeleteDC, GetDeviceCaps, HORZRES, VERTRES,
        };

        let wide = |s: &str| -> Vec<u16> {
            std::ffi::OsStr::new(s)
                .encode_wide()
                .chain(Some(0))
                .collect()
        };
        let winspool = wide("WINSPOOL");
        let device = wide("Microsoft Print to PDF");
        // SAFETY: NUL-terminated strings; the DC is released before returning.
        let hdc = unsafe {
            CreateDCW(
                winspool.as_ptr(),
                device.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        assert!(!hdc.is_null(), "printer DC should open");
        // SAFETY: capability indexes on a valid DC.
        let (w, h) = unsafe {
            (
                GetDeviceCaps(hdc, HORZRES as i32),
                GetDeviceCaps(hdc, VERTRES as i32),
            )
        };
        // SAFETY: releasing the DC opened above.
        unsafe { DeleteDC(hdc) };
        assert!(
            w > 0 && h > 0,
            "printable area should be reported, got {w}x{h}"
        );
    }

    /// End to end through the real spooler: plot a page with black ink, run
    /// the GDI fallback against "Microsoft Print to PDF" redirected to a file
    /// (no dialog, no paper), then rasterise the result and require that ink
    /// actually landed on the page. Catches off-page draws (the blank-paper
    /// regression) without a physical printer.
    #[cfg(target_os = "windows")]
    #[test]
    fn gdi_fallback_lands_ink_on_the_printed_page() {
        use printpdf::{Color, Mm, PdfDocument, PdfPage as OutputPage, PdfSaveOptions, Rgb};

        let mut document = PdfDocument::new("gdi ink test");
        let mut ops = Vec::new();
        // A big black block in the middle of an A4 page.
        ops.push(printpdf::Op::SetFillColor {
            col: Color::Rgb(Rgb {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                icc_profile: None,
            }),
        });
        // printpdf 0.9's DrawRectangle never paints (its serializer always
        // ends the path with `n`), so the ink must be a filled polygon — the
        // same op the plot pipeline draws hatches with.
        ops.push(printpdf::Op::DrawPolygon {
            polygon: printpdf::Polygon {
                rings: vec![printpdf::PolygonRing {
                    points: vec![
                        printpdf::LinePoint {
                            p: printpdf::Point {
                                x: Mm(60.0).into(),
                                y: Mm(60.0).into(),
                            },
                            bezier: false,
                        },
                        printpdf::LinePoint {
                            p: printpdf::Point {
                                x: Mm(150.0).into(),
                                y: Mm(60.0).into(),
                            },
                            bezier: false,
                        },
                        printpdf::LinePoint {
                            p: printpdf::Point {
                                x: Mm(150.0).into(),
                                y: Mm(100.0).into(),
                            },
                            bezier: false,
                        },
                        printpdf::LinePoint {
                            p: printpdf::Point {
                                x: Mm(60.0).into(),
                                y: Mm(100.0).into(),
                            },
                            bezier: false,
                        },
                    ],
                }],
                mode: printpdf::PaintMode::Fill,
                winding_order: printpdf::WindingOrder::NonZero,
            },
        });
        document
            .pages
            .push(OutputPage::new(Mm(210.0), Mm(148.0), ops));
        let bytes = document.save(&PdfSaveOptions::default(), &mut Vec::new());
        let plot = std::env::temp_dir().join("ocs_gdi_ink_source.pdf");
        std::fs::write(&plot, &bytes).unwrap();

        let printed = std::env::temp_dir().join("ocs_gdi_ink_output.pdf");
        let _ = std::fs::remove_file(&printed);

        let result = super::gdi_raster_print(&plot, "Microsoft Print to PDF", 1, Some(&printed));
        assert!(result.is_ok(), "GDI print failed: {result:?}");

        // The spooled PDF must contain the black block: rasterise it and
        // count clearly-dark pixels.
        let page = crate::scene::model::pdf_raster::rasterize_page(&printed.to_string_lossy(), "1")
            .expect("spooled output should rasterise");
        let dark = page
            .pixels
            .chunks_exact(4)
            .filter(|px| px[0] < 100 && px[1] < 100 && px[2] < 100)
            .count();
        std::fs::remove_file(&plot).ok();
        std::fs::remove_file(&printed).ok();
        assert!(
            dark > page.width as usize * page.height as usize / 50,
            "printed page should carry ink; only {dark} dark pixels"
        );
    }

    #[cfg(target_os = "windows")]
    /// The plot sheet's orientation must drive the printer's DEVMODE: a
    /// landscape sheet spooled through "Microsoft Print to PDF" (whose
    /// default is portrait) has to come out as a landscape page.
    #[cfg(target_os = "windows")]
    #[test]
    fn gdi_fallback_prints_landscape_sheets_landscape() {
        use printpdf::{Color, Mm, PdfDocument, PdfPage as OutputPage, PdfSaveOptions, Rgb};

        let mut document = PdfDocument::new("gdi orientation test");
        let mut ops = Vec::new();
        ops.push(printpdf::Op::SetFillColor {
            col: Color::Rgb(Rgb {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                icc_profile: None,
            }),
        });
        ops.push(printpdf::Op::DrawPolygon {
            polygon: printpdf::Polygon {
                rings: vec![printpdf::PolygonRing {
                    points: vec![
                        printpdf::LinePoint {
                            p: printpdf::Point {
                                x: Mm(20.0).into(),
                                y: Mm(20.0).into(),
                            },
                            bezier: false,
                        },
                        printpdf::LinePoint {
                            p: printpdf::Point {
                                x: Mm(270.0).into(),
                                y: Mm(20.0).into(),
                            },
                            bezier: false,
                        },
                        printpdf::LinePoint {
                            p: printpdf::Point {
                                x: Mm(270.0).into(),
                                y: Mm(45.0).into(),
                            },
                            bezier: false,
                        },
                        printpdf::LinePoint {
                            p: printpdf::Point {
                                x: Mm(20.0).into(),
                                y: Mm(45.0).into(),
                            },
                            bezier: false,
                        },
                    ],
                }],
                mode: printpdf::PaintMode::Fill,
                winding_order: printpdf::WindingOrder::NonZero,
            },
        });
        // Landscape A4: 297 × 210 mm.
        document
            .pages
            .push(OutputPage::new(Mm(297.0), Mm(210.0), ops));
        let bytes = document.save(&PdfSaveOptions::default(), &mut Vec::new());
        let plot = std::env::temp_dir().join("ocs_gdi_orientation_source.pdf");
        std::fs::write(&plot, &bytes).unwrap();
        let printed = std::env::temp_dir().join("ocs_gdi_orientation_output.pdf");
        let _ = std::fs::remove_file(&printed);

        let result = super::gdi_raster_print(&plot, "Microsoft Print to PDF", 1, Some(&printed));
        assert!(result.is_ok(), "GDI print failed: {result:?}");

        let page = crate::scene::model::pdf_raster::rasterize_page(&printed.to_string_lossy(), "1")
            .expect("spooled output should rasterise");
        std::fs::remove_file(&plot).ok();
        std::fs::remove_file(&printed).ok();

        // Whichever mechanism wins — DEVMODE orientation or the raster
        // rotation fallback — the ink must land on the page and the plot's
        // long side must run along the page's long side.
        let mut min_x = page.width;
        let mut max_x = 0u32;
        let mut min_y = page.height;
        let mut max_y = 0u32;
        let mut dark = 0usize;
        for y in 0..page.height {
            for x in 0..page.width {
                let px = &page.pixels
                    [((y * page.width + x) * 4) as usize..((y * page.width + x) * 4 + 3) as usize];
                if px.iter().all(|&c| c < 100) {
                    dark += 1;
                    min_x = min_x.min(x);
                    max_x = max_x.max(x);
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
            }
        }
        assert!(
            dark > 1_000,
            "printed page must carry ink; {dark} dark pixels"
        );
        let ink_w = (max_x - min_x + 1) as f64;
        let ink_h = (max_y - min_y + 1) as f64;
        assert_eq!(
            ink_w > ink_h,
            page.width > page.height,
            "ink aspect must follow the page: ink {:.0}x{:.0} on page {}x{}",
            ink_w,
            ink_h,
            page.width,
            page.height
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_opens_selected_printer_or_printer_list() {
        assert_eq!(
            printer_properties_command(Some("  Office LaserJet  ")),
            (
                "rundll32.exe",
                vec![
                    "printui.dll,PrintUIEntry".to_string(),
                    "/e".to_string(),
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
            vec!["x-apple.systempreferences:com.apple.Print-Scan-Settings.extension".to_string()],
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

// ── Driver options (CUPS PPD) ─────────────────────────────────────────────

/// One option a CUPS printer's driver exposes, as `lpoptions -l` lists it:
/// the PPD keyword, its description, the choices, and the current default.
#[derive(Clone, Debug, PartialEq)]
pub struct PrinterOption {
    /// PPD keyword, e.g. `MediaType`, passed back verbatim as `-o key=value`.
    pub key: String,
    /// The driver's description, e.g. `Media Type`.
    pub label: String,
    /// Keyword of the currently selected choice.
    pub default: String,
    pub choices: Vec<PrinterChoice>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrinterChoice {
    /// PPD choice keyword, e.g. `PhotographicHighGloss`.
    pub keyword: String,
    /// Readable form of the keyword, e.g. `Photographic High Gloss`.
    pub label: String,
}

impl PrinterOption {
    pub fn label_for(&self, keyword: &str) -> String {
        self.choices
            .iter()
            .find(|choice| choice.keyword == keyword)
            .map(|choice| choice.label.clone())
            .unwrap_or_else(|| humanize_keyword(keyword))
    }
}

/// Ask CUPS for the options of `printer` (blocking, one `lpoptions` spawn).
/// The sheet (`PageSize`) is left out: the plot dialog chooses it.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "windows")))]
pub fn printer_options(printer: &str) -> Result<Vec<PrinterOption>, String> {
    let output = std::process::Command::new("lpoptions")
        .args(["-p", printer, "-l"])
        .output()
        .map_err(|error| format!("lpoptions: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(parse_lpoptions(&String::from_utf8_lossy(&output.stdout)))
}

/// Windows drivers keep their options behind `DocumentProperties`; there is
/// no list to show in-line.
#[cfg(any(target_arch = "wasm32", target_os = "windows"))]
pub fn printer_options(_printer: &str) -> Result<Vec<PrinterOption>, String> {
    Ok(Vec::new())
}

/// Parse `lpoptions -l` output: one option per line as
/// `Key/Description: choice *default choice …`, the asterisk marking the
/// current default. Numeric-range options print the same way and are kept.
pub fn parse_lpoptions(text: &str) -> Vec<PrinterOption> {
    let mut options = Vec::new();
    for line in text.lines() {
        let Some((head, tail)) = line.split_once(':') else {
            continue;
        };
        let (key, label) = head
            .split_once('/')
            .map(|(key, label)| (key.trim(), label.trim()))
            .unwrap_or((head.trim(), head.trim()));
        if key.is_empty() || key.eq_ignore_ascii_case("PageSize") {
            continue;
        }
        let mut default = String::new();
        let choices: Vec<PrinterChoice> = tail
            .split_whitespace()
            .map(|token| {
                let (keyword, is_default) = match token.strip_prefix('*') {
                    Some(keyword) => (keyword, true),
                    None => (token, false),
                };
                if is_default {
                    default = keyword.to_string();
                }
                PrinterChoice {
                    keyword: keyword.to_string(),
                    label: humanize_keyword(keyword),
                }
            })
            .collect();
        if choices.is_empty() {
            continue;
        }
        if default.is_empty() {
            default = choices[0].keyword.clone();
        }
        options.push(PrinterOption {
            key: key.to_string(),
            label: if label.is_empty() {
                key.to_string()
            } else {
                label.to_string()
            },
            default,
            choices,
        });
    }
    options
}

/// `PhotographicHighGloss` → `Photographic High Gloss`, `DuplexNoTumble` →
/// `Duplex No Tumble`; keywords that are already words or numbers pass
/// through. Consecutive capitals (`RGB`, `A4`) stay together.
pub fn humanize_keyword(keyword: &str) -> String {
    let mut out = String::with_capacity(keyword.len() + 4);
    let chars: Vec<char> = keyword.chars().collect();
    for (index, &ch) in chars.iter().enumerate() {
        if index > 0 && ch.is_ascii_uppercase() {
            let previous = chars[index - 1];
            let next_lower = chars.get(index + 1).is_some_and(|c| c.is_ascii_lowercase());
            if previous.is_ascii_lowercase() || (previous.is_ascii_uppercase() && next_lower) {
                out.push(' ');
            }
        }
        out.push(ch);
    }
    out
}

/// The desktop environment's printer settings panel — where the driver
/// options cannot be listed (no CUPS) the user still gets somewhere useful.
/// Tries the panel of the running desktop first, then the
/// distribution-neutral tools, and only then the CUPS web interface.
#[cfg(target_os = "linux")]
pub fn open_desktop_printer_settings(printer: Option<&str>) -> Result<(), String> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut candidates: Vec<(&str, Vec<String>)> = Vec::new();
    if desktop.contains("gnome") || desktop.contains("ubuntu") || desktop.contains("cinnamon") {
        candidates.push(("gnome-control-center", vec!["printers".into()]));
    }
    if desktop.contains("kde") || desktop.contains("plasma") {
        candidates.push(("kcmshell6", vec!["kcm_printer_manager".into()]));
        candidates.push(("kcmshell5", vec!["kcm_printer_manager".into()]));
    }
    candidates.push(("system-config-printer", Vec::new()));
    candidates.push(("gnome-control-center", vec!["printers".into()]));
    for (program, args) in candidates {
        if std::process::Command::new(program)
            .args(&args)
            .spawn()
            .is_ok()
        {
            return Ok(());
        }
    }
    let target = printer
        .map(|name| format!("http://localhost:631/printers/{name}"))
        .unwrap_or_else(|| "http://localhost:631/printers".to_string());
    std::process::Command::new("xdg-open")
        .arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open printer settings: {error}"))
}

/// Show the driver's own document-properties sheet for `printer`, owned by
/// the window `owner` (an `HWND`, 0 for none), and on OK store the result as
/// the user's printing preferences for that printer — which is what the
/// print job then honours. Returns `Ok(false)` when the user cancelled.
#[cfg(target_os = "windows")]
pub fn edit_printer_preferences(printer: &str, owner: isize) -> Result<bool, String> {
    use windows_sys::Win32::Graphics::Gdi::DEVMODEW;
    use windows_sys::Win32::Graphics::Printing::{
        ClosePrinter, DocumentPropertiesW, OpenPrinterW, SetPrinterW, PRINTER_HANDLE,
        PRINTER_INFO_9W,
    };
    // winspool.h: DM_OUT_BUFFER = DM_COPY, DM_IN_PROMPT = DM_PROMPT,
    // DM_IN_BUFFER = DM_MODIFY.
    const DM_OUT_BUFFER: u32 = 2;
    const DM_IN_PROMPT: u32 = 4;
    const DM_IN_BUFFER: u32 = 8;
    const IDOK: i32 = 1;

    let name: Vec<u16> = printer.encode_utf16().chain(std::iter::once(0)).collect();
    let mut handle = PRINTER_HANDLE {
        Value: std::ptr::null_mut(),
    };
    // SAFETY: `name` is NUL-terminated and outlives the call; a null default
    // asks for PRINTER_ACCESS_USE, enough for per-user preferences.
    if unsafe { OpenPrinterW(name.as_ptr(), &mut handle, std::ptr::null()) } == 0 {
        return Err(format!("Could not open printer \"{printer}\""));
    }
    let owner = owner as windows_sys::Win32::Foundation::HWND;
    let outcome = (|| {
        // SAFETY: with null buffers the call only reports the DEVMODE size.
        let size = unsafe {
            DocumentPropertiesW(
                owner,
                handle,
                name.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
            )
        };
        if size <= 0 {
            return Err(format!(
                "Printer \"{printer}\" reports no document properties"
            ));
        }
        // DEVMODEW is u16-aligned; a u16 buffer keeps the cast sound.
        let words = (size as usize).div_ceil(2);
        let mut current = vec![0u16; words];
        let mut edited = vec![0u16; words];
        // SAFETY: both buffers hold `size` bytes as the driver requested.
        let fetched = unsafe {
            DocumentPropertiesW(
                owner,
                handle,
                name.as_ptr(),
                current.as_mut_ptr().cast::<DEVMODEW>(),
                std::ptr::null(),
                DM_OUT_BUFFER,
            )
        };
        if fetched < 0 {
            return Err(format!("Could not read the preferences of \"{printer}\""));
        }
        edited.copy_from_slice(&current);
        // SAFETY: as above; DM_IN_PROMPT runs the driver's modal sheet.
        let response = unsafe {
            DocumentPropertiesW(
                owner,
                handle,
                name.as_ptr(),
                edited.as_mut_ptr().cast::<DEVMODEW>(),
                current.as_ptr().cast::<DEVMODEW>(),
                DM_IN_BUFFER | DM_IN_PROMPT | DM_OUT_BUFFER,
            )
        };
        if response != IDOK {
            return Ok(false);
        }
        let info = PRINTER_INFO_9W {
            pDevMode: edited.as_mut_ptr().cast::<DEVMODEW>(),
        };
        // SAFETY: level 9 takes a PRINTER_INFO_9W; the DEVMODE outlives the call.
        if unsafe { SetPrinterW(handle, 9, (&info as *const PRINTER_INFO_9W).cast::<u8>(), 0) } == 0
        {
            return Err(format!("Could not save the preferences of \"{printer}\""));
        }
        Ok(true)
    })();
    // SAFETY: `handle` came from OpenPrinterW above.
    unsafe { ClosePrinter(handle) };
    outcome
}

#[cfg(test)]
mod driver_option_tests {
    use super::*;

    const LPOPTIONS: &str = "PageSize/Media Size: 101.6x180.6mm *A4 A4.Borderless A5\n\
MediaType/Media Type: *Stationery PhotographicHighGloss Photographic Envelope\n\
ColorModel/Print Color Mode: *RGB Gray AutoGray ProcessGray\n\
Duplex/2-Sided Printing: *None DuplexNoTumble DuplexTumble\n\
cupsPrintQuality/Print Quality: *Normal High\n\
print-scaling/Print Scaling: auto auto-fit fill *fit none\n\
Broken line without colon\n";

    #[test]
    fn lpoptions_output_becomes_options_with_defaults() {
        let options = parse_lpoptions(LPOPTIONS);
        let keys: Vec<&str> = options.iter().map(|o| o.key.as_str()).collect();
        // PageSize is the plot dialog's business; everything else is listed.
        assert_eq!(
            keys,
            [
                "MediaType",
                "ColorModel",
                "Duplex",
                "cupsPrintQuality",
                "print-scaling"
            ]
        );
        let media = &options[0];
        assert_eq!(media.label, "Media Type");
        assert_eq!(media.default, "Stationery");
        assert_eq!(media.choices[1].keyword, "PhotographicHighGloss");
        assert_eq!(media.choices[1].label, "Photographic High Gloss");
        assert_eq!(media.label_for("Envelope"), "Envelope");
        let scaling = &options[4];
        assert_eq!(scaling.default, "fit");
        assert_eq!(scaling.choices.len(), 5);
    }

    #[test]
    fn keywords_read_as_words() {
        assert_eq!(humanize_keyword("DuplexNoTumble"), "Duplex No Tumble");
        assert_eq!(humanize_keyword("RGB"), "RGB");
        assert_eq!(humanize_keyword("AutoGray"), "Auto Gray");
        assert_eq!(humanize_keyword("ProcessGray"), "Process Gray");
        assert_eq!(humanize_keyword("auto-fit"), "auto-fit");
        assert_eq!(
            humanize_keyword("PhotographicSemiGloss"),
            "Photographic Semi Gloss"
        );
        assert_eq!(humanize_keyword("A4"), "A4");
    }
}
