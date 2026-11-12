// PASTESPEC — paste the clipboard in a chosen format: the copied objects, the
// copied objects as a block, or the system clipboard's text as multi-line
// text or, when it is in rows of cells, as a table.

use crate::t;
use document::entities::mtext::AttachmentPoint;
use document::entities::{MText, Table};
use document::types::Vector3;
use document::EntityType;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, WorkingPlane};
use crate::scene::model::wire_model::WireModel;

/// What the command is asking for.
#[derive(Clone, Copy, PartialEq)]
enum Step {
    /// The format to paste in.
    Format,
    /// Where the text goes.
    Text,
    /// Where the table goes.
    Table,
}

pub struct PasteSpecCommand {
    /// Objects copied inside the application are waiting on the clipboard.
    objects: bool,
    /// The system clipboard's text, already escaped for MTEXT.
    text: Option<String>,
    /// The table of that text's cells, when it is in rows of cells.
    table: Option<Table>,
    style: String,
    height: f64,
    step: Step,
    plane: WorkingPlane,
}

impl PasteSpecCommand {
    pub fn new(
        objects: bool,
        text: Option<&str>,
        table: Option<Table>,
        style: String,
        height: f64,
    ) -> Self {
        Self {
            objects,
            text: text.map(mtext_value),
            table,
            style,
            height,
            step: Step::Format,
            plane: WorkingPlane::default(),
        }
    }
}

/// Plain text as an MTEXT value: control characters escaped, lines as
/// paragraphs.
fn mtext_value(text: &str) -> String {
    text.trim_end_matches(['\r', '\n'])
        .replace('\\', "\\\\")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace("\r\n", "\\P")
        .replace(['\r', '\n'], "\\P")
}

/// The cells of text copied as rows of cells — cells apart by tabs, rows by
/// line ends, as a spreadsheet or a word processor's table puts a copied
/// range on the clipboard — each as a table cell's text. `None` for text
/// with no second cell in any row: that is lines of text, not a grid.
///
/// A cell that holds a line end or a tab is copied in quotes, with its own
/// quotes doubled; it stays one cell. Rows shorter than the widest are
/// filled with empty cells, and empty rows at the end are left out.
pub fn grid_cells(text: &str) -> Option<Vec<Vec<String>>> {
    let text: Vec<char> = text.chars().collect();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut at = 0;
    loop {
        let (cell, after) = match quoted_cell(&text, at) {
            Some(quoted) => quoted,
            None => {
                let end = text[at..]
                    .iter()
                    .position(|c| matches!(c, '\t' | '\r' | '\n'))
                    .map_or(text.len(), |offset| at + offset);
                (text[at..end].iter().collect(), end)
            }
        };
        row.push(mtext_value(&cell));
        match text.get(after) {
            Some('\t') => at = after + 1,
            ended => {
                rows.push(std::mem::take(&mut row));
                if ended.is_none() {
                    break;
                }
                // A line end is one of "\r\n", "\n" or "\r".
                let pair = text[after] == '\r' && text.get(after + 1) == Some(&'\n');
                at = after + if pair { 2 } else { 1 };
            }
        }
    }
    while rows
        .last()
        .is_some_and(|row| row.iter().all(String::is_empty))
    {
        rows.pop();
    }
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns < 2 {
        return None;
    }
    for row in &mut rows {
        row.resize(columns, String::new());
    }
    Some(rows)
}

/// The cell in quotes that starts at `at`, without its quotes, and the place
/// of the tab or line end after it. `None` when the text there is not a cell
/// in quotes: no quote, no closing quote before a tab, a line end or the end
/// of the text, or no line end or tab inside — only those put a cell in
/// quotes, so quotes around anything else are the cell's own text.
fn quoted_cell(text: &[char], at: usize) -> Option<(String, usize)> {
    if text.get(at) != Some(&'"') {
        return None;
    }
    let mut cell = String::new();
    let mut i = at + 1;
    loop {
        match (*text.get(i)?, text.get(i + 1).copied()) {
            ('"', Some('"')) => {
                cell.push('"');
                i += 2;
            }
            ('"', None | Some('\t' | '\r' | '\n')) => {
                return cell.contains(['\t', '\r', '\n']).then_some((cell, i + 1));
            }
            ('"', Some(_)) => return None,
            (other, _) => {
                cell.push(other);
                i += 1;
            }
        }
    }
}

impl CadCommand for PasteSpecCommand {
    fn name(&self) -> &'static str {
        "PASTESPEC"
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn prompt(&self) -> String {
        if self.step != Step::Format {
            return t!("PASTESPEC  Specify insertion point:").into_owned();
        }
        // Only the formats on hand; none until the clipboard has been read.
        let formats: Vec<String> = self
            .options()
            .into_iter()
            .map(|option| option.label)
            .collect();
        if formats.is_empty() {
            return self.name().to_string();
        }
        t!(
            "PASTESPEC  Paste as [%{formats}]:",
            formats = formats.join("/")
        )
        .into_owned()
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.step != Step::Format {
            return Vec::new();
        }
        let mut options = Vec::new();
        if self.objects {
            options.push(CmdOption::new("Objects", "O"));
            options.push(CmdOption::new("Block", "B"));
        }
        if self.text.is_some() {
            options.push(CmdOption::new("Text", "T"));
        }
        if self.table.is_some() {
            options.push(CmdOption::new("TAble", "TA"));
        }
        options
    }

    fn wants_text_input(&self) -> bool {
        self.step == Step::Format
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.step != Step::Format {
            return None;
        }
        match text
            .trim()
            .trim_start_matches('_')
            .to_ascii_uppercase()
            .as_str()
        {
            "O" | "OBJECTS" if self.objects => Some(CmdResult::Dispatch("PASTECLIP".into())),
            "B" | "BLOCK" if self.objects => Some(CmdResult::Dispatch("PASTEBLOCK".into())),
            "T" | "TEXT" if self.text.is_some() => {
                self.step = Step::Text;
                Some(CmdResult::NeedPoint)
            }
            "TA" | "TABLE" if self.table.is_some() => {
                self.step = Step::Table;
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let p = self.plane.to_local(pt);
        let at = Vector3::new(p.x, p.y, p.z);
        let entity = match self.step {
            Step::Format => None,
            Step::Text => self.text.clone().map(|value| {
                let mut mtext = MText::new();
                mtext.value = value;
                mtext.insertion_point = at;
                mtext.height = self.height;
                mtext.style = self.style.clone();
                mtext.attachment_point = AttachmentPoint::TopLeft;
                mtext.rectangle_width = 0.0;
                EntityType::MText(Box::new(mtext))
            }),
            Step::Table => self.table.clone().map(|mut table| {
                table.insertion_point = at;
                EntityType::Table(Box::new(table))
            }),
        };
        match entity {
            Some(entity) => CmdResult::CommitAndExit(self.plane.place_entity(entity)),
            None => CmdResult::NeedPoint,
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if self.step != Step::Table {
            return None;
        }
        let table = self.table.as_ref()?;
        Some(crate::modules::annotate::data_link::table_grid_preview(
            table,
            &self.plane,
            pt,
        ))
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.step != Step::Format {
            return CmdResult::NeedPoint;
        }
        // Enter takes the first format offered.
        let first = self.options().first().map(|o| o.keyword.clone());
        first
            .and_then(|keyword| self.on_text_input(&keyword))
            .unwrap_or(CmdResult::Cancel)
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["PASTESPEC"]
});

#[cfg(test)]
mod tests {
    use super::{grid_cells, mtext_value};

    #[test]
    fn plain_text_becomes_escaped_paragraphs() {
        assert_eq!(mtext_value("a{b}\\c\r\nline 2\n"), "a\\{b\\}\\\\c\\Pline 2");
    }

    fn grid(text: &str) -> Vec<Vec<String>> {
        grid_cells(text).expect("rows of cells")
    }

    #[test]
    fn rows_of_cells_are_read_whatever_the_line_end() {
        let rows = [["a", "b"], ["c", "d"]];
        for text in ["a\tb\r\nc\td\r\n", "a\tb\nc\td", "a\tb\rc\td\r"] {
            assert_eq!(grid(text), rows, "{text:?}");
        }
    }

    #[test]
    fn lines_of_text_are_not_a_grid() {
        for text in ["", "one line", "one\r\ntwo\r\n", "\r\n\r\n"] {
            assert_eq!(grid_cells(text), None, "{text:?}");
        }
    }

    #[test]
    fn a_cell_in_quotes_is_one_cell_with_its_line_break() {
        // As a spreadsheet copies a cell of two lines that also has quotes
        // of its own: in quotes, its quotes doubled.
        assert_eq!(
            grid("\"say \"\"hi\"\"\nsecond line\"\tend\r\n"),
            [["say \"hi\"\\Psecond line", "end"]]
        );
    }

    #[test]
    fn quotes_that_are_a_cells_own_text_are_kept() {
        // A cell is put in quotes only for a line end or a tab inside it, so
        // 5" stays 5", "quoted" keeps its quotes, and a cell that opens with
        // a quote it does not close before the next cell keeps it.
        assert_eq!(
            grid("5\" pipe\t\"quoted\"\r\n\"a\" b\t\"\""),
            [["5\" pipe", "\"quoted\""], ["\"a\" b", "\"\""]]
        );
        assert_eq!(grid("\"open\tb"), [["\"open", "b"]]);
    }

    #[test]
    fn cell_text_is_escaped_like_pasted_text() {
        assert_eq!(grid("C:\\parts\t{2}"), [["C:\\\\parts", "\\{2\\}"]]);
    }
}
