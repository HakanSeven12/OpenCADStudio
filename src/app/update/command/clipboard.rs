use super::*;
use super::super::util::*;

/// Which text input, if any, holds focus when Ctrl+V arrives: a focused
/// text input pastes natively, so the shortcut must not paste again.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct PasteFocusProbe {
    /// The id of the text input whose `focusable` call comes next.
    pending: Option<Option<iced::advanced::widget::Id>>,
    focus: Option<crate::app::PasteFocus>,
}

#[cfg(not(target_arch = "wasm32"))]
impl iced::advanced::widget::Operation<crate::app::PasteFocus> for PasteFocusProbe {
    fn text_input(
        &mut self,
        id: Option<&iced::advanced::widget::Id>,
        _bounds: iced::Rectangle,
        _state: &mut dyn iced::advanced::widget::operation::TextInput,
    ) {
        self.pending = Some(id.cloned());
    }

    fn focusable(
        &mut self,
        _id: Option<&iced::advanced::widget::Id>,
        _bounds: iced::Rectangle,
        state: &mut dyn iced::advanced::widget::operation::Focusable,
    ) {
        // A text input reports `text_input` then `focusable`; other
        // focusables (text editors) only the latter.
        let Some(id) = self.pending.take() else {
            return;
        };
        if state.is_focused() {
            let command_line =
                iced::advanced::widget::Id::new(crate::ui::command_line::CMD_INPUT_ID);
            self.focus = Some(if id.as_ref() == Some(&command_line) {
                crate::app::PasteFocus::CommandLine
            } else {
                crate::app::PasteFocus::Field
            });
        }
    }

    fn traverse(
        &mut self,
        operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation<crate::app::PasteFocus>),
    ) {
        if self.focus.is_none() {
            operate(self);
        }
    }

    fn finish(&self) -> iced::advanced::widget::operation::Outcome<crate::app::PasteFocus> {
        iced::advanced::widget::operation::Outcome::Some(
            self.focus.unwrap_or(crate::app::PasteFocus::None),
        )
    }
}

/// The pixels read off the system clipboard (RGBA, row by row) as a PNG.
#[cfg(not(target_arch = "wasm32"))]
fn clipboard_png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let pixels = image::RgbaImage::from_raw(width, height, rgba.to_vec())?;
    let mut png = std::io::Cursor::new(Vec::new());
    pixels.write_to(&mut png, image::ImageFormat::Png).ok()?;
    Some(png.into_inner())
}

/// The file name, without its extension, of a picture pasted now.
#[cfg(not(target_arch = "wasm32"))]
fn pasted_image_stem() -> String {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    format!("pasted-{stamp}")
}

/// Write a pasted picture into `folder` as `stem.png`, or under the next
/// name not taken there, and give the file's path.
#[cfg(not(target_arch = "wasm32"))]
fn save_pasted_png(
    folder: &std::path::Path,
    stem: &str,
    png: &[u8],
) -> std::io::Result<std::path::PathBuf> {
    let name = crate::sys::unique_pasted_image_name(stem, "png", |name| folder.join(name).exists());
    let path = folder.join(name);
    std::fs::create_dir_all(folder)?;
    std::fs::write(&path, png)?;
    Ok(path)
}

impl OpenCADStudio {
    pub(in crate::app) fn on_paste_shortcut(&mut self) -> Task<Message> {
        if self.mtext_editor.is_some() {
            // Web reads via the browser's async Clipboard API (iced's
            // sync clipboard read returns nothing there); native uses
            // iced's clipboard.
            #[cfg(target_arch = "wasm32")]
            return Task::perform(crate::sys::read_clipboard_text(), Message::MTextPasteClip);
            #[cfg(not(target_arch = "wasm32"))]
            return iced::clipboard::read_text()
                .map(|result| Message::MTextPasteClip(result.ok().map(|text| (*text).clone())));
        }
        if self.text_inline.is_some() {
            // Web: the iced text_input can't reach the async clipboard,
            // so paste it ourselves. Native: the focused text_input
            // already handled Ctrl+V — doing it here would duplicate.
            #[cfg(target_arch = "wasm32")]
            return Task::perform(
                crate::sys::read_clipboard_text(),
                Message::TextInlinePasteClip,
            );
            #[cfg(not(target_arch = "wasm32"))]
            return Task::none();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            iced::advanced::widget::operate(PasteFocusProbe::default())
                .map(Message::PasteShortcutResolved)
        }
        #[cfg(target_arch = "wasm32")]
        {
            if self.clipboard.is_empty() {
                self.read_system_clipboard_for_paste()
            } else {
                Task::done(Message::Command("PASTECLIP".to_string()))
            }
        }
    }

    pub(in crate::app) fn read_system_clipboard_for_paste(&self) -> Task<Message> {
        #[cfg(target_arch = "wasm32")]
        {
            Task::perform(crate::sys::read_clipboard_text(), |text| {
                Message::SystemClipboardPaste(match text {
                    Some(text) if !text.is_empty() => crate::app::SystemClipboardText::Text(text),
                    _ => crate::app::SystemClipboardText::EmptyOrUnsupported,
                })
            })
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            iced::clipboard::read_text().map(|result| {
                use crate::app::SystemClipboardText as Text;
                use iced::clipboard::Error;

                let result = match result {
                    Ok(text) if !text.is_empty() => Text::Text((*text).clone()),
                    Ok(_) | Err(Error::ContentNotAvailable) => Text::EmptyOrUnsupported,
                    Err(Error::ClipboardUnavailable) | Err(Error::Unknown { .. }) => {
                        Text::Unavailable
                    }
                    Err(Error::ClipboardOccupied) => Text::Occupied,
                    Err(Error::ConversionFailure) => Text::ConversionFailed,
                };
                Message::SystemClipboardPaste(result)
            })
        }
    }

    /// Read the system clipboard for PASTESPEC — its text, then its picture
    /// and the link a copied range of cells comes with — and report them at
    /// once, so the prompt lists every format on hand. The web gets the
    /// picture's encoded file from the Clipboard API; native gets pixels,
    /// kept as a PNG.
    pub(in crate::app) fn read_system_clipboard_formats(&self) -> Task<Message> {
        use crate::io::ole_embed::EmbeddedImage;
        #[cfg(target_arch = "wasm32")]
        {
            Task::perform(
                async {
                    let text = crate::sys::read_clipboard_text().await;
                    let picture = crate::sys::read_clipboard_image()
                        .await
                        .and_then(|(name, bytes)| EmbeddedImage::from_bytes(name, bytes).ok());
                    (text, picture)
                },
                |(text, picture)| Message::PasteSpecial(text, picture, None),
            )
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            iced::clipboard::read_text().then(|text| {
                let text = text.ok().map(|text| (*text).clone());
                iced::clipboard::read_image().map(move |picture| {
                    let picture = picture.ok().and_then(|picture| {
                        let (width, height) = (picture.size.width, picture.size.height);
                        Some(EmbeddedImage {
                            bytes: clipboard_png(width, height, &picture.rgba)?,
                            pixel_width: width,
                            pixel_height: height,
                            name: format!("{}.png", pasted_image_stem()),
                            source: None,
                        })
                    });
                    Message::PasteSpecial(text.clone(), picture, crate::sys::clipboard_link::read())
                })
            })
        }
    }

    /// PASTESPEC's Image formats: place the picture read off the clipboard
    /// as IMAGEEMBED places a picked file, inside the drawing, or as IMAGE
    /// does, a file the drawing points at. For that the picture is saved
    /// where a plain paste saves it: beside the drawing, or in the config
    /// folder for a drawing never saved.
    pub(in crate::app) fn place_pasted_image(
        &mut self,
        image: crate::io::ole_embed::EmbeddedImage,
        embedded: bool,
    ) -> Task<Message> {
        if embedded {
            return self.update(Message::ImageEmbedPickResult(Ok(image)));
        }
        // The web has no folder to save in: the read left the picture in
        // the image registry under its name.
        #[cfg(target_arch = "wasm32")]
        let saved: Result<std::path::PathBuf, String> = Ok(std::path::PathBuf::from(&image.name));
        #[cfg(not(target_arch = "wasm32"))]
        let saved = self
            .pasted_image_folder()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
            .and_then(|folder| {
                let stem = image.name.strip_suffix(".png").unwrap_or(&image.name);
                save_pasted_png(&folder, stem, &image.bytes)
            })
            .map_err(|error| error.to_string());
        self.update(Message::ImagePickResult(
            saved.map(|path| (path, image.pixel_width, image.pixel_height)),
        ))
    }

    /// Where a pasted picture is saved: beside the drawing, or in the config
    /// folder for a drawing never saved.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn pasted_image_folder(&self) -> Option<std::path::PathBuf> {
        self.tabs[self.active_tab]
            .current_path
            .as_ref()
            .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
            .filter(|folder| !folder.as_os_str().is_empty())
            .or_else(|| crate::config::config_dir().map(|dir| dir.join("pasted")))
    }

    /// Read an image off the system clipboard for a paste that found no
    /// text. The web gets the encoded file from the Clipboard API; native
    /// gets pixels, which are written as a PNG beside the drawing (or, for a
    /// drawing never saved, in the config folder) so the image entity keeps
    /// a file to point at.
    pub(in crate::app) fn read_system_clipboard_image(&self) -> Task<Message> {
        #[cfg(target_arch = "wasm32")]
        {
            Task::perform(
                crate::sys::read_clipboard_image(),
                Message::SystemClipboardPasteImage,
            )
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let folder = self.pasted_image_folder();
            iced::clipboard::read_image().map(move |image| {
                let saved = image.ok().zip(folder.clone()).and_then(|(image, folder)| {
                    let png = clipboard_png(image.size.width, image.size.height, &image.rgba)?;
                    let path = save_pasted_png(&folder, &pasted_image_stem(), &png).ok()?;
                    Some((path.to_string_lossy().into_owned(), png))
                });
                Message::SystemClipboardPasteImage(saved)
            })
        }
    }

    /// Attach a clipboard image (a pasted screenshot) the way the IMAGE
    /// command's pick does: register the bytes name-keyed, decode for the
    /// file's true pixel size, then commit the entity — but placed directly
    /// at the cursor (0,0 before any pointer move) with the command's
    /// default width, skipping the pick prompts a paste should not run.
    pub(in crate::app) fn paste_clipboard_image(&mut self, name: String, bytes: Vec<u8>) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].is_start {
            self.command_line
                .push_error(crate::tr!("common", "no-drawing-open-use-new-or-open-first").as_ref());
            return Task::none();
        }
        crate::scene::model::image_model::register_source(&name, std::sync::Arc::new(bytes));
        let Some(decoded) = crate::scene::model::image_model::resolve_image(&name) else {
            self.command_line.push_error(
                crate::tr!("clipboard", "image-unsupported", name = name.clone()).as_ref(),
            );
            return Task::none();
        };
        let (pixels_w, pixels_h) = (decoded.source_width, decoded.source_height);
        // Same default footprint as the IMAGE command's Enter placement: one
        // world unit per 100 px (min 1), height from the true aspect ratio.
        let width = (pixels_w as f64 / 100.0).max(1.0);
        let aspect = if pixels_h == 0 {
            1.0
        } else {
            pixels_w as f64 / pixels_h as f64
        };
        let height = width / aspect;
        let origin = self.tabs[i].last_cursor_world;
        let mut image = document::entities::RasterImage::with_size(
            &name,
            document::types::Vector3::new(origin.x, origin.y, origin.z),
            pixels_w as f64,
            pixels_h as f64,
            width,
            height,
        );
        image.flags = document::entities::ImageDisplayFlags::SHOW_IMAGE
            | document::entities::ImageDisplayFlags::USE_CLIPPING_BOUNDARY;
        let short = std::path::Path::new(&name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&name)
            .to_string();
        self.command_line.push_output(
            crate::tr!(
                "clipboard",
                "pasted-image",
                name = short,
                width = pixels_w,
                height = pixels_h
            )
            .as_ref(),
        );
        self.apply_cmd_result(crate::command::CmdResult::CommitAndExit(
            document::EntityType::RasterImage(image),
        ))
    }

}
