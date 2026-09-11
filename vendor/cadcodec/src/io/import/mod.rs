//! Import external 3D file formats into [`CadDocument`].
//!
//! This module provides importers that convert common 3D interchange formats
//! into acadrust entities.  Tessellated formats produce [`Mesh`] entities.
//!
//! # Quick start
//!
//! ```rust,ignore
//! use acadrust::io::import::{import_file, ImportConfig};
//!
//! let doc = import_file("model.stl", &ImportConfig::default())?;
//! println!("Imported {} entities", doc.entities().count());
//! ```
//!
//! # Supported formats
//!
//! | Extension | Module | Entity type |
//! |-----------|--------|-------------|
//! | `.stl`    | [`stl`] | [`Mesh`] |
//! | `.dae`    | [`collada`] | [`Mesh`] (one per geometry, layer-per-material) |
//! | `.obj`    | [`obj`] | [`Mesh`] (with MTL material support) |
//! | `.gltf` / `.glb` | [`gltf`] | [`Mesh`] (glTF 2.0 with PBR colors) |
//! | `.fbx`    | [`fbx`] | [`Mesh`] (binary and ASCII FBX) |

pub mod collada;
pub mod color_mapping;
pub mod fbx;
pub mod gltf;
pub mod obj;
pub mod stl;

use std::path::Path;

use crate::document::CadDocument;
use crate::error::{DxfError, Result};
use crate::types::Color;

// Re-export importers
pub use collada::ColladaImporter;
pub use fbx::FbxImporter;
pub use gltf::GltfImporter;
pub use obj::ObjImporter;
pub use stl::StlImporter;

/// Detected import format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    /// STL (Stereolithography) — ASCII or binary
    Stl,
    /// COLLADA (.dae) — XML-based 3D interchange
    Collada,
    /// Wavefront OBJ
    Obj,
    /// glTF 2.0 (.gltf JSON + .bin)
    Gltf,
    /// glTF Binary (.glb)
    Glb,
    /// Autodesk FBX (binary or ASCII)
    Fbx,
}

/// Configuration for file import.
#[derive(Debug, Clone)]
pub struct ImportConfig {
    /// Prefix prepended to generated layer names (default: `"imported"`).
    pub layer_prefix: String,
    /// Default color for entities when no material color is available.
    pub default_color: Color,
    /// Merge duplicate vertices within `merge_tolerance` distance (default: `true`).
    pub merge_vertices: bool,
    /// Distance tolerance for vertex merging (default: `1e-9`).
    pub merge_tolerance: f64,
    /// Uniform scale factor applied to all coordinates (default: `1.0`).
    pub scale_factor: f64,
}

impl Default for ImportConfig {
    fn default() -> Self {
        Self {
            layer_prefix: "imported".to_string(),
            default_color: Color::ByLayer,
            merge_vertices: true,
            merge_tolerance: 1e-9,
            scale_factor: 1.0,
        }
    }
}

/// Detect the import format from a file path's extension.
pub fn detect_format(path: &Path) -> Result<ImportFormat> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "stl" => Ok(ImportFormat::Stl),
        "dae" => Ok(ImportFormat::Collada),
        "obj" => Ok(ImportFormat::Obj),
        "gltf" => Ok(ImportFormat::Gltf),
        "glb" => Ok(ImportFormat::Glb),
        "fbx" => Ok(ImportFormat::Fbx),
        _ => Err(DxfError::ImportError(format!(
            "Unsupported import format: .{}",
            ext
        ))),
    }
}

/// Import a file into a [`CadDocument`], auto-detecting the format from the
/// file extension.
///
/// This is a convenience wrapper around the format-specific importers.
pub fn import_file(path: impl AsRef<Path>, config: &ImportConfig) -> Result<CadDocument> {
    let path = path.as_ref();
    let format = detect_format(path)?;

    match format {
        ImportFormat::Stl => StlImporter::from_file(path)?
            .with_config(config.clone())
            .import(),
        ImportFormat::Collada => ColladaImporter::from_file(path)?
            .with_config(config.clone())
            .import(),
        ImportFormat::Obj => ObjImporter::from_file(path)?
            .with_config(config.clone())
            .import(),
        ImportFormat::Gltf | ImportFormat::Glb => GltfImporter::from_file(path)?
            .with_config(config.clone())
            .import(),
        ImportFormat::Fbx => FbxImporter::from_file(path)?
            .with_config(config.clone())
            .import(),
    }
}
