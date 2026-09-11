//! MultiLeader entity implementation.
//!
//! The MultiLeader (MLEADER) entity is an advanced annotation object that can have
//! multiple leader lines connecting to text (MText), block, or tolerance content.

use crate::entities::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};

use bitflags::bitflags;

// ============================================================================
// Enums
// ============================================================================

/// Content type for multileader annotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum LeaderContentType {
    /// No content (leader only).
    None = 0,
    /// Content is a block reference.
    Block = 1,
    /// Content is multiline text.
    #[default]
    MText = 2,
    /// Content is a tolerance frame.
    Tolerance = 3,
}

impl From<i16> for LeaderContentType {
    fn from(value: i16) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Block,
            2 => Self::MText,
            3 => Self::Tolerance,
            _ => Self::None,
        }
    }
}

/// Leader path type (line style).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum MultiLeaderPathType {
    /// Leader is invisible.
    Invisible = 0,
    /// Straight line segments (polyline).
    #[default]
    StraightLineSegments = 1,
    /// Spline curve.
    Spline = 2,
}

impl From<i16> for MultiLeaderPathType {
    fn from(value: i16) -> Self {
        match value {
            0 => Self::Invisible,
            1 => Self::StraightLineSegments,
            2 => Self::Spline,
            _ => Self::StraightLineSegments,
        }
    }
}

/// Text attachment point relative to landing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum TextAttachmentType {
    /// Top of top line.
    TopOfTopLine = 0,
    /// Middle of top line.
    MiddleOfTopLine = 1,
    /// Middle of text (vertical center).
    #[default]
    MiddleOfText = 2,
    /// Middle of bottom line.
    MiddleOfBottomLine = 3,
    /// Bottom of bottom line.
    BottomOfBottomLine = 4,
    /// Bottom line.
    BottomLine = 5,
    /// Bottom of top line, underline bottom line.
    BottomOfTopLineUnderlineBottomLine = 6,
    /// Bottom of top line, underline top line.
    BottomOfTopLineUnderlineTopLine = 7,
    /// Bottom of top line, underline all.
    BottomOfTopLineUnderlineAll = 8,
    /// Center of text (for vertical attachment).
    CenterOfText = 9,
    /// Center of text with overline (for vertical attachment).
    CenterOfTextOverline = 10,
}

impl From<i16> for TextAttachmentType {
    fn from(value: i16) -> Self {
        match value {
            0 => Self::TopOfTopLine,
            1 => Self::MiddleOfTopLine,
            2 => Self::MiddleOfText,
            3 => Self::MiddleOfBottomLine,
            4 => Self::BottomOfBottomLine,
            5 => Self::BottomLine,
            6 => Self::BottomOfTopLineUnderlineBottomLine,
            7 => Self::BottomOfTopLineUnderlineTopLine,
            8 => Self::BottomOfTopLineUnderlineAll,
            9 => Self::CenterOfText,
            10 => Self::CenterOfTextOverline,
            _ => Self::MiddleOfText,
        }
    }
}

/// Text angle type for leader content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum TextAngleType {
    /// Text angle parallel to last leader line segment.
    ParallelToLastLeaderLine = 0,
    /// Text is always horizontal.
    #[default]
    Horizontal = 1,
    /// Like parallel, but rotated 180° if needed for readability.
    Optimized = 2,
}

impl From<i16> for TextAngleType {
    fn from(value: i16) -> Self {
        match value {
            0 => Self::ParallelToLastLeaderLine,
            1 => Self::Horizontal,
            2 => Self::Optimized,
            _ => Self::Horizontal,
        }
    }
}

/// Block content connection type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum BlockContentConnectionType {
    /// Connect to block extents.
    #[default]
    BlockExtents = 0,
    /// Connect to block base point.
    BasePoint = 1,
}

impl From<i16> for BlockContentConnectionType {
    fn from(value: i16) -> Self {
        match value {
            0 => Self::BlockExtents,
            1 => Self::BasePoint,
            _ => Self::BlockExtents,
        }
    }
}

/// Text attachment direction (horizontal or vertical).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum TextAttachmentDirectionType {
    /// Leaders attach to left/right of content.
    #[default]
    Horizontal = 0,
    /// Leaders attach to top/bottom of content.
    Vertical = 1,
}

impl From<i16> for TextAttachmentDirectionType {
    fn from(value: i16) -> Self {
        match value {
            0 => Self::Horizontal,
            1 => Self::Vertical,
            _ => Self::Horizontal,
        }
    }
}

/// Text attachment point type (left/center/right).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum TextAttachmentPointType {
    /// Attach to left.
    Left = 1,
    /// Attach to center.
    #[default]
    Center = 2,
    /// Attach to right.
    Right = 3,
}

impl From<i16> for TextAttachmentPointType {
    fn from(value: i16) -> Self {
        match value {
            1 => Self::Left,
            2 => Self::Center,
            3 => Self::Right,
            _ => Self::Center,
        }
    }
}

/// Text alignment type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum TextAlignmentType {
    /// Left alignment.
    #[default]
    Left = 0,
    /// Center alignment.
    Center = 1,
    /// Right alignment.
    Right = 2,
}

impl From<i16> for TextAlignmentType {
    fn from(value: i16) -> Self {
        match value {
            0 => Self::Left,
            1 => Self::Center,
            2 => Self::Right,
            _ => Self::Left,
        }
    }
}

/// Flow direction for text columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum FlowDirectionType {
    /// Horizontal flow.
    #[default]
    Horizontal = 1,
    /// Vertical flow.
    Vertical = 3,
    /// Use style setting.
    ByStyle = 5,
}

impl From<i16> for FlowDirectionType {
    fn from(value: i16) -> Self {
        match value {
            1 => Self::Horizontal,
            3 => Self::Vertical,
            5 => Self::ByStyle,
            _ => Self::Horizontal,
        }
    }
}

/// Line spacing style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum LineSpacingStyle {
    /// At least the specified spacing.
    #[default]
    AtLeast = 1,
    /// Exactly the specified spacing.
    Exactly = 2,
}

impl From<i16> for LineSpacingStyle {
    fn from(value: i16) -> Self {
        match value {
            1 => Self::AtLeast,
            2 => Self::Exactly,
            _ => Self::AtLeast,
        }
    }
}

// ============================================================================
// Bitflags
// ============================================================================

bitflags! {
    /// Property override flags for MultiLeader.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct MultiLeaderPropertyOverrideFlags: u32 {
        /// No overrides.
        const NONE = 0;
        /// Override path type.
        const PATH_TYPE = 0x1;
        /// Override line color.
        const LINE_COLOR = 0x2;
        /// Override leader line type.
        const LEADER_LINE_TYPE = 0x4;
        /// Override leader line weight.
        const LEADER_LINE_WEIGHT = 0x8;
        /// Override enable landing.
        const ENABLE_LANDING = 0x10;
        /// Override landing gap.
        const LANDING_GAP = 0x20;
        /// Override enable dogleg.
        const ENABLE_DOGLEG = 0x40;
        /// Override landing distance.
        const LANDING_DISTANCE = 0x80;
        /// Override arrowhead.
        const ARROWHEAD = 0x100;
        /// Override arrowhead size.
        const ARROWHEAD_SIZE = 0x200;
        /// Override content type.
        const CONTENT_TYPE = 0x400;
        /// Override text style.
        const TEXT_STYLE = 0x800;
        /// Override text left attachment.
        const TEXT_LEFT_ATTACHMENT = 0x1000;
        /// Override text angle.
        const TEXT_ANGLE = 0x2000;
        /// Override text alignment.
        const TEXT_ALIGNMENT = 0x4000;
        /// Override text color.
        const TEXT_COLOR = 0x8000;
        /// Override text height.
        const TEXT_HEIGHT = 0x10000;
        /// Override text frame.
        const TEXT_FRAME = 0x20000;
        /// Override use default MText.
        const ENABLE_USE_DEFAULT_MTEXT = 0x40000;
        /// Override block content.
        const BLOCK_CONTENT = 0x80000;
        /// Override block content color.
        const BLOCK_CONTENT_COLOR = 0x100000;
        /// Override block content scale.
        const BLOCK_CONTENT_SCALE = 0x200000;
        /// Override block content rotation.
        const BLOCK_CONTENT_ROTATION = 0x400000;
        /// Override block content connection.
        const BLOCK_CONTENT_CONNECTION = 0x800000;
        /// Override scale factor.
        const SCALE_FACTOR = 0x1000000;
        /// Override text right attachment.
        const TEXT_RIGHT_ATTACHMENT = 0x2000000;
        /// Override text switch alignment type.
        const TEXT_SWITCH_ALIGNMENT_TYPE = 0x4000000;
        /// Override text attachment direction.
        const TEXT_ATTACHMENT_DIRECTION = 0x8000000;
        /// Override text top attachment.
        const TEXT_TOP_ATTACHMENT = 0x10000000;
        /// Override text bottom attachment.
        const TEXT_BOTTOM_ATTACHMENT = 0x20000000;
    }
}

bitflags! {
    /// Property override flags for individual leader lines.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct LeaderLinePropertyOverrideFlags: u32 {
        /// No overrides.
        const NONE = 0;
        /// Override path type.
        const PATH_TYPE = 1;
        /// Override line color.
        const LINE_COLOR = 2;
        /// Override line type.
        const LINE_TYPE = 4;
        /// Override line weight.
        const LINE_WEIGHT = 8;
        /// Override arrowhead size.
        const ARROWHEAD_SIZE = 16;
        /// Override arrowhead.
        const ARROWHEAD = 32;
    }
}

// ============================================================================
// Support Structures
// ============================================================================

/// Start/end point pair for leader line breaks.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StartEndPointPair {
    /// Break start point.
    pub start_point: Vector3,
    /// Break end point.
    pub end_point: Vector3,
}

impl StartEndPointPair {
    /// Creates a new start/end point pair.
    pub fn new(start_point: Vector3, end_point: Vector3) -> Self {
        Self {
            start_point,
            end_point,
        }
    }
}

impl Default for StartEndPointPair {
    fn default() -> Self {
        Self {
            start_point: Vector3::ZERO,
            end_point: Vector3::ZERO,
        }
    }
}

/// Break data attached to one leader-line segment.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LeaderLineBreakInfo {
    /// Segment index this break data belongs to.
    pub segment_index: i32,
    /// Start/end pairs cut out of the segment.
    pub break_points: Vec<StartEndPointPair>,
}

/// A single leader line with vertices.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LeaderLine {
    /// Index of this leader line.
    pub index: i32,
    /// Segment index.
    pub segment_index: i32,
    /// Points (vertices) along the leader line.
    pub points: Vec<Vector3>,
    /// Break start/end point pairs.
    pub break_points: Vec<StartEndPointPair>,
    /// Number of break info entries.
    pub break_info_count: i32,
    /// Complete per-segment break data. The legacy `segment_index` and
    /// `break_points` fields mirror the first entry for source compatibility.
    #[cfg_attr(feature = "serde", serde(default))]
    pub break_infos: Vec<LeaderLineBreakInfo>,
    /// Path type (straight, spline, invisible).
    pub path_type: MultiLeaderPathType,
    /// Line color override.
    pub line_color: Color,
    /// Line type handle.
    pub line_type_handle: Option<Handle>,
    /// Line weight override.
    pub line_weight: LineWeight,
    /// Arrowhead block handle.
    pub arrowhead_handle: Option<Handle>,
    /// Arrowhead size.
    pub arrowhead_size: f64,
    /// Property override flags.
    pub override_flags: LeaderLinePropertyOverrideFlags,
}

impl LeaderLine {
    /// Creates a new leader line with the given index.
    pub fn new(index: i32) -> Self {
        Self {
            index,
            segment_index: 0,
            points: Vec::new(),
            break_points: Vec::new(),
            break_info_count: 0,
            break_infos: Vec::new(),
            path_type: MultiLeaderPathType::StraightLineSegments,
            line_color: Color::ByBlock,
            line_type_handle: None,
            line_weight: LineWeight::ByLayer,
            arrowhead_handle: None,
            arrowhead_size: 0.18,
            override_flags: LeaderLinePropertyOverrideFlags::NONE,
        }
    }

    /// Creates a leader line from a list of points.
    pub fn from_points(index: i32, points: Vec<Vector3>) -> Self {
        let mut line = Self::new(index);
        line.points = points;
        line
    }

    /// Adds a point to the leader line.
    pub fn add_point(&mut self, point: Vector3) {
        self.points.push(point);
    }

    /// Returns the number of points.
    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    /// Gets the start point (first point).
    pub fn start_point(&self) -> Option<Vector3> {
        self.points.first().copied()
    }

    /// Gets the end point (last point).
    pub fn end_point(&self) -> Option<Vector3> {
        self.points.last().copied()
    }

    /// Calculates the total length of the leader line.
    pub fn length(&self) -> f64 {
        if self.points.len() < 2 {
            return 0.0;
        }
        self.points.windows(2).map(|w| (w[1] - w[0]).length()).sum()
    }
}

impl Default for LeaderLine {
    fn default() -> Self {
        Self::new(0)
    }
}

/// A leader root containing one or more leader lines.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LeaderRoot {
    /// Index of this leader root.
    pub leader_index: i32,
    /// Whether content is valid.
    pub content_valid: bool,
    /// Unknown flag (ODA writes true).
    pub unknown: bool,
    /// Connection point at content.
    pub connection_point: Vector3,
    /// Direction from connection point.
    pub direction: Vector3,
    /// Break start/end point pairs.
    pub break_points: Vec<StartEndPointPair>,
    /// Leader lines in this root.
    pub lines: Vec<LeaderLine>,
    /// Landing distance (dogleg length).
    pub landing_distance: f64,
    /// Text attachment direction (R2010+).
    pub text_attachment_direction: TextAttachmentDirectionType,
}

impl LeaderRoot {
    /// Creates a new leader root with the given index.
    pub fn new(index: i32) -> Self {
        Self {
            leader_index: index,
            content_valid: true,
            unknown: true,
            connection_point: Vector3::ZERO,
            direction: Vector3::new(1.0, 0.0, 0.0),
            break_points: Vec::new(),
            lines: Vec::new(),
            landing_distance: 0.36,
            text_attachment_direction: TextAttachmentDirectionType::Horizontal,
        }
    }

    /// Adds a leader line to this root.
    pub fn add_line(&mut self, line: LeaderLine) {
        self.lines.push(line);
    }

    /// Creates a leader line and adds it to this root.
    pub fn create_line(&mut self, points: Vec<Vector3>) -> &mut LeaderLine {
        let index = self.lines.len() as i32;
        let line = LeaderLine::from_points(index, points);
        self.lines.push(line);
        self.lines.last_mut().unwrap()
    }

    /// Returns the number of leader lines.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }
}

impl Default for LeaderRoot {
    fn default() -> Self {
        Self::new(0)
    }
}

/// Block attribute for block content.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockAttribute {
    /// Handle to the attribute definition.
    pub attribute_definition_handle: Option<Handle>,
    /// Attribute index.
    pub index: i16,
    /// Attribute width.
    pub width: f64,
    /// Attribute text value.
    pub text: String,
}

impl BlockAttribute {
    /// Creates a new block attribute.
    pub fn new(text: &str) -> Self {
        Self {
            attribute_definition_handle: None,
            index: 0,
            width: 0.0,
            text: text.to_string(),
        }
    }
}

impl Default for BlockAttribute {
    fn default() -> Self {
        Self::new("")
    }
}

/// Pre-R2007 per-leader arrowhead override.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MultiLeaderArrowheadOverride {
    /// Leader index this override applies to.
    pub index: i32,
    pub is_default: bool,
    pub arrowhead_handle: Option<Handle>,
}

// ============================================================================
// MultiLeader Context Data
// ============================================================================

/// Context data for MultiLeader annotation (geometry and content).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MultiLeaderAnnotContext {
    /// Leader roots (each can have multiple leader lines).
    pub leader_roots: Vec<LeaderRoot>,
    /// Undocumented low five flags in the standalone object-context leader
    /// root header. Root-count bits are derived from `leader_roots`.
    pub standalone_flags: u8,
    /// Whether standalone DWG context stores a zero root count followed by
    /// the seven-bit compatibility header instead of a direct BL count.
    pub standalone_uses_root_flags: bool,

    // Scale and positioning
    /// Overall scale factor.
    pub scale_factor: f64,
    /// Content base point.
    pub content_base_point: Vector3,

    // Text content properties
    /// Whether there is text content.
    pub has_text_contents: bool,
    /// Text string (may contain MTEXT markup).
    pub text_string: String,
    /// Text normal vector.
    pub text_normal: Vector3,
    /// Text location.
    pub text_location: Vector3,
    /// Text direction.
    pub text_direction: Vector3,
    /// Text rotation in radians.
    pub text_rotation: f64,
    /// Text height (scaled).
    pub text_height: f64,
    /// Text width boundary.
    pub text_width: f64,
    /// Text height boundary.
    pub text_boundary_height: f64,
    /// Line spacing factor.
    pub line_spacing_factor: f64,
    /// Line spacing style.
    pub line_spacing_style: LineSpacingStyle,
    /// Text color.
    pub text_color: Color,
    /// Text attachment point.
    pub text_attachment_point: TextAttachmentPointType,
    /// Text flow direction.
    pub text_flow_direction: FlowDirectionType,
    /// Text alignment.
    pub text_alignment: TextAlignmentType,
    /// Text left attachment type.
    pub text_left_attachment: TextAttachmentType,
    /// Text right attachment type.
    pub text_right_attachment: TextAttachmentType,
    /// Text top attachment type.
    pub text_top_attachment: TextAttachmentType,
    /// Text bottom attachment type.
    pub text_bottom_attachment: TextAttachmentType,
    /// Whether text height is automatic.
    pub text_height_automatic: bool,
    /// Word break enabled.
    pub word_break: bool,
    /// Undocumented text-context bit following the word-break flag.
    pub dwg_unknown_text_bit: bool,
    /// Text style handle.
    pub text_style_handle: Option<Handle>,

    // Block content properties
    /// Whether there is block content.
    pub has_block_contents: bool,
    /// Block content handle.
    pub block_content_handle: Option<Handle>,
    /// Block content normal.
    pub block_content_normal: Vector3,
    /// Block content location.
    pub block_content_location: Vector3,
    /// Block content scale.
    pub block_content_scale: Vector3,
    /// Block rotation in radians.
    pub block_rotation: f64,
    /// Block content color.
    pub block_content_color: Color,
    /// Block connection type.
    pub block_connection_type: BlockContentConnectionType,

    // Column properties
    /// Column type.
    pub column_type: i16,
    /// Column width.
    pub column_width: f64,
    /// Column gutter.
    pub column_gutter: f64,
    /// Column flow reversed.
    pub column_flow_reversed: bool,
    /// Column sizes.
    pub column_sizes: Vec<f64>,

    // Background fill
    /// Background fill enabled.
    pub background_fill_enabled: bool,
    /// Background mask fill on.
    pub background_mask_fill_on: bool,
    /// Background fill color.
    pub background_fill_color: Color,
    /// Background scale factor.
    pub background_scale_factor: f64,
    /// Background transparency.
    pub background_transparency: i32,

    // Transformation
    /// Base point.
    pub base_point: Vector3,
    /// Base direction vector.
    pub base_direction: Vector3,
    /// Base vertical vector.
    pub base_vertical: Vector3,
    /// Normal reversed.
    pub normal_reversed: bool,

    // Arrowhead
    /// Arrowhead size (scaled).
    pub arrowhead_size: f64,
    /// Landing gap.
    pub landing_gap: f64,

    // Transformation matrix (4x4)
    /// Transformation matrix (16 values, column-major).
    pub transform_matrix: [f64; 16],

    /// Scale object handle.
    pub scale_handle: Option<Handle>,
}

impl MultiLeaderAnnotContext {
    /// Creates a new annotation context with defaults.
    pub fn new() -> Self {
        // Identity matrix
        let mut transform_matrix = [0.0; 16];
        transform_matrix[0] = 1.0;
        transform_matrix[5] = 1.0;
        transform_matrix[10] = 1.0;
        transform_matrix[15] = 1.0;

        Self {
            leader_roots: Vec::new(),
            standalone_flags: 0,
            standalone_uses_root_flags: false,
            scale_factor: 1.0,
            content_base_point: Vector3::ZERO,
            has_text_contents: false,
            text_string: String::new(),
            text_normal: Vector3::new(0.0, 0.0, 1.0),
            text_location: Vector3::ZERO,
            text_direction: Vector3::new(1.0, 0.0, 0.0),
            text_rotation: 0.0,
            text_height: 0.18,
            text_width: 0.0,
            text_boundary_height: 0.0,
            line_spacing_factor: 1.0,
            line_spacing_style: LineSpacingStyle::AtLeast,
            text_color: Color::ByBlock,
            text_attachment_point: TextAttachmentPointType::Center,
            text_flow_direction: FlowDirectionType::Horizontal,
            text_alignment: TextAlignmentType::Left,
            text_left_attachment: TextAttachmentType::MiddleOfText,
            text_right_attachment: TextAttachmentType::MiddleOfText,
            text_top_attachment: TextAttachmentType::CenterOfText,
            text_bottom_attachment: TextAttachmentType::CenterOfText,
            text_height_automatic: false,
            word_break: true,
            dwg_unknown_text_bit: false,
            text_style_handle: None,
            has_block_contents: false,
            block_content_handle: None,
            block_content_normal: Vector3::new(0.0, 0.0, 1.0),
            block_content_location: Vector3::ZERO,
            block_content_scale: Vector3::new(1.0, 1.0, 1.0),
            block_rotation: 0.0,
            block_content_color: Color::ByBlock,
            block_connection_type: BlockContentConnectionType::BlockExtents,
            column_type: 0,
            column_width: 0.0,
            column_gutter: 0.0,
            column_flow_reversed: false,
            column_sizes: Vec::new(),
            background_fill_enabled: false,
            background_mask_fill_on: false,
            background_fill_color: Color::ByBlock,
            background_scale_factor: 1.5,
            background_transparency: 0,
            base_point: Vector3::ZERO,
            base_direction: Vector3::new(1.0, 0.0, 0.0),
            base_vertical: Vector3::new(0.0, 1.0, 0.0),
            normal_reversed: false,
            arrowhead_size: 0.18,
            landing_gap: 0.09,
            transform_matrix,
            scale_handle: None,
        }
    }

    /// Adds a leader root and returns a mutable reference.
    pub fn add_leader_root(&mut self) -> &mut LeaderRoot {
        let index = self.leader_roots.len() as i32;
        self.leader_roots.push(LeaderRoot::new(index));
        self.leader_roots.last_mut().unwrap()
    }

    /// Gets the number of leader roots.
    pub fn leader_root_count(&self) -> usize {
        self.leader_roots.len()
    }

    /// Sets text content.
    pub fn set_text_content(&mut self, text: &str, location: Vector3) {
        self.has_text_contents = true;
        self.text_string = text.to_string();
        self.text_location = location;
        self.content_base_point = location;
    }

    /// Sets block content.
    pub fn set_block_content(&mut self, block_handle: Handle, location: Vector3) {
        self.has_block_contents = true;
        self.block_content_handle = Some(block_handle);
        self.block_content_location = location;
        self.content_base_point = location;
    }
}

impl Default for MultiLeaderAnnotContext {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// MultiLeader Entity
// ============================================================================

/// MultiLeader (MLEADER) entity.
///
/// An advanced annotation object with multiple leader lines connecting to
/// text (MText), block, or tolerance content.
///
/// # Example
///
/// ```ignore
/// use acadrust::entities::MultiLeader;
/// use acadrust::types::Vector3;
///
/// // Create a multileader with text content
/// let mut mleader = MultiLeader::new();
/// mleader.set_text_content("Note", Vector3::new(10.0, 10.0, 0.0));
///
/// // Add a leader from (0,0) to (5,5) to (10,10)
/// let root = mleader.add_leader_root();
/// root.create_line(vec![
///     Vector3::new(0.0, 0.0, 0.0),
///     Vector3::new(5.0, 5.0, 0.0),
/// ]);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MultiLeader {
    /// Common entity data.
    pub common: EntityCommon,
    /// Native DWG MLEADER version (expected value is 2 on R2010+).
    pub dwg_version: i16,

    // Style reference
    /// Handle to MultiLeader style.
    pub style_handle: Option<Handle>,

    // Content settings
    /// Content type (None, Block, MText, Tolerance).
    pub content_type: LeaderContentType,
    /// Context data (geometry and content).
    pub context: MultiLeaderAnnotContext,
    /// Block attributes (for block content).
    pub block_attributes: Vec<BlockAttribute>,
    /// Pre-R2007 arrowhead overrides.
    pub arrowhead_overrides: Vec<MultiLeaderArrowheadOverride>,

    // Leader line settings
    /// Path type (straight, spline, invisible).
    pub path_type: MultiLeaderPathType,
    /// Leader line color.
    pub line_color: Color,
    /// Leader line type handle.
    pub line_type_handle: Option<Handle>,
    /// Leader line weight.
    pub line_weight: LineWeight,
    /// Enable landing.
    pub enable_landing: bool,
    /// Enable dogleg.
    pub enable_dogleg: bool,
    /// Dogleg length.
    pub dogleg_length: f64,
    /// Arrowhead block handle.
    pub arrowhead_handle: Option<Handle>,
    /// Arrowhead size.
    pub arrowhead_size: f64,

    // Text settings
    /// Text style handle.
    pub text_style_handle: Option<Handle>,
    /// Text color.
    pub text_color: Color,
    /// Draw text with frame.
    pub text_frame: bool,
    /// Text height.
    pub text_height: f64,
    /// Text left attachment type.
    pub text_left_attachment: TextAttachmentType,
    /// Text right attachment type.
    pub text_right_attachment: TextAttachmentType,
    /// Text top attachment type.
    pub text_top_attachment: TextAttachmentType,
    /// Text bottom attachment type.
    pub text_bottom_attachment: TextAttachmentType,
    /// Text attachment direction.
    pub text_attachment_direction: TextAttachmentDirectionType,
    /// Text attachment point.
    pub text_attachment_point: TextAttachmentPointType,
    /// Text alignment.
    pub text_alignment: TextAlignmentType,
    /// Text angle type.
    pub text_angle_type: TextAngleType,
    /// Text direction negative.
    pub text_direction_negative: bool,
    /// Text align in IPE.
    pub text_align_in_ipe: i16,

    // Block settings
    /// Block content handle.
    pub block_content_handle: Option<Handle>,
    /// Block content color.
    pub block_content_color: Color,
    /// Block content connection type.
    pub block_connection_type: BlockContentConnectionType,
    /// Block rotation in radians.
    pub block_rotation: f64,
    /// Block scale.
    pub block_scale: Vector3,

    // General settings
    /// Scale factor.
    pub scale_factor: f64,
    /// Property override flags.
    pub property_override_flags: MultiLeaderPropertyOverrideFlags,
    /// Enable annotation scale.
    pub enable_annotation_scale: bool,
    /// Extend leader to text.
    pub extend_leader_to_text: bool,
}

impl MultiLeader {
    /// Creates a new MultiLeader with default settings.
    pub fn new() -> Self {
        Self {
            common: EntityCommon::default(),
            dwg_version: 2,
            style_handle: None,
            content_type: LeaderContentType::MText,
            context: MultiLeaderAnnotContext::new(),
            block_attributes: Vec::new(),
            arrowhead_overrides: Vec::new(),
            path_type: MultiLeaderPathType::StraightLineSegments,
            line_color: Color::ByBlock,
            line_type_handle: None,
            line_weight: LineWeight::ByLayer,
            enable_landing: true,
            enable_dogleg: true,
            dogleg_length: 0.36,
            arrowhead_handle: None,
            arrowhead_size: 0.18,
            text_style_handle: None,
            text_color: Color::ByBlock,
            text_frame: false,
            text_height: 0.18,
            text_left_attachment: TextAttachmentType::MiddleOfText,
            text_right_attachment: TextAttachmentType::MiddleOfText,
            text_top_attachment: TextAttachmentType::CenterOfText,
            text_bottom_attachment: TextAttachmentType::CenterOfText,
            text_attachment_direction: TextAttachmentDirectionType::Horizontal,
            text_attachment_point: TextAttachmentPointType::Center,
            text_alignment: TextAlignmentType::Left,
            text_angle_type: TextAngleType::Horizontal,
            text_direction_negative: false,
            text_align_in_ipe: 0,
            block_content_handle: None,
            block_content_color: Color::ByBlock,
            block_connection_type: BlockContentConnectionType::BlockExtents,
            block_rotation: 0.0,
            block_scale: Vector3::new(1.0, 1.0, 1.0),
            scale_factor: 1.0,
            property_override_flags: MultiLeaderPropertyOverrideFlags::NONE,
            // Non-annotative by default (mirrors MTEXT.is_annotative). The DWG
            // reader sets this explicitly from the entity bit and the DXF reader
            // from group code 293; a fresh MULTILEADER must not inherit `true`,
            // or a reader that missed the flag would over-scale every instance.
            enable_annotation_scale: false,
            extend_leader_to_text: false,
        }
    }

    /// Creates a MultiLeader with text content.
    pub fn with_text(text: &str, text_location: Vector3, leader_points: Vec<Vector3>) -> Self {
        let mut mleader = Self::new();
        mleader.set_text_content(text, text_location);

        // Add leader root and line
        let root = mleader.add_leader_root();
        root.connection_point = text_location;
        if !leader_points.is_empty() {
            root.create_line(leader_points);
        }

        mleader
    }

    /// Sets text content.
    pub fn set_text_content(&mut self, text: &str, location: Vector3) {
        self.content_type = LeaderContentType::MText;
        self.context.set_text_content(text, location);
    }

    /// Sets block content.
    pub fn set_block_content(&mut self, block_handle: Handle, location: Vector3) {
        self.content_type = LeaderContentType::Block;
        self.block_content_handle = Some(block_handle);
        self.context.set_block_content(block_handle, location);
    }

    /// Adds a leader root.
    pub fn add_leader_root(&mut self) -> &mut LeaderRoot {
        self.context.add_leader_root()
    }

    /// Gets the number of leader roots.
    pub fn leader_root_count(&self) -> usize {
        self.context.leader_root_count()
    }

    /// Gets the total number of leader lines across all roots.
    pub fn total_leader_line_count(&self) -> usize {
        self.context
            .leader_roots
            .iter()
            .map(|r| r.line_count())
            .sum()
    }

    /// Gets the content text (if text content).
    pub fn text(&self) -> Option<&str> {
        if self.content_type == LeaderContentType::MText && self.context.has_text_contents {
            Some(&self.context.text_string)
        } else {
            None
        }
    }

    /// Sets the text content string.
    pub fn set_text(&mut self, text: &str) {
        if self.content_type == LeaderContentType::MText {
            self.context.text_string = text.to_string();
            self.context.has_text_contents = true;
        }
    }

    /// Translates the multileader by the given offset.
    pub fn translate(&mut self, offset: Vector3) {
        // Translate context points
        self.context.content_base_point = self.context.content_base_point + offset;
        self.context.text_location = self.context.text_location + offset;
        self.context.block_content_location = self.context.block_content_location + offset;
        self.context.base_point = self.context.base_point + offset;

        // Translate all leader roots and lines
        for root in &mut self.context.leader_roots {
            root.connection_point = root.connection_point + offset;
            for bp in &mut root.break_points {
                bp.start_point = bp.start_point + offset;
                bp.end_point = bp.end_point + offset;
            }
            for line in &mut root.lines {
                for point in &mut line.points {
                    *point = *point + offset;
                }
                for bp in &mut line.break_points {
                    bp.start_point = bp.start_point + offset;
                    bp.end_point = bp.end_point + offset;
                }
            }
        }
    }

    /// Returns the bounding box of all leader line points and content.
    pub fn bounding_box(&self) -> Option<(Vector3, Vector3)> {
        let mut points: Vec<Vector3> = Vec::new();

        // Add content point
        points.push(self.context.content_base_point);

        // Add all leader line points
        for root in &self.context.leader_roots {
            points.push(root.connection_point);
            for line in &root.lines {
                points.extend(&line.points);
            }
        }

        if points.is_empty() {
            return None;
        }

        let mut min = points[0];
        let mut max = points[0];

        for p in &points[1..] {
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            min.z = min.z.min(p.z);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
            max.z = max.z.max(p.z);
        }

        Some((min, max))
    }
}

impl Default for MultiLeader {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for MultiLeader {
    fn handle(&self) -> Handle {
        self.common.handle
    }

    fn set_handle(&mut self, handle: Handle) {
        self.common.handle = handle;
    }

    fn layer(&self) -> &str {
        &self.common.layer
    }

    fn set_layer(&mut self, layer: String) {
        self.common.layer = layer;
    }

    fn color(&self) -> Color {
        self.common.color
    }

    fn set_color(&mut self, color: Color) {
        self.common.color = color;
    }

    fn line_weight(&self) -> LineWeight {
        self.common.line_weight
    }

    fn set_line_weight(&mut self, line_weight: LineWeight) {
        self.common.line_weight = line_weight;
    }

    fn transparency(&self) -> Transparency {
        self.common.transparency
    }

    fn set_transparency(&mut self, transparency: Transparency) {
        self.common.transparency = transparency;
    }

    fn is_invisible(&self) -> bool {
        self.common.invisible
    }

    fn set_invisible(&mut self, invisible: bool) {
        self.common.invisible = invisible;
    }

    fn bounding_box(&self) -> BoundingBox3D {
        // Calculate bounding box from all leader line points and content location
        let mut min = Vector3::new(f64::MAX, f64::MAX, f64::MAX);
        let mut max = Vector3::new(f64::MIN, f64::MIN, f64::MIN);

        // Include content location
        let loc = &self.context.content_base_point;
        min.x = min.x.min(loc.x);
        min.y = min.y.min(loc.y);
        min.z = min.z.min(loc.z);
        max.x = max.x.max(loc.x);
        max.y = max.y.max(loc.y);
        max.z = max.z.max(loc.z);

        // Include all leader line points
        for root in &self.context.leader_roots {
            for line in &root.lines {
                for pt in &line.points {
                    min.x = min.x.min(pt.x);
                    min.y = min.y.min(pt.y);
                    min.z = min.z.min(pt.z);
                    max.x = max.x.max(pt.x);
                    max.y = max.y.max(pt.y);
                    max.z = max.z.max(pt.z);
                }
            }
        }

        BoundingBox3D::new(min, max)
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_multileader(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "MULTILEADER"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_multileader(self, transform);
    }
}

// ============================================================================
// Builder
// ============================================================================

/// Builder for MultiLeader entities.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MultiLeaderBuilder {
    multileader: MultiLeader,
}

impl MultiLeaderBuilder {
    /// Creates a new builder.
    pub fn new() -> Self {
        Self {
            multileader: MultiLeader::new(),
        }
    }

    /// Sets text content.
    pub fn text(mut self, text: &str, location: Vector3) -> Self {
        self.multileader.set_text_content(text, location);
        self
    }

    /// Sets block content.
    pub fn block(mut self, block_handle: Handle, location: Vector3) -> Self {
        self.multileader.set_block_content(block_handle, location);
        self
    }

    /// Sets content type to None.
    pub fn no_content(mut self) -> Self {
        self.multileader.content_type = LeaderContentType::None;
        self
    }

    /// Adds a leader line.
    pub fn leader_line(mut self, points: Vec<Vector3>) -> Self {
        if self.multileader.context.leader_roots.is_empty() {
            self.multileader.add_leader_root();
        }
        let root = self.multileader.context.leader_roots.last_mut().unwrap();
        root.create_line(points);
        self
    }

    /// Adds a new leader root.
    pub fn new_root(mut self) -> Self {
        self.multileader.add_leader_root();
        self
    }

    /// Sets the path type.
    pub fn path_type(mut self, path_type: MultiLeaderPathType) -> Self {
        self.multileader.path_type = path_type;
        self
    }

    /// Sets the arrowhead size.
    pub fn arrowhead_size(mut self, size: f64) -> Self {
        self.multileader.arrowhead_size = size;
        self.multileader.context.arrowhead_size = size;
        self
    }

    /// Sets the text height.
    pub fn text_height(mut self, height: f64) -> Self {
        self.multileader.text_height = height;
        self.multileader.context.text_height = height;
        self
    }

    /// Sets whether to draw text frame.
    pub fn text_frame(mut self, frame: bool) -> Self {
        self.multileader.text_frame = frame;
        self
    }

    /// Sets the line color.
    pub fn line_color(mut self, color: Color) -> Self {
        self.multileader.line_color = color;
        self
    }

    /// Sets the text color.
    pub fn text_color(mut self, color: Color) -> Self {
        self.multileader.text_color = color;
        self.multileader.context.text_color = color;
        self
    }

    /// Enables or disables landing (dogleg).
    pub fn landing(mut self, enable: bool) -> Self {
        self.multileader.enable_landing = enable;
        self
    }

    /// Sets the dogleg length.
    pub fn dogleg_length(mut self, length: f64) -> Self {
        self.multileader.dogleg_length = length;
        self
    }

    /// Sets the scale factor.
    pub fn scale(mut self, scale: f64) -> Self {
        self.multileader.scale_factor = scale;
        self.multileader.context.scale_factor = scale;
        self
    }

    /// Builds the MultiLeader.
    pub fn build(self) -> MultiLeader {
        self.multileader
    }
}

impl Default for MultiLeaderBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multileader_creation() {
        let mleader = MultiLeader::new();
        assert_eq!(mleader.content_type, LeaderContentType::MText);
        assert!(mleader.enable_landing);
        assert!(mleader.enable_dogleg);
        assert_eq!(mleader.leader_root_count(), 0);
    }

    #[test]
    fn test_multileader_with_text() {
        let points = vec![Vector3::new(0.0, 0.0, 0.0), Vector3::new(5.0, 5.0, 0.0)];
        let mleader = MultiLeader::with_text("Note", Vector3::new(10.0, 10.0, 0.0), points);

        assert_eq!(mleader.content_type, LeaderContentType::MText);
        assert_eq!(mleader.text(), Some("Note"));
        assert_eq!(mleader.leader_root_count(), 1);
        assert_eq!(mleader.total_leader_line_count(), 1);
    }

    #[test]
    fn test_multileader_add_leader_root() {
        let mut mleader = MultiLeader::new();
        let root = mleader.add_leader_root();
        root.create_line(vec![Vector3::ZERO, Vector3::new(1.0, 1.0, 0.0)]);
        root.create_line(vec![
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 1.0, 0.0),
        ]);

        assert_eq!(mleader.leader_root_count(), 1);
        assert_eq!(mleader.total_leader_line_count(), 2);
    }

    #[test]
    fn test_leader_line() {
        let mut line = LeaderLine::new(0);
        line.add_point(Vector3::new(0.0, 0.0, 0.0));
        line.add_point(Vector3::new(3.0, 4.0, 0.0));

        assert_eq!(line.point_count(), 2);
        assert_eq!(line.start_point(), Some(Vector3::new(0.0, 0.0, 0.0)));
        assert_eq!(line.end_point(), Some(Vector3::new(3.0, 4.0, 0.0)));
        assert!((line.length() - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_multileader_translate() {
        let mut mleader = MultiLeader::with_text(
            "Note",
            Vector3::new(10.0, 10.0, 0.0),
            vec![Vector3::ZERO, Vector3::new(5.0, 5.0, 0.0)],
        );

        mleader.translate(Vector3::new(5.0, 5.0, 0.0));

        assert_eq!(mleader.context.text_location, Vector3::new(15.0, 15.0, 0.0));
        let root = &mleader.context.leader_roots[0];
        let line = &root.lines[0];
        assert_eq!(line.points[0], Vector3::new(5.0, 5.0, 0.0));
        assert_eq!(line.points[1], Vector3::new(10.0, 10.0, 0.0));
    }

    #[test]
    fn test_multileader_bounding_box() {
        let mleader = MultiLeader::with_text(
            "Note",
            Vector3::new(10.0, 10.0, 0.0),
            vec![Vector3::new(0.0, 0.0, 0.0), Vector3::new(5.0, 5.0, 0.0)],
        );

        let bbox = mleader.bounding_box().unwrap();
        assert_eq!(bbox.0, Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(bbox.1, Vector3::new(10.0, 10.0, 0.0));
    }

    #[test]
    fn test_content_types() {
        assert_eq!(LeaderContentType::from(0), LeaderContentType::None);
        assert_eq!(LeaderContentType::from(1), LeaderContentType::Block);
        assert_eq!(LeaderContentType::from(2), LeaderContentType::MText);
        assert_eq!(LeaderContentType::from(3), LeaderContentType::Tolerance);
    }

    #[test]
    fn test_path_types() {
        assert_eq!(MultiLeaderPathType::from(0), MultiLeaderPathType::Invisible);
        assert_eq!(
            MultiLeaderPathType::from(1),
            MultiLeaderPathType::StraightLineSegments
        );
        assert_eq!(MultiLeaderPathType::from(2), MultiLeaderPathType::Spline);
    }

    #[test]
    fn test_text_attachment_types() {
        assert_eq!(
            TextAttachmentType::from(0),
            TextAttachmentType::TopOfTopLine
        );
        assert_eq!(
            TextAttachmentType::from(2),
            TextAttachmentType::MiddleOfText
        );
        assert_eq!(
            TextAttachmentType::from(9),
            TextAttachmentType::CenterOfText
        );
    }

    #[test]
    fn test_builder() {
        let mleader = MultiLeaderBuilder::new()
            .text("Note", Vector3::new(10.0, 10.0, 0.0))
            .leader_line(vec![Vector3::ZERO, Vector3::new(5.0, 5.0, 0.0)])
            .arrowhead_size(0.25)
            .text_height(0.2)
            .text_frame(true)
            .scale(2.0)
            .build();

        assert_eq!(mleader.text(), Some("Note"));
        assert_eq!(mleader.arrowhead_size, 0.25);
        assert_eq!(mleader.text_height, 0.2);
        assert!(mleader.text_frame);
        assert_eq!(mleader.scale_factor, 2.0);
    }

    #[test]
    fn test_property_override_flags() {
        let flags = MultiLeaderPropertyOverrideFlags::PATH_TYPE
            | MultiLeaderPropertyOverrideFlags::LINE_COLOR
            | MultiLeaderPropertyOverrideFlags::TEXT_HEIGHT;

        assert!(flags.contains(MultiLeaderPropertyOverrideFlags::PATH_TYPE));
        assert!(flags.contains(MultiLeaderPropertyOverrideFlags::LINE_COLOR));
        assert!(flags.contains(MultiLeaderPropertyOverrideFlags::TEXT_HEIGHT));
        assert!(!flags.contains(MultiLeaderPropertyOverrideFlags::ARROWHEAD));
    }

    #[test]
    fn test_leader_line_override_flags() {
        let flags = LeaderLinePropertyOverrideFlags::LINE_COLOR
            | LeaderLinePropertyOverrideFlags::ARROWHEAD;

        assert!(flags.contains(LeaderLinePropertyOverrideFlags::LINE_COLOR));
        assert!(flags.contains(LeaderLinePropertyOverrideFlags::ARROWHEAD));
        assert!(!flags.contains(LeaderLinePropertyOverrideFlags::PATH_TYPE));
    }

    #[test]
    fn test_block_attribute() {
        let attr = BlockAttribute::new("Value");
        assert_eq!(attr.text, "Value");
        assert_eq!(attr.index, 0);
        assert_eq!(attr.width, 0.0);
    }

    #[test]
    fn test_annot_context_defaults() {
        let ctx = MultiLeaderAnnotContext::new();
        assert_eq!(ctx.scale_factor, 1.0);
        assert_eq!(ctx.text_height, 0.18);
        assert!(!ctx.has_text_contents);
        assert!(!ctx.has_block_contents);
        assert_eq!(ctx.leader_roots.len(), 0);
    }
}
