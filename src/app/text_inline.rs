// In-place single-line TEXT editor: a plain text-entry box anchored at the
// insertion-point click. Unlike MText, plain-text entities carry no inline
// formatting, so there is no toolbar or rich preview — just a field the user
// types into, committed on Enter.
//
// This module also hosts `begin_text_edit`, the shared router that opens the
// right in-place editor (this plain box, or the rich MText editor) for any
// text-bearing entity, plus the field read/write helpers both editors use to
// commit back to the correct entity slot.

use acadrust::types::Vector3;
use acadrust::{EntityType, Handle, Text};
use glam::DVec3;

/// Which text slot of which entity an editor session reads from and writes to.
/// `Text`/`AttDef`/`AttEnt`/`Dim`/`Tolerance` are plain (single-line box);
/// `MText`/`MLeader` are rich (MText editor).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TextEntityField {
    /// `Text.value`.
    Text,
    /// `AttributeDefinition.default_value`.
    AttDef,
    /// `AttributeEntity` value.
    AttEnt,
    /// `Dimension` text override (`base.text`).
    Dim,
    /// `Tolerance.text` (feature-control-frame string).
    Tolerance,
    /// `MText.value`.
    MText,
    /// `MultiLeader.context.text_string`.
    MLeader,
}

impl TextEntityField {
    /// True for fields edited with the rich MText editor.
    pub fn is_rich(self) -> bool {
        matches!(self, TextEntityField::MText | TextEntityField::MLeader)
    }
}

/// Read the current text of an entity's editable slot, or `None` if the entity
/// carries no editable text.
pub fn read_text_field(entity: &EntityType) -> Option<(String, TextEntityField)> {
    match entity {
        EntityType::Text(t) => Some((t.value.clone(), TextEntityField::Text)),
        EntityType::MText(m) => Some((m.value.clone(), TextEntityField::MText)),
        EntityType::AttributeDefinition(a) => {
            Some((a.default_value.clone(), TextEntityField::AttDef))
        }
        EntityType::AttributeEntity(a) => {
            Some((a.get_value().to_string(), TextEntityField::AttEnt))
        }
        EntityType::Dimension(d) => Some((d.base().text.clone(), TextEntityField::Dim)),
        EntityType::Tolerance(t) => Some((t.text.clone(), TextEntityField::Tolerance)),
        EntityType::MultiLeader(ml) => {
            Some((ml.context.text_string.clone(), TextEntityField::MLeader))
        }
        _ => None,
    }
}

/// Write `value` into an entity's editable slot. Returns true on a match.
pub fn write_text_field(entity: &mut EntityType, field: TextEntityField, value: String) -> bool {
    match (entity, field) {
        (EntityType::Text(t), TextEntityField::Text) => t.value = value,
        (EntityType::MText(m), TextEntityField::MText) => m.value = value,
        (EntityType::AttributeDefinition(a), TextEntityField::AttDef) => a.default_value = value,
        (EntityType::AttributeEntity(a), TextEntityField::AttEnt) => a.set_value(value),
        (EntityType::Dimension(d), TextEntityField::Dim) => d.base_mut().text = value,
        (EntityType::Tolerance(t), TextEntityField::Tolerance) => t.text = value,
        (EntityType::MultiLeader(ml), TextEntityField::MLeader) => {
            ml.context.text_string = value;
        }
        _ => return false,
    }
    true
}

fn vec3(v: Vector3) -> DVec3 {
    DVec3::new(v.x, v.y, v.z)
}

/// Live state of the open in-place TEXT editor. Absent (`None`) when no editor
/// is up.
pub struct TextInlineState {
    /// World insertion point (WCS, same convention the committed entity uses).
    pub pos: DVec3,
    /// The plain text being entered.
    pub value: String,
    /// Text height (drawing units), used when creating a new TEXT entity.
    pub height: f64,
    /// `Some` when editing an existing entity; `None` for a fresh TEXT.
    pub editing: Option<Handle>,
    /// Which entity slot this session writes to on commit.
    pub field: TextEntityField,
    /// Fully prepared entity supplied by the interactive TEXT command. Editing
    /// existing text and legacy direct-open paths leave this empty.
    pub creation: Option<Text>,
    /// Canvas-space anchor where the field is drawn (the insertion-point click).
    pub screen_anchor: iced::Point,
}
pub(super) fn can_edit_text(mut handle: Handle, document: &acadrust::CadDocument) -> bool {
    for _ in 0..8 {
        match document.get_entity(handle) {
            Some(acadrust::EntityType::Leader(l)) => {
                let ann = l.annotation_handle;
                if ann.is_null() || ann == handle {
                    return false;
                }
                handle = ann;
            }
            _ => break,
        }
    }
    if let Some(entity) = document.get_entity(handle) {
        read_text_field(entity).is_some()
    } else {
        false
    }
}

impl super::OpenCADStudio {
    /// Open the right in-place editor for `handle`: the plain box for single-
    /// line text entities, the rich MText editor for MText / MultiLeader. A
    /// Leader resolves to the entity it annotates. Returns the focus task for
    /// the plain box (the rich editor needs no field focus).
    pub(super) fn begin_text_edit(&mut self, handle: Handle) -> iced::Task<super::Message> {
        let i = self.active_tab;
        if self.tabs[i].scene.is_layer_locked(handle) {
            return iced::Task::none();
        }
        // Resolve a Leader chain to the annotated entity.
        let mut target = handle;
        for _ in 0..8 {
            match self.tabs[i].scene.document.get_entity(target) {
                Some(EntityType::Leader(l)) => {
                    let ann = l.annotation_handle;
                    if ann.is_null() || ann == target {
                        return iced::Task::none();
                    }
                    target = ann;
                }
                _ => break,
            }
        }
        if self.tabs[i].scene.is_layer_locked(target) {
            return iced::Task::none();
        }
        // Snapshot what we need before borrowing `self` mutably to open.
        let Some(entity) = self.tabs[i].scene.document.get_entity(target) else {
            return iced::Task::none();
        };
        let Some((value, field)) = read_text_field(entity) else {
            return iced::Task::none();
        };
        let (pos, height) = match entity {
            EntityType::Text(t) => (vec3(t.insertion_point), t.height),
            EntityType::MText(m) => (vec3(m.insertion_point), m.height),
            EntityType::AttributeDefinition(a) => (vec3(a.insertion_point), a.height),
            EntityType::AttributeEntity(a) => (vec3(a.insertion_point), a.height),
            EntityType::Dimension(d) => (vec3(d.base().insertion_point), 0.25),
            EntityType::Tolerance(t) => (vec3(t.insertion_point), t.text_height),
            EntityType::MultiLeader(ml) => {
                (vec3(ml.context.text_location), ml.context.text_height)
            }
            _ => (DVec3::ZERO, 0.25),
        };

        if field.is_rich() {
            self.open_mtext_editor(pos, Some(target), &value, height, None);
            self.unfocus_widgets()
        } else {
            self.open_text_inline(pos, Some(target), &value, height, field, None);
            iced::widget::operation::focus(iced::widget::Id::new(super::view::TEXT_INLINE_ID))
        }
    }

    /// Open the in-place plain-text editor at `pos`, writing to `field` on
    /// commit. `handle` is `Some` when editing an existing entity.
    pub(super) fn open_text_inline(
        &mut self,
        pos: DVec3,
        handle: Option<Handle>,
        initial: &str,
        height: f64,
        field: TextEntityField,
        creation: Option<Text>,
    ) {
        if handle.is_some_and(|h| self.tabs[self.active_tab].scene.is_layer_locked(h)) {
            return;
        }
        let mut state = TextInlineState {
            pos,
            value: initial.to_string(),
            height: if height > 0.0 { height } else { 0.25 },
            editing: handle,
            field,
            creation,
            screen_anchor: iced::Point::new(60.0, 90.0),
        };
        if let Some(p) = self.tabs[self.active_tab].scene.selection.borrow().last_move_pos {
            state.screen_anchor = p;
        }
        self.text_inline = Some(state);
    }

    /// Commit the editor: create a new TEXT entity or update the edited slot.
    /// Empty content drops a new entity and leaves an edited one untouched.
    pub(super) fn text_inline_commit(&mut self) -> bool {
        let i = self.active_tab;
        let Some(ed) = self.text_inline.take() else { return false };
        if ed.value.trim().is_empty() && ed.editing.is_none() {
            self.refresh_properties();
            return false;
        }
        if let Some(h) = ed.editing {
            if self.tabs[i].scene.is_layer_locked(h) {
                self.refresh_properties();
                return false;
            }
            self.push_undo_snapshot(i, "TEXT");
            if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(h) {
                write_text_field(entity, ed.field, ed.value.clone());
            }
            self.tabs[i]
                .scene
                .bump_entities(&[(h, crate::scene::ChangeKind::Modified)]);
            self.tabs[i].dirty = true;
        } else {
            let plane = if self.tabs[i].editing_model_space() {
                self.tabs[i].ucs_xform().working_plane()
            } else {
                crate::command::WorkingPlane::default()
            };
            let command_creation = ed.creation.is_some();
            let mut t = if let Some(mut prepared) = ed.creation {
                prepared.value = ed.value.clone();
                prepared
            } else {
                let position = plane.to_local(ed.pos);
                Text::with_value(
                    &ed.value,
                    Vector3::new(position.x, position.y, position.z),
                )
                .with_height(ed.height)
            };
            // New text inherits the document's current text style (STYLE), not
            // the entity default. See #92.
            let cur_style = self.tabs[i]
                .scene
                .document
                .header
                .current_text_style_name
                .clone();
            if !cur_style.is_empty() {
                if t.style.trim().is_empty() {
                    t.style = cur_style;
                }
            }
            if command_creation {
                let annotation_multiplier = if crate::scene::annotative::text_style_is_annotative(
                    &self.tabs[i].scene.document,
                    &t.style,
                ) {
                    self.tabs[i].scene.creation_annotation_multiplier()
                } else {
                    1.0
                };
                let display_height = crate::entities::text::text_run_placement_at_scale(
                    &t,
                    &self.tabs[i].scene.document,
                    annotation_multiplier as f32,
                )
                .height as f64
                    * annotation_multiplier;
                if let Some(command) = self.tabs[i].suspended_cmd.as_mut() {
                    command.on_editor_display_height(display_height);
                }
            }
            self.push_undo_snapshot(i, "TEXT");
            self.tabs[i].scene.document.header.current_text_style_name = t.style.clone();
            let variable_height = self.tabs[i]
                .scene
                .document
                .text_styles
                .iter()
                .find(|style| style.name.eq_ignore_ascii_case(&t.style))
                .is_none_or(|style| style.height <= 1.0e-9);
            if variable_height
                && !matches!(
                    t.horizontal_alignment,
                    acadrust::entities::TextHorizontalAlignment::Aligned
                )
            {
                self.tabs[i].scene.document.header.text_height = t.height;
            }
            let _ = self.commit_entity_handle(plane.place_entity(EntityType::Text(t)));
            self.tabs[i].dirty = true;
        }
        self.refresh_properties();
        true
    }

    /// Discard the editor without changing the drawing.
    pub(super) fn text_inline_cancel(&mut self) {
        self.text_inline = None;
    }
}
