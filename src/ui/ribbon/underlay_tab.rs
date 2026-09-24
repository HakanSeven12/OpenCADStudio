//! The contextual "PDF Underlay" tab: shown and brought forward while only
//! PDF underlays are selected, gone when the selection changes.

use iced::widget::{column, row, slider, text, text_input};
use iced::{Element, Length};

use super::*;
use crate::modules::{ModuleEvent, ToolDef};

/// What the tab shows for the selected underlay (the first one).
#[derive(Clone, Debug, PartialEq)]
pub struct UnderlayContext {
    pub contrast: u8,
    pub fade: u8,
    pub monochrome: bool,
    pub shown: bool,
    pub snap: bool,
    pub contrast_text: String,
    pub fade_text: String,
}

impl UnderlayContext {
    pub fn new(contrast: u8, fade: u8, monochrome: bool, shown: bool, snap: bool) -> Self {
        Self {
            contrast,
            fade,
            monochrome,
            shown,
            snap,
            contrast_text: contrast.to_string(),
            fade_text: fade.to_string(),
        }
    }
}

/// The tab's edits; the host writes them to the selected underlays.
#[derive(Clone, Debug)]
pub enum UnderlayTabMsg {
    Contrast(u8),
    Fade(u8),
    ContrastText(String),
    FadeText(String),
    /// Slider released or value submitted: write contrast and fade.
    Commit,
}

pub(super) const ADJUST: &str = "Adjust";

fn tool(id: &'static str, label: &'static str, icon: &'static [u8]) -> ToolDef {
    ToolDef {
        id,
        label,
        icon: IconKind::Svg(icon),
        event: ModuleEvent::Command(id.to_string()),
    }
}

fn groups() -> &'static [RibbonGroup] {
    static GROUPS: std::sync::OnceLock<Vec<RibbonGroup>> = std::sync::OnceLock::new();
    const FRAMES: &[u8] = include_bytes!("../../../assets/icons/underlay_frames.svg");
    const CLIP: &[u8] = include_bytes!("../../../assets/icons/xclip.svg");
    GROUPS.get_or_init(|| {
        vec![
            RibbonGroup {
                title: ADJUST,
                tools: vec![RibbonItem::LargeTool(tool(
                    "_PDFULMONO",
                    "Display in\nMonochrome",
                    FRAMES,
                ))],
            },
            RibbonGroup {
                title: "Clipping",
                tools: vec![
                    RibbonItem::LargeTool(tool("_PDFULCLIP", "Create Clipping\nBoundary", CLIP)),
                    RibbonItem::LargeTool(tool("_PDFULUNCLIP", "Remove\nClipping", CLIP)),
                ],
            },
            RibbonGroup {
                title: "Options",
                tools: vec![
                    RibbonItem::LargeTool(tool("_PDFULSHOW", "Show\nUnderlay", FRAMES)),
                    RibbonItem::LargeTool(tool(
                        "_PDFULSNAP",
                        "Enable\nSnap",
                        include_bytes!("../../../assets/icons/snap_underlays.svg"),
                    )),
                    RibbonItem::LargeTool(tool(
                        "EXTERNALREFERENCES",
                        "External\nReferences",
                        crate::ui::icons::FOLDER_OPEN,
                    )),
                ],
            },
            RibbonGroup {
                title: "PDF Layers",
                tools: vec![RibbonItem::LargeTool(tool(
                    "ULAYERS",
                    "Edit\nLayers",
                    include_bytes!("../../../assets/icons/underlay_layers.svg"),
                ))],
            },
            RibbonGroup {
                title: "PDF Import",
                tools: vec![RibbonItem::LargeTool(tool(
                    "_PDFULIMPORT",
                    "Import As\nObjects",
                    include_bytes!("../../../assets/icons/cui_import.svg"),
                ))],
            },
        ]
    })
}

/// Contrast / Fade: a label, a 0–100 slider and the value box.
fn adjust_rows<'a>(ctx: &'a UnderlayContext) -> Element<'a, Message> {
    let line = |label: &'static str,
                value: u8,
                buffer: &'a str,
                on_slide: fn(u8) -> UnderlayTabMsg,
                on_text: fn(String) -> UnderlayTabMsg| {
        row![
            text(t!(label)).size(11).width(Length::Fixed(64.0)),
            slider(0..=100u8, value, move |v| Message::UnderlayTab(on_slide(v)))
                .on_release(Message::UnderlayTab(UnderlayTabMsg::Commit))
                .width(Length::Fixed(110.0)),
            text_input("", buffer)
                .on_input(move |s| Message::UnderlayTab(on_text(s)))
                .on_submit(Message::UnderlayTab(UnderlayTabMsg::Commit))
                .size(11)
                .padding([2, 4])
                .width(Length::Fixed(44.0)),
        ]
        .spacing(6)
        .align_y(iced::Center)
    };
    column![
        line(
            "Contrast",
            ctx.contrast,
            &ctx.contrast_text,
            UnderlayTabMsg::Contrast,
            UnderlayTabMsg::ContrastText,
        ),
        line(
            "Fade",
            ctx.fade,
            &ctx.fade_text,
            UnderlayTabMsg::Fade,
            UnderlayTabMsg::FadeText,
        ),
    ]
    .spacing(8)
    .padding([6, 4])
    .into()
}

impl Ribbon {
    /// Show the tab for the selected underlay, or drop it (`None`). A tab
    /// that appears comes to the front; one that goes returns the tab that
    /// was active before it.
    pub fn set_underlay_context(&mut self, context: Option<UnderlayContext>) {
        match (&self.underlay_ctx, &context) {
            (None, Some(_)) => self.underlay_tab_active = true,
            (Some(_), None) => self.underlay_tab_active = false,
            _ => {}
        }
        // A value being typed survives a refresh that leaves the value as is.
        let mut context = context;
        if let (Some(old), Some(new)) = (&self.underlay_ctx, &mut context) {
            if (old.contrast, old.fade) == (new.contrast, new.fade) {
                new.contrast_text = old.contrast_text.clone();
                new.fade_text = old.fade_text.clone();
            }
        }
        self.underlay_ctx = context;
    }

    pub fn underlay_context_mut(&mut self) -> Option<&mut UnderlayContext> {
        self.underlay_ctx.as_mut()
    }

    /// Select a tab by its module id; "pdf_underlay" is the contextual tab.
    pub fn select_by_id(&mut self, id: &str) -> bool {
        if id == "pdf_underlay" {
            self.select_underlay_tab();
            return self.underlay_tab_active;
        }
        match self.modules.iter().position(|m| m.id() == id) {
            Some(index) => {
                self.select(index);
                true
            }
            None => false,
        }
    }

    pub fn select_underlay_tab(&mut self) {
        if self.underlay_ctx.is_some() {
            self.underlay_tab_active = true;
        }
    }

    pub(super) fn underlay_tab_shown(&self) -> bool {
        self.underlay_tab_active && self.underlay_ctx.is_some()
    }

    pub(super) fn underlay_panels<'a>(
        &'a self,
        ts: widgets::ToggleState,
        style_ctx: &StyleContext<'_>,
    ) -> Vec<Panel<'a>> {
        let Some(ctx) = self.underlay_ctx.as_ref() else {
            return Vec::new();
        };
        groups()
            .iter()
            .map(|g| {
                let lead = |_: bool| -> Vec<Element<'a, Message>> {
                    if g.title == ADJUST {
                        vec![adjust_rows(ctx)]
                    } else {
                        Vec::new()
                    }
                };
                self.panel(g, ts, style_ctx, &lead)
            })
            .collect()
    }
}
