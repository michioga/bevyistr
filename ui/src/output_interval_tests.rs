use super::*;
use crate::layout::{UndoInProgress, UndoStack, push_undo_before_setup_change, undo_redo_system};

fn app() -> App {
    let mut app = App::new();
    app.init_resource::<AnalysisSetup>()
        .init_resource::<InputFocus>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<UiKeyboardState>()
        .init_resource::<UndoStack>()
        .init_resource::<UndoInProgress>()
        .insert_resource(SidebarPage::Solve)
        .add_plugins(bevy::ui_widgets::ButtonPlugin)
        .add_systems(Startup, |mut c: Commands| {
            c.spawn(Node::default()).with_children(spawn);
        })
        .add_systems(
            Update,
            (update, push_undo_before_setup_change, undo_redo_system).chain(),
        );
    app.update();
    app
}

fn type_value(app: &mut App, target: OutputTarget, value: &str) -> Entity {
    let entity = app
        .world_mut()
        .query::<(Entity, &Interval)>()
        .iter(app.world())
        .find(|(_, i)| i.0 == target)
        .unwrap()
        .0;
    app.world_mut()
        .get_mut::<EditableText>(entity)
        .unwrap()
        .editor_mut()
        .set_text(value);
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(entity, bevy::input_focus::FocusCause::Navigated);
    entity
}

fn key(app: &mut App, code: KeyCode) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(code);
    app.update();
}

#[test]
fn exact_interval_enter_escape_validation_and_undo() {
    let mut app = app();
    let original = app.world().resource::<AnalysisSetup>().output.clone();
    type_value(&mut app, OutputTarget::Res, "5");
    app.update();
    assert_eq!(app.world().resource::<AnalysisSetup>().output, original);
    key(&mut app, KeyCode::Enter);
    let edited = app.world().resource::<AnalysisSetup>().output.clone();
    assert_eq!(edited.write_res.frequency, 5);
    assert_eq!(edited.write_vis.frequency, 1);
    for value in ["0", "2147483648", ""] {
        let input = type_value(&mut app, OutputTarget::Res, value);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.world().resource::<AnalysisSetup>().output, edited);
        key(&mut app, KeyCode::Escape);
        assert_eq!(
            editable_value(app.world().get::<EditableText>(input).unwrap()),
            "5"
        );
    }
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    app.world_mut()
        .resource_mut::<UiKeyboardState>()
        .text_editing = false;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyZ);
    app.update();
    assert_eq!(app.world().resource::<AnalysisSetup>().output, original);
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
    assert_eq!(app.world().resource::<AnalysisSetup>().output, edited);
}

#[test]
fn off_and_eigen_and_hidden_inputs_cannot_commit() {
    let mut app = app();
    let button = app
        .world_mut()
        .query::<(Entity, &Toggle)>()
        .iter(app.world())
        .find(|(_, b)| b.0 == OutputTarget::Res)
        .unwrap()
        .0;
    crate::widget_test_input::click(app.world_mut(), button);
    app.update();
    assert!(
        !app.world()
            .resource::<AnalysisSetup>()
            .output
            .write_res
            .enabled
    );
    type_value(&mut app, OutputTarget::Res, "7");
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.world()
            .resource::<AnalysisSetup>()
            .output
            .write_res
            .frequency,
        1
    );
    app.world_mut()
        .resource_mut::<AnalysisSetup>()
        .solver
        .analysis_type = AnalysisType::Eigen;
    type_value(&mut app, OutputTarget::Vis, "7");
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.world()
            .resource::<AnalysisSetup>()
            .output
            .write_vis
            .frequency,
        1
    );
    app.world_mut()
        .resource_mut::<AnalysisSetup>()
        .solver
        .analysis_type = AnalysisType::Static;
    *app.world_mut().resource_mut::<SidebarPage>() = SidebarPage::Results;
    type_value(&mut app, OutputTarget::Vis, "7");
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.world()
            .resource::<AnalysisSetup>()
            .output
            .write_vis
            .frequency,
        1
    );
}
