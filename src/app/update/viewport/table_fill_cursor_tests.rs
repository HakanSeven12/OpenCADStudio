use super::*;

use super::*;

#[test]
fn only_a_table_fill_diamond_follows_the_bare_cursor() {
    let table = document::Handle::new(0x20);
    let line = document::Handle::new(0x21);
    let originals = vec![
        (table, document::EntityType::Table(Box::default())),
        (
            line,
            document::EntityType::Line(document::entities::Line::default()),
        ),
    ];
    let fill = crate::entities::table::CELL_GRIP_FILL;
    assert!(table_fill_follows_cursor(&originals, table, fill));
    assert!(!table_fill_follows_cursor(&originals, table, fill - 1));
    assert!(!table_fill_follows_cursor(&originals, line, fill));
}
