//! PASTESPEC: the formats it offers for what the clipboard holds, and what
//! each one places.
use super::{Message, OpenCADStudio};
use crate::io::ole_embed::EmbeddedImage;
use crate::sys::clipboard_link::SourceLink;
use document::entities::{Line, Ole2Frame, OlePresentation, RasterImage, Table};
use document::types::Vector3;
use document::EntityType;

/// Three rows of two cells, as a spreadsheet puts a copied range on the
/// clipboard as text: cells apart by tabs, rows by line ends.
const RANGE: &str = "Item\tQty\r\nBolt M8\t4\r\nNut M8\t4\r\n";

/// PASTESPEC in a new drawing, once the system clipboard's text is read.
fn started(text: &str) -> OpenCADStudio {
    started_with(false, Some(text), None)
}

/// PASTESPEC in a new drawing — with a line copied in the program when
/// `objects` — once the system clipboard is read: its text and its picture.
fn started_with(
    objects: bool,
    text: Option<&str>,
    picture: Option<EmbeddedImage>,
) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    if objects {
        let i = app.active_tab;
        let line = app.tabs[i]
            .scene
            .add_entity(EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::UNIT_X,
            )));
        app.tabs[i].scene.select_entity(line, true);
        let _ = app.dispatch_command("COPYCLIP");
        app.tabs[i].scene.deselect_all();
    }
    let _ = app.dispatch_command("PASTESPEC");
    let _ = app.update(Message::PasteSpecial(
        text.map(str::to_string),
        picture,
        None,
    ));
    app
}

/// A picture as it is read off the clipboard: 40 × 30 pixels, as a PNG.
fn picture() -> EmbeddedImage {
    let pixels = image::RgbaImage::from_pixel(40, 30, image::Rgba([200, 30, 30, 255]));
    let mut png = std::io::Cursor::new(Vec::new());
    pixels
        .write_to(&mut png, image::ImageFormat::Png)
        .expect("a PNG of the pixels");
    EmbeddedImage::from_bytes("pasted-1.png".to_string(), png.into_inner())
        .expect("a picture to place")
}

/// The pictures held inside the drawing.
fn embedded(app: &OpenCADStudio) -> Vec<Ole2Frame> {
    app.tabs[app.active_tab]
        .scene
        .document
        .model_space_entities()
        .filter_map(|entity| match entity {
            EntityType::Ole2Frame(frame) => Some((**frame).clone()),
            _ => None,
        })
        .collect()
}

/// The pictures the drawing points at in files.
fn linked(app: &OpenCADStudio) -> Vec<RasterImage> {
    app.tabs[app.active_tab]
        .scene
        .document
        .model_space_entities()
        .filter_map(|entity| match entity {
            EntityType::RasterImage(image) => Some(image.clone()),
            _ => None,
        })
        .collect()
}

/// A folder of its own for a drawing that is saved there.
fn drawing_folder(name: &str) -> std::path::PathBuf {
    let folder = std::env::temp_dir().join(format!("ocs-pastespec-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("a folder for the drawing");
    folder
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

/// The tables placed in the drawing.
fn tables(app: &OpenCADStudio) -> Vec<Table> {
    app.tabs[app.active_tab]
        .scene
        .document
        .model_space_entities()
        .filter_map(|entity| match entity {
            EntityType::Table(table) => Some((**table).clone()),
            _ => None,
        })
        .collect()
}

/// The texts placed in the drawing. Model space only: a table keeps the
/// text of its cells in a block of its own.
fn texts(app: &OpenCADStudio) -> Vec<String> {
    app.tabs[app.active_tab]
        .scene
        .document
        .model_space_entities()
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
fn the_rows_of_a_pasted_table_are_as_high_as_their_text() {
    // One line in the first row; below it a cell of two lines, and a cell
    // whose one line is longer than its column and wraps.
    let mut app = started(concat!(
        "Mark\tNote\r\n",
        "S1\t\"Slab t = 150\nmesh on both faces\"\r\n",
        "C1\tA description that is much longer than the width of a column of the table\r\n",
    ));
    answer(&mut app, &["TA", "0,0"]);
    let placed = tables(&app);
    let rows = &placed.first().expect("the pasted table").rows;
    let one_line = rows[0].height;
    assert!(
        rows[1].height > one_line * 1.5,
        "two lines need more than the {one_line} of one: {}",
        rows[1].height
    );
    assert!(
        rows[2].height > one_line * 1.5,
        "a line that wraps needs more than the {one_line} of one: {}",
        rows[2].height
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
fn the_prompt_leaves_out_the_formats_that_are_not_there() {
    let app = started("First line\r\nSecond line\r\n");
    let command = app.tabs[app.active_tab]
        .active_cmd
        .as_ref()
        .expect("PASTESPEC is asking for the format");
    let labels: Vec<String> = command
        .options()
        .into_iter()
        .map(|option| option.label)
        .collect();
    assert_eq!(labels.len(), 1, "lines of text are text only");
    assert!(
        command.prompt().contains(&format!("[{}]", labels[0])),
        "the prompt {:?} lists {labels:?} and nothing else",
        command.prompt()
    );
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

#[test]
fn a_copied_picture_is_offered_as_an_image_and_as_an_embedded_image() {
    let app = started_with(false, None, Some(picture()));
    assert_eq!(keywords(&app), ["I", "E"]);
}

#[test]
fn a_picture_joins_the_formats_of_what_else_was_copied() {
    // A spreadsheet copies a range as text and as a picture of it.
    let app = started_with(true, Some(RANGE), Some(picture()));
    assert_eq!(keywords(&app), ["O", "B", "T", "TA", "I", "E"]);
}

#[test]
fn embedded_image_places_the_picture_inside_the_drawing() {
    let source = picture();
    let mut app = started_with(false, None, Some(source.clone()));
    // The format, the insertion point, and Enter for the default width: a
    // drawing unit for each 100 pixels, one at the least.
    answer(&mut app, &["E", "5,5"]);
    let i = app.active_tab;
    assert!(
        app.tabs[i].active_cmd.is_none(),
        "the command ends with the picture placed"
    );
    let placed = embedded(&app);
    assert_eq!(placed.len(), 1, "one picture inside the drawing");
    match document::entities::extract_presentation(&placed[0].encoded_payload()) {
        Some(OlePresentation::Raster(bytes)) => assert_eq!(bytes, source.bytes),
        other => panic!("the drawing holds the picture's own bytes, not {other:?}"),
    }
    // 40 × 30 pixels at one unit wide, its lower left corner at the point.
    assert_eq!(placed[0].upper_left_corner, Vector3::new(5.0, 5.75, 0.0));
    assert_eq!(placed[0].lower_right_corner, Vector3::new(6.0, 5.0, 0.0));
    assert!(linked(&app).is_empty(), "and no file to point at");
    // One undo step takes the picture off again.
    let _ = app.automation_op(r#"{"op":"undo"}"#);
    assert!(embedded(&app).is_empty());
}

#[test]
fn image_saves_the_picture_beside_the_drawing_and_points_at_it() {
    let folder = drawing_folder("image");
    let source = picture();
    let mut app = started_with(false, None, Some(source.clone()));
    let i = app.active_tab;
    app.tabs[i].current_path = Some(folder.join("drawing.dwg"));
    answer(&mut app, &["I", "5,5"]);
    assert!(
        app.tabs[i].active_cmd.is_none(),
        "the command ends with the picture placed"
    );
    let placed = linked(&app);
    assert_eq!(placed.len(), 1, "one picture the drawing points at");
    let file = std::path::PathBuf::from(&placed[0].file_path);
    assert_eq!(
        file.parent(),
        Some(folder.as_path()),
        "the picture is saved beside the drawing"
    );
    assert_eq!(
        std::fs::read(&file).expect("the saved picture"),
        source.bytes
    );
    assert_eq!(placed[0].insertion_point, Vector3::new(5.0, 5.0, 0.0));
    assert!(embedded(&app).is_empty(), "and nothing inside the drawing");
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn enter_takes_the_first_format_of_a_picture() {
    let folder = drawing_folder("enter");
    let mut app = started_with(false, None, Some(picture()));
    let i = app.active_tab;
    app.tabs[i].current_path = Some(folder.join("drawing.dwg"));
    // Enter at the format prompt, then the insertion point and the width.
    let _ = app.feed_command(crate::command::StepInput::Enter);
    answer(&mut app, &["5,5"]);
    assert_eq!(linked(&app).len(), 1, "Image is the first format offered");
    assert!(embedded(&app).is_empty());
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn with_nothing_to_paste_the_command_says_so_and_ends() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let errors = app.command_line.error_revision;
    let _ = app.dispatch_command("PASTESPEC");
    let _ = app.update(Message::PasteSpecial(None, None, None));
    let i = app.active_tab;
    assert!(app.tabs[i].active_cmd.is_none(), "no format, no command");
    assert!(
        app.command_line.error_revision > errors,
        "the command line says the clipboard holds nothing to paste"
    );
}

/// The prompts PASTESPEC has left in the command history.
fn prompts_shown(app: &OpenCADStudio) -> Vec<String> {
    app.command_line
        .history
        .iter()
        .filter(|entry| {
            matches!(entry.kind, crate::ui::command_line::EntryKind::Info)
                && entry.text.contains("PASTESPEC")
        })
        .map(|entry| entry.text.clone())
        .collect()
}

#[test]
fn the_prompt_is_shown_once_when_the_clipboard_has_been_read() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    // As the command line runs it: after every message the prompt of the
    // running step is kept in the command history.
    let _ = app.update(Message::Command("PASTESPEC".to_string()));
    assert_eq!(
        prompts_shown(&app),
        Vec::<String>::new(),
        "there is nothing to ask while the clipboard is being read"
    );
    let _ = app.update(Message::PasteSpecial(Some(RANGE.to_string()), None, None));
    let shown = prompts_shown(&app);
    assert_eq!(shown.len(), 1, "one prompt, with every format: {shown:?}");
}

#[test]
fn copied_objects_can_be_asked_for_while_the_clipboard_is_read() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    let line = app.tabs[i]
        .scene
        .add_entity(EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::UNIT_X,
        )));
    app.tabs[i].scene.select_entity(line, true);
    let _ = app.dispatch_command("COPYCLIP");
    let _ = app.update(Message::Command("PASTESPEC".to_string()));
    // A line such as `PASTESPEC O` is answered before the read is back.
    assert_eq!(keywords(&app), ["O", "B"]);
    assert_eq!(
        prompts_shown(&app),
        Vec::<String>::new(),
        "the prompt waits for the formats of the system clipboard"
    );
    let _ = app.update(Message::PasteSpecial(Some(RANGE.to_string()), None, None));
    assert_eq!(keywords(&app), ["O", "B", "T", "TA"]);
    let shown = prompts_shown(&app);
    assert_eq!(shown.len(), 1, "one prompt, with every format: {shown:?}");
}

/// Rows of cells in a file, as a spreadsheet keeps them.
const BARS: &str = "Mark,Qty,Length\nB1,4,5.20\nC1,6,3.00\nS1,1,12.50\n";

/// A file that holds `contents`, in a folder of its own, and the link a
/// spreadsheet leaves on the clipboard when `cells` of it are copied.
fn copied_from_a_file(name: &str, contents: &str, cells: &str) -> (std::path::PathBuf, SourceLink) {
    let file = drawing_folder(name).join("bars.csv");
    std::fs::write(&file, contents).expect("the file the cells are copied from");
    let link = SourceLink {
        file: file.to_string_lossy().into_owned(),
        sheet: "bars".to_string(),
        cells: cells.to_string(),
    };
    (file, link)
}

/// PASTESPEC in a new drawing, once the system clipboard is read: the text
/// of a copied range and the link to where it was copied from.
fn started_with_a_link(text: &str, link: SourceLink) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let _ = app.dispatch_command("PASTESPEC");
    let _ = app.update(Message::PasteSpecial(
        Some(text.to_string()),
        None,
        Some(link),
    ));
    app
}

fn remove_folder_of(file: &std::path::Path) {
    if let Some(folder) = file.parent() {
        let _ = std::fs::remove_dir_all(folder);
    }
}

#[test]
fn a_range_copied_from_a_saved_file_is_offered_as_a_link() {
    let (file, link) = copied_from_a_file("link-offered", BARS, "A1:B3");
    let app = started_with_a_link("Mark\tQty\r\nB1\t4\r\nC1\t6\r\n", link);
    assert_eq!(keywords(&app), ["T", "TA", "L"]);
    remove_folder_of(&file);
}

#[test]
fn link_places_a_table_of_the_cells_linked_to_their_file() {
    let (file, link) = copied_from_a_file("link-placed", BARS, "A1:B3");
    // The cells come from the file, not from the text beside the link.
    let mut app = started_with_a_link("not\tthe file\r\n", link);
    // The format, then the insertion point of the table.
    answer(&mut app, &["L", "5,5"]);
    let i = app.active_tab;
    assert!(
        app.tabs[i].active_cmd.is_none(),
        "the command ends with the table placed"
    );
    let placed = tables(&app);
    assert_eq!(placed.len(), 1, "one table for the linked cells");
    assert_eq!(
        cells(&placed[0]),
        [["Mark", "Qty"], ["B1", "4"], ["C1", "6"]]
    );
    assert_eq!(placed[0].insertion_point, Vector3::new(5.0, 5.0, 0.0));
    // The drawing holds the data link the table reads: named after the
    // file, to those cells of it.
    let document = &app.tabs[i].scene.document;
    let links = crate::app::annotation_data::data_link_entries(document);
    assert_eq!(links.len(), 1, "one data link: {links:?}");
    assert_eq!(links[0].0, "bars");
    assert_eq!(
        placed[0].cell(0, 0).and_then(|cell| cell.data_link_handle),
        Some(links[0].1),
        "the table's cells are tied to that link"
    );
    assert_eq!(
        crate::app::annotation_data::read_data_link(document, links[0].1),
        Ok(vec![
            vec!["Mark".to_string(), "Qty".to_string()],
            vec!["B1".to_string(), "4".to_string()],
            vec!["C1".to_string(), "6".to_string()],
        ]),
        "the link reads the copied cells, not the whole file"
    );
    remove_folder_of(&file);
}

#[test]
fn a_linked_table_follows_its_file() {
    let (file, link) = copied_from_a_file("link-updated", BARS, "A1:B3");
    let mut app = started_with_a_link("Mark\tQty\r\n", link);
    answer(&mut app, &["L", "0,0"]);
    assert_eq!(tables(&app).len(), 1, "the linked table");
    // The file changes; the tables of the drawing are updated from it.
    std::fs::write(&file, "Mark,Qty,Length\nB1,8,5.20\nC2,6,3.00\n").expect("the changed file");
    let _ = app.dispatch_command("DATALINKUPDATE _RUN U ALL");
    let placed = tables(&app);
    assert_eq!(placed.len(), 1);
    assert_eq!(
        cells(&placed[0]),
        [["Mark", "Qty"], ["B1", "8"], ["C2", "6"]]
    );
    remove_folder_of(&file);
}

#[test]
fn a_link_to_a_file_that_is_gone_says_so_and_places_nothing() {
    let (file, link) = copied_from_a_file("link-gone", BARS, "A1:B3");
    remove_folder_of(&file);
    let mut app = started_with_a_link("Mark\tQty\r\n", link);
    assert_eq!(keywords(&app), ["T", "TA", "L"]);
    let errors = app.command_line.error_revision;
    answer(&mut app, &["L", "0,0"]);
    let i = app.active_tab;
    assert!(
        app.command_line.error_revision > errors,
        "the command line says the file could not be read"
    );
    assert!(tables(&app).is_empty(), "no table without its cells");
    assert!(
        crate::app::annotation_data::data_link_entries(&app.tabs[i].scene.document).is_empty(),
        "and no data link"
    );
}
