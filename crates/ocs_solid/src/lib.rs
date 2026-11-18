//! OpenCADStudio on Solidcore: B-rep records read from and written to ACIS, and what the app
//! asks of Solidcore beside its modelling calls.

pub mod acis;
pub mod display;
pub mod geometry;
pub mod history;
pub mod model;
pub mod surfacing;
pub mod sweep;

pub use acis::{append, append_polyline_wire, lift, lift_body, lift_wire, Lifted, Loss, Unappendable, WireSpan, Written};
pub use history::{
    extrude_region, loft_path_geometry, loft_section_geometry, loft_sections, rebuild_body, rebuild_extrusion_with_mode, rebuild_history,
    rebuild_history_tree, rebuild_loft_with_options, rebuild_sweep_with_mode, revolve, sweep_default_base, sweep_history_path_has_corner,
    sweep_history_path_length, sweep_history_placements, sweep_history_reference_point, sweep_history_refusal, sweep_profile_geometry,
    sweep_spatial_profile, HistoryRebuildError, LoftSection,
};
