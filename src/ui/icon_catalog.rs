//! Stable command-icon identities and asset associations.
//!
//! This module deliberately contains no SVG rendering implementation. It maps
//! commands to logical icon IDs and delegates enabled/disabled rendering to
//! [`crate::ui::icons`]. UI surfaces therefore do not need to know asset paths.

use iced::Element;

static LINE: &[u8] = include_bytes!("../../assets/icons/line.svg");
static POLYLINE: &[u8] = include_bytes!("../../assets/icons/polyline.svg");
static RECTANGLE: &[u8] = include_bytes!("../../assets/icons/shapes/rect.svg");
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
static ISOLATE: &[u8] = include_bytes!("../../assets/icons/status/isolate.svg");
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
    Rectangle,
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
    /// Built-in artwork registered directly by the command catalog.
    Builtin {
        key: &'static str,
        svg: &'static [u8],
    },
}

/// Resolve an icon ID to its source artwork.
pub const fn bytes(id: IconId) -> &'static [u8] {
    use IconId::*;
    match id {
        Line => LINE,
        Polyline => POLYLINE,
        Rectangle => RECTANGLE,
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
        Undo => crate::ui::icons::undo_icon(),
        Redo => crate::ui::icons::redo_icon(),
        Isolate => ISOLATE,
        Pan => crate::ui::icons::pan_icon(),
        Zoom => crate::ui::icons::zoom_icon(),
        ZoomExtents => ZOOM_EXTENTS,
        Properties => PROPERTIES,
        Options => OPTIONS,
        New => NEW,
        Open => OPEN,
        Save => SAVE,
        SaveAs => SAVE_AS,
        Print => PRINT,
        Builtin { svg, .. } => svg,
    }
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
    fn every_icon_id_has_nonempty_artwork() {
        for id in [
            IconId::Line,
            IconId::Polyline,
            IconId::Rectangle,
            IconId::Circle,
            IconId::CircleDiameter,
            IconId::Circle2Point,
            IconId::Circle3Point,
            IconId::CircleTangentRadius,
            IconId::CircleThreeTangents,
            IconId::Arc3Point,
            IconId::ArcStartCenterEnd,
            IconId::ArcStartCenterAngle,
            IconId::ArcStartCenterLength,
            IconId::ArcStartEndAngle,
            IconId::ArcStartEndDirection,
            IconId::ArcStartEndRadius,
            IconId::ArcCenterStartEnd,
            IconId::ArcCenterStartAngle,
            IconId::ArcCenterStartLength,
            IconId::ArcContinue,
            IconId::Cut,
            IconId::CopyClipboard,
            IconId::Paste,
            IconId::Erase,
            IconId::Move,
            IconId::Copy,
            IconId::Scale,
            IconId::Rotate,
            IconId::Mirror,
            IconId::Stretch,
            IconId::DrawOrder,
            IconId::Undo,
            IconId::Redo,
            IconId::Isolate,
            IconId::Pan,
            IconId::Zoom,
            IconId::ZoomExtents,
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
