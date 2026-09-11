//! PlotSettings object - Plot configuration settings

use crate::types::Handle;

/// Plot paper units
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PlotPaperUnits {
    /// Inches
    #[default]
    Inches = 0,
    /// Millimeters
    Millimeters = 1,
    /// Pixels
    Pixels = 2,
}

impl PlotPaperUnits {
    /// Create from DXF code
    pub fn from_code(code: i16) -> Self {
        match code {
            1 => PlotPaperUnits::Millimeters,
            2 => PlotPaperUnits::Pixels,
            _ => PlotPaperUnits::Inches,
        }
    }

    /// Convert to DXF code
    pub fn to_code(self) -> i16 {
        self as i16
    }
}

/// Plot rotation angle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PlotRotation {
    /// No rotation (0 degrees)
    #[default]
    None = 0,
    /// 90 degrees counter-clockwise
    Degrees90 = 1,
    /// 180 degrees
    Degrees180 = 2,
    /// 270 degrees counter-clockwise (90 clockwise)
    Degrees270 = 3,
}

impl PlotRotation {
    /// Create from DXF code
    pub fn from_code(code: i16) -> Self {
        match code {
            1 => PlotRotation::Degrees90,
            2 => PlotRotation::Degrees180,
            3 => PlotRotation::Degrees270,
            _ => PlotRotation::None,
        }
    }

    /// Convert to DXF code
    pub fn to_code(self) -> i16 {
        self as i16
    }

    /// Get rotation angle in degrees
    pub fn to_degrees(self) -> f64 {
        match self {
            PlotRotation::None => 0.0,
            PlotRotation::Degrees90 => 90.0,
            PlotRotation::Degrees180 => 180.0,
            PlotRotation::Degrees270 => 270.0,
        }
    }

    /// Get rotation angle in radians
    pub fn to_radians(self) -> f64 {
        self.to_degrees().to_radians()
    }
}

/// Plot type - what area of the drawing to plot
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PlotType {
    /// Plot the last screen display
    LastScreenDisplay = 0,
    /// Plot drawing extents
    Extents = 1,
    /// Plot drawing limits
    Limits = 2,
    /// Plot a named view
    View = 3,
    /// Plot a specific window
    #[default]
    Window = 4,
    /// Plot the layout
    Layout = 5,
}

impl PlotType {
    /// Create from DXF code
    pub fn from_code(code: i16) -> Self {
        match code {
            0 => PlotType::LastScreenDisplay,
            1 => PlotType::Extents,
            2 => PlotType::Limits,
            3 => PlotType::View,
            5 => PlotType::Layout,
            _ => PlotType::Window,
        }
    }

    /// Convert to DXF code
    pub fn to_code(self) -> i16 {
        self as i16
    }
}

/// Scale type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ScaledType {
    /// Scale to fit
    #[default]
    ScaleToFit = 0,
    /// Use custom scale
    CustomScale = 1,
    /// 1:1
    OneToOne = 16,
    /// 1:2
    OneToTwo = 17,
    /// 1:4
    OneToFour = 18,
    /// 1:8
    OneToEight = 19,
    /// 1:10
    OneToTen = 20,
    /// 1:16
    OneToSixteen = 21,
    /// 1:20
    OneToTwenty = 22,
    /// 1:30
    OneToThirty = 23,
    /// 1:40
    OneToForty = 24,
    /// 1:50
    OneToFifty = 25,
    /// 1:100
    OneToHundred = 26,
    /// 2:1
    TwoToOne = 27,
    /// 4:1
    FourToOne = 28,
    /// 8:1
    EightToOne = 29,
    /// 10:1
    TenToOne = 30,
    /// 100:1
    HundredToOne = 31,
}

impl ScaledType {
    /// Create from DXF code
    pub fn from_code(code: i16) -> Self {
        match code {
            0 => ScaledType::ScaleToFit,
            1 => ScaledType::CustomScale,
            16 => ScaledType::OneToOne,
            17 => ScaledType::OneToTwo,
            18 => ScaledType::OneToFour,
            19 => ScaledType::OneToEight,
            20 => ScaledType::OneToTen,
            21 => ScaledType::OneToSixteen,
            22 => ScaledType::OneToTwenty,
            23 => ScaledType::OneToThirty,
            24 => ScaledType::OneToForty,
            25 => ScaledType::OneToFifty,
            26 => ScaledType::OneToHundred,
            27 => ScaledType::TwoToOne,
            28 => ScaledType::FourToOne,
            29 => ScaledType::EightToOne,
            30 => ScaledType::TenToOne,
            31 => ScaledType::HundredToOne,
            _ => ScaledType::CustomScale,
        }
    }

    /// Convert to DXF code
    pub fn to_code(self) -> i16 {
        self as i16
    }

    /// Get the scale factor
    pub fn scale_factor(&self) -> f64 {
        match self {
            ScaledType::ScaleToFit => 0.0, // Special case
            ScaledType::CustomScale => 1.0,
            ScaledType::OneToOne => 1.0,
            ScaledType::OneToTwo => 0.5,
            ScaledType::OneToFour => 0.25,
            ScaledType::OneToEight => 0.125,
            ScaledType::OneToTen => 0.1,
            ScaledType::OneToSixteen => 0.0625,
            ScaledType::OneToTwenty => 0.05,
            ScaledType::OneToThirty => 1.0 / 30.0,
            ScaledType::OneToForty => 0.025,
            ScaledType::OneToFifty => 0.02,
            ScaledType::OneToHundred => 0.01,
            ScaledType::TwoToOne => 2.0,
            ScaledType::FourToOne => 4.0,
            ScaledType::EightToOne => 8.0,
            ScaledType::TenToOne => 10.0,
            ScaledType::HundredToOne => 100.0,
        }
    }
}

/// Shade plot mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ShadePlotMode {
    /// As displayed
    #[default]
    AsDisplayed = 0,
    /// Wireframe
    Wireframe = 1,
    /// Hidden
    Hidden = 2,
    /// Rendered
    Rendered = 3,
}

impl ShadePlotMode {
    /// Create from DXF code
    pub fn from_code(code: i16) -> Self {
        match code {
            1 => ShadePlotMode::Wireframe,
            2 => ShadePlotMode::Hidden,
            3 => ShadePlotMode::Rendered,
            _ => ShadePlotMode::AsDisplayed,
        }
    }

    /// Convert to DXF code
    pub fn to_code(self) -> i16 {
        self as i16
    }
}

/// Shade plot resolution level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ShadePlotResolutionLevel {
    /// Draft
    Draft = 0,
    /// Preview
    Preview = 1,
    /// Normal
    #[default]
    Normal = 2,
    /// Presentation
    Presentation = 3,
    /// Maximum
    Maximum = 4,
    /// Custom
    Custom = 5,
}

impl ShadePlotResolutionLevel {
    /// Create from DXF code
    pub fn from_code(code: i16) -> Self {
        match code {
            0 => ShadePlotResolutionLevel::Draft,
            1 => ShadePlotResolutionLevel::Preview,
            3 => ShadePlotResolutionLevel::Presentation,
            4 => ShadePlotResolutionLevel::Maximum,
            5 => ShadePlotResolutionLevel::Custom,
            _ => ShadePlotResolutionLevel::Normal,
        }
    }

    /// Convert to DXF code
    pub fn to_code(self) -> i16 {
        self as i16
    }
}

/// Plot flags
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlotFlags {
    /// Bits not currently assigned by the published flag schema.
    ///
    /// Keeping these bits makes a read/write cycle lossless when a newer CAD
    /// version introduces another flag.
    pub unknown_bits: i32,
    /// Plot viewport borders
    pub plot_viewport_borders: bool,
    /// Show plot styles
    pub show_plot_styles: bool,
    /// Plot centered
    pub plot_centered: bool,
    /// Plot hidden
    pub plot_hidden: bool,
    /// Use standard scale
    pub use_standard_scale: bool,
    /// Plot plot styles
    pub plot_plot_styles: bool,
    /// Scale lineweights
    pub scale_lineweights: bool,
    /// Print lineweights
    pub print_lineweights: bool,
    /// Draw viewports first
    pub draw_viewports_first: bool,
    /// Model type
    pub model_type: bool,
    /// Update paper
    pub update_paper: bool,
    /// Zoom to paper on update
    pub zoom_to_paper_on_update: bool,
    /// Initializing
    pub initializing: bool,
    /// Prev plot init
    pub prev_plot_init: bool,
}

impl PlotFlags {
    /// Create from DXF bit value
    pub fn from_bits(bits: i32) -> Self {
        let bits = bits & 0xFFFF;
        Self {
            unknown_bits: bits & !0x7EFF,
            plot_viewport_borders: (bits & 1) != 0,
            show_plot_styles: (bits & 2) != 0,
            plot_centered: (bits & 4) != 0,
            plot_hidden: (bits & 8) != 0,
            use_standard_scale: (bits & 16) != 0,
            plot_plot_styles: (bits & 32) != 0,
            scale_lineweights: (bits & 64) != 0,
            print_lineweights: (bits & 128) != 0,
            draw_viewports_first: (bits & 512) != 0,
            model_type: (bits & 1024) != 0,
            update_paper: (bits & 2048) != 0,
            zoom_to_paper_on_update: (bits & 4096) != 0,
            initializing: (bits & 8192) != 0,
            prev_plot_init: (bits & 16384) != 0,
        }
    }

    /// Convert to DXF bit value
    pub fn to_bits(&self) -> i32 {
        let mut bits = self.unknown_bits & !0x7EFF;
        if self.plot_viewport_borders {
            bits |= 1;
        }
        if self.show_plot_styles {
            bits |= 2;
        }
        if self.plot_centered {
            bits |= 4;
        }
        if self.plot_hidden {
            bits |= 8;
        }
        if self.use_standard_scale {
            bits |= 16;
        }
        if self.plot_plot_styles {
            bits |= 32;
        }
        if self.scale_lineweights {
            bits |= 64;
        }
        if self.print_lineweights {
            bits |= 128;
        }
        if self.draw_viewports_first {
            bits |= 512;
        }
        if self.model_type {
            bits |= 1024;
        }
        if self.update_paper {
            bits |= 2048;
        }
        if self.zoom_to_paper_on_update {
            bits |= 4096;
        }
        if self.initializing {
            bits |= 8192;
        }
        if self.prev_plot_init {
            bits |= 16384;
        }
        bits
    }
}

/// Paper margins
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PaperMargin {
    /// Left margin (unprintable area)
    pub left: f64,
    /// Bottom margin (unprintable area)
    pub bottom: f64,
    /// Right margin (unprintable area)
    pub right: f64,
    /// Top margin (unprintable area)
    pub top: f64,
}

impl PaperMargin {
    /// Create a new margin with equal values
    pub fn uniform(margin: f64) -> Self {
        Self {
            left: margin,
            bottom: margin,
            right: margin,
            top: margin,
        }
    }

    /// Create a new margin with specified values
    pub fn new(left: f64, bottom: f64, right: f64, top: f64) -> Self {
        Self {
            left,
            bottom,
            right,
            top,
        }
    }

    /// Check if all margins are zero
    pub fn is_zero(&self) -> bool {
        self.left == 0.0 && self.bottom == 0.0 && self.right == 0.0 && self.top == 0.0
    }

    /// Get horizontal margin total
    pub fn horizontal_total(&self) -> f64 {
        self.left + self.right
    }

    /// Get vertical margin total
    pub fn vertical_total(&self) -> f64 {
        self.top + self.bottom
    }
}

/// Plot window area
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlotWindow {
    /// Lower-left X coordinate
    pub lower_left_x: f64,
    /// Lower-left Y coordinate
    pub lower_left_y: f64,
    /// Upper-right X coordinate
    pub upper_right_x: f64,
    /// Upper-right Y coordinate
    pub upper_right_y: f64,
}

impl PlotWindow {
    /// Create a new plot window
    pub fn new(x1: f64, y1: f64, x2: f64, y2: f64) -> Self {
        Self {
            lower_left_x: x1.min(x2),
            lower_left_y: y1.min(y2),
            upper_right_x: x1.max(x2),
            upper_right_y: y1.max(y2),
        }
    }

    /// Get window width
    pub fn width(&self) -> f64 {
        self.upper_right_x - self.lower_left_x
    }

    /// Get window height
    pub fn height(&self) -> f64 {
        self.upper_right_y - self.lower_left_y
    }

    /// Check if window is empty
    pub fn is_empty(&self) -> bool {
        self.width() <= 0.0 || self.height() <= 0.0
    }
}

/// Plot settings object
///
/// Contains all settings for plotting (printing) a layout.
///
/// # DXF Object Type
/// PLOTSETTINGS
///
/// # Example
/// ```ignore
/// use acadrust::objects::{PlotSettings, PlotRotation, PlotPaperUnits};
///
/// let mut settings = PlotSettings::new("Layout1");
/// settings.paper_size = "ISO_A4_(210.00_x_297.00_MM)".to_string();
/// settings.rotation = PlotRotation::Degrees90;
/// settings.paper_units = PlotPaperUnits::Millimeters;
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlotSettings {
    /// Unique handle
    pub handle: Handle,
    /// Owner handle
    pub owner: Handle,
    /// Reactor handles.
    pub reactors: Vec<Handle>,
    /// Extension dictionary handle.
    pub xdictionary_handle: Option<Handle>,
    /// Page/Layout name (DXF code 1)
    pub page_name: String,
    /// Printer/plotter name (DXF code 2)
    pub printer_name: String,
    /// Paper size name (DXF code 4)
    pub paper_size: String,
    /// Plot view name (DXF code 6)
    pub plot_view_name: String,
    /// Current style sheet (DXF code 7)
    pub current_style_sheet: String,
    /// Paper width (DXF code 44)
    pub paper_width: f64,
    /// Paper height (DXF code 45)
    pub paper_height: f64,
    /// Unprintable margins (DXF codes 40-43)
    pub margins: PaperMargin,
    /// Plot origin X (DXF code 46)
    pub origin_x: f64,
    /// Plot origin Y (DXF code 47)
    pub origin_y: f64,
    /// Plot window (DXF codes 48-51, 140-141)
    pub plot_window: PlotWindow,
    /// Custom print scale numerator (DXF code 142)
    pub scale_numerator: f64,
    /// Custom print scale denominator (DXF code 143)
    pub scale_denominator: f64,
    /// Paper units (DXF code 72)
    pub paper_units: PlotPaperUnits,
    /// Plot rotation (DXF code 73)
    pub rotation: PlotRotation,
    /// Plot type (DXF code 74)
    pub plot_type: PlotType,
    /// Standard scale type (DXF code 75)
    pub scale_type: ScaledType,
    /// Shade plot mode (DXF code 76)
    pub shade_plot_mode: ShadePlotMode,
    /// Shade plot resolution level (DXF code 77)
    pub shade_plot_resolution: ShadePlotResolutionLevel,
    /// Shade plot custom DPI (DXF code 78)
    pub shade_plot_dpi: i16,
    /// Plot flags (DXF code 70)
    pub flags: PlotFlags,
    /// Standard scale factor stored independently in code 147.
    pub standard_scale_factor: f64,
    /// Paper image origin X (DXF code 148).
    pub paper_image_origin_x: f64,
    /// Paper image origin Y (DXF code 149).
    pub paper_image_origin_y: f64,
    /// Named plot-view handle used by native DWG records.
    pub plot_view_handle: Handle,
    /// Shade-plot visual-style handle (DXF code 333).
    pub visual_style_handle: Handle,
    /// Cached custom-scale calculation.
    cached_scale: Option<f64>,
}

impl PlotSettings {
    /// Object type name
    pub const OBJECT_TYPE: &'static str = "PLOTSETTINGS";

    /// Create new plot settings
    pub fn new(page_name: impl Into<String>) -> Self {
        Self {
            handle: Handle::NULL,
            owner: Handle::NULL,
            reactors: Vec::new(),
            xdictionary_handle: None,
            page_name: page_name.into(),
            printer_name: String::new(),
            paper_size: String::new(),
            plot_view_name: String::new(),
            current_style_sheet: String::new(),
            paper_width: 0.0,
            paper_height: 0.0,
            margins: PaperMargin::default(),
            origin_x: 0.0,
            origin_y: 0.0,
            plot_window: PlotWindow::default(),
            scale_numerator: 1.0,
            scale_denominator: 1.0,
            paper_units: PlotPaperUnits::default(),
            rotation: PlotRotation::default(),
            plot_type: PlotType::default(),
            scale_type: ScaledType::default(),
            shade_plot_mode: ShadePlotMode::default(),
            shade_plot_resolution: ShadePlotResolutionLevel::default(),
            shade_plot_dpi: 300,
            flags: PlotFlags::default(),
            standard_scale_factor: 1.0,
            paper_image_origin_x: 0.0,
            paper_image_origin_y: 0.0,
            plot_view_handle: Handle::NULL,
            visual_style_handle: Handle::NULL,
            cached_scale: None,
        }
    }

    /// Create with a standard paper size
    pub fn with_paper(page_name: impl Into<String>, paper_size: impl Into<String>) -> Self {
        let mut settings = Self::new(page_name);
        settings.paper_size = paper_size.into();
        settings
    }

    /// Get the plot scale factor
    pub fn scale_factor(&self) -> f64 {
        if let Some(cached) = self.cached_scale {
            return cached;
        }

        if self.scale_denominator == 0.0 {
            1.0
        } else {
            self.scale_numerator / self.scale_denominator
        }
    }

    /// Set a custom scale
    pub fn set_custom_scale(&mut self, numerator: f64, denominator: f64) {
        self.scale_numerator = numerator;
        self.scale_denominator = denominator;
        self.scale_type = ScaledType::CustomScale;
        self.cached_scale = None;
    }

    /// Set scale to fit
    pub fn set_scale_to_fit(&mut self) {
        self.scale_type = ScaledType::ScaleToFit;
        self.flags.use_standard_scale = false;
    }

    /// Set a standard scale
    pub fn set_standard_scale(&mut self, scale: ScaledType) {
        self.scale_type = scale;
        self.flags.use_standard_scale = true;

        // Update numerator/denominator based on scale
        match scale {
            ScaledType::OneToOne => {
                self.scale_numerator = 1.0;
                self.scale_denominator = 1.0;
            }
            ScaledType::OneToTwo => {
                self.scale_numerator = 1.0;
                self.scale_denominator = 2.0;
            }
            ScaledType::TwoToOne => {
                self.scale_numerator = 2.0;
                self.scale_denominator = 1.0;
            }
            ScaledType::OneToTen => {
                self.scale_numerator = 1.0;
                self.scale_denominator = 10.0;
            }
            ScaledType::TenToOne => {
                self.scale_numerator = 10.0;
                self.scale_denominator = 1.0;
            }
            _ => {}
        }
        self.cached_scale = None;
    }

    /// Get printable width (paper width minus margins)
    pub fn printable_width(&self) -> f64 {
        self.paper_width - self.margins.horizontal_total()
    }

    /// Get printable height (paper height minus margins)
    pub fn printable_height(&self) -> f64 {
        self.paper_height - self.margins.vertical_total()
    }

    /// Set paper dimensions
    pub fn set_paper_size(&mut self, width: f64, height: f64) {
        self.paper_width = width;
        self.paper_height = height;
    }

    /// Set paper dimensions with margins
    pub fn set_paper_with_margins(&mut self, width: f64, height: f64, margins: PaperMargin) {
        self.paper_width = width;
        self.paper_height = height;
        self.margins = margins;
    }

    /// Set plot window
    pub fn set_plot_window(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) {
        self.plot_window = PlotWindow::new(x1, y1, x2, y2);
        self.plot_type = PlotType::Window;
    }

    /// Set plot origin
    pub fn set_origin(&mut self, x: f64, y: f64) {
        self.origin_x = x;
        self.origin_y = y;
    }

    /// Center the plot
    pub fn center_plot(&mut self) {
        self.flags.plot_centered = true;
    }

    /// Check if using scale to fit
    pub fn is_scale_to_fit(&self) -> bool {
        matches!(self.scale_type, ScaledType::ScaleToFit)
    }

    /// Check if using a custom scale
    pub fn is_custom_scale(&self) -> bool {
        matches!(self.scale_type, ScaledType::CustomScale)
    }

    /// Set printer name
    pub fn set_printer(&mut self, name: impl Into<String>) {
        self.printer_name = name.into();
    }

    /// Set style sheet
    pub fn set_style_sheet(&mut self, name: impl Into<String>) {
        self.current_style_sheet = name.into();
    }

    /// Builder: Set paper size name
    pub fn with_paper_size(mut self, size: impl Into<String>) -> Self {
        self.paper_size = size.into();
        self
    }

    /// Builder: Set printer
    pub fn with_printer(mut self, printer: impl Into<String>) -> Self {
        self.printer_name = printer.into();
        self
    }

    /// Builder: Set rotation
    pub fn with_rotation(mut self, rotation: PlotRotation) -> Self {
        self.rotation = rotation;
        self
    }

    /// Builder: Set paper units
    pub fn with_units(mut self, units: PlotPaperUnits) -> Self {
        self.paper_units = units;
        self
    }

    /// Builder: Set scale
    pub fn with_scale(mut self, numerator: f64, denominator: f64) -> Self {
        self.set_custom_scale(numerator, denominator);
        self
    }
}

impl Default for PlotSettings {
    fn default() -> Self {
        Self::new("")
    }
}
