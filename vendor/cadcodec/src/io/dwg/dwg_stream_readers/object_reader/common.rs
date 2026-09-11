//! Object type constants shared between reader and writer.
//!
//! These match the fixed type codes (ODA spec table 2.12).
//! Fixed types 0–82 use literal codes; non-fixed types use
//! their DXF class number (500+).

// ── Fixed entity types (graphical) ──────────────────────────────────

pub const OBJ_UNUSED: i16 = 0;
pub const OBJ_TEXT: i16 = 1;
pub const OBJ_ATTRIB: i16 = 2;
pub const OBJ_ATTDEF: i16 = 3;
pub const OBJ_BLOCK: i16 = 4;
pub const OBJ_ENDBLK: i16 = 5;
pub const OBJ_SEQEND: i16 = 6;
pub const OBJ_INSERT: i16 = 7;
pub const OBJ_MINSERT: i16 = 8;
// 9 = skipped
pub const OBJ_VERTEX_2D: i16 = 10;
pub const OBJ_VERTEX_3D: i16 = 11;
pub const OBJ_VERTEX_MESH: i16 = 12;
pub const OBJ_VERTEX_PFACE: i16 = 13;
pub const OBJ_VERTEX_PFACE_FACE: i16 = 14;
pub const OBJ_POLYLINE_2D: i16 = 15;
pub const OBJ_POLYLINE_3D: i16 = 16;
pub const OBJ_ARC: i16 = 17;
pub const OBJ_CIRCLE: i16 = 18;
pub const OBJ_LINE: i16 = 19;
pub const OBJ_DIMENSION_ORDINATE: i16 = 20;
pub const OBJ_DIMENSION_LINEAR: i16 = 21;
pub const OBJ_DIMENSION_ALIGNED: i16 = 22;
pub const OBJ_DIMENSION_ANG_3PT: i16 = 23;
pub const OBJ_DIMENSION_ANG_2LN: i16 = 24;
pub const OBJ_DIMENSION_RADIUS: i16 = 25;
pub const OBJ_DIMENSION_DIAMETER: i16 = 26;
pub const OBJ_POINT: i16 = 27;
pub const OBJ_3DFACE: i16 = 28;
pub const OBJ_POLYLINE_PFACE: i16 = 29;
pub const OBJ_POLYLINE_MESH: i16 = 30;
pub const OBJ_SOLID: i16 = 31;
pub const OBJ_TRACE: i16 = 32;
pub const OBJ_SHAPE: i16 = 33;
pub const OBJ_VIEWPORT: i16 = 34;
pub const OBJ_ELLIPSE: i16 = 35;
pub const OBJ_SPLINE: i16 = 36;
pub const OBJ_REGION: i16 = 37;
pub const OBJ_3DSOLID: i16 = 38;
pub const OBJ_BODY: i16 = 39;
pub const OBJ_RAY: i16 = 40;
pub const OBJ_XLINE: i16 = 41;
pub const OBJ_DICTIONARY: i16 = 42;
pub const OBJ_OLEFRAME: i16 = 43;
pub const OBJ_MTEXT: i16 = 44;
pub const OBJ_LEADER: i16 = 45;
pub const OBJ_TOLERANCE: i16 = 46;
pub const OBJ_MLINE: i16 = 47;

// ── Table control / table entry types ───────────────────────────────

pub const OBJ_BLOCK_CONTROL: i16 = 48;
pub const OBJ_BLOCK_HEADER: i16 = 49;
pub const OBJ_LAYER_CONTROL: i16 = 50;
pub const OBJ_LAYER: i16 = 51;
pub const OBJ_STYLE_CONTROL: i16 = 52;
pub const OBJ_STYLE: i16 = 53;
// 54-55 skipped
pub const OBJ_LTYPE_CONTROL: i16 = 56;
pub const OBJ_LTYPE: i16 = 57;
// 58-59 skipped
pub const OBJ_VIEW_CONTROL: i16 = 60;
pub const OBJ_VIEW: i16 = 61;
pub const OBJ_UCS_CONTROL: i16 = 62;
pub const OBJ_UCS: i16 = 63;
pub const OBJ_VPORT_CONTROL: i16 = 64;
pub const OBJ_VPORT: i16 = 65;
pub const OBJ_APPID_CONTROL: i16 = 66;
pub const OBJ_APPID: i16 = 67;
pub const OBJ_DIMSTYLE_CONTROL: i16 = 68;
pub const OBJ_DIMSTYLE: i16 = 69;
pub const OBJ_VPENT_HDR_CONTROL: i16 = 70;
pub const OBJ_VPENT_HDR: i16 = 71;

// ── Non-graphical objects ────────────────────────────────────────────

pub const OBJ_GROUP: i16 = 72;
pub const OBJ_MLINESTYLE: i16 = 73;
pub const OBJ_OLE2FRAME: i16 = 74;
pub const OBJ_DUMMY: i16 = 75;
pub const OBJ_LONG_TRANSACTION: i16 = 76;

// ── Standard fixed entity/object types (75+) ────────────────────────
//
// These match the reference ObjectType enum values.  Types 77–82 are
// fixed type codes in the ODA spec; class-based types (MESH, IMAGE,
// MULTILEADER) always use class numbers ≥500 in the binary.

pub const OBJ_LWPOLYLINE: i16 = 77; // 0x4D — fixed entity
pub const OBJ_HATCH: i16 = 78; // 0x4E — fixed entity
pub const OBJ_XRECORD: i16 = 79; // 0x4F — fixed non-entity
pub const OBJ_PLACEHOLDER: i16 = 80; // 0x50 — fixed non-entity
pub const OBJ_VBA_PROJECT: i16 = 81; // 0x51 — fixed non-entity
pub const OBJ_LAYOUT: i16 = 82; // 0x52 — fixed non-entity

// Class-based (UNLISTED) entity types — always use class number (500+)
// in the binary stream.  Matched via dxf_name → type_code translation.
pub const OBJ_IMAGE: i16 = -1;
pub const OBJ_MESH: i16 = -2;
pub const OBJ_MULTILEADER: i16 = -3;
pub const OBJ_WIPEOUT: i16 = -4;
// ACAD_SURFACE family — ACIS-backed graphical entities resolved by class name.
pub const OBJ_SURFACE: i16 = -5;
pub const OBJ_PLANESURFACE: i16 = -6;
pub const OBJ_EXTRUDEDSURFACE: i16 = -7;
pub const OBJ_LOFTEDSURFACE: i16 = -8;
pub const OBJ_REVOLVEDSURFACE: i16 = -9;
pub const OBJ_SWEPTSURFACE: i16 = -10;
pub const OBJ_NURBSURFACE: i16 = -11;
// Underlay references (PDF/DWF/DGN) — always class-based graphical entities.
// One sentinel each so the builder can recover the underlay flavour from the
// resolved type code; the bitstream layout is identical for all three.
pub const OBJ_PDFUNDERLAY: i16 = -12;
pub const OBJ_DWFUNDERLAY: i16 = -13;
pub const OBJ_DGNUNDERLAY: i16 = -14;
// HELIX (AcDbHelix) is a class-based, spline-derived graphical entity.
pub const OBJ_HELIX: i16 = -15;
// ACAD_TABLE (AcDbTable) is a class-based, INSERT-derived graphical entity.
pub const OBJ_TABLE: i16 = -16;
// AcDbLight (point/spot/distant light) is a class-based graphical entity. It is
// parsed for its glyph geometry only; the record is preserved verbatim for
// write-back, so no native encoder is needed.
pub const OBJ_LIGHT: i16 = -17;
pub const OBJ_ARC_DIMENSION: i16 = -18;
pub const OBJ_LARGE_RADIAL_DIMENSION: i16 = -19;
pub const OBJ_CAMERA: i16 = -20;
pub const OBJ_SECTIONOBJECT: i16 = -21;
pub const OBJ_ARCALIGNEDTEXT: i16 = -22;
pub const OBJ_RTEXT: i16 = -23;
pub const OBJ_GEOPOSITIONMARKER: i16 = -24;
pub const OBJ_NAVISWORKSMODEL: i16 = -25;
pub const OBJ_POINTCLOUD: i16 = -26;
pub const OBJ_POINTCLOUDEX: i16 = -27;
pub const OBJ_MPOLYGON: i16 = -28;
pub const OBJ_PROXY_ENTITY: i16 = 498;
pub const OBJ_PROXY_OBJECT: i16 = 499;

// Class-based non-entity objects — also resolved via class mapping for
// portable type codes.  The values here match the reference ObjectType.
pub const OBJ_DICTIONARYWDFLT: i16 = 0x78; // 120
pub const OBJ_DICTIONARYVAR: i16 = 0x79; // 121
pub const OBJ_PLOTSETTINGS: i16 = 0x7A; // 122
pub const OBJ_MLEADERSTYLE: i16 = 0x7B; // 123
pub const OBJ_IMAGEDEF: i16 = 0x7C; // 124
pub const OBJ_IMAGEDEFREACTOR: i16 = 0x7D; // 125
pub const OBJ_SCALE: i16 = 0x7E; // 126
pub const OBJ_SORTENTSTABLE: i16 = 0x7F; // 127
pub const OBJ_RASTERVARIABLES: i16 = 0x80; // 128
pub const OBJ_DBCOLOR: i16 = 0x81; // 129
pub const OBJ_WIPEOUTVARIABLES: i16 = 0x82; // 130
pub const OBJ_TABLECONTENT: i16 = 0x69; // 105
pub const OBJ_TABLESTYLE: i16 = 0x6A; // 106
                                      // GEODATA has no fixed DWG type code (it is always class-based); 0x83 is a free
                                      // internal sentinel used by the class-number map + builder dispatch.
pub const OBJ_GEODATA: i16 = 0x83; // 131
                                   // AcDbBlockVisibilityParameter is always class-based; 0x84 is a free internal
                                   // sentinel for the class-number map + builder dispatch.
pub const OBJ_BLOCKVISIBILITYPARAMETER: i16 = 0x84; // 132
                                                    // AcDbBlockRepresentationData — links an anonymous evaluated block back to its
                                                    // dynamic block definition. 0x85 is a free internal sentinel.
pub const OBJ_BLOCKREPRESENTATIONDATA: i16 = 0x85; // 133
                                                   // AcDbSpatialFilter (XCLIP clip boundary) is always class-based; 0x86 is a free
                                                   // internal sentinel for the class-number map + builder dispatch.
pub const OBJ_SPATIALFILTER: i16 = 0x86; // 134
                                         // Underlay definition objects (PDF/DWF/DGN) — always class-based; 0x87..0x89 are
                                         // free internal sentinels. One each so the builder recovers the underlay flavour
                                         // from the resolved type code (the bitstream body is identical for all three).
pub const OBJ_PDFDEFINITION: i16 = 0x87; // 135
pub const OBJ_DWFDEFINITION: i16 = 0x88; // 136
pub const OBJ_DGNDEFINITION: i16 = 0x89; // 137
                                         // AcDbField and AcDbFieldList are always class-based; 0x8A..0x8B are free
                                         // internal sentinels used by the builder dispatch.
pub const OBJ_FIELD: i16 = 0x8A; // 138
pub const OBJ_FIELDLIST: i16 = 0x8B; // 139
pub const OBJ_VISUALSTYLE: i16 = 0x8C; // 140
pub const OBJ_MATERIAL: i16 = 0x8D; // 141
pub const OBJ_OBJECT_PTR: i16 = 0x8E; // 142

/// Returns true if the type code is a graphical entity (not a table / object).
pub fn is_entity_type(type_code: i16) -> bool {
    // Fixed entity types: 1–47, 74 (OLE2FRAME), 77 (LWPOLYLINE), 78 (HATCH)
    // EXCEPT: 42 (OBJ_DICTIONARY) is a non-graphical object, not an entity.
    // Class-based entity sentinels: -28..-1.
    // Class-based entity types (≥500) are NOT included here; the builder
    // checks the class's is_an_entity flag directly.
    matches!(type_code, -28..=-1 | 1..=41 | 43..=47 | 74 | 77 | 78)
}

/// Returns true if the type code is a table control or entry.
pub fn is_table_type(type_code: i16) -> bool {
    matches!(type_code, 48..=71)
}

/// Map a DXF class name to the internal OBJ_* type code constant.
///
/// This is used to translate class-based type codes (500+) read from the
/// binary stream into the internal constants used by the document builder.
pub fn dxf_name_to_type_code(dxf_name: &str) -> Option<i16> {
    match dxf_name.to_uppercase().as_str() {
        // Entities
        "LWPOLYLINE" => Some(OBJ_LWPOLYLINE),
        "HATCH" => Some(OBJ_HATCH),
        "IMAGE" => Some(OBJ_IMAGE),
        "WIPEOUT" => Some(OBJ_WIPEOUT),
        "MESH" => Some(OBJ_MESH),
        "MULTILEADER" => Some(OBJ_MULTILEADER),
        "OLE2FRAME" => Some(OBJ_OLE2FRAME),
        "LONG_TRANSACTION" => Some(OBJ_LONG_TRANSACTION),
        // AcDbViewRepBlockReference (Model Documentation drawing views, R2013+)
        // is a subclass of AcDbBlockReference, so its record begins with the
        // INSERT fields. Decode it as an INSERT to place the baked view
        // geometry (the *A / *S view blocks) that the view border frames.
        "ACDBVIEWREPBLOCKREFERENCE" => Some(OBJ_INSERT),
        // ACAD_SURFACE family (ACIS-backed graphical entities).
        "SURFACE" => Some(OBJ_SURFACE),
        "PLANESURFACE" => Some(OBJ_PLANESURFACE),
        "EXTRUDEDSURFACE" => Some(OBJ_EXTRUDEDSURFACE),
        "LOFTEDSURFACE" => Some(OBJ_LOFTEDSURFACE),
        "REVOLVEDSURFACE" => Some(OBJ_REVOLVEDSURFACE),
        "SWEPTSURFACE" => Some(OBJ_SWEPTSURFACE),
        "NURBSURFACE" => Some(OBJ_NURBSURFACE),
        // Underlay references (PDF/DWF/DGN).
        "PDFUNDERLAY" => Some(OBJ_PDFUNDERLAY),
        "DWFUNDERLAY" => Some(OBJ_DWFUNDERLAY),
        "DGNUNDERLAY" => Some(OBJ_DGNUNDERLAY),
        "HELIX" => Some(OBJ_HELIX),
        "ACAD_TABLE" => Some(OBJ_TABLE),
        "LIGHT" => Some(OBJ_LIGHT),
        "ARC_DIMENSION" => Some(OBJ_ARC_DIMENSION),
        "LARGE_RADIAL_DIMENSION" => Some(OBJ_LARGE_RADIAL_DIMENSION),
        "CAMERA" => Some(OBJ_CAMERA),
        "SECTIONOBJECT" => Some(OBJ_SECTIONOBJECT),
        "ARCALIGNEDTEXT" => Some(OBJ_ARCALIGNEDTEXT),
        "RTEXT" => Some(OBJ_RTEXT),
        "POSITIONMARKER" | "GEOPOSITIONMARKER" => Some(OBJ_GEOPOSITIONMARKER),
        "COORDINATION_MODEL" | "NAVISWORKSMODEL" => Some(OBJ_NAVISWORKSMODEL),
        "ACDBPOINTCLOUD" | "POINTCLOUD" => Some(OBJ_POINTCLOUD),
        "ACDBPOINTCLOUDEX" | "POINTCLOUDEX" => Some(OBJ_POINTCLOUDEX),
        "MPOLYGON" => Some(OBJ_MPOLYGON),
        "ACAD_PROXY_ENTITY" => Some(OBJ_PROXY_ENTITY),
        "ACAD_PROXY_OBJECT" => Some(OBJ_PROXY_OBJECT),
        // Non-entity objects
        "ACDBDICTIONARYWDFLT" => Some(OBJ_DICTIONARYWDFLT),
        "DICTIONARYVAR" => Some(OBJ_DICTIONARYVAR),
        "LAYOUT" => Some(OBJ_LAYOUT),
        "XRECORD" => Some(OBJ_XRECORD),
        "ACDBPLACEHOLDER" => Some(OBJ_PLACEHOLDER),
        "PLOTSETTINGS" => Some(OBJ_PLOTSETTINGS),
        "MLEADERSTYLE" => Some(OBJ_MLEADERSTYLE),
        "IMAGEDEF" => Some(OBJ_IMAGEDEF),
        "IMAGEDEF_REACTOR" => Some(OBJ_IMAGEDEFREACTOR),
        "PDFDEFINITION" => Some(OBJ_PDFDEFINITION),
        "DWFDEFINITION" => Some(OBJ_DWFDEFINITION),
        "DGNDEFINITION" => Some(OBJ_DGNDEFINITION),
        "SCALE" => Some(OBJ_SCALE),
        "SORTENTSTABLE" => Some(OBJ_SORTENTSTABLE),
        "RASTERVARIABLES" => Some(OBJ_RASTERVARIABLES),
        "DBCOLOR" => Some(OBJ_DBCOLOR),
        "WIPEOUTVARIABLES" => Some(OBJ_WIPEOUTVARIABLES),
        "TABLECONTENT" => Some(OBJ_TABLECONTENT),
        "TABLESTYLE" => Some(OBJ_TABLESTYLE),
        "VISUALSTYLE" => Some(OBJ_VISUALSTYLE),
        "MATERIAL" => Some(OBJ_MATERIAL),
        "GEODATA" | "ACDBGEODATA" => Some(OBJ_GEODATA),
        "FIELD" | "ACDBFIELD" => Some(OBJ_FIELD),
        "FIELDLIST" | "ACDBFIELDLIST" => Some(OBJ_FIELDLIST),
        "SPATIAL_FILTER" | "SPATIALFILTER" => Some(OBJ_SPATIALFILTER),
        "BLOCKVISIBILITYPARAMETER" => Some(OBJ_BLOCKVISIBILITYPARAMETER),
        "ACDB_BLOCKREPRESENTATION_DATA" | "BLOCKREPRESENTATION" => {
            Some(OBJ_BLOCKREPRESENTATIONDATA)
        }
        "OBJECT_PTR" => Some(OBJ_OBJECT_PTR),
        _ => None,
    }
}

/// Returns true if the given class represents a graphical entity based
/// on its `is_an_entity` flag.
pub fn is_class_entity(is_an_entity: bool) -> bool {
    is_an_entity
}
