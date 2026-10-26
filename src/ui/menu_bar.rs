//! Classic pull-down menus above the ribbon (MENUBAR 1). Every entry runs a
//! command, so the menus stay a view of the command set rather than a second
//! implementation of it.

use crate::app::Message;
use crate::ui::style::chrome;
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{mouse, overlay, renderer, Shell};
use iced::widget::{button, container, text};
use iced::{
    Background, Border, Element, Event, Fill, Length, Rectangle, Renderer, Shadow, Size, Theme,
    Vector,
};
use iced_aw::menu::{Item, Menu, MenuBar};

/// Height of the menu row, and of a menu title in it.
const ROW_H: f32 = 25.0;
const TITLE_H: f32 = 20.0;

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
                Title::new(
                    container(text(crate::t!(title).into_owned()).size(12))
                        .center_y(TITLE_H)
                        .padding([0, 9]),
                ),
                Menu::new(items).width(240.0).offset(2.0),
            )
        })
        .collect();
    let bar = MenuBar::new(roots)
        .close_on_item_click_global(true)
        .style(|theme: &Theme, _| {
            let palette = theme.palette();
            iced_aw::style::menu_bar::Style {
                bar_background: Background::Color(chrome::bar(theme)),
                bar_border: Border::default(),
                bar_shadow: Default::default(),
                menu_background: Background::Color(palette.background.base.color),
                menu_border: Border {
                    color: palette.background.strong.color,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                menu_shadow: Default::default(),
                path: Background::Color(chrome::hover(theme)),
                path_border: Border::default(),
            }
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
