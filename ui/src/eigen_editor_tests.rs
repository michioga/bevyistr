use super::*;
use crate::layout::{UndoInProgress, UndoStack, push_undo_before_setup_change, undo_redo_system};
use fem_core::{AnalysisSetup, AnalysisType};

fn app() -> App {
    let mut app = App::new();
    let mut setup = AnalysisSetup::default();
    setup.solver.analysis_type = AnalysisType::Eigen;
    app.insert_resource(setup)
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<InputFocus>()
        .init_resource::<UiKeyboardState>()
        .init_resource::<SolverEditorState>()
        .init_resource::<UndoStack>()
        .init_resource::<UndoInProgress>()
        .add_systems(Startup, |mut commands: Commands| {
            commands
                .spawn(Node::default())
                .with_children(spawn_solver_exact_editor);
        })
        .add_systems(
            Update,
            (
                solver_numeric_input_system,
                push_undo_before_setup_change,
                undo_redo_system,
            )
                .chain(),
        );
    app.update();
    app
}

fn type_value(app: &mut App, field: SolverField, value: &str) -> Entity {
    let entity = app
        .world_mut()
        .query::<(Entity, &SolverValueInput)>()
        .iter(app.world())
        .find(|(_, f)| f.0 == field)
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
fn imported_time_integration_can_be_edited_without_changing_step_controls() {
    let mut app = app();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("time.cnt");
    std::fs::write(&path, concat!(
        "!SOLUTION, TYPE=DYNAMIC\n!DYNAMIC, TYPE=NONLINEAR\n",
        "1,1\n0,1e5,10000,10\n0.5,0.25\n1,1,0,0\n50,1,50\n1,0,0,0,0,0\n!END\n"
    )).unwrap();
    hecmw::load_cnt_file(&path, &fem_core::FemMesh::demo_hex8(), 0)
        .unwrap().merge_into(&mut app.world_mut().resource_mut::<AnalysisSetup>());
    app.update();

    type_value(&mut app, SolverField::DynamicIncrement, "0");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.world().resource::<AnalysisSetup>().solver.dynamic.as_ref().unwrap().fallback_increment, 10.);
    key(&mut app, KeyCode::Escape);
    type_value(&mut app, SolverField::DynamicIncrement, "1.2345678901234567");
    key(&mut app, KeyCode::Enter);
    type_value(&mut app, SolverField::DynamicSubsteps, "1234");
    key(&mut app, KeyCode::Enter);
    let settings = &app.world().resource::<AnalysisSetup>().solver;
    let dynamic = settings.dynamic.as_ref().unwrap();
    assert_eq!(dynamic.fallback_increment, 1.2345678901234567);
    assert_eq!(dynamic.fallback_substeps, 1234);
    assert_eq!(dynamic.gamma, 0.5);
    assert_eq!(settings.substeps, 1);
}

#[test]
fn eigen_values_commit_restore_and_undo_at_f64_precision() {
    let mut app = app();
    let before = app.world().resource::<AnalysisSetup>().solver.clone();
    type_value(&mut app, SolverField::EigenShift, "-0.12345678901234567");
    app.update();
    assert_eq!(app.world().resource::<AnalysisSetup>().solver, before);
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.world().resource::<AnalysisSetup>().solver.eigen.shift,
        -0.12345678901234567
    );
    assert_eq!(
        app.world()
            .resource::<AnalysisSetup>()
            .solver
            .convergence_tol,
        before.convergence_tol
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    let input = type_value(&mut app, SolverField::EigenShift, "9");
    key(&mut app, KeyCode::Escape);
    assert_eq!(
        editable_value(app.world().get::<EditableText>(input).unwrap()),
        format!("{:.16e}", -0.12345678901234567_f64)
    );
    let after = app.world().resource::<AnalysisSetup>().solver.clone();
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
    assert_eq!(app.world().resource::<AnalysisSetup>().solver, before);
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
    assert_eq!(app.world().resource::<AnalysisSetup>().solver, after);
}

#[test]
fn hidden_eigen_input_cannot_commit_after_analysis_change() {
    let mut app = app();
    type_value(&mut app, SolverField::EigenModes, "9");
    app.world_mut()
        .resource_mut::<AnalysisSetup>()
        .solver
        .analysis_type = AnalysisType::Static;
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.world()
            .resource::<AnalysisSetup>()
            .solver
            .eigen
            .mode_count,
        3
    );
    assert!(app.world().resource::<InputFocus>().get().is_none());
    let display = app
        .world_mut()
        .query::<(&SolverControlGroup, &Node)>()
        .iter(app.world())
        .find(|(g, _)| matches!(g, SolverControlGroup::Eigen))
        .unwrap()
        .1
        .display;
    assert_eq!(display, Display::None);
    app.world_mut()
        .resource_mut::<AnalysisSetup>()
        .solver
        .analysis_type = AnalysisType::Eigen;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    app.update();
    let display = app
        .world_mut()
        .query::<(&SolverControlGroup, &Node)>()
        .iter(app.world())
        .find(|(g, _)| matches!(g, SolverControlGroup::Eigen))
        .unwrap()
        .1
        .display;
    assert_eq!(display, Display::Flex);
}

#[test]
fn eigen_numeric_domains_and_precision_are_independent() {
    for value in ["0", "-1", "NaN", "inf", "1e-999"] {
        assert!(parse_solver_value(SolverField::EigenTolerance, value).is_err());
    }
    for value in ["0", "-1", "1.5", "2147483648"] {
        assert!(parse_solver_value(SolverField::EigenModes, value).is_err());
        assert!(parse_solver_value(SolverField::EigenMaxIterations, value).is_err());
    }
    let mut settings = fem_core::SolverSettings::default();
    for (field, value) in [
        (SolverField::EigenTolerance, 1.2345678901234567e-100),
        (SolverField::EigenShift, -0.12345678901234567),
        (SolverField::EigenShift, 0.),
    ] {
        apply_solver_value(&mut settings, field, ParsedSolverValue::Double(value));
        assert_eq!(
            parse_solver_value(field, &solver_value_text(&settings, field)),
            Ok(ParsedSolverValue::Double(value))
        );
    }
}
