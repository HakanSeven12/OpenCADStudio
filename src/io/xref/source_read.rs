//! Production byte readers for drawing sources; no path resolution or merging.
use super::{Arc, PathBuf, XrefRead};
use document::ifccad::IfccadExternalSourceFormat;

pub(super) fn inferred_format(bytes: &[u8]) -> IfccadExternalSourceFormat {
    if bytes.starts_with(b"AC10") {
        IfccadExternalSourceFormat::Dwg
    } else if bytes.starts_with(&[0x28, 0xb5, 0x2f, 0xfd])
        || bytes.iter().find(|byte| !byte.is_ascii_whitespace()) == Some(&b'{')
        || bytes.first().is_some_and(|byte| byte >> 5 == 5)
    {
        IfccadExternalSourceFormat::Ifccad
    } else {
        IfccadExternalSourceFormat::Dxf
    }
}

pub(super) fn read(
    name: &str,
    bytes: &[u8],
    format: IfccadExternalSourceFormat,
    progress: Option<Arc<dyn Fn(u16) + Send + Sync>>,
) -> XrefRead {
    let content = None;
    let outcome = match format {
        IfccadExternalSourceFormat::Dwg => {
            let mut reader = document::DwgReader::from_stream(std::io::Cursor::new(bytes.to_vec()));
            if let Some(progress) = progress {
                reader.set_progress_callback(progress);
            }
            reader.read_with_stats().map_err(|error| error.to_string())
        }
        IfccadExternalSourceFormat::Dxf => {
            document::DxfReader::from_reader(std::io::Cursor::new(bytes.to_vec()))
                .and_then(|reader| reader.read_with_stats())
                .map_err(|error| error.to_string())
        }
        IfccadExternalSourceFormat::Ifccad => {
            Err("Native IFCCAD XREF sources require the follow-up source reader".into())
        }
    }
    .and_then(|outcome| {
        if !outcome.stats.has_usable_drawing_data() {
            return Err("Source has no readable drawing records".into());
        }
        Ok(outcome)
    });
    XrefRead {
        resolved: Some(PathBuf::from(name)),
        outcome: Some(outcome),
        source_sha256: None,
        content,
    }
}
