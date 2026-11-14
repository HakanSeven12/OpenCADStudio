use document::objects::ObjectType;
use document::{Document, DxfVersion};
use OpenCADStudio::io::crs::{self, CrsDefinition};

#[test]
fn authored_epsg_5514_geodata_survives_dwg_and_dxf_reopen() {
    for format in ["dwg", "dxf"] {
        let mut document = Document::new();
        document.version = DxfVersion::AC1032;
        let crs = CrsDefinition::from_epsg(5514).unwrap();
        let handle = crs::set_document_crs(&mut document, &crs).unwrap();
        let model_space = document.block_records.get("*Model_Space").unwrap().handle;
        let Some(ObjectType::GeoData(geo)) = document.objects.get_mut(&handle) else {
            unreachable!()
        };
        geo.design_point = document::types::Vector3::new(-742_925.25, -1_043_020.5, 0.0);
        geo.reference_point = document::types::Vector3::new(14.42076, 50.08804, 0.0);
        geo.north_direction = document::types::Vector2::new(0.0, 1.0);
        geo.up_direction = document::types::Vector3::UNIT_Z;
        geo.horizontal_unit_scale = 1.0;
        geo.vertical_unit_scale = 1.0;
        // Version 2/3 GEODATA has no separate datum field in the native
        // format; the RSS tag is a persisted non-CRS metadata field.
        geo.geo_rss_tag = "site-control".to_string();
        let expected_definition = geo.coordinate_system_definition.clone();
        let expected_geo = geo.clone();

        let bytes = OpenCADStudio::io::save_to_bytes(&document, format, document.version)
            .unwrap_or_else(|error| panic!("save {format}: {error}"));
        let reopened = OpenCADStudio::io::load_bytes(&format!("geodata.{format}"), bytes)
            .unwrap_or_else(|error| panic!("reopen {format}: {error}"));
        let reopened_handle = crs::model_space_geo_data_handle(&reopened)
            .unwrap_or_else(|| panic!("canonical GEODATA missing after {format} reopen"));
        assert_eq!(reopened_handle, handle, "{format} handle");
        let reopened_geo = crs::model_space_geo_data(&reopened).unwrap();
        assert_eq!(reopened_geo.host_block, model_space, "{format} host block");
        assert_eq!(reopened_geo.coordinate_type, 2, "{format} coordinate type");
        assert_eq!(
            reopened_geo.coordinate_system_definition, expected_definition,
            "{format} definition"
        );
        assert_eq!(
            CrsDefinition::from_geodata(reopened_geo).unwrap().code(),
            Some(5514),
            "{format} resolved CRS"
        );
        assert_eq!(
            reopened_geo.design_point, expected_geo.design_point,
            "{format} design point"
        );
        assert_eq!(
            reopened_geo.reference_point, expected_geo.reference_point,
            "{format} reference point"
        );
        assert_eq!(
            reopened_geo.north_direction, expected_geo.north_direction,
            "{format} north direction"
        );
        assert_eq!(
            reopened_geo.up_direction, expected_geo.up_direction,
            "{format} up direction"
        );
        assert_eq!(
            reopened_geo.geo_rss_tag, "site-control",
            "{format} non-CRS metadata"
        );
    }
}
