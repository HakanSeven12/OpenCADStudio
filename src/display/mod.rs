//! What entities draw as, before any GPU is involved: the shapes an entity
//! becomes (`render_entity`), the wire model the scene caches and picks
//! (`wire_model`), shared block instances (`instance_model`) and the vertex
//! of a text quad (`text_vertex`). The drawing model produces these; the
//! scene caches them and the pipeline uploads them.

pub mod dim_geom;
pub mod hatch_model;
pub mod hatch_patterns;
pub mod instance_model;
pub mod linetype_style;
pub mod point_residuals;
pub mod proxy_graphics;
pub mod render_entity;
pub mod solid_mesh;
pub mod tess_util;
pub mod text_vertex;
pub mod wire_model;
