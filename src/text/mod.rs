//! Fonts and text layout with no GPU or scene behind them: the font faces
//! and their lookup (`font_repo`, `sysfont`), shape and stroke fonts (`shx`,
//! `lff`), TrueType shaping (`ttf_glyph`, `web_font`), the SDF glyph atlas
//! and glyph quad layout.

pub mod font_face;
pub mod font_repo;
pub mod glyph_quads;
pub mod lff;
pub mod sdf_atlas;
pub mod shape_files;
pub mod shx;
pub mod sysfont;
pub mod ttf_glyph;
pub mod web_font;
