//! The Files palette (FILESPALETTE / FILESPALETTECLOSE): browse a folder,
//! open a drawing with a double-click, drag one onto the drawing area to
//! insert it as a block.

use std::path::PathBuf;

use iced::Task;

use crate::app::{Message, OpenCADStudio};
use crate::ui::dock::PanelId;
use crate::ui::window::block_palette::Item;
use crate::ui::window::files_palette::{is_drawing, FileAction, FilesMsg};

impl OpenCADStudio {
    /// Open or close the palette. Opened, it shows the folder it was left on.
    pub(crate) fn set_files_palette(&mut self, open: bool) {
        self.files_palette.show = open;
        if open {
            self.dock_open_at_default(PanelId::Files);
            if let Some(dir) = self.files_palette.folder.clone().filter(|d| d.is_dir()) {
                self.files_palette.open_folder(dir);
            }
        } else {
            self.dock_unpeek(PanelId::Files);
        }
        self.persist_settings_if_changed();
    }

    /// After the settings are read: an open palette lists the folder it was
    /// left on.
    pub(crate) fn files_palette_restore(&mut self) {
        if self.files_palette.show && self.files_palette.entries.is_empty() {
            if let Some(dir) = self.files_palette.folder.clone() {
                self.files_palette.open_folder(dir);
            }
        }
    }

    /// FILESPALETTE and FILESPALETTECLOSE.
    pub(in crate::app) fn dispatch_files_palette(&mut self, cmd: &str) -> Option<Task<Message>> {
        match cmd {
            "FILESPALETTE" => self.set_files_palette(true),
            "FILESPALETTECLOSE" => self.set_files_palette(false),
            _ => return None,
        }
        Some(Task::none())
    }

    fn files_palette_go(&mut self, dir: PathBuf) {
        self.files_palette.filter.clear();
        self.files_palette.open_folder(dir);
        self.persist_settings_if_changed();
    }

    /// Read the preview of a drawing once, in the background.
    fn files_palette_preview(&mut self, path: &PathBuf) -> Task<Message> {
        if !is_drawing(path) || self.files_palette.thumbs.contains_key(path) {
            return Task::none();
        }
        self.files_palette.thumbs.insert(path.clone(), None);
        Task::run(
            super::blocks_palette::read_thumbnails(vec![path.clone()]),
            |read| Message::FilesPalette(FilesMsg::Thumbnails(read)),
        )
    }

    fn files_palette_open(&mut self, path: PathBuf) -> Task<Message> {
        if path.is_dir() {
            self.files_palette_go(path);
            Task::none()
        } else {
            self.update(Message::FileDropped(path))
        }
    }

    pub(super) fn on_files_palette(&mut self, m: FilesMsg) -> Task<Message> {
        match m {
            #[cfg(target_arch = "wasm32")]
            FilesMsg::Browse => {}
            #[cfg(not(target_arch = "wasm32"))]
            FilesMsg::Browse => {
                let start = self.files_palette.folder.clone();
                return Task::perform(
                    async move {
                        let mut dialog =
                            rfd::AsyncFileDialog::new().set_title(crate::t!("Browse...").as_ref());
                        if let Some(dir) = start.filter(|d| d.is_dir()) {
                            dialog = dialog.set_directory(dir);
                        }
                        match dialog.pick_folder().await {
                            Some(h) => Ok(crate::sys::handle_path(&h)),
                            None => Err("Cancelled".to_string()),
                        }
                    },
                    |r| Message::FilesPalette(FilesMsg::Picked(r)),
                );
            }
            FilesMsg::Picked(Ok(dir)) => self.files_palette_go(dir),
            FilesMsg::Picked(Err(e)) => {
                if e != "Cancelled" {
                    self.command_line.push_error(&e);
                }
            }
            FilesMsg::Up => {
                if let Some(parent) = self
                    .files_palette
                    .folder
                    .as_ref()
                    .and_then(|d| d.parent())
                    .map(PathBuf::from)
                {
                    self.files_palette_go(parent);
                }
            }
            FilesMsg::Refresh => {
                if let Some(dir) = self.files_palette.folder.clone() {
                    self.files_palette.open_folder(dir);
                }
            }
            FilesMsg::Filter(s) => self.files_palette.filter = s,
            FilesMsg::Press(path) => {
                // A drawing pressed here and released over the drawing area
                // is inserted there, as from the Blocks palette.
                self.block_palette.pressed = is_drawing(&path).then(|| Item::File(path.clone()));
                self.files_palette.selected = Some(path.clone());
                return self.files_palette_preview(&path);
            }
            FilesMsg::Release(path) => {
                if self.block_palette.pressed.as_ref() == Some(&Item::File(path)) {
                    self.block_palette.pressed = None;
                }
            }
            FilesMsg::Open(path) => {
                self.block_palette.pressed = None;
                return self.files_palette_open(path);
            }
            FilesMsg::Menu(path, action) => match action {
                FileAction::Open => return self.files_palette_open(path),
                FileAction::Insert => return self.palette_insert(Item::File(path), None),
                // The web build has no file manager to show it in.
                #[cfg(target_arch = "wasm32")]
                FileAction::Reveal => {
                    let _ = path;
                }
                #[cfg(not(target_arch = "wasm32"))]
                FileAction::Reveal => {
                    if let Err(error) = crate::sys::reveal_in_file_manager(&path) {
                        self.command_line.push_error(
                            crate::tf!("Could not open file location: {error}").as_ref(),
                        );
                    }
                }
            },
            FilesMsg::Thumbnails(read) => {
                for (path, thumb) in read {
                    self.files_palette.thumbs.insert(path, thumb);
                }
            }
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of its own per test (tests run in parallel), listed.
    fn app_with_folder(test: &str) -> (OpenCADStudio, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "ocs-files-palette-{test}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Sub")).unwrap();
        std::fs::write(dir.join("Plan.dwg"), b"").unwrap();
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.on_files_palette(FilesMsg::Picked(Ok(dir.clone())));
        (app, dir)
    }

    #[test]
    fn a_picked_folder_lists_and_up_and_into_folders_move_around() {
        let (mut app, dir) = app_with_folder("move");
        let names: Vec<&str> = app
            .files_palette
            .entries
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(names, ["Sub", "Plan.dwg"]);
        let _ = app.on_files_palette(FilesMsg::Open(dir.join("Sub")));
        assert_eq!(
            app.files_palette.folder.as_deref(),
            Some(dir.join("Sub").as_path())
        );
        let _ = app.on_files_palette(FilesMsg::Up);
        assert_eq!(app.files_palette.folder.as_deref(), Some(dir.as_path()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pressed_drawing_is_dragged_until_released_on_the_palette() {
        let (mut app, dir) = app_with_folder("drag");
        let plan = dir.join("Plan.dwg");
        let _ = app.on_files_palette(FilesMsg::Press(plan.clone()));
        assert_eq!(app.block_palette.pressed, Some(Item::File(plan.clone())));
        assert_eq!(app.files_palette.selected.as_ref(), Some(&plan));
        // Released over the palette: a click, nothing to drop.
        let _ = app.on_files_palette(FilesMsg::Release(plan.clone()));
        assert!(app.block_palette.pressed.is_none());
        // A folder is never dragged.
        let _ = app.on_files_palette(FilesMsg::Press(dir.join("Sub")));
        assert!(app.block_palette.pressed.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_commands_open_and_close_the_palette() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("FILESPALETTE");
        assert!(app.files_palette.show);
        assert!(app.dock.is_placed(PanelId::Files));
        let _ = app.run_command_line("FILESPALETTECLOSE");
        assert!(!app.files_palette.show);
    }
}
