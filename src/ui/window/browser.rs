//! The Browser: a standing outline of what the drawing is built from.
//!
//! Three sections, each read straight from the document rather than from a
//! parallel model the panel would have to keep in sync:
//!
//! * **Origin planes** — the three world planes, as somewhere to start a
//!   sketch when the drawing is still empty and there is no face to pick.
//! * **Sketch** — the open sketch, if there is one.
//! * **Bodies** — every `Solid3D`, labelled with the operation that made it,
//!   recovered from the DWG solid history.
//!
//! What this panel deliberately is *not* is a parametric feature tree. The
//! history records how each solid was built, but nothing re-runs it, so
//! editing an earlier feature cannot rebuild the ones after it. Showing the
//! list as if it were editable history would promise exactly that. It is a
//! table of contents, and it is labelled as one.

use crate::app::Message;
use crate::ui::dock::PanelId;
use codec::objects::SolidHistoryOperation;
use codec::{CadDocument, EntityType, Handle};
use iced::widget::{button, column, container, scrollable, text};
use iced::{Background, Element, Fill, Theme};

/// The three world planes, with the argument `CREATESKETCH` takes for each.
const ORIGIN_PLANES: [(&str, &str); 3] = [("XY", "XY"), ("XZ", "XZ"), ("YZ", "YZ")];

/// Human name for the operation that produced a solid. `None` when the
/// drawing carries no history for it — a solid imported as plain geometry,
/// or one whose history was stripped on save.
fn operation_label(operation: &SolidHistoryOperation) -> &'static str {
    match operation {
        SolidHistoryOperation::Box(_) => "Box",
        SolidHistoryOperation::Wedge(_) => "Wedge",
        SolidHistoryOperation::Sphere(_) => "Sphere",
        SolidHistoryOperation::Cone(_) => "Cone",
        SolidHistoryOperation::Cylinder(_) => "Cylinder",
        SolidHistoryOperation::Torus(_) => "Torus",
        SolidHistoryOperation::Pyramid(_) => "Pyramid",
        SolidHistoryOperation::Sweep(_) => "Sweep",
        SolidHistoryOperation::Loft(_) => "Loft",
        SolidHistoryOperation::Extrusion(_) => "Extrude",
        SolidHistoryOperation::Revolve(_) => "Revolve",
        SolidHistoryOperation::Fillet(_) => "Fillet",
        SolidHistoryOperation::Chamfer(_) => "Chamfer",
        _ => "Solid",
    }
}

/// Every `Solid3D` in model space, with the label to show for it.
fn bodies(document: &CadDocument) -> Vec<(Handle, String)> {
    document
        .entities()
        .filter(|entity| matches!(entity, EntityType::Solid3D(_)))
        .map(|entity| {
            let handle = entity.common().handle;
            let label = crate::scene::model::solid_history::primitive_property_operation(
                document, handle,
            )
            .as_ref()
            .map_or("Solid", operation_label);
            (handle, format!("{label} {:X}", handle.value()))
        })
        .collect()
}

fn section_header(label: String) -> Element<'static, Message> {
    container(text(label).size(11))
        .width(Fill)
        .padding([3, 6])
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.weak.color)),
            ..Default::default()
        })
        .into()
}

/// A clickable row that runs `command`.
fn entry_row(label: String, command: String) -> Element<'static, Message> {
    button(text(label).size(12))
        .on_press(Message::Command(command))
        .width(Fill)
        .padding([3, 10])
        .style(button::subtle)
        .into()
}

/// A row that states something rather than doing something.
fn note_row(label: String) -> Element<'static, Message> {
    container(text(label).size(11).style(crate::ui::style::common::muted_style))
        .width(Fill)
        .padding([3, 10])
        .into()
}

pub fn view<'a>(
    document: &'a CadDocument,
    open_sketch: Option<&'a str>,
    width: f32,
    auto_collapse: bool,
) -> Element<'a, Message> {
    let title_bar =
        crate::ui::dock::title_bar(PanelId::Browser, crate::t!("Browser").into_owned(), auto_collapse);

    let mut tree = column![].spacing(1);

    tree = tree.push(section_header(crate::t!("Origin planes").into_owned()));
    for (label, argument) in ORIGIN_PLANES {
        tree = tree.push(entry_row(
            label.to_string(),
            format!("CREATESKETCH {argument}"),
        ));
    }

    tree = tree.push(section_header(crate::t!("Sketch").into_owned()));
    match open_sketch {
        Some(name) => {
            tree = tree.push(entry_row(name.to_string(), "FINISHSKETCH".to_string()));
        }
        None => {
            tree = tree.push(note_row(crate::t!("No sketch open").into_owned()));
        }
    }

    let bodies = bodies(document);
    tree = tree.push(section_header(crate::tf!("Bodies ({})", bodies.len()).into_owned()));
    if bodies.is_empty() {
        tree = tree.push(note_row(crate::t!("No solids yet").into_owned()));
    } else {
        for (handle, label) in bodies {
            // Selecting by handle is what SELECT already takes, so clicking a
            // row reuses the ordinary selection path.
            tree = tree.push(entry_row(label, format!("SELECT {:X}", handle.value())));
        }
    }

    let body = scrollable(container(tree).padding(iced::Padding {
        top: 0.0,
        right: 8.0,
        bottom: 6.0,
        left: 0.0,
    }))
    .width(Fill)
    .height(Fill);

    crate::ui::dock::frame(column![title_bar, body].spacing(6), width)
}
