//! Stable command-icon identities and asset associations.
//!
//! This module deliberately contains no SVG rendering implementation. It maps
//! commands to logical icon IDs and delegates enabled/disabled rendering to
//! [`crate::ui::icons`]. UI surfaces therefore do not need to know asset paths.

use iced::Element;

static LINE: &[u8] = include_bytes!("../../assets/icons/line.svg");
static POLYLINE: &[u8] = include_bytes!("../../assets/icons/polyline.svg");
static CIRCLE: &[u8] = include_bytes!("../../assets/icons/circle/circle_cr.svg");
static CIRCLE_CD: &[u8] = include_bytes!("../../assets/icons/circle/circle_cd.svg");
static CIRCLE_2P: &[u8] = include_bytes!("../../assets/icons/circle/circle_2p.svg");
static CIRCLE_3P: &[u8] = include_bytes!("../../assets/icons/circle/circle_3p.svg");
static CIRCLE_TTR: &[u8] = include_bytes!("../../assets/icons/circle/circle_ttr.svg");
static CIRCLE_TTT: &[u8] = include_bytes!("../../assets/icons/circle/circle_ttt.svg");
static ARC_3P: &[u8] = include_bytes!("../../assets/icons/arc/arc_3p.svg");
static ARC_SCE: &[u8] = include_bytes!("../../assets/icons/arc/arc_sce.svg");
static ARC_SCA: &[u8] = include_bytes!("../../assets/icons/arc/arc_sca.svg");
static ARC_SCL: &[u8] = include_bytes!("../../assets/icons/arc/arc_scl.svg");
static ARC_SEA: &[u8] = include_bytes!("../../assets/icons/arc/arc_sea.svg");
static ARC_SED: &[u8] = include_bytes!("../../assets/icons/arc/arc_sed.svg");
static ARC_SER: &[u8] = include_bytes!("../../assets/icons/arc/arc_ser.svg");
static ARC_CSE: &[u8] = include_bytes!("../../assets/icons/arc/arc_cse.svg");
static ARC_CSA: &[u8] = include_bytes!("../../assets/icons/arc/arc_csa.svg");
static ARC_CSL: &[u8] = include_bytes!("../../assets/icons/arc/arc_csl.svg");
static ARC_CONT: &[u8] = include_bytes!("../../assets/icons/arc/arc_cont.svg");
static CUT: &[u8] = include_bytes!("../../assets/icons/cut.svg");
static COPY_CLIP: &[u8] = include_bytes!("../../assets/icons/copy_clip.svg");
static PASTE: &[u8] = include_bytes!("../../assets/icons/paste.svg");
static ERASE: &[u8] = include_bytes!("../../assets/icons/erase.svg");
static MOVE: &[u8] = include_bytes!("../../assets/icons/move.svg");
static COPY: &[u8] = include_bytes!("../../assets/icons/copy.svg");
static SCALE: &[u8] = include_bytes!("../../assets/icons/scale.svg");
static ROTATE: &[u8] = include_bytes!("../../assets/icons/rotate.svg");
static MIRROR: &[u8] = include_bytes!("../../assets/icons/mirror.svg");
static STRETCH: &[u8] = include_bytes!("../../assets/icons/stretch.svg");
static DRAW_ORDER: &[u8] = include_bytes!("../../assets/icons/modify_draworder.svg");
static UNDO: &[u8] = include_bytes!("../../assets/icons/ui/undo.svg");
static REDO: &[u8] = include_bytes!("../../assets/icons/ui/redo.svg");
static ISOLATE: &[u8] = include_bytes!("../../assets/icons/status/isolate.svg");
static PAN: &[u8] = include_bytes!("../../assets/icons/pan.svg");
static ZOOM: &[u8] = include_bytes!("../../assets/icons/zoom_in.svg");
static ZOOM_EXTENTS: &[u8] = include_bytes!("../../assets/icons/zoom_ext.svg");
static PROPERTIES: &[u8] = include_bytes!("../../assets/icons/properties.svg");
static OPTIONS: &[u8] = include_bytes!("../../assets/icons/options_tool.svg");
static NEW: &[u8] = include_bytes!("../../assets/icons/ui/doc_new.svg");
static OPEN: &[u8] = include_bytes!("../../assets/icons/ui/folder_open.svg");
static SAVE: &[u8] = include_bytes!("../../assets/icons/ui/save.svg");
static SAVE_AS: &[u8] = include_bytes!("../../assets/icons/ui/file_export.svg");
static PRINT: &[u8] = include_bytes!("../../assets/icons/ui/print.svg");

/// Stable icon names, independent of asset paths and UI placement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconId {
    Line,
    Polyline,
    Circle,
    CircleDiameter,
    Circle2Point,
    Circle3Point,
    CircleTangentRadius,
    CircleThreeTangents,
    Arc3Point,
    ArcStartCenterEnd,
    ArcStartCenterAngle,
    ArcStartCenterLength,
    ArcStartEndAngle,
    ArcStartEndDirection,
    ArcStartEndRadius,
    ArcCenterStartEnd,
    ArcCenterStartAngle,
    ArcCenterStartLength,
    ArcContinue,
    Cut,
    CopyClipboard,
    Paste,
    Erase,
    Move,
    Copy,
    Scale,
    Rotate,
    Mirror,
    Stretch,
    DrawOrder,
    Undo,
    Redo,
    Isolate,
    Pan,
    Zoom,
    ZoomExtents,
    Properties,
    Options,
    New,
    Open,
    Save,
    SaveAs,
    Print,
}

/// Resolve an icon ID to its source artwork.
pub const fn bytes(id: IconId) -> &'static [u8] {
    use IconId::*;
    match id {
        Line => LINE,
        Polyline => POLYLINE,
        Circle => CIRCLE,
        CircleDiameter => CIRCLE_CD,
        Circle2Point => CIRCLE_2P,
        Circle3Point => CIRCLE_3P,
        CircleTangentRadius => CIRCLE_TTR,
        CircleThreeTangents => CIRCLE_TTT,
        Arc3Point => ARC_3P,
        ArcStartCenterEnd => ARC_SCE,
        ArcStartCenterAngle => ARC_SCA,
        ArcStartCenterLength => ARC_SCL,
        ArcStartEndAngle => ARC_SEA,
        ArcStartEndDirection => ARC_SED,
        ArcStartEndRadius => ARC_SER,
        ArcCenterStartEnd => ARC_CSE,
        ArcCenterStartAngle => ARC_CSA,
        ArcCenterStartLength => ARC_CSL,
        ArcContinue => ARC_CONT,
        Cut => CUT,
        CopyClipboard => COPY_CLIP,
        Paste => PASTE,
        Erase => ERASE,
        Move => MOVE,
        Copy => COPY,
        Scale => SCALE,
        Rotate => ROTATE,
        Mirror => MIRROR,
        Stretch => STRETCH,
        DrawOrder => DRAW_ORDER,
        Undo => UNDO,
        Redo => REDO,
        Isolate => ISOLATE,
        Pan => PAN,
        Zoom => ZOOM,
        ZoomExtents => ZOOM_EXTENTS,
        Properties => PROPERTIES,
        Options => OPTIONS,
        New => NEW,
        Open => OPEN,
        Save => SAVE,
        SaveAs => SAVE_AS,
        Print => PRINT,
    }
}

/// Resolve a CAD command line to an icon. Exact variants are checked first;
/// command arguments and transparent-command apostrophes are normalized.
pub fn command_icon(command: &str) -> Option<IconId> {
    use IconId::*;
    let normalized = command.trim().trim_start_matches('\'').to_ascii_uppercase();
    let exact = match normalized.as_str() {
        "LINE" => Some(Line),
        "PLINE" | "POLYLINE" => Some(Polyline),
        "CIRCLE" => Some(Circle),
        "CIRCLE_CD" => Some(CircleDiameter),
        "CIRCLE_2P" => Some(Circle2Point),
        "CIRCLE_3P" => Some(Circle3Point),
        "CIRCLE_TTR" => Some(CircleTangentRadius),
        "CIRCLE_TTT" => Some(CircleThreeTangents),
        "ARC" | "ARC_3P" => Some(Arc3Point),
        "ARC_SCE" => Some(ArcStartCenterEnd),
        "ARC_SCA" => Some(ArcStartCenterAngle),
        "ARC_SCL" => Some(ArcStartCenterLength),
        "ARC_SEA" => Some(ArcStartEndAngle),
        "ARC_SED" => Some(ArcStartEndDirection),
        "ARC_SER" => Some(ArcStartEndRadius),
        "ARC_CSE" => Some(ArcCenterStartEnd),
        "ARC_CSA" => Some(ArcCenterStartAngle),
        "ARC_CSL" => Some(ArcCenterStartLength),
        "ARC_CONT" => Some(ArcContinue),
        "CUTCLIP" => Some(Cut),
        "COPYCLIP" | "COPYBASE" => Some(CopyClipboard),
        "PASTE" | "PASTECLIP" | "PASTEBLOCK" | "PASTEORIG" => Some(Paste),
        "ERASE" | "DELETE" => Some(Erase),
        "MOVE" => Some(Move),
        "COPY" => Some(Copy),
        "SCALE" => Some(Scale),
        "ROTATE" => Some(Rotate),
        "MIRROR" => Some(Mirror),
        "STRETCH" => Some(Stretch),
        "DRAWORDER" | "DRAWORDER_FRONT" | "DRAWORDER_BACK" | "DRAWORDER_ABOVE"
        | "DRAWORDER_UNDER" => Some(DrawOrder),
        "UNDO" => Some(Undo),
        "REDO" => Some(Redo),
        "ISOLATEOBJECTS" | "HIDEOBJECTS" | "UNISOLATEOBJECTS" => Some(Isolate),
        "PAN" => Some(Pan),
        "ZOOM" | "ZOOM DYNAMIC" => Some(Zoom),
        "ZOOM EXTENTS" => Some(ZoomExtents),
        "PROPERTIES" => Some(Properties),
        "OPTIONS" => Some(Options),
        "NEW" => Some(New),
        "OPEN" => Some(Open),
        "SAVE" | "QSAVE" => Some(Save),
        "SAVEAS" => Some(SaveAs),
        "PRINT" | "PLOT" => Some(Print),
        _ => None,
    };
    exact.or_else(|| match normalized.split_whitespace().next()? {
        "DRAWORDER" => Some(DrawOrder),
        _ => None,
    })
}

/// Render catalog artwork with the shared semantic enabled/disabled treatment.
pub fn render<'a, M: 'a>(id: IconId, size: f32, enabled: bool) -> Element<'a, M> {
    if enabled {
        crate::ui::icons::semantic(bytes(id), size)
    } else {
        crate::ui::icons::semantic_disabled(bytes(id), size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_lookup_normalizes_variants_and_arguments() {
        assert_eq!(command_icon("circle"), Some(IconId::Circle));
        assert_eq!(command_icon("CIRCLE_2P"), Some(IconId::Circle2Point));
        assert_eq!(command_icon("ARC"), Some(IconId::Arc3Point));
        assert_eq!(command_icon("ARC_3P"), Some(IconId::Arc3Point));
        assert_eq!(command_icon("ARC_SEA"), Some(IconId::ArcStartEndAngle));
        assert_eq!(command_icon("'PAN"), Some(IconId::Pan));
        assert_eq!(command_icon("DRAWORDER F"), Some(IconId::DrawOrder));
        assert_eq!(command_icon("ZOOM EXTENTS"), Some(IconId::ZoomExtents));
        assert_eq!(command_icon("NOT_A_COMMAND"), None);
    }

    #[test]
    fn every_icon_id_has_nonempty_artwork() {
        for id in [
            IconId::Line,
            IconId::Polyline,
            IconId::Circle,
            IconId::Arc3Point,
            IconId::Cut,
            IconId::CopyClipboard,
            IconId::Paste,
            IconId::Erase,
            IconId::Move,
            IconId::DrawOrder,
            IconId::Undo,
            IconId::Redo,
            IconId::Isolate,
            IconId::Pan,
            IconId::Zoom,
            IconId::Properties,
            IconId::Options,
            IconId::New,
            IconId::Open,
            IconId::Save,
            IconId::SaveAs,
            IconId::Print,
        ] {
            assert!(!bytes(id).is_empty(), "missing artwork for {id:?}");
        }
    }
}
