//! DWG Document Builder — maps raw DWG parsed data into CadDocument.
//!
//! This module bridges the gap between the low-level object readers
//! (which produce `*Data` structs) and the high-level domain model
//! (entities, objects, tables in `CadDocument`).
//!
//! ## Two-Pass Architecture
//!
//! **Pass 1 (Tables):** Read all table entries (layers, block headers,
//! text styles, linetypes) and build handle→name lookup maps.
//!
//! **Pass 2 (Entities & Objects):** Read entities and objects, resolving
//! handle references (e.g., layer_handle → layer name, block_handle →
//! block name) using the maps built in Pass 1.

use crate::document::CadDocument;
use crate::entities::EntityCommon;
use crate::entities::*;
use crate::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader;
use crate::io::dwg::dwg_stream_readers::object_reader::common::*;
use crate::io::dwg::dwg_stream_readers::object_reader::entities;
use crate::io::dwg::dwg_stream_readers::object_reader::objects;
use crate::io::dwg::dwg_stream_readers::object_reader::tables;

/// Recognize AutoCAD's hidden per-viewport layer-override records.
///
/// These records use `<base layer> @ <decimal viewport handle>` names but
/// resolve as the base layer in AutoCAD's public layer table.
fn viewport_override_base_layer(name: &str) -> Option<&str> {
    let (base, viewport) = name.rsplit_once(" @ ")?;
    (!base.is_empty() && !viewport.is_empty() && viewport.bytes().all(|byte| byte.is_ascii_digit()))
        .then_some(base)
}
use crate::io::dwg::dwg_stream_readers::object_reader::{DwgObjectReader, EntityCommonData};
use crate::io::dwg::parallel::{filter_map_slice, for_each_mut, map_chunks, map_mut, worker_count};
use crate::io::read::{push_read_diagnostic, ReadDiagnostic, ReadStage};
use crate::notification::{NotificationCollection, NotificationType};
use crate::types::Handle;
use crate::types::LineWeight;
use std::collections::{HashMap, HashSet};

fn boundary_path_from_dwg(path: entities::HatchBoundaryPath) -> crate::entities::BoundaryPath {
    use crate::entities::hatch::*;

    let mut result = BoundaryPath::with_flags(BoundaryPathFlags::from_bits(path.flags as u32));
    if !path.polyline_vertices.is_empty() {
        result.add_edge(BoundaryEdge::Polyline(PolylineEdge {
            vertices: path
                .polyline_vertices
                .into_iter()
                .map(|(point, bulge)| crate::types::Vector3::new(point.x, point.y, bulge))
                .collect(),
            is_closed: path.polyline_closed,
        }));
    }
    for edge in path.edges {
        match edge {
            entities::HatchEdge::Line(value) => result.add_edge(BoundaryEdge::Line(LineEdge {
                start: value.start,
                end: value.end,
            })),
            entities::HatchEdge::Arc(value) => {
                result.add_edge(BoundaryEdge::CircularArc(CircularArcEdge {
                    center: value.center,
                    radius: value.radius,
                    start_angle: value.start_angle,
                    end_angle: value.end_angle,
                    counter_clockwise: value.ccw,
                }))
            }
            entities::HatchEdge::Ellipse(value) => {
                result.add_edge(BoundaryEdge::EllipticArc(EllipticArcEdge {
                    center: value.center,
                    major_axis_endpoint: value.major_endpoint,
                    minor_axis_ratio: value.minor_ratio,
                    start_angle: value.start_angle,
                    end_angle: value.end_angle,
                    counter_clockwise: value.ccw,
                }))
            }
            entities::HatchEdge::Spline(value) => {
                result.add_edge(BoundaryEdge::Spline(SplineEdge {
                    degree: value.degree,
                    rational: value.rational,
                    periodic: value.periodic,
                    knots: value.knots,
                    control_points: value.control_points,
                    fit_points: value.fit_points,
                    start_tangent: value.start_tangent,
                    end_tangent: value.end_tangent,
                }))
            }
        }
    }
    result
}

fn read_hatch_scale_context_data(
    reader: &mut DwgMergedReader,
    version: crate::io::dwg::DwgVersion,
) -> crate::objects::HatchScaleContext {
    let mut pattern_lines = Vec::new();
    for _ in 0..reader.read_bit_short().max(0).min(10_000) {
        let angle = reader.read_bit_double();
        let base_point =
            crate::types::Vector2::new(reader.read_bit_double(), reader.read_bit_double());
        let offset = crate::types::Vector2::new(reader.read_bit_double(), reader.read_bit_double());
        let mut dash_lengths = Vec::new();
        for _ in 0..reader.read_bit_short().max(0).min(10_000) {
            dash_lengths.push(reader.read_bit_double());
        }
        pattern_lines.push(crate::entities::HatchPatternLine {
            angle,
            base_point,
            offset,
            dash_lengths,
        });
    }
    let pattern_scale = reader.read_bit_double();
    let pattern_base = crate::types::Vector3::new(
        reader.read_bit_double(),
        reader.read_bit_double(),
        reader.read_bit_double(),
    );
    let mut loops = Vec::new();
    for _ in 0..reader.read_bit_long().max(0).min(100_000) {
        let loop_type = reader.read_bit_long();
        let supports_context = reader.read_bit();
        let boundary = (!supports_context).then(|| {
            boundary_path_from_dwg(entities::read_hatch_boundary_path_contents(
                reader, version, loop_type, false,
            ))
        });
        loops.push(crate::objects::HatchLoopContext {
            loop_type,
            supports_context,
            boundary,
        });
    }
    crate::objects::HatchScaleContext {
        pattern_lines,
        pattern_scale,
        pattern_base,
        loops,
    }
}

/// Pending vertex data collected during Pass 2, keyed by owner (parent polyline) handle.
enum PendingVertex {
    V2D(entities::Vertex2DData),
    V3D(entities::Vertex3DData, EntityCommon),
    PfaceFace(entities::PfaceFaceData, EntityCommon),
}

/// Pending polyline entities awaiting vertex assembly.
#[derive(Default)]
struct PendingPolylines {
    /// Vertex data keyed by owner (parent polyline) handle.
    vertices: HashMap<u64, Vec<PendingVertex>>,
    /// SEQEND handle keyed by owner (parent polyline) handle.
    seqends: HashMap<u64, crate::types::Handle>,
    /// Polyline entities awaiting vertex assembly, keyed by their handle.
    polylines: Vec<(u64, EntityType)>,
}

struct Pass2Header {
    model_space_block_handle: Handle,
    paper_space_block_handle: Handle,
}

struct Pass2Output {
    version: crate::types::DxfVersion,
    header: Pass2Header,
    entities: Vec<std::sync::Arc<EntityType>>,
    objects: HashMap<Handle, crate::objects::ObjectType>,
    eed_by_handle: HashMap<Handle, Vec<(u64, Vec<u8>)>>,
    xdic_by_handle: HashMap<Handle, Handle>,
    reactors_by_handle: HashMap<Handle, Vec<Handle>>,
    dwg_data_store_handles: HashSet<Handle>,
    context_scales: HashMap<Handle, Handle>,
    block_visibility_params: HashMap<Handle, crate::objects::BlockVisibilityParameter>,
    block_representations: HashMap<Handle, Handle>,
    fields: HashMap<Handle, crate::document::FieldDef>,
    dgn_ls_definitions: HashMap<Handle, crate::objects::DgnLsDefinition>,
    dgn_ls_components: HashMap<Handle, crate::objects::DgnLsComponent>,
    view_rep_refs: HashMap<Handle, Vec<Handle>>,
    section_view_reps: Vec<Handle>,
    section_view_style: Option<crate::entities::SectionViewStyle>,
}

impl Pass2Output {
    fn new(
        version: crate::types::DxfVersion,
        model_space_block_handle: Handle,
        paper_space_block_handle: Handle,
        capacity: usize,
    ) -> Self {
        Self {
            version,
            header: Pass2Header {
                model_space_block_handle,
                paper_space_block_handle,
            },
            entities: Vec::with_capacity(capacity),
            objects: HashMap::with_capacity(capacity / 8),
            eed_by_handle: HashMap::new(),
            xdic_by_handle: HashMap::new(),
            reactors_by_handle: HashMap::new(),
            dwg_data_store_handles: HashSet::new(),
            context_scales: HashMap::new(),
            block_visibility_params: HashMap::new(),
            block_representations: HashMap::new(),
            fields: HashMap::new(),
            dgn_ls_definitions: HashMap::new(),
            dgn_ls_components: HashMap::new(),
            view_rep_refs: HashMap::new(),
            section_view_reps: Vec::new(),
            section_view_style: None,
        }
    }

    fn add_entity(&mut self, entity: EntityType) -> std::result::Result<Handle, ()> {
        let handle = entity.common().handle;
        self.entities.push(std::sync::Arc::new(entity));
        Ok(handle)
    }
}

struct ClassNames {
    cpp: HashMap<i16, String>,
    dxf: HashMap<i16, String>,
}

impl ClassNames {
    fn from_document(document: &CadDocument) -> Self {
        Self {
            cpp: document
                .classes
                .iter()
                .map(|class| (class.class_number, class.cpp_class_name.clone()))
                .collect(),
            dxf: document
                .classes
                .iter()
                .map(|class| (class.class_number, class.dxf_name.clone()))
                .collect(),
        }
    }
}

struct Pass2Chunk {
    output: Pass2Output,
    pending: PendingPolylines,
    pending_attributes: HashMap<u64, Vec<AttributeEntity>>,
    failures: Vec<RecordFailure>,
}

struct RecordFailure {
    code: &'static str,
    handle: u64,
    offset: usize,
    type_code: i16,
    message: String,
}

impl Pass2Chunk {
    fn new(
        version: crate::types::DxfVersion,
        model_space_block_handle: Handle,
        paper_space_block_handle: Handle,
        capacity: usize,
    ) -> Self {
        Self {
            output: Pass2Output::new(
                version,
                model_space_block_handle,
                paper_space_block_handle,
                capacity,
            ),
            pending: PendingPolylines::default(),
            pending_attributes: HashMap::new(),
            failures: Vec::new(),
        }
    }
}

pub struct DwgBuildOutcome {
    pub notifications: NotificationCollection,
    pub decoded_records: usize,
    pub skipped_records: usize,
    pub diagnostics: Vec<ReadDiagnostic>,
}

/// Handle-to-name resolution maps built from table entries.
struct HandleMaps {
    /// handle → layer name
    layers: HashMap<u64, String>,
    /// handle → block name
    blocks: HashMap<u64, String>,
    /// handle → text style name
    text_styles: HashMap<u64, String>,
    /// handle → linetype name
    linetypes: HashMap<u64, String>,
    /// Linetype names in table order, EXCLUDING ByBlock/ByLayer. A pre-R2018
    /// MLINESTYLE element stores its linetype as a 0-based index into this list
    /// (0x7FFF = ByLayer); R2018+ stores a handle instead.
    linetype_order: Vec<String>,
    /// handle → dimension style name
    dim_styles: HashMap<u64, String>,
    /// handle → named view name
    views: HashMap<u64, String>,
}

impl HandleMaps {
    fn new() -> Self {
        Self {
            layers: HashMap::new(),
            blocks: HashMap::new(),
            text_styles: HashMap::new(),
            linetypes: HashMap::new(),
            linetype_order: Vec::new(),
            dim_styles: HashMap::new(),
            views: HashMap::new(),
        }
    }

    fn layer_name(&self, handle: u64) -> String {
        self.layers
            .get(&handle)
            .cloned()
            .unwrap_or_else(|| "0".to_string())
    }

    fn block_name(&self, handle: u64) -> String {
        self.blocks
            .get(&handle)
            .cloned()
            .unwrap_or_else(|| format!("*U{}", handle))
    }

    fn style_name(&self, handle: u64) -> String {
        self.text_styles
            .get(&handle)
            .cloned()
            .unwrap_or_else(|| "Standard".to_string())
    }

    #[allow(dead_code)]
    fn dimstyle_name(&self, handle: u64) -> String {
        self.dim_styles
            .get(&handle)
            .cloned()
            .unwrap_or_else(|| "Standard".to_string())
    }
}

fn mtext_from_data(data: entities::MTextData, common: EntityCommon, maps: &HandleMaps) -> MText {
    let mut e = MText::new();
    e.common = common;
    e.value = data.value;
    e.insertion_point = data.insertion_point;
    e.height = data.height;
    e.rectangle_width = data.rectangle_width;
    if data.rectangle_height != 0.0 {
        e.rectangle_height = Some(data.rectangle_height);
    }
    e.extents_width = data.extents_width;
    e.extents_height = data.extents_height;
    e.normal = data.normal;
    e.attachment_point = match data.attachment_point {
        2 => AttachmentPoint::TopCenter,
        3 => AttachmentPoint::TopRight,
        4 => AttachmentPoint::MiddleLeft,
        5 => AttachmentPoint::MiddleCenter,
        6 => AttachmentPoint::MiddleRight,
        7 => AttachmentPoint::BottomLeft,
        8 => AttachmentPoint::BottomCenter,
        9 => AttachmentPoint::BottomRight,
        _ => AttachmentPoint::TopLeft,
    };
    e.drawing_direction = match data.drawing_direction {
        3 => DrawingDirection::TopToBottom,
        5 => DrawingDirection::ByStyle,
        _ => DrawingDirection::LeftToRight,
    };
    e.rotation = data.x_direction.y.atan2(data.x_direction.x);
    e.dwg_x_direction = Some(data.x_direction);
    e.line_spacing_factor = data.linespacing_factor;
    e.line_spacing_style = crate::entities::LineSpacingStyle::from(data.linespacing_style);
    e.background_fill_flags = data.background_flags;
    e.background_scale = data.background_scale;
    e.background_color = data.background_color;
    e.background_transparency = data.background_transparency;
    e.is_annotative = data.is_annotative;
    e.column_data = MTextColumnData {
        column_type: data.column_type,
        column_count: data.column_count,
        flow_reversed: data.column_flow_reversed,
        auto_height: data.column_auto_height,
        width: data.column_width,
        gutter: data.column_gutter,
        heights: data.column_heights,
    };
    e.style = maps.style_name(data.style_handle);
    e
}

/// Builds a `CadDocument` from parsed DWG object data.
pub struct DwgDocumentBuilder {
    obj_reader: DwgObjectReader,
    /// Whether to use failsafe mode (report skipped records via notifications).
    failsafe: bool,
    /// Notifications collected during building.
    notifications: NotificationCollection,
    /// Shared with `DwgReader`; reports the object-build portion of the
    /// reader's 0..=1000 progress range.
    progress: Option<std::sync::Arc<dyn Fn(u16) + Send + Sync>>,
}

impl DwgDocumentBuilder {
    /// Create a new builder wrapping the object reader.
    pub fn new(obj_reader: DwgObjectReader) -> Self {
        Self {
            obj_reader,
            failsafe: false,
            notifications: NotificationCollection::new(),
            progress: None,
        }
    }

    /// Enable or disable failsafe mode.
    ///
    /// When enabled, skipped records are reported as notifications
    /// instead of being silently lost.
    pub fn set_failsafe(&mut self, failsafe: bool) {
        self.failsafe = failsafe;
    }

    pub fn set_progress_callback(&mut self, progress: std::sync::Arc<dyn Fn(u16) + Send + Sync>) {
        self.progress = Some(progress);
    }

    #[inline]
    fn report_progress(&self, value: u16) {
        if let Some(progress) = &self.progress {
            progress(value.min(1000));
        }
    }

    /// Build the document by iterating all handles and dispatching objects.
    ///
    /// Uses a two-pass approach:
    /// 1. Read table entries → build handle→name maps
    /// 2. Read entities and objects → resolve handle references
    ///
    /// Returns collected notifications (skipped records, warnings).
    pub fn build(self, document: &mut CadDocument) -> NotificationCollection {
        self.build_with_stats(document).notifications
    }

    pub fn build_with_stats(mut self, document: &mut CadDocument) -> DwgBuildOutcome {
        let perf = std::env::var_os("PERF").is_some();
        let build_started = web_time::Instant::now();
        document
            .vx_table
            .set_handle(document.header.vpent_hdr_control_handle);
        document.vx_table.clear();
        document.vx_control_entries.clear();
        let mut handles = self.obj_reader.handles();
        // Sort handles numerically so that entity records are processed in
        // allocation order.  This ensures polyline vertex records are
        // encountered in the correct sequence (the writer allocates
        // sequential handles for child entities).
        handles.sort_unstable();
        let mut skipped_pass1 = 0u32;
        let mut skipped_pass2 = 0u32;
        let mut decoded_pass2 = 0usize;
        let mut diagnostics = Vec::new();
        let total_handles = handles.len();
        self.report_progress(55);

        // Build class_number → internal type code mapping for non-fixed types.
        // The DWG binary uses class numbers (500+) for object types defined in
        // the CLASSES section.  We translate these to our internal OBJ_*
        // constants so the match statements work correctly.
        let class_map = Self::build_class_type_map(document);

        // Build a set of class numbers that represent graphical entities
        // (as opposed to non-entity objects).  Used in Pass 2 to correctly
        // classify unresolved class-based types (≥500) that aren't in
        // dxf_name_to_type_code.
        let entity_class_numbers: std::collections::HashSet<i16> = document
            .classes
            .iter()
            .filter(|c| c.is_an_entity && c.class_number >= 500)
            .map(|c| c.class_number)
            .collect();
        let class_names = ClassNames::from_document(document);

        // ── Pass 1: Build handle→name maps from table entries ──────────
        //
        // In addition to building handle→name lookup maps (for Pass 2
        // entity resolution), we now also create full domain objects
        // (Layer, BlockRecord, TextStyle, LineType, DimStyle) and
        // populate the document tables.  This mirrors what the DXF
        // reader does in its TABLES section reader.
        let mut maps = HandleMaps::new();

        // Parsed table entries collected for post-loop domain-object creation.
        // We collect first and create domain objects after the loop so that
        // cross-references (e.g. layer → linetype name) can be resolved
        // using the fully-populated handle→name maps.
        enum ParsedEntry {
            Layer(u64, tables::LayerData),
            Block(u64, tables::BlockHeaderData),
            Style(u64, tables::TextStyleData),
            Ltype(u64, tables::LinetypeData),
            DimStyle(u64, tables::DimStyleData),
            View(u64, tables::ViewData),
            Ucs(u64, tables::UcsData),
            VPort(u64, tables::VPortData),
            AppId(u64, tables::AppIdData),
            Vx(u64, tables::VxTableRecordData),
            /// BLOCK_CONTROL hard-owner refs: (model_space_handle, paper_space_handle).
            /// These are the authoritative active model/paper space designation —
            /// the file header's block handles are unreliable on some versions.
            BlockControl(u64, u64),
            VxControl(Vec<u64>),
        }
        let mut parsed_entries: Vec<ParsedEntry> = Vec::new();
        let catalog_started = web_time::Instant::now();
        enum CatalogFailure {
            MissingOffset,
            NegativeOffset(i64),
            RecordType,
        }
        type CatalogResult =
            std::result::Result<(u64, usize, i16, i16), (u64, Option<u64>, CatalogFailure)>;
        let catalog_results: Vec<CatalogResult> = filter_map_slice(&handles, |&handle| {
            let Some(offset) = self.obj_reader.offset_for(handle) else {
                return Some(Err((handle, None, CatalogFailure::MissingOffset)));
            };
            if offset < 0 {
                return Some(Err((handle, None, CatalogFailure::NegativeOffset(offset))));
            }
            let source_offset = offset as usize;
            let raw = match self.obj_reader.type_code_at(source_offset) {
                Ok(raw) => raw,
                Err(_error) => {
                    return Some(Err((
                        handle,
                        Some(source_offset as u64),
                        CatalogFailure::RecordType,
                    )));
                }
            };
            Some(Ok((
                handle,
                source_offset,
                raw,
                Self::resolve_type_code(raw, &class_map),
            )))
        });
        let mut record_catalog = Vec::with_capacity(catalog_results.len());
        let mut skipped_catalog = 0usize;
        for result in catalog_results {
            match result {
                Ok(record) => record_catalog.push(record),
                Err((handle, source_offset, failure)) => {
                    skipped_catalog = skipped_catalog.saturating_add(1);
                    let message = match failure {
                        CatalogFailure::MissingOffset => {
                            format!("No source offset for handle {handle:#X}")
                        }
                        CatalogFailure::NegativeOffset(offset) => {
                            format!("Negative source offset {offset} for handle {handle:#X}")
                        }
                        CatalogFailure::RecordType => {
                            format!("Could not read record type at handle {handle:#X}")
                        }
                    };
                    self.notifications
                        .notify(NotificationType::Error, message.clone());
                    let mut diagnostic = ReadDiagnostic::new(
                        "record-catalog-failed",
                        ReadStage::RecordStream,
                        message,
                    );
                    diagnostic.source_offset = source_offset;
                    diagnostic.source_offset_basis = Some("object-section-byte".to_string());
                    diagnostic.section = Some("AcDb:AcDbObjects".to_string());
                    diagnostic.record_handle = Some(handle);
                    push_read_diagnostic(&mut diagnostics, diagnostic);
                }
            }
        }
        self.report_progress(75);
        if perf {
            eprintln!(
                "[perf] dwg-build catalog={:.1}ms records={}",
                catalog_started.elapsed().as_secs_f64() * 1000.0,
                record_catalog.len(),
            );
        }
        self.report_progress(110);
        let pass1_started = web_time::Instant::now();

        for &(handle, offset, _, type_code) in &record_catalog {
            if is_table_type(type_code) {
                let (_, mut reader) = match self.obj_reader.read_record_at(offset) {
                    Ok(record) => record,
                    Err(error) => {
                        skipped_pass1 += 1;
                        let message = format!(
                            "Could not read table record at handle {:#X}: {}",
                            handle, error
                        );
                        self.notifications
                            .notify(NotificationType::Error, message.clone());
                        let mut diagnostic = ReadDiagnostic::new(
                            "record-read-failed",
                            ReadStage::RecordStream,
                            message,
                        );
                        diagnostic.source_offset = Some(offset as u64);
                        diagnostic.source_offset_basis = Some("object-section-byte".to_string());
                        diagnostic.section = Some("AcDb:AcDbObjects".to_string());
                        diagnostic.record_handle = Some(handle);
                        diagnostic.record_type = Some(type_code.to_string());
                        push_read_diagnostic(&mut diagnostics, diagnostic);
                        continue;
                    }
                };
                // Wrap in catch_unwind to survive corrupt/misaligned records
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let non_entity = self
                        .obj_reader
                        .read_common_non_entity_data(&mut reader, type_code);
                    let obj_handle = non_entity.common.handle;
                    let eed_raw = non_entity.common.eed_raw;
                    let xdic = non_entity.xdictionary_handle;
                    let reactors = non_entity.reactors.clone();
                    (obj_handle, type_code, eed_raw, xdic, reactors)
                }));
                let (obj_handle, type_code, eed_raw_pass1, xdic_pass1, reactors_pass1) =
                    match result {
                        Ok(v) => v,
                        Err(_) => {
                            skipped_pass1 += 1;
                            let message = format!(
                                "Skipped corrupt table record at handle {:#X} (panic in common data)",
                                handle
                            );
                            self.notifications
                                .notify(NotificationType::Error, message.clone());
                            let mut diagnostic = ReadDiagnostic::new(
                                "record-decode-panicked",
                                ReadStage::RecordStream,
                                message,
                            );
                            diagnostic.source_offset = Some(offset as u64);
                            diagnostic.source_offset_basis =
                                Some("object-section-byte".to_string());
                            diagnostic.section = Some("AcDb:AcDbObjects".to_string());
                            diagnostic.record_handle = Some(handle);
                            diagnostic.record_type = Some(type_code.to_string());
                            push_read_diagnostic(&mut diagnostics, diagnostic);
                            continue;
                        }
                    };
                // Table<T> starts with synthetic low control handles. Replace
                // them with the actual control-object handles from this DWG
                // before writing the document back. Otherwise a real record
                // such as *Model_Space can share a synthetic handle (observed:
                // model block 0x2 vs default LAYER_CONTROL 0x2); the writer's
                // handle map then keeps only one record and the saved drawing
                // reopens with an empty model space.
                let control_handle = Handle::from(obj_handle);
                match type_code {
                    OBJ_BLOCK_CONTROL => {
                        document.block_records.set_handle(control_handle);
                        document.header.block_control_handle = control_handle;
                    }
                    OBJ_LAYER_CONTROL => {
                        document.layers.set_handle(control_handle);
                        document.header.layer_control_handle = control_handle;
                    }
                    OBJ_STYLE_CONTROL => {
                        document.text_styles.set_handle(control_handle);
                        document.header.style_control_handle = control_handle;
                    }
                    OBJ_LTYPE_CONTROL => {
                        document.line_types.set_handle(control_handle);
                        document.header.linetype_control_handle = control_handle;
                    }
                    OBJ_VIEW_CONTROL => {
                        document.views.set_handle(control_handle);
                        document.header.view_control_handle = control_handle;
                    }
                    OBJ_UCS_CONTROL => {
                        document.ucss.set_handle(control_handle);
                        document.header.ucs_control_handle = control_handle;
                    }
                    OBJ_VPORT_CONTROL => {
                        document.vports.set_handle(control_handle);
                        document.header.vport_control_handle = control_handle;
                    }
                    OBJ_APPID_CONTROL => {
                        document.app_ids.set_handle(control_handle);
                        document.header.appid_control_handle = control_handle;
                    }
                    OBJ_DIMSTYLE_CONTROL => {
                        document.dim_styles.set_handle(control_handle);
                        document.header.dimstyle_control_handle = control_handle;
                    }
                    OBJ_VPENT_HDR_CONTROL => {
                        document.vx_table.set_handle(control_handle);
                        document.header.vpent_hdr_control_handle = control_handle;
                    }
                    _ => {}
                }
                // Save EED for DWG round-trip write-back
                if !eed_raw_pass1.is_empty() {
                    document
                        .eed_by_handle
                        .insert(Handle::from(obj_handle), eed_raw_pass1);
                }
                // Save xdictionary handle for DWG round-trip write-back
                if let Some(xdic) = xdic_pass1 {
                    document
                        .xdic_by_handle
                        .insert(Handle::from(obj_handle), Handle::from(xdic));
                }
                // Save reactors for DWG round-trip write-back
                if !reactors_pass1.is_empty() {
                    document.reactors_by_handle.insert(
                        Handle::from(obj_handle),
                        reactors_pass1.iter().map(|&h| Handle::from(h)).collect(),
                    );
                }

                let table_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    match type_code {
                        OBJ_LAYER => {
                            let mut data = tables::read_layer(
                                &mut reader,
                                self.obj_reader.version(),
                                self.obj_reader.dxf_version(),
                            );
                            if let Some(base) = viewport_override_base_layer(&data.name) {
                                data.name = base.to_owned();
                            }
                            Some(ParsedEntry::Layer(obj_handle, data))
                        }
                        OBJ_BLOCK_HEADER => {
                            let data =
                                tables::read_block_header(&mut reader, self.obj_reader.version());
                            Some(ParsedEntry::Block(obj_handle, data))
                        }
                        OBJ_BLOCK_CONTROL => {
                            // Capture the authoritative *Model_Space / *Paper_Space
                            // designation (hard-owner refs) so block-name dedup can
                            // keep the canonical names on the correct records.
                            let data = tables::read_block_control(&mut reader);
                            Some(ParsedEntry::BlockControl(
                                data.model_space_handle,
                                data.paper_space_handle,
                            ))
                        }
                        OBJ_STYLE => {
                            let data =
                                tables::read_text_style(&mut reader, self.obj_reader.version());
                            Some(ParsedEntry::Style(obj_handle, data))
                        }
                        OBJ_LTYPE => {
                            let data =
                                tables::read_linetype(&mut reader, self.obj_reader.version());
                            Some(ParsedEntry::Ltype(obj_handle, data))
                        }
                        OBJ_DIMSTYLE => {
                            let data = tables::read_dimstyle(
                                &mut reader,
                                self.obj_reader.version(),
                                self.obj_reader.dxf_version(),
                            );
                            Some(ParsedEntry::DimStyle(obj_handle, data))
                        }
                        OBJ_VIEW => {
                            let data = tables::read_view(&mut reader, self.obj_reader.version());
                            Some(ParsedEntry::View(obj_handle, data))
                        }
                        OBJ_UCS => {
                            let data = tables::read_ucs(&mut reader, self.obj_reader.version());
                            Some(ParsedEntry::Ucs(obj_handle, data))
                        }
                        OBJ_VPORT => {
                            let data = tables::read_vport(&mut reader, self.obj_reader.version());
                            Some(ParsedEntry::VPort(obj_handle, data))
                        }
                        OBJ_APPID => {
                            let data = tables::read_appid(&mut reader, self.obj_reader.version());
                            Some(ParsedEntry::AppId(obj_handle, data))
                        }
                        OBJ_VPENT_HDR_CONTROL => {
                            let data = tables::read_vx_control(&mut reader);
                            Some(ParsedEntry::VxControl(data.entry_handles))
                        }
                        OBJ_VPENT_HDR => {
                            let data = tables::read_vx_table_record(&mut reader);
                            Some(ParsedEntry::Vx(obj_handle, data))
                        }
                        _ => None,
                    }
                }));
                match table_result {
                    Ok(Some(entry)) => {
                        // Populate handle→name maps (needed by Pass 2)
                        match &entry {
                            ParsedEntry::Layer(h, data) => {
                                maps.layers.insert(*h, data.name.clone());
                            }
                            ParsedEntry::Block(h, data) => {
                                maps.blocks.insert(*h, data.name.clone());
                            }
                            ParsedEntry::Style(h, data) => {
                                maps.text_styles.insert(*h, data.name.clone());
                            }
                            ParsedEntry::Ltype(h, data) => {
                                maps.linetypes.insert(*h, data.name.clone());
                                // Ordered list for pre-R2018 MLINESTYLE index
                                // resolution — the special ByBlock/ByLayer are
                                // not part of the linetype index space.
                                if !data.name.eq_ignore_ascii_case("ByBlock")
                                    && !data.name.eq_ignore_ascii_case("ByLayer")
                                {
                                    maps.linetype_order.push(data.name.clone());
                                }
                            }
                            ParsedEntry::DimStyle(h, data) => {
                                maps.dim_styles.insert(*h, data.name.clone());
                            }
                            ParsedEntry::View(h, data) => {
                                maps.views.insert(*h, data.name.clone());
                            }
                            ParsedEntry::Ucs(_, _) => {}
                            ParsedEntry::VPort(_, _) => {}
                            ParsedEntry::AppId(_, _) => {}
                            ParsedEntry::Vx(_, _) => {}
                            ParsedEntry::BlockControl(m, p) => {
                                // Seed the authoritative active model/paper space
                                // handles (used by the block-name dedup below).
                                if *m != 0 {
                                    document.header.model_space_block_handle = Handle::from(*m);
                                }
                                if *p != 0 {
                                    document.header.paper_space_block_handle = Handle::from(*p);
                                }
                            }
                            ParsedEntry::VxControl(handles) => {
                                document.vx_control_entries =
                                    handles.iter().copied().map(Handle::from).collect();
                            }
                        }
                        // The block control is not a table record — don't store it.
                        if !matches!(
                            entry,
                            ParsedEntry::BlockControl(..) | ParsedEntry::VxControl(..)
                        ) {
                            parsed_entries.push(entry);
                        }
                    }
                    Ok(None) => {}
                    Err(_) => {
                        skipped_pass1 += 1;
                        let message = format!(
                            "Skipped corrupt table record at handle {:#X}, type_code={}",
                            handle, type_code
                        );
                        self.notifications
                            .notify(NotificationType::Error, message.clone());
                        let mut diagnostic = ReadDiagnostic::new(
                            "record-decode-failed",
                            ReadStage::RecordStream,
                            message,
                        );
                        diagnostic.source_offset = Some(offset as u64);
                        diagnostic.source_offset_basis = Some("object-section-byte".to_string());
                        diagnostic.section = Some("AcDb:AcDbObjects".to_string());
                        diagnostic.record_handle = Some(handle);
                        diagnostic.record_type = Some(type_code.to_string());
                        push_read_diagnostic(&mut diagnostics, diagnostic);
                    }
                }
            }
        }

        // ── Deduplicate block names ────────────────────────────────────
        //
        // DWG binary format stores ALL paper-space blocks as "*Paper_Space"
        // and anonymous blocks share names ("*U", "*D", etc.).  Our
        // Table<BlockRecord> is keyed by name, so duplicates would
        // overwrite each other.  Rename duplicates using the DXF
        // convention: *Paper_Space, *Paper_Space0, *Paper_Space1, …
        //
        // The header's model_space_block_handle / paper_space_block_handle
        // (read from the DWG file header before this function) identify
        // the "active" model/paper space blocks, which keep their
        // canonical names.
        {
            let active_model = document.header.model_space_block_handle;
            let active_paper = document.header.paper_space_block_handle;

            // Collect (index, handle, base_name) for all Block entries
            let block_info: Vec<(usize, u64, String)> = parsed_entries
                .iter()
                .enumerate()
                .filter_map(|(idx, e)| {
                    if let ParsedEntry::Block(h, data) = e {
                        Some((idx, *h, data.name.clone()))
                    } else {
                        None
                    }
                })
                .collect();

            // Group by name
            let mut name_groups: std::collections::HashMap<String, Vec<(usize, u64)>> =
                std::collections::HashMap::new();
            for (idx, h, name) in &block_info {
                name_groups
                    .entry(name.clone())
                    .or_default()
                    .push((*idx, *h));
            }

            // Rename duplicates
            for (base_name, entries) in &name_groups {
                if entries.len() <= 1 {
                    continue;
                }
                // Determine which entry keeps the canonical (un-suffixed)
                // name.  Prefer the one matching the header's active
                // model/paper space handle; fall back to the first entry.
                let active_h = if base_name.eq_ignore_ascii_case("*Model_Space") {
                    active_model
                } else if base_name.eq_ignore_ascii_case("*Paper_Space") {
                    active_paper
                } else {
                    Handle::NULL
                };

                let canonical_idx = entries
                    .iter()
                    .find(|(_, h)| !active_h.is_null() && Handle::from(*h) == active_h)
                    .or_else(|| entries.first())
                    .map(|&(idx, _)| idx);

                let mut suffix = 0usize;
                for &(idx, h) in entries {
                    if Some(idx) == canonical_idx {
                        continue; // keep canonical name
                    }
                    let new_name = format!("{}{}", base_name, suffix);
                    if let ParsedEntry::Block(_, ref mut data) = parsed_entries[idx] {
                        data.name = new_name.clone();
                    }
                    maps.blocks.insert(h, new_name);
                    suffix += 1;
                }
            }
        }

        // ── Post-Pass 1: Populate document tables from parsed data ─────
        //
        // Now that all handle→name maps are complete, create domain objects
        // with resolved cross-references and add them to the document.
        //
        // Clear initialisation-defaults for block records first: the
        // defaults (created by CadDocument::new()) use handles 0x15 / 0x18
        // which may collide with objects from the DWG file.
        let _ = document.block_records.remove("*Model_Space");
        let _ = document.block_records.remove("*Paper_Space");

        let layer_transparency_app_handle = parsed_entries.iter().find_map(|entry| match entry {
            ParsedEntry::AppId(handle, data)
                if data.name.eq_ignore_ascii_case("AcCmTransparency") =>
            {
                Some(*handle)
            }
            _ => None,
        });
        let mut cleared_default_vports = false;
        for entry in &parsed_entries {
            match entry {
                ParsedEntry::Layer(h, data) => {
                    let mut layer = crate::tables::Layer::new(&data.name);
                    layer.handle = Handle::from(*h);
                    layer.flags.frozen = data.frozen;
                    layer.flags.off = data.off;
                    layer.flags.frozen_in_new_viewport = data.frozen_in_new_vp;
                    layer.flags.locked = data.locked;
                    layer.flags.xref_dependent = data.xref_dependent;
                    layer.is_plottable = data.plottable;
                    layer.line_weight = LineWeight::from_value(data.line_weight);
                    layer.color = data.color;
                    layer.color_name.clone_from(&data.color_name);
                    layer.book_name.clone_from(&data.book_name);
                    if let Some(app_handle) = layer_transparency_app_handle {
                        if let Some(bytes) =
                            document
                                .eed_by_handle
                                .get(&Handle::from(*h))
                                .and_then(|blocks| {
                                    blocks
                                        .iter()
                                        .find(|(handle, _)| *handle == app_handle)
                                        .map(|(_, bytes)| bytes.as_slice())
                                })
                        {
                            if bytes.first() == Some(&71) && bytes.len() >= 5 {
                                let raw =
                                    i32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
                                layer.transparency =
                                    crate::types::Transparency::from_alpha_value(raw as u32);
                            }
                        }
                    }
                    // Resolve linetype handle → name
                    layer.line_type = maps
                        .linetypes
                        .get(&data.linetype_handle)
                        .cloned()
                        .unwrap_or_else(|| "Continuous".to_string());
                    // Material handle
                    if let Some(mh) = data.material_handle {
                        layer.material = Handle::from(mh);
                    }
                    // Plotstyle handle (R2000+)
                    if let Some(ph) = data.plotstyle_handle {
                        layer.plotstyle_handle = Handle::from(ph);
                    }
                    // External reference block record handle
                    if data.xref_handle != 0 {
                        layer.xref_block_record_handle = Handle::from(data.xref_handle);
                    }
                    // Remove default entry if it exists, then add
                    let _ = document.layers.remove(&data.name);
                    let _ = document.layers.add(layer);
                }
                ParsedEntry::Block(h, data) => {
                    let mut br = crate::tables::BlockRecord::new(&data.name);
                    br.handle = Handle::from(*h);
                    br.flags.anonymous = data.anonymous;
                    br.flags.has_attributes = data.has_attributes;
                    br.flags.is_xref = data.is_xref;
                    br.flags.is_xref_overlay = data.is_xref_overlay;
                    br.block_entity_handle = Handle::from(data.block_entity_handle);
                    br.block_end_handle = Handle::from(data.endblk_handle);
                    br.units = data.units.unwrap_or(0);
                    br.explodable = data.explodable.unwrap_or(true);
                    br.scale_uniformly = data.scale_uniformly.map(|v| v != 0).unwrap_or(false);
                    br.xref_path = data.xref_path.clone();
                    br.description = data.description.clone().unwrap_or_default();
                    br.insert_count_bytes = data.insert_count_bytes.clone();
                    br.preview_data = data.preview_data.clone();
                    br.insert_handles = data
                        .insert_handles
                        .iter()
                        .map(|&h| Handle::from(h))
                        .collect();
                    br.base_point = data.base_point;
                    if let Some(layout_h) = data.layout_handle {
                        br.layout = Handle::from(layout_h);
                    }
                    // Update header handles for model/paper space
                    // (uses the deduplicated name, so only the active block
                    // with the canonical name "*Model_Space" / "*Paper_Space"
                    // sets the header handle)
                    if data.name.eq_ignore_ascii_case("*Model_Space") {
                        document.header.model_space_block_handle = br.handle;
                    } else if data.name.eq_ignore_ascii_case("*Paper_Space") {
                        document.header.paper_space_block_handle = br.handle;
                    }
                    // Remove default entry if it exists, then add
                    let _ = document.block_records.remove(&data.name);
                    let _ = document.block_records.add(br);
                }
                ParsedEntry::Style(h, data) => {
                    let mut style = crate::tables::TextStyle::new(&data.name);
                    style.handle = Handle::from(*h);
                    style.height = data.height;
                    style.width_factor = data.width_factor;
                    style.oblique_angle = data.oblique_angle;
                    style.last_height = data.last_height;
                    style.font_file = data.font_file.clone();
                    style.big_font_file = data.big_font_file.clone();
                    style.is_shape_file = data.is_shape_file;
                    style.is_vertical = data.is_vertical;
                    style.flags.backward = (data.generation & 2) != 0;
                    style.flags.upside_down = (data.generation & 4) != 0;
                    // Only mark xref-dependent if the xref block record handle is valid
                    style.xref_dependent = data.xref_dependent && data.xref_handle != 0;
                    // Use add_allow_duplicate for shape-file-only styles (empty name)
                    // so multiple empty-named styles are preserved. Named styles use
                    // add_or_replace to avoid duplicates (e.g. "Standard").
                    if data.name.is_empty() {
                        document.text_styles.add_allow_duplicate(style);
                    } else {
                        document.text_styles.add_or_replace(style);
                    }
                }
                ParsedEntry::Ltype(h, data) => {
                    let mut lt = crate::tables::LineType::new(&data.name);
                    lt.handle = Handle::from(*h);
                    lt.description = data.description.clone();
                    lt.pattern_length = data.pattern_length;
                    lt.xref_dependent = data.xref_dependent;
                    lt.elements = data
                        .segments
                        .iter()
                        .zip(data.shape_handles.iter().chain(std::iter::repeat(&0u64)))
                        .map(|(s, &sh)| {
                            use crate::tables::linetype::{
                                LineTypeComplexContent, LineTypeComplexData, LineTypeElement,
                            };
                            let is_complex = s.dwg_flags != 0
                                || s.offset_x.abs() > 1e-12
                                || s.offset_y.abs() > 1e-12
                                || (s.scale - 1.0).abs() > 1e-12
                                || s.rotation.abs() > 1e-12
                                || s.shape_number != 0
                                || sh != 0;
                            let complex = if is_complex {
                                let content = if s.dwg_flags & 0x02 != 0 {
                                    LineTypeComplexContent::Text {
                                        text: s.text.clone(),
                                    }
                                } else {
                                    LineTypeComplexContent::Shape {
                                        shape_number: s.shape_number,
                                    }
                                };
                                Some(LineTypeComplexData {
                                    content,
                                    style_handle: Handle::from(sh),
                                    scale: s.scale,
                                    rotation: s.rotation,
                                    absolute_rotation: s.dwg_flags & 0x01 != 0,
                                    offset: [s.offset_x, s.offset_y],
                                })
                            } else {
                                None
                            };
                            LineTypeElement {
                                length: s.length,
                                complex,
                            }
                        })
                        .collect();
                    let _ = document.line_types.remove(&data.name);
                    let _ = document.line_types.add(lt);
                }
                ParsedEntry::DimStyle(h, data) => {
                    let mut ds = crate::tables::DimStyle::new(&data.name);
                    ds.handle = Handle::from(*h);
                    ds.xref_reference = data.xref_reference;
                    ds.xref_resolved = data.xref_resolved;
                    ds.xref_dependent = data.xref_dependent;
                    ds.xref_handle = Handle::from(data.xref_handle);
                    ds.dimscale = data.dimscale;
                    ds.dimasz = data.dimasz;
                    ds.dimexo = data.dimexo;
                    ds.dimdli = data.dimdli;
                    ds.dimexe = data.dimexe;
                    ds.dimrnd = data.dimrnd;
                    ds.dimdle = data.dimdle;
                    ds.dimtp = data.dimtp;
                    ds.dimtm = data.dimtm;
                    ds.dimtol = data.dimtol;
                    ds.dimlim = data.dimlim;
                    ds.dimtih = data.dimtih;
                    ds.dimtoh = data.dimtoh;
                    ds.dimse1 = data.dimse1;
                    ds.dimse2 = data.dimse2;
                    ds.dimtad = data.dimtad;
                    ds.dimzin = data.dimzin;
                    ds.dimazin = data.dimazin;
                    ds.dimtxt = data.dimtxt;
                    ds.dimcen = data.dimcen;
                    ds.dimtsz = data.dimtsz;
                    ds.dimaltf = data.dimaltf;
                    ds.dimlfac = data.dimlfac;
                    ds.dimtvp = data.dimtvp;
                    ds.dimtfac = data.dimtfac;
                    ds.dimgap = data.dimgap;
                    ds.dimalt = data.dimalt;
                    ds.dimaltd = data.dimaltd;
                    ds.dimtofl = data.dimtofl;
                    ds.dimsah = data.dimsah;
                    ds.dimtix = data.dimtix;
                    ds.dimsoxd = data.dimsoxd;
                    ds.dimclrd = data.dimclrd.approximate_index();
                    ds.dimclre = data.dimclre.approximate_index();
                    ds.dimclrt = data.dimclrt.approximate_index();
                    ds.dimclrd_true_color = data.dimclrd.is_true_color().then_some(data.dimclrd);
                    ds.dimclre_true_color = data.dimclre.is_true_color().then_some(data.dimclre);
                    ds.dimclrt_true_color = data.dimclrt.is_true_color().then_some(data.dimclrt);
                    ds.dimsd1 = data.dimsd1;
                    ds.dimsd2 = data.dimsd2;
                    ds.dimtolj = data.dimtolj;
                    ds.dimtzin = data.dimtzin;
                    ds.dimupt = data.dimupt;
                    ds.dimfit = data.dimfit;
                    ds.dimatfit = data.dimatfit;
                    ds.dimunit = data.dimunit;
                    ds.dimlwd = data.dimlwd;
                    ds.dimlwe = data.dimlwe;
                    ds.dimpost = data.dimpost.clone();
                    ds.dimapost = data.dimapost.clone();
                    ds.dimaltrnd = data.dimaltrnd;
                    ds.dimadec = data.dimadec;
                    ds.dimdec = data.dimdec;
                    ds.dimtdec = data.dimtdec;
                    ds.dimaltu = data.dimaltu;
                    ds.dimalttd = data.dimalttd;
                    ds.dimaunit = data.dimaunit;
                    ds.dimfrac = data.dimfrac;
                    ds.dimlunit = data.dimlunit;
                    ds.dimdsep = data.dimdsep;
                    ds.dimtmove = data.dimtmove;
                    ds.dimjust = data.dimjust;
                    ds.dimaltz = data.dimaltz;
                    ds.dimalttz = data.dimalttz;
                    // R2007+ fields
                    ds.dimfxl = data.dimfxl;
                    ds.dimjogang = data.dimjogang;
                    ds.dimtfill = data.dimtfill;
                    ds.dimtfillclr = data.dimtfillclr.approximate_index();
                    ds.dimtfillclr_true_color =
                        data.dimtfillclr.is_true_color().then_some(data.dimtfillclr);
                    ds.dimarcsym = data.dimarcsym;
                    ds.dimfxlon = data.dimfxlon;
                    ds.dimtxtdirection = data.dimtxtdirection;
                    ds.dimaltmzf = data.dimaltmzf;
                    ds.dimaltmzs = data.dimaltmzs.clone();
                    ds.dimmzf = data.dimmzf;
                    ds.dimmzs = data.dimmzs.clone();
                    ds.dimblk_name = data.dimblk_name.clone();
                    ds.dimblk1_name = data.dimblk1_name.clone();
                    ds.dimblk2_name = data.dimblk2_name.clone();
                    // Resolve text style handle
                    if data.dimtxsty_handle != 0 {
                        ds.dimtxsty_handle = Handle::from(data.dimtxsty_handle);
                        ds.dimtxsty = maps
                            .text_styles
                            .get(&data.dimtxsty_handle)
                            .cloned()
                            .unwrap_or_else(|| "Standard".to_string());
                    }
                    // R2000+ block handles
                    if let Some(h) = data.dimldrblk_handle {
                        ds.dimldrblk = Handle::from(h);
                    }
                    if let Some(h) = data.dimblk_handle {
                        ds.dimblk = Handle::from(h);
                    }
                    if let Some(h) = data.dimblk1_handle {
                        ds.dimblk1 = Handle::from(h);
                    }
                    if let Some(h) = data.dimblk2_handle {
                        ds.dimblk2 = Handle::from(h);
                    }
                    // R2007+ linetype handles
                    if data.dimltype_handle != 0 {
                        ds.dimltex_handle = Handle::from(data.dimltype_handle);
                    }
                    if data.dimltex1_handle != 0 {
                        ds.dimltex1_handle = Handle::from(data.dimltex1_handle);
                    }
                    if data.dimltex2_handle != 0 {
                        ds.dimltex2_handle = Handle::from(data.dimltex2_handle);
                    }
                    let _ = document.dim_styles.remove(&data.name);
                    let _ = document.dim_styles.add(ds);
                }
                ParsedEntry::View(h, data) => {
                    let mut view = crate::tables::View::new(&data.name);
                    view.handle = Handle::from(*h);
                    view.height = data.height;
                    view.width = data.width;
                    view.center = crate::types::Vector3::new(data.center.x, data.center.y, 0.0);
                    view.target = data.target;
                    view.direction = data.direction;
                    view.twist_angle = data.twist_angle;
                    view.lens_length = data.lens_length;
                    view.front_clip = data.front_clip;
                    view.back_clip = data.back_clip;
                    view.perspective = data.perspective;
                    view.front_clipping = data.front_clipping;
                    view.back_clipping = data.back_clipping;
                    view.front_clip_at_eye = data.front_clip_z;
                    view.render_mode =
                        ViewportRenderMode::from_value(data.render_mode.unwrap_or(0) as i16);
                    view.use_default_lights = data.use_default_lights;
                    view.default_lighting_type = data.default_lighting_type as i16;
                    view.brightness = data.brightness;
                    view.contrast = data.contrast;
                    view.ambient_color = data.ambient_color.clone();
                    view.paper_space = data.paper_space;
                    view.ucs_associated = data.ucs_associated;
                    view.ucs_origin = data.ucs_origin;
                    view.ucs_x_axis = data.ucs_x_axis;
                    view.ucs_y_axis = data.ucs_y_axis;
                    view.ucs_elevation = data.ucs_elevation;
                    view.ucs_ortho_type = data.ucs_ortho_type;
                    view.camera_plottable = data.camera_plottable;
                    view.xref_reference = data.xref_reference;
                    view.xref_resolved = data.xref_resolved;
                    view.xref_dependent = data.xref_dependent;
                    view.xref_handle = Handle::from(data.xref_handle);
                    view.background_handle = Handle::from(data.background_handle);
                    view.live_section_handle = Handle::from(data.live_section_handle);
                    view.visual_style_handle = Handle::from(data.visual_style_handle);
                    view.sun_handle = Handle::from(data.sun_handle);
                    view.named_ucs_handle = Handle::from(data.named_ucs_handle);
                    view.base_ucs_handle = Handle::from(data.base_ucs_handle);
                    let _ = document.views.remove(&data.name);
                    let _ = document.views.add(view);
                }
                ParsedEntry::Ucs(h, data) => {
                    let mut ucs = crate::tables::Ucs::new(&data.name);
                    ucs.handle = Handle::from(*h);
                    ucs.origin = data.origin;
                    ucs.x_axis = data.x_axis;
                    ucs.y_axis = data.y_axis;
                    ucs.elevation = data.elevation.unwrap_or(0.0);
                    ucs.ortho_view_type = data.ortho_view_type.unwrap_or(0);
                    ucs.ortho_type = data.ortho_type.unwrap_or(0);
                    ucs.named_ucs_handle = Handle::from(data.named_ucs_handle);
                    ucs.base_ucs_handle = Handle::from(data.base_ucs_handle);
                    ucs.xref_reference = data.xref_reference;
                    ucs.xref_resolved = data.xref_resolved;
                    ucs.xref_dependent = data.xref_dependent;
                    ucs.xref_handle = Handle::from(data.xref_handle);
                    let _ = document.ucss.remove(&data.name);
                    let _ = document.ucss.add(ucs);
                }
                ParsedEntry::VPort(h, data) => {
                    if !cleared_default_vports {
                        document.vports.clear();
                        cleared_default_vports = true;
                    }
                    let mut vp = crate::tables::VPort::new(&data.name);
                    vp.handle = Handle::from(*h);
                    vp.lower_left = data.lower_left;
                    vp.upper_right = data.upper_right;
                    vp.view_center = data.view_center;
                    vp.snap_base = data.snap_base;
                    vp.snap_spacing = data.snap_spacing;
                    vp.grid_spacing = data.grid_spacing;
                    vp.view_direction = data.view_direction;
                    vp.view_target = data.view_target;
                    vp.view_height = data.view_height;
                    vp.aspect_ratio = if data.view_height.abs() > 1e-10 {
                        data.aspect_ratio_times_height / data.view_height
                    } else {
                        1.0
                    };
                    vp.lens_length = data.lens_length;
                    vp.view_twist = data.view_twist;
                    vp.front_clip = data.front_clip;
                    vp.back_clip = data.back_clip;
                    vp.perspective = data.perspective;
                    vp.front_clipping = data.front_clipping;
                    vp.back_clipping = data.back_clipping;
                    vp.front_clip_at_eye = data.front_clip_at_eye;
                    vp.ucsfollow = data.ucsfollow;
                    vp.circle_zoom = data.circle_zoom;
                    vp.fast_zoom = data.fast_zoom;
                    vp.ucsicon_lower = data.ucsicon_lower;
                    vp.ucsicon_origin = data.ucsicon_origin;
                    vp.grid_on = data.grid_on;
                    vp.snap_on = data.snap_on;
                    vp.snap_style = data.snap_style;
                    vp.snap_isopair = data.snap_isopair;
                    vp.snap_rotation = data.snap_rotation;
                    vp.render_mode =
                        ViewportRenderMode::from_value(data.render_mode.unwrap_or(0) as i16);
                    vp.use_default_lights = data.use_default_lights;
                    vp.default_lighting_type = data.default_lighting_type as i16;
                    vp.brightness = data.brightness;
                    vp.contrast = data.contrast;
                    vp.ambient_color = data.ambient_color.clone();
                    vp.ucs_at_origin = data.ucs_at_origin;
                    vp.ucs_per_viewport = data.ucs_per_viewport;
                    vp.ucs_origin = data.ucs_origin;
                    vp.ucs_x_axis = data.ucs_x_axis;
                    vp.ucs_y_axis = data.ucs_y_axis;
                    vp.ucs_elevation = data.ucs_elevation;
                    vp.ucs_ortho_type = data.ucs_ortho_type;
                    vp.grid_flags = crate::entities::GridFlags::from_bits(data.grid_flags);
                    vp.grid_major = data.grid_major;
                    vp.xref_reference = data.xref_reference;
                    vp.xref_resolved = data.xref_resolved;
                    vp.xref_dependent = data.xref_dependent;
                    vp.xref_handle = Handle::from(data.xref_handle);
                    vp.background_handle = Handle::from(data.background_handle);
                    vp.visual_style_handle = Handle::from(data.visual_style_handle);
                    vp.sun_handle = Handle::from(data.sun_handle);
                    vp.named_ucs_handle = Handle::from(data.named_ucs_handle);
                    vp.base_ucs_handle = Handle::from(data.base_ucs_handle);
                    document.vports.add_allow_duplicate(vp);
                }
                ParsedEntry::AppId(h, data) => {
                    let mut app = crate::tables::AppId::new(&data.name);
                    app.handle = Handle::from(*h);
                    let _ = document.app_ids.remove(&data.name);
                    let _ = document.app_ids.add(app);
                }
                ParsedEntry::Vx(h, data) => {
                    let mut record = crate::tables::VxTableRecord::new(&data.name);
                    record.handle = Handle::from(*h);
                    record.is_xref_reference = data.is_xref_reference;
                    record.is_xref_resolved = data.is_xref_resolved;
                    record.is_xref_dependent = data.is_xref_dependent;
                    record.xref_handle = Handle::from(data.xref_handle);
                    record.is_on = data.is_on;
                    record.viewport = Handle::from(data.viewport);
                    record.previous_entry = Handle::from(data.previous_entry);
                    record.legacy_viewport_entity_address = data.legacy_viewport_entity_address;
                    record.legacy_viewport_index = data.legacy_viewport_index;
                    record.legacy_previous_entry_index = data.legacy_previous_entry_index;
                    document.vx_table.add_allow_duplicate(record);
                }
                // Block control is consumed during Pass 1 (header seeding); it is
                // never stored as a parsed table entry.
                ParsedEntry::BlockControl(..) | ParsedEntry::VxControl(..) => {}
            }
        }

        // Build a reverse map: entity_handle → block_record_handle
        // from the canonical entity_handles read from the DWG binary
        // (R2004+).  This is needed because entity_mode=1 only says
        // "paper space" without specifying WHICH paper space.
        let mut binary_entity_owner: ahash::AHashMap<Handle, Handle> = ahash::AHashMap::new();
        for entry in &parsed_entries {
            if let ParsedEntry::Block(h, data) = entry {
                let br_handle = Handle::from(*h);
                // Save original entity_handles from the DWG binary for the writer
                let orig_handles: Vec<Handle> = data
                    .entity_handles
                    .iter()
                    .map(|&eh| Handle::from(eh))
                    .collect();
                document
                    .block_entity_handles
                    .insert(br_handle, orig_handles);
                for &eh in &data.entity_handles {
                    binary_entity_owner.insert(Handle::from(eh), br_handle);
                }
            }
        }

        // ── Clear default objects before reading file objects ─────────
        //
        // initialize_defaults() created placeholder dictionaries, layouts,
        // and other objects.  The DWG file supplies its own complete set of
        // objects, so the defaults must be removed to avoid phantom layouts
        // (with stale block_record handles) and orphaned dictionary entries
        // that corrupt the file when written back as DXF.
        document.objects.clear();
        if perf {
            eprintln!(
                "[perf] dwg-build pass1={:.1}ms tables={} blocks={} owner-links={}",
                pass1_started.elapsed().as_secs_f64() * 1000.0,
                parsed_entries.len(),
                document.block_records.len(),
                binary_entity_owner.len(),
            );
        }

        // ── Pass 2: Read entities and non-table objects ────────────────
        let mut pending = PendingPolylines {
            vertices: HashMap::new(),
            seqends: HashMap::new(),
            polylines: Vec::new(),
        };
        // Pending attribute entities keyed by owner (INSERT) handle.
        let mut pending_attributes: HashMap<u64, Vec<AttributeEntity>> = HashMap::new();
        let pass2_records: Vec<(u64, usize, i16, i16)> = record_catalog
            .iter()
            .copied()
            .filter(|(_, _, _, type_code)| !is_table_type(*type_code))
            .collect();
        // LIGHT's optional IES/photometric tail is controlled by the
        // LIGHTINGUNITS entry in AcDbVariableDictionary, not by record length.
        // Resolve the dictionary variable before the parallel entity pass so a
        // LIGHT with has_photometric_data=false still consumes its presence bit.
        let mut variable_entries: Vec<(String, u64)> = Vec::new();
        let mut variable_values: HashMap<u64, String> = HashMap::new();
        for &(_, offset, _, type_code) in &pass2_records {
            if type_code != OBJ_DICTIONARY && type_code != OBJ_DICTIONARYVAR {
                continue;
            }
            let (_, mut reader) = match self.obj_reader.read_record_at(offset) {
                Ok(record) => record,
                Err(_) => continue,
            };
            let common = self
                .obj_reader
                .read_common_non_entity_data(&mut reader, type_code);
            if type_code == OBJ_DICTIONARY {
                let data = objects::read_dictionary(&mut reader, self.obj_reader.version());
                variable_entries.extend(
                    data.entries
                        .into_iter()
                        .map(|entry| (entry.name, entry.handle)),
                );
            } else {
                let data = objects::read_dictionary_variable(&mut reader);
                variable_values.insert(common.common.handle, data.value);
            }
        }
        let photometric_lighting = variable_entries.iter().any(|(name, handle)| {
            name.eq_ignore_ascii_case("LIGHTINGUNITS")
                && variable_values
                    .get(handle)
                    .is_some_and(|value| value.trim() == "2")
        });
        document.reserve_loaded_entities(pass2_records.len());
        document.objects.reserve(pass2_records.len().min(16_384));

        let source_version = document.version;
        let model_space_block_handle = document.header.model_space_block_handle;
        let paper_space_block_handle = document.header.paper_space_block_handle;
        let worker_count = worker_count();
        let chunk_size = 512usize;
        let batch_size = chunk_size * worker_count * 4;
        let pass2_started = web_time::Instant::now();
        let mut decode_seconds = 0.0f64;
        let mut commit_seconds = 0.0f64;

        let pass2_total = pass2_records.len().max(1);
        let mut pass2_done = 0usize;
        for batch in pass2_records.chunks(batch_size) {
            let decode_started = web_time::Instant::now();
            let chunks: Vec<Pass2Chunk> = map_chunks(batch, chunk_size, |records| {
                let mut chunk = Pass2Chunk::new(
                    source_version,
                    model_space_block_handle,
                    paper_space_block_handle,
                    records.len(),
                );
                for &(handle, offset, raw_type_code, type_code) in records {
                    let (_, reader) = match self.obj_reader.read_record_at(offset) {
                        Ok(record) => record,
                        Err(error) => {
                            chunk.failures.push(RecordFailure {
                                    code: "record-read-failed",
                                    handle,
                                    offset,
                                    type_code,
                                    message: format!(
                                        "Could not read record at handle {handle:#X}, type_code={type_code}: {error}"
                                    ),
                                });
                            continue;
                        }
                    };
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        self.process_pass2_record(
                            handle,
                            raw_type_code,
                            type_code,
                            reader,
                            &mut chunk.output,
                            &maps,
                            &mut chunk.pending,
                            &mut chunk.pending_attributes,
                            &entity_class_numbers,
                            &class_names,
                            photometric_lighting,
                        );
                    }));
                    if result.is_err() {
                        chunk.failures.push(RecordFailure {
                                code: "record-decode-panicked",
                                handle,
                                offset,
                                type_code,
                                message: format!(
                                    "Skipped corrupt record at handle {handle:#X}, type_code={type_code} (panic recovered)"
                                ),
                            });
                    }
                }
                chunk
            });
            decode_seconds += decode_started.elapsed().as_secs_f64();

            let commit_started = web_time::Instant::now();
            for mut chunk in chunks {
                decoded_pass2 = decoded_pass2
                    .saturating_add(chunk.output.entities.len())
                    .saturating_add(chunk.output.objects.len());
                document
                    .eed_by_handle
                    .extend(chunk.output.eed_by_handle.drain());
                document
                    .xdic_by_handle
                    .extend(chunk.output.xdic_by_handle.drain());
                document
                    .reactors_by_handle
                    .extend(chunk.output.reactors_by_handle.drain());
                document
                    .dwg_data_store_handles
                    .extend(chunk.output.dwg_data_store_handles.drain());
                document
                    .context_scales
                    .extend(chunk.output.context_scales.drain());
                document
                    .block_visibility_params
                    .extend(chunk.output.block_visibility_params.drain());
                document
                    .block_representations
                    .extend(chunk.output.block_representations.drain());
                document.fields.extend(chunk.output.fields.drain());
                document
                    .dgn_ls_definitions
                    .extend(chunk.output.dgn_ls_definitions.drain());
                document
                    .dgn_ls_components
                    .extend(chunk.output.dgn_ls_components.drain());
                document
                    .view_rep_refs
                    .extend(chunk.output.view_rep_refs.drain());
                for view_rep in chunk.output.section_view_reps.drain(..) {
                    if !document.section_view_reps.contains(&view_rep) {
                        document.section_view_reps.push(view_rep);
                    }
                }
                if document.section_view_style.is_none() {
                    document.section_view_style = chunk.output.section_view_style.take();
                }
                document.objects.extend(chunk.output.objects.drain());
                document.add_loaded_entity_batch(&mut chunk.output.entities);
                for (owner, mut vertices) in chunk.pending.vertices.drain() {
                    pending
                        .vertices
                        .entry(owner)
                        .or_default()
                        .append(&mut vertices);
                }
                pending.seqends.extend(chunk.pending.seqends.drain());
                pending.polylines.append(&mut chunk.pending.polylines);
                for (owner, mut attributes) in chunk.pending_attributes.drain() {
                    pending_attributes
                        .entry(owner)
                        .or_default()
                        .append(&mut attributes);
                }
                for failure in chunk.failures {
                    skipped_pass2 += 1;
                    self.notifications
                        .notify(NotificationType::Error, failure.message.clone());
                    let mut diagnostic =
                        ReadDiagnostic::new(failure.code, ReadStage::RecordStream, failure.message);
                    diagnostic.source_offset = Some(failure.offset as u64);
                    diagnostic.source_offset_basis = Some("object-section-byte".to_string());
                    diagnostic.section = Some("AcDb:AcDbObjects".to_string());
                    diagnostic.record_handle = Some(failure.handle);
                    diagnostic.record_type = Some(failure.type_code.to_string());
                    push_read_diagnostic(&mut diagnostics, diagnostic);
                }
            }
            commit_seconds += commit_started.elapsed().as_secs_f64();
            pass2_done = pass2_done.saturating_add(batch.len());
            let value = 110u32 + ((pass2_done as u64 * 760) / pass2_total as u64) as u32;
            self.report_progress(value.min(870) as u16);
        }
        if perf {
            eprintln!(
                "[perf] dwg-build pass2={:.1}ms decode={:.1}ms commit={:.1}ms records={} threads={}",
                pass2_started.elapsed().as_secs_f64() * 1000.0,
                decode_seconds * 1000.0,
                commit_seconds * 1000.0,
                pass2_records.len(),
                worker_count,
            );
        }
        let post_started = web_time::Instant::now();
        self.report_progress(875);

        // ── Post-pass: Assemble polyline vertices and add to document ──
        for (poly_handle, mut entity) in pending.polylines {
            if let Some(verts) = pending.vertices.remove(&poly_handle) {
                match &mut entity {
                    EntityType::Polyline2D(ref mut e) => {
                        e.vertices = verts
                            .into_iter()
                            .filter_map(|v| {
                                if let PendingVertex::V2D(d) = v {
                                    Some(crate::entities::polyline::Vertex2D {
                                        location: crate::types::Vector3::new(d.x, d.y, d.z),
                                        flags: crate::entities::polyline::VertexFlags::from_bits(
                                            d.flags,
                                        ),
                                        start_width: d.start_width,
                                        end_width: d.end_width,
                                        bulge: d.bulge,
                                        curve_tangent: d.tangent_dir,
                                        id: d.vertex_id,
                                    })
                                } else {
                                    None
                                }
                            })
                            .collect();
                    }
                    EntityType::Polyline3D(ref mut e) => {
                        e.vertices = verts
                            .into_iter()
                            .filter_map(|v| {
                                if let PendingVertex::V3D(d, _ec) = v {
                                    Some(crate::entities::polyline3d::Vertex3DPolyline {
                                        handle: d.handle,
                                        layer: String::new(),
                                        position: d.position,
                                        flags: d.flags as i32,
                                    })
                                } else {
                                    None
                                }
                            })
                            .collect();
                    }
                    EntityType::PolyfaceMesh(ref mut e) => {
                        for v in verts {
                            match v {
                                PendingVertex::V3D(d, ec) => {
                                    e.vertices.push(crate::entities::polyface_mesh::PolyfaceVertex {
                                        common: ec,
                                        location: d.position,
                                        flags: crate::entities::polyface_mesh::PolyfaceVertexFlags::from_bits_truncate(d.flags as i16),
                                        bulge: 0.0,
                                        start_width: 0.0,
                                        end_width: 0.0,
                                        curve_tangent: 0.0,
                                        id: 0,
                                    });
                                }
                                PendingVertex::PfaceFace(f, ec) => {
                                    e.faces.push(crate::entities::polyface_mesh::PolyfaceFace {
                                        common: ec,
                                        flags: crate::entities::polyface_mesh::PolyfaceVertexFlags::NONE,
                                        index1: f.index1,
                                        index2: f.index2,
                                        index3: f.index3,
                                        index4: f.index4,
                                        color: None,
                                    });
                                }
                                _ => {}
                            }
                        }
                        // Restore the seqend handle for this polyface mesh
                        if let Some(sh) = pending.seqends.get(&poly_handle).copied() {
                            e.seqend_handle = Some(sh);
                        }
                    }
                    EntityType::PolygonMesh(ref mut e) => {
                        e.vertices = verts
                            .into_iter()
                            .filter_map(|v| {
                                if let PendingVertex::V3D(d, _ec) = v {
                                    let mut c = crate::entities::EntityCommon::new();
                                    c.handle = d.handle;
                                    Some(crate::entities::polygon_mesh::PolygonMeshVertex {
                                        common: c,
                                        location: d.position,
                                        flags: 0,
                                    })
                                } else {
                                    None
                                }
                            })
                            .collect();
                    }
                    _ => {}
                }
            }
            decoded_pass2 = decoded_pass2.saturating_add(1);
            document.add_loaded_entity(entity);
        }

        // ── Post-pass: Attach pending attribute entities to parent INSERTs ──
        if !pending_attributes.is_empty() {
            for entity in &mut document.entities {
                let entity = std::sync::Arc::make_mut(entity);
                if let EntityType::Insert(ref mut ins) = entity {
                    let insert_handle = ins.common.handle.value();
                    if let Some(attribs) = pending_attributes.remove(&insert_handle) {
                        ins.attributes = attribs;
                    }
                    if let Some(seqend) = pending.seqends.get(&insert_handle).copied() {
                        ins.seqend_handle = Some(seqend);
                    }
                }
            }
        }

        // MLINE entities and MLINESTYLE objects are decoded independently in
        // parallel. Resolve the display name after every object is committed.
        {
            let style_names: HashMap<Handle, String> = document
                .objects
                .iter()
                .filter_map(|(handle, object)| match object {
                    crate::objects::ObjectType::MLineStyle(style) => {
                        Some((*handle, style.name.clone()))
                    }
                    _ => None,
                })
                .collect();
            if !style_names.is_empty() {
                for entity in &mut document.entities {
                    let style_name = match entity.as_ref() {
                        EntityType::MLine(mline) => mline
                            .style_handle
                            .and_then(|handle| style_names.get(&handle))
                            .cloned(),
                        _ => None,
                    };
                    if let Some(style_name) = style_name {
                        if let EntityType::MLine(mline) = std::sync::Arc::make_mut(entity) {
                            mline.style_name = style_name;
                        }
                    }
                }
            }
        }

        // TOLERANCE stores its dimension style as a handle in DWG, while the
        // public entity model also exposes the resolved name used by property
        // editors and DXF output. Table entries and entities are decoded in
        // separate passes, so restore the name only after the DIMSTYLE table is
        // complete. Keep the handle authoritative and leave unresolved values
        // untouched for lossless round-tripping.
        {
            let style_names: HashMap<Handle, String> = document
                .dim_styles
                .iter()
                .map(|style| (style.handle, style.name.clone()))
                .collect();
            if !style_names.is_empty() {
                for entity in &mut document.entities {
                    let style_name = match entity.as_ref() {
                        EntityType::Tolerance(tolerance) => tolerance
                            .dimension_style_handle
                            .and_then(|handle| style_names.get(&handle))
                            .cloned(),
                        _ => None,
                    };
                    if let Some(style_name) = style_name {
                        if let EntityType::Tolerance(tolerance) = std::sync::Arc::make_mut(entity) {
                            tolerance.dimension_style_name = style_name;
                        }
                    }
                }
            }
        }

        // GROUP names live in the owning dictionary; the GROUP body only
        // carries a separate unnamed flag. Restore the public name without
        // using it to synthesize or overwrite that flag.
        {
            let group_names: HashMap<Handle, String> = document
                .objects
                .values()
                .filter_map(|object| match object {
                    crate::objects::ObjectType::Dictionary(dictionary) => {
                        Some(dictionary.entries.iter())
                    }
                    _ => None,
                })
                .flatten()
                .filter_map(|(name, handle)| {
                    matches!(
                        document.objects.get(handle),
                        Some(crate::objects::ObjectType::Group(_))
                    )
                    .then(|| (*handle, name.clone()))
                })
                .collect();
            for (handle, name) in group_names {
                if let Some(crate::objects::ObjectType::Group(group)) =
                    document.objects.get_mut(&handle)
                {
                    group.name = name;
                }
            }
        }

        document.resolve_xrecord_names();

        // Advanced material properties are not stored in the AcDbMaterial
        // object body. AutoCAD keeps them in the ADVMATERIAL XRECORD owned by
        // the material's extension dictionary. Expose those values on the
        // Material model while retaining the XRECORD as the authoritative
        // on-disk representation.
        document.resolve_xrecord_backed_properties();

        // ── Post-pass: cache each RasterImage's path from its IMAGEDEF ──
        //
        // An IMAGE entity carries no path of its own — the referenced
        // ImageDefinition object holds it (the entity's `file_path` is only a
        // convenience cache). Copy it across so rendering and loading can see
        // the path directly: a resolvable local image loads its pixels, and an
        // unresolved reference (a URL, a missing file) can show its path as
        // text instead of a blank frame.
        {
            let def_paths: HashMap<Handle, String> = document
                .objects
                .iter()
                .filter_map(|(h, o)| match o {
                    crate::objects::ObjectType::ImageDefinition(d) if !d.file_name.is_empty() => {
                        Some((*h, d.file_name.clone()))
                    }
                    _ => None,
                })
                .collect();
            if !def_paths.is_empty() {
                for entity in &mut document.entities {
                    let needs = matches!(&**entity, EntityType::RasterImage(im)
                        if im.file_path.is_empty()
                            && im.definition_handle.is_some_and(|h| def_paths.contains_key(&h)));
                    if !needs {
                        continue;
                    }
                    if let EntityType::RasterImage(im) = std::sync::Arc::make_mut(entity) {
                        if let Some(p) = im.definition_handle.and_then(|h| def_paths.get(&h)) {
                            im.file_path = p.clone();
                        }
                    }
                }
            }
        }

        // ── Post-pass: cache each RasterImage's path from its IMAGEDEF ──
        //
        // An IMAGE entity carries no path of its own — the referenced
        // ImageDefinition object holds it (the entity's `file_path` is only a
        // convenience cache). Copy it across so rendering and loading can see
        // the path directly: a resolvable local image loads its pixels, and an
        // unresolved reference (a URL, a missing file) can show its path as
        // text instead of a blank frame.
        {
            let def_paths: HashMap<Handle, String> = document
                .objects
                .iter()
                .filter_map(|(h, o)| match o {
                    crate::objects::ObjectType::ImageDefinition(d) if !d.file_name.is_empty() => {
                        Some((*h, d.file_name.clone()))
                    }
                    _ => None,
                })
                .collect();
            if !def_paths.is_empty() {
                for entity in &mut document.entities {
                    let needs = matches!(&**entity, EntityType::RasterImage(im)
                        if im.file_path.is_empty()
                            && im.definition_handle.is_some_and(|h| def_paths.contains_key(&h)));
                    if !needs {
                        continue;
                    }
                    if let EntityType::RasterImage(im) = std::sync::Arc::make_mut(entity) {
                        if let Some(p) = im.definition_handle.and_then(|h| def_paths.get(&h)) {
                            im.file_path = p.clone();
                        }
                    }
                }
            }
        }

        // ── Post-pass: Correct entity ownership from binary data ───────
        //
        // The DWG entity_mode=1 flag means "paper space entity" but does
        // NOT specify WHICH paper space.  During Pass 2, all entity_mode=1
        // entities were routed to the single *Paper_Space block record.
        // Use the canonical entity_handle lists from the binary block
        // records (R2004+) to correct ownership for entities that belong
        // to non-active paper spaces (*Paper_Space0, *Paper_Space1, etc.).
        if perf {
            eprintln!(
                "[perf] dwg-build post={:.1}ms",
                post_started.elapsed().as_secs_f64() * 1000.0,
            );
        }
        self.report_progress(925);
        let ownership_started = web_time::Instant::now();
        // Rebuild block membership in O(entities + blocks). The ordinary
        // add-entity path scans every block record for every entity, which
        // dominates load time in block-heavy drawings. Owner correction and
        // validation share the same parallel pass over the entity arena.
        let source_record_order = (!self.obj_reader.version().r2004_plus()).then(|| {
            record_catalog
                .iter()
                .map(|(handle, offset, _, _)| (Handle::from(*handle), *offset))
                .collect::<ahash::AHashMap<_, _>>()
        });
        Self::rebuild_block_membership(
            document,
            Some(&binary_entity_owner),
            source_record_order.as_ref(),
        );
        if perf {
            eprintln!(
                "[perf] dwg-build ownership={:.1}ms entities={} blocks={}",
                ownership_started.elapsed().as_secs_f64() * 1000.0,
                document.entities.len(),
                document.block_records.len(),
            );
        }
        let tail_started = web_time::Instant::now();

        // Ensure allocations made by the repair steps start above every source
        // record, even when the file's HANDSEED was stale.
        let max_from_reader = handles.iter().max().copied().unwrap_or(0);
        if max_from_reader + 1 > document.header.handle_seed {
            document.header.handle_seed = max_from_reader + 1;
        }

        // BLOCK/ENDBLK markers are structural records and are not retained in
        // the flat entity arena. Damaged files can point a block record at a
        // handle now occupied by a table entry or ordinary object (0718 uses
        // its Standard DIMSTYLE as *Paper_Space's BLOCK marker). Rehome only
        // those colliding markers; the writer will synthesize their records.
        let mut occupied: std::collections::HashSet<Handle> =
            document.objects.keys().copied().collect();
        occupied.extend(document.entities().filter_map(|entity| {
            (!matches!(
                entity,
                crate::entities::EntityType::Block(_) | crate::entities::EntityType::BlockEnd(_)
            ))
            .then_some(entity.common().handle)
        }));
        for handle in [
            document.block_records.handle(),
            document.layers.handle(),
            document.text_styles.handle(),
            document.line_types.handle(),
            document.views.handle(),
            document.ucss.handle(),
            document.vports.handle(),
            document.app_ids.handle(),
            document.dim_styles.handle(),
            document.vx_table.handle(),
        ] {
            occupied.insert(handle);
        }
        occupied.extend(document.block_records.iter().map(|record| record.handle));
        occupied.extend(document.layers.iter().map(|entry| entry.handle));
        occupied.extend(document.text_styles.iter().map(|entry| entry.handle));
        occupied.extend(document.line_types.iter().map(|entry| entry.handle));
        occupied.extend(document.views.iter().map(|entry| entry.handle));
        occupied.extend(document.ucss.iter().map(|entry| entry.handle));
        occupied.extend(document.vports.iter().map(|entry| entry.handle));
        occupied.extend(document.app_ids.iter().map(|entry| entry.handle));
        occupied.extend(document.dim_styles.iter().map(|entry| entry.handle));
        occupied.extend(document.vx_table.iter().map(|entry| entry.handle));
        fn allocate_marker(
            occupied: &mut std::collections::HashSet<Handle>,
            next_handle: &mut u64,
        ) -> Handle {
            while occupied.contains(&Handle::new(*next_handle)) {
                *next_handle += 1;
            }
            let handle = Handle::new(*next_handle);
            *next_handle += 1;
            occupied.insert(handle);
            handle
        }
        let mut next_handle = document.header.handle_seed.max(1);
        for record in document.block_records.iter_mut() {
            if record.block_entity_handle.is_null()
                || occupied.contains(&record.block_entity_handle)
            {
                record.block_entity_handle = allocate_marker(&mut occupied, &mut next_handle);
            } else {
                occupied.insert(record.block_entity_handle);
            }
            if record.block_end_handle.is_null() || occupied.contains(&record.block_end_handle) {
                record.block_end_handle = allocate_marker(&mut occupied, &mut next_handle);
            } else {
                occupied.insert(record.block_end_handle);
            }
        }
        document.header.handle_seed = document.header.handle_seed.max(next_handle);

        // ── Post-pass: Resolve root dictionary handle ──────────────────
        //
        // The DWG header often stores dictionary handles as relative
        // references that resolve to 0 during header reading.  Now that
        // all objects have been read, scan for the actual root dictionary
        // (owner == NULL) and update the header.
        let root_is_dictionary = matches!(
            document
                .objects
                .get(&document.header.named_objects_dict_handle),
            Some(crate::objects::ObjectType::Dictionary(_))
        );
        if !root_is_dictionary {
            let mut best = Handle::NULL;
            let mut best_count = 0usize;
            for (h, obj) in &document.objects {
                if let crate::objects::ObjectType::Dictionary(dict) = obj {
                    if dict.owner.is_null() {
                        if dict.entries.len() > best_count
                            || (dict.entries.len() == best_count && h.value() > best.value())
                        {
                            best = *h;
                            best_count = dict.entries.len();
                        }
                    }
                }
            }
            if !best.is_null() {
                document.header.named_objects_dict_handle = best;
            } else {
                // Some damaged files point NOD at a table control and give
                // every top-level dictionary that same invalid owner. Preserve
                // those valid child dictionaries behind a fresh root instead
                // of writing a null NOD, which makes ODA demand recovery.
                let header_root = document.header.named_objects_dict_handle;
                let mut invalid_root_votes: HashMap<Handle, usize> = HashMap::new();
                for (handle, reactors) in &document.reactors_by_handle {
                    if !matches!(
                        document.objects.get(handle),
                        Some(
                            crate::objects::ObjectType::Dictionary(_)
                                | crate::objects::ObjectType::DictionaryWithDefault(_)
                        )
                    ) {
                        continue;
                    }
                    for reactor in reactors {
                        if !matches!(
                            document.objects.get(reactor),
                            Some(crate::objects::ObjectType::Dictionary(_))
                        ) {
                            *invalid_root_votes.entry(*reactor).or_default() += 1;
                        }
                    }
                }
                let previous_root = invalid_root_votes
                    .into_iter()
                    .max_by_key(|(_, count)| *count)
                    .map(|(handle, _)| handle)
                    .filter(|handle| !handle.is_null())
                    .unwrap_or(header_root);
                let root_handle = document.allocate_handle();
                let children = [
                    ("ACAD_GROUP", document.header.acad_group_dict_handle),
                    (
                        "ACAD_MLINESTYLE",
                        document.header.acad_mlinestyle_dict_handle,
                    ),
                    ("ACAD_LAYOUT", document.header.acad_layout_dict_handle),
                    (
                        "ACAD_PLOTSETTINGS",
                        document.header.acad_plotsettings_dict_handle,
                    ),
                    (
                        "ACAD_PLOTSTYLENAME",
                        document.header.acad_plotstylename_dict_handle,
                    ),
                    ("ACAD_MATERIAL", document.header.acad_material_dict_handle),
                    ("ACAD_COLOR", document.header.acad_color_dict_handle),
                    (
                        "ACAD_VISUALSTYLE",
                        document.header.acad_visualstyle_dict_handle,
                    ),
                ];
                let mut root = crate::objects::Dictionary::new();
                root.handle = root_handle;
                for (name, child_handle) in children {
                    let child_is_dictionary = matches!(
                        document.objects.get(&child_handle),
                        Some(
                            crate::objects::ObjectType::Dictionary(_)
                                | crate::objects::ObjectType::DictionaryWithDefault(_)
                        )
                    );
                    if !child_is_dictionary {
                        continue;
                    }
                    root.add_entry(name, child_handle);
                    if let Some(child) = document.objects.get_mut(&child_handle) {
                        match child {
                            crate::objects::ObjectType::Dictionary(dictionary) => {
                                dictionary.owner = root_handle;
                            }
                            crate::objects::ObjectType::DictionaryWithDefault(dictionary) => {
                                dictionary.owner = root_handle;
                            }
                            _ => {}
                        }
                    }
                    if let Some(reactors) = document.reactors_by_handle.get_mut(&child_handle) {
                        for reactor in reactors {
                            if *reactor == previous_root {
                                *reactor = root_handle;
                            }
                        }
                    }
                }

                // A damaged NOD often leaves every former root child with a
                // reactor pointing at the non-dictionary header handle. Repair
                // all of those references, not only the few child handles that
                // survived header decoding. Keep every recovered dictionary
                // reachable from the fresh root; known roles retain their
                // standard names, unknown roles get stable recovery names.
                let top_level: Vec<Handle> = document
                    .reactors_by_handle
                    .iter()
                    .filter_map(|(handle, reactors)| {
                        reactors.contains(&previous_root).then_some(*handle)
                    })
                    .collect();
                for child_handle in top_level {
                    let inferred_name = match document.objects.get(&child_handle) {
                        Some(crate::objects::ObjectType::Dictionary(dictionary)) => {
                            let values: Vec<&crate::objects::ObjectType> = dictionary
                                .entries
                                .iter()
                                .filter_map(|(_, handle)| document.objects.get(handle))
                                .collect();
                            if values.iter().any(|object| {
                                matches!(object, crate::objects::ObjectType::Layout(_))
                            }) {
                                "ACAD_LAYOUT".to_string()
                            } else if values.iter().any(|object| {
                                matches!(object, crate::objects::ObjectType::MLineStyle(_))
                            }) {
                                "ACAD_MLINESTYLE".to_string()
                            } else if values.iter().any(|object| {
                                matches!(object, crate::objects::ObjectType::PlotSettings(_))
                            }) {
                                "ACAD_PLOTSETTINGS".to_string()
                            } else if values.iter().any(|object| {
                                matches!(object, crate::objects::ObjectType::MultiLeaderStyle(_))
                            }) {
                                "ACAD_MLEADERSTYLE".to_string()
                            } else if values.iter().any(|object| {
                                matches!(object, crate::objects::ObjectType::TableStyle(_))
                            }) {
                                "ACAD_TABLESTYLE".to_string()
                            } else if values.iter().any(|object| {
                                matches!(object, crate::objects::ObjectType::Scale(_))
                            }) {
                                "ACAD_SCALELIST".to_string()
                            } else if values.iter().any(|object| {
                                matches!(object, crate::objects::ObjectType::VisualStyle(_))
                            }) {
                                "ACAD_VISUALSTYLE".to_string()
                            } else if values.iter().any(|object| {
                                matches!(object, crate::objects::ObjectType::Material(_))
                            }) {
                                "ACAD_MATERIAL".to_string()
                            } else if values.iter().any(|object| {
                                matches!(object, crate::objects::ObjectType::BookColor(_))
                            }) {
                                "ACAD_COLOR".to_string()
                            } else if values.iter().any(|object| {
                                matches!(object, crate::objects::ObjectType::Group(_))
                            }) {
                                "ACAD_GROUP".to_string()
                            } else if dictionary.get("AcDsRecords").is_some()
                                || dictionary.get("AcDsSchemas").is_some()
                            {
                                "ACAD_ACDSRECORDS".to_string()
                            } else if dictionary.get("CANNOSCALE").is_some() {
                                "AcDbVariableDictionary".to_string()
                            } else {
                                format!("RECOVERED_{:X}", child_handle.value())
                            }
                        }
                        _ => continue,
                    };
                    let name = if root.get(&inferred_name).is_some() {
                        format!("RECOVERED_{:X}", child_handle.value())
                    } else {
                        inferred_name
                    };
                    root.add_entry(name, child_handle);
                    if let Some(crate::objects::ObjectType::Dictionary(dictionary)) =
                        document.objects.get_mut(&child_handle)
                    {
                        if dictionary.owner.is_null() || dictionary.owner == previous_root {
                            dictionary.owner = root_handle;
                        }
                    }
                    if let Some(reactors) = document.reactors_by_handle.get_mut(&child_handle) {
                        for reactor in reactors {
                            if *reactor == previous_root {
                                *reactor = root_handle;
                            }
                        }
                    }
                }
                document
                    .objects
                    .insert(root_handle, crate::objects::ObjectType::Dictionary(root));
                document.header.named_objects_dict_handle = root_handle;
            }
        }

        // Summary notification
        let total_skipped = skipped_catalog
            .saturating_add(skipped_pass1 as usize)
            .saturating_add(skipped_pass2 as usize);
        if total_skipped > 0 {
            self.notifications.notify(
                NotificationType::Warning,
                format!(
                    "DWG build summary: {} of {} handles processed, {} records skipped ({} table, {} entity/object)",
                    total_handles.saturating_sub(total_skipped),
                    total_handles,
                    total_skipped,
                    skipped_pass1,
                    skipped_pass2,
                ),
            );
        }

        let annotative_started = web_time::Instant::now();
        // ── Annotative flag from `AcadAnnotative` EED (STYLE / DIMSTYLE) ──
        // These records have no native annotative field; the flag is stored as
        // extended data under the `AcadAnnotative` application.
        if let Some(anno_h) = document
            .app_ids
            .get("AcadAnnotative")
            .map(|a| a.handle.value())
        {
            let wide = self.obj_reader.version().r2007_plus();
            let flags: std::collections::HashMap<Handle, bool> = document
                .eed_by_handle
                .iter()
                .filter_map(|(h, blocks)| {
                    blocks
                        .iter()
                        .find(|(a, _)| *a == anno_h)
                        .and_then(|(_, bytes)| {
                            crate::io::dwg::annotative_eed::decode_flag(bytes, wide)
                        })
                        .map(|f| (*h, f))
                })
                .collect();
            for ts in document.text_styles.iter_mut() {
                if let Some(&f) = flags.get(&ts.handle) {
                    ts.annotative = f;
                }
            }
            for ds in document.dim_styles.iter_mut() {
                if let Some(&f) = flags.get(&ds.handle) {
                    ds.annotative = f;
                }
            }
        }
        if perf {
            eprintln!(
                "[perf] dwg-build annotative={:.1}ms",
                annotative_started.elapsed().as_secs_f64() * 1000.0,
            );
        }

        let eed_started = web_time::Instant::now();
        // ── Decode entity EED blobs into structured records ──────────────────
        // The object reader keeps every EED block as verbatim `raw_dwg_eed`
        // bytes (preserved for a byte-exact re-save). Additionally decode each
        // block whose application is known into `records`, so callers — plugins
        // reading XDATA via `read_record`, the DXF writer — see the same values
        // a DXF read would surface. The raw blob is kept, so a plain round-trip
        // still emits it verbatim; the writer prefers raw over records per app.
        {
            let wide = self.obj_reader.version().r2007_plus();
            let app_name_by_handle: ahash::AHashMap<u64, String> = document
                .app_ids
                .iter()
                .map(|a| (a.handle.value(), a.name.clone()))
                .collect();
            let layer_name_by_handle: ahash::AHashMap<u64, String> = document
                .layers
                .iter()
                .map(|l| (l.handle.value(), l.name.clone()))
                .collect();
            if !app_name_by_handle.is_empty() {
                for_each_mut(&mut document.entities, |entity| {
                    let records: Vec<crate::xdata::ExtendedDataRecord> = {
                        let xd = &entity.common().extended_data;
                        xd.raw_dwg_eed
                            .iter()
                            .filter_map(|(app_handle, bytes)| {
                                let Some(name) = app_name_by_handle.get(app_handle) else {
                                    return None;
                                };
                                if xd.get_record(name).is_some() {
                                    return None;
                                }
                                crate::io::dwg::eed_codec::decode_values(bytes, wide, |h| {
                                    layer_name_by_handle.get(&h).cloned()
                                })
                                .map(|values| {
                                    let mut rec =
                                        crate::xdata::ExtendedDataRecord::new(name.clone());
                                    rec.values = values;
                                    rec
                                })
                            })
                            .collect()
                    };
                    if !records.is_empty() {
                        let xd = &mut std::sync::Arc::make_mut(entity).common_mut().extended_data;
                        for record in records {
                            xd.add_record(record);
                        }
                    }
                });
            }
        }
        if perf {
            eprintln!(
                "[perf] dwg-build eed={:.1}ms",
                eed_started.elapsed().as_secs_f64() * 1000.0,
            );
        }

        let acis_started = web_time::Instant::now();
        // ── AcDs SAB ordering ──────────────────────────────────────────────
        // R2013+ modeler geometry (3DSOLID/REGION/BODY/SURFACE) is stored as
        // SAB blobs in the AcDs section, one per entity whose `has_ds_data` bit
        // is set. The AcDs data-store indexes those blobs through a search
        // segment sorted ascending by owning-entity handle, and the blobs are
        // laid out in that same record order — so the i-th blob (in file order)
        // belongs to the i-th flagged modeler entity taken in ascending handle
        // order. `attach_acds_sab_blobs` pairs blob[i] with this list's i-th
        // handle. (Ordering by object-stream file offset instead mispaired
        // blobs whenever the object-stream order diverged from handle order.)
        {
            let mut ordered: Vec<Handle> = document
                .entities()
                .filter(|e| {
                    matches!(
                        e,
                        EntityType::Solid3D(_)
                            | EntityType::Region(_)
                            | EntityType::Body(_)
                            | EntityType::Surface(_)
                    ) && e.common().has_ds_data
                })
                .map(|e| e.common().handle)
                .collect();
            ordered.sort_by_key(|h| h.value());
            document.acis_sab_handles = ordered;
        }
        if perf {
            eprintln!(
                "[perf] dwg-build acis-order={:.1}ms",
                acis_started.elapsed().as_secs_f64() * 1000.0,
            );
        }

        let repair_started = web_time::Instant::now();
        // ── Handle-collision repair ────────────────────────────────────────
        // The document is seeded with standard table entries (Standard dim
        // style, default block records, …) at low handles before the file's
        // objects are read, so a synthesized entry can end up sharing a handle
        // with a file object that legitimately owns it — e.g. the Standard dim
        // style vs a paper-space block record. A duplicate handle makes that
        // reference ambiguous and a strict reader rejects the owning object
        // ("improperly read"). Re-home any dim-style entry whose handle also
        // belongs to a block record, following the header references so the
        // Standard style stays reachable.
        {
            use std::collections::HashSet;

            // Retained defaults that were not present in the source must not
            // keep one of their preallocated low handles when the source uses
            // that handle for a different record. Prefer the source record and
            // move only the synthetic table entry.
            let source_records: HashSet<u64> = record_catalog
                .iter()
                .map(|(handle, _, _, _)| *handle)
                .collect();
            let mut source_views = HashSet::new();
            let mut source_ucss = HashSet::new();
            let mut source_vports = HashSet::new();
            let mut source_app_ids = HashSet::new();
            for entry in &parsed_entries {
                match entry {
                    ParsedEntry::View(handle, _) => {
                        source_views.insert(*handle);
                    }
                    ParsedEntry::Ucs(handle, _) => {
                        source_ucss.insert(*handle);
                    }
                    ParsedEntry::VPort(handle, _) => {
                        source_vports.insert(*handle);
                    }
                    ParsedEntry::AppId(handle, _) => {
                        source_app_ids.insert(*handle);
                    }
                    _ => {}
                }
            }
            let collides = |handle: Handle, source_entries: &HashSet<u64>| {
                !handle.is_null()
                    && source_records.contains(&handle.value())
                    && !source_entries.contains(&handle.value())
            };

            let source_layers: HashSet<u64> = maps.layers.keys().copied().collect();
            let layer_collisions: Vec<Handle> = document
                .layers
                .iter()
                .filter(|entry| collides(entry.handle, &source_layers))
                .map(|entry| entry.handle)
                .collect();
            for old in layer_collisions {
                let new = document.allocate_handle();
                for entry in document.layers.iter_mut() {
                    if entry.handle == old {
                        entry.handle = new;
                    }
                }
                if document.header.current_layer_handle == old {
                    document.header.current_layer_handle = new;
                }
            }

            let source_linetypes: HashSet<u64> = maps.linetypes.keys().copied().collect();
            let linetype_collisions: Vec<Handle> = document
                .line_types
                .iter()
                .filter(|entry| collides(entry.handle, &source_linetypes))
                .map(|entry| entry.handle)
                .collect();
            for old in linetype_collisions {
                let new = document.allocate_handle();
                for entry in document.line_types.iter_mut() {
                    if entry.handle == old {
                        entry.handle = new;
                    }
                }
                for handle in [
                    &mut document.header.current_linetype_handle,
                    &mut document.header.continuous_linetype_handle,
                    &mut document.header.bylayer_linetype_handle,
                    &mut document.header.byblock_linetype_handle,
                    &mut document.header.dim_linetype_handle,
                    &mut document.header.dim_linetype1_handle,
                    &mut document.header.dim_linetype2_handle,
                ] {
                    if *handle == old {
                        *handle = new;
                    }
                }
            }

            let source_styles: HashSet<u64> = maps.text_styles.keys().copied().collect();
            let style_collisions: Vec<Handle> = document
                .text_styles
                .iter()
                .filter(|entry| collides(entry.handle, &source_styles))
                .map(|entry| entry.handle)
                .collect();
            for old in style_collisions {
                let new = document.allocate_handle();
                for entry in document.text_styles.iter_mut() {
                    if entry.handle == old {
                        entry.handle = new;
                    }
                }
                if document.header.current_text_style_handle == old {
                    document.header.current_text_style_handle = new;
                }
                if document.header.dim_text_style_handle == old {
                    document.header.dim_text_style_handle = new;
                }
            }

            let source_dimstyles: HashSet<u64> = maps.dim_styles.keys().copied().collect();
            let dimstyle_collisions: Vec<Handle> = document
                .dim_styles
                .iter()
                .filter(|entry| collides(entry.handle, &source_dimstyles))
                .map(|entry| entry.handle)
                .collect();
            for old in dimstyle_collisions {
                let new = document.allocate_handle();
                for entry in document.dim_styles.iter_mut() {
                    if entry.handle == old {
                        entry.handle = new;
                    }
                }
                if document.header.current_dimstyle_handle == old {
                    document.header.current_dimstyle_handle = new;
                }
            }

            let view_collisions: Vec<Handle> = document
                .views
                .iter()
                .filter(|entry| collides(entry.handle, &source_views))
                .map(|entry| entry.handle)
                .collect();
            for old in view_collisions {
                let new = document.allocate_handle();
                for entry in document.views.iter_mut() {
                    if entry.handle == old {
                        entry.handle = new;
                    }
                }
            }

            let ucs_collisions: Vec<Handle> = document
                .ucss
                .iter()
                .filter(|entry| collides(entry.handle, &source_ucss))
                .map(|entry| entry.handle)
                .collect();
            for old in ucs_collisions {
                let new = document.allocate_handle();
                for entry in document.ucss.iter_mut() {
                    if entry.handle == old {
                        entry.handle = new;
                    }
                }
            }

            let vport_collisions: Vec<Handle> = document
                .vports
                .iter()
                .filter(|entry| collides(entry.handle, &source_vports))
                .map(|entry| entry.handle)
                .collect();
            for old in vport_collisions {
                let new = document.allocate_handle();
                for entry in document.vports.iter_mut() {
                    if entry.handle == old {
                        entry.handle = new;
                    }
                }
            }

            let app_id_collisions: Vec<Handle> = document
                .app_ids
                .iter()
                .filter(|entry| collides(entry.handle, &source_app_ids))
                .map(|entry| entry.handle)
                .collect();
            for old in app_id_collisions {
                let new = document.allocate_handle();
                for entry in document.app_ids.iter_mut() {
                    if entry.handle == old {
                        entry.handle = new;
                    }
                }
            }
        }

        {
            use std::collections::HashSet;
            let block_handles: HashSet<u64> = document
                .block_records
                .iter()
                .map(|b| b.handle.value())
                .collect();
            let colliding: Vec<u64> = document
                .dim_styles
                .iter()
                .map(|d| d.handle.value())
                .filter(|h| block_handles.contains(h))
                .collect();
            for old in colliding {
                let new_h = document.allocate_handle();
                for d in document.dim_styles.iter_mut() {
                    if d.handle.value() == old {
                        d.handle = new_h;
                    }
                }
                if document.header.current_dimstyle_handle.value() == old {
                    document.header.current_dimstyle_handle = new_h;
                }
                if document.header.dim_text_style_handle.value() == old {
                    document.header.dim_text_style_handle = new_h;
                }
            }
        }

        document.header.current_layer_name = document
            .layers
            .iter()
            .find(|layer| layer.handle == document.header.current_layer_handle)
            .map(|layer| layer.name.clone())
            .unwrap_or_default();

        // ── Post-pass: guarantee the mandatory *Model_Space / *Paper_Space ──
        // block records exist and enumerate their geometry.
        //
        // The block-control table names the model/paper-space handles, but a
        // file can reach here without their BLOCK_HEADER ever materialising as a
        // record (absent from the object stream). The DWG writer emits a block's
        // contents by walking `BlockRecord::entity_handles`, so a missing record
        // — or one whose owned list stayed empty while entities point at it via
        // `owner_handle` — serialises to nothing, silently dropping that space's
        // geometry on the next save. Synthesize the missing records (the writer
        // fabricates their BLOCK/ENDBLK markers from the allocated handles) and
        // rebuild any empty owned-list from ownership so the round-trip is
        // lossless.
        {
            let mut added_record = false;
            for (h, is_model) in [
                (document.header.model_space_block_handle, true),
                (document.header.paper_space_block_handle, false),
            ] {
                if h.is_null() || document.block_records.iter().any(|br| br.handle == h) {
                    continue;
                }
                let mut br = if is_model {
                    crate::tables::BlockRecord::model_space()
                } else {
                    crate::tables::BlockRecord::paper_space()
                };
                // The captured handle may be POISON: a damaged file can point
                // its Layout at an object that is not a block record at all
                // (seen in the wild: BLOCK_CONTROL.model_space NULL and the
                // "Model" Layout pointing at the LAYER_CONTROL handle).
                // Synthesizing the record under that handle duplicates it in
                // the object stream on the next save — AutoCAD/ODA then follow
                // the handle, find the layer table, and abort the whole file.
                // Allocate a fresh handle instead and re-point the header and
                // the owning Layout at it.
                let collides = document.objects.contains_key(&h)
                    || document.layers.handle() == h
                    || document.line_types.handle() == h
                    || document.text_styles.handle() == h
                    || document.dim_styles.handle() == h
                    || document.layers.iter().any(|l| l.handle == h)
                    || document.get_entity(h).is_some();
                let original_handle = h;
                let h = if collides {
                    let fresh = document.allocate_handle();
                    if is_model {
                        document.header.model_space_block_handle = fresh;
                    } else {
                        document.header.paper_space_block_handle = fresh;
                    }
                    for obj in document.objects.values_mut() {
                        if let crate::objects::ObjectType::Layout(l) = obj {
                            if l.block_record == h {
                                l.block_record = fresh;
                            }
                        }
                    }
                    for entity in &mut document.entities {
                        if entity.common().owner_handle == original_handle {
                            std::sync::Arc::make_mut(entity).common_mut().owner_handle = fresh;
                        }
                    }
                    fresh
                } else {
                    h
                };
                br.handle = h;
                br.block_entity_handle = document.allocate_handle();
                br.block_end_handle = document.allocate_handle();
                // Cross-link the owning Layout object, if present, so the record
                // and its Layout reference each other like a normally-read pair.
                for (oh, obj) in document.objects.iter() {
                    if let crate::objects::ObjectType::Layout(l) = obj {
                        if l.block_record == h {
                            br.layout = *oh;
                            break;
                        }
                    }
                }
                let _ = document.block_records.add(br);
                added_record = true;
            }
            if added_record {
                Self::rebuild_block_membership(document, None, None);
            }
        }

        document.resolve_book_colors();

        // The current model-space annotation scale (CANNOSCALE) is not carried
        // in the DWG header stream, only in the AcDbVariableDictionary. Reflect
        // it into the header so consumers (and DXF export) see the real scale
        // rather than the "1:1" default.
        Self::reflect_annotation_scale(document);

        if perf {
            eprintln!(
                "[perf] dwg-build repair={:.1}ms",
                repair_started.elapsed().as_secs_f64() * 1000.0,
            );
            eprintln!(
                "[perf] dwg-build tail={:.1}ms total={:.1}ms",
                tail_started.elapsed().as_secs_f64() * 1000.0,
                build_started.elapsed().as_secs_f64() * 1000.0,
            );
        }

        DwgBuildOutcome {
            notifications: self.notifications,
            decoded_records: parsed_entries.len().saturating_add(decoded_pass2),
            skipped_records: skipped_catalog
                .saturating_add(skipped_pass1 as usize)
                .saturating_add(skipped_pass2 as usize),
            diagnostics,
        }
    }

    /// Populate the header's current annotation scale (CANNOSCALE) from the
    /// AcDbVariableDictionary — the DWG header stream omits it. Sets the scale
    /// name and, from the referenced AcDbScale, the numeric value
    /// (paper units / drawing units, e.g. "1:70" → 1/70).
    fn reflect_annotation_scale(document: &mut CadDocument) {
        let var_handle = document.objects.values().find_map(|o| match o {
            crate::objects::ObjectType::Dictionary(d) => d
                .entries
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("CANNOSCALE"))
                .map(|(_, vh)| *vh),
            _ => None,
        });
        let Some(vh) = var_handle else {
            return;
        };
        let name = match document.objects.get(&vh) {
            Some(crate::objects::ObjectType::DictionaryVariable(dv)) => dv.value.clone(),
            _ => return,
        };
        if name.trim().is_empty() {
            return;
        }
        let value = document.objects.values().find_map(|o| match o {
            crate::objects::ObjectType::Scale(s)
                if s.name.eq_ignore_ascii_case(&name) && s.drawing_units != 0.0 =>
            {
                Some(s.paper_units / s.drawing_units)
            }
            _ => None,
        });
        document.header.current_annotation_scale = name;
        if let Some(v) = value {
            document.header.annotation_scale_value = v;
        }
    }

    fn rebuild_block_membership(
        document: &mut CadDocument,
        binary_entity_owner: Option<&ahash::AHashMap<Handle, Handle>>,
        source_record_order: Option<&ahash::AHashMap<Handle, usize>>,
    ) {
        let valid_owners: ahash::AHashSet<Handle> = document
            .block_records
            .iter()
            .map(|record| record.handle)
            .collect();
        let model_space = document.header.model_space_block_handle;
        let paper_space = document.header.paper_space_block_handle;
        let model_is_valid = valid_owners.contains(&model_space);
        let paper_is_valid = valid_owners.contains(&paper_space);
        let memberships: Vec<Option<(Handle, Handle)>> =
            map_mut(&mut document.entities, |entity| {
                if matches!(
                    entity.as_ref(),
                    EntityType::AttributeEntity(_) | EntityType::Block(_) | EntityType::BlockEnd(_)
                ) {
                    return None;
                }

                let common = entity.common();
                let handle = common.handle;
                let mut owner = binary_entity_owner
                    .and_then(|owners| owners.get(&handle).copied())
                    .unwrap_or(common.owner_handle);
                if owner.is_null() {
                    owner = if common.entity_mode == Some(1) && paper_is_valid {
                        paper_space
                    } else {
                        model_space
                    };
                }
                if !valid_owners.contains(&owner) && model_is_valid {
                    owner = model_space;
                }
                if common.owner_handle != owner {
                    std::sync::Arc::make_mut(entity).common_mut().owner_handle = owner;
                }
                valid_owners.contains(&owner).then_some((owner, handle))
            });
        let mut by_owner: ahash::AHashMap<Handle, Vec<Handle>> =
            ahash::AHashMap::with_capacity(document.block_records.len());
        for (owner, handle) in memberships.into_iter().flatten() {
            by_owner.entry(owner).or_default().push(handle);
        }

        let (block_records, canonical_block_order) =
            (&mut document.block_records, &document.block_entity_handles);
        for record in block_records.iter_mut() {
            let mut handles = by_owner.remove(&record.handle).unwrap_or_default();
            if let Some(canonical) = canonical_block_order
                .get(&record.handle)
                .filter(|canonical| !canonical.is_empty())
            {
                let order: ahash::AHashMap<Handle, usize> = canonical
                    .iter()
                    .copied()
                    .enumerate()
                    .map(|(index, handle)| (handle, index))
                    .collect();
                handles.sort_by_key(|handle| order.get(handle).copied().unwrap_or(usize::MAX));
            } else if let Some(order) = source_record_order {
                handles.sort_by_key(|handle| order.get(handle).copied().unwrap_or(usize::MAX));
            }
            record.entity_handles = handles;
        }
    }

    /// Process a single object record in Pass 2.
    fn process_pass2_record(
        &self,
        handle: u64,
        raw_type_code: i16,
        type_code: i16,
        mut reader: crate::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader,
        document: &mut Pass2Output,
        maps: &HandleMaps,
        pending: &mut PendingPolylines,
        pending_attributes: &mut HashMap<u64, Vec<AttributeEntity>>,
        entity_class_numbers: &std::collections::HashSet<i16>,
        class_names: &ClassNames,
        photometric_lighting: bool,
    ) {
        // For class-based types (≥500) that weren't resolved via the class
        // map, check the class's is_an_entity flag.  This prevents misreading
        // object data as entity data (different binary layout).
        let is_entity = if type_code >= 500 {
            entity_class_numbers.contains(&type_code)
        } else {
            is_entity_type(type_code)
        };
        if is_entity {
            let entity_data = self
                .obj_reader
                .read_common_entity_data(&mut reader, type_code);
            let entity_common = map_entity_common(
                &entity_data,
                maps,
                document.header.model_space_block_handle,
                document.header.paper_space_block_handle,
            );

            match type_code {
                // ── Simple entities ────────────────────────────────
                OBJ_LINE => {
                    let data = entities::read_line(&mut reader, self.obj_reader.version());
                    let mut e = Line::new();
                    e.common = entity_common;
                    e.start = data.start;
                    e.end = data.end;
                    e.thickness = data.thickness;
                    e.normal = data.normal;
                    let _ = document.add_entity(EntityType::Line(e));
                }
                OBJ_POINT => {
                    let data = entities::read_point(&mut reader);
                    let mut e = Point::new();
                    e.common = entity_common;
                    e.location = data.location;
                    e.thickness = data.thickness;
                    e.normal = data.normal;
                    e.x_axis_angle = data.x_axis_angle;
                    let _ = document.add_entity(EntityType::Point(e));
                }
                OBJ_CIRCLE => {
                    let data = entities::read_circle(&mut reader);
                    let mut e = Circle::new();
                    e.common = entity_common;
                    e.center = data.center;
                    e.radius = data.radius;
                    e.thickness = data.thickness;
                    e.normal = data.normal;
                    let _ = document.add_entity(EntityType::Circle(e));
                }
                OBJ_LIGHT => {
                    let data = entities::read_light(&mut reader, photometric_lighting);
                    let mut e = Light::new();
                    e.common = entity_common;
                    e.class_version = data.class_version;
                    e.name = data.name;
                    e.light_type = data.light_type;
                    e.status = data.status;
                    e.light_color = data.light_color;
                    e.plot_glyph = data.plot_glyph;
                    e.intensity = data.intensity;
                    e.position = data.position;
                    e.target = data.target;
                    e.attenuation_type = data.attenuation_type;
                    e.use_attenuation_limits = data.use_attenuation_limits;
                    e.attenuation_start_limit = data.attenuation_start_limit;
                    e.attenuation_end_limit = data.attenuation_end_limit;
                    e.hotspot_angle = data.hotspot_angle;
                    e.falloff_angle = data.falloff_angle;
                    e.cast_shadows = data.cast_shadows;
                    e.shadow_type = data.shadow_type;
                    e.shadow_map_size = data.shadow_map_size;
                    e.shadow_map_softness = data.shadow_map_softness;
                    e.photometric_mode = data.photometric_mode;
                    e.photometric_data = data.photometric_data;
                    let _ = document.add_entity(EntityType::Light(e));
                }
                OBJ_CAMERA
                | OBJ_SECTIONOBJECT
                | OBJ_ARCALIGNEDTEXT
                | OBJ_RTEXT
                | OBJ_GEOPOSITIONMARKER
                | OBJ_NAVISWORKSMODEL
                | OBJ_POINTCLOUD
                | OBJ_POINTCLOUDEX
                | OBJ_OLEFRAME
                | OBJ_PROXY_ENTITY => {
                    let data = match type_code {
                        OBJ_CAMERA => entities::read_camera(&mut reader),
                        OBJ_SECTIONOBJECT => entities::read_section_object(&mut reader),
                        OBJ_ARCALIGNEDTEXT => entities::read_arc_aligned_text(&mut reader),
                        OBJ_RTEXT => {
                            let mut data = entities::read_remote_text(&mut reader);
                            if let ExtendedEntityData::RemoteText(remote) = &mut data {
                                remote.style_name = maps.style_name(remote.style_handle.value());
                            }
                            data
                        }
                        OBJ_GEOPOSITIONMARKER => {
                            let mut marker = entities::read_geo_position_marker(
                                &mut reader,
                                self.obj_reader.version(),
                                self.obj_reader.dxf_version(),
                            );
                            marker.data.embedded_mtext = marker
                                .embedded_mtext
                                .map(|mtext| mtext_from_data(mtext, EntityCommon::new(), &maps));
                            ExtendedEntityData::GeoPositionMarker(marker.data)
                        }
                        OBJ_NAVISWORKSMODEL => entities::read_coordination_model(&mut reader),
                        OBJ_POINTCLOUD => entities::read_point_cloud(
                            &mut reader,
                            self.obj_reader.version(),
                            self.obj_reader.dxf_version(),
                        ),
                        OBJ_POINTCLOUDEX => entities::read_point_cloud_ex(&mut reader),
                        OBJ_OLEFRAME => {
                            entities::read_ole_frame(&mut reader, self.obj_reader.version())
                        }
                        OBJ_PROXY_ENTITY => entities::read_proxy_entity(
                            &mut reader,
                            self.obj_reader.version(),
                            self.obj_reader.dxf_version(),
                            entity_common.graphic_data.clone().unwrap_or_default(),
                            entity_common.handle.value(),
                        ),
                        _ => unreachable!(),
                    };
                    let _ = document.add_entity(EntityType::Extended(ExtendedEntity {
                        common: entity_common,
                        data,
                    }));
                }
                OBJ_ARC => {
                    let data = entities::read_arc(&mut reader);
                    let mut e = Arc::new();
                    e.common = entity_common;
                    e.center = data.center;
                    e.radius = data.radius;
                    e.start_angle = data.start_angle;
                    e.end_angle = data.end_angle;
                    e.thickness = data.thickness;
                    e.normal = data.normal;
                    let _ = document.add_entity(EntityType::Arc(e));
                }
                OBJ_ELLIPSE => {
                    let data = entities::read_ellipse(&mut reader);
                    let mut e = Ellipse::new();
                    e.common = entity_common;
                    e.center = data.center;
                    e.major_axis = data.major_axis;
                    e.minor_axis_ratio = data.minor_axis_ratio;
                    e.start_parameter = data.start_parameter;
                    e.end_parameter = data.end_parameter;
                    e.normal = data.normal;
                    let _ = document.add_entity(EntityType::Ellipse(e));
                }
                OBJ_RAY => {
                    let data = entities::read_ray(&mut reader);
                    let mut e = Ray::new(data.base_point, data.direction);
                    e.common = entity_common;
                    let _ = document.add_entity(EntityType::Ray(e));
                }
                OBJ_XLINE => {
                    let data = entities::read_xline(&mut reader);
                    let mut e = XLine::new(data.base_point, data.direction);
                    e.common = entity_common;
                    let _ = document.add_entity(EntityType::XLine(e));
                }
                OBJ_SOLID | OBJ_TRACE => {
                    let data = entities::read_solid(&mut reader);
                    let z = data.elevation;
                    let mut e = Solid::new(
                        crate::types::Vector3::new(data.first_corner.x, data.first_corner.y, z),
                        crate::types::Vector3::new(data.second_corner.x, data.second_corner.y, z),
                        crate::types::Vector3::new(data.third_corner.x, data.third_corner.y, z),
                        crate::types::Vector3::new(data.fourth_corner.x, data.fourth_corner.y, z),
                    );
                    e.common = entity_common;
                    e.thickness = data.thickness;
                    e.normal = data.normal;
                    e.is_trace = type_code == OBJ_TRACE;
                    let _ = document.add_entity(EntityType::Solid(e));
                }
                OBJ_3DFACE => {
                    let data = entities::read_face3d(&mut reader, self.obj_reader.version());
                    let mut e = Face3D::new(
                        data.first_corner,
                        data.second_corner,
                        data.third_corner,
                        data.fourth_corner,
                    );
                    // The reader already decoded the invisible-edge flags; the
                    // DXF path applies them but the DWG builder used to drop
                    // them, so file-hidden 3DFACE edges rendered visible.
                    e.invisible_edges = crate::entities::face3d::InvisibleEdgeFlags::from_bits(
                        data.invisible_edges as u8,
                    );
                    e.common = entity_common;
                    let _ = document.add_entity(EntityType::Face3D(e));
                }
                OBJ_SHAPE => {
                    let data = entities::read_shape(&mut reader);
                    let mut e = Shape::new();
                    e.common = entity_common;
                    e.insertion_point = data.insertion_point;
                    e.size = data.size;
                    e.rotation = data.rotation;
                    e.relative_x_scale = data.relative_x_scale;
                    e.oblique_angle = data.oblique_angle;
                    e.thickness = data.thickness;
                    e.shape_number = data.shape_number as i32;
                    e.normal = data.normal;
                    e.style_handle = Some(Handle::from(data.style_handle));
                    let _ = document.add_entity(EntityType::Shape(e));
                }

                // ── Moderate entities ──────────────────────────────
                OBJ_INSERT => {
                    let data = entities::read_insert(&mut reader, self.obj_reader.version());
                    let view_rep_handle = class_names
                        .dxf
                        .get(&raw_type_code)
                        .filter(|name| name.eq_ignore_ascii_case("ACDBVIEWREPBLOCKREFERENCE"))
                        .map(|_| Handle::from(reader.read_handle()));
                    let block_name = maps.block_name(data.block_handle);
                    let mut e = Insert::new(block_name, data.insert_point);
                    e.common = entity_common;
                    e.set_x_scale(data.x_scale);
                    e.set_y_scale(data.y_scale);
                    e.set_z_scale(data.z_scale);
                    e.rotation = data.rotation;
                    e.normal = data.normal;
                    e.view_rep_handle = view_rep_handle;
                    let _ = document.add_entity(EntityType::Insert(e));
                }
                OBJ_MINSERT => {
                    let data = entities::read_minsert(&mut reader, self.obj_reader.version());
                    let block_name = maps.block_name(data.insert.block_handle);
                    let mut e = Insert::new(block_name, data.insert.insert_point);
                    e.common = entity_common;
                    e.set_x_scale(data.insert.x_scale);
                    e.set_y_scale(data.insert.y_scale);
                    e.set_z_scale(data.insert.z_scale);
                    e.rotation = data.insert.rotation;
                    e.normal = data.insert.normal;
                    e.column_count = data.column_count as u16;
                    e.row_count = data.row_count as u16;
                    e.column_spacing = data.column_spacing;
                    e.row_spacing = data.row_spacing;
                    e.mark_as_minsert();
                    let _ = document.add_entity(EntityType::Insert(e));
                }
                OBJ_TABLE => {
                    // ACAD_TABLE is INSERT-derived: the insert base positions the
                    // table and links it to the block that renders its cells; on
                    // R2010+ the inline table content (columns/rows/cells) follows.
                    let data = entities::read_table(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                    );
                    let mut e = crate::entities::Table::default();
                    e.common = entity_common;
                    e.insertion_point = data.insert.insert_point;
                    e.normal = data.insert.normal;
                    e.horizontal_direction = data.horizontal_direction;
                    if data.insert.block_handle != 0 {
                        e.block_record_handle = Some(Handle::from(data.insert.block_handle));
                    }
                    if data.style_handle != 0 {
                        e.table_style_handle = Some(Handle::from(data.style_handle));
                    }
                    e.value_flags = data.value_flags;
                    e.columns = data.columns;
                    e.rows = data.rows;
                    e.name = data.name;
                    e.description = data.description;
                    e.field_handles = data.field_handles;
                    e.base_style = data.base_style;
                    e.merged_ranges = data.merged_ranges;
                    e.break_options = BreakOptionFlags::from_bits_retain(data.break_options as u32);
                    e.break_flow_direction =
                        BreakFlowDirection::from(data.break_flow_direction as u8);
                    e.break_spacing = data.break_spacing;
                    e.break_data = data.break_data;
                    e.break_ranges = data.break_ranges;
                    e.dwg_unknown_byte = data.unknown_byte;
                    e.dwg_unknown_handle =
                        (data.unknown_handle != 0).then(|| Handle::from(data.unknown_handle));
                    e.dwg_unknown_long1 = data.unknown_long1;
                    e.dwg_unknown_long2 = data.unknown_long2;
                    e.dwg_unknown_short = data.unknown_short;
                    e.override_flag = data.legacy_style_override.is_some();
                    e.override_border_color = data.legacy_border_colors.is_some();
                    e.override_border_line_weight = data.legacy_border_line_weights.is_some();
                    e.override_border_visibility = data.legacy_border_visibility.is_some();
                    e.legacy_style_override = data.legacy_style_override;
                    e.legacy_border_colors = data.legacy_border_colors;
                    e.legacy_border_line_weights = data.legacy_border_line_weights;
                    e.legacy_border_visibility = data.legacy_border_visibility;
                    let _ = document.add_entity(EntityType::Table(e));
                }
                OBJ_LWPOLYLINE => {
                    let data = entities::read_lwpolyline(&mut reader, self.obj_reader.version());
                    let mut e = LwPolyline::new();
                    e.common = entity_common;
                    e.vertices = data
                        .vertices
                        .into_iter()
                        .map(|v| crate::entities::lwpolyline::LwVertex {
                            location: crate::types::Vector2::new(v.x, v.y),
                            start_width: v.start_width,
                            end_width: v.end_width,
                            bulge: v.bulge,
                            vertex_id: v.vertex_id,
                        })
                        .collect();
                    e.elevation = data.elevation;
                    e.thickness = data.thickness;
                    e.constant_width = data.constant_width;
                    e.normal = data.normal;
                    e.is_closed = (data.flag & 0x200) != 0;
                    e.plinegen = (data.flag & 0x100) != 0;
                    let _ = document.add_entity(EntityType::LwPolyline(e));
                }
                OBJ_SPLINE => {
                    let data = entities::read_spline(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                    );
                    let mut e = Spline::new();
                    e.common = entity_common;
                    e.degree = data.degree;
                    e.flags.rational = data.rational;
                    e.flags.closed = data.closed;
                    e.flags.periodic = data.periodic;
                    e.knots = data.knots;
                    e.control_points = data.control_points;
                    e.weights = data.weights;
                    e.fit_points = data.fit_points;
                    e.knot_tolerance = data.knot_tolerance;
                    e.control_tolerance = data.control_tolerance;
                    e.fit_tolerance = data.fit_tolerance;
                    e.begin_tangent = data.begin_tangent;
                    e.end_tangent = data.end_tangent;
                    e.knot_parameterization = data.knot_param;
                    e.cv_frame_visible = data.flags1 & 2 != 0;
                    e.dwg_flags1 = data.flags1;
                    let _ = document.add_entity(EntityType::Spline(e));
                }
                OBJ_HELIX => {
                    // HELIX = full spline record + helix parameters.
                    let data = entities::read_spline(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                    );
                    let mut e = crate::entities::Helix::new();
                    e.common = entity_common;
                    e.spline.degree = data.degree;
                    e.spline.flags.rational = data.rational;
                    e.spline.flags.closed = data.closed;
                    e.spline.flags.periodic = data.periodic;
                    e.spline.knots = data.knots;
                    e.spline.control_points = data.control_points;
                    e.spline.weights = data.weights;
                    e.spline.fit_points = data.fit_points;
                    e.spline.knot_tolerance = data.knot_tolerance;
                    e.spline.control_tolerance = data.control_tolerance;
                    e.spline.fit_tolerance = data.fit_tolerance;
                    e.spline.begin_tangent = data.begin_tangent;
                    e.spline.end_tangent = data.end_tangent;
                    e.spline.knot_parameterization = data.knot_param;
                    e.spline.cv_frame_visible = data.flags1 & 2 != 0;
                    e.spline.dwg_flags1 = data.flags1;
                    // AcDbHelix parameters follow the spline record.
                    e.major_version = reader.read_bit_long();
                    e.maintenance_version = reader.read_bit_long();
                    e.axis_base_point = reader.read_3bit_double();
                    e.start_point = reader.read_3bit_double();
                    e.axis_vector = reader.read_3bit_double();
                    e.radius = reader.read_bit_double();
                    e.turns = reader.read_bit_double();
                    e.turn_height = reader.read_bit_double();
                    e.handedness = reader.read_bit();
                    e.constraint = crate::entities::HelixConstraint::from_code(reader.read_byte());
                    let _ = document.add_entity(EntityType::Helix(e));
                }
                OBJ_TEXT => {
                    let data = entities::read_text(&mut reader, self.obj_reader.version());
                    let mut e = Text::new();
                    e.common = entity_common;
                    e.value = data.value;
                    e.insertion_point = data.insertion_point;
                    e.height = data.height;
                    e.horizontal_alignment = match data.horizontal_alignment {
                        1 => TextHorizontalAlignment::Center,
                        2 => TextHorizontalAlignment::Right,
                        3 => TextHorizontalAlignment::Aligned,
                        4 => TextHorizontalAlignment::Middle,
                        5 => TextHorizontalAlignment::Fit,
                        _ => TextHorizontalAlignment::Left,
                    };
                    e.vertical_alignment = match data.vertical_alignment {
                        1 => TextVerticalAlignment::Bottom,
                        2 => TextVerticalAlignment::Middle,
                        3 => TextVerticalAlignment::Top,
                        _ => TextVerticalAlignment::Baseline,
                    };
                    // Only set alignment_point when alignment mode actually uses it
                    e.alignment_point =
                        if data.horizontal_alignment != 0 || data.vertical_alignment != 0 {
                            Some(data.alignment_point)
                        } else {
                            None
                        };
                    e.rotation = data.rotation;
                    e.oblique_angle = data.oblique_angle;
                    e.width_factor = data.width_factor;
                    e.normal = data.normal;
                    e.style = maps.style_name(data.style_handle);
                    e.thickness = data.thickness;
                    e.generation_flags = data.generation;
                    let _ = document.add_entity(EntityType::Text(e));
                }
                OBJ_MTEXT => {
                    let data = entities::read_mtext(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                    );
                    let e = mtext_from_data(data, entity_common, &maps);
                    let _ = document.add_entity(EntityType::MText(e));
                }
                OBJ_LEADER => {
                    let data = entities::read_leader(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                    );
                    let mut e = Leader::new();
                    e.common = entity_common;
                    e.vertices = data.vertices;
                    e.normal = data.normal;
                    e.horizontal_direction = data.horizontal_direction;
                    e.annotation_handle = Handle::from(data.annotation_handle);
                    e.dimension_style = maps.dimstyle_name(data.dimstyle_handle);
                    e.arrow_enabled = data.arrowhead_on;
                    e.path_type = LeaderPathType::from_value(data.path_type);
                    e.creation_type = LeaderCreationType::from_value(data.annotation_type);
                    e.hookline_direction =
                        HooklineDirection::from_value(data.hookline_on_x_dir as i16);
                    // R2010+ LEADER records do not carry the annotation box
                    // height/width. Keep Leader::new()'s semantic defaults
                    // instead of replacing them with the reader's absence
                    // sentinel (0.0).
                    if self.obj_reader.dxf_version() <= crate::types::DxfVersion::AC1021 {
                        e.text_height = data.text_height;
                        e.text_width = data.text_width;
                    }
                    e.block_offset = data.block_offset;
                    e.annotation_offset = data.annotation_offset;
                    e.origin = data.origin;
                    e.dimension_gap = data.dimgap;
                    e.arrowhead_type = data.arrowhead_type;
                    e.arrow_size = data.dimasz;
                    e.byblock_color = data.byblock_color;
                    e.dwg_unknown_bit1 = data.unknown_bit;
                    e.dwg_unknown_bit2 = data.unknown_bit2;
                    e.dwg_unknown_bit3 = data.unknown_bit3;
                    e.dwg_unknown_bit4 = data.unknown_bit4;
                    e.dwg_unknown_bit5 = data.unknown_bit5;
                    e.dwg_unknown_short1 = data.unknown_short1;
                    e.hookline_enabled =
                        self.obj_reader.version().r13_14_only() && (data.arrowhead_type & 8) != 0;
                    let _ = document.add_entity(EntityType::Leader(e));
                }
                OBJ_TOLERANCE => {
                    let data = entities::read_tolerance(&mut reader, self.obj_reader.version());
                    let mut e = Tolerance::new();
                    e.common = entity_common;
                    e.insertion_point = data.insertion_point;
                    e.text = data.text;
                    e.direction = data.direction;
                    e.normal = data.normal;
                    e.dimension_style_handle = Some(Handle::from(data.dimstyle_handle));
                    e.dimension_style_name.clear();
                    e.dwg_unknown_short = data.unknown_short;
                    e.text_height = data.text_height;
                    e.dimension_gap = data.dimgap;
                    let _ = document.add_entity(EntityType::Tolerance(e));
                }

                // ── Complex entities ───────────────────────────────
                OBJ_HATCH | OBJ_MPOLYGON => {
                    let data = if type_code == OBJ_MPOLYGON {
                        entities::read_mpolygon(&mut reader, self.obj_reader.version())
                    } else {
                        entities::read_hatch(&mut reader, self.obj_reader.version())
                    };
                    let mut e = Hatch::new();
                    e.common = entity_common;
                    e.is_mpolygon = data.is_mpolygon;
                    e.elevation = data.elevation;
                    e.normal = data.normal;
                    let mut pat = HatchPattern::new(&data.pattern_name);
                    pat.lines = data
                        .pattern_lines
                        .into_iter()
                        .map(|pl| crate::entities::hatch::HatchPatternLine {
                            angle: pl.angle,
                            base_point: pl.base_point,
                            offset: pl.offset,
                            dash_lengths: pl.dashes,
                        })
                        .collect();
                    e.pattern = pat;
                    e.is_solid = data.is_solid;
                    e.is_associative = data.is_associative;
                    e.is_double = data.is_double;
                    e.pattern_angle = data.pattern_angle;
                    e.pattern_scale = data.pattern_scale;
                    e.pattern_type = match data.pattern_type {
                        0 => crate::entities::hatch::HatchPatternType::UserDefined,
                        2 => crate::entities::hatch::HatchPatternType::Custom,
                        _ => crate::entities::hatch::HatchPatternType::Predefined,
                    };
                    e.style = match data.style {
                        1 => crate::entities::hatch::HatchStyleType::Outer,
                        2 => crate::entities::hatch::HatchStyleType::Ignore,
                        _ => crate::entities::hatch::HatchStyleType::Normal,
                    };
                    e.pixel_size = data.pixel_size;
                    // Collect boundary handle counts before consuming paths
                    let boundary_handle_counts: Vec<i32> =
                        data.paths.iter().map(|p| p.boundary_handle_count).collect();
                    e.paths = data.paths.into_iter().map(boundary_path_from_dwg).collect();
                    e.seed_points = data.seed_points;
                    e.mpolygon_hatch_color = data.mpolygon_hatch_color;
                    e.mpolygon_x_direction = data.mpolygon_x_direction;
                    e.mpolygon_boundary_handle_count = data.mpolygon_boundary_handle_count;
                    // Map gradient data
                    e.gradient_color.enabled = data.gradient_enabled;
                    e.gradient_color.reserved = data.gradient_reserved;
                    e.gradient_color.angle = data.gradient_angle;
                    e.gradient_color.shift = data.gradient_shift;
                    e.gradient_color.is_single_color = data.gradient_single_color;
                    e.gradient_color.color_tint = data.gradient_tint;
                    e.gradient_color.colors = data
                        .gradient_colors
                        .into_iter()
                        .map(
                            |(value, color)| crate::entities::hatch::GradientColorEntry {
                                value,
                                color,
                            },
                        )
                        .collect();
                    e.gradient_color.name = data.gradient_name;
                    // Read boundary object handles from handle stream
                    for (path, &count) in e.paths.iter_mut().zip(boundary_handle_counts.iter()) {
                        for _ in 0..count {
                            let h = reader.read_handle();
                            if h != 0 {
                                path.add_boundary_handle(Handle::new(h));
                            }
                        }
                    }
                    let _ = document.add_entity(EntityType::Hatch(e));
                }
                OBJ_VIEWPORT => {
                    let data = entities::read_viewport(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                    );
                    let mut e = Viewport::new();
                    e.common = entity_common;
                    e.center = data.center;
                    e.width = data.width;
                    e.height = data.height;
                    e.view_center =
                        crate::types::Vector3::new(data.view_center.x, data.view_center.y, 0.0);
                    e.view_direction = data.view_direction;
                    e.view_target = data.view_target;
                    e.view_height = data.view_height;
                    e.lens_length = data.lens_length;
                    e.front_clip_z = data.front_clip_z;
                    e.back_clip_z = data.back_clip_z;
                    e.twist_angle = data.twist_angle;
                    e.snap_angle = data.snap_angle;
                    e.snap_base =
                        crate::types::Vector3::new(data.snap_base.x, data.snap_base.y, 0.0);
                    e.snap_spacing =
                        crate::types::Vector3::new(data.snap_spacing.x, data.snap_spacing.y, 0.0);
                    e.grid_spacing =
                        crate::types::Vector3::new(data.grid_spacing.x, data.grid_spacing.y, 0.0);
                    e.circle_sides = data.circle_sides;
                    if self.obj_reader.version().r2007_plus() {
                        e.grid_major = data.grid_major;
                    }
                    e.status = ViewportStatusFlags::from_bits(data.status_flags);
                    e.style_sheet = data.style_sheet;
                    e.render_mode = ViewportRenderMode::from_value(data.render_mode as i16);
                    e.ucs_at_origin = data.ucs_at_origin;
                    e.ucs_per_viewport = data.ucs_per_viewport;
                    e.ucs_origin = data.ucs_origin;
                    e.ucs_x_axis = data.ucs_x_axis;
                    e.ucs_y_axis = data.ucs_y_axis;
                    e.elevation = data.ucs_elevation;
                    e.ucs_ortho_type = data.ucs_ortho_type;
                    if self.obj_reader.version().r2004_plus() {
                        e.shade_plot_mode = data.shade_plot_mode;
                    }
                    if self.obj_reader.version().r2007_plus() {
                        e.default_lighting = data.default_lighting;
                        e.default_lighting_type = data.default_lighting_type as i16;
                        e.brightness = data.brightness;
                        e.contrast = data.contrast;
                        e.ambient_color = data.ambient_color;
                    }
                    // Read frozen layer handles
                    for _ in 0..data.frozen_layer_count {
                        let h = reader.read_handle();
                        if h != 0 {
                            e.frozen_layers.push(Handle::new(h));
                        }
                    }
                    // Clip-boundary handle (H 340): first entity-specific handle
                    // after the frozen layers. Non-NULL => the viewport is
                    // clipped by a boundary entity.
                    let clip = reader.read_handle();
                    if clip != 0 {
                        e.clip_boundary_handle = Handle::new(clip);
                    }
                    // R2000 carries an obsolete viewport-entity-header handle.
                    if self.obj_reader.version() == crate::io::dwg::dwg_version::DwgVersion::AC15 {
                        let _ = reader.read_handle();
                    }
                    let ucs = reader.read_handle();
                    if ucs != 0 {
                        e.ucs_handle = Handle::new(ucs);
                    }
                    let base_ucs = reader.read_handle();
                    if base_ucs != 0 {
                        e.base_ucs_handle = Handle::new(base_ucs);
                    }
                    if self.obj_reader.version().r2007_plus() {
                        let background = reader.read_handle();
                        let visual_style = reader.read_handle();
                        let shade_plot = reader.read_handle();
                        let sun = reader.read_handle();
                        if background != 0 {
                            e.background_handle = Handle::new(background);
                        }
                        if visual_style != 0 {
                            e.visual_style_handle = Handle::new(visual_style);
                        }
                        if shade_plot != 0 {
                            e.shade_plot_handle = Handle::new(shade_plot);
                        }
                        if sun != 0 {
                            e.sun_handle = Handle::new(sun);
                        }
                    }
                    let _ = document.add_entity(EntityType::Viewport(e));
                }
                OBJ_POLYLINE_2D => {
                    let data = entities::read_polyline2d(&mut reader, self.obj_reader.version());
                    let mut e = Polyline2D::new();
                    e.common = entity_common;
                    e.flags = PolylineFlags::from_bits(data.flags as u16);
                    e.smooth_surface = SmoothSurfaceType::from(data.smooth_surface);
                    e.elevation = data.elevation;
                    e.thickness = data.thickness;
                    e.normal = data.normal;
                    e.start_width = data.start_width;
                    e.end_width = data.end_width;
                    let h = e.common.handle.value();
                    pending.polylines.push((h, EntityType::Polyline2D(e)));
                }
                OBJ_POLYLINE_3D => {
                    let data = entities::read_polyline3d(&mut reader, self.obj_reader.version());
                    let mut e = Polyline3D::new();
                    e.common = entity_common;
                    e.flags.closed = (data.closed_flag & 1) != 0;
                    // smooth_type was decoded by the reader but the builder used
                    // to drop it (spline/curve-fit 3D polylines lost their fit).
                    e.smooth_type = crate::entities::polyline3d::SmoothSurfaceType::from_value(
                        data.smooth_type as i16,
                    );
                    e.flags.spline_fit = data.smooth_type != 0;
                    let h = e.common.handle.value();
                    pending.polylines.push((h, EntityType::Polyline3D(e)));
                }

                // ── Dimension types ────────────────────────────────
                OBJ_DIMENSION_LINEAR => {
                    let data = entities::read_dimension_linear(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                    );
                    let mut dim = DimensionLinear::new(data.first_point, data.second_point);
                    dim.base.common = entity_common;
                    map_dimension_common(&mut dim.base, &data.common, &maps);
                    dim.definition_point = data.definition_point;
                    dim.base.definition_point = data.definition_point;
                    dim.rotation = data.rotation;
                    dim.ext_line_rotation = data.ext_line_rotation;
                    let _ = document.add_entity(EntityType::Dimension(Dimension::Linear(dim)));
                }
                OBJ_DIMENSION_ALIGNED => {
                    let data = entities::read_dimension_aligned(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                    );
                    let mut dim = DimensionAligned::new(data.first_point, data.second_point);
                    dim.base.common = entity_common;
                    map_dimension_common(&mut dim.base, &data.common, &maps);
                    dim.definition_point = data.definition_point;
                    dim.base.definition_point = data.definition_point;
                    dim.ext_line_rotation = data.ext_line_rotation;
                    let _ = document.add_entity(EntityType::Dimension(Dimension::Aligned(dim)));
                }
                OBJ_DIMENSION_RADIUS => {
                    let data = entities::read_dimension_radius(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                    );
                    let mut dim = DimensionRadius::new(data.angle_vertex, data.definition_point);
                    dim.base.common = entity_common;
                    map_dimension_common(&mut dim.base, &data.common, &maps);
                    dim.base.definition_point = data.definition_point;
                    dim.leader_length = data.leader_length;
                    let _ = document.add_entity(EntityType::Dimension(Dimension::Radius(dim)));
                }
                OBJ_DIMENSION_DIAMETER => {
                    let data = entities::read_dimension_diameter(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                    );
                    let mut dim = DimensionDiameter::new(data.angle_vertex, data.definition_point);
                    dim.base.common = entity_common;
                    map_dimension_common(&mut dim.base, &data.common, &maps);
                    dim.base.definition_point = data.definition_point;
                    dim.leader_length = data.leader_length;
                    let _ = document.add_entity(EntityType::Dimension(Dimension::Diameter(dim)));
                }
                OBJ_DIMENSION_ANG_2LN => {
                    let data = entities::read_dimension_angular_2ln(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                    );
                    let mut dim = DimensionAngular2Ln::default();
                    dim.base.common = entity_common;
                    map_dimension_common(&mut dim.base, &data.common, &maps);
                    dim.dimension_arc =
                        crate::types::Vector3::new(data.dimension_arc.x, data.dimension_arc.y, 0.0);
                    dim.first_point = data.first_point;
                    dim.second_point = data.second_point;
                    dim.angle_vertex = data.angle_vertex;
                    dim.definition_point = data.definition_point;
                    dim.base.definition_point = data.definition_point;
                    let _ = document.add_entity(EntityType::Dimension(Dimension::Angular2Ln(dim)));
                }
                OBJ_DIMENSION_ANG_3PT => {
                    let data = entities::read_dimension_angular_3pt(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                    );
                    let mut dim = DimensionAngular3Pt::default();
                    dim.base.common = entity_common;
                    map_dimension_common(&mut dim.base, &data.common, &maps);
                    dim.first_point = data.first_point;
                    dim.second_point = data.second_point;
                    dim.angle_vertex = data.angle_vertex;
                    dim.definition_point = data.definition_point;
                    dim.base.definition_point = data.definition_point;
                    let _ = document.add_entity(EntityType::Dimension(Dimension::Angular3Pt(dim)));
                }
                OBJ_DIMENSION_ORDINATE => {
                    let data = entities::read_dimension_ordinate(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                    );
                    let mut dim = DimensionOrdinate::new(
                        data.feature_location,
                        data.leader_endpoint,
                        data.is_ordinate_type_x,
                    );
                    dim.base.common = entity_common;
                    map_dimension_common(&mut dim.base, &data.common, &maps);
                    dim.definition_point = data.definition_point;
                    dim.base.definition_point = data.definition_point;
                    dim.refresh_measurement();
                    let _ = document.add_entity(EntityType::Dimension(Dimension::Ordinate(dim)));
                }
                OBJ_ARC_DIMENSION => {
                    let data = entities::read_dimension_arc(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                    );
                    let mut dim = DimensionArc::default();
                    dim.base.common = entity_common;
                    map_dimension_common(&mut dim.base, &data.common, &maps);
                    dim.definition_point = data.definition_point;
                    dim.base.definition_point = data.definition_point;
                    dim.first_extension_point = data.first_extension_point;
                    dim.second_extension_point = data.second_extension_point;
                    dim.center_point = data.center_point;
                    dim.is_partial = data.is_partial;
                    dim.arc_start_parameter = data.arc_start_parameter;
                    dim.arc_end_parameter = data.arc_end_parameter;
                    dim.has_leader = data.has_leader;
                    dim.first_leader_point = data.first_leader_point;
                    dim.second_leader_point = data.second_leader_point;
                    let _ = document.add_entity(EntityType::Dimension(Dimension::Arc(dim)));
                }
                OBJ_LARGE_RADIAL_DIMENSION => {
                    let data = entities::read_dimension_large_radial(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                    );
                    let mut dim = DimensionLargeRadial::default();
                    dim.base.common = entity_common;
                    map_dimension_common(&mut dim.base, &data.common, &maps);
                    dim.definition_point = data.definition_point;
                    dim.base.definition_point = data.definition_point;
                    dim.chord_point = data.chord_point;
                    dim.jog_angle = data.jog_angle;
                    dim.override_center = data.override_center;
                    dim.jog_point = data.jog_point;
                    let _ = document.add_entity(EntityType::Dimension(Dimension::LargeRadial(dim)));
                }

                OBJ_MLINE => {
                    let data = entities::read_mline(&mut reader);
                    let mut e = MLine::new();
                    e.common = entity_common;
                    e.scale_factor = data.scale_factor;
                    e.justification = MLineJustification::from(data.justification as i16);
                    e.start_point = data.start_point;
                    e.normal = data.normal;
                    e.style_element_count = data.lines_in_style as usize;
                    // Link the entity to its MLINESTYLE via the hard-pointer handle
                    // read from the handle stream. Without this the entity keeps the
                    // `MLine::new()` default ("Standard" / no handle), so a drawing's
                    // custom multiline style (element offsets, per-line colours and
                    // linetypes) is lost and the multiline is drawn with Standard's
                    // ±0.5 offsets in the entity colour.
                    if data.style_handle != 0 {
                        let sh = Handle::new(data.style_handle);
                        e.style_handle = Some(sh);
                    }
                    // Populate vertices from parsed data
                    e.vertices = data
                        .vertices
                        .into_iter()
                        .map(|vd| {
                            use crate::entities::mline::{MLineSegment, MLineVertex};
                            let mut mv = MLineVertex::new(vd.position);
                            mv.direction = vd.direction;
                            mv.miter = vd.miter;
                            mv.segments = vd
                                .segments
                                .into_iter()
                                .map(|sd| MLineSegment {
                                    parameters: sd.parameters,
                                    area_fill_parameters: sd.area_fill_parameters,
                                })
                                .collect();
                            mv
                        })
                        .collect();
                    let _ = document.add_entity(EntityType::MLine(e));
                }

                OBJ_POLYLINE_PFACE => {
                    let (_num_verts, _num_faces, _owned_count) =
                        entities::read_polyface_mesh(&mut reader, self.obj_reader.version());
                    let mut e = PolyfaceMesh::new();
                    e.common = entity_common;
                    let h = e.common.handle.value();
                    pending.polylines.push((h, EntityType::PolyfaceMesh(e)));
                }

                OBJ_MESH => {
                    let data = entities::read_mesh(&mut reader);
                    let mut e = Mesh::new();
                    e.common = entity_common;
                    e.version = data.version;
                    e.blend_crease = data.blend_crease;
                    e.subdivision_level = data.subdivision_level;
                    e.vertices = data.vertices;
                    e.faces = data
                        .faces
                        .into_iter()
                        .map(|f| MeshFace {
                            vertices: f.into_iter().map(|v| v as usize).collect(),
                        })
                        .collect();
                    e.edges = data
                        .edges
                        .into_iter()
                        .enumerate()
                        .map(|(i, (a, b))| MeshEdge {
                            start: a as usize,
                            end: b as usize,
                            crease: data.crease_values.get(i).copied().filter(|v| *v != 0.0),
                        })
                        .collect();
                    e.override_option = data.override_option;
                    let _ = document.add_entity(EntityType::Mesh(e));
                }

                OBJ_MULTILEADER => {
                    let data = entities::read_multileader(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                    );
                    let mut e = MultiLeader::new();
                    e.common = entity_common;
                    e.dwg_version = data.dwg_version;
                    e.context = data.context;
                    e.style_handle = if data.style_handle != 0 {
                        Some(Handle::from(data.style_handle))
                    } else {
                        None
                    };
                    // Retain (not truncate) so flag bits the enum doesn't name
                    // are preserved for a lossless re-write.
                    e.property_override_flags = MultiLeaderPropertyOverrideFlags::from_bits_retain(
                        data.property_override_flags,
                    );
                    e.path_type = MultiLeaderPathType::from(data.path_type);
                    e.line_color = data.line_color;
                    e.line_type_handle = if data.line_type_handle != 0 {
                        Some(Handle::from(data.line_type_handle))
                    } else {
                        None
                    };
                    e.line_weight = LineWeight::from_value(data.line_weight as i16);
                    e.enable_landing = data.enable_landing;
                    e.enable_dogleg = data.enable_dogleg;
                    e.dogleg_length = data.dogleg_length;
                    e.arrowhead_handle = if data.arrowhead_handle != 0 {
                        Some(Handle::from(data.arrowhead_handle))
                    } else {
                        None
                    };
                    e.arrowhead_size = data.arrowhead_size;
                    e.content_type = LeaderContentType::from(data.content_type);
                    e.text_style_handle = if data.text_style_handle != 0 {
                        Some(Handle::from(data.text_style_handle))
                    } else {
                        None
                    };
                    e.text_left_attachment = TextAttachmentType::from(data.text_left_attachment);
                    e.text_right_attachment = TextAttachmentType::from(data.text_right_attachment);
                    e.text_angle_type = TextAngleType::from(data.text_angle_type);
                    e.text_alignment = TextAlignmentType::from(data.text_alignment);
                    e.text_color = data.text_color;
                    e.text_frame = data.text_frame;
                    e.block_content_handle = if data.block_content_handle != 0 {
                        Some(Handle::from(data.block_content_handle))
                    } else {
                        None
                    };
                    e.block_content_color = data.block_content_color;
                    e.block_scale = data.block_scale;
                    e.block_rotation = data.block_rotation;
                    e.block_connection_type =
                        BlockContentConnectionType::from(data.block_connection_type);
                    e.enable_annotation_scale = data.enable_annotation_scale;
                    e.block_attributes = data.block_attributes;
                    e.arrowhead_overrides = data.arrowhead_overrides;
                    e.text_direction_negative = data.text_direction_negative;
                    e.text_align_in_ipe = data.text_align_in_ipe;
                    e.text_attachment_point =
                        TextAttachmentPointType::from(data.text_attachment_point);
                    e.scale_factor = data.scale_factor;
                    e.text_attachment_direction =
                        TextAttachmentDirectionType::from(data.text_attachment_direction);
                    e.text_bottom_attachment =
                        TextAttachmentType::from(data.text_bottom_attachment);
                    e.text_top_attachment = TextAttachmentType::from(data.text_top_attachment);
                    e.extend_leader_to_text = data.extend_leader_to_text;
                    let _ = document.add_entity(EntityType::MultiLeader(e));
                }

                // ── Attribute entities ─────────────────────────────
                OBJ_ATTDEF => {
                    let data = entities::read_attribute_definition(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                    );
                    let mut e = AttributeDefinition::new(
                        data.tag.clone(),
                        data.prompt.clone(),
                        data.text_data.value.clone(),
                    );
                    e.common = entity_common;
                    e.insertion_point = data.text_data.insertion_point;
                    e.height = data.text_data.height;
                    e.rotation = data.text_data.rotation;
                    // Carry the full text geometry the reader parsed — same as
                    // ATTRIB. Without these the attribute reverts to
                    // left/baseline default width/oblique/style and, crucially,
                    // loses its flags, so a CONSTANT attribute (whose value is
                    // drawn straight from the block, with no ATTRIB) is treated
                    // as a plain template and never rendered.
                    e.horizontal_alignment = match data.text_data.horizontal_alignment {
                        1 => HorizontalAlignment::Center,
                        2 => HorizontalAlignment::Right,
                        3 => HorizontalAlignment::Aligned,
                        4 => HorizontalAlignment::Middle,
                        5 => HorizontalAlignment::Fit,
                        _ => HorizontalAlignment::Left,
                    };
                    e.vertical_alignment = match data.text_data.vertical_alignment {
                        1 => VerticalAlignment::Bottom,
                        2 => VerticalAlignment::Middle,
                        3 => VerticalAlignment::Top,
                        _ => VerticalAlignment::Baseline,
                    };
                    e.alignment_point = if data.text_data.horizontal_alignment != 0
                        || data.text_data.vertical_alignment != 0
                    {
                        data.text_data.alignment_point
                    } else {
                        crate::types::Vector3::ZERO
                    };
                    e.width_factor = data.text_data.width_factor;
                    e.oblique_angle = data.text_data.oblique_angle;
                    e.normal = data.text_data.normal;
                    e.text_style = maps.style_name(data.text_data.style_handle);
                    e.flags = AttributeFlags::from_bits(data.flags as i32);
                    e.text_generation_flags = data.text_data.generation;
                    e.field_length = data.field_length;
                    e.lock_position = data.lock_position;
                    e.mtext_flag = MTextFlag::from_value(data.att_type as i16);
                    e.is_multiline = data.att_type > 1;
                    e.line_count = e.default_value.matches("\\P").count() as i16 + 1;
                    e.embedded_mtext = data.embedded_mtext.map(|mtext| {
                        Box::new(mtext_from_data(mtext, EntityCommon::default(), &maps))
                    });
                    let _ = document.add_entity(EntityType::AttributeDefinition(e));
                }
                OBJ_ATTRIB => {
                    let data = entities::read_attribute_entity(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                    );
                    let mut e =
                        AttributeEntity::new(data.tag.clone(), data.text_data.value.clone());
                    e.common = entity_common;
                    e.insertion_point = data.text_data.insertion_point;
                    e.height = data.text_data.height;
                    e.rotation = data.text_data.rotation;
                    // Carry the full text geometry the reader parsed. Without
                    // these the attribute reverts to left/baseline with no
                    // alignment point (DataFlags 0x02|0x40), discarding the
                    // real placement — AutoCAD's R2018 reader rejects it.
                    e.horizontal_alignment = match data.text_data.horizontal_alignment {
                        1 => HorizontalAlignment::Center,
                        2 => HorizontalAlignment::Right,
                        3 => HorizontalAlignment::Aligned,
                        4 => HorizontalAlignment::Middle,
                        5 => HorizontalAlignment::Fit,
                        _ => HorizontalAlignment::Left,
                    };
                    e.vertical_alignment = match data.text_data.vertical_alignment {
                        1 => VerticalAlignment::Bottom,
                        2 => VerticalAlignment::Middle,
                        3 => VerticalAlignment::Top,
                        _ => VerticalAlignment::Baseline,
                    };
                    // Match the writer/reader convention: an alignment point is
                    // only meaningful when the text is not left/baseline.
                    e.alignment_point = if data.text_data.horizontal_alignment != 0
                        || data.text_data.vertical_alignment != 0
                    {
                        data.text_data.alignment_point
                    } else {
                        crate::types::Vector3::ZERO
                    };
                    e.width_factor = data.text_data.width_factor;
                    e.oblique_angle = data.text_data.oblique_angle;
                    e.normal = data.text_data.normal;
                    e.text_style = maps.style_name(data.text_data.style_handle);
                    // Carry the flag byte the reader parsed. Dropping it left
                    // `flags.invisible` false, so an attribute tagged invisible
                    // (ATTMODE 1 should hide it) was still drawn. Also carry the
                    // text-generation (backward / upside-down), field length and
                    // lock-position, which were likewise being discarded.
                    e.flags = AttributeFlags::from_bits(data.flags as i32);
                    e.text_generation_flags = data.text_data.generation;
                    e.field_length = data.field_length;
                    e.lock_position = data.lock_position;
                    e.mtext_flag = MTextFlag::from_value(data.att_type as i16);
                    e.is_multiline = data.att_type > 1;
                    e.line_count = e.value.matches("\\P").count() as i16 + 1;
                    e.embedded_mtext = data.embedded_mtext.map(|mtext| {
                        Box::new(mtext_from_data(mtext, EntityCommon::default(), &maps))
                    });
                    // Collect pending — will be attached to parent INSERT
                    // after Pass 2 (owner_handle = INSERT handle).
                    pending_attributes
                        .entry(entity_data.owner_handle)
                        .or_default()
                        .push(e);
                }

                // ── Structural markers (BLOCK / ENDBLK / SEQEND) ──
                // These are DWG-internal structural entities. They mark
                // block boundaries and sequence terminators. They are
                // silently consumed — their information is already
                // represented by BlockRecord table entries.
                OBJ_BLOCK => {
                    // BLOCK entity: read block name after common entity data
                    let name = reader.read_variable_text();
                    let mut b = crate::entities::Block::new(name, crate::types::Vector3::ZERO);
                    b.common = entity_common;
                    let _ = document.add_entity(EntityType::Block(b));
                }
                OBJ_ENDBLK => {
                    // ENDBLK marks the end of a block definition.
                    let mut be = crate::entities::BlockEnd::new();
                    be.common = entity_common;
                    let _ = document.add_entity(EntityType::BlockEnd(be));
                }
                OBJ_SEQEND => {
                    // SEQEND terminates a polyline vertex or INSERT
                    // attribute sequence. Store the seqend handle so
                    // it can be preserved on the parent polyline.
                    entities::read_seqend(&mut reader);
                    pending
                        .seqends
                        .insert(entity_data.owner_handle, entity_common.handle);
                }

                // ── Vertex child entities ──────────────────────────
                // Vertex records are children of POLYLINE_2D,
                // POLYLINE_3D, POLYLINE_PFACE, or POLYLINE_MESH.
                // Collect vertex data and attach to parent polylines
                // in the post-processing step after Pass 2.
                OBJ_VERTEX_2D => {
                    let mut data = entities::read_vertex2d(&mut reader, self.obj_reader.version());
                    data.handle = entity_common.handle;
                    pending
                        .vertices
                        .entry(entity_data.owner_handle)
                        .or_default()
                        .push(PendingVertex::V2D(data));
                }
                OBJ_VERTEX_3D | OBJ_VERTEX_MESH => {
                    let mut data = entities::read_vertex3d(&mut reader);
                    data.handle = entity_common.handle;
                    pending
                        .vertices
                        .entry(entity_data.owner_handle)
                        .or_default()
                        .push(PendingVertex::V3D(data, entity_common));
                }
                OBJ_VERTEX_PFACE => {
                    let mut data = entities::read_vertex3d(&mut reader);
                    data.handle = entity_common.handle;
                    pending
                        .vertices
                        .entry(entity_data.owner_handle)
                        .or_default()
                        .push(PendingVertex::V3D(data, entity_common));
                }
                OBJ_VERTEX_PFACE_FACE => {
                    let mut data = entities::read_pface_face(&mut reader);
                    data.handle = entity_common.handle;
                    pending
                        .vertices
                        .entry(entity_data.owner_handle)
                        .or_default()
                        .push(PendingVertex::PfaceFace(data, entity_common));
                }

                // ── Underlay reference (PDF / DWF / DGN) ───────────
                code @ (OBJ_PDFUNDERLAY | OBJ_DWFUNDERLAY | OBJ_DGNUNDERLAY) => {
                    use crate::entities::underlay::{Underlay, UnderlayDisplayFlags, UnderlayType};
                    let utype = if code == OBJ_DWFUNDERLAY {
                        UnderlayType::Dwf
                    } else if code == OBJ_DGNUNDERLAY {
                        UnderlayType::Dgn
                    } else {
                        UnderlayType::Pdf
                    };
                    let data = entities::read_underlay(&mut reader);
                    let mut e = Underlay::new(utype);
                    e.common = entity_common;
                    e.normal = data.normal;
                    e.insertion_point = data.insertion_point;
                    e.rotation = data.rotation;
                    e.x_scale = data.x_scale;
                    e.y_scale = data.y_scale;
                    e.z_scale = data.z_scale;
                    let eflags = UnderlayDisplayFlags::from_bits_truncate(data.flags);
                    e.flags = eflags;
                    // The "clip inside" bit doubles as the clip-inversion flag.
                    e.clip_inverted = eflags.contains(UnderlayDisplayFlags::CLIP_INSIDE);
                    e.contrast = data.contrast;
                    e.fade = data.fade;
                    if data.definition_handle != 0 {
                        e.definition_handle = Handle::from(data.definition_handle);
                    }
                    e.clip_boundary_vertices = data.clip_boundary_vertices;
                    let _ = document.add_entity(EntityType::Underlay(e));
                }

                // ── Raster image / Wipeout ─────────────────────────
                OBJ_IMAGE => {
                    let data = entities::read_raster_image(&mut reader, self.obj_reader.version());
                    let mut e =
                        RasterImage::new("", data.insertion_point, data.size.x, data.size.y);
                    e.common = entity_common;
                    e.class_version = data.class_version;
                    e.u_vector = data.u_vector;
                    e.v_vector = data.v_vector;
                    e.flags = ImageDisplayFlags::from_bits_truncate(data.flags);
                    e.clipping_enabled = data.clipping_enabled;
                    e.brightness = data.brightness;
                    e.contrast = data.contrast;
                    e.fade = data.fade;
                    // Propagate clip boundary the same way Wipeout does — the
                    // parser used to discard the vertices, leaving the default
                    // boundary on the entity. Without this, clip regions
                    // shrink/expand by orders of magnitude on render.
                    e.clip_boundary = crate::entities::raster_image::ClipBoundary {
                        clip_type: if data.clip_type == 1 {
                            crate::entities::raster_image::ClipType::Rectangular
                        } else {
                            crate::entities::raster_image::ClipType::Polygonal
                        },
                        clip_mode: if data.clip_inverted {
                            crate::entities::raster_image::ClipMode::Inside
                        } else {
                            crate::entities::raster_image::ClipMode::Outside
                        },
                        vertices: data.clip_boundary_vertices,
                    };
                    if data.definition_handle != 0 {
                        e.definition_handle = Some(Handle::from(data.definition_handle));
                    }
                    if data.reactor_handle != 0 {
                        e.definition_reactor_handle = Some(Handle::from(data.reactor_handle));
                    }
                    let _ = document.add_entity(EntityType::RasterImage(e));
                }
                OBJ_WIPEOUT => {
                    let data = entities::read_wipeout(&mut reader, self.obj_reader.version());
                    let mut e = Wipeout::new();
                    e.common = entity_common;
                    e.class_version = data.class_version;
                    e.insertion_point = data.insertion_point;
                    e.u_vector = data.u_vector;
                    e.v_vector = data.v_vector;
                    e.size = data.size;
                    e.flags = WipeoutDisplayFlags::from_bits_truncate(data.flags);
                    e.clipping_enabled = data.clipping_enabled;
                    e.brightness = data.brightness;
                    e.contrast = data.contrast;
                    e.fade = data.fade;
                    e.clip_mode = if data.clip_inverted {
                        crate::entities::WipeoutClipMode::Inside
                    } else {
                        crate::entities::WipeoutClipMode::Outside
                    };
                    e.clip_type = if data.clip_type == 1 {
                        crate::entities::WipeoutClipType::Rectangular
                    } else {
                        crate::entities::WipeoutClipType::Polygonal
                    };
                    e.clip_boundary_vertices = data.clip_boundary_vertices;
                    if data.definition_handle != 0 {
                        e.definition_handle = Some(Handle::from(data.definition_handle));
                    }
                    if data.reactor_handle != 0 {
                        e.definition_reactor_handle = Some(Handle::from(data.reactor_handle));
                    }
                    let _ = document.add_entity(EntityType::Wipeout(e));
                }

                // ── OLE2 Frame ──────────────────────────────────────
                OBJ_OLE2FRAME => {
                    let data = entities::read_ole2frame(&mut reader, self.obj_reader.version());
                    let mut e = Ole2Frame::new();
                    e.common = entity_common;
                    e.ole_object_type = OleObjectType::from_i16(data.object_type);
                    e.upper_left_corner = data.upper_left;
                    e.lower_right_corner = data.lower_right;
                    e.storage = data.storage;
                    e.envelope = data.envelope;
                    e.dwg_mode = data.mode;
                    e.is_paper_space = data.mode == 1;
                    e.lock_aspect = data.lock_aspect;
                    let _ = document.add_entity(EntityType::Ole2Frame(e));
                }

                // ── Polygon mesh (POLYLINE with mesh flag) ──────────
                OBJ_POLYLINE_MESH => {
                    let (flags, smooth_type, m_count, n_count, m_smooth, n_smooth, _owned_count) =
                        entities::read_polygon_mesh(&mut reader, self.obj_reader.version());
                    let mut e = PolygonMeshEntity::new();
                    e.common = entity_common;
                    e.flags = PolygonMeshFlags::from_bits_truncate(flags);
                    e.m_vertex_count = m_count;
                    e.n_vertex_count = n_count;
                    e.m_smooth_density = m_smooth;
                    e.n_smooth_density = n_smooth;
                    e.smooth_type = SurfaceSmoothType::from_i16(smooth_type);
                    // Vertices will be assembled from VERTEX_MESH records
                    let poly_handle = entity_data.common.handle;
                    pending
                        .polylines
                        .push((poly_handle, EntityType::PolygonMesh(e)));
                }

                // ── ACIS entities (3DSOLID, REGION, BODY) ───────────
                OBJ_3DSOLID => {
                    let data = entities::read_acis_entity(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                        entity_common.has_ds_data,
                    );
                    let mut e = Solid3D::new();
                    e.common = entity_common;
                    e.acis_data.version = if data.is_binary {
                        crate::entities::solid3d::AcisVersion::Version2
                    } else {
                        crate::entities::solid3d::AcisVersion::Version1
                    };
                    e.acis_data.sat_data = data.sat_data;
                    e.acis_data.sab_data = data.sab_data;
                    e.acis_data.is_binary = data.is_binary;
                    e.acis_data.revision = data.revision;
                    e.acis_data.materials = data.materials;
                    e.acis_data.wireframe_data_present = data.wireframe_data_present;
                    e.acis_data.wireframe_point_present = data.wireframe_point_present;
                    e.acis_data.wireframe_isoline_present = data.wireframe_isoline_present;
                    e.acis_data.acis_empty_bit = data.acis_empty_bit;
                    e.acis_data.extra_acis_data = data.extra_acis_data.map(Box::new);
                    e.acis_data.wireframe_isolines = data.isolines;
                    // The wireframe anchor AutoCAD bakes in (point_present +
                    // 3BD) is the body's bounding-box centre — the natural
                    // reference point. Empty/degenerate bodies (no anchor) fall
                    // back to the geometry centre, then the SAT placement.
                    e.point_of_reference = if data.point != crate::types::Vector3::ZERO {
                        data.point
                    } else {
                        e.acis_data
                            .geometry_centre()
                            .or_else(|| e.acis_data.placement_origin())
                            .unwrap_or(data.point)
                    };
                    e.wires = data.wires;
                    e.silhouettes = data.silhouettes;

                    // 3DSOLID R2007+: history_id handle
                    // (always present since R2007, regardless of ACIS version)
                    if self.obj_reader.version().r2007_plus() {
                        let h = reader.read_handle();
                        if h != 0 {
                            e.history_handle = Some(Handle::new(h));
                        }
                    }
                    let _ = document.add_entity(EntityType::Solid3D(e));
                }
                OBJ_REGION => {
                    let data = entities::read_acis_entity(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                        entity_common.has_ds_data,
                    );
                    let mut e = Region::new();
                    e.common = entity_common;
                    e.acis_data.version = if data.is_binary {
                        crate::entities::solid3d::AcisVersion::Version2
                    } else {
                        crate::entities::solid3d::AcisVersion::Version1
                    };
                    e.acis_data.sat_data = data.sat_data;
                    e.acis_data.sab_data = data.sab_data;
                    e.acis_data.is_binary = data.is_binary;
                    e.acis_data.revision = data.revision;
                    e.acis_data.materials = data.materials;
                    e.acis_data.wireframe_data_present = data.wireframe_data_present;
                    e.acis_data.wireframe_point_present = data.wireframe_point_present;
                    e.acis_data.wireframe_isoline_present = data.wireframe_isoline_present;
                    e.acis_data.acis_empty_bit = data.acis_empty_bit;
                    e.acis_data.extra_acis_data = data.extra_acis_data.map(Box::new);
                    e.acis_data.wireframe_isolines = data.isolines;
                    // The wireframe anchor AutoCAD bakes in (point_present +
                    // 3BD) is the body's bounding-box centre — the natural
                    // reference point. Empty/degenerate bodies (no anchor) fall
                    // back to the geometry centre, then the SAT placement.
                    e.point_of_reference = if data.point != crate::types::Vector3::ZERO {
                        data.point
                    } else {
                        e.acis_data
                            .geometry_centre()
                            .or_else(|| e.acis_data.placement_origin())
                            .unwrap_or(data.point)
                    };
                    e.wires = data.wires;
                    e.silhouettes = data.silhouettes;
                    let _ = document.add_entity(EntityType::Region(e));
                }
                OBJ_BODY => {
                    let data = entities::read_acis_entity(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                        entity_common.has_ds_data,
                    );
                    let mut e = Body::new();
                    e.common = entity_common;
                    e.acis_data.version = if data.is_binary {
                        crate::entities::solid3d::AcisVersion::Version2
                    } else {
                        crate::entities::solid3d::AcisVersion::Version1
                    };
                    e.acis_data.sat_data = data.sat_data;
                    e.acis_data.sab_data = data.sab_data;
                    e.acis_data.is_binary = data.is_binary;
                    e.acis_data.revision = data.revision;
                    e.acis_data.materials = data.materials;
                    e.acis_data.wireframe_data_present = data.wireframe_data_present;
                    e.acis_data.wireframe_point_present = data.wireframe_point_present;
                    e.acis_data.wireframe_isoline_present = data.wireframe_isoline_present;
                    e.acis_data.acis_empty_bit = data.acis_empty_bit;
                    e.acis_data.extra_acis_data = data.extra_acis_data.map(Box::new);
                    e.acis_data.wireframe_isolines = data.isolines;
                    // The wireframe anchor AutoCAD bakes in (point_present +
                    // 3BD) is the body's bounding-box centre — the natural
                    // reference point. Empty/degenerate bodies (no anchor) fall
                    // back to the geometry centre, then the SAT placement.
                    e.point_of_reference = if data.point != crate::types::Vector3::ZERO {
                        data.point
                    } else {
                        e.acis_data
                            .geometry_centre()
                            .or_else(|| e.acis_data.placement_origin())
                            .unwrap_or(data.point)
                    };
                    e.wires = data.wires;
                    e.silhouettes = data.silhouettes;
                    if self.obj_reader.version().r2007_plus() && !e.common.has_ds_data {
                        let h = reader.read_handle();
                        if h != 0 {
                            e.history_handle = Some(Handle::new(h));
                        }
                    }
                    let _ = document.add_entity(EntityType::Body(e));
                }

                // ── ACAD_SURFACE family (ACIS-backed) ───────────────
                OBJ_SURFACE | OBJ_PLANESURFACE | OBJ_EXTRUDEDSURFACE | OBJ_LOFTEDSURFACE
                | OBJ_REVOLVEDSURFACE | OBJ_SWEPTSURFACE | OBJ_NURBSURFACE => {
                    let kind = match type_code {
                        OBJ_PLANESURFACE => crate::entities::SurfaceKind::Plane,
                        OBJ_EXTRUDEDSURFACE => crate::entities::SurfaceKind::Extruded,
                        OBJ_LOFTEDSURFACE => crate::entities::SurfaceKind::Lofted,
                        OBJ_REVOLVEDSURFACE => crate::entities::SurfaceKind::Revolved,
                        OBJ_SWEPTSURFACE => crate::entities::SurfaceKind::Swept,
                        OBJ_NURBSURFACE => crate::entities::SurfaceKind::Nurb,
                        _ => crate::entities::SurfaceKind::Generic,
                    };
                    let data = entities::read_surface(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                        entity_common.has_ds_data,
                        kind,
                    );
                    let mut e = Surface::new(kind);
                    e.common = entity_common;
                    e.acis_data.version = if data.acis.is_binary {
                        crate::entities::solid3d::AcisVersion::Version2
                    } else {
                        crate::entities::solid3d::AcisVersion::Version1
                    };
                    e.acis_data.sat_data = data.acis.sat_data;
                    e.acis_data.sab_data = data.acis.sab_data;
                    e.acis_data.is_binary = data.acis.is_binary;
                    e.acis_data.revision = data.acis.revision;
                    e.acis_data.materials = data.acis.materials;
                    e.acis_data.wireframe_data_present = data.acis.wireframe_data_present;
                    e.acis_data.wireframe_point_present = data.acis.wireframe_point_present;
                    e.acis_data.wireframe_isoline_present = data.acis.wireframe_isoline_present;
                    e.acis_data.acis_empty_bit = data.acis.acis_empty_bit;
                    e.acis_data.extra_acis_data = data.acis.extra_acis_data.map(Box::new);
                    e.acis_data.wireframe_isolines = data.acis.isolines;
                    e.wires = data.acis.wires;
                    e.silhouettes = data.acis.silhouettes;
                    e.point_of_reference = data.acis.point;
                    e.modeler_format_version = data.modeler_format_version;
                    e.u_isolines = data.u_isolines;
                    e.v_isolines = data.v_isolines;
                    e.surface_data = data.surface_data;
                    e.history_handle =
                        (data.history_handle != 0).then(|| Handle::from(data.history_handle));
                    let _ = document.add_entity(EntityType::Surface(e));
                }

                // ── Catch-all ──────────────────────────────────────
                _ => {
                    // Class numbers ≥500 are per-file; resolve the class name so
                    // the model-documentation decodes below are portable.
                    let cpp_class = class_names
                        .cpp
                        .get(&type_code)
                        .map(String::as_str)
                        .unwrap_or("");
                    match cpp_class {
                        "CAcLayoutPrintConfig" => {
                            let mut data = entities::read_layout_print_config(
                                &mut reader,
                            );
                            if let ExtendedEntityData::LayoutPrintConfig(value) = &mut data {
                                value.raw_dwg_data = Some(reader.raw_merged_data());
                                value.raw_dwg_handle_bits = reader.get_handle_bits();
                                value.raw_dwg_version = Some(document.version);
                            }
                            let _ = document.add_entity(EntityType::Extended(
                                ExtendedEntity {
                                    common: entity_common,
                                    data,
                                },
                            ));
                        }
                        "mcsDbObjectFormat" => {
                            let _ = document.add_entity(EntityType::Extended(
                                ExtendedEntity {
                                    common: entity_common,
                                    data: ExtendedEntityData::Format(
                                        crate::entities::FormatData {
                                            raw_dwg_data: Some(
                                                reader.raw_merged_data(),
                                            ),
                                            raw_dwg_handle_bits:
                                                reader.get_handle_bits(),
                                            raw_dwg_version: Some(
                                                document.version,
                                            ),
                                            raw_dxf_codes: None,
                                        },
                                    ),
                                },
                            ));
                        }
                        "AcDbBlockAngularConstraintParameterEntity" => {
                            if let Some(data) =
                                crate::io::dwg::dwg_stream_readers::object_reader::dynamic_block::read_dynamic_block_data(
                                    &mut reader,
                                    "BLOCKANGULARCONSTRAINTPARAMETERENTITY",
                                )
                            {
                                let _ = document.add_entity(EntityType::Extended(
                                    ExtendedEntity {
                                        common: entity_common,
                                        data: ExtendedEntityData::DynamicBlock(data),
                                    },
                                ));
                            }
                        }
                        name @ (
                            "AcDbProxyEntityWrapper"
                            | "PtDbWall"
                            | "mcsDbObject"
                            | "mcsDbObjectNotePosition"
                            | "mcsDbObjectLevelMark"
                            | "mcsDbObjectRelationMark"
                        ) => {
                            let dxf_name = class_names
                                .dxf
                                .get(&type_code)
                                .cloned()
                                .unwrap_or_else(|| name.to_string());
                            let (payload, object_ids) =
                                read_registered_payload(
                                    &mut reader,
                                    entity_common.handle.value(),
                                );
                            let _ = document.add_entity(EntityType::Extended(
                                ExtendedEntity {
                                    common: entity_common,
                                    data: ExtendedEntityData::RegisteredClass(
                                        crate::entities::RegisteredClassEntityData {
                                            dxf_name,
                                            cpp_class_name: name.to_string(),
                                            properties: Vec::new(),
                                            payload,
                                            object_ids,
                                        },
                                    ),
                                },
                            ));
                        }
                        name @ (
                            "AcDbBlockAlignmentParameterEntity"
                            | "AcDbBlockBasepointParameterEntity"
                            | "AcDbBlockFlipParameterEntity"
                            | "AcDbBlockLinearParameterEntity"
                            | "AcDbBlockPointParameterEntity"
                            | "AcDbBlockRotationParameterEntity"
                            | "AcDbBlockVisibilityParameterEntity"
                            | "AcDbBlockFlipGripEntity"
                            | "AcDbBlockLinearGripEntity"
                            | "AcDbBlockPolarGripEntity"
                            | "AcDbBlockRotationGripEntity"
                            | "AcDbBlockVisibilityGripEntity"
                            | "AcDbBlockXYGripEntity"
                            | "AcDbBlockXYParameterEntity"
                        ) => {
                            let dxf_name = class_names
                                .dxf
                                .get(&type_code)
                                .cloned()
                                .unwrap_or_else(|| name.to_string());
                            if let Some(data) =
                                crate::objects::DynamicBlockData::empty_entity_from_dxf_name(
                                    &dxf_name,
                                )
                            {
                                let _ = document.add_entity(EntityType::Extended(
                                    ExtendedEntity {
                                        common: entity_common,
                                        data: ExtendedEntityData::DynamicBlock(data),
                                    },
                                ));
                            }
                        }
                        // AcDbSectionSymbol ("SECTIONLINE"): decode the section
                        // "A-A" mark for display. The reader is positioned at
                        // the class-specific data (common entity data already
                        // consumed), so the geometry reads cleanly from here.
                        "AcDbSectionSymbol" => {
                            let mut e = decode_section_symbol(&mut reader)
                                .unwrap_or_else(SectionSymbol::new);
                            e.common = entity_common;
                            let _ = document.add_entity(EntityType::SectionSymbol(e));
                        }
                        // AcDbViewBorder ("DRAWINGVIEW"): the view's paper
                        // rectangle / scale, and — as the first object-specific
                        // handle — the view's *active* viewport (the one
                        // carrying the real camera), the last hop of the
                        // section-mark viewing-direction chain.
                        "AcDbViewBorder" => {
                            let mut e = ViewBorder::new();
                            e.common = entity_common;
                            e.active_viewport = Handle::from(reader.read_handle());
                            e.scale_handle = Handle::from(reader.read_handle());
                            e.version = reader.read_bit_short();
                            e.min = [
                                reader.read_raw_double(),
                                reader.read_raw_double(),
                            ];
                            e.max = [
                                reader.read_raw_double(),
                                reader.read_raw_double(),
                            ];
                            e.scale = reader.read_raw_double();
                            e.rotation_angle = reader.read_raw_double();
                            e.center = [
                                reader.read_raw_double(),
                                reader.read_raw_double(),
                            ];
                            let _ = document.add_entity(EntityType::ViewBorder(e));
                        }
                        _ => {
                            // Keep the class's real DXF name (e.g. an AEC
                            // object's "AEC_WALL") so the entity reports its
                            // actual type instead of a numeric placeholder;
                            // fall back to DWG_TYPE_<code> for an unresolved
                            // class.
                            let name = class_names
                                .dxf
                                .get(&type_code)
                                .cloned()
                                .filter(|n| !n.is_empty())
                                .unwrap_or_else(|| format!("DWG_TYPE_{}", type_code));
                            let mut e = UnknownEntity::new(name);
                            e.common = entity_common;
                            e.dwg_type_code = type_code;
                            e.dwg_handle_bits = reader.get_handle_bits();
                            e.raw_dwg_data = Some(reader.raw_merged_data());
                            e.dwg_source_version = Some(document.version);
                            let _ = document.add_entity(EntityType::Unknown(e));
                        }
                    }
                }
            }
        } else if !is_table_type(type_code) {
            // ── Non-graphical objects ──────────────────────────────
            let non_entity_data = self
                .obj_reader
                .read_common_non_entity_data(&mut reader, type_code);
            let owner_handle = Handle::from(non_entity_data.owner_handle);
            if non_entity_data.has_ds_data {
                document
                    .dwg_data_store_handles
                    .insert(Handle::from(non_entity_data.common.handle));
            }
            // Save raw EED blobs for DWG round-trip write-back
            if !non_entity_data.common.eed_raw.is_empty() {
                document.eed_by_handle.insert(
                    Handle::from(non_entity_data.common.handle),
                    non_entity_data.common.eed_raw.clone(),
                );
            }
            // Save xdictionary handle for DWG round-trip write-back
            if let Some(xdic) = non_entity_data.xdictionary_handle {
                document.xdic_by_handle.insert(
                    Handle::from(non_entity_data.common.handle),
                    Handle::from(xdic),
                );
            }
            // Save reactors for DWG round-trip write-back
            if !non_entity_data.reactors.is_empty() {
                document.reactors_by_handle.insert(
                    Handle::from(non_entity_data.common.handle),
                    non_entity_data
                        .reactors
                        .iter()
                        .map(|&h| Handle::from(h))
                        .collect(),
                );
            }

            match type_code {
                OBJ_DUMMY | OBJ_OBJECT_PTR => {
                    let data = if type_code == OBJ_DUMMY {
                        crate::objects::DataObjectData::Dummy
                    } else {
                        crate::objects::DataObjectData::ObjectPointer
                    };
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::DataObject(
                            crate::objects::DataObject {
                                handle: Handle::from(handle),
                                owner: owner_handle,
                                reactors: non_entity_data
                                    .reactors
                                    .iter()
                                    .copied()
                                    .map(Handle::from)
                                    .collect(),
                                xdictionary_handle: non_entity_data
                                    .xdictionary_handle
                                    .map(Handle::from),
                                data,
                            },
                        ),
                    );
                }
                OBJ_LONG_TRANSACTION => {
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::DataObject(
                            crate::objects::DataObject {
                                handle: Handle::from(handle),
                                owner: owner_handle,
                                reactors: non_entity_data
                                    .reactors
                                    .iter()
                                    .copied()
                                    .map(Handle::from)
                                    .collect(),
                                xdictionary_handle: non_entity_data
                                    .xdictionary_handle
                                    .map(Handle::from),
                                data: crate::objects::DataObjectData::LongTransaction,
                            },
                        ),
                    );
                }
                OBJ_DICTIONARY => {
                    let data = objects::read_dictionary(&mut reader, self.obj_reader.version());
                    let mut obj = crate::objects::Dictionary::new();
                    obj.handle = Handle::from(handle);
                    obj.owner = owner_handle;
                    obj.hard_owner = data.hard_owner;
                    obj.duplicate_cloning = data.duplicate_cloning;
                    for entry in data.entries {
                        obj.add_entry(entry.name, Handle::from(entry.handle));
                    }
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::Dictionary(obj),
                    );
                }
                OBJ_DICTIONARYWDFLT => {
                    let data = objects::read_dictionary_with_default(&mut reader);
                    let mut obj = crate::objects::DictionaryWithDefault::new();
                    obj.handle = Handle::from(handle);
                    obj.owner = owner_handle;
                    obj.hard_owner = data.hard_owner;
                    obj.duplicate_cloning = data.duplicate_cloning;
                    obj.default_handle = Handle::from(data.default_handle);
                    for entry in data.entries {
                        obj.entries.push((entry.name, Handle::from(entry.handle)));
                    }
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::DictionaryWithDefault(obj),
                    );
                }
                OBJ_DICTIONARYVAR => {
                    let data = objects::read_dictionary_variable(&mut reader);
                    let mut obj = crate::objects::DictionaryVariable::new("", &data.value);
                    obj.handle = Handle::from(handle);
                    obj.owner_handle = owner_handle;
                    obj.schema_number = data.schema_number as i16;
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::DictionaryVariable(obj),
                    );
                }
                OBJ_LAYOUT => {
                    let data = objects::read_layout(&mut reader, self.obj_reader.version());
                    let mut obj = crate::objects::Layout::new(&data.name);
                    obj.handle = Handle::from(handle);
                    obj.owner = owner_handle;
                    obj.reactors = non_entity_data
                        .reactors
                        .iter()
                        .copied()
                        .map(Handle::from)
                        .collect();
                    obj.xdictionary_handle = non_entity_data
                        .xdictionary_handle
                        .map(Handle::from);
                    obj.flags = data.flags;
                    obj.tab_order = data.tab_order;
                    obj.min_limits = data.min_limits;
                    obj.max_limits = data.max_limits;
                    obj.insertion_base = (
                        data.insertion_base.x,
                        data.insertion_base.y,
                        data.insertion_base.z,
                    );
                    obj.min_extents = (data.min_extents.x, data.min_extents.y, data.min_extents.z);
                    obj.max_extents = (data.max_extents.x, data.max_extents.y, data.max_extents.z);
                    obj.elevation = data.elevation;
                    obj.ucs_origin = (data.ucs_origin.x, data.ucs_origin.y, data.ucs_origin.z);
                    obj.ucs_x_axis = (data.x_axis.x, data.x_axis.y, data.x_axis.z);
                    obj.ucs_y_axis = (data.y_axis.x, data.y_axis.y, data.y_axis.z);
                    obj.ucs_ortho_type = data.ucs_ortho_type;
                    obj.block_record = Handle::from(data.block_record_handle);
                    obj.viewport = Handle::from(data.viewport_handle);
                    obj.base_ucs = Handle::from(data.base_ucs_handle);
                    obj.named_ucs = Handle::from(data.named_ucs_handle);
                    obj.viewports = data
                        .viewport_handles
                        .iter()
                        .copied()
                        .map(Handle::from)
                        .collect();
                    obj.paper_width = data.plot_settings.paper_width;
                    obj.paper_height = data.plot_settings.paper_height;
                    obj.plot_rotation = data.plot_settings.rotation;
                    let ps = &data.plot_settings;
                    obj.plot_flags =
                        crate::objects::PlotFlags::from_bits(
                            ps.plot_flags as i32,
                        );
                    obj.plot_page_name = ps.page_name.clone();
                    obj.plot_printer_name = ps.printer_name.clone();
                    obj.paper_size = ps.paper_size.clone();
                    obj.plot_view_name = ps.plot_view_name.clone();
                    obj.plot_style_sheet = ps.current_style_sheet.clone();
                    obj.plot_margin_left = ps.left_margin;
                    obj.plot_margin_bottom = ps.bottom_margin;
                    obj.plot_margin_right = ps.right_margin;
                    obj.plot_margin_top = ps.top_margin;
                    obj.plot_origin_x = ps.origin_x;
                    obj.plot_origin_y = ps.origin_y;
                    obj.plot_window_min_x = ps.window_min_x;
                    obj.plot_window_min_y = ps.window_min_y;
                    obj.plot_window_max_x = ps.window_max_x;
                    obj.plot_window_max_y = ps.window_max_y;
                    obj.plot_paper_units = ps.paper_units;
                    obj.plot_type = ps.plot_type;
                    obj.plot_scale_numerator = ps.scale_numerator;
                    obj.plot_scale_denominator = ps.scale_denominator;
                    obj.plot_scale_type = ps.scale_type;
                    obj.plot_scale_factor = ps.scale_factor;
                    obj.paper_image_origin_x = ps.paper_image_x;
                    obj.paper_image_origin_y = ps.paper_image_y;
                    obj.shade_plot_mode = ps.shade_plot_mode;
                    obj.shade_plot_resolution = ps.shade_plot_resolution;
                    obj.shade_plot_dpi = ps.shade_plot_dpi;
                    obj.plot_view_handle =
                        Handle::from(ps.plot_view_handle);
                    if obj.plot_view_name.is_empty()
                        && !obj.plot_view_handle.is_null()
                    {
                        if let Some(name) =
                            maps.views.get(&obj.plot_view_handle.value())
                        {
                            obj.plot_view_name = name.clone();
                        }
                    }
                    obj.visual_style_handle =
                        Handle::from(ps.visual_style_handle);
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::Layout(obj),
                    );
                }
                OBJ_GROUP => {
                    let data = objects::read_group(&mut reader);
                    let mut obj = crate::objects::Group::new("");
                    obj.handle = Handle::from(handle);
                    obj.owner = owner_handle;
                    obj.description = data.description;
                    obj.unnamed = data.unnamed != 0;
                    obj.selectable = data.selectable;
                    for eh in data.entity_handles {
                        obj.entities.push(Handle::from(eh));
                    }
                    document
                        .objects
                        .insert(Handle::from(handle), crate::objects::ObjectType::Group(obj));
                }
                OBJ_MLINESTYLE => {
                    let data = objects::read_mlinestyle(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                    );
                    let mut obj = crate::objects::MLineStyle::new(&data.name);
                    obj.handle = Handle::from(handle);
                    obj.owner = owner_handle;
                    obj.description = data.description;
                    obj.fill_color = data.fill_color;
                    obj.start_angle = data.start_angle;
                    obj.end_angle = data.end_angle;
                    // DWG binary swaps some flag pairs vs DXF:
                    //   DWG bit 1=DisplayJoints, 2=FillOn (DXF: 1=FillOn, 2=DisplayJoints)
                    //   DWG bit 0x20=StartRound, 0x40=StartInner (DXF: 0x20=StartInner, 0x40=StartRound)
                    //   DWG bit 0x200=EndRound, 0x400=EndInner (DXF: 0x200=EndInner, 0x400=EndRound)
                    let f = data.flags as i32;
                    obj.flags = crate::objects::MLineStyleFlags {
                        display_joints: (f & 1) != 0,
                        fill_on: (f & 2) != 0,
                        start_square_cap: (f & 16) != 0,
                        start_round_cap: (f & 0x20) != 0,
                        start_inner_arcs_cap: (f & 0x40) != 0,
                        end_square_cap: (f & 0x100) != 0,
                        end_round_cap: (f & 0x200) != 0,
                        end_inner_arcs_cap: (f & 0x400) != 0,
                    };
                    // Transfer elements
                    obj.elements = data
                        .elements
                        .iter()
                        .map(|e| {
                            let linetype = if self
                                .obj_reader
                                .version()
                                .r2018_plus(self.obj_reader.dxf_version())
                            {
                                maps.linetypes
                                    .get(&e.linetype_index_or_handle)
                                    .cloned()
                                    .unwrap_or_else(|| "BYLAYER".to_string())
                            } else {
                                // Pre-R2018: a bit_short index into the linetypes
                                // in table order (ByBlock/ByLayer excluded);
                                // 0x7FFF means ByLayer.
                                let idx = e.linetype_index_or_handle;
                                if idx == 0x7FFF {
                                    "ByLayer".to_string()
                                } else {
                                    maps.linetype_order
                                        .get(idx as usize)
                                        .cloned()
                                        .unwrap_or_else(|| "BYLAYER".to_string())
                                }
                            };
                            crate::objects::MLineStyleElement::full(e.offset, e.color, linetype)
                        })
                        .collect();
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::MLineStyle(obj),
                    );
                }
                OBJ_XRECORD => {
                    let data = objects::read_xrecord(&mut reader);
                    let mut obj = crate::objects::XRecord::new();
                    obj.handle = Handle::from(handle);
                    obj.owner = owner_handle;
                    obj.reactors = non_entity_data
                        .reactors
                        .iter()
                        .copied()
                        .map(Handle::from)
                        .collect();
                    obj.xdictionary_handle =
                        non_entity_data.xdictionary_handle.map(Handle::from);
                    obj.cloning_flags =
                        crate::objects::DictionaryCloningFlags::from_value(data.cloning_flags);
                    obj.entries = data.entries;
                    obj.object_references = data.object_references;
                    obj.preserve_object_reference_stream = true;
                    obj.entries_complete = data.entries_complete;
                    obj.raw_data = data.raw_data;
                    obj.raw_dwg_data = Some(reader.raw_merged_data());
                    obj.raw_dwg_handle_bits = reader.get_handle_bits();
                    obj.raw_dwg_version = Some(document.version);
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::XRecord(obj),
                    );
                }
                OBJ_PLOTSETTINGS => {
                    let data =
                        objects::read_plot_settings_obj(&mut reader, self.obj_reader.version());
                    let mut obj = crate::objects::PlotSettings::new(&data.page_name);
                    obj.handle = Handle::from(handle);
                    obj.owner = owner_handle;
                    obj.reactors = non_entity_data
                        .reactors
                        .iter()
                        .copied()
                        .map(Handle::from)
                        .collect();
                    obj.xdictionary_handle = non_entity_data
                        .xdictionary_handle
                        .map(Handle::from);
                    obj.printer_name = data.printer_name;
                    obj.paper_size = data.paper_size;
                    obj.plot_view_name = data.plot_view_name;
                    obj.current_style_sheet = data.current_style_sheet;
                    obj.paper_width = data.paper_width;
                    obj.paper_height = data.paper_height;
                    obj.margins = crate::objects::PaperMargin::new(
                        data.left_margin,
                        data.bottom_margin,
                        data.right_margin,
                        data.top_margin,
                    );
                    obj.origin_x = data.origin_x;
                    obj.origin_y = data.origin_y;
                    obj.plot_window = crate::objects::PlotWindow::new(
                        data.window_min_x,
                        data.window_min_y,
                        data.window_max_x,
                        data.window_max_y,
                    );
                    obj.scale_numerator = data.scale_numerator;
                    obj.scale_denominator = data.scale_denominator;
                    obj.paper_units = crate::objects::PlotPaperUnits::from_code(data.paper_units);
                    obj.rotation = crate::objects::PlotRotation::from_code(data.rotation);
                    obj.plot_type = crate::objects::PlotType::from_code(data.plot_type);
                    obj.scale_type = crate::objects::ScaledType::from_code(data.scale_type);
                    obj.shade_plot_mode =
                        crate::objects::ShadePlotMode::from_code(data.shade_plot_mode);
                    obj.shade_plot_resolution = crate::objects::ShadePlotResolutionLevel::from_code(
                        data.shade_plot_resolution,
                    );
                    obj.shade_plot_dpi = data.shade_plot_dpi;
                    obj.flags = crate::objects::PlotFlags::from_bits(data.plot_flags as i32);
                    obj.standard_scale_factor = data.scale_factor;
                    obj.paper_image_origin_x = data.paper_image_x;
                    obj.paper_image_origin_y = data.paper_image_y;
                    obj.plot_view_handle =
                        Handle::from(data.plot_view_handle);
                    if obj.plot_view_name.is_empty()
                        && !obj.plot_view_handle.is_null()
                    {
                        if let Some(name) =
                            maps.views.get(&obj.plot_view_handle.value())
                        {
                            obj.plot_view_name = name.clone();
                        }
                    }
                    obj.visual_style_handle =
                        Handle::from(data.visual_style_handle);
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::PlotSettings(obj),
                    );
                }
                OBJ_MLEADERSTYLE => {
                    let data = objects::read_multileader_style(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                    );
                    let mut obj = crate::objects::MultiLeaderStyle::new("");
                    obj.handle = Handle::from(handle);
                    obj.owner_handle = owner_handle;
                    obj.description = data.description;
                    obj.content_type = crate::objects::LeaderContentType::from(data.content_type);
                    obj.multileader_draw_order =
                        crate::objects::MultiLeaderDrawOrderType::from(data.multileader_draw_order);
                    obj.leader_draw_order =
                        crate::objects::LeaderDrawOrderType::from(data.leader_draw_order);
                    obj.max_leader_points = data.max_leader_points;
                    obj.first_segment_angle = data.first_segment_angle;
                    obj.second_segment_angle = data.second_segment_angle;
                    obj.path_type = crate::objects::MultiLeaderPathType::from(data.path_type);
                    obj.line_color = data.line_color;
                    obj.line_type_handle = if data.line_type_handle != 0 {
                        Some(Handle::from(data.line_type_handle))
                    } else {
                        None
                    };
                    obj.line_weight = LineWeight::from_value(data.line_weight as i16);
                    obj.enable_landing = data.enable_landing;
                    obj.landing_gap = data.landing_gap;
                    obj.enable_dogleg = data.enable_dogleg;
                    obj.landing_distance = data.landing_distance;
                    obj.arrowhead_handle = if data.arrowhead_handle != 0 {
                        Some(Handle::from(data.arrowhead_handle))
                    } else {
                        None
                    };
                    obj.arrowhead_size = data.arrowhead_size;
                    obj.default_text = data.default_text;
                    obj.text_style_handle = if data.text_style_handle != 0 {
                        Some(Handle::from(data.text_style_handle))
                    } else {
                        None
                    };
                    obj.text_left_attachment =
                        crate::objects::TextAttachmentType::from(data.text_left_attachment);
                    obj.text_right_attachment =
                        crate::objects::TextAttachmentType::from(data.text_right_attachment);
                    obj.text_angle_type = crate::objects::TextAngleType::from(data.text_angle_type);
                    obj.text_alignment =
                        crate::objects::TextAlignmentType::from(data.text_alignment);
                    obj.text_color = data.text_color;
                    obj.text_height = data.text_height;
                    obj.text_frame = data.text_frame;
                    obj.text_always_left = data.text_always_left;
                    obj.align_space = data.align_space;
                    obj.block_content_handle = if data.block_content_handle != 0 {
                        Some(Handle::from(data.block_content_handle))
                    } else {
                        None
                    };
                    obj.block_content_color = data.block_content_color;
                    obj.block_content_scale_x = data.block_content_scale_x;
                    obj.block_content_scale_y = data.block_content_scale_y;
                    obj.block_content_scale_z = data.block_content_scale_z;
                    obj.enable_block_scale = data.enable_block_scale;
                    obj.block_content_rotation = data.block_content_rotation;
                    obj.enable_block_rotation = data.enable_block_rotation;
                    obj.block_content_connection = crate::objects::BlockContentConnectionType::from(
                        data.block_content_connection,
                    );
                    obj.scale_factor = data.scale_factor;
                    obj.property_changed = data.property_changed;
                    obj.is_annotative = data.is_annotative;
                    obj.break_gap_size = data.break_gap_size;
                    obj.text_attachment_direction =
                        crate::objects::TextAttachmentDirectionType::from(
                            data.text_attachment_direction,
                        );
                    obj.text_top_attachment =
                        crate::objects::TextAttachmentType::from(data.text_top_attachment);
                    obj.text_bottom_attachment =
                        crate::objects::TextAttachmentType::from(data.text_bottom_attachment);
                    obj.unknown_flag_298 = data.unknown_flag_298;
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::MultiLeaderStyle(obj),
                    );
                }
                OBJ_IMAGEDEF => {
                    let data = objects::read_image_definition(&mut reader);
                    let mut obj = crate::objects::ImageDefinition::new(&data.file_name);
                    obj.handle = Handle::from(handle);
                    obj.owner = owner_handle;
                    obj.class_version = data.class_version;
                    obj.is_loaded = data.is_loaded;
                    obj.size_in_pixels =
                        (data.size_in_pixels.x as u32, data.size_in_pixels.y as u32);
                    obj.pixel_size = (data.pixel_size.x, data.pixel_size.y);
                    obj.resolution_unit =
                        crate::objects::ResolutionUnit::from_code(data.resolution_unit as i32);
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::ImageDefinition(obj),
                    );
                }
                code @ (OBJ_PDFDEFINITION | OBJ_DWFDEFINITION | OBJ_DGNDEFINITION) => {
                    use crate::entities::underlay::UnderlayType;
                    let utype = if code == OBJ_DWFDEFINITION {
                        UnderlayType::Dwf
                    } else if code == OBJ_DGNDEFINITION {
                        UnderlayType::Dgn
                    } else {
                        UnderlayType::Pdf
                    };
                    let data = objects::read_underlay_definition(&mut reader);
                    let mut obj = crate::objects::UnderlayDefinition::new(utype);
                    obj.handle = Handle::from(handle);
                    obj.owner_handle = owner_handle;
                    obj.file_path = data.file_path;
                    obj.page_name = data.page_name;
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::UnderlayDefinition(obj),
                    );
                }
                OBJ_IMAGEDEFREACTOR => {
                    let _data = objects::read_image_definition_reactor(&mut reader);
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::ImageDefinitionReactor(
                            crate::objects::ImageDefinitionReactor {
                                handle: Handle::from(handle),
                                owner: owner_handle,
                                image_handle: Handle::NULL,
                            },
                        ),
                    );
                }
                OBJ_SCALE => {
                    let data = objects::read_scale(&mut reader);
                    let mut obj = crate::objects::Scale::new(
                        &data.name,
                        data.paper_units,
                        data.drawing_units,
                    );
                    obj.handle = Handle::from(handle);
                    obj.owner_handle = owner_handle;
                    obj.is_unit_scale = data.is_unit_scale;
                    document
                        .objects
                        .insert(Handle::from(handle), crate::objects::ObjectType::Scale(obj));
                }
                OBJ_SORTENTSTABLE => {
                    let data = objects::read_sort_entities_table(&mut reader);
                    let mut obj = crate::objects::SortEntitiesTable::new();
                    obj.handle = Handle::from(handle);
                    obj.owner_handle = owner_handle;
                    obj.block_owner_handle = Handle::from(data.block_owner_handle);
                    for entry in data.entries {
                        obj.add_entry(
                            Handle::from(entry.entity_handle),
                            Handle::from(entry.sort_handle),
                        );
                    }
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::SortEntitiesTable(obj),
                    );
                }
                OBJ_RASTERVARIABLES => {
                    let data = objects::read_raster_variables(&mut reader);
                    let obj = crate::objects::RasterVariables {
                        handle: Handle::from(handle),
                        owner: owner_handle,
                        class_version: data.class_version,
                        display_image_frame: data.display_image_frame,
                        image_quality: data.image_quality,
                        units: data.units,
                    };
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::RasterVariables(obj),
                    );
                }
                OBJ_DBCOLOR => {
                    let data = objects::read_book_color(&mut reader);
                    let obj = crate::objects::BookColor {
                        handle: Handle::from(handle),
                        owner: owner_handle,
                        color: data.color,
                        color_name: data.color_name,
                        book_name: data.book_name,
                    };
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::BookColor(obj),
                    );
                }
                OBJ_PLACEHOLDER => {
                    objects::read_placeholder(&mut reader);
                    let obj = crate::objects::PlaceHolder {
                        handle: Handle::from(handle),
                        owner: owner_handle,
                    };
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::PlaceHolder(obj),
                    );
                }
                OBJ_WIPEOUTVARIABLES => {
                    let data = objects::read_wipeout_variables(&mut reader);
                    let obj = crate::objects::WipeoutVariables {
                        handle: Handle::from(handle),
                        owner: owner_handle,
                        display_frame: data.display_frame,
                    };
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::WipeoutVariables(obj),
                    );
                }
                OBJ_VBA_PROJECT => {
                    if let Some(data) =
                        crate::io::dwg::dwg_stream_readers::object_reader::class_object::read_class_object_data(
                            &mut reader,
                            "VBA_PROJECT",
                            self.obj_reader.version(),
                            self.obj_reader.dxf_version(),
                        )
                    {
                        document.objects.insert(
                            Handle::from(handle),
                            crate::objects::ObjectType::ClassObject(
                                crate::objects::ClassObject {
                                    handle: Handle::from(handle),
                                    owner: owner_handle,
                                    reactors: non_entity_data
                                        .reactors
                                        .iter()
                                        .copied()
                                        .map(Handle::from)
                                        .collect(),
                                    xdictionary_handle: non_entity_data
                                        .xdictionary_handle
                                        .map(Handle::from),
                                    data,
                                },
                            ),
                        );
                    }
                }
                OBJ_PROXY_OBJECT => {
                    let class_id = reader.read_bit_long();
                    let dxf_subclass = if self.obj_reader.dxf_version()
                        > crate::types::DxfVersion::AC1015
                    {
                        reader.read_variable_text()
                    } else {
                        String::new()
                    };
                    let (version, dwg_version, maintenance_version) = if self
                        .obj_reader
                        .version()
                        .r2018_plus(self.obj_reader.dxf_version())
                    {
                        let dwg_version = reader.read_bit_long();
                        let maintenance_version = reader.read_bit_long();
                        (
                            (maintenance_version << 16)
                                | (dwg_version & 0xffff),
                            dwg_version,
                            maintenance_version,
                        )
                    } else {
                        let version = reader.read_bit_long();
                        (version, version & 0xffff, version >> 16)
                    };
                    let from_dxf = if self.obj_reader.version().r2000_plus() {
                        reader.read_bit()
                    } else {
                        false
                    };
                    let object_data_bits = reader.main_remaining_bits() as u32;
                    let mut object_data =
                        vec![0u8; object_data_bits.div_ceil(8) as usize];
                    for bit_index in 0..object_data_bits as usize {
                        if reader.read_bit() {
                            object_data[bit_index / 8] |=
                                0x80 >> (bit_index % 8);
                        }
                    }
                    let text_data_bits =
                        reader.text_remaining_bits() as u32;
                    let mut text_data =
                        vec![0u8; text_data_bits.div_ceil(8) as usize];
                    for bit_index in 0..text_data_bits as usize {
                        if reader.read_text_bit() {
                            text_data[bit_index / 8] |=
                                0x80 >> (bit_index % 8);
                        }
                    }
                    let mut object_ids = Vec::new();
                    while reader.handle_remaining_bits() >= 8 {
                        let (value, reference_type) =
                            reader.read_handle_reference(handle);
                        let kind = match reference_type {
                            crate::io::dwg::dwg_reference_type::DwgReferenceType::SoftOwnership => {
                                crate::objects::ProxyReferenceKind::SoftOwnership
                            }
                            crate::io::dwg::dwg_reference_type::DwgReferenceType::HardOwnership => {
                                crate::objects::ProxyReferenceKind::HardOwnership
                            }
                            crate::io::dwg::dwg_reference_type::DwgReferenceType::SoftPointer => {
                                crate::objects::ProxyReferenceKind::SoftPointer
                            }
                            crate::io::dwg::dwg_reference_type::DwgReferenceType::HardPointer => {
                                crate::objects::ProxyReferenceKind::HardPointer
                            }
                            crate::io::dwg::dwg_reference_type::DwgReferenceType::Undefined => {
                                crate::objects::ProxyReferenceKind::Undefined
                            }
                        };
                        object_ids.push(crate::objects::ProxyObjectReference {
                            handle: Handle::from(value),
                            kind,
                        });
                    }
                    let payload =
                        crate::objects::ProxyPayload::from_bits(
                            &object_data,
                            object_data_bits,
                        );
                    let object_type = if let Some(envelope) =
                        crate::objects::semantic_property::decode_registered_class_envelope(
                            &payload,
                        )
                    {
                        crate::objects::ObjectType::RegisteredClass(
                            crate::objects::RegisteredClassObject {
                                handle: Handle::from(handle),
                                owner: owner_handle,
                                reactors: non_entity_data
                                    .reactors
                                    .iter()
                                    .copied()
                                    .map(Handle::from)
                                    .collect(),
                                xdictionary_handle: non_entity_data
                                    .xdictionary_handle
                                    .map(Handle::from),
                                dxf_name: envelope.dxf_name,
                                cpp_class_name: envelope.cpp_class_name,
                                properties: envelope.properties,
                                payload: envelope.payload,
                                object_ids,
                                raw_dwg_data: None,
                                raw_dwg_handle_bits: 0,
                                raw_dwg_version: None,
                            },
                        )
                    } else {
                        crate::objects::ObjectType::ProxyObject(
                            crate::objects::ProxyObject {
                                handle: Handle::from(handle),
                                owner: owner_handle,
                                reactors: non_entity_data
                                    .reactors
                                    .iter()
                                    .copied()
                                    .map(Handle::from)
                                    .collect(),
                                xdictionary_handle: non_entity_data
                                    .xdictionary_handle
                                    .map(Handle::from),
                                proxy_id: 499,
                                class_id,
                                dxf_subclass,
                                version,
                                dwg_version,
                                maintenance_version,
                                from_dxf,
                                payload,
                                text_payload:
                                    crate::objects::ProxyPayload::from_bits(
                                        &text_data,
                                        text_data_bits,
                                    ),
                                object_ids,
                            },
                        )
                    };
                    document.objects.insert(
                        Handle::from(handle),
                        object_type,
                    );
                }
                OBJ_VISUALSTYLE => {
                    let mut obj = objects::read_visual_style(
                        &mut reader,
                        self.obj_reader.version(),
                        self.obj_reader.dxf_version(),
                    );
                    obj.handle = Handle::from(handle);
                    obj.owner = owner_handle;
                    obj.reactors = non_entity_data
                        .reactors
                        .iter()
                        .copied()
                        .map(Handle::from)
                        .collect();
                    obj.xdictionary_handle = non_entity_data
                        .xdictionary_handle
                        .map(Handle::from);
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::VisualStyle(obj),
                    );
                }
                OBJ_MATERIAL => {
                    let mut obj =
                        objects::read_material(&mut reader, self.obj_reader.version());
                    obj.handle = Handle::from(handle);
                    obj.owner = owner_handle;
                    obj.reactors = non_entity_data
                        .reactors
                        .iter()
                        .copied()
                        .map(Handle::from)
                        .collect();
                    obj.xdictionary_handle = non_entity_data
                        .xdictionary_handle
                        .map(Handle::from);
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::Material(obj),
                    );
                }
                OBJ_TABLECONTENT => {
                    let (
                        name,
                        description,
                        columns,
                        rows,
                        field_handles,
                        base_style,
                        merged_ranges,
                        style_handle,
                    ) = entities::read_table_content(
                        &mut reader,
                        self.obj_reader.version(),
                    );
                    let mut obj = crate::entities::Table::new(
                        crate::types::Vector3::ZERO,
                        0,
                        0,
                    );
                    obj.common.handle = Handle::from(handle);
                    obj.common.owner_handle = owner_handle;
                    obj.common.reactors = non_entity_data
                        .reactors
                        .iter()
                        .copied()
                        .map(Handle::from)
                        .collect();
                    obj.common.xdictionary_handle = non_entity_data
                        .xdictionary_handle
                        .map(Handle::from);
                    obj.name = name;
                    obj.description = description;
                    obj.columns = columns;
                    obj.rows = rows;
                    obj.field_handles = field_handles;
                    obj.base_style = base_style;
                    obj.merged_ranges = merged_ranges;
                    obj.table_style_handle = (style_handle != 0)
                        .then(|| Handle::from(style_handle));
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::TableContent(obj),
                    );
                }
                OBJ_TABLESTYLE => {
                    let mut obj =
                        objects::read_table_style(&mut reader, self.obj_reader.version());
                    obj.handle = Handle::from(handle);
                    obj.owner_handle = owner_handle;
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::TableStyle(obj),
                    );
                }
                OBJ_GEODATA => {
                    let data = objects::read_geodata(&mut reader);
                    let mut obj = crate::objects::GeoData::new();
                    obj.handle = Handle::from(handle);
                    obj.owner = owner_handle;
                    obj.reactors = non_entity_data
                        .reactors
                        .iter()
                        .copied()
                        .map(Handle::from)
                        .collect();
                    obj.xdictionary_handle = non_entity_data
                        .xdictionary_handle
                        .map(Handle::from);
                    obj.version = data.version;
                    obj.host_block = Handle::from(data.host_block);
                    obj.coordinate_type = data.coordinate_type;
                    obj.design_point = data.design_point;
                    obj.reference_point = data.reference_point;
                    obj.obsolete_observation_point = data.obsolete_observation_point;
                    obj.obsolete_scale_vector = data.obsolete_scale_vector;
                    obj.north_direction = data.north_direction;
                    obj.up_direction = data.up_direction;
                    obj.horizontal_unit_scale = data.horizontal_unit_scale;
                    obj.vertical_unit_scale = data.vertical_unit_scale;
                    obj.horizontal_units = data.horizontal_units;
                    obj.vertical_units = data.vertical_units;
                    obj.scale_estimation_method = data.scale_estimation_method;
                    obj.user_scale_factor = data.user_scale_factor;
                    obj.sea_level_correction = data.sea_level_correction;
                    obj.sea_level_elevation = data.sea_level_elevation;
                    obj.coordinate_projection_radius = data.coordinate_projection_radius;
                    obj.coordinate_system_definition = data.coordinate_system_definition;
                    obj.coordinate_system_datum = data.coordinate_system_datum;
                    obj.coordinate_system_wkt = data.coordinate_system_wkt;
                    obj.geo_rss_tag = data.geo_rss_tag;
                    obj.observation_from_tag = data.observation_from_tag;
                    obj.observation_to_tag = data.observation_to_tag;
                    obj.observation_coverage_tag = data.observation_coverage_tag;
                    obj.mesh_points = data
                        .mesh_points
                        .into_iter()
                        .map(|(source, destination)| crate::objects::GeoDataMeshPoint {
                            source,
                            destination,
                        })
                        .collect();
                    obj.mesh_faces = data
                        .mesh_faces
                        .into_iter()
                        .map(|(first, second, third)| crate::objects::GeoDataMeshFace {
                            first,
                            second,
                            third,
                        })
                        .collect();
                    obj.civil_data_present = data.civil_data_present;
                    obj.civil_obsolete_flag = data.civil_obsolete_flag;
                    obj.civil_reference_point1 = data.civil_reference_point1;
                    obj.civil_reference_point2 = data.civil_reference_point2;
                    obj.civil_unknown1 = data.civil_unknown1;
                    obj.civil_unknown2 = data.civil_unknown2;
                    obj.civil_unknown_flag1 = data.civil_unknown_flag1;
                    obj.civil_zero_point1 = data.civil_zero_point1;
                    obj.civil_zero_point2 = data.civil_zero_point2;
                    obj.civil_unknown_flag2 = data.civil_unknown_flag2;
                    obj.civil_north_angle_degrees = data.civil_north_angle_degrees;
                    obj.civil_north_angle_radians = data.civil_north_angle_radians;
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::GeoData(obj),
                    );
                }
                OBJ_SPATIALFILTER => {
                    let data = objects::read_spatial_filter(&mut reader);
                    let mut obj = crate::objects::SpatialFilter::new();
                    obj.handle = Handle::from(handle);
                    obj.owner = owner_handle;
                    obj.boundary_points = data.points;
                    obj.normal = data.extrusion;
                    obj.origin = data.clip_bound_origin;
                    obj.display_enabled = data.display_enabled;
                    obj.front_clip = data.front_clip;
                    obj.back_clip = data.back_clip;
                    obj.inverse_block_transform =
                        matrix_from_row_major(&data.inverse_block_transform);
                    obj.clip_bound_transform = matrix_from_row_major(&data.clip_bound_transform);
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::SpatialFilter(obj),
                    );
                }
                OBJ_BLOCKVISIBILITYPARAMETER => {
                    let mut param = objects::read_block_visibility_parameter(&mut reader);
                    param.handle = Handle::from(handle);
                    param.owner = owner_handle;
                    document
                        .block_visibility_params
                        .insert(Handle::from(handle), param.clone());
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::BlockVisibilityParameter(param),
                    );
                }
                OBJ_BLOCKREPRESENTATIONDATA => {
                    let flags = reader.read_bit_short();
                    let block = Handle::from(reader.read_handle());
                    document
                        .block_representations
                        .insert(Handle::from(handle), block);
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::DynamicBlock(
                            crate::objects::DynamicBlockObject {
                                handle: Handle::from(handle),
                                owner: owner_handle,
                                reactors: non_entity_data
                                    .reactors
                                    .iter()
                                    .copied()
                                    .map(Handle::from)
                                    .collect(),
                                xdictionary_handle: non_entity_data
                                    .xdictionary_handle
                                    .map(Handle::from),
                                dxf_name:
                                    "ACDB_BLOCKREPRESENTATION_DATA".to_string(),
                                cpp_class_name:
                                    "AcDbBlockRepresentationData".to_string(),
                                data:
                                    crate::objects::DynamicBlockData::Representation(
                                        crate::objects::BlockRepresentationData {
                                            flags,
                                            block,
                                        },
                                    ),
                            },
                        ),
                    );
                }
                OBJ_FIELD => {
                    let mut data =
                        crate::io::dwg::dwg_stream_readers::object_reader::field::read_field_object(
                            &mut reader,
                            self.obj_reader.version(),
                        );
                    data.handle = Handle::from(handle);
                    data.owner = owner_handle;
                    document.fields.insert(
                        Handle::from(handle),
                        crate::document::FieldDef {
                            handle: Handle::from(handle),
                            owner: owner_handle,
                            evaluator: data.evaluator_id.clone(),
                            code: data.code.clone(),
                            objects: data.referenced_objects.clone(),
                        },
                    );
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::Field(data),
                    );
                }
                OBJ_FIELDLIST => {
                    let mut data =
                        crate::io::dwg::dwg_stream_readers::object_reader::field::read_field_list(
                            &mut reader,
                        );
                    data.handle = Handle::from(handle);
                    data.owner = owner_handle;
                    document.objects.insert(
                        Handle::from(handle),
                        crate::objects::ObjectType::FieldList(data),
                    );
                }
                _ => {
                    // Annotative object-context leaves (*OBJECTCONTEXTDATA) carry
                    // their annotation scale as the FIRST object-specific handle
                    // (right after the common owner/reactors/xdict handles that
                    // read_common_non_entity_data already consumed). The
                    // data-stream and handle-stream read cursors are independent,
                    // and raw_merged_data()/get_handle_bits() snapshot the whole
                    // object independent of either cursor — so we can decode the
                    // fields we understand AND still capture the verbatim record.
                    let class_name = class_names
                        .dxf
                        .get(&type_code)
                        .map(|name| name.to_uppercase());
                    if let Some(dxf_name) = class_name.as_deref() {
                        if crate::objects::is_associative_object_name(dxf_name) {
                            if let Some(data) =
                                crate::io::dwg::dwg_stream_readers::object_reader::associative::read_associative_data(
                                    &mut reader,
                                    dxf_name,
                                    self.obj_reader.version(),
                                    document.version,
                                )
                            {
                                let cpp_class_name = class_names
                                    .cpp
                                    .get(&type_code)
                                    .cloned()
                                    .or_else(|| {
                                        crate::objects::associative_cpp_class_name(dxf_name)
                                            .map(str::to_string)
                                    })
                                    .unwrap_or_default();
                                document.objects.insert(
                                    Handle::from(handle),
                                    crate::objects::ObjectType::Associative(
                                        crate::objects::AssociativeObject {
                                            handle: Handle::from(handle),
                                            owner: owner_handle,
                                            reactors: non_entity_data
                                                .reactors
                                                .iter()
                                                .map(|&value| Handle::from(value))
                                                .collect(),
                                            xdictionary_handle: non_entity_data
                                                .xdictionary_handle
                                                .map(Handle::from),
                                            dxf_name: dxf_name.to_string(),
                                            cpp_class_name,
                                            data,
                                            source_version: Some(document.version),
                                        },
                                    ),
                                );
                                return;
                            }
                        }
                        if let Some(data) =
                            crate::io::dwg::dwg_stream_readers::object_reader::data_objects::read_data_object_data(
                                &mut reader,
                                dxf_name,
                            )
                        {
                            document.objects.insert(
                                Handle::from(handle),
                                crate::objects::ObjectType::DataObject(
                                    crate::objects::DataObject {
                                        handle: Handle::from(handle),
                                        owner: owner_handle,
                                        reactors: non_entity_data
                                            .reactors
                                            .iter()
                                            .copied()
                                            .map(Handle::from)
                                            .collect(),
                                        xdictionary_handle: non_entity_data
                                            .xdictionary_handle
                                            .map(Handle::from),
                                        data,
                                    },
                                ),
                            );
                            return;
                        }
                        if let Some(data) =
                            crate::io::dwg::dwg_stream_readers::object_reader::dynamic_block::read_dynamic_block_data(
                                &mut reader,
                                dxf_name,
                            )
                        {
                            let cpp_class_name = class_names
                                .cpp
                                .get(&type_code)
                                .cloned()
                                .unwrap_or_default();
                            document.objects.insert(
                                Handle::from(handle),
                                crate::objects::ObjectType::DynamicBlock(
                                    crate::objects::DynamicBlockObject {
                                        handle: Handle::from(handle),
                                        owner: owner_handle,
                                        reactors: non_entity_data
                                            .reactors
                                            .iter()
                                            .copied()
                                            .map(Handle::from)
                                            .collect(),
                                        xdictionary_handle: non_entity_data
                                            .xdictionary_handle
                                            .map(Handle::from),
                                        dxf_name: dxf_name.to_string(),
                                        cpp_class_name,
                                        data,
                                    },
                                ),
                            );
                            return;
                        }
                        if let Ok(dwg_version) =
                            crate::io::dwg::dwg_version::DwgVersion::from_dxf_version(
                                document.version,
                            )
                        {
                            if let Some(data) =
                                crate::io::dwg::dwg_stream_readers::object_reader::dynamic_block::read_solid_history_data(
                                    &mut reader,
                                    dxf_name,
                                    dwg_version,
                                    document.version,
                                )
                            {
                                let cpp_class_name = class_names
                                    .cpp
                                    .get(&type_code)
                                    .cloned()
                                    .unwrap_or_default();
                                document.objects.insert(
                                    Handle::from(handle),
                                    crate::objects::ObjectType::DynamicBlock(
                                        crate::objects::DynamicBlockObject {
                                            handle: Handle::from(handle),
                                            owner: owner_handle,
                                            reactors: non_entity_data
                                                .reactors
                                                .iter()
                                                .copied()
                                                .map(Handle::from)
                                                .collect(),
                                            xdictionary_handle: non_entity_data
                                                .xdictionary_handle
                                                .map(Handle::from),
                                            dxf_name: dxf_name.to_string(),
                                            cpp_class_name,
                                            data,
                                        },
                                    ),
                                );
                                return;
                            }
                        }
                        let class_name_upper = dxf_name.to_uppercase();
                        if matches!(
                            class_name_upper.as_str(),
                            "LSDEFINITION"
                                | "LSSYMBOLCOMPONENT"
                                | "LSCOMPOUNDCOMPONENT"
                                | "LSSTROKEPATTERNCOMPONENT"
                                | "LSPOINTCOMPONENT"
                                | "LSINTERNALCOMPONENT"
                        ) {
                            let h = Handle::from(handle);
                            let Some(data) =
                                crate::io::dwg::dwg_stream_readers::object_reader::dgn_linestyle::read_dgn_line_style_data(
                                    &mut reader,
                                    &class_name_upper,
                                )
                            else {
                                // Unsupported DGN encodings must remain lossless.
                                // Do not allocate fabricated component arrays or
                                // silently discard the original record.
                                document.objects.insert(h, crate::objects::ObjectType::Unknown {
                                    type_name: dxf_name.to_string(),
                                    handle: h,
                                    owner: owner_handle,
                                    raw_dxf_codes: None,
                                    raw_dwg_data: Some(reader.raw_merged_data()),
                                    raw_dwg_handle_bits: reader.get_handle_bits(),
                                    raw_dwg_version: Some(document.version),
                                });
                                return;
                            };
                            match &data {
                                crate::objects::DgnLineStyleData::Definition {
                                    description,
                                    root_component,
                                    ..
                                } => {
                                    document.dgn_ls_definitions.insert(
                                        h,
                                        crate::objects::DgnLsDefinition {
                                            handle: h,
                                            name: description.clone(),
                                            root_component: *root_component,
                                        },
                                    );
                                }
                                crate::objects::DgnLineStyleData::Component {
                                    kind,
                                    description,
                                    scale,
                                    component,
                                    ..
                                } => {
                                    let references = component.references();
                                    document.dgn_ls_components.insert(
                                        h,
                                        crate::objects::DgnLsComponent {
                                            handle: h,
                                            component_type: *kind,
                                            description: description.clone(),
                                            refs: references,
                                            scale: *scale,
                                        },
                                    );
                                }
                                crate::objects::DgnLineStyleData::Registered {
                                    ..
                                } => {}
                            }
                            document.objects.insert(
                                h,
                                crate::objects::ObjectType::DgnLineStyle(
                                    crate::objects::DgnLineStyleObject {
                                        handle: h,
                                        owner: owner_handle,
                                        reactors: non_entity_data
                                            .reactors
                                            .iter()
                                            .copied()
                                            .map(Handle::from)
                                            .collect(),
                                        xdictionary_handle: non_entity_data
                                            .xdictionary_handle
                                            .map(Handle::from),
                                        data,
                                    },
                                ),
                            );
                            return;
                        }
                        if matches!(
                            class_name_upper.as_str(),
                            "ACAD_PROXY_OBJECT_WRAPPER"
                                | "AEC_REFEDIT_STATUS_TRACKER"
                                | "EXACXREFPANELOBJECT"
                                | "XREFPANELOBJECT"
                                | "NPOCOLLECTION"
                                | "MCDBCONTAINER2"
                        ) {
                            let (payload, object_ids) =
                                read_registered_payload(&mut reader, handle);
                            document.objects.insert(
                                Handle::from(handle),
                                crate::objects::ObjectType::RegisteredClass(
                                    crate::objects::RegisteredClassObject {
                                        handle: Handle::from(handle),
                                        owner: owner_handle,
                                        reactors: non_entity_data
                                            .reactors
                                            .iter()
                                            .copied()
                                            .map(Handle::from)
                                            .collect(),
                                        xdictionary_handle: non_entity_data
                                            .xdictionary_handle
                                            .map(Handle::from),
                                        dxf_name: dxf_name.to_string(),
                                        cpp_class_name: class_names
                                            .cpp
                                            .get(&type_code)
                                            .cloned()
                                            .unwrap_or_default(),
                                        properties: Vec::new(),
                                        payload,
                                        object_ids,
                                        raw_dwg_data: Some(reader.raw_merged_data()),
                                        raw_dwg_handle_bits: reader.get_handle_bits(),
                                        raw_dwg_version: Some(document.version),
                                    },
                                ),
                            );
                            return;
                        }
                        if let Some(data) =
                            crate::io::dwg::dwg_stream_readers::object_reader::class_object::read_class_object_data(
                                &mut reader,
                                dxf_name,
                                self.obj_reader.version(),
                                self.obj_reader.dxf_version(),
                            )
                        {
                            if let crate::objects::ClassObjectData::SectionViewStyle(
                                style,
                            ) = &data
                            {
                                document.section_view_style = Some(
                                    crate::entities::SectionViewStyle {
                                        show_arrows: style.flags & 0x02 != 0,
                                        show_plane_line: style.flags & 0x08 != 0,
                                        show_end_lines: style.flags & 0x20 != 0,
                                        arrow_size: style.arrow_symbol_size,
                                        arrow_extension: style
                                            .arrow_symbol_extension_length,
                                        label_height: style.identifier_height,
                                        label_offset: style.identifier_offset,
                                        label_position: style
                                            .identifier_position,
                                        arrow_position: style.arrow_position,
                                        end_line_length: style.end_line_length,
                                        end_line_overshoot: style
                                            .end_line_overshoot,
                                        arrow_start_handle: style
                                            .arrow_start_symbol
                                            .value(),
                                        arrow_end_handle: style
                                            .arrow_end_symbol
                                            .value(),
                                        arrow_is_default: style
                                            .arrow_start_symbol
                                            .is_null()
                                            && style
                                                .arrow_end_symbol
                                                .is_null(),
                                    },
                                );
                            }
                            document.objects.insert(
                                Handle::from(handle),
                                crate::objects::ObjectType::ClassObject(
                                    crate::objects::ClassObject {
                                        handle: Handle::from(handle),
                                        owner: owner_handle,
                                        reactors: non_entity_data
                                            .reactors
                                            .iter()
                                            .copied()
                                            .map(Handle::from)
                                            .collect(),
                                        xdictionary_handle: non_entity_data
                                            .xdictionary_handle
                                            .map(Handle::from),
                                        data,
                                    },
                                ),
                            );
                            return;
                        }
                    }
                    let is_context_data = class_name
                        .as_deref()
                        .map(|n| {
                            n.contains("OBJECTCONTEXTDATA")
                                || n == "ACDB_HATCHSCALECONTEXTDATA_CLASS"
                                || n == "ACDB_HATCHVIEWCONTEXTDATA_CLASS"
                        })
                        .unwrap_or(false);
                    // Decode every context leaf with a known schema into native,
                    // version-portable fields. These objects no longer depend
                    // on same-version raw-record passthrough.
                    let modeled = if is_context_data {
                        match class_name.as_deref().unwrap_or("") {
                            "ACDB_ANNOTSCALEOBJECTCONTEXTDATA_CLASS" => {
                                let class_version = reader.read_bit_short();
                                let is_default = reader.read_bit();
                                let scale = reader.read_handle();
                                Some((
                                    crate::objects::ObjectContextKind::AnnotScale,
                                    class_version,
                                    is_default,
                                    scale,
                                ))
                            }
                            "ACDB_BLKREFOBJECTCONTEXTDATA_CLASS" => {
                                let class_version = reader.read_bit_short();
                                let is_default = reader.read_bit();
                                let rotation = reader.read_bit_double();
                                let insertion = crate::types::Vector3::new(
                                    reader.read_bit_double(),
                                    reader.read_bit_double(),
                                    reader.read_bit_double(),
                                );
                                let scale_factor = crate::types::Vector3::new(
                                    reader.read_bit_double(),
                                    reader.read_bit_double(),
                                    reader.read_bit_double(),
                                );
                                let scale = reader.read_handle();
                                Some((
                                    crate::objects::ObjectContextKind::BlkRef {
                                        rotation,
                                        insertion,
                                        scale_factor,
                                    },
                                    class_version,
                                    is_default,
                                    scale,
                                ))
                            }
                            "ACDB_TEXTOBJECTCONTEXTDATA_CLASS" => {
                                let class_version = reader.read_bit_short();
                                let is_default = reader.read_bit();
                                let horizontal_mode = reader.read_bit_short();
                                let rotation = reader.read_bit_double();
                                let insertion = crate::types::Vector2::new(
                                    reader.read_raw_double(),
                                    reader.read_raw_double(),
                                );
                                let alignment = crate::types::Vector2::new(
                                    reader.read_raw_double(),
                                    reader.read_raw_double(),
                                );
                                let scale = reader.read_handle();
                                Some((
                                    crate::objects::ObjectContextKind::Text {
                                        horizontal_mode,
                                        rotation,
                                        insertion,
                                        alignment,
                                    },
                                    class_version,
                                    is_default,
                                    scale,
                                ))
                            }
                            "ACDB_MTEXTOBJECTCONTEXTDATA_CLASS" => {
                                let class_version = reader.read_bit_short();
                                let is_default = reader.read_bit();
                                let attachment = reader.read_bit_long();
                                // Binary stores x_axis_dir BEFORE ins_pt.
                                let x_axis_dir = crate::types::Vector3::new(
                                    reader.read_bit_double(),
                                    reader.read_bit_double(),
                                    reader.read_bit_double(),
                                );
                                let insertion = crate::types::Vector3::new(
                                    reader.read_bit_double(),
                                    reader.read_bit_double(),
                                    reader.read_bit_double(),
                                );
                                let rect_width = reader.read_bit_double();
                                let rect_height = reader.read_bit_double();
                                let extents_width = reader.read_bit_double();
                                let extents_height = reader.read_bit_double();
                                let column_type = reader.read_bit_long();
                                let columns = if column_type != 0 {
                                    let num_heights = reader.read_bit_long();
                                    let width = reader.read_bit_double();
                                    let gutter = reader.read_bit_double();
                                    let auto_height = reader.read_bit();
                                    let flow_reversed = reader.read_bit();
                                    let heights = if !auto_height && column_type == 2 {
                                        (0..num_heights.max(0))
                                            .map(|_| reader.read_bit_double())
                                            .collect()
                                    } else {
                                        Vec::new()
                                    };
                                    Some(crate::objects::MTextColumns {
                                        num_heights,
                                        width,
                                        gutter,
                                        auto_height,
                                        flow_reversed,
                                        heights,
                                    })
                                } else {
                                    None
                                };
                                let scale = reader.read_handle();
                                Some((
                                    crate::objects::ObjectContextKind::MText(
                                        crate::objects::MTextContext {
                                            attachment,
                                            x_axis_dir,
                                            insertion,
                                            rect_width,
                                            rect_height,
                                            extents_width,
                                            extents_height,
                                            column_type,
                                            columns,
                                        },
                                    ),
                                    class_version,
                                    is_default,
                                    scale,
                                ))
                            }
                            "ACDB_MLEADEROBJECTCONTEXTDATA_CLASS" => {
                                let class_version = reader.read_bit_short();
                                let is_default = reader.read_bit();
                                let scale = reader.read_handle();
                                let context = entities::read_multileader_annotation_context(
                                    &mut reader,
                                    self.obj_reader.version(),
                                    self.obj_reader.dxf_version(),
                                    true,
                                );
                                Some((
                                    crate::objects::ObjectContextKind::MLeader(
                                        context,
                                    ),
                                    class_version,
                                    is_default,
                                    scale,
                                ))
                            }
                            "ACDB_MTEXTATTRIBUTEOBJECTCONTEXTDATA_CLASS" => {
                                let class_version = reader.read_bit_short();
                                let is_default = reader.read_bit();
                                let horizontal_mode = reader.read_bit_short();
                                let rotation = reader.read_bit_double();
                                let insertion = crate::types::Vector2::new(
                                    reader.read_raw_double(),
                                    reader.read_raw_double(),
                                );
                                let alignment = crate::types::Vector2::new(
                                    reader.read_raw_double(),
                                    reader.read_raw_double(),
                                );
                                let enable_context = reader.read_bit();
                                let context_data = if enable_context {
                                    // The nested object omits its type, handle and
                                    // EED preface, but retains the non-entity common
                                    // tail before AcDbObjectContextData.
                                    let embedded_reactor_count =
                                        reader.read_bit_long().max(0).min(100_000)
                                            as usize;
                                    let embedded_no_xdic = reader.read_bit();
                                    let embedded_has_binary_data = if self
                                        .obj_reader
                                        .version()
                                        .r2013_plus(self.obj_reader.dxf_version())
                                    {
                                        reader.read_bit()
                                    } else {
                                        false
                                    };
                                    let embedded_class_version =
                                        reader.read_bit_short();
                                    let embedded_is_default =
                                        reader.read_bit();
                                    let attachment = reader.read_bit_long();
                                    let x_axis_dir = crate::types::Vector3::new(
                                        reader.read_bit_double(),
                                        reader.read_bit_double(),
                                        reader.read_bit_double(),
                                    );
                                    let context_insertion =
                                        crate::types::Vector3::new(
                                            reader.read_bit_double(),
                                            reader.read_bit_double(),
                                            reader.read_bit_double(),
                                        );
                                    let rect_width = reader.read_bit_double();
                                    let rect_height = reader.read_bit_double();
                                    let extents_width =
                                        reader.read_bit_double();
                                    let extents_height =
                                        reader.read_bit_double();
                                    let column_type = reader.read_bit_long();
                                    let columns = if column_type != 0 {
                                        let num_heights =
                                            reader.read_bit_long();
                                        let width = reader.read_bit_double();
                                        let gutter = reader.read_bit_double();
                                        let auto_height = reader.read_bit();
                                        let flow_reversed = reader.read_bit();
                                        let heights =
                                            if !auto_height && column_type == 2 {
                                                (0..num_heights.max(0))
                                                    .map(|_| {
                                                        reader.read_bit_double()
                                                    })
                                                    .collect()
                                            } else {
                                                Vec::new()
                                            };
                                        Some(crate::objects::MTextColumns {
                                            num_heights,
                                            width,
                                            gutter,
                                            auto_height,
                                            flow_reversed,
                                            heights,
                                        })
                                    } else {
                                        None
                                    };
                                    Some((
                                        embedded_reactor_count,
                                        embedded_no_xdic,
                                        embedded_has_binary_data,
                                        embedded_class_version,
                                        embedded_is_default,
                                        crate::objects::MTextContext {
                                            attachment,
                                            x_axis_dir,
                                            insertion: context_insertion,
                                            rect_width,
                                            rect_height,
                                            extents_width,
                                            extents_height,
                                            column_type,
                                            columns,
                                        },
                                    ))
                                } else {
                                    None
                                };
                                let scale = reader.read_handle();
                                let context = context_data.map(
                                    |(
                                        embedded_reactor_count,
                                        embedded_no_xdic,
                                        embedded_has_binary_data,
                                        embedded_class_version,
                                        embedded_is_default,
                                        mtext,
                                    )| {
                                    let embedded_owner =
                                        reader.read_handle();
                                    let embedded_reactors = (0
                                        ..embedded_reactor_count)
                                        .map(|_| Handle::from(reader.read_handle()))
                                        .collect();
                                    let embedded_xdictionary_handle =
                                        if embedded_no_xdic {
                                            None
                                        } else {
                                            Some(Handle::from(
                                                reader.read_handle(),
                                            ))
                                        };
                                    let context_scale =
                                        reader.read_handle();
                                    crate::objects::EmbeddedMTextContext {
                                        owner_handle: Handle::from(
                                            embedded_owner,
                                        ),
                                        reactors: embedded_reactors,
                                        xdictionary_handle:
                                            embedded_xdictionary_handle,
                                        has_binary_data:
                                            embedded_has_binary_data,
                                        class_version:
                                            embedded_class_version,
                                        is_default: embedded_is_default,
                                        scale: Handle::from(context_scale),
                                        mtext,
                                    }
                                });
                                Some((
                                    crate::objects::ObjectContextKind::MTextAttribute(
                                        crate::objects::MTextAttributeContext {
                                            horizontal_mode,
                                            rotation,
                                            insertion,
                                            alignment,
                                            enable_context,
                                            context,
                                        },
                                    ),
                                    class_version,
                                    is_default,
                                    scale,
                                ))
                            }
                            "ACDB_LEADEROBJECTCONTEXTDATA_CLASS" => {
                                let class_version = reader.read_bit_short();
                                let is_default = reader.read_bit();
                                let mut points = Vec::new();
                                for _ in 0..reader.read_bit_long().max(0).min(100_000) {
                                    points.push(reader.read_3bit_double());
                                }
                                let x_direction = reader.read_3bit_double();
                                let annotation_enabled = reader.read_bit();
                                let insertion_offset =
                                    reader.read_3bit_double();
                                let endpoint_projection =
                                    reader.read_3bit_double();
                                let scale = reader.read_handle();
                                Some((
                                    crate::objects::ObjectContextKind::Leader(
                                        crate::objects::LeaderContext {
                                            points,
                                            x_direction,
                                            annotation_enabled,
                                            insertion_offset,
                                            endpoint_projection,
                                        },
                                    ),
                                    class_version,
                                    is_default,
                                    scale,
                                ))
                            }
                            "ACDB_FCFOBJECTCONTEXTDATA_CLASS" => {
                                let class_version = reader.read_bit_short();
                                let is_default = reader.read_bit();
                                let location = crate::types::Vector3::new(
                                    reader.read_bit_double(),
                                    reader.read_bit_double(),
                                    reader.read_bit_double(),
                                );
                                let horizontal_direction = crate::types::Vector3::new(
                                    reader.read_bit_double(),
                                    reader.read_bit_double(),
                                    reader.read_bit_double(),
                                );
                                let scale = reader.read_handle();
                                Some((
                                    crate::objects::ObjectContextKind::Fcf {
                                        location,
                                        horizontal_direction,
                                    },
                                    class_version,
                                    is_default,
                                    scale,
                                ))
                            }
                            "ACDB_HATCHSCALECONTEXTDATA_CLASS" => {
                                let class_version = reader.read_bit_short();
                                let is_default = reader.read_bit();
                                let scale = reader.read_handle();
                                Some((
                                    crate::objects::ObjectContextKind::HatchScale(
                                        read_hatch_scale_context_data(
                                            &mut reader,
                                            self.obj_reader.version(),
                                        ),
                                    ),
                                    class_version,
                                    is_default,
                                    scale,
                                ))
                            }
                            "ACDB_HATCHVIEWCONTEXTDATA_CLASS" => {
                                let class_version = reader.read_bit_short();
                                let is_default = reader.read_bit();
                                let hatch = read_hatch_scale_context_data(
                                    &mut reader,
                                    self.obj_reader.version(),
                                );
                                let view_normal = crate::types::Vector3::new(
                                    reader.read_bit_double(),
                                    reader.read_bit_double(),
                                    reader.read_bit_double(),
                                );
                                let view_rotation = reader.read_bit_double();
                                let evaluate_hatch = reader.read_bit();
                                let scale = reader.read_handle();
                                let view = reader.read_handle();
                                Some((
                                    crate::objects::ObjectContextKind::HatchView(
                                        crate::objects::HatchViewContext {
                                            hatch,
                                            view: Handle::from(view),
                                            view_normal,
                                            view_rotation,
                                            evaluate_hatch,
                                        },
                                    ),
                                    class_version,
                                    is_default,
                                    scale,
                                ))
                            }
                            "ACDB_ALDIMOBJECTCONTEXTDATA_CLASS"
                            | "ACDB_ANGDIMOBJECTCONTEXTDATA_CLASS"
                            | "ACDB_DMDIMOBJECTCONTEXTDATA_CLASS"
                            | "ACDB_RADIMOBJECTCONTEXTDATA_CLASS"
                            | "ACDB_RADIMLGOBJECTCONTEXTDATA_CLASS"
                            | "ACDB_ORDDIMOBJECTCONTEXTDATA_CLASS" => {
                                let class_version = reader.read_bit_short();
                                let is_default = reader.read_bit();
                                // AcDbDimensionObjectContextData base (data stream).
                                let def_pt = crate::types::Vector2::new(
                                    reader.read_raw_double(),
                                    reader.read_raw_double(),
                                );
                                let is_def_textloc = reader.read_bit();
                                let text_rotation = reader.read_bit_double();
                                let b293 = reader.read_bit();
                                let dimtofl = reader.read_bit();
                                let dimosxd = reader.read_bit();
                                let dimatfit = reader.read_bit();
                                let dimtix = reader.read_bit();
                                let dimtmove = reader.read_bit();
                                let override_code = reader.read_byte();
                                let has_arrow2 = reader.read_bit();
                                let flip_arrow2 = reader.read_bit();
                                let flip_arrow1 = reader.read_bit();
                                let mut p3 = || {
                                    crate::types::Vector3::new(
                                        reader.read_bit_double(),
                                        reader.read_bit_double(),
                                        reader.read_bit_double(),
                                    )
                                };
                                let subtype = match class_name.as_deref().unwrap_or("") {
                                    "ACDB_ALDIMOBJECTCONTEXTDATA_CLASS" => {
                                        crate::objects::DimSubtype::Aligned { dimline_pt: p3() }
                                    }
                                    "ACDB_ANGDIMOBJECTCONTEXTDATA_CLASS" => {
                                        crate::objects::DimSubtype::Angular { arc_pt: p3() }
                                    }
                                    "ACDB_DMDIMOBJECTCONTEXTDATA_CLASS" => {
                                        crate::objects::DimSubtype::Diametric {
                                            first_arc_pt: p3(),
                                            def_pt: p3(),
                                        }
                                    }
                                    "ACDB_RADIMOBJECTCONTEXTDATA_CLASS" => {
                                        crate::objects::DimSubtype::Radial { first_arc_pt: p3() }
                                    }
                                    "ACDB_RADIMLGOBJECTCONTEXTDATA_CLASS" => {
                                        crate::objects::DimSubtype::RadialLarge {
                                            ovr_center: p3(),
                                            jog_point: p3(),
                                        }
                                    }
                                    _ => crate::objects::DimSubtype::Ordinate {
                                        feature_location_pt: p3(),
                                        leader_endpt: p3(),
                                    },
                                };
                                drop(p3);
                                // Handle stream: scale (soft owner) then block (hard ptr).
                                let scale = reader.read_handle();
                                let block = reader.read_handle();
                                Some((
                                    crate::objects::ObjectContextKind::Dim(
                                        crate::objects::DimContext {
                                            def_pt,
                                            is_def_textloc,
                                            text_rotation,
                                            block: Handle::from(block),
                                            b293,
                                            dimtofl,
                                            dimosxd,
                                            dimatfit,
                                            dimtix,
                                            dimtmove,
                                            override_code,
                                            has_arrow2,
                                            flip_arrow2,
                                            flip_arrow1,
                                            subtype,
                                        },
                                    ),
                                    class_version,
                                    is_default,
                                    scale,
                                ))
                            }
                            _ => None,
                        }
                    } else {
                        None
                    };

                    let raw_handle_bits = reader.get_handle_bits();
                    let raw_data = reader.raw_merged_data();

                    // Model-documentation view graph, recorded so section marks
                    // can derive their viewing direction from real file data:
                    // - AcDbViewRep: keep its object-specific handle references
                    //   (they include the view's border entity).
                    // - AcDbViewRepSectionDefinition: its owner is the section
                    //   (result) view's AcDbViewRep.
                    match class_name.as_deref() {
                        Some("ACDBVIEWREP") => {
                            let mut hs: Vec<Handle> = Vec::new();
                            for _ in 0..20 {
                                let h = reader.read_handle();
                                hs.push(Handle::from(h));
                            }
                            while hs.last().map(|h| h.value()) == Some(0) {
                                hs.pop();
                            }
                            document.view_rep_refs.insert(Handle::from(handle), hs);
                        }
                        Some("ACDBVIEWREPSECTIONDEFINITION") => {
                            let owner = Handle::from(non_entity_data.owner_handle);
                            if owner.value() != 0
                                && !document.section_view_reps.contains(&owner)
                            {
                                document.section_view_reps.push(owner);
                            }
                        }
                        _ => {}
                    }

                    // AcDbSectionViewStyle: decode the display fields (arrow
                    // size, label height, line/arrow visibility) that drive the
                    // section-mark renderer. The reader sits at the class-specific
                    // data (common non-entity data already consumed); the raw bytes
                    // above still drive verbatim write-back. Keep the first found —
                    // a drawing normally has a single active section-view style.
                    if class_name.as_deref() == Some("ACDBSECTIONVIEWSTYLE")
                        && document.section_view_style.is_none()
                    {
                        let r2018 = self
                            .obj_reader
                            .version()
                            .r2018_plus(self.obj_reader.dxf_version());
                        if let Some(svs) = decode_section_view_style(&mut reader, r2018) {
                            document.section_view_style = Some(svs);
                        }
                    }

                    if let Some((kind, class_version, is_default, scale)) = modeled {
                        if scale != 0 {
                            document
                                .context_scales
                                .insert(Handle::from(handle), Handle::from(scale));
                        }
                        let reactors = non_entity_data
                            .reactors
                            .iter()
                            .map(|&h| Handle::from(h))
                            .collect();
                        let xdictionary_handle =
                            non_entity_data.xdictionary_handle.map(Handle::from);
                        let object = crate::objects::ObjectContextData {
                            handle: Handle::from(handle),
                            owner_handle,
                            reactors,
                            xdictionary_handle,
                            class_version,
                            is_default,
                            scale: Handle::from(scale),
                            kind,
                        };
                        document.objects.insert(
                            Handle::from(handle),
                            crate::objects::ObjectType::ObjectContextData(object),
                        );
                    } else {
                        // Non-modeled context leaf: still capture its annotation
                        // scale (first object handle) into the side map, then
                        // preserve the whole object verbatim as Unknown. Other
                        // unrecognised non-entity objects: verbatim only.
                        if is_context_data {
                            let scale = reader.read_handle();
                            if scale != 0 {
                                document
                                    .context_scales
                                    .insert(Handle::from(handle), Handle::from(scale));
                            }
                        }
                        let type_name =
                            Self::unknown_object_type_name(class_names, raw_type_code);
                        document.objects.insert(
                            Handle::from(handle),
                            crate::objects::ObjectType::Unknown {
                                type_name,
                                handle: Handle::from(handle),
                                owner: owner_handle,
                                raw_dxf_codes: None,
                                raw_dwg_data: Some(raw_data),
                                raw_dwg_handle_bits: raw_handle_bits,
                                raw_dwg_version: Some(document.version),
                            },
                        );
                    }
                }
            }
        }
        // Table types already processed in Pass 1
    }

    /// Build a class_number → internal OBJ_* type code mapping.
    ///
    /// The DWG binary uses class numbers (≥500) for non-fixed object types.
    /// This builds a translation table so the builder can match them against
    /// the internal OBJ_* constants.
    fn build_class_type_map(document: &CadDocument) -> HashMap<i16, i16> {
        let mut map = HashMap::new();
        for class in document.classes.iter() {
            if let Some(internal_code) = dxf_name_to_type_code(&class.dxf_name) {
                if class.class_number >= 500 {
                    map.insert(class.class_number, internal_code);
                }
            }
        }
        map
    }

    /// Resolve a raw DWG type code to the internal OBJ_* constant.
    ///
    /// Fixed type codes (0–82) pass through unchanged.
    /// Class-based codes (≥500) are looked up in the class map.
    fn resolve_type_code(raw: i16, class_map: &HashMap<i16, i16>) -> i16 {
        if raw >= 500 {
            class_map.get(&raw).copied().unwrap_or(raw)
        } else {
            raw
        }
    }

    /// Name an unsupported object by its CLASSES-section dxfname when the
    /// drawing's class map knows it, falling back to the positional code.
    fn unknown_object_type_name(class_names: &ClassNames, raw_type_code: i16) -> String {
        class_names
            .dxf
            .get(&raw_type_code)
            .cloned()
            .unwrap_or_else(|| format!("DWG_OBJ_{}", raw_type_code))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classes::DxfClass;

    #[test]
    fn unknown_object_type_name_uses_class_dxf_name() {
        let mut document = CadDocument::new();
        document.classes.clear();
        let mut class = DxfClass::new("IRD_OBJ_RECORD", "AcDbIrdObjRecord");
        class.class_number = 524;
        document.classes.push_preserving(class);

        assert_eq!(
            DwgDocumentBuilder::unknown_object_type_name(
                &ClassNames::from_document(&document),
                524
            ),
            "IRD_OBJ_RECORD"
        );
    }

    #[test]
    fn unknown_object_type_name_falls_back_to_raw_class_number() {
        let mut document = CadDocument::new();
        document.classes.clear();

        assert_eq!(
            DwgDocumentBuilder::unknown_object_type_name(
                &ClassNames::from_document(&document),
                524
            ),
            "DWG_OBJ_524"
        );
    }

    #[test]
    fn unknown_object_type_name_uses_raw_number_after_type_resolution() {
        let mut document = CadDocument::new();
        document.classes.clear();
        let mut class = DxfClass::new("IRD_OBJ_RECORD", "AcDbIrdObjRecord");
        class.class_number = 524;
        document.classes.push_preserving(class);

        let mut class_map = HashMap::new();
        class_map.insert(524, 82);

        assert_eq!(DwgDocumentBuilder::resolve_type_code(524, &class_map), 82);
        assert_eq!(
            DwgDocumentBuilder::unknown_object_type_name(
                &ClassNames::from_document(&document),
                524
            ),
            "IRD_OBJ_RECORD"
        );
    }
}

fn read_registered_payload(
    reader: &mut crate::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader,
    current_handle: u64,
) -> (
    crate::objects::ProxyPayload,
    Vec<crate::objects::ProxyObjectReference>,
) {
    let bit_count = reader.main_remaining_bits().max(0) as u32;
    let mut data = vec![0u8; bit_count.div_ceil(8) as usize];
    for bit_index in 0..bit_count as usize {
        if reader.read_bit() {
            data[bit_index / 8] |= 0x80 >> (bit_index % 8);
        }
    }
    let mut references = Vec::new();
    while reader.handle_remaining_bits() >= 8 {
        let (handle, reference_type) = reader.read_handle_reference(current_handle);
        if handle == 0 {
            break;
        }
        let kind = match reference_type {
            crate::io::dwg::dwg_reference_type::DwgReferenceType::SoftOwnership => {
                crate::objects::ProxyReferenceKind::SoftOwnership
            }
            crate::io::dwg::dwg_reference_type::DwgReferenceType::HardOwnership => {
                crate::objects::ProxyReferenceKind::HardOwnership
            }
            crate::io::dwg::dwg_reference_type::DwgReferenceType::SoftPointer => {
                crate::objects::ProxyReferenceKind::SoftPointer
            }
            crate::io::dwg::dwg_reference_type::DwgReferenceType::HardPointer => {
                crate::objects::ProxyReferenceKind::HardPointer
            }
            crate::io::dwg::dwg_reference_type::DwgReferenceType::Undefined => {
                crate::objects::ProxyReferenceKind::Undefined
            }
        };
        references.push(crate::objects::ProxyObjectReference {
            handle: Handle::from(handle),
            kind,
        });
    }
    (
        crate::objects::ProxyPayload::from_bits(&data, bit_count),
        references,
    )
}

/// Build a [`Matrix4`](crate::types::Matrix4) from 12 doubles holding a 3×4
/// transform in row-major order (3 rows of 4: `[R | t]`); bottom row implied.
/// Decode an `AcDbSectionSymbol` from its `AcDbViewSymbol` base followed by the
/// complete repeated point records.
///
/// The field order is cross-validated with ODA DXF exports of native R2013 and
/// R2018 SECTIONLINE entities. Unknown-but-verified scalar fields are retained
/// under `raw_*` names instead of assigning speculative semantics.
fn decode_section_symbol(
    reader: &mut crate::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader,
) -> Option<SectionSymbol> {
    let mut symbol = SectionSymbol::new();
    symbol.view_symbol_version = reader.read_bit_short();
    symbol.style_handle = Handle::from(reader.read_handle());
    symbol.symbol_scale = reader.read_bit_double();
    symbol.view_rep_handle = Handle::from(reader.read_handle());
    symbol.raw_view_symbol_70 = reader.read_bit_short();
    symbol.version = reader.read_bit_short();
    symbol.raw_point_count_90 = reader.read_bit_long();
    symbol.raw_flags_90 = reader.read_bit_long();
    symbol.raw_point_record_count = reader.read_bit_long();

    let point_count = usize::try_from(symbol.raw_point_record_count).ok()?;
    if point_count > 1_000_000 {
        return None;
    }
    symbol.points.reserve(point_count);
    for _ in 0..point_count {
        symbol.points.push(crate::entities::SectionSymbolPoint {
            point: reader.read_3bit_double(),
            bulge: reader.read_bit_double(),
            label: reader.read_variable_text(),
            label_offset: reader.read_3bit_double(),
            raw_flag_280: reader.read_byte(),
        });
    }
    symbol.sync_display_fields();
    Some(symbol)
}

/// Decode the display fields of an `AcDbSectionViewStyle` (DWG class 795).
///
/// `reader` must be positioned at the class-specific data (after
/// `read_common_non_entity_data`). Fields are read in LibreDWG `dwg2.spec` order
/// (cross-validated against a real sample): the `AcDbModelDocViewStyle` base
/// (version, description, modified-flag), then the section-view fields through
/// `arrow_symbol_extension_length`. The DATA-stream reads and the interleaved
/// handle reads use independent cursors, so reading the two null arrow-symbol
/// handles in place keeps the DATA cursor aligned. R2013 files have no R2018+
/// base fields.
///
/// Returns the fields the renderer needs; the caller keeps the raw record for
/// verbatim write-back.
fn decode_section_view_style(
    reader: &mut crate::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader,
    r2018_plus: bool,
) -> Option<SectionViewStyle> {
    // AcDbModelDocViewStyle base.
    let _mdoc_class_version = reader.read_bit_short();
    let _desc = reader.read_variable_text();
    let _is_modified = reader.read_bit();
    // R2018+ added a display name and a style-flags word to the base class.
    if r2018_plus {
        let _display_name = reader.read_variable_text();
        let _viewstyle_flags = reader.read_bit_long();
    }
    // AcDbSectionViewStyle.
    let _class_version = reader.read_bit_short();
    let flags = reader.read_bit_long();
    let _identifier_color = reader.read_cm_color();
    let identifier_height = reader.read_bit_double();
    // Handle stream (independent cursor), in order: identifier_style, then the
    // two arrow-symbol handles. A null (0) arrow handle means the default arrow.
    let _identifier_style = reader.read_handle();
    let arrow_start = reader.read_handle();
    let arrow_end = reader.read_handle();
    let _arrow_color = reader.read_cm_color();
    let arrow_size = reader.read_bit_double();
    let _exclude = reader.read_variable_text();
    let arrow_extension = reader.read_bit_double();
    // Continue through the DATA stream (LibreDWG order) to the late-stored
    // placement fields: identifier_position/offset and arrow_position are
    // physically at the record's tail, after the plane/bend/end/view-label
    // and hatch groups.
    let _plane_linewt = reader.read_bit_long();
    let _plane_color = reader.read_cm_color();
    let _bend_linewt = reader.read_bit_long();
    let _bend_color = reader.read_cm_color();
    let _bend_line_length = reader.read_bit_double();
    let end_line_length = reader.read_bit_double();
    let _viewlabel_color = reader.read_cm_color();
    let _viewlabel_height = reader.read_bit_double();
    let _viewlabel_attachment = reader.read_bit_long();
    let _viewlabel_offset = reader.read_bit_double();
    let _viewlabel_alignment = reader.read_bit_long();
    let _hatch_color = reader.read_cm_color();
    let _hatch_bg_color = reader.read_cm_color();
    let _hatch_scale = reader.read_bit_double();
    let _hatch_transparency = reader.read_bit_long();
    let _unknown_b1 = reader.read_bit();
    let _unknown_b2 = reader.read_bit();
    let identifier_position = reader.read_bit_long();
    let identifier_offset = reader.read_bit_double();
    let arrow_position = reader.read_bit_long();
    let end_line_overshoot = reader.read_bit_double();

    // Sanity gate: a valid style has finite sizes.
    if !identifier_height.is_finite() || !arrow_size.is_finite() || !arrow_extension.is_finite() {
        return None;
    }
    Some(SectionViewStyle {
        show_arrows: flags & 0x02 != 0,
        show_plane_line: flags & 0x08 != 0,
        show_end_lines: flags & 0x20 != 0,
        arrow_size,
        arrow_extension,
        label_height: identifier_height,
        label_offset: if identifier_offset.is_finite() {
            identifier_offset
        } else {
            0.0
        },
        label_position: identifier_position,
        arrow_position,
        end_line_length: if end_line_length.is_finite() {
            end_line_length
        } else {
            0.0
        },
        end_line_overshoot: if end_line_overshoot.is_finite() {
            end_line_overshoot
        } else {
            0.0
        },
        arrow_start_handle: arrow_start,
        arrow_end_handle: arrow_end,
        arrow_is_default: arrow_start == 0 && arrow_end == 0,
    })
}

/// DWG stores the spatial-filter transforms row-major (unlike DXF code 40,
/// which is column-major).
fn matrix_from_row_major(v: &[f64; 12]) -> crate::types::Matrix4 {
    crate::types::Matrix4 {
        m: [
            [v[0], v[1], v[2], v[3]],
            [v[4], v[5], v[6], v[7]],
            [v[8], v[9], v[10], v[11]],
            [0.0, 0.0, 0.0, 1.0],
        ],
    }
}

fn map_dimension_common(
    base: &mut crate::entities::dimension::DimensionBase,
    common: &entities::DimensionCommonData,
    maps: &HandleMaps,
) {
    base.version = common.version_byte;
    base.normal = common.normal;
    base.text_middle_point = common.text_middle_point;
    base.dwg_flags_byte = common.flags_byte;
    // Flags byte bit 0: dimension text positioned at a user-defined location.
    base.text_user_positioned = (common.flags_byte & 0x01) != 0;
    base.text = common.text.clone();
    base.text_rotation = common.text_rotation;
    base.horizontal_direction = common.horizontal_direction;
    base.attachment_point = match common.attachment_point {
        1 => crate::entities::dimension::AttachmentPointType::TopLeft,
        2 => crate::entities::dimension::AttachmentPointType::TopCenter,
        3 => crate::entities::dimension::AttachmentPointType::TopRight,
        4 => crate::entities::dimension::AttachmentPointType::MiddleLeft,
        5 => crate::entities::dimension::AttachmentPointType::MiddleCenter,
        6 => crate::entities::dimension::AttachmentPointType::MiddleRight,
        7 => crate::entities::dimension::AttachmentPointType::BottomLeft,
        8 => crate::entities::dimension::AttachmentPointType::BottomCenter,
        9 => crate::entities::dimension::AttachmentPointType::BottomRight,
        _ => crate::entities::dimension::AttachmentPointType::MiddleCenter,
    };
    base.line_spacing_factor = common.linespacing_factor;
    base.line_spacing_style = common.linespacing_style;
    base.insertion_scale = common.ins_scale;
    base.insertion_rotation = common.ins_rotation;
    base.dwg_unknown_bit = common.unknown_bit;
    base.flip_arrow1 = common.flip_arrow1;
    base.flip_arrow2 = common.flip_arrow2;
    base.actual_measurement = common.actual_measurement;
    base.insertion_point =
        crate::types::Vector3::new(common.insertion_point.x, common.insertion_point.y, 0.0);
    base.style_name = maps.dimstyle_name(common.dimstyle_handle);
    base.block_name = maps.block_name(common.block_handle);
}

fn map_entity_common(
    data: &EntityCommonData,
    maps: &HandleMaps,
    model_space_handle: Handle,
    paper_space_handle: Handle,
) -> EntityCommon {
    let mut common = EntityCommon::new();
    common.handle = Handle::from(data.common.handle);
    // Resolve owner from entity_mode:
    //   0 = explicit owner (handle read from stream)
    //   1 = paper space (implicit)
    //   2 = model space (implicit)
    common.owner_handle = match data.entity_mode {
        1 => paper_space_handle,
        2 => model_space_handle,
        _ => Handle::from(data.owner_handle),
    };
    common.color = data.color;
    common.transparency = data.transparency;
    common.invisible = data.invisible;
    common.linetype_scale = data.linetype_scale;
    common.layer = maps.layer_name(data.layer_handle);
    // Line weight (raw DWG index byte → LineWeight)
    common.line_weight = crate::types::LineWeight::from_dwg_index(data.line_weight);
    // Reactors
    common.reactors = data.reactors.iter().map(|&h| Handle::from(h)).collect();
    // XDictionary handle
    common.xdictionary_handle = data.xdictionary_handle.map(Handle::from);
    common.color_book_handle = data.color_book_handle.map(Handle::from);
    common.full_visual_style_handle = data.full_visual_style_handle.map(Handle::from);
    common.face_visual_style_handle = data.face_visual_style_handle.map(Handle::from);
    common.edge_visual_style_handle = data.edge_visual_style_handle.map(Handle::from);
    // Linetype (from flags + optional handle)
    // EntityCommon uses empty string for "ByLayer" convention
    common.linetype = match data.linetype_flags {
        0b00 => String::new(), // ByLayer → empty (EntityCommon convention)
        0b01 => "ByBlock".to_string(),
        0b10 => "Continuous".to_string(),
        0b11 => maps
            .linetypes
            .get(&data.linetype_handle)
            .cloned()
            .unwrap_or_default(),
        _ => String::new(),
    };
    common.linetype_handle =
        (data.linetype_handle != 0).then(|| Handle::from(data.linetype_handle));
    // EED raw bytes for DWG round-trip
    common.extended_data.raw_dwg_eed = data.common.eed_raw.clone();
    // Graphic data for DWG round-trip
    common.graphic_data = data.graphic_data.clone();
    // DWG round-trip: preserve entity_mode, material/plotstyle/shadow flags
    common.entity_mode = Some(data.entity_mode);
    common.material_flags = data.material_flags;
    common.material_handle = data.material_handle.map(Handle::from);
    common.shadow_flags = data.shadow_flags;
    common.plotstyle_flags = data.plotstyle_flags;
    common.plotstyle_handle = data.plotstyle_handle.map(Handle::from);
    // R2013+: geometry-in-AcDs flag, needed to pair AcDs SAB blobs with the
    // right modeler entity in object-stream order.
    common.has_ds_data = data.has_ds_data;
    common
}

#[cfg(test)]
mod sdb_regression_tests {
    use super::viewport_override_base_layer;

    #[test]
    fn recognizes_only_viewport_override_shadow_layer_names() {
        assert_eq!(
            viewport_override_base_layer("A-ANNO-TXT @ 8"),
            Some("A-ANNO-TXT")
        );
        assert_eq!(
            viewport_override_base_layer("A-ANNO-NOTE @ 48"),
            Some("A-ANNO-NOTE")
        );
        assert_eq!(viewport_override_base_layer("user@domain"), None);
        assert_eq!(viewport_override_base_layer("Layer @ viewport"), None);
        assert_eq!(viewport_override_base_layer(" @ 8"), None);
        assert_eq!(viewport_override_base_layer("Layer @ "), None);
    }
}

#[cfg(test)]
mod hatch_context_regression_tests {
    use super::{read_hatch_scale_context_data, DwgMergedReader, DwgObjectReader};
    use crate::io::dwg::DwgVersion;
    use crate::types::DxfVersion;
    use std::collections::HashMap;

    struct DecodedView {
        owner: u64,
        reactor: u64,
        scale: u64,
        hatch: crate::objects::HatchScaleContext,
        view: u64,
        normal: crate::types::Vector3,
        rotation: f64,
        evaluate: bool,
        main_remaining: i64,
        handle_remaining: i64,
    }

    fn decode_view(hex: &str, handle_bits: i64) -> DecodedView {
        let bytes: Vec<u8> = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        let mut reader = DwgMergedReader::new(bytes.clone(), DxfVersion::AC1032, 0);
        let type_code = reader.read_object_type();
        reader.setup_text_and_handle(bytes.len() as i64 * 8 - handle_bits);
        let object_reader =
            DwgObjectReader::new(Vec::new(), DxfVersion::AC1032, HashMap::new()).unwrap();
        let common = object_reader.read_common_non_entity_data(&mut reader, type_code);
        assert_eq!(reader.read_bit_short(), 4);
        assert!(reader.read_bit());
        let scale = reader.read_handle();
        let hatch = read_hatch_scale_context_data(&mut reader, DwgVersion::AC24);
        let normal = crate::types::Vector3::new(
            reader.read_bit_double(),
            reader.read_bit_double(),
            reader.read_bit_double(),
        );
        let rotation = reader.read_bit_double();
        let evaluate = reader.read_bit();
        let view = reader.read_handle();
        DecodedView {
            owner: common.owner_handle,
            reactor: common.reactors[0],
            scale,
            hatch,
            view,
            normal,
            rotation,
            evaluate,
            main_remaining: reader.main_remaining_bits(),
            handle_remaining: reader.handle_remaining_bits(),
        }
    }

    #[test]
    fn hatch_view_records_consume_each_loop_flag_before_the_view_tail() {
        let dense = decode_view(
            "5500C3010CA40641281D000000000000045811F753E3A49AC5707F0305A88A9F643F27F000000000000045807DD4F8E926B1645F81D09BD9D51CAD478811F753E3A49AC5707E7DD4F8E926B15C5F8305A88A9F643F27E7DD4F8E926B15C1F800000000000045807DD4F8E926B1645F81D09BD9D51CAD478811F753E3A49AC5717E7DD4F8E926B15C1FB520E83D07A0F41E83D078300C0001A62010C3010C54C1F849503F",
            86,
        );
        assert_eq!(dense.owner, 0xC0431);
        assert_eq!(dense.reactor, 0xC0431);
        assert_eq!(dense.scale, 0x7E125);
        assert_eq!(dense.hatch.loops.len(), 7);
        assert_eq!(
            dense
                .hatch
                .loops
                .iter()
                .map(|entry| entry.loop_type)
                .collect::<Vec<_>>(),
            [7, 7, 7, 7, 7, 7, 1560]
        );
        assert!(dense.hatch.loops.iter().all(|entry| entry.supports_context));
        assert!(dense
            .hatch
            .loops
            .iter()
            .all(|entry| entry.boundary.is_none()));
        assert_eq!(dense.view, 0);
        assert_eq!(dense.normal, crate::types::Vector3::UNIT_Z);
        assert_eq!(dense.rotation, 0.0);
        assert!(!dense.evaluate);
        assert_eq!(dense.main_remaining, 0);
        assert!(dense.handle_remaining <= 7);

        let elevated = decode_view(
            "5500C1BCD32406412810305A88A9F643F27E3A82004F7A7038A005CEC992033D8908029DC0F42F5A8F25FD0305A88A9F64212803A82004F7A7038A005CEC992033D89080000000000000505E0A5703D0BD6A3C97F3520480D11D154329133B33074811820A860DE68EA605198A8605BCDB",
            113,
        );
        assert_eq!(elevated.owner, 0x6F347);
        assert_eq!(elevated.reactor, 0x6F347);
        assert_eq!(elevated.scale, 0x28CC5);
        assert_eq!(
            elevated
                .hatch
                .loops
                .iter()
                .map(|entry| entry.loop_type)
                .collect::<Vec<_>>(),
            [1, 17]
        );
        assert!(elevated
            .hatch
            .loops
            .iter()
            .all(|entry| entry.supports_context));
        assert!(elevated
            .hatch
            .loops
            .iter()
            .all(|entry| entry.boundary.is_none()));
        assert_eq!(elevated.view, 0x2DE6D);
        assert_eq!(elevated.normal.x, 0.0);
        assert_eq!(elevated.normal.y, 0.0);
        assert!((elevated.normal.z - 26.59707029352436).abs() < 1e-12);
        assert_eq!(elevated.rotation, 0.0);
        assert!(!elevated.evaluate);
        assert_eq!(elevated.main_remaining, 0);
        assert!(elevated.handle_remaining <= 7);
    }
}
