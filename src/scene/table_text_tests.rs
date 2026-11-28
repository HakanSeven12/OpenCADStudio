// Tests of how the scene draws a table (`entities::table`).
use document::entities::Table;

/// A new table shows its stored picture, whose cell text sits the table
/// style's margins in from the cell's edges. Drawn from its cells, as it
/// is once edited, the text stays where the picture put it: the style's
/// margins are not raised to half the text height.
#[test]
fn a_table_drawn_from_its_cells_puts_its_text_where_its_picture_does() {
    use document::types::Vector3;
    use document::EntityType;
    let mut scene = crate::scene::Scene::new();
    // Margins 0.06 and data text 0.18 high, top-centred: a margin
    // smaller than half the text height.
    let style = scene.document.allocate_handle();
    scene.document.objects.insert(
        style,
        document::objects::ObjectType::TableStyle(document::objects::TableStyle::standard()),
    );
    let mut table = Table::new(Vector3::ZERO, 1, 2);
    table.table_style_handle = Some(style);
    for column in &mut table.columns {
        column.width = 2.0;
    }
    table.rows[0].height = 0.36;
    table.rows[0].style_id = 3;
    table.set_cell_text(0, 0, "Mark");
    table.set_cell_text(0, 1, "Qty");
    // The first cell top-left, so its left margin shows too.
    let cell = table.cell_mut(0, 0).unwrap();
    let own = cell.style.get_or_insert_with(Default::default);
    own.alignment = 1;
    own.property_flags
        .insert(document::entities::table::CellStylePropertyFlags::ALIGNMENT);
    let handle = scene.add_entity(EntityType::Table(Box::new(table)));
    let Some(EntityType::Table(placed)) = scene.document.get_entity(handle) else {
        panic!("the table is in the drawing");
    };
    assert!(
        placed.block_record_handle.is_some(),
        "a new table has a picture"
    );
    let mut from_cells = (**placed).clone();
    from_cells.block_record_handle = None;
    // The box of the text drawn for `table`: left, bottom, right, top.
    let text_box = |table: &Table| {
        let entity = EntityType::Table(Box::new(table.clone()));
        let wires = crate::scene::tessellate_entity(
            &scene.document,
            &Default::default(),
            None,
            [0.0, 0.0, 0.0, 1.0],
            1.0,
            None,
            &entity,
            None,
            None,
            None,
            None,
            &Default::default(),
        );
        let mut bounds = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        for vertex in wires.iter().flat_map(|wire| &wire.text_verts) {
            let x = vertex.pos[0] as f64 + vertex.pos_low[0] as f64;
            let y = vertex.pos[1] as f64 + vertex.pos_low[1] as f64;
            bounds = [
                bounds[0].min(x),
                bounds[1].min(y),
                bounds[2].max(x),
                bounds[3].max(y),
            ];
        }
        assert!(bounds[0] <= bounds[2], "the cells' text is drawn");
        bounds
    };
    let (picture, cells) = (text_box(placed), text_box(&from_cells));
    assert!(
        picture
            .iter()
            .zip(&cells)
            .all(|(a, b)| (a - b).abs() < 1e-4),
        "picture text {picture:?}, drawn from the cells {cells:?}"
    );
}
