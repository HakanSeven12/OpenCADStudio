//! Named-parameter table editor using buffered `(name, formula)` rows.
//! edited in a working buffer (`OpenCADStudio::named_parameter_editor_rows`)
//! and committed to `Scene::named_parameters` only on Apply — matching the
//! alias editor's "closing discards unapplied edits" convention, which
//! suits a formula table even better: `ParameterTable::set` validates (and
//! can reject) a formula immediately, so typing a not-yet-finished name or
//! formula must not touch the live table on every keystroke.
//!
//! Unlike the alias editor, each row also shows a live-computed resolved
//! value or error (`preview`, below) — built fresh from the whole buffer on
//! every render, not cached: the same "cheap enough to recompute from
//! scratch" philosophy `ParameterTable`/`sketch_solve.rs` already use
//! throughout this project, and it means the preview reflects circular/
//! undefined-reference problems across rows immediately, before Apply.

use crate::app::Message;
use crate::scene::named_parameters::ParameterTable;
use crate::scene::Scene;
use crate::t;
use acadrust::types::Handle;
use acadrust::EntityType;
use iced::widget::tooltip::Position as TipPos;
use iced::widget::{button, column, container, row, scrollable, text, text_input, tooltip, Space};
use iced::{Background, Element, Length, Theme};

/// Short "Kind handle" label for an entity without a user-facing name.
fn entity_label(scene: &Scene, handle: Handle) -> String {
    match scene.document.get_entity(handle) {
        Some(EntityType::Line(_)) => format!("Line {handle}"),
        Some(EntityType::Circle(_)) => format!("Circle {handle}"),
        Some(EntityType::Arc(_)) => format!("Arc {handle}"),
        Some(_) => format!("Entity {handle}"),
        None => format!("(erased {handle})"),
    }
}

/// One line per constraint currently driven by `name`, e.g.
/// `"Distance: Line 0x2A, Line 0x2B"` — empty when nothing references it
/// yet (a parameter defined but not yet used by any constraint).
fn usage_lines(scene: &Scene, name: &str) -> Vec<String> {
    scene
        .parameter_usage(name)
        .iter()
        .map(|u| {
            let entities = u
                .entities
                .iter()
                .map(|h| entity_label(scene, *h))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{:?}: {entities}", u.kind)
        })
        .collect()
}

/// The "Used by" column's cell for one row: a compact kind-count summary
/// (e.g. `"Distance ×2, Radius ×1"`), with the full per-constraint entity
/// list (`usage_lines`) as a hover tooltip — the column is too narrow to
/// show entity lists inline once a parameter drives more than one or two
/// constraints. A blank name (a fresh, not-yet-named row) or a name driving
/// nothing shows a muted em dash instead.
fn used_by_cell<'a>(scene: &Scene, name: &str) -> Element<'a, Message> {
    if name.is_empty() {
        return Space::new().into();
    }
    let lines = usage_lines(scene, name);
    if lines.is_empty() {
        return text("—").size(11).style(muted_style).into();
    }
    let mut counts: Vec<(String, usize)> = Vec::new();
    for line in &lines {
        let kind = line.split(':').next().unwrap_or(line).to_string();
        match counts.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, n)) => *n += 1,
            None => counts.push((kind, 1)),
        }
    }
    let summary = counts
        .iter()
        .map(|(k, n)| format!("{k} ×{n}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut body = column![].spacing(2);
    for line in &lines {
        body = body.push(text(line.clone()).size(11));
    }
    tooltip(
        text(summary).size(11).style(muted_style),
        container(body)
            .style(container::bordered_box)
            .padding([4, 8]),
        TipPos::Top,
    )
    .into()
}

/// Which column of a parameter row a text edit targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParamField {
    Name,
    Formula,
}

/// One working-buffer row. Free text until Apply — `ParameterTable::set`
/// does the real validation then, not on every keystroke.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParamEditorRow {
    pub name: String,
    pub formula: String,
}

/// Right-hand lane reserved for the scrollbar so it never overlaps the ✕
/// column — same convention and value as `alias_editor::GUTTER`.
const GUTTER: f32 = 16.0;

fn muted_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.68)),
    }
}

fn danger_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().danger.base.color),
    }
}

/// Every buffer index sharing a (trimmed) name with at least one other
/// non-blank row — `ParameterTable::set` upserts by name, so two rows typed
/// with the same name would otherwise silently collapse into "whichever one
/// happens to `set` last wins," discarding the other's formula with no
/// indication anything was lost. Both `preview` and the real Apply
/// (`OpenCADStudio::apply_named_parameter_editor_rows`) check this first and
/// refuse every row in a name collision rather than guessing which one the
/// user meant.
pub(crate) fn duplicate_name_rows(rows: &[ParamEditorRow]) -> std::collections::HashSet<usize> {
    let mut by_name: std::collections::HashMap<&str, Vec<usize>> = std::collections::HashMap::new();
    for (idx, row) in rows.iter().enumerate() {
        let name = row.name.trim();
        if !name.is_empty() {
            by_name.entry(name).or_default().push(idx);
        }
    }
    by_name
        .into_values()
        .filter(|idxs| idxs.len() > 1)
        .flatten()
        .collect()
}

/// Builds a scratch table from every row's *current* text and resolves each
/// row's own value against it, so the preview reflects cross-row references
/// (including a row referencing one defined further down — order in the
/// buffer doesn't matter, same as it doesn't for the real table). Returns
/// one `Result` per row, in row order:
/// - `Err` from a blank name is suppressed (`Ok` with no text shown instead,
///   handled by the caller) — a fresh row from "+ Add parameter" before the
///   user has typed a name yet isn't an error to flag.
/// - A name shared with another row is its own error (`duplicate_name_rows`)
///   before anything is even attempted.
/// - `Err` from `ParameterTable::set` itself (bad name, parse error, a
///   formula that would close a cycle right now) is that row's own problem.
/// - `Err(UndefinedReference)`/other resolve failure means the row's own
///   formula is fine but something *it* depends on isn't.
fn preview(rows: &[ParamEditorRow]) -> Vec<Option<Result<f64, String>>> {
    let duplicates = duplicate_name_rows(rows);
    let mut table = ParameterTable::new();
    let mut set_errors: Vec<Option<String>> = vec![None; rows.len()];
    for (idx, row) in rows.iter().enumerate() {
        let name = row.name.trim();
        if name.is_empty() || duplicates.contains(&idx) {
            continue;
        }
        if let Err(e) = table.set(name, row.formula.trim()) {
            set_errors[idx] = Some(e.to_string());
        }
    }
    rows.iter()
        .enumerate()
        .map(|(idx, row)| {
            let name = row.name.trim();
            if name.is_empty() {
                return None;
            }
            if duplicates.contains(&idx) {
                return Some(Err(format!("duplicate name '{name}'")));
            }
            if let Some(err) = &set_errors[idx] {
                return Some(Err(err.clone()));
            }
            Some(table.resolve(name).map_err(|e| e.to_string()))
        })
        .collect()
}

/// Build the named-parameter editor content. `rows` is the live working
/// buffer; `scene` supplies the "Used by" column, computed against the
/// *live*, already-applied constraint set (`Scene::parameter_usage`) —
/// unapplied edits to a row's name in the buffer don't retroactively
/// relabel what's shown, same as the rest of this editor only takes effect
/// on Apply.
pub fn view_window<'a>(
    rows: &'a [ParamEditorRow],
    scene: &'a Scene,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let title = text(t!("Named Parameters")).size(15);
    let hint = text(t!(
        "Type a name and a formula (e.g. hole_dia = 12, hole_spacing = 2 * hole_dia + 1.5). Apply to save and re-solve; closing discards unapplied edits."
    ))
    .size(11)
    .style(muted_style);

    let gutter = iced::Padding {
        top: 0.0,
        right: GUTTER,
        bottom: 0.0,
        left: 0.0,
    };
    const NAME_WIDTH: f32 = 120.0;
    const VALUE_WIDTH: f32 = 100.0;
    const USED_BY_WIDTH: f32 = 160.0;

    let head = container(
        row![
            container(text(t!("Name")).size(11).style(muted_style))
                .width(Length::Fixed(NAME_WIDTH)),
            container(text(t!("Formula")).size(11).style(muted_style)).width(sizing.width),
            container(text(t!("Value")).size(11).style(muted_style))
                .width(Length::Fixed(VALUE_WIDTH)),
            container(text(t!("Used by")).size(11).style(muted_style))
                .width(Length::Fixed(USED_BY_WIDTH)),
            Space::new().width(Length::Fixed(30.0)),
        ]
        .spacing(8),
    )
    .padding(gutter);

    let results = preview(rows);
    let mut list = column![].spacing(3);
    for (idx, row) in rows.iter().enumerate() {
        let name_box = text_input(t!("name").as_ref(), &row.name)
            .on_input(move |v| Message::NamedParametersInput {
                idx,
                field: ParamField::Name,
                value: v,
            })
            .size(13)
            .padding([3, 6])
            .width(Length::Fixed(NAME_WIDTH));
        let formula_box = text_input(t!("formula").as_ref(), &row.formula)
            .on_input(move |v| Message::NamedParametersInput {
                idx,
                field: ParamField::Formula,
                value: v,
            })
            .size(13)
            .padding([3, 6])
            .width(sizing.width);
        let value_cell: Element<'_, Message> = match results.get(idx).cloned().flatten() {
            Some(Ok(value)) => {
                container(text(format!("= {value:.4}")).size(12).style(muted_style)).into()
            }
            Some(Err(msg)) => container(text(msg).size(11).style(danger_style)).into(),
            None => Space::new().into(),
        };
        let used_by_cell = used_by_cell(scene, row.name.trim());
        let del = button(crate::ui::icons::themed_danger_text(
            crate::ui::icons::CLOSE,
            12.0,
        ))
        .on_press(Message::NamedParametersRemove(idx))
        .padding([2, 6])
        .style(button::danger);
        list = list.push(
            row![
                name_box,
                formula_box,
                container(value_cell).width(Length::Fixed(VALUE_WIDTH)),
                container(used_by_cell).width(Length::Fixed(USED_BY_WIDTH)),
                del
            ]
            .spacing(8)
            .align_y(iced::Center),
        );
    }

    let add = button(text(t!("+ Add parameter")).size(12))
        .on_press(Message::NamedParametersAdd)
        .padding([4, 10])
        .style(button::secondary);

    let apply = button(text(t!("Apply")).size(12))
        .on_press(Message::NamedParametersApply)
        .padding([4, 16])
        .style(button::primary);

    container(
        column![
            title,
            hint,
            Space::new().height(6),
            head,
            scrollable(container(list).padding(gutter)).height(sizing.height),
            Space::new().height(6),
            row![add, Space::new().width(sizing.width), apply].align_y(iced::Center),
        ]
        .spacing(6)
        .width(sizing.width)
        .height(sizing.height),
    )
    .padding(12)
    .width(sizing.width)
    .height(sizing.height)
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.base.color)),
        ..Default::default()
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, formula: &str) -> ParamEditorRow {
        ParamEditorRow {
            name: name.to_string(),
            formula: formula.to_string(),
        }
    }

    #[test]
    fn preview_resolves_a_literal_and_a_cross_row_reference() {
        let rows = vec![
            row("hole_dia", "5"),
            row("hole_spacing", "2 * hole_dia + 1.5"),
        ];
        let results = preview(&rows);
        assert_eq!(results[0], Some(Ok(5.0)));
        assert_eq!(results[1], Some(Ok(11.5)));
    }

    #[test]
    fn preview_resolves_a_forward_reference_regardless_of_row_order() {
        // `plate_width` (row 0) references `plate_len` (row 1) -- defined
        // *after* it in the buffer. Order in the editor must not matter, same
        // as it doesn't for the real `ParameterTable`.
        let rows = vec![row("plate_width", "plate_len / 2"), row("plate_len", "20")];
        let results = preview(&rows);
        assert_eq!(results[0], Some(Ok(10.0)));
        assert_eq!(results[1], Some(Ok(20.0)));
    }

    #[test]
    fn preview_reports_a_cycle_on_the_row_that_closes_it() {
        let rows = vec![row("a", "b"), row("b", "a")];
        let results = preview(&rows);
        // Row 0 (a = b) is accepted first (b doesn't exist yet, no cycle);
        // row 1 (b = a) is the one that would close the loop and is rejected.
        assert!(
            matches!(results[0], Some(Ok(_)))
                || matches!(&results[0], Some(Err(e)) if e.contains("not defined"))
        );
        assert!(matches!(&results[1], Some(Err(e)) if e.to_lowercase().contains("circular")));
    }

    #[test]
    fn preview_leaves_a_blank_row_without_a_name_unflagged() {
        let rows = vec![row("", "")];
        assert_eq!(preview(&rows), vec![None]);
    }

    #[test]
    fn preview_reports_a_malformed_formula_on_its_own_row() {
        let rows = vec![row("bad", "1 +")];
        let results = preview(&rows);
        assert!(matches!(&results[0], Some(Err(e)) if e.contains("formula error")));
    }

    #[test]
    fn preview_flags_every_row_sharing_a_duplicate_name_instead_of_picking_one() {
        let rows = vec![row("x", "1"), row("x", "2"), row("y", "3")];
        let results = preview(&rows);
        assert!(matches!(&results[0], Some(Err(e)) if e.contains("duplicate")));
        assert!(matches!(&results[1], Some(Err(e)) if e.contains("duplicate")));
        assert_eq!(
            results[2],
            Some(Ok(3.0)),
            "an unrelated row's name must not be affected"
        );
    }

    #[test]
    fn usage_lines_names_the_constraint_kind_and_its_entities() {
        use crate::scene::sketch_constraints::{ConstraintKind, SketchRef, SketchScope};
        let mut scene = Scene::new();
        let line = scene.add_entity(acadrust::EntityType::Line(
            acadrust::entities::Line::from_points(
                acadrust::types::Vector3::new(0.0, 0.0, 0.0),
                acadrust::types::Vector3::new(10.0, 0.0, 0.0),
            ),
        ));
        scene
            .sketch_constraint_set_mut(SketchScope::ModelSpace)
            .add(
                ConstraintKind::Distance,
                vec![SketchRef::point(line, 0), SketchRef::point(line, 1)],
                Some(crate::scene::named_parameters::DrivingValue::Named(
                    "gap".to_string(),
                )),
            );

        let lines = usage_lines(&scene, "gap");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("Distance: Line "), "got {lines:?}");
        assert!(
            lines[0].contains(&format!("{line}")),
            "should name the actual entity handle, got {lines:?}"
        );
    }

    #[test]
    fn usage_lines_is_empty_for_a_parameter_nothing_references() {
        let scene = Scene::new();
        assert!(usage_lines(&scene, "unused").is_empty());
    }

    #[test]
    fn entity_label_reports_erased_for_a_dangling_handle() {
        let scene = Scene::new();
        let label = entity_label(&scene, acadrust::types::Handle::new(999));
        assert!(label.starts_with("(erased "), "got {label}");
    }
}
