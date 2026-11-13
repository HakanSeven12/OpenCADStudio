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
