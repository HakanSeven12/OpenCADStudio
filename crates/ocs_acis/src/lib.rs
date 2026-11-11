//! Conversion between ACIS records and kernel B-rep topology.
//!
//! Provenance preserves untouched source records during lowering.

mod append;
mod history;
mod lift;
mod lower;

pub use append::{append, append_polyline_wire, Unappendable, Written};
pub use history::{rebuild_body, rebuild_extrusion_with_mode, rebuild_history, rebuild_history_tree, rebuild_sweep_with_mode, sweep_history_path_length, sweep_history_refusal, sweep_spatial_profile, sweep_history_path_has_corner, sweep_history_placements, sweep_history_reference_point, sweep_profile_geometry, sweep_default_base, HistoryRebuildError};
pub use history::{loft_section_geometry, loft_path_geometry, rebuild_loft_with_options};
pub use lift::{lift, lift_body, lift_wire, offset_surface, Loss, WireSpan};
pub use lower::{lower, pending, Unwritable};
