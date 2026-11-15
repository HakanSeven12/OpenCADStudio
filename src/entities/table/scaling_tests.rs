use super::*;
use document::entities::table::{CellContentGeometry, CellStyle, CellStylePropertyFlags as P};
use document::types::Vector3;
use glam::DVec3;

fn geometry() -> CellContentGeometry {
    CellContentGeometry {
        distance_to_top_left: Vector3::new(0.3, 0.4, 0.0),
        distance_to_center: Vector3::new(1.0, 0.5, 0.0),
        width: 1.0,
        height: 0.5,
        outer_width: 1.2,
        outer_height: 0.7,
        flags: 0,
    }
}

#[test]
fn table_scaling_transforms_content_styles_and_cached_geometry_once() {
    for factor in [0.001, 0.5, 3.0, 1000.0] {
        let mut table = Table::new(Vector3::new(4.0, 6.0, 0.0), 3, 4);
        table.set_cell_text(2, 1, "Content");
        let cell = &mut table.rows[2].cells[1];
        let mut style = CellStyle::new();
        style.property_flags = P::TEXT_HEIGHT | P::MARGIN_LEFT;
        style.text_height = 0.2;
        style.margin_left = 0.1;
        cell.style = Some(style);
        cell.geometry = Some(geometry());
        cell.geometries.push(geometry());
        cell.contents[0].geometry = Some(geometry());
        cell.contents[0].text_height = 0.2;
        cell.geometry_width_with_gap = 2.0;
        cell.geometry_height_with_gap = 1.0;
        let before = table.clone();
        Transformable::apply_transform(
            &mut table,
            &EntityTransform::Scale {
                center: DVec3::new(1.0, 2.0, 0.0),
                factor,
            },
        );
        let near = |a: f64, b: f64| assert!((a - b).abs() < 1e-9, "{a} != {b}");
        near(table.insertion_point.x, 1.0 + 3.0 * factor);
        near(table.insertion_point.y, 2.0 + 4.0 * factor);
        near(table.total_width(), before.total_width() * factor);
        near(table.total_height(), before.total_height() * factor);
        let cell = &table.rows[2].cells[1];
        near(cell.style.as_ref().unwrap().text_height, 0.2 * factor);
        near(cell.style.as_ref().unwrap().margin_left, 0.1 * factor);
        near(cell.contents[0].text_height, 0.2 * factor);
        near(cell.geometry_width_with_gap, 2.0 * factor);
        near(cell.geometry_height_with_gap, factor);
        for geometry in [
            cell.geometry.as_ref().unwrap(),
            &cell.geometries[0],
            cell.contents[0].geometry.as_ref().unwrap(),
        ] {
            near(geometry.distance_to_center.x, factor);
            near(geometry.distance_to_top_left.y, 0.4 * factor);
            near(geometry.outer_width, 1.2 * factor);
        }
    }
}

#[test]
fn table_scaling_indicator_fits_small_cells_and_multi_letter_columns() {
    let doc = document::Document::new();
    for size in [0.00001, 0.005, 5.0] {
        let mut table = Table::new(Vector3::ZERO, 3, 28);
        for column in &mut table.columns {
            column.width = size;
        }
        for row in &mut table.rows {
            row.height = size;
        }
        let indicator = table_indicator(&table, &doc, Some((1, 1, 2, 2))).unwrap();
        assert!(indicator.labels.iter().any(|(label, _)| label == "AB"));
        assert!(
            indicator.text_height <= size as f32 * 0.3,
            "labels exceed their cells: {} at size {size}",
            indicator.text_height
        );
        let corner = indicator.boxes[0].0;
        assert!((corner[1] - corner[0]).length() <= size as f32 * 1.01);
        assert!((corner[3] - corner[0]).length() <= size as f32 * 1.01);
        let outline = indicator.outline.unwrap();
        assert!(((outline[1] - outline[0]).length() - size as f32 * 2.0).abs() < 1e-5);
    }
}

/// One row and one column dragged down to almost nothing (a cell's top or
/// left grip pulled onto the neighbour's far edge) leave the header bands
/// at the size the rest of the grid gives them.
#[test]
fn table_indicator_bands_survive_one_collapsed_row_and_column() {
    let doc = document::Document::new();
    let mut table = Table::new(Vector3::ZERO, 4, 4);
    for column in &mut table.columns {
        column.width = 20.0;
    }
    for row in &mut table.rows {
        row.height = 10.0;
    }
    let before = table_indicator(&table, &doc, Some((2, 2, 2, 2))).unwrap();
    table.rows[1].height = 1.0e-6;
    table.columns[1].width = 1.0e-6;
    let after = table_indicator(&table, &doc, Some((2, 2, 2, 2))).unwrap();
    assert!(
        (after.text_height - before.text_height).abs() < 1e-6,
        "labels shrank from {} to {}",
        before.text_height,
        after.text_height
    );
    let corner = |indicator: &TableIndicator| {
        let c = indicator.boxes[0].0;
        ((c[1] - c[0]).length(), (c[3] - c[0]).length())
    };
    let (w0, h0) = corner(&before);
    let (w1, h1) = corner(&after);
    assert!(
        (w1 - w0).abs() < 1e-5 && (h1 - h0).abs() < 1e-5,
        "corner band {w0}x{h0} -> {w1}x{h1}"
    );
    assert!(w1 > 0.1 && h1 > 0.1, "corner band {w1}x{h1}");
}
