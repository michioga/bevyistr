use super::*;
use crate::{
    results_ui::{PlaybackState, apply_slider_to_results},
    slider::{SliderId, SliderState, SliderTrack},
    solve_results_ui::{
        OpenRunResultsButton, SolveResultsState, open_run_results_system, poll_run_results_system,
    },
};
use fem_core::{FemModelVersion, FemResultSet, ResultField, StepResult};
use visualization::VisualizationSettings;

fn fixture(phase: SolverRunPhase) -> (tempfile::TempDir, App, Entity, Entity) {
    let dir = tempfile::tempdir().unwrap();
    let mut model = FemModel::demo_hex8();
    model.add_mesh("second", fem_core::FemMesh::demo_hex8());
    let mut source = crate::run_results::RunResultSource::capture(dir.path(), "job", 1, 0).unwrap();
    let offsets = hecmw::assembly_id_offsets(&model);
    let count: usize = model.meshes.iter().map(|mesh| mesh.nodes.len()).sum();
    let mut text = format!("*fstrresult\n{count} 0\n1 0\n3\nDISPLACEMENT\n");
    for (mi, mesh) in model.meshes.iter().enumerate() {
        for node in &mesh.nodes {
            let id = hecmw::remap_node(&offsets, mi, node.id);
            text.push_str(&format!("{}\n{} 0 0\n", id.0, mi + 1));
        }
    }
    for step in [2, 10] {
        std::fs::write(dir.path().join(format!("job.res.0.{step}")), &text).unwrap();
    }
    source.finish().unwrap();
    let mut app = App::new();
    app.insert_resource(FrontistrRunState {
        phase,
        results_source: Some(source),
        ..default()
    })
    .insert_resource(model)
    .init_resource::<FemModelVersion>()
    .init_resource::<fem_core::ResultGeometry>()
    .init_resource::<SolveResultsState>()
    .init_resource::<VisualizationSettings>()
    .insert_resource(SidebarPage::Solve)
    .insert_resource(PlaybackState {
        playing: true,
        elapsed: 0.15,
        ..default()
    })
    .insert_resource(FemResultSet {
        by_mesh: vec![vec![StepResult {
            step: 99,
            fields: vec![ResultField::NodeScalar {
                name: "Old".into(),
                values: vec![99.0; 8],
                min: 99.0,
                max: 99.0,
            }],
            ..default()
        }]],
        ..default()
    })
    .add_systems(Update, open_run_results_system);
    let button = app
        .world_mut()
        .spawn((
            OpenRunResultsButton,
            Interaction::None,
            BackgroundColor::default(),
        ))
        .id();
    let slider = app
        .world_mut()
        .spawn((
            SliderTrack,
            SliderState {
                id: SliderId::ResultStep,
                min: 0.0,
                max: 99.0,
                value: 99.0,
                dragging: false,
            },
        ))
        .id();
    (dir, app, button, slider)
}

fn wait(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.update();
        let status = &app.world().resource::<SolveResultsState>().status;
        if status.starts_with("Loaded") || status.starts_with("Could not") {
            break;
        }
        assert!(Instant::now() < deadline, "loading timed out: {status}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn successful_handoff_replaces_results_resets_timeline_and_survives_reopen() {
    let (_dir, mut app, button, slider) = fixture(SolverRunPhase::Succeeded);
    app.add_systems(
        Update,
        (
            poll_run_results_system.after(open_run_results_system),
            apply_slider_to_results.after(poll_run_results_system),
        ),
    );
    wait(&mut app);
    assert_eq!(*app.world().resource::<SidebarPage>(), SidebarPage::Results);
    let results = app.world().resource::<FemResultSet>();
    assert_eq!(results.by_mesh.len(), 2);
    for (mi, steps) in results.by_mesh.iter().enumerate() {
        let ResultField::NodeVector {
            values,
            min_mag,
            max_mag,
            ..
        } = &steps[0].fields[0]
        else {
            panic!()
        };
        assert_eq!(values.len(), 8);
        assert_eq!(values[0].x, (mi + 1) as f32);
        assert_eq!((*min_mag, *max_mag), (1.0, 2.0));
    }
    assert_eq!(
        results.by_mesh[0]
            .iter()
            .map(|s| s.step)
            .collect::<Vec<_>>(),
        [2, 10]
    );
    assert_eq!(results.active.as_ref().unwrap().field_name, "Displacement");
    assert_eq!(app.world().get::<SliderState>(slider).unwrap().value, 1.0);
    assert_eq!(app.world().get::<SliderState>(slider).unwrap().max, 1.0);
    assert!(!app.world().resource::<PlaybackState>().playing);
    // No idle change ticks: stopped playback must not rebuild large meshes.
    let tick = app
        .world()
        .get_resource_ref::<FemResultSet>()
        .unwrap()
        .last_changed();
    let settings_tick = app
        .world()
        .get_resource_ref::<VisualizationSettings>()
        .unwrap()
        .last_changed();
    app.update();
    assert_eq!(
        app.world()
            .get_resource_ref::<FemResultSet>()
            .unwrap()
            .last_changed(),
        tick
    );
    assert_eq!(
        app.world()
            .get_resource_ref::<VisualizationSettings>()
            .unwrap()
            .last_changed(),
        settings_tick
    );
    app.world_mut()
        .get_mut::<SliderState>(slider)
        .unwrap()
        .value = 0.0;
    app.update();
    assert_eq!(
        app.world()
            .resource::<VisualizationSettings>()
            .contour
            .as_ref()
            .unwrap()
            .step_index,
        0
    );
    app.world_mut()
        .get_mut::<Interaction>(button)
        .unwrap()
        .set_if_neq(Interaction::None);
    app.update();
    *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Pressed;
    wait(&mut app);
    assert_eq!(app.world().resource::<FemResultSet>().by_mesh[0].len(), 2);
    assert_eq!(app.world().get::<SliderState>(slider).unwrap().value, 1.0);
}

#[test]
fn incomplete_result_keeps_old_display_and_solve_page() {
    let (dir, mut app, _, _) = fixture(SolverRunPhase::Succeeded);
    std::fs::write(dir.path().join("job.res.0.10"), "*fstrresult\n").unwrap();
    app.add_systems(
        Update,
        poll_run_results_system.after(open_run_results_system),
    );
    wait(&mut app);
    assert!(
        app.world()
            .resource::<SolveResultsState>()
            .status
            .starts_with("Could not")
    );
    assert_eq!(*app.world().resource::<SidebarPage>(), SidebarPage::Solve);
    assert_eq!(
        app.world().resource::<FemResultSet>().by_mesh[0][0].step,
        99
    );
}

#[test]
fn model_changes_cancel_pending_results_and_non_success_cannot_open() {
    let (_dir, mut app, _, _) = fixture(SolverRunPhase::Succeeded);
    app.update(); // queue the worker, but do not poll yet
    app.world_mut().resource_mut::<FemModelVersion>().value += 1;
    app.add_systems(
        Update,
        poll_run_results_system.after(open_run_results_system),
    );
    app.update();
    assert!(
        app.world()
            .resource::<SolveResultsState>()
            .status
            .contains("cancelled")
    );
    assert_eq!(
        app.world().resource::<FemResultSet>().by_mesh[0][0].step,
        99
    );
    for phase in [
        SolverRunPhase::Running,
        SolverRunPhase::Failed,
        SolverRunPhase::Stopped,
    ] {
        let (_dir, mut app, _, _) = fixture(phase);
        app.add_systems(
            Update,
            poll_run_results_system.after(open_run_results_system),
        );
        app.update();
        assert!(
            app.world()
                .resource::<FrontistrRunState>()
                .completed_results()
                .is_none()
        );
        assert_eq!(*app.world().resource::<SidebarPage>(), SidebarPage::Solve);
        assert_eq!(
            app.world().resource::<FemResultSet>().by_mesh[0][0].step,
            99
        );
    }
}
