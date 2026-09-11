//! DXF writer module

mod binary_writer;
mod section_writer;
mod stream_writer;
mod text_writer;

pub use binary_writer::DxfBinaryWriter;
pub use section_writer::SectionWriter;
pub use stream_writer::{value_type_for_code, DxfStreamWriter, DxfStreamWriterExt};
pub use text_writer::DxfTextWriter;

use crate::document::CadDocument;
use crate::entities::EntityType;
use crate::error::Result;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

/// DXF file writer
pub struct DxfWriter<'a> {
    document: &'a CadDocument,
    /// Whether to write binary DXF format
    pub binary: bool,
}

impl<'a> DxfWriter<'a> {
    /// Create a new DXF writer for ASCII output
    pub fn new(document: &'a CadDocument) -> Self {
        Self {
            document,
            binary: false,
        }
    }

    /// Create a new DXF writer for binary output
    pub fn new_binary(document: &'a CadDocument) -> Self {
        Self {
            document,
            binary: true,
        }
    }

    /// Set whether to write binary format
    pub fn set_binary(&mut self, binary: bool) {
        self.binary = binary;
    }

    /// Write to a file
    pub fn write_to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        self.write_to_writer(writer)
    }

    /// Write to any writer
    pub fn write_to_writer<W: Write>(&self, writer: W) -> Result<()> {
        if self.binary {
            let mut stream_writer = DxfBinaryWriter::new(writer)?;
            self.write_dxf(&mut stream_writer)?;
            stream_writer.flush()?;
        } else {
            let mut stream_writer = DxfTextWriter::new(writer);
            self.write_dxf(&mut stream_writer)?;
            stream_writer.flush()?;
        }
        Ok(())
    }

    /// Write to a byte vector (useful for testing)
    pub fn write_to_vec(&self) -> Result<Vec<u8>> {
        // Pre-allocate based on entity count: ~512 bytes per entity is a reasonable estimate
        let entity_count = self.document.entities().count();
        let estimated = (entity_count + 64) * 512;
        let mut buffer = Vec::with_capacity(estimated);
        self.write_to_writer(&mut buffer)?;
        Ok(buffer)
    }

    /// Write DXF content to a stream writer
    fn write_dxf<W: DxfStreamWriter>(&self, writer: &mut W) -> Result<()> {
        let prepared = crate::io::loft_parameters::prepared(self.document);
        self.write_prepared_dxf(writer, prepared.as_ref())
    }

    fn write_prepared_dxf<W: DxfStreamWriter>(
        &self,
        writer: &mut W,
        document: &CadDocument,
    ) -> Result<()> {
        let handle_start = compute_max_handle(document);
        let extra_handles = count_extra_handles(document, document.version);
        let handle_seed = handle_start + extra_handles + 1;
        let mut section_writer = SectionWriter::new(writer, handle_start, handle_seed);
        section_writer.set_version(document.version);
        section_writer.build_valid_handles(document);

        // Write all sections
        section_writer.write_header(document)?;
        section_writer.write_classes(document)?;
        section_writer.write_tables(document)?;
        section_writer.write_blocks(document)?;
        section_writer.write_entities(document)?;
        section_writer.write_objects(document)?;
        section_writer.write_acdsdata()?;

        // Write EOF
        writer.write_string(0, "EOF")?;

        Ok(())
    }

    /// Get a reference to the document
    pub fn document(&self) -> &CadDocument {
        &self.document
    }
}

fn count_extra_handles(document: &CadDocument, version: crate::types::DxfVersion) -> u64 {
    use crate::objects::ObjectType;

    let mut count = 0u64;

    for entity in document.entities() {
        match entity {
            EntityType::Polyline(polyline) => {
                // Vertices + SEQEND all use allocate_handle()
                count += polyline.vertices.len() as u64 + 1;
            }
            EntityType::Polyline2D(polyline) => {
                // Plain 2D polylines down-save as LWPOLYLINE for R2000+,
                // which emits no child records (see SectionWriter).
                if version >= crate::types::DxfVersion::AC1015
                    && section_writer::polyline2d_downsaves_to_lwpolyline(polyline)
                {
                    continue;
                }
                // Vertices + SEQEND all use allocate_handle()
                count += polyline.vertices.len() as u64 + 1;
            }
            EntityType::Polyline3D(polyline) => {
                for vertex in &polyline.vertices {
                    if vertex.handle.is_null() {
                        count += 1;
                    }
                }
                // SEQEND always written
                count += 1;
            }
            EntityType::PolyfaceMesh(mesh) => {
                for vertex in &mesh.vertices {
                    if vertex.common.handle.is_null() {
                        count += 1;
                    }
                }
                for face in &mesh.faces {
                    if face.common.handle.is_null() {
                        count += 1;
                    }
                }
                if mesh.seqend_handle.is_none() {
                    count += 1;
                }
            }
            EntityType::PolygonMesh(mesh) => {
                for vertex in &mesh.vertices {
                    if vertex.common.handle.is_null() {
                        count += 1;
                    }
                }
                // SEQEND always needs a handle
                count += 1;
            }
            EntityType::Insert(insert) => {
                // SEQEND for attribute sequence
                if insert.has_attributes() {
                    count += 1;
                }
            }
            _ => {}
        }
    }

    // Count SortEntitiesTable sort entries (allocate_handle used for each)
    for obj in document.objects.values() {
        if let ObjectType::SortEntitiesTable(table) = obj {
            count += table.len() as u64;
        }
    }

    // Null layer handles are allocated while writing the LAYER table.
    count += document
        .layers
        .iter()
        .filter(|layer| layer.handle.is_null())
        .count() as u64;

    count
}

/// Compute the true maximum handle across all document objects.
/// Returns max_handle + 1 (the first safe handle to allocate).
/// This is needed because DWG-loaded documents may have next_handle
/// below some object handles (DWG reader doesn't call resolve_references).
fn compute_max_handle(document: &CadDocument) -> u64 {
    let mut max = document.next_handle();

    for entity in document.entities() {
        let h = entity.common().handle.value();
        if h >= max {
            max = h + 1;
        }
    }
    for (handle, _) in &document.objects {
        let h = handle.value();
        if h >= max {
            max = h + 1;
        }
    }
    for br in document.block_records.iter() {
        let h = br.handle.value();
        if h >= max {
            max = h + 1;
        }
        for eh in &br.entity_handles {
            let h = eh.value();
            if h >= max {
                max = h + 1;
            }
        }
        // BLOCK/ENDBLK markers are excluded from document.entities(), so scan
        // their handles here explicitly to avoid re-issuing them.
        let h = br.block_entity_handle.value();
        if h >= max {
            max = h + 1;
        }
        let h = br.block_end_handle.value();
        if h >= max {
            max = h + 1;
        }
    }
    for r in document.layers.iter() {
        let h = r.handle.value();
        if h >= max {
            max = h + 1;
        }
    }
    for r in document.line_types.iter() {
        let h = r.handle.value();
        if h >= max {
            max = h + 1;
        }
    }
    for r in document.text_styles.iter() {
        let h = r.handle.value();
        if h >= max {
            max = h + 1;
        }
    }
    for r in document.dim_styles.iter() {
        let h = r.handle.value();
        if h >= max {
            max = h + 1;
        }
    }
    for r in document.app_ids.iter() {
        let h = r.handle.value();
        if h >= max {
            max = h + 1;
        }
    }
    for r in document.views.iter() {
        let h = r.handle.value();
        if h >= max {
            max = h + 1;
        }
    }
    for r in document.vports.iter() {
        let h = r.handle.value();
        if h >= max {
            max = h + 1;
        }
    }
    for r in document.ucss.iter() {
        let h = r.handle.value();
        if h >= max {
            max = h + 1;
        }
    }

    max
}

/// Convenience function to write a document to a file
pub fn write_dxf<P: AsRef<Path>>(document: &CadDocument, path: P) -> Result<()> {
    let writer = DxfWriter::new(document);
    writer.write_to_file(path)
}

/// Convenience function to write a document to a binary DXF file
pub fn write_binary_dxf<P: AsRef<Path>>(document: &CadDocument, path: P) -> Result<()> {
    let writer = DxfWriter::new_binary(document);
    writer.write_to_file(path)
}
