use super::*;
use crate::layout::{UndoInProgress, UndoStack, push_undo_before_setup_change, undo_redo_system};
use bevy::input::{
    ButtonState,
    keyboard::{Key, KeyboardInput},
};

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((bevy::ui_widgets::ButtonPlugin, bevy::ui_widgets::MenuPlugin));
    app.init_resource::<AnalysisSetup>()
        .init_resource::<InputFocus>()
        .init_resource::<UndoStack>()
        .init_resource::<UndoInProgress>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<fem_core::UiKeyboardState>()
        .insert_resource(SidebarPage::Solve)
        .add_message::<KeyboardInput>()
        .add_systems(
            PreUpdate,
            bevy::input_focus::dispatch_focused_input::<KeyboardInput>,
        )
        .add_systems(
            Update,
            (push_undo_before_setup_change, undo_redo_system).chain(),
        )
        .add_systems(Startup, |mut commands: Commands| {
            commands.spawn(Node::default()).with_children(spawn);
        });
    app.world_mut().spawn(bevy::window::PrimaryWindow);
    register(&mut app);
    app.update();
    app
}
fn button(app: &mut App, kind: Kind) -> Entity {
    app.world_mut()
        .query::<(&Kind, &Children)>()
        .iter(app.world())
        .find(|(k, _)| **k == kind)
        .unwrap()
        .1[0]
}
fn open(app: &mut App, kind: Kind) -> Entity {
    let button = button(app, kind);
    assert!(app.world().get::<WidgetButton>(button).is_some());
    let label = app.world().get::<Children>(button).unwrap()[0];
    assert_eq!(app.world().get::<Pickable>(label), Some(&Pickable::IGNORE));
    crate::widget_test_input::click(app.world_mut(), button);
    app.update();
    app.world_mut()
        .query_filtered::<Entity, With<Popup>>()
        .single(app.world())
        .unwrap()
}
fn item(app: &mut App, choice: Choice) -> Entity {
    app.world_mut()
        .query::<(Entity, &Choice)>()
        .iter(app.world())
        .find(|(_, c)| **c == choice)
        .unwrap()
        .0
}
fn click_choice(app: &mut App, choice: Choice) {
    let entity = item(app, choice);
    let label = app.world().get::<Children>(entity).unwrap()[0];
    assert_eq!(app.world().get::<Pickable>(label), Some(&Pickable::IGNORE));
    crate::widget_test_input::click(app.world_mut(), entity);
    app.update();
}
fn key(app: &mut App, code: KeyCode, logical_key: Key) {
    let window = app
        .world_mut()
        .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
        .single(app.world())
        .unwrap();
    app.world_mut().write_message(KeyboardInput {
        key_code: code,
        logical_key,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });
    app.update();
}
fn assert_export(app: &App, choice: Choice) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.cnt");
    hecmw::cnt_writer::write_cnt_file(&path, app.world().resource::<AnalysisSetup>()).unwrap();
    let cnt = std::fs::read_to_string(path).unwrap();
    assert!(
        cnt.contains(&choice.keyword()),
        "{} missing from export",
        choice.keyword()
    );
}

#[test]
fn pointer_selection_preserves_numbers_exports_and_supports_undo_redo() {
    let mut app = app();
    let original = SolverSettings {
        substeps: 17,
        max_iterations: 123,
        convergence_tol: 2e-7,
        ..default()
    };
    app.world_mut().resource_mut::<AnalysisSetup>().solver = original.clone();
    app.update();
    app.world_mut().resource_mut::<UndoStack>().undo.clear();
    let popup = open(&mut app, Kind::Method);
    let choices: Vec<Choice> = app
        .world()
        .get::<Children>(popup)
        .unwrap()
        .iter()
        .filter_map(|e| app.world().get::<Choice>(e).copied())
        .collect();
    assert_eq!(choices[0], Choice::Method(LinearSolverMethod::Mumps));
    click_choice(&mut app, choices[0]);
    assert!(app.world().get_entity(popup).is_err());
    let mut expected = original.clone();
    expected.solver_method = LinearSolverMethod::Mumps;
    assert_eq!(app.world().resource::<AnalysisSetup>().solver, expected);
    assert_eq!(app.world().resource::<UndoStack>().undo.len(), 1);
    assert_export(&app, choices[0]);
    // Selecting the current value must not create another undo entry.
    open(&mut app, Kind::Method);
    click_choice(&mut app, choices[0]);
    assert_eq!(app.world().resource::<UndoStack>().undo.len(), 1);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyZ);
    app.update();
    assert_eq!(app.world().resource::<AnalysisSetup>().solver, original);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyY);
    app.update();
    assert_eq!(app.world().resource::<AnalysisSetup>().solver, expected);
}

#[test]
fn keyboard_escape_cancels_and_enter_selects_analysis() {
    let mut app = app();
    let original = app.world().resource::<AnalysisSetup>().solver.clone();
    let popup = open(&mut app, Kind::Analysis);
    key(&mut app, KeyCode::Escape, Key::Escape);
    assert!(app.world().get_entity(popup).is_err());
    assert_eq!(app.world().resource::<AnalysisSetup>().solver, original);
    assert!(app.world().resource::<UndoStack>().undo.is_empty());
    open(&mut app, Kind::Analysis);
    let choice = Choice::Analysis(AnalysisType::NlStatic);
    let selected = item(&mut app, choice);
    key(&mut app, KeyCode::ArrowDown, Key::ArrowDown);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(selected));
    key(&mut app, KeyCode::Enter, Key::Enter);
    assert_eq!(
        app.world().resource::<AnalysisSetup>().solver.analysis_type,
        AnalysisType::NlStatic
    );
    assert_eq!(
        app.world().resource::<AnalysisSetup>().solver.solver_method,
        original.solver_method
    );
    assert_export(&app, choice);
}

#[test]
fn focus_loss_page_change_and_external_settings_change_close_without_applying() {
    let mut app = app();
    let original = app.world().resource::<AnalysisSetup>().solver.clone();
    let popup = open(&mut app, Kind::Method);
    app.world_mut().resource_mut::<InputFocus>().clear();
    app.update();
    assert!(app.world().get_entity(popup).is_err());
    assert_eq!(app.world().resource::<AnalysisSetup>().solver, original);
    let popup = open(&mut app, Kind::Method);
    *app.world_mut().resource_mut::<SidebarPage>() = SidebarPage::Results;
    app.update();
    assert!(app.world().get_entity(popup).is_err());
    assert!(app.world().resource::<InputFocus>().get().is_none());
    assert_eq!(app.world().resource::<AnalysisSetup>().solver, original);
    *app.world_mut().resource_mut::<SidebarPage>() = SidebarPage::Solve;
    open(&mut app, Kind::Method);
    app.world_mut()
        .resource_mut::<AnalysisSetup>()
        .solver
        .solver_method = LinearSolverMethod::Direct;
    // A stale click before sync cannot overwrite the externally restored value.
    click_choice(&mut app, Choice::Method(LinearSolverMethod::Mumps));
    assert_eq!(
        app.world().resource::<AnalysisSetup>().solver.solver_method,
        LinearSolverMethod::Direct
    );
    let label = app
        .world_mut()
        .query::<(&Label, &Text)>()
        .iter(app.world())
        .find(|(l, _)| l.0 == Kind::Method)
        .unwrap()
        .1;
    assert!(label.0.contains("Direct"));
}

#[test]
fn all_menu_choices_keep_existing_cnt_mapping() {
    let mut app = app();
    for kind in [Kind::Analysis, Kind::Method] {
        for choice in kind.choices() {
            open(&mut app, kind);
            click_choice(&mut app, choice);
            assert_export(&app, choice);
        }
    }
}
