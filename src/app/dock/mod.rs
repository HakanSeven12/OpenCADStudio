//! The app side of docking: pallets docked on the drawing view's edges (one
//! group shown per edge, listed in an icon strip) or floating over it.
//!
//! The persisted layout and its pure geometry live in [`crate::ui::dock`];
//! this module holds the transient state handling (`update`) and drawing
//! (`view`).

mod update;
mod view;

#[cfg(test)]
mod tests;
