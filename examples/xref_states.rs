// Dump every xref block_record's raw is_loaded bit before any resolve.
// cargo run --release --example xref_states -- <dwg>
use acadrust::io::dwg::DwgReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).unwrap();
    let doc = DwgReader::from_file(&path)?.read()?;

    for br in doc.block_records.iter() {
        if br.flags.is_xref || br.flags.is_xref_overlay {
            println!(
                "name={:<30} is_xref={} overlay={} is_loaded={:?} xref_path={:?} entity_handles={}",
                br.name,
                br.flags.is_xref,
                br.flags.is_xref_overlay,
                br.is_loaded,
                br.xref_path,
                br.entity_handles.len(),
            );
        }
    }
    Ok(())
}
