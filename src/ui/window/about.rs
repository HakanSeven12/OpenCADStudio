use crate::app::Message;
use crate::t;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::dialog_button;
use iced::widget::{column, container, row, svg, text, Space};
use iced::{Background, Border, Element, Fill, Length, Shrink, Theme};

fn primary_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().primary.base.color),
    }
}

fn surface_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(
            theme.palette().background.weakest.color,
        )),
        border: Border {
            color: theme.palette().background.neutral.color,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    }
}

fn info_card<'a>(
    label: std::borrow::Cow<'static, str>,
    value: impl iced::widget::text::IntoFragment<'a>,
    width: Length,
) -> Element<'a, Message> {
    container(
        column![
            text(label).size(10).style(muted_style),
            text(value).size(14),
        ]
        .spacing(5),
    )
    .padding([10, 12])
    .width(width)
    .height(Length::Fixed(62.0))
    .style(surface_style)
    .into()
}

/// A full-width label/value line. CPU and GPU names are too long for the
/// fixed-width cards above.
fn info_row<'a>(
    label: std::borrow::Cow<'static, str>,
    value: impl iced::widget::text::IntoFragment<'a>,
    width: Length,
) -> Element<'a, Message> {
    container(
        row![
            text(label).size(11).style(muted_style).width(Length::Fixed(110.0)),
            text(value).size(13).width(Fill),
        ]
        .spacing(8)
        .align_y(iced::Center),
    )
    .padding([8, 12])
    .width(width)
    .style(surface_style)
    .into()
}

/// `Adapter (backend)` for the GPU row.
fn gpu_summary() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        "WebGPU / WebGL".to_string()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::gpu_backend::active_gpu_summary()
    }
}

pub(crate) fn platform_name() -> &'static str {
    #[cfg(target_arch = "wasm32")]
    {
        "Web"
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        match std::env::consts::OS {
            "linux" => "Linux",
            "windows" => "Windows",
            "macos" => "macOS",
            other => other,
        }
    }
}

pub(crate) fn architecture_name() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x86-64",
        "aarch64" => "ARM64",
        "wasm32" => "WebAssembly 32-bit",
        other => other,
    }
}

/// The text `Copy Info` puts on the clipboard: everything a bug report needs
/// about the build and the machine, including which graphics backend the
/// resolver picked and why.
pub(crate) fn report() -> String {
    use crate::sysinfo::{format_bytes, system_info};
    let sys = system_info();
    #[allow(unused_mut)]
    let mut out = format!(
        "Open CAD Studio v{}\nRevision: {}\nCommit date: {}\nProfile: {}\nFeatures: {}\n\n\
         [System]\nOS: {} ({} {})\nCPU: {}\nLogical processors: {}\nRAM: {} total, {} available\n",
        env!("OCS_FULL_VERSION"),
        env!("OCS_GIT_REV"),
        env!("OCS_COMMIT_DATE"),
        env!("OCS_BUILD_PROFILE"),
        env!("OCS_BUILD_FEATURES"),
        sys.os,
        platform_name(),
        architecture_name(),
        sys.cpu,
        sys.logical_cores,
        format_bytes(sys.ram_total),
        format_bytes(sys.ram_available),
    );
    #[cfg(not(target_arch = "wasm32"))]
    {
        use crate::gpu_backend::{active_gpu, load_prefs};
        let gpu = active_gpu();
        let prefs = load_prefs();
        let or_unknown = |s: &str| {
            if s.is_empty() {
                "Unknown".to_string()
            } else {
                s.to_string()
            }
        };
        out.push_str(&format!(
            "\n[Graphics]\nGPU: {}\nDriver: {}\nBackend: {}\nSelected by: {}\n\
             Below WebGPU baseline: {}\nWGPU_BACKEND: {}\n\
             Saved options: backend={}, compat renderer={}, OpenGL on older GPUs={}\n",
            or_unknown(gpu.adapter.as_deref().unwrap_or("")),
            or_unknown(&gpu.driver),
            or_unknown(gpu.backend.as_deref().unwrap_or("")),
            or_unknown(&gpu.origin),
            if gpu.legacy { "yes" } else { "no" },
            std::env::var("WGPU_BACKEND").unwrap_or_else(|_| "(unset)".into()),
            prefs.backend.as_str().unwrap_or("auto"),
            prefs.compat_renderer,
            prefs.legacy_gl,
        ));
    }
    #[cfg(target_arch = "wasm32")]
    {
        out.push_str("\n[Graphics]\nBackend: WebGPU / WebGL\n");
    }
    out
}

/// Build card text: the metadata suffix without its leading `+`
/// (`194.gef189d77`, `gef189d77.dirty`), or "Release" for a clean build on
/// the release tag.
fn build_label() -> std::borrow::Cow<'static, str> {
    match env!("OCS_BUILD_METADATA").strip_prefix('+') {
        Some(suffix) => std::borrow::Cow::Borrowed(suffix),
        None => t!("Release"),
    }
}

pub fn view_window(
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'static, Message> {
    // The hero carries the full build identity (`2026.36+194.gef189d77`);
    // the cards split it into the release version and the build details.
    let version = format!("v{}", env!("OCS_FULL_VERSION"));
    let build = build_label();
    let content_width = if matches!(sizing.width, Length::Fill) {
        Fill
    } else {
        Shrink
    };
    let card_width = if matches!(sizing.width, Length::Fill) {
        Length::FillPortion(1)
    } else {
        Length::Fixed(148.0)
    };
    let logo = svg(svg::Handle::from_memory(include_bytes!(
        "../../../assets/logo.svg"
    )))
    .width(Length::Fixed(72.0))
    .height(Length::Fixed(72.0));

    let hero = container(
        row![
            logo,
            column![
                text("Open CAD Studio").size(28).style(primary_style),
                text(t!("CAD application for Architecture & Engineering"))
                    .size(11)
                    .style(muted_style),
                text(version.clone()).size(13),
            ]
            .spacing(5),
        ]
        .spacing(18)
        .align_y(iced::Center),
    )
    .padding([14, 16])
    .width(content_width)
    .style(surface_style);

    let metadata = row![
        info_card(t!("Version"), env!("OCS_APP_VERSION"), card_width),
        info_card(t!("Platform"), platform_name(), card_width),
        info_card(t!("Arch"), architecture_name(), card_width),
    ]
    .spacing(8)
    .width(content_width);

    let build_info = row![
        info_card(t!("Build"), build, card_width),
        info_card(t!("Commit date"), env!("OCS_COMMIT_DATE"), card_width),
        info_card(t!("Profile"), env!("OCS_BUILD_PROFILE"), card_width),
    ]
    .spacing(8)
    .width(content_width);

    let row_width = if matches!(sizing.width, Length::Fill) {
        Fill
    } else {
        Length::Fixed(148.0 * 3.0 + 16.0)
    };
    let system = column![
        info_row(t!("CPU"), crate::sysinfo::cpu_summary(), row_width),
        info_row(t!("RAM"), crate::sysinfo::ram_summary(), row_width),
        info_row(t!("GPU (backend)"), gpu_summary(), row_width),
    ]
    .spacing(6)
    .width(content_width);

    let copy = dialog_button(t!("Copy Info"), Message::AboutCopyInfo, true);

    container(
        column![
            hero,
            metadata,
            build_info,
            system,
            row![Space::new().width(content_width), copy]
                .width(sizing.width)
                .align_y(iced::Center),
        ]
        .spacing(12)
        .padding(16)
        .width(sizing.width)
        .height(sizing.height),
    )
    .width(sizing.width)
    .height(sizing.height)
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.palette().background.base.color,
        )),
        ..Default::default()
    })
    .into()
}
