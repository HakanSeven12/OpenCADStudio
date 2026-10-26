//! Classic pull-down menus above the ribbon (MENUBAR 1). Every entry runs a
//! command, so the menus stay a view of the command set rather than a second
//! implementation of it.

use crate::app::Message;
use crate::ui::style::chrome;
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{mouse, overlay, renderer, Shell};
use iced::widget::{button, container, row, text, Space};
use iced::{
    Background, Border, Element, Event, Fill, Length, Rectangle, Renderer, Shadow, Size, Theme,
    Vector,
};
use iced_aw::menu::{Item, Menu, MenuBar};

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
            Cmd("Clean Screen", "CLEANSCREENON", "Ctrl+0"),
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
            Cmd("Clean Screen", "CLEANSCREENON", "Ctrl+0"),
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

/// Width of a pull-down menu.
const MENU_W: f32 = 290.0;
const ENTRY_H: f32 = 22.0;
const ICON: f32 = 16.0;

/// The menu bar, one pull-down per menu. `drawings` are the open drawings
/// the Window menu lists: tab index and name.
pub fn view<'a>(drawings: Vec<(usize, String)>) -> Element<'a, Message> {
    let roots = MENUS
        .iter()
        .map(|&(title, entries)| {
            let mut items = items(entries);
            if title == "Window" && !drawings.is_empty() {
                items.push(Item::new(separator()));
                items.extend(drawings.iter().enumerate().map(|(n, (tab, name))| {
                    Item::new(entry(
                        None,
                        format!("{} {name}", n + 1),
                        "",
                        Some(Message::TabSwitch(*tab)),
                    ))
                }));
            }
            Item::with_menu(
                Title::new(
                    container(text(crate::t!(title).into_owned()).size(12))
                        .center_y(TITLE_H)
                        .padding([0, 9]),
                ),
                menu(items),
            )
        })
        .collect();
    let bar = MenuBar::new(roots)
        .close_on_item_click_global(true)
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
        })
        .width(Fill);
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

fn menu<'a>(items: Vec<Item<'a, Message, Theme, Renderer>>) -> Menu<'a, Message, Theme, Renderer> {
    Menu::new(items).width(MENU_W).padding(2).offset(0.0)
}

fn items<'a>(entries: &'static [Entry]) -> Vec<Item<'a, Message, Theme, Renderer>> {
    entries
        .iter()
        .map(|entry_def| match *entry_def {
            Cmd(label, command, shortcut) => Item::new(entry(
                command_icon(command),
                crate::t!(label).into_owned(),
                shortcut,
                Some(Message::MenuCommand(command.to_string())),
            )),
            Sep => Item::new(separator()),
            Sub(label, entries) => Item::with_menu(
                entry(None, crate::t!(label).into_owned(), "", None),
                menu(items(entries)),
            ),
        })
        .collect()
}

/// The ribbon icon of the command a menu line starts.
fn command_icon(line: &str) -> Option<&'static [u8]> {
    crate::modules::registry::command_icon(line.split_whitespace().next()?)
}

/// A 22 px menu row: a 16 px icon column, the label, and the shortcut
/// right-aligned in muted text; or, without `on_press`, a submenu's row
/// with an arrow.
fn entry<'a>(
    icon: Option<&'static [u8]>,
    label: String,
    shortcut: &'static str,
    on_press: Option<Message>,
) -> Element<'a, Message> {
    let icon: Element<'a, Message> = match icon {
        Some(bytes) => crate::ui::icons::semantic(bytes, ICON),
        None => Space::new().width(ICON).height(ICON).into(),
    };
    let tail: Element<'a, Message> = if on_press.is_some() {
        text(shortcut)
            .size(11)
            .style(|theme: &Theme| text::Style {
                color: Some(chrome::muted(theme)),
            })
            .into()
    } else {
        crate::ui::icons::themed_arrow_right(8.0)
    };
    let submenu = on_press.is_none();
    let row = button(
        row![icon, text(label).size(12), Space::new().width(Fill), tail]
            .spacing(8)
            .align_y(iced::Center),
    )
    .style(move |theme: &Theme, status| entry_style(theme, status, submenu))
    .width(Fill)
    .height(ENTRY_H)
    .padding([0, 8]);
    match on_press {
        Some(message) => row.on_press(message).into(),
        None => row.into(),
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
