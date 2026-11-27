//! The Files palette (FILESPALETTE): a folder's sub-folders and drawings, to
//! open a drawing for editing without a file dialog.
//!
//! A double-click opens a drawing in its own tab (as dropping it on the
//! window does) or goes into a folder; a drawing dragged onto the drawing
//! area is inserted there as a block, the way a library drawing is from the
//! Blocks palette. The context menu offers both and the file's location.
//! The palette remembers its folder across sessions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::app::Message;
use crate::ui::dock::PanelId;
use crate::ui::kit;
use crate::ui::kit::{list_row, text_cell};
use crate::ui::style::kit_metrics as km;
use iced::widget::{column, container, image, row, text, Space};
use iced::{Element, Fill, Length, Padding};

const UP_ICON: &[u8] = include_bytes!("../../../assets/icons/ui/tri_up.svg");
const REFRESH_ICON: &[u8] = include_bytes!("../../../assets/icons/ui/refresh.svg");
/// Height of the selected drawing's preview under the list.
const PREVIEW_H: f32 = 140.0;

/// The palette's state.
#[derive(Debug, Default)]
pub struct FilesPalette {
    pub show: bool,
    /// The folder shown.
    pub folder: Option<PathBuf>,
    /// Its sub-folders, then its drawings.
    pub entries: Vec<Entry>,
    /// Why the folder could not be read.
    pub error: Option<String>,
    /// Typed into the filter: entries whose name contains it.
    pub filter: String,
    /// The entry clicked last.
    pub selected: Option<PathBuf>,
    /// Previews read so far (`None`: the drawing has none).
    pub thumbs: HashMap<PathBuf, Option<image::Handle>>,
}

/// A sub-folder or a drawing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub folder: bool,
}

#[derive(Debug, Clone)]
pub enum FilesMsg {
    Browse,
    Picked(Result<PathBuf, String>),
    Up,
    Refresh,
    Filter(String),
    /// Pressed on an entry: selects it; a drawing pressed and released over
    /// the drawing area is inserted there.
    Press(PathBuf),
    Release(PathBuf),
    /// Double-click: open the drawing, or go into the folder.
    Open(PathBuf),
    Menu(PathBuf, FileAction),
    /// Previews read in the background.
    Thumbnails(Vec<(PathBuf, Option<image::Handle>)>),
}

/// What the context menu does with an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileAction {
    Open,
    Insert,
    Reveal,
}

/// A drawing the palette lists (DWG, DXF).
pub fn is_drawing(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("dwg") || e.eq_ignore_ascii_case("dxf"))
}

/// The sub-folders of `dir`, then its drawings, each sorted by name without
/// regard to case; hidden entries (a leading dot) are left out.
pub fn list(dir: &Path) -> Result<Vec<Entry>, String> {
    let mut folders = Vec::new();
    let mut drawings = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let folder = path.is_dir();
        if !folder && !is_drawing(&path) {
            continue;
        }
        let e = Entry { path, name, folder };
        if folder {
            folders.push(e);
        } else {
            drawings.push(e);
        }
    }
    for part in [&mut folders, &mut drawings] {
        part.sort_by_cached_key(|e| e.name.to_lowercase());
    }
    folders.extend(drawings);
    Ok(folders)
}

impl FilesPalette {
    /// The entries the filter lets through.
    pub fn shown(&self) -> impl Iterator<Item = &Entry> {
        let filter = self.filter.trim().to_lowercase();
        self.entries
            .iter()
            .filter(move |e| filter.is_empty() || e.name.to_lowercase().contains(&filter))
    }

    fn entry(&self, path: &Path) -> Option<&Entry> {
        self.entries.iter().find(|e| e.path == path)
    }

    /// Show `dir`: read it, keep the filter, drop a selection it does not
    /// hold.
    pub fn open_folder(&mut self, dir: PathBuf) {
        match list(&dir) {
            Ok(entries) => {
                self.entries = entries;
                self.error = None;
            }
            Err(e) => {
                self.entries.clear();
                self.error = Some(e);
            }
        }
        if self
            .selected
            .as_ref()
            .is_some_and(|s| self.entry(s).is_none())
        {
            self.selected = None;
        }
        self.folder = Some(dir);
    }
}

fn pm(m: FilesMsg) -> Message {
    Message::FilesPalette(m)
}

fn icon_button<'a>(
    bytes: &'static [u8],
    tip: String,
    msg: Option<FilesMsg>,
) -> Element<'a, Message> {
    kit::tool_button(
        crate::ui::icons::semantic(bytes, km::ROW_ICON),
        tip,
        msg.map(pm),
        false,
    )
}

/// The context menu of an entry.
fn menu(entry: &Entry) -> Element<'static, Message> {
    use crate::ui::kit::menu::Item as MenuItem;
    let path = entry.path.clone();
    let item = |label: String, action: FileAction| {
        MenuItem::new(label, pm(FilesMsg::Menu(path.clone(), action)))
    };
    let mut rows = if entry.folder {
        vec![item(crate::t!("Open").into_owned(), FileAction::Open)]
    } else {
        vec![
            item(crate::t!("Open Drawing").into_owned(), FileAction::Open),
            item(crate::t!("Insert").into_owned(), FileAction::Insert),
        ]
    };
    rows.push(MenuItem::Separator);
    rows.push(item(
        crate::t!("Open File Location").into_owned(),
        FileAction::Reveal,
    ));
    crate::ui::kit::menu::view(rows)
}

fn entry_row<'a>(palette: &'a FilesPalette, entry: &'a Entry) -> Element<'a, Message> {
    let icon = if entry.folder {
        crate::ui::icons::themed_secondary(crate::ui::icons::FOLDER_OPEN, km::ROW_ICON)
    } else {
        crate::ui::icons::themed_secondary(crate::ui::icons::DOC, km::ROW_ICON)
    };
    let cell = row![
        container(icon).padding(Padding::ZERO.left(km::CELL_PAD_X)),
        text_cell(entry.name.clone(), false),
    ]
    .align_y(iced::Center);
    let path = entry.path.clone();
    let area = list_row(vec![(cell.into(), Fill)])
        .selected(palette.selected.as_ref() == Some(&entry.path))
        .on_press(pm(FilesMsg::Press(path.clone())))
        .on_release(pm(FilesMsg::Release(path.clone())))
        .on_double_click(pm(FilesMsg::Open(path)))
        .view();
    let entry = entry.clone();
    iced_aw::ContextMenu::new(area, move || menu(&entry)).into()
}

/// The palette.
pub fn view<'a>(
    palette: &'a FilesPalette,
    width: f32,
    chrome: crate::ui::dock::Chrome,
) -> Element<'a, Message> {
    let title_bar =
        crate::ui::dock::title_bar(PanelId::Files, crate::t!("Files").into_owned(), chrome);
    let has_parent = palette.folder.as_ref().and_then(|f| f.parent()).is_some();
    let tools = kit::toolbar(vec![
        icon_button(
            UP_ICON,
            crate::t!("Up").into_owned(),
            has_parent.then_some(FilesMsg::Up),
        ),
        icon_button(
            crate::ui::icons::FOLDER_OPEN,
            crate::t!("Browse...").into_owned(),
            Some(FilesMsg::Browse),
        ),
        icon_button(
            REFRESH_ICON,
            crate::t!("Refresh").into_owned(),
            palette.folder.is_some().then_some(FilesMsg::Refresh),
        ),
    ]);
    let Some(folder) = palette.folder.as_ref() else {
        let body = column![
            kit::band(tools),
            kit::empty_state(crate::tr!("files-palette", "choose-folder")),
        ];
        return kit::palette(title_bar, body, width);
    };
    let place = text(folder.file_name().map_or_else(
        || folder.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    ))
    .size(crate::ui::style::text::UI);
    let filter = kit::search_field(&crate::t!("Filter"), &palette.filter, |s| {
        pm(FilesMsg::Filter(s))
    });
    let rows: Vec<Element<'a, Message>> = palette.shown().map(|e| entry_row(palette, e)).collect();
    let list: Element<'a, Message> = if let Some(error) = &palette.error {
        kit::empty_state(error.clone())
    } else if rows.is_empty() {
        kit::empty_state(crate::tr!("files-palette", "empty"))
    } else {
        kit::scroll_area(column(rows))
    };
    let preview: Element<'a, Message> = match palette
        .selected
        .as_ref()
        .filter(|p| is_drawing(p))
        .and_then(|p| palette.thumbs.get(p))
    {
        Some(Some(handle)) => kit::band(
            container(image(handle.clone()).height(Length::Fixed(PREVIEW_H))).center_x(Fill),
        ),
        _ => Space::new().into(),
    };
    let body = column![
        kit::band(tools),
        kit::band(place),
        kit::band(filter),
        container(list).height(Fill),
        container(preview).padding(Padding::ZERO.bottom(km::TOOL_GAP)),
    ];
    kit::palette(title_bar, body, width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_first_then_drawings_by_name_and_the_filter() {
        let dir = std::env::temp_dir().join(format!("ocs-files-palette-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for d in ["b Folder", "A folder", ".hidden"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        for f in ["z.dwg", "Plan.DXF", "a.DWG", "notes.txt", ".~lock.dwg"] {
            std::fs::write(dir.join(f), b"").unwrap();
        }
        let names: Vec<String> = list(&dir).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(
            names,
            ["A folder", "b Folder", "a.DWG", "Plan.DXF", "z.dwg"]
        );

        let mut palette = FilesPalette::default();
        palette.open_folder(dir.clone());
        palette.filter = "PLAN".into();
        let shown: Vec<&str> = palette.shown().map(|e| e.name.as_str()).collect();
        assert_eq!(shown, ["Plan.DXF"]);

        // A selection the new folder does not hold is dropped.
        palette.selected = Some(dir.join("z.dwg"));
        palette.open_folder(dir.join("A folder"));
        assert!(palette.selected.is_none());
        assert!(palette.entries.is_empty() && palette.error.is_none());

        palette.open_folder(dir.join("gone"));
        assert!(palette.error.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
