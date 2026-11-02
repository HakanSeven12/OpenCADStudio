//! A text style from the selected characters (#2413): "New style from
//! selection" and "Update <style> from selection" in the editor's style
//! menu. Only what a text style stores is read: the font with its bold and
//! italic, the height, the width factor and the oblique angle. A value the
//! characters do not share stays as the style has it.
use super::raw_adapter::EditError;
use super::raw_edit::editor_document;
use super::raw_style::{
    base_width, object_number, patch_font, patch_object_control, patch_width, root_font,
};
use super::*;
use crate::app::style_ops::StyleKind;
use crate::app::text_style_draft::TextStyleDraft;
use crate::entities::text_support::{TTF_FLAG_BOLD, TTF_FLAG_ITALIC};
use crate::ui::font_picker::{stroke_file, FontChoice};
use codec::entities::mtext_format::{MTextScalar, SpanProperties};
use codec::tables::TextStyle;

/// What the selected characters share of the values a text style stores;
/// `None` where they differ.
#[derive(Debug, Default, PartialEq)]
pub(in crate::app) struct SharedValues {
    /// A font of the characters' own, with its bold and italic; `None` also
    /// when they are written in the style's font.
    pub font: Option<(FontChoice, bool, bool)>,
    pub height: Option<f64>,
    pub width: Option<f64>,
    /// In degrees.
    pub oblique: Option<f64>,
}

/// Equal up to the last bits a conversion may round.
fn same(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.)
}

/// The one value every item has, `None` when they differ or there is none.
fn shared<T>(items: impl IntoIterator<Item = T>, eq: impl Fn(&T, &T) -> bool) -> Option<T> {
    let mut items = items.into_iter();
    let first = items.next()?;
    items.all(|item| eq(&item, &first)).then_some(first)
}

/// The font a style draws with, as the font picker names it.
fn style_font(style: &TextStyle) -> FontChoice {
    if style.true_type_font.trim().is_empty() {
        FontChoice::Stroke(style.font_file.clone())
    } else {
        FontChoice::TrueType(style.true_type_font.clone())
    }
}

fn style_face(style: &TextStyle) -> (bool, bool) {
    let flags = style.true_type_font_flags;
    (flags & TTF_FLAG_BOLD != 0, flags & TTF_FLAG_ITALIC != 0)
}

impl SharedValues {
    /// `source` with the shared values written in; every other value stays
    /// exactly as stored. A style that asks for the height of each text
    /// keeps asking, and a value a style cannot hold is left out.
    pub(in crate::app) fn apply_to(&self, source: &TextStyle) -> TextStyle {
        let mut draft = TextStyleDraft::load(source);
        if let Some((font, bold, italic)) = &self.font {
            let new_font = !font.same_font(&style_font(source));
            match font {
                FontChoice::Stroke(name) if new_font => draft.pick_stroke_font(stroke_file(name)),
                FontChoice::TrueType(family) => {
                    if new_font {
                        draft.pick_system_font(family.clone());
                    }
                    if (*bold, *italic) != style_face(source) {
                        draft.set_face(*bold, *italic);
                    }
                }
                _ => {}
            }
        }
        let mut style = draft.resolved();
        if let Some(width) = self
            .width
            .filter(|w| (0.01..=100.).contains(w) && !same(*w, style.width_factor))
        {
            style.width_factor = width;
        }
        if let Some(oblique) = self
            .oblique
            .filter(|o| (-85.0..=85.).contains(o) && !same(*o, style.oblique_angle.to_degrees()))
        {
            style.oblique_angle = oblique.to_radians();
        }
        if let Some(height) = self
            .height
            .filter(|h| style.height > 0. && *h > 0. && !same(*h, style.height))
        {
            style.height = height;
        }
        style
    }
}

impl MTextEditorState {
    /// The values the selected characters share, or all of the text's when
    /// nothing is selected.
    pub(in crate::app) fn selection_values(&self) -> SharedValues {
        let cells = doc_to_cells(&editor_document(&self.content.text()));
        let range = match self.sel {
            Some((a, b)) if a != b => a.min(b).min(cells.len())..a.max(b).min(cells.len()),
            _ => 0..cells.len(),
        };
        let props: Vec<&SpanProperties> = cells[range]
            .iter()
            .filter_map(|cell| match cell {
                Cell::Char(_, p) | Cell::Stack { props: p, .. } => Some(p),
                Cell::Break(..) => None,
            })
            .collect();
        let style = &self.object_style;
        let base = self.height_value();
        let numbers = |value: &dyn Fn(&SpanProperties) -> f64| {
            shared(props.iter().map(|p| value(p)), |a, b| same(*a, *b))
        };
        SharedValues {
            font: shared(
                props.iter().map(|p| {
                    p.font
                        .as_ref()
                        .filter(|f| !f.name.trim().is_empty())
                        .map(|f| (FontChoice::of_name(&f.name), f.bold, f.italic))
                }),
                |a, b| match (a, b) {
                    (Some((fa, ba, ia)), Some((fb, bb, ib))) => {
                        fa.same_font(fb) && (ba, ia) == (bb, ib)
                    }
                    (None, None) => true,
                    _ => false,
                },
            )
            .flatten(),
            height: numbers(&|p| match p.height {
                Some(MTextScalar::Factor(f)) => base * f,
                Some(MTextScalar::Absolute(a)) => a,
                None => base,
            }),
            width: numbers(&|p| style.width * p.width_factor.unwrap_or(1.)),
            oblique: numbers(&|p| p.oblique_angle.unwrap_or(style.oblique)),
        }
    }
}

impl crate::app::OpenCADStudio {
    /// Open the name field of "New style from selection", or from its own
    /// Cancel go back to the style menu.
    pub(in crate::app) fn mtext_new_style_open(&mut self) -> iced::Task<crate::app::Message> {
        let Some(style) = self.mtext_editor.as_ref().map(|ed| ed.style.clone()) else {
            return iced::Task::none();
        };
        let name = self.unique_suffixed_name(StyleKind::Text, &style);
        let Some(ed) = self.mtext_editor.as_mut() else {
            return iced::Task::none();
        };
        if ed.menu == Some(EditorMenu::NewStyle) {
            ed.menu = Some(EditorMenu::Style);
            return iced::Task::none();
        }
        ed.new_style_name = name;
        ed.menu = Some(EditorMenu::NewStyle);
        let id = iced::widget::Id::new(NEW_STYLE_NAME_ID);
        iced::Task::batch([
            iced::widget::operation::focus(id.clone()),
            iced::widget::operation::select_all(id),
        ])
    }

    /// Make a new text style from the selection, or (`None`) update the
    /// text's own style from it, as one undo step with the texts the style
    /// change carries over to. The edited text then takes the style, and its
    /// whole-text codes that only repeat the style go.
    pub(in crate::app) fn mtext_style_from_selection(&mut self, name: Option<String>) {
        // The style manager holds its own transaction.
        if self.style_stage.is_some() {
            return;
        }
        let Some(ed) = self.mtext_editor.as_mut() else {
            return;
        };
        ed.menu = None;
        let values = ed.selection_values();
        let i = self.active_tab;
        let Some(source) = self.tabs[i]
            .scene
            .document
            .text_styles
            .get(&ed.style)
            .cloned()
        else {
            return;
        };
        let name = name.map(|n| n.trim().to_string());
        match &name {
            Some(new) if new.is_empty() => return,
            Some(new) if new.contains('|') => {
                self.command_line
                    .push_error(crate::t!("Referenced style · Read only").as_ref());
                return;
            }
            Some(new) if self.style_exists(StyleKind::Text, new) => {
                self.command_line
                    .push_error(crate::tf!("Style '{new}' already exists.").as_ref());
                return;
            }
            None if source.xref_dependent || source.name.contains('|') => {
                self.command_line
                    .push_error(crate::t!("Referenced style · Read only").as_ref());
                return;
            }
            _ => {}
        }
        let mut style = values.apply_to(&source);
        self.style_stage_begin();
        let doc = &mut self.tabs[i].scene.document;
        if let Some(new) = &name {
            style.name = new.clone();
            style.handle = doc.allocate_handle();
            let _ = doc.text_styles.add(style);
        } else if let Some(stored) = doc.text_styles.get_mut(&source.name) {
            *stored = style;
        }
        self.style_stage_commit();
        self.style_stage = None;
        let message = if let Some(name) = name {
            self.mtext_object_style(name.clone());
            crate::tf!("Style '{name}' created.")
        } else {
            let name = source.name;
            crate::tf!("Style '{name}' updated.")
        };
        self.command_line.push_output(message.as_ref());
        self.mtext_drop_style_repeats();
    }

    /// After the text style changed elsewhere (the style manager's Apply),
    /// show its new values; a whole-text width keeps the width it draws with.
    pub(in crate::app) fn mtext_follow_style_table(&mut self) {
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let doc = &self.tabs[self.active_tab].scene.document;
        if raw_style::ObjectStyle::of(doc, &ed.style) != ed.object_style {
            self.mtext_object_style(ed.style.clone());
        }
    }

    /// Drop the whole-text font, `\W` and `\Q` that only repeat the style,
    /// as one undo step. Codes that format part of the text stay.
    fn mtext_drop_style_repeats(&mut self) {
        let Some(ed) = self.mtext_editor.as_ref() else {
            return;
        };
        let Some(style) = self.tabs[self.active_tab]
            .scene
            .document
            .text_styles
            .get(&ed.style)
        else {
            return;
        };
        let object = &ed.object_style;
        let raw = ed.content.text();
        let step = |raw: String, patch: &dyn Fn(&str) -> Result<String, EditError>| {
            patch(&raw).unwrap_or(raw)
        };
        let mut out = raw.clone();
        if base_width(&out, object.width).is_ok_and(|w| same(w, object.width)) {
            out = step(out, &|r| patch_width(r, object.width, None));
        }
        if object_number(&out, 'Q').is_some_and(|q| same(q, object.oblique)) {
            out = step(out, &|r| patch_object_control(r, 'Q', None));
        }
        if root_font(&out).is_some_and(|f| {
            FontChoice::of_name(&f.name).same_font(&style_font(style))
                && (f.bold, f.italic) == object.font_flags
        }) {
            out = step(out, &|r| patch_font(r, None, object.font_flags));
        }
        if out != raw && self.set_object_source(Ok(out)) {
            let doc = &self.tabs[self.active_tab].scene.document;
            if let Some(ed) = self.mtext_editor.as_mut() {
                ed.seed_object_values(doc);
            }
        }
        self.rebuild_mtext_preview();
    }
}

/// The name field of "New style from selection", focused when it opens.
pub(in crate::app) const NEW_STYLE_NAME_ID: &str = "mtext-new-style-name";
