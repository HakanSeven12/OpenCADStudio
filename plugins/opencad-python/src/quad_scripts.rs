//! The code Python quad items run when picked (API v15).
//!
//! A script registers its panels and ends; its interpreter and functions go
//! with it. An item therefore carries its code as text, kept here for the
//! plugin's session by `(panel id, control id)` and run by `on_quad_action`.

use std::collections::HashMap;
use std::sync::Mutex;

static SCRIPTS: Mutex<Option<HashMap<(String, String), String>>> = Mutex::new(None);

/// Set panel `panel_id`'s item code to `scripts` (control id, code); an
/// empty list forgets the panel (it was unregistered).
pub(crate) fn replace_panel(panel_id: &str, scripts: Vec<(String, String)>) {
    let Ok(mut guard) = SCRIPTS.lock() else {
        return;
    };
    let map = guard.get_or_insert_with(HashMap::new);
    map.retain(|(panel, _), _| panel != panel_id);
    for (control_id, code) in scripts {
        map.insert((panel_id.to_owned(), control_id), code);
    }
}

/// The code of item `control_id` of panel `panel_id`, if it has any.
pub(crate) fn code(panel_id: &str, control_id: &str) -> Option<String> {
    SCRIPTS
        .lock()
        .ok()?
        .as_ref()?
        .get(&(panel_id.to_owned(), control_id.to_owned()))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panel_keeps_its_items_code_until_it_is_replaced() {
        replace_panel("p", vec![("a".into(), "x = 1".into())]);
        assert_eq!(code("p", "a").as_deref(), Some("x = 1"));
        replace_panel("p", vec![("b".into(), "y = 2".into())]);
        assert_eq!(code("p", "a"), None);
        assert_eq!(code("p", "b").as_deref(), Some("y = 2"));
        replace_panel("p", Vec::new());
        assert_eq!(code("p", "b"), None);
    }
}
