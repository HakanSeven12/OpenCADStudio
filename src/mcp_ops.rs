//! Declarative description of every MCP op. Single source of truth for the
//! advertised JSON Schema, the pre-dispatch validator, and test examples.

use serde_json::{json, Map, Value};

pub const TARGET_VERSIONS: &[&str] = &[
    "R14", "2000", "2004", "2007", "2010", "2013", "2018",
    "AC1014", "AC1015", "AC1018", "AC1021", "AC1024", "AC1027", "AC1032",
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
    /// Escape hatch: a JSON Schema fragment as text.
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
    Param { name, ty, required: true, doc }
}

const fn opt(name: &'static str, ty: Ty, doc: &'static str) -> Param {
    Param { name, ty, required: false, doc }
}

// ============================================================================
// OP DEFINITIONS
// ============================================================================

pub static NEW: OpDef = OpDef {
    name: "new",
    doc: "Start an empty document, optionally seeded from a DWG/DXF/DWT template.",
    batchable: true,
    params: &[
        opt("template", Ty::Str, "Optional template drawing path whose styles and tables survive."),
    ],
    rules: &[],
    example: r#"{"op":"new"}"#,
};

pub static OPEN: OpDef = OpDef {
    name: "open",
    doc: "Open an existing drawing file (DWG or DXF).",
    batchable: true,
    params: &[
        req("path", Ty::Str, "Absolute path to the drawing file."),
    ],
    rules: &[],
    example: r#"{"op":"open","path":"drawing.dxf"}"#,
};

pub static ACTIVATE: OpDef = OpDef {
    name: "activate",
    doc: "Switch the active drawing tab to another open document.",
    batchable: true,
    params: &[
        req("document_id", Ty::Int, "Target document ID to activate."),
    ],
    rules: &[],
    example: r#"{"op":"activate","document_id":1}"#,
};

pub static SWITCH_DOCUMENT: OpDef = OpDef {
    name: "switch_document",
    doc: "Alias for activate. Switch the active drawing tab to another open document.",
    batchable: true,
    params: &[
        req("document_id", Ty::Int, "Target document ID to activate."),
    ],
    rules: &[],
    example: r#"{"op":"switch_document","document_id":1}"#,
};

pub static RUN: OpDef = OpDef {
    name: "run",
    doc: "Execute a command string with prompt answers separated by spaces (e.g. 'LINE 0,0 10,10').",
    batchable: true,
    params: &[
        req("cmd", Ty::Str, "Command name and arguments."),
    ],
    rules: &[],
    example: r#"{"op":"run","cmd":"LINE 0,0 10,10"}"#,
};

pub static START: OpDef = OpDef {
    name: "start",
    doc: "Start an interactive command waiting for subsequent 'input' steps.",
    batchable: true,
    params: &[
        req("cmd", Ty::Str, "Command name to start."),
    ],
    rules: &[],
    example: r#"{"op":"start","cmd":"LINE"}"#,
};

pub static INPUT: OpDef = OpDef {
    name: "input",
    doc: "Feed the next step input to an active command.",
    batchable: true,
    params: &[
        req("kind", Ty::Enum(&["text", "token", "point", "entity", "structure", "selection", "enter"]), "Input kind."),
        opt("text", Ty::Str, "Free text or token option."),
        opt("point", Ty::Point, "Point coordinates."),
        opt("space", Ty::Enum(&["wcs", "ucs", "relative"]), "Coordinate space for point (defaults to wcs)."),
        opt("handle", Ty::Handle, "Target entity handle for entity/structure pick."),
    ],
    rules: &[
        Rule::RequiredWhen { key: "kind", equals: "token", then: &["text"] },
        Rule::RequiredWhen { key: "kind", equals: "point", then: &["point"] },
        Rule::RequiredWhen { key: "kind", equals: "entity", then: &["handle", "point"] },
        Rule::RequiredWhen { key: "kind", equals: "structure", then: &["handle", "point"] },
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
        req("value", Ty::Raw(r#"{"description":"New property value"}"#), "New value for the field."),
    ],
    rules: &[],
    example: r#"{"op":"property","field":"color","value":1}"#,
};

pub static GRIP_DRAG: OpDef = OpDef {
    name: "grip_drag",
    doc: "Drag one grip of an entity to a world point the way a grip pick-and-place in the viewport does. Read grips for the ids; object snaps, ortho and polar are off for the move.",
    batchable: true,
    params: &[
        req("handle", Ty::Handle, "Entity whose grip moves."),
        req("grip_id", Ty::Int, "Grip id from the grips read."),
        req("point", Ty::Point, "World point [x, y, z] the grip moves to."),
    ],
    rules: &[],
    example: r#"{"op":"grip_drag","handle":"2A","grip_id":0,"point":[10,0,0]}"#,
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

pub static SET_PROPERTIES: OpDef = OpDef {
    name: "set_properties",
    doc: "Atomically update record properties by RFC 6901 JSON pointer paths.",
    batchable: true,
    params: &[
        req("collection", Ty::Str, "Record collection name."),
        opt("handle", Ty::Handle, "Target record handle if applicable."),
        req("updates", Ty::ArrayOf(&Ty::Raw(r#"{"type":"object","properties":{"path":{"type":"string"},"value":{},"expected":{}},"required":["path","value"]}"#)), "List of pointer replacements."),
    ],
    rules: &[],
    example: r#"{"op":"set_properties","collection":"entities","handle":"2A","updates":[{"path":"/common/layer","value":"Walls"}]}"#,
};

pub static ACTION: OpDef = OpDef {
    name: "action",
    doc: "Trigger a named GUI action or dialog command.",
    batchable: true,
    params: &[
        req("name", Ty::ActionName, "Name of the UI action to execute."),
    ],
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

pub static WBLOCK: OpDef = OpDef {
    name: "wblock",
    doc: "Export selected entities or a block definition to an external DWG/DXF file.",
    batchable: true,
    params: &[
        req("path", Ty::Str, "Target drawing path for export."),
        opt("handles", Ty::Handles, "Entity handles to export."),
        opt("block", Ty::Str, "Block name to export."),
        opt("template", Ty::Str, "Base template file to carry styles and tables."),
        opt("normalize", Ty::Bool, "Translate exported entities to origin [0,0,0]."),
    ],
    rules: &[],
    example: r#"{"op":"wblock","path":"part.dwg","handles":["2A"]}"#,
};

pub static PLOT: OpDef = OpDef {
    name: "plot",
    doc: "Plot layouts to a PDF file. scale (e.g. \"1:100\") overrides fit; fit defaults to true when scale is omitted.",
    batchable: true,
    params: &[
        req("path", Ty::Str, "Output PDF file path (must end in .pdf)."),
        opt("layout", Ty::Str, "Target layout name or 'all' (defaults to 'Model')."),
        opt("area", Ty::EnumCi(&["extents", "display", "limits", "layout", "window"]), "Plot area (defaults to extents)."),
        opt("window", Ty::Window, "World [x0, y0, x1, y1] rectangle; required when area=window."),
        opt("paper", Ty::Str, "Canonical paper size name from paper catalog."),
        opt("orientation", Ty::EnumCi(&["Portrait", "Landscape"]), "Sheet orientation."),
        opt("fit", Ty::Bool, "Fit drawing to sheet (defaults to true if scale omitted)."),
        opt("scale", Ty::Str, "Plot scale as paper:drawing ratio (e.g. \"1:100\")."),
        opt("center", Ty::Bool, "Center plot on sheet (defaults to true)."),
        opt("offset_x", Ty::Num, "Plot offset X in mm."),
        opt("offset_y", Ty::Num, "Plot offset Y in mm."),
        opt("upside_down", Ty::Bool, "Rotate content 180 degrees."),
        opt("plot_style", Ty::Str, "CTB file path or named plot style table."),
        opt("transparency", Ty::Bool, "Honor transparency settings."),
        opt("lineweights", Ty::Bool, "Honor lineweight settings."),
        opt("merge_lines", Ty::Bool, "Merge overlapping lines."),
        opt("stamp", Ty::Bool, "Print plot stamp."),
        opt("per_page", Ty::Bool, "When layout='all', write one PDF per layout as <stem>-<Layout>.pdf."),
    ],
    rules: &[Rule::RequiredWhen { key: "area", equals: "window", then: &["window"] }],
    example: r#"{"op":"plot","path":"out.pdf","area":"extents","scale":"1:100"}"#,
};

pub static ENTITIES_CREATE: OpDef = OpDef {
    name: "entities_create",
    doc: "Create new CAD entities from typed definitions.",
    batchable: true,
    params: &[
        req("entities", Ty::ArrayOf(&Ty::Raw(r#"{"type":"object","properties":{"type":{"type":"string"}},"required":["type"]}"#)), "List of entity definitions to create."),
    ],
    rules: &[],
    example: r#"{"op":"entities_create","entities":[{"type":"Line","start":[0,0],"end":[10,10]}]}"#,
};

pub static ENTITIES_DELETE: OpDef = OpDef {
    name: "entities_delete",
    doc: "Delete entities by their handles.",
    batchable: true,
    params: &[
        req("handles", Ty::Handles, "Handles of entities to delete."),
    ],
    rules: &[],
    example: r#"{"op":"entities_delete","handles":["2A"]}"#,
};

pub static ENTITIES_TRANSFORM: OpDef = OpDef {
    name: "entities_transform",
    doc: "Move, copy, rotate, scale, mirror or array entities.",
    batchable: true,
    params: &[
        req("handles", Ty::Handles, "Entities to transform."),
        opt("action", Ty::EnumCi(&["move", "copy", "rotate", "scale", "mirror", "array"]), "Transform action (defaults to move)."),
        opt("vector", Ty::Point, "move/copy: displacement [dx, dy] or [dx, dy, dz]."),
        opt("center", Ty::Point, "rotate/scale: base point."),
        opt("angle_deg", Ty::Num, "rotate: angle in degrees CCW."),
        opt("factor", Ty::Num, "scale: non-zero scale factor."),
        opt("axis", Ty::ArrayOf(&Ty::Point), "mirror: axis as two points [[x1, y1], [x2, y2]]."),
        opt("copy", Ty::Bool, "mirror/copy: keep originals and return new copies."),
        opt("rows", Ty::Int, "array: row count (1..100, defaults to 1)."),
        opt("columns", Ty::Int, "array: column count (1..100, defaults to 1)."),
        opt("row_spacing", Ty::Num, "array: row spacing."),
        opt("column_spacing", Ty::Num, "array: column spacing."),
    ],
    rules: &[
        Rule::RequiredWhen { key: "action", equals: "scale", then: &["factor"] },
        Rule::RequiredWhen { key: "action", equals: "mirror", then: &["axis"] },
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
        opt("insert_at", Ty::Point, "Placement coordinates for the replacement Insert reference (defaults to base)."),
        opt("replace", Ty::Bool, "Overwrite an existing block definition of the same name."),
    ],
    rules: &[],
    example: r#"{"op":"block_define","name":"DOOR_90","handles":["2A","2B"],"base":[0,0]}"#,
};

pub static BLOCK_DELETE: OpDef = OpDef {
    name: "block_delete",
    doc: "Delete a block definition and all its child entities and inserts.",
    batchable: true,
    params: &[
        req("name", Ty::Str, "Block name to delete."),
    ],
    rules: &[],
    example: r#"{"op":"block_delete","name":"DOOR"}"#,
};

pub static XDATA_SET: OpDef = OpDef {
    name: "xdata_set",
    doc: "Attach or clear extended entity data (XData) for a registered application.",
    batchable: true,
    params: &[
        req("app", Ty::Str, "Application name."),
        req("handles", Ty::Handles, "Target entity handles."),
        opt("data", Ty::ArrayOf(&Ty::Raw(r#"{"type":"object","properties":{"code":{"type":"integer"},"value":{}},"required":["code","value"]}"#)), "Typed XData entries [{code, value}]. Empty list removes app record."),
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
        opt("highlight", Ty::Bool, "Also select the entities (defaults to true)."),
    ],
    rules: &[],
    example: r#"{"op":"view_focus","handles":["2A"]}"#,
};

pub static ENTITIES_COPY_TO: OpDef = OpDef {
    name: "entities_copy_to",
    doc: "Copy entities from the current document into another open document.",
    batchable: true,
    params: &[
        req("handles", Ty::Handles, "Entities in the current active document to copy."),
        opt("target_document_id", Ty::Int, "Target document ID to receive the copied entities."),
        opt("document_id", Ty::Int, "Deprecated alias for target_document_id."),
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
        opt("select", Ty::Bool, "Also select the recalled entities in the viewport (defaults to true)."),
    ],
    rules: &[],
    example: r#"{"op":"selection_set_load","name":"Sel1"}"#,
};

pub static USER_SELECT: OpDef = OpDef {
    name: "user_select",
    doc: "Prompt the user at the desktop screen to interactively pick entities.",
    batchable: true,
    params: &[
        opt("prompt", Ty::Str, "Prompt text pinned on screen while user picks."),
        opt("detail", Ty::EnumCi(&["summary", "geometry", "full"]), "Returned entity detail level (defaults to full)."),
        opt("type", Ty::Str, "Entity type filter."),
        opt("layer", Ty::Str, "Layer filter."),
        opt("clear", Ty::Bool, "Clear current selection before pick begins (defaults to true)."),
    ],
    rules: &[],
    example: r#"{"op":"user_select","prompt":"Pick entities"}"#,
};

pub static GETPOINT: OpDef = OpDef {
    name: "getpoint",
    doc: "Prompt the user at the desktop screen to click one point in the viewport.",
    batchable: true,
    params: &[
        opt("prompt", Ty::Str, "Prompt text displayed while waiting for click."),
    ],
    rules: &[],
    example: r#"{"op":"getpoint","prompt":"Click start point"}"#,
};

pub static CLOSE: OpDef = OpDef {
    name: "close",
    doc: "Close the active drawing document.",
    batchable: true,
    params: &[
        opt("discard", Ty::Bool, "Erase unsaved changes instead of refusing a dirty document."),
    ],
    rules: &[],
    example: r#"{"op":"close"}"#,
};

pub static SYSVAR: OpDef = OpDef {
    name: "sysvar",
    doc: "Read or write CAD system variables (ltscale, pdmode, pdsize, clayer, etc.).",
    batchable: true,
    params: &[
        opt("get", Ty::ArrayOf(&Ty::Str), "List of variable names to read."),
        opt("set", Ty::Raw(r#"{"type":"object","description":"Variable name=value pairs to set"}"#), "Key-value map of variables to set in one undo transaction."),
    ],
    rules: &[],
    example: r#"{"op":"sysvar","get":["LTSCALE"]}"#,
};

pub static LAYOUT_CREATE: OpDef = OpDef {
    name: "layout_create",
    doc: "Create a new paper-space layout tab.",
    batchable: true,
    params: &[
        req("name", Ty::Str, "Layout tab name."),
    ],
    rules: &[],
    example: r#"{"op":"layout_create","name":"Sheet1"}"#,
};

pub static PAGE_SETUP_SET: OpDef = OpDef {
    name: "page_setup_set",
    doc: "Configure page setup / print settings for a layout.",
    batchable: true,
    params: &[
        req("layout", Ty::Str, "Target layout name."),
        opt("paper", Ty::Str, "Paper size name."),
        opt("orientation", Ty::EnumCi(&["Portrait", "Landscape"]), "Paper orientation."),
        opt("fit", Ty::Bool, "Fit to paper (defaults to true)."),
        opt("scale", Ty::Str, "Paper-to-drawing scale ratio."),
        opt("center", Ty::Bool, "Center drawing on sheet (defaults to true)."),
        opt("offset_x", Ty::Num, "X offset in mm."),
        opt("offset_y", Ty::Num, "Y offset in mm."),
        opt("plot_style", Ty::Str, "CTB file path or named style."),
        opt("area", Ty::EnumCi(&["extents", "display", "limits", "layout", "window"]), "Plot area."),
        opt("window", Ty::Window, "World [x0, y0, x1, y1] rectangle; required when area=window."),
    ],
    rules: &[Rule::RequiredWhen { key: "area", equals: "window", then: &["window"] }],
    example: r#"{"op":"page_setup_set","layout":"Sheet1","paper":"A4"}"#,
};

pub static FILE_IDENTITY: OpDef = OpDef {
    name: "file_identity",
    doc: "Inspect or refresh internal drawing UUIDs and fingerprint identity.",
    batchable: true,
    params: &[
        opt("renew", Ty::Bool, "Generate fresh document identity tokens."),
    ],
    rules: &[],
    example: r#"{"op":"file_identity"}"#,
};

pub static SAVE: OpDef = OpDef {
    name: "save",
    doc: "Save the drawing to disk, optionally changing format or CAD version.",
    batchable: true,
    params: &[
        opt("path", Ty::Str, "Destination file path."),
        opt("target_format", Ty::Enum(&["dwg", "dxf"]), "Target CAD format."),
        opt("target_version", Ty::Enum(TARGET_VERSIONS), "Target AutoCAD version."),
        opt("allow_lossy", Ty::Bool, "Acknowledge dropping unsupported passthrough records."),
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
        opt("target_format", Ty::Enum(&["dwg", "dxf"]), "Explicit output format matching extension."),
        opt("target_version", Ty::Enum(TARGET_VERSIONS), "Explicit CAD version."),
        opt("allow_lossy", Ty::Bool, "Acknowledge dropping unsupported records."),
        opt("overwrite", Ty::Bool, "Replace destination if file already exists."),
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
        req("steps", Ty::Raw(r##"{"type":"array","minItems":1,"maxItems":64,"description":"Sequential operations list","items":{"$ref":"#/$defs/batch_step"}}"##), "List of batch steps to execute."),
    ],
    rules: &[],
    example: r#"{"op":"batch","steps":[{"op":"run","cmd":"LINE 0,0 10,0"}]}"#,
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
    &SET_PROPERTIES,
    &ACTION,
    &EMBED_IMAGE,
    &WBLOCK,
    &PLOT,
    &ENTITIES_CREATE,
    &ENTITIES_DELETE,
    &ENTITIES_TRANSFORM,
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
    &PAGE_SETUP_SET,
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
        Ty::Raw(s) => serde_json::from_str(s).expect("Raw fragment must be valid JSON"),
    }
}

pub fn shared_defs() -> Value {
    let batch_variants: Vec<Value> = OPS
        .iter()
        .filter(|op| op.batchable)
        .map(|op| op_schema(op))
        .collect();

    json!({
        "point":  {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 3},
        "window": {"type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4},
        "handle": {"type": "string", "pattern": "^(0[xX])?[0-9A-Fa-f]+$"},
        "batch_step": {
            "type": "object",
            "properties": {
                "op": {"type": "string"},
                "document_id": {"type": "integer", "minimum": 0}
            },
            "required": ["op"],
            "anyOf": batch_variants
        }
    })
}

fn rules_text(op: &OpDef) -> String {
    let mut out = String::new();
    for r in op.rules {
        match r {
            Rule::AnyOf(keys) => {
                out.push_str(&format!(" Requires at least one of: {}.", keys.join(", ")));
            }
            Rule::RequiredWhen { key, equals, then } => {
                out.push_str(&format!(" When {key}={equals}, requires: {}.", then.join(", ")));
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
    json!({
        "type": "object",
        "description": format!("{}{}", op.doc, rules_text(op)),
        "properties": props,
        "required": required,
    })
}

pub fn execute_request_schema() -> Value {
    let variants: Vec<Value> = OPS.iter().map(|op| op_schema(op)).collect();
    let op_names: Vec<&str> = OPS.iter().map(|op| op.name).collect();
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "op": {"type": "string", "enum": op_names, "description": "Semantic editor operation."},
            "request_id": {"type": "string", "minLength": 1, "maxLength": 128, "description": "Caller-generated idempotency key."},
            "document_id": {"type": "integer", "minimum": 0, "description": "Target document from current state."},
            "revision": {"type": "integer", "minimum": 0, "description": "Expected edit revision from current state."},
            "geometry_revision": {"type": "integer", "minimum": 0, "description": "Expected geometry revision."},
            "camera_revision": {"type": "integer", "minimum": 0, "description": "Expected camera revision."},
            "selection": {"type": "array", "items": {"$ref": "#/$defs/handle"}, "description": "Expected selected handles."},
            "steps": {"type": "array", "minItems": 1, "maxItems": 64, "description": "Sequential batch operations.", "items": {"$ref": "#/$defs/batch_step"}}
        },
        "required": ["op", "request_id"],
        "anyOf": variants,
        "$defs": shared_defs()
    })
}

pub fn batch_step_schema() -> Value {
    let batch_variants: Vec<Value> = OPS
        .iter()
        .filter(|op| op.batchable)
        .map(|op| op_schema(op))
        .collect();

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
                validate_type(inner, item)
                    .map_err(|e| format!("item {idx}: {e}"))?;
            }
        }
        Ty::Raw(_) => {}
    }
    Ok(())
}

pub fn validate_request(
    request: &Value,
    is_batch_step: bool,
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
        return Err(format!("operation '{op_name}' cannot be used in a batch step"));
    }

    // Check required params
    for p in op_def.params {
        if p.required {
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
                let any_present = keys.iter().any(|k| obj.get(*k).is_some_and(|v| !v.is_null()));
                if !any_present {
                    return Err(format!(
                        "Missing {} for {}. Example request: {}",
                        keys.join(" or "),
                        op_name,
                        op_def.example
                    ));
                }
            }
            Rule::RequiredWhen { key, equals, then } => {
                let matches = obj.get(*key).and_then(Value::as_str).is_some_and(|v| v.eq_ignore_ascii_case(equals));
                if matches {
                    for req_key in *then {
                        let val = obj.get(*req_key);
                        if val.is_none() || val.unwrap().is_null() {
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
                validate_type(&p.ty, val).map_err(|e| {
                    format!("parameter '{}' for op '{}': {e}", p.name, op_name)
                })?;
            }
        }
    }

    // Detect unknown keys (Phase 1: warning)
    let mut warnings = Vec::new();
    let mut allowed: Vec<&str> = op_def.params.iter().map(|p| p.name).collect();
    if is_batch_step {
        allowed.extend(["op", "document_id"]);
    } else {
        allowed.extend(ENVELOPE_KEYS.iter().copied());
    }

    for (k, _) in obj {
        if !allowed.contains(&k.as_str()) {
            if k == "data_base64" {
                warnings.push("data_base64 is supported only in the Web/WASM build, not in native desktop".into());
            } else {
                warnings.push(format!("unknown parameter '{k}' ignored for op '{op_name}'"));
            }
        }
    }

    Ok(ValidationResult { warnings })
}
