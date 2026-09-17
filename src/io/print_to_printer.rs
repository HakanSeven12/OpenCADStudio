// print_to_printer — send the current layout to the system printer.
//
// Desktop strategy:
//   1. Render the drawing to a temporary PDF (reusing the PDF export pipeline).
//   2. Send that PDF to the system printer with `lp` (Linux/macOS) or
//      `ShellExecute PRINTTO` on Windows, naming the exact target printer —
//      including the spooler's own default when none was selected.
//
// The function is async so the UI remains responsive while the job is queued.
//
// The web build has no system printers: `preview_page_pdf` /
// `preview_pages_pdf` render the same PDF bytes, rasterise them, and show
// the pages in an in-page overlay — the way AutoCAD previews plot output on
// screen rather than handing a file to an external viewer.

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
    use windows_sys::Win32::Graphics::Printing::{
        PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL, PRINTER_INFO_2W, PRINTER_INFO_4W,
    };

    // Level 4 is served from the local registry cache and returns instantly,
    // but on Windows 11 it can miss per-user network printers that only the
    // spooler RPC (level 2) reports (#1261a). The level-2 query is expensive —
    // it opens every remote connection and can wait out RPC timeouts on dead
    // print servers — so it only runs when the cached query came back empty;
    // machines whose level-4 list works never pay for it. (The flags stay
    // LOCAL|CONNECTIONS on purpose: the EnumPrintersW documentation restricts
    // level 4 to exactly those flags, and PRINTER_ENUM_NETWORK is only valid
    // with level 1, so completeness comes from the level-2 retry, not flags.)
    let mut names = enum_printer_names(
        4,
        PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS,
        std::mem::size_of::<PRINTER_INFO_4W>(),
        // pPrinterName is the first field of PRINTER_INFO_4W.
        0,
    )?;
    if names.is_empty() {
        names = enum_printer_names(
            2,
            PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS,
            std::mem::size_of::<PRINTER_INFO_2W>(),
            // pPrinterName is the second pointer field of PRINTER_INFO_2W.
            std::mem::size_of::<usize>(),
        )?;
    }
    // The spooler's own default is authoritative even when enumeration missed
    // it; always offer it explicitly so the list can never hide it (#1261b).
    if let Some(default) = windows_default_printer() {
        if !names.iter().any(|name| name.eq_ignore_ascii_case(&default)) {
            names.push(default);
        }
    }
    // Windows treats printer names case-insensitively, so merge away case
    // variants between the two queries as well.
    names.sort_by_key(|name| name.to_ascii_lowercase());
    names.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    Ok(names)
}

/// Query the spooler with `EnumPrintersW` at `level` and return the sorted,
/// deduplicated printer names. `record_bytes` is the byte size of the level's
/// `PRINTER_INFO_*W` record and `name_offset` the byte offset of its
/// `pPrinterName` field.
#[cfg(target_os = "windows")]
fn enum_printer_names(
    level: u32,
    flags: u32,
    record_bytes: usize,
    name_offset: usize,
) -> std::io::Result<Vec<String>> {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_INSUFFICIENT_BUFFER};
    use windows_sys::Win32::Graphics::Printing::EnumPrintersW;

    debug_assert!(record_bytes % std::mem::size_of::<usize>() == 0);
    debug_assert!(name_offset % std::mem::size_of::<usize>() == 0);

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
                level,
                data,
                buffer_bytes,
                &mut bytes_needed,
                &mut printer_count,
            )
        };
        if success != 0 {
            return windows_printer_names(
                &buffer,
                printer_count as usize,
                record_bytes,
                name_offset,
            );
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
fn windows_printer_names(
    buffer: &[usize],
    printer_count: usize,
    record_bytes: usize,
    name_offset: usize,
) -> std::io::Result<Vec<String>> {
    use std::io::{Error, ErrorKind};

    if record_bytes == 0 || name_offset + std::mem::size_of::<usize>() > record_bytes {
        return Err(Error::from(ErrorKind::InvalidInput));
    }
    let buffer_bytes = std::mem::size_of_val(buffer);
    if printer_count > buffer_bytes / record_bytes {
        return Err(Error::from(ErrorKind::InvalidData));
    }
    let base = buffer.as_ptr().cast::<u8>();
    let mut names = Vec::with_capacity(printer_count);
    for index in 0..printer_count {
        let field = index * record_bytes + name_offset;
        if field > buffer_bytes || buffer_bytes - field < std::mem::size_of::<usize>() {
            return Err(Error::from(ErrorKind::InvalidData));
        }
        // SAFETY: the field lies inside the buffer; `read_unaligned` accepts
        // any address, and PRINTER_INFO_4W/_2W layouts keep their leading
        // pointer fields at word multiples so this reads the real pointer.
        let printer_name =
            unsafe { base.add(field).cast::<usize>().read_unaligned() } as *const u16;
        if printer_name.is_null() {
            continue;
        }
        let offset = (printer_name as usize)
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

/// The spooler's configured default printer, if any. `GetDefaultPrinterW` is
/// the only source that matches what every other Windows application means by
/// "default printer" (#1261b): the shell `print` verb instead hands the file
/// to the registered PDF handler, which applies its own — often different —
/// default, or just opens the viewer (#1261c).
#[cfg(target_os = "windows")]
fn windows_default_printer() -> Option<String> {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_INSUFFICIENT_BUFFER};
    use windows_sys::Win32::Graphics::Printing::GetDefaultPrinterW;

    let mut capacity = 0u32;
    // First call sizes the buffer: it fails with ERROR_INSUFFICIENT_BUFFER
    // when a default exists (capacity then includes the NUL terminator), and
    // with e.g. ERROR_FILE_NOT_FOUND when no default is configured at all.
    if unsafe { GetDefaultPrinterW(std::ptr::null_mut(), &mut capacity) } != 0 {
        return None;
    }
    if unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER || capacity == 0 {
        return None;
    }
    let mut buffer = vec![0u16; capacity as usize];
    if unsafe { GetDefaultPrinterW(buffer.as_mut_ptr(), &mut capacity) } == 0 {
        return None;
    }
    let length = buffer.iter().position(|&unit| unit == 0).unwrap_or(0);
    let name = String::from_utf16_lossy(&buffer[..length]);
    let trimmed = name.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Probe the spooler for `name` before dispatching a job (#1261c).
/// `OpenPrinterW` with no `PRINTER_DEFAULTSW` is a pure
/// existence/permission check: it spools nothing and shows no UI, and gives a
/// decisive error where the PDF handler would otherwise swallow a bad printer
/// name while `ShellExecuteExW` still reports success.
#[cfg(target_os = "windows")]
fn ensure_printer_exists(name: &str) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{
        GetLastError, ERROR_ACCESS_DENIED, ERROR_INVALID_PRINTER_NAME,
    };
    use windows_sys::Win32::Graphics::Printing::{ClosePrinter, OpenPrinterW, PRINTER_HANDLE};

    let wide: Vec<u16> = OsStr::new(name).encode_wide().chain(Some(0)).collect();
    let mut handle = PRINTER_HANDLE::default();
    // SAFETY: `wide` is NUL-terminated and `handle` is an out-parameter.
    if unsafe { OpenPrinterW(wide.as_ptr(), &mut handle, std::ptr::null()) } == 0 {
        let error = unsafe { GetLastError() };
        return Err(match error {
            ERROR_INVALID_PRINTER_NAME => {
                format!("printer '{name}' is not installed on this computer")
            }
            ERROR_ACCESS_DENIED => format!("access to printer '{name}' was denied"),
            error => format!("Windows could not open printer '{name}' (error {error})"),
        });
    }
    if !handle.Value.is_null() {
        // SAFETY: the handle was just returned by a successful OpenPrinterW.
        unsafe { ClosePrinter(handle) };
    }
    Ok(())
}

/// Human-readable reasons for the failure codes `ShellExecuteExW` reports.
/// Per the ShellExecute documentation, `hInstApp` values in 1..=32 are the
/// `SE_ERR_*` instance codes; anything else came from `GetLastError`.
#[cfg(target_os = "windows")]
fn describe_shell_execute_failure(code: u32) -> &'static str {
    use windows_sys::Win32::Foundation::ERROR_NO_ASSOCIATION;
    use windows_sys::Win32::UI::Shell::{
        SE_ERR_ACCESSDENIED, SE_ERR_ASSOCINCOMPLETE, SE_ERR_DDEBUSY, SE_ERR_DDEFAIL,
        SE_ERR_DDETIMEOUT, SE_ERR_DLLNOTFOUND, SE_ERR_FNF, SE_ERR_NOASSOC, SE_ERR_OOM,
        SE_ERR_PNF, SE_ERR_SHARE,
    };
    match code {
        0 => "the operating system is out of memory or resources",
        SE_ERR_FNF => "the rendered PDF file could not be found",
        SE_ERR_PNF => "the path to the rendered PDF file could not be found",
        SE_ERR_ACCESSDENIED => "Windows denied access to the print command",
        SE_ERR_OOM => "not enough memory to start the PDF application",
        SE_ERR_SHARE => "a sharing violation occurred while opening the PDF",
        SE_ERR_ASSOCINCOMPLETE => "the PDF file association is invalid or incomplete",
        SE_ERR_DDETIMEOUT => "the PDF application did not respond in time (DDE timeout)",
        SE_ERR_DDEFAIL => "the DDE print transaction failed",
        SE_ERR_DDEBUSY => "the PDF application was busy (DDE busy)",
        SE_ERR_DLLNOTFOUND => "a DLL required by the print handler is missing",
        SE_ERR_NOASSOC | ERROR_NO_ASSOCIATION => {
            "Windows has no PDF application registered with Print support"
        }
        _ => "unknown ShellExecute failure",
    }
}

#[cfg(all(test, target_os = "windows"))]
mod printer_buffer_tests {
    use super::{
        describe_shell_execute_failure, ensure_printer_exists, windows_default_printer,
        windows_printer_names,
    };
    use windows_sys::Win32::Graphics::Printing::{PRINTER_INFO_2W, PRINTER_INFO_4W};

    #[test]
    fn printer_names_stay_within_the_returned_buffer() {
        assert!(
            windows_printer_names(&[], 0, std::mem::size_of::<PRINTER_INFO_4W>(), 0)
                .unwrap()
                .is_empty()
        );
        assert!(
            windows_printer_names(&[], 1, std::mem::size_of::<PRINTER_INFO_4W>(), 0).is_err()
        );
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
                windows_printer_names(&buffer, 1, std::mem::size_of::<PRINTER_INFO_4W>(), 0)
                    .unwrap(),
                ["Printer \u{03b1}"]
            );
            (*record).pPrinterName = buffer.as_mut_ptr().cast::<u16>().wrapping_sub(1);
            assert!(
                windows_printer_names(&buffer, 1, std::mem::size_of::<PRINTER_INFO_4W>(), 0)
                    .is_err()
            );
            (*record).pPrinterName = name_ptr;
        }
        buffer[name_offset / std::mem::size_of::<usize>()..].fill(usize::MAX);
        assert!(
            windows_printer_names(&buffer, 1, std::mem::size_of::<PRINTER_INFO_4W>(), 0).is_err()
        );
    }

    /// Level 2 records keep `pPrinterName` in the second field; the name
    /// reader must honour the layout it is given.
    #[test]
    fn level2_names_read_the_second_field() {
        let record_bytes = std::mem::size_of::<PRINTER_INFO_2W>();
        let mut buffer = vec![0usize; record_bytes / std::mem::size_of::<usize>() + 8];
        let name: Vec<u16> = "Net \u{263a}\0".encode_utf16().collect();
        // SAFETY: the aligned allocation holds the record and UTF-16 name.
        unsafe {
            let record = buffer.as_mut_ptr().cast::<PRINTER_INFO_2W>();
            let name_ptr = buffer
                .as_mut_ptr()
                .cast::<u8>()
                .add(record_bytes)
                .cast::<u16>();
            std::ptr::copy_nonoverlapping(name.as_ptr(), name_ptr, name.len());
            (*record).pServerName = std::ptr::null_mut();
            (*record).pPrinterName = name_ptr;
        }
        assert_eq!(
            windows_printer_names(&buffer, 1, record_bytes, std::mem::size_of::<usize>())
                .unwrap(),
            ["Net \u{263a}"]
        );
        // Pointing the reader at the wrong offset sees the null pServerName
        // instead and yields no names.
        assert!(
            windows_printer_names(&buffer, 1, record_bytes, 0)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn shell_failures_are_described() {
        use windows_sys::Win32::Foundation::ERROR_NO_ASSOCIATION;
        use windows_sys::Win32::UI::Shell::{SE_ERR_DDETIMEOUT, SE_ERR_FNF, SE_ERR_NOASSOC};
        assert!(describe_shell_execute_failure(SE_ERR_FNF).contains("PDF"));
        assert!(describe_shell_execute_failure(SE_ERR_DDETIMEOUT).contains("respond"));
        assert!(describe_shell_execute_failure(SE_ERR_NOASSOC).contains("Print support"));
        assert!(describe_shell_execute_failure(ERROR_NO_ASSOCIATION).contains("Print support"));
        assert!(describe_shell_execute_failure(0).contains("memory"));
        assert!(!describe_shell_execute_failure(9_999).is_empty());
    }

    /// Opening a printer by name spools nothing — it is a pure spooler
    /// lookup — so a bogus name must surface a clear error naming it.
    #[test]
    fn a_missing_printer_surfaces_a_clear_error() {
        let error = ensure_printer_exists("ocs-no-such-printer-3f9c").unwrap_err();
        assert!(error.contains("ocs-no-such-printer-3f9c"), "{error}");
    }

    /// The machine's real default cannot be asserted in a unit test; check
    /// only the invariant that a resolved default is a usable name.
    #[test]
    fn a_resolved_default_printer_is_a_trimmed_non_empty_name() {
        if let Some(name) = windows_default_printer() {
            assert!(!name.is_empty());
            assert_eq!(name.trim(), name);
        }
    }
}

/// Build the platform printer-properties command.
#[cfg(not(target_arch = "wasm32"))]
fn printer_properties_command(printer: Option<&str>) -> (&'static str, Vec<String>) {
    let named = printer.map(str::trim).filter(|name| !name.is_empty());

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
/// viewer on desktop, an in-page rasterised overlay on the web. `stem` names
/// the web download (`{stem}_preview.pdf`); the desktop temp file is unchanged.
pub fn preview_page_pdf(
    stem: &str,
    page: &crate::io::pdf_export::PdfPageInput,
) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = stem;
        let tmp = temp_pdf_path("preview");
        crate::io::pdf_export::export_pdf(page, &tmp)?;
        open_in_viewer(&tmp)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let bytes = crate::io::pdf_export::render_pdf_bytes(std::slice::from_ref(page), None)?;
        web_preview_overlay(stem, &bytes)
    }
}

/// Web-only multi-page variant used by print-all: render every layout into
/// one PDF and preview the first pages in the overlay.
#[cfg(target_arch = "wasm32")]
pub fn preview_pages_pdf(
    stem: &str,
    pages: &[crate::io::pdf_export::PdfPageInput],
) -> Result<(), String> {
    let bytes = crate::io::pdf_export::render_pdf_bytes(pages, None)?;
    web_preview_overlay(stem, &bytes)
}

/// Web preview: rasterise the plotted PDF (the same hayro pipeline that draws
/// PDF underlays) and show the pages in an in-page overlay. The browser gets
/// plain PNGs, so neither popup permission nor a PDF viewer plugin is needed.
/// Rasterisation is bounded by a total pixel budget so large sheets lower the
/// preview DPI instead of the tab's memory ceiling.
#[cfg(target_arch = "wasm32")]
fn web_preview_overlay(stem: &str, bytes: &[u8]) -> Result<(), String> {
    use crate::scene::model::pdf_raster;
    const PREVIEW_DPI: f32 = 150.0;
    const MAX_PREVIEW_PAGES: usize = 12;
    let (pages, total_pages) = pdf_raster::rasterize_preview_pages(
        std::sync::Arc::new(bytes.to_vec()),
        PREVIEW_DPI,
        MAX_PREVIEW_PAGES,
    );
    let mut pngs = Vec::with_capacity(pages.len());
    for page in &pages {
        let encoded =
            image::RgbaImage::from_raw(page.width, page.height, page.pixels.as_ref().clone())
                .and_then(|img| {
                    let mut out = Vec::new();
                    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
                        .ok()?;
                    Some(out)
                });
        if let Some(png) = encoded {
            pngs.push(png);
        }
    }
    crate::sys::show_pdf_preview(&format!("{stem}_preview.pdf"), bytes, &pngs, total_pages);
    Ok(())
}

/// Dispatch a PDF to a specific printer with [`PrintOptions`].
#[cfg(not(target_arch = "wasm32"))]
fn dispatch_to_printer_opts(path: &std::path::Path, opts: &PrintOptions) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Foundation::{GetLastError, ERROR_NO_ASSOCIATION};
        use windows_sys::Win32::UI::Shell::{
            ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SE_ERR_NOASSOC,
            SHELLEXECUTEINFOW,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

        let wide = |s: &str| -> Vec<u16> { OsStr::new(s).encode_wide().chain(Some(0)).collect() };
        let path_wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // Resolve what the job actually targets (#1261b). The dialog's
        // "System default printer" arrives as `printer: None`; the old code
        // then used the shell `print` verb, which defers to the registered
        // PDF application — whose default printer is frequently not the
        // system default, or which just opens the viewer. Resolve the
        // spooler's own default and address it explicitly through `printto`,
        // exactly like a named printer; only fall back to the bare `print`
        // verb when Windows itself reports no default printer. This is
        // resolved once per dispatch so the "Sent to printer: X" message
        // names the printer that really received the job.
        let target = opts
            .printer
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .or_else(windows_default_printer);
        // A printer that disappeared since the dialog enumerated it (unplugged
        // USB device, deleted network share) would be silently handed to the
        // PDF application: ShellExecute would "succeed" while nothing prints
        // (#1261c). Probe the spooler first so it fails loudly here instead.
        if let Some(name) = &target {
            ensure_printer_exists(name)?;
        }
        let (verb, params, label) = match &target {
            Some(name) => (wide("printto"), Some(wide(name)), name.clone()),
            None => (wide("print"), None, "default printer".to_string()),
        };
        let params_ptr = params
            .as_ref()
            .map(|v| v.as_ptr())
            .unwrap_or(std::ptr::null());
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
                // Note: a non-zero return from ShellExecuteExW only means the
                // PDF handler accepted the job; whether it actually spools is
                // unobservable here without real hardware.
                return Err(format!(
                    "Windows print dispatch to '{label}' failed (code {code}): {}",
                    describe_shell_execute_failure(code),
                ));
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
