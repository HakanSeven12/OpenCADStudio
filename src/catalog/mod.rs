//! The libraries a drawing draws from, as this application ships and reads
//! them: linetypes, hatch patterns, plot style tables, paper sizes, visual
//! styles and coordinate systems. Reading and writing drawing files is `io`;
//! drawing them is `scene`.

pub mod crs;
pub mod linetypes;
pub mod paper_catalog;
pub mod patterns;
pub mod plot_style;
pub mod visual_styles;
