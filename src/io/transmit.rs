//! Shared ETRANSMIT dependency discovery, reference rewriting, and packaging.
//!
//! Packaged documents are clones: the user's drawings are never modified.
//! Packages preserve source-relative folders, group files by type, or stay
//! flat. References are rewritten to match those locations, with unique names
//! allocated when different sources collide.

use crate::io::transmit_zip::{entry_name, file_name};
use document::{Document, EntityType};
use std::collections::HashMap;

pub(crate) const REPORT_FILE: &str = "ETRANSMIT.txt";
const XREF_FOLDER: &str = "xref";
const FONT_FOLDER: &str = "Fonts";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransmittalLayout {
    Preserve,
    #[default]
    Structured,
    Flat,
}

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
    Linetype,
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
            Self::Linetype => "external linetype file",
        }
    }
}

/// Type-to-folder mapping for Structured packages. Empty folders intentionally
/// mean the package root. The settings UI can expose these mappings later
/// without changing collection or reference rewriting.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TransmittalFolders {
    xrefs: String,
    images: String,
    underlays: String,
    point_clouds: String,
    fonts: String,
    plot_styles: String,
    linetypes: String,
}

impl Default for TransmittalFolders {
    fn default() -> Self {
        Self {
            xrefs: XREF_FOLDER.into(),
            images: "Images".into(),
            underlays: "Underlays".into(),
            point_clouds: "PointClouds".into(),
            fonts: FONT_FOLDER.into(),
            plot_styles: "PlotStyles".into(),
            linetypes: "Linetypes".into(),
        }
    }
}

impl TransmittalFolders {
    fn folder(&self, kind: DependencyKind) -> &str {
        match kind {
            DependencyKind::Xref => &self.xrefs,
            DependencyKind::Image => &self.images,
            DependencyKind::Underlay => &self.underlays,
            DependencyKind::PointCloud => &self.point_clouds,
            DependencyKind::Font | DependencyKind::BigFont | DependencyKind::Shape => &self.fonts,
            DependencyKind::PlotStyle => &self.plot_styles,
            DependencyKind::Linetype => &self.linetypes,
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
struct Resource {
    bytes: Vec<u8>,
    identity: String,
    file_name: String,
    preserved_path: Option<String>,
}

#[derive(Debug)]
struct Addition {
    path: String,
    is_new: bool,
}

#[derive(Debug)]
pub(crate) struct TransmittalPackage {
    pub entries: Vec<(String, Vec<u8>)>,
    pub issues: Vec<DependencyIssue>,
    pub warnings: Vec<String>,
    drawing_name: String,
    layout: TransmittalLayout,
    folders: TransmittalFolders,
    claimed: HashMap<String, String>,
    sources: HashMap<String, String>,
    packaged: Vec<(String, String, String)>,
}

impl TransmittalPackage {
    #[cfg(test)]
    pub fn new(drawing_name: &str) -> Result<Self, String> {
        Self::with_layout(drawing_name, TransmittalLayout::Structured)
    }

    pub fn with_layout(drawing_name: &str, layout: TransmittalLayout) -> Result<Self, String> {
        Self::with_folders(drawing_name, layout, TransmittalFolders::default())
    }

    fn with_folders(
        drawing_name: &str,
        layout: TransmittalLayout,
        folders: TransmittalFolders,
    ) -> Result<Self, String> {
        let drawing_name = entry_name(&file_name(drawing_name)?)?;
        let mut claimed = HashMap::new();
        claimed.insert(drawing_name.to_ascii_lowercase(), "the drawing".into());
        claimed.insert(
            REPORT_FILE.to_ascii_lowercase(),
            "the transmittal report".into(),
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
            layout,
            folders,
            claimed,
            sources: HashMap::new(),
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_drawing(&mut self, bytes: Vec<u8>) {
        self.entries.insert(0, (self.drawing_name.clone(), bytes));
    }

    fn nested_owner_folder<'a>(&self, packaged_path: &'a str) -> &'a str {
        packaged_path
            .rsplit_once('/')
            .map(|(folder, _)| folder)
            .unwrap_or("")
    }

    fn reference(&self, owner_folder: &str, target: &str, kind: DependencyKind) -> String {
        // AutoCAD's standard Fonts and PlotStyles folders support bare names.
        // Other mappings need an explicit relative path, especially when an
        // empty mapping puts a nested drawing's dependency at the root.
        let bare_name = self.layout == TransmittalLayout::Structured
            && match kind {
                DependencyKind::Font | DependencyKind::BigFont | DependencyKind::Shape => {
                    self.folders.fonts.eq_ignore_ascii_case(FONT_FOLDER)
                }
                DependencyKind::PlotStyle => {
                    self.folders.plot_styles.eq_ignore_ascii_case("PlotStyles")
                }
                _ => false,
            };
        package_reference(owner_folder, target, bare_name)
    }

    fn reserve_source(&mut self, kind: DependencyKind, identity: String, path: &str) {
        self.sources
            .insert(source_key(kind, &identity), path.to_owned());
    }

    fn add(&mut self, dependency: &Dependency, resource: Resource) -> Result<Addition, String> {
        let key = source_key(dependency.kind, &resource.identity);
        if let Some(existing) = self.sources.get(&key) {
            return Ok(Addition {
                path: existing.clone(),
                is_new: false,
            });
        }

        let name = file_name(&resource.file_name)?;
        let base = match self.layout {
            TransmittalLayout::Flat => name,
            TransmittalLayout::Preserve => resource.preserved_path.clone().unwrap_or(name),
            TransmittalLayout::Structured => {
                let folder = self.folders.folder(dependency.kind).trim();
                if folder.is_empty() {
                    name
                } else {
                    format!("{folder}/{name}")
                }
            }
        };
        let mut candidate = entry_name(&base)?;
        let mut suffix = 0usize;
        loop {
            let key = candidate.to_ascii_lowercase();
            let Some(previous) = self.claimed.get(&key) else {
                break;
            };
            let identical = (dependency.kind != DependencyKind::Xref && previous != "the drawing")
                .then(|| {
                    self.entries
                        .iter()
                        .find(|(entry, existing)| {
                            entry.eq_ignore_ascii_case(&candidate) && **existing == resource.bytes
                        })
                        .map(|(entry, _)| entry.clone())
                })
                .flatten();
            if let Some(existing) = identical {
                self.sources.insert(
                    source_key(dependency.kind, &resource.identity),
                    existing.clone(),
                );
                return Ok(Addition {
                    path: existing,
                    is_new: false,
                });
            }
            suffix += 1;
            candidate = entry_name(&suffixed_name(&base, suffix))?;
        }

        self.claimed.insert(
            candidate.to_ascii_lowercase(),
            format!("{} `{}`", dependency.kind.label(), dependency.reference),
        );
        self.sources.insert(key, candidate.clone());
        self.packaged.push((
            candidate.clone(),
            dependency.kind.label().into(),
            format!("{} (from {})", dependency.reference, dependency.owner),
        ));
        self.entries.push((candidate.clone(), resource.bytes));
        Ok(Addition {
            path: candidate,
            is_new: true,
        })
    }

    fn replace(&mut self, path: &str, bytes: Vec<u8>) {
        if let Some((_, existing)) = self.entries.iter_mut().find(|(name, _)| name == path) {
            *existing = bytes;
        }
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

    pub fn finish(mut self) -> Self {
        let mut report = format!(
            "OpenCADStudio transmittal report\n\
             ================================\n\
             Drawing: {}\n\n",
            self.drawing_name
        );
        match self.layout {
            TransmittalLayout::Preserve => report.push_str(
                "Source-relative folder paths are preserved where possible. Resources\n\
                 outside the source root fall back to safe package-root filenames.\n\
                 References are rewritten relative to each owning drawing.\n",
            ),
            TransmittalLayout::Structured => report.push_str(
                "Resources are grouped by type: xref/, Images/, Underlays/, PointClouds/,\n\
                 PlotStyles/, and Linetypes/. Fonts, big fonts, and SHX files use Fonts/.\n\
                 References are rewritten relative to each owning drawing; fonts and plot\n\
                 tables retain bare-filename references for package-folder discovery.\n",
            ),
            TransmittalLayout::Flat => report.push_str(
                "All files are stored flat at the package root. Xref and resource paths\n\
                 are rewritten to their allocated filenames; font references remain bare.\n",
            ),
        }
        report
            .push_str("Original drawings are not modified. Check missing dependencies below.\n\n");
        report.push_str("Packaged files\n--------------\n");
        for (name, kind, source) in &self.packaged {
            report.push_str(&format!("{name}\n  Type: {kind}\n  Source: {source}\n"));
        }
        report
            .push_str("\nMissing or unpackaged dependencies\n----------------------------------\n");
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

fn source_key(kind: DependencyKind, identity: &str) -> String {
    format!("{kind:?}:{identity}")
}

fn suffixed_name(path: &str, suffix: usize) -> String {
    let (folder, name) = path
        .rsplit_once('/')
        .map_or(("", path), |(folder, name)| (folder, name));
    let (stem, extension) = name
        .rfind('.')
        .filter(|dot| *dot > 0)
        .map_or((name, None), |dot| (&name[..dot], Some(&name[dot..])));
    let renamed = format!("{stem}_{suffix}{}", extension.unwrap_or(""));
    if folder.is_empty() {
        renamed
    } else {
        format!("{folder}/{renamed}")
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
                    && existing.reference == dependency.reference
                    && existing.bold == dependency.bold
                    && existing.italic == dependency.italic
            })
        {
            dependencies.push(dependency);
        }
    };

    // This is the only external linetype source reference OCS retains. The
    // linetype table itself is embedded and must not be exported as a made-up
    // `.lin` file.
    if let Some(file) = crate::io::drawing_variable(doc, "CENTERLTYPEFILE") {
        push(Dependency::new(DependencyKind::Linetype, &file, &owner));
    }
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
        DependencyKind::Linetype => {
            let name = dependency
                .reference
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&dependency.reference);
            name.eq_ignore_ascii_case("acad.lin") || name.eq_ignore_ascii_case("acadiso.lin")
        }
        _ => false,
    }
}

fn same_reference(stored: &str, dependency: &Dependency) -> bool {
    stored.trim() == dependency.reference
}

/// Preserve a stored relative path lexically within the package. Absolute and
/// escaping paths have no safe source-relative form and fall back to basename.
#[cfg(any(target_arch = "wasm32", test))]
fn preserved_reference(source_folder: Option<&str>, raw: &str) -> Option<String> {
    let raw = raw.trim().replace('\\', "/");
    if raw.starts_with('/')
        || (raw.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && raw.as_bytes().get(1) == Some(&b':'))
    {
        return None;
    }
    let mut parts: Vec<&str> = source_folder?
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    for part in raw.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    entry_name(&parts.join("/")).ok()
}

/// Source folder for a nested web xref, kept separate from its packaged owner
/// folder. `None` means the source is outside the root drawing's hierarchy.
#[cfg(any(target_arch = "wasm32", test))]
fn preserved_parent(path: Option<&str>) -> Option<String> {
    path.map(|path| {
        path.rsplit_once('/')
            .map(|(folder, _)| folder)
            .unwrap_or("")
            .to_owned()
    })
}

fn package_reference(owner_folder: &str, target: &str, bare_name: bool) -> String {
    let name = file_name(target).unwrap_or_else(|_| target.to_owned());
    if bare_name {
        return name;
    }
    let target_folder = target
        .rsplit_once('/')
        .map(|(folder, _)| folder)
        .unwrap_or("");
    if owner_folder == target_folder {
        return name;
    }
    let owners: Vec<&str> = owner_folder
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let targets: Vec<&str> = target_folder
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let common = owners
        .iter()
        .zip(&targets)
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts = Vec::new();
    parts.extend(std::iter::repeat_n("..", owners.len() - common));
    parts.extend_from_slice(&targets[common..]);
    parts.push(&name);
    parts.join("/")
}

fn rewrite_reference(doc: &mut Document, dependency: &Dependency, packaged: &str) {
    match dependency.kind {
        DependencyKind::Xref => {
            for record in doc.block_records.iter_mut() {
                if same_reference(&record.xref_path, dependency) {
                    record.xref_path = packaged.to_owned();
                }
            }
            for entity in doc.entities_mut() {
                if let EntityType::Block(block) = entity {
                    if same_reference(&block.xref_path, dependency) {
                        block.xref_path = packaged.to_owned();
                    }
                }
            }
        }
        DependencyKind::Image => {
            for object in doc.objects.values_mut() {
                if let document::objects::ObjectType::ImageDefinition(definition) = object {
                    if same_reference(&definition.file_name, dependency) {
                        definition.file_name = packaged.to_owned();
                    }
                }
            }
            for entity in doc.entities_mut() {
                if let EntityType::RasterImage(image) = entity {
                    if same_reference(&image.file_path, dependency) {
                        image.file_path = packaged.to_owned();
                    }
                }
            }
        }
        DependencyKind::Underlay => {
            for object in doc.objects.values_mut() {
                if let document::objects::ObjectType::UnderlayDefinition(definition) = object {
                    if same_reference(&definition.file_path, dependency) {
                        definition.file_path = packaged.to_owned();
                    }
                }
            }
        }
        DependencyKind::PointCloud => {
            for object in doc.objects.values_mut() {
                let document::objects::ObjectType::ClassObject(object) = object else {
                    continue;
                };
                match &mut object.data {
                    document::objects::ClassObjectData::PointCloudDefinitionEx(definition)
                    | document::objects::ClassObjectData::PointCloudDefinition(definition)
                        if same_reference(&definition.source_filename, dependency) =>
                    {
                        definition.source_filename = packaged.to_owned();
                    }
                    _ => {}
                }
            }
        }
        DependencyKind::Font | DependencyKind::Shape => {
            for style in doc.text_styles.iter_mut() {
                if same_reference(&style.font_file, dependency) {
                    style.font_file = packaged.to_owned();
                }
            }
            // Inline font families stay families. A file-valued inline override
            // does need retargeting when its source was flattened or renamed.
            if font_file_valued(&dependency.reference) {
                for entity in doc.entities_mut() {
                    rewrite_inline_fonts(entity, dependency, packaged);
                }
            }
        }
        DependencyKind::BigFont => {
            for style in doc.text_styles.iter_mut() {
                if same_reference(&style.big_font_file, dependency) {
                    style.big_font_file = packaged.to_owned();
                }
            }
        }
        DependencyKind::PlotStyle => {
            for object in doc.objects.values_mut() {
                if let document::objects::ObjectType::Layout(layout) = object {
                    if same_reference(&layout.plot_style_sheet, dependency) {
                        layout.plot_style_sheet = packaged.to_owned();
                    }
                }
            }
        }
        DependencyKind::Linetype => {
            if crate::io::drawing_variable(doc, "CENTERLTYPEFILE")
                .is_some_and(|value| value.trim() == dependency.reference)
            {
                crate::io::set_drawing_variable(doc, "CENTERLTYPEFILE", packaged);
            }
        }
    }
}

fn font_file_valued(reference: &str) -> bool {
    let lower = reference.to_ascii_lowercase();
    reference.contains(['/', '\\'])
        || [".ttf", ".otf", ".ttc", ".shx"]
            .iter()
            .any(|extension| lower.ends_with(extension))
}

#[cfg(any(target_arch = "wasm32", test))]
fn preserved_font_reference(source_folder: Option<&str>, reference: &str) -> Option<String> {
    if font_file_valued(reference) {
        preserved_reference(source_folder, reference)
    } else {
        None
    }
}

fn rewrite_inline_fonts(entity: &mut EntityType, dependency: &Dependency, packaged: &str) {
    let rewrite = |text: &mut String| rewrite_formatted_font(text, dependency, packaged);
    match entity {
        EntityType::MText(text) => rewrite(&mut text.value),
        EntityType::Dimension(dimension) => {
            rewrite(&mut dimension.base_mut().text);
            if let Some(text) = &mut dimension.base_mut().user_text {
                rewrite(text);
            }
        }
        EntityType::AttributeDefinition(attribute) => rewrite(&mut attribute.default_value),
        EntityType::AttributeEntity(attribute) => rewrite(&mut attribute.value),
        EntityType::Tolerance(tolerance) => rewrite(&mut tolerance.text),
        EntityType::MultiLeader(leader) => {
            rewrite(&mut leader.context.text_string);
            for attribute in &mut leader.block_attributes {
                rewrite(&mut attribute.text);
            }
        }
        EntityType::Table(table) => {
            for row in &mut table.rows {
                for cell in &mut row.cells {
                    for content in &mut cell.contents {
                        rewrite(&mut content.value.text);
                        rewrite(&mut content.value.formatted_value);
                        for attribute in &mut content.attributes {
                            rewrite(&mut attribute.value);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn rewrite_formatted_font(text: &mut String, dependency: &Dependency, packaged: &str) {
    if !(text.contains("\\f") || text.contains("\\F")) {
        return;
    }
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut copied = 0usize;
    let mut cursor = 0usize;
    while cursor + 1 < bytes.len() {
        if bytes[cursor] != b'\\' || !matches!(bytes[cursor + 1], b'f' | b'F') {
            cursor += 1;
            continue;
        }
        let value = cursor + 2;
        let Some(relative_end) = text[value..].find(';') else {
            break;
        };
        let end = value + relative_end;
        let token = &text[value..end];
        let (name, flags) = token
            .split_once('|')
            .map_or((token, None), |(name, _flags)| {
                (name, Some(&token[name.len()..]))
            });
        output.push_str(&text[copied..value]);
        if name.trim() == dependency.reference {
            output.push_str(packaged);
            output.push_str(flags.unwrap_or(""));
        } else {
            output.push_str(token);
        }
        copied = end;
        cursor = end;
    }
    output.push_str(&text[copied..]);
    *text = output;
}

fn serialize(doc: &Document, name: &str) -> Result<Vec<u8>, String> {
    let extension = std::path::Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("dwg");
    let format = if extension.eq_ignore_ascii_case("dxf") {
        "dxf"
    } else {
        "dwg"
    };
    crate::io::save_to_bytes(doc, format, doc.version)
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use std::path::{Path, PathBuf};

    pub fn collect(
        _doc: &Document,
        source: &Path,
        drawing_name: &str,
        active_plot_style: Option<&str>,
        layout: TransmittalLayout,
    ) -> Result<TransmittalPackage, String> {
        let original = std::fs::read(source)
            .map_err(|error| format!("cannot read {}: {error}", source.display()))?;
        // The native command packages the last-saved drawing, not unsaved scene
        // edits. Read that same snapshot before cloning and rewriting it.
        let saved = crate::io::load_bytes(&source.to_string_lossy(), original.clone())
            .map_err(|error| format!("cannot inspect {}: {error}", source.display()))?;
        let mut package = TransmittalPackage::with_layout(drawing_name, layout)?;
        package.reserve_source(DependencyKind::Xref, identity(source), drawing_name);
        let parent = source.parent().unwrap_or_else(|| Path::new("."));
        let source_root = std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
        let (rewritten, changed) = rewrite(
            &saved,
            source,
            "",
            &source_root,
            active_plot_style,
            &mut package,
        );
        let drawing = if changed {
            serialize(&rewritten, drawing_name)?
        } else {
            original
        };
        package.set_drawing(drawing);
        Ok(package.finish())
    }

    fn rewrite(
        doc: &Document,
        source: &Path,
        owner_folder: &str,
        source_root: &Path,
        active_plot_style: Option<&str>,
        package: &mut TransmittalPackage,
    ) -> (Document, bool) {
        let mut output = doc.clone();
        output.source_path = Some(source.to_string_lossy().into_owned());
        let base = source.parent().unwrap_or_else(|| Path::new("."));
        let mut changed = false;
        for dependency in direct_dependencies(doc, active_plot_style) {
            if built_in_resource(&dependency) {
                continue;
            }
            let resource = match find_resource(&dependency, base, source_root) {
                Ok(resource) => resource,
                Err(reason) => {
                    package.fail(&dependency, reason);
                    continue;
                }
            };
            let resource = if resource.is_some() {
                resource
            } else if dependency.kind == DependencyKind::PlotStyle {
                crate::io::plot_style::session_table(&dependency.reference).map(|bytes| Resource {
                    bytes: (*bytes).clone(),
                    identity: format!("session:{}", dependency.reference),
                    file_name: dependency.reference.clone(),
                    // A session-only table has no trustworthy source hierarchy.
                    preserved_path: None,
                })
            } else {
                None
            };
            let Some(resource) = resource else {
                if dependency.kind == DependencyKind::PlotStyle {
                    use crate::io::plot_style::PlotStyleTableExt as _;
                    if crate::io::plot_style::PlotStyleTable::builtin(&dependency.reference).is_ok()
                    {
                        continue;
                    }
                }
                package.fail(&dependency, "file not found");
                continue;
            };
            let original_bytes = resource.bytes.clone();
            let original_identity = resource.identity.clone();
            let addition = match package.add(&dependency, resource) {
                Ok(addition) => addition,
                Err(reason) => {
                    package.fail(&dependency, reason);
                    continue;
                }
            };
            let reference = package.reference(owner_folder, &addition.path, dependency.kind);
            if reference != dependency.reference {
                rewrite_reference(&mut output, &dependency, &reference);
                changed = true;
            }
            if dependency.kind != DependencyKind::Xref || !addition.is_new {
                continue;
            }
            let nested_owner_folder = package.nested_owner_folder(&addition.path);
            match crate::io::load_bytes(&dependency.reference, original_bytes.clone()) {
                Ok(nested) => {
                    let nested_source = PathBuf::from(&original_identity);
                    let (nested, nested_changed) = rewrite(
                        &nested,
                        &nested_source,
                        nested_owner_folder,
                        source_root,
                        None,
                        package,
                    );
                    if nested_changed {
                        match serialize(&nested, &addition.path) {
                            Ok(bytes) => package.replace(&addition.path, bytes),
                            Err(error) => package.fail(
                                &dependency,
                                format!("the xref was packaged, but its paths could not be rewritten: {error}"),
                            ),
                        }
                    }
                }
                Err(error) => package.fail(
                    &dependency,
                    format!("the xref was packaged, but its nested dependencies could not be inspected: {error}"),
                ),
            }
        }
        (output, changed)
    }

    fn find_resource(
        dependency: &Dependency,
        base: &Path,
        source_root: &Path,
    ) -> Result<Option<Resource>, String> {
        let raw = dependency.reference.trim();
        let path = match dependency.kind {
            DependencyKind::Xref => {
                crate::io::xref::resolve_path(raw, base).filter(|path| path.is_file())
            }
            DependencyKind::Image
            | DependencyKind::Underlay
            | DependencyKind::PointCloud
            | DependencyKind::Linetype => {
                crate::io::resolve_image_file(raw, Some(base)).map(PathBuf::from)
            }
            DependencyKind::BigFont | DependencyKind::Shape | DependencyKind::Font => {
                crate::io::font_repo::font_file_path(raw, Some(base)).map(PathBuf::from)
            }
            DependencyKind::PlotStyle => plot_style_path(raw, base),
        };
        if let Some(path) = path {
            let bytes = std::fs::read(&path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
            let file_name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| dependency.reference.clone());
            let preserved_path = preserved_native_path(&path, source_root, &file_name);
            return Ok(Some(Resource {
                bytes,
                identity: identity(&path),
                file_name,
                preserved_path,
            }));
        }
        if dependency.kind == DependencyKind::Font {
            if let Some((name, bytes)) = crate::scene::text::sysfont::transmittal_face(
                &dependency.reference,
                dependency.bold,
                dependency.italic,
            ) {
                return Ok(Some(Resource {
                    bytes,
                    identity: format!(
                        "face:{}:{}:{}",
                        dependency.reference, dependency.bold, dependency.italic
                    ),
                    file_name: name,
                    preserved_path: None,
                }));
            }
        }
        Ok(None)
    }

    fn preserved_native_path(path: &Path, source_root: &Path, file_name: &str) -> Option<String> {
        let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if let Ok(relative) = resolved.strip_prefix(source_root) {
            let candidate = relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            if entry_name(&candidate).is_ok() {
                return Some(candidate);
            }
        }
        Some(file_name.to_owned())
    }

    fn identity(path: &Path) -> String {
        std::fs::canonicalize(path)
            .ok()
            .or_else(|| {
                if path.is_absolute() {
                    Some(path.to_path_buf())
                } else {
                    std::env::current_dir().ok().map(|base| base.join(path))
                }
            })
            .unwrap_or_else(|| path.to_path_buf())
            .to_string_lossy()
            .into_owned()
    }

    fn plot_style_path(name: &str, base: &Path) -> Option<PathBuf> {
        if let Some(path) = crate::io::plot_style::drawing_table_path(name, Some(base)) {
            return Some(path);
        }
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
    use std::sync::Arc;

    pub fn collect(
        doc: &Document,
        drawing_name: &str,
        active_plot_style: Option<&str>,
        layout: TransmittalLayout,
    ) -> Result<(TransmittalPackage, Document), String> {
        let mut package = TransmittalPackage::with_layout(drawing_name, layout)?;
        package.reserve_source(
            DependencyKind::Xref,
            crate::io::xref::file_name_key(drawing_name),
            drawing_name,
        );
        let (drawing, _) = rewrite(doc, "", Some(""), active_plot_style, &mut package);
        Ok((package.finish(), drawing))
    }

    fn rewrite(
        doc: &Document,
        owner_folder: &str,
        source_folder: Option<&str>,
        active_plot_style: Option<&str>,
        package: &mut TransmittalPackage,
    ) -> (Document, bool) {
        let mut output = doc.clone();
        let mut changed = false;
        for dependency in direct_dependencies(doc, active_plot_style) {
            if built_in_resource(&dependency) {
                continue;
            }
            let mut registry_key = None;
            let resource = match dependency.kind {
                DependencyKind::Xref => {
                    registry_key = crate::io::xref::find_registered_source(&dependency.reference);
                    registry_key.as_ref().and_then(|key| {
                        crate::io::xref::registered_bytes(key).map(|bytes| Resource {
                            bytes: (*bytes).clone(),
                            identity: crate::io::xref::file_name_key(key),
                            file_name: file_name(key).unwrap_or_else(|_| key.clone()),
                            preserved_path: preserved_reference(
                                source_folder,
                                &dependency.reference,
                            ),
                        })
                    })
                }
                DependencyKind::Image => {
                    crate::scene::model::image_model::registered_source_bytes(&dependency.reference)
                        .map(|bytes| registered_resource(&dependency, source_folder, bytes))
                }
                DependencyKind::Underlay => {
                    crate::scene::model::pdf_raster::registered_source_bytes(&dependency.reference)
                        .map(|bytes| registered_resource(&dependency, source_folder, bytes))
                }
                DependencyKind::Font => true_type_resource(&dependency, source_folder),
                DependencyKind::PlotStyle => {
                    match crate::io::plot_style::session_table(&dependency.reference) {
                        Some(bytes) => Some(Resource {
                            bytes: (*bytes).clone(),
                            identity: format!("session:{}", dependency.reference),
                            file_name: dependency.reference.clone(),
                            preserved_path: preserved_reference(
                                source_folder,
                                &dependency.reference,
                            ),
                        }),
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
                DependencyKind::BigFont
                | DependencyKind::Shape
                | DependencyKind::PointCloud
                | DependencyKind::Linetype => None,
            };
            let Some(resource) = resource else {
                package.fail(
                    &dependency,
                    "original bytes are not loaded in this browser session",
                );
                continue;
            };
            let original_preserved_path = resource.preserved_path.clone();
            let addition = match package.add(&dependency, resource) {
                Ok(addition) => addition,
                Err(reason) => {
                    package.fail(&dependency, reason);
                    continue;
                }
            };
            let reference = package.reference(owner_folder, &addition.path, dependency.kind);
            if reference != dependency.reference {
                rewrite_reference(&mut output, &dependency, &reference);
                changed = true;
            }
            if dependency.kind != DependencyKind::Xref || !addition.is_new {
                continue;
            }
            let nested_owner_folder = package.nested_owner_folder(&addition.path);
            let Some(key) = registry_key else { continue };
            match crate::io::xref::registered_document(&key) {
                Ok(nested) => {
                    let nested_source_folder = preserved_parent(original_preserved_path.as_deref());
                    let (nested, nested_changed) = rewrite(
                        &nested,
                        nested_owner_folder,
                        nested_source_folder.as_deref(),
                        None,
                        package,
                    );
                    if nested_changed {
                        match serialize(&nested, &addition.path) {
                            Ok(bytes) => package.replace(&addition.path, bytes),
                            Err(error) => package.fail(
                                &dependency,
                                format!("the xref was packaged, but its paths could not be rewritten: {error}"),
                            ),
                        }
                    }
                }
                Err(error) => package.fail(
                    &dependency,
                    format!("the xref was packaged, but its nested dependencies could not be inspected: {error}"),
                ),
            }
        }
        (output, changed)
    }

    fn registered_resource(
        dependency: &Dependency,
        source_folder: Option<&str>,
        bytes: Arc<Vec<u8>>,
    ) -> Resource {
        Resource {
            bytes: (*bytes).clone(),
            identity: dependency.reference.clone(),
            file_name: dependency.reference.clone(),
            preserved_path: preserved_reference(source_folder, &dependency.reference),
        }
    }

    fn true_type_resource(
        dependency: &Dependency,
        source_folder: Option<&str>,
    ) -> Option<Resource> {
        if dependency.reference.to_ascii_lowercase().ends_with(".shx") {
            return None;
        }
        crate::scene::text::sysfont::transmittal_face(
            &dependency.reference,
            dependency.bold,
            dependency.italic,
        )
        .map(|(name, bytes)| Resource {
            bytes,
            identity: format!(
                "face:{}:{}:{}",
                dependency.reference, dependency.bold, dependency.italic
            ),
            file_name: name,
            preserved_path: preserved_font_reference(source_folder, &dependency.reference),
        })
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) use web::collect as collect_web;

#[cfg(test)]
mod tests {
    use super::*;

    fn resource(dependency: &Dependency, bytes: &[u8]) -> Resource {
        Resource {
            bytes: bytes.to_vec(),
            identity: format!("{}:{}", dependency.owner, dependency.reference),
            file_name: dependency.reference.clone(),
            preserved_path: None,
        }
    }

    #[test]
    fn direct_inventory_covers_resources_inline_fonts_and_external_linetypes() {
        let mut doc = Document::new();
        crate::io::set_drawing_variable(&mut doc, "CENTERLTYPEFILE", "catalog/custom.lin");
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
        assert!(dependencies.iter().any(|d| {
            d.kind == DependencyKind::Linetype && d.reference == "catalog/custom.lin"
        }));
    }

    #[test]
    fn case_variant_references_remain_distinct_until_sources_resolve() {
        let mut doc = Document::new();
        for (name, path) in [("LOWER", "Site.dwg"), ("UPPER", "SITE.DWG")] {
            let mut record = document::tables::BlockRecord::new(name);
            record.flags.is_xref = true;
            record.xref_path = path.into();
            doc.block_records.add(record).unwrap();
        }
        let dependencies = direct_dependencies(&doc, None);
        let mut paths: Vec<&str> = dependencies
            .iter()
            .filter(|dependency| dependency.kind == DependencyKind::Xref)
            .map(|dependency| dependency.reference.as_str())
            .collect();
        paths.sort_unstable();
        assert_eq!(paths, ["SITE.DWG", "Site.dwg"]);

        let mut package = TransmittalPackage::new("root.dwg").unwrap();
        for dependency in &dependencies {
            let addition = package
                .add(dependency, resource(dependency, b"drawing"))
                .unwrap();
            let packaged = package.reference("", &addition.path, dependency.kind);
            rewrite_reference(&mut doc, dependency, &packaged);
        }
        assert_eq!(
            doc.block_records.get("LOWER").unwrap().xref_path,
            "xref/Site.dwg"
        );
        assert_eq!(
            doc.block_records.get("UPPER").unwrap().xref_path,
            "xref/SITE_1.DWG"
        );
    }

    #[test]
    fn embedded_linetypes_do_not_invent_an_external_dependency() {
        let doc = Document::new();
        assert!(!direct_dependencies(&doc, None)
            .iter()
            .any(|dependency| dependency.kind == DependencyKind::Linetype));
        let mut default_catalog = Document::new();
        crate::io::set_drawing_variable(&mut default_catalog, "CENTERLTYPEFILE", "acadiso.lin");
        let dependency = direct_dependencies(&default_catalog, None)
            .into_iter()
            .find(|dependency| dependency.kind == DependencyKind::Linetype)
            .unwrap();
        assert!(built_in_resource(&dependency));
    }

    #[test]
    fn web_preserve_tracks_outside_root_sources_separately() {
        assert_eq!(
            preserved_reference(Some("xrefs"), "details/a.dwg"),
            Some("xrefs/details/a.dwg".into())
        );
        assert_eq!(
            preserved_reference(Some("xrefs"), "../shared.dwg"),
            Some("shared.dwg".into())
        );
        assert_eq!(preserved_reference(Some(""), "../outside.dwg"), None);
        assert_eq!(preserved_reference(None, "images/a.png"), None);
        assert_eq!(
            preserved_parent(Some("xrefs/nested.dwg")),
            Some("xrefs".into())
        );
        assert_eq!(preserved_parent(Some("outside.dwg")), Some(String::new()));
        assert_eq!(preserved_parent(None), None);
        assert_eq!(
            preserved_font_reference(Some(""), "assets/custom.ttf"),
            Some("assets/custom.ttf".into())
        );
        assert_eq!(preserved_font_reference(None, "assets/custom.ttf"), None);
        assert_eq!(preserved_font_reference(Some(""), "Arial"), None);
    }

    #[test]
    fn package_paths_match_autocad_layout() {
        assert_eq!(
            package_reference("", "xref/site.dwg", false),
            "xref/site.dwg"
        );
        assert_eq!(
            package_reference("xref", "xref/site.dwg", false),
            "site.dwg"
        );
        assert_eq!(
            package_reference("xref", "xref/root.dwg", false),
            "root.dwg"
        );
        assert_eq!(package_reference("xref", "root.png", false), "../root.png");
        assert_eq!(
            package_reference("xref", "Fonts/arial.ttf", true),
            "arial.ttf"
        );
        assert_eq!(package_reference("", "Fonts/arial.ttf", true), "arial.ttf");
        assert_eq!(
            package_reference("xref", "arial.ttf", false),
            "../arial.ttf",
            "an empty font-folder mapping needs an explicit root-relative reference"
        );
    }

    #[test]
    fn package_allocates_unique_names_and_reuses_sources() {
        let mut package = TransmittalPackage::new("plan.dwg").unwrap();
        package.set_drawing(b"drawing".to_vec());
        let first = Dependency::new(DependencyKind::Image, "a/site.png", "plan.dwg");
        let first_path = package.add(&first, resource(&first, b"first")).unwrap();
        assert_eq!(first_path.path, "Images/site.png");
        assert!(first_path.is_new);
        assert!(
            !package
                .add(&first, resource(&first, b"first"))
                .unwrap()
                .is_new
        );

        let collision = Dependency::new(DependencyKind::Image, "b/SITE.PNG", "nested.dwg");
        let collision_path = package
            .add(&collision, resource(&collision, b"different"))
            .unwrap();
        assert_eq!(collision_path.path, "Images/SITE_1.PNG");
        let identical_case_variant =
            Dependency::new(DependencyKind::Image, "c/SITE.PNG", "other.dwg");
        let entry_count = package.entries.len();
        let deduplicated = package
            .add(
                &identical_case_variant,
                resource(&identical_case_variant, b"first"),
            )
            .unwrap();
        assert_eq!(deduplicated.path, "Images/site.png");
        assert!(!deduplicated.is_new);
        assert_eq!(package.entries.len(), entry_count);
        let xref = Dependency::new(DependencyKind::Xref, "other/plan.dwg", "nested.dwg");
        assert_eq!(
            package
                .add(&xref, resource(&xref, b"drawing"))
                .unwrap()
                .path,
            "xref/plan.dwg"
        );
        let font = Dependency::new(DependencyKind::Font, "path/arial.ttf", "plan.dwg");
        assert_eq!(
            package.add(&font, resource(&font, b"font")).unwrap().path,
            "Fonts/arial.ttf"
        );
        assert!(package.issues.is_empty());
    }

    #[test]
    fn flat_package_uses_unique_root_names_without_subfolders() {
        let mut package =
            TransmittalPackage::with_layout("plan.dwg", TransmittalLayout::Flat).unwrap();
        package.set_drawing(b"root".to_vec());
        let xref = Dependency::new(DependencyKind::Xref, "other/plan.dwg", "root.dwg");
        let xref_path = package.add(&xref, resource(&xref, b"nested")).unwrap();
        assert_eq!(xref_path.path, "plan_1.dwg");
        assert_eq!(package.nested_owner_folder(&xref_path.path), "");
        assert_eq!(
            package.reference(
                package.nested_owner_folder(&xref_path.path),
                &xref_path.path,
                xref.kind
            ),
            "plan_1.dwg"
        );
        let font = Dependency::new(DependencyKind::Font, "fonts/arial.ttf", "root.dwg");
        assert_eq!(
            package.add(&font, resource(&font, b"font")).unwrap().path,
            "arial.ttf"
        );
        assert!(package.entries.iter().all(|(name, _)| !name.contains('/')));
    }

    #[test]
    fn structured_folders_may_be_empty_to_mean_package_root() {
        let folders = TransmittalFolders {
            images: String::new(),
            fonts: String::new(),
            ..TransmittalFolders::default()
        };
        let mut package =
            TransmittalPackage::with_folders("plan.dwg", TransmittalLayout::Structured, folders)
                .unwrap();
        package.set_drawing(b"root".to_vec());
        let xref = Dependency::new(DependencyKind::Xref, "nested.dwg", "root.dwg");
        let xref_path = package.add(&xref, resource(&xref, b"nested")).unwrap();
        assert_eq!(package.nested_owner_folder(&xref_path.path), "xref");

        let image = Dependency::new(DependencyKind::Image, "images/site.png", "nested.dwg");
        let image_path = package.add(&image, resource(&image, b"image")).unwrap();
        assert_eq!(image_path.path, "site.png");
        assert_eq!(
            package.reference("xref", &image_path.path, image.kind),
            "../site.png"
        );

        let font = Dependency::new(DependencyKind::Font, "assets/custom.shx", "nested.dwg");
        let font_path = package.add(&font, resource(&font, b"font")).unwrap();
        assert_eq!(font_path.path, "custom.shx");
        assert_eq!(
            package.reference("xref", &font_path.path, font.kind),
            "../custom.shx"
        );
    }

    #[test]
    fn formatted_font_file_override_is_rewritten_without_changing_flags() {
        let dependency = Dependency::font("old/Custom.ttf", "root.dwg", false, true);
        let mut text = EntityType::MText(Box::new(document::entities::MText::new()));
        let EntityType::MText(value) = &mut text else {
            unreachable!()
        };
        value.value = "{\\f old/Custom.ttf |b0|i1;Text}".into();
        rewrite_inline_fonts(&mut text, &dependency, "Custom_1.ttf");
        let EntityType::MText(value) = text else {
            unreachable!()
        };
        assert_eq!(value.value, "{\\fCustom_1.ttf|b0|i1;Text}");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_package_rewrites_recursive_layout_and_reopens_isolated() {
        use document::objects::{
            ClassObject, ClassObjectData, ImageDefinition, ObjectType, PointCloudDefinition,
            UnderlayDefinition,
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
        std::fs::create_dir_all(dir.join("catalog")).unwrap();
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
        write(dir.join("catalog/custom.lin"), b"*EXTERNAL\nA,1,-1\n");
        let styled_text = |style: &str| {
            let mut text = document::entities::MText::new();
            text.style = style.to_owned();
            text.value = "keep the text style referenced".into();
            text
        };

        let mut inner = Document::new();
        let mut inner_style = TextStyle::new("Inner");
        inner_style.big_font_file = "inner-big.shx".into();
        inner.text_styles.add(inner_style).unwrap();
        inner
            .add_entity(EntityType::MText(Box::new(styled_text("Inner"))))
            .unwrap();
        write(
            dir.join("inner.dxf"),
            &crate::io::save_to_bytes(&inner, "dxf", inner.version).unwrap(),
        );

        let mut nested = Document::new();
        let mut nested_style = TextStyle::new("Nested");
        nested_style.font_file = "nested-assets/nested.shx".into();
        nested.text_styles.add(nested_style).unwrap();
        nested
            .add_entity(EntityType::MText(Box::new(styled_text("Nested"))))
            .unwrap();
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
        crate::io::set_drawing_variable(&mut root, "CENTERLTYPEFILE", "catalog/custom.lin");
        root.line_types.add(LineType::new("EMBEDDED_ONLY")).unwrap();
        let mut nested_ref = BlockRecord::new("NESTED");
        nested_ref.flags.is_xref = true;
        nested_ref.xref_path = "nested.dwg".into();
        root.block_records.add(nested_ref).unwrap();
        let image_handle = root.allocate_handle();
        let mut image = ImageDefinition::with_dimensions("root.png", 8, 8);
        image.handle = image_handle;
        root.objects
            .insert(image_handle, ObjectType::ImageDefinition(image));
        let mut raster = document::entities::RasterImage::new(
            "root.png",
            document::types::Vector3::ZERO,
            8.0,
            8.0,
        );
        raster.definition_handle = Some(image_handle);
        root.add_entity(EntityType::RasterImage(raster)).unwrap();
        for (kind, definition) in [
            (
                document::entities::UnderlayType::Pdf,
                UnderlayDefinition::pdf("root.pdf", "1"),
            ),
            (
                document::entities::UnderlayType::Dgn,
                UnderlayDefinition::dgn("root.dgn", "Model"),
            ),
        ] {
            let handle = root.allocate_handle();
            let mut definition = definition;
            definition.handle = handle;
            root.objects
                .insert(handle, ObjectType::UnderlayDefinition(definition));
            let mut underlay = document::entities::Underlay::new(kind);
            underlay.definition_handle = handle;
            root.add_entity(EntityType::Underlay(underlay)).unwrap();
        }
        let cloud_handle = root.allocate_handle();
        let mut cloud_definition = ClassObject::new(ClassObjectData::PointCloudDefinitionEx(
            PointCloudDefinition {
                source_filename: "scan.rcs".into(),
                ..Default::default()
            },
        ));
        cloud_definition.handle = cloud_handle;
        root.objects
            .insert(cloud_handle, ObjectType::ClassObject(cloud_definition));
        let cloud = document::entities::PointCloudExData {
            class_version: 1,
            extents_min: document::types::Vector3::ZERO,
            extents_max: document::types::Vector3::ZERO,
            ucs_origin: document::types::Vector3::ZERO,
            ucs_x_direction: document::types::Vector3::new(1.0, 0.0, 0.0),
            ucs_y_direction: document::types::Vector3::new(0.0, 1.0, 0.0),
            ucs_z_direction: document::types::Vector3::new(0.0, 0.0, 1.0),
            locked: false,
            definition_handle: cloud_handle,
            reactor_handle: document::Handle::NULL,
            name: "scan".into(),
            show_intensity: false,
            show_cropping: true,
            hidden_scans: Vec::new(),
            hidden_regions: Vec::new(),
            stylization_type: 1,
            intensity_color_scheme: String::new(),
            current_color_scheme: String::new(),
            classification_color_scheme: String::new(),
            elevation_min: 0.0,
            elevation_max: 0.0,
            intensity_min: 0,
            intensity_max: 100,
            intensity_out_of_range_behavior: 1,
            elevation_out_of_range_behavior: 1,
            elevation_apply_to_fixed_range: false,
            intensity_as_gradient: false,
            elevation_as_gradient: false,
            croppings: Vec::new(),
        };
        root.add_entity(EntityType::Extended(Box::new(
            document::entities::ExtendedEntity {
                common: document::entities::EntityCommon::new(),
                data: document::entities::ExtendedEntityData::PointCloudEx(cloud),
            },
        )))
        .unwrap();
        let mut text_style = TextStyle::new("RootText");
        text_style.font_file = "custom.ttf".into();
        text_style.big_font_file = "root-big.shx".into();
        root.text_styles.add(text_style).unwrap();
        root.add_entity(EntityType::MText(Box::new(styled_text("RootText"))))
            .unwrap();
        let mut custom_shape = TextStyle::new("CustomShapes");
        custom_shape.is_shape_file = true;
        custom_shape.font_file = "symbols.shx".into();
        root.text_styles.add(custom_shape).unwrap();
        root.add_entity(EntityType::MText(Box::new(styled_text("CustomShapes"))))
            .unwrap();

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
            TransmittalLayout::Structured,
        )
        .unwrap();
        let entries: HashMap<String, &[u8]> = package
            .entries
            .iter()
            .map(|(name, bytes)| (name.to_ascii_lowercase(), bytes.as_slice()))
            .collect();
        for (name, expected) in [
            ("images/root.png", b"root image".as_slice()),
            ("underlays/root.pdf", b"root pdf"),
            ("underlays/root.dgn", b"root dgn"),
            ("pointclouds/scan.rcs", b"synthetic point cloud"),
            ("linetypes/custom.lin", b"*EXTERNAL\nA,1,-1\n"),
            ("fonts/custom.ttf", b"synthetic TrueType payload"),
            ("fonts/root-big.shx", b"synthetic big-font payload"),
            ("fonts/symbols.shx", b"synthetic shape payload"),
            ("fonts/nested.shx", b"nested font"),
            ("fonts/inner-big.shx", b"inner big font"),
        ] {
            assert_eq!(entries.get(name), Some(&expected), "entry {name}");
        }
        assert!(!entries.contains_key("opencadstudio.lin"));
        assert_eq!(
            entries.get("xref/inner.dxf"),
            Some(&std::fs::read(dir.join("inner.dxf")).unwrap().as_slice())
        );
        assert_eq!(
            entries.get(format!("plotstyles/{}", plot_name.to_ascii_lowercase()).as_str()),
            Some(&b"synthetic plot table".as_slice())
        );
        assert!(entries.contains_key(&REPORT_FILE.to_ascii_lowercase()));
        assert!(package.issues.is_empty(), "{:?}", package.issues);

        let packaged_root =
            crate::io::load_bytes("root.dwg", entries["root.dwg"].to_vec()).unwrap();
        assert_eq!(
            packaged_root
                .block_records
                .get("NESTED")
                .map(|record| record.xref_path.as_str()),
            Some("xref/nested.dwg")
        );
        assert_eq!(
            crate::io::drawing_variable(&packaged_root, "CENTERLTYPEFILE").as_deref(),
            Some("Linetypes/custom.lin")
        );
        assert!(packaged_root.line_types.get("EMBEDDED_ONLY").is_some());
        let packaged_nested =
            crate::io::load_bytes("nested.dwg", entries["xref/nested.dwg"].to_vec()).unwrap();
        assert_eq!(
            packaged_nested
                .text_styles
                .get("Nested")
                .map(|style| style.font_file.as_str()),
            Some("nested.shx")
        );
        assert_eq!(
            packaged_nested
                .block_records
                .get("INNER")
                .map(|record| record.xref_path.as_str()),
            Some("inner.dxf")
        );

        // Extract only package entries and resolve from the new layout.
        let isolated = dir.join("isolated");
        for (name, bytes) in &package.entries {
            let path = isolated.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        let reopened = crate::io::load_bytes_finalized(
            &isolated.join("root.dwg"),
            entries["root.dwg"].to_vec(),
        )
        .unwrap();
        assert_eq!(
            crate::io::xref::resolve_path(
                &reopened.block_records.get("NESTED").unwrap().xref_path,
                &isolated,
            ),
            Some(isolated.join("xref/nested.dwg"))
        );
        assert_eq!(
            crate::io::xref::resolve_path("nested.dwg", &isolated),
            Some(isolated.join("xref/nested.dwg")),
            "a flat reference also finds the structured xref"
        );
        let reopened_nested = crate::io::load_bytes_finalized(
            &isolated.join("xref/nested.dwg"),
            entries["xref/nested.dwg"].to_vec(),
        )
        .unwrap();
        assert_eq!(
            crate::io::font_repo::font_file_path(
                &reopened_nested.text_styles.get("Nested").unwrap().font_file,
                reopened_nested
                    .source_path
                    .as_deref()
                    .map(Path::new)
                    .and_then(Path::parent),
            )
            .map(PathBuf::from),
            Some(isolated.join("Fonts/nested.shx"))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_plot_tables_use_each_owning_drawing() {
        use document::objects::{Layout, ObjectType};
        use document::tables::BlockRecord;
        use std::sync::Arc;

        let dir = std::env::temp_dir().join(format!(
            "ocs-transmit-plot-owners-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("PlotStyles")).unwrap();
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        let name = format!("Owner-{}.ctb", std::process::id());
        let mut root_table = crate::io::plot_style::PlotStyleTable::identity(&name);
        root_table.aci_entries[1].lineweight = 13;
        let root_table_bytes = root_table.to_bytes().unwrap();
        std::fs::write(dir.join("PlotStyles").join(&name), &root_table_bytes).unwrap();
        let mut nested_table = crate::io::plot_style::PlotStyleTable::identity(&name);
        nested_table.aci_entries[1].lineweight = 17;
        let nested_table_bytes = nested_table.to_bytes().unwrap();
        std::fs::write(dir.join("nested").join(&name), &nested_table_bytes).unwrap();
        let mut session_table = crate::io::plot_style::PlotStyleTable::identity(&name);
        session_table.aci_entries[1].lineweight = 25;
        crate::io::plot_style::register_session_table(
            &name,
            Arc::new(session_table.to_bytes().unwrap()),
        );

        let with_table = |doc: &mut Document| {
            let handle = doc.allocate_handle();
            let mut layout = Layout::new("Test");
            layout.handle = handle;
            layout.plot_style_sheet = name.clone();
            doc.objects.insert(handle, ObjectType::Layout(layout));
        };
        let mut nested = Document::new();
        with_table(&mut nested);
        std::fs::write(
            dir.join("nested/nested.dwg"),
            crate::io::save_to_bytes(&nested, "dwg", nested.version).unwrap(),
        )
        .unwrap();
        let mut root = Document::new();
        with_table(&mut root);
        let mut xref = BlockRecord::new("NESTED");
        xref.flags.is_xref = true;
        xref.xref_path = "nested/nested.dwg".into();
        root.block_records.add(xref).unwrap();
        std::fs::write(
            dir.join("root.dwg"),
            crate::io::save_to_bytes(&root, "dwg", root.version).unwrap(),
        )
        .unwrap();

        let package = native::collect(
            &root,
            &dir.join("root.dwg"),
            "root.dwg",
            None,
            TransmittalLayout::Structured,
        )
        .unwrap();
        let entries: HashMap<String, &[u8]> = package
            .entries
            .iter()
            .map(|(path, bytes)| (path.to_ascii_lowercase(), bytes.as_slice()))
            .collect();
        let packaged_root =
            crate::io::load_bytes("root.dwg", entries["root.dwg"].to_vec()).unwrap();
        let packaged_nested =
            crate::io::load_bytes("nested.dwg", entries["xref/nested.dwg"].to_vec()).unwrap();
        let style_name = |doc: &Document| {
            doc.objects
                .values()
                .find_map(|object| match object {
                    ObjectType::Layout(layout) if layout.name == "Test" => {
                        Some(layout.plot_style_sheet.clone())
                    }
                    _ => None,
                })
                .unwrap()
        };
        let root_reference = style_name(&packaged_root);
        let nested_reference = style_name(&packaged_nested);
        assert_ne!(root_reference, nested_reference);
        let packaged_root_table = crate::io::plot_style::PlotStyleTable::from_bytes(
            &root_reference,
            entries[&format!("plotstyles/{root_reference}").to_ascii_lowercase()],
        )
        .unwrap();
        let packaged_nested_table = crate::io::plot_style::PlotStyleTable::from_bytes(
            &nested_reference,
            entries[&format!("plotstyles/{nested_reference}").to_ascii_lowercase()],
        )
        .unwrap();
        assert_eq!(packaged_root_table.aci_entries[1].lineweight, 13);
        assert_eq!(packaged_nested_table.aci_entries[1].lineweight, 17);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_preserve_layout_retains_source_hierarchy() {
        use document::objects::{ImageDefinition, ObjectType};
        use document::tables::{BlockRecord, TextStyle};

        let dir = std::env::temp_dir().join(format!(
            "ocs-transmit-preserve-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = dir.join("project");
        std::fs::create_dir_all(project.join("xrefs/details")).unwrap();
        std::fs::create_dir_all(project.join("xrefs/assets")).unwrap();
        std::fs::create_dir_all(project.join("xrefs/textures")).unwrap();
        std::fs::create_dir_all(project.join("images")).unwrap();
        std::fs::create_dir_all(project.join("PlotStyles")).unwrap();
        std::fs::write(project.join("images/root.png"), b"root image").unwrap();
        std::fs::write(
            project.join("PlotStyles/preserved.ctb"),
            b"preserved plot table",
        )
        .unwrap();
        std::fs::write(project.join("xrefs/textures/symbol.png"), b"nested image").unwrap();
        std::fs::write(project.join("xrefs/assets/nested.shx"), b"nested font").unwrap();

        let inner = Document::new();
        std::fs::write(
            project.join("xrefs/details/inner.dxf"),
            crate::io::save_to_bytes(&inner, "dxf", inner.version).unwrap(),
        )
        .unwrap();
        let outside = Document::new();
        std::fs::write(
            dir.join("outside.dwg"),
            crate::io::save_to_bytes(&outside, "dwg", outside.version).unwrap(),
        )
        .unwrap();

        let mut nested = Document::new();
        let mut inner_ref = BlockRecord::new("INNER");
        inner_ref.flags.is_xref = true;
        inner_ref.xref_path = "details/inner.dxf".into();
        nested.block_records.add(inner_ref).unwrap();
        let mut style = TextStyle::new("Nested");
        style.font_file = "assets/nested.shx".into();
        nested.text_styles.add(style).unwrap();
        let mut text = document::entities::MText::new();
        text.style = "Nested".into();
        nested
            .add_entity(EntityType::MText(Box::new(text)))
            .unwrap();
        let image_handle = nested.allocate_handle();
        let mut image = ImageDefinition::with_dimensions("textures/symbol.png", 8, 8);
        image.handle = image_handle;
        nested
            .objects
            .insert(image_handle, ObjectType::ImageDefinition(image));
        let mut raster = document::entities::RasterImage::new(
            "textures/symbol.png",
            document::types::Vector3::ZERO,
            8.0,
            8.0,
        );
        raster.definition_handle = Some(image_handle);
        nested.add_entity(EntityType::RasterImage(raster)).unwrap();
        std::fs::write(
            project.join("xrefs/nested.dwg"),
            crate::io::save_to_bytes(&nested, "dwg", nested.version).unwrap(),
        )
        .unwrap();

        let mut root = Document::new();
        for (name, path) in [
            ("NESTED", "xrefs/nested.dwg"),
            ("OUTSIDE", "../outside.dwg"),
        ] {
            let mut record = BlockRecord::new(name);
            record.flags.is_xref = true;
            record.xref_path = path.into();
            root.block_records.add(record).unwrap();
        }
        let image_handle = root.allocate_handle();
        let mut image = ImageDefinition::with_dimensions("images/root.png", 8, 8);
        image.handle = image_handle;
        root.objects
            .insert(image_handle, ObjectType::ImageDefinition(image));
        let mut raster = document::entities::RasterImage::new(
            "images/root.png",
            document::types::Vector3::ZERO,
            8.0,
            8.0,
        );
        raster.definition_handle = Some(image_handle);
        root.add_entity(EntityType::RasterImage(raster)).unwrap();
        let layout_handle = root.allocate_handle();
        let mut layout = document::objects::Layout::new("Test");
        layout.handle = layout_handle;
        layout.plot_style_sheet = "PlotStyles/preserved.ctb".into();
        root.objects
            .insert(layout_handle, ObjectType::Layout(layout));
        std::fs::write(
            project.join("root.dwg"),
            crate::io::save_to_bytes(&root, "dwg", root.version).unwrap(),
        )
        .unwrap();

        let package = native::collect(
            &root,
            &project.join("root.dwg"),
            "root.dwg",
            None,
            TransmittalLayout::Preserve,
        )
        .unwrap();
        let entries: HashMap<&str, &[u8]> = package
            .entries
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
            .collect();
        for name in [
            "images/root.png",
            "xrefs/nested.dwg",
            "xrefs/details/inner.dxf",
            "xrefs/textures/symbol.png",
            "xrefs/assets/nested.shx",
            "PlotStyles/preserved.ctb",
            "outside.dwg",
        ] {
            assert!(entries.contains_key(name), "missing {name}");
        }
        assert!(entries.keys().all(|name| !name.starts_with("..")));
        let packaged_root =
            crate::io::load_bytes("root.dwg", entries["root.dwg"].to_vec()).unwrap();
        assert_eq!(
            packaged_root.block_records.get("NESTED").unwrap().xref_path,
            "xrefs/nested.dwg"
        );
        assert_eq!(
            packaged_root
                .block_records
                .get("OUTSIDE")
                .unwrap()
                .xref_path,
            "outside.dwg"
        );
        assert!(packaged_root.objects.values().any(|object| matches!(
            object,
            ObjectType::Layout(layout) if layout.plot_style_sheet == "PlotStyles/preserved.ctb"
        )));
        let packaged_nested =
            crate::io::load_bytes("nested.dwg", entries["xrefs/nested.dwg"].to_vec()).unwrap();
        assert_eq!(
            packaged_nested
                .block_records
                .get("INNER")
                .unwrap()
                .xref_path,
            "details/inner.dxf"
        );
        assert_eq!(
            packaged_nested.text_styles.get("Nested").unwrap().font_file,
            "assets/nested.shx"
        );
        assert!(packaged_nested.objects.values().any(|object| matches!(
            object,
            ObjectType::ImageDefinition(definition) if definition.file_name == "textures/symbol.png"
        )));

        let isolated = dir.join("isolated");
        for (name, bytes) in &package.entries {
            let path = isolated.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        assert_eq!(
            crate::io::xref::resolve_path("xrefs/nested.dwg", &isolated),
            Some(isolated.join("xrefs/nested.dwg"))
        );
        // Compared as paths: joined onto its folder, a name with a `/` holds
        // both separators on Windows.
        assert_eq!(
            crate::io::resolve_image_file("textures/symbol.png", Some(&isolated.join("xrefs")))
                .map(std::path::PathBuf::from),
            Some(isolated.join("xrefs/textures/symbol.png"))
        );
        assert_eq!(
            crate::io::font_repo::font_file_path(
                "assets/nested.shx",
                Some(&isolated.join("xrefs"))
            )
            .map(std::path::PathBuf::from),
            Some(isolated.join("xrefs/assets/nested.shx"))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_flat_package_rewrites_and_reopens_recursive_resources() {
        use document::tables::{BlockRecord, TextStyle};

        let dir = std::env::temp_dir().join(format!(
            "ocs-transmit-flat-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/nested.shx"), b"nested font").unwrap();
        std::fs::write(dir.join("root.ttf"), b"root font").unwrap();

        let mut nested = Document::new();
        let mut style = TextStyle::new("Nested");
        style.font_file = "assets/nested.shx".into();
        nested.text_styles.add(style).unwrap();
        let mut text = document::entities::MText::new();
        text.style = "Nested".into();
        nested
            .add_entity(EntityType::MText(Box::new(text)))
            .unwrap();
        std::fs::write(
            dir.join("nested.dwg"),
            crate::io::save_to_bytes(&nested, "dwg", nested.version).unwrap(),
        )
        .unwrap();

        let mut root = Document::new();
        let mut xref = BlockRecord::new("NESTED");
        xref.flags.is_xref = true;
        xref.xref_path = "nested.dwg".into();
        root.block_records.add(xref).unwrap();
        let mut style = TextStyle::new("Root");
        style.font_file = "root.ttf".into();
        root.text_styles.add(style).unwrap();
        let mut text = document::entities::MText::new();
        text.style = "Root".into();
        root.add_entity(EntityType::MText(Box::new(text))).unwrap();
        std::fs::write(
            dir.join("root.dwg"),
            crate::io::save_to_bytes(&root, "dwg", root.version).unwrap(),
        )
        .unwrap();

        let package = native::collect(
            &root,
            &dir.join("root.dwg"),
            "root.dwg",
            None,
            TransmittalLayout::Flat,
        )
        .unwrap();
        let entries: HashMap<&str, &[u8]> = package
            .entries
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
            .collect();
        assert_eq!(entries.get("nested.shx"), Some(&b"nested font".as_slice()));
        assert_eq!(entries.get("root.ttf"), Some(&b"root font".as_slice()));
        assert!(entries.contains_key("nested.dwg"));
        assert!(entries.keys().all(|name| !name.contains('/')));

        let packaged_root =
            crate::io::load_bytes("root.dwg", entries["root.dwg"].to_vec()).unwrap();
        assert_eq!(
            packaged_root.block_records.get("NESTED").unwrap().xref_path,
            "nested.dwg"
        );
        let packaged_nested =
            crate::io::load_bytes("nested.dwg", entries["nested.dwg"].to_vec()).unwrap();
        assert_eq!(
            packaged_nested.text_styles.get("Nested").unwrap().font_file,
            "nested.shx"
        );
        let isolated = dir.join("isolated");
        std::fs::create_dir_all(&isolated).unwrap();
        for (name, bytes) in &package.entries {
            std::fs::write(isolated.join(name), bytes).unwrap();
        }
        assert_eq!(
            crate::io::xref::resolve_path("nested.dwg", &isolated),
            Some(isolated.join("nested.dwg"))
        );
        assert_eq!(
            crate::io::font_repo::font_file_path("nested.shx", Some(&isolated)),
            Some(isolated.join("nested.shx").to_string_lossy().into_owned())
        );
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
        assert!(report.contains("xref/"));
        assert!(report.contains("Fonts/"));
    }
}
