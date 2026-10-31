//! One validated Apply / Save path and the editor's undo history. A failed
//! commit keeps the draft and says why; an unchanged one changes nothing.
use super::*;

/// Every editor value one undo step brings back, not only the text.
macro_rules! editor_history {
    ($($field:ident: $ty:ty),* $(,)?) => {
        #[derive(Clone, Debug, PartialEq)]
        pub(super) struct EditorSnapshot { raw: String, $($field: $ty),* }
        impl MTextEditorState {
            fn snapshot(&self) -> EditorSnapshot {
                EditorSnapshot { raw: self.content.text(), $($field: self.$field.clone()),* }
            }
            fn restore(&mut self, snapshot: EditorSnapshot) {
                self.content = text_editor::Content::with_text(&snapshot.raw);
                $(self.$field = snapshot.$field;)*
                self.caret_blink_on = true;
            }
        }
    }
}
editor_history! {
    height: String, rotation: String, rotation_dirty: bool,
    style: String, font: String, width: String, object_style: super::raw_style::ObjectStyle,
    object_color: Option<codec::types::Color>, color_aci: u16, oblique: String, char_space: String,
    annotative: bool, attachment: AttachmentPoint, line_spacing: f32, rect_width: f64,
    line_spacing_style: codec::entities::LineSpacingStyle, line_spacing_draft: Option<String>,
    mask_draft: Option<super::entity_options::MaskDraft>,
    direction: Option<codec::entities::DrawingDirection>,
    sel: Option<(usize, usize)>, caret: usize, sel_anchor: usize,
    typing_format: super::raw_edit::TypingFormat, rect_width_draft: Option<String>,
}

impl MTextEditorState {
    /// Record the state before a change, as one undo step.
    pub(in crate::app) fn record_undo(&mut self) {
        let snapshot = self.snapshot();
        if self.undo.last() != Some(&snapshot) {
            self.undo.push(snapshot);
            if self.undo.len() > 100 {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
    }

    /// The colour the whole text is drawn with: the object's own, or a
    /// whole-text colour where the target has no object colour.
    pub(in crate::app) fn whole_text_color(&self) -> codec::types::Color {
        self.object_color.unwrap_or_else(|| {
            if matches!(self.color_aci, 0 | 256) {
                self.original
                    .as_ref()
                    .map_or(codec::types::Color::ByLayer, |m| m.common.color)
            } else {
                codec::types::Color::from_index(self.color_aci as i16)
            }
        })
    }

    /// Why the draft cannot be saved yet: a field that is not a valid number.
    fn draft_error(&self) -> Option<&'static str> {
        let positive = |s: &str| {
            s.trim()
                .replace(',', ".")
                .parse::<f64>()
                .is_ok_and(|v| v.is_finite() && v > 0.0)
        };
        let valid = positive(&self.height)
            && positive(&self.width)
            && self.rotation_input_value().is_some();
        if !valid {
            return Some("MTEXT: enter a valid height, width factor and rotation before saving.");
        }
        self.mask_scale_value()
            .is_none()
            .then_some("MTEXT: the mask border offset factor must be from 1 to 5.")
    }
}

impl MTextEditorState {
    /// What the status bar's box-width field shows.
    pub(in crate::app) fn rect_width_field(&self) -> String {
        self.rect_width_draft
            .clone()
            .unwrap_or_else(|| super::raw_span::number(self.rect_width))
    }

    /// Characters in the text (a fraction counts its glyphs), not counting
    /// paragraph breaks.
    pub(in crate::app) fn visible_character_count(&self) -> usize {
        doc_to_cells(&self.doc)
            .iter()
            .filter(|c| !matches!(c, Cell::Break(..)))
            .count()
    }

    /// Which way the box grows from its attachment point: 1 rightwards,
    /// 2 both ways (centred), -1 leftwards.
    pub(in crate::app) fn box_growth(&self) -> f32 {
        use AttachmentPoint as A;
        match self.attachment {
            A::TopCenter | A::MiddleCenter | A::BottomCenter => 2.0,
            A::TopRight | A::MiddleRight | A::BottomRight => -1.0,
            _ => 1.0,
        }
    }
}

impl crate::app::OpenCADStudio {
    /// The status bar's box-width field: kept as typed; a positive number
    /// sets the box width, as the command's box does.
    pub(in crate::app) fn mtext_rect_width_field(&mut self, value: String) {
        let Some(ed) = self.mtext_editor.as_mut() else {
            return;
        };
        match value
            .trim()
            .replace(',', ".")
            .parse::<f64>()
            .ok()
            .filter(|w| w.is_finite() && *w > 0.0)
        {
            Some(width) => {
                ed.record_undo();
                ed.rect_width = width;
                ed.keep_width = true;
                ed.rect_width_draft = (super::raw_span::number(width) != value).then_some(value);
            }
            None => ed.rect_width_draft = Some(value),
        }
        self.rebuild_mtext_preview();
    }

    /// A ruler drag: `Some` width while it moves, `None` when it ends. The
    /// whole drag is one undo step.
    pub(in crate::app) fn mtext_box_drag(&mut self, width: Option<f64>) {
        let Some(ed) = self.mtext_editor.as_mut() else {
            return;
        };
        let Some(width) = width.filter(|w| w.is_finite() && *w > 0.0) else {
            ed.box_dragging = false;
            return;
        };
        if !ed.box_dragging {
            ed.record_undo();
            ed.box_dragging = true;
        }
        ed.rect_width = width;
        ed.rect_width_draft = None;
        ed.keep_width = true;
        self.rebuild_mtext_preview();
    }

    /// The spacing factor typed in the Paragraph menu: kept as typed; a
    /// factor from 0.25 to 4 applies at once, as AutoCAD accepts.
    pub(in crate::app) fn mtext_line_spacing_input(&mut self, value: String) {
        let Some(ed) = self.mtext_editor.as_mut() else {
            return;
        };
        match value
            .trim()
            .replace(',', ".")
            .parse::<f32>()
            .ok()
            .filter(|f| (0.25..=4.0).contains(f))
        {
            Some(factor) => {
                ed.record_undo();
                ed.line_spacing = factor;
                ed.line_spacing_draft =
                    (super::raw_span::number(f64::from(factor)) != value).then_some(value);
            }
            None => ed.line_spacing_draft = Some(value),
        }
        self.rebuild_mtext_preview();
    }

    /// At least the factor (`false`) or exactly it (`true`).
    pub(in crate::app) fn mtext_line_spacing_mode(&mut self, exact: bool) {
        use codec::entities::LineSpacingStyle as S;
        let Some(ed) = self.mtext_editor.as_mut() else {
            return;
        };
        let style = if exact { S::Exactly } else { S::AtLeast };
        if ed.line_spacing_style != style {
            ed.record_undo();
            ed.line_spacing_style = style;
        }
        self.rebuild_mtext_preview();
    }

    pub(in crate::app) fn mtext_undo(&mut self) {
        if let Some(ed) = self.mtext_editor.as_mut() {
            if let Some(previous) = ed.undo.pop() {
                ed.redo.push(ed.snapshot());
                ed.restore(previous);
            }
        }
        self.rebuild_mtext_preview();
    }

    pub(in crate::app) fn mtext_redo(&mut self) {
        if let Some(ed) = self.mtext_editor.as_mut() {
            if let Some(next) = ed.redo.pop() {
                ed.undo.push(ed.snapshot());
                ed.restore(next);
            }
        }
        self.rebuild_mtext_preview();
    }

    fn mtext_commit_error(&mut self, message: &str) {
        self.command_line.push_error(crate::t!(message).as_ref());
    }

    /// Save the draft to the drawing: a new MTEXT is created, an edited one
    /// changes only when something differs. `keep_open` is Apply: the editor
    /// stays on the saved entity. Returns whether the draft was saved; when
    /// it was not, the editor and its draft stay as they were, except that
    /// closing an empty new text discards it.
    pub(super) fn commit_mtext_draft(&mut self, keep_open: bool) -> bool {
        let i = self.active_tab;
        let Some(ed) = self.mtext_editor.as_ref() else {
            return false;
        };
        if let Some(error) = ed.draft_error() {
            self.mtext_commit_error(error);
            return false;
        }
        if self.command_mtext_input {
            if keep_open {
                self.rebuild_mtext_preview();
            } else {
                self.pending_command_editor_text = Some(ed.build_mtext().value);
                self.command_mtext_input = false;
                self.mtext_editor = None;
                self.refresh_properties();
            }
            return true;
        }
        let editing = ed.editing;
        let keep_width = ed.keep_width;
        let empty = ed.content.text().trim().is_empty();
        let mut mt = ed.build_mtext();
        if empty {
            if editing.is_some() {
                self.mtext_commit_error(
                    "MTEXT: an existing entity cannot be emptied here; use the drawing's delete command.",
                );
            } else if !keep_open {
                self.mtext_editor = None;
                self.refresh_properties();
            }
            return false;
        }
        let annotative = mt.is_annotative
            || editing.is_none()
                && crate::scene::annotative::text_style_is_annotative(
                    &self.tabs[i].scene.document,
                    &mt.style,
                );
        mt.is_annotative = annotative;
        let handle = if let Some(h) = editing {
            if self.tabs[i].scene.is_layer_locked(h) {
                self.mtext_commit_error(
                    "MTEXT: the target layer is locked; the draft was not saved.",
                );
                return false;
            }
            let (unchanged, annotation_changed) = match self.tabs[i].scene.document.get_entity(h) {
                Some(EntityType::MText(t)) => (
                    t.value == mt.value
                        && t.height == mt.height
                        && t.rotation == mt.rotation
                        && t.dwg_x_direction == mt.dwg_x_direction
                        && t.common.color == mt.common.color
                        && t.style == mt.style
                        && t.attachment_point == mt.attachment_point
                        && t.line_spacing_factor == mt.line_spacing_factor
                        && t.line_spacing_style == mt.line_spacing_style
                        && t.rectangle_width == mt.rectangle_width
                        && t.rectangle_height == mt.rectangle_height
                        && t.column_data == mt.column_data
                        && t.background_fill_flags == mt.background_fill_flags
                        && t.background_scale == mt.background_scale
                        && t.background_color == mt.background_color
                        && t.drawing_direction == mt.drawing_direction
                        && t.is_annotative == mt.is_annotative,
                    t.is_annotative != mt.is_annotative,
                ),
                Some(EntityType::MultiLeader(ml)) => (
                    ml.context.text_string == mt.value
                        && ml.context.text_height == mt.height
                        && ml.context.line_spacing_factor == mt.line_spacing_factor
                        && ml.context.line_spacing_style == mt.line_spacing_style
                        && !(keep_width
                            && mt.rectangle_width > 0.0
                            && ml.context.text_width != mt.rectangle_width),
                    false,
                ),
                _ => {
                    self.mtext_commit_error(
                        "MTEXT: the target entity no longer exists; the draft was not saved.",
                    );
                    return false;
                }
            };
            if !unchanged {
                self.push_undo_snapshot(i, "MTEXT");
                match self.tabs[i].scene.document.get_entity_mut(h) {
                    Some(EntityType::MText(t)) => {
                        t.value = mt.value;
                        t.height = mt.height;
                        t.rotation = mt.rotation;
                        t.dwg_x_direction = mt.dwg_x_direction;
                        t.common.color = mt.common.color;
                        t.style = mt.style;
                        t.attachment_point = mt.attachment_point;
                        t.line_spacing_factor = mt.line_spacing_factor;
                        t.line_spacing_style = mt.line_spacing_style;
                        t.rectangle_width = mt.rectangle_width;
                        t.rectangle_height = mt.rectangle_height;
                        t.column_data = mt.column_data;
                        t.background_fill_flags = mt.background_fill_flags;
                        t.background_scale = mt.background_scale;
                        t.background_color = mt.background_color;
                        t.drawing_direction = mt.drawing_direction;
                    }
                    Some(EntityType::MultiLeader(ml)) => {
                        ml.context.text_string = mt.value;
                        ml.context.text_height = mt.height;
                        ml.context.line_spacing_factor = mt.line_spacing_factor;
                        ml.context.line_spacing_style = mt.line_spacing_style;
                        if keep_width && mt.rectangle_width > 0.0 {
                            ml.context.text_width = mt.rectangle_width;
                        }
                    }
                    _ => unreachable!("target checked before the change"),
                }
                // The annotation scales change only with the annotative flag.
                if annotation_changed {
                    crate::scene::annotative::set_entity_annotative(
                        &mut self.tabs[i].scene.document,
                        h,
                        annotative,
                    );
                    if annotative {
                        if let Some(scale) = self.tabs[i].scene.creation_annotation_scale_handle() {
                            crate::scene::annotative::create_annotation_context(
                                &mut self.tabs[i].scene.document,
                                h,
                                scale,
                            );
                        }
                    }
                }
                self.tabs[i].scene.sync_displayed_annotation_context(h);
                self.tabs[i]
                    .scene
                    .bump_entities(&[(h, crate::scene::ChangeKind::Modified)]);
                self.tabs[i].dirty = true;
            }
            h
        } else {
            let layer = self
                .text_layer(i)
                .unwrap_or_else(|| self.tabs[i].active_layer.clone());
            if self.tabs[i]
                .scene
                .document
                .layers
                .get(&layer)
                .is_some_and(|l| l.is_locked())
            {
                self.mtext_commit_error(
                    "MTEXT: the target layer is locked; the draft was not saved.",
                );
                return false;
            }
            let plane = if self.tabs[i].editing_model_space() {
                self.tabs[i].ucs_xform().working_plane()
            } else {
                crate::command::WorkingPlane::default()
            };
            let position = plane.to_local(DVec3::new(
                mt.insertion_point.x,
                mt.insertion_point.y,
                mt.insertion_point.z,
            ));
            mt.insertion_point = Vector3::new(position.x, position.y, position.z);
            self.push_undo_snapshot(i, "MTEXT");
            let Some(h) = self.commit_new_text(plane.place_entity(EntityType::MText(Box::new(mt))))
            else {
                self.mtext_commit_error("MTEXT: creation failed; the draft was retained.");
                return false;
            };
            if annotative {
                if let Some(scale) = self.tabs[i].scene.creation_annotation_scale_handle() {
                    crate::scene::annotative::create_annotation_context(
                        &mut self.tabs[i].scene.document,
                        h,
                        scale,
                    );
                }
            }
            self.tabs[i].dirty = true;
            h
        };
        if keep_open {
            // The editor now edits what was saved: the next Apply compares
            // against it, and undo starts over from it.
            let original = match self.tabs[i].scene.document.get_entity(handle) {
                Some(EntityType::MText(t)) => Some((**t).clone()),
                _ => None,
            };
            let ed = self.mtext_editor.as_mut().unwrap();
            ed.editing = Some(handle);
            if let Some(original) = original {
                let degrees = format!("{:.6}", original.rotation.to_degrees());
                ed.rotation = degrees
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string();
                ed.original = Some(original);
            }
            ed.rotation_dirty = false;
            ed.object_color = None;
            ed.mask_draft = None;
            ed.direction = None;
            ed.undo.clear();
            ed.redo.clear();
            self.rebuild_mtext_preview();
        } else {
            self.mtext_editor = None;
        }
        self.refresh_properties();
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{Message, OpenCADStudio};
    use codec::{EntityType, Handle, MText};
    use glam::DVec3;

    fn app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs
            .push(crate::app::document::DocumentTab::new_drawing(1));
        app.active_tab = 1;
        app
    }
    fn with_text(raw: &str) -> (OpenCADStudio, Handle) {
        let mut app = app();
        let mut mt = MText::with_value(raw, codec::types::Vector3::new(0., 0., 0.));
        mt.common.color = codec::types::Color::Index(5);
        mt.height = 2.5;
        let h = app.tabs[1]
            .scene
            .document
            .add_entity(EntityType::MText(Box::new(mt)))
            .unwrap();
        app.open_mtext_editor(DVec3::ZERO, Some(h), raw, 2.5, None);
        (app, h)
    }
    fn mt_of(app: &OpenCADStudio, h: Handle) -> &MText {
        let Some(EntityType::MText(mt)) = app.tabs[1].scene.document.get_entity(h) else {
            panic!()
        };
        mt
    }
    fn mtexts(app: &OpenCADStudio) -> usize {
        app.tabs[1]
            .scene
            .document
            .entities()
            .filter(|e| matches!(e, EntityType::MText(_)))
            .count()
    }

    #[test]
    fn an_unchanged_apply_changes_nothing_in_the_drawing() {
        let (mut app, h) = with_text(r"\pxqc;ab");
        let before = mt_of(&app, h).clone();
        app.tabs[1].dirty = false;
        app.mtext_apply();
        assert!(!app.tabs[1].dirty);
        assert_eq!(mt_of(&app, h), &before);
    }

    #[test]
    fn an_invalid_field_keeps_the_draft_open_and_the_entity_as_it_was() {
        let (mut app, h) = with_text("ab");
        let before = mt_of(&app, h).clone();
        app.mtext_type("c");
        let _ = app.update(Message::MTextHeight("x".into()));
        let _ = app.update(Message::MTextOk);
        assert!(app.mtext_editor.is_some());
        assert_eq!(mt_of(&app, h), &before);
        assert_eq!(app.mtext_editor.as_ref().unwrap().content.text(), "abc");
        // Corrected, it saves and closes.
        let _ = app.update(Message::MTextHeight("3".into()));
        let _ = app.update(Message::MTextOk);
        assert!(app.mtext_editor.is_none());
        assert_eq!(mt_of(&app, h).value, "abc");
        assert_eq!(mt_of(&app, h).height, 3.0);
    }

    #[test]
    fn an_existing_text_is_not_emptied_and_a_new_empty_one_is_dropped() {
        let (mut app, h) = with_text("ab");
        let ed = app.mtext_editor.as_mut().unwrap();
        ed.sel = Some((0, 2));
        app.mtext_delete();
        assert!(!app.mtext_commit());
        assert!(app.mtext_editor.is_some());
        assert_eq!(mt_of(&app, h).value, "ab");
        let mut app = self::app();
        app.open_mtext_editor(DVec3::ZERO, None, "", 2.5, None);
        assert!(!app.mtext_commit());
        assert!(app.mtext_editor.is_none());
        assert_eq!(mtexts(&app), 0);
    }

    #[test]
    fn repeated_apply_on_a_new_text_updates_the_one_it_created() {
        let mut app = app();
        app.open_mtext_editor(DVec3::ZERO, None, "a", 2.5, None);
        app.mtext_apply();
        app.mtext_type("b");
        app.mtext_apply();
        assert_eq!(mtexts(&app), 1);
        let h = app.mtext_editor.as_ref().unwrap().editing.unwrap();
        assert_eq!(mt_of(&app, h).value, "ab");
    }

    #[test]
    fn whole_text_colour_is_the_entity_colour_and_the_text_is_not_rewritten() {
        let (mut app, h) = with_text(r"\pxqc,t4;a{\C1;b}");
        let _ = app.update(Message::MTextColorChanged(codec::types::Color::Index(3)));
        assert_eq!(
            app.mtext_editor.as_ref().unwrap().whole_text_color(),
            codec::types::Color::Index(3)
        );
        assert!(app.mtext_commit());
        assert_eq!(mt_of(&app, h).common.color, codec::types::Color::Index(3));
        assert_eq!(mt_of(&app, h).value, r"\pxqc,t4;a{\C1;b}");
    }

    #[test]
    fn one_undo_brings_back_every_value_of_the_step() {
        let (mut app, h) = with_text("ab");
        let _ = app.update(Message::MTextRotation("45".into()));
        let _ = app.update(Message::MTextColorChanged(codec::types::Color::Index(3)));
        app.mtext_type("c");
        app.mtext_undo();
        assert_eq!(app.mtext_editor.as_ref().unwrap().content.text(), "ab");
        app.mtext_undo();
        assert_eq!(app.mtext_editor.as_ref().unwrap().object_color, None);
        app.mtext_undo();
        let ed = app.mtext_editor.as_ref().unwrap();
        assert_eq!(ed.rotation, "0");
        assert!(!ed.rotation_dirty);
        app.mtext_redo();
        app.mtext_redo();
        app.mtext_redo();
        assert!(app.mtext_commit());
        let mt = mt_of(&app, h);
        assert_eq!(mt.value, "abc");
        assert_eq!(mt.common.color, codec::types::Color::Index(3));
        assert!((mt.rotation.to_degrees() - 45.0).abs() < 1e-9);
    }

    #[test]
    fn a_locked_layer_keeps_the_draft() {
        let (mut app, h) = with_text("ab");
        let layer = mt_of(&app, h).common.layer.clone();
        app.tabs[1]
            .scene
            .document
            .layers
            .get_mut(&layer)
            .unwrap()
            .lock();
        app.mtext_type("c");
        assert!(!app.mtext_commit());
        assert!(app.mtext_editor.is_some());
        assert_eq!(mt_of(&app, h).value, "ab");
    }
}
