use super::*;

fn prompt_of(app: &OpenCADStudio, i: usize) -> String {
    app.tabs[i]
        .active_cmd
        .as_ref()
        .expect("the array is still being authored")
        .prompt()
        .to_string()
}

/// Drive a Parameter Sets row through the parameter it places and the object
/// selection its action asks for, as far as the array's first spacing prompt.
fn spacing_prompt(label: &str, points: &[&str]) -> (OpenCADStudio, String) {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line(&format!("BPARAMETERSET {label}"));
    for point in points {
        feed(&mut app, point);
    }
    for _ in 0..4 {
        let prompt = prompt_of(&app, i);
        if prompt.contains("distance between") {
            return (app, prompt);
        }
        if prompt.contains("Select objects") {
            let targets = members(&app);
            let _ = app.feed_command(crate::command::StepInput::SelectionComplete(targets));
            let _ = app.feed_command(crate::command::StepInput::Enter);
            continue;
        }
        feed(&mut app, "");
    }
    let prompt = prompt_of(&app, i);
    assert!(
        prompt.contains("distance between"),
        "the array reaches its spacing prompt, got {prompt}"
    );
    (app, prompt)
}

#[test]
fn array_rows_ask_for_spacing_with_one_term_per_shape() {
    for (label, points, expected) in [
        (
            "Linear Array",
            &["0,0", "20,0", "0,10"] as &[&str],
            "BACTION  Specify distance between columns:",
        ),
        (
            "Polar Array",
            &["0,0", "20,0", "10,10"],
            "BACTION  Specify distance between items:",
        ),
        (
            "XY Array Box Set",
            &["0,0", "20,20", "10,10"],
            "BACTION  Specify distance between columns (or distance between rows):",
        ),
    ] {
        let (_app, prompt) = spacing_prompt(label, points);
        assert_eq!(prompt, expected, "{label} asks with its own term");
    }
}

#[test]
fn xy_array_follow_up_prompt_uses_the_term_it_offered_and_refuses_a_point() {
    let (mut app, _) = spacing_prompt("XY Array Box Set", &["0,0", "20,20", "10,10"]);
    let i = app.active_tab;
    feed(&mut app, "10");
    assert_eq!(
        prompt_of(&app, i),
        "BACTION  Specify distance between rows:",
        "the follow-up names the alternative the first prompt offered"
    );
    feed(&mut app, "5,5");
    assert_eq!(
        prompt_of(&app, i),
        "BACTION  Specify distance between rows:",
        "a refused answer leaves the prompt"
    );
    assert_eq!(
        app.command_line.last_error.as_deref(),
        Some("Specify a positive distance as a single value, not a point."),
        "the refusal says what form it wants"
    );
}
