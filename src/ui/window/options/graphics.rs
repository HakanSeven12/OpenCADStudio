//! Options > Graphics: which graphics backend the next launch uses.
//!
//! Everything here is read before the application boots (the backend is
//! chosen first), so changes are saved at once and apply on restart rather
//! than going through OK / Apply.

use crate::app::Message;
use crate::gpu_backend::{ActiveGpu, BackendChoice, GraphicsPrefs};
use iced::widget::{checkbox, column, pick_list, row, text, Space};
use iced::{Element, Fill};

fn current_row(label: std::borrow::Cow<'static, str>, value: String) -> Element<'static, Message> {
    row![
        text(label).size(12).width(150),
        text(value).size(12).width(Fill),
    ]
    .spacing(10)
    .into()
}

fn unknown(s: &str) -> String {
    if s.is_empty() {
        crate::t!("Unknown").into_owned()
    } else {
        s.to_string()
    }
}

pub(crate) fn view(prefs: GraphicsPrefs, active: ActiveGpu) -> Element<'static, Message> {
    let backend = pick_list(
        Some(prefs.backend),
        BackendChoice::available(),
        |choice: &BackendChoice| crate::t!(choice.label()).into_owned(),
    )
    .on_select(Message::GraphicsBackendChanged)
    .width(260);

    let mut col = column![
        text(crate::t!("Graphics")).size(15),
        Space::new().height(6),
        text(crate::t!(
            "Saved immediately. Changes take effect the next time Open CAD Studio starts."
        ))
        .size(11)
        .width(Fill),
        Space::new().height(16),
        text(crate::t!("This session")).size(13),
        Space::new().height(8),
        current_row(
            crate::t!("GPU"),
            unknown(active.adapter.as_deref().unwrap_or(""))
        ),
        Space::new().height(4),
        current_row(crate::t!("Driver"), unknown(&active.driver)),
        Space::new().height(4),
        current_row(
            crate::t!("Backend"),
            match active.backend.as_deref() {
                Some("dx12") => "DirectX 12".to_string(),
                Some("vulkan") => "Vulkan".to_string(),
                Some("gl") => "OpenGL".to_string(),
                Some("metal") => "Metal".to_string(),
                Some(other) => other.to_string(),
                None => {
                    if cfg!(target_os = "macos") {
                        "Metal".to_string()
                    } else {
                        unknown("")
                    }
                }
            },
        ),
        Space::new().height(4),
        current_row(crate::t!("Selected by"), unknown(&active.origin)),
        Space::new().height(20),
        text(crate::t!("Next launch")).size(13),
        Space::new().height(8),
        row![text(crate::t!("Graphics backend")).size(12).width(150), backend]
            .spacing(10)
            .align_y(iced::Center),
        Space::new().height(12),
        row![
            checkbox(prefs.compat_renderer)
                .on_toggle(Message::GraphicsCompatToggled)
                .size(15),
            text(crate::t!(
                "Use the compatibility renderer (for GPUs without shader storage buffers)"
            ))
            .size(12),
        ]
        .spacing(8)
        .align_y(iced::Center),
    ];

    if cfg!(target_os = "windows") {
        col = col.push(Space::new().height(10)).push(
            row![
                checkbox(prefs.legacy_gl)
                    .on_toggle(Message::GraphicsLegacyGlToggled)
                    .size(15),
                text(crate::t!(
                    "Prefer OpenGL on older GPUs (automatic backend only)"
                ))
                .size(12),
            ]
            .spacing(8)
            .align_y(iced::Center),
        );
    }

    col.push(Space::new().height(16))
        .push(
            text(crate::t!(
                "If a pinned backend fails to start, the next launch falls back to automatic selection. \
                 The --backend option and the WGPU_BACKEND variable override this page."
            ))
            .size(11)
            .width(Fill),
        )
        .spacing(0)
        .into()
}
