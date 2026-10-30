//! Classic pull-down menus above the ribbon (MENUBAR 1). Every entry runs a
//! command, so the menus stay a view of the command set rather than a second
//! implementation of it.

use crate::app::Message;
use crate::ui::style::chrome;
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{mouse, overlay, renderer, Shell};
use iced::widget::{button, container, mouse_area, row, text, Space};
use iced::{
    Background, Border, Element, Event, Fill, Length, Rectangle, Renderer, Shadow, Size, Theme,
    Vector,
};
use iced_aw::menu::{DrawPath, Item, Menu, MenuBar};

/// Height of the menu row, and of a menu title in it.
const ROW_H: f32 = 25.0;
const TITLE_H: f32 = 20.0;

/// One menu entry.
#[derive(Clone, Copy)]
enum Entry {
    /// Label, command line (a space separates the answers to its prompts,
    /// as in a menu macro) and shortcut text.
    Cmd(&'static str, &'static str, &'static str),
    Sep,
    /// Label and the entries of a submenu.
    Sub(&'static str, &'static [Entry]),
}

use Entry::{Cmd, Sep, Sub};

/// The menus in menu-bar order. The Window menu also lists the open
/// drawings (see `view`).
const MENUS: &[(&str, &[Entry])] = &[
    (
        "File",
        &[
            Cmd("New...", "NEW", "Ctrl+N"),
            Cmd("New Sheet Set...", "NEWSHEETSET", ""),
            Cmd("Open...", "OPEN", "Ctrl+O"),
            Cmd("Open Sheet Set...", "OPENSHEETSET", ""),
            Cmd("Load Markup Set...", "MARKUP", ""),
            Cmd("Close", "CLOSE", ""),
            Sep,
            Cmd("Import...", "IMPORT", ""),
            Cmd("Attach...", "ATTACH", ""),
            Sep,
            Cmd("Save", "QSAVE", "Ctrl+S"),
            Cmd("Save As...", "SAVEAS", "Ctrl+Shift+S"),
            Cmd("Export...", "EXPORT", ""),
            Cmd("eTransmit...", "ETRANSMIT", ""),
            Sep,
            Cmd("Page Setup Manager...", "PAGESETUP", ""),
            Cmd("Plotter Manager...", "PLOTTERMANAGER", ""),
            Cmd("Plot Style Manager...", "STYLESMANAGER", ""),
            Cmd("Plot Preview", "PREVIEW", ""),
            Cmd("Plot...", "PLOT", "Ctrl+P"),
            Cmd("Publish...", "PUBLISH", ""),
            Cmd("View Plot and Publish Details...", "VIEWPLOTDETAILS", ""),
            Sep,
            Sub(
                "Drawing Utilities",
                &[
                    Cmd("Audit", "AUDIT", ""),
                    Cmd("Status", "STATUS", ""),
                    Cmd("Purge...", "PURGE", ""),
                    Cmd("Recover...", "RECOVER", ""),
                    Cmd("Recover Drawing and Xrefs...", "RECOVERALL", ""),
                ],
            ),
            Cmd("Send...", "SEND", ""),
            Cmd("Drawing Properties...", "DWGPROPS", ""),
            Sep,
            Cmd("Exit", "QUIT", "Ctrl+Q"),
        ],
    ),
    (
        "Edit",
        &[
            Cmd("Undo", "U", "Ctrl+Z"),
            Cmd("Redo", "MREDO", "Ctrl+Y"),
            Sep,
            Cmd("Cut", "CUTCLIP", "Ctrl+X"),
            Cmd("Copy", "COPYCLIP", "Ctrl+C"),
            Cmd("Copy with Base Point", "COPYBASE", "Ctrl+Shift+C"),
            Cmd("Copy Link", "COPYLINK", ""),
            Cmd("Paste", "PASTECLIP", "Ctrl+V"),
            Cmd("Paste as Block", "PASTEBLOCK", "Ctrl+Shift+V"),
            Cmd("Paste as Hyperlink", "PASTEASHYPERLINK", ""),
            Cmd("Paste to Original Coordinates", "PASTEORIG", ""),
            Cmd("Paste Special...", "PASTESPEC", ""),
            Sep,
            Cmd("Clear", "ERASE", "Del"),
            Cmd("Select All", "AI_SELALL", "Ctrl+A"),
            Sep,
            Cmd("OLE Links...", "OLELINKS", ""),
            Cmd("Find...", "FIND", ""),
        ],
    ),
    (
        "View",
        &[
            Cmd("Redraw", "REDRAWALL", ""),
            Cmd("Regen", "REGEN", ""),
            Cmd("Regen All", "REGENALL", ""),
            Sep,
            Sub(
                "Zoom",
                &[
                    Cmd("Realtime", "ZOOM", ""),
                    Cmd("Previous", "ZOOM P", ""),
                    Cmd("Window", "ZOOM W", ""),
                    Cmd("Dynamic", "ZOOM D", ""),
                    Cmd("Scale", "ZOOM S", ""),
                    Cmd("Center", "ZOOM C", ""),
                    Cmd("Object", "ZOOM O", ""),
                    Sep,
                    Cmd("In", "ZOOM 2X", ""),
                    Cmd("Out", "ZOOM .5X", ""),
                    Sep,
                    Cmd("All", "ZOOM A", ""),
                    Cmd("Extents", "ZOOM E", ""),
                ],
            ),
            Sub(
                "Pan",
                &[
                    Cmd("Realtime", "PAN", ""),
                    Cmd("Point", "-PAN", ""),
                    Sep,
                    Cmd("Left", "PAN LEFT", ""),
                    Cmd("Right", "PAN RIGHT", ""),
                    Cmd("Up", "PAN UP", ""),
                    Cmd("Down", "PAN DOWN", ""),
                ],
            ),
            Sep,
            Cmd("Clean Screen", "CLEANSCREEN", "Ctrl+0"),
            Sep,
            Sub(
                "Viewports",
                &[
                    Cmd("Named Viewports...", "VPORTS", ""),
                    Cmd("New Viewports...", "VPORTS", ""),
                    Sep,
                    Cmd("1 Viewport", "-VPORTS 1", ""),
                    Cmd("2 Viewports", "-VPORTS 2", ""),
                    Cmd("3 Viewports", "-VPORTS 3", ""),
                    Cmd("4 Viewports", "-VPORTS 4", ""),
                    Sep,
                    Cmd("Join", "-VPORTS J", ""),
                ],
            ),
            Cmd("Named Views...", "VIEW", ""),
            Sub(
                "3D Views",
                &[
                    Cmd("Viewpoint Presets...", "DDVPOINT", ""),
                    Cmd("Plan View", "PLAN", ""),
                    Sep,
                    Cmd("Top", "-VIEW TOP", ""),
                    Cmd("Bottom", "-VIEW BOTTOM", ""),
                    Cmd("Left", "-VIEW LEFT", ""),
                    Cmd("Right", "-VIEW RIGHT", ""),
                    Cmd("Front", "-VIEW FRONT", ""),
                    Cmd("Back", "-VIEW BACK", ""),
                    Sep,
                    Cmd("SW Isometric", "-VIEW SWISO", ""),
                    Cmd("SE Isometric", "-VIEW SEISO", ""),
                    Cmd("NE Isometric", "-VIEW NEISO", ""),
                    Cmd("NW Isometric", "-VIEW NWISO", ""),
                ],
            ),
            Sep,
            Sub(
                "Visual Styles",
                &[
                    Cmd("2D Wireframe", "VSCURRENT 2", ""),
                    Cmd("Wireframe", "VSCURRENT W", ""),
                    Cmd("Hidden", "VSCURRENT H", ""),
                    Cmd("Realistic", "VSCURRENT R", ""),
                    Cmd("Conceptual", "VSCURRENT C", ""),
                    Cmd("Shaded", "VSCURRENT S", ""),
                    Sep,
                    Cmd("Visual Styles Manager...", "VISUALSTYLES", ""),
                ],
            ),
            Sep,
            Sub(
                "Display",
                &[
                    Cmd("UCS Icon", "UCSICON", ""),
                    Cmd("Attribute Display", "ATTDISP", ""),
                    Cmd("Text Window", "TEXTSCR", "F2"),
                ],
            ),
            Cmd("Toolbars...", "TOOLBAR", ""),
        ],
    ),
    (
        "Insert",
        &[
            Cmd("Block...", "INSERT", ""),
            Cmd("DWG Reference...", "XATTACH", ""),
            Cmd("DWF Underlay...", "DWFATTACH", ""),
            Cmd("DGN Underlay...", "DGNATTACH", ""),
            Cmd("PDF Underlay...", "PDFATTACH", ""),
            Cmd("Raster Image Reference...", "IMAGEATTACH", ""),
            Sep,
            Cmd("Field...", "FIELD", ""),
            Sep,
            Sub(
                "Layout",
                &[
                    Cmd("New Layout", "LAYOUT N", ""),
                    Cmd("Layout from Template...", "LAYOUT T", ""),
                    Cmd("Create Layout Wizard", "LAYOUTWIZARD", ""),
                ],
            ),
            Sep,
            Cmd("OLE Object...", "INSERTOBJ", ""),
            Sep,
            Cmd("External References", "EXTERNALREFERENCES", ""),
            Cmd("Hyperlink...", "HYPERLINK", "Ctrl+K"),
        ],
    ),
    (
        "Format",
        &[
            Cmd("Layer...", "LAYER", ""),
            Cmd("Layer States Manager...", "LAYERSTATE", ""),
            Sub(
                "Layer Tools",
                &[
                    Cmd("Make Object's Layer Current", "LAYMCUR", ""),
                    Cmd("Layer Previous", "LAYERP", ""),
                    Cmd("Layer Match", "LAYMCH", ""),
                    Cmd("Change to Current Layer", "LAYCUR", ""),
                    Cmd("Copy Objects to New Layer", "COPYTOLAYER", ""),
                    Sep,
                    Cmd("Layer Isolate", "LAYISO", ""),
                    Cmd("Layer Unisolate", "LAYUNISO", ""),
                    Cmd("Layer Off", "LAYOFF", ""),
                    Cmd("Turn All Layers On", "LAYON", ""),
                    Cmd("Layer Freeze", "LAYFRZ", ""),
                    Cmd("Thaw All Layers", "LAYTHW", ""),
                    Cmd("Layer Lock", "LAYLCK", ""),
                    Cmd("Layer Unlock", "LAYULK", ""),
                    Sep,
                    Cmd("Layer Merge", "LAYMRG", ""),
                    Cmd("Layer Delete", "LAYDEL", ""),
                ],
            ),
            Sep,
            Cmd("Color...", "COLOR", ""),
            Cmd("Linetype...", "LINETYPE", ""),
            Cmd("Lineweight...", "LWEIGHT", ""),
            Cmd("Transparency", "TRANSPARENCY", ""),
            Cmd("Scale List...", "SCALELISTEDIT", ""),
            Sep,
            Cmd("Text Style...", "STYLE", ""),
            Cmd("Dimension Style...", "DIMSTYLE", ""),
            Cmd("Table Style...", "TABLESTYLE", ""),
            Cmd("Multileader Style...", "MLEADERSTYLE", ""),
            Cmd("Plot Style...", "PLOTSTYLE", ""),
            Cmd("Point Style...", "PTYPE", ""),
            Cmd("Multiline Style...", "MLSTYLE", ""),
            Sep,
            Cmd("Units...", "UNITS", ""),
            Cmd("Thickness", "THICKNESS", ""),
            Cmd("Drawing Limits", "LIMITS", ""),
            Sep,
            Cmd("Rename...", "RENAME", ""),
        ],
    ),
    (
        "Tools",
        &[
            Sub(
                "Workspaces",
                &[
                    Cmd("2D Drafting", "WSCURRENT 2D", ""),
                    Cmd("3D Modeling", "WSCURRENT 3D", ""),
                    Sep,
                    Cmd("Save Current As...", "WSSAVE", ""),
                    Cmd("Workspace Settings...", "WSSETTINGS", ""),
                    Cmd("Customize...", "CUI", ""),
                ],
            ),
            Sub(
                "Palettes",
                &[
                    Cmd("Properties", "PROPERTIES", "Ctrl+1"),
                    Cmd("Layers", "LAYER", ""),
                    Cmd("Design Center", "ADCENTER", "Ctrl+2"),
                    Cmd("Tool Palettes", "TOOLPALETTES", "Ctrl+3"),
                    Cmd("Sheet Set Manager", "SHEETSET", "Ctrl+4"),
                    Cmd("Markup Set Manager", "MARKUP", "Ctrl+7"),
                    Cmd("QuickCalc", "QUICKCALC", "Ctrl+8"),
                    Cmd("External References", "EXTERNALREFERENCES", ""),
                ],
            ),
            Cmd("Toolbars...", "TOOLBAR", ""),
            Cmd("Command Line", "COMMANDLINE", "Ctrl+9"),
            Cmd("Clean Screen", "CLEANSCREEN", "Ctrl+0"),
            Sep,
            Cmd("Spelling", "SPELL", ""),
            Cmd("Quick Select...", "QSELECT", ""),
            Sub(
                "Draw Order",
                &[
                    Cmd("Bring to Front", "DRAWORDER F", ""),
                    Cmd("Send to Back", "DRAWORDER B", ""),
                    Cmd("Bring Above Objects", "DRAWORDER A", ""),
                    Cmd("Send Under Objects", "DRAWORDER U", ""),
                    Sep,
                    Cmd("Bring Text to Front", "TEXTTOFRONT T", ""),
                    Cmd("Bring Dimensions to Front", "TEXTTOFRONT D", ""),
                    Cmd("Bring Leaders to Front", "TEXTTOFRONT L", ""),
                    Cmd("Send Hatches to Back", "HATCHTOBACK", ""),
                ],
            ),
            Sub(
                "Inquiry",
                &[
                    Cmd("Distance", "MEASUREGEOM D", ""),
                    Cmd("Radius", "MEASUREGEOM R", ""),
                    Cmd("Angle", "MEASUREGEOM A", ""),
                    Cmd("Area", "MEASUREGEOM AR", ""),
                    Cmd("Volume", "MEASUREGEOM V", ""),
                    Sep,
                    Cmd("Region/Mass Properties", "MASSPROP", ""),
                    Cmd("List", "LIST", ""),
                    Cmd("ID Point", "ID", ""),
                    Cmd("Time", "TIME", ""),
                    Cmd("Status", "STATUS", ""),
                    Cmd("Set Variable", "SETVAR", ""),
                ],
            ),
            Sep,
            Cmd("Update Fields", "UPDATEFIELD", ""),
            Cmd("Block Editor", "BEDIT", ""),
            Cmd("Data Extraction...", "DATAEXTRACTION", ""),
            Sep,
            Cmd("Load Application...", "APPLOAD", ""),
            Cmd("Run Script...", "SCRIPT", ""),
            Sep,
            Cmd("Named UCS...", "UCSMAN", ""),
            Sub(
                "New UCS",
                &[
                    Cmd("World", "UCS W", ""),
                    Cmd("Origin", "UCS O", ""),
                    Cmd("3 Point", "UCS 3", ""),
                    Cmd("Object", "UCS OB", ""),
                    Cmd("View", "UCS V", ""),
                ],
            ),
            Cmd("Geographic Location...", "GEOGRAPHICLOCATION", ""),
            Sep,
            Cmd("Drafting Settings...", "DSETTINGS", ""),
            Sub(
                "Customize",
                &[
                    Cmd("Interface...", "CUI", ""),
                    Cmd("Tool Palettes...", "CUSTOMIZE", ""),
                    Cmd("Edit Program Parameters (acad.pgp)", "AI_EDITCUSTFILE", ""),
                ],
            ),
            Cmd("Options...", "OPTIONS", ""),
        ],
    ),
    (
        "Draw",
        &[
            Cmd("Line", "LINE", ""),
            Cmd("Ray", "RAY", ""),
            Cmd("Construction Line", "XLINE", ""),
            Cmd("Multiline", "MLINE", ""),
            Sep,
            Cmd("Polyline", "PLINE", ""),
            Cmd("3D Polyline", "3DPOLY", ""),
            Cmd("Polygon", "POLYGON", ""),
            Cmd("Rectangle", "RECTANG", ""),
            Cmd("Helix", "HELIX", ""),
            Sep,
            Sub(
                "Arc",
                &[
                    Cmd("3 Points", "ARC", ""),
                    Cmd("Start, Center, End", "ARC SCE", ""),
                    Cmd("Start, Center, Angle", "ARC SCA", ""),
                    Cmd("Start, Center, Length", "ARC SCL", ""),
                    Cmd("Start, End, Angle", "ARC SEA", ""),
                    Cmd("Start, End, Direction", "ARC SED", ""),
                    Cmd("Start, End, Radius", "ARC SER", ""),
                    Sep,
                    Cmd("Center, Start, End", "ARC CSE", ""),
                    Cmd("Center, Start, Angle", "ARC CSA", ""),
                    Cmd("Center, Start, Length", "ARC CSL", ""),
                    Sep,
                    Cmd("Continue", "ARC C", ""),
                ],
            ),
            Sub(
                "Circle",
                &[
                    Cmd("Center, Radius", "CIRCLE", ""),
                    Cmd("Center, Diameter", "CIRCLE D", ""),
                    Sep,
                    Cmd("2 Points", "CIRCLE 2P", ""),
                    Cmd("3 Points", "CIRCLE 3P", ""),
                    Sep,
                    Cmd("Tan, Tan, Radius", "CIRCLE TTR", ""),
                    Cmd("Tan, Tan, Tan", "CIRCLE TTT", ""),
                ],
            ),
            Cmd("Donut", "DONUT", ""),
            Sub(
                "Spline",
                &[
                    Cmd("Fit Points", "SPLINE F", ""),
                    Cmd("Control Vertices", "SPLINE CV", ""),
                ],
            ),
            Sub(
                "Ellipse",
                &[
                    Cmd("Center", "ELLIPSE C", ""),
                    Cmd("Axis, End", "ELLIPSE", ""),
                    Cmd("Arc", "ELLIPSE A", ""),
                ],
            ),
            Sub(
                "Block",
                &[
                    Cmd("Make...", "BLOCK", ""),
                    Cmd("Write Block...", "WBLOCK", ""),
                    Cmd("Base", "BASE", ""),
                    Sep,
                    Cmd("Define Attributes...", "ATTDEF", ""),
                ],
            ),
            Cmd("Table...", "TABLE", ""),
            Sub(
                "Point",
                &[
                    Cmd("Single Point", "POINT", ""),
                    Cmd("Multiple Point", "POINT M", ""),
                    Sep,
                    Cmd("Divide", "DIVIDE", ""),
                    Cmd("Measure", "MEASURE", ""),
                ],
            ),
            Sep,
            Cmd("Hatch...", "HATCH", ""),
            Cmd("Gradient...", "GRADIENT", ""),
            Cmd("Boundary...", "BOUNDARY", ""),
            Cmd("Region", "REGION", ""),
            Cmd("Wipeout", "WIPEOUT", ""),
            Cmd("Revision Cloud", "REVCLOUD", ""),
            Sep,
            Sub(
                "Text",
                &[
                    Cmd("Multiline Text...", "MTEXT", ""),
                    Cmd("Single Line Text", "TEXT", ""),
                ],
            ),
        ],
    ),
    (
        "Dimension",
        &[
            Cmd("Quick Dimension", "QDIM", ""),
            Cmd("Dimension", "DIM", ""),
            Sep,
            Cmd("Linear", "DIMLINEAR", ""),
            Cmd("Aligned", "DIMALIGNED", ""),
            Cmd("Arc Length", "DIMARC", ""),
            Cmd("Ordinate", "DIMORDINATE", ""),
            Sep,
            Cmd("Radius", "DIMRADIUS", ""),
            Cmd("Jogged", "DIMJOGGED", ""),
            Cmd("Diameter", "DIMDIAMETER", ""),
            Cmd("Angular", "DIMANGULAR", ""),
            Sep,
            Cmd("Baseline", "DIMBASELINE", ""),
            Cmd("Continue", "DIMCONTINUE", ""),
            Cmd("Dimension Space", "DIMSPACE", ""),
            Cmd("Dimension Break", "DIMBREAK", ""),
            Sep,
            Cmd("Multileader", "MLEADER", ""),
            Cmd("Tolerance...", "TOLERANCE", ""),
            Cmd("Center Mark", "DIMCENTER", ""),
            Cmd("Inspection", "DIMINSPECT", ""),
            Cmd("Jogged Linear", "DIMJOGLINE", ""),
            Sep,
            Cmd("Oblique", "DIMEDIT O", ""),
            Sub(
                "Align Text",
                &[
                    Cmd("Home", "DIMTEDIT H", ""),
                    Cmd("Angle", "DIMTEDIT A", ""),
                    Sep,
                    Cmd("Left", "DIMTEDIT L", ""),
                    Cmd("Center", "DIMTEDIT C", ""),
                    Cmd("Right", "DIMTEDIT R", ""),
                ],
            ),
            Sep,
            Cmd("Dimension Style...", "DIMSTYLE", ""),
            Cmd("Override", "DIMOVERRIDE", ""),
            Cmd("Update", "-DIMSTYLE A", ""),
            Cmd("Reassociate Dimensions", "DIMREASSOCIATE", ""),
        ],
    ),
    (
        "Modify",
        &[
            Cmd("Properties", "PROPERTIES", "Ctrl+1"),
            Cmd("Match Properties", "MATCHPROP", ""),
            Cmd("Change to ByLayer", "SETBYLAYER", ""),
            Sub(
                "Object",
                &[
                    Cmd("Hatch...", "HATCHEDIT", ""),
                    Cmd("Polyline", "PEDIT", ""),
                    Cmd("Spline", "SPLINEDIT", ""),
                    Cmd("Multiline...", "MLEDIT", ""),
                    Sep,
                    Cmd("Block Description...", "BLOCKDESC", ""),
                    Cmd("Attribute", "EATTEDIT", ""),
                    Sep,
                    Cmd("Text Edit...", "TEXTEDIT", ""),
                    Cmd("Text Scale", "SCALETEXT", ""),
                    Cmd("Text Justify", "JUSTIFYTEXT", ""),
                ],
            ),
            Sub(
                "Clip",
                &[
                    Cmd("Xref", "XCLIP", ""),
                    Cmd("Image", "IMAGECLIP", ""),
                    Cmd("Underlay", "CLIP", ""),
                    Cmd("Viewport", "VPCLIP", ""),
                ],
            ),
            Sep,
            Cmd("Erase", "ERASE", ""),
            Cmd("Copy", "COPY", ""),
            Cmd("Mirror", "MIRROR", ""),
            Cmd("Offset", "OFFSET", ""),
            Sub(
                "Array",
                &[
                    Cmd("Rectangular Array", "ARRAYRECT", ""),
                    Cmd("Path Array", "ARRAYPATH", ""),
                    Cmd("Polar Array", "ARRAYPOLAR", ""),
                ],
            ),
            Cmd("Delete Duplicate Objects", "OVERKILL", ""),
            Sep,
            Cmd("Move", "MOVE", ""),
            Cmd("Rotate", "ROTATE", ""),
            Cmd("Scale", "SCALE", ""),
            Cmd("Stretch", "STRETCH", ""),
            Cmd("Lengthen", "LENGTHEN", ""),
            Sep,
            Cmd("Trim", "TRIM", ""),
            Cmd("Extend", "EXTEND", ""),
            Cmd("Break", "BREAK", ""),
            Cmd("Join", "JOIN", ""),
            Cmd("Chamfer", "CHAMFER", ""),
            Cmd("Fillet", "FILLET", ""),
            Sep,
            Cmd("Change Space", "CHSPACE", ""),
            Cmd("Explode", "EXPLODE", ""),
        ],
    ),
    (
        "Parametric",
        &[
            Cmd("AutoConstrain", "AUTOCONSTRAIN", ""),
            Sub(
                "Geometric Constraints",
                &[
                    Cmd("Coincident", "GCCOINCIDENT", ""),
                    Cmd("Collinear", "GCCOLLINEAR", ""),
                    Cmd("Concentric", "GCCONCENTRIC", ""),
                    Cmd("Fix", "GCFIX", ""),
                    Cmd("Parallel", "GCPARALLEL", ""),
                    Cmd("Perpendicular", "GCPERPENDICULAR", ""),
                    Cmd("Horizontal", "GCHORIZONTAL", ""),
                    Cmd("Vertical", "GCVERTICAL", ""),
                    Cmd("Tangent", "GCTANGENT", ""),
                    Cmd("Smooth (G2)", "GCSMOOTH", ""),
                    Cmd("Symmetric", "GCSYMMETRIC", ""),
                    Cmd("Equal", "GCEQUAL", ""),
                ],
            ),
            Sub(
                "Constraint Bars",
                &[
                    Cmd("Select Objects", "CONSTRAINTBAR", ""),
                    Cmd("Show All", "CONSTRAINTBAR S", ""),
                    Cmd("Hide All", "CONSTRAINTBAR H", ""),
                ],
            ),
            Sub(
                "Dimensional Constraints",
                &[
                    Cmd("Aligned", "DCALIGNED", ""),
                    Cmd("Horizontal", "DCHORIZONTAL", ""),
                    Cmd("Vertical", "DCVERTICAL", ""),
                    Cmd("Angular", "DCANGULAR", ""),
                    Cmd("Radius", "DCRADIUS", ""),
                    Cmd("Diameter", "DCDIAMETER", ""),
                    Sep,
                    Cmd("Convert", "DCCONVERT", ""),
                ],
            ),
            Sub(
                "Dynamic Dimensions",
                &[
                    Cmd("Select Objects", "DCDISPLAY", ""),
                    Cmd("Show All", "DCDISPLAY S", ""),
                    Cmd("Hide All", "DCDISPLAY H", ""),
                ],
            ),
            Sep,
            Cmd("Delete Constraints", "DELCONSTRAINT", ""),
            Cmd("Constraint Settings...", "CONSTRAINTSETTINGS", ""),
            Cmd("Parameters Manager", "PARAMETERS", ""),
        ],
    ),
    (
        "Window",
        &[
            Cmd("Close", "CLOSE", ""),
            Cmd("Close All", "CLOSEALL", ""),
            Sep,
            Cmd("Cascade", "SYSWINDOWS C", ""),
            Cmd("Tile Horizontally", "SYSWINDOWS H", ""),
            Cmd("Tile Vertically", "SYSWINDOWS V", ""),
            Cmd("Arrange Icons", "SYSWINDOWS A", ""),
        ],
    ),
    (
        "Help",
        &[
            Cmd("Help", "HELP", "F1"),
            Cmd("Keyboard Shortcuts", "SHORTCUTS", ""),
            Cmd("Send Feedback", "FEEDBACK", ""),
            Sep,
            Cmd("About Open CAD Studio", "ABOUT", ""),
        ],
    ),
];

const ENTRY_H: f32 = 22.0;
const ICON: f32 = crate::ui::style::ribbon_metrics::SMALL_ICON;
/// A row's label and the shortcut and aliases at its end.
const LABEL_SIZE: f32 = 12.0;
const TAIL_SIZE: f32 = 11.0;
/// Gap between a row's parts, its side padding, the least gap before the
/// shortcut, and the submenu arrow.
const ROW_SPACING: f32 = 8.0;
const ROW_PAD: f32 = 8.0;
const TAIL_GAP: f32 = 8.0;
const ARROW: f32 = 8.0;
/// The menu's own padding.
const MENU_PAD: f32 = 2.0;
/// The menus the Start tab keeps: it has no drawing for the others.
const START_MENUS: [&str; 3] = ["File", "Window", "Help"];

/// What the menu bar shows besides its fixed menus.
pub struct MenuContext<'a> {
    /// The open drawings the Window menu lists: tab index and name.
    pub drawings: Vec<(usize, String)>,
    /// The command aliases (alias → command), shown after the shortcuts.
    pub aliases: &'a rustc_hash::FxHashMap<String, String>,
    /// The Start tab is on show: only File, Window and Help.
    pub is_start: bool,
}

/// The menu bar, one pull-down per menu.
pub fn view<'a>(context: MenuContext<'_>) -> Element<'a, Message> {
    let aliases = aliases_by_command(context.aliases);
    let roots =
        MENUS
            .iter()
            .filter(|(title, _)| !context.is_start || START_MENUS.contains(title))
            .map(|&(title, entries)| {
                let mut rows = rows(entries, &aliases);
                if title == "Window" && !context.drawings.is_empty() {
                    rows.push(Row::Sep);
                    rows.extend(context.drawings.iter().enumerate().map(|(n, (tab, name))| {
                        Row::Cmd {
                            icon: None,
                            label: format!("{} {name}", n + 1),
                            tail: String::new(),
                            on_press: Some(Box::new(Message::TabSwitch(*tab))),
                        }
                    }));
                }
                Item::with_menu(
                    Title::new(
                        container(text(crate::t!(title).into_owned()).size(12))
                            .center_y(TITLE_H)
                            .padding([0, 9]),
                    ),
                    menu(rows),
                )
            })
            .collect();
    let bar = MenuBar::new(roots)
        .close_on_item_click_global(true)
        // The open menu's title stays lit while the pointer is in the menu.
        .draw_path(DrawPath::Backdrop)
        .style(|theme: &Theme, _| iced_aw::style::menu_bar::Style {
            bar_background: Background::Color(chrome::bar(theme)),
            bar_border: Border::default(),
            bar_shadow: Default::default(),
            menu_background: Background::Color(chrome::panel(theme)),
            menu_border: Border {
                color: chrome::border(theme),
                width: 1.0,
                radius: 0.0.into(),
            },
            menu_shadow: Default::default(),
            path: Background::Color(chrome::hover(theme)),
            path_border: Border::default(),
        });
    container(bar)
        .padding(iced::Padding {
            top: 1.0,
            ..iced::Padding::ZERO
        })
        .height(ROW_H)
        .width(Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(chrome::bar(theme))),
            ..Default::default()
        })
        .into()
}

/// The aliases of each command, sorted, from the alias → command table.
fn aliases_by_command(
    aliases: &rustc_hash::FxHashMap<String, String>,
) -> rustc_hash::FxHashMap<String, Vec<String>> {
    let mut by_command: rustc_hash::FxHashMap<String, Vec<String>> = Default::default();
    for (alias, command) in aliases {
        by_command
            .entry(command.to_uppercase())
            .or_default()
            .push(alias.to_uppercase());
    }
    for list in by_command.values_mut() {
        list.sort_unstable_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
    }
    by_command
}

/// Whether OCS has the command a menu line starts.
fn implemented(line: &str) -> bool {
    thread_local! {
        static KNOWN: rustc_hash::FxHashSet<&'static str> =
            crate::command::all_registered_command_names().into_iter().collect();
    }
    let Some(verb) = line.split_whitespace().next() else {
        return false;
    };
    let verb = verb.to_uppercase();
    KNOWN.with(|known| known.contains(verb.as_str()))
}

/// A menu item as the command search lists it.
pub struct SearchEntry {
    pub label: String,
    /// The menu and submenus it is in, e.g. `File › Drawing Utilities`.
    pub path: String,
    /// Its command line.
    pub line: &'static str,
    pub icon: Option<&'static [u8]>,
}

/// Every menu item that runs a command, in menu order.
pub fn search_entries() -> Vec<SearchEntry> {
    fn walk(entries: &'static [Entry], path: &str, out: &mut Vec<SearchEntry>) {
        for entry_def in entries {
            match *entry_def {
                Cmd(label, line, _) => out.push(SearchEntry {
                    label: crate::t!(label).into_owned(),
                    path: path.to_string(),
                    line,
                    icon: command_icon(line),
                }),
                Sep => {}
                Sub(label, entries) => {
                    walk(entries, &format!("{path} › {}", crate::t!(label)), out);
                }
            }
        }
    }
    let mut out = Vec::new();
    for &(title, entries) in MENUS {
        walk(entries, &crate::t!(title), &mut out);
    }
    out
}

/// One row of a menu before it is drawn, so the menu can be as wide as its
/// widest row.
enum Row {
    /// A command: its icon, label, shortcut and aliases, and what a click
    /// sends (`None` while OCS does not have the command yet).
    Cmd {
        icon: Option<&'static [u8]>,
        label: String,
        tail: String,
        on_press: Option<Box<Message>>,
    },
    Sep,
    Sub(String, Vec<Row>),
}

impl Row {
    /// The width the row needs.
    fn width(&self) -> f32 {
        use crate::ui::style::text_metrics::width;
        let (label, end) = match self {
            Row::Cmd { label, tail, .. } if tail.is_empty() => (label, 0.0),
            Row::Cmd { label, tail, .. } => (label, TAIL_GAP + width(tail, TAIL_SIZE, false)),
            Row::Sub(label, _) => (label, TAIL_GAP + ARROW),
            Row::Sep => return 0.0,
        };
        // Icon, label, filler and tail with a gap between each.
        ROW_PAD + ICON + 3.0 * ROW_SPACING + width(label, LABEL_SIZE, false) + end + ROW_PAD
    }
}

/// A menu as wide as its widest row.
fn menu<'a>(rows: Vec<Row>) -> Menu<'a, Message, Theme, Renderer> {
    let width = rows.iter().map(Row::width).fold(0.0, f32::max) + 2.0 * MENU_PAD + 2.0;
    let items = rows
        .into_iter()
        .map(|row| match row {
            Row::Cmd {
                icon,
                label,
                tail,
                on_press,
            } => Item::new(entry(icon, label, tail, on_press.map(|m| *m), false)),
            Row::Sep => Item::new(idle(separator())),
            Row::Sub(label, rows) => {
                Item::with_menu(entry(None, label, String::new(), None, true), menu(rows))
            }
        })
        .collect();
    Menu::new(items).width(width).padding(MENU_PAD).offset(0.0)
}

fn rows(
    entries: &'static [Entry],
    aliases: &rustc_hash::FxHashMap<String, Vec<String>>,
) -> Vec<Row> {
    entries
        .iter()
        .map(|entry_def| match *entry_def {
            Cmd(label, command, shortcut) => {
                // A command's aliases follow its shortcut; not those of a
                // line that answers prompts too.
                let own = (!command.contains(' '))
                    .then(|| aliases.get(command))
                    .flatten()
                    .map(|list| list.join(", "))
                    .unwrap_or_default();
                let tail = [shortcut, own.as_str()]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join("   ");
                Row::Cmd {
                    icon: command_icon(command),
                    label: crate::t!(label).into_owned(),
                    tail,
                    on_press: implemented(command)
                        .then(|| Box::new(Message::MenuCommand(command.to_string()))),
                }
            }
            Sep => Row::Sep,
            Sub(label, entries) => Row::Sub(crate::t!(label).into_owned(), rows(entries, aliases)),
        })
        .collect()
}

/// The ribbon icon of the command a menu line starts.
fn command_icon(line: &str) -> Option<&'static [u8]> {
    crate::modules::registry::command_icon(line.split_whitespace().next()?)
}

/// `content` with the plain pointer: a row that takes no click would let the
/// drawing's crosshair cursor show through the menu.
fn idle<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    mouse_area(content)
        .interaction(mouse::Interaction::Idle)
        .into()
}

/// A 22 px menu row: a 16 px icon column, the label, and the shortcut and
/// aliases right-aligned in muted text; a `submenu`'s row has an arrow
/// instead. Without `on_press` a command row is greyed out: OCS does not
/// have that command yet.
fn entry<'a>(
    icon: Option<&'static [u8]>,
    label: String,
    tail: String,
    on_press: Option<Message>,
    submenu: bool,
) -> Element<'a, Message> {
    let enabled = on_press.is_some() || submenu;
    let icon: Element<'a, Message> = match icon {
        Some(bytes) if enabled => crate::ui::icons::tool(bytes, ICON),
        Some(bytes) => crate::ui::icons::tool_disabled(bytes, ICON),
        None => Space::new().width(ICON).height(ICON).into(),
    };
    let tail: Element<'a, Message> = if submenu {
        crate::ui::icons::themed_arrow_right(ARROW)
    } else {
        text(tail)
            .size(TAIL_SIZE)
            .style(|theme: &Theme| text::Style {
                color: Some(chrome::muted(theme)),
            })
            .into()
    };
    let row = button(
        row![
            icon,
            text(label).size(LABEL_SIZE),
            Space::new().width(Fill),
            tail
        ]
        .spacing(ROW_SPACING)
        .align_y(iced::Center),
    )
    .style(move |theme: &Theme, status| entry_style(theme, status, submenu))
    .width(Fill)
    .height(ENTRY_H)
    .padding([0.0, ROW_PAD]);
    match on_press {
        Some(message) => row.on_press(message).into(),
        None => idle(row.into()),
    }
}

/// Rows light under the pointer; a submenu's row, which has no press of its
/// own, draws like an enabled one (the open submenu's row is lit by the bar's
/// path fill).
fn entry_style(theme: &Theme, status: button::Status, submenu: bool) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: hovered.then(|| Background::Color(chrome::hover(theme))),
        text_color: if hovered {
            chrome::strong(theme)
        } else if submenu || status != button::Status::Disabled {
            chrome::text(theme)
        } else {
            chrome::muted(theme)
        },
        ..Default::default()
    }
}

/// A 1 px line with 2 px above and below and 4 px at the sides.
fn separator<'a>() -> Element<'a, Message> {
    container(
        container(Space::new().width(Fill).height(1)).style(|theme: &Theme| container::Style {
            background: Some(Background::Color(chrome::border(theme))),
            ..Default::default()
        }),
    )
    .padding([2, 4])
    .width(Fill)
    .into()
}

/// A menu title on the bar: `tab_text`, and `strong` on a `hover` fill
/// under the pointer.
struct Title<'a> {
    content: Element<'a, Message>,
}

impl<'a> Title<'a> {
    fn new(content: impl Into<Element<'a, Message>>) -> Self {
        Self {
            content: content.into(),
        }
    }
}

impl<'a> Widget<Message, Theme, Renderer> for Title<'a> {
    /// Whether the pointer is on the title.
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<bool>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(false)
    }

    fn diff(&mut self, tree: &mut widget::Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Event::Mouse(mouse::Event::CursorMoved { .. }) = event {
            // The fill follows the pointer onto and off the title.
            let hovered = cursor.is_over(layout.bounds());
            let state = tree.state.downcast_mut::<bool>();
            if *state != hovered {
                *state = hovered;
                shell.request_redraw();
            }
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let hovered = cursor.is_over(layout.bounds());
        if hovered {
            renderer::Renderer::fill_quad(
                renderer,
                renderer::Quad {
                    bounds: layout.bounds(),
                    border: Border::default(),
                    shadow: Shadow::default(),
                    snap: true,
                },
                Background::Color(chrome::hover(theme)),
            );
        }
        let style = renderer::Style {
            text_color: if hovered {
                chrome::strong(theme)
            } else {
                chrome::tab_text(theme)
            },
        };
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            &style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a> From<Title<'a>> for Element<'a, Message> {
    fn from(title: Title<'a>) -> Self {
        Element::new(title)
    }
}

#[cfg(test)]
mod tests {
    use super::{Entry, MENUS};

    fn commands(entries: &'static [Entry], out: &mut Vec<&'static str>) {
        for entry in entries {
            match *entry {
                Entry::Cmd(_, line, _) => out.push(line.split_whitespace().next().unwrap()),
                Entry::Sep => {}
                Entry::Sub(_, entries) => commands(entries, out),
            }
        }
    }

    /// Lists the menu commands OCS's command table does not know: a to-do
    /// list, not a failure. Running one reports it on the command line.
    #[test]
    fn menu_commands_ocs_does_not_have_yet() {
        let known = crate::command::all_registered_command_names();
        let mut all = Vec::new();
        for (_, entries) in MENUS {
            commands(entries, &mut all);
        }
        let mut missing: Vec<_> = all
            .into_iter()
            .filter(|command| !known.iter().any(|k| k.eq_ignore_ascii_case(command)))
            .collect();
        missing.sort_unstable();
        missing.dedup();
        println!(
            "Menu commands OCS does not have yet ({}):\n{}",
            missing.len(),
            missing.join("\n")
        );
    }

    #[test]
    fn the_menu_tree_has_every_menu_of_the_design() {
        let titles: Vec<_> = MENUS.iter().map(|(title, _)| *title).collect();
        assert_eq!(
            titles,
            [
                "File",
                "Edit",
                "View",
                "Insert",
                "Format",
                "Tools",
                "Draw",
                "Dimension",
                "Modify",
                "Parametric",
                "Window",
                "Help"
            ]
        );
    }
}
