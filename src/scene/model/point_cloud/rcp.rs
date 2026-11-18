// Scan project (.rcp) reader: a zip holding one XML whose
// `<VoxelTreeRunTime>` entries list the project's scans — each with its
// transform (Translation / Rotation / Scale), visibility and the .rcs path
// (`RelativePath` against the project's folder, `Path` absolute).

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

/// One visible scan of the project.
pub struct ScanEntry {
    pub path: PathBuf,
    /// The scan's identifier ("{…}").
    pub id: String,
    pub translation: [f64; 3],
    pub rotation: [f64; 3],
    pub scale: [f64; 3],
}

/// The project's preview picture (the JPEG it carries), for the attach
/// dialog.
pub fn preview(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    for i in 0..archive.len() {
        let file = archive.by_index(i).ok()?;
        if file.name().to_ascii_lowercase().ends_with(".jpg") {
            let mut picture = Vec::new();
            file.take(32 << 20).read_to_end(&mut picture).ok()?;
            return Some(picture);
        }
    }
    None
}

/// How deeply a project's XML may nest; real projects nest a few levels.
const MAX_XML_DEPTH: usize = 256;

pub fn scans(bytes: &[u8], project: &Path) -> Option<Vec<ScanEntry>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    let folder = project.parent().unwrap_or(Path::new(""));
    for i in 0..archive.len() {
        let file = archive.by_index(i).ok()?;
        if !file.name().to_ascii_lowercase().ends_with(".xml") {
            continue;
        }
        let mut text = String::new();
        // The project XML is small; the cap keeps a crafted archive in check.
        if file.take(64 << 20).read_to_string(&mut text).is_err() {
            continue;
        }
        let text = text.trim_start_matches('\u{feff}');
        if !crate::xml_depth::within(text, MAX_XML_DEPTH) {
            continue;
        }
        let Ok(doc) = roxmltree::Document::parse(text) else {
            continue;
        };
        let nodes: Vec<_> = doc
            .descendants()
            .filter(|n| n.has_tag_name("VoxelTreeRunTime"))
            .collect();
        if nodes.is_empty() {
            continue;
        }
        return Some(
            nodes
                .into_iter()
                .filter_map(|node| entry(node, folder))
                .collect(),
        );
    }
    None
}

fn child<'a, 'i>(node: roxmltree::Node<'a, 'i>, tag: &str) -> Option<roxmltree::Node<'a, 'i>> {
    node.children().find(|n| n.has_tag_name(tag))
}

fn value<'a>(node: roxmltree::Node<'a, '_>, tag: &str) -> Option<&'a str> {
    child(node, tag)?.attribute("Value")
}

fn xyz(node: roxmltree::Node, tag: &str, default: f64) -> [f64; 3] {
    let n = child(node, tag);
    ["x", "y", "z"].map(|a| {
        n.and_then(|n| n.attribute(a))
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| v.is_finite())
            .unwrap_or(default)
    })
}

fn entry(node: roxmltree::Node, folder: &Path) -> Option<ScanEntry> {
    if value(node, "Visible") == Some("0") {
        return None;
    }
    let relative = value(node, "RelativePath")
        .filter(|p| !p.is_empty())
        .map(|p| folder.join(p.replace('\\', "/")))
        .filter(|p| p.is_file());
    let path = relative.or_else(|| {
        value(node, "Path")
            .map(PathBuf::from)
            .filter(|p| p.is_file())
    })?;
    Some(ScanEntry {
        path,
        id: node.attribute("Id").unwrap_or_default().to_string(),
        translation: xyz(node, "Translation", 0.0),
        rotation: xyz(node, "Rotation", 0.0),
        scale: xyz(node, "Scale", 1.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A project archive holding `xml` as its project file.
    fn project(xml: &str) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("project.xml", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
        zip.finish().unwrap().into_inner()
    }

    /// A project whose one scan, a file that exists, sits inside `levels`
    /// extra elements.
    fn nested(levels: usize) -> Vec<u8> {
        let scan = format!(
            "<VoxelTreeRunTime Id=\"s1\"><Path Value=\"{}\"/>\
             <Translation x=\"1\" y=\"2\" z=\"3\"/></VoxelTreeRunTime>",
            concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")
        );
        project(&format!(
            "<Project>{}{scan}{}</Project>",
            "<a>".repeat(levels),
            "</a>".repeat(levels)
        ))
    }

    #[test]
    fn a_project_nested_too_deeply_is_not_read() {
        let ids = |bytes: &[u8]| {
            scans(bytes, Path::new("p.rcp")).map(|s| {
                s.iter()
                    .map(|e| (e.id.clone(), e.translation))
                    .collect::<Vec<_>>()
            })
        };
        // 256 levels at most: the project, the scan (its entries close
        // themselves) and 254 more.
        let read = Some(vec![("s1".to_string(), [1.0, 2.0, 3.0])]);
        assert_eq!(ids(&nested(0)), read);
        assert_eq!(ids(&nested(254)), read);
        assert_eq!(ids(&nested(255)), None);
        // 3,000 levels overflowed a test thread's stack inside the parser.
        assert_eq!(ids(&nested(100_000)), None);
    }
}
