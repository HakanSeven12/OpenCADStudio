//! Classic pull-down menus above the ribbon (MENUBAR 1). Every entry runs a
//! command, so the menus stay a view of the command set rather than a second
//! implementation of it.

use crate::app::Message;
use iced::widget::{button, container, text};
use iced::{Background, Border, Element, Fill, Theme};
use iced_aw::menu::{Item, Menu, MenuBar};

type Entries = &'static [(&'static str, &'static str)];

/// Menu title and its (label, command) entries, in menu-bar order.
const MENUS: &[(&str, Entries)] = &[
    (
        "File",
        &[
            ("New", "QNEW"),
            ("Open", "OPEN"),
            ("Close", "CLOSE"),
            ("Save", "QSAVE"),
            ("Save As", "SAVEAS"),
            ("Page Setup Manager", "PAGESETUP"),
            ("Plot", "PLOT"),
            ("Publish", "PUBLISH"),
            ("Audit", "AUDIT"),
            ("Purge", "PURGE"),
            ("Exit", "QUIT"),
        ],
    ),
    (
        "Edit",
        &[
            ("Undo", "UNDO"),
            ("Redo", "REDO"),
            ("Cut", "CUTCLIP"),
            ("Copy", "COPYCLIP"),
            ("Copy with Base Point", "COPYBASE"),
            ("Paste", "PASTECLIP"),
            ("Paste as Block", "PASTEBLOCK"),
            ("Paste to Original Coordinates", "PASTEORIG"),
            ("Paste Special", "PASTESPEC"),
            ("Erase", "ERASE"),
            ("Find and Replace", "FIND"),
        ],
    ),
    (
        "View",
        &[
            ("Redraw", "REDRAW"),
            ("Regen", "REGEN"),
            ("Regen All", "REGENALL"),
            ("Zoom", "ZOOM"),
            ("Pan", "PAN"),
            ("Orbit", "3DORBIT"),
            ("Named Views", "VIEW"),
            ("View Twist", "DVIEW"),
            ("Viewports", "VPORTS"),
            ("Clean Screen", "CLEANSCREEN"),
        ],
    ),
    (
        "Insert",
        &[
            ("Block", "INSERT"),
            ("External References", "EXTERNALREFERENCES"),
            ("Raster Image", "IMAGEATTACH"),
            ("PDF Underlay", "PDFATTACH"),
            ("Field", "FIELD"),
            ("Hyperlink", "HYPERLINK"),
        ],
    ),
    (
        "Format",
        &[
            ("Layer", "LAYER"),
            ("Color", "COLOR"),
            ("Linetype", "LINETYPE"),
            ("Text Style", "STYLE"),
            ("Dimension Style", "DIMSTYLE"),
            ("Table Style", "TABLESTYLE"),
            ("Multileader Style", "MLEADERSTYLE"),
            ("Multiline Style", "MLSTYLE"),
            ("Point Style", "PTYPE"),
            ("Units", "UNITS"),
            ("Drawing Limits", "LIMITS"),
        ],
    ),
    (
        "Tools",
        &[
            ("Quick Select", "QSELECT"),
            ("Draw Order", "DRAWORDER"),
            ("Distance", "DIST"),
            ("Area", "AREA"),
            ("List", "LIST"),
            ("ID Point", "ID"),
            ("Run Script", "SCRIPT"),
            ("New UCS", "UCS"),
            ("Drafting Settings", "DSETTINGS"),
            ("Options", "OPTIONS"),
        ],
    ),
    (
        "Draw",
        &[
            ("Line", "LINE"),
            ("Ray", "RAY"),
            ("Construction Line", "XLINE"),
            ("Multiline", "MLINE"),
            ("Polyline", "PLINE"),
            ("3D Polyline", "3DPOLY"),
            ("Polygon", "POLYGON"),
            ("Rectangle", "RECTANG"),
            ("Arc", "ARC"),
            ("Circle", "CIRCLE"),
            ("Donut", "DONUT"),
            ("Spline", "SPLINE"),
            ("Ellipse", "ELLIPSE"),
            ("Block", "BLOCK"),
            ("Table", "TABLE"),
            ("Point", "POINT"),
            ("Hatch", "HATCH"),
            ("Boundary", "BOUNDARY"),
            ("Region", "REGION"),
            ("Wipeout", "WIPEOUT"),
            ("Revision Cloud", "REVCLOUD"),
            ("Text", "TEXT"),
            ("Multiline Text", "MTEXT"),
        ],
    ),
    (
        "Dimension",
        &[
            ("Quick Dimension", "QDIM"),
            ("Linear", "DIMLINEAR"),
            ("Aligned", "DIMALIGNED"),
            ("Arc Length", "DIMARC"),
            ("Ordinate", "DIMORDINATE"),
            ("Radius", "DIMRADIUS"),
            ("Jogged", "DIMJOGGED"),
            ("Diameter", "DIMDIAMETER"),
            ("Angular", "DIMANGULAR"),
            ("Baseline", "DIMBASELINE"),
            ("Continue", "DIMCONTINUE"),
            ("Multileader", "MLEADER"),
            ("Tolerance", "TOLERANCE"),
            ("Center Mark", "DIMCENTER"),
            ("Dimension Style", "DIMSTYLE"),
        ],
    ),
    (
        "Modify",
        &[
            ("Properties", "PROPERTIES"),
            ("Match Properties", "MATCHPROP"),
            ("Erase", "ERASE"),
            ("Copy", "COPY"),
            ("Mirror", "MIRROR"),
            ("Offset", "OFFSET"),
            ("Array", "ARRAY"),
            ("Move", "MOVE"),
            ("Rotate", "ROTATE"),
            ("Scale", "SCALE"),
            ("Stretch", "STRETCH"),
            ("Lengthen", "LENGTHEN"),
            ("Trim", "TRIM"),
            ("Extend", "EXTEND"),
            ("Break", "BREAK"),
            ("Join", "JOIN"),
            ("Chamfer", "CHAMFER"),
            ("Fillet", "FILLET"),
            ("Explode", "EXPLODE"),
            ("Explode Text", "TXTEXP"),
            ("Edit Polyline", "PEDIT"),
            ("Edit Spline", "SPLINEDIT"),
            ("Edit Hatch", "HATCHEDIT"),
        ],
    ),
    (
        "Parametric",
        &[
            ("Auto Constrain", "AUTOCONSTRAIN"),
            ("Coincident", "GCCOINCIDENT"),
            ("Parallel", "PCONSTRAINT"),
            ("Perpendicular", "GCPERPENDICULAR"),
            ("Horizontal", "GCHORIZONTAL"),
            ("Vertical", "GCVERTICAL"),
            ("Tangent", "TCONSTRAINT"),
            ("Equal", "GCEQUAL"),
            ("Fix", "GCFIX"),
            ("Dimensional", "DIMCONSTRAINT"),
            ("Delete Constraints", "DELCONSTRAINT"),
            ("Parameters Manager", "PARAMETERS"),
        ],
    ),
    ("Help", &[("Help", "HELP"), ("About", "ABOUT")]),
];

fn entry_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.palette();
    let pair = match status {
        button::Status::Hovered | button::Status::Pressed => palette.primary.weak,
        _ => palette.background.base,
    };
    button::Style {
        background: Some(Background::Color(pair.color)),
        text_color: pair.text,
        ..Default::default()
    }
}

/// The menu bar, one pull-down per menu.
pub fn view<'a>() -> Element<'a, Message> {
    let roots = MENUS
        .iter()
        .map(|&(title, entries)| {
            let items = entries
                .iter()
                .map(|&(label, command)| {
                    Item::new(
                        button(text(crate::t!(label).into_owned()).size(12))
                            .on_press(Message::Command(command.to_string()))
                            .style(entry_style)
                            .padding([4, 12])
                            .width(Fill),
                    )
                })
                .collect();
            Item::with_menu(
                container(text(crate::t!(title).into_owned()).size(12)).padding([3, 8]),
                Menu::new(items).width(240.0).offset(2.0),
            )
        })
        .collect();
    MenuBar::new(roots)
        .close_on_item_click_global(true)
        .style(|theme: &Theme, _| {
            let palette = theme.palette();
            iced_aw::style::menu_bar::Style {
                bar_background: Background::Color(palette.background.weakest.color),
                bar_border: Border::default(),
                bar_shadow: Default::default(),
                menu_background: Background::Color(palette.background.base.color),
                menu_border: Border {
                    color: palette.background.strong.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                menu_shadow: Default::default(),
                path: Background::Color(palette.primary.weak.color),
                path_border: Border::default(),
            }
        })
        .width(Fill)
        .into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_menu_entry_runs_a_known_command() {
        let mut known = crate::command::all_registered_command_names();
        known.extend(["UNDO", "REDO", "QSAVE", "OPEN", "ZOOM"]);
        let unknown: Vec<_> = super::MENUS
            .iter()
            .flat_map(|(_, entries)| entries.iter())
            .filter(|(_, command)| !known.contains(command))
            .collect();
        assert!(unknown.is_empty(), "{unknown:?}");
    }
}
