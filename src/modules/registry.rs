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
    // 3D modeling
    ("SOLIDFILLET", "FILLETEDGE"),
    ("SOLIDCHAMFER", "CHAMFEREDGE"),
    ("PYR", "PYRAMID"),
    // Layers & properties
    ("LAYER", "LAYERS"),
    ("-LAYER", "LAYERS"),
    ("LAYMCH", "LAYMATCH"),
    ("CHPROP", "PROPERTIES"),
    ("CHANGE", "PROPERTIES"),
    ("PROPS", "PROPERTIES"),
    // Blocks, references, underlays & point clouds
    ("-BLOCK", "BLOCK"),
    ("BMAKE", "BLOCK"),
    ("-BMAKE", "BLOCK"),
    ("-WBLOCK", "WBLOCK"),
    ("WB", "WBLOCK"),
    ("-WB", "WBLOCK"),
    ("-INSERT", "INSERT"),
    ("-ATTDEF", "ATTDEF"),
    ("-ATTEDIT", "ATTEDIT"),
    ("EATTEDIT", "ATTEDIT"),
    ("-BEDIT", "BEDIT"),
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
    ("-POINTCLOUDATTACH", "POINTCLOUDATTACH"),
    ("POINTCLOUDMANAGER", "POINTCLOUDATTACH"),
    ("-POINTCLOUDMANAGER", "POINTCLOUDATTACH"),
    ("XREF", "XATTACH"),
    ("-XREF", "XATTACH"),
    ("ATTACH", "XATTACH"),
    ("-ATTACH", "XATTACH"),
    ("EXTERNALREFERENCES", "XATTACH"),
    ("ADCCLOSE", "ADCENTER"),
    // Annotation, dimensions, text, leaders & tables
    ("DTEXT", "TEXT"),
    ("-MTEXT", "MTEXT"),
    ("TEXTEDIT", "DDEDIT"),
    ("TEDIT", "DDEDIT"),
    ("MTEDIT", "DDEDIT"),
    ("-STYLE", "STYLE"),
    ("DDSTYLE", "STYLE"),
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
    ("DCE", "DIMCENTER"),
    ("CENTERMARK", "DIMCENTER"),
    ("CENTERRESET", "CENTERLINE"),
    ("CENTERREASSOCIATE", "CENTERLINE"),
    ("CENTERDISASSOCIATE", "CENTERLINE"),
    ("DDIM", "DIMSTYLE"),
    ("-DIMSTYLE", "DIMSTYLE"),
    ("COUNT", "COUNTLIST"),
    ("-SCALELISTEDIT", "SCALELISTEDIT"),
    ("-DATAEXTRACTION", "DATAEXTRACTION"),
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
    // View, layout, manage & file
    ("-VIEW", "VIEW"),
    ("-VPORTS", "VPORTS"),
    ("-PAN", "PAN"),
    ("ORBIT", "3DORBIT"),
    ("3DO", "3DORBIT"),
    ("3O", "3DORBIT"),
    ("ZS", "ZOOM"),
    ("-MVIEW", "MVIEW"),
    ("PLOT", "PRINT"),
    ("-PLOT", "PRINT"),
    ("-PURGE", "PURGE"),
    ("-PU", "PURGE"),
    ("PREFERENCES", "OPTIONS"),
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
            super::draw::inquiry::measuregeom::tool(),
            super::insert::content_browser::tool(),
            super::insert::design_center::tool(),
            super::insert::minsert::tool(),
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
        // Panel launcher and standalone tool commands with dedicated tool SVGs.
        for (command, label, icon) in [
            (
                "FIND",
                "Find",
                include_bytes!("../../assets/icons/find.svg") as &'static [u8],
            ),
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
            (
                "POINTCLOUDUNCROP",
                "Uncrop Point Cloud",
                include_bytes!("../../assets/icons/xclip_remove.svg"),
            ),
        ] {
            out.entry(command.to_string())
                .or_insert((label, IconKind::Svg(icon)));
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

/// The ribbon icon of a command line, for surfaces that show the command
/// outside the ribbon. A leading `'` (transparent use) is ignored.
pub fn command_icon(command: &str) -> Option<&'static [u8]> {
    let key = command.trim().trim_start_matches('\'').to_ascii_uppercase();
    match ribbon_commands().get(&key)?.1 {
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

        // System variables must not receive command icons.
        for sysvar in ["AUPREC", "LUPREC", "PDMODE", "PDSIZE", "AFLAGS"] {
            assert!(
                command_icon(sysvar).is_none(),
                "system variable {sysvar} should not have a command icon"
            );
        }
    }
}
