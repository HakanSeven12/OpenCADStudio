// Core ribbon module registry — one boxed instance of every built-in CAD
// module, in ribbon-tab display order. External add-ons are appended on top
// of this list at runtime by `plugin::registry`.
//
// To add a core module:
//   1. Create src/modules/my_name/mod.rs  (implement CadModule as MyNameModule)
//   2. Add `pub mod my_name;` to src/modules/mod.rs
//   3. Add one `Box::new(super::my_name::MyNameModule)` line below, in the
//      position you want its tab to appear.
use crate::modules::{CadModule, IconKind, ModuleEvent, RibbonItem, ToolDef};
use std::collections::HashMap;
use std::sync::OnceLock;

/// Returns one boxed instance of every registered core CAD module.
/// Called once at startup by `Ribbon::new()`.
pub fn all_modules() -> Vec<Box<dyn CadModule>> {
    vec![
        Box::new(super::draw::DrawModule),
        Box::new(super::parametric::ParametricModule),
        Box::new(super::model::ModelModule),
        Box::new(super::insert::InsertModule),
        Box::new(super::annotate::AnnotateModule),
        Box::new(super::view::ViewModule),
        Box::new(super::manage::ManageModule),
        Box::new(super::layout::LayoutModule),
        Box::new(super::block_editor::BlockEditorModule),
        Box::new(super::table_cell::TableCellModule),
    ]
}

/// Synonym command names, sub-command variants, and `-` command-line forms
/// mapped to the canonical command whose ribbon icon they share.
const COMMAND_ICON_ALIASES: &[(&str, &str)] = &[
    // Draw & shapes
    ("RECTANG", "RECT"),
    ("RECTANGLE", "RECT"),
    ("POLYGON", "POLY"),
    ("REVCLOUD_RECTANGULAR", "REVCLOUD"),
    ("REVCLOUD_POLYGONAL", "REVCLOUD"),
    ("REVCLOUD_FREEHAND", "REVCLOUD"),
    ("CONSTRUCTIONLINE", "XLINE"),
    ("SPLINEFIT", "SPLINE"),
    ("FITSPLINE", "SPLINE"),
    ("SOLID", "SOLID2D"),
    ("SO", "SOLID2D"),
    ("DOUGHNUT", "DONUT"),
    ("BHATCH", "HATCH"),
    ("-HATCH", "HATCH"),
    ("-BHATCH", "HATCH"),
    ("BOUNDARY", "BPOLY"),
    ("-BOUNDARY", "BPOLY"),
    ("IMAGE", "IMAGEATTACH"),
    ("-IMAGE", "IMAGEATTACH"),
    ("BMPOUT", "IMAGEATTACH"),
    // Modify & clipboard
    ("PASTE", "PASTECLIP"),
    ("ARRAY", "ARRAYRECT"),
    ("-ARRAY", "ARRAYRECT"),
    ("DRAWORDER", "DRAWORDER_FRONT"),
    ("3DMOVE", "MOVE"),
    ("3DROTATE", "ROTATE"),
    ("ROTATE3D", "ROTATE"),
    ("3DMIRROR", "MIRROR"),
    ("MIRROR3D", "MIRROR"),
    ("3DALIGN", "ALIGN"),
    ("ALIGN3D", "ALIGN"),
    ("3DSCALE", "SCALE"),
    ("SCALE3D", "SCALE"),
    ("-OVERKILL", "OVERKILL"),
    ("-SETBYLAYER", "SETBYLAYER"),
    ("-HATCHEDIT", "HATCHEDIT"),
    // 3D modeling & solids
    ("SOLIDFILLET", "FILLETEDGE"),
    ("SOLIDCHAMFER", "CHAMFEREDGE"),
    ("PYR", "PYRAMID"),
    ("SL", "SLICE"),
    ("SECTION", "SECTIONPLANE"),
    ("SECTIONCULL", "SECTIONPLANE"),
    ("FLATTEN", "FLATSHOT"),
    ("INF", "INTERFERE"),
    ("CONVTOSURFACE", "THICKEN"),
    ("REG", "REGION"),
    // Layers & properties
    ("LAYER", "LAYERS"),
    ("-LAYER", "LAYERS"),
    ("LAYMCH", "LAYMATCH"),
    ("LAYDEL", "LAYERS"),
    ("LAYMRG", "LAYERS"),
    ("LAYTRANS", "LAYERS"),
    ("LAYERSTATE", "LAYERS"),
    ("LAS", "LAYERS"),
    ("LMAN", "LAYERS"),
    ("COLOUR", "COLOR"),
    ("DDCOLOR", "COLOR"),
    ("LINETYPE", "BYLAYER"),
    ("-LINETYPE", "BYLAYER"),
    ("DDLTYPE", "BYLAYER"),
    ("CHPROP", "PROPERTIES"),
    ("CHANGE", "PROPERTIES"),
    ("PROPS", "PROPERTIES"),
    ("PROPERTIESCLOSE", "PROPERTIES"),
    // Blocks, dynamic block authoring, references, underlays & point clouds
    ("-BLOCK", "BLOCK"),
    ("BMAKE", "BLOCK"),
    ("-BMAKE", "BLOCK"),
    ("-WBLOCK", "WBLOCK"),
    ("WB", "WBLOCK"),
    ("-WB", "WBLOCK"),
    ("-INSERT", "INSERT"),
    ("BLOCKPALETTE", "INSERT"),
    ("BLOCKSPALETTE", "INSERT"),
    ("BLOCKSPALETTECLOSE", "INSERT"),
    ("-ATTDEF", "ATTDEF"),
    ("-ATTEDIT", "ATTEDIT"),
    ("EATTEDIT", "ATTEDIT"),
    ("BATTMAN", "ATTMAN"),
    ("ATTREDEF", "ATTMAN"),
    ("ATTEXT", "DATAEXTRACTION"),
    ("EATTEXT", "DATAEXTRACTION"),
    ("-BEDIT", "BEDIT"),
    ("BPARAMETER", "PARAMETERS"),
    ("PARAMETERSCLOSE", "PARAMETERS"),
    ("BACTIONTOOL", "BACTION"),
    ("BARRAY", "BACTION"),
    ("BLOOKUPTABLE", "TABLE"),
    ("BTABLE", "TABLE"),
    ("BPROPERTIESTABLE", "TABLE"),
    ("BCLOSETESTBLOCK", "BCLOSE"),
    ("BAUTHORPALETTECLOSE", "BAUTHORPALETTE"),
    ("-REFEDIT", "REFEDIT"),
    ("-HYPERLINK", "HYPERLINK"),
    ("XCLIP", "CLIP"),
    ("PDFCLIP", "CLIP"),
    ("DWFCLIP", "CLIP"),
    ("DGNCLIP", "CLIP"),
    ("IMAGECLIP", "CLIP"),
    ("VPCLIP", "CLIP"),
    ("IMAGEADJUST", "ADJUST"),
    ("-IMAGEADJUST", "ADJUST"),
    ("PDFADJUST", "ADJUST"),
    ("DWFADJUST", "ADJUST"),
    ("DGNADJUST", "ADJUST"),
    ("-PDFIMPORT", "PDFIMPORT"),
    ("-PDFATTACH", "PDFATTACH"),
    ("DWFATTACH", "PDFATTACH"),
    ("-DWFATTACH", "PDFATTACH"),
    ("DGNATTACH", "PDFATTACH"),
    ("-DGNATTACH", "PDFATTACH"),
    ("UNDERLAY", "PDFATTACH"),
    ("-POINTCLOUDATTACH", "POINTCLOUDATTACH"),
    ("POINTCLOUDMANAGER", "POINTCLOUDATTACH"),
    ("-POINTCLOUDMANAGER", "POINTCLOUDATTACH"),
    ("POINTCLOUDMANAGERCLOSE", "POINTCLOUDATTACH"),
    ("XREF", "XATTACH"),
    ("-XREF", "XATTACH"),
    ("ATTACH", "XATTACH"),
    ("-ATTACH", "XATTACH"),
    ("EXTERNALREFERENCES", "XATTACH"),
    ("EXTERNALREFERENCESCLOSE", "XATTACH"),
    ("XOPEN", "OPEN"),
    ("IMAGEEMBED", "IMAGEATTACH"),
    ("ADCCLOSE", "ADCENTER"),
    // Annotation, dimensions, text, leaders & tables
    ("DTEXT", "TEXT"),
    ("-MTEXT", "MTEXT"),
    ("TEXTEDIT", "DDEDIT"),
    ("TEDIT", "DDEDIT"),
    ("MTEDIT", "DDEDIT"),
    ("TXT2MTXT", "MTEXT"),
    ("TCASE", "TEXT"),
    ("TEXTFIT", "TEXT"),
    ("SCALETEXT", "TEXT"),
    ("JUSTIFYTEXT", "TEXT"),
    ("ARCTEXT", "TEXT"),
    ("TCOUNT", "TEXT"),
    ("SPELL", "TEXT"),
    ("FIELD", "TEXT"),
    ("UPDATEFIELD", "TEXT"),
    ("TEXTMASK", "WIPEOUT"),
    ("TEXTUNMASK", "WIPEOUT"),
    ("TEXTTOFRONT", "DRAWORDER_FRONT"),
    ("TEXTTOBACK", "DRAWORDER_BACK"),
    ("HATCHTOBACK", "DRAWORDER_BACK"),
    ("HB", "DRAWORDER_BACK"),
    ("-STYLE", "STYLE"),
    ("DDSTYLE", "STYLE"),
    ("STYLESMANAGER", "STYLE"),
    ("MLSTYLE", "MLEADERSTYLE"),
    ("-TABLE", "TABLE"),
    ("-TABLESTYLE", "TABLESTYLE"),
    ("QLEADER", "LEADER"),
    ("QL", "LEADER"),
    ("MLEADEREDIT", "MLEADERADD"),
    ("AIMLEADEREDITADD", "MLEADERADD"),
    ("AIMLEADEREDITREMOVE", "MLEADERREMOVE"),
    ("DIMHORIZONTAL", "DIMLINEAR"),
    ("DIMVERTICAL", "DIMLINEAR"),
    ("DIMROTATED", "DIMLINEAR"),
    ("DIMTED", "DIMTEDIT"),
    ("DSPACE", "DIMSPACE"),
    ("DIMJOG", "DIMJOGGED"),
    ("DJO", "DIMJOGGED"),
    ("JOGLINE", "DIMJOGLINE"),
    ("-DIMINSPECT", "DIMINSPECT"),
    ("DIMREASSOCIATE", "DIMEDIT"),
    ("DRE", "DIMEDIT"),
    ("DCE", "DIMCENTER"),
    ("CENTERMARK", "DIMCENTER"),
    ("CENTERRESET", "CENTERLINE"),
    ("CENTERREASSOCIATE", "CENTERLINE"),
    ("CENTERDISASSOCIATE", "CENTERLINE"),
    ("DDIM", "DIMSTYLE"),
    ("-DIMSTYLE", "DIMSTYLE"),
    ("COUNT", "COUNTLIST"),
    ("COUNTPALETTE", "COUNTLIST"),
    ("COUNTCLOSE", "COUNTLIST"),
    ("-SCALELISTEDIT", "SCALELISTEDIT"),
    ("ANNOUPDATE", "SCALELISTEDIT"),
    ("ANNORESET", "SCALELISTEDIT"),
    ("ANNOSCALE", "SCALELISTEDIT"),
    ("OBJECTSCALE", "SCALELISTEDIT"),
    ("-DATAEXTRACTION", "DATAEXTRACTION"),
    ("DE", "DATAEXTRACTION"),
    // Inquiry, selection & clipboard
    ("LI", "LIST"),
    ("DBLIST", "LIST"),
    ("STATUS", "LIST"),
    ("MASSPROP", "AREA"),
    ("CAL", "QUICKCALC"),
    ("SELECT", "SELECTALL"),
    ("PSELECT", "SELECTALL"),
    ("DESELECT", "SELECTALL"),
    ("DESELALL", "SELECTALL"),
    ("SELECTSIMILAR", "SELECTALL"),
    ("SELSIM", "SELECTALL"),
    ("ADDSELECTED", "SELECTALL"),
    ("FI", "FILTER"),
    ("COPYBASE", "COPYCLIP"),
    ("NCOPYALL", "NCOPY"),
    ("OOPS", "UNDO"),
    ("CLR", "CLEAR"),
    // Parametric constraints
    ("GCCOINCIDENT", "CCONSTRAINT"),
    ("GCHORIZONTAL", "HCONSTRAINT"),
    ("GCVERTICAL", "VCONSTRAINT"),
    ("GCPERPENDICULAR", "QCONSTRAINT"),
    ("GCPARALLEL", "PCONSTRAINT"),
    ("GCTANGENT", "TCONSTRAINT"),
    ("GCSMOOTH", "SCONSTRAINT"),
    ("GCCONCENTRIC", "CNCONSTRAINT"),
    ("GCCOLLINEAR", "LCONSTRAINT"),
    ("GCSYMMETRIC", "SYCONSTRAINT"),
    ("GCEQUAL", "ECONSTRAINT"),
    ("GCFIX", "FXCONSTRAINT"),
    ("DIMCONSTRAINT", "DCONSTRAINT"),
    ("DCALIGNED", "DCONSTRAINT"),
    ("DCHORIZONTAL", "DCONSTRAINT"),
    ("DCVERTICAL", "DCONSTRAINT"),
    ("DCLINEAR", "DCONSTRAINT"),
    ("DCRADIUS", "DCONSTRAINT"),
    ("DCDIAMETER", "DCONSTRAINT"),
    ("DCANGULAR", "ACONSTRAINT"),
    ("-PARAMETERS", "PARAMETERS"),
    ("-PAR", "PARAMETERS"),
    // View, layout, manage, import/export & file
    ("-VIEW", "VIEW"),
    ("PLAN", "VIEW_TOP"),
    ("-VPORTS", "VPORTS"),
    ("VPJOIN", "VPORTS"),
    ("SYNCPVIEWPORTS", "VPORTS"),
    ("VPSYNC", "VPORTS"),
    ("HORIZONTAL", "TILEHORIZ"),
    ("VERTICAL", "TILEVERT"),
    ("-PAN", "PAN"),
    ("ORBIT", "3DORBIT"),
    ("3DO", "3DORBIT"),
    ("3O", "3DORBIT"),
    ("ZS", "ZOOM"),
    ("HI", "HIDE"),
    ("VS", "VISUALSTYLES"),
    ("PERSP", "PERSPECTIVE"),
    ("-MVIEW", "MVIEW"),
    ("MSPACE", "MVIEW"),
    ("MS", "MVIEW"),
    ("PSPACE", "MVIEW"),
    ("PS", "MVIEW"),
    ("CHSPACE", "MVIEW"),
    ("MVSETUP", "MVIEW"),
    ("LAYOUTMANAGER", "LAYOUT"),
    ("LAYOUTPANEL", "LAYOUT"),
    ("CLEANSCREEN", "CLEANSCREENON"),
    ("DSETTINGS", "OSNAP"),
    ("ISODRAFT", "SNAP"),
    ("ISOPLANE", "SNAP"),
    ("PLOT", "PRINT"),
    ("-PLOT", "PRINT"),
    ("QUICKPRINT", "PRINT"),
    ("PRINTALL", "PRINT"),
    ("PUBLISH", "PRINT"),
    ("PRINTERS", "PRINT"),
    ("PLOTSTYLE", "PAGESETUP"),
    ("PLOTSTYLEEDITOR", "PAGESETUP"),
    ("PLOTSTYLEPANEL", "PAGESETUP"),
    ("PSETUPIN", "PAGESETUP"),
    ("-PURGE", "PURGE"),
    ("-PU", "PURGE"),
    ("PREFERENCES", "OPTIONS"),
    ("UNITS", "OPTIONS"),
    ("UN", "OPTIONS"),
    ("DDUNITS", "OPTIONS"),
    ("DWGUNITS", "OPTIONS"),
    ("CUI", "USERINTERFACE"),
    ("CUILOAD", "CUIIMPORT"),
    ("SHORTCUTS", "ALIASEDIT"),
    ("RIBBON", "USERINTERFACE"),
    ("RIBBONCLOSE", "USERINTERFACE"),
    ("TOOLPALETTESCLOSE", "TOOLPALETTES"),
    ("SHEETSETHIDE", "SHEETSET"),
    ("NEWSHEETSET", "SHEETSET"),
    ("OPENSHEETSET", "SHEETSET"),
    ("SAVEALL", "SAVE"),
    ("QS", "QSAVE"),
    ("SA", "SAVEAS"),
    ("EXPORTSTEP", "EXPORT"),
    ("STEPOUT", "EXPORT"),
    ("STPOUT", "EXPORT"),
    ("EXPORTSTL", "EXPORT"),
    ("STLOUT", "EXPORT"),
    ("FBXEXPORT", "EXPORT"),
    ("EXPORTSVG", "EXPORT"),
    ("ETRANSMIT", "EXPORT"),
    ("ARCHIVE", "EXPORT"),
    ("IMPORT", "IMPORTOBJ"),
    ("OBJIMPORT", "IMPORTOBJ"),
    ("ACISIN", "IMPORTOBJ"),
    ("WMFIN", "IMPORTOBJ"),
    ("SVGIN", "IMPORTOBJ"),
    ("REDRAWALL", "REDRAW"),
    ("REGEN", "REDRAW"),
    ("REGENALL", "REDRAW"),
    ("XRELOAD", "REDRAW"),
    ("DWGPROP", "DWGPROPS"),
    ("MREDO", "REDO"),
    ("AI_SELALL", "SELECTALL"),
];

/// Label and icon of every core ribbon command, keyed by its upper-case
/// command line (`LINE`, `ZOOM EXTENTS`). The ribbon definition beside each
/// command stays the one source; the shortcut list, the context menus and the
/// command-line suggestions read it from here.
pub fn ribbon_commands() -> &'static HashMap<String, (&'static str, IconKind)> {
    static COMMANDS: OnceLock<HashMap<String, (&'static str, IconKind)>> = OnceLock::new();
    COMMANDS.get_or_init(|| {
        let mut out = HashMap::new();
        let tool = |out: &mut HashMap<String, (&'static str, IconKind)>, t: &ToolDef| {
            if let ModuleEvent::Command(command) = &t.event {
                let upper = command.to_ascii_uppercase();
                if let Some(verb) = upper
                    .split([';', ' '])
                    .next()
                    .filter(|v| !v.is_empty() && *v != upper.as_str())
                {
                    out.entry(verb.to_string()).or_insert((t.label, t.icon));
                }
                out.insert(upper, (t.label, t.icon));
            }
            if !t.id.is_empty() {
                out.entry(t.id.to_ascii_uppercase())
                    .or_insert((t.label, t.icon));
            }
        };
        for module in all_modules() {
            for group in module.ribbon_groups() {
                for item in &group.tools {
                    match item {
                        RibbonItem::Tool(t)
                        | RibbonItem::LabeledTool(t)
                        | RibbonItem::LargeTool(t) => tool(&mut out, t),
                        RibbonItem::Dropdown {
                            id,
                            default,
                            icon,
                            items,
                            ..
                        }
                        | RibbonItem::LabeledDropdown {
                            id,
                            default,
                            icon,
                            items,
                            ..
                        }
                        | RibbonItem::LargeDropdown {
                            id,
                            default,
                            icon,
                            items,
                            ..
                        } => {
                            for (command, item_label, item_icon) in items {
                                let upper = command.to_ascii_uppercase();
                                if let Some(verb) = upper
                                    .split([';', ' '])
                                    .next()
                                    .filter(|v| !v.is_empty() && *v != upper.as_str())
                                {
                                    out.entry(verb.to_string())
                                        .or_insert((*item_label, *item_icon));
                                }
                                out.insert(upper, (*item_label, *item_icon));
                            }
                            if !id.is_empty() {
                                let default_label =
                                    items.first().map_or(*default, |(_, l, _)| *l);
                                out.entry(id.to_ascii_uppercase())
                                    .or_insert((default_label, *icon));
                            }
                        }
                        RibbonItem::ToolGrid { columns }
                        | RibbonItem::StyleComboGroup { rows: columns, .. } => {
                            for t in columns.iter().flatten() {
                                tool(&mut out, t);
                            }
                        }
                        RibbonItem::LayerComboGroup { row2, row3 } => {
                            for t in row2.iter().chain(row3) {
                                tool(&mut out, t);
                            }
                        }
                        RibbonItem::PropertiesGroup { match_prop } => tool(&mut out, match_prop),
                    }
                }
            }
        }
        // Standalone module tools not placed directly on a primary ribbon group.
        for standalone in [
            super::annotate::ddedit::tool(),
            super::annotate::dimtedit::tool(),
            super::annotate::leader_cmd::tool(),
            super::draw::changelog::tool(),
            super::draw::donate::tool(),
            super::draw::report::tool(),
            super::draw::draw::point::tool(),
            super::draw::draw::poly3d::tool(),
            super::draw::draw::sketch::tool(),
            super::draw::draw::solid2d::tool(),
            super::draw::draw::trace::tool(),
            super::draw::draw::wipeout::tool(),
            super::draw::inquiry::measuregeom::tool(),
            super::insert::content_browser::tool(),
            super::insert::design_center::tool(),
            super::insert::minsert::tool(),
            super::parametric::normal::tool(),
            super::parametric::equal_distance_tool::tool(),
            super::parametric::center_point_tool::tool(),
            super::parametric::midpoint_tool::tool(),
            super::parametric::point_on_curve_tool::tool(),
            super::parametric::angle_tool::tool(),
            super::parametric::distance_tool::tool(),
            super::view::orbit::tool(),
            super::view::ortho::tool(),
            super::view::persp::tool(),
            super::view::view_front::tool(),
            super::view::view_iso::tool(),
            super::view::view_right::tool(),
            super::view::view_top::tool(),
            super::view::vports_config::tool(),
            super::view::zoom_ext::tool(),
            super::view::zoom_in::tool(),
            super::view::zoom_out::tool(),
            super::view::zoom_window::tool(),
        ] {
            if let ModuleEvent::Command(command) = &standalone.event {
                out.entry(command.to_ascii_uppercase())
                    .or_insert((standalone.label, standalone.icon));
            }
            if !standalone.id.is_empty() {
                out.entry(standalone.id.to_ascii_uppercase())
                    .or_insert((standalone.label, standalone.icon));
            }
        }
        for (command, label, icon) in crate::ui::ribbon::panel_tools() {
            out.entry(command.to_string())
                .or_insert((label, IconKind::Svg(icon)));
        }
        // File, session, drafting-aid, snap-modifier, and dialog-launcher commands
        // with dedicated SVGs in `assets/icons/`.
        for (command, label, icon) in [
            // File, session & help
            ("UNDO", "Undo", crate::ui::icons::undo_icon()),
            ("REDO", "Redo", crate::ui::icons::redo_icon()),
            ("NEW", "New", crate::ui::icons::DOC_NEW),
            ("QNEW", "New", crate::ui::icons::DOC_NEW),
            ("OPEN", "Open", crate::ui::icons::FOLDER_OPEN),
            ("SAVE", "Save", crate::ui::icons::SAVE),
            ("QSAVE", "Save", crate::ui::icons::SAVE),
            ("SAVEAS", "Save As", crate::ui::icons::FILE_EXPORT),
            ("EXPORT", "Export", crate::ui::icons::FILE_EXPORT),
            ("EXPORTPDF", "Export PDF", crate::ui::icons::FILE_EXPORT),
            ("CLOSE", "Close", crate::ui::icons::CLOSE),
            ("CLOSEALL", "Close All", crate::ui::icons::CLOSE),
            ("QUIT", "Exit", crate::ui::icons::CLOSE),
            ("EXIT", "Exit", crate::ui::icons::CLOSE),
            (
                "HELP",
                "Help",
                include_bytes!("../../assets/icons/ui/help.svg") as &'static [u8],
            ),
            (
                "SELECTALL",
                "Select All",
                include_bytes!("../../assets/icons/blocks/select_objects.svg"),
            ),
            (
                "QSELECT",
                "Quick Select",
                include_bytes!("../../assets/icons/blocks/quick_select.svg"),
            ),
            // Status-bar & drafting aids
            ("OSNAP", "Object Snap", crate::ui::icons::ST_OSNAP),
            ("DDOSNAP", "Object Snap", crate::ui::icons::ST_OSNAP),
            ("OTRACK", "Object Snap Tracking", crate::ui::icons::ST_OTRACK),
            ("POLAR", "Polar Tracking", crate::ui::icons::ST_POLAR),
            ("GRID", "Grid", crate::ui::icons::GRID),
            ("SNAP", "Snap", crate::ui::icons::SNAP),
            ("LWEIGHT", "Lineweight", crate::ui::icons::ST_LWT),
            (
                "QUICKPROPERTIES",
                "Quick Properties",
                crate::ui::icons::ST_QUICKPROPS,
            ),
            ("QP", "Quick Properties", crate::ui::icons::ST_QUICKPROPS),
            ("FILTER", "Selection Filter", crate::ui::icons::ST_FILTER),
            (
                "CLEANSCREENON",
                "Clean Screen On",
                crate::ui::icons::ST_CLEANSCREEN,
            ),
            (
                "CLEANSCREENOFF",
                "Clean Screen Off",
                crate::ui::icons::ST_CLEANSCREEN,
            ),
            (
                "ISOLATEOBJECTS",
                "Isolate Objects",
                crate::ui::icons::ST_ISOLATE,
            ),
            ("HIDEOBJECTS", "Hide Objects", crate::ui::icons::ST_ISOLATE),
            (
                "UNISOLATEOBJECTS",
                "End Object Isolation",
                crate::ui::icons::ST_ISOLATE,
            ),
            // Snap modifiers, calculator, styles & mesh/layout tools
            ("MTP", "Mid Between 2 Points", crate::ui::icons::mtp_icon()),
            ("M2P", "Mid Between 2 Points", crate::ui::icons::mtp_icon()),
            ("FROM", "From", crate::ui::icons::from_icon()),
            (
                "QUICKCALC",
                "QuickCalc",
                include_bytes!("../../assets/icons/quickcalc/quickcalc.svg"),
            ),
            (
                "QC",
                "QuickCalc",
                include_bytes!("../../assets/icons/quickcalc/quickcalc.svg"),
            ),
            (
                "CALC",
                "QuickCalc",
                include_bytes!("../../assets/icons/quickcalc/quickcalc.svg"),
            ),
            ("FIND", "Find", include_bytes!("../../assets/icons/find.svg")),
            (
                "STYLE",
                "Text Style",
                include_bytes!("../../assets/icons/text_style.svg"),
            ),
            (
                "DIMSTYLE",
                "Dimension Style",
                include_bytes!("../../assets/icons/dim_style.svg"),
            ),
            // Quad-table dimension commands whose tools live off-ribbon
            // (docs/quad-menu.md §3.9): DIMEDIT is dim_edit.svg the way
            // annotate/dimedit.rs claims it; DIMCENTER rides CENTERMARK's
            // line.svg rather than a hand-invented icon.
            ("DIMEDIT", "Dim Edit", include_bytes!("../../assets/icons/dim_edit.svg")),
            ("DIMCENTER", "Dim Center", include_bytes!("../../assets/icons/line.svg")),
            (
                "MLEADERSTYLE",
                "Multileader Style",
                include_bytes!("../../assets/icons/mleader.svg"),
            ),
            (
                "TABLESTYLE",
                "Table Style",
                include_bytes!("../../assets/icons/table.svg"),
            ),
            // Quad-table Table commands: TABLEDIT edits a cell's text.
            (
                "TABLEDIT",
                "Table Cell Edit",
                include_bytes!("../../assets/icons/table_cell_contents.svg"),
            ),
            (
                "POINTCLOUDUNCROP",
                "Uncrop Point Cloud",
                include_bytes!("../../assets/icons/xclip_remove.svg"),
            ),
            ("3DFACE", "3D Face", crate::ui::icons::MESH),
            ("3DMESH", "3D Mesh", crate::ui::icons::MESH),
            ("MESH", "Mesh", crate::ui::icons::MESH),
            (
                "LAYOUT",
                "Layout",
                include_bytes!("../../assets/icons/layout_tabs.svg"),
            ),
            (
                "DGNIMPORT",
                "DGN Import",
                include_bytes!("../../assets/icons/cui_import.svg"),
            ),
            (
                "COPYTOLAYER",
                "Copy to Layer",
                include_bytes!("../../assets/icons/layers/copytolayer.svg"),
            ),
            (
                "LAYMCUR",
                "Make Object's Layer Current",
                include_bytes!("../../assets/icons/layers/laymcur.svg"),
            ),
            (
                "VPLAYER",
                "Viewport Layer",
                include_bytes!("../../assets/icons/layers/layvpfrz.svg"),
            ),
            (
                "BACTION",
                "Block Action",
                include_bytes!("../../assets/icons/ui/bolt.svg"),
            ),
            ("RESETBLOCK", "Reset Block", crate::ui::icons::REFRESH),
            ("LIST", "List", crate::ui::icons::LIST),
            ("REDRAW", "Redraw", crate::ui::icons::REFRESH),
            (
                "HIDE",
                "Hide",
                include_bytes!("../../assets/icons/hidden.svg"),
            ),
            ("DWGPROPS", "Drawing Properties", crate::ui::icons::INFO),
            (
                "SLICE",
                "Slice",
                include_bytes!("../../assets/icons/model/slice.svg"),
            ),
            (
                "SECTIONPLANE",
                "Section Plane",
                include_bytes!("../../assets/icons/model/sectionplane.svg"),
            ),
            (
                "FLATSHOT",
                "Flatshot",
                include_bytes!("../../assets/icons/model/flatshot.svg"),
            ),
            (
                "INTERFERE",
                "Interfere",
                include_bytes!("../../assets/icons/model/interfere.svg"),
            ),
            (
                "THICKEN",
                "Thicken",
                include_bytes!("../../assets/icons/model/thicken.svg"),
            ),
            (
                "SOLIDEDIT",
                "Solid Edit",
                include_bytes!("../../assets/icons/model/solidedit.svg"),
            ),
            (
                "3DARRAY",
                "3D Array",
                include_bytes!("../../assets/icons/model/array3d.svg"),
            ),
            ("SETVAR", "Set Variable", SYSVAR_ICON),
        ] {
            out.entry(command.to_string())
                .or_insert((label, IconKind::Svg(icon)));
        }
        // System variables use the dedicated `VAR` badge icon.
        for &sysvar in SYSTEM_VARIABLES {
            out.entry(sysvar.to_string())
                .or_insert(("System Variable", IconKind::Svg(SYSVAR_ICON)));
        }
        // Propagate icons to command synonyms, sub-command variants, and `-` forms.
        for &(alias, target) in COMMAND_ICON_ALIASES {
            if let Some(entry) = out.get(target).copied() {
                out.entry(alias.to_string()).or_insert(entry);
            }
        }
        // Propagate icons to default PGP command aliases.
        for (alias, target) in crate::app::alias::default_alias_rows() {
            if let Some(entry) = out.get(&target).copied() {
                out.entry(alias).or_insert(entry);
            }
        }
        out
    })
}

/// Dedicated `VAR` badge SVG for system variables and `SETVAR`.
static SYSVAR_ICON: &[u8] = include_bytes!("../../assets/icons/sysvar.svg");

/// Registered system variables that display the `VAR` badge icon in
/// command-line autocomplete and action lists.
const SYSTEM_VARIABLES: &[&str] = &[
    "AFLAGS",
    "ANGBASE",
    "ANGDIR",
    "ANNOALLVISIBLE",
    "ANNOAUTOSCALE",
    "APERTURE",
    "APSTATE",
    "ATTDIA",
    "ATTDISP",
    "ATTMODE",
    "ATTMODE0",
    "ATTMODE1",
    "ATTMODE2",
    "ATTREQ",
    "AUNITS",
    "AUPREC",
    "BACKGROUNDPLOT",
    "BACTIONBARMODE",
    "BACTIONCOLOR",
    "BCONSTATUSMODE",
    "BDEPENDENCYHIGHLIGHT",
    "BGRIPOBJCOLOR",
    "BGRIPOBJSIZE",
    "BLIPMODE",
    "BLOCKEDITLOCK",
    "BLOCKEDITOR",
    "BLOCKMRULIST",
    "BLOCKNAVIGATE",
    "BLOCKREDEFINEMODE",
    "BLOCKSTATE",
    "BLOCKTESTWINDOW",
    "BPARAMETERCOLOR",
    "BPARAMETERFONT",
    "BPARAMETERSIZE",
    "BPTEXTHORIZONTAL",
    "BTMARKDISPLAY",
    "BVMODE",
    "CAMERADISPLAY",
    "CANNOSCALE",
    "CCONSTRAINTFORM",
    "CDIMSTY",
    "CECOLOR",
    "CELTSCALE",
    "CELTYPE",
    "CENTERCROSSGAP",
    "CENTERCROSSSIZE",
    "CENTEREXE",
    "CENTERLAYER",
    "CENTERLTSCALE",
    "CENTERLTYPE",
    "CENTERLTYPEFILE",
    "CENTERMARKEXE",
    "CETRANSPARENCY",
    "CLAYER",
    "CLIPROMPTLINES",
    "CMLJUST",
    "CMLSCALE",
    "CMLSTYLE",
    "COLORSCHEME",
    "COLORTHEME",
    "COMMANDLINEFADETIME",
    "COMMANDLINEOPACITY",
    "COMPARECOLORADDED",
    "COMPARECOLORMODIFIED",
    "COMPARECOLORREMOVED",
    "COMPARETOLERANCE",
    "CONSTRAINTBARDISPLAY",
    "CONSTRAINTBARMODE",
    "CONSTRAINTINFER",
    "CONSTRAINTNAMEFORMAT",
    "CONSTRAINTSOLVEMODE",
    "COORDS",
    "CROSSINGAREACOLOR",
    "CTAB",
    "CTABLESTYLE",
    "CURSORSIZE",
    "HPNAME",
    "CURSORTYPE",
    "DELOBJ",
    "DGNFRAME",
    "DGNIMPORTMODE",
    "DGNOSNAP",
    "DIMASO",
    "DIMASSOC",
    "DIMCONTINUEMODE",
    "DIMLAYER",
    "DIMSHO",
    "DISPSILH",
    "DONUTID",
    "DONUTOD",
    "DRAGMODE",
    "DWFFRAME",
    "DWFOSNAP",
    "DXEVAL",
    "DYNCONSTRAINTDISPLAY",
    "DYNMODE",
    "ELEVATION",
    "ERSTATE",
    "EXPLMODE",
    "FIELDDISPLAY",
    "FIELDEVAL",
    "FILEASSOC",
    "FILEDIA",
    "FILETAB",
    "FILLMODE",
    "FRAME",
    "FRAMES0",
    "FRAMES1",
    "FRAMES2",
    "FRAMES3",
    "GRIDMODE",
    "GRIDUNIT",
    "GRIPCOLOR",
    "GRIPDYNCOLOR",
    "GRIPHOT",
    "GRIPHOVER",
    "GRIPOBJLIMIT",
    "GRIPSIZE",
    "GRIPTIPS",
    "HALOGAP",
    "HATCHLAYER",
    "HPASSOC",
    "HPDLGMODE",
    "HPGAPTOL",
    "HYPERLINKBASE",
    "IMAGEFRAME",
    "INSBASE",
    "INSNAME",
    "INSUNITS",
    "ISAVEBAK",
    "ISOLINES",
    "LAYOUTCREATEVIEWPORT",
    "LAYOUTTAB",
    "LIMCHECK",
    "LTSCALE",
    "LUNITS",
    "LUPREC",
    "LWDISPLAY",
    "MAXACTVP",
    "MENUBAR",
    "MIRRTEXT",
    "MLEADERLAYER",
    "MSLTSCALE",
    "LOGFILEMODE",
    "LOGFILEPATH",
    "SAVEFILEPATH",
    "MSOLESCALE",
    "OLEFRAME",
    "OLEHIDE",
    "OLEQUALITY",
    "OLESTARTUP",
    "ORTHOMODE",
    "OSMODE",
    "OSNAPZ",
    "PAPERUPDATE",
    "PDFFRAME",
    "PDFIMPORTFILTER",
    "PDFIMPORTIMAGEPATH",
    "PDFIMPORTLAYERS",
    "PDFIMPORTMODE",
    "PDFOSNAP",
    "PDFSHXBESTFONT",
    "PDFSHXLAYER",
    "PDFSHXTEXTSETTINGS",
    "PDMODE",
    "PDSIZE",
    "PELLIPSE",
    "PICKADD",
    "PICKBOX",
    "PICKDRAG",
    "PICKSTYLE",
    "PLIMCHECK",
    "PLINEGEN",
    "PLOTOFFSET",
    "PLOTROTMODE",
    "PLOTTRANSPARENCYOVERRIDE",
    "POINTCLOUD2DVSDISPLAY",
    "POINTCLOUDAUTOUPDATE",
    "POINTCLOUDBOUNDARY",
    "POINTCLOUDCACHESIZE",
    "POINTCLOUDCLIPFRAME",
    "POINTCLOUDDENSITY",
    "POINTCLOUDLIGHTING",
    "POINTCLOUDLIGHTSOURCE",
    "POINTCLOUDLOCK",
    "POINTCLOUDLOD",
    "POINTCLOUDPOINTMAX",
    "POINTCLOUDPOINTMAXLEGACY",
    "POINTCLOUDPOINTSIZE",
    "POINTCLOUDRTDENSITY",
    "POINTCLOUDSHADING",
    "POINTCLOUDVISRETAIN",
    "PSLTSCALE",
    "QPLOCATION",
    "QPMODE",
    "QTEXTMODE",
    "QUAD",
    "QUADCOMMANDLAUNCH",
    "QUADWIDTH",
    "REFEDITNAME",
    "REGENMODE",
    "SAVETIME",
    "SDI",
    "SELECTIONAREA",
    "SELECTIONAREAOPACITY",
    "SELECTIONCYCLING",
    "SELECTIONEFFECT",
    "SELECTIONEFFECTCOLOR",
    "SELECTIONPREVIEW",
    "SELECTSIMILARMODE",
    "SETBYLAYERMODE",
    "SHADEDGE",
    "SHADEMODE",
    "SHORTCUTMENU",
    "SHORTCUTMENUDURATION",
    "SHOWHIST",
    "SKETCHINC",
    "SKPOLY",
    "SKTOLERANCE",
    "SNAPANG",
    "SOLIDHIST",
    "SORTENTS",
    "SPLFRAME",
    "SPLINESEGS",
    "SPLINETYPE",
    "SSFOUND",
    "SSLOCATE",
    "SSMAUTOOPEN",
    "SSMPOLLTIME",
    "SSMSHEETSTATUS",
    "SSMSTATE",
    "SURFACEASSOCIATIVITY",
    "SURFTYPE",
    "SURFU",
    "SURFV",
    "TEXTFILL",
    "TEXTLAYER",
    "TEXTQLTY",
    "TEXTSIZE",
    "TEXTSTYLE",
    "THICKNESS",
    "TILEMODE",
    "TRACEWID",
    "TRIMEXTENDMODE",
    "REVCLOUDCREATEMODE",
    "REVCLOUDARCVARIANCE",
    "REVCLOUDGRIPS",
    "GROUPDISPLAYMODE",
    "OFFSETGAPTYPE",
    "MATCHMODE",
    "REVCLOUDAPPROXARCLEN",
    "TXT2MTXTCOMBINEMTEXT",
    "TXT2MTXTLINESPACING",
    "TXT2MTXTTEXTORDER",
    "TXT2MTXTWORDWRAP",
    "UCSDETECT",
    "UCSFOLLOW",
    "UCSORTHO",
    "UCSVIEW",
    "UOSNAP",
    "UOSNAP0",
    "UOSNAP1",
    "USERI",
    "USERR",
    "USRTIMER",
    "VISRETAIN",
    "VPHIGHLIGHT",
    "VPHIGHLIGHTOPACITY",
    "VSCURRENT",
    "VSEDGECOLOR",
    "VSEDGESMOOTH",
    "WINDOWAREACOLOR",
    "WINDOWSAREACOLOR",
    "WIPEOUTFRAME",
    "WORLDVIEW",
    "XCLIPFRAME",
    "XDWGFADECTL",
    "XEDIT",
    "XFADECTL",
    "XREFLAYER",
    "ZOOMFACTOR",
    "ZOOMWHEEL",
];

/// The ribbon icon of a command line, for surfaces that show the command
/// outside the ribbon. A leading `'` (transparent use) or `-` (command-line
/// variant) is ignored when looking up the base command's icon.
pub fn command_icon(command: &str) -> Option<&'static [u8]> {
    let key = command.trim().trim_start_matches('\'').to_ascii_uppercase();
    let commands = ribbon_commands();
    let entry = commands
        .get(&key)
        .or_else(|| key.strip_prefix('-').and_then(|base| commands.get(base)))?;
    match entry.1 {
        IconKind::Svg(bytes) => Some(bytes),
        IconKind::Glyph(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_aliases_and_variants_resolve_target_icons() {
        for (alias, target) in [
            ("RECTANG", "RECT"),
            ("REVCLOUD_RECTANGULAR", "REVCLOUD"),
            ("REVCLOUD_POLYGONAL", "REVCLOUD"),
            ("REVCLOUD_FREEHAND", "REVCLOUD"),
            ("POLYGON", "POLY"),
            ("ARC", "ARC_3P"),
            ("PASTE", "PASTECLIP"),
            ("ARRAY", "ARRAYRECT"),
            ("CONSTRUCTIONLINE", "XLINE"),
            ("XCLIP", "CLIP"),
            ("IMAGECLIP", "CLIP"),
            ("IMAGEADJUST", "ADJUST"),
            ("LAYER", "LAYERS"),
            ("-LAYER", "LAYERS"),
            ("QLEADER", "LEADER"),
            ("GCCOINCIDENT", "CCONSTRAINT"),
            ("GCEQUAL", "ECONSTRAINT"),
            ("GCVERTICAL", "VCONSTRAINT"),
            ("-BLOCK", "BLOCK"),
            ("-INSERT", "INSERT"),
            ("-LAYOUT", "LAYOUT"),
            ("-DGNIMPORT", "DGNIMPORT"),
            ("-OSNAP", "OSNAP"),
            ("M2P", "MTP"),
            ("QC", "QUICKCALC"),
            ("U", "UNDO"),
            ("MREDO", "REDO"),
            ("QNEW", "NEW"),
            ("QSAVE", "SAVE"),
            ("QUIT", "CLOSE"),
            ("EXIT", "CLOSE"),
            ("SL", "SLICE"),
            ("SECTION", "SECTIONPLANE"),
            ("SECTIONCULL", "SECTIONPLANE"),
            ("FLATTEN", "FLATSHOT"),
            ("INF", "INTERFERE"),
            ("CONVTOSURFACE", "THICKEN"),
            ("BPARAMETER", "PARAMETERS"),
            ("BACTIONTOOL", "BACTION"),
            ("BLOOKUPTABLE", "TABLE"),
            ("REGEN", "REDRAW"),
            ("LI", "LIST"),
            ("REC", "RECT"),
            ("L", "LINE"),
        ] {
            let alias_icon = command_icon(alias);
            let target_icon = command_icon(target);
            assert!(
                alias_icon.is_some(),
                "expected command_icon({alias:?}) to resolve an icon"
            );
            assert_eq!(
                alias_icon.map(|b| (b.as_ptr(), b.len())),
                target_icon.map(|b| (b.as_ptr(), b.len())),
                "expected {alias} to share the icon of {target}"
            );
        }

        for cmd in [
            "UNDO",
            "REDO",
            "NEW",
            "OPEN",
            "SAVE",
            "SAVEAS",
            "CLOSE",
            "HELP",
            "MTP",
            "FROM",
            "QUICKCALC",
            "ORTHO",
            "OSNAP",
            "POLAR",
            "GRID",
            "SNAP",
            "WIPEOUT",
            "NRCONSTRAINT",
            "3DFACE",
            "3DMESH",
            "SLICE",
            "SECTIONPLANE",
            "FLATSHOT",
            "INTERFERE",
            "THICKEN",
            "SOLIDEDIT",
            "3DARRAY",
            "COPYTOLAYER",
            "LAYMCUR",
            "VPLAYER",
            "BACTION",
            "LIST",
            "REDRAW",
            "HIDE",
            "DWGPROPS",
        ] {
            assert!(
                command_icon(cmd).is_some(),
                "expected command_icon({cmd:?}) to resolve an icon"
            );
        }

        // System variables and SETVAR must resolve to the VAR badge icon.
        for sysvar in [
            "SETVAR",
            "AUPREC",
            "LUPREC",
            "PDMODE",
            "PDSIZE",
            "AFLAGS",
            "PICKBOX",
            "SAVETIME",
            "ANGDIR",
            "INSUNITS",
            "TILEMODE",
        ] {
            let icon = command_icon(sysvar);
            assert_eq!(
                icon.map(|b| (b.as_ptr(), b.len())),
                Some((SYSVAR_ICON.as_ptr(), SYSVAR_ICON.len())),
                "system variable {sysvar} should resolve to SYSVAR_ICON"
            );
        }
    }
}

/// Applicable-command side table for the quad cursor menu
/// (docs/quad-menu.md §3.3): canonical `entity_type_name()` keys
/// (src/entities/traits.rs:174) mapped to 6–12 existing OCS commands, generic
/// modify first, then the type-specific edits. Labels and icons resolve
/// through `ribbon_commands()`.
pub static QUAD_ENTITY_COMMANDS: &[(&str, &[&str])] = &[
    (
        "Line",
        &[
            "MOVE",
            "COPY",
            "ROTATE",
            "SCALE",
            "ARRAYRECT",
            "FILLET",
            "CHAMFER",
            "TRIM",
            "EXTEND",
            "LENGTHEN",
            "BREAK",
            "ERASE",
        ],
    ),
    (
        "CenterMark",
        &["MOVE", "COPY", "ROTATE", "SCALE", "MIRROR", "ERASE"],
    ),
    (
        "CenterLine",
        &["MOVE", "COPY", "ROTATE", "SCALE", "MIRROR", "ERASE"],
    ),
    (
        "LwPolyline",
        &[
            "MOVE",
            "COPY",
            "ROTATE",
            "SCALE",
            "MIRROR",
            "PEDIT",
            "OFFSET",
            "EXPLODE",
            "ERASE",
        ],
    ),
    (
        "Revcloud",
        &[
            "MOVE",
            "COPY",
            "ROTATE",
            "SCALE",
            "REVCLOUD_RECTANGULAR",
            "REVCLOUD_POLYGONAL",
            "ERASE",
        ],
    ),
    (
        "Circle",
        &["MOVE", "COPY", "ROTATE", "SCALE", "MIRROR", "OFFSET", "ERASE"],
    ),
    (
        "Arc",
        &[
            "MOVE",
            "COPY",
            "ROTATE",
            "SCALE",
            "MIRROR",
            "OFFSET",
            "LENGTHEN",
            "DIMRADIUS",
            "ERASE",
        ],
    ),
    (
        "Ellipse",
        &[
            "MOVE",
            "COPY",
            "ROTATE",
            "SCALE",
            "MIRROR",
            "ARRAYRECT",
            "OFFSET",
            "ERASE",
        ],
    ),
    (
        "Spline",
        &[
            "MOVE",
            "COPY",
            "ROTATE",
            "SCALE",
            "MIRROR",
            "SPLINEDIT",
            "LENGTHEN",
            "ERASE",
        ],
    ),
    (
        "Point",
        &["MOVE", "COPY", "ROTATE", "SCALE", "MATCHPROP", "LIST", "ERASE"],
    ),
    (
        "MText",
        &[
            "MOVE",
            "COPY",
            "ROTATE",
            "SCALE",
            "ARRAYRECT",
            "TEXTEDIT",
            "MTEDIT",
            "TXT2MTXT",
            "ERASE",
        ],
    ),
    (
        "Text",
        &[
            "MOVE",
            "COPY",
            "ROTATE",
            "SCALE",
            "MIRROR",
            "TEXTEDIT",
            "TXT2MTXT",
            "ERASE",
        ],
    ),
    (
        "Dimension",
        &[
            "MOVE",
            "COPY",
            "ROTATE",
            "SCALE",
            "MIRROR",
            "DIMEDIT",
            "DIMTEDIT",
            "DIMSPACE",
            "DIMOVERRIDE",
            "DIMBREAK",
            "DIMCENTER",
            "ERASE",
        ],
    ),
    (
        "Hatch",
        &[
            "MOVE",
            "COPY",
            "ROTATE",
            "SCALE",
            "MIRROR",
            "OFFSET",
            "HATCHEDIT",
            "MATCHPROP",
            "ERASE",
        ],
    ),
    (
        "Insert",
        &[
            "MOVE",
            "COPY",
            "ROTATE",
            "SCALE",
            "MIRROR",
            "ARRAYRECT",
            "EXPLODE",
            "CLIP",
            "BEDIT",
            "REFEDIT",
            "ATTREDEF",
            "ERASE",
        ],
    ),
    (
        "MultiLeader",
        &["MOVE", "COPY", "ROTATE", "SCALE", "MIRROR", "EXPLODE", "ERASE"],
    ),
    (
        "Leader",
        &[
            "MOVE",
            "COPY",
            "ROTATE",
            "SCALE",
            "MIRROR",
            "LEADER",
            "EXPLODE",
            "ERASE",
        ],
    ),
    (
        "Table",
        &["MOVE", "COPY", "ROTATE", "SCALE", "TABLEDIT", "EXPLODE", "ERASE"],
    ),
    (
        "Viewport",
        &["MOVE", "COPY", "SCALE", "VPLAYER", "VPCLIP", "ERASE"],
    ),
    (
        "Solid",
        &[
            "MOVE",
            "COPY",
            "ROTATE",
            "SCALE",
            "MIRROR",
            "ARRAYRECT",
            "EXPLODE",
            "ERASE",
        ],
    ),
];

/// The applicable commands of one entity type key: exact canonical
/// (`entity_type_name()`) match first, then a case-insensitive fallback, so
/// plugins using their own `class_id` casing still resolve. Unknown keys give
/// an empty slice.
pub fn quad_entity_commands(type_key: &str) -> &'static [&'static str] {
    if let Some((_, cmds)) = QUAD_ENTITY_COMMANDS.iter().find(|(key, _)| *key == type_key) {
        return cmds;
    }
    QUAD_ENTITY_COMMANDS
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(type_key))
        .map_or(&[], |(_, cmds)| cmds)
}

/// Creation commands the No Selection quad offers as tabs, grouped under
/// short labels (docs/quad-menu.md §1, "No Selection Quad" row). Every
/// command resolves a ribbon icon exactly like the ribbon itself.
pub static QUAD_NO_SELECTION_COMMAND_GROUPS: &[(&str, &[&str])] = &[
    (
        "Draw",
        &[
            "LINE",
            "CIRCLE",
            "ARC",
            "ELLIPSE",
            "RECT",
            "PLINE",
            "SPLINE",
            "POINT",
            "HATCH",
            "REVCLOUD",
        ],
    ),
    (
        "Dimension",
        &[
            "DIMLINEAR",
            "DIMALIGNED",
            "DIMRADIUS",
            "DIMDIAMETER",
            "DIMANGULAR",
            "DIMJOGGED",
            "DIMCENTER",
        ],
    ),
    (
        "Annotate",
        &["TEXT", "MTEXT", "LEADER", "MLEADER", "TABLE", "ATTDEF"],
    ),
    (
        "Block",
        &["INSERT", "BEDIT", "ATTEDIT", "XATTACH"],
    ),
];

/// The No Selection quad's tab groups (label + creation commands).
pub fn quad_no_selection_groups() -> &'static [(&'static str, &'static [&'static str])] {
    QUAD_NO_SELECTION_COMMAND_GROUPS
}

#[cfg(test)]
mod quad_registry_tests {
    use super::*;

    /// Commands deliberately left to the glyph-fallback tile because the
    /// registry carries no SVG for them. Kept empty while every quad table
    /// command resolves a ribbon icon; entries here are a tracked deliverable
    /// (docs/quad-menu.md §3.9), not a pass-silently hook.
    const QUAD_GLYPH_FALLBACK_WHITELIST: &[&str] = &[];

    #[test]
    fn quad_entity_tables_resolve_icons_and_stay_unique() {
        let mut all: Vec<&str> = Vec::new();
        for (type_key, cmds) in QUAD_ENTITY_COMMANDS.iter() {
            assert!(!cmds.is_empty(), "no commands for {type_key}");
            for cmd in *cmds {
                assert_eq!(
                    *cmd,
                    cmd.to_ascii_uppercase(),
                    "quad commands must be upper-case command lines: {cmd}"
                );
                if !QUAD_GLYPH_FALLBACK_WHITELIST.contains(cmd) {
                    assert!(
                        command_icon(cmd).is_some(),
                        "expected command_icon({cmd:?}) to resolve an icon (quad type {type_key})"
                    );
                }
                assert_eq!(
                    cmds.iter().filter(|c| **c == *cmd).count(),
                    1,
                    "duplicate command {cmd} in the {type_key} quad table"
                );
            }
            all.extend_from_slice(cmds);
        }
        assert!(all.contains(&"MOVE"), "generic modify must lead the tables");
    }

    #[test]
    fn quad_no_selection_groups_resolve_icons() {
        assert!(!QUAD_NO_SELECTION_COMMAND_GROUPS.is_empty());
        for (label, cmds) in QUAD_NO_SELECTION_COMMAND_GROUPS.iter() {
            assert!(!label.is_empty(), "group labels are short English tabs");
            for cmd in *cmds {
                if !QUAD_GLYPH_FALLBACK_WHITELIST.contains(cmd) {
                    assert!(
                        command_icon(cmd).is_some(),
                        "expected command_icon({cmd:?}) to resolve an icon (no-selection {label})"
                    );
                }
            }
        }
    }

    #[test]
    fn quad_entity_commands_lookup() {
        let line = quad_entity_commands("Line");
        assert!(!line.is_empty());
        assert_eq!(quad_entity_commands("line"), line);
        assert_eq!(quad_entity_commands("LINE"), line);
        assert_eq!(quad_entity_commands("lwPOLYLINE"), quad_entity_commands("LwPolyline"));
        assert_eq!(quad_entity_commands("Hatch"), quad_entity_commands("HATCH"));
        assert!(quad_entity_commands("NotAnEntity").is_empty());
        assert!(quad_entity_commands("").is_empty());
    }

    #[test]
    fn quad_sysvars_resolve_the_var_badge() {
        for sysvar in ["QUAD", "QUADWIDTH", "QUADCOMMANDLAUNCH"] {
            assert!(
                SYSTEM_VARIABLES.contains(&sysvar),
                "{sysvar} must be a registered system variable"
            );
            let icon = command_icon(sysvar);
            assert_eq!(
                icon.map(|b| (b.as_ptr(), b.len())),
                Some((SYSVAR_ICON.as_ptr(), SYSVAR_ICON.len())),
                "system variable {sysvar} should resolve to SYSVAR_ICON"
            );
        }
    }
}
