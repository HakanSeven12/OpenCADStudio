// DWF export — plots the shared paper-space `PlotPage` model to a DWF 6
// package: a zip after a "(DWF V06.00)" tag holding a manifest and, for each
// sheet, a descriptor and a W2D graphics stream. It is the package the DWF
// underlay reader (`scene::model::underlay_vector::dwf`) opens, written the
// other way round.
//
// Scope of this first version:
// - Wires (curves flattened, dashes cut as on screen), lineweights, colours
//   through the plot style, solid fills, hatches, wipeouts and text runs.
// - Raster images are left out.
// - Graphics units are 1/1200 inch on paper, y up, origin at the sheet's
//   lower left. The stream's `Units` carry the plot scale, so a reader maps
//   the sheet back to the units it was plotted from.

use std::path::Path;

use crate::io::plot::PlotPage;
use crate::io::plot_style::PlotStyleTable;

/// One sheet of the package: a plot page under the name readers list it by.
pub struct DwfSheet<'a> {
    pub name: &'a str,
    pub page: &'a PlotPage,
}

/// The bytes of a DWF 6 package with one section per sheet, in order.
pub fn build_dwf(
    _sheets: &[DwfSheet<'_>],
    _fallback_plot_style: Option<&PlotStyleTable>,
) -> Result<Vec<u8>, String> {
    Err("DWF export is not written yet".to_string())
}

/// Export several sheets into one DWF file.
#[cfg(not(target_arch = "wasm32"))]
pub fn export_dwf_pages(
    sheets: &[DwfSheet<'_>],
    path: &Path,
    fallback_plot_style: Option<&PlotStyleTable>,
) -> Result<(), String> {
    let bytes = build_dwf(sheets, fallback_plot_style)?;
    let temp_path = super::save_temp_path(path);
    if let Err(error) = std::fs::write(&temp_path, bytes) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!("Failed to write DWF data: {error}"));
    }
    if let Err(error) = super::replace_save_file(&temp_path, path, false) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!("Failed to replace DWF file: {error}"));
    }
    Ok(())
}

/// Export one page to a DWF file, as the sheet `name`.
#[cfg(not(target_arch = "wasm32"))]
pub fn export_dwf(page: &PlotPage, name: &str, path: &Path) -> Result<(), String> {
    export_dwf_pages(&[DwfSheet { name, page }], path, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::plot::{PlotContent, PlotOptions, PlotWire};
    use crate::scene::model::hatch_model::{HatchModel, HatchPattern, PatFamily};
    use crate::scene::model::underlay_vector::dwf as reader;
    use crate::scene::model::underlay_vector::{Segment, Sheet};
    use crate::scene::model::wire_model::SearchableTextRun;
    use crate::scene::WireModel;
    use std::io::Read;

    /// One graphics unit on paper, in millimetres: what a coordinate may be
    /// off by after rounding to the stream's integers.
    const UNIT_MM: f64 = 25.4 / 1200.0;

    fn page(content: PlotContent, w: f64, h: f64) -> PlotPage {
        PlotPage {
            content,
            paper_w: w,
            paper_h: h,
            offset_x: 0.0,
            offset_y: 0.0,
            rotation_deg: 0,
            content_rotation: 0.0,
            scale: 1.0,
            clip: None,
            options: PlotOptions::default(),
            plot_style: None,
            geopdf: None,
        }
    }

    fn wire(points: Vec<[f32; 3]>, color: [f32; 4]) -> WireModel {
        WireModel::solid("1A".to_string(), points, color, false)
    }

    fn wires(list: Vec<WireModel>) -> PlotContent {
        PlotContent {
            wires: std::sync::Arc::new(
                list.into_iter()
                    .map(|wire| PlotWire {
                        wire,
                        draw_depth: 0.5,
                    })
                    .collect(),
            ),
            ..Default::default()
        }
    }

    fn square_hatch(pattern: HatchPattern, name: &str, color: [f32; 4]) -> HatchModel {
        HatchModel {
            entity: None,
            effective_layer: None,
            pattern_origin: None,
            render_instance: None,
            world_origin: [0.0, 0.0],
            boundary: std::sync::Arc::new(vec![
                [20.0, 20.0],
                [40.0, 20.0],
                [40.0, 40.0],
                [20.0, 40.0],
            ]),
            boundary_wcs: None,
            fill_plane: None,
            fill_plane_boundary: None,
            boundary_exterior: None,
            boundary_sources: None,
            boundary_paths: None,
            style: document::entities::HatchStyleType::Normal,
            pattern,
            name: name.to_string(),
            color,
            aci: 0,
            line_weight_px: 1.0,
            angle_offset: 0.0,
            scale: 1.0,
            draw_depth: 0.3,
        }
    }

    fn run(text: &str, origin: [f64; 2], height: f32, rotation: f32, font: &str) -> WireModel {
        let mut wire = WireModel::default();
        wire.searchable_text = vec![SearchableTextRun {
            text: text.to_string(),
            origin: [origin[0], origin[1], 0.0],
            height,
            rotation,
            color: [0.0, 0.0, 1.0, 1.0],
            bold: false,
            font: font.to_string(),
            adv_width: 10.0,
        }];
        wire
    }

    /// The page as the sheet "Model" of a package, read back by the underlay
    /// reader.
    fn read(page: &PlotPage) -> Sheet {
        let bytes = build_dwf(
            &[DwfSheet {
                name: "Model",
                page,
            }],
            None,
        )
        .expect("builds");
        reader::sheet(&bytes, "Model").expect("the reader opens the sheet")
    }

    /// The straight pieces of the sheet's stroked paths, in order.
    fn strokes(sheet: &Sheet) -> Vec<[[f64; 2]; 2]> {
        sheet
            .paths
            .iter()
            .filter(|path| path.stroke.is_some())
            .flat_map(|path| &path.subpaths)
            .flat_map(|subpath| &subpath.segments)
            .map(|segment| match segment {
                Segment::Line(a, b) => [*a, *b],
                Segment::Cubic(a, _, _, d) => [*a, *d],
            })
            .collect()
    }

    /// Twice the area the sheet's filled paths cover, each closed subpath a
    /// polygon.
    fn filled_area(sheet: &Sheet) -> f64 {
        sheet
            .paths
            .iter()
            .filter(|path| path.fill.is_some())
            .flat_map(|path| &path.subpaths)
            .map(|subpath| {
                subpath
                    .segments
                    .iter()
                    .map(|segment| match segment {
                        Segment::Line(a, b) | Segment::Cubic(a, _, _, b) => {
                            a[0] * b[1] - b[0] * a[1]
                        }
                    })
                    .sum::<f64>()
                    .abs()
                    / 2.0
            })
            .sum()
    }

    fn near(a: [f64; 2], b: [f64; 2], tolerance: f64) -> bool {
        (a[0] - b[0]).abs() <= tolerance && (a[1] - b[1]).abs() <= tolerance
    }

    fn unzip(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("a zip");
        (0..archive.len())
            .map(|index| {
                let mut file = archive.by_index(index).expect("an entry");
                let mut data = Vec::new();
                file.read_to_end(&mut data).expect("inflates");
                (file.name().to_string(), data)
            })
            .collect()
    }

    #[test]
    fn an_empty_page_is_a_dwf_6_package_with_one_sheet() {
        let empty = page(PlotContent::default(), 297.0, 210.0);
        let bytes = build_dwf(
            &[DwfSheet {
                name: "Model",
                page: &empty,
            }],
            None,
        )
        .expect("builds");

        assert!(bytes.starts_with(b"(DWF V06.00)"), "the package tag");
        let files = unzip(&bytes);
        let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names[0], "manifest.xml", "{names:?}");
        assert_eq!(
            names
                .iter()
                .filter(|name| name.ends_with("descriptor.xml"))
                .count(),
            1,
            "{names:?}"
        );
        let stream = files
            .iter()
            .find(|(name, _)| name.ends_with(".w2d"))
            .map(|(_, data)| data)
            .expect("a graphics stream");
        assert!(stream.starts_with(b"(W2D V06.00)"));
        assert!(stream.ends_with(b"(EndOfDWF)"));
        assert_eq!(reader::sheet_names(&bytes), Some(vec!["Model".to_string()]));
    }

    #[test]
    fn the_zip_counts_its_offsets_from_the_start_of_the_file() {
        let empty = page(PlotContent::default(), 297.0, 210.0);
        let bytes = build_dwf(
            &[DwfSheet {
                name: "Model",
                page: &empty,
            }],
            None,
        )
        .expect("builds");

        // The end record names where the directory starts, and the
        // directory where its first file does: right after the tag.
        let end = bytes
            .windows(4)
            .rposition(|window| window == b"PK\x05\x06")
            .expect("the zip's end record");
        let offset = |at: usize| {
            u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes")) as usize
        };
        let directory = offset(end + 16);
        assert_eq!(&bytes[directory..directory + 4], b"PK\x01\x02");
        assert_eq!(offset(directory + 42), 12);
        assert_eq!(&bytes[12..16], b"PK\x03\x04");
    }

    #[test]
    fn the_sheet_reads_back_at_its_paper_size() {
        let sheet = read(&page(PlotContent::default(), 297.0, 210.0));

        let [x0, y0, x1, y1] = sheet.rect;
        assert!(near([x0, y0], [0.0, 0.0], UNIT_MM), "{:?}", sheet.rect);
        assert!(near([x1, y1], [297.0, 210.0], UNIT_MM), "{:?}", sheet.rect);
    }

    #[test]
    fn the_descriptor_names_the_paper_and_the_scale_to_it() {
        let empty = page(PlotContent::default(), 420.0, 297.0);
        let bytes = build_dwf(
            &[DwfSheet {
                name: "A3 <plan>",
                page: &empty,
            }],
            None,
        )
        .expect("builds");

        let files = unzip(&bytes);
        let descriptor = files
            .iter()
            .find(|(name, _)| name.ends_with("descriptor.xml"))
            .map(|(_, data)| String::from_utf8(data.clone()).expect("utf-8"))
            .expect("a descriptor");
        let xml = roxmltree::Document::parse(&descriptor).expect("well-formed descriptor");
        let page_node = xml.root_element();
        assert_eq!(page_node.attribute("name"), Some("A3 <plan>"));
        let paper = xml
            .descendants()
            .find(|node| node.tag_name().name() == "Paper")
            .expect("a Paper element");
        assert_eq!(paper.attribute("units"), Some("mm"));
        assert_eq!(paper.attribute("width"), Some("420"));
        assert_eq!(paper.attribute("height"), Some("297"));
        let graphics = xml
            .descendants()
            .find(|node| node.tag_name().name() == "GraphicResource")
            .expect("a graphics resource");
        let transform: Vec<f64> = graphics
            .attribute("transform")
            .expect("a transform")
            .split_whitespace()
            .map(|value| value.parse().expect("a number"))
            .collect();
        assert_eq!(transform.len(), 16);
        assert!((transform[0] - UNIT_MM).abs() < 1e-12, "{transform:?}");
        assert!((transform[5] - UNIT_MM).abs() < 1e-12, "{transform:?}");
        let manifest = files
            .iter()
            .find(|(name, _)| name == "manifest.xml")
            .map(|(_, data)| String::from_utf8(data.clone()).expect("utf-8"))
            .expect("a manifest");
        roxmltree::Document::parse(&manifest).expect("well-formed manifest");
    }

    #[test]
    fn a_wire_reads_back_where_it_was_drawn_in_its_colour() {
        let content = wires(vec![wire(
            vec![[10.0, 20.0, 0.0], [110.0, 20.0, 0.0], [110.0, 70.0, 0.0]],
            [1.0, 0.0, 0.0, 1.0],
        )]);

        let sheet = read(&page(content, 297.0, 210.0));

        let lines = strokes(&sheet);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(near(lines[0][0], [10.0, 20.0], UNIT_MM), "{lines:?}");
        assert!(near(lines[0][1], [110.0, 20.0], UNIT_MM), "{lines:?}");
        assert!(near(lines[1][1], [110.0, 70.0], UNIT_MM), "{lines:?}");
        assert_eq!(
            sheet.paths[0].stroke.map(|stroke| stroke.0),
            Some([255, 0, 0])
        );
    }

    #[test]
    fn a_plot_scale_reads_back_in_drawing_units() {
        // Half a millimetre of paper for each drawing unit.
        let content = wires(vec![wire(
            vec![[0.0, 0.0, 0.0], [100.0, 40.0, 0.0]],
            [0.0, 0.0, 0.0, 1.0],
        )]);
        let mut scaled = page(content, 297.0, 210.0);
        scaled.scale = 0.5;

        let sheet = read(&scaled);

        let lines = strokes(&sheet);
        assert!(near(lines[0][1], [100.0, 40.0], 2.0 * UNIT_MM), "{lines:?}");
        assert!(
            near(
                [sheet.rect[2], sheet.rect[3]],
                [594.0, 420.0],
                2.0 * UNIT_MM
            ),
            "{:?}",
            sheet.rect
        );
    }

    #[test]
    fn a_pen_up_starts_a_new_stroke() {
        let content = wires(vec![wire(
            vec![
                [0.0, 0.0, 0.0],
                [10.0, 0.0, 0.0],
                [f32::NAN, f32::NAN, f32::NAN],
                [50.0, 5.0, 0.0],
                [60.0, 5.0, 0.0],
            ],
            [0.0, 0.0, 0.0, 1.0],
        )]);

        let sheet = read(&page(content, 297.0, 210.0));

        let lines = strokes(&sheet);
        assert_eq!(lines.len(), 2, "no stroke joins the two runs: {lines:?}");
        assert!(near(lines[1][0], [50.0, 5.0], UNIT_MM), "{lines:?}");
    }

    #[test]
    fn a_lineweight_reads_back_in_millimetres() {
        let mut thick = wire(
            vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
            [0.0, 0.0, 0.0, 1.0],
        );
        // 96 px to the inch: 0.5 mm.
        thick.line_weight_px = 0.5 * 96.0 / 25.4;

        let sheet = read(&page(wires(vec![thick]), 297.0, 210.0));

        let width = sheet.paths[0].stroke.expect("a stroke").1;
        assert!((width - 0.5).abs() <= UNIT_MM, "{width}");
    }

    #[test]
    fn a_white_wire_plots_black_as_on_paper() {
        let content = wires(vec![wire(
            vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
            [1.0, 1.0, 1.0, 1.0],
        )]);

        let sheet = read(&page(content, 297.0, 210.0));

        assert_eq!(
            sheet.paths[0].stroke.map(|stroke| stroke.0),
            Some([0, 0, 0])
        );
    }

    #[test]
    fn a_wire_drawn_on_top_comes_later_in_the_stream() {
        let red = wire(
            vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
            [1.0, 0.0, 0.0, 1.0],
        );
        let blue = wire(
            vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
            [0.0, 0.0, 1.0, 1.0],
        );
        let content = PlotContent {
            wires: std::sync::Arc::new(vec![
                PlotWire {
                    wire: red,
                    draw_depth: 0.8,
                },
                PlotWire {
                    wire: blue,
                    draw_depth: 0.2,
                },
            ]),
            ..Default::default()
        };

        let sheet = read(&page(content, 297.0, 210.0));

        let colors: Vec<[u8; 3]> = sheet
            .paths
            .iter()
            .filter_map(|path| path.stroke.map(|stroke| stroke.0))
            .collect();
        assert_eq!(colors, vec![[0, 0, 255], [255, 0, 0]]);
    }

    #[test]
    fn a_filled_triangle_reads_back_filled() {
        let mut fill = WireModel::default();
        fill.color = [0.0, 0.0, 1.0, 1.0];
        fill.fill_tris = vec![[5.0, 5.0, 0.0], [25.0, 5.0, 0.0], [5.0, 15.0, 0.0]];

        let sheet = read(&page(wires(vec![fill]), 297.0, 210.0));

        let filled: Vec<_> = sheet
            .paths
            .iter()
            .filter(|path| path.fill.is_some())
            .collect();
        assert_eq!(filled.len(), 1);
        assert_eq!(filled[0].fill, Some([0, 0, 255]));
        assert!(
            (filled_area(&sheet) - 100.0).abs() < 1.0,
            "{}",
            filled_area(&sheet)
        );
    }

    #[test]
    fn a_solid_hatch_reads_back_as_its_area() {
        let content = PlotContent {
            hatches: vec![square_hatch(
                HatchPattern::Solid,
                "SOLID",
                [1.0, 1.0, 0.0, 1.0],
            )],
            ..Default::default()
        };

        let sheet = read(&page(content, 297.0, 210.0));

        assert!(
            (filled_area(&sheet) - 400.0).abs() < 1.0,
            "{}",
            filled_area(&sheet)
        );
        assert!(sheet
            .paths
            .iter()
            .all(|path| path.fill == Some([255, 255, 0])));
    }

    #[test]
    fn a_wipeout_reads_back_white() {
        let content = PlotContent {
            wipeouts: vec![square_hatch(
                HatchPattern::Solid,
                "WIPEOUT_FILL",
                [0.2, 0.2, 0.2, 1.0],
            )],
            ..Default::default()
        };

        let sheet = read(&page(content, 297.0, 210.0));

        assert!(
            (filled_area(&sheet) - 400.0).abs() < 1.0,
            "{}",
            filled_area(&sheet)
        );
        assert!(sheet
            .paths
            .iter()
            .all(|path| path.fill == Some([255, 255, 255])));
    }

    #[test]
    fn a_pattern_hatch_reads_back_as_its_lines() {
        // Horizontal lines 5 apart across a square from 20 to 40.
        let pattern = HatchPattern::Pattern(vec![PatFamily {
            angle_deg: 0.0,
            x0: 0.0,
            y0: 0.0,
            dx: 0.0,
            dy: 5.0,
            dashes: Vec::new(),
        }]);
        let content = PlotContent {
            hatches: vec![square_hatch(pattern, "LINES", [0.0, 1.0, 0.0, 1.0])],
            ..Default::default()
        };

        let sheet = read(&page(content, 297.0, 210.0));

        let lines = strokes(&sheet);
        assert!(lines.len() >= 3, "{lines:?}");
        for [a, b] in &lines {
            assert!((a[1] - b[1]).abs() <= UNIT_MM, "a level line: {a:?} {b:?}");
            assert!((a[0] - b[0]).abs() > 19.0, "across the square: {a:?} {b:?}");
        }
        assert_eq!(filled_area(&sheet), 0.0, "no fill for a pattern");
    }

    #[test]
    fn text_reads_back_with_its_font_height_and_rotation() {
        let quarter = std::f32::consts::FRAC_PI_2;
        let content = wires(vec![run("ROOM 12", [20.0, 30.0], 5.0, quarter, "Arial")]);

        let sheet = read(&page(content, 297.0, 210.0));

        assert_eq!(sheet.texts.len(), 1);
        let text = &sheet.texts[0];
        assert_eq!(text.text, "ROOM 12");
        assert!(
            near(text.origin, [20.0, 30.0], UNIT_MM),
            "{:?}",
            text.origin
        );
        assert!((text.height - 5.0).abs() < 0.02, "{}", text.height);
        assert!(
            (text.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-3,
            "{}",
            text.rotation
        );
        assert_eq!(text.font, "Arial");
        assert_eq!(text.color, [0, 0, 255]);
    }

    #[test]
    fn text_beyond_ascii_reads_back() {
        let content = wires(vec![
            run("Ñandú ø 25 — «sala»", [20.0, 30.0], 5.0, 0.0, "Arial"),
            run("it's 3\\4", [20.0, 50.0], 5.0, 0.0, "Arial"),
        ]);

        let sheet = read(&page(content, 297.0, 210.0));

        let texts: Vec<&str> = sheet.texts.iter().map(|text| text.text.as_str()).collect();
        assert_eq!(texts, vec!["Ñandú ø 25 — «sala»", "it's 3\\4"]);
    }

    #[test]
    fn several_sheets_keep_their_names_and_order() {
        let first = page(
            wires(vec![wire(
                vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
                [0.0, 0.0, 0.0, 1.0],
            )]),
            297.0,
            210.0,
        );
        let second = page(
            wires(vec![wire(
                vec![[0.0, 0.0, 0.0], [0.0, 30.0, 0.0]],
                [0.0, 0.0, 0.0, 1.0],
            )]),
            420.0,
            297.0,
        );

        let bytes = build_dwf(
            &[
                DwfSheet {
                    name: "Plan",
                    page: &first,
                },
                DwfSheet {
                    name: "Details",
                    page: &second,
                },
            ],
            None,
        )
        .expect("builds");

        assert_eq!(
            reader::sheet_names(&bytes),
            Some(vec!["Plan".to_string(), "Details".to_string()])
        );
        let details = reader::sheet(&bytes, "Details").expect("the second sheet");
        assert!(near(strokes(&details)[0][1], [0.0, 30.0], UNIT_MM));
        assert!(near(
            [details.rect[2], details.rect[3]],
            [420.0, 297.0],
            UNIT_MM
        ));
    }

    #[test]
    fn a_long_wire_keeps_every_point() {
        // More points than one polyline of the stream can count.
        let points: Vec<[f32; 3]> = (0..70_000)
            .map(|index| [(index % 200) as f32, (index / 200) as f32 * 0.5, 0.0])
            .collect();
        let count = points.len();

        let sheet = read(&page(
            wires(vec![wire(points, [0.0, 0.0, 0.0, 1.0])]),
            297.0,
            210.0,
        ));

        let lines = strokes(&sheet);
        assert_eq!(lines.len(), count - 1);
        assert!(near(lines[count - 2][1], [199.0, 174.5], UNIT_MM));
    }

    #[test]
    fn a_large_sheet_keeps_its_far_points() {
        // Across an A0 sheet: further than a 16-bit step reaches.
        let content = wires(vec![wire(
            vec![[5.0, 5.0, 0.0], [1180.0, 835.0, 0.0], [5.0, 835.0, 0.0]],
            [0.0, 0.0, 0.0, 1.0],
        )]);

        let sheet = read(&page(content, 1189.0, 841.0));

        let lines = strokes(&sheet);
        assert!(near(lines[0][1], [1180.0, 835.0], UNIT_MM), "{lines:?}");
        assert!(near(lines[1][1], [5.0, 835.0], UNIT_MM), "{lines:?}");
    }

    #[test]
    fn a_dashed_wire_is_cut_into_its_dashes() {
        let mut dashed = wire(
            vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]],
            [0.0, 0.0, 0.0, 1.0],
        );
        dashed.pattern_length = 10.0;
        dashed.pattern = [6.0, -4.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];

        let sheet = read(&page(wires(vec![dashed]), 297.0, 210.0));

        let lines = strokes(&sheet);
        assert!(lines.len() >= 9, "several dashes: {lines:?}");
        let lit: f64 = lines.iter().map(|[a, b]| (b[0] - a[0]).abs()).sum();
        assert!(lit > 50.0 && lit < 75.0, "about six tenths lit: {lit}");
    }

    #[test]
    fn a_page_turned_a_quarter_reads_back_turned() {
        // The page turns the content a quarter: +x of the content runs up
        // the sheet, +y towards its left edge.
        let content = wires(vec![wire(
            vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]],
            [0.0, 0.0, 0.0, 1.0],
        )]);
        let mut turned = page(content, 210.0, 297.0);
        turned.rotation_deg = 90;

        let sheet = read(&turned);

        let lines = strokes(&sheet);
        assert!(near(lines[0][0], [210.0, 0.0], UNIT_MM), "{lines:?}");
        assert!(near(lines[0][1], [210.0, 100.0], UNIT_MM), "{lines:?}");
    }
}
