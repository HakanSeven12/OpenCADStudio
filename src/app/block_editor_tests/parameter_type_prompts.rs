//! The parameter-type prompt names the types it offers and answers the letters
//! it highlights in them.

use super::*;

fn active_prompt(app: &OpenCADStudio, i: usize) -> String {
    match app.tabs[i].active_cmd.as_ref() {
        Some(command) => command.prompt().to_string(),
        None => String::new(),
    }
}

/// Open the type chooser and answer it with one entry, returning the prompt the
/// entry leaves on screen. A fresh session keeps the entries independent: the
/// parameter each one starts would otherwise be placed by the previous test.
fn started_by(entry: &str) -> (OpenCADStudio, String) {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.dispatch_command("BPARAMETER");
    feed(&mut app, entry);
    let prompt = active_prompt(&app, i);
    (app, prompt)
}

const OFFERED: &[(char, &str)] = &[
    ('A', "ALIGNMENT"),
    ('B', "BASE"),
    ('O', "POINT"),
    ('L', "LINEAR"),
    ('P', "POLAR"),
    ('X', "XY"),
    ('R', "ROTATION"),
    ('F', "FLIP"),
    ('V', "VISIBILITY"),
];

#[test]
fn the_parameter_type_prompt_offers_the_types_it_shows() {
    let (app, prompt) = started_by("");
    assert!(
        prompt.contains(
            "Enter parameter type [Alignment/Base/pOint/Linear/Polar/Xy/Rotation/Flip/Visibility/lookup]:"
        ),
        "{prompt}"
    );
    assert!(
        !prompt.contains("<Visibility>"),
        "the set offers no default, so the prompt advertises none: {prompt}"
    );
    assert!(app.tabs[app.active_tab].active_cmd.is_some());
}

#[test]
fn each_offered_letter_starts_the_parameter_it_names() {
    for (letter, word) in OFFERED {
        let (_, by_letter) = started_by(&letter.to_lowercase().to_string());
        let (_, by_word) = started_by(word);
        assert!(
            !by_letter.is_empty(),
            "{letter} ends the session instead of asking for a location"
        );
        assert!(
            !by_letter.contains("Enter parameter type ["),
            "{letter} answers with the type chooser again, so it started nothing: {by_letter}"
        );
        assert_eq!(by_letter, by_word, "{letter} should start {word}");
    }
    let (_, by_name) = started_by("lookup");
    assert!(
        !by_name.contains("Enter parameter type ["),
        "lookup is offered by its full word: {by_name}"
    );
}

#[test]
fn an_entry_the_type_prompt_does_not_offer_is_refused() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    for entry in ["K", "k", "2,2", "", "LINE"] {
        let _ = app.dispatch_command("BPARAMETER");
        let before = app.tabs[i].scene.document.clone();
        app.command_line.last_error = None;
        feed(&mut app, entry);
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("Invalid option keyword."),
            "{entry}"
        );
        let prompt = active_prompt(&app, i);
        assert!(
            prompt.contains("Enter parameter type ["),
            "{entry} leaves the type chooser open: {prompt}"
        );
        assert_eq!(
            app.tabs[i].scene.document, before,
            "{entry} writes no parameter"
        );
    }
}
