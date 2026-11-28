use super::*;
use crate::{
    contact_ui::{
        CaptureContactSideButton, ContactCaptureSide, ContactDefinitionSettings,
        FinalizeContactButton, capture_contact_side_button_system, finalize_contact_button_system,
    },
    project_io::StepMeshSettings,
    selection_ui::{rebuild_sets_list, set_button_system, spawn_contact_sets_ui},
    step_import::{StepImportAction, step_action_buttons},
};
use fem_core::{
    AnalysisType, ContactCandidateState, ContactSlaveRef, FemNodeSet, SelectionFilter,
    SelectionLevel,
};
use selection::SelectionState;
use visualization::{ContactDraftPreview, DefinedContactPreview, MpcPairDraftPreview};

const BOX_LENGTH: f32 = 20.0;
const END_DISPLACEMENT: f32 = -0.04;

#[test]
#[ignore = "Requires Gmsh on PATH and installed FrontISTR"]
fn step_contact_groups_run_through_frontistr_and_open_results() {
    let input = tempfile::tempdir().unwrap();
    write_step_box(input.path());
    let output = tempfile::tempdir().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        check_step_contact_workflow(&input, &output);
    }));
    if let Err(panic) = result {
        eprintln!(
            "STEP contact inputs and logs retained at {}",
            output.keep().display()
        );
        std::panic::resume_unwind(panic);
    }
}

fn check_step_contact_workflow(input: &tempfile::TempDir, output: &tempfile::TempDir) {
    bevy::tasks::IoTaskPool::get_or_init(bevy::tasks::TaskPool::new);
    let first = input.path().join("box.step");
    let second = input.path().join("mate.step");
    std::fs::copy(&first, &second).unwrap();
    let mut app = App::new();
    app.init_resource::<MeshLoadRequest>()
        .init_resource::<PendingCntLoad>()
        .init_resource::<MeshLoadStatus>()
        .init_resource::<FemModel>()
        .init_resource::<AnalysisSetup>()
        .init_resource::<StepImport>()
        .init_resource::<StepMeshSettings>()
        .init_resource::<FemModelVersion>()
        .init_resource::<CameraFitRequest>()
        .init_resource::<SidebarPage>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<SelectionFilter>()
        .init_resource::<SelectionState>()
        .init_resource::<ContactDefinitionSettings>()
        .init_resource::<ContactCandidateState>()
        .init_resource::<ContactDraftPreview>()
        .init_resource::<MpcPairDraftPreview>()
        .init_resource::<DefinedContactPreview>()
        .init_resource::<FemResultSet>()
        .init_resource::<ResultGeometry>()
        .init_resource::<ResultRequests>()
        .init_resource::<SolveResultsState>()
        .init_resource::<VisualizationSettings>()
        .init_resource::<PlaybackState>()
        .insert_resource(installed_run(SolverLaunchMode::Direct))
        .add_systems(Startup, |mut commands: Commands| {
            commands
                .spawn(Node::default())
                .with_children(spawn_contact_sets_ui);
        })
        .add_systems(
            Update,
            (
                step_action_buttons,
                mesh_load_system,
                rebuild_sets_list,
                set_button_system,
                capture_contact_side_button_system,
                finalize_contact_button_system,
                run_frontistr_button_system,
                poll_frontistr_process_system,
                open_run_results_system,
                poll_run_results_system,
            )
                .chain(),
        );
    app.world_mut()
        .resource_mut::<StepMeshSettings>()
        .options
        .max_size_mm = Some(4.0);

    let accept = button(&mut app, StepImportAction::Accept);
    let regenerate = button(&mut app, StepImportAction::Regenerate);
    let capture_slave = button(
        &mut app,
        CaptureContactSideButton(ContactCaptureSide::Slave),
    );
    let capture_master = button(
        &mut app,
        CaptureContactSideButton(ContactCaptureSide::Master),
    );
    let finalize = button(&mut app, FinalizeContactButton);
    let run_button = button(&mut app, RunFrontistrButton);

    app.world_mut()
        .resource_mut::<MeshLoadRequest>()
        .request(first);
    wait_for_step_preview(&mut app);
    click(&mut app, accept);
    app.world_mut()
        .resource_mut::<MeshLoadRequest>()
        .request_import(second);
    wait_for_step_preview(&mut app);
    app.world_mut()
        .resource_mut::<StepImport>()
        .toggle_contact_face(Vec3::new(-1.0, 5.0, 3.0), Vec3::X);
    click(&mut app, regenerate);
    wait_for_step_preview(&mut app);
    click(&mut app, accept);
    assert!(
        app.world_mut()
            .resource_mut::<FemModel>()
            .translate_part(1, Vec3::X * BOX_LENGTH)
    );
    *app.world_mut().resource_mut::<SidebarPage>() = SidebarPage::Contact;
    app.update();

    let model = app.world().resource::<FemModel>();
    let master_mesh = &model.meshes[0];
    let slave_mesh = &model.meshes[1];
    let master_group = face_nodes_at(master_mesh, BOX_LENGTH);
    let slave_group = face_nodes_at(slave_mesh, BOX_LENGTH);
    let master_faces = master_mesh
        .surface_sets
        .iter()
        .find(|group| group.name == master_group.name)
        .unwrap();
    let slave_faces = slave_mesh
        .surface_sets
        .iter()
        .find(|group| group.name == slave_group.name)
        .unwrap();
    assert!(slave_faces.surfaces.len() > master_faces.surfaces.len() * 2);
    let slave_label = format!(
        "[2] [N] {}  ({} nodes)",
        slave_group.name,
        slave_group.nodes.len()
    );
    let master_label = format!(
        "[1] [S] {}  ({} faces)",
        master_group.name,
        master_faces.surfaces.len()
    );
    let (node_count, face_count) = (slave_group.nodes.len(), master_faces.surfaces.len());
    let setup = compression_setup(model);

    let slave_row = set_row(&mut app, &slave_label);
    let master_row = set_row(&mut app, &master_label);
    click(&mut app, slave_row);
    assert_eq!(
        app.world().resource::<SelectionFilter>().level,
        SelectionLevel::Node
    );
    assert_eq!(app.world().resource::<SelectionState>().len(), node_count);
    click(&mut app, capture_slave);
    click(&mut app, master_row);
    assert_eq!(
        app.world().resource::<SelectionFilter>().level,
        SelectionLevel::Face
    );
    assert_eq!(app.world().resource::<SelectionState>().len(), face_count);
    click(&mut app, capture_master);
    click(&mut app, finalize);
    let model = app.world().resource::<FemModel>();
    let [pair] = model.contacts.as_slice() else {
        panic!("{:?}", app.world().resource::<ContactDefinitionSettings>());
    };
    assert_eq!(pair.master.mesh_index, 0);
    assert!(matches!(pair.slave, ContactSlaveRef::Nodes(nodes) if nodes.mesh_index == 1));
    let pair_name = pair.name.clone();

    app.insert_resource(setup);
    app.world_mut()
        .resource_mut::<FrontistrRunState>()
        .register_export(output.path().into(), "blocks".into());
    *app.world_mut().resource_mut::<SidebarPage>() = SidebarPage::Solve;
    click(&mut app, run_button);
    wait_until(&mut app, 60, |world| {
        let run = world.resource::<FrontistrRunState>();
        let status = &world.resource::<SolveResultsState>().status;
        assert!(
            run.is_running()
                || (run.completed_results().is_some() && !status.starts_with("Could not")),
            "{}\n{status}\n{}",
            run.status_label(),
            run.log_label(),
        );
        *world.resource::<SidebarPage>() == SidebarPage::Results
    });

    let world = app.world();
    let model = world.resource::<FemModel>();
    let results = world.resource::<FemResultSet>();
    assert_eq!(results.by_mesh.len(), 2);
    let mut max_difference = 0.0_f32;
    for (mesh, frames) in model.meshes.iter().zip(&results.by_mesh) {
        let last = frames.last().unwrap();
        assert_eq!(last.step, 4);
        let Some(ResultField::NodeVector { values, .. }) = last.field_by_name("Displacement")
        else {
            panic!("final displacement missing");
        };
        assert_eq!(values.len(), mesh.nodes.len());
        for (node, value) in mesh.nodes.iter().zip(values) {
            let expected = END_DISPLACEMENT * node.position.x / (2.0 * BOX_LENGTH);
            assert!(value.is_finite());
            max_difference = max_difference.max((*value - Vec3::X * expected).length());
        }
    }
    assert!(
        max_difference < 2e-4,
        "max displacement error {max_difference:e}"
    );

    let control = output.path().join("hecmw_ctrl.dat");
    let file_names = hecmw::load_hecmw_ctrl(&control).unwrap();
    assert!(
        file_names.import_issues.is_empty(),
        "{:?}",
        file_names.import_issues
    );
    let (mesh_path, cnt_path) = hecmw::resolve_paths(&control, &file_names);
    let imported = hecmw::load_mesh_project(mesh_path.unwrap()).unwrap();
    let cnt = hecmw::load_cnt_file(cnt_path.unwrap(), &imported.mesh, 0).unwrap();
    assert!(
        imported.import_issues.is_empty(),
        "{:?}",
        imported.import_issues
    );
    assert!(cnt.import_issues.is_empty(), "{:?}", cnt.import_issues);
    assert_eq!(imported.contacts.len(), 1);
    assert_eq!(imported.contacts[0].name, pair_name);
    assert_eq!(
        imported.contacts[0].pair_type,
        hecmw::HecmwContactPairType::NodeSurface
    );
    assert_eq!(cnt.contact_settings[0].pair_name, pair_name);
    println!(
        "STEP contact: {node_count} slave nodes, {face_count} master faces, max displacement error {max_difference:e}"
    );
}

fn button(app: &mut App, component: impl Component) -> Entity {
    app.world_mut()
        .spawn((
            component,
            ButtonInteraction::None,
            BackgroundColor::default(),
            BorderColor::default(),
        ))
        .id()
}

fn wait_for_step_preview(app: &mut App) {
    wait_until(app, 30, |world| {
        world.resource::<StepImport>().preview().is_some()
    });
}

fn set_row(app: &mut App, text: &str) -> Entity {
    app.world_mut()
        .query::<(Entity, &Name)>()
        .iter(app.world())
        .find_map(|(entity, name)| {
            (name.as_str().strip_prefix("SetButton_") == Some(text)).then_some(entity)
        })
        .unwrap_or_else(|| panic!("missing {text}"))
}

fn face_nodes_at(mesh: &FemMesh, x: f32) -> &FemNodeSet {
    mesh.node_sets
        .iter()
        .find(|group| {
            !group.nodes.is_empty()
                && group
                    .nodes
                    .iter()
                    .all(|node| (mesh.node_position(*node).unwrap().x - x).abs() < 1e-5)
        })
        .unwrap()
}

fn compression_setup(model: &FemModel) -> AnalysisSetup {
    let mut setup = AnalysisSetup::default();
    setup.solver.analysis_type = AnalysisType::NlStatic;
    setup.solver.substeps = 4;
    setup.output.write_vis.enabled = false;
    setup.add_material("BLOCK", Some(1000.0), Some(0.0), None);
    for (index, mesh) in model.meshes.iter().enumerate() {
        setup.add_section(
            index,
            "BLOCK",
            Some("STEP_SOLID".into()),
            SectionKind::Solid,
        );
        let end = face_nodes_at(mesh, if index == 0 { 0.0 } else { 2.0 * BOX_LENGTH });
        setup.boundary_conditions.extend([
            BoundaryCondition {
                group_id: 1,
                name: "End displacement".into(),
                mesh_index: index,
                nodes: end.nodes.clone(),
                ngrp_name: Some(end.name.clone()),
                rotation_center: None,
                dof_start: 1,
                dof_end: 1,
                value: if index == 0 { 0.0 } else { END_DISPLACEMENT },
            },
            BoundaryCondition {
                group_id: 1,
                name: "Constrain transverse motion".into(),
                mesh_index: index,
                nodes: mesh.nodes.iter().map(|node| node.id).collect(),
                ngrp_name: None,
                rotation_center: None,
                dof_start: 2,
                dof_end: 3,
                value: 0.0,
            },
        ]);
    }
    setup
}
