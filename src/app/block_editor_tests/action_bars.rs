use super::*;

#[test]
fn block_action_bars_group_native_actions_and_preserve_drawing_history() {
    use crate::scene::{
        block_action_bars as bars,
        block_actions::{self as actions, Kind},
    };
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    assert!(app.block_editor_preferences.action_bars);
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter = actions::all_parameters(&app.tabs[i].scene.document, definition)[0].clone();
    let targets = members(&app);
    let mut handles = vec![];
    for kind in [Kind::Move, Kind::Scale] {
        handles.push(
            actions::create(
                &mut app.tabs[i].scene.document,
                definition,
                kind,
                parameter.handle,
                &targets,
                glam::DVec3::new(4., 4., 0.),
            )
            .unwrap(),
        );
    }
    let groups = bars::groups(&app.tabs[i].scene.document, definition);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].parameter, parameter.handle);
    assert_eq!(groups[0].entries.len(), 2);
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let local_depth = main_depth(&app);
    let _ = app.update(Message::BlockActionBarSelect(handles[0]));
    assert_eq!(app.tabs[i].selected_authoring, Some(handles[0]));
    let _ = app.update(Message::BlockActionBarMove(
        parameter.handle,
        iced::Vector::new(80., 25.),
    ));
    assert_eq!(
        app.tabs[i].action_bars.offsets[&parameter.handle],
        iced::Vector::new(80., 25.)
    );
    let _ = app.dispatch_command("BACTIONBAR Hide");
    let _ = app.feed_command(crate::command::StepInput::Enter);
    assert!(app.tabs[i].action_bars.hidden.contains(&parameter.handle));
    app.tabs[i].selected_authoring = None;
    let _ = app.update(Message::BlockActionBarSelect(handles[0]));
    assert!(
        app.tabs[i].selected_authoring.is_none(),
        "hidden controls cannot select"
    );
    let _ = app.dispatch_command("BACTIONBAR Show All");
    assert!(app.tabs[i].action_bars.hidden.is_empty());
    assert!(app.tabs[i]
        .action_bars
        .offsets
        .contains_key(&parameter.handle));
    let _ = app.update(Message::BlockActionBarHide(parameter.handle));
    let _ = app.dispatch_command("BACTIONBAR Reset");
    assert!(app.tabs[i].action_bars.hidden.is_empty());
    assert!(app.tabs[i].action_bars.offsets.is_empty());
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    assert_eq!(main_depth(&app), local_depth);
    let _ = app.update(Message::BlockActionBarEdit(handles[0]));
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().name(),
        "BACTIONSET"
    );
    let _ = app.update(Message::CommandEscape);
    let _ = app.dispatch_command("LINE");
    for command in ["BACTIONSET", "BASSOCIATE", "BLOOKUPTABLE"] {
        let _ = app.dispatch_command(command);
        assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    }
    let _ = app.update(Message::CommandEscape);
    let _ = app.dispatch_command("BACTIONBARMODE 0");
    let _ = app.dispatch_command("LINE");
    let _ = app.dispatch_command("BACTIONBAR Reset");
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    let _ = app.update(Message::CommandEscape);
    let _ = app.update(Message::BlockActionBarEdit(handles[0]));
    assert!(app.tabs[i].active_cmd.is_none());
    let _ = app.dispatch_command("BACTIONSET");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().name(),
        "BACTIONSET"
    );
}

#[test]
fn action_bars_at_one_anchor_are_stacked_without_overlapping() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0.2,0 0.3,0 0.25,0");
    let _ = app.run_command_line("BPARAMETER LINEAR 0.2,0 0.3,0 0.25,0");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameters = actions::all_parameters(&app.tabs[i].scene.document, definition);
    assert_eq!(parameters.len(), 2);
    let targets = members(&app);
    for parameter in &parameters {
        actions::create(
            &mut app.tabs[i].scene.document,
            definition,
            Kind::Move,
            parameter.handle,
            &targets,
            glam::DVec3::new(4., 4., 0.),
        )
        .unwrap();
    }
    let bounds = iced::Rectangle::new(iced::Point::new(0.0, 0.0), iced::Size::new(800.0, 600.0));
    let layout = app.action_bar_layout(bounds);
    assert_eq!(layout.len(), 2, "both parameters carry a bar");
    let card = super::super::block_action_bars::BAR_HEIGHT;
    let rects: Vec<_> = layout
        .iter()
        .map(|(_, point, _, width)| iced::Rectangle::new(*point, iced::Size::new(*width, card)))
        .collect();
    let (a, b) = (rects[0], rects[1]);
    let overlap =
        a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
    assert!(
        !overlap,
        "the two bars must not cover each other: {a:?} {b:?}"
    );
    for rect in [a, b] {
        assert!(
            rect.y >= bounds.y && rect.y + rect.height <= bounds.y + bounds.height,
            "a bar stays inside the viewport: {rect:?}"
        );
    }
}

#[test]
fn action_bars_near_the_bottom_edge_stack_upwards_without_overlapping() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    for _ in 0..6 {
        let _ = app.run_command_line("BPARAMETER LINEAR 0.2,-0.19 0.3,-0.19 0.25,-0.19");
    }
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameters = actions::all_parameters(&app.tabs[i].scene.document, definition);
    assert_eq!(parameters.len(), 6);
    let targets = members(&app);
    for parameter in &parameters {
        actions::create(
            &mut app.tabs[i].scene.document,
            definition,
            Kind::Move,
            parameter.handle,
            &targets,
            glam::DVec3::new(4., 4., 0.),
        )
        .unwrap();
    }
    let bounds = iced::Rectangle::new(iced::Point::new(0.0, 0.0), iced::Size::new(800.0, 600.0));
    let layout = app.action_bar_layout(bounds);
    assert_eq!(layout.len(), 6);
    let card = super::super::block_action_bars::BAR_HEIGHT;
    let rects: Vec<_> = layout
        .iter()
        .map(|(_, point, _, width)| iced::Rectangle::new(*point, iced::Size::new(*width, card)))
        .collect();
    for (index, a) in rects.iter().enumerate() {
        assert!(
            a.y >= bounds.y && a.y + a.height <= bounds.y + bounds.height,
            "bar {index} stays inside the viewport: {a:?}"
        );
        for (other, b) in rects.iter().enumerate().skip(index + 1) {
            let overlap = a.x < b.x + b.width
                && b.x < a.x + a.width
                && a.y < b.y + b.height
                && b.y < a.y + a.height;
            assert!(!overlap, "bar {index} covers bar {other}: {a:?} {b:?}");
        }
    }
}

#[test]
fn a_dragged_action_bar_keeps_its_place_and_the_others_make_room() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0.2,0 0.3,0 0.25,0");
    let _ = app.run_command_line("BPARAMETER LINEAR 0.2,0 0.3,0 0.25,0");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameters = actions::all_parameters(&app.tabs[i].scene.document, definition);
    assert_eq!(parameters.len(), 2);
    let targets = members(&app);
    for parameter in &parameters {
        actions::create(
            &mut app.tabs[i].scene.document,
            definition,
            Kind::Move,
            parameter.handle,
            &targets,
            glam::DVec3::new(4., 4., 0.),
        )
        .unwrap();
    }
    let bounds = iced::Rectangle::new(iced::Point::new(0.0, 0.0), iced::Size::new(800.0, 600.0));
    let before = app.action_bar_layout(bounds);
    assert_eq!(before.len(), 2);
    let parameter = before[1].0.parameter;
    let origin = before[1].1;
    let effective = before[1].2;
    assert_ne!(effective, iced::Vector::ZERO, "the second card was stacked");
    let _ = app.update(Message::BlockActionBarMove(
        parameter,
        effective + iced::Vector::new(0., 50.),
    ));
    let after = app.action_bar_layout(bounds);
    let moved = after
        .iter()
        .find(|(group, ..)| group.parameter == parameter)
        .expect("the dragged card is still laid out");
    assert_eq!(
        moved.1,
        iced::Point::new(origin.x, origin.y + 50.0),
        "a drag continues from the place the card was drawn"
    );
    let card = super::super::block_action_bars::BAR_HEIGHT;
    let dragged = iced::Rectangle::new(moved.1, iced::Size::new(moved.3, card));
    let other = iced::Rectangle::new(after[0].1, iced::Size::new(after[0].3, card));
    let overlap = dragged.x < other.x + other.width
        && other.x < dragged.x + dragged.width
        && dragged.y < other.y + other.height
        && other.y < dragged.y + dragged.height;
    assert!(
        !overlap,
        "the remaining bar makes room for the dragged one: {dragged:?} {other:?}"
    );
}

#[test]
fn block_action_bars_open_lookup_and_orphan_association_and_ignore_stale_callbacks() {
    use crate::scene::{
        block_action_bars as bars,
        block_actions::{self as actions, Kind},
    };
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LOOKUP 3,3");
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let lookup =
        crate::scene::block_lookup::parameters(&app.tabs[i].scene.document, definition)[0].0;
    let _ = app.update(Message::BlockActionBarEdit(lookup));
    assert_eq!(app.active_modal, Some(ModalKind::BlockLookup));
    assert_eq!(
        app.block_lookup.as_ref().unwrap().drafts[app.block_lookup.as_ref().unwrap().selected]
            .parameter,
        lookup
    );
    let _ = app.update(Message::CloseModal);
    let parameter = actions::all_parameters(&app.tabs[i].scene.document, definition)[0].clone();
    let targets = members(&app);
    let action = actions::create(
        &mut app.tabs[i].scene.document,
        definition,
        Kind::Move,
        parameter.handle,
        &targets,
        glam::DVec3::new(4., 4., 0.),
    )
    .unwrap();
    let Some(document::objects::ObjectType::DynamicBlock(object)) =
        app.tabs[i].scene.document.objects.get_mut(&action)
    else {
        panic!()
    };
    let document::objects::DynamicBlockData::MoveAction(native) = &mut object.data else {
        panic!()
    };
    native
        .connections
        .iter_mut()
        .for_each(|connection| connection.code = -1);
    assert!(bars::groups(&app.tabs[i].scene.document, definition)
        .iter()
        .any(|group| group.entries.iter().any(
            |entry| entry.handle == action && matches!(entry.kind, bars::EntryKind::Orphan(_))
        )));
    let _ = app.update(Message::BlockActionBarEdit(action));
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().name(),
        "BASSOCIATE"
    );
    let _ = app.update(Message::CommandEscape);
    let _ = app.dispatch_command("BEDIT_DISCARD");
    let before = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::BlockActionBarMove(
        parameter.handle,
        iced::Vector::new(2., 3.),
    ));
    let _ = app.update(Message::BlockActionBarHide(parameter.handle));
    let _ = app.update(Message::BlockActionBarEdit(action));
    assert!(app.tabs[i].active_cmd.is_none());
    assert!(app.tabs[i].action_bars.offsets.is_empty());
    assert_eq!(app.tabs[i].scene.document, before);
}
