//! Create one disposable DWG with a paper layout and rectangular viewport.
//! Usage: cargo run --example create_layout_fixture -- /absolute/output.dwg

use std::io::Write;

use ocs_plugin_api::host::codec::{
    entities::{Line, Text, Viewport},
    CadDocument, DwgWriter, EntityType, Vector3,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args().nth(1).ok_or("expected output DWG path")?;
    let output = std::path::PathBuf::from(output);
    let twisted = std::env::args().nth(2).as_deref() == Some("--twisted");
    if output.extension().is_none_or(|value| value != "dwg")
        || output.parent().is_none_or(|parent| !parent.is_dir())
    {
        return Err("output must be a .dwg in an existing directory".into());
    }
    let mut document = CadDocument::new();
    document.add_layout("Sheet")?;
    document.add_entity(EntityType::Line(Line::from_coords(
        -50.0, 0.0, 0.0, 50.0, 0.0, 0.0,
    )))?;
    document.add_entity_to_layout(
        EntityType::Text(Text::with_value(
            "Sheet note",
            Vector3::new(20.0, 20.0, 0.0),
        )),
        "Sheet",
    )?;
    let mut viewport = Viewport::new();
    viewport.id = 2;
    viewport.center = Vector3::new(100.0, 80.0, 0.0);
    viewport.width = 100.0;
    viewport.height = 50.0;
    viewport.view_height = 100.0;
    viewport.view_center = Vector3::new(10.0, 20.0, 0.0);
    viewport.view_target = Vector3::ZERO;
    if twisted {
        viewport.twist_angle = std::f64::consts::FRAC_PI_2;
        viewport.view_target = Vector3::new(5.0, 10.0, 0.0);
    }
    document.add_entity_to_layout(EntityType::Viewport(viewport), "Sheet")?;

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)?;
    DwgWriter::write_to_writer(&mut file, &document)?;
    file.flush()?;
    println!("Created {}", output.display());
    Ok(())
}
