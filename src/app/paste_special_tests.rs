//! PASTESPEC: the formats it offers for what the clipboard holds, and what
//! each one places.
use super::{Message, OpenCADStudio};
use document::entities::Table;
use document::types::Vector3;
use document::EntityType;

/// Three rows of two cells, as a spreadsheet puts a copied range on the
/// clipboard as text: cells apart by tabs, rows by line ends.
const RANGE: &str = "Item\tQty\r\nBolt M8\t4\r\nNut M8\t4\r\n";

/// PASTESPEC in a new drawing, once the system clipboard's text is read.
fn started(text: &str) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let _ = app.dispatch_command("PASTESPEC");
    let _ = app.update(Message::PasteSpecial(Some(text.to_string())));
    app
}

/// The keywords of the formats the running command offers.
fn keywords(app: &OpenCADStudio) -> Vec<String> {
    app.tabs[app.active_tab]
        .active_cmd
        .as_ref()
        .map(|command| {
            command
                .options()
                .into_iter()
                .map(|option| option.keyword)
                .collect()
        })
        .unwrap_or_default()
}

/// Type `words` at the running command, one answer each, then Enter.
fn answer(app: &mut OpenCADStudio, words: &[&str]) {
    let mut tokens = vec!["PASTESPEC".to_string()];
    tokens.extend(words.iter().map(|word| word.to_string()));
    let _ = app.finish_active_command(&tokens, true);
}

fn tables(app: &OpenCADStudio) -> Vec<Table> {
    app.tabs[app.active_tab]
        .scene
        .document
        .entities()
        .filter_map(|entity| match entity {
            EntityType::Table(table) => Some((**table).clone()),
            _ => None,
        })
        .collect()
}

fn texts(app: &OpenCADStudio) -> Vec<String> {
    app.tabs[app.active_tab]
        .scene
        .document
        .entities()
        .filter_map(|entity| match entity {
            EntityType::MText(text) => Some(text.value.clone()),
            _ => None,
        })
        .collect()
}

fn cells(table: &Table) -> Vec<Vec<String>> {
    (0..table.row_count())
        .map(|row| {
            (0..table.column_count())
                .map(|column| {
                    table
                        .cell(row, column)
                        .map(|cell| cell.text_value().to_string())
                        .unwrap_or_default()
                })
                .collect()
        })
        .collect()
}

#[test]
fn a_copied_range_is_offered_as_a_table() {
    let app = started(RANGE);
    assert_eq!(
        keywords(&app),
        ["T", "TA"],
        "text in rows of cells is offered as Text and as a Table"
    );
}

#[test]
fn text_without_cells_is_not_offered_as_a_table() {
    let app = started("First line\r\nSecond line\r\n");
    assert_eq!(keywords(&app), ["T"]);
}

#[test]
fn table_places_the_range_as_a_table_at_the_point() {
    let mut app = started(RANGE);
    answer(&mut app, &["TA", "10,20"]);
    let i = app.active_tab;
    assert!(
        app.tabs[i].active_cmd.is_none(),
        "the command ends with the table placed"
    );
    let placed = tables(&app);
    assert_eq!(placed.len(), 1, "one table for the range");
    assert_eq!(
        cells(&placed[0]),
        [["Item", "Qty"], ["Bolt M8", "4"], ["Nut M8", "4"]]
    );
    assert_eq!(placed[0].insertion_point, Vector3::new(10.0, 20.0, 0.0));
    assert!(texts(&app).is_empty(), "and no text beside it");
    // One undo step takes the table off again.
    let _ = app.automation_op(r#"{"op":"undo"}"#);
    assert!(tables(&app).is_empty());
}

#[test]
fn the_rows_of_a_pasted_table_are_data_rows_in_the_current_table_style() {
    let mut app = started(RANGE);
    answer(&mut app, &["TA", "0,0"]);
    let placed = tables(&app);
    let table = placed.first().expect("the pasted table");
    let document = &app.tabs[app.active_tab].scene.document;
    let current = document
        .objects
        .iter()
        .find_map(|(handle, object)| match object {
            document::objects::ObjectType::TableStyle(style)
                if style
                    .name
                    .eq_ignore_ascii_case(&document.header.current_table_style_name) =>
            {
                Some(*handle)
            }
            _ => None,
        });
    assert!(current.is_some(), "the drawing has a current table style");
    assert_eq!(table.table_style_handle, current);
    // A copied range has no title or header of its own: every row is data.
    for row in 0..table.row_count() {
        assert_eq!(
            crate::entities::table::cell_role_kind(table, false, false, row, 0),
            2,
            "row {row} is a data row"
        );
    }
}

#[test]
fn a_cell_with_a_line_break_stays_one_cell() {
    // A cell that holds a line break, a tab or a quote is copied in quotes,
    // its own quotes doubled.
    let mut app = started("Part\tNote\r\nPipe\t\"2\"\" bore\nsee \tdetail\"\r\nCap\tspare\r\n");
    answer(&mut app, &["TA", "0,0"]);
    let placed = tables(&app);
    assert_eq!(placed.len(), 1);
    assert_eq!(
        cells(&placed[0]),
        [
            ["Part", "Note"],
            ["Pipe", "2\" bore\\Psee \tdetail"],
            ["Cap", "spare"],
        ]
    );
}

#[test]
fn rows_of_different_lengths_fill_one_grid() {
    let mut app = started("A\tB\tC\r\n\r\nD\r\n\tE\r\n");
    answer(&mut app, &["TA", "0,0"]);
    let placed = tables(&app);
    assert_eq!(placed.len(), 1);
    assert_eq!(
        cells(&placed[0]),
        [["A", "B", "C"], ["", "", ""], ["D", "", ""], ["", "E", ""]]
    );
}

#[test]
fn text_still_pastes_a_range_as_text() {
    let mut app = started(RANGE);
    answer(&mut app, &["T", "10,20"]);
    assert_eq!(texts(&app), ["Item\tQty\\PBolt M8\t4\\PNut M8\t4"]);
    assert!(tables(&app).is_empty());
}

#[test]
fn the_prompt_lists_the_formats_on_hand() {
    // Nothing was copied in the program, so Objects and Block are not there
    // to ask for.
    let app = started(RANGE);
    let command = app.tabs[app.active_tab]
        .active_cmd
        .as_ref()
        .expect("PASTESPEC is asking for the format");
    let labels: Vec<String> = command
        .options()
        .into_iter()
        .map(|option| option.label)
        .collect();
    assert_eq!(labels.len(), 2);
    assert!(
        command
            .prompt()
            .contains(&format!("[{}]", labels.join("/"))),
        "the prompt {:?} lists {labels:?} and nothing else",
        command.prompt()
    );
}
