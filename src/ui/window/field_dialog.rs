//! Field dialog (FIELD and the ƒ buttons of the attribute definition
//! dialogs): a category and field list on the left, the chosen field's
//! format on the right, a live preview and the field expression below.

use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_input, Space,
};
use iced::{Element, Fill, Length};

use crate::app::Message;
use crate::t;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button, field_style};
use crate::ui::window::pdf_dialogs::{accent_text, card_style};

/// Where the chosen field goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldTarget {
    /// FIELD: a new multi-line text placed on screen.
    NewText,
    /// The Default of the Attribute Definition dialog.
    AttdefDefault,
    /// The Default of the Edit Attribute Definition dialog.
    AttdefEdit,
    /// The field a text already shows, edited in place.
    EditText(codec::Handle),
}

/// The categories the dialog offers, with their fields (only the ones the
/// application evaluates).
pub const CATEGORIES: &[(&str, &[&str])] = &[
    ("All", &[]),
    (
        "Date & Time",
        &["CreateDate", "Date", "PlotDate", "SaveDate"],
    ),
    (
        "Document",
        &[
            "Author",
            "Comments",
            "Filename",
            "Filesize",
            "HyperlinkBase",
            "Keywords",
            "LastSavedBy",
            "Subject",
            "Title",
        ],
    ),
    ("Linked", &["Hyperlink"]),
    (
        "Objects",
        &[
            "BlockPlaceholder",
            "Count",
            "CountInArea",
            "Formula",
            "NamedObject",
            "Object",
        ],
    ),
    ("Other", &["DieselExpression", "SystemVariable"]),
    (
        "Plot",
        &[
            "DeviceName",
            "Login",
            "PageSetupName",
            "PaperSize",
            "PlotDate",
            "PlotOrientation",
            "PlotScale",
            "PlotStyleTable",
        ],
    ),
    ("SheetSet", SHEET_SET_FIELDS),
];

/// The SheetSet category, in the reference's order.
pub const SHEET_SET_FIELDS: &[&str] = &[
    "CurrentSheetCategory",
    "CurrentSheetCustom",
    "CurrentSheetDescription",
    "CurrentSheetIssuePurpose",
    "CurrentSheetNumber",
    "CurrentSheetNumberAndTitle",
    "CurrentSheetRevisionDate",
    "CurrentSheetRevisionNumber",
    "CurrentSheetSet",
    "CurrentSheetSetCustom",
    "CurrentSheetSetDescription",
    "CurrentSheetSetProjectMilestone",
    "CurrentSheetSetProjectName",
    "CurrentSheetSetProjectNumber",
    "CurrentSheetSetProjectPhase",
    "CurrentSheetSubSet",
    "CurrentSheetTitle",
    "SheetSet",
    "SheetSetPlaceholder",
    "SheetView",
];

/// A sheet set field's code without format: `\AcSm[.16.2] Component.Property`.
fn sheet_set_code(name: &str, custom: &str) -> String {
    let (version, target) = match name {
        "CurrentSheetCategory" => (".16.2", "Sheet.Category".to_string()),
        "CurrentSheetCustom" => ("", format!("Sheet.{custom}")),
        "CurrentSheetDescription" => ("", "Sheet.Description".into()),
        "CurrentSheetIssuePurpose" => (".16.2", "Sheet.IssuePurpose".into()),
        "CurrentSheetNumber" => ("", "Sheet.Number".into()),
        "CurrentSheetNumberAndTitle" => ("", "Sheet.NumberAndTitle".into()),
        "CurrentSheetRevisionDate" => (".16.2", "Sheet.RevisionDate".into()),
        "CurrentSheetRevisionNumber" => (".16.2", "Sheet.RevisionNumber".into()),
        "CurrentSheetSet" => ("", "SheetSet.Name".into()),
        "CurrentSheetSetCustom" => ("", format!("SheetSet.{custom}")),
        "CurrentSheetSetDescription" => ("", "SheetSet.Description".into()),
        "CurrentSheetSetProjectMilestone" => (".16.2", "SheetSet.ProjectMilestone".into()),
        "CurrentSheetSetProjectName" => (".16.2", "SheetSet.ProjectName".into()),
        "CurrentSheetSetProjectNumber" => (".16.2", "SheetSet.ProjectNumber".into()),
        "CurrentSheetSetProjectPhase" => (".16.2", "SheetSet.ProjectPhase".into()),
        "CurrentSheetSubSet" => ("", "Subset.Name".into()),
        "CurrentSheetTitle" => ("", "Sheet.Title".into()),
        _ => return "\\AcSm".into(),
    };
    format!("\\AcSm{version} {target}")
}

/// The navigation tree of a SheetSet (set, subsets, sheets) or SheetView
/// (set, view categories) field: depth, component id, label, is a sheet.
pub fn nav_nodes(
    db: &codec::sheet_set::SheetSetDatabase,
    views: bool,
) -> Vec<(u16, String, String, bool)> {
    use codec::sheet_set::{number_and_title, ComponentKind, Element as SsElement};
    let set = db.sheet_set();
    let mut out = vec![(0, set.id().to_string(), db.name().to_string(), false)];
    if views {
        // Views of the unnamed (default) category sit under the set, then each
        // named category with its views.
        let all: Vec<&SsElement> = db
            .sheets()
            .into_iter()
            .flat_map(|s| db.sheet_views(s))
            .collect();
        let named: Vec<&SsElement> = db.view_categories();
        let in_named = |v: &SsElement| {
            codec::sheet_set::SheetSetDatabase::view_category_of(v)
                .is_some_and(|c| named.iter().any(|n| n.id() == c))
        };
        for v in all.iter().filter(|v| !in_named(v)) {
            out.push((1, v.id().to_string(), number_and_title(v), true));
        }
        for c in &named {
            out.push((
                1,
                c.id().to_string(),
                c.prop("Name").unwrap_or("").to_string(),
                false,
            ));
            for v in all
                .iter()
                .filter(|v| codec::sheet_set::SheetSetDatabase::view_category_of(v) == Some(c.id()))
            {
                out.push((2, v.id().to_string(), number_and_title(v), true));
            }
        }
        return out;
    }
    fn go(el: &SsElement, depth: u16, out: &mut Vec<(u16, String, String, bool)>) {
        for c in &el.children {
            match ComponentKind::of(c) {
                Some(ComponentKind::Subset) => {
                    out.push((
                        depth,
                        c.id().to_string(),
                        c.prop("Name").unwrap_or("").to_string(),
                        false,
                    ));
                    go(c, depth + 1, out);
                }
                Some(ComponentKind::Sheet) => {
                    out.push((depth, c.id().to_string(), number_and_title(c), true))
                }
                _ => {}
            }
        }
    }
    go(set, 1, &mut out);
    out
}

/// The properties a navigation node offers: list label, code property, version.
pub fn nav_props(
    db: &codec::sheet_set::SheetSetDatabase,
    id: &str,
) -> Vec<(String, String, &'static str)> {
    use codec::sheet_set::{
        custom_properties, ComponentKind, CUSTOM_SHEET_PROP, CUSTOM_SHEET_SET_PROP,
    };
    let Some(el) = db.find(id) else {
        return Vec::new();
    };
    let fixed = |list: &[(&str, &str, &'static str)]| -> Vec<(String, String, &'static str)> {
        list.iter()
            .map(|(l, p, v)| (l.to_string(), p.to_string(), *v))
            .collect()
    };
    let custom = |flag: i32| -> Vec<(String, String, &'static str)> {
        custom_properties(db.sheet_set())
            .into_iter()
            .filter(|(_, _, f)| f & flag != 0)
            .map(|(n, _, _)| (n.clone(), n, ""))
            .collect()
    };
    match ComponentKind::of(el) {
        Some(ComponentKind::SheetSet) => {
            let mut v = fixed(&[
                ("SheetSetName", "Name", ""),
                ("SheetSetDescription", "Description", ""),
                ("ProjectNumber", "ProjectNumber", ".16.2"),
                ("ProjectName", "ProjectName", ".16.2"),
                ("ProjectPhase", "ProjectPhase", ".16.2"),
                ("ProjectMilestone", "ProjectMilestone", ".16.2"),
            ]);
            v.extend(custom(CUSTOM_SHEET_SET_PROP));
            v
        }
        Some(ComponentKind::Subset) => fixed(&[("SheetSubsetName", "Name", "")]),
        Some(ComponentKind::Sheet) => {
            let mut v = fixed(&[
                ("SheetNumberAndTitle", "NumberAndTitle", ""),
                ("SheetTitle", "Title", ""),
                ("SheetNumber", "Number", ""),
                ("SheetDescription", "Description", ""),
                ("RevisionNumber", "RevisionNumber", ".16.2"),
                ("RevisionDate", "RevisionDate", ".16.2"),
                ("IssuePurpose", "IssuePurpose", ".16.2"),
                ("Category", "Category", ".16.2"),
            ]);
            v.extend(custom(CUSTOM_SHEET_PROP));
            v
        }
        None if el.name == "AcSmSheetView" => fixed(&[
            ("ViewNumberAndTitle", "NumberAndTitle", ""),
            ("ViewTitle", "Title", ""),
            ("ViewNumber", "Number", ""),
            ("ViewportScale", "ViewportScale", ".16.2"),
        ]),
        None => fixed(&[("ViewCategoryName", "Name", "")]),
    }
}

/// The sheet drawing, named view and layout a sheet or sheet view node links to.
pub fn nav_link(
    db: &codec::sheet_set::SheetSetDatabase,
    id: &str,
) -> Option<(String, String, String)> {
    let el = db.find(id)?;
    let (sheet, view) = if el.name == "AcSmSheetView" {
        let views = db.parent_of(id)?;
        let sheet = db.parent_of(views.id())?;
        (
            sheet.id().to_string(),
            el.named("NamedView")
                .and_then(|v| v.prop("Name"))
                .unwrap_or("")
                .to_string(),
        )
    } else if codec::sheet_set::ComponentKind::of(el)
        == Some(codec::sheet_set::ComponentKind::Sheet)
    {
        (id.to_string(), String::new())
    } else {
        return None;
    };
    let r = db.layout_reference(&sheet, "Layout")?;
    Some((r.file_name, view, r.name))
}

impl FieldDialogState {
    /// `\AcSm Database("…").SheetSet("…")[.Component("…")].Property` of the
    /// picked node, with the text case and, on a sheet, the hyperlink to it.
    fn navigation_code(&self) -> String {
        let Some(db) = self.ss_sets.get(self.ss_set) else {
            return "\\AcSm".into();
        };
        let Some((_, prop, version)) = nav_props(db, &self.ss_node)
            .into_iter()
            .find(|(l, _, _)| *l == self.ss_prop)
        else {
            return "\\AcSm".into();
        };
        let set = db.sheet_set().id().to_string();
        let path = codec::sheet_set::native_path(db.path.as_deref().unwrap_or(""));
        // ViewportScale takes a scale format (and its version), not a text case.
        let scale = prop == "ViewportScale";
        let version = if scale {
            codec::fields::PLOT_SCALE_FORMATS
                .get(self.ss_scale_format)
                .map_or("", |(_, c)| {
                    if c.starts_with("\\AcVar.16.2") {
                        ".16.2"
                    } else {
                        ""
                    }
                })
        } else {
            version
        };
        let mut code = format!("\\AcSm{version} Database(\"{path}\").SheetSet(\"{set}\")");
        if self.ss_node != set {
            code.push_str(&format!(".Component(\"{}\")", self.ss_node));
        }
        code.push('.');
        code.push_str(&prop);
        if scale {
            let f = viewport_scale_picture(self.ss_scale_format);
            if !f.is_empty() {
                code.push(' ');
                code.push_str(&f);
            }
            return code;
        }
        if self.text_case != 0 {
            code.push_str(&format!(" \\f \"%tc{}\"", self.text_case));
        }
        // A sheet links to its layout, a view to its named view on the sheet.
        if self.ss_href {
            if let Some((file, view, layout)) = nav_link(db, &self.ss_node) {
                code.push_str(&format!(" \\href \"{file}#{view},{layout}##1\""));
            }
        }
        code
    }
}

/// SheetSetPlaceholder types: list label, the `?` target, field version.
pub const PLACEHOLDER_TYPES: &[(&str, &str, &str)] = &[
    ("SheetNumberAndTitle", "?Sheet.NumberAndTitle", ""),
    ("SheetTitle", "?Sheet.Title", ""),
    ("SheetNumber", "?Sheet.Number", ""),
    ("SheetDescription", "?Sheet.Description", ""),
    ("RevisionNumber", "?Sheet.RevisionNumber", ".16.2"),
    ("RevisionDate", "?Sheet.RevisionDate", ".16.2"),
    ("IssuePurpose", "?Sheet.IssuePurpose", ".16.2"),
    ("Category", "?Sheet.Category", ".16.2"),
    ("Custom", "?Sheet.", ""),
    ("ViewNumberAndTitle", "?View.NumberAndTitle", ""),
    ("ViewTitle", "?View.Title", ""),
    ("ViewNumber", "?View.Number", ""),
    ("ViewportScale", "?View.ViewportScale", ".16.2"),
];

/// ViewportScale placeholder formats: the PlotScale pictures.
pub fn viewport_scale_picture(k: usize) -> String {
    codec::fields::PLOT_SCALE_FORMATS
        .get(k)
        .and_then(|(_, code)| code.find("\\f ").map(|p| code[p..].to_string()))
        .unwrap_or_default()
}

/// The fields of a category; "All" lists every field once, sorted.
pub fn fields_of(category: usize) -> Vec<&'static str> {
    if category == 0 {
        let mut all: Vec<&'static str> = CATEGORIES
            .iter()
            .flat_map(|(_, f)| f.iter().copied())
            .collect();
        all.sort_unstable_by_key(|s| s.to_ascii_lowercase());
        all.dedup();
        return all;
    }
    CATEGORIES
        .get(category)
        .map(|(_, f)| f.to_vec())
        .unwrap_or_default()
}

/// The date formats the dialog lists, in the reference order.
pub const DATE_FORMATS: &[&str] = &[
    "M/d/yyyy",
    "dddd, MMMM dd, yyyy",
    "MMMM d, yyyy",
    "M/d/yy",
    "yyyy-MM-dd",
    "d-MMM-yy",
    "M.d.yyyy",
    "MMM. d, yy",
    "d MMMM yyyy",
    "dd.MM.yyyy",
    "dd/MM/yyyy",
    "yyyy/MM/dd",
    "yyyy-M-d",
    "MMMM yy",
    "MMM-yy",
    "M/d/yyyy h:mm tt",
    "M/d/yyyy h:mm:ss tt",
    "h:mm tt",
    "h:mm:ss tt",
    "HH:mm",
    "HH:mm:ss",
    "%#x",
    "%#c",
    "%x",
    "%c",
    "%X",
];

pub const TEXT_CASES: &[&str] = &[
    "(none)",
    "Uppercase",
    "Lowercase",
    "First capital",
    "Title case",
];
pub const SIZE_UNITS: &[&str] = &["Bytes", "Kilobytes", "Megabytes"];
pub const NAMED_TYPES: &[&str] = &[
    "Block",
    "Dimstyle",
    "Layer",
    "Linetype",
    "Tablestyle",
    "Textstyle",
    "View",
];

/// How a field is formatted, by its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Date,
    Text,
    Filename,
    Filesize,
    SystemVariable,
    Diesel,
    NamedObject,
    Object,
    Formula,
    Hyperlink,
    PlotScale,
    /// Count / CountInArea: a JSON expression.
    Count,
    /// A CurrentSheet* / CurrentSheetSet* field (text case).
    SheetSet,
    /// CurrentSheetCustom / CurrentSheetSetCustom (text case + property).
    SheetSetCustom,
    SheetSetPlaceholder,
    /// SheetSet / SheetView: a component picked in a sheet set.
    SheetSetNavigation,
    BlockPlaceholder,
    /// A custom drawing property: `\AcVar CustomDP.<Name>` (text case).
    CustomProperty,
}

/// The pseudo field name of the document custom properties; the property
/// itself is `FieldDialogState::custom_property`.
pub const CUSTOM_PROPERTY: &str = "CustomDP";

/// Formula precisions: the current one, then 0 to 8 decimals.
pub const FORMULA_PRECISIONS: &[&str] = &[
    "Current precision",
    "0",
    "0.0",
    "0.00",
    "0.000",
    "0.0000",
    "0.00000",
    "0.000000",
    "0.0000000",
    "0.00000000",
];

/// Decimal separators of a number: list label and `%ds` character (the
/// period is the default and writes none).
pub const DECIMAL_SEPARATORS: &[(&str, char)] = &[("Period", '.'), ("Comma", ','), ("Space", ' ')];

/// Thousands separators of a number: list label and `%th` character.
pub const THOUSANDS_SEPARATORS: &[(&str, Option<char>)] = &[
    ("None", None),
    ("Comma", Some(',')),
    ("Period", Some('.')),
    ("Space", Some(' ')),
];

/// The coordinates a point property shows: list label and `%pt` bits (all
/// three is the default and writes none).
pub const COORDINATES: &[(&str, u8)] = &[("X, Y, Z", 7), ("X, Y", 3), ("X", 1), ("Y", 2), ("Z", 4)];

/// Object properties that are points. A new field reads them the way the
/// Properties palette does (`AcObjProp.16.2`), in the current UCS, as level
/// marks do; a field opened from the drawing keeps the evaluator it has.
pub const POINT_PROPERTIES: &[&str] = &[
    "Center",
    "EndPoint",
    "InsertionPoint",
    "Position",
    "StartPoint",
];

/// Object properties whose value is text: they take a text case rather than
/// a number format.
pub const TEXT_PROPERTIES: &[&str] = &["Color", "EffectiveName", "Layer", "Linetype", "TextString"];

/// The text between the brackets that follow `key` and its digits in a
/// format picture (`%ct8[100]` → `100`).
fn bracketed<'a>(picture: &'a str, key: &str) -> Option<&'a str> {
    let after = &picture[picture.find(key)? + key.len()..];
    let after = after.trim_start_matches(|c: char| c.is_ascii_digit());
    let inner = after.strip_prefix('[')?;
    Some(&inner[..inner.find(']')?])
}

pub fn kind_of(name: &str) -> FieldKind {
    match name {
        "CreateDate" | "Date" | "PlotDate" | "SaveDate" => FieldKind::Date,
        "Filename" => FieldKind::Filename,
        "Filesize" => FieldKind::Filesize,
        "SystemVariable" => FieldKind::SystemVariable,
        "DieselExpression" => FieldKind::Diesel,
        "NamedObject" => FieldKind::NamedObject,
        "Object" => FieldKind::Object,
        "Formula" => FieldKind::Formula,
        "Hyperlink" => FieldKind::Hyperlink,
        "PlotScale" => FieldKind::PlotScale,
        "Count" | "CountInArea" => FieldKind::Count,
        "CurrentSheetCustom" | "CurrentSheetSetCustom" => FieldKind::SheetSetCustom,
        "SheetSetPlaceholder" => FieldKind::SheetSetPlaceholder,
        "SheetSet" | "SheetView" => FieldKind::SheetSetNavigation,
        n if SHEET_SET_FIELDS.contains(&n) => FieldKind::SheetSet,
        "BlockPlaceholder" => FieldKind::BlockPlaceholder,
        CUSTOM_PROPERTY => FieldKind::CustomProperty,
        _ => FieldKind::Text,
    }
}

/// The Field dialog's working copy.
#[derive(Debug, Clone)]
pub struct FieldDialogState {
    pub target: FieldTarget,
    pub category: usize,
    pub name: &'static str,
    pub date_format: String,
    pub text_case: usize,
    /// 1 = path only, 2 = file name only, 3 = path and file name.
    pub file_parts: u8,
    pub file_extension: bool,
    pub size_unit: usize,
    pub sysvar: String,
    pub diesel: String,
    pub named_type: usize,
    pub named_names: Vec<(String, codec::Handle)>,
    pub named: Option<usize>,
    /// The object picked for an Object field, its type and properties.
    pub object: Option<(codec::Handle, String)>,
    pub object_props: Vec<&'static str>,
    pub object_prop: Option<&'static str>,
    /// Whether the object field opened from the drawing used the versioned
    /// evaluator (`AcObjProp.16.2`); `None` for a new field.
    pub object_versioned: Option<bool>,
    pub formula: String,
    /// Indexes into codec FORMULA_FORMATS and FORMULA_PRECISIONS.
    pub formula_format: usize,
    pub formula_precision: usize,
    /// Additional format of a formula or object property: the conversion
    /// factor (`%ct8[…]`, empty for none), the text around the value
    /// (`%ps[prefix,suffix]`), indexes into DECIMAL_SEPARATORS and
    /// THOUSANDS_SEPARATORS, and for a point the index into COORDINATES.
    pub conversion: String,
    pub prefix: String,
    pub suffix: String,
    pub decimal_separator: usize,
    pub thousands_separator: usize,
    pub coordinates: usize,
    pub hyperlink_text: String,
    pub hyperlink_url: String,
    /// Index into codec PLOT_SCALE_FORMATS.
    pub plot_scale: usize,
    /// The JSON expression of a Count / CountInArea field.
    pub count_expression: String,
    /// The block being edited in the block editor; block placeholders are
    /// only offered there.
    pub placeholder_block: Option<String>,
    /// Index into codec BLOCK_PLACEHOLDER_PROPERTIES.
    pub placeholder_property: usize,
    pub preview: String,
    /// The drawing's custom property names in stored order (listed under
    /// Document) and the one chosen.
    pub custom_properties: Vec<String>,
    pub custom_property: String,
    /// Sheet set fields: the open set's sheet (flags 2) and set (flags 1)
    /// custom property names, the chosen one, the placeholder type and the
    /// ViewportScale format.
    pub ss_sheet_custom: Vec<String>,
    pub ss_set_custom: Vec<String>,
    pub ss_custom: String,
    pub ss_placeholder: usize,
    pub ss_scale_format: usize,
    /// The open sheet set's name (SheetSet / SheetView fields).
    pub ss_set_name: String,
    /// SheetSet / SheetView: the sets offered (open ones and any picked with
    /// "..."), the one shown, the node and property picked, associate hyperlink.
    pub ss_sets: Vec<codec::sheet_set::SheetSetDatabase>,
    pub ss_set: usize,
    pub ss_node: String,
    pub ss_prop: String,
    pub ss_href: bool,
    /// Today in each of DATE_FORMATS, for the Examples list.
    pub examples: Vec<String>,
}

impl FieldDialogState {
    /// The rows of the field list: the category's fields, and under Document
    /// the drawing's custom properties after them, in stored order.
    pub fn entries(&self) -> Vec<(String, bool, FieldDialogMsg)> {
        let mut rows: Vec<(String, bool, FieldDialogMsg)> = fields_of(self.category)
            .into_iter()
            .map(|n| (n.to_string(), self.name == n, FieldDialogMsg::Name(n)))
            .collect();
        if CATEGORIES.get(self.category).is_some_and(|(c, _)| *c == "Document") {
            rows.extend(self.custom_properties.iter().map(|property| {
                (
                    property.clone(),
                    self.name == CUSTOM_PROPERTY && self.custom_property == *property,
                    FieldDialogMsg::CustomProperty(property.clone()),
                )
            }));
        }
        rows
    }

    pub fn new(target: FieldTarget) -> Self {
        Self {
            target,
            category: 0,
            name: "Date",
            date_format: "%x".into(),
            text_case: 0,
            file_parts: 3,
            file_extension: true,
            size_unit: 0,
            sysvar: crate::entities::field::SYSVARS[0].into(),
            diesel: String::new(),
            named_type: 2,
            named_names: Vec::new(),
            named: None,
            object: None,
            object_props: Vec::new(),
            object_prop: None,
            object_versioned: None,
            formula: String::new(),
            formula_format: 0,
            formula_precision: 0,
            conversion: String::new(),
            prefix: String::new(),
            suffix: String::new(),
            decimal_separator: 0,
            thousands_separator: 0,
            coordinates: 0,
            hyperlink_text: String::new(),
            hyperlink_url: String::new(),
            plot_scale: 0,
            count_expression: String::new(),
            placeholder_block: None,
            placeholder_property: 7,
            preview: String::new(),
            custom_properties: Vec::new(),
            custom_property: String::new(),
            ss_sheet_custom: Vec::new(),
            ss_set_custom: Vec::new(),
            ss_custom: String::new(),
            ss_placeholder: 0,
            ss_scale_format: 6,
            ss_set_name: String::new(),
            ss_sets: Vec::new(),
            ss_set: 0,
            ss_node: String::new(),
            ss_prop: String::new(),
            ss_href: true,
            examples: Vec::new(),
        }
    }

    /// The field code (without the `%<…>%` wrapper) and the objects it refers to.
    pub fn code(&self) -> (String, Vec<codec::Handle>) {
        let case = |base: String| match self.text_case {
            0 => base,
            n => format!("{base} \\f \"%tc{n}\""),
        };
        match kind_of(self.name) {
            FieldKind::Date => (
                format!("\\AcVar {} \\f \"{}\"", self.name, self.date_format),
                vec![],
            ),
            FieldKind::Text => (case(format!("\\AcVar {}", self.name)), vec![]),
            FieldKind::CustomProperty => (
                case(crate::entities::field::custom_property_code(
                    &self.custom_property,
                )),
                vec![],
            ),
            FieldKind::Filename => {
                let bits = self.file_parts
                    + if self.file_extension && self.file_parts & 2 != 0 {
                        4
                    } else {
                        0
                    };
                // The text case goes first in the same format: "%tc1%fn6".
                let case = match self.text_case {
                    0 => String::new(),
                    n => format!("%tc{n}"),
                };
                (format!("\\AcVar Filename \\f \"{case}%fn{bits}\""), vec![])
            }
            // Bytes are whole; kilo- and megabytes keep two decimals.
            FieldKind::Filesize => (
                match self.size_unit {
                    0 => "\\AcVar Filesize \\f \"%ld%by1\"".to_string(),
                    n => format!("\\AcVar Filesize \\f \"%.2f%by{}\"", n + 1),
                },
                vec![],
            ),
            FieldKind::Formula => {
                let (formula, objects) = indexed_object_ids(self.formula.trim());
                let body = format!("\\AcExpr ({formula})");
                match self.number_picture() {
                    f if f.is_empty() => (body, objects),
                    f => (format!("{body} \\f \"{f}\""), objects),
                }
            }
            FieldKind::Hyperlink => (
                format!(
                    "\\AcVar \\href \"{}##{}#0\"",
                    self.hyperlink_url.trim(),
                    self.hyperlink_text
                ),
                vec![],
            ),
            FieldKind::PlotScale => (
                codec::fields::PLOT_SCALE_FORMATS
                    .get(self.plot_scale)
                    .map_or_else(|| "\\AcVar PlotScale".to_string(), |f| f.1.to_string()),
                vec![],
            ),
            FieldKind::Count => {
                let evaluator = if self.name == "CountInArea" {
                    "AcCount2"
                } else {
                    "AcCount"
                };
                match self.count_expression.trim() {
                    "" => (format!("\\{evaluator}"), vec![]),
                    json => (format!("\\{evaluator} {json}"), vec![]),
                }
            }
            FieldKind::SheetSet | FieldKind::SheetSetCustom => {
                (case(sheet_set_code(self.name, &self.ss_custom)), vec![])
            }
            FieldKind::SheetSetNavigation => (self.navigation_code(), vec![]),
            FieldKind::SheetSetPlaceholder => {
                let (label, target, version) =
                    PLACEHOLDER_TYPES[self.ss_placeholder.min(PLACEHOLDER_TYPES.len() - 1)];
                let target = if label == "Custom" {
                    format!("{target}{}", self.ss_custom)
                } else {
                    target.to_string()
                };
                if label == "ViewportScale" {
                    // The version follows the format, as for PlotScale.
                    let version = codec::fields::PLOT_SCALE_FORMATS
                        .get(self.ss_scale_format)
                        .map_or("", |(_, code)| {
                            if code.starts_with("\\AcVar.16.2") {
                                ".16.2"
                            } else {
                                ""
                            }
                        });
                    let base = format!("\\AcSm{version} {target}");
                    match viewport_scale_picture(self.ss_scale_format) {
                        f if f.is_empty() => (base, vec![]),
                        f => (format!("{base} {f}"), vec![]),
                    }
                } else {
                    (case(format!("\\AcSm{version} {target}")), vec![])
                }
            }
            FieldKind::BlockPlaceholder => {
                let (_, property, format) =
                    codec::fields::BLOCK_PLACEHOLDER_PROPERTIES[self.placeholder_property];
                // Text properties take the chosen case; the others keep their own format.
                let format = match (format, self.text_case) {
                    ("%tc4", 0) => String::new(),
                    ("%tc4", n) => format!("%tc{n}"),
                    (other, _) => other.to_string(),
                };
                let code = format!("\\AcObjProp.16.2 Object(?BlockRefId,1).{property}");
                if format.is_empty() {
                    (code, vec![])
                } else {
                    (format!("{code} \\f \"{format}\""), vec![])
                }
            }
            FieldKind::SystemVariable => (format!("\\AcVar {}", self.sysvar), vec![]),
            FieldKind::Diesel => (format!("\\AcDiesel {}", self.diesel), vec![]),
            FieldKind::NamedObject => match self.named.and_then(|i| self.named_names.get(i)) {
                Some((_, handle)) => (
                    case("\\AcObjProp Object(%<\\_ObjIdx 0>%).Name".into()),
                    vec![*handle],
                ),
                None => ("\\AcObjProp".into(), vec![]),
            },
            FieldKind::Object => match (&self.object, self.object_prop) {
                (Some((handle, _)), Some(prop)) => {
                    let versioned = self
                        .object_versioned
                        .unwrap_or_else(|| POINT_PROPERTIES.contains(&prop));
                    let evaluator = if versioned {
                        "AcObjProp.16.2"
                    } else {
                        "AcObjProp"
                    };
                    let body = format!("\\{evaluator} Object(%<\\_ObjIdx 0>%).{prop}");
                    if TEXT_PROPERTIES.contains(&prop) {
                        (case(body), vec![*handle])
                    } else {
                        match self.number_picture() {
                            f if f.is_empty() => (body, vec![*handle]),
                            f => (format!("{body} \\f \"{f}\""), vec![*handle]),
                        }
                    }
                }
                _ => ("\\AcObjProp".into(), vec![]),
            },
        }
    }

    /// Open on the field `code` a text already shows: its name and the
    /// settings its code spells. An Object field returns the property it
    /// reads, for the caller to match against the object's own list.
    pub fn seed_from_code(&mut self, code: &str, objects: &[codec::Handle]) -> Option<String> {
        let code = code.trim();
        let (body, picture) = match code.find(" \\f \"") {
            Some(at) => (code[..at].trim(), code[at + 4..].trim().trim_matches('"')),
            None => (code, ""),
        };
        self.category = 0;
        self.formula_format = codec::fields::FORMULA_FORMATS
            .iter()
            .position(|(_, format)| !format.is_empty() && picture.contains(format))
            .unwrap_or(0);
        let digits = |after: &str| -> Option<usize> {
            let tail = picture.split(after).nth(1)?;
            let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        };
        self.formula_precision =
            digits("%pr").map_or(0, |n| (n + 1).min(FORMULA_PRECISIONS.len() - 1));
        if let Some(case) = digits("%tc") {
            self.text_case = case;
        }
        let character = |key: &str| digits(key).and_then(|n| char::from_u32(n as u32));
        if let Some(separator) = character("%ds") {
            self.decimal_separator = DECIMAL_SEPARATORS
                .iter()
                .position(|(_, c)| *c == separator)
                .unwrap_or(0);
        }
        if let Some(separator) = character("%th") {
            self.thousands_separator = THOUSANDS_SEPARATORS
                .iter()
                .position(|(_, c)| *c == Some(separator))
                .unwrap_or(0);
        }
        if let Some(bits) = digits("%pt") {
            self.coordinates = COORDINATES
                .iter()
                .position(|(_, b)| usize::from(*b) == bits)
                .unwrap_or(0);
        }
        if let Some(factor) = bracketed(picture, "%ct8") {
            self.conversion = factor.trim().to_string();
        }
        if let Some(pair) = bracketed(picture, "%ps") {
            let (prefix, suffix) = pair.split_once(',').unwrap_or((pair, ""));
            self.prefix = prefix.to_string();
            self.suffix = suffix.to_string();
        }
        let (evaluator, rest) = body.split_once(' ').unwrap_or((body, ""));
        let rest = rest.trim();
        if evaluator.starts_with("\\AcExpr") {
            self.name = "Formula";
            let inner = rest
                .strip_prefix('(')
                .and_then(|inner| inner.strip_suffix(')'))
                .unwrap_or(rest);
            self.formula = typed_object_ids(inner, objects);
        } else if evaluator.starts_with("\\AcObjProp") {
            let property = rest.rsplit_once(").").map(|(_, property)| property.trim())?;
            if property == "Name" {
                self.name = "NamedObject";
                return None;
            }
            self.name = "Object";
            self.object_versioned = Some(evaluator.starts_with("\\AcObjProp."));
            return Some(property.to_string());
        } else if evaluator == "\\AcDiesel" {
            self.name = "DieselExpression";
            self.diesel = rest.to_string();
        } else if let Some(property) = evaluator
            .starts_with("\\AcVar")
            .then(|| rest.split(" \\f ").next().unwrap_or(rest))
            .and_then(crate::entities::field::custom_property_name)
        {
            self.name = CUSTOM_PROPERTY;
            self.custom_property = property.to_string();
        } else if evaluator.starts_with("\\AcVar") {
            let variable = rest.split_whitespace().next().unwrap_or("");
            if let Some(name) = fields_of(0)
                .into_iter()
                .find(|name| name.eq_ignore_ascii_case(variable))
            {
                self.name = name;
                if kind_of(name) == FieldKind::Date && !picture.is_empty() {
                    self.date_format = picture.to_string();
                }
            } else if !variable.is_empty() {
                self.name = "SystemVariable";
                self.sysvar = variable.to_string();
            }
        }
        None
    }

    /// The number format, precision and additional format picked for a
    /// formula or an object property (`%lu2%pr3%ds44%ct8[0.0001]%ps[,m²]`),
    /// empty for the drawing's own units.
    fn number_picture(&self) -> String {
        let mut picture = codec::fields::FORMULA_FORMATS
            .get(self.formula_format)
            .map_or("", |f| f.1)
            .to_string();
        let point = kind_of(self.name) == FieldKind::Object
            && self
                .object_prop
                .is_some_and(|p| POINT_PROPERTIES.contains(&p));
        if let Some((_, bits)) = COORDINATES
            .get(self.coordinates)
            .filter(|_| point && self.coordinates > 0)
        {
            picture.push_str(&format!("%pt{bits}"));
        }
        if self.formula_precision > 0 {
            picture.push_str(&format!("%pr{}", self.formula_precision - 1));
        }
        if let Some((_, separator)) = DECIMAL_SEPARATORS
            .get(self.decimal_separator)
            .filter(|_| self.decimal_separator > 0)
        {
            picture.push_str(&format!("%ds{}", u32::from(*separator)));
        }
        if let Some((_, Some(separator))) = THOUSANDS_SEPARATORS.get(self.thousands_separator) {
            picture.push_str(&format!("%th{}", u32::from(*separator)));
        }
        let factor = self.conversion.trim();
        if factor.parse::<f64>().is_ok_and(|f| f != 1.0) {
            picture.push_str(&format!("%ct8[{factor}]"));
        }
        // A comma ends the prefix and a bracket the pair; a quote would end
        // the picture.
        let prefix: String = self
            .prefix
            .chars()
            .filter(|c| !",]\"".contains(*c))
            .collect();
        let suffix: String = self
            .suffix
            .chars()
            .filter(|c| !"]\"".contains(*c))
            .collect();
        if !prefix.is_empty() || !suffix.is_empty() {
            picture.push_str(&format!("%ps[{prefix},{suffix}]"));
        }
        picture
    }

    /// The field can be inserted (an object field needs its object and
    /// property, a named object its name, a DIESEL field an expression).
    pub fn complete(&self) -> bool {
        match kind_of(self.name) {
            FieldKind::NamedObject => self.named.is_some(),
            FieldKind::Object => self.object.is_some() && self.object_prop.is_some(),
            FieldKind::Diesel => !self.diesel.trim().is_empty(),
            FieldKind::Formula => !self.formula.trim().is_empty(),
            FieldKind::Hyperlink => !self.hyperlink_url.trim().is_empty(),
            FieldKind::BlockPlaceholder => self.placeholder_block.is_some(),
            _ => true,
        }
    }
}

/// Table references in a formula are typed as `%<\_ObjId HANDLE>%`; the stored
/// code numbers them (`%<\_ObjIdx n>%`) and lists the objects.
/// The stored, numbered object references of a formula (`%<\_ObjIdx 0>%`)
/// back as the typed ones the dialog edits (`%<\_ObjId 2A>%`).
fn typed_object_ids(formula: &str, objects: &[codec::Handle]) -> String {
    let mut out = String::new();
    let mut rest = formula;
    while let Some(start) = rest.find("%<\\_ObjIdx ") {
        let after = &rest[start + "%<\\_ObjIdx ".len()..];
        let Some(end) = after.find(">%") else { break };
        let Some(handle) = after[..end]
            .trim()
            .parse::<usize>()
            .ok()
            .and_then(|index| objects.get(index))
        else {
            break;
        };
        out.push_str(&rest[..start]);
        out.push_str(&format!("%<\\_ObjId {:X}>%", handle.value()));
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

fn indexed_object_ids(formula: &str) -> (String, Vec<codec::Handle>) {
    let mut out = String::new();
    let mut objects: Vec<codec::Handle> = Vec::new();
    let mut rest = formula;
    while let Some(start) = rest.find("%<\\_ObjId ") {
        let after = &rest[start + "%<\\_ObjId ".len()..];
        let Some(end) = after.find(">%") else { break };
        let Ok(value) = u64::from_str_radix(after[..end].trim(), 16) else {
            break;
        };
        let handle = codec::Handle::new(value);
        let index = objects
            .iter()
            .position(|h| *h == handle)
            .unwrap_or_else(|| {
                objects.push(handle);
                objects.len() - 1
            });
        out.push_str(&rest[..start]);
        out.push_str(&format!("%<\\_ObjIdx {index}>%"));
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    (out, objects)
}

/// The geometric properties a field can show for an object of this type.
/// Its own properties first, then the ones every object has.
pub fn object_properties(entity: &codec::EntityType) -> Vec<&'static str> {
    use codec::EntityType as E;
    let own: &[&'static str] = match entity {
        E::Line(_) => &["Center", "EndPoint", "Length", "StartPoint"],
        E::Circle(_) => &["Area", "Center", "Circumference", "Diameter", "Radius"],
        E::Arc(_) => &["Center", "Length", "Radius"],
        E::Ellipse(_) => &["Area", "Center"],
        E::LwPolyline(_) | E::Polyline2D(_) | E::Spline(_) => &["Area", "Length"],
        E::Hatch(_) => &["Area"],
        E::Insert(_) => &[
            "EffectiveName",
            "InsertionPoint",
            "XScaleFactor",
            "YScaleFactor",
            "ZScaleFactor",
        ],
        E::Text(_) | E::MText(_) => &["InsertionPoint", "TextString"],
        _ => &[],
    };
    let general: [&'static str; 4] = ["Color", "Layer", "Linetype", "LinetypeScale"];
    own.iter().chain(general.iter()).copied().collect()
}

#[derive(Debug, Clone)]
pub enum FieldDialogMsg {
    Category(usize),
    Name(&'static str),
    /// A custom drawing property of the Document category.
    CustomProperty(String),
    DateFormat(String),
    DateExample(usize),
    TextCase(usize),
    FileParts(u8),
    FileExtension(bool),
    SizeUnit(usize),
    SysVar(String),
    Diesel(String),
    NamedType(usize),
    Named(usize),
    SelectObject,
    ObjectProp(&'static str),
    Formula(String),
    FormulaFormat(usize),
    FormulaPrecision(usize),
    Conversion(String),
    Prefix(String),
    Suffix(String),
    DecimalSeparator(usize),
    ThousandsSeparator(usize),
    Coordinates(usize),
    Evaluate,
    /// Sum / Average / Count of a cell range, or one Cell, picked in a table.
    TableFunction(&'static str),
    PlaceholderProperty(usize),
    HyperlinkText(String),
    HyperlinkUrl(String),
    BrowseHyperlink,
    PlotScale(usize),
    CountExpression(String),
    ShowCountInstances,
    SsCustom(String),
    SsPlaceholder(usize),
    SsScaleFormat(usize),
    SsSet(usize),
    SsNode(String),
    SsProp(String),
    SsHref(bool),
    SsBrowse,
    SsPicked(Option<std::path::PathBuf>),
    Help,
    Ok,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Pick(usize, String);

impl std::fmt::Display for Pick {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}

fn msg(m: FieldDialogMsg) -> Message {
    Message::FieldDialog(m)
}

fn card<'a>(title: String, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(
        column![
            text(title.to_uppercase()).size(10).style(accent_text),
            content.into()
        ]
        .spacing(8),
    )
    .padding([10, 12])
    .width(Fill)
    .style(card_style)
    .into()
}

/// A selectable list: one row per item, the chosen one highlighted.
fn list<'a>(rows: Vec<(String, bool, FieldDialogMsg)>, height: f32) -> Element<'a, Message> {
    let rows = rows.into_iter().map(|(label, on, m)| {
        button(text(label).size(12))
            .on_press(msg(m))
            .style(button_style(on))
            .padding([3, 8])
            .width(Fill)
            .into()
    });
    container(scrollable(
        iced::widget::Column::with_children(rows.collect::<Vec<Element<'a, Message>>>()).spacing(1),
    ))
    .height(Length::Fixed(height))
    .padding(2)
    .style(|theme: &iced::Theme| field_style(theme, text_input::Status::Active).into_container())
    .into()
}

trait IntoContainer {
    fn into_container(self) -> container::Style;
}

impl IntoContainer for text_input::Style {
    fn into_container(self) -> container::Style {
        container::Style {
            background: Some(self.background),
            border: self.border,
            ..Default::default()
        }
    }
}

fn table_button<'a>(label: String, function: &'static str) -> Element<'a, Message> {
    button(text(label).size(12))
        .on_press(msg(FieldDialogMsg::TableFunction(function)))
        .style(button_style(false))
        .padding([5, 10])
        .into()
}

fn case_list<'a>(state: &FieldDialogState) -> Element<'a, Message> {
    list(
        TEXT_CASES
            .iter()
            .enumerate()
            .map(|(i, c)| {
                (
                    t!(*c).into_owned(),
                    state.text_case == i,
                    FieldDialogMsg::TextCase(i),
                )
            })
            .collect(),
        130.0,
    )
}

fn format_panel<'a>(state: &'a FieldDialogState) -> Element<'a, Message> {
    let examples = &state.examples;
    match kind_of(state.name) {
        FieldKind::Date => column![
            row![
                text(t!("Date format")).size(12).width(Length::Fixed(96.0)),
                text_input("", &state.date_format)
                    .size(12)
                    .padding([5, 8])
                    .style(field_style)
                    .on_input(|v| msg(FieldDialogMsg::DateFormat(v))),
            ]
            .spacing(8)
            .align_y(iced::Center),
            text(t!("Examples")).size(12),
            list(
                examples
                    .iter()
                    .enumerate()
                    .map(|(i, e)| (
                        e.clone(),
                        DATE_FORMATS.get(i) == Some(&state.date_format.as_str()),
                        FieldDialogMsg::DateExample(i)
                    ))
                    .collect(),
                220.0,
            ),
        ]
        .spacing(8)
        .into(),
        FieldKind::Text | FieldKind::CustomProperty => column![text(t!("Format")).size(12), case_list(state)]
            .spacing(8)
            .into(),
        FieldKind::Filename => {
            let part = |label: &str, bits: u8| {
                checkbox(state.file_parts == bits)
                    .label(t!(label).into_owned())
                    .text_size(12)
                    .size(14)
                    .on_toggle(move |_| msg(FieldDialogMsg::FileParts(bits)))
            };
            let mut ext = checkbox(state.file_extension)
                .label(t!("Display file extension").into_owned())
                .text_size(12)
                .size(14);
            if state.file_parts & 2 != 0 {
                ext = ext.on_toggle(|v| msg(FieldDialogMsg::FileExtension(v)));
            }
            column![
                part("Filename only", 2),
                part("Path only", 1),
                part("Path and filename", 3),
                ext,
                text(t!("Format")).size(12),
                case_list(state),
            ]
            .spacing(8)
            .into()
        }
        FieldKind::Filesize => column![
            text(t!("Format")).size(12),
            list(
                SIZE_UNITS
                    .iter()
                    .enumerate()
                    .map(|(i, u)| (
                        t!(*u).into_owned(),
                        state.size_unit == i,
                        FieldDialogMsg::SizeUnit(i)
                    ))
                    .collect(),
                90.0,
            ),
        ]
        .spacing(8)
        .into(),
        FieldKind::SystemVariable => column![
            text(t!("System variable")).size(12),
            list(
                crate::entities::field::SYSVARS
                    .iter()
                    .map(|v| (
                        v.to_string(),
                        state.sysvar == *v,
                        FieldDialogMsg::SysVar(v.to_string())
                    ))
                    .collect(),
                250.0,
            ),
        ]
        .spacing(8)
        .into(),
        FieldKind::Diesel => column![
            text(t!("Diesel expression")).size(12),
            text_input("$(getvar,dwgname)", &state.diesel)
                .size(12)
                .padding([5, 8])
                .style(field_style)
                .on_input(|v| msg(FieldDialogMsg::Diesel(v))),
        ]
        .spacing(8)
        .into(),
        FieldKind::NamedObject => column![
            row![
                text(t!("Named object type"))
                    .size(12)
                    .width(Length::Fixed(120.0)),
                pick_list(
                    Some(Pick(
                        state.named_type,
                        t!(NAMED_TYPES[state.named_type]).into_owned()
                    )),
                    NAMED_TYPES
                        .iter()
                        .enumerate()
                        .map(|(i, n)| Pick(i, t!(*n).into_owned()))
                        .collect::<Vec<_>>(),
                    |p: &Pick| p.1.clone(),
                )
                .on_select(|p| msg(FieldDialogMsg::NamedType(p.0)))
                .text_size(12)
                .padding([5, 8])
                .width(Fill),
            ]
            .spacing(8)
            .align_y(iced::Center),
            text(t!("Name")).size(12),
            list(
                state
                    .named_names
                    .iter()
                    .enumerate()
                    .map(|(i, (n, _))| (
                        n.clone(),
                        state.named == Some(i),
                        FieldDialogMsg::Named(i)
                    ))
                    .collect(),
                150.0,
            ),
            text(t!("Format")).size(12),
            case_list(state),
        ]
        .spacing(8)
        .into(),
        FieldKind::Formula => number_format_rows(state),
        FieldKind::Hyperlink => column![
            row![
                text(t!("Text to display"))
                    .size(12)
                    .width(Length::Fixed(120.0)),
                text_input("", &state.hyperlink_text)
                    .size(12)
                    .padding([5, 8])
                    .style(field_style)
                    .on_input(|v| msg(FieldDialogMsg::HyperlinkText(v))),
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                text(t!("Address")).size(12).width(Length::Fixed(120.0)),
                text_input("https://", &state.hyperlink_url)
                    .size(12)
                    .padding([5, 8])
                    .font(iced::Font::MONOSPACE)
                    .style(field_style)
                    .on_input(|v| msg(FieldDialogMsg::HyperlinkUrl(v))),
                button(text("...").size(12))
                    .on_press(msg(FieldDialogMsg::BrowseHyperlink))
                    .style(button_style(false))
                    .padding([5, 10]),
            ]
            .spacing(8)
            .align_y(iced::Center),
        ]
        .spacing(8)
        .into(),
        FieldKind::BlockPlaceholder => match &state.placeholder_block {
            None => column![text(t!("Only accessible in the block editor"))
                .size(12)
                .style(muted_style)]
            .into(),
            Some(block) => {
                let (_, _, format) =
                    codec::fields::BLOCK_PLACEHOLDER_PROPERTIES[state.placeholder_property];
                let mut panel = column![
                    row![
                        text(t!("Block name")).size(12).width(Length::Fixed(110.0)),
                        container(text(block.clone()).size(12).style(muted_style))
                            .padding([5, 8])
                            .width(Fill),
                    ]
                    .spacing(8)
                    .align_y(iced::Center),
                    text(t!("Block reference property")).size(12),
                    list(
                        codec::fields::BLOCK_PLACEHOLDER_PROPERTIES
                            .iter()
                            .enumerate()
                            .map(|(i, (label, _, _))| {
                                (
                                    t!(*label).into_owned(),
                                    state.placeholder_property == i,
                                    FieldDialogMsg::PlaceholderProperty(i),
                                )
                            })
                            .collect(),
                        150.0,
                    ),
                ]
                .spacing(8);
                if format == "%tc4" {
                    panel = panel
                        .push(text(t!("Format")).size(12))
                        .push(case_list(state));
                }
                panel.into()
            }
        },
        FieldKind::PlotScale => column![
            text(t!("Format")).size(12),
            list(
                codec::fields::PLOT_SCALE_FORMATS
                    .iter()
                    .enumerate()
                    .map(|(i, (n, _))| (
                        t!(*n).into_owned(),
                        state.plot_scale == i,
                        FieldDialogMsg::PlotScale(i)
                    ))
                    .collect(),
                170.0,
            ),
        ]
        .spacing(8)
        .into(),
        FieldKind::Count => column![].into(),
        FieldKind::SheetSet => column![text(t!("Format")).size(12), case_list(state)]
            .spacing(8)
            .into(),
        FieldKind::SheetSetCustom => {
            let names = if state.name == "CurrentSheetCustom" {
                &state.ss_sheet_custom
            } else {
                &state.ss_set_custom
            };
            column![
                text(t!("Format")).size(12),
                case_list(state),
                text(t!("Custom property name")).size(12),
                pick_list(
                    (!state.ss_custom.is_empty()).then(|| state.ss_custom.clone()),
                    names.clone(),
                    |s: &String| s.clone(),
                )
                .on_select(|s| msg(FieldDialogMsg::SsCustom(s)))
                .text_size(12)
                .padding([5, 8])
                .width(Fill),
            ]
            .spacing(8)
            .into()
        }
        FieldKind::SheetSetPlaceholder => {
            let kind = PLACEHOLDER_TYPES[state.ss_placeholder.min(PLACEHOLDER_TYPES.len() - 1)].0;
            let types = list(
                PLACEHOLDER_TYPES
                    .iter()
                    .enumerate()
                    .map(|(i, (n, _, _))| {
                        (
                            n.to_string(),
                            state.ss_placeholder == i,
                            FieldDialogMsg::SsPlaceholder(i),
                        )
                    })
                    .collect(),
                200.0,
            );
            let format: Element<'a, Message> = if kind == "ViewportScale" {
                list(
                    codec::fields::PLOT_SCALE_FORMATS
                        .iter()
                        .enumerate()
                        .map(|(i, (n, _))| {
                            (
                                t!(*n).into_owned(),
                                state.ss_scale_format == i,
                                FieldDialogMsg::SsScaleFormat(i),
                            )
                        })
                        .collect(),
                    170.0,
                )
            } else {
                case_list(state)
            };
            let mut col = column![
                text(t!("Placeholder type")).size(12),
                types,
                text(t!("Format")).size(12),
                format
            ]
            .spacing(8);
            if kind == "Custom" {
                col = col.push(text(t!("Custom property name")).size(12)).push(
                    pick_list(
                        (!state.ss_custom.is_empty()).then(|| state.ss_custom.clone()),
                        state.ss_sheet_custom.clone(),
                        |s: &String| s.clone(),
                    )
                    .on_select(|s| msg(FieldDialogMsg::SsCustom(s)))
                    .text_size(12)
                    .padding([5, 8])
                    .width(Fill),
                );
            }
            col.into()
        }
        FieldKind::SheetSetNavigation => {
            let picks: Vec<Pick> = state
                .ss_sets
                .iter()
                .enumerate()
                .map(|(i, d)| Pick(i, d.name().to_string()))
                .collect();
            let current = picks.get(state.ss_set).cloned();
            let set_row = row![
                pick_list(current, picks, |p: &Pick| p.1.clone())
                    .on_select(|p: Pick| msg(FieldDialogMsg::SsSet(p.0)))
                    .text_size(12)
                    .padding([5, 8])
                    .width(Fill),
                button(text("...").size(12))
                    .on_press(msg(FieldDialogMsg::SsBrowse))
                    .style(button_style(false))
                    .padding([5, 10]),
            ]
            .spacing(6);
            let db = state.ss_sets.get(state.ss_set);
            let nodes = db
                .map(|d| nav_nodes(d, state.name == "SheetView"))
                .unwrap_or_default();
            let tree = list(
                nodes
                    .iter()
                    .map(|(depth, id, label, _)| {
                        (
                            format!("{}{label}", "    ".repeat(usize::from(*depth))),
                            *id == state.ss_node,
                            FieldDialogMsg::SsNode(id.clone()),
                        )
                    })
                    .collect(),
                118.0,
            );
            let props = db.map(|d| nav_props(d, &state.ss_node)).unwrap_or_default();
            let prop_list = list(
                props
                    .iter()
                    .map(|(l, _, _)| {
                        (
                            l.clone(),
                            *l == state.ss_prop,
                            FieldDialogMsg::SsProp(l.clone()),
                        )
                    })
                    .collect(),
                92.0,
            );
            let linkable = db.is_some_and(|d| nav_link(d, &state.ss_node).is_some())
                && state.ss_prop != "ViewportScale";
            let mut href = checkbox(state.ss_href && linkable)
                .label(t!("Associate hyperlink").into_owned())
                .text_size(12)
                .size(14);
            if linkable {
                href = href.on_toggle(|v| msg(FieldDialogMsg::SsHref(v)));
            }
            let format: Element<'a, Message> = if state.ss_prop == "ViewportScale" {
                list(
                    codec::fields::PLOT_SCALE_FORMATS
                        .iter()
                        .enumerate()
                        .map(|(i, (n, _))| {
                            (
                                t!(*n).into_owned(),
                                state.ss_scale_format == i,
                                FieldDialogMsg::SsScaleFormat(i),
                            )
                        })
                        .collect(),
                    120.0,
                )
            } else {
                case_list(state)
            };
            column![
                text(t!("Sheet set")).size(12),
                set_row,
                text(t!("Sheet navigation tree")).size(12),
                tree,
                text(t!("Property")).size(12),
                prop_list,
                text(t!("Format")).size(12),
                format,
                href,
            ]
            .spacing(8)
            .into()
        }
        FieldKind::Object => column![
            row![
                text(t!("Object type")).size(12).width(Length::Fixed(96.0)),
                container(
                    text(
                        state
                            .object
                            .as_ref()
                            .map(|(_, t)| t.clone())
                            .unwrap_or_default()
                    )
                    .size(12)
                    .style(muted_style)
                )
                .padding([5, 8])
                .width(Fill),
                button(text(t!("Select object")).size(12))
                    .on_press(msg(FieldDialogMsg::SelectObject))
                    .style(button_style(false))
                    .padding([5, 10]),
            ]
            .spacing(8)
            .align_y(iced::Center),
            text(t!("Property")).size(12),
            list(
                state
                    .object_props
                    .iter()
                    .map(|p| (
                        t!(*p).into_owned(),
                        state.object_prop == Some(*p),
                        FieldDialogMsg::ObjectProp(p)
                    ))
                    .collect(),
                180.0,
            ),
            if state
                .object_prop
                .is_some_and(|p| TEXT_PROPERTIES.contains(&p))
            {
                column![text(t!("Format")).size(12), case_list(state)]
                    .spacing(8)
                    .into()
            } else {
                number_format_rows(state)
            },
        ]
        .spacing(8)
        .into(),
    }
}

/// Format and Precision of a number, for a formula or an object property,
/// with its additional format and, for a point, the coordinates it shows.
fn number_format_rows(state: &FieldDialogState) -> Element<'_, Message> {
    const LABEL: f32 = 120.0;
    let formats: Vec<Pick> = codec::fields::FORMULA_FORMATS
        .iter()
        .enumerate()
        .map(|(i, (n, _))| Pick(i, t!(*n).into_owned()))
        .collect();
    let precisions: Vec<Pick> = FORMULA_PRECISIONS
        .iter()
        .enumerate()
        .map(|(i, n)| Pick(i, t!(*n).into_owned()))
        .collect();
    // "Comma (,)": the name and the character it writes.
    let separator = |name: &str, c: char| match c {
        ' ' => t!(name).into_owned(),
        c => format!("{} ({c})", t!(name)),
    };
    let decimals: Vec<Pick> = DECIMAL_SEPARATORS
        .iter()
        .enumerate()
        .map(|(i, (n, c))| Pick(i, separator(n, *c)))
        .collect();
    let thousands: Vec<Pick> = THOUSANDS_SEPARATORS
        .iter()
        .enumerate()
        .map(|(i, (n, c))| {
            Pick(
                i,
                c.map_or_else(|| t!(*n).into_owned(), |c| separator(n, c)),
            )
        })
        .collect();
    let choice = |label: &str, picks: Vec<Pick>, at: usize, on: fn(usize) -> FieldDialogMsg| {
        row![
            text(t!(label)).size(12).width(Length::Fixed(LABEL)),
            pick_list(picks.get(at).cloned(), picks, |p: &Pick| p.1.clone())
                .on_select(move |p| msg(on(p.0)))
                .text_size(12)
                .padding([5, 8])
                .width(Fill),
        ]
        .spacing(8)
        .align_y(iced::Center)
    };
    let entry = |label: &str, value: &str, on: fn(String) -> FieldDialogMsg| {
        row![
            text(t!(label)).size(12).width(Length::Fixed(LABEL)),
            text_input("", value)
                .size(12)
                .padding([5, 8])
                .style(field_style)
                .on_input(move |v| msg(on(v))),
        ]
        .spacing(8)
        .align_y(iced::Center)
    };
    let mut rows = column![
        choice(
            "Format",
            formats,
            state.formula_format,
            FieldDialogMsg::FormulaFormat
        ),
        choice(
            "Precision",
            precisions,
            state.formula_precision,
            FieldDialogMsg::FormulaPrecision
        ),
    ]
    .spacing(8);
    let point = kind_of(state.name) == FieldKind::Object
        && state
            .object_prop
            .is_some_and(|p| POINT_PROPERTIES.contains(&p));
    if point {
        let coordinates: Vec<Pick> = COORDINATES
            .iter()
            .enumerate()
            .map(|(i, (n, _))| Pick(i, (*n).to_string()))
            .collect();
        rows = rows.push(choice(
            "Coordinates",
            coordinates,
            state.coordinates,
            FieldDialogMsg::Coordinates,
        ));
    }
    rows.push(
        text(t!("Additional format").to_uppercase())
            .size(10)
            .style(accent_text),
    )
    .push(entry(
        "Conversion factor",
        &state.conversion,
        FieldDialogMsg::Conversion,
    ))
    .push(entry("Prefix", &state.prefix, FieldDialogMsg::Prefix))
    .push(entry("Suffix", &state.suffix, FieldDialogMsg::Suffix))
    .push(choice(
        "Decimal separator",
        decimals,
        state.decimal_separator,
        FieldDialogMsg::DecimalSeparator,
    ))
    .push(choice(
        "Thousands separator",
        thousands,
        state.thousands_separator,
        FieldDialogMsg::ThousandsSeparator,
    ))
    .into()
}

pub fn view<'a>(
    state: &'a FieldDialogState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let categories: Vec<Pick> = CATEGORIES
        .iter()
        .enumerate()
        .map(|(i, (c, _))| Pick(i, t!(*c).into_owned()))
        .collect();
    let left = card(
        t!("Field").into_owned(),
        column![
            row![
                text(t!("Category")).size(12).width(Length::Fixed(70.0)),
                pick_list(
                    Some(categories[state.category].clone()),
                    categories.clone(),
                    |p: &Pick| p.1.clone()
                )
                .on_select(|p| msg(FieldDialogMsg::Category(p.0)))
                .text_size(12)
                .padding([5, 8])
                .width(Fill),
            ]
            .spacing(8)
            .align_y(iced::Center),
            list(state.entries(), 330.0),
        ]
        .spacing(8),
    );
    let preview = card(
        t!("Preview").into_owned(),
        container(
            text(if state.preview.is_empty() {
                "----".to_string()
            } else {
                state.preview.clone()
            })
            .size(15),
        )
        .padding([6, 8])
        .width(Fill),
    );
    let right = match kind_of(state.name) {
        FieldKind::Formula => column![
            card(
                t!("Formula").into_owned(),
                column![
                    text_input("(12+3)*2", &state.formula)
                        .size(12)
                        .padding([8, 8])
                        .font(iced::Font::MONOSPACE)
                        .style(field_style)
                        .on_input(|v| msg(FieldDialogMsg::Formula(v))),
                    row![
                        table_button(t!("Average").into_owned(), "Average"),
                        table_button(t!("Sum").into_owned(), "Sum"),
                        table_button(t!("Count").into_owned(), "Count"),
                        table_button(t!("Cell").into_owned(), "Cell"),
                        Space::new().width(Fill),
                        button(text(t!("Evaluate")).size(12))
                            .on_press(msg(FieldDialogMsg::Evaluate))
                            .style(button_style(false))
                            .padding([5, 14]),
                    ]
                    .spacing(6),
                ]
                .spacing(8),
            ),
            card(t!("Format").into_owned(), format_panel(state)),
            preview,
        ]
        .spacing(10),
        FieldKind::Hyperlink => column![
            card(t!("Hyperlink").into_owned(), format_panel(state)),
            preview
        ]
        .spacing(10),
        FieldKind::Count => column![
            card(
                t!("Expression").into_owned(),
                column![
                    text_input("", &state.count_expression)
                        .size(12)
                        .padding([8, 8])
                        .font(iced::Font::MONOSPACE)
                        .style(field_style)
                        .on_input(|v| msg(FieldDialogMsg::CountExpression(v))),
                    row![
                        button(text(t!("Evaluate")).size(12))
                            .on_press(msg(FieldDialogMsg::Evaluate))
                            .style(button_style(false))
                            .padding([5, 14]),
                        button(text(t!("Show Count Instances")).size(12))
                            .on_press(msg(FieldDialogMsg::ShowCountInstances))
                            .style(button_style(false))
                            .padding([5, 14]),
                    ]
                    .spacing(8),
                ]
                .spacing(8),
            ),
            preview,
        ]
        .spacing(10),
        FieldKind::BlockPlaceholder => column![
            card(t!("Block placeholder").into_owned(), format_panel(state)),
            preview
        ]
        .spacing(10),
        FieldKind::SheetSetNavigation => column![
            card(t!("Sheet set").into_owned(), format_panel(state)),
            preview
        ]
        .spacing(10),
        _ => column![
            card(t!("Format").into_owned(), format_panel(state)),
            preview
        ]
        .spacing(10),
    };
    let (code, _) = state.code();
    let expression = column![
        text(t!("Field expression")).size(12),
        container(
            text(format!("%<{code}>%"))
                .size(12)
                .font(iced::Font::MONOSPACE)
                .style(muted_style)
        )
        .padding([5, 8])
        .width(Fill)
        .style(
            |theme: &iced::Theme| field_style(theme, text_input::Status::Disabled).into_container()
        ),
    ]
    .spacing(4);
    let mut insert = button(text(t!("Insert")).size(12))
        .style(button_style(true))
        .padding([6, 16]);
    if state.complete() {
        insert = insert.on_press(msg(FieldDialogMsg::Ok));
    }
    let footer = row![
        button(text("?").size(12))
            .on_press(msg(FieldDialogMsg::Help))
            .style(button_style(false))
            .padding([5, 11]),
        Space::new().width(Fill),
        dialog_button(t!("Cancel"), Message::CloseModal, false),
        insert,
    ]
    .spacing(6)
    .align_y(iced::Center);
    column![
        row![container(left).width(Length::Fixed(240.0)), right].spacing(10),
        expression,
        footer,
    ]
    .spacing(10)
    .padding([10, 12])
    .width(sizing.width)
    .into()
}

#[cfg(test)]
mod tests {
    use super::{FieldDialogState, FieldTarget};

    /// Open `code` in the dialog as the edit of an existing field would, and
    /// give back the code the dialog writes when nothing is changed.
    fn reopened(code: &str, objects: &[codec::Handle], props: &[&'static str]) -> String {
        let mut state = FieldDialogState::new(FieldTarget::NewText);
        if let Some(property) = state.seed_from_code(code, objects) {
            state.object = Some((objects[0], "Line".into()));
            state.object_props = props.to_vec();
            state.object_prop = props.iter().copied().find(|p| *p == property);
        }
        let (written, written_objects) = state.code();
        assert_eq!(written_objects, objects);
        written
    }

    #[test]
    fn a_level_mark_keeps_its_ucs_coordinate_and_prefix() {
        // A level mark: the end point's Y in the current UCS, after "+".
        let code = "\\AcObjProp.16.2 Object(%<\\_ObjIdx 0>%).EndPoint \\f \"%lu6%pt2%ps[+,]\"";
        let line = codec::Handle::new(0x2A);
        let props = ["Center", "EndPoint", "Length", "StartPoint"];
        assert_eq!(reopened(code, &[line], &props), code);
    }

    #[test]
    fn a_formula_keeps_its_conversion_factor_and_separators() {
        for code in [
            // A door width in metres shown in centimetres.
            "\\AcExpr (0.9*1) \\f \"%lu2%pr0%ct8[100]\"",
            "\\AcExpr (1234.5) \\f \"%lu2%pr2%ds44%th46\"",
            "\\AcExpr (74.08) \\f \"%lu2%pr1%ps[,m²]\"",
        ] {
            assert_eq!(reopened(code, &[], &[]), code);
        }
    }

    #[test]
    fn the_additional_format_goes_into_the_picture() {
        let mut state = FieldDialogState::new(FieldTarget::NewText);
        state.name = "Object";
        state.object = Some((codec::Handle::new(0x2A), "Polyline".into()));
        state.object_props = vec!["Area", "Length"];
        state.object_prop = Some("Area");
        state.formula_format = 2;
        state.formula_precision = 3;
        state.conversion = "0.0001".into();
        // A comma would end the prefix; a bracket would end the pair.
        state.prefix = "A,]".into();
        state.suffix = " m²".into();
        state.decimal_separator = 1;
        state.thousands_separator = 2;
        // Coordinates belong to points only.
        state.coordinates = 3;
        assert_eq!(
            state.code().0,
            "\\AcObjProp Object(%<\\_ObjIdx 0>%).Area \\f \"%lu2%pr2%ds44%th46%ct8[0.0001]%ps[A, m²]\""
        );

        // A text property takes a case instead of a number format.
        state.object_prop = Some("Layer");
        state.text_case = 1;
        assert_eq!(
            state.code().0,
            "\\AcObjProp Object(%<\\_ObjIdx 0>%).Layer \\f \"%tc1\""
        );
        state.object_prop = Some("Area");
        state.text_case = 0;

        // A factor of one, or one that is not a number, converts nothing.
        for factor in ["1", "abc", ""] {
            state.conversion = factor.into();
            assert!(!state.code().0.contains("%ct"), "{factor}");
        }
    }

    #[test]
    fn a_new_point_field_reads_the_current_ucs() {
        let mut state = FieldDialogState::new(FieldTarget::NewText);
        state.name = "Object";
        state.object = Some((codec::Handle::new(0x2A), "Block reference".into()));
        state.object_prop = Some("InsertionPoint");
        assert!(state.code().0.starts_with("\\AcObjProp.16.2 "));
        state.object_prop = Some("XScaleFactor");
        assert!(state.code().0.starts_with("\\AcObjProp "));
    }

    #[test]
    fn a_block_reference_offers_its_own_and_the_general_properties() {
        use codec::entities::Insert;
        use codec::types::Vector3;
        let door = codec::EntityType::Insert(Insert::new("DOOR", Vector3::ZERO));
        assert_eq!(
            super::object_properties(&door),
            [
                "EffectiveName",
                "InsertionPoint",
                "XScaleFactor",
                "YScaleFactor",
                "ZScaleFactor",
                "Color",
                "Layer",
                "Linetype",
                "LinetypeScale",
            ]
        );
    }
}
