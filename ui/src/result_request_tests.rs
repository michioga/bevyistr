use super::*;
use crate::{
    project_io::CameraFitRequest,
    result_loading::{Loaded, ResultLoadTask},
    result_open::{ResultOpenState, poll_result_open, sync_result_page},
    result_requests::ResultRequests,
    results_ui::ResultLoadError,
};
use std::sync::mpsc;

fn manual_request(app: &mut App) -> mpsc::Sender<Loaded> {
    let (tx, rx) = mpsc::channel();
    app.world_mut()
        .resource_scope(|world, mut requests: Mut<ResultRequests>| {
            world.resource_mut::<ResultOpenState>().begin(
                "manual.0007.pvtu".into(),
                ResultLoadTask::from_receiver(rx),
                &mut requests,
            );
        });
    tx
}

fn analysis_request(app: &mut App) -> mpsc::Sender<Result<Vec<Vec<StepResult>>, String>> {
    let (tx, rx) = mpsc::channel();
    let run_id = app
        .world()
        .resource::<FrontistrRunState>()
        .completed_results()
        .unwrap()
        .id;
    let version = app.world().resource::<FemModelVersion>().value;
    let request = app.world_mut().resource_mut::<ResultRequests>().begin();
    app.insert_resource(SolveResultsState::pending_for_test(
        run_id, version, request, rx,
    ));
    tx
}

fn manual_scene() -> hecmw::vtk_scene::ResultScene {
    let mut mesh = fem_core::FemMesh::demo_hex8();
    for node in &mut mesh.nodes {
        node.position.x += 50.0;
    }
    hecmw::vtk_scene::ResultScene {
        model: FemModel::single_mesh("Manual", mesh),
        steps: vec![vec![sample("Manual", 7)]],
    }
}

fn sample(name: &str, step: u32) -> StepResult {
    StepResult {
        step,
        fields: vec![ResultField::NodeScalar {
            name: name.into(),
            values: vec![step as f32; 8],
            min: step as f32,
            max: step as f32,
        }],
        ..default()
    }
}

fn configure(app: &mut App, manual_first: bool) {
    app.init_resource::<ResultOpenState>()
        .init_resource::<ResultLoadError>()
        .init_resource::<CameraFitRequest>();
    if manual_first {
        app.add_systems(
            Update,
            (poll_result_open, poll_run_results_system)
                .chain()
                .after(open_run_results_system),
        );
    } else {
        app.add_systems(
            Update,
            (poll_run_results_system, poll_result_open)
                .chain()
                .after(open_run_results_system),
        );
    }
    app.add_systems(
        Update,
        sync_result_page
            .after(poll_result_open)
            .after(poll_run_results_system),
    );
}

#[test]
fn newer_manual_request_keeps_its_result_when_old_analysis_finishes() {
    for manual_first in [true, false] {
        for old_ready in [true, false] {
            let (_dir, mut app, _, _) = fixture(SolverRunPhase::Succeeded);
            configure(&mut app, manual_first);
            let older = analysis_request(&mut app);
            let newer = manual_request(&mut app);
            if old_ready {
                older
                    .send(Ok(vec![vec![sample("Old analysis", 2)]; 2]))
                    .unwrap();
            }
            newer.send(Ok(manual_scene())).unwrap();
            app.update();
            if !old_ready {
                assert!(
                    older
                        .send(Ok(vec![vec![sample("Old analysis", 2)]; 2]))
                        .is_err()
                );
                app.update();
            }
            let results = app.world().resource::<FemResultSet>();
            assert_eq!(results.by_mesh.len(), 1);
            assert_eq!(results.active.as_ref().unwrap().field_name, "Manual");
            assert_eq!(results.by_mesh[0][0].step, 7);
            let geometry = app.world().resource::<fem_core::ResultGeometry>();
            assert!(
                geometry.model.as_ref().unwrap().meshes[0].nodes[0]
                    .position
                    .x
                    > 40.0
            );
            assert!(geometry.visible);
            assert!(
                app.world()
                    .resource::<ResultOpenState>()
                    .status
                    .contains("manual.0007.pvtu")
            );
            assert!(
                app.world()
                    .resource::<SolveResultsState>()
                    .status
                    .contains("newer result")
            );
            assert_eq!(*app.world().resource::<SidebarPage>(), SidebarPage::Results);
        }
    }
}

#[test]
fn newer_analysis_request_discards_old_manual_success_or_error() {
    for manual_first in [true, false] {
        for old_error in [true, false] {
            for old_ready in [true, false] {
                let (_dir, mut app, _, _) = fixture(SolverRunPhase::Succeeded);
                configure(&mut app, manual_first);
                let older = manual_request(&mut app);
                let newer = analysis_request(&mut app);
                let old_reply = if old_error {
                    Err("Old manual error".into())
                } else {
                    Ok(manual_scene())
                };
                if old_ready {
                    older.send(old_reply).unwrap();
                }
                newer
                    .send(Ok(vec![vec![sample("Automatic", 10)]; 2]))
                    .unwrap();
                app.update();
                if !old_ready {
                    assert!(
                        older
                            .send(if old_error {
                                Err("Old manual error".into())
                            } else {
                                Ok(manual_scene())
                            })
                            .is_err()
                    );
                    app.update();
                }
                let results = app.world().resource::<FemResultSet>();
                assert_eq!(results.by_mesh.len(), 2);
                assert_eq!(results.active.as_ref().unwrap().field_name, "Automatic");
                assert_eq!(results.by_mesh[0][0].step, 10);
                assert!(
                    app.world()
                        .resource::<fem_core::ResultGeometry>()
                        .model
                        .is_none()
                );
                assert!(app.world().resource::<ResultOpenState>().status.is_empty());
                assert!(app.world().resource::<ResultLoadError>().0.is_none());
                assert_eq!(*app.world().resource::<SidebarPage>(), SidebarPage::Results);
            }
        }
    }
}

#[test]
fn completed_manual_source_label_is_cleared_by_later_analysis() {
    let (_dir, mut app, _, _) = fixture(SolverRunPhase::Succeeded);
    configure(&mut app, true);
    let _older = analysis_request(&mut app);
    manual_request(&mut app).send(Ok(manual_scene())).unwrap();
    app.update();
    assert!(
        app.world()
            .resource::<ResultOpenState>()
            .status
            .contains("manual.0007.pvtu")
    );
    assert_eq!(app.world().resource::<CameraFitRequest>().revision, 1);
    analysis_request(&mut app)
        .send(Ok(vec![vec![sample("Automatic", 10)]; 2]))
        .unwrap();
    app.update();
    assert!(app.world().resource::<ResultOpenState>().status.is_empty());
    assert_eq!(app.world().resource::<CameraFitRequest>().revision, 2);
    assert_eq!(
        app.world()
            .resource::<FemResultSet>()
            .active
            .as_ref()
            .unwrap()
            .field_name,
        "Automatic"
    );
}

#[test]
fn newest_failure_retains_previous_display_without_installing_an_older_success() {
    for manual_first in [true, false] {
        for manual_newer in [true, false] {
            let (_dir, mut app, _, _) = fixture(SolverRunPhase::Succeeded);
            configure(&mut app, manual_first);
            if manual_newer {
                let old = analysis_request(&mut app);
                let new = manual_request(&mut app);
                old.send(Ok(vec![vec![sample("Old automatic", 2)]; 2]))
                    .unwrap();
                new.send(Err("New manual failed".into())).unwrap();
            } else {
                let old = manual_request(&mut app);
                let new = analysis_request(&mut app);
                old.send(Ok(manual_scene())).unwrap();
                new.send(Err("New automatic failed".into())).unwrap();
            }
            app.update();
            let results = app.world().resource::<FemResultSet>();
            assert_eq!(results.by_mesh[0][0].step, 99);
            assert_eq!(results.by_mesh[0][0].fields[0].name(), "Old");
            assert!(
                app.world()
                    .resource::<fem_core::ResultGeometry>()
                    .model
                    .is_none()
            );
            assert_eq!(*app.world().resource::<SidebarPage>(), SidebarPage::Solve);
            if manual_newer {
                assert_eq!(
                    app.world().resource::<ResultLoadError>().0.as_deref(),
                    Some("New manual failed")
                );
            } else {
                assert!(
                    app.world()
                        .resource::<SolveResultsState>()
                        .status
                        .contains("New automatic failed")
                );
            }
        }
    }
}
