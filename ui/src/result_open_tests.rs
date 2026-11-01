use super::*;
use std::sync::mpsc;

fn queued_state(
    path: &str,
    receiver: mpsc::Receiver<crate::result_loading::Loaded>,
) -> (ResultOpenState, ResultRequests) {
    let mut requests = ResultRequests::default();
    let mut state = ResultOpenState::default();
    state.begin(
        path.into(),
        ResultLoadTask::from_receiver(receiver),
        &mut requests,
    );
    (state, requests)
}
#[test]
fn opening_is_transactional_and_model_page_preserves_pre_geometry() {
    for piece_count in [1, 2] {
        check_transactional_open(piece_count);
    }
}

fn check_transactional_open(piece_count: usize) {
    let mut app = App::new();
    let pre = FemModel::demo_hex8();
    let mut result_mesh = fem_core::FemMesh::demo_hex8();
    result_mesh.nodes[0].position.x = 10.;
    let (tx, rx) = mpsc::channel();
    let mut scene = hecmw::vtk_scene::ResultScene {
        model: FemModel::single_mesh("Result", result_mesh),
        steps: vec![vec![fem_core::StepResult {
            step: 5,
            eigenvalue: None,
            time: 1.,
            fields: vec![fem_core::ResultField::NodeScalar {
                name: "NodalMISES".into(),
                values: vec![1.; 8],
                min: 1.,
                max: 1.,
            }],
        }]],
    };
    if piece_count == 2 {
        scene
            .model
            .add_mesh("Second piece", fem_core::FemMesh::demo_hex8());
        scene.steps.push(scene.steps[0].clone());
    }
    tx.send(Ok(scene)).unwrap();
    let (opening, requests) = queued_state("job.0005.pvtu", rx);
    app.insert_resource(pre.clone())
        .insert_resource(opening)
        .insert_resource(requests)
        .init_resource::<ResultLoadError>()
        .init_resource::<ResultGeometry>()
        .init_resource::<FemResultSet>()
        .init_resource::<visualization::VisualizationSettings>()
        .init_resource::<PlaybackState>()
        .init_resource::<crate::project_io::CameraFitRequest>()
        .insert_resource(SidebarPage::Model)
        .add_systems(Update, (poll_result_open, sync_result_page).chain());
    app.update();
    assert_eq!(
        app.world().resource::<FemResultSet>().by_mesh.len(),
        piece_count
    );
    assert_eq!(
        app.world()
            .resource::<ResultGeometry>()
            .model
            .as_ref()
            .unwrap()
            .meshes
            .len(),
        piece_count
    );
    assert_eq!(
        app.world()
            .resource::<ResultOpenState>()
            .status
            .contains("not welded"),
        piece_count > 1
    );
    assert_eq!(
        app.world().resource::<FemModel>().meshes[0].nodes,
        pre.meshes[0].nodes
    );
    assert_eq!(*app.world().resource::<SidebarPage>(), SidebarPage::Results);
    assert!(app.world().resource::<ResultGeometry>().visible);
    *app.world_mut().resource_mut::<SidebarPage>() = SidebarPage::Model;
    app.update();
    assert!(!app.world().resource::<ResultGeometry>().visible);
    assert_eq!(
        app.world().resource::<FemModel>().meshes[0].nodes,
        pre.meshes[0].nodes
    );
    let (tx, rx) = mpsc::channel();
    tx.send(Err("invalid data".into())).unwrap();
    app.world_mut().resource_mut::<ResultOpenState>().pending =
        Some(ResultLoadTask::from_receiver(rx));
    app.update();
    assert_eq!(app.world().resource::<FemResultSet>().by_mesh[0][0].step, 5);
    assert_eq!(*app.world().resource::<SidebarPage>(), SidebarPage::Model);
}
#[test]
fn failed_or_empty_readers_keep_display_and_finish_the_loading_status() {
    for outcome in ["empty", "error", "disconnected"] {
        let pre = FemModel::demo_hex8();
        let old_fields = vec![fem_core::ResultField::NodeScalar {
            name: "Old".into(),
            values: vec![99.0; 8],
            min: 99.0,
            max: 99.0,
        }];
        let old_contour = visualization::ContourSettings {
            mesh_index: 0,
            step_index: 0,
            field_name: "Old".into(),
            show_deformation: false,
            displacement_field: "Displacement".into(),
            deformation_scale: 3.0,
        };
        let (tx, rx) = mpsc::channel();
        let mut app = App::new();
        let (opening, requests) = queued_state("new.0010.vtu", rx);
        app.insert_resource(pre.clone())
            .insert_resource(opening)
            .insert_resource(requests)
            .init_resource::<ResultLoadError>()
            .insert_resource(ResultGeometry {
                model: Some(pre.clone()),
                visible: false,
            })
            .insert_resource(FemResultSet {
                by_mesh: vec![vec![fem_core::StepResult {
                    step: 99,
                    fields: old_fields.clone(),
                    ..default()
                }]],
                active: None,
            })
            .insert_resource(visualization::VisualizationSettings {
                contour: Some(old_contour.clone()),
                ..default()
            })
            .insert_resource(PlaybackState {
                playing: false,
                elapsed: 0.25,
                ..default()
            })
            .insert_resource(crate::project_io::CameraFitRequest { revision: 9 })
            .insert_resource(SidebarPage::Model)
            .add_systems(Update, poll_result_open);
        let slider = app
            .world_mut()
            .spawn((
                SliderTrack,
                SliderState {
                    id: SliderId::ResultStep,
                    min: 0.0,
                    max: 20.0,
                    value: 7.0,
                    dragging: false,
                },
            ))
            .id();
        app.update(); // waiting keeps the existing display
        assert!(
            app.world()
                .resource::<ResultOpenState>()
                .status
                .starts_with("Loading")
        );
        match outcome {
            "empty" => {
                let mut model = FemModel::demo_hex8();
                model.meshes[0].nodes[0].position.x = 100.0;
                tx.send(Ok(hecmw::vtk_scene::ResultScene {
                    model,
                    steps: vec![vec![]],
                }))
                .unwrap();
            }
            "error" => {
                tx.send(Err("bad file".into())).unwrap();
            }
            _ => drop(tx),
        }
        app.update();
        let state = app.world().resource::<ResultOpenState>();
        assert!(state.pending.is_none());
        assert!(state.status.contains("previous display retained"));
        assert!(!state.status.starts_with("Loading"));
        assert!(app.world().resource::<ResultLoadError>().0.is_some());
        let results = app.world().resource::<FemResultSet>();
        assert_eq!(results.by_mesh[0][0].step, 99);
        assert_eq!(results.by_mesh[0][0].fields, old_fields);
        assert_eq!(
            app.world()
                .resource::<visualization::VisualizationSettings>()
                .contour,
            Some(old_contour)
        );
        assert_eq!(
            app.world().resource::<FemModel>().meshes[0].nodes,
            pre.meshes[0].nodes
        );
        let geometry = app.world().resource::<ResultGeometry>();
        assert_eq!(
            geometry.model.as_ref().unwrap().meshes[0].nodes,
            pre.meshes[0].nodes
        );
        assert!(!geometry.visible);
        assert_eq!(*app.world().resource::<SidebarPage>(), SidebarPage::Model);
        assert_eq!(
            app.world()
                .resource::<crate::project_io::CameraFitRequest>()
                .revision,
            9
        );
        assert_eq!(app.world().get::<SliderState>(slider).unwrap().value, 7.0);
        assert_eq!(app.world().get::<SliderState>(slider).unwrap().max, 20.0);
        assert!(!app.world().resource::<PlaybackState>().playing);
        assert_eq!(app.world().resource::<PlaybackState>().elapsed, 0.25);
    }
}

#[test]
fn finds_res_mesh_in_project_ancestor() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("job.msh"), "").unwrap();
    let res = dir.path().join("fstrRES/STEP1/job.res.0.1");
    assert_eq!(candidate_mesh(&res), Some(dir.path().join("job.msh")));
    assert!(candidate_mesh(&dir.path().join("unknown.res.0.1")).is_none());
}
