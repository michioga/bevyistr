use super::*;

#[path = "solve_results_tests.rs"]
mod solve_results_tests;

#[test]
fn export_target_controls_ready_state_and_can_be_cleared() {
    let mut state = FrontistrRunState::default();
    assert_eq!(state.phase, SolverRunPhase::Idle);
    assert!(state.project().is_none());

    state.register_export(PathBuf::from("project"), "model".to_string());
    assert_eq!(state.phase, SolverRunPhase::Ready);
    assert_eq!(state.project().unwrap().stem, "model");

    state.clear_export_target();
    assert_eq!(state.phase, SolverRunPhase::Idle);
    assert!(state.project().is_none());
}

#[test]
fn solver_log_keeps_a_bounded_tail() {
    let mut state = FrontistrRunState::default();
    for index in 0..(MAX_LOG_LINES + 5) {
        state.append_log(ProcessOutputStream::Stdout, format!("line {index}"));
    }

    assert_eq!(state.log_lines.len(), MAX_LOG_LINES);
    assert_eq!(state.log_lines.front().unwrap(), "line 5");
    assert_eq!(
        state.log_lines.back().unwrap(),
        &format!("line {}", MAX_LOG_LINES + 4)
    );
}

#[test]
fn first_run_writes_inputs_without_a_separate_export() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = FrontistrRunState::default();
    assert!(
        state
            .prepare_inputs(
                &FemModel::demo_hex8(),
                &AnalysisSetup::default(),
                "demo",
                |hint| {
                    assert!(hint.is_none());
                    Some(dir.path().into())
                }
            )
            .unwrap()
    );
    for file in ["demo.msh", "demo.cnt", "hecmw_ctrl.dat"] {
        assert!(dir.path().join(file).is_file(), "missing {file}");
    }
    assert_eq!(state.phase, SolverRunPhase::Ready);
    assert_eq!(state.last_output_directory(), Some(dir.path()));
    // This model can run again without another folder dialog.
    assert!(
        state
            .prepare_inputs(
                &FemModel::demo_hex8(),
                &AnalysisSetup::default(),
                "ignored",
                |_| panic!("unexpected picker")
            )
            .unwrap()
    );
    assert_eq!(state.project().unwrap().stem, "demo");
}

#[test]
fn cancelled_first_run_does_not_reuse_the_previous_output_folder() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = FrontistrRunState::from_preferences(&SolverPreferences {
        last_output_directory: Some(dir.path().into()),
        ..default()
    });
    assert!(
        !state
            .prepare_inputs(
                &FemModel::demo_hex8(),
                &AnalysisSetup::default(),
                "demo",
                |hint| {
                    assert_eq!(hint, Some(dir.path()));
                    None
                }
            )
            .unwrap()
    );
    assert!(state.project().is_none());
    assert!(!state.is_running());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    assert!(state.message.contains("cancelled"));
}

#[test]
fn validation_precedes_the_run_folder_dialog() {
    let mut setup = AnalysisSetup::default();
    setup.solver.substeps = 0;
    let mut state = FrontistrRunState::default();
    assert!(
        state
            .prepare_inputs(&FemModel::demo_hex8(), &setup, "demo", |_| panic!(
                "invalid input should not open picker"
            ))
            .is_err()
    );
    assert!(state.project().is_none());
}

#[test]
fn failed_input_write_does_not_remember_a_new_target() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = FrontistrRunState::default();
    let missing = dir.path().join("missing");
    assert!(
        state
            .prepare_inputs(
                &FemModel::demo_hex8(),
                &AnalysisSetup::default(),
                "demo",
                |_| Some(missing)
            )
            .is_err()
    );
    assert!(state.project().is_none());
    assert!(state.last_output_directory().is_none());
}

#[test]
fn run_button_is_available_before_export_and_cannot_start_twice() {
    let mut app = App::new();
    app.init_resource::<FrontistrRunState>();
    app.init_resource::<AnalysisSetup>();
    let button = app
        .world_mut()
        .spawn((
            RunFrontistrButton,
            Interaction::None,
            BackgroundColor::default(),
            BorderColor::default(),
        ))
        .id();
    app.add_systems(Update, run_frontistr_button_system);
    app.update();
    assert_eq!(
        app.world().get::<BackgroundColor>(button).unwrap().0,
        RUN_NORMAL
    );
    app.world_mut().resource_mut::<FrontistrRunState>().phase = SolverRunPhase::Running;
    *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Pressed;
    app.update();
    assert_eq!(
        app.world().get::<BackgroundColor>(button).unwrap().0,
        BUTTON_DISABLED
    );
    assert!(app.world().resource::<FrontistrRunState>().is_running());
}

#[test]
fn mpi_rank_adjustment_is_exact_and_bounded() {
    let mut state = FrontistrRunState::default();
    state.set_launch_mode(SolverLaunchMode::Mpi);
    state.mpi_ranks = 1;

    state.adjust_mpi_ranks(-1);
    assert_eq!(state.mpi_ranks, 1);
    state.adjust_mpi_ranks(7);
    assert_eq!(state.mpi_ranks, 8);
    state.mpi_ranks = 4096;
    state.adjust_mpi_ranks(1);
    assert_eq!(state.mpi_ranks, 4096);
}

#[test]
fn execution_controls_switch_modes_and_freeze_during_a_run() {
    fn spawn_panel(mut commands: Commands) {
        commands
            .spawn(Node::default())
            .with_children(spawn_solver_execution_ui);
    }
    let mut app = App::new();
    app.insert_resource(FrontistrRunState {
        launch_mode: SolverLaunchMode::Direct,
        mpi_ranks: 2,
        ..default()
    });
    app.add_systems(Startup, spawn_panel);
    app.add_systems(
        Update,
        (
            solver_launch_mode_button_system,
            mpi_rank_adjust_button_system,
            update_mpi_rank_controls_system,
            update_frontistr_run_ui_system,
        )
            .chain(),
    );
    app.update();
    let controls = app
        .world_mut()
        .query_filtered::<Entity, With<MpiRankControls>>()
        .single(app.world())
        .unwrap();
    assert_eq!(
        app.world().get::<Node>(controls).unwrap().display,
        Display::None
    );
    let mpi_button = app
        .world_mut()
        .query::<(Entity, &SolverLaunchModeButton)>()
        .iter(app.world())
        .find(|(_, mode)| mode.0 == SolverLaunchMode::Mpi)
        .unwrap()
        .0;
    *app.world_mut().get_mut::<Interaction>(mpi_button).unwrap() = Interaction::Pressed;
    app.update();
    assert_eq!(
        app.world().get::<Node>(controls).unwrap().display,
        Display::Flex
    );
    let increment = app
        .world_mut()
        .query::<(Entity, &MpiRankAdjustButton)>()
        .iter(app.world())
        .find(|(_, delta)| delta.0 == 1)
        .unwrap()
        .0;
    *app.world_mut().get_mut::<Interaction>(increment).unwrap() = Interaction::Pressed;
    app.update();
    assert_eq!(app.world().resource::<FrontistrRunState>().mpi_ranks, 3);

    app.world_mut().resource_mut::<FrontistrRunState>().phase = SolverRunPhase::Running;
    let direct = app
        .world_mut()
        .query::<(Entity, &SolverLaunchModeButton)>()
        .iter(app.world())
        .find(|(_, mode)| mode.0 == SolverLaunchMode::Direct)
        .unwrap()
        .0;
    *app.world_mut().get_mut::<Interaction>(direct).unwrap() = Interaction::Pressed;
    *app.world_mut().get_mut::<Interaction>(increment).unwrap() = Interaction::None;
    app.update();
    *app.world_mut().get_mut::<Interaction>(increment).unwrap() = Interaction::Pressed;
    app.update();
    let state = app.world().resource::<FrontistrRunState>();
    assert_eq!(state.launch_mode, SolverLaunchMode::Mpi);
    assert_eq!(state.mpi_ranks, 3);
}
