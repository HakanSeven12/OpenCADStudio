//! Shared ETRANSMIT dependency discovery and package bookkeeping.
//!
//! The drawing remains authoritative: references are not renamed or rebound.
//! A same-named dependency is therefore reused only when its bytes are exactly
//! equal; otherwise it is reported as a conflict instead of silently replacing
//! another file in the flat package.

use crate::io::transmit_zip::entry_name;
use document::{Document, EntityType};
use std::collections::HashMap;

pub(crate) const LINETYPES_FILE: &str = "opencadstudio.lin";
pub(crate) const REPORT_FILE: &str = "ETRANSMIT.txt";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DependencyKind {
    Xref,
    Image,
    Underlay,
    PointCloud,
    Font,
    BigFont,
    Shape,
    PlotStyle,
}

impl DependencyKind {
    fn label(self) -> &'static str {
        match self {
            Self::Xref => "external reference",
            Self::Image => "raster image",
            Self::Underlay => "underlay",
            Self::PointCloud => "point cloud",
            Self::Font => "font",
            Self::BigFont => "big font",
            Self::Shape => "shape file",
            Self::PlotStyle => "plot style table",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Dependency {
    pub kind: DependencyKind,
    pub reference: String,
    pub owner: String,
    pub bold: bool,
    pub italic: bool,
}

impl Dependency {
    fn new(kind: DependencyKind, reference: &str, owner: &str) -> Self {
        Self {
            kind,
            reference: reference.trim().to_string(),
            owner: owner.to_string(),
            bold: false,
            italic: false,
        }
    }

    fn font(reference: &str, owner: &str, bold: bool, italic: bool) -> Self {
        let mut dependency = Self::new(DependencyKind::Font, reference, owner);
        dependency.bold = bold;
        dependency.italic = italic;
        dependency
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DependencyIssue {
    pub kind: DependencyKind,
    pub reference: String,
    pub owner: String,
    pub reason: String,
}

#[derive(Debug)]
pub(crate) struct TransmittalPackage {
    pub entries: Vec<(String, Vec<u8>)>,
    pub issues: Vec<DependencyIssue>,
    pub warnings: Vec<String>,
    drawing_name: String,
    claimed: HashMap<String, String>,
    packaged: Vec<(String, String, String)>,
}

impl TransmittalPackage {
    pub fn new(drawing_name: &str) -> Result<Self, String> {
        let drawing_name = entry_name(drawing_name)?;
        let mut claimed = HashMap::new();
        claimed.insert(drawing_name.to_ascii_lowercase(), "the drawing".into());
        claimed.insert(
            REPORT_FILE.to_ascii_lowercase(),
            "the transmittal report".into(),
        );
        claimed.insert(
            LINETYPES_FILE.to_ascii_lowercase(),
            "the generated linetype file".into(),
        );
        Ok(Self {
            entries: Vec::new(),
            issues: Vec::new(),
            warnings: Vec::new(),
            packaged: vec![(
                drawing_name.clone(),
                "drawing".into(),
                "the transmittal root".into(),
            )],
            drawing_name,
            claimed,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_drawing(&mut self, bytes: Vec<u8>) {
        self.entries.insert(0, (self.drawing_name.clone(), bytes));
    }

    pub fn add(&mut self, dependency: &Dependency, bytes: Vec<u8>) {
        let name = match entry_name(&dependency.reference) {
            Ok(name) => name,
            Err(reason) => {
                self.fail(dependency, reason);
                return;
            }
        };
        self.add_named(dependency, name, bytes);
    }

    fn add_named(&mut self, dependency: &Dependency, name: String, bytes: Vec<u8>) {
        let key = name.to_ascii_lowercase();
        if let Some(previous) = self.claimed.get(&key) {
            let identical = self
                .entries
                .iter()
                .find(|(entry, _)| entry.eq_ignore_ascii_case(&name))
                .is_some_and(|(_, existing)| *existing == bytes)
                && previous != "the drawing";
            if identical {
                return;
            }
            self.fail(
                dependency,
                format!("package name `{name}` conflicts with {previous}; references were not rewritten"),
            );
            return;
        }
        self.claimed.insert(
            key,
            format!("{} `{}`", dependency.kind.label(), dependency.reference),
        );
        self.packaged.push((
            name.clone(),
            dependency.kind.label().into(),
            format!("{} (from {})", dependency.reference, dependency.owner),
        ));
        self.entries.push((name, bytes));
    }

    pub fn fail(&mut self, dependency: &Dependency, reason: impl Into<String>) {
        let issue = DependencyIssue {
            kind: dependency.kind,
            reference: dependency.reference.clone(),
            owner: dependency.owner.clone(),
            reason: reason.into(),
        };
        if !self.issues.contains(&issue) {
            self.issues.push(issue);
        }
    }

    pub fn add_linetypes(&mut self, output: crate::io::linetypes::writer::LinOutput) {
        for (name, reason) in output.skipped {
            if reason != crate::io::linetypes::writer::SkipReason::NoPattern {
                self.warnings.push(format!(
                    "Linetype `{name}` was not written to {LINETYPES_FILE}: {}.",
                    reason.label()
                ));
            }
        }
        for (name, warning) in output.warnings {
            self.warnings.push(format!(
                "Linetype `{name}` in {LINETYPES_FILE}: {}.",
                warning.label()
            ));
        }
        if output.written.is_empty() {
            return;
        }
        let text = format!(
            ";; Generated by OpenCADStudio ETRANSMIT from the drawing's linetype table.\n{}",
            output.text
        );
        self.entries
            .push((LINETYPES_FILE.into(), text.into_bytes()));
        self.packaged.push((
            LINETYPES_FILE.into(),
            "generated linetype definitions".into(),
            self.drawing_name.clone(),
        ));
    }

    pub fn finish(mut self) -> Self {
        let mut report = String::from(
            "OpenCADStudio transmittal report\n\
             ================================\n\n\
             References retain their stored names. Install packaged fonts, shape files,\n\
             and plot style tables in the receiving program's support folders if it\n\
             does not discover them beside the drawing automatically.\n\n",
        );
        report.push_str("Packaged files\n--------------\n");
        for (name, kind, source) in &self.packaged {
            report.push_str(&format!("{name}\n  Type: {kind}\n  Source: {source}\n"));
        }
        report.push_str(
            "\nMissing or conflicting dependencies\n-----------------------------------\n",
        );
        if self.issues.is_empty() {
            report.push_str("None.\n");
        } else {
            for issue in &self.issues {
                report.push_str(&format!(
                    "{} `{}` (from {}): {}\n",
                    issue.kind.label(),
                    issue.reference,
                    issue.owner,
                    issue.reason
                ));
            }
        }
        report.push_str("\nWarnings\n--------\n");
        if self.warnings.is_empty() {
            report.push_str("None.\n");
        } else {
            for warning in &self.warnings {
                report.push_str(warning);
                report.push('\n');
            }
        }
        self.entries.push((REPORT_FILE.into(), report.into_bytes()));
        self
    }
}

fn owner_name(doc: &Document) -> String {
    doc.source_path
        .as_deref()
        .and_then(|path| path.rsplit(['/', '\\']).next())
        .filter(|name| !name.is_empty())
        .unwrap_or("the drawing")
        .to_string()
}

/// Direct external resources of one drawing. Recursive collectors call this
/// again for every xref with that xref as the owner.
pub(crate) fn direct_dependencies(
    doc: &Document,
    active_plot_style: Option<&str>,
) -> Vec<Dependency> {
    let owner = owner_name(doc);
    let mut dependencies = Vec::new();
    let mut push = |dependency: Dependency| {
        if !dependency.reference.is_empty()
            && !dependencies.iter().any(|existing: &Dependency| {
                existing.kind == dependency.kind
                    && existing
                        .reference
                        .eq_ignore_ascii_case(&dependency.reference)
                    && existing.bold == dependency.bold
                    && existing.italic == dependency.italic
            })
        {
            dependencies.push(dependency);
        }
    };

    for record in doc.block_records.iter() {
        push(Dependency::new(
            DependencyKind::Xref,
            &record.xref_path,
            &owner,
        ));
    }
    for object in doc.objects.values() {
        match object {
            document::objects::ObjectType::ImageDefinition(definition) => {
                push(Dependency::new(
                    DependencyKind::Image,
                    &definition.file_name,
                    &owner,
                ));
            }
            document::objects::ObjectType::UnderlayDefinition(definition) => {
                push(Dependency::new(
                    DependencyKind::Underlay,
                    &definition.file_path,
                    &owner,
                ));
            }
            document::objects::ObjectType::ClassObject(object) => match &object.data {
                document::objects::ClassObjectData::PointCloudDefinitionEx(definition)
                | document::objects::ClassObjectData::PointCloudDefinition(definition) => {
                    push(Dependency::new(
                        DependencyKind::PointCloud,
                        &definition.source_filename,
                        &owner,
                    ));
                }
                _ => {}
            },
            _ => {}
        }
    }
    // Some imported/legacy documents have only the entity's cached image path.
    for entity in doc.entities() {
        if let EntityType::RasterImage(image) = entity {
            push(Dependency::new(
                DependencyKind::Image,
                &image.file_path,
                &owner,
            ));
        }
    }

    for style in doc.text_styles.iter() {
        let bold = style.true_type_font_flags & crate::entities::text_support::TTF_FLAG_BOLD != 0;
        let italic =
            style.true_type_font_flags & crate::entities::text_support::TTF_FLAG_ITALIC != 0;
        let primary_kind = if style.is_shape_file {
            DependencyKind::Shape
        } else {
            DependencyKind::Font
        };
        let mut primary = Dependency::new(primary_kind, &style.font_file, &owner);
        primary.bold = bold;
        primary.italic = italic;
        push(primary);
        if !style.true_type_font.trim().is_empty() {
            push(Dependency::font(
                &style.true_type_font,
                &owner,
                bold,
                italic,
            ));
        }
        push(Dependency::new(
            DependencyKind::BigFont,
            &style.big_font_file,
            &owner,
        ));
    }
    for entity in doc.entities() {
        scan_inline_fonts(entity, &owner, &mut push);
    }

    for object in doc.objects.values() {
        if let document::objects::ObjectType::Layout(layout) = object {
            push(Dependency::new(
                DependencyKind::PlotStyle,
                &layout.plot_style_sheet,
                &owner,
            ));
        }
    }
    if let Some(name) = active_plot_style {
        push(Dependency::new(DependencyKind::PlotStyle, name, &owner));
    }
    dependencies
}

fn scan_inline_fonts(entity: &EntityType, owner: &str, push: &mut impl FnMut(Dependency)) {
    let mut scan = |text: &str| scan_formatted_text(text, owner, push);
    match entity {
        EntityType::MText(text) => scan(&text.value),
        EntityType::Dimension(dimension) => {
            scan(&dimension.base().text);
            if let Some(text) = &dimension.base().user_text {
                scan(text);
            }
        }
        EntityType::AttributeDefinition(attribute) => scan(&attribute.default_value),
        EntityType::AttributeEntity(attribute) => scan(&attribute.value),
        EntityType::Tolerance(tolerance) => scan(&tolerance.text),
        EntityType::MultiLeader(leader) => {
            scan(&leader.context.text_string);
            for attribute in &leader.block_attributes {
                scan(&attribute.text);
            }
        }
        EntityType::Table(table) => {
            for row in &table.rows {
                for cell in &row.cells {
                    for content in &cell.contents {
                        scan(&content.value.text);
                        scan(&content.value.formatted_value);
                        for attribute in &content.attributes {
                            scan(&attribute.value);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn scan_formatted_text(text: &str, owner: &str, push: &mut impl FnMut(Dependency)) {
    if !(text.contains("\\f") || text.contains("\\F")) {
        return;
    }
    let parsed = document::entities::mtext_format::parse_mtext(text, false);
    for paragraph in parsed.paragraphs {
        for span in paragraph.spans {
            if let Some(font) = span.properties.font {
                push(Dependency::font(&font.name, owner, font.bold, font.italic));
            }
        }
    }
}

fn built_in_resource(dependency: &Dependency) -> bool {
    match dependency.kind {
        DependencyKind::Font | DependencyKind::BigFont | DependencyKind::Shape => {
            crate::scene::text::lff::is_builtin(&dependency.reference)
                || crate::io::linetypes::is_bundled_shape_file(&dependency.reference)
        }
        _ => false,
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    pub fn collect(
        doc: &Document,
        source: &Path,
        drawing_name: &str,
        active_plot_style: Option<&str>,
    ) -> Result<TransmittalPackage, String> {
        let drawing = std::fs::read(source)
            .map_err(|error| format!("cannot read {}: {error}", source.display()))?;
        let mut package = TransmittalPackage::new(drawing_name)?;
        package.set_drawing(drawing);
        let mut visited = HashSet::new();
        if let Some(identity) = identity(source) {
            visited.insert(identity);
        }
        visit(doc, source, active_plot_style, &mut package, &mut visited);
        let output = crate::io::linetypes::writer::write_linetypes_at(
            doc,
            doc.line_types.iter(),
            source.parent(),
        );
        package.add_linetypes(output);
        Ok(package.finish())
    }

    fn visit(
        doc: &Document,
        source: &Path,
        active_plot_style: Option<&str>,
        package: &mut TransmittalPackage,
        visited: &mut HashSet<PathBuf>,
    ) {
        let base = source.parent().unwrap_or_else(|| Path::new("."));
        for dependency in direct_dependencies(doc, active_plot_style) {
            if built_in_resource(&dependency) {
                continue;
            }
            if dependency.kind == DependencyKind::PlotStyle {
                if let Some(bytes) = crate::io::plot_style::session_table(&dependency.reference) {
                    package.add(&dependency, (*bytes).clone());
                    continue;
                }
                use crate::io::plot_style::PlotStyleTableExt as _;
                if crate::io::plot_style::PlotStyleTable::builtin(&dependency.reference).is_ok() {
                    continue;
                }
            }
            let resolved = resolve(&dependency, base);
            let Some(path) = resolved else {
                if dependency.kind == DependencyKind::Font {
                    if let Some((name, bytes)) = crate::scene::text::sysfont::transmittal_face(
                        &dependency.reference,
                        dependency.bold,
                        dependency.italic,
                    ) {
                        package.add_named(&dependency, name, bytes);
                        continue;
                    }
                }
                package.fail(&dependency, "file not found");
                continue;
            };
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    package.fail(
                        &dependency,
                        format!("cannot read {}: {error}", path.display()),
                    );
                    continue;
                }
            };
            if dependency.kind == DependencyKind::Xref {
                let Some(identity) = identity(&path) else {
                    continue;
                };
                if !visited.insert(identity) {
                    continue;
                }
            }
            package.add(&dependency, bytes.clone());
            if dependency.kind != DependencyKind::Xref {
                continue;
            }
            match crate::io::load_bytes(&path.to_string_lossy(), bytes) {
                Ok(mut nested) => {
                    nested.source_path = Some(path.to_string_lossy().into_owned());
                    visit(&nested, &path, None, package, visited);
                }
                Err(error) => package.fail(
                    &dependency,
                    format!("the xref was packaged, but its nested dependencies could not be inspected: {error}"),
                ),
            }
        }
    }

    fn identity(path: &Path) -> Option<PathBuf> {
        std::fs::canonicalize(path).ok().or_else(|| {
            if path.is_absolute() {
                Some(path.to_path_buf())
            } else {
                std::env::current_dir().ok().map(|base| base.join(path))
            }
        })
    }

    fn resolve(dependency: &Dependency, base: &Path) -> Option<PathBuf> {
        let raw = dependency.reference.trim();
        match dependency.kind {
            DependencyKind::Xref => {
                crate::io::xref::resolve_path(raw, base).filter(|p| p.is_file())
            }
            DependencyKind::Image | DependencyKind::Underlay | DependencyKind::PointCloud => {
                crate::io::resolve_image_file(raw, Some(base)).map(PathBuf::from)
            }
            DependencyKind::BigFont | DependencyKind::Shape => {
                crate::io::font_repo::font_file_path(raw, Some(base)).map(PathBuf::from)
            }
            DependencyKind::Font => {
                crate::io::font_repo::font_file_path(raw, Some(base)).map(PathBuf::from)
            }
            DependencyKind::PlotStyle => plot_style_path(raw),
        }
    }

    fn plot_style_path(name: &str) -> Option<PathBuf> {
        let folder = crate::io::plot_style::plot_styles_dir().ok()?;
        let direct = folder.join(name);
        if direct.is_file() {
            return Some(direct);
        }
        std::fs::read_dir(folder)
            .ok()?
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(name)
            })
            .map(|entry| entry.path())
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::collect as collect_native;

#[cfg(target_arch = "wasm32")]
mod web {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Arc;

    pub fn collect(
        doc: &Document,
        drawing_name: &str,
        active_plot_style: Option<&str>,
    ) -> Result<TransmittalPackage, String> {
        let mut package = TransmittalPackage::new(drawing_name)?;
        let mut visited = HashSet::new();
        visited.insert(crate::io::xref::file_name_key(drawing_name));
        visit(doc, active_plot_style, &mut package, &mut visited);
        package.add_linetypes(crate::io::linetypes::writer::write_linetypes_at(
            doc,
            doc.line_types.iter(),
            None,
        ));
        Ok(package.finish())
    }

    fn visit(
        doc: &Document,
        active_plot_style: Option<&str>,
        package: &mut TransmittalPackage,
        visited: &mut HashSet<String>,
    ) {
        for dependency in direct_dependencies(doc, active_plot_style) {
            if built_in_resource(&dependency) {
                continue;
            }
            let mut registry_key = None;
            let bytes = match dependency.kind {
                DependencyKind::Xref => {
                    registry_key = crate::io::xref::find_registered_source(&dependency.reference);
                    registry_key
                        .as_deref()
                        .and_then(crate::io::xref::registered_bytes)
                }
                DependencyKind::Image => {
                    crate::scene::model::image_model::registered_source_bytes(&dependency.reference)
                }
                DependencyKind::Underlay => {
                    crate::scene::model::pdf_raster::registered_source_bytes(&dependency.reference)
                }
                DependencyKind::Font => true_type_bytes(&dependency).map(Arc::new),
                DependencyKind::PlotStyle => {
                    match crate::io::plot_style::session_table(&dependency.reference) {
                        Some(bytes) => Some(bytes),
                        None => {
                            use crate::io::plot_style::PlotStyleTableExt as _;
                            if crate::io::plot_style::PlotStyleTable::builtin(&dependency.reference)
                                .is_ok()
                            {
                                continue;
                            }
                            None
                        }
                    }
                }
                DependencyKind::BigFont | DependencyKind::Shape | DependencyKind::PointCloud => {
                    None
                }
            };
            let Some(bytes) = bytes else {
                package.fail(
                    &dependency,
                    "original bytes are not loaded in this browser session",
                );
                continue;
            };
            if dependency.kind == DependencyKind::Xref {
                let Some(key) = registry_key.as_ref() else {
                    continue;
                };
                if !visited.insert(crate::io::xref::file_name_key(key)) {
                    continue;
                }
            }
            package.add(&dependency, (*bytes).clone());
            if dependency.kind != DependencyKind::Xref {
                continue;
            }
            let Some(key) = registry_key else { continue };
            match crate::io::xref::registered_document(&key) {
                Ok(mut nested) => {
                    nested.source_path = Some(key.clone());
                    visit(&nested, None, package, visited);
                }
                Err(error) => package.fail(
                    &dependency,
                    format!("the xref was packaged, but its nested dependencies could not be inspected: {error}"),
                ),
            }
        }
    }

    fn true_type_bytes(dependency: &Dependency) -> Option<Vec<u8>> {
        if dependency.reference.to_ascii_lowercase().ends_with(".shx") {
            return None;
        }
        crate::scene::text::sysfont::transmittal_face(
            &dependency.reference,
            dependency.bold,
            dependency.italic,
        )
        .map(|(_, bytes)| bytes)
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) use web::collect as collect_web;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_inventory_covers_resources_and_inline_fonts() {
        let mut doc = Document::new();
        let mut record = document::tables::BlockRecord::new("SITE");
        record.xref_path = "xrefs/site.dwg".into();
        doc.block_records.add(record).unwrap();
        let mut style = document::tables::TextStyle::new("Notes");
        style.font_file = "fonts/romans-custom.shx".into();
        style.big_font_file = "bigfont.shx".into();
        style.true_type_font = "Arial".into();
        doc.text_styles.add(style).unwrap();
        let mut shape = document::tables::TextStyle::new("Shapes");
        shape.is_shape_file = true;
        shape.font_file = "shapes/symbols.shx".into();
        doc.text_styles.add(shape).unwrap();
        let mut text = document::entities::MText::new();
        text.value = "{\\fArial|b1|i1;Formatted}".into();
        doc.add_entity(EntityType::MText(Box::new(text))).unwrap();

        let dependencies = direct_dependencies(&doc, Some("Office.ctb"));
        assert!(dependencies.iter().any(|d| d.kind == DependencyKind::Xref));
        assert!(dependencies
            .iter()
            .any(|d| d.kind == DependencyKind::BigFont));
        assert!(dependencies.iter().any(|d| d.kind == DependencyKind::Shape));
        assert!(dependencies
            .iter()
            .any(|d| d.kind == DependencyKind::PlotStyle));
        assert!(dependencies
            .iter()
            .any(|d| d.kind == DependencyKind::Font && d.bold && d.italic));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_package_contains_recursive_resources_and_generated_linetypes() {
        use document::objects::{
            ClassObject, ClassObjectData, ImageDefinition, ObjectType, PointCloudDefinition,
            UnderlayDefinition,
        };
        use document::tables::linetype::{
            LineTypeComplexContent, LineTypeComplexData, LineTypeElement,
        };
        use document::tables::{BlockRecord, LineType, TextStyle};
        use std::path::{Path, PathBuf};
        use std::sync::Arc;

        let dir = std::env::temp_dir().join(format!(
            "ocs-transmit-native-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("nested-assets")).unwrap();
        let write = |path: PathBuf, bytes: &[u8]| std::fs::write(path, bytes).unwrap();
        write(dir.join("root.png"), b"root image");
        write(dir.join("root.pdf"), b"root pdf");
        write(dir.join("root.dgn"), b"root dgn");
        write(dir.join("scan.rcs"), b"synthetic point cloud");
        write(dir.join("custom.ttf"), b"synthetic TrueType payload");
        write(dir.join("root-big.shx"), b"synthetic big-font payload");
        write(dir.join("symbols.shx"), b"synthetic shape payload");
        write(dir.join("nested-assets/nested.shx"), b"nested font");
        write(dir.join("inner-big.shx"), b"inner big font");

        let mut inner = Document::new();
        let mut inner_style = TextStyle::new("Inner");
        inner_style.big_font_file = "inner-big.shx".into();
        inner.text_styles.add(inner_style).unwrap();
        write(
            dir.join("inner.dxf"),
            &crate::io::save_to_bytes(&inner, "dxf", inner.version).unwrap(),
        );

        let mut nested = Document::new();
        let mut nested_style = TextStyle::new("Nested");
        nested_style.font_file = "nested-assets/nested.shx".into();
        nested.text_styles.add(nested_style).unwrap();
        let mut inner_ref = BlockRecord::new("INNER");
        inner_ref.flags.is_xref = true;
        inner_ref.xref_path = "inner.dxf".into();
        nested.block_records.add(inner_ref).unwrap();
        write(
            dir.join("nested.dwg"),
            &crate::io::save_to_bytes(&nested, "dwg", nested.version).unwrap(),
        );

        let mut root = Document::new();
        root.source_path = Some(dir.join("root.dwg").to_string_lossy().into_owned());
        let mut nested_ref = BlockRecord::new("NESTED");
        nested_ref.flags.is_xref = true;
        nested_ref.xref_path = "nested.dwg".into();
        root.block_records.add(nested_ref).unwrap();
        let image_handle = root.allocate_handle();
        let mut image = ImageDefinition::with_dimensions("root.png", 8, 8);
        image.handle = image_handle;
        root.objects
            .insert(image_handle, ObjectType::ImageDefinition(image));
        for definition in [
            UnderlayDefinition::pdf("root.pdf", "1"),
            UnderlayDefinition::dgn("root.dgn", "Model"),
        ] {
            let handle = root.allocate_handle();
            root.objects
                .insert(handle, ObjectType::UnderlayDefinition(definition));
        }
        let cloud_handle = root.allocate_handle();
        root.objects.insert(
            cloud_handle,
            ObjectType::ClassObject(ClassObject::new(ClassObjectData::PointCloudDefinitionEx(
                PointCloudDefinition {
                    source_filename: "scan.rcs".into(),
                    ..Default::default()
                },
            ))),
        );
        let mut text_style = TextStyle::new("RootText");
        text_style.font_file = "custom.ttf".into();
        text_style.big_font_file = "root-big.shx".into();
        root.text_styles.add(text_style).unwrap();
        let mut custom_shape = TextStyle::new("CustomShapes");
        custom_shape.is_shape_file = true;
        custom_shape.font_file = "symbols.shx".into();
        root.text_styles.add(custom_shape).unwrap();
        let bundled_handle = root.allocate_handle();
        let mut bundled_shape = TextStyle::new("BundledShapes");
        bundled_shape.is_shape_file = true;
        bundled_shape.font_file = "ltypeshp.shx".into();
        bundled_shape.handle = bundled_handle;
        root.text_styles.add(bundled_shape).unwrap();
        let mut dashed = LineType::new("TRANSMIT_DASH");
        dashed.add_element(LineTypeElement::dash(5.0));
        dashed.add_element(LineTypeElement {
            length: -2.0,
            complex: Some(LineTypeComplexData {
                content: LineTypeComplexContent::Shape { shape_number: 132 },
                style_handle: bundled_handle,
                ..Default::default()
            }),
        });
        root.line_types.add(dashed).unwrap();

        let root_bytes = crate::io::save_to_bytes(&root, "dwg", root.version).unwrap();
        write(dir.join("root.dwg"), &root_bytes);
        let plot_name = format!("Transmittal-Test-{}.ctb", std::process::id());
        crate::io::plot_style::register_session_table(
            &plot_name,
            Arc::new(b"synthetic plot table".to_vec()),
        );
        let package = native::collect(
            &root,
            Path::new(&dir.join("root.dwg")),
            "root.dwg",
            Some(&plot_name),
        )
        .unwrap();
        let entries: HashMap<String, &[u8]> = package
            .entries
            .iter()
            .map(|(name, bytes)| (name.to_ascii_lowercase(), bytes.as_slice()))
            .collect();
        for (name, expected) in [
            ("root.png", b"root image".as_slice()),
            ("root.pdf", b"root pdf"),
            ("root.dgn", b"root dgn"),
            ("scan.rcs", b"synthetic point cloud"),
            ("custom.ttf", b"synthetic TrueType payload"),
            ("root-big.shx", b"synthetic big-font payload"),
            ("symbols.shx", b"synthetic shape payload"),
            ("nested.shx", b"nested font"),
            ("inner-big.shx", b"inner big font"),
        ] {
            assert_eq!(entries.get(name), Some(&expected), "entry {name}");
        }
        assert_eq!(entries.get("root.dwg"), Some(&root_bytes.as_slice()));
        let nested_bytes = std::fs::read(dir.join("nested.dwg")).unwrap();
        let inner_bytes = std::fs::read(dir.join("inner.dxf")).unwrap();
        assert_eq!(entries.get("nested.dwg"), Some(&nested_bytes.as_slice()));
        assert_eq!(entries.get("inner.dxf"), Some(&inner_bytes.as_slice()));
        assert_eq!(
            entries.get(&plot_name.to_ascii_lowercase()),
            Some(&b"synthetic plot table".as_slice())
        );
        let linetypes = String::from_utf8(entries[LINETYPES_FILE].to_vec()).unwrap();
        assert!(linetypes.contains("*TRANSMIT_DASH"));
        assert!(linetypes.contains("BOX,ltypeshp.shx"));
        assert!(entries.contains_key(&REPORT_FILE.to_ascii_lowercase()));
        assert!(package.issues.is_empty(), "{:?}", package.issues);

        // Use only package bytes in a new folder: stored subfolder references
        // must fall back to the flat sibling names on the receiving machine.
        let isolated = dir.join("isolated");
        std::fs::create_dir_all(&isolated).unwrap();
        for (name, bytes) in &package.entries {
            std::fs::write(isolated.join(name), bytes).unwrap();
        }
        let reopened = crate::io::load_bytes_finalized(
            &isolated.join("root.dwg"),
            entries["root.dwg"].to_vec(),
        )
        .unwrap();
        assert!(reopened.line_types.get("TRANSMIT_DASH").is_some());
        assert_eq!(
            crate::io::xref::resolve_path("nested.dwg", &isolated),
            Some(isolated.join("nested.dwg"))
        );
        let reopened_nested = crate::io::load_bytes_finalized(
            &isolated.join("nested.dwg"),
            entries["nested.dwg"].to_vec(),
        )
        .unwrap();
        assert_eq!(
            crate::io::resolve_image_file(
                "nested-assets/nested.shx",
                reopened_nested
                    .source_path
                    .as_deref()
                    .map(Path::new)
                    .and_then(Path::parent),
            )
            .map(PathBuf::from),
            Some(isolated.join("nested.shx"))
        );
        assert!(isolated.join(LINETYPES_FILE).is_file());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn report_lists_drawing_before_web_serialization_finishes() {
        let package = TransmittalPackage::new("web.dwg").unwrap().finish();
        let report = package
            .entries
            .iter()
            .find(|(name, _)| name == REPORT_FILE)
            .unwrap();
        let report = String::from_utf8(report.1.clone()).unwrap();
        assert!(report.contains("web.dwg\n  Type: drawing"));
    }

    #[test]
    fn package_rejects_conflicts_and_keeps_drawing() {
        let mut package = TransmittalPackage::new("plan.dwg").unwrap();
        package.set_drawing(b"drawing".to_vec());
        let first = Dependency::new(DependencyKind::Image, "a/site.png", "plan.dwg");
        package.add(&first, b"first".to_vec());
        package.add(&first, b"first".to_vec());
        let conflict = Dependency::new(DependencyKind::Image, "b/SITE.PNG", "nested.dwg");
        package.add(&conflict, b"different".to_vec());
        let drawing_conflict =
            Dependency::new(DependencyKind::Xref, "other/plan.dwg", "nested.dwg");
        package.add(&drawing_conflict, b"drawing".to_vec());
        let package = package.finish();
        assert_eq!(package.entries[0], ("plan.dwg".into(), b"drawing".to_vec()));
        assert_eq!(
            package
                .entries
                .iter()
                .filter(|(name, _)| name == "site.png")
                .count(),
            1
        );
        assert_eq!(package.issues.len(), 2);
        let report = package
            .entries
            .iter()
            .find(|(name, _)| name == REPORT_FILE)
            .unwrap();
        let report = String::from_utf8(report.1.clone()).unwrap();
        assert!(report.contains("conflicts"));
        assert!(report.contains("plan.dwg"));
    }
}
