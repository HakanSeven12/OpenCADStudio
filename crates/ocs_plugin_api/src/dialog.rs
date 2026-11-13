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

/// Emphasis of a [`DialogControl::StyledText`] (API v19).
///
/// The host maps each style onto its theme (font weight, size and the theme's
/// success/warning/danger colours), so plugins state intent, not colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextStyle {
    /// Regular body text at full contrast.
    #[default]
    Normal,
    /// De-emphasised secondary text (the look of [`DialogControl::InfoText`]).
    Muted,
    /// Bold body text, e.g. a key figure.
    Strong,
    /// A larger bold heading that introduces a group of controls.
    Heading,
    /// A positive state ("check passed"), bold in the theme's success colour.
    Success,
    /// A state that needs attention, bold in the theme's warning colour.
    Warning,
    /// A failed state, bold in the theme's danger colour.
    Danger,
}

/// Maximum nesting depth accepted when decoding dialog/panel UI data. Real
/// dialogs nest a handful of rows/fields deep; a hostile frame that nests
/// `Row`/`Fields` controls tens of thousands deep previously overflowed the
/// decoding thread's stack (which aborts the host/runner process). The cap is
/// well above every legitimate layout and far below any stack limit.
pub(crate) const MAX_DIALOG_NESTING: usize = 64;

/// Maximum total control/value nodes accepted per top-level decoded dialog
/// value, bounding allocation and CPU magnification from shared sub-blocks.
pub(crate) const MAX_DIALOG_NODES: usize = 10_000;

/// Per-thread `DialogControl`/`DialogValue` decode budget. `depth` is the
/// current recursion depth (0 outside a decode); `nodes` counts total dialog
/// nodes decoded within the current frame (re-zeroed by
/// [`dialog_decode_frame_begin`] at each frame boundary).
#[derive(Clone, Copy)]
struct DialogDecodeBudget {
    depth: usize,
    nodes: usize,
}

thread_local! {
    static DIALOG_DECODE_BUDGET: std::cell::Cell<DialogDecodeBudget> =
        const { std::cell::Cell::new(DialogDecodeBudget { depth: 0, nodes: 0 }) };
}

/// Reset the per-thread decode budget at a frame boundary: call this right
/// before decoding one whole wire frame so the node budget applies per frame
/// rather than accumulating across frames.
///
/// The production callers (the wire transports) only exist in the `host`
/// feature build; the feature-less data-contract build keeps the helper behind
/// the same gate (review finding 15) while tests still reference it.
#[cfg(any(feature = "host", test))]
pub(crate) fn dialog_decode_frame_begin() {
    DIALOG_DECODE_BUDGET.with(|budget| {
        budget.set(DialogDecodeBudget { depth: 0, nodes: 0 });
    });
}

/// Enter one dialog node during a decode. Returns `Err` when the payload
/// exceeds the nesting or node budget; the error aborts the decode before the
/// next level of recursion can consume stack.
fn dialog_decode_enter() -> Result<(), String> {
    DIALOG_DECODE_BUDGET.with(|budget| {
        let mut budget = budget.get();
        budget.depth += 1;
        budget.nodes += 1;
        DIALOG_DECODE_BUDGET.set(budget);
        if budget.depth > MAX_DIALOG_NESTING {
            return Err(format!(
                "dialog control nesting exceeds the decoder depth budget ({MAX_DIALOG_NESTING})"
            ));
        }
        if budget.nodes > MAX_DIALOG_NODES {
            return Err(format!(
                "dialog value exceeds the decoder node budget ({MAX_DIALOG_NODES})"
            ));
        }
        Ok(())
    })
}

/// Leave one dialog node (restores the depth budget even on decode failure).
fn dialog_decode_exit() {
    DIALOG_DECODE_BUDGET.with(|budget| {
        let mut budget = budget.get();
        budget.depth = budget.depth.saturating_sub(1);
        DIALOG_DECODE_BUDGET.set(budget);
    });
}

struct DialogDecodeGuard;

impl Drop for DialogDecodeGuard {
    fn drop(&mut self) {
        dialog_decode_exit();
    }
}

/// Charge `count` decoded dialog nodes against the frame node budget.
fn dialog_decode_add_nodes(count: usize) -> Result<(), String> {
    DIALOG_DECODE_BUDGET.with(|budget| {
        let mut budget = budget.get();
        budget.nodes = budget.nodes.saturating_add(count);
        DIALOG_DECODE_BUDGET.set(budget);
        if budget.nodes > MAX_DIALOG_NODES {
            Err(format!(
                "dialog value exceeds the decoder node budget ({MAX_DIALOG_NODES})"
            ))
        } else {
            Ok(())
        }
    })
}

/// Bounded deserialize helper for the recursive `Vec<DialogControl>` carriers
/// (`Row`, `Column`, `Fields` and `DialogRow`). Charges one nesting level and
/// the decoded element total against the frame budget: a payload past the
/// budget refuses before the next level recurses, so the decoding stack stays
/// bounded. The wire layout is unchanged.
fn budgeted_controls<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<DialogControl>, D::Error> {
    dialog_decode_enter()
        .map_err(<D::Error as serde::de::Error>::custom)?;
    let _guard = DialogDecodeGuard;
    let controls = Vec::<DialogControl>::deserialize(deserializer)?;
    dialog_decode_add_nodes(controls.len())
        .map_err(<D::Error as serde::de::Error>::custom)?;
    Ok(controls)
}

/// Bounded deserialize helper for the recursive `DialogValue::Fields` map.
fn budgeted_dialog_value_map<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<HashMap<String, DialogValue>, D::Error> {
    dialog_decode_enter()
        .map_err(<D::Error as serde::de::Error>::custom)?;
    let _guard = DialogDecodeGuard;
    let map = HashMap::<String, DialogValue>::deserialize(deserializer)?;
    dialog_decode_add_nodes(map.len())
        .map_err(<D::Error as serde::de::Error>::custom)?;
    Ok(map)
}

/// A single interactive control inside a dialog row.
///
/// Deserialization is bounded (see [`MAX_DIALOG_NESTING`]): the wire layout is
/// unchanged, but a payload whose `Row`/`Column`/`Fields` nesting passes the
/// decoder budget is rejected instead of overflowing the stack.
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
        #[serde(deserialize_with = "budgeted_controls")]
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
        #[serde(deserialize_with = "budgeted_controls")]
        controls: Vec<DialogControl>,
        #[serde(default)]
        align_h: Option<HorizontalAlign>,
        #[serde(default)]
        align_v: Option<VerticalAlign>,
    },
    /// A vertical column grouping of controls.
    Column {
        #[serde(deserialize_with = "budgeted_controls")]
        controls: Vec<DialogControl>,
        #[serde(default)]
        align_h: Option<HorizontalAlign>,
        #[serde(default)]
        align_v: Option<VerticalAlign>,
    },
    /// Static text with an id, so a later `update_panel` / dialog update can
    /// rewrite it in place (API v17). Plain `InfoText` stays id-less.
    Label {
        id: String,
        text: String,
        #[serde(default)]
        align_h: Option<HorizontalAlign>,
    },
    /// Static text with an emphasis [`TextStyle`] (API v19): headings, key
    /// figures and pass/fail states. Appended so older variants keep their
    /// wire index; a v18 host cannot decode it, so a plugin that sends it
    /// declares v19.
    StyledText {
        text: String,
        #[serde(default)]
        style: TextStyle,
        #[serde(default)]
        align_h: Option<HorizontalAlign>,
    },
    /// A static SVG image (API v19), e.g. a chart the plugin renders itself.
    /// `svg` is the complete document text; it is scaled to the available
    /// width keeping its aspect ratio, or to `height` logical pixels when
    /// given. External references are not loaded. Appended for the same wire
    /// reason as [`DialogControl::StyledText`].
    Svg {
        id: String,
        svg: String,
        #[serde(default)]
        height: Option<f32>,
    },
}

fn default_true() -> bool {
    true
}

impl DialogControl {
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::InfoText { .. }
            | Self::StyledText { .. }
            | Self::Row { .. }
            | Self::Column { .. } => None,
            Self::Label { id, .. }
            | Self::Svg { id, .. }
            | Self::Text { id, .. }
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
    #[serde(deserialize_with = "budgeted_controls")]
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
/// plugins keep their bincode discriminant indices (`0..=7`). Deserialization
/// is bounded (see [`MAX_DIALOG_NESTING`]): the wire layout is unchanged, but
/// a payload whose nested `Fields` maps pass the decoder budget is rejected
/// instead of overflowing the stack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DialogValue {
    Text(String),
    Number(f64),
    Bool(bool),
    Choice { index: usize, text: String },
    Color([u8; 3]),
    FilePath(String),
    Fields(#[serde(deserialize_with = "budgeted_dialog_value_map")] HashMap<String, DialogValue>),
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

/// Current UI form contents of a registered persistent panel (API v18, additive).
///
/// This is UI input only: it is neither document authority nor a document revision.
/// In particular, it does not establish that a record or entity targeted by a later
/// write still exists or is unchanged. The host attaches [`tab_id`](Self::tab_id) so
/// the panel read remains bound to the document tab that served it.
///
/// Typed values have the same shapes as [`DialogResponse::Submitted`]:
///
/// - [`DialogControl::Text`] produces [`DialogValue::Text`].
/// - [`DialogControl::Number`] produces [`DialogValue::Number`] when its current text
///   is a valid number. Partial or otherwise malformed numeric text is retained in
///   [`raw_text`](Self::raw_text) and reported in [`errors`](Self::errors), rather than
///   being replaced by a default or a normalized value.
/// - [`DialogControl::Checkbox`] produces [`DialogValue::Bool`].
/// - [`DialogControl::Choice`] produces [`DialogValue::Choice`], carrying both the
///   selected `index` and its current `text`.
/// - [`DialogControl::Color`] produces [`DialogValue::Color`].
/// - [`DialogControl::FilePicker`] produces [`DialogValue::FilePath`].
/// - [`DialogControl::Table`] produces [`DialogValue::Table`] with every current row
///   and cell string; table cells do not have separate `raw_text` entries.
/// - [`DialogControl::Fields`] produces [`DialogValue::Fields`], whose map is built by
///   recursively reading its child controls. [`DialogControl::Row`] and
///   [`DialogControl::Column`] are transparent containers and are recursively read at
///   their location without producing values of their own.
/// - [`DialogControl::InfoText`] and [`DialogControl::Button`] have no form value.
///   Button activation remains a panel action, not a snapshot entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanelSnapshot {
    /// Identifier of the registered panel that was read.
    pub panel_id: String,
    /// Host-authenticated identity of the plugin that registered this panel.
    /// This is assigned by the host, never accepted from the form.
    pub plugin_id: String,
    /// Host-attached identifier of the document tab owning the panel session.
    pub tab_id: u64,
    /// Complete typed values for all readable value-bearing controls, keyed by the
    /// declared control id. Nested `Fields` use nested maps as described above.
    ///
    /// An unreadable value is omitted only with a corresponding entry in `errors`;
    /// the host never silently supplies the panel declaration's default instead.
    pub values: HashMap<String, DialogValue>,
    /// Exact current text of every `Text`, `Number`, and `FilePicker` control, keyed
    /// by control id, including controls reached through nested containers. Number
    /// text is preserved verbatim even when it is partial or malformed.
    pub raw_text: HashMap<String, String>,
    /// Read-back problems, including duplicate or missing fields, values whose shape
    /// does not match the declared control, and malformed input. An empty vector is a
    /// clean read. Plugins must reject a snapshot with any errors rather than write
    /// from a partial or ambiguous form.
    pub errors: Vec<String>,
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
                options: vec![
                    "Concrete".to_string(),
                    "Steel".to_string(),
                    "Timber".to_string(),
                ],
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
    fn styled_text_and_svg_roundtrip_and_are_appended_to_the_wire_enum() {
        // API v19: both variants ride at the enum tail, so every older
        // variant keeps its bincode tag and an older plugin's panels decode
        // unchanged on a v19 host.
        let tag = |control: &DialogControl| -> u32 {
            let bytes = bincode::serialize(control).expect("serialize control");
            u32::from_le_bytes(bytes[..4].try_into().expect("variant tag"))
        };
        let label = DialogControl::Label {
            id: "l".to_string(),
            text: "x".to_string(),
            align_h: None,
        };
        let styled = DialogControl::StyledText {
            text: "Check passed".to_string(),
            style: TextStyle::Success,
            align_h: Some(HorizontalAlign::Center),
        };
        let svg = DialogControl::Svg {
            id: "chart".to_string(),
            svg: "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"/>".to_string(),
            height: Some(240.0),
        };
        assert_eq!(tag(&label), 12);
        assert_eq!(tag(&styled), 13);
        assert_eq!(tag(&svg), 14);
        assert_eq!(styled.id(), None);
        assert_eq!(svg.id(), Some("chart"));

        let panel = PanelDef::new("results", "Results")
            .with_control(styled)
            .with_control(svg);
        let bytes = bincode::serialize(&panel).expect("serialize panel");
        let restored: PanelDef = bincode::deserialize(&bytes).expect("deserialize panel");
        assert_eq!(panel, restored);

        // JSON (manifest/registry tooling) uses lowercase style names and
        // defaults a missing style to `Normal`.
        let json = serde_json::json!({ "StyledText": { "text": "t" } });
        let parsed: DialogControl = serde_json::from_value(json).expect("default style");
        assert_eq!(
            parsed,
            DialogControl::StyledText {
                text: "t".to_string(),
                style: TextStyle::Normal,
                align_h: None,
            }
        );
        assert_eq!(
            serde_json::to_value(TextStyle::Danger).expect("style json"),
            serde_json::json!("danger")
        );
    }

    #[test]
    fn quad_panel_def_roundtrips_over_bincode() {
        // API v15: the quad wire shape (owned strings, OwnedIconKind icon)
        // rides the same serializable-data discipline as PanelDef above.
        let panel = QuadPanelDef::new("align_tools", "Align")
            .with_applies_to(vec![
                "LWPOLYLINE".to_string(),
                "PLUGIN_ALIGN_DISPLAY".to_string(),
            ])
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
    fn panel_snapshot_bincode_roundtrip_preserves_raw_text() {
        let mut fields = HashMap::new();
        fields.insert("name".to_string(), DialogValue::Text("Bracket".to_string()));

        let mut values = HashMap::new();
        values.insert("geometry".to_string(), DialogValue::Fields(fields));
        values.insert(
            "material".to_string(),
            DialogValue::Choice {
                index: 1,
                text: "Steel".to_string(),
            },
        );
        values.insert(
            "rows".to_string(),
            DialogValue::Table(vec![vec!["Bolt".to_string(), "8".to_string()]]),
        );

        let mut raw_text = HashMap::new();
        raw_text.insert("name".to_string(), "Bracket".to_string());
        raw_text.insert("quantity".to_string(), "-".to_string());

        let snapshot = PanelSnapshot {
            panel_id: "part_editor".to_string(),
            plugin_id: "example.part_editor".to_string(),
            tab_id: 42,
            values,
            raw_text,
            errors: vec!["field 'quantity': malformed number".to_string()],
        };
        let bytes = bincode::serialize(&snapshot).expect("serialize panel snapshot");
        let restored: PanelSnapshot =
            bincode::deserialize(&bytes).expect("deserialize panel snapshot");
        assert_eq!(snapshot, restored);
        assert_eq!(
            restored.raw_text.get("quantity").map(String::as_str),
            Some("-")
        );
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
                headers: vec![
                    "Item".to_string(),
                    "Qty".to_string(),
                    "Material".to_string(),
                ],
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
        values.insert(
            "bom".to_string(),
            DialogValue::Table(vec![vec![
                "Bolt M6".to_string(),
                "10".to_string(),
                "Steel".to_string(),
            ]]),
        );
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

    /// A deeply nested `Row` chain still decodes under the budget unchanged.
    #[test]
    fn dialog_decode_accepts_nesting_within_budget() {
        let mut control = DialogControl::Text {
            id: "leaf".to_string(),
            label: String::new(),
            default: String::new(),
        };
        for _ in 0..MAX_DIALOG_NESTING - 1 {
            control = DialogControl::row(vec![control]);
        }
        dialog_decode_frame_begin();
        let bytes = bincode::serialize(&control).expect("serialize dialog control");
        let restored: DialogControl = bincode::deserialize(&bytes).expect("decode dialog control");
        assert_eq!(restored, control);
    }

    /// A payload nesting past the budget is REFUSED instead of overflowing the
    /// decoding thread's stack (which aborts the whole host/runner process).
    #[test]
    fn dialog_decode_rejects_nesting_past_budget() {
        let mut control = DialogControl::Text {
            id: "leaf".to_string(),
            label: String::new(),
            default: String::new(),
        };
        for _ in 0..MAX_DIALOG_NESTING + 8 {
            control = DialogControl::row(vec![control]);
        }
        dialog_decode_frame_begin();
        let bytes = bincode::serialize(&control).expect("serialize dialog control");
        match bincode::deserialize::<DialogControl>(&bytes) {
            Ok(_) => panic!("over-budget dialog decoded"),
            Err(error) => assert!(
                error.to_string().contains("depth budget"),
                "expected the depth-budget rejection, got: {error}"
            ),
        }
    }

    /// A flat frame with more dialog nodes than the node budget (a wide
    /// dialogs/panels batch) is refused before decoding allocates for it.
    #[test]
    fn dialog_decode_rejects_node_budget_overflow() {
        let flat = DialogValue::Fields(
            (0..MAX_DIALOG_NODES + 1)
                .map(|i| (format!("field{i}"), DialogValue::Bool(true)))
                .collect(),
        );
        dialog_decode_frame_begin();
        let bytes = bincode::serialize(&flat).expect("serialize dialog value");
        match bincode::deserialize::<DialogValue>(&bytes) {
            Ok(_) => panic!("over-budget dialog value decoded"),
            Err(error) => assert!(
                error.to_string().contains("node budget"),
                "expected the node-budget rejection, got: {error}"
            ),
        }
    }
}

