//! Graphical entity types.
//!
//! This module contains all 41 supported CAD entity types — from simple
//! primitives ([`Line`], [`Circle`], [`Arc`]) through complex objects
//! ([`Hatch`], [`Spline`], [`MultiLeader`], [`Mesh`]).
//!
//! Every entity carries [`EntityCommon`] data (layer, color, line weight,
//! handle, etc.) alongside its type-specific fields.
//!
//! Entities are stored in [`CadDocument`](crate::document::CadDocument) and
//! wrapped in the [`EntityType`] enum for heterogeneous collections.

use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transform, Transparency, Vector3};

pub mod acis;
pub mod arc;
pub mod attribute_definition;
pub mod attribute_entity;
pub mod block;
pub mod centerline;
pub mod circle;
pub mod dimension;
pub mod ellipse;
pub mod embedded_entity;
pub mod explode;
pub mod extended_entity;
pub mod face3d;
pub mod hatch;
pub mod helix;
pub mod insert;
pub mod leader;
pub mod light;
pub mod line;
pub mod lwpolyline;
pub mod mesh;
pub mod mirror;
pub mod mline;
pub mod mtext;
pub mod mtext_format;
pub mod multileader;
pub mod ole2frame;
pub mod ole_presentation;
pub mod point;
pub mod polyface_mesh;
pub mod polygon_mesh;
pub mod polyline;
pub mod polyline3d;
pub mod proxy_graphics;
pub mod raster_image;
pub mod ray;
pub mod section_symbol;
pub mod seqend;
pub mod shape;
pub mod solid;
pub mod solid3d;
pub mod spline;
pub mod surface;
pub mod table;
pub mod text;
pub mod tolerance;
pub mod transform;
pub mod translate;
pub mod underlay;
pub mod unknown_entity;
pub mod view_border;
pub mod viewport;
pub mod wipeout;
pub mod xline;

pub use arc::Arc;
pub use attribute_definition::{
    AttributeDefinition, AttributeFlags, HorizontalAlignment, MTextFlag, VerticalAlignment,
};
pub use attribute_entity::AttributeEntity;
pub use block::{Block, BlockEnd};
pub use centerline::{
    CenterLineAssociation, CenterLineSource, CenterLineSourceKind, CenterMarkAssociation,
    CenterMarkSource, CenterMarkSourceKind, CENTERLINE_XDATA_APPLICATION,
    CENTERMARK_XDATA_APPLICATION,
};
pub use circle::Circle;
pub use dimension::*;
pub use ellipse::Ellipse;
pub use embedded_entity::EmbeddedEntity;
pub use extended_entity::*;
pub use face3d::{Face3D, InvisibleEdgeFlags};
pub use hatch::*;
pub use helix::{Helix, HelixConstraint};
pub use insert::Insert;
pub use leader::{HooklineDirection, Leader, LeaderCreationType, LeaderPathType};
pub use light::{Light, LightPhotometricData};
pub use line::Line;
pub use lwpolyline::{LwPolyline, LwVertex};
pub use mesh::{Mesh, MeshBuilder, MeshEdge, MeshFace};
pub use mline::{
    MLine, MLineBuilder, MLineFlags, MLineJustification, MLineSegment, MLineStyle,
    MLineStyleElement, MLineStyleFlags, MLineVertex,
};
pub use mtext::{AttachmentPoint, DrawingDirection, MText, MTextColumnData};
pub use multileader::{
    BlockAttribute, BlockContentConnectionType, FlowDirectionType, LeaderContentType, LeaderLine,
    LeaderLinePropertyOverrideFlags, LeaderRoot, LineSpacingStyle, MultiLeader,
    MultiLeaderAnnotContext, MultiLeaderArrowheadOverride, MultiLeaderBuilder, MultiLeaderPathType,
    MultiLeaderPropertyOverrideFlags, StartEndPointPair, TextAlignmentType, TextAngleType,
    TextAttachmentDirectionType, TextAttachmentPointType, TextAttachmentType,
};
pub use ole2frame::{Ole2Frame, OleFrameEnvelope, OleObjectType};
pub use ole_presentation::{extract_presentation, OlePresentation};
pub use point::Point;
pub use polyface_mesh::{
    PolyfaceFace, PolyfaceMesh, PolyfaceMeshFlags, PolyfaceSmoothType, PolyfaceVertex,
    PolyfaceVertexFlags,
};
pub use polygon_mesh::{
    PolygonMesh as PolygonMeshEntity, PolygonMeshFlags, PolygonMeshVertex, SurfaceSmoothType,
};
pub use polyline::{
    Polyline, Polyline2D, PolylineFlags, SmoothSurfaceType, Vertex2D, Vertex3D, VertexFlags,
};
pub use polyline3d::{Polyline3D, Polyline3DFlags, Vertex3DPolyline};
pub use proxy_graphics::{ProxyGraphicRecord, ProxyGraphics, ProxyUnicodeText};
pub use raster_image::{
    ClipBoundary, ClipMode, ClipType, ImageDefinition, ImageDisplayFlags, ImageDisplayQuality,
    RasterImage, RasterImageBuilder, ResolutionUnit,
};
pub use ray::Ray;
pub use section_symbol::{SectionSymbol, SectionSymbolPoint, SectionViewStyle};
pub use seqend::Seqend;
pub use shape::{gdt_shapes, standard_shapes, Shape};
pub use solid::Solid;
pub use solid3d::{
    AcisData, AcisMaterial, AcisVersion, Body, Region, Silhouette, Solid3D, Wire, WireType,
};
pub use spline::{Spline, SplineFlags};
pub use surface::{Surface, SurfaceData, SurfaceKind, SurfaceSweepOptions};
pub use table::{
    BorderPropertyFlags, BorderType, BreakFlowDirection, BreakOptionFlags, CellBorder, CellContent,
    CellContentGeometry, CellEdgeFlags, CellRange, CellStateFlags, CellStyle,
    CellStylePropertyFlags, CellStyleType, CellType, CellValue, CellValueType, ContentLayoutFlags,
    LegacyBorderOverrides, LegacyTableStyleOverride, Table, TableAttribute, TableBreakData,
    TableBreakRange, TableBuilder, TableCell, TableCellContentType, TableColumn, TableCustomData,
    TableRow, ValueUnitType,
};
pub use text::{Text, TextHorizontalAlignment, TextVerticalAlignment};
pub use tolerance::{gdt_symbols, Tolerance};
pub use underlay::{
    DgnUnderlay, DgnUnderlayDefinition, DwfUnderlay, DwfUnderlayDefinition, PdfUnderlay,
    PdfUnderlayDefinition, Underlay, UnderlayDefinition, UnderlayDisplayFlags, UnderlayType,
};
pub use unknown_entity::UnknownEntity;
pub use view_border::ViewBorder;
pub use viewport::{GridFlags, StandardView, Viewport, ViewportRenderMode, ViewportStatusFlags};
pub use wipeout::{Wipeout, WipeoutClipMode, WipeoutClipType, WipeoutDisplayFlags};
pub use xline::XLine;

/// Base trait for all CAD entities
pub trait Entity {
    /// Get the entity's unique handle
    fn handle(&self) -> Handle;

    /// Set the entity's handle
    fn set_handle(&mut self, handle: Handle);

    /// Get the entity's layer name
    fn layer(&self) -> &str;

    /// Set the entity's layer name
    fn set_layer(&mut self, layer: String);

    /// Get the entity's color
    fn color(&self) -> Color;

    /// Set the entity's color
    fn set_color(&mut self, color: Color);

    /// Get the entity's line weight
    fn line_weight(&self) -> LineWeight;

    /// Set the entity's line weight
    fn set_line_weight(&mut self, weight: LineWeight);

    /// Get the entity's transparency
    fn transparency(&self) -> Transparency;

    /// Set the entity's transparency
    fn set_transparency(&mut self, transparency: Transparency);

    /// Check if the entity is invisible
    fn is_invisible(&self) -> bool;

    /// Set the entity's visibility
    fn set_invisible(&mut self, invisible: bool);

    /// Get the bounding box of the entity
    fn bounding_box(&self) -> BoundingBox3D;

    /// Transform the entity by a translation vector
    fn translate(&mut self, offset: Vector3);

    /// Get the entity type name
    fn entity_type(&self) -> &'static str;

    /// Apply a general transform to the entity
    ///
    /// This is the main transformation method. Default implementation
    /// only supports translation for backward compatibility.
    fn apply_transform(&mut self, transform: &Transform) {
        // Default: extract translation and apply
        let origin = Vector3::ZERO;
        let translated = transform.apply(origin);
        self.translate(translated);
    }

    /// Apply rotation around an axis
    fn apply_rotation(&mut self, axis: Vector3, angle: f64) {
        self.apply_transform(&Transform::from_rotation(axis, angle));
    }

    /// Apply uniform scaling
    fn apply_scaling(&mut self, scale: f64) {
        self.apply_transform(&Transform::from_scale(scale));
    }

    /// Apply non-uniform scaling
    fn apply_scaling_xyz(&mut self, scale: Vector3) {
        self.apply_transform(&Transform::from_scaling(scale));
    }

    /// Apply scaling with a specific origin point
    fn apply_scaling_with_origin(&mut self, scale: Vector3, origin: Vector3) {
        self.apply_transform(&Transform::from_scaling_with_origin(scale, origin));
    }

    /// Apply a mirror transform with entity-specific corrections
    ///
    /// Override this for entities that need post-processing after mirroring
    /// (e.g., arc angle swaps, bulge negation, face winding reversal).
    fn apply_mirror(&mut self, transform: &Transform) {
        self.apply_transform(transform);
    }

    /// Mirror the entity across the YZ plane (negate X coordinates)
    fn mirror_x(&mut self) {
        self.apply_mirror(&Transform::from_mirror_x());
    }

    /// Mirror the entity across the XZ plane (negate Y coordinates)
    fn mirror_y(&mut self) {
        self.apply_mirror(&Transform::from_mirror_y());
    }

    /// Mirror the entity across the XY plane (negate Z coordinates)
    fn mirror_z(&mut self) {
        self.apply_mirror(&Transform::from_mirror_z());
    }

    /// Mirror the entity across a line defined by two points (in the XY plane)
    fn mirror_about_line(&mut self, p1: Vector3, p2: Vector3) {
        self.apply_mirror(&Transform::from_mirror_line(p1, p2));
    }

    /// Mirror the entity across an arbitrary plane
    fn mirror_about_plane(&mut self, point: Vector3, normal: Vector3) {
        self.apply_mirror(&Transform::from_mirror_plane(point, normal));
    }
}

/// Common entity data shared by all entities
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EntityCommon {
    /// Unique handle
    pub handle: Handle,
    /// Layer name
    pub layer: String,
    /// Color
    pub color: Color,
    /// Line weight
    pub line_weight: LineWeight,
    /// Linetype name (empty string = "ByLayer")
    pub linetype: String,
    /// DWG linetype handle. Preserved so R13/R14 entities can round-trip
    /// non-ByLayer linetypes even when the table name cannot be resolved.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub linetype_handle: Option<Handle>,
    /// Linetype scale factor (default 1.0)
    pub linetype_scale: f64,
    /// Transparency
    #[cfg_attr(feature = "serde", serde(default))]
    pub transparency: Transparency,
    /// Named/color-book identity from DXF group code 430.
    #[cfg_attr(feature = "serde", serde(default))]
    pub color_name: Option<String>,
    /// Visibility flag
    pub invisible: bool,
    /// Extended data (XDATA)
    pub extended_data: crate::xdata::ExtendedData,
    /// Raw entity graphic data bytes (stored for DWG round-trip; None otherwise).
    #[cfg_attr(feature = "serde", serde(skip))]
    pub graphic_data: Option<Vec<u8>>,
    /// Reactor handles — objects attached as reactors ({ACAD_REACTORS})
    pub reactors: Vec<Handle>,
    /// Extended dictionary handle ({ACAD_XDICTIONARY}) — hard-owner handle to a Dictionary
    pub xdictionary_handle: Option<Handle>,
    /// Owner handle (soft pointer, code 330)
    pub owner_handle: Handle,

    // ── Native reference/round-trip fields ──
    /// AcDbColor object handle for a color-book color — R2004+.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub color_book_handle: Option<Handle>,
    /// Full visual-style override handle — R2010+; DXF code 348.
    pub full_visual_style_handle: Option<Handle>,
    /// Face visual-style override handle — R2010+.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub face_visual_style_handle: Option<Handle>,
    /// Edge visual-style override handle — R2010+.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub edge_visual_style_handle: Option<Handle>,
    /// Material flags (BB: 00=bylayer, 01=byblock, 10=reserved, 11=handle) — R2007+
    #[cfg_attr(feature = "serde", serde(skip))]
    pub material_flags: u8,
    /// Material handle (only valid when material_flags == 0b11) — R2007+
    #[cfg_attr(feature = "serde", serde(skip))]
    pub material_handle: Option<Handle>,
    /// Shadow flags (RC) — R2007+
    #[cfg_attr(feature = "serde", serde(skip))]
    pub shadow_flags: u8,
    /// Plotstyle flags (BB: 00=bylayer, 01=byblock, 10=reserved, 11=handle) — R2000+
    #[cfg_attr(feature = "serde", serde(skip))]
    pub plotstyle_flags: u8,
    /// Plotstyle handle (only valid when plotstyle_flags == 0b11) — R2000+
    #[cfg_attr(feature = "serde", serde(skip))]
    pub plotstyle_handle: Option<Handle>,
    /// Entity mode (0=owned, 1=paper, 2=model) — raw DWG value for round-trip
    #[cfg_attr(feature = "serde", serde(skip))]
    pub entity_mode: Option<u8>,
    /// R2013+ `has_ds_data` bit: the entity's modeler geometry (3DSOLID/REGION/
    /// BODY/SURFACE) is stored as a SAB blob in the `AcDb:AcDsPrototype_1b`
    /// data-store section rather than inline. Set by the DWG reader; the writer
    /// re-derives it from the presence of ACIS data. Not part of the logical
    /// model, so skipped for serde.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub has_ds_data: bool,
}

impl EntityCommon {
    fn preserve_storage_data_from(&mut self, source: &Self) {
        self.linetype_handle = source.linetype_handle;
        self.graphic_data = source.graphic_data.clone();
        self.color_book_handle = source.color_book_handle;
        self.face_visual_style_handle = source.face_visual_style_handle;
        self.edge_visual_style_handle = source.edge_visual_style_handle;
        self.material_flags = source.material_flags;
        self.material_handle = source.material_handle;
        self.shadow_flags = source.shadow_flags;
        self.plotstyle_flags = source.plotstyle_flags;
        self.plotstyle_handle = source.plotstyle_handle;
        self.entity_mode = source.entity_mode;
        self.has_ds_data = source.has_ds_data;
    }

    /// Create new common entity data with defaults
    pub fn new() -> Self {
        EntityCommon {
            handle: Handle::NULL,
            layer: "0".to_string(),
            color: Color::ByLayer,
            line_weight: LineWeight::ByLayer,
            linetype: String::new(),
            linetype_handle: None,
            linetype_scale: 1.0,
            transparency: Transparency::BY_LAYER,
            color_name: None,
            invisible: false,
            extended_data: crate::xdata::ExtendedData::new(),
            graphic_data: None,
            reactors: Vec::new(),
            xdictionary_handle: None,
            owner_handle: Handle::NULL,
            color_book_handle: None,
            full_visual_style_handle: None,
            face_visual_style_handle: None,
            edge_visual_style_handle: None,
            material_flags: 0,
            material_handle: None,
            shadow_flags: 0,
            plotstyle_flags: 0,
            plotstyle_handle: None,
            entity_mode: None,
            has_ds_data: false,
        }
    }

    /// Create with a specific layer
    pub fn with_layer(layer: impl Into<String>) -> Self {
        EntityCommon {
            layer: layer.into(),
            ..Self::new()
        }
    }

    /// Check whether a linetype name is set (not empty and not "ByLayer")
    pub fn has_linetype(&self) -> bool {
        !self.linetype.is_empty() && self.linetype != "ByLayer"
    }
}

impl Default for EntityCommon {
    fn default() -> Self {
        Self::new()
    }
}

/// Enumeration of all entity types for type-safe storage
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EntityType {
    /// Point entity
    Point(Point),
    /// Line entity
    Line(Line),
    /// Circle entity
    Circle(Circle),
    /// Arc entity
    Arc(Arc),
    /// Ellipse entity
    Ellipse(Ellipse),
    /// 3D Polyline entity
    Polyline(Polyline),
    /// 2D Polyline entity (heavy polyline)
    Polyline2D(Polyline2D),
    /// 3D Polyline entity (new style)
    Polyline3D(Polyline3D),
    /// Lightweight polyline entity
    LwPolyline(LwPolyline),
    /// Text entity
    Text(Text),
    /// Multi-line text entity
    MText(MText),
    /// Spline entity
    Spline(Spline),
    /// Helix entity (spline-derived 3D spiral)
    Helix(Helix),
    /// Dimension entity
    Dimension(Dimension),
    /// Hatch entity
    Hatch(Hatch),
    /// Solid entity
    Solid(Solid),
    /// 3D Face entity
    Face3D(Face3D),
    /// Insert entity (block reference)
    Insert(Insert),
    /// Block entity (block definition start)
    Block(Block),
    /// BlockEnd entity (block definition end)
    BlockEnd(BlockEnd),
    /// Ray entity (semi-infinite line)
    Ray(Ray),
    /// XLine entity (construction line, infinite)
    XLine(XLine),
    /// Viewport entity (paper space viewport)
    Viewport(Viewport),
    /// Attribute definition entity
    AttributeDefinition(AttributeDefinition),
    /// Attribute entity (block attribute instance)
    AttributeEntity(AttributeEntity),
    /// Leader entity
    Leader(Leader),
    /// MultiLeader entity
    MultiLeader(MultiLeader),
    /// MLine (multiline) entity
    MLine(MLine),
    /// Mesh entity
    Mesh(Mesh),
    /// RasterImage entity
    RasterImage(RasterImage),
    /// Solid3D entity
    Solid3D(Solid3D),
    /// Region entity
    Region(Region),
    /// Body entity
    Body(Body),
    /// Surface entity (ACAD_SURFACE family: lofted/swept/extruded/etc.)
    Surface(Surface),
    /// Table entity
    Table(Table),
    /// Tolerance entity (geometric tolerancing)
    Tolerance(Tolerance),
    /// PolyfaceMesh entity
    PolyfaceMesh(PolyfaceMesh),
    /// Wipeout entity
    Wipeout(Wipeout),
    /// Shape entity
    Shape(Shape),
    /// Underlay entity (PDF, DWF, DGN)
    Underlay(Underlay),
    /// End-of-sequence marker
    Seqend(Seqend),
    /// OLE2 embedded object
    Ole2Frame(Ole2Frame),
    /// Polygon mesh (3D surface mesh)
    PolygonMesh(PolygonMeshEntity),
    /// Light entity (point / spot / distant light source)
    Light(Light),
    SectionSymbol(SectionSymbol),
    ViewBorder(ViewBorder),
    /// Structured class-based and legacy entities.
    Extended(ExtendedEntity),
    /// Unknown / unsupported entity type (common fields only)
    Unknown(UnknownEntity),
}

impl EntityType {
    /// Restore storage-only data omitted from serde after editing the public
    /// representation of an entity. The source and destination must have the
    /// same variant and identity; callers remain responsible for that check.
    pub fn preserve_storage_data_from(&mut self, source: &Self) {
        self.common_mut()
            .preserve_storage_data_from(source.common());
        match (self, source) {
            (Self::Extended(value), Self::Extended(source)) => {
                value.data.preserve_storage_data_from(&source.data);
            }
            (Self::Unknown(value), Self::Unknown(source)) => {
                value.raw_dwg_data = source.raw_dwg_data.clone();
                value.raw_dxf_codes = source.raw_dxf_codes.clone();
                value.dwg_source_version = source.dwg_source_version;
            }
            _ => {}
        }
    }

    /// Get a reference to the entity trait object
    pub fn as_entity(&self) -> &dyn Entity {
        match self {
            EntityType::Point(e) => e,
            EntityType::Line(e) => e,
            EntityType::Circle(e) => e,
            EntityType::Arc(e) => e,
            EntityType::Ellipse(e) => e,
            EntityType::Polyline(e) => e,
            EntityType::Polyline2D(e) => e,
            EntityType::Polyline3D(e) => e,
            EntityType::LwPolyline(e) => e,
            EntityType::Text(e) => e,
            EntityType::MText(e) => e,
            EntityType::Spline(e) => e,
            EntityType::Helix(e) => e,
            EntityType::Dimension(e) => e,
            EntityType::Hatch(e) => e,
            EntityType::Solid(e) => e,
            EntityType::Face3D(e) => e,
            EntityType::Insert(e) => e,
            EntityType::Block(e) => e,
            EntityType::BlockEnd(e) => e,
            EntityType::Ray(e) => e,
            EntityType::XLine(e) => e,
            EntityType::Viewport(e) => e,
            EntityType::AttributeDefinition(e) => e,
            EntityType::AttributeEntity(e) => e,
            EntityType::Leader(e) => e,
            EntityType::MultiLeader(e) => e,
            EntityType::MLine(e) => e,
            EntityType::Mesh(e) => e,
            EntityType::RasterImage(e) => e,
            EntityType::Solid3D(e) => e,
            EntityType::Region(e) => e,
            EntityType::Body(e) => e,
            EntityType::Surface(e) => e,
            EntityType::Table(e) => e,
            EntityType::Tolerance(e) => e,
            EntityType::PolyfaceMesh(e) => e,
            EntityType::Wipeout(e) => e,
            EntityType::Shape(e) => e,
            EntityType::Underlay(e) => e,
            EntityType::Seqend(e) => e,
            EntityType::Ole2Frame(e) => e,
            EntityType::PolygonMesh(e) => e,
            EntityType::Light(e) => e,
            EntityType::SectionSymbol(e) => e,
            EntityType::ViewBorder(e) => e,
            EntityType::Extended(e) => e,
            EntityType::Unknown(e) => e,
        }
    }

    /// Get a mutable reference to the entity trait object
    pub fn as_entity_mut(&mut self) -> &mut dyn Entity {
        match self {
            EntityType::Point(e) => e,
            EntityType::Line(e) => e,
            EntityType::Circle(e) => e,
            EntityType::Arc(e) => e,
            EntityType::Ellipse(e) => e,
            EntityType::Polyline(e) => e,
            EntityType::Polyline2D(e) => e,
            EntityType::Polyline3D(e) => e,
            EntityType::LwPolyline(e) => e,
            EntityType::MText(e) => e,
            EntityType::Text(e) => e,
            EntityType::Spline(e) => e,
            EntityType::Helix(e) => e,
            EntityType::Dimension(e) => e,
            EntityType::Hatch(e) => e,
            EntityType::Solid(e) => e,
            EntityType::Face3D(e) => e,
            EntityType::Insert(e) => e,
            EntityType::Block(e) => e,
            EntityType::BlockEnd(e) => e,
            EntityType::Ray(e) => e,
            EntityType::XLine(e) => e,
            EntityType::Viewport(e) => e,
            EntityType::AttributeDefinition(e) => e,
            EntityType::AttributeEntity(e) => e,
            EntityType::Leader(e) => e,
            EntityType::MultiLeader(e) => e,
            EntityType::MLine(e) => e,
            EntityType::Mesh(e) => e,
            EntityType::RasterImage(e) => e,
            EntityType::Solid3D(e) => e,
            EntityType::Region(e) => e,
            EntityType::Body(e) => e,
            EntityType::Surface(e) => e,
            EntityType::Table(e) => e,
            EntityType::Tolerance(e) => e,
            EntityType::PolyfaceMesh(e) => e,
            EntityType::Wipeout(e) => e,
            EntityType::Shape(e) => e,
            EntityType::Underlay(e) => e,
            EntityType::Seqend(e) => e,
            EntityType::Ole2Frame(e) => e,
            EntityType::PolygonMesh(e) => e,
            EntityType::Light(e) => e,
            EntityType::SectionSymbol(e) => e,
            EntityType::ViewBorder(e) => e,
            EntityType::Extended(e) => e,
            EntityType::Unknown(e) => e,
        }
    }

    /// Get a reference to the entity's common data
    pub fn common(&self) -> &EntityCommon {
        match self {
            EntityType::Point(e) => &e.common,
            EntityType::Line(e) => &e.common,
            EntityType::Circle(e) => &e.common,
            EntityType::Arc(e) => &e.common,
            EntityType::Ellipse(e) => &e.common,
            EntityType::Polyline(e) => &e.common,
            EntityType::Polyline2D(e) => &e.common,
            EntityType::Polyline3D(e) => &e.common,
            EntityType::LwPolyline(e) => &e.common,
            EntityType::Text(e) => &e.common,
            EntityType::MText(e) => &e.common,
            EntityType::Spline(e) => &e.common,
            EntityType::Helix(e) => &e.common,
            EntityType::Dimension(e) => &e.base().common,
            EntityType::Hatch(e) => &e.common,
            EntityType::Solid(e) => &e.common,
            EntityType::Face3D(e) => &e.common,
            EntityType::Insert(e) => &e.common,
            EntityType::Block(e) => &e.common,
            EntityType::BlockEnd(e) => &e.common,
            EntityType::Ray(e) => &e.common,
            EntityType::XLine(e) => &e.common,
            EntityType::Viewport(e) => &e.common,
            EntityType::AttributeDefinition(e) => &e.common,
            EntityType::AttributeEntity(e) => &e.common,
            EntityType::Leader(e) => &e.common,
            EntityType::MultiLeader(e) => &e.common,
            EntityType::MLine(e) => &e.common,
            EntityType::Mesh(e) => &e.common,
            EntityType::RasterImage(e) => &e.common,
            EntityType::Solid3D(e) => &e.common,
            EntityType::Region(e) => &e.common,
            EntityType::Body(e) => &e.common,
            EntityType::Surface(e) => &e.common,
            EntityType::Table(e) => &e.common,
            EntityType::Tolerance(e) => &e.common,
            EntityType::PolyfaceMesh(e) => &e.common,
            EntityType::Wipeout(e) => &e.common,
            EntityType::Shape(e) => &e.common,
            EntityType::Underlay(e) => &e.common,
            EntityType::Seqend(e) => &e.common,
            EntityType::Ole2Frame(e) => &e.common,
            EntityType::PolygonMesh(e) => &e.common,
            EntityType::Light(e) => &e.common,
            EntityType::SectionSymbol(e) => &e.common,
            EntityType::ViewBorder(e) => &e.common,
            EntityType::Extended(e) => &e.common,
            EntityType::Unknown(e) => &e.common,
        }
    }

    /// Get a mutable reference to the entity's common data
    pub fn common_mut(&mut self) -> &mut EntityCommon {
        match self {
            EntityType::Point(e) => &mut e.common,
            EntityType::Line(e) => &mut e.common,
            EntityType::Circle(e) => &mut e.common,
            EntityType::Arc(e) => &mut e.common,
            EntityType::Ellipse(e) => &mut e.common,
            EntityType::Polyline(e) => &mut e.common,
            EntityType::Polyline2D(e) => &mut e.common,
            EntityType::Polyline3D(e) => &mut e.common,
            EntityType::LwPolyline(e) => &mut e.common,
            EntityType::Text(e) => &mut e.common,
            EntityType::MText(e) => &mut e.common,
            EntityType::Spline(e) => &mut e.common,
            EntityType::Helix(e) => &mut e.common,
            EntityType::Dimension(e) => &mut e.base_mut().common,
            EntityType::Hatch(e) => &mut e.common,
            EntityType::Solid(e) => &mut e.common,
            EntityType::Face3D(e) => &mut e.common,
            EntityType::Insert(e) => &mut e.common,
            EntityType::Block(e) => &mut e.common,
            EntityType::BlockEnd(e) => &mut e.common,
            EntityType::Ray(e) => &mut e.common,
            EntityType::XLine(e) => &mut e.common,
            EntityType::Viewport(e) => &mut e.common,
            EntityType::AttributeDefinition(e) => &mut e.common,
            EntityType::AttributeEntity(e) => &mut e.common,
            EntityType::Leader(e) => &mut e.common,
            EntityType::MultiLeader(e) => &mut e.common,
            EntityType::MLine(e) => &mut e.common,
            EntityType::Mesh(e) => &mut e.common,
            EntityType::RasterImage(e) => &mut e.common,
            EntityType::Solid3D(e) => &mut e.common,
            EntityType::Region(e) => &mut e.common,
            EntityType::Body(e) => &mut e.common,
            EntityType::Surface(e) => &mut e.common,
            EntityType::Table(e) => &mut e.common,
            EntityType::Tolerance(e) => &mut e.common,
            EntityType::PolyfaceMesh(e) => &mut e.common,
            EntityType::Wipeout(e) => &mut e.common,
            EntityType::Shape(e) => &mut e.common,
            EntityType::Underlay(e) => &mut e.common,
            EntityType::Seqend(e) => &mut e.common,
            EntityType::Ole2Frame(e) => &mut e.common,
            EntityType::PolygonMesh(e) => &mut e.common,
            EntityType::Light(e) => &mut e.common,
            EntityType::SectionSymbol(e) => &mut e.common,
            EntityType::ViewBorder(e) => &mut e.common,
            EntityType::Extended(e) => &mut e.common,
            EntityType::Unknown(e) => &mut e.common,
        }
    }
}

#[cfg(test)]
mod storage_data_tests {
    use super::*;

    #[test]
    fn public_record_edit_can_preserve_opaque_entity_data() {
        let mut source = UnknownEntity::new("CUSTOM_ENTITY");
        source.common.graphic_data = Some(vec![1, 2, 3]);
        source.raw_dwg_data = Some(vec![4, 5, 6]);
        source.raw_dxf_codes = Some(vec![(100, "payload".into())]);
        let source = EntityType::Unknown(source);

        let mut edited = source.clone();
        if let EntityType::Unknown(value) = &mut edited {
            value.common.layer = "Edited".into();
            value.common.graphic_data = None;
            value.raw_dwg_data = None;
            value.raw_dxf_codes = None;
        }
        edited.preserve_storage_data_from(&source);

        let EntityType::Unknown(edited) = edited else {
            unreachable!()
        };
        assert_eq!(edited.common.layer, "Edited");
        assert_eq!(edited.common.graphic_data, Some(vec![1, 2, 3]));
        assert_eq!(edited.raw_dwg_data, Some(vec![4, 5, 6]));
        assert_eq!(edited.raw_dxf_codes, Some(vec![(100, "payload".into())]));
    }
}
