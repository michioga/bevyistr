use crate::{ButtonInteraction, step_import::StepImport};
use super::*;
use crate::{
    project_io::{CameraFitRequest, apply_pending_cnt_system, mesh_load_system},
    result_requests::ResultRequests,
    results_ui::PlaybackState,
    solve_results_ui::{SolveResultsState, open_run_results_system, poll_run_results_system},
};
use fem_core::{
    BoundaryCondition, ElementId, FemElement, FemMaterial, FemMesh, FemModelVersion, FemNode,
    FemResultSet, MeshLoadRequest, NodalLoad, NodeId, PendingCntLoad, ResultField, ResultGeometry,
    Section, SectionKind,
};
use std::path::Path;
use visualization::VisualizationSettings;

const LENGTH: u32 = 4;
const YOUNG_MODULUS: f32 = 1000.0;
const NODAL_FORCE: f32 = 1.0;

// Unit cross section, Poisson ratio zero, uniform end traction. The exact
// displacement is Ux = 4 * nodal_force * x / E, with Uy and Uz zero.
fn write_reference_project(directory: &Path) {
    let id = |x: u32, y: u32, z: u32| NodeId(1 + 4 * x + 2 * z + y);
    let mut nodes = Vec::new();
    for x in 0..=LENGTH {
        for z in 0..=1 {
            for y in 0..=1 {
                nodes.push(FemNode::new(
                    id(x, y, z),
                    Vec3::new(x as f32, y as f32, z as f32),
                ));
            }
        }
    }
    let elements = (0..LENGTH)
        .map(|x| {
            FemElement::new_hex8(
                ElementId(x + 1),
                [
                    id(x, 0, 0),
                    id(x + 1, 0, 0),
                    id(x + 1, 1, 0),
                    id(x, 1, 0),
                    id(x, 0, 1),
                    id(x + 1, 0, 1),
                    id(x + 1, 1, 1),
                    id(x, 1, 1),
                ],
            )
        })
        .collect();
    let mut setup = AnalysisSetup::default();
    setup.materials.push(FemMaterial {
        name: "BAR".into(),
        young_modulus: Some(YOUNG_MODULUS),
        poisson_ratio: Some(0.0),
        density: None,
    });
    setup.sections.push(Section {
        name: "BAR".into(),
        mesh_index: 0,
        material_name: "BAR".into(),
        element_set_name: None,
        kind: SectionKind::Solid,
    });
    setup.boundary_conditions.push(BoundaryCondition {
        name: "Fixed end".into(),
        mesh_index: 0,
        nodes: nodes
            .iter()
            .filter(|node| node.position.x == 0.0)
            .map(|node| node.id)
            .collect(),
        ngrp_name: None,
        rotation_center: None,
        dof_start: 1,
        dof_end: 3,
        value: 0.0,
    });
    setup.nodal_loads.extend(
        nodes
            .iter()
            .filter(|node| node.position.x == LENGTH as f32)
            .map(|node| NodalLoad {
                name: "End traction".into(),
                mesh_index: 0,
                node: node.id,
                ngrp_name: None,
                rotation_center: None,
                dof: 1,
                value: NODAL_FORCE,
            }),
    );
    setup.output.write_vis.enabled = false;
    let model = FemModel::single_mesh("Elastic bar", FemMesh::new(nodes, elements));
    hecmw::write_frontistr_project(directory, "bar", &model, &setup).unwrap();
}

fn load_project(input: &Path, output: &Path, mut run: FrontistrRunState) -> (App, Entity, Entity) {
    let control = input.join("hecmw_ctrl.dat");
    let files = hecmw::load_hecmw_ctrl(&control).unwrap();
    let (mesh, cnt) = hecmw::resolve_paths(&control, &files);
    let mut request = MeshLoadRequest::default();
    request.request(mesh.unwrap());
    let mut pending_cnt = PendingCntLoad::default();
    pending_cnt.request(cnt.unwrap(), 0, 0);
    run.register_export(output.into(), "bar".into());

    let mut app = App::new();
    app.insert_resource(request)
        .insert_resource(pending_cnt)
        .insert_resource(run)
        .insert_resource(SidebarPage::Solve)
        .init_resource::<FemModel>()
        .init_resource::<MeshLoadStatus>()
        .init_resource::<StepImport>()
        .init_resource::<FemModelVersion>()
        .init_resource::<CameraFitRequest>()
        .init_resource::<AnalysisSetup>()
        .init_resource::<FemResultSet>()
        .init_resource::<ResultGeometry>()
        .init_resource::<ResultRequests>()
        .init_resource::<SolveResultsState>()
        .init_resource::<VisualizationSettings>()
        .init_resource::<PlaybackState>()
        .add_systems(
            Update,
            (
                mesh_load_system,
                apply_pending_cnt_system,
                run_frontistr_button_system,
                stop_frontistr_button_system,
                poll_frontistr_process_system,
                open_run_results_system,
                poll_run_results_system,
            )
                .chain(),
        );
    let button = app
        .world_mut()
        .spawn((
            RunFrontistrButton,
            ButtonInteraction::None,
            BackgroundColor::default(),
            BorderColor::default(),
        ))
        .id();
    let stop_button = app
        .world_mut()
        .spawn((
            StopFrontistrButton,
            ButtonInteraction::None,
            BackgroundColor::default(),
            BorderColor::default(),
        ))
        .id();
    app.update();
    let world = app.world();
    let mesh_status = world.resource::<MeshLoadStatus>();
    assert!(mesh_status.error.is_none(), "{mesh_status:?}");
    let mesh = &world.resource::<FemModel>().meshes[0];
    assert_eq!(mesh.elements.len(), LENGTH as usize);
    let setup = world.resource::<AnalysisSetup>();
    assert_eq!(setup.nodal_loads.len(), 4);
    assert_eq!(
        setup
            .boundary_conditions
            .iter()
            .map(|bc| bc.nodes.len())
            .sum::<usize>(),
        4
    );
    assert!(setup.output.write_res.enabled);
    (app, button, stop_button)
}

fn click(app: &mut App, button: Entity) {
    *app.world_mut().get_mut::<ButtonInteraction>(button).unwrap() = ButtonInteraction::Pressed;
    app.update();
    *app.world_mut().get_mut::<ButtonInteraction>(button).unwrap() = ButtonInteraction::None;
}

fn wait_until(app: &mut App, seconds: u64, mut finished: impl FnMut(&World) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        app.update();
        if finished(app.world()) {
            return;
        }
        if Instant::now() >= deadline {
            app.world_mut()
                .resource_mut::<FrontistrRunState>()
                .request_stop();
            let stop_deadline = Instant::now() + Duration::from_secs(10);
            while app.world().resource::<FrontistrRunState>().is_running()
                && Instant::now() < stop_deadline
            {
                std::thread::sleep(Duration::from_millis(20));
                app.update();
            }
            let run = app.world().resource::<FrontistrRunState>();
            panic!(
                "Solver workflow timed out: {}\n{}",
                run.status_label(),
                run.log_label()
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn solve_and_check(app: &mut App, button: Entity, force: f32, seconds: u64) -> u64 {
    *app.world_mut().resource_mut::<SidebarPage>() = SidebarPage::Solve;
    click(app, button);
    wait_until(app, seconds, |world| {
        let run = world.resource::<FrontistrRunState>();
        let status = &world.resource::<SolveResultsState>().status;
        assert!(
            run.is_running()
                || (run.completed_results().is_some() && !status.starts_with("Could not")),
            "{:?}: {}\n{status}\n{}",
            run.launch_mode,
            run.status_label(),
            run.log_label()
        );
        *world.resource::<SidebarPage>() == SidebarPage::Results
    });
    let world = app.world();
    let run = world.resource::<FrontistrRunState>();
    let launch_mode = run.launch_mode;
    assert_eq!(run.phase, SolverRunPhase::Succeeded);
    let log = std::fs::read_to_string(run.log_path.as_ref().unwrap()).unwrap();
    assert!(log.contains("FrontISTR exit code: 0"));
    assert_eq!(
        log.contains("=== Partitioning (hecmw_part1) ==="),
        launch_mode == SolverLaunchMode::Mpi
    );
    let results = world.resource::<FemResultSet>();
    let active = results.active.as_ref().unwrap();
    let steps = &results.by_mesh[0];
    assert_eq!(
        steps.iter().map(|step| step.step).collect::<Vec<_>>(),
        [0, 1]
    );
    assert_eq!(active.step_index, 1);
    assert_eq!(active.field_name, "Displacement");
    let contour = world
        .resource::<VisualizationSettings>()
        .contour
        .as_ref()
        .unwrap();
    assert_eq!(contour.step_index, 1);
    assert_eq!(contour.field_name, "Displacement");
    let Some(ResultField::NodeVector {
        values, max_mag, ..
    }) = steps[1].field_by_name("Displacement")
    else {
        panic!("Missing displacement vector");
    };
    let nodes = &world.resource::<FemModel>().meshes[0].nodes;
    assert_eq!(values.len(), nodes.len());
    for (node, value) in nodes.iter().zip(values) {
        let expected = Vec3::X * (4.0 * force * node.position.x / YOUNG_MODULUS);
        assert!(
            value.abs_diff_eq(expected, 1e-5),
            "{launch_mode:?}, node {:?}: {value:?}, expected {expected:?}",
            node.id
        );
    }
    println!(
        "{launch_mode:?}: {force} per loaded node, {} solved nodes, max displacement {max_mag:.6}",
        nodes.len()
    );
    run.completed_results().unwrap().id
}

fn check_latest_results(run: FrontistrRunState, fixture: bool) {
    let input = tempfile::tempdir().unwrap();
    write_reference_project(input.path());
    let output = tempfile::tempdir().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_round_trip(input.path(), output.path(), run, fixture);
    }));
    if let Err(panic) = result {
        if !fixture {
            let directory = output.keep();
            eprintln!(
                "Solver input and full diagnostics retained at {}",
                directory.display()
            );
        }
        std::panic::resume_unwind(panic);
    }
}

fn run_round_trip(input: &Path, output: &Path, run: FrontistrRunState, fixture: bool) {
    let old_result = output.join("bar.res.0.9");
    std::fs::write(&old_result, "old result").unwrap();
    let (mut app, button, stop_button) = load_project(input, output, run);
    let seconds = if fixture { 10 } else { 90 };
    let first_id = solve_and_check(&mut app, button, NODAL_FORCE, seconds);
    for load in &mut app.world_mut().resource_mut::<AnalysisSetup>().nodal_loads {
        load.value *= 2.0;
    }
    if fixture {
        stop_running_fixture(&mut app, button, stop_button, output);
    }
    let second_id = solve_and_check(&mut app, button, NODAL_FORCE * 2.0, seconds);
    assert_ne!(first_id, second_id);
    assert_eq!(std::fs::read_to_string(old_result).unwrap(), "old result");
}

fn stop_running_fixture(app: &mut App, run_button: Entity, stop_button: Entity, output: &Path) {
    let old_results = std::fs::read(output.join("bar.res.0.1")).unwrap();
    let gate = output.join("live-output");
    std::fs::write(&gate, "").unwrap();
    *app.world_mut().resource_mut::<SidebarPage>() = SidebarPage::Solve;
    click(app, run_button);
    wait_until(app, 10, |world| {
        let run = world.resource::<FrontistrRunState>();
        assert!(run.is_running(), "{}", run.status_label());
        run.log_lines
            .iter()
            .any(|line| line == "running-before-completion")
    });
    click(app, stop_button);
    wait_until(app, 10, |world| {
        !world.resource::<FrontistrRunState>().is_running()
    });
    let world = app.world();
    let run = world.resource::<FrontistrRunState>();
    assert_eq!(run.phase, SolverRunPhase::Stopped);
    assert!(run.completed_results().is_none());
    assert_eq!(*world.resource::<SidebarPage>(), SidebarPage::Solve);
    let results = world.resource::<FemResultSet>();
    let Some(ResultField::NodeVector { max_mag, .. }) =
        results.by_mesh[0][1].field_by_name("Displacement")
    else {
        panic!("Stop discarded the previous result");
    };
    assert!((*max_mag - 0.016).abs() < 1e-6);
    let log = std::fs::read_to_string(run.log_path.as_ref().unwrap()).unwrap();
    assert!(log.contains("=== Run stopped by user ==="));
    assert_eq!(
        std::fs::read(output.join("bar.res.0.1")).unwrap(),
        old_results
    );
    std::fs::remove_file(gate).unwrap();
}

#[test]
fn subprocess_workflow_preserves_results_on_stop_and_loads_the_retry() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join(if cfg!(windows) {
        "bar-solver.exe"
    } else {
        "bar-solver"
    });
    std::fs::copy(crate::solver_process::subprocess_fixture(), &executable).unwrap();
    let run = FrontistrRunState {
        executable,
        runtime_environment: RuntimeEnvironment::Inherited,
        ..default()
    };
    check_latest_results(run, true);
}

fn installed_run(launch_mode: SolverLaunchMode) -> FrontistrRunState {
    FrontistrRunState::from_preferences(&SolverPreferences {
        executable: std::env::var_os("FRONTISTR_EXECUTABLE")
            .map_or_else(|| "fistr1".into(), PathBuf::from),
        partitioner: std::env::var_os("FRONTISTR_PARTITIONER").map(PathBuf::from),
        mpi_launcher: std::env::var_os("FRONTISTR_MPI_LAUNCHER").map(PathBuf::from),
        launch_mode,
        mpi_ranks: 2,
        ..default()
    })
}

#[test]
#[ignore = "Requires installed FrontISTR"]
fn installed_frontistr_workflow_direct() {
    check_latest_results(installed_run(SolverLaunchMode::Direct), false);
}

#[test]
#[ignore = "Requires installed FrontISTR, a matching MPI launcher and hecmw_part1"]
fn installed_frontistr_workflow_mpi() {
    check_latest_results(installed_run(SolverLaunchMode::Mpi), false);
}

#[test]
#[ignore = "Requires Gmsh on PATH"]
fn step_model_preserves_analysis_groups_in_mesh_and_solver_inputs() {
    use crate::{
        assembly::AssemblyEditorState,
        project_io::{SaveStepMeshControls, StepMeshSettings, save_step_mesh_button_system},
        step_import::{StepImportAction, step_action_buttons},
        step_preview_view::draw_step_preview,
    };
    use bevy::camera::visibility::RenderLayers;
    use camera::OrbitCamera;
    use fem_core::MainViewportCamera;

    let input = tempfile::tempdir().unwrap();
    std::fs::write(
        input.path().join("box.geo"),
        "SetFactory(\"OpenCASCADE\");\nBox(1) = {0,0,0,20,10,6};\nSave \"box.step\";\n",
    )
    .unwrap();
    let gmsh = std::process::Command::new("gmsh")
        .current_dir(input.path())
        .args(["box.geo", "-parse_and_exit", "-nopopup"])
        .output()
        .unwrap();
    assert!(
        gmsh.status.success(),
        "{}",
        String::from_utf8_lossy(&gmsh.stderr)
    );

    bevy::tasks::IoTaskPool::get_or_init(bevy::tasks::TaskPool::new);
    let mut request = MeshLoadRequest::default();
    request.request(input.path().join("box.step"));
    let old_path = input.path().join("old.msh");
    let original = FemModel::single_mesh("old.msh", FemMesh::new(vec![], vec![]));
    let mut status = MeshLoadStatus::default();
    status.loaded(old_path.clone());
    let mut run = FrontistrRunState::default();
    run.register_export(input.path().into(), "old".into());

    let mut pending_cnt = PendingCntLoad::default();
    pending_cnt.request(input.path().join("superseded.cnt"), 0, 0);
    let mut app = App::new();
    app.insert_resource(request)
        .insert_resource(pending_cnt)
        .insert_resource(original)
        .insert_resource(status)
        .insert_resource(run)
        .init_resource::<StepImport>()
        .init_resource::<StepMeshSettings>()
        .init_resource::<FemModelVersion>()
        .init_resource::<CameraFitRequest>()
        .init_resource::<AnalysisSetup>()
        .init_resource::<AssemblyEditorState>()
        .init_resource::<SidebarPage>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .add_systems(
            Update,
            (
                step_action_buttons,
                mesh_load_system,
                draw_step_preview,
                save_step_mesh_button_system,
            )
                .chain(),
        );
    app.world_mut()
        .resource_mut::<AnalysisSetup>()
        .materials
        .push(FemMaterial {
            name: "OLD".into(),
            young_modulus: Some(1000.0),
            poisson_ratio: Some(0.25),
            density: None,
        });
    let initial_view = Transform::from_xyz(4.0, 10.0, 20.0);
    let original_layers = RenderLayers::layer(0);
    let camera = app
        .world_mut()
        .spawn((
            MainViewportCamera,
            initial_view,
            original_layers.clone(),
            OrbitCamera {
                focus: Vec3::ZERO,
                target_focus: Vec3::ZERO,
                radius: 10.0,
                min_radius: 1.0,
                max_radius: 100.0,
            },
        ))
        .id();
    let accept = app
        .world_mut()
        .spawn((StepImportAction::Accept, ButtonInteraction::None))
        .id();
    let retry = app
        .world_mut()
        .spawn((StepImportAction::Regenerate, ButtonInteraction::None))
        .id();
    let discard = app
        .world_mut()
        .spawn((StepImportAction::Discard, ButtonInteraction::None))
        .id();
    let save_controls = app
        .world_mut()
        .spawn((
            SaveStepMeshControls,
            Node {
                display: Display::None,
                ..default()
            },
        ))
        .id();
    app.update();
    assert!(app.world().resource::<StepImport>().is_meshing());
    assert!(app.world().resource::<PendingCntLoad>().path.is_none());
    wait_until(&mut app, 30, |world| {
        world.resource::<StepImport>().preview().is_some()
    });
    let world = app.world();
    assert_eq!(world.resource::<FemModel>().parts[0].name, "old.msh");
    assert_eq!(world.resource::<AnalysisSetup>().materials[0].name, "OLD");
    assert_eq!(
        world.resource::<MeshLoadStatus>().last_path.as_deref(),
        Some(old_path.as_path())
    );
    assert_eq!(
        world
            .resource::<FrontistrRunState>()
            .project()
            .unwrap()
            .stem,
        "old"
    );
    assert_eq!(world.resource::<FemModelVersion>().value, 0);
    assert_ne!(world.get::<RenderLayers>(camera), Some(&original_layers));

    // WGPU rejects a nonzero depth bias on the preview's line pipeline.
    let world = app.world_mut();
    let mut visuals = world.query_filtered::<
        (&Mesh3d, &MeshMaterial3d<StandardMaterial>),
        With<crate::step_preview_view::StepPreviewVisual>,
    >();
    let meshes = world.resource::<Assets<Mesh>>();
    let materials = world.resource::<Assets<StandardMaterial>>();
    let mut visible_edge_meshes = 0;
    for (mesh, material) in visuals.iter(world) {
        if meshes.get(&mesh.0).unwrap().primitive_topology()
            == bevy::mesh::PrimitiveTopology::LineList
        {
            visible_edge_meshes += 1;
            assert_eq!(materials.get(&material.0).unwrap().depth_bias, 0.0);
        }
    }
    assert_eq!(visible_edge_meshes, 1);

    click(&mut app, discard);
    assert!(app.world().resource::<StepImport>().preview().is_none());
    assert_eq!(
        app.world().get::<RenderLayers>(camera),
        Some(&original_layers)
    );
    assert_eq!(app.world().get::<Transform>(camera), Some(&initial_view));

    app.world_mut()
        .resource_mut::<MeshLoadRequest>()
        .request(input.path().join("box.step"));
    wait_until(&mut app, 30, |world| {
        world.resource::<StepImport>().preview().is_some()
    });
    app.world_mut()
        .resource_mut::<StepMeshSettings>()
        .options
        .max_size_mm = Some(4.0);
    click(&mut app, accept);
    assert_eq!(app.world().resource::<FemModelVersion>().value, 0);

    click(&mut app, retry);
    assert!(app.world().resource::<StepImport>().is_meshing());
    wait_until(&mut app, 30, |world| {
        world.resource::<StepImport>().preview().is_some()
    });
    click(&mut app, accept);
    assert!(app.world().resource::<StepImport>().preview().is_none());
    assert_eq!(app.world().resource::<FemModelVersion>().value, 1);
    assert!(app.world().resource::<AnalysisSetup>().materials.is_empty());
    assert_eq!(
        app.world().get::<RenderLayers>(camera),
        Some(&original_layers)
    );
    let world = app.world();
    assert_eq!(
        world.get::<Node>(save_controls).unwrap().display,
        Display::Flex
    );
    let model = world.resource::<FemModel>();
    assert_eq!(model.parts[0].name, "box.step");
    assert!(
        world
            .resource::<MeshLoadStatus>()
            .message
            .contains("20.000000")
    );
    let mesh = &model.meshes[0];
    let group_at_end = |x: f32| {
        mesh.node_sets
            .iter()
            .find(|group| {
                group
                    .nodes
                    .iter()
                    .all(|node| (mesh.node_position(*node).unwrap().x - x).abs() < 1.0e-5_f32)
            })
            .unwrap()
    };
    let fixed = group_at_end(0.0_f32);
    let loaded = group_at_end(20.0_f32);
    assert_ne!(fixed.name, loaded.name);
    assert_eq!(mesh.node_sets.len(), 6);
    let mesh_path = input.path().join("geometry.msh");
    assert_eq!(
        hecmw::write_msh_geometry_file(&mesh_path, mesh).unwrap(),
        (mesh.nodes.len(), mesh.elements.len())
    );
    let geometry = hecmw::load_mesh_project(&mesh_path).unwrap();
    assert_eq!(geometry.mesh.node_sets, mesh.node_sets);
    assert_eq!(geometry.mesh.surface_sets, mesh.surface_sets);
    assert_eq!(geometry.mesh.element_sets, mesh.element_sets);
    assert_eq!(geometry.mesh.elements, mesh.elements);
    assert!(geometry.materials.is_empty());
    assert!(geometry.sections.is_empty());

    let mut setup = AnalysisSetup::default();
    setup.materials.push(FemMaterial {
        name: "STEEL".into(),
        young_modulus: Some(210_000.0),
        poisson_ratio: Some(0.3),
        density: None,
    });
    setup.sections.push(Section {
        name: "Solid".into(),
        mesh_index: 0,
        material_name: "STEEL".into(),
        element_set_name: Some("STEP_SOLID".into()),
        kind: SectionKind::Solid,
    });
    setup.boundary_conditions.push(BoundaryCondition {
        name: "Fixed end".into(),
        mesh_index: 0,
        nodes: fixed.nodes.clone(),
        ngrp_name: Some(fixed.name.clone()),
        rotation_center: None,
        dof_start: 1,
        dof_end: 3,
        value: 0.0,
    });
    setup
        .nodal_loads
        .extend(loaded.nodes.iter().map(|&node| NodalLoad {
            name: "Axial force".into(),
            mesh_index: 0,
            node,
            ngrp_name: Some(loaded.name.clone()),
            rotation_center: None,
            dof: 1,
            value: NODAL_FORCE,
        }));

    for launch_mode in [SolverLaunchMode::Direct, SolverLaunchMode::Mpi] {
        let output = tempfile::tempdir().unwrap();
        let mut run = FrontistrRunState {
            launch_mode,
            mpi_ranks: 2,
            ..default()
        };
        assert!(
            run.prepare_inputs(model, &setup, "box", |_| Some(output.path().into()))
                .unwrap()
        );
        let control = output.path().join("hecmw_ctrl.dat");
        let references = hecmw::load_hecmw_ctrl(&control).unwrap();
        assert!(
            references.import_issues.is_empty(),
            "{:?}",
            references.import_issues
        );
        let parallel = launch_mode == SolverLaunchMode::Mpi;
        assert_eq!(references.distributed_mesh_path.is_some(), parallel);
        assert_eq!(
            output.path().join("hecmw_part_ctrl.dat").is_file(),
            parallel
        );
        let (mesh_path, cnt_path) = hecmw::resolve_paths(&control, &references);
        let project = hecmw::load_mesh_project(mesh_path.unwrap()).unwrap();
        assert_eq!(project.mesh.elements, mesh.elements);
        assert_eq!(project.mesh.node_sets, mesh.node_sets);
        assert_eq!(project.mesh.surface_sets, mesh.surface_sets);
        assert_eq!(project.mesh.element_sets, mesh.element_sets);
        assert_eq!(project.materials.len(), 1);
        assert_eq!(project.materials[0].name, "STEEL");
        assert_eq!(
            project.sections[0].element_set_name.as_deref(),
            Some("STEP_SOLID")
        );
        let cnt = hecmw::load_cnt_file(cnt_path.unwrap(), &project.mesh, 0).unwrap();
        assert!(cnt.import_issues.is_empty(), "{:?}", cnt.import_issues);
        assert_eq!(cnt.boundary_conditions[0].nodes, fixed.nodes);
        assert_eq!(
            cnt.nodal_loads
                .iter()
                .map(|load| load.node)
                .collect::<Vec<_>>(),
            loaded.nodes
        );
        assert!(cnt.nodal_loads.iter().all(|load| load.value == NODAL_FORCE));
    }

    app.world_mut()
        .resource_mut::<MeshLoadRequest>()
        .request_import(input.path().join("box.step"));
    wait_until(&mut app, 30, |world| {
        world.resource::<StepImport>().preview().is_some()
    });
    assert_eq!(app.world().resource::<FemModel>().parts.len(), 1);
    click(&mut app, accept);
    assert_eq!(app.world().resource::<FemModel>().parts.len(), 2);
}
