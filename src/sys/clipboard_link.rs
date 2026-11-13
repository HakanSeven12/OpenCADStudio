//! The link a spreadsheet leaves on the clipboard beside a copied range: the
//! file, the sheet and the cells the range was copied from.

/// Where a copied range of cells lives: the source PASTESPEC's Link format
/// keeps a pasted table tied to.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceLink {
    /// The file, by its full path.
    pub file: String,
    /// The sheet of the file.
    pub sheet: String,
    /// The cells of the sheet, as a range such as `B2:E5`.
    pub cells: String,
}

impl SourceLink {
    /// The link in the clipboard's `ObjectLink` format: the class of the
    /// copied object, its document and the item of it, each ended by a NUL.
    /// For a range of cells the document is the file and the item is
    /// `sheet!cells`.
    pub fn from_object_link(text: &str) -> Option<Self> {
        let mut parts = text.split('\0');
        let (_class, document, item) = (parts.next()?, parts.next()?, parts.next()?);
        let (sheet, cells) = item.rsplit_once('!')?;
        Self::of(document, sheet, cells)
    }

    /// The link in the clipboard's `Link` format: the application, the topic
    /// and the item, each ended by a NUL. For a range of cells the topic is
    /// `folder\[file]sheet` and the item is the cells.
    pub fn from_link(text: &str) -> Option<Self> {
        let mut parts = text.split('\0');
        let (_application, topic, cells) = (parts.next()?, parts.next()?, parts.next()?);
        let (file, sheet) = topic.rsplit_once(']')?;
        let (folder, name) = file.rsplit_once('[')?;
        Self::of(&format!("{folder}{name}"), sheet, cells)
    }

    fn of(file: &str, sheet: &str, cells: &str) -> Option<Self> {
        // A workbook never saved has a name and no folder: there is no file
        // to link to yet.
        if !file.contains(['\\', '/']) || sheet.is_empty() {
            return None;
        }
        Some(Self {
            file: file.to_string(),
            sheet: sheet.to_string(),
            cells: cell_range(cells)?,
        })
    }
}

/// A range of cells as `B2:E5` (or one cell as `B2:B2`), from the notation
/// the clipboard gives it in.
fn cell_range(cells: &str) -> Option<String> {
    let mut corners = cells.split(':').map(cell);
    let first = corners.next()??;
    let last = match corners.next() {
        Some(corner) => corner?,
        None => first.clone(),
    };
    corners.next().is_none().then(|| format!("{first}:{last}"))
}

/// A cell as `B2`. The clipboard names it by row and column, a number
/// behind a letter each — `R2C2`, with the letters of the spreadsheet's
/// language, so `F2C2` or `Z2S2` are the same cell — or by its column's
/// letters and its row, `B2` or `$B$2`.
fn cell(text: &str) -> Option<String> {
    let text = text.trim().replace('$', "");
    let (first_letters, rest) = leading(&text, char::is_alphabetic);
    let (first_number, rest) = leading(rest, |c| c.is_ascii_digit());
    if first_letters.is_empty() || first_number.is_empty() {
        return None;
    }
    if rest.is_empty() {
        // A column has up to three letters; a longer word is a name.
        let row: u32 = first_number.parse().ok()?;
        let column =
            first_letters.len() <= 3 && first_letters.chars().all(|c| c.is_ascii_alphabetic());
        return (column && row > 0).then(|| format!("{}{row}", first_letters.to_uppercase()));
    }
    let (second_letters, rest) = leading(rest, char::is_alphabetic);
    let (second_number, rest) = leading(rest, |c| c.is_ascii_digit());
    if second_letters.is_empty() || second_number.is_empty() || !rest.is_empty() {
        return None;
    }
    let (row, column): (u32, u32) = (first_number.parse().ok()?, second_number.parse().ok()?);
    (row > 0 && column > 0).then(|| format!("{}{row}", column_letters(column)))
}

/// `text` split after its leading characters that `wanted` accepts.
fn leading(text: &str, wanted: impl Fn(char) -> bool) -> (&str, &str) {
    text.split_at(text.find(|c| !wanted(c)).unwrap_or(text.len()))
}

/// The letters of a column by its number: 1 is `A`, 27 is `AA`.
fn column_letters(mut column: u32) -> String {
    let mut letters = Vec::new();
    while column > 0 {
        column -= 1;
        letters.push(b'A' + (column % 26) as u8);
        column /= 26;
    }
    letters.reverse();
    String::from_utf8(letters).unwrap_or_default()
}

/// The link the system clipboard holds beside a copied range of cells, when
/// the range comes from a saved file. Only Windows has these formats.
pub fn read() -> Option<SourceLink> {
    #[cfg(target_os = "windows")]
    {
        let clipboard = windows::Clipboard::open()?;
        clipboard
            .text("ObjectLink")
            .as_deref()
            .and_then(SourceLink::from_object_link)
            .or_else(|| {
                clipboard
                    .text("Link")
                    .as_deref()
                    .and_then(SourceLink::from_link)
            })
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use windows_sys::Win32::Globalization::{MultiByteToWideChar, CP_ACP};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    /// The system clipboard, open for reading until dropped.
    pub struct Clipboard;

    impl Clipboard {
        /// Opens the clipboard. Another program may hold it for a moment, so
        /// this tries for a tenth of a second before giving up.
        pub fn open() -> Option<Self> {
            for _ in 0..10 {
                // SAFETY: no window owns the open clipboard; it is closed
                // again when the value is dropped.
                if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                    return Some(Self);
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            None
        }

        /// The data of a registered text format, with its NULs: these
        /// formats hold a list of strings in the system's code page.
        pub fn text(&self, format: &str) -> Option<String> {
            let name: Vec<u16> = format.encode_utf16().chain([0]).collect();
            // SAFETY: the clipboard is open; the handle it returns stays
            // valid until the clipboard is closed, and its bytes are copied
            // while it is locked.
            let bytes = unsafe {
                let handle = GetClipboardData(RegisterClipboardFormatW(name.as_ptr()));
                if handle.is_null() {
                    return None;
                }
                let data = GlobalLock(handle) as *const u8;
                if data.is_null() {
                    return None;
                }
                let bytes = std::slice::from_raw_parts(data, GlobalSize(handle)).to_vec();
                GlobalUnlock(handle);
                bytes
            };
            let length = i32::try_from(bytes.len())
                .ok()
                .filter(|length| *length > 0)?;
            // SAFETY: both buffers are as long as the lengths passed.
            unsafe {
                let size =
                    MultiByteToWideChar(CP_ACP, 0, bytes.as_ptr(), length, std::ptr::null_mut(), 0);
                if size <= 0 {
                    return None;
                }
                let mut wide = vec![0u16; size as usize];
                MultiByteToWideChar(CP_ACP, 0, bytes.as_ptr(), length, wide.as_mut_ptr(), size);
                Some(String::from_utf16_lossy(&wide))
            }
        }
    }

    impl Drop for Clipboard {
        fn drop(&mut self) {
            // SAFETY: this value opened the clipboard.
            unsafe {
                CloseClipboard();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{cell_range, SourceLink};

    fn link(file: &str, sheet: &str, cells: &str) -> Option<SourceLink> {
        Some(SourceLink {
            file: file.to_string(),
            sheet: sheet.to_string(),
            cells: cells.to_string(),
        })
    }

    #[test]
    fn an_object_link_names_the_file_the_sheet_and_the_cells() {
        // As a spreadsheet leaves it for B2:E5 of a sheet of a saved file.
        let copied = "Excel.Sheet.12\0C:\\lists\\año 2026\\bar list.xlsx\0Bar list!F2C2:F5C5\0\0\0";
        assert_eq!(
            SourceLink::from_object_link(copied),
            link("C:\\lists\\año 2026\\bar list.xlsx", "Bar list", "B2:E5")
        );
    }

    #[test]
    fn a_link_names_them_too() {
        let copied = "Excel\0C:\\lists\\año 2026\\[bar list.xlsx]Bar list\0F2C2:F5C5\0\0";
        assert_eq!(
            SourceLink::from_link(copied),
            link("C:\\lists\\año 2026\\bar list.xlsx", "Bar list", "B2:E5")
        );
    }

    #[test]
    fn a_workbook_never_saved_has_no_file_to_link_to() {
        // It has a name and no folder.
        assert_eq!(
            SourceLink::from_object_link("Excel.Sheet.12\0Book1\0Sheet1!R1C1:R4C4\0\0\0"),
            None
        );
        assert_eq!(
            SourceLink::from_link("Excel\0[Book1]Sheet1\0R1C1:R4C4\0\0"),
            None
        );
    }

    #[test]
    fn what_is_not_a_range_of_a_sheet_is_no_link() {
        for copied in ["", "Word.Document.12\0C:\\notes\\memo.docx\0\0\0", "a\0b"] {
            assert_eq!(SourceLink::from_object_link(copied), None, "{copied:?}");
            assert_eq!(SourceLink::from_link(copied), None, "{copied:?}");
        }
    }

    #[test]
    fn cells_are_read_whatever_the_letters_of_rows_and_columns() {
        for cells in ["R2C2:R5C5", "F2C2:F5C5", "Z2S2:Z5S5", "L2C2:L5C5"] {
            assert_eq!(cell_range(cells).as_deref(), Some("B2:E5"), "{cells}");
        }
        assert_eq!(cell_range("B2:E5").as_deref(), Some("B2:E5"));
        assert_eq!(cell_range("$b$2:$e$5").as_deref(), Some("B2:E5"));
        // One cell is a range of itself; columns go on after Z.
        assert_eq!(cell_range("R3C1").as_deref(), Some("A3:A3"));
        assert_eq!(cell_range("R1C27:R2C28").as_deref(), Some("AA1:AB2"));
    }

    #[test]
    fn what_is_not_cells_is_no_range() {
        for cells in [
            "",
            "R0C1",
            "R1C0",
            "Table1",
            "R1C1:R2C2:R3C3",
            "12",
            "R1C",
            "B0",
        ] {
            assert_eq!(cell_range(cells), None, "{cells:?}");
        }
    }
}
