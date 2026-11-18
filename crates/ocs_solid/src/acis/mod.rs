//! Solidcore records read from ACIS documents and written into them.

mod append;
mod lift;

pub use append::{append, append_polyline_wire, Unappendable, Written};
pub use lift::{lift, lift_body, lift_wire, Lifted, Loss, WireSpan};
