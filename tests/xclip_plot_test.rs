use codec::entities::{Insert, Line};
use codec::objects::{Dictionary, DictionaryWithDefault, ObjectType, SpatialFilter};
use codec::tables::BlockRecord;
use codec::types::{Matrix4, Vector2, Vector3};
use codec::EntityType;
use OpenCADStudio::scene::pick::xclip::{
    insert_spatial_filter, world_clip_polygon_for_transform,
};
use OpenCADStudio::scene::Scene;

#[test]
fn xclip_in_paper_and_viewport() {
    let mut scene = Scene::new();
    scene.document = OpenCADStudio::io::load_bytes(
        "embedded-image-print.dxf",
        include_bytes!("fixtures/embedded-image-print.dxf").to_vec(),
    )
    .expect("fixture DXF with a Layout1");

    let block_name = "HOUSE";
    let br_h = scene.document.allocate_handle();
    let mut br = BlockRecord::new(block_name);
    br.handle = br_h;
    scene.document.block_records.add(br).unwrap();

    let mut line = Line::new();
    line.start = Vector3::new(0.0, 0.0, 0.0);
    line.end = Vector3::new(100.0, 100.0, 0.0);
    let mut line_e = EntityType::Line(line);
    line_e.common_mut().owner_handle = br_h;
    scene.add_entity(line_e);

    // Setup XCLIP on paper space insert:
    let h_xdict1 = scene.document.allocate_handle();
    let h_filter1 = scene.document.allocate_handle();
    let h_spatial1 = scene.document.allocate_handle();

    let mut xdict1 = Dictionary::new();
    xdict1.handle = h_xdict1;
    xdict1.add_entry("ACAD_FILTER", h_filter1);
    scene.document.objects.insert(h_xdict1, ObjectType::Dictionary(xdict1));

    let mut filter_dict1 = Dictionary::new();
    filter_dict1.handle = h_filter1;
    filter_dict1.add_entry("SPATIAL", h_spatial1);
    scene.document.objects.insert(h_filter1, ObjectType::Dictionary(filter_dict1));

    let mut sf1 = SpatialFilter::new();
    sf1.handle = h_spatial1;
    sf1.display_enabled = true;
    sf1.boundary_points = vec![Vector2::new(0.0, 0.0), Vector2::new(50.0, 50.0)];
    scene.document.objects.insert(h_spatial1, ObjectType::SpatialFilter(sf1));

    scene.set_current_layout("Layout1".into());

    // Insert on Paper space
    let mut ins_paper = Insert::new(block_name, Vector3::new(200.0, 0.0, 0.0));
    ins_paper.common.xdictionary_handle = Some(h_xdict1);
    scene.add_entity(EntityType::Insert(ins_paper));

    let (paper_wires, _model_wires) = scene.plot_wire_groups(None);
    assert!(!paper_wires.is_empty(), "expected clipped paper wires");
    let house_wire = paper_wires
        .iter()
        .find(|w| w.points.iter().any(|p| p[0] >= 200.0 && p[0] <= 250.0))
        .expect("found clipped house wire in paper space");
    // Verify point coordinates do not exceed clip boundary (250.0)
    for p in &house_wire.points {
        if p[0].is_finite() {
            assert!(p[0] <= 250.001, "point X {} exceeds clip boundary 250.0", p[0]);
            assert!(p[1] <= 50.001, "point Y {} exceeds clip boundary 50.0", p[1]);
        }
    }
}

#[test]
fn test_xclip_extension_dictionary_resolution() {
    let mut scene = Scene::new();
    let block_name = "TEST_EXT_DICT";
    let br_h = scene.document.allocate_handle();
    let mut br = BlockRecord::new(block_name);
    br.handle = br_h;
    scene.document.block_records.add(br).unwrap();

    let ins_handle = scene.document.allocate_handle();
    let mut ins = Insert::new(block_name, Vector3::new(100.0, 50.0, 0.0));
    ins.common.handle = ins_handle;

    // Create DictionaryWithDefault as the extension dictionary, owned by ins_handle:
    let h_xdict = scene.document.allocate_handle();
    ins.common.xdictionary_handle = Some(h_xdict);
    let mut xdict = DictionaryWithDefault::new();
    xdict.handle = h_xdict;
    xdict.owner = ins_handle;

    let h_filter = scene.document.allocate_handle();
    // Mixed case and whitespace in key:
    xdict.entries.push(("  acad_filter  ".to_string(), h_filter));
    scene.document.objects.insert(h_xdict, ObjectType::DictionaryWithDefault(xdict));

    // Sub-dictionary (ACAD_FILTER) also as DictionaryWithDefault:
    let mut filter_dict = DictionaryWithDefault::new();
    filter_dict.handle = h_filter;
    filter_dict.owner = h_xdict;
    let h_spatial = scene.document.allocate_handle();
    filter_dict.entries.push(("spatial".to_string(), h_spatial));
    scene.document.objects.insert(h_filter, ObjectType::DictionaryWithDefault(filter_dict));

    let mut sf = SpatialFilter::new();
    sf.handle = h_spatial;
    sf.display_enabled = true;
    sf.boundary_points = vec![Vector2::new(10.0, 10.0), Vector2::new(40.0, 40.0)];
    scene.document.objects.insert(h_spatial, ObjectType::SpatialFilter(sf));

    let resolved = insert_spatial_filter(&scene.document, &ins);
    assert!(resolved.is_some(), "SpatialFilter should be resolved through a DictionaryWithDefault with loosely written keys");
    let sf_ref = resolved.unwrap();
    assert_eq!(sf_ref.handle, h_spatial);
}

#[test]
fn test_xclip_inverse_block_transform_used_as_stored() {
    // The codec decodes the on-disk column-major 4x3 layout into row-major
    // `Matrix4`, so the stored transform is used as-is: insert at (200, 150),
    // inverse is translation (-200, -150, 0).
    let mut sf = SpatialFilter::new();
    sf.inverse_block_transform = Matrix4 {
        m: [
            [1.0, 0.0, 0.0, -200.0],
            [0.0, 1.0, 0.0, -150.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
    sf.boundary_points = vec![Vector2::new(210.0, 160.0), Vector2::new(240.0, 190.0)];

    let ins = Insert::new("B", Vector3::new(200.0, 150.0, 0.0));
    let xform = ins.get_transform();
    let poly = world_clip_polygon_for_transform(&sf, &xform);

    // Boundary was defined in WCS: [210, 160] to [240, 190].
    // Inv maps 210 -> 210 - 200 = 10, 160 -> 160 - 150 = 10.
    // xform maps 10 -> 10 + 200 = 210, 10 -> 10 + 150 = 160.
    // The world polygon corners must match the original rectangle!
    assert_eq!(poly.len(), 4);
    assert!((poly[0][0] - 210.0).abs() < 1e-4);
    assert!((poly[0][1] - 160.0).abs() < 1e-4);
    assert!((poly[2][0] - 240.0).abs() < 1e-4);
    assert!((poly[2][1] - 190.0).abs() < 1e-4);
}

#[test]
fn test_xclip_multiple_inserts_same_block_different_clips() {
    let mut scene = Scene::new();
    let block_name = "FLOOR_PLAN_MAIN";
    let br_h = scene.document.allocate_handle();
    let mut br = BlockRecord::new(block_name);
    br.handle = br_h;
    scene.document.block_records.add(br).unwrap();

    // Block contains a diagonal line from (0, 0) to (100, 100)
    let mut line = Line::new();
    line.start = Vector3::new(0.0, 0.0, 0.0);
    line.end = Vector3::new(100.0, 100.0, 0.0);
    let mut line_e = EntityType::Line(line);
    line_e.common_mut().owner_handle = br_h;
    scene.add_entity(line_e);

    // Insert 1: placed at (0, 0), clipped to [10, 10]..[30, 30]
    let h_xdict1 = scene.document.allocate_handle();
    let h_filter1 = scene.document.allocate_handle();
    let h_spatial1 = scene.document.allocate_handle();
    let mut xdict1 = Dictionary::new();
    xdict1.handle = h_xdict1;
    xdict1.add_entry("ACAD_FILTER", h_filter1);
    scene.document.objects.insert(h_xdict1, ObjectType::Dictionary(xdict1));
    let mut fdict1 = Dictionary::new();
    fdict1.handle = h_filter1;
    fdict1.add_entry("SPATIAL", h_spatial1);
    scene.document.objects.insert(h_filter1, ObjectType::Dictionary(fdict1));
    let mut sf1 = SpatialFilter::new();
    sf1.handle = h_spatial1;
    sf1.display_enabled = true;
    sf1.boundary_points = vec![Vector2::new(10.0, 10.0), Vector2::new(30.0, 30.0)];
    scene.document.objects.insert(h_spatial1, ObjectType::SpatialFilter(sf1));

    let mut ins1 = Insert::new(block_name, Vector3::new(0.0, 0.0, 0.0));
    ins1.common.xdictionary_handle = Some(h_xdict1);
    scene.add_entity(EntityType::Insert(ins1));

    // Insert 2: placed at (500, 0), clipped to [560, 60]..[580, 80].
    // Inverse block transform is translation (-500, 0, 0), as decoded by
    // the codec from the on-disk column-major layout:
    let h_xdict2 = scene.document.allocate_handle();
    let h_filter2 = scene.document.allocate_handle();
    let h_spatial2 = scene.document.allocate_handle();
    let mut xdict2 = Dictionary::new();
    xdict2.handle = h_xdict2;
    xdict2.add_entry("ACAD_FILTER", h_filter2);
    scene.document.objects.insert(h_xdict2, ObjectType::Dictionary(xdict2));
    let mut fdict2 = Dictionary::new();
    fdict2.handle = h_filter2;
    fdict2.add_entry("SPATIAL", h_spatial2);
    scene.document.objects.insert(h_filter2, ObjectType::Dictionary(fdict2));
    let mut sf2 = SpatialFilter::new();
    sf2.handle = h_spatial2;
    sf2.display_enabled = true;
    sf2.inverse_block_transform = Matrix4 {
        m: [
            [1.0, 0.0, 0.0, -500.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
    sf2.boundary_points = vec![Vector2::new(560.0, 60.0), Vector2::new(580.0, 80.0)];
    scene.document.objects.insert(h_spatial2, ObjectType::SpatialFilter(sf2));

    let mut ins2 = Insert::new(block_name, Vector3::new(500.0, 0.0, 0.0));
    ins2.common.xdictionary_handle = Some(h_xdict2);
    scene.add_entity(EntityType::Insert(ins2));

    scene.set_current_layout("Model".into());
    let (model_wires, _) = scene.plot_wire_groups(None);

    // Filter wires for insert 1 (near origin) and insert 2 (near 500)
    let ins1_points: Vec<[f32; 3]> = model_wires
        .iter()
        .flat_map(|w| w.points.iter().copied())
        .filter(|p| p[0].is_finite() && p[0] < 200.0)
        .collect();

    let ins2_points: Vec<[f32; 3]> = model_wires
        .iter()
        .flat_map(|w| w.points.iter().copied())
        .filter(|p| p[0].is_finite() && p[0] >= 500.0)
        .collect();

    assert!(!ins1_points.is_empty(), "Insert 1 must have plotted wires");
    assert!(!ins2_points.is_empty(), "Insert 2 must have plotted wires");

    // All points for Insert 1 must be within [10, 10]..[30, 30]
    for p in &ins1_points {
        assert!(p[0] >= 9.99 && p[0] <= 30.01, "Insert 1 point X {} outside [10, 30]", p[0]);
        assert!(p[1] >= 9.99 && p[1] <= 30.01, "Insert 1 point Y {} outside [10, 30]", p[1]);
    }

    // All points for Insert 2 must be within [560, 60]..[580, 80]
    for p in &ins2_points {
        assert!(p[0] >= 559.99 && p[0] <= 580.01, "Insert 2 point X {} outside [560, 580]", p[0]);
        assert!(p[1] >= 59.99 && p[1] <= 80.01, "Insert 2 point Y {} outside [60, 80]", p[1]);
    }
}
