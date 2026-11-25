//! The Blocks palette's Libraries tab as a drawing browser: a library drawing
//! opens from the context menu, and the palette comes back on the tab and in
//! the folder it was left on.

use std::path::PathBuf;

use crate::app::settings::PaletteBlockRef;
use crate::app::{Message, OpenCADStudio};
use crate::ui::window::block_palette::{BlockPaletteMsg, Item, MenuAction, Tab};

fn library(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("ocs-bp-library-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Pfeile")).unwrap();
    std::fs::write(root.join("Pfeile/Pfeil.dwg"), b"x").unwrap();
    root
}

fn send(app: &mut OpenCADStudio, msg: BlockPaletteMsg) {
    let _ = app.update_root(Message::BlockPalette(msg));
}

#[test]
fn a_library_drawing_opens_from_the_context_menu() {
    let root = library("open");
    let drawing = root.join("Pfeile/Pfeil.dwg");
    let mut app = OpenCADStudio::new_for_test();
    send(
        &mut app,
        BlockPaletteMsg::Menu(Item::File(drawing.clone()), MenuAction::OpenDrawing),
    );
    assert_eq!(
        app.opening.as_ref().and_then(|o| o.source_path.clone()),
        Some(drawing.clone()),
        "the drawing is being opened"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn only_drawings_offer_open_drawing() {
    let whole = PaletteBlockRef {
        name: "Pfeil".into(),
        source: "/lib/Pfeil.dwg".into(),
        time: 0,
        drawing: true,
    };
    let block = PaletteBlockRef {
        drawing: false,
        ..whole.clone()
    };
    assert_eq!(
        Item::File("/lib/Pfeil.dwg".into()).drawing(),
        Some(PathBuf::from("/lib/Pfeil.dwg"))
    );
    assert_eq!(
        Item::Ref(whole).drawing(),
        Some(PathBuf::from("/lib/Pfeil.dwg"))
    );
    assert_eq!(Item::Ref(block).drawing(), None, "a block of a drawing");
    assert_eq!(Item::Current("Pfeil".into()).drawing(), None);
    assert_eq!(Item::Folder("/lib".into()).drawing(), None);
}

#[test]
fn the_palette_comes_back_on_its_tab_and_library_folder() {
    let root = library("resume");
    let mut app = OpenCADStudio::new_for_test();
    send(
        &mut app,
        BlockPaletteMsg::LibrarySelect(root.display().to_string()),
    );
    send(&mut app, BlockPaletteMsg::Tab(Tab::Libraries));
    let sub = Item::Folder(root.join("Pfeile"));
    send(&mut app, BlockPaletteMsg::Press(sub.clone()));
    send(&mut app, BlockPaletteMsg::Release(sub));
    assert_eq!(app.block_palette.library_dir, Some(root.join("Pfeile")));

    let settings = app.current_settings();
    assert_eq!(settings.block_palette_tab, Tab::Libraries.index());
    assert_eq!(
        settings.block_library_folder,
        root.join("Pfeile").display().to_string()
    );

    // The next session opens on the Libraries tab, in that folder.
    let mut next = OpenCADStudio::new_for_test();
    next.apply_settings(&settings);
    assert_eq!(next.block_palette.tab, Tab::Libraries);
    next.open_blocks_palette(None);
    assert_eq!(next.block_palette.library_dir, Some(root.join("Pfeile")));
    assert!(
        next.block_palette
            .library_entries
            .iter()
            .any(|e| e.path == root.join("Pfeile/Pfeil.dwg")),
        "the folder's drawings are listed"
    );

    // A folder that is gone falls back to the library root.
    let mut gone = settings.clone();
    gone.block_library_folder = root.join("weg").display().to_string();
    let mut fallback = OpenCADStudio::new_for_test();
    fallback.apply_settings(&gone);
    fallback.open_blocks_palette(None);
    assert_eq!(fallback.block_palette.library_dir, Some(root.clone()));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn tabs_round_trip_through_their_settings_index() {
    for tab in [Tab::Current, Tab::Recent, Tab::Favorites, Tab::Libraries] {
        assert_eq!(Tab::from_u8(tab.index()), tab);
    }
    assert_eq!(
        Tab::from_u8(9),
        Tab::Current,
        "an unknown index opens the first tab"
    );
}
