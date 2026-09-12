use super::{ModalKind, OpenCADStudio};
use crate::scene::pipeline::{GpuAdapter, GpuStatus};

impl OpenCADStudio {
    pub(super) fn queue_startup_prompts(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if !self.default_assoc_prompted {
            self.pending_startup_modals
                .push_back(ModalKind::AssocPrompt);
        }
        if self.donation_prompt_version != env!("OCS_APP_VERSION") {
            self.pending_startup_modals
                .push_back(ModalKind::DonationPrompt);
        }
        self.show_next_startup_modal();
    }

    pub(super) fn show_next_startup_modal(&mut self) {
        if self.active_modal.is_none() && self.opening.is_none() && self.pending_opens.is_empty() {
            if let Some(&kind) = self.pending_startup_modals.front() {
                self.active_modal = Some(kind);
                self.reset_modal_geometry();
            }
        }
    }

    /// Notice when the scene is drawn by a software rasterizer, or not at
    /// all, and say so everywhere the user might look: the command line,
    /// the log, a one-time popup, and a status-bar pill that stays.
    ///
    /// Runs after every message; the pipeline records the adapter when iced
    /// first builds a device and counts the viewport's draws in the meantime,
    /// so the verdict lands on the first message after the first frame
    /// (`subscription` keeps frames ticking until it does). A working GPU
    /// gets no word here — the `[gpu] adapter:` log line already names it.
    pub(super) fn refresh_gpu_status(&mut self) {
        let Some(status) =
            crate::scene::pipeline::gpu_status_if_changed(&mut self.gpu_status_generation)
        else {
            return;
        };
        if status == self.gpu_status {
            return;
        }
        self.gpu_status = status;
        let Some(identity) = self.gpu_status.identity() else {
            return;
        };
        match &self.gpu_status {
            GpuStatus::Software(adapter) => {
                self.command_line.push_warning(&software_rasterizer_warning(adapter));
            }
            GpuStatus::NoRenderer => {
                crate::scene::pipeline::report_gpu_line(
                    "[gpu] no adapter: wgpu found nothing to draw with; the interface is on \
                     iced's software renderer and the scene is not drawn at all",
                );
                self.command_line
                    .push_warning(&crate::tr!("gpu", "command-line-no-renderer"));
            }
            GpuStatus::Unknown | GpuStatus::Hardware(_) => {}
        }
        // The popup: once per verdict, unless the user silenced this one.
        let already_queued = self.active_modal == Some(ModalKind::GpuWarning)
            || self.pending_startup_modals.contains(&ModalKind::GpuWarning);
        if self.gpu_warning_silenced != identity && !already_queued {
            self.pending_startup_modals.push_back(ModalKind::GpuWarning);
            self.show_next_startup_modal();
        }
    }

    pub(super) fn mark_startup_modal_shown(&mut self) {
        if self.active_modal.is_some()
            && self.active_modal.as_ref() == self.pending_startup_modals.front()
        {
            self.pending_startup_modals.pop_front();
        }
        if self.active_modal == Some(ModalKind::DonationPrompt)
            && self.donation_prompt_version != env!("OCS_APP_VERSION")
        {
            self.donation_prompt_version = env!("OCS_APP_VERSION").to_string();
            self.save_config();
        }
    }
}

/// The command-line warning for a software rasterizer, naming the adapter so
/// the user can tell llvmpipe from SwiftShader from WARP.
pub(super) fn software_rasterizer_warning(adapter: &GpuAdapter) -> String {
    let adapter = &adapter.name;
    crate::tf!("GPU unavailable — rendering in software on the CPU ({adapter}). Large drawings will navigate slowly; check the graphics driver (a driver update without a reboot is the usual cause).")
        .into_owned()
}

/// What usually puts a machine in this state, per platform. The cause is
/// almost always outside the app, and it differs: on Linux it is a driver
/// updated under a running kernel module, on Windows a driver that is
/// missing or a session (Remote Desktop, a VM) that has no GPU to offer, on
/// macOS a VM without a Metal device, in a browser hardware acceleration
/// switched off.
pub(crate) fn gpu_platform_hint() -> String {
    if cfg!(target_arch = "wasm32") {
        crate::tr!("gpu", "hint-web")
    } else if cfg!(target_os = "windows") {
        crate::tr!("gpu", "hint-windows")
    } else if cfg!(target_os = "macos") {
        crate::tr!("gpu", "hint-macos")
    } else {
        crate::tr!("gpu", "hint-linux")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{config::AppConfig, Message};
    use iced::wgpu;

    #[test]
    fn the_software_warning_names_the_adapter_and_every_platform_has_a_hint() {
        let cpu = GpuAdapter {
            name: "llvmpipe (LLVM 20.1.8, 256 bits)".to_string(),
            backend: wgpu::Backend::Vulkan,
            device_type: wgpu::DeviceType::Cpu,
        };
        let warning = software_rasterizer_warning(&cpu);
        // The name carries parentheses and commas of its own; the localized
        // template must still place it whole. The loader follows the system
        // locale here, so the assertions stay language-neutral.
        assert!(warning.contains("(llvmpipe (LLVM 20.1.8, 256 bits))"), "{warning}");
        assert!(warning.contains("GPU"), "{warning}");
        assert!(!warning.contains("{adapter}") && !warning.contains("__ocs_"), "{warning}");
        // Every platform has a hint, and it is a sentence, not a missing key.
        let hint = gpu_platform_hint();
        assert!(hint.len() > 20 && !hint.contains("hint-"), "{hint}");
    }

    #[test]
    fn silencing_the_popup_is_per_verdict_and_survives_a_settings_round_trip() {
        let mut app = OpenCADStudio::new_for_test();
        app.gpu_status = GpuStatus::NoRenderer;
        let _ = app.update(Message::GpuWarningSilence);
        assert_eq!(app.gpu_warning_silenced, "no-renderer");
        assert!(app.active_modal.is_none());

        let saved = serde_json::to_string(&app.current_config()).unwrap();
        let mut restarted = OpenCADStudio::new_for_test();
        restarted.apply_config(serde_json::from_str(&saved).unwrap());
        assert_eq!(restarted.gpu_warning_silenced, "no-renderer");
        // A different verdict is not covered by that choice.
        let software = GpuStatus::Software(GpuAdapter {
            name: "llvmpipe".to_string(),
            backend: wgpu::Backend::Vulkan,
            device_type: wgpu::DeviceType::Cpu,
        });
        assert_ne!(software.identity().unwrap(), restarted.gpu_warning_silenced);
    }

    #[test]
    fn donation_prompt_persists_once_per_version() {
        let mut app = OpenCADStudio::new_for_test();
        // Older settings have no donation version; headless construction never prompts.
        app.apply_config(serde_json::from_str::<AppConfig>("{}").unwrap());
        assert!(app.active_modal.is_none());
        assert!(app.pending_startup_modals.is_empty());
        app.default_assoc_prompted = true;
        app.queue_startup_prompts();
        assert_eq!(app.active_modal, Some(ModalKind::DonationPrompt));
        assert!(app.donation_prompt_version.is_empty());

        let _ = app.update(Message::ModalContentResized(iced::Size::new(540.0, 280.0)));
        let saved = serde_json::to_string(&app.current_config()).unwrap();
        let _ = app.update(Message::CloseModal);
        assert!(app.active_modal.is_none());

        let mut restarted = OpenCADStudio::new_for_test();
        restarted.apply_config(serde_json::from_str(&saved).unwrap());
        restarted.queue_startup_prompts();
        assert_eq!(restarted.donation_prompt_version, env!("OCS_APP_VERSION"));
        assert!(restarted.active_modal.is_none());

        restarted.donation_prompt_version = "previous-release".to_string();
        restarted.queue_startup_prompts();
        assert_eq!(restarted.active_modal, Some(ModalKind::DonationPrompt));
        let _ = restarted.update(Message::CommandEscape);
        assert!(restarted.active_modal.is_none());
        assert_eq!(restarted.donation_prompt_version, env!("OCS_APP_VERSION"));
        let _ = restarted.update(Message::Noop);
        assert!(restarted.active_modal.is_none());
    }

    #[test]
    fn startup_dialogs_wait_their_turn_and_preserve_unseen_prompts() {
        let mut app = OpenCADStudio::new_for_test();
        app.apply_config(AppConfig::default());
        app.queue_startup_prompts();
        assert_eq!(app.active_modal, Some(ModalKind::AssocPrompt));
        let _ = app.update(Message::UpdateCheckResult(Some(
            crate::io::update_check::UpdateInfo {
                version: "next-release".to_string(),
                body: String::new(),
            },
        )));
        assert_eq!(app.active_modal, Some(ModalKind::AssocPrompt));
        let _ = app.update(Message::AssocPromptNo);
        assert_eq!(app.active_modal, Some(ModalKind::DonationPrompt));

        // A file-recovery dialog may interrupt startup before the first frame.
        app.active_modal = Some(ModalKind::Recovery);
        let _ = app.update(Message::CloseModal);
        assert_eq!(app.active_modal, Some(ModalKind::DonationPrompt));
        assert!(app.donation_prompt_version.is_empty());
        let _ = app.update(Message::ModalContentResized(iced::Size::new(540.0, 280.0)));
        assert_eq!(app.donation_prompt_version, env!("OCS_APP_VERSION"));
        let _ = app.update(Message::CloseModal);
        assert_eq!(app.active_modal, Some(ModalKind::UpdateNotice));
        let _ = app.update(Message::UpdateNoticeClose);
        assert!(app.active_modal.is_none());
        assert!(app.pending_startup_modals.is_empty());
    }
}
