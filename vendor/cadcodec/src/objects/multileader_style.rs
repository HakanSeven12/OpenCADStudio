//! MultiLeaderStyle object implementation.
//!
//! Defines the visual properties and behavior for MultiLeader entities.

use crate::types::{Color, Handle, LineWeight};

use bitflags::bitflags;

// ============================================================================
// Enums
// ============================================================================

/// Content type for multileader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum LeaderContentType {
    /// No content.
    None = 0,
    /// Block content.
    Block = 1,
    /// MText content (default).
    #[default]
    MText = 2,
    /// Tolerance content.
    Tolerance = 3,
}

impl From<i16> for LeaderContentType {
    fn from(value: i16) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Block,
            2 => Self::MText,
            3 => Self::Tolerance,
            _ => Self::MText,
        }
    }
}

/// Path type for leader lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum MultiLeaderPathType {
    /// Invisible leader lines.
    Invisible = 0,
    /// Straight line segments (default).
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

/// Text attachment type for horizontal attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum TextAttachmentType {
    /// Top of top line.
    TopOfTopLine = 0,
    /// Middle of top line.
    #[default]
    MiddleOfTopLine = 1,
    /// Middle of text.
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
    /// Center of text (vertical).
    CenterOfText = 9,
    /// Center of text with overline (vertical).
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
            _ => Self::MiddleOfTopLine,
        }
    }
}

/// Text attachment direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum TextAttachmentDirectionType {
    /// Horizontal attachment.
    #[default]
    Horizontal = 0,
    /// Vertical attachment.
    Vertical = 1,
}

impl From<i16> for TextAttachmentDirectionType {
    fn from(value: i16) -> Self {
        match value {
            1 => Self::Vertical,
            _ => Self::Horizontal,
        }
    }
}

/// Text alignment type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum TextAlignmentType {
    /// Left aligned.
    #[default]
    Left = 0,
    /// Center aligned.
    Center = 1,
    /// Right aligned.
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

/// Text angle type for leader text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum TextAngleType {
    /// Parallel to last leader line segment.
    ParallelToLastLeaderLine = 0,
    /// Always horizontal (default).
    #[default]
    Horizontal = 1,
    /// Optimized for readability.
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
            1 => Self::BasePoint,
            _ => Self::BlockExtents,
        }
    }
}

/// Leader draw order type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum LeaderDrawOrderType {
    /// Draw leader head first.
    #[default]
    LeaderHeadFirst = 0,
    /// Draw leader tail first.
    LeaderTailFirst = 1,
}

impl From<i16> for LeaderDrawOrderType {
    fn from(value: i16) -> Self {
        match value {
            1 => Self::LeaderTailFirst,
            _ => Self::LeaderHeadFirst,
        }
    }
}

/// MultiLeader draw order type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum MultiLeaderDrawOrderType {
    /// Draw content first.
    #[default]
    ContentFirst = 0,
    /// Draw leader first.
    LeaderFirst = 1,
}

impl From<i16> for MultiLeaderDrawOrderType {
    fn from(value: i16) -> Self {
        match value {
            1 => Self::LeaderFirst,
            _ => Self::ContentFirst,
        }
    }
}

// ============================================================================
// Flags
// ============================================================================

bitflags! {
    /// Property override flags for MultiLeader.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct MultiLeaderPropertyOverrideFlags: i32 {
        /// No overrides.
        const NONE = 0;
        /// Path type override.
        const PATH_TYPE = 0x1;
        /// Line color override.
        const LINE_COLOR = 0x2;
        /// Leader line type override.
        const LEADER_LINE_TYPE = 0x4;
        /// Leader line weight override.
        const LEADER_LINE_WEIGHT = 0x8;
        /// Enable landing override.
        const ENABLE_LANDING = 0x10;
        /// Landing gap override.
        const LANDING_GAP = 0x20;
        /// Enable dogleg override.
        const ENABLE_DOGLEG = 0x40;
        /// Landing distance override.
        const LANDING_DISTANCE = 0x80;
        /// Arrowhead override.
        const ARROWHEAD = 0x100;
        /// Arrowhead size override.
        const ARROWHEAD_SIZE = 0x200;
        /// Content type override.
        const CONTENT_TYPE = 0x400;
        /// Text style override.
        const TEXT_STYLE = 0x800;
        /// Text left attachment override.
        const TEXT_LEFT_ATTACHMENT = 0x1000;
        /// Text angle override.
        const TEXT_ANGLE = 0x2000;
        /// Text alignment override.
        const TEXT_ALIGNMENT = 0x4000;
        /// Text color override.
        const TEXT_COLOR = 0x8000;
        /// Text height override.
        const TEXT_HEIGHT = 0x10000;
        /// Text frame override.
        const TEXT_FRAME = 0x20000;
        /// Enable use default mtext override.
        const ENABLE_USE_DEFAULT_MTEXT = 0x40000;
        /// Block content override.
        const BLOCK_CONTENT = 0x80000;
        /// Block content color override.
        const BLOCK_CONTENT_COLOR = 0x100000;
        /// Block content scale override.
        const BLOCK_CONTENT_SCALE = 0x200000;
        /// Block content rotation override.
        const BLOCK_CONTENT_ROTATION = 0x400000;
        /// Block content connection override.
        const BLOCK_CONTENT_CONNECTION = 0x800000;
        /// Scale factor override.
        const SCALE_FACTOR = 0x1000000;
        /// Text right attachment override.
        const TEXT_RIGHT_ATTACHMENT = 0x2000000;
        /// Text switch alignment type override.
        const TEXT_SWITCH_ALIGNMENT_TYPE = 0x4000000;
        /// Text attachment direction override.
        const TEXT_ATTACHMENT_DIRECTION = 0x8000000;
        /// Text top attachment override.
        const TEXT_TOP_ATTACHMENT = 0x10000000;
        /// Text bottom attachment override.
        const TEXT_BOTTOM_ATTACHMENT = 0x20000000;
    }
}

bitflags! {
    /// Property override flags for leader lines.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct LeaderLinePropertyOverrideFlags: i32 {
        /// No overrides.
        const NONE = 0;
        /// Path type override.
        const PATH_TYPE = 1;
        /// Line color override.
        const LINE_COLOR = 2;
        /// Line type override.
        const LINE_TYPE = 4;
        /// Line weight override.
        const LINE_WEIGHT = 8;
        /// Arrowhead size override.
        const ARROWHEAD_SIZE = 16;
        /// Arrowhead override.
        const ARROWHEAD = 32;
    }
}

// ============================================================================
// MultiLeaderStyle
// ============================================================================

/// MultiLeader style object.
///
/// Defines the visual properties and behavior for MultiLeader entities.
///
/// # DXF Information
/// - Object type: MLEADERSTYLE
/// - Subclass marker: AcDbMLeaderStyle
///
/// # Example
///
/// ```ignore
/// use acadrust::objects::MultiLeaderStyle;
///
/// let mut style = MultiLeaderStyle::new("MyStyle");
/// style.text_height = 0.25;
/// style.arrowhead_size = 0.25;
/// style.enable_landing = true;
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MultiLeaderStyle {
    /// Object handle.
    pub handle: Handle,

    /// Owner handle.
    pub owner_handle: Handle,

    /// Style name.
    /// DXF code: 3
    pub name: String,

    /// Style description.
    /// DXF code: 300
    pub description: String,

    // ========== Leader Line Properties ==========
    /// Leader path type.
    /// DXF code: 173
    pub path_type: MultiLeaderPathType,

    /// Leader line color.
    /// DXF code: 91
    pub line_color: Color,

    /// Leader line type handle.
    /// DXF code: 340
    pub line_type_handle: Option<Handle>,

    /// Leader line weight.
    /// DXF code: 92
    pub line_weight: LineWeight,

    /// Enable landing (dogleg).
    /// DXF code: 290
    pub enable_landing: bool,

    /// Enable dogleg.
    /// DXF code: 291
    pub enable_dogleg: bool,

    /// Landing/dogleg length.
    /// DXF code: 43
    pub landing_distance: f64,

    /// Gap between leader and content.
    /// DXF code: 42
    pub landing_gap: f64,

    // ========== Arrowhead Properties ==========
    /// Arrowhead block handle.
    /// DXF code: 341
    pub arrowhead_handle: Option<Handle>,

    /// Arrowhead size.
    /// DXF code: 44
    pub arrowhead_size: f64,

    // ========== Content Properties ==========
    /// Content type.
    /// DXF code: 170
    pub content_type: LeaderContentType,

    // ========== Text Properties ==========
    /// Text style handle.
    /// DXF code: 342
    pub text_style_handle: Option<Handle>,

    /// Text left attachment type.
    /// DXF code: 174
    pub text_left_attachment: TextAttachmentType,

    /// Text right attachment type.
    /// DXF code: 178
    pub text_right_attachment: TextAttachmentType,

    /// Text top attachment type.
    /// DXF code: 273
    pub text_top_attachment: TextAttachmentType,

    /// Text bottom attachment type.
    /// DXF code: 272
    pub text_bottom_attachment: TextAttachmentType,

    /// Text attachment direction.
    /// DXF code: 271
    pub text_attachment_direction: TextAttachmentDirectionType,

    /// Text angle type.
    /// DXF code: 175
    pub text_angle_type: TextAngleType,

    /// Text alignment.
    /// DXF code: 176
    pub text_alignment: TextAlignmentType,

    /// Text color.
    /// DXF code: 93
    pub text_color: Color,

    /// Text height.
    /// DXF code: 45
    pub text_height: f64,

    /// Draw text frame.
    /// DXF code: 292
    pub text_frame: bool,

    /// Text always left aligned.
    /// DXF code: 297
    pub text_always_left: bool,

    /// Default text contents.
    /// DXF code: 304
    pub default_text: String,

    // ========== Block Properties ==========
    /// Block content handle.
    /// DXF code: 343
    pub block_content_handle: Option<Handle>,

    /// Block content color.
    /// DXF code: 94
    pub block_content_color: Color,

    /// Block content connection type.
    /// DXF code: 177
    pub block_content_connection: BlockContentConnectionType,

    /// Block content rotation.
    /// DXF code: 141
    pub block_content_rotation: f64,

    /// Block content X scale.
    /// DXF code: 47
    pub block_content_scale_x: f64,

    /// Block content Y scale.
    /// DXF code: 49
    pub block_content_scale_y: f64,

    /// Block content Z scale.
    /// DXF code: 140
    pub block_content_scale_z: f64,

    /// Enable block scale.
    /// DXF code: 293
    pub enable_block_scale: bool,

    /// Enable block rotation.
    /// DXF code: 294
    pub enable_block_rotation: bool,

    // ========== Scale and Constraints ==========
    /// Overall scale factor.
    /// DXF code: 142
    pub scale_factor: f64,

    /// Align space.
    /// DXF code: 46
    pub align_space: f64,

    /// Break gap size.
    /// DXF code: 143
    pub break_gap_size: f64,

    /// Max leader segment points.
    /// DXF code: 90
    pub max_leader_points: i32,

    /// First segment angle constraint.
    /// DXF code: 40
    pub first_segment_angle: f64,

    /// Second segment angle constraint.
    /// DXF code: 41
    pub second_segment_angle: f64,

    // ========== Draw Order ==========
    /// Leader draw order.
    /// DXF code: 172
    pub leader_draw_order: LeaderDrawOrderType,

    /// MultiLeader draw order.
    /// DXF code: 171
    pub multileader_draw_order: MultiLeaderDrawOrderType,

    // ========== Flags ==========
    /// Is annotative.
    /// DXF code: 296
    pub is_annotative: bool,

    /// Property changed flag.
    /// DXF code: 295
    pub property_changed: bool,

    /// R2013+ undocumented flag.
    /// DXF code: 298
    pub unknown_flag_298: bool,
}

impl MultiLeaderStyle {
    /// Object type name.
    pub const OBJECT_NAME: &'static str = "MLEADERSTYLE";

    /// Subclass marker.
    pub const SUBCLASS_MARKER: &'static str = "AcDbMLeaderStyle";

    /// Default style name.
    pub const STANDARD: &'static str = "Standard";

    /// Creates a new MultiLeaderStyle with default values.
    pub fn new(name: &str) -> Self {
        MultiLeaderStyle {
            handle: Handle::NULL,
            owner_handle: Handle::NULL,
            name: name.to_string(),
            description: String::new(),

            // Leader line
            path_type: MultiLeaderPathType::StraightLineSegments,
            line_color: Color::ByBlock,
            line_type_handle: None,
            line_weight: LineWeight::ByBlock,
            enable_landing: true,
            enable_dogleg: true,
            landing_distance: 0.36,
            landing_gap: 0.09,

            // Arrowhead
            arrowhead_handle: None,
            arrowhead_size: 0.18,

            // Content
            content_type: LeaderContentType::MText,

            // Text
            text_style_handle: None,
            text_left_attachment: TextAttachmentType::MiddleOfTopLine,
            text_right_attachment: TextAttachmentType::MiddleOfTopLine,
            text_top_attachment: TextAttachmentType::TopOfTopLine,
            text_bottom_attachment: TextAttachmentType::BottomOfBottomLine,
            text_attachment_direction: TextAttachmentDirectionType::Horizontal,
            text_angle_type: TextAngleType::Horizontal,
            text_alignment: TextAlignmentType::Left,
            text_color: Color::ByBlock,
            text_height: 0.18,
            text_frame: false,
            text_always_left: false,
            default_text: String::new(),

            // Block
            block_content_handle: None,
            block_content_color: Color::ByBlock,
            block_content_connection: BlockContentConnectionType::BlockExtents,
            block_content_rotation: 0.0,
            block_content_scale_x: 1.0,
            block_content_scale_y: 1.0,
            block_content_scale_z: 1.0,
            enable_block_scale: false,
            enable_block_rotation: false,

            // Scale and constraints
            scale_factor: 1.0,
            align_space: 0.0,
            break_gap_size: 0.125,
            max_leader_points: 2,
            first_segment_angle: 0.0,
            second_segment_angle: 0.0,

            // Draw order
            leader_draw_order: LeaderDrawOrderType::LeaderHeadFirst,
            multileader_draw_order: MultiLeaderDrawOrderType::ContentFirst,

            // Flags
            is_annotative: false,
            property_changed: false,
            unknown_flag_298: false,
        }
    }

    /// Creates the standard MultiLeaderStyle.
    pub fn standard() -> Self {
        Self::new(Self::STANDARD)
    }

    /// Sets all block content scale factors uniformly.
    pub fn set_block_scale(&mut self, scale: f64) {
        self.block_content_scale_x = scale;
        self.block_content_scale_y = scale;
        self.block_content_scale_z = scale;
    }

    /// Gets the uniform block scale if all factors are equal.
    pub fn uniform_block_scale(&self) -> Option<f64> {
        if (self.block_content_scale_x - self.block_content_scale_y).abs() < 1e-10
            && (self.block_content_scale_y - self.block_content_scale_z).abs() < 1e-10
        {
            Some(self.block_content_scale_x)
        } else {
            None
        }
    }

    /// Sets the block content rotation in degrees.
    pub fn set_block_rotation_degrees(&mut self, degrees: f64) {
        self.block_content_rotation = degrees.to_radians();
    }

    /// Gets the block content rotation in degrees.
    pub fn block_rotation_degrees(&self) -> f64 {
        self.block_content_rotation.to_degrees()
    }

    /// Returns true if this style uses text content.
    pub fn has_text_content(&self) -> bool {
        self.content_type == LeaderContentType::MText
    }

    /// Returns true if this style uses block content.
    pub fn has_block_content(&self) -> bool {
        self.content_type == LeaderContentType::Block
    }

    /// Returns true if this style uses tolerance content.
    pub fn has_tolerance_content(&self) -> bool {
        self.content_type == LeaderContentType::Tolerance
    }

    /// Returns true if leader lines are visible.
    pub fn has_visible_leaders(&self) -> bool {
        self.path_type != MultiLeaderPathType::Invisible
    }

    /// Returns true if leaders use spline curves.
    pub fn has_spline_leaders(&self) -> bool {
        self.path_type == MultiLeaderPathType::Spline
    }
}

impl Default for MultiLeaderStyle {
    fn default() -> Self {
        Self::standard()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multileaderstyle_creation() {
        let style = MultiLeaderStyle::new("TestStyle");
        assert_eq!(style.name, "TestStyle");
        assert!(style.description.is_empty());
    }

    #[test]
    fn test_standard_style() {
        let style = MultiLeaderStyle::standard();
        assert_eq!(style.name, "Standard");
    }

    #[test]
    fn test_default_values() {
        let style = MultiLeaderStyle::default();
        assert_eq!(style.path_type, MultiLeaderPathType::StraightLineSegments);
        assert_eq!(style.content_type, LeaderContentType::MText);
        assert!(style.enable_landing);
        assert!(style.enable_dogleg);
        assert!((style.landing_distance - 0.36).abs() < 1e-10);
        assert!((style.arrowhead_size - 0.18).abs() < 1e-10);
        assert!((style.text_height - 0.18).abs() < 1e-10);
        assert!((style.scale_factor - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_content_type_enum() {
        assert_eq!(LeaderContentType::from(0), LeaderContentType::None);
        assert_eq!(LeaderContentType::from(1), LeaderContentType::Block);
        assert_eq!(LeaderContentType::from(2), LeaderContentType::MText);
        assert_eq!(LeaderContentType::from(3), LeaderContentType::Tolerance);
        assert_eq!(LeaderContentType::from(99), LeaderContentType::MText);
    }

    #[test]
    fn test_path_type_enum() {
        assert_eq!(MultiLeaderPathType::from(0), MultiLeaderPathType::Invisible);
        assert_eq!(
            MultiLeaderPathType::from(1),
            MultiLeaderPathType::StraightLineSegments
        );
        assert_eq!(MultiLeaderPathType::from(2), MultiLeaderPathType::Spline);
    }

    #[test]
    fn test_text_attachment_type() {
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
    fn test_set_block_scale() {
        let mut style = MultiLeaderStyle::new("Test");
        style.set_block_scale(2.5);

        assert_eq!(style.block_content_scale_x, 2.5);
        assert_eq!(style.block_content_scale_y, 2.5);
        assert_eq!(style.block_content_scale_z, 2.5);
        assert_eq!(style.uniform_block_scale(), Some(2.5));
    }

    #[test]
    fn test_non_uniform_block_scale() {
        let mut style = MultiLeaderStyle::new("Test");
        style.block_content_scale_x = 1.0;
        style.block_content_scale_y = 2.0;
        style.block_content_scale_z = 1.0;

        assert_eq!(style.uniform_block_scale(), None);
    }

    #[test]
    fn test_block_rotation_degrees() {
        let mut style = MultiLeaderStyle::new("Test");
        style.set_block_rotation_degrees(45.0);

        assert!((style.block_rotation_degrees() - 45.0).abs() < 1e-10);
    }

    #[test]
    fn test_has_text_content() {
        let mut style = MultiLeaderStyle::new("Test");
        style.content_type = LeaderContentType::MText;
        assert!(style.has_text_content());
        assert!(!style.has_block_content());
        assert!(!style.has_tolerance_content());
    }

    #[test]
    fn test_has_block_content() {
        let mut style = MultiLeaderStyle::new("Test");
        style.content_type = LeaderContentType::Block;
        assert!(!style.has_text_content());
        assert!(style.has_block_content());
    }

    #[test]
    fn test_has_tolerance_content() {
        let mut style = MultiLeaderStyle::new("Test");
        style.content_type = LeaderContentType::Tolerance;
        assert!(style.has_tolerance_content());
    }

    #[test]
    fn test_has_visible_leaders() {
        let mut style = MultiLeaderStyle::new("Test");
        assert!(style.has_visible_leaders());

        style.path_type = MultiLeaderPathType::Invisible;
        assert!(!style.has_visible_leaders());
    }

    #[test]
    fn test_has_spline_leaders() {
        let mut style = MultiLeaderStyle::new("Test");
        assert!(!style.has_spline_leaders());

        style.path_type = MultiLeaderPathType::Spline;
        assert!(style.has_spline_leaders());
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
        let flags =
            LeaderLinePropertyOverrideFlags::PATH_TYPE | LeaderLinePropertyOverrideFlags::ARROWHEAD;

        assert!(flags.contains(LeaderLinePropertyOverrideFlags::PATH_TYPE));
        assert!(flags.contains(LeaderLinePropertyOverrideFlags::ARROWHEAD));
        assert!(!flags.contains(LeaderLinePropertyOverrideFlags::LINE_COLOR));
    }

    #[test]
    fn test_text_alignment_enum() {
        assert_eq!(TextAlignmentType::from(0), TextAlignmentType::Left);
        assert_eq!(TextAlignmentType::from(1), TextAlignmentType::Center);
        assert_eq!(TextAlignmentType::from(2), TextAlignmentType::Right);
    }

    #[test]
    fn test_text_angle_enum() {
        assert_eq!(
            TextAngleType::from(0),
            TextAngleType::ParallelToLastLeaderLine
        );
        assert_eq!(TextAngleType::from(1), TextAngleType::Horizontal);
        assert_eq!(TextAngleType::from(2), TextAngleType::Optimized);
    }

    #[test]
    fn test_block_connection_enum() {
        assert_eq!(
            BlockContentConnectionType::from(0),
            BlockContentConnectionType::BlockExtents
        );
        assert_eq!(
            BlockContentConnectionType::from(1),
            BlockContentConnectionType::BasePoint
        );
    }

    #[test]
    fn test_draw_order_enums() {
        assert_eq!(
            LeaderDrawOrderType::from(0),
            LeaderDrawOrderType::LeaderHeadFirst
        );
        assert_eq!(
            LeaderDrawOrderType::from(1),
            LeaderDrawOrderType::LeaderTailFirst
        );

        assert_eq!(
            MultiLeaderDrawOrderType::from(0),
            MultiLeaderDrawOrderType::ContentFirst
        );
        assert_eq!(
            MultiLeaderDrawOrderType::from(1),
            MultiLeaderDrawOrderType::LeaderFirst
        );
    }
}
