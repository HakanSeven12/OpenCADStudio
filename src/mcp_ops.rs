//! Declarative description of every MCP op. Single source of truth for the
//! advertised JSON Schema, the pre-dispatch validator, and test examples.

use serde_json::{json, Map, Value};

pub const TARGET_VERSIONS: &[&str] = &[
    "R14", "2000", "2004", "2007", "2010", "2013", "2018", "AC1014", "AC1015", "AC1018", "AC1021",
    "AC1024", "AC1027", "AC1032",
];

pub const ENVELOPE_KEYS: &[&str] = &[
    "op",
    "request_id",
    "document_id",
    "revision",
    "geometry_revision",
    "camera_revision",
    "selection",
    "client_id",
    "steps",
];

#[derive(Clone, Copy)]
pub enum Ty {
    Str,
    Bool,
    Num,
    Int,
    /// [x, y] or [x, y, z]; a missing z is 0.0 at runtime.
    Point,
    /// Hex string, optional 0x prefix. JSON integers are rejected at runtime.
    Handle,
    /// Non-empty array of Handle.
    Handles,
    /// [x0, y0, x1, y1]
    Window,
    Enum(&'static [&'static str]),
    /// Runtime compares case-insensitively; the schema advertises canonical case.
    EnumCi(&'static [&'static str]),
    ActionName,
    ArrayOf(&'static Ty),
    /// Entity definitions with on-demand creation schemas.
    Entities,
    BatchSteps,
    /// Inline JSON Schema fragment, validated before dispatch.
    Raw(&'static str),
}

pub struct Param {
    pub name: &'static str,
    pub ty: Ty,
    pub required: bool,
    pub doc: &'static str,
}

pub enum Rule {
    /// At least one of these keys must be present.
    AnyOf(&'static [&'static str]),
    /// Exactly one key must be present and non-null (or deferred by a batch
    /// binding). Explicit null is invalid, not an omitted alternative.
    ExactlyOneOf(&'static [&'static str]),
    /// Reject unknown request keys instead of retaining the legacy warning.
    RejectUnknown,
    /// If `key` equals `equals`, every key in `then` must be present.
    RequiredWhen {
        key: &'static str,
        equals: &'static str,
        then: &'static [&'static str],
    },
}

pub struct OpDef {
    pub name: &'static str,
    pub doc: &'static str,
    pub batchable: bool,
    pub params: &'static [Param],
    pub rules: &'static [Rule],
    /// Minimal valid payload (JSON text). Drives schema, validator and parity tests.
    pub example: &'static str,
}

const fn req(name: &'static str, ty: Ty, doc: &'static str) -> Param {
    Param {
        name,
        ty,
        required: true,
        doc,
    }
}

const fn opt(name: &'static str, ty: Ty, doc: &'static str) -> Param {
    Param {
        name,
        ty,
        required: false,
        doc,
    }
}

const COLOR_SCHEMA: &str = r#"{"anyOf":[{"type":"string","enum":["ByLayer","ByBlock"]},{"type":"integer","minimum":0,"maximum":256},{"type":"object","properties":{"Index":{"type":"integer","minimum":0,"maximum":256}},"required":["Index"],"additionalProperties":false},{"type":"object","properties":{"Rgb":{"type":"object","properties":{"r":{"type":"integer","minimum":0,"maximum":255},"g":{"type":"integer","minimum":0,"maximum":255},"b":{"type":"integer","minimum":0,"maximum":255}},"required":["r","g","b"],"additionalProperties":false}},"required":["Rgb"],"additionalProperties":false},{"type":"object","properties":{"rgb":{"type":"array","items":{"type":"integer","minimum":0,"maximum":255},"minItems":3,"maxItems":3}},"required":["rgb"],"additionalProperties":false}]}"#;

// ============================================================================
// OP DEFINITIONS
// ============================================================================

pub static NEW: OpDef = OpDef {
    name: "new",
    doc: "Start an empty document, optionally seeded from a DWG/DXF/DWT template.",
    batchable: true,
    params: &[opt(
        "template",
        Ty::Str,
        "Optional template drawing path whose styles and tables survive.",
    )],
    rules: &[],
    example: r#"{"op":"new"}"#,
};

pub static OPEN: OpDef = OpDef {
    name: "open",
    doc: "Open an existing drawing file (DWG or DXF).",
    batchable: true,
    params: &[req("path", Ty::Str, "Absolute path to the drawing file.")],
    rules: &[],
    example: r#"{"op":"open","path":"drawing.dxf"}"#,
};

pub static ACTIVATE: OpDef = OpDef {
    name: "activate",
    doc: "Switch the active drawing tab to another open document.",
    batchable: true,
    params: &[req(
        "document_id",
        Ty::Int,
        "Target document ID to activate.",
    )],
    rules: &[],
    example: r#"{"op":"activate","document_id":1}"#,
};

pub static SWITCH_DOCUMENT: OpDef = OpDef {
    name: "switch_document",
    doc: "Alias for activate. Switch the active drawing tab to another open document.",
    batchable: true,
    params: &[req(
        "document_id",
        Ty::Int,
        "Target document ID to activate.",
    )],
    rules: &[],
    example: r#"{"op":"switch_document","document_id":1}"#,
};

pub static RUN: OpDef = OpDef {
    name: "run",
    doc:
        "Execute a command string with prompt answers separated by spaces (e.g. 'LINE 0,0 10,10').",
    batchable: true,
    params: &[req("cmd", Ty::Str, "Command name and arguments.")],
    rules: &[],
    example: r#"{"op":"run","cmd":"LINE 0,0 10,10"}"#,
};

pub static START: OpDef = OpDef {
    name: "start",
    doc: "Start an interactive command waiting for subsequent 'input' steps.",
    batchable: true,
    params: &[req("cmd", Ty::Str, "Command name to start.")],
    rules: &[],
    example: r#"{"op":"start","cmd":"LINE"}"#,
};

pub static INPUT: OpDef = OpDef {
    name: "input",
    doc: "Feed the next step input to an active command.",
    batchable: true,
    params: &[
        req(
            "kind",
            Ty::Enum(&[
                "text",
                "token",
                "point",
                "entity",
                "structure",
                "selection",
                "enter",
            ]),
            "Input kind.",
        ),
        opt("text", Ty::Str, "Free text or token option."),
        opt("point", Ty::Point, "Point coordinates."),
        opt(
            "space",
            Ty::Enum(&["wcs", "ucs", "relative"]),
            "Coordinate space for point (defaults to wcs).",
        ),
        opt(
            "handle",
            Ty::Handle,
            "Target entity handle for entity/structure pick.",
        ),
        opt(
            "handles",
            Ty::Handles,
            "Entity handles to hand a selection prompt (kind selection), instead of the current selection.",
        ),
    ],
    rules: &[
        Rule::RequiredWhen {
            key: "kind",
            equals: "token",
            then: &["text"],
        },
        Rule::RequiredWhen {
            key: "kind",
            equals: "point",
            then: &["point"],
        },
        Rule::RequiredWhen {
            key: "kind",
            equals: "entity",
            then: &["handle", "point"],
        },
        Rule::RequiredWhen {
            key: "kind",
            equals: "structure",
            then: &["handle", "point"],
        },
    ],
    example: r#"{"op":"input","kind":"point","point":[10,20]}"#,
};

pub static CANCEL: OpDef = OpDef {
    name: "cancel",
    doc: "Cancel the active command, equivalent to pressing Escape.",
    batchable: true,
    params: &[],
    rules: &[],
    example: r#"{"op":"cancel"}"#,
};

pub static UNDO: OpDef = OpDef {
    name: "undo",
    doc: "Undo the last drawing modification.",
    batchable: true,
    params: &[],
    rules: &[],
    example: r#"{"op":"undo"}"#,
};

pub static REDO: OpDef = OpDef {
    name: "redo",
    doc: "Redo the previously undone drawing modification.",
    batchable: true,
    params: &[],
    rules: &[],
    example: r#"{"op":"redo"}"#,
};

pub static SELECT: OpDef = OpDef {
    name: "select",
    doc: "Modify current entity selection by handles, filters, or query criteria.",
    batchable: true,
    params: &[
        opt("handles", Ty::Handles, "Explicit entity handles to select."),
        opt("type", Ty::Str, "Entity type filter."),
        opt("layer", Ty::Str, "Layer name filter."),
        opt("window", Ty::Raw(r#"{"type":"array","items":{"type":"number"},"minItems":4,"maxItems":4}"#), "World XY box [x1, y1, x2, y2]: selects what lies inside it, as a viewport window does."),
        opt("crossing", Ty::Bool, "With window: also select what the box crosses."),
        opt("clear", Ty::Bool, "Clear existing selection before applying."),
        opt("where", Ty::ArrayOf(&Ty::Raw(r#"{"type":"object","properties":{"path":{"type":"string"},"op":{"type":"string"},"value":{}},"required":["path"]}"#)), "Property filter predicates."),
    ],
    rules: &[],
    example: r#"{"op":"select","clear":true}"#,
};

pub static PROPERTY: OpDef = OpDef {
    name: "property",
    doc: "Modify a single property of selected entities.",
    batchable: true,
    params: &[
        req("field", Ty::Str, "Property field name."),
        req(
            "value",
            Ty::Raw(r#"{"description":"New property value"}"#),
            "New value for the field.",
        ),
    ],
    rules: &[],
    example: r#"{"op":"property","field":"color","value":1}"#,
};

pub static GRIP_DRAG: OpDef = OpDef {
    name: "grip_drag",
    doc: "Drag one grip to a world point the way a grip pick-and-place in the viewport does. Built-ins: supply handle (read grips for the ids); object snaps, ortho and polar are off for the move. Plugin-declared custom grips (API v11): supply plugin (the owner id from the grips read's plugin_grips) instead of a handle — the drag routes to the owning plugin (begin, move, settle), whose settle writes are its own single undo step.",
    batchable: true,
    params: &[
        opt("handle", Ty::Handle, "Entity whose grip moves (built-in grips)."),
        req("grip_id", Ty::Int, "Grip id from the grips read (entity grip id, or the plugin grip's id for custom grips)."),
        req("point", Ty::Point, "World point [x, y, z] the grip moves to."),
        opt("plugin", Ty::Str, "Owner plugin id when dragging a plugin-declared custom grip (from the grips read's plugin_grips); omit for built-in entity grips."),
    ],
    rules: &[],
    example: r#"{"op":"grip_drag","plugin":"opencad.civil_ifc","grip_id":7,"point":[10,0,0]}"#,
};

pub static CLICK: OpDef = OpDef {
    name: "click",
    doc: "Left-click the viewport where a world point shows, as the mouse would. modifiers holds ctrl and/or shift (e.g. Ctrl+click picks the solid of a composite under the cursor).",
    batchable: true,
    params: &[
        req("point", Ty::Point, "World point [x, y, z] to click on."),
        opt("modifiers", Ty::Str, "Keys held during the click: \"ctrl\", \"shift\" or \"ctrl+shift\"."),
    ],
    rules: &[],
    example: r#"{"op":"click","point":[10,0,0],"modifiers":"ctrl"}"#,
};

pub static POINTER: OpDef = OpDef {
    name: "pointer",
    doc: "Replay mouse events on the drawing canvas in order, as the mouse sends them: move to canvas pixels or to a world point (projected through the camera the cursor edits with), press and release a button, turn the wheel. A grip press-move-release goes through the same cursor path as a hand drag. Each step reports the cursor's world point.",
    batchable: true,
    params: &[
        req(
            "events",
            Ty::ArrayOf(&Ty::Raw(r#"{"type":"object","properties":{"move":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2},"move_world":{"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3},"press":{"type":"string","enum":["left","right","middle"]},"release":{"type":"string","enum":["left","right","middle"]},"wheel":{"type":"number"}}}"#)),
            "Events in order; each holds one of move, move_world, press, release, wheel (lines, positive zooms in) or leave (true: the pointer leaves the canvas).",
        ),
        opt("modifiers", Ty::Str, "Keys held throughout: \"ctrl\", \"shift\" or \"ctrl+shift\"."),
    ],
    rules: &[],
    example: r#"{"op":"pointer","events":[{"move_world":[10,0,0]},{"press":"left"},{"release":"left"},{"move_world":[12,3,0]},{"press":"left"},{"release":"left"}]}"#,
};

pub static SET_PROPERTIES: OpDef = OpDef {
    name: "set_properties",
    doc: "Atomically update record properties by RFC 6901 JSON pointer paths.",
    batchable: true,
    params: &[
        req("collection", Ty::Str, "Record collection name."),
        opt("handle", Ty::Handle, "Target record handle if applicable."),
        opt(
            "name",
            Ty::Str,
            "Target table record name (layers, styles, ...) instead of handle.",
        ),
        req(
            "updates",
            Ty::ArrayOf(&Ty::Raw(
                r#"{"type":"object","properties":{"path":{"type":"string"},"value":{},"expected":{}},"required":["path","value"]}"#,
            )),
            "List of pointer replacements. A colour takes an index number, {\"rgb\":[r,g,b]} or its serialized form ({\"Index\":3}, \"ByLayer\").",
        ),
    ],
    rules: &[],
    example: r#"{"op":"set_properties","collection":"layers","name":"Walls","updates":[{"path":"/color","value":3}]}"#,
};

pub static PANEL_INPUT: OpDef = OpDef {
    name: "panel_input",
    doc: "Edit one native dock-panel field. Changes only the UI draft; records and geometry are not written.",
    batchable: false,
    params: &[
        req("panel_id", Ty::Str, "Registered native panel id."),
        req("control_id", Ty::Str, "Field id declared by that panel."),
        opt("text", Ty::Str, "Exact draft text (preferred for Number fields)."),
        opt("value", Ty::Num, "Finite numeric draft converted to text."),
    ],
    rules: &[],
    example: r#"{"op":"panel_input","panel_id":"cvifc.ifc_h_properties","control_id":"SegmentLength","text":"101.5"}"#,
};

pub static PANEL_BUTTON: OpDef = OpDef {
    name: "panel_button",
    doc: "Activate a native dock-panel button's declared action. The ordinary document-bound plugin command owns validation, records, and undo.",
    batchable: false,
    params: &[
        req("panel_id", Ty::Str, "Registered native panel id."),
        req("control_id", Ty::Str, "Button id declared by that panel, or reset."),
    ],
    rules: &[],
    example: r#"{"op":"panel_button","panel_id":"cvifc.ifc_h_properties","control_id":"apply"}"#,
};

pub static ACTION: OpDef = OpDef {
    name: "action",
    doc: "Trigger a named GUI action or dialog command.",
    batchable: true,
    params: &[req(
        "name",
        Ty::ActionName,
        "Name of the UI action to execute.",
    )],
    rules: &[],
    example: r#"{"op":"action","name":"zoom_extents"}"#,
};

pub static EMBED_IMAGE: OpDef = OpDef {
    name: "embed_image",
    doc: "Embed or link a raster image into the drawing with optional reference scaling and calibration.",
    batchable: true,
    params: &[
        req("path", Ty::Str, "Path to the image file."),
        opt("at", Ty::Point, "Lower-left corner [x, y] or [x, y, z]."),
        opt("width", Ty::Num, "World width of the image."),
        opt("linked", Ty::Bool, "Store as path-linked RasterImage instead of embedded OLE2FRAME."),
        opt("calibrate", Ty::Raw(r#"{"type":"object","description":"Automatic 2-point reference calibration: {point_a:[px1,py1], point_b:[px2,py2], distance:650.0, align_to:[0,0]}"}"#), "Automatic 2-point reference calibration: {point_a:[px1,py1], point_b:[px2,py2], distance:650.0, align_to:[0,0]}."),
        opt("layer", Ty::Str, "Layer name to place the image on (auto-created if missing, e.g. '_XREF')."),
        opt("lock_layer", Ty::Bool, "Lock the target layer after attaching."),
        opt("source_points", Ty::ArrayOf(&Ty::Point), "Pixel points in image [[px1,py1],[px2,py2]] for 2-point alignment."),
        opt("target_points", Ty::ArrayOf(&Ty::Point), "CAD world points [[x1,y1],[x2,y2]] for 2-point alignment."),
    ],
    rules: &[],
    example: r#"{"op":"embed_image","path":"plan.png","linked":true,"calibrate":{"point_a":[52,910],"point_b":[450,910],"distance":6500},"layer":"_XREF","lock_layer":true}"#,
};

pub static DGN_IMPORT: OpDef = OpDef {
    name: "dgn_import",
    doc: "Import a DGN model synchronously with named options and no prompt replay.",
    batchable: true,
    params: &[
        req("path", Ty::Str, "DGN source file path."),
        opt("model", Ty::Str, "Design model name (case-insensitive; omitted uses the first)."),
        opt("units", Ty::EnumCi(&["Master", "Sub"]), "DGN unit conversion (Master or Sub)."),
        opt("mapping", Ty::Str, "Conversion mapping; Standard, optionally with ;codepage=N."),
        opt("codepage", Ty::Int, "V7 text code page; 0/1252 select the default; explicit value overrides mapping suffix."),
    ],
    rules: &[],
    example: r#"{"op":"dgn_import","path":"cadastral.dgn","model":"Default","units":"Master","mapping":"Standard","codepage":1250}"#,
};

pub static WBLOCK: OpDef = OpDef {
    name: "wblock",
    doc: "Export selected entities or a block definition to an external DWG/DXF file.",
    batchable: true,
    params: &[
        req("path", Ty::Str, "Target drawing path for export."),
        opt("handles", Ty::Handles, "Entity handles to export."),
        opt("block", Ty::Str, "Block name to export."),
        opt(
            "template",
            Ty::Str,
            "Base template file to carry styles and tables.",
        ),
        opt(
            "normalize",
            Ty::Bool,
            "Translate exported entities to origin [0,0,0].",
        ),
    ],
    rules: &[],
    example: r#"{"op":"wblock","path":"part.dwg","handles":["2A"]}"#,
};

pub static PLOT: OpDef = OpDef {
    name: "plot",
    doc: "Plot layouts to a PDF or SVG file. scale (e.g. \"1:100\") overrides fit; fit defaults to true when scale is omitted.",
    batchable: true,
    params: &[
        req("path", Ty::Str, "Output PDF or SVG file path (selected by extension)."),
        opt("layout", Ty::Str, "Target layout name or 'all' (defaults to 'Model')."),
        opt("area", Ty::EnumCi(&["extents", "display", "limits", "layout", "window"]), "Plot area (defaults to extents)."),
        opt("window", Ty::Window, "World [x0, y0, x1, y1] rectangle; required when area=window."),
        opt("paper", Ty::Str, "Canonical paper size name from paper catalog."),
        opt("orientation", Ty::EnumCi(&["Portrait", "Landscape"]), "Sheet orientation."),
        opt("shade", Ty::EnumCi(crate::io::plot::SHADE_PLOT_MODES), "Shaded viewport plot mode."),
        opt("fit", Ty::Bool, "Fit drawing to sheet (defaults to true if scale omitted)."),
        opt("scale", Ty::Str, "Plot scale as paper:drawing ratio (e.g. \"1:100\")."),
        opt("center", Ty::Bool, "Center plot on sheet (defaults to true)."),
        opt("offset_x", Ty::Num, "Plot offset X in mm."),
        opt("offset_y", Ty::Num, "Plot offset Y in mm."),
        opt("upside_down", Ty::Bool, "Rotate content 180 degrees."),
        opt("plot_style", Ty::Str, "CTB file path or named plot style table."),
        opt("transparency", Ty::Bool, "Honor transparency settings."),
        opt("lineweights", Ty::Bool, "Honor lineweight settings."),
        opt("scale_lineweights", Ty::Bool, "Scale lineweights with a non-fit plot scale."),
        opt("paperspace_last", Ty::Bool, "Plot model-space objects before paper-space objects."),
        opt("apply_plot_styles", Ty::Bool, "Enable/disable the selected or requested plot style; explicit false wins."),
        opt("merge_lines", Ty::Bool, "Merge overlapping lines."),
        opt("text_as_outlines", Ty::Bool, "SVG only: emit font-independent glyph paths instead of text elements."),
        opt("layer_attributes", Ty::Bool, "SVG only: tag each drawn item with its drawing layer as data-layer groups the consumer can toggle."),
        opt("non_scaling_stroke", Ty::Bool, "SVG only: keep stroke widths constant under downstream SVG transforms (web maps)."),
        opt("stamp", Ty::Bool, "Print plot stamp."),
        opt("per_page", Ty::Bool, "When layout='all', write one output file per layout as <stem>-<Layout>.<ext>."),
    ],
    rules: &[Rule::RequiredWhen { key: "area", equals: "window", then: &["window"] }],
    example: r#"{"op":"plot","path":"out.pdf","area":"extents","scale":"1:100"}"#,
};

pub static ENTITIES_CREATE: OpDef = OpDef {
    name: "entities_create",
    doc: "Create new CAD entities from typed definitions.",
    batchable: true,
    params: &[req(
        "entities",
        Ty::Entities,
        "Entity definitions. Read ocs_read op entity_schema with parameters.type for fields and examples before creating an unfamiliar type.",
    )],
    rules: &[],
    example: r#"{"op":"entities_create","entities":[{"type":"Line","start":[0,0],"end":[10,10]}]}"#,
};

pub static ENTITIES_SET: OpDef = OpDef {
    name: "entities_set",
    doc: "Atomically apply shared common-property writes to any number of entity handles in one undo step.",
    batchable: true,
    params: &[
        req("handles", Ty::Handles, "Non-empty unique entity handle list."),
        opt("layer", Ty::Str, "Existing target layer name; omitted leaves layers unchanged."),
        opt("color", Ty::Raw(COLOR_SCHEMA), "ByLayer/ByBlock, ACI 0..256, {Index:N}, {Rgb:{r,g,b}}, or {rgb:[r,g,b]}."),
        opt("linetype", Ty::Str, "Existing linetype name, ByLayer, or ByBlock."),
        opt("linetype_scale", Ty::Num, "Finite positive linetype scale."),
        opt("invisible", Ty::Bool, "Exact visibility state (false = visible)."),
        opt("label", Ty::Str, "Optional non-empty undo-history label."),
    ],
    rules: &[
        Rule::AnyOf(&["layer", "color", "linetype", "linetype_scale", "invisible"]),
        Rule::RejectUnknown,
    ],
    example: r#"{"op":"entities_set","handles":["2A","2B"],"layer":"Walls","color":"ByLayer","label":"Cadastral cleanup"}"#,
};

pub static ENTITIES_DELETE: OpDef = OpDef {
    name: "entities_delete",
    doc: "Delete entities by their handles.",
    batchable: true,
    params: &[req(
        "handles",
        Ty::Handles,
        "Handles of entities to delete.",
    )],
    rules: &[],
    example: r#"{"op":"entities_delete","handles":["2A"]}"#,
};

pub static ENTITIES_TRANSFORM: OpDef = OpDef {
    name: "entities_transform",
    doc: "Move, copy, rotate, scale, mirror or array entities.",
    batchable: true,
    params: &[
        req("handles", Ty::Handles, "Entities to transform."),
        opt(
            "action",
            Ty::EnumCi(&["move", "copy", "rotate", "scale", "mirror", "array"]),
            "Transform action (defaults to move).",
        ),
        opt(
            "vector",
            Ty::Point,
            "move/copy: displacement [dx, dy] or [dx, dy, dz].",
        ),
        opt("center", Ty::Point, "rotate/scale: base point."),
        opt("angle_deg", Ty::Num, "rotate: angle in degrees CCW."),
        opt("factor", Ty::Num, "scale: non-zero scale factor."),
        opt(
            "axis",
            Ty::ArrayOf(&Ty::Point),
            "mirror: axis as two points [[x1, y1], [x2, y2]].",
        ),
        opt(
            "copy",
            Ty::Bool,
            "mirror/copy: keep originals and return new copies.",
        ),
        opt("rows", Ty::Int, "array: row count (1..100, defaults to 1)."),
        opt(
            "columns",
            Ty::Int,
            "array: column count (1..100, defaults to 1).",
        ),
        opt("row_spacing", Ty::Num, "array: row spacing."),
        opt("column_spacing", Ty::Num, "array: column spacing."),
    ],
    rules: &[
        Rule::RequiredWhen {
            key: "action",
            equals: "scale",
            then: &["factor"],
        },
        Rule::RequiredWhen {
            key: "action",
            equals: "mirror",
            then: &["axis"],
        },
    ],
    example: r#"{"op":"entities_transform","handles":["2A"],"action":"move","vector":[10,0]}"#,
};

pub static TEXT_REPLACE: OpDef = OpDef {
    name: "text_replace",
    doc: "Find and replace text in drawing text entities. Give find (+ optional replace) for one pair, or pairs for several.",
    batchable: true,
    params: &[
        opt("find", Ty::Str, "Text to find."),
        opt("replace", Ty::Str, "Replacement text. Defaults to empty string (deletes the match)."),
        opt("pairs", Ty::ArrayOf(&Ty::Raw(
            r#"{"type":"object","properties":{"find":{"type":"string"},"replace":{"type":"string"}},"required":["find"]}"#
        )), "Several find/replace pairs applied in order."),
        opt("match_case", Ty::Bool, "Case-sensitive search."),
        opt("whole_word", Ty::Bool, "Match only whole words."),
        opt("ignore_accents", Ty::Bool, "Ignore accents/diacritics (defaults to true when match_case is false)."),
        opt("dry_run", Ty::Bool, "Report matches without changing anything."),
        opt("replace_all", Ty::Bool, "Replace all occurrences within matching entities (defaults to true)."),
        opt("scope", Ty::Enum(&["all", "active_space", "model_space", "blocks"]), "Search scope."),
        opt("layer", Ty::Str, "Optional layer name filter."),
        opt("handles", Ty::Handles, "Optional handles filter."),
        opt("type", Ty::Str, "Optional entity type filter."),
        opt("bounds", Ty::Window, "Optional bounding box filter [min_x, min_y, max_x, max_y]."),
    ],
    rules: &[Rule::AnyOf(&["find", "pairs"])],
    example: r#"{"op":"text_replace","find":"OLD","replace":"NEW"}"#,
};

pub static BLOCK_DEFINE: OpDef = OpDef {
    name: "block_define",
    doc: "Define a block from existing entities and place an Insert reference.",
    batchable: true,
    params: &[
        req("name", Ty::Str, "Block name."),
        req("handles", Ty::Handles, "Entities that form the block."),
        req("base", Ty::Point, "Block origin point."),
        opt(
            "insert_at",
            Ty::Point,
            "Placement coordinates for the replacement Insert reference (defaults to base).",
        ),
        opt(
            "replace",
            Ty::Bool,
            "Overwrite an existing block definition of the same name.",
        ),
    ],
    rules: &[],
    example: r#"{"op":"block_define","name":"DOOR_90","handles":["2A","2B"],"base":[0,0]}"#,
};

pub static BLOCK_DELETE: OpDef = OpDef {
    name: "block_delete",
    doc: "Delete a block definition and all its child entities. A referenced block is refused; force erases the referencing inserts.",
    batchable: true,
    params: &[
        req("name", Ty::Str, "Block name to delete."),
        opt(
            "force",
            Ty::Bool,
            "Erase the referencing inserts instead of refusing a referenced block. Result reports how many references were overridden.",
        ),
    ],
    rules: &[],
    example: r#"{"op":"block_delete","name":"DOOR","force":true}"#,
};

pub static XDATA_SET: OpDef = OpDef {
    name: "xdata_set",
    doc: "Attach or clear extended entity data (XData) for a registered application.",
    batchable: true,
    params: &[
        req("app", Ty::Str, "Application name."),
        req("handles", Ty::Handles, "Target entity handles."),
        opt(
            "data",
            Ty::ArrayOf(&Ty::Raw(
                r#"{"type":"object","properties":{"code":{"type":"integer"},"value":{}},"required":["code","value"]}"#,
            )),
            "Typed XData entries [{code, value}]. Empty list removes app record.",
        ),
    ],
    rules: &[],
    example: r#"{"op":"xdata_set","app":"APP","handles":["2A"],"data":[{"code":1000,"value":"tag"}]}"#,
};

pub static VIEW_FOCUS: OpDef = OpDef {
    name: "view_focus",
    doc: "Focus the viewport camera on the bounds of specific entities.",
    batchable: true,
    params: &[
        req("handles", Ty::Handles, "Entities to focus on."),
        opt(
            "highlight",
            Ty::Bool,
            "Also select the entities (defaults to true).",
        ),
    ],
    rules: &[],
    example: r#"{"op":"view_focus","handles":["2A"]}"#,
};

pub static ENTITIES_COPY_TO: OpDef = OpDef {
    name: "entities_copy_to",
    doc: "Copy entities from the current document into another open document.",
    batchable: true,
    params: &[
        req(
            "handles",
            Ty::Handles,
            "Entities in the current active document to copy.",
        ),
        opt(
            "target_document_id",
            Ty::Int,
            "Target document ID to receive the copied entities.",
        ),
        opt(
            "document_id",
            Ty::Int,
            "Deprecated alias for target_document_id.",
        ),
    ],
    rules: &[Rule::AnyOf(&["target_document_id", "document_id"])],
    example: r#"{"op":"entities_copy_to","handles":["2A"],"target_document_id":2}"#,
};

pub static GROUP_CREATE: OpDef = OpDef {
    name: "group_create",
    doc: "Create a named entity group.",
    batchable: true,
    params: &[
        req("name", Ty::Str, "Group name."),
        req("handles", Ty::Handles, "Entities in the group."),
    ],
    rules: &[],
    example: r#"{"op":"group_create","name":"Group1","handles":["2A"]}"#,
};

pub static SELECTION_SET_SAVE: OpDef = OpDef {
    name: "selection_set_save",
    doc: "Save an entity handle set under a name for later recall.",
    batchable: true,
    params: &[
        req("name", Ty::Str, "Selection set name."),
        req("handles", Ty::Handles, "Entities to save."),
    ],
    rules: &[],
    example: r#"{"op":"selection_set_save","name":"Sel1","handles":["2A"]}"#,
};

pub static SELECTION_SET_LOAD: OpDef = OpDef {
    name: "selection_set_load",
    doc: "Recall a previously saved selection set by name.",
    batchable: true,
    params: &[
        req("name", Ty::Str, "Selection set name."),
        opt(
            "select",
            Ty::Bool,
            "Also select the recalled entities in the viewport (defaults to true).",
        ),
    ],
    rules: &[],
    example: r#"{"op":"selection_set_load","name":"Sel1"}"#,
};

pub static USER_SELECT: OpDef = OpDef {
    name: "user_select",
    doc: "Prompt the user at the desktop screen to interactively pick entities.",
    batchable: true,
    params: &[
        opt(
            "prompt",
            Ty::Str,
            "Prompt text pinned on screen while user picks.",
        ),
        opt(
            "detail",
            Ty::EnumCi(&["summary", "geometry", "full"]),
            "Returned entity detail level (defaults to full).",
        ),
        opt("type", Ty::Str, "Entity type filter."),
        opt("layer", Ty::Str, "Layer filter."),
        opt(
            "clear",
            Ty::Bool,
            "Clear current selection before pick begins (defaults to true).",
        ),
    ],
    rules: &[],
    example: r#"{"op":"user_select","prompt":"Pick entities"}"#,
};

pub static GETPOINT: OpDef = OpDef {
    name: "getpoint",
    doc: "Prompt the user at the desktop screen to click one point in the viewport.",
    batchable: true,
    params: &[opt(
        "prompt",
        Ty::Str,
        "Prompt text displayed while waiting for click.",
    )],
    rules: &[],
    example: r#"{"op":"getpoint","prompt":"Click start point"}"#,
};

pub static CLOSE: OpDef = OpDef {
    name: "close",
    doc: "Close the active drawing document.",
    batchable: true,
    params: &[opt(
        "discard",
        Ty::Bool,
        "Erase unsaved changes instead of refusing a dirty document.",
    )],
    rules: &[],
    example: r#"{"op":"close"}"#,
};

pub static SYSVAR: OpDef = OpDef {
    name: "sysvar",
    doc: "Read or write CAD system variables (ltscale, pdmode, pdsize, clayer, etc.).",
    batchable: true,
    params: &[
        opt(
            "get",
            Ty::ArrayOf(&Ty::Str),
            "List of variable names to read.",
        ),
        opt(
            "set",
            Ty::Raw(r#"{"type":"object","description":"Variable name=value pairs to set"}"#),
            "Key-value map of variables to set in one undo transaction.",
        ),
    ],
    rules: &[],
    example: r#"{"op":"sysvar","get":["LTSCALE"]}"#,
};

pub static LAYOUT_CREATE: OpDef = OpDef {
    name: "layout_create",
    doc: "Create a new paper-space layout tab.",
    batchable: true,
    params: &[req("name", Ty::Str, "Layout tab name.")],
    rules: &[],
    example: r#"{"op":"layout_create","name":"Sheet1"}"#,
};

pub static LAYOUT_RENAME: OpDef = OpDef {
    name: "layout_rename",
    doc: "Rename a paper-space layout; Model is protected.",
    batchable: true,
    params: &[
        req("name", Ty::Str, "Existing layout name (case-insensitive)."),
        req("new_name", Ty::Str, "New unique layout name."),
    ],
    rules: &[],
    example: r#"{"op":"layout_rename","name":"Sheet1","new_name":"Floor Plan"}"#,
};

pub static LAYOUT_ACTIVATE: OpDef = OpDef {
    name: "layout_activate",
    doc: "Activate an existing Model or paper-space layout without modifying the drawing.",
    batchable: true,
    params: &[req("name", Ty::Str, "Layout name (case-insensitive).")],
    rules: &[],
    example: r#"{"op":"layout_activate","name":"Floor Plan"}"#,
};

pub static LAYOUT_DELETE: OpDef = OpDef {
    name: "layout_delete",
    doc: "Delete a paper-space layout in one undo step; Model is protected.",
    batchable: true,
    params: &[req("name", Ty::Str, "Layout name (case-insensitive).")],
    rules: &[],
    example: r#"{"op":"layout_delete","name":"Floor Plan"}"#,
};

pub static PAGE_SETUP_SET: OpDef = OpDef {
    name: "page_setup_set",
    doc: "Configure page setup / print settings for a layout.",
    batchable: true,
    params: &[
        req("layout", Ty::Str, "Target layout name."),
        opt("paper", Ty::Str, "Paper size name."),
        opt(
            "orientation",
            Ty::EnumCi(&["Portrait", "Landscape"]),
            "Paper orientation.",
        ),
        opt("fit", Ty::Bool, "Fit to paper (defaults to true)."),
        opt("scale", Ty::Str, "Paper-to-drawing scale ratio."),
        opt(
            "center",
            Ty::Bool,
            "Center drawing on sheet (defaults to true).",
        ),
        opt("offset_x", Ty::Num, "X offset in mm."),
        opt("offset_y", Ty::Num, "Y offset in mm."),
        opt("plot_style", Ty::Str, "CTB file path or named style."),
        opt(
            "area",
            Ty::EnumCi(&["extents", "display", "limits", "layout", "window"]),
            "Plot area.",
        ),
        opt(
            "window",
            Ty::Window,
            "World [x0, y0, x1, y1] rectangle; required when area=window.",
        ),
    ],
    rules: &[Rule::RequiredWhen {
        key: "area",
        equals: "window",
        then: &["window"],
    }],
    example: r#"{"op":"page_setup_set","layout":"Sheet1","paper":"A4"}"#,
};

pub static XREF_RELOAD: OpDef = OpDef {
    name: "xref_reload",
    doc: "Reload one direct external reference by id, preserving prior content on load failure.",
    batchable: true,
    params: &[req("id", Ty::Handle, "Direct reference id from xrefs.")],
    rules: &[Rule::RejectUnknown],
    example: r#"{"op":"xref_reload","id":"2A"}"#,
};

pub static XREF_UNLOAD: OpDef = OpDef {
    name: "xref_unload",
    doc: "Unload one direct external reference without deleting its definition.",
    batchable: true,
    params: &[req("id", Ty::Handle, "Direct reference id from xrefs.")],
    rules: &[Rule::RejectUnknown],
    example: r#"{"op":"xref_unload","id":"2A"}"#,
};

pub static XREF_DETACH: OpDef = OpDef {
    name: "xref_detach",
    doc:
        "Detach one direct reference, its insertion, and owned dependent records in one undo step.",
    batchable: true,
    params: &[req("id", Ty::Handle, "Direct reference id from xrefs.")],
    rules: &[Rule::RejectUnknown],
    example: r#"{"op":"xref_detach","id":"2A"}"#,
};

pub static XREF_BIND: OpDef = OpDef {
    name: "xref_bind",
    doc: "Bind one direct DWG/DXF xref into a local block definition.",
    batchable: true,
    params: &[req("id", Ty::Handle, "Direct DWG xref id from xrefs.")],
    rules: &[Rule::RejectUnknown],
    example: r#"{"op":"xref_bind","id":"2A"}"#,
};

pub static XREF_SET_PATH: OpDef = OpDef {
    name: "xref_set_path",
    doc: "Set one direct reference's stored path or path type without implicitly reloading it.",
    batchable: true,
    params: &[
        req("id", Ty::Handle, "Direct reference id from xrefs."),
        opt(
            "path",
            Ty::Str,
            "New stored path; mutually exclusive with path_type.",
        ),
        opt(
            "path_type",
            Ty::EnumCi(&["full", "relative", "none"]),
            "Convert stored path style; mutually exclusive with path.",
        ),
    ],
    rules: &[
        Rule::ExactlyOneOf(&["path", "path_type"]),
        Rule::RejectUnknown,
    ],
    example: r#"{"op":"xref_set_path","id":"2A","path_type":"relative"}"#,
};

pub static XREF_SET_TYPE: OpDef = OpDef {
    name: "xref_set_type",
    doc: "Switch one direct DWG/DXF xref between attachment and overlay.",
    batchable: true,
    params: &[
        req("id", Ty::Handle, "Direct DWG xref id from xrefs."),
        req(
            "reference_type",
            Ty::EnumCi(&["attach", "overlay"]),
            "External reference type.",
        ),
    ],
    rules: &[Rule::RejectUnknown],
    example: r#"{"op":"xref_set_type","id":"2A","reference_type":"overlay"}"#,
};

pub static FILE_IDENTITY: OpDef = OpDef {
    name: "file_identity",
    doc: "Inspect or refresh internal drawing UUIDs and fingerprint identity.",
    batchable: true,
    params: &[opt(
        "renew",
        Ty::Bool,
        "Generate fresh document identity tokens.",
    )],
    rules: &[],
    example: r#"{"op":"file_identity"}"#,
};

pub static SAVE: OpDef = OpDef {
    name: "save",
    doc: "Save the drawing to disk, optionally changing format or CAD version.",
    batchable: true,
    params: &[
        opt("path", Ty::Str, "Destination file path."),
        opt(
            "target_format",
            Ty::Enum(&["dwg", "dxf"]),
            "Target CAD format.",
        ),
        opt(
            "target_version",
            Ty::Enum(TARGET_VERSIONS),
            "Target AutoCAD version.",
        ),
        opt(
            "allow_lossy",
            Ty::Bool,
            "Acknowledge dropping unsupported passthrough records.",
        ),
    ],
    rules: &[],
    example: r#"{"op":"save"}"#,
};

pub static SAVE_VERIFIED: OpDef = OpDef {
    name: "save_verified",
    doc: "Save to an explicit path, reload and verify roundtrip semantic equality.",
    batchable: false,
    params: &[
        req("path", Ty::Str, "Destination file path (.dwg or .dxf)."),
        opt(
            "target_format",
            Ty::Enum(&["dwg", "dxf"]),
            "Explicit output format matching extension.",
        ),
        opt(
            "target_version",
            Ty::Enum(TARGET_VERSIONS),
            "Explicit CAD version.",
        ),
        opt(
            "allow_lossy",
            Ty::Bool,
            "Acknowledge dropping unsupported records.",
        ),
        opt(
            "overwrite",
            Ty::Bool,
            "Replace destination if file already exists.",
        ),
    ],
    rules: &[],
    example: r#"{"op":"save_verified","path":"drawing.dwg"}"#,
};

pub static STOP: OpDef = OpDef {
    name: "stop",
    doc: "Irreversible kill switch: disables all automation mutations for the session until manually re-enabled in the desktop GUI.",
    batchable: false,
    params: &[],
    rules: &[],
    example: r#"{"op":"stop"}"#,
};

pub static BATCH: OpDef = OpDef {
    name: "batch",
    doc: "Execute sequential operations with fresh state and step idempotency keys. Stops on first error.",
    batchable: false,
    params: &[
        req("steps", Ty::BatchSteps, "List of batch steps to execute."),
    ],
    rules: &[],
    example: r#"{"op":"batch","steps":[{"op":"run","cmd":"LINE 0,0 10,0"}]}"#,
};

// Read-only operations curated for intent tools. These deliberately live
// outside OPS: OPS is the execute request catalogue and validator.
pub static STATE: OpDef = OpDef {
    name: "state",
    doc: "Inspect the active document, selection, viewport and command state.",
    batchable: false,
    params: &[],
    rules: &[],
    example: r#"{"op":"state"}"#,
};

pub static QUERY: OpDef = OpDef {
    name: "query",
    doc: "Query entities with property, type, layer and spatial filters.",
    batchable: false,
    params: &[
        opt("handles", Ty::ArrayOf(&Ty::Str), "Exact entity handles."),
        opt("type", Ty::Str, "Entity type filter."),
        opt("layer", Ty::Str, "Layer name filter for query."),
        opt(
            "detail",
            Ty::Raw(
                r#"{"type":"string","enum":["summary","geometry","full"],"default":"geometry"}"#,
            ),
            "Entity detail returned by query.",
        ),
        opt(
            "fields",
            Ty::ArrayOf(&Ty::Str),
            "Return only these entity fields plus handle.",
        ),
        opt(
            "where",
            Ty::Raw(
                r#"{"type":"array","description":"All property filters must match.","items":{"type":"object","properties":{"path":{"type":"string"},"op":{"type":"string","enum":["eq","ne","lt","lte","gt","gte","contains","starts_with","ends_with","in","exists","not_exists"],"default":"eq"},"value":{}},"required":["path"],"additionalProperties":false}}"#,
            ),
            "Property filter predicates.",
        ),
        opt(
            "near",
            Ty::Raw(r#"{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":3}"#),
            "Rank planar curves by exact kernel distance to this world XY point.",
        ),
        opt(
            "contains_point",
            Ty::Raw(r#"{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":3}"#),
            "Return closed planar curves containing this world XY point.",
        ),
        opt(
            "bounds",
            Ty::Window,
            "Filter entities whose world XY bounds overlap [min_x,min_y,max_x,max_y].",
        ),
        opt(
            "intersections",
            Ty::Raw(r#"{"type":"array","items":{"type":"string"},"minItems":2,"maxItems":2}"#),
            "Return exact kernel intersections between two planar curve handles.",
        ),
        opt(
            "offset",
            Ty::Raw(r#"{"type":"integer","minimum":0}"#),
            "Number of matching entities to skip.",
        ),
        opt(
            "limit",
            Ty::Raw(r#"{"type":"integer","minimum":1,"maximum":10000}"#),
            "Maximum number of entities to return.",
        ),
    ],
    rules: &[],
    example: r#"{"op":"query","layer":"Walls"}"#,
};

pub static TEXT_SEARCH: OpDef = OpDef {
    name: "text_search",
    doc: "Find drawing text, including text in block definitions.",
    batchable: false,
    params: &[
        req("find", Ty::Str, "Text query string for text_search."),
        opt(
            "match_case",
            Ty::Raw(r#"{"type":"boolean","default":false}"#),
            "Case-sensitive text search.",
        ),
        opt(
            "whole_word",
            Ty::Raw(r#"{"type":"boolean","default":false}"#),
            "Match only whole words.",
        ),
        opt(
            "ignore_accents",
            Ty::Bool,
            "Ignore accents/diacritics in text (defaults to true when match_case is false).",
        ),
        opt(
            "scope",
            Ty::Raw(r#"{"type":"string","enum":["all","active_space","blocks"],"default":"all"}"#),
            "Text search scope.",
        ),
    ],
    rules: &[],
    example: r#"{"op":"text_search","find":"DOOR"}"#,
};

pub static AUDIT: OpDef = OpDef {
    name: "audit",
    doc: "Audit document compatibility for an intended CAD output.",
    batchable: false,
    params: &[
        opt(
            "path",
            Ty::Str,
            "Optional intended output path for audit; extension determines target format.",
        ),
        opt(
            "target_format",
            Ty::Enum(&["dwg", "dxf"]),
            "Intended output format for audit.",
        ),
        opt(
            "target_version",
            Ty::Enum(TARGET_VERSIONS),
            "Intended CAD output version for audit.",
        ),
    ],
    rules: &[],
    example: r#"{"op":"audit","target_format":"dxf","target_version":"2018"}"#,
};

pub static COMMANDS: OpDef = OpDef {
    name: "commands",
    doc: "List available commands and their prompt manifests.",
    batchable: false,
    params: &[opt("name", Ty::Str, "Command name manifest to return.")],
    rules: &[],
    example: r#"{"op":"commands","name":"LINE"}"#,
};

pub static MEASURE: OpDef = OpDef {
    name: "measure",
    doc: "Measure exact curve, bounds and mesh properties for entities.",
    batchable: false,
    params: &[opt(
        "handles",
        Ty::ArrayOf(&Ty::Str),
        "Exact entity handles; omit to measure the selection.",
    )],
    rules: &[],
    example: r#"{"op":"measure","handles":["2A"]}"#,
};

pub static NODE_GRAPH: OpDef = OpDef {
    name: "node_graph",
    doc: "Drive the node graph of the active drawing: list node types (action library), read nodes with their ports, values and links (state), and edit it — add, remove, connect, disconnect, set an input, fold a section or coordinate group (toggle), show/hide the panel, clear, run, save/load an .ocg file. Every action answers with the graph state; edits are one undo step each.",
    batchable: true,
    params: &[
        opt(
            "action",
            Ty::EnumCi(&[
                "library", "state", "show", "hide", "add", "remove", "connect", "disconnect",
                "set", "toggle", "clear", "run", "save", "load",
            ]),
            "What to do; defaults to state.",
        ),
        opt("type", Ty::Str, "Node type to add, as listed by action library (e.g. Line, Point, Move, Selected Objects)."),
        opt(
            "kind",
            Ty::EnumCi(&["object", "operation"]),
            "Which node a type names when it is both (Point: the drawing object or the coordinate).",
        ),
        opt("node", Ty::Int, "Node id for remove, set and toggle."),
        opt(
            "field",
            Ty::Str,
            "Port field for set and toggle: a row (start_x), a coordinate group (start_x|start_y|start_z, value a point) or a section port (section:color, value an object of fields). toggle also takes a section title.",
        ),
        opt("value", Ty::Raw(r#"{"description":"Any JSON value: number, text, boolean, point {x,y,z}, object {handle} or list"}"#), "Value for set."),
        opt("from", Ty::Raw(r#"{"type":"object","properties":{"node":{"type":"integer","minimum":1},"field":{"type":"string"}},"required":["node","field"],"additionalProperties":false}"#), "Output port {node, field} for connect."),
        opt("to", Ty::Raw(r#"{"type":"object","properties":{"node":{"type":"integer","minimum":1},"field":{"type":"string"}},"required":["node","field"],"additionalProperties":false}"#), "Input port {node, field} for connect and disconnect."),
        opt("position", Ty::Point, "Canvas position of an added node."),
        opt("path", Ty::Str, "Absolute .ocg path for save and load."),
    ],
    rules: &[
        Rule::RequiredWhen { key: "action", equals: "add", then: &["type"] },
        Rule::RequiredWhen { key: "action", equals: "remove", then: &["node"] },
        Rule::RequiredWhen { key: "action", equals: "connect", then: &["from", "to"] },
        Rule::RequiredWhen { key: "action", equals: "disconnect", then: &["to"] },
        Rule::RequiredWhen { key: "action", equals: "set", then: &["node", "field", "value"] },
        Rule::RequiredWhen { key: "action", equals: "toggle", then: &["node", "field"] },
        Rule::RequiredWhen { key: "action", equals: "save", then: &["path"] },
        Rule::RequiredWhen { key: "action", equals: "load", then: &["path"] },
    ],
    example: r#"{"op":"node_graph","action":"add","type":"Line"}"#,
};

pub static OPS: &[&OpDef] = &[
    &NEW,
    &OPEN,
    &ACTIVATE,
    &SWITCH_DOCUMENT,
    &RUN,
    &START,
    &INPUT,
    &CANCEL,
    &UNDO,
    &REDO,
    &SELECT,
    &PROPERTY,
    &GRIP_DRAG,
    &CLICK,
    &POINTER,
    &SET_PROPERTIES,
    &PANEL_INPUT,
    &PANEL_BUTTON,
    &ACTION,
    &EMBED_IMAGE,
    &DGN_IMPORT,
    &WBLOCK,
    &PLOT,
    &ENTITIES_CREATE,
    &ENTITIES_SET,
    &ENTITIES_DELETE,
    &ENTITIES_TRANSFORM,
    &NODE_GRAPH,
    &TEXT_REPLACE,
    &BLOCK_DEFINE,
    &BLOCK_DELETE,
    &XDATA_SET,
    &VIEW_FOCUS,
    &ENTITIES_COPY_TO,
    &GROUP_CREATE,
    &SELECTION_SET_SAVE,
    &SELECTION_SET_LOAD,
    &USER_SELECT,
    &GETPOINT,
    &CLOSE,
    &SYSVAR,
    &LAYOUT_CREATE,
    &LAYOUT_RENAME,
    &LAYOUT_ACTIVATE,
    &LAYOUT_DELETE,
    &PAGE_SETUP_SET,
    &XREF_RELOAD,
    &XREF_UNLOAD,
    &XREF_DETACH,
    &XREF_BIND,
    &XREF_SET_PATH,
    &XREF_SET_TYPE,
    &FILE_IDENTITY,
    &SAVE,
    &SAVE_VERIFIED,
    &STOP,
    &BATCH,
];

pub fn find_op(name: &str) -> Option<&'static OpDef> {
    OPS.iter().copied().find(|op| op.name == name)
}

// ============================================================================
// SCHEMA EMITTERS
// ============================================================================

fn ty_schema(ty: &Ty) -> Value {
    match ty {
        Ty::Str => json!({"type": "string"}),
        Ty::Bool => json!({"type": "boolean"}),
        Ty::Num => json!({"type": "number"}),
        Ty::Int => json!({"type": "integer"}),
        Ty::Point => json!({"$ref": "#/$defs/point"}),
        Ty::Handle => json!({"$ref": "#/$defs/handle"}),
        Ty::Handles => json!({"type": "array", "minItems": 1, "items": {"$ref": "#/$defs/handle"}}),
        Ty::Window => json!({"$ref": "#/$defs/window"}),
        Ty::Enum(v) | Ty::EnumCi(v) => json!({"type": "string", "enum": v}),
        Ty::ActionName => {
            let names = crate::app::automation_action_names();
            json!({"type": "string", "enum": names})
        }
        Ty::ArrayOf(inner) => json!({"type": "array", "items": ty_schema(inner)}),
        Ty::BatchSteps => {
            json!({"type":"array","minItems":1,"maxItems":64,"items":{"$ref":"#/$defs/batch_step"}})
        }
        Ty::Entities => {
            json!({"type":"array","minItems":1,"items":{"type":"object","required":["type"],"properties":{"type":{"type":"string"}}}})
        }
        Ty::Raw(s) => serde_json::from_str(s).expect("Raw fragment must be valid JSON"),
    }
}

pub(crate) fn primitive_defs() -> Value {
    json!({
        "point": {"type":"array","items":{"type":"number"},"minItems":2,"maxItems":3},
        "window": {"type":"array","items":{"type":"number"},"minItems":4,"maxItems":4},
        "handle": {"type":"string","pattern":"^(0[xX])?[0-9A-Fa-f]+$"}
    })
}

pub fn shared_defs() -> Value {
    let mut defs = primitive_defs();
    defs["batch_step"] = batch_step_schema();
    for op in OPS {
        defs[format!("op_{}", op.name)] = op_schema(op);
    }
    defs
}

fn op_ref(op: &OpDef) -> Value {
    json!({"$ref": format!("#/$defs/op_{}", op.name)})
}

fn rules_text(op: &OpDef) -> String {
    let mut out = String::new();
    for r in op.rules {
        match r {
            Rule::AnyOf(keys) => {
                out.push_str(&format!(" Requires at least one of: {}.", keys.join(", ")));
            }
            Rule::ExactlyOneOf(keys) => {
                out.push_str(&format!(" Requires exactly one of: {}.", keys.join(", ")));
            }
            Rule::RejectUnknown => {}
            Rule::RequiredWhen { key, equals, then } => {
                out.push_str(&format!(
                    " When {key}={equals}, requires: {}.",
                    then.join(", ")
                ));
            }
        }
    }
    out
}

pub fn op_schema(op: &OpDef) -> Value {
    let mut props = Map::new();
    props.insert("op".into(), json!({"enum": [op.name]}));
    for p in op.params {
        let mut s = ty_schema(&p.ty);
        if !p.doc.is_empty() {
            s["description"] = json!(p.doc);
        }
        props.insert(p.name.into(), s);
    }
    let mut required = vec!["op"];
    required.extend(op.params.iter().filter(|p| p.required).map(|p| p.name));
    let mut schema = json!({
        "type": "object",
        "description": format!("{}{}", op.doc, rules_text(op)),
        "properties": props,
        "required": required,
    });
    for rule in op.rules {
        if let Rule::ExactlyOneOf(keys) = rule {
            schema["oneOf"] = keys
                .iter()
                .map(|key| json!({"required":[key]}))
                .collect::<Vec<_>>()
                .into();
        }
    }
    schema
}

/// The common envelope is optional in static preflight; dispatch requires an id.
/// Shared with the advertised execute schema so constraints cannot drift.
fn request_envelope_schema() -> Value {
    json!({
        "type":"object",
        "properties": {
            "op": {"type":"string","enum": OPS.iter().map(|op| op.name).collect::<Vec<_>>(),"description":"Semantic editor operation."},
            "request_id": {"type":"string","minLength":1,"maxLength":128,"description":"Caller-generated idempotency key (at most 128 UTF-8 bytes)."},
            "document_id": {"type":"integer","minimum":0,"description":"Target document from current state."},
            "revision": {"type":"integer","minimum":0,"description":"Expected edit revision from current state."},
            "geometry_revision": {"type":"integer","minimum":0,"description":"Expected geometry revision."},
            "camera_revision": {"type":"integer","minimum":0,"description":"Expected camera revision."},
            "selection": {"type":"array","items":{"$ref":"#/$defs/handle"},"description":"Expected selected handles."}
        },
        "$defs":primitive_defs()
    })
}

pub fn execute_request_schema() -> Value {
    let mut schema = request_envelope_schema();
    schema["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
    schema["properties"]["steps"] = json!({"type":"array","minItems":1,"maxItems":64,"description":"Sequential batch operations.","items":{"$ref":"#/$defs/batch_step"}});
    schema["required"] = json!(["op", "request_id"]);
    schema["anyOf"] = Value::Array(OPS.iter().map(|op| op_ref(op)).collect());
    schema["$defs"] = shared_defs();
    schema
}

pub fn batch_step_schema() -> Value {
    let mut batch_variants: Vec<Value> = OPS
        .iter()
        .filter(|op| op.batchable)
        .map(|op| op_ref(op))
        .collect();

    batch_variants.push(json!({
        "required":["bindings"],
        "properties":{
            "op":{"enum":OPS.iter().filter(|op| op.batchable).map(|op| op.name).collect::<Vec<_>>()},
            "bindings":{"type":"object","minProperties":1,
                "description":"Bind an operation parameter to an earlier completed step response using a zero-based step and RFC 6901 path. Validated again after resolution.",
                "additionalProperties":{"type":"object","required":["step","path"],"additionalProperties":false,
                    "properties":{"step":{"type":"integer","minimum":0},"path":{"type":"string","pattern":"^(|/.*)$"}}}}
        }
    }));

    json!({
        "type": "object",
        "properties": {
            "op": {"type": "string"},
            "document_id": {"type": "integer", "minimum": 0}
        },
        "required": ["op"],
        "anyOf": batch_variants
    })
}

// ============================================================================
// PRE-DISPATCH VALIDATOR
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationResult {
    pub warnings: Vec<String>,
}

fn check_point(val: &Value) -> bool {
    val.as_array().is_some_and(|arr| {
        (2..=3).contains(&arr.len()) && arr.iter().all(|v| v.as_f64().is_some_and(f64::is_finite))
    })
}

fn check_handle(val: &Value) -> bool {
    let Some(s) = val.as_str() else { return false };
    let hex = s.trim_start_matches("0x").trim_start_matches("0X");
    !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit())
}

fn check_window(val: &Value) -> bool {
    val.as_array().is_some_and(|arr| {
        arr.len() == 4 && arr.iter().all(|v| v.as_f64().is_some_and(f64::is_finite))
    })
}

fn validate_type(ty: &Ty, val: &Value) -> Result<(), String> {
    match ty {
        Ty::Str => {
            if val.as_str().is_none() {
                return Err("expected string".into());
            }
        }
        Ty::Bool => {
            if val.as_bool().is_none() {
                return Err("expected boolean".into());
            }
        }
        Ty::Num => {
            if !val.as_f64().is_some_and(f64::is_finite) {
                return Err("expected finite number".into());
            }
        }
        Ty::Int => {
            if val.as_i64().is_none() && val.as_u64().is_none() {
                return Err("expected integer".into());
            }
        }
        Ty::Point => {
            if !check_point(val) {
                return Err("expected [x,y] or [x,y,z] point".into());
            }
        }
        Ty::Handle => {
            if !check_handle(val) {
                return Err("expected hexadecimal handle string".into());
            }
        }
        Ty::Handles => {
            let Some(arr) = val.as_array() else {
                return Err("expected array of handles".into());
            };
            if arr.is_empty() {
                return Err("handles array must not be empty".into());
            }
            if !arr.iter().all(check_handle) {
                return Err("all items in handles must be valid hexadecimal handle strings".into());
            }
        }
        Ty::Window => {
            if !check_window(val) {
                return Err("expected [x0, y0, x1, y1] window array".into());
            }
        }
        Ty::Enum(allowed) => {
            let Some(s) = val.as_str() else {
                return Err("expected string".into());
            };
            if !allowed.contains(&s) {
                return Err(format!("expected one of: {}", allowed.join(", ")));
            }
        }
        Ty::EnumCi(allowed) => {
            let Some(s) = val.as_str() else {
                return Err("expected string".into());
            };
            if !allowed.iter().any(|a| a.eq_ignore_ascii_case(s)) {
                return Err(format!("expected one of: {}", allowed.join(", ")));
            }
        }
        Ty::ActionName => {
            let Some(s) = val.as_str() else {
                return Err("expected action name string".into());
            };
            if !crate::app::automation_action_names().contains(&s) {
                return Err(format!("unknown action name: '{s}'"));
            }
        }
        Ty::ArrayOf(inner) => {
            let Some(arr) = val.as_array() else {
                return Err("expected array".into());
            };
            for (idx, item) in arr.iter().enumerate() {
                validate_type(inner, item).map_err(|e| format!("item {idx}: {e}"))?;
            }
        }
        Ty::Entities => {
            let entities = val
                .as_array()
                .filter(|a| !a.is_empty())
                .ok_or("expected nonempty entities array")?;
            for (index, entity) in entities.iter().enumerate() {
                crate::entity_creation::validate_entity(entity, &format!("/entities/{index}"))?;
            }
        }
        Ty::BatchSteps => {
            validate_execute_request(&json!({"op":"batch","steps":val}), "batch")?;
        }
        Ty::Raw(fragment) => {
            let schema: Value = serde_json::from_str(fragment).expect("valid inline schema");
            crate::schema_validation::validate_shape(&schema, val, "")?;
        }
    }
    Ok(())
}

fn validate_parameter(ty: &Ty, val: &Value, path: &str) -> Result<(), String> {
    match ty {
        Ty::ArrayOf(inner) => {
            let items = val
                .as_array()
                .ok_or_else(|| format!("{path}: expected array"))?;
            for (i, item) in items.iter().enumerate() {
                validate_parameter(inner, item, &format!("{path}/{i}"))?;
            }
            Ok(())
        }
        Ty::Raw(fragment) => {
            let schema: Value = serde_json::from_str(fragment).expect("valid inline schema");
            crate::schema_validation::validate_shape(&schema, val, path)
        }
        Ty::Entities => validate_type(ty, val),
        _ => validate_type(ty, val).map_err(|error| format!("{path}: {error}")),
    }
}

pub fn validate_request(request: &Value, is_batch_step: bool) -> Result<ValidationResult, String> {
    validate_request_deferred(request, is_batch_step, &[])
}

pub fn validate_request_deferred(
    request: &Value,
    is_batch_step: bool,
    deferred: &[&str],
) -> Result<ValidationResult, String> {
    let Some(obj) = request.as_object() else {
        return Err("request must be a JSON object".into());
    };

    let Some(op_name) = obj.get("op").and_then(Value::as_str) else {
        return Err("missing 'op' string in request".into());
    };

    let Some(op_def) = find_op(op_name) else {
        return Err(format!("unknown operation: '{op_name}'"));
    };

    if is_batch_step && !op_def.batchable {
        return Err(format!(
            "operation '{op_name}' cannot be used in a batch step"
        ));
    }

    // Check required params
    for p in op_def.params {
        if p.required && !deferred.contains(&p.name) {
            let val = obj.get(p.name);
            if val.is_none() || val.unwrap().is_null() {
                return Err(format!(
                    "Missing {} for {}. Example request: {}",
                    p.name, op_name, op_def.example
                ));
            }
        }
    }

    // Check rules
    for r in op_def.rules {
        match r {
            Rule::AnyOf(keys) => {
                let any_present = keys
                    .iter()
                    .any(|k| deferred.contains(k) || obj.get(*k).is_some_and(|v| !v.is_null()));
                if !any_present {
                    return Err(format!(
                        "Missing {} for {}. Example request: {}",
                        keys.join(" or "),
                        op_name,
                        op_def.example
                    ));
                }
            }
            Rule::ExactlyOneOf(keys) => {
                let present = keys
                    .iter()
                    .filter(|key| deferred.contains(*key) || obj.contains_key(**key))
                    .count();
                let explicit_null = keys.iter().any(|key| {
                    !deferred.contains(key) && obj.get(*key).is_some_and(Value::is_null)
                });
                if present != 1 || explicit_null {
                    return Err(format!(
                        "Supply exactly one of {} for {}. Example request: {}",
                        keys.join(", "),
                        op_name,
                        op_def.example
                    ));
                }
            }
            Rule::RejectUnknown => {}
            Rule::RequiredWhen { key, equals, then } => {
                let matches = obj
                    .get(*key)
                    .and_then(Value::as_str)
                    .is_some_and(|v| v.eq_ignore_ascii_case(equals));
                if matches {
                    for req_key in *then {
                        let val = obj.get(*req_key);
                        if !deferred.contains(req_key) && (val.is_none() || val.unwrap().is_null())
                        {
                            return Err(format!(
                                "Missing {} for {}. Example request: {}",
                                req_key, op_name, op_def.example
                            ));
                        }
                    }
                }
            }
        }
    }

    // Validate types for present parameters
    for p in op_def.params {
        if let Some(val) = obj.get(p.name) {
            if !val.is_null() {
                validate_parameter(&p.ty, val, &format!("/{}", p.name))
                    .map_err(|e| format!("parameter '{}' for op '{}': {e}", p.name, op_name))?;
            }
        }
    }

    // Legacy operations warn on unknown keys; explicitly strict operations
    // reject them so a typo cannot become a silent no-op.
    let reject_unknown = op_def
        .rules
        .iter()
        .any(|rule| matches!(rule, Rule::RejectUnknown));
    let mut warnings = Vec::new();
    let mut allowed: Vec<&str> = op_def.params.iter().map(|p| p.name).collect();
    if is_batch_step {
        allowed.extend(["op", "document_id"]);
    } else {
        allowed.extend(ENVELOPE_KEYS.iter().copied());
    }
    if is_batch_step && reject_unknown {
        allowed.push("bindings");
    }

    for (k, _) in obj {
        if !allowed.contains(&k.as_str()) {
            if reject_unknown {
                return Err(format!("unknown parameter '{k}' for op '{op_name}'"));
            }
            if k == "data_base64" {
                warnings.push(
                    "data_base64 is supported only in the Web/WASM build, not in native desktop"
                        .into(),
                );
            } else {
                warnings.push(format!(
                    "unknown parameter '{k}' ignored for op '{op_name}'"
                ));
            }
        }
    }

    Ok(ValidationResult { warnings })
}

const MAX_BATCH_STEPS: usize = 64;

/// Validate bindings without dispatching or substituting untyped placeholder values.
pub fn batch_bindings(step: &Value, index: usize) -> Result<Vec<&str>, String> {
    let Some(bindings) = step.get("bindings") else {
        return Ok(Vec::new());
    };
    let bindings = bindings
        .as_object()
        .filter(|v| !v.is_empty())
        .ok_or("bindings must be a nonempty object")?;
    let op = find_op(step["op"].as_str().unwrap_or_default()).ok_or("unknown bound operation")?;
    for (target, source) in bindings {
        if !op.params.iter().any(|p| p.name == target) {
            return Err(format!("cannot bind parameter '{target}'"));
        }
        if step.get(target).is_some() {
            return Err(format!("'{target}' has both a literal and a binding"));
        }
        let source = source
            .as_object()
            .ok_or("binding must contain step and path")?;
        if source.len() != 2
            || source
                .get("step")
                .and_then(Value::as_u64)
                .is_none_or(|s| s >= index as u64)
        {
            return Err(
                "binding must refer to an earlier step and contain only step and path".into(),
            );
        }
        let path = source
            .get("path")
            .and_then(Value::as_str)
            .ok_or("binding path must be a JSON Pointer")?;
        if !path.is_empty() && !path.starts_with('/') {
            return Err("binding path must be a JSON Pointer".into());
        }
        let mut chars = path.chars();
        while let Some(c) = chars.next() {
            if c == '~' && !matches!(chars.next(), Some('0' | '1')) {
                return Err("invalid JSON Pointer escape".into());
            }
        }
    }
    Ok(bindings.keys().map(String::as_str).collect())
}

pub fn validate_execute_request(request: &Value, op: &str) -> Result<ValidationResult, String> {
    crate::schema_validation::validate_shape(&request_envelope_schema(), request, "")?;
    if request["request_id"]
        .as_str()
        .is_some_and(|id| id.len() > 128)
    {
        return Err("/request_id: must not exceed 128 UTF-8 bytes".into());
    }
    if op == "batch" {
        let steps = request["steps"].as_array().ok_or_else(|| {
            r#"Missing steps for batch. Example request: {"op":"batch","request_id":"draw-1","steps":[{"op":"run","cmd":"LINE 0,0 10,0"}]}"#.to_string()
        })?;
        if steps.is_empty() || steps.len() > MAX_BATCH_STEPS {
            return Err(format!(
                "batch steps must contain 1 to {MAX_BATCH_STEPS} operations"
            ));
        }
        let mut warnings = Vec::new();
        for (index, step) in steps.iter().enumerate() {
            let step_obj = step
                .as_object()
                .ok_or_else(|| format!("batch step {index} must be an object"))?;
            if step_obj.contains_key("request_id") {
                return Err(format!(
                    "batch step {index} must omit request_id; the batch assigns idempotency keys"
                ));
            }
            let step_op = step_obj
                .get("op")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("batch step {index} is missing op"))?;
            if !find_op(step_op).is_some_and(|op| op.batchable) {
                return Err(format!("Unknown operation {step_op} in batch step {index}"));
            }
            let deferred = batch_bindings(step, index)
                .map_err(|error| format!("batch step {index}: {error}"))?;
            let mut literal = step.clone();
            literal.as_object_mut().unwrap().remove("bindings");
            crate::schema_validation::validate_shape(
                &request_envelope_schema(),
                &literal,
                &format!("/steps/{index}"),
            )?;
            let step_val = validate_request_deferred(&literal, true, &deferred)
                .map_err(|error| format!("batch step {index}: {error}"))?;
            for w in step_val.warnings {
                warnings.push(format!("batch step {index}: {w}"));
            }
        }
        return Ok(ValidationResult { warnings });
    }
    validate_request(request, false)
}
