//! Consolidated integration test runner for OpenCADStudio.
//!
//! Gathers and executes all integration test suites as submodules within a
//! single test binary.

#[path = "active_space_roundtrip.rs"]
mod active_space_roundtrip;

#[path = "annotative_context_roundtrip.rs"]
mod annotative_context_roundtrip;

#[path = "annotative_context_synthesis.rs"]
mod annotative_context_synthesis;

#[path = "block_hatch_export.rs"]
mod block_hatch_export;

#[path = "block_text_draw_depth.rs"]
mod block_text_draw_depth;

#[path = "dim_copy_check.rs"]
mod dim_copy_check;

#[path = "dim_leader_render_check.rs"]
mod dim_leader_render_check;

#[path = "dim_line_color_roundtrip.rs"]
mod dim_line_color_roundtrip;

#[path = "explode_regression.rs"]
mod explode_regression;

#[path = "flatten_regression.rs"]
mod flatten_regression;

#[path = "hatch_shader_lod.rs"]
mod hatch_shader_lod;

#[path = "mesh_shader_limits.rs"]
mod mesh_shader_limits;

#[path = "page_setup_root_repair.rs"]
mod page_setup_root_repair;

#[path = "parametric_constraints_solve.rs"]
mod parametric_constraints_solve;

#[path = "pdf_export_images_check.rs"]
mod pdf_export_images_check;

#[path = "pdf_export_text_check.rs"]
mod pdf_export_text_check;

#[path = "presspull_curved_boundary.rs"]
mod presspull_curved_boundary;

#[path = "section_plane_roundtrip.rs"]
mod section_plane_roundtrip;

#[path = "text_font_rendering.rs"]
mod text_font_rendering;

#[path = "text_shader_minification.rs"]
mod text_shader_minification;

#[path = "ucs_face_plane.rs"]
mod ucs_face_plane;

#[path = "viewport_wide_polyline_plot_width.rs"]
mod viewport_wide_polyline_plot_width;

#[path = "xclip_plot_test.rs"]
mod xclip_plot_test;

#[path = "zoom_extents_coverage.rs"]
mod zoom_extents_coverage;

#[path = "zoom_extents_xline.rs"]
mod zoom_extents_xline;
