// DocApi v2 entity conversions now live in the crate (`ocs_doc_api::convert`),
// moved out of the host to minimize /src surface (the conversion logic is
// host-independent: acadrust + crate DTOs only). Re-exported here so existing
// `crate::app::doc_api_convert::*` paths keep working unchanged.
#[allow(unused_imports)] // re-export for tests + downstream; non-test build may not use all of it
pub use ocs_doc_api::convert::{
    bulge_arc_segment, curve_spec_to_entity, entity_bounds, entity_kind_name,
    entity_to_profile_curves, transform_entity_geometry,
};
