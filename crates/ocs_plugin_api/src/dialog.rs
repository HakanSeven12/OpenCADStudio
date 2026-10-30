//! Declarative dialog specification and response types for Open CAD Studio plugins.
//!
//! Plugins define dialogs using serializable data structures: rows of controls
//! such as text inputs, numeric inputs, checkboxes, dropdown choices, color pickers,
//! and file pickers. The host renders these natively in its own UI theme and returns
//! user-entered values upon submission.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
            Self::InfoText { .. } | Self::Row { .. } | Self::Column { .. } => None,
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

/// Value returned by an interactive control.
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
}

