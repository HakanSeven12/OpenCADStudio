//! Print the command catalog's current coverage and fallback inventory.
//!
//! Run with: `cargo run --example command_metadata_report`

fn main() {
    print!(
        "{}",
        OpenCADStudio::command::catalog::coverage_report().render_text()
    );
}
