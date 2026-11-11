//! Declarative dialog specification and response types for Open CAD Studio plugins.
//!
//! Plugins define dialogs using serializable data structures: rows of controls
//! such as text inputs, numeric inputs, checkboxes, dropdown choices, color pickers,
//! and file pickers. The host renders these natively in its own UI theme and returns
//! user-entered values upon submission.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::ribbon::owned::OwnedIconKind;

/// Horizontal alignment / justification for rows, columns, and controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum HorizontalAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// Vertical alignment for controls within rows or columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum VerticalAlign {
    Top,
    #[default]
    Middle,
    Bottom,
}

/// A single interactive control inside a dialog row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DialogControl {
    /// Informational / descriptive static text.
    InfoText {
        text: String,
        #[serde(default)]
        align_h: Option<HorizontalAlign>,
    },
    /// Single-line text input field.
    Text {
        id: String,
        label: String,
        default: String,
    },
    /// Numeric floating-point or integer input field with optional bounds and step.
    Number {
        id: String,
        label: String,
        default: f64,
        min: Option<f64>,
        max: Option<f64>,
        step: Option<f64>,
    },
    /// Boolean checkbox toggle.
    Checkbox {
        id: String,
        label: String,
        default: bool,
    },
    /// Dropdown / pick-list choice.
    Choice {
        id: String,
        label: String,
        options: Vec<String>,
        default_index: usize,
    },
    /// Color picker (RGB [0..255]).
    Color {
        id: String,
        label: String,
        default: [u8; 3],
    },
    /// File picker button and path entry.
    FilePicker {
        id: String,
        label: String,
        default_path: String,
        filter_name: String,
        extensions: Vec<String>,
        save: bool,
    },
    /// Grouped property fields section with aligned 2-column key-value layout.
    Fields {
        id: String,
        title: Option<String>,
        controls: Vec<DialogControl>,
        #[serde(default)]
        collapsed: bool,
    },
    /// Interactive tabular data grid with column headers and editable cells.
    Table {
        id: String,
        label: Option<String>,
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
        #[serde(default = "default_true")]
        allow_add: bool,
        #[serde(default = "default_true")]
        allow_delete: bool,
    },
    /// Action button (useful in panels and custom forms).
    Button {
        id: String,
        label: String,
        #[serde(default)]
        action: Option<String>,
        #[serde(default)]
        align_h: Option<HorizontalAlign>,
        #[serde(default)]
        width: Option<f32>,
    },
    /// A horizontal row grouping of controls.
    Row {
        controls: Vec<DialogControl>,
        #[serde(default)]
        align_h: Option<HorizontalAlign>,
        #[serde(default)]
        align_v: Option<VerticalAlign>,
    },
    /// A vertical column grouping of controls.
    Column {
        controls: Vec<DialogControl>,
        #[serde(default)]
        align_h: Option<HorizontalAlign>,
        #[serde(default)]
        align_v: Option<VerticalAlign>,
    },
}

fn default_true() -> bool {
    true
}

impl DialogControl {
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Row { .. } | Self::Column { .. } => None,
            Self::InfoText { text, .. } => {
                if let Some((id, _)) = text.split_once("::") {
                    if !id.is_empty() && !id.contains(' ') && !id.contains('\n') {
                        return Some(id);
                    }
                }
                None
            }
            Self::Text { id, .. }
            | Self::Number { id, .. }
            | Self::Checkbox { id, .. }
            | Self::Choice { id, .. }
            | Self::Color { id, .. }
            | Self::FilePicker { id, .. }
            | Self::Fields { id, .. }
            | Self::Table { id, .. }
            | Self::Button { id, .. } => Some(id.as_str()),
        }
    }

    pub fn row(controls: Vec<DialogControl>) -> Self {
        Self::Row {
            controls,
            align_h: None,
            align_v: None,
        }
    }

    pub fn column(controls: Vec<DialogControl>) -> Self {
        Self::Column {
            controls,
            align_h: None,
            align_v: None,
        }
    }
}

/// A horizontal row of controls inside a dialog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DialogRow {
    pub controls: Vec<DialogControl>,
    #[serde(default)]
    pub align_h: Option<HorizontalAlign>,
    #[serde(default)]
    pub align_v: Option<VerticalAlign>,
}

impl DialogRow {
    pub fn new(controls: Vec<DialogControl>) -> Self {
        Self {
            controls,
            align_h: None,
            align_v: None,
        }
    }

    pub fn single(control: DialogControl) -> Self {
        Self {
            controls: vec![control],
            align_h: None,
            align_v: None,
        }
    }

    pub fn with_align_h(mut self, align_h: HorizontalAlign) -> Self {
        self.align_h = Some(align_h);
        self
    }

    pub fn with_align_v(mut self, align_v: VerticalAlign) -> Self {
        self.align_v = Some(align_v);
        self
    }
}

/// Definition of a modal dialog requested by a plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DialogDef {
    /// Window title.
    pub title: String,
    /// Vertical sequence of rows.
    pub rows: Vec<DialogRow>,
    /// Label for the confirmation button (defaults to "OK").
    pub submit_label: String,
    /// Label for the cancel button (defaults to "Cancel").
    pub cancel_label: String,
    /// Optional dialog window width in logical pixels.
    #[serde(default)]
    pub width: Option<f32>,
}

impl DialogDef {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            rows: Vec::new(),
            submit_label: "OK".to_string(),
            cancel_label: "Cancel".to_string(),
            width: None,
        }
    }

    pub fn with_row(mut self, row: DialogRow) -> Self {
        self.rows.push(row);
        self
    }

    pub fn with_control(mut self, control: DialogControl) -> Self {
        self.rows.push(DialogRow::single(control));
        self
    }

    pub fn submit_label(mut self, label: impl Into<String>) -> Self {
        self.submit_label = label.into();
        self
    }

    pub fn cancel_label(mut self, label: impl Into<String>) -> Self {
        self.cancel_label = label.into();
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
}

/// Definition of a persistent dockable side panel / palette.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanelDef {
    /// Unique identifier for the dock panel (e.g. "sample_tool").
    pub id: String,
    /// Title displayed in the dock header bar and rail tab.
    pub title: String,
    /// Sequential list of controls inside the panel.
    pub controls: Vec<DialogControl>,
    /// Optional default width in logical pixels.
    #[serde(default)]
    pub width: Option<f32>,
    /// Optional command to open / toggle this panel from the CAD command prompt.
    #[serde(default)]
    pub command: Option<String>,
}

impl PanelDef {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            controls: Vec::new(),
            width: None,
            command: None,
        }
    }

    pub fn with_control(mut self, control: DialogControl) -> Self {
        self.controls.push(control);
        self
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }
}

// ── Quad cursor menu panels (API v15, append-only) ──────────────────────────

/// Declarative quad cursor menu panel contributed by a plugin (API v15).
///
/// The wire shape is owned end to end, exactly like [`PanelDef`]: a plugin
/// declares the panel once at registration (`PluginRequest::
/// RegisterQuadPanels`) and every later quad open reads the host-side session
/// copy — no per-hover traffic, the host resolves `applies_to` at quad-open
/// time. The icon rides [`OwnedIconKind`] (the ribbon tools' proven SVG-on-IPC
/// encoding); an [`crate::ribbon::IconKind`] authoring value converts with
/// `Into`.
///
/// Identity law: the host exposes the panel as `plugin:<plugin_id>:<panel_id>`
/// (the namespacing guard), so plugin panels can never collide with the
/// host's builtin quad panels; one plugin owns a raw `panel_id` at a time and
/// a batch registration replaces it per-panel for the same owner only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuadPanelDef {
    /// Raw panel id as the plugin names it (e.g. "align_tools").
    pub panel_id: String,
    /// Title shown as the quad tab strip label.
    pub title: String,
    /// Host entity type keys this panel's tab is offered for (`entity_type_name()`
    /// vocabulary, e.g. "LWPOLYLINE"), or the plugin's own declared class ids;
    /// `"*"` (and an empty list) mean universal.
    #[serde(default)]
    pub applies_to: Vec<String>,
    /// The panel's items in declared order.
    #[serde(default)]
    pub items: Vec<QuadItemDef>,
}

/// One item in a plugin quad panel (API v15).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuadItemDef {
    /// Raw control id (e.g. "flip"); unique inside the panel by registration
    /// contract (duplicates are refused).
    pub control_id: String,
    /// Label shown on the tile tooltip.
    pub label: String,
    /// Icon source; [`OwnedIconKind::Svg`] carries raw `<svg>` bytes
    /// (≤ 8 KiB is the registration budget — an oversized or unparseable
    /// icon degrades to a glyph tile instead of refusing the item).
    pub icon: OwnedIconKind,
    /// What picking the item does.
    pub action: QuadActionDef,
}

/// A quad item's action (API v15).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuadActionDef {
    /// Launch the verb through the host's ordinary command path (undo,
    /// interactivity and prompt handling are the host's, per API v7's
    /// panel-toggle precedent).
    Command(String),
    /// A plugin callback: the host routes the pick to the OWNING plugin as
    /// `HostNotification::QuadAction` and the plugin responds however it
    /// wants (fetching context via `selection()` / `entities_batch()` — no
    /// streaming).
    Callback,
}

impl QuadPanelDef {
    pub fn new(panel_id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            panel_id: panel_id.into(),
            title: title.into(),
            applies_to: Vec::new(),
            items: Vec::new(),
        }
    }

    /// Restrict the panel to specific entity type keys (see the field docs;
    /// `"*"` or an empty list is universal).
    pub fn with_applies_to(mut self, applies_to: Vec<String>) -> Self {
        self.applies_to = applies_to;
        self
    }

    pub fn with_item(mut self, item: QuadItemDef) -> Self {
        self.items.push(item);
        self
    }
}

/// Value returned by an interactive control.
///
/// Compatibility rule: new variants MUST be appended at the end so existing
/// plugins keep their bincode discriminant indices (`0..=7`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DialogValue {
    Text(String),
    Number(f64),
    Bool(bool),
    Choice { index: usize, text: String },
    Color([u8; 3]),
    FilePath(String),
    Fields(HashMap<String, DialogValue>),
    Table(Vec<Vec<String>>),
    /// Dynamically replaces the options list and selected index of a Choice control.
    ChoiceOptions {
        options: Vec<String>,
        selected_index: usize,
    },
    /// Dynamically enables or disables a control in an open dialog or panel.
    Enabled(bool),
    /// Dynamically shows or hides a control in an open dialog or panel.
    Visible(bool),
}

/// An interaction event emitted by a control inside an open dialog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DialogEvent {
    /// Identifier of the control that triggered the event.
    pub control_id: String,
    /// Action verb: "click", "pick", "change", "toggle", "submit", "add_row", "remove_row".
    pub action: String,
    /// New or associated value, if any.
    pub value: Option<DialogValue>,
    /// Snapshot of all current dialog values at the moment the event occurred.
    pub current_values: HashMap<String, DialogValue>,
}

/// The plugin's response to a [`DialogEvent`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DialogActionResponse {
    /// Keep dialog open; update controls with new values in-place with zero flicker.
    UpdateValues(HashMap<String, DialogValue>),
    /// Keep dialog open; replace layout with a new DialogDef.
    ReplaceDialog(Box<DialogDef>),
    /// Close dialog and submit current values (as if user clicked OK).
    Submit,
    /// Close dialog without submitting (as if user clicked Cancel / Esc).
    Close,
    /// No changes to the dialog.
    None,
}

/// Response returned to the plugin after dialog interaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DialogResponse {
    /// The user closed or cancelled the dialog (e.g. Cancel button, Esc key).
    Cancelled,
    /// The user submitted the dialog with field values.
    Submitted(HashMap<String, DialogValue>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_def_row_construction_and_bincode_roundtrip() {
        let dialog = DialogDef::new("Beam Generator")
            .submit_label("Create Beam")
            .cancel_label("Abort")
            .with_row(DialogRow::new(vec![
                DialogControl::Number {
                    id: "width".to_string(),
                    label: "Width (mm):".to_string(),
                    default: 200.0,
                    min: Some(50.0),
                    max: Some(1000.0),
                    step: Some(10.0),
                },
                DialogControl::Number {
                    id: "height".to_string(),
                    label: "Height (mm):".to_string(),
                    default: 400.0,
                    min: Some(100.0),
                    max: Some(2000.0),
                    step: Some(10.0),
                },
            ]))
            .with_control(DialogControl::Choice {
                id: "material".to_string(),
                label: "Material:".to_string(),
                options: vec!["Concrete".to_string(), "Steel".to_string(), "Timber".to_string()],
                default_index: 0,
            })
            .with_control(DialogControl::Checkbox {
                id: "reinforced".to_string(),
                label: "Reinforced:".to_string(),
                default: true,
            });

        assert_eq!(dialog.rows.len(), 3);
        assert_eq!(dialog.rows[0].controls.len(), 2);
        assert_eq!(dialog.rows[1].controls.len(), 1);

        // Bincode roundtrip
        let bytes = bincode::serialize(&dialog).expect("serialize dialog");
        let restored: DialogDef = bincode::deserialize(&bytes).expect("deserialize dialog");
        assert_eq!(dialog, restored);

        // Response roundtrip
        let mut values = HashMap::new();
        values.insert("width".to_string(), DialogValue::Number(200.0));
        values.insert("height".to_string(), DialogValue::Number(400.0));
        values.insert(
            "material".to_string(),
            DialogValue::Choice {
                index: 1,
                text: "Steel".to_string(),
            },
        );
        values.insert("reinforced".to_string(), DialogValue::Bool(true));
        let response = DialogResponse::Submitted(values);

        let resp_bytes = bincode::serialize(&response).expect("serialize response");
        let restored_resp: DialogResponse =
            bincode::deserialize(&resp_bytes).expect("deserialize response");
        assert_eq!(response, restored_resp);
    }

    #[test]
    fn quad_panel_def_roundtrips_over_bincode() {
        // API v15: the quad wire shape (owned strings, OwnedIconKind icon)
        // rides the same serializable-data discipline as PanelDef above.
        let panel = QuadPanelDef::new("align_tools", "Align")
            .with_applies_to(vec!["LWPOLYLINE".to_string(), "PLUGIN_ALIGN_DISPLAY".to_string()])
            .with_item(QuadItemDef {
                control_id: "flip".to_string(),
                label: "Flip".to_string(),
                icon: crate::ribbon::IconKind::Glyph("F").into(),
                action: QuadActionDef::Command("FLIPALIGNED".to_string()),
            })
            .with_item(QuadItemDef {
                control_id: "report".to_string(),
                label: "Report".to_string(),
                icon: OwnedIconKind::Svg("<svg/>".into()),
                action: QuadActionDef::Callback,
            });

        assert_eq!(panel.items.len(), 2);
        let bytes = bincode::serialize(&panel).expect("serialize quad panel");
        let restored: QuadPanelDef = bincode::deserialize(&bytes).expect("deserialize quad panel");
        assert_eq!(panel, restored);
    }

    #[test]
    fn fields_and_table_bincode_roundtrip() {
        let dialog = DialogDef::new("BOM Inspector")
            .width(600.0)
            .with_control(DialogControl::Fields {
                id: "geom".to_string(),
                title: Some("Geometry Properties".to_string()),
                controls: vec![
                    DialogControl::Number {
                        id: "length".to_string(),
                        label: "Length:".to_string(),
                        default: 100.0,
                        min: Some(0.0),
                        max: None,
                        step: None,
                    },
                    DialogControl::Text {
                        id: "name".to_string(),
                        label: "Part Name:".to_string(),
                        default: "Bracket".to_string(),
                    },
                ],
                collapsed: false,
            })
            .with_control(DialogControl::Table {
                id: "bom".to_string(),
                label: Some("Bill of Materials".to_string()),
                headers: vec!["Item".to_string(), "Qty".to_string(), "Material".to_string()],
                rows: vec![
                    vec!["Bolt M6".to_string(), "8".to_string(), "Steel".to_string()],
                    vec!["Nut M6".to_string(), "8".to_string(), "Steel".to_string()],
                ],
                allow_add: true,
                allow_delete: true,
            });

        let bytes = bincode::serialize(&dialog).expect("serialize dialog");
        let restored: DialogDef = bincode::deserialize(&bytes).expect("deserialize dialog");
        assert_eq!(dialog, restored);

        let mut field_map = HashMap::new();
        field_map.insert("length".to_string(), DialogValue::Number(100.0));
        field_map.insert("name".to_string(), DialogValue::Text("Bracket".to_string()));

        let mut values = HashMap::new();
        values.insert("geom".to_string(), DialogValue::Fields(field_map));
        values.insert("bom".to_string(), DialogValue::Table(vec![
            vec!["Bolt M6".to_string(), "10".to_string(), "Steel".to_string()],
        ]));
        let response = DialogResponse::Submitted(values);

        let resp_bytes = bincode::serialize(&response).expect("serialize response");
        let restored_resp: DialogResponse =
            bincode::deserialize(&resp_bytes).expect("deserialize response");
        assert_eq!(response, restored_resp);
    }

    #[test]
    fn dialog_event_and_action_response_bincode_roundtrip() {
        let mut current_values = HashMap::new();
        current_values.insert("count".to_string(), DialogValue::Number(42.0));
        current_values.insert(
            "category".to_string(),
            DialogValue::ChoiceOptions {
                options: vec!["A".to_string(), "B".to_string()],
                selected_index: 1,
            },
        );
        current_values.insert("btn".to_string(), DialogValue::Enabled(false));
        current_values.insert("section".to_string(), DialogValue::Visible(true));

        let event = DialogEvent {
            control_id: "pick_file".to_string(),
            action: "pick".to_string(),
            value: Some(DialogValue::FilePath("C:/scripts/test.lsp".to_string())),
            current_values: current_values.clone(),
        };

        let evt_bytes = bincode::serialize(&event).expect("serialize event");
        let restored_evt: DialogEvent = bincode::deserialize(&evt_bytes).expect("deserialize event");
        assert_eq!(event, restored_evt);

        let mut update_values = HashMap::new();
        update_values.insert(
            "startup_scripts".to_string(),
            DialogValue::Table(vec![vec!["C:/scripts/test.lsp".to_string()]]),
        );
        update_values.insert("status_label".to_string(), DialogValue::Text("Script added".to_string()));

        let action_resp = DialogActionResponse::UpdateValues(update_values);
        let resp_bytes = bincode::serialize(&action_resp).expect("serialize action response");
        let restored_resp: DialogActionResponse =
            bincode::deserialize(&resp_bytes).expect("deserialize action response");
        assert_eq!(action_resp, restored_resp);
    }

    #[test]
    fn dialog_value_and_info_text_bincode_wire_compatibility() {
        let cases: Vec<(u32, DialogValue)> = vec![
            (0, DialogValue::Text("hello".into())),
            (1, DialogValue::Number(1.5)),
            (2, DialogValue::Bool(true)),
            (
                3,
                DialogValue::Choice {
                    index: 0,
                    text: "A".into(),
                },
            ),
            (4, DialogValue::Color([255, 0, 128])),
            (5, DialogValue::FilePath("a.lsp".into())),
            (6, DialogValue::Fields(HashMap::new())),
            (7, DialogValue::Table(vec![])),
            (
                8,
                DialogValue::ChoiceOptions {
                    options: vec!["X".into()],
                    selected_index: 0,
                },
            ),
            (9, DialogValue::Enabled(false)),
            (10, DialogValue::Visible(true)),
        ];

        for (expected_discriminant, val) in cases {
            let bytes = bincode::serialize(&val).expect("serialize DialogValue");
            let disc = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            assert_eq!(
                disc, expected_discriminant,
                "DialogValue discriminant mismatch for {val:?}"
            );
        }

        // Old plugin's InfoText layout: discriminant 0u32 + String + Option<HorizontalAlign>
        let mut legacy_info_bytes = Vec::new();
        legacy_info_bytes.extend_from_slice(&0u32.to_le_bytes());
        legacy_info_bytes
            .extend_from_slice(&bincode::serialize(&"Hello".to_string()).unwrap());
        legacy_info_bytes
            .extend_from_slice(&bincode::serialize(&Some(HorizontalAlign::Center)).unwrap());

        let decoded: DialogControl =
            bincode::deserialize(&legacy_info_bytes).expect("decode legacy InfoText");
        assert_eq!(
            decoded,
            DialogControl::InfoText {
                text: "Hello".to_string(),
                align_h: Some(HorizontalAlign::Center),
            }
        );
    }
}

