//! The dimension style window: manager, New, Modify / Override, Compare.
//!
//! The window is one transaction: Set Current, a new or renamed style,
//! Delete, Modify's OK and Override's OK change the drawing live, and
//! closing the window keeps them as one undo step. Modify, New and Override
//! edit a draft that only OK writes back.

use crate::app::{Message, ModalKind, OpenCADStudio, StyleKind};
use crate::scene::creation_style::{dim_style_extras, set_dim_style_extras, DimStyleExtras};
use crate::tr;
use crate::ui::style::dimstyle::{
    self as ui, alt_below, center_mode, field_id, field_tab, lineweight_value, with_zero_flag,
    CompareRow, CompareState, CreateDraft, DraftNames, DsMsg, Edit, Mode, Notice, Pick,
    PreviewArrows, PreviewTexts, StyleRow, View, OTHER,
};
use crate::ui::style::mleaderstyle::{trim_number, LinetypeRow, LoadDraft};
use document::entities::{Dimension, DimensionAligned, DimensionAngular3Pt, DimensionRadius};
use document::tables::DimStyle;
use document::types::{Color as AcadColor, Vector3};
use document::{Document, Handle};
use iced::Task;

/// The characters a style name may not hold.
fn unsupported_name(name: &str) -> bool {
    name.trim().is_empty()
        || name.chars().any(|c| {
            matches!(
                c,
                '\\' | '<' | '>' | '/' | '?' | '"' | ':' | ';' | '*' | '|' | ',' | '=' | '`'
            )
        })
}

fn is_xref(style: &DimStyle) -> bool {
    style.xref_dependent || style.name.contains('|')
}

/// The style family member a "Use for" choice makes: the parent's name with
/// the dimension type's suffix.
fn family_suffix(use_for: u8) -> &'static str {
    match use_for {
        1 => "$0",
        2 => "$2",
        3 => "$4",
        4 => "$3",
        5 => "$6",
        _ => "$7",
    }
}

/// The name a linetype handle reads as in the lists.
fn linetype_name(doc: &Document, handle: Handle) -> String {
    if handle.is_null() {
        return "ByBlock".to_string();
    }
    doc.line_types
        .iter()
        .find(|lt| lt.handle == handle)
        .map(|lt| lt.name.clone())
        .unwrap_or_else(|| "ByBlock".to_string())
}

/// The linetype a list name selects: ByBlock is no linetype of its own.
fn linetype_handle(doc: &Document, name: &str) -> Handle {
    if name.eq_ignore_ascii_case("ByBlock") {
        return Handle::NULL;
    }
    doc.line_types
        .iter()
        .find(|lt| lt.name.eq_ignore_ascii_case(name))
        .map(|lt| lt.handle)
        .unwrap_or(Handle::NULL)
}

fn text_style_name(doc: &Document, style: &DimStyle) -> String {
    doc.text_styles
        .iter()
        .find(|t| t.handle == style.dimtxsty_handle && !style.dimtxsty_handle.is_null())
        .map(|t| t.name.clone())
        .unwrap_or_else(|| {
            if style.dimtxsty.is_empty() {
                "Standard".to_string()
            } else {
                style.dimtxsty.clone()
            }
        })
}

/// Fill the typed fields of a settings face from `style`.
fn texts_of(style: &DimStyle, extras: DimStyleExtras) -> Vec<(&'static str, String)> {
    let (prefix, suffix) = split_template(&style.dimpost.replace("\\X", ""), "<>");
    let (alt_prefix, alt_suffix) = split_template(&style.dimapost, "[]");
    vec![
        ("dimdle", trim_number(style.dimdle)),
        ("dimdli", trim_number(style.dimdli)),
        ("dimexe", trim_number(style.dimexe)),
        ("dimexo", trim_number(style.dimexo)),
        ("dimfxl", trim_number(style.dimfxl)),
        ("dimasz", trim_number(style.dimasz)),
        ("dimcen", trim_number(style.dimcen.abs())),
        ("dimbreak", trim_number(extras.break_size)),
        ("dimjogang", trim_number(style.dimjogang.to_degrees())),
        ("dimjag", trim_number(extras.jog_height)),
        ("dimtxt", trim_number(style.dimtxt)),
        ("dimtfac", trim_number(style.dimtfac)),
        ("dimgap", trim_number(style.dimgap.abs())),
        ("dimscale", trim_number(style.dimscale)),
        ("dimrnd", trim_number(style.dimrnd)),
        ("prefix", prefix),
        ("suffix", suffix),
        ("dimlfac", trim_number(style.dimlfac.abs())),
        ("dimmzf", trim_number(style.dimmzf)),
        ("dimmzs", style.dimmzs.clone()),
        ("dimaltf", trim_number(style.dimaltf)),
        ("dimaltrnd", trim_number(style.dimaltrnd)),
        ("alt_prefix", alt_prefix),
        ("alt_suffix", alt_suffix),
        ("dimaltmzf", trim_number(style.dimaltmzf)),
        ("dimaltmzs", style.dimaltmzs.clone()),
        ("dimtp", trim_number(style.dimtp)),
        ("dimtm", trim_number(style.dimtm)),
    ]
}

/// Prefix and suffix around `marker`; a template without one is a suffix.
fn split_template(template: &str, marker: &str) -> (String, String) {
    match template.split_once(marker) {
        Some((prefix, suffix)) => (prefix.to_string(), suffix.to_string()),
        None => (String::new(), template.to_string()),
    }
}

fn join_template(prefix: &str, suffix: &str, marker: &str) -> String {
    if prefix.is_empty() {
        suffix.to_string()
    } else {
        format!("{prefix}{marker}{suffix}")
    }
}

/// The variables Compare lists: description key, variable.
const COMPARE_VARS: &[(&str, &str)] = &[
    ("cmp-alt-precision", "DIMALTD"),
    ("cmp-alt-post", "DIMAPOST"),
    ("cmp-alt-round", "DIMALTRND"),
    ("cmp-alt-scale", "DIMALTF"),
    ("cmp-alt-tol-precision", "DIMALTTD"),
    ("cmp-alt-tol-zeroes", "DIMALTTZ"),
    ("cmp-alt-units", "DIMALTU"),
    ("cmp-alt-zero-factor", "DIMALTMZF"),
    ("cmp-alt-zero-suffix", "DIMALTMZS"),
    ("cmp-alt-zeroes", "DIMALTZ"),
    ("cmp-alternate", "DIMALT"),
    ("cmp-angle-format", "DIMAUNIT"),
    ("cmp-angle-precision", "DIMADEC"),
    ("cmp-angle-zeroes", "DIMAZIN"),
    ("cmp-arc-symbol", "DIMARCSYM"),
    ("cmp-arrow", "DIMBLK"),
    ("cmp-arrow1", "DIMBLK1"),
    ("cmp-arrow2", "DIMBLK2"),
    ("cmp-arrow-size", "DIMASZ"),
    ("cmp-center", "DIMCEN"),
    ("cmp-separator", "DIMDSEP"),
    ("cmp-dim-color", "DIMCLRD"),
    ("cmp-dim-ext", "DIMDLE"),
    ("cmp-dim-forced", "DIMTOFL"),
    ("cmp-dim-linetype", "DIMLTYPE"),
    ("cmp-dim-lineweight", "DIMLWD"),
    ("cmp-dim-spacing", "DIMDLI"),
    ("cmp-ext1-linetype", "DIMLTEX1"),
    ("cmp-ext2-linetype", "DIMLTEX2"),
    ("cmp-ext-color", "DIMCLRE"),
    ("cmp-ext-extension", "DIMEXE"),
    ("cmp-ext-lineweight", "DIMLWE"),
    ("cmp-ext-offset", "DIMEXO"),
    ("cmp-fit", "DIMATFIT"),
    ("cmp-text-movement", "DIMTMOVE"),
    ("cmp-fixed-on", "DIMFXLON"),
    ("cmp-fixed-length", "DIMFXL"),
    ("cmp-fraction", "DIMFRAC"),
    ("cmp-jog-angle", "DIMJOGANG"),
    ("cmp-leader-arrow", "DIMLDRBLK"),
    ("cmp-length-scale", "DIMLFAC"),
    ("cmp-length-units", "DIMLUNIT"),
    ("cmp-no-dim1", "DIMSD1"),
    ("cmp-no-dim2", "DIMSD2"),
    ("cmp-no-outside", "DIMSOXD"),
    ("cmp-no-ext1", "DIMSE1"),
    ("cmp-no-ext2", "DIMSE2"),
    ("cmp-scale", "DIMSCALE"),
    ("cmp-precision", "DIMDEC"),
    ("cmp-round", "DIMRND"),
    ("cmp-separate", "DIMSAH"),
    ("cmp-fill-color", "DIMTFILLCLR"),
    ("cmp-fill", "DIMTFILL"),
    ("cmp-text-color", "DIMCLRT"),
    ("cmp-text-direction", "DIMTXTDIRECTION"),
    ("cmp-text-height", "DIMTXT"),
    ("cmp-text-inside", "DIMTIX"),
    ("cmp-inside-align", "DIMTIH"),
    ("cmp-text-offset", "DIMGAP"),
    ("cmp-outside-align", "DIMTOH"),
    ("cmp-pos-hor", "DIMJUST"),
    ("cmp-pos-vert", "DIMTAD"),
    ("cmp-post", "DIMPOST"),
    ("cmp-text-style", "DIMTXSTY"),
    ("cmp-tol-lower", "DIMTM"),
    ("cmp-tol-upper", "DIMTP"),
    ("cmp-tol-deviation", "DIMTOL"),
    ("cmp-tol-limits", "DIMLIM"),
    ("cmp-tol-vert", "DIMTOLJ"),
    ("cmp-tol-precision", "DIMTDEC"),
    ("cmp-tol-scale", "DIMTFAC"),
    ("cmp-tol-zeroes", "DIMTZIN"),
    ("cmp-zero-factor", "DIMMZF"),
    ("cmp-zero-suffix", "DIMMZS"),
    ("cmp-zeroes", "DIMZIN"),
];

fn compare_description(key: &str) -> String {
    crate::i18n::format_attribute("dim-style", key, &[]).unwrap_or_else(|| key.to_string())
}

/// How Compare writes `variable` of `style`.
fn compare_value(doc: &Document, style: &DimStyle, variable: &str) -> String {
    let real = |v: f64| format!("{v:.4}");
    let on_off = |v: bool| {
        if v {
            tr!("dim-style", "on")
        } else {
            tr!("dim-style", "off")
        }
    };
    let color = |index: i16, true_color: Option<AcadColor>| match (index, true_color) {
        (_, Some(AcadColor::Rgb { r, g, b })) => format!("{r},{g},{b}"),
        (0, _) => "BYBLOCK".to_string(),
        (256, _) => "BYLAYER".to_string(),
        (i, _) => i.to_string(),
    };
    let block = |handle: Handle| {
        if handle.is_null() {
            return "ClosedFilled".to_string();
        }
        doc.block_records
            .iter()
            .find(|b| b.handle == handle)
            .map(|b| b.name.trim_start_matches('_').to_string())
            .unwrap_or_else(|| "ClosedFilled".to_string())
    };
    let linetype = |handle: Handle| linetype_name(doc, handle).to_uppercase();
    match variable {
        "DIMALTD" => style.dimaltd.to_string(),
        "DIMAPOST" => style.dimapost.clone(),
        "DIMALTRND" => real(style.dimaltrnd),
        "DIMALTF" => real(style.dimaltf),
        "DIMALTTD" => style.dimalttd.to_string(),
        "DIMALTTZ" => style.dimalttz.to_string(),
        "DIMALTU" => style.dimaltu.to_string(),
        "DIMALTMZF" => real(style.dimaltmzf),
        "DIMALTMZS" => style.dimaltmzs.clone(),
        "DIMALTZ" => style.dimaltz.to_string(),
        "DIMALT" => on_off(style.dimalt),
        "DIMAUNIT" => style.dimaunit.to_string(),
        "DIMADEC" => style.dimadec.to_string(),
        "DIMAZIN" => style.dimazin.to_string(),
        "DIMARCSYM" => style.dimarcsym.to_string(),
        "DIMBLK" => block(style.dimblk),
        "DIMBLK1" => block(style.dimblk1),
        "DIMBLK2" => block(style.dimblk2),
        "DIMASZ" => real(style.dimasz),
        "DIMCEN" => real(style.dimcen),
        "DIMDSEP" => char::from_u32(style.dimdsep as u32)
            .map(String::from)
            .unwrap_or_default(),
        "DIMCLRD" => color(style.dimclrd, style.dimclrd_true_color),
        "DIMDLE" => real(style.dimdle),
        "DIMTOFL" => on_off(style.dimtofl),
        "DIMLTYPE" => linetype(style.dimltex_handle),
        "DIMLWD" => style.dimlwd.to_string(),
        "DIMDLI" => real(style.dimdli),
        "DIMLTEX1" => linetype(style.dimltex1_handle),
        "DIMLTEX2" => linetype(style.dimltex2_handle),
        "DIMCLRE" => color(style.dimclre, style.dimclre_true_color),
        "DIMEXE" => real(style.dimexe),
        "DIMLWE" => style.dimlwe.to_string(),
        "DIMEXO" => real(style.dimexo),
        "DIMATFIT" => style.dimatfit.to_string(),
        "DIMTMOVE" => style.dimtmove.to_string(),
        "DIMFXLON" => on_off(style.dimfxlon),
        "DIMFXL" => real(style.dimfxl),
        "DIMFRAC" => style.dimfrac.to_string(),
        "DIMJOGANG" => trim_number(style.dimjogang.to_degrees()),
        "DIMLDRBLK" => block(style.dimldrblk),
        "DIMLFAC" => real(style.dimlfac),
        "DIMLUNIT" => style.dimlunit.to_string(),
        "DIMSD1" => on_off(style.dimsd1),
        "DIMSD2" => on_off(style.dimsd2),
        "DIMSOXD" => on_off(style.dimsoxd),
        "DIMSE1" => on_off(style.dimse1),
        "DIMSE2" => on_off(style.dimse2),
        "DIMSCALE" => real(style.dimscale),
        "DIMDEC" => style.dimdec.to_string(),
        "DIMRND" => real(style.dimrnd),
        "DIMSAH" => on_off(style.dimsah),
        "DIMTFILLCLR" => color(style.dimtfillclr, style.dimtfillclr_true_color),
        "DIMTFILL" => style.dimtfill.to_string(),
        "DIMCLRT" => color(style.dimclrt, style.dimclrt_true_color),
        "DIMTXTDIRECTION" => on_off(style.dimtxtdirection),
        "DIMTXT" => real(style.dimtxt),
        "DIMTIX" => on_off(style.dimtix),
        "DIMTIH" => on_off(style.dimtih),
        "DIMGAP" => real(style.dimgap),
        "DIMTOH" => on_off(style.dimtoh),
        "DIMJUST" => style.dimjust.to_string(),
        "DIMTAD" => style.dimtad.to_string(),
        "DIMPOST" => style.dimpost.clone(),
        "DIMTXSTY" => text_style_name(doc, style),
        "DIMTM" => real(style.dimtm),
        "DIMTP" => real(style.dimtp),
        "DIMTOL" => on_off(style.dimtol),
        "DIMLIM" => on_off(style.dimlim),
        "DIMTOLJ" => style.dimtolj.to_string(),
        "DIMTDEC" => style.dimtdec.to_string(),
        "DIMTFAC" => real(style.dimtfac),
        "DIMTZIN" => style.dimtzin.to_string(),
        "DIMMZF" => real(style.dimmzf),
        "DIMMZS" => style.dimmzs.clone(),
        "DIMZIN" => style.dimzin.to_string(),
        _ => String::new(),
    }
}

/// Compare's rows: every variable of `first`, or only those `second`
/// differs in, ordered by description.
fn compare_rows(doc: &Document, first: &DimStyle, second: Option<&DimStyle>) -> Vec<CompareRow> {
    let mut rows: Vec<CompareRow> = COMPARE_VARS
        .iter()
        .filter_map(|(key, variable)| {
            let a = compare_value(doc, first, variable);
            let b = second.map(|s| compare_value(doc, s, variable));
            if b.as_ref().is_some_and(|b| *b == a) {
                return None;
            }
            Some(CompareRow {
                description: compare_description(key),
                variable,
                first: a,
                second: b,
            })
        })
        .collect();
    rows.sort_by_key(|row| row.description.to_lowercase());
    rows
}

/// The sample values of the preview, written the way `style` writes them.
fn preview_texts(style: &DimStyle) -> PreviewTexts {
    let at = |x: f64, y: f64| Vector3::new(x, y, 0.0);
    let text = |dim: Dimension| {
        crate::entities::dimension::dimension_text_value(&dim, Some(style)).unwrap_or_default()
    };
    let angle = 60f64.to_radians();
    PreviewTexts {
        horizontal: text(Dimension::Aligned(DimensionAligned::new(at(0.0, 0.0), at(14.11, 0.0)))),
        vertical: text(Dimension::Aligned(DimensionAligned::new(at(0.0, 0.0), at(0.0, 16.6)))),
        aligned: text(Dimension::Aligned(DimensionAligned::new(at(0.0, 0.0), at(28.07, 0.0)))),
        radius: text(Dimension::Radius(DimensionRadius::new(at(0.0, 0.0), at(11.17, 0.0)))),
        angle: text(Dimension::Angular3Pt(DimensionAngular3Pt::new(
            at(0.0, 0.0),
            at(10.0, 0.0),
            at(10.0 * angle.cos(), 10.0 * angle.sin()),
        ))),
    }
}

impl OpenCADStudio {
    fn ds_style(&self, name: &str) -> Option<&DimStyle> {
        self.tabs[self.active_tab]
            .scene
            .document
            .dim_styles
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }

    fn ds_current(&self) -> String {
        self.tabs[self.active_tab]
            .scene
            .document
            .header
            .current_dimstyle_name
            .clone()
    }

    /// The current style's overrides, when they belong to it.
    fn ds_overrides(&self) -> Vec<(i16, document::xdata::XDataValue)> {
        let current = self.ds_current();
        match &self.tabs[self.active_tab].scene.dim_overrides {
            Some((owner, values)) if owner.eq_ignore_ascii_case(&current) => values.clone(),
            _ => Vec::new(),
        }
    }

    /// The current style with its overrides laid over it.
    fn ds_overridden(&self) -> Option<DimStyle> {
        let mut style = self.ds_style(&self.ds_current())?.clone();
        crate::entities::dim_override::apply_pairs(&mut style, &self.ds_overrides());
        Some(style)
    }

    pub(super) fn on_dim_style_dialog_open(&mut self) -> Task<Message> {
        let current = self.ds_current();
        let selected = self
            .ds_style(&current)
            .or_else(|| {
                self.tabs[self.active_tab]
                    .scene
                    .document
                    .dim_styles
                    .iter()
                    .next()
            })
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "Standard".to_string());
        self.dimstyle_selected = selected;
        self.dsd = Default::default();
        self.active_modal = Some(ModalKind::DimStyle);
        // Linetypes loaded from Select Linetype belong to the session too.
        self.style_stage_begin_linetypes();
        Task::none()
    }

    /// The ✕ of the window closes its topmost face first; `false` when only
    /// the manager is open.
    pub(super) fn dim_style_close_face(&mut self) -> bool {
        let d = &mut self.dsd;
        if d.notice.take().is_some()
            || d.delete_ask.take().is_some()
            || d.load.take().is_some()
            || d.pick.take().is_some()
            || d.create.take().is_some()
            || d.edit.take().is_some()
            || d.compare.take().is_some()
        {
            d.open_list = None;
            return true;
        }
        false
    }

    /// The window closes: the session's changes become one undo step.
    pub(super) fn dim_style_close(&mut self) {
        self.dsd = Default::default();
        self.after_style_change(StyleKind::Dim);
        self.style_stage_commit();
        self.style_stage = None;
    }

    /// The Text Style manager opened from the settings face closed: what it
    /// did not apply is taken back and the face comes back in the same
    /// transaction.
    pub(super) fn dim_style_back_from_text_styles(&mut self) {
        self.style_stage_child_close();
        self.active_modal = Some(ModalKind::DimStyle);
        self.reset_modal_geometry();
        // A style made or renamed there is offered in the list.
        let i = self.active_tab;
        if let Some(edit) = self.dsd.edit.as_mut() {
            let doc = &self.tabs[i].scene.document;
            if let Some(style) = doc.text_styles.iter().find(|t| t.handle == edit.draft.dimtxsty_handle) {
                edit.draft.dimtxsty = style.name.clone();
            }
        }
    }

    /// The view of the window's current face.
    pub(in crate::app) fn dim_style_view(&self) -> View<'_> {
        let tab = &self.tabs[self.active_tab];
        let doc = &tab.scene.document;
        let current = &doc.header.current_dimstyle_name;
        let used: Vec<String> = doc
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Dimension(dim) => Some(dim.base().style_name.to_lowercase()),
                document::EntityType::Leader(leader) => Some(leader.dimension_style.to_lowercase()),
                _ => None,
            })
            .collect();
        let mut styles: Vec<StyleRow> = doc
            .dim_styles
            .iter()
            .map(|s| StyleRow {
                name: s.name.clone(),
                annotative: s.annotative,
                xref: is_xref(s),
                in_use: used.contains(&s.name.to_lowercase()),
            })
            .collect();
        styles.sort_by_key(|s| s.name.to_lowercase());
        let overrides = self.ds_overrides();
        let override_lines: Vec<String> = match self.ds_style(current) {
            Some(base) => {
                let mut over = base.clone();
                crate::entities::dim_override::apply_pairs(&mut over, &overrides);
                compare_rows(doc, &over, Some(base))
                    .into_iter()
                    .map(|row| format!("{} = {}", row.description, row.first))
                    .collect()
            }
            None => Vec::new(),
        };
        let d = &self.dsd;
        let shown = if let Some(edit) = &d.edit {
            Some(edit.draft.clone())
        } else if d.overrides_selected {
            self.ds_overridden()
        } else {
            self.ds_style(&self.dimstyle_selected).cloned()
        };
        let description = if d.overrides_selected && !override_lines.is_empty() {
            format!("{} + {}", current, override_lines.join(", "))
        } else {
            self.dimstyle_selected.clone()
        };
        let arrows_of = |s: &DimStyle| PreviewArrows {
            first: crate::entities::arrowheads::label(doc, if s.dimsah { s.dimblk1 } else { s.dimblk }),
            second: crate::entities::arrowheads::label(doc, if s.dimsah { s.dimblk2 } else { s.dimblk }),
            leader: crate::entities::arrowheads::label(doc, s.dimldrblk),
        };
        let shown_arrows = shown.as_ref().map(arrows_of).unwrap_or_default();
        let names = match &d.edit {
            Some(edit) => {
                let s = &edit.draft;
                let a = arrows_of(s);
                DraftNames {
                    dimltype: linetype_name(doc, s.dimltex_handle),
                    ext1: linetype_name(doc, s.dimltex1_handle),
                    ext2: linetype_name(doc, s.dimltex2_handle),
                    first: a.first,
                    second: a.second,
                    leader: a.leader,
                    text_style: text_style_name(doc, s),
                }
            }
            None => DraftNames::default(),
        };
        let mut linetypes = vec![
            LinetypeRow {
                name: "ByLayer".into(),
                description: String::new(),
                pattern: Vec::new(),
            },
            LinetypeRow {
                name: "ByBlock".into(),
                description: String::new(),
                pattern: Vec::new(),
            },
        ];
        linetypes.extend(
            doc.line_types
                .iter()
                .filter(|lt| {
                    !lt.name.eq_ignore_ascii_case("ByLayer") && !lt.name.eq_ignore_ascii_case("ByBlock")
                })
                .map(|lt| LinetypeRow {
                    name: lt.name.clone(),
                    description: lt.description.clone(),
                    pattern: lt.elements.iter().map(|e| e.length).collect(),
                }),
        );
        let mut text_styles: Vec<String> = doc
            .text_styles
            .iter()
            .filter(|t| !t.is_shape_file && !t.name.is_empty())
            .map(|t| t.name.clone())
            .collect();
        text_styles.sort_by_key(|n| n.to_lowercase());
        let compare_rows = match &d.compare {
            Some(c) => match self.ds_style(&c.first) {
                Some(first) => {
                    let second = c
                        .second
                        .as_ref()
                        .filter(|s| **s != c.first)
                        .and_then(|s| self.ds_style(s));
                    compare_rows(doc, first, second)
                }
                None => Vec::new(),
            },
            None => Vec::new(),
        };
        View {
            dialog: d,
            styles,
            selected: &self.dimstyle_selected,
            current,
            overrides: override_lines,
            preview: shown.as_ref().map(preview_texts).unwrap_or_default(),
            shown,
            shown_arrows,
            description,
            loaded_linetypes: doc.line_types.iter().map(|lt| lt.name.clone()).collect(),
            linetypes,
            names,
            arrow_opts: crate::entities::arrowheads::options(std::iter::empty()),
            text_styles,
            user_blocks: super::mleader_style::user_blocks(doc),
            compare_rows,
            unit: if doc.header.measurement == 1 { 2.5 } else { 0.18 },
        }
    }

    fn ds_focus(&self, id: iced::widget::Id) -> Task<Message> {
        Task::batch([
            iced::widget::operation::focus(id.clone()),
            iced::widget::operation::select_all(id),
        ])
    }

    fn ds_begin_edit(&mut self, mode: Mode, name: String, draft: DimStyle, extras: DimStyleExtras) {
        self.dsd.edit = Some(Edit {
            mode,
            name,
            texts: texts_of(&draft, extras),
            dimtaln: extras.dimtaln,
            draft,
            tab: 0,
        });
        self.dsd.context = None;
        self.dsd.open_list = None;
    }

    fn ds_name_notice(&self, name: &str) -> Option<Notice> {
        if name.is_empty() {
            Some(Notice::EmptyName)
        } else if unsupported_name(name) {
            Some(Notice::UnsupportedCharacters)
        } else if self.style_exists(StyleKind::Dim, name) {
            Some(Notice::Exists(name.to_string()))
        } else {
            None
        }
    }

    pub(super) fn on_dim_style_dlg(&mut self, message: DsMsg) -> Task<Message> {
        let i = self.active_tab;
        match message {
            DsMsg::Select(name) => {
                if self.dsd.edit.is_none() {
                    self.dimstyle_selected = name;
                    self.dsd.overrides_selected = false;
                    self.dsd.context = None;
                }
            }
            DsMsg::SelectOverrides => {
                self.dimstyle_selected = self.ds_current();
                self.dsd.overrides_selected = true;
                self.dsd.context = None;
            }
            DsMsg::ListFilter(in_use) => self.dsd.in_use_only = in_use,
            DsMsg::HideXrefs(hide) => self.dsd.list_xrefs = !hide,
            DsMsg::Context(name) => {
                self.dsd.context = if self.dsd.context == name { None } else { name };
                if let Some(name) = &self.dsd.context {
                    self.dimstyle_selected = name.clone();
                    self.dsd.overrides_selected = false;
                }
            }
            DsMsg::RenameStart(name) => {
                self.dsd.context = None;
                if self.ds_style(&name).is_some_and(|s| !is_xref(s)) {
                    self.dimstyle_selected = name.clone();
                    self.dsd.rename = Some((name.clone(), name));
                    return self.ds_focus(iced::widget::Id::new("ds-rename"));
                }
            }
            DsMsg::RenameInput(value) => {
                if let Some((_, typed)) = self.dsd.rename.as_mut() {
                    *typed = value;
                }
            }
            DsMsg::RenameCommit => {
                if let Some((old, new)) = self.dsd.rename.clone() {
                    let new = new.trim().to_string();
                    // Style names are not case sensitive: a change of case
                    // keeps the name.
                    if new.eq_ignore_ascii_case(&old) {
                        self.dsd.rename = None;
                    } else if let Some(notice) = self.ds_name_notice(&new) {
                        self.dsd.notice = Some(notice);
                    } else {
                        self.rename_style_storage(StyleKind::Dim, &old, &new);
                        if self.tabs[i]
                            .scene
                            .dim_overrides
                            .as_ref()
                            .is_some_and(|(owner, _)| owner.eq_ignore_ascii_case(&old))
                        {
                            if let Some((owner, _)) = self.tabs[i].scene.dim_overrides.as_mut() {
                                *owner = new.clone();
                            }
                        }
                        self.dimstyle_selected = new;
                        self.dsd.rename = None;
                        self.after_style_change(StyleKind::Dim);
                    }
                }
            }
            DsMsg::RenameCancel => self.dsd.rename = None,
            DsMsg::SetCurrent => {
                self.dsd.context = None;
                let name = self.dimstyle_selected.clone();
                if self.ds_style(&name).is_some() && !self.dsd.overrides_selected {
                    let doc = &mut self.tabs[i].scene.document;
                    doc.header.current_dimstyle_name = name.clone();
                    if let Some(style) = doc.dim_styles.get(&name) {
                        doc.header.current_dimstyle_handle = style.handle;
                    }
                    crate::scene::creation_style::sync_header_dim_vars(doc);
                    // Overrides belong to the style they were made for.
                    if self.tabs[i]
                        .scene
                        .dim_overrides
                        .as_ref()
                        .is_some_and(|(owner, _)| !owner.eq_ignore_ascii_case(&name))
                    {
                        self.tabs[i].scene.dim_overrides = None;
                    }
                    self.after_style_change(StyleKind::Dim);
                }
            }
            DsMsg::New => {
                let start = if self.dsd.overrides_selected {
                    self.ds_current()
                } else {
                    self.dimstyle_selected.clone()
                };
                let annotative = self.ds_style(&start).is_some_and(|s| s.annotative);
                self.dsd.context = None;
                self.dsd.create = Some(CreateDraft {
                    name: crate::i18n::plain(tr!("dim-style", "copy-of", name = start.clone())),
                    start_with: start,
                    annotative,
                    use_for: 0,
                });
                return self.ds_focus(iced::widget::Id::new("ds-new-name"));
            }
            DsMsg::CreateName(name) => {
                if let Some(draft) = self.dsd.create.as_mut() {
                    draft.name = name;
                }
            }
            DsMsg::CreateStart(start) => {
                let annotative = self.ds_style(&start).is_some_and(|s| s.annotative);
                if let Some(draft) = self.dsd.create.as_mut() {
                    // A name still as offered follows the style it copies.
                    let offered = crate::i18n::plain(tr!("dim-style", "copy-of", name = draft.start_with.clone()));
                    if draft.name == offered {
                        draft.name = crate::i18n::plain(tr!("dim-style", "copy-of", name = start.clone()));
                    }
                    draft.start_with = start;
                    draft.annotative = annotative;
                }
            }
            DsMsg::CreateAnnotative(on) => {
                if let Some(draft) = self.dsd.create.as_mut() {
                    draft.annotative = on;
                }
            }
            DsMsg::CreateUse(use_for) => {
                if let Some(draft) = self.dsd.create.as_mut() {
                    draft.use_for = use_for;
                }
            }
            DsMsg::CreateCancel => self.dsd.create = None,
            DsMsg::CreateContinue => return self.ds_create_continue(),
            DsMsg::NoticeClose => {
                let notice = self.dsd.notice.take();
                if let Some(Notice::Value(..)) = notice {
                    if let Some(field) = self.dsd.edit.as_ref().and_then(|_| self.ds_invalid_field()) {
                        return self.ds_focus(field_id(field));
                    }
                }
                if self.dsd.create.is_some() {
                    return self.ds_focus(iced::widget::Id::new("ds-new-name"));
                }
                if self.dsd.rename.is_some() {
                    return self.ds_focus(iced::widget::Id::new("ds-rename"));
                }
            }
            DsMsg::Modify => {
                if self.dsd.overrides_selected {
                    return self.on_dim_style_dlg(DsMsg::Override);
                }
                let name = self.dimstyle_selected.clone();
                if let Some(style) = self.ds_style(&name).cloned() {
                    let extras = dim_style_extras(&self.tabs[i].scene.document, style.handle);
                    self.ds_begin_edit(Mode::Modify, name, style, extras);
                }
            }
            DsMsg::Override => {
                if let Some(style) = self.ds_overridden() {
                    let mut extras = dim_style_extras(&self.tabs[i].scene.document, style.handle);
                    if let Some(document::xdata::XDataValue::Integer16(v)) = self
                        .ds_overrides()
                        .iter()
                        .find(|(code, _)| *code == crate::entities::dim_override::DIMTALN)
                        .map(|(_, v)| v.clone())
                    {
                        extras.dimtaln = v;
                    }
                    let name = style.name.clone();
                    self.ds_begin_edit(Mode::Override, name, style, extras);
                }
            }
            DsMsg::Compare => {
                let first = self.dimstyle_selected.clone();
                self.dsd.context = None;
                self.dsd.compare = Some(CompareState {
                    second: Some(first.clone()),
                    first,
                });
            }
            DsMsg::CompareFirst(name) => {
                if let Some(c) = self.dsd.compare.as_mut() {
                    c.first = name;
                }
            }
            DsMsg::CompareSecond(name) => {
                if let Some(c) = self.dsd.compare.as_mut() {
                    c.second = (name != tr!("dim-style", "none-style")).then_some(name);
                }
            }
            DsMsg::CompareCopy => {
                let view = self.dim_style_view();
                let mut out = String::new();
                for row in &view.compare_rows {
                    out.push_str(&row.description);
                    out.push('\t');
                    out.push_str(row.variable);
                    out.push('\t');
                    out.push_str(&row.first);
                    if let Some(second) = &row.second {
                        out.push('\t');
                        out.push_str(second);
                    }
                    out.push_str("\r\n");
                }
                return iced::clipboard::write(out).map(|_| Message::Noop);
            }
            DsMsg::CompareClose => self.dsd.compare = None,
            DsMsg::Delete(name) => {
                self.dsd.context = None;
                let Some(style) = self.ds_style(&name) else {
                    return Task::none();
                };
                if is_xref(style) {
                    return Task::none();
                }
                let why = if name.eq_ignore_ascii_case(&self.ds_current()) {
                    Some(tr!("dim-style", "delete-current"))
                } else if self.style_in_use(StyleKind::Dim, &name) {
                    Some(tr!("dim-style", "delete-used"))
                } else {
                    None
                };
                match why {
                    Some(why) => self.dsd.notice = Some(Notice::Delete(why)),
                    None => self.dsd.delete_ask = Some(name),
                }
            }
            DsMsg::DeleteAnswer(yes) => {
                if let Some(name) = self.dsd.delete_ask.take() {
                    if yes && self.remove_style_storage(StyleKind::Dim, &name) {
                        self.dimstyle_selected = self.ds_current();
                        self.after_style_change(StyleKind::Dim);
                    }
                }
            }
            DsMsg::Help => {
                let help = if self.dsd.edit.is_some() {
                    tr!("dim-style", "help-modify")
                } else if self.dsd.create.is_some() {
                    tr!("dim-style", "help-create")
                } else if self.dsd.compare.is_some() {
                    tr!("dim-style", "help-compare")
                } else {
                    tr!("dim-style", "help-manager")
                };
                self.command_line.push_info(&help);
            }
            DsMsg::Tab(tab) => {
                self.dsd.open_list = None;
                if let Some(e) = self.dsd.edit.as_mut() {
                    e.tab = tab;
                }
            }
            DsMsg::Text(field, value) => {
                if let Some(e) = self.dsd.edit.as_mut() {
                    // Symmetrical tolerances keep both values the same.
                    if field == "dimtp" && ui::tolerance_method(&e.draft) == 1 {
                        e.set_text("dimtm", value.clone());
                    }
                    if let Some(v) = value.trim().parse::<f64>().ok().filter(|v| v.is_finite()) {
                        ds_live_value(e, field, v);
                    }
                    e.set_text(field, value);
                }
            }
            DsMsg::Set(field, code) => {
                self.dsd.open_list = None;
                let doc = &self.tabs[i].scene.document;
                let text_style = (field == "dimtxsty")
                    .then(|| doc.text_styles.get(&code).map(|t| (t.name.clone(), t.handle)))
                    .flatten();
                if let Some(e) = self.dsd.edit.as_mut() {
                    ds_set(e, field, &code, text_style);
                }
            }
            DsMsg::Toggle(field, on) => {
                if let Some(e) = self.dsd.edit.as_mut() {
                    ds_toggle(e, field, on);
                }
            }
            DsMsg::Color(field, color) => {
                self.dsd.open_list = None;
                if let Some(e) = self.dsd.edit.as_mut() {
                    let (index, true_color) = match color {
                        AcadColor::ByBlock => (0, None),
                        AcadColor::ByLayer => (256, None),
                        AcadColor::Index(i) => (i16::from(i), None),
                        AcadColor::Rgb { r, g, b } => (
                            crate::ui::color_select::color_to_aci_string(color)
                                .parse()
                                .unwrap_or(7),
                            Some(AcadColor::Rgb { r, g, b }),
                        ),
                        AcadColor::None => (0, None),
                    };
                    let d = &mut e.draft;
                    match field {
                        "dimclrd" => (d.dimclrd, d.dimclrd_true_color) = (index, true_color),
                        "dimclre" => (d.dimclre, d.dimclre_true_color) = (index, true_color),
                        "dimclrt" => (d.dimclrt, d.dimclrt_true_color) = (index, true_color),
                        _ => {
                            d.dimtfill = 2;
                            (d.dimtfillclr, d.dimtfillclr_true_color) = (index, true_color);
                        }
                    }
                }
            }
            DsMsg::ColorMore(field) => {
                self.dsd.open_list = None;
                let current = self
                    .dsd
                    .edit
                    .as_ref()
                    .map(|e| {
                        let d = &e.draft;
                        match field {
                            "dimclrd" => ui::style_color(d.dimclrd, d.dimclrd_true_color),
                            "dimclre" => ui::style_color(d.dimclre, d.dimclre_true_color),
                            "dimclrt" => ui::style_color(d.dimclrt, d.dimclrt_true_color),
                            _ => ui::style_color(d.dimtfillclr, d.dimtfillclr_true_color),
                        }
                    })
                    .unwrap_or(AcadColor::ByBlock);
                return self.update(Message::OpenColorWindow(
                    crate::app::ColorPickTarget::DimStyle(field),
                    current,
                ));
            }
            DsMsg::OpenList(field) => {
                self.dsd.open_list = if self.dsd.open_list == Some(field) {
                    None
                } else {
                    Some(field)
                };
            }
            DsMsg::Arrow(slot, value) => {
                self.dsd.open_list = None;
                if value == OTHER {
                    let current = self
                        .dsd
                        .edit
                        .as_ref()
                        .map(|e| {
                            let d = &e.draft;
                            let handle = match slot {
                                0 if d.dimsah => d.dimblk1,
                                0 => d.dimblk,
                                1 if d.dimsah => d.dimblk2,
                                1 => d.dimblk,
                                _ => d.dimldrblk,
                            };
                            crate::entities::arrowheads::label(&self.tabs[i].scene.document, handle)
                        })
                        .unwrap_or_default();
                    self.dsd.pick = Some(Pick::UserArrow(slot, current));
                } else {
                    self.ds_set_arrow(slot, &value);
                }
            }
            DsMsg::Linetype(field, value) => {
                self.dsd.open_list = None;
                if value == OTHER {
                    let current = self
                        .dsd
                        .edit
                        .as_ref()
                        .map(|e| {
                            let handle = match field {
                                "dimltex1" => e.draft.dimltex1_handle,
                                "dimltex2" => e.draft.dimltex2_handle,
                                _ => e.draft.dimltex_handle,
                            };
                            linetype_name(&self.tabs[i].scene.document, handle)
                        })
                        .unwrap_or_default();
                    self.dsd.pick = Some(Pick::Linetype(field, current));
                } else {
                    self.ds_set_linetype(field, &value);
                }
            }
            DsMsg::Lineweight(field, weight) => {
                self.dsd.open_list = None;
                if let Some(e) = self.dsd.edit.as_mut() {
                    let value = lineweight_value(weight);
                    if field == "dimlwe" {
                        e.draft.dimlwe = value;
                    } else {
                        e.draft.dimlwd = value;
                    }
                }
            }
            DsMsg::PickChoice(value) => match self.dsd.pick.as_mut() {
                Some(Pick::Linetype(_, name) | Pick::UserArrow(_, name)) => *name = value,
                None => {}
            },
            DsMsg::PickCancel => self.dsd.pick = None,
            DsMsg::PickOk => match self.dsd.pick.take() {
                Some(Pick::Linetype(field, name)) if !name.is_empty() => {
                    self.ds_set_linetype(field, &name);
                }
                Some(Pick::UserArrow(slot, name)) if !name.is_empty() => {
                    self.ds_set_arrow(slot, &name);
                }
                _ => {}
            },
            // Load… lists the program's linetypes first; File… picks a
            // `.lin` file instead.
            DsMsg::PickLoad => {
                let source = crate::io::linetypes::catalog_source();
                self.dsd.load = Some(LoadDraft {
                    file: String::new(),
                    dir: None,
                    entries: crate::io::linetypes::source_entries(&source),
                    source,
                    picked: Vec::new(),
                });
            }
            DsMsg::LoadFile => {
                return Task::perform(crate::io::pick_linetype_file_named(), |picked| {
                    Message::DimStyleDlg(DsMsg::LoadFileChosen(picked))
                });
            }
            DsMsg::LoadFileChosen(Some(file)) => {
                if let Some(load) = self.dsd.load.as_mut() {
                    load.entries = crate::io::linetypes::source_entries(&file.source);
                    load.file = file.name;
                    load.source = file.source;
                    load.dir = file.dir;
                    load.picked.clear();
                }
            }
            DsMsg::LoadFileChosen(None) => {}
            DsMsg::LoadToggle(name) => {
                if let Some(load) = self.dsd.load.as_mut() {
                    if let Some(at) = load.picked.iter().position(|p| p.eq_ignore_ascii_case(&name)) {
                        load.picked.remove(at);
                    } else {
                        load.picked.push(name);
                    }
                }
            }
            DsMsg::LoadCancel => self.dsd.load = None,
            DsMsg::LoadOk => {
                if let Some(load) = self.dsd.load.take() {
                    let doc = &mut self.tabs[i].scene.document;
                    crate::io::linetypes::populate_document_named(
                        doc,
                        &load.source,
                        load.dir.as_deref(),
                        &load.picked,
                    );
                    // The last loaded linetype is the one picked.
                    if let (Some(Pick::Linetype(_, selected)), Some(last)) =
                        (self.dsd.pick.as_mut(), load.picked.last())
                    {
                        *selected = last.clone();
                    }
                }
            }
            DsMsg::TextStyles => return self.ds_open_text_styles(),
            DsMsg::Ok => return self.ds_ok(),
            DsMsg::Cancel => {
                self.dsd.edit = None;
                self.dsd.pick = None;
                self.dsd.open_list = None;
            }
        }
        Task::none()
    }

    fn ds_set_arrow(&mut self, slot: u8, value: &str) {
        let i = self.active_tab;
        let doc = &mut self.tabs[i].scene.document;
        // A standard arrowhead's block is made on first use.
        let Some(handle) = crate::entities::arrowheads::resolve(doc, value) else {
            return;
        };
        let Some(e) = self.dsd.edit.as_mut() else {
            return;
        };
        let d = &mut e.draft;
        match slot {
            // The first arrowhead sets the second too.
            0 => {
                d.dimblk = handle;
                d.dimblk1 = handle;
                d.dimblk2 = handle;
                d.dimsah = false;
            }
            1 => {
                let first = if d.dimsah { d.dimblk1 } else { d.dimblk };
                if handle == first {
                    d.dimblk = first;
                    d.dimblk1 = first;
                    d.dimblk2 = first;
                    d.dimsah = false;
                } else {
                    d.dimblk1 = first;
                    d.dimblk2 = handle;
                    d.dimsah = true;
                }
            }
            _ => d.dimldrblk = handle,
        }
    }

    fn ds_set_linetype(&mut self, field: &str, value: &str) {
        let handle = linetype_handle(&self.tabs[self.active_tab].scene.document, value);
        if let Some(e) = self.dsd.edit.as_mut() {
            match field {
                "dimltex1" => e.draft.dimltex1_handle = handle,
                "dimltex2" => e.draft.dimltex2_handle = handle,
                _ => e.draft.dimltex_handle = handle,
            }
        }
    }

    fn ds_create_continue(&mut self) -> Task<Message> {
        let Some(draft) = self.dsd.create.clone() else {
            return Task::none();
        };
        let name = if draft.use_for == 0 {
            draft.name.clone()
        } else {
            format!("{}{}", draft.start_with, family_suffix(draft.use_for))
        };
        if draft.use_for == 0 {
            if let Some(notice) = self.ds_name_notice(&name) {
                self.dsd.notice = Some(notice);
                return Task::none();
            }
        }
        let i = self.active_tab;
        // A family member that exists already is edited in place.
        if let Some(existing) = self.ds_style(&name).cloned() {
            self.dsd.create = None;
            let extras = dim_style_extras(&self.tabs[i].scene.document, existing.handle);
            let title = format!("{}: {}", draft.start_with, ui::use_for_label(draft.use_for));
            self.ds_begin_edit(Mode::Modify, title, existing, extras);
            return Task::none();
        }
        let Some(mut style) = self.ds_style(&draft.start_with).cloned() else {
            return Task::none();
        };
        let extras = dim_style_extras(&self.tabs[i].scene.document, style.handle);
        style.name = name.clone();
        style.handle = Handle::NULL;
        style.xref_dependent = false;
        style.annotative = draft.annotative;
        let title = if draft.use_for == 0 {
            name
        } else {
            format!("{}: {}", draft.start_with, ui::use_for_label(draft.use_for))
        };
        let previous = self.dimstyle_selected.clone();
        self.dsd.create = None;
        self.ds_begin_edit(Mode::New(previous), title, style, extras);
        Task::none()
    }

    /// The first typed value OK cannot take, by field.
    fn ds_invalid_field(&self) -> Option<&'static str> {
        let e = self.dsd.edit.as_ref()?;
        ds_validate(e).err().map(|(field, _)| field)
    }

    fn ds_ok(&mut self) -> Task<Message> {
        let Some(e) = self.dsd.edit.as_ref() else {
            return Task::none();
        };
        if let Err((field, rule)) = ds_validate(e) {
            if let Some(e) = self.dsd.edit.as_mut() {
                e.tab = field_tab(field);
            }
            self.dsd.notice = Some(Notice::Value(tr!("dim-style", "invalid-value"), rule));
            return Task::none();
        }
        let Some(e) = self.dsd.edit.take() else {
            return Task::none();
        };
        self.dsd.pick = None;
        self.dsd.open_list = None;
        let (style, extras) = ds_commit_draft(&e);
        let i = self.active_tab;
        match e.mode {
            Mode::Modify => {
                let doc = &mut self.tabs[i].scene.document;
                let handle = style.handle;
                let name = style.name.clone();
                doc.dim_styles.add_or_replace(style);
                set_dim_style_extras(doc, handle, extras);
                self.ds_refresh_dimensions(&name);
            }
            Mode::New(_) => {
                let doc = &mut self.tabs[i].scene.document;
                let mut style = style;
                style.handle = doc.allocate_handle();
                let handle = style.handle;
                let name = style.name.clone();
                let _ = doc.dim_styles.add(style);
                set_dim_style_extras(doc, handle, extras);
                self.dimstyle_selected = name;
                self.dsd.overrides_selected = false;
            }
            Mode::Override => {
                let current = self.ds_current();
                if let Some(base) = self.ds_style(&current).cloned() {
                    let mut pairs = crate::entities::dim_override::diff_pairs(&base, &style);
                    let base_extras = dim_style_extras(&self.tabs[i].scene.document, base.handle);
                    if extras.dimtaln != base_extras.dimtaln {
                        pairs.push((
                            crate::entities::dim_override::DIMTALN,
                            document::xdata::XDataValue::Integer16(extras.dimtaln),
                        ));
                    }
                    self.tabs[i].scene.dim_overrides =
                        (!pairs.is_empty()).then_some((current.clone(), pairs));
                    self.dimstyle_selected = current;
                    self.dsd.overrides_selected = self.tabs[i].scene.dim_overrides.is_some();
                }
            }
        }
        self.after_style_change(StyleKind::Dim);
        Task::none()
    }

    /// Every dimension on `name` is drawn again from its changed style.
    fn ds_refresh_dimensions(&mut self, name: &str) {
        let i = self.active_tab;
        let stale: Vec<Handle> = self.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Dimension(dim)
                    if dim.base().style_name.eq_ignore_ascii_case(name) =>
                {
                    Some(entity.common().handle)
                }
                _ => None,
            })
            .collect();
        for handle in stale {
            if let Some(document::EntityType::Dimension(dimension)) =
                self.tabs[i].scene.document.get_entity_mut(handle)
            {
                crate::entities::dimension::reset_automatic_text_position(dimension.base_mut());
            }
            self.tabs[i].scene.invalidate_dim_block_recorded(handle);
        }
    }

    /// Text style "…": the Text Style manager opens as a child of this
    /// window's transaction, so the whole session stays one undo step.
    fn ds_open_text_styles(&mut self) -> Task<Message> {
        if self.dsd.edit.is_none() {
            return Task::none();
        }
        self.dsd.open_list = None;
        let parent = self.style_stage.take();
        let task = self.on_text_style_dialog_open();
        self.style_parent_stage = parent;
        task
    }
}

/// A number typed into a field that changes how the face looks (the
/// preview, the enabled fields) goes into the draft as it is typed.
fn ds_live_value(e: &mut Edit, field: &str, v: f64) {
    let d = &mut e.draft;
    match field {
        "dimdle" => d.dimdle = v,
        "dimdli" => d.dimdli = v,
        "dimexe" => d.dimexe = v,
        "dimexo" => d.dimexo = v,
        "dimfxl" => d.dimfxl = v,
        "dimasz" => d.dimasz = v,
        "dimcen" if d.dimcen != 0.0 => d.dimcen = v.abs() * d.dimcen.signum(),
        "dimjogang" => d.dimjogang = v.to_radians(),
        "dimtxt" => d.dimtxt = v,
        "dimtfac" => d.dimtfac = v,
        "dimgap" => d.dimgap = if d.dimgap < 0.0 { -v.abs() } else { v.abs() },
        "dimscale" if d.dimscale != 0.0 || d.annotative => d.dimscale = v,
        "dimrnd" => d.dimrnd = v,
        "dimlfac" => d.dimlfac = if d.dimlfac < 0.0 { -v.abs() } else { v.abs() },
        "dimaltf" => d.dimaltf = v,
        "dimaltrnd" => d.dimaltrnd = v,
        "dimtp" => {
            if ui::tolerance_method(d) == 1 {
                d.dimtm = v;
            }
            d.dimtp = v;
        }
        "dimtm" => d.dimtm = v,
        _ => {}
    }
}

fn ds_set(e: &mut Edit, field: &str, code: &str, text_style: Option<(String, Handle)>) {
    let value: i16 = code.parse().unwrap_or(0);
    let typed = |key: &str| e.text(key).trim().parse::<f64>().ok().filter(|v| v.is_finite());
    let center_size = typed("dimcen").map(f64::abs).unwrap_or(2.5);
    let tp = typed("dimtp").unwrap_or(0.0);
    let overall = typed("dimscale").filter(|v| *v > 0.0).unwrap_or(1.0);
    let d = &mut e.draft;
    match field {
        "dimtxsty" => {
            if let Some((name, handle)) = text_style {
                d.dimtxsty = name;
                d.dimtxsty_handle = handle;
            }
        }
        "center" => {
            d.dimcen = match value {
                1 => center_size,
                2 => -center_size,
                _ => 0.0,
            }
        }
        "dimarcsym" => d.dimarcsym = value,
        "dimtad" => d.dimtad = value,
        "dimjust" => d.dimjust = value,
        "dimtxtdirection" => d.dimtxtdirection = value == 1,
        "text_align" => {
            (d.dimtih, d.dimtoh) = match value {
                0 => (true, true),
                1 => (false, false),
                _ => (false, true),
            }
        }
        "fit" => {
            d.dimtix = value == 4;
            match value {
                0 => d.dimatfit = 3,
                1 => d.dimatfit = 1,
                2 => d.dimatfit = 2,
                3 => d.dimatfit = 0,
                _ => {}
            }
        }
        "dimtmove" => d.dimtmove = value,
        "scale_mode" => {
            d.dimscale = if value == 1 { 0.0 } else { overall };
        }
        "dimlunit" => {
            d.dimlunit = value;
            d.dimdec = d.dimdec.clamp(0, 8);
        }
        "dimdec" => d.dimdec = value,
        "dimfrac" => d.dimfrac = value,
        "dimdsep" => d.dimdsep = value,
        "dimaunit" => d.dimaunit = value,
        "dimadec" => d.dimadec = value,
        "dimaltu" => d.dimaltu = value,
        "dimaltd" => d.dimaltd = value,
        "dimalttd" => d.dimalttd = value,
        "alt_place" => {
            let template = d.dimpost.replace("\\X", "");
            d.dimpost = if value == 1 {
                format!("\\X{template}")
            } else {
                template
            };
        }
        "tol_method" => {
            let gap = d.dimgap.abs();
            match value {
                1 => {
                    d.dimtol = true;
                    d.dimlim = false;
                    d.dimtm = tp;
                    d.dimgap = gap;
                }
                2 => {
                    d.dimtol = true;
                    d.dimlim = false;
                    d.dimgap = gap;
                    // A deviation needs two values to tell it apart.
                    if (d.dimtp - d.dimtm).abs() < 1e-12 {
                        d.dimtm = -0.0;
                        d.dimtp = tp;
                    }
                }
                3 => {
                    d.dimtol = false;
                    d.dimlim = true;
                    d.dimgap = gap;
                }
                4 => {
                    d.dimtol = false;
                    d.dimlim = false;
                    d.dimgap = -gap.max(f64::EPSILON);
                }
                _ => {
                    d.dimtol = false;
                    d.dimlim = false;
                    d.dimgap = gap;
                }
            }
            if value == 1 {
                let tp_text = e.text("dimtp").to_string();
                e.set_text("dimtm", tp_text);
            }
            return;
        }
        "dimtdec" => d.dimtdec = value,
        "dimtolj" => d.dimtolj = value,
        "dimtaln" => {
            e.dimtaln = value;
            return;
        }
        _ => {}
    }
}

fn ds_toggle(e: &mut Edit, field: &str, on: bool) {
    let d = &mut e.draft;
    match field {
        "dimsd1" => d.dimsd1 = on,
        "dimsd2" => d.dimsd2 = on,
        "dimse1" => d.dimse1 = on,
        "dimse2" => d.dimse2 = on,
        "dimfxlon" => d.dimfxlon = on,
        "frame" => d.dimgap = if on { -d.dimgap.abs().max(f64::EPSILON) } else { d.dimgap.abs() },
        "annotative" => d.annotative = on,
        "dimsoxd" => d.dimsoxd = on,
        "dimupt" => d.dimupt = on,
        "dimtofl" => d.dimtofl = on,
        "dimalt" => d.dimalt = on,
        "layout_only" => d.dimlfac = if on { -d.dimlfac.abs() } else { d.dimlfac.abs() },
        "dimazin.lead" => d.dimazin = (d.dimazin & !1) | i16::from(on),
        "dimazin.trail" => d.dimazin = (d.dimazin & !2) | (i16::from(on) << 1),
        _ => {
            let Some((var, part)) = field.split_once('.') else {
                return;
            };
            let bit = match part {
                "lead" => 0,
                "trail" => 1,
                "feet" => 2,
                _ => 3,
            };
            let slot = match var {
                "dimzin" => &mut d.dimzin,
                "dimaltz" => &mut d.dimaltz,
                "dimtzin" => &mut d.dimtzin,
                "dimalttz" => &mut d.dimalttz,
                _ => return,
            };
            *slot = with_zero_flag(*slot, bit, on);
        }
    }
}

/// The typed values of the face: each field's rule, by field. A field the
/// face shows inert is not checked.
fn ds_validate(e: &Edit) -> Result<(), (&'static str, String)> {
    let num = |field: &'static str| -> Result<f64, (&'static str, String)> {
        e.text(field)
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| (field, tr!("dim-style", "rule-number")))
    };
    let non_negative = |field: &'static str| -> Result<f64, (&'static str, String)> {
        let v = num(field)?;
        if v < 0.0 {
            return Err((field, tr!("dim-style", "rule-non-negative")));
        }
        Ok(v)
    };
    let positive = |field: &'static str| -> Result<f64, (&'static str, String)> {
        let v = num(field)?;
        if v <= 0.0 {
            return Err((field, tr!("dim-style", "rule-positive")));
        }
        Ok(v)
    };
    let d = &e.draft;
    for field in ["dimdli", "dimexe", "dimexo", "dimasz", "dimbreak", "dimtxt", "dimgap", "dimrnd"] {
        non_negative(field)?;
    }
    if draws_ticks_hint(d) {
        non_negative("dimdle")?;
    }
    if d.dimfxlon {
        non_negative("dimfxl")?;
    }
    if center_mode(d.dimcen) != 0 {
        non_negative("dimcen")?;
    }
    let jog = num("dimjogang")?;
    if !(5.0..=90.0).contains(&jog) {
        return Err(("dimjogang", tr!("dim-style", "rule-jog-angle")));
    }
    positive("dimjag")?;
    if ui::linear_kind_is_fraction(d.dimlunit) || ui::tolerance_method(d) != 0 {
        positive("dimtfac")?;
    }
    if !d.annotative && d.dimscale != 0.0 {
        positive("dimscale")?;
    }
    let lfac = num("dimlfac")?;
    if lfac == 0.0 {
        return Err(("dimlfac", tr!("dim-style", "rule-nonzero")));
    }
    if ui::zero_flag(d.dimzin, 0) {
        positive("dimmzf")?;
    }
    if d.dimalt {
        positive("dimaltf")?;
        non_negative("dimaltrnd")?;
        if ui::zero_flag(d.dimaltz, 0) {
            positive("dimaltmzf")?;
        }
    }
    if matches!(ui::tolerance_method(d), 1..=3) {
        num("dimtp")?;
        num("dimtm")?;
    }
    Ok(())
}

fn draws_ticks_hint(d: &DimStyle) -> bool {
    d.dimtsz > 0.0
}

/// The style and extras OK writes: the draft with the typed values.
fn ds_commit_draft(e: &Edit) -> (DimStyle, DimStyleExtras) {
    let mut d = e.draft.clone();
    let typed = |field: &str, fallback: f64| -> f64 {
        e.text(field)
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .unwrap_or(fallback)
    };
    d.dimdle = typed("dimdle", d.dimdle);
    d.dimdli = typed("dimdli", d.dimdli);
    d.dimexe = typed("dimexe", d.dimexe);
    d.dimexo = typed("dimexo", d.dimexo);
    d.dimfxl = typed("dimfxl", d.dimfxl);
    d.dimasz = typed("dimasz", d.dimasz);
    d.dimcen = match center_mode(d.dimcen) {
        1 => typed("dimcen", d.dimcen).abs(),
        2 => -typed("dimcen", d.dimcen).abs(),
        _ => 0.0,
    };
    d.dimjogang = typed("dimjogang", d.dimjogang.to_degrees()).to_radians();
    d.dimtxt = typed("dimtxt", d.dimtxt);
    d.dimtfac = typed("dimtfac", d.dimtfac);
    let gap = typed("dimgap", d.dimgap.abs()).abs();
    d.dimgap = if d.dimgap < 0.0 { -gap.max(f64::EPSILON) } else { gap };
    if d.annotative {
        d.dimscale = 0.0;
    } else if d.dimscale != 0.0 {
        d.dimscale = typed("dimscale", d.dimscale);
    }
    d.dimrnd = typed("dimrnd", d.dimrnd);
    // A template whose prefix and suffix were not changed stays as written
    // ("<>" and "" both read as neither).
    let below = alt_below(&d);
    let typed_post = (e.text("prefix").to_string(), e.text("suffix").to_string());
    if split_template(&d.dimpost.replace("\\X", ""), "<>") != typed_post {
        let template = join_template(&typed_post.0, &typed_post.1, "<>");
        d.dimpost = if below {
            format!("\\X{template}")
        } else {
            template
        };
    }
    let lfac = typed("dimlfac", d.dimlfac.abs()).abs();
    d.dimlfac = if d.dimlfac < 0.0 { -lfac } else { lfac };
    d.dimmzf = typed("dimmzf", d.dimmzf);
    d.dimmzs = e.text("dimmzs").to_string();
    d.dimaltf = typed("dimaltf", d.dimaltf);
    d.dimaltrnd = typed("dimaltrnd", d.dimaltrnd);
    let typed_alt = (e.text("alt_prefix").to_string(), e.text("alt_suffix").to_string());
    if split_template(&d.dimapost, "[]") != typed_alt {
        d.dimapost = join_template(&typed_alt.0, &typed_alt.1, "[]");
    }
    d.dimaltmzf = typed("dimaltmzf", d.dimaltmzf);
    d.dimaltmzs = e.text("dimaltmzs").to_string();
    d.dimtp = typed("dimtp", d.dimtp);
    d.dimtm = if ui::tolerance_method(&e.draft) == 1 {
        d.dimtp
    } else {
        typed("dimtm", d.dimtm)
    };
    let extras = DimStyleExtras {
        jog_height: typed("dimjag", 1.5),
        break_size: typed("dimbreak", 3.75),
        dimtaln: e.dimtaln,
    };
    (d, extras)
}
