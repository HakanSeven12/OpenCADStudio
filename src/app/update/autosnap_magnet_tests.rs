use super::{Message, OpenCADStudio};

#[test]
fn the_magnet_checkbox_is_saved_and_restored() {
    let mut app = OpenCADStudio::new_for_test();
    assert!(app.autosnap_magnet, "the magnet is on by default");

    let _ = app.update_root(Message::AutoSnapMagnetChanged(false));
    assert!(!app.autosnap_magnet);
    let saved = app
        .last_saved_config
        .clone()
        .expect("the change is saved")
        .settings;
    assert!(!saved.autosnap_magnet);

    let mut next = OpenCADStudio::new_for_test();
    next.apply_settings(&saved);
    assert!(!next.autosnap_magnet, "the next session keeps it off");

    let _ = next.update_root(Message::AutoSnapMagnetChanged(true));
    assert!(next.autosnap_magnet);
}
