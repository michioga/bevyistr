//! Project input shared by the command line and the file dialogs.

use super::{
    BUTTON_HOVERED, BUTTON_NORMAL, BUTTON_PRESSED, CameraFitRequest, PANEL_BORDER,
    step_mesh::{StepMeshSettings, is_step},
};
use crate::{
    ButtonInteraction,
    solver_runner::FrontistrRunState,
    step_import::{StepImport, StepSource},
};
use bevy::prelude::*;
use fem_core::{
    AnalysisSetup, ContactType, FemModel, FemModelVersion, MeshLoadRequest, MeshLoadStatus,
};
use std::path::{Path, PathBuf};

/// Opens `hecmw_ctrl.dat`, reads mesh/cnt stems, and loads both files in
/// one click — the "Open Project" shortcut.
#[derive(Component)]
pub(crate) struct OpenProjectButton;

#[derive(Component)]
pub(crate) struct OpenSetupButton;

/// Initial input requested by the application's command line.
#[derive(Resource)]
pub struct StartupInput(pub PathBuf);

pub(crate) fn queue_startup_input(
    input: Option<Res<StartupInput>>,
    mut request: ResMut<MeshLoadRequest>,
    mut load_status: ResMut<MeshLoadStatus>,
    mut pending_cnt: ResMut<fem_core::PendingCntLoad>,
    version: Res<FemModelVersion>,
    mut run_state: ResMut<FrontistrRunState>,
) {
    let Some(input) = input else { return };
    let path = input.0.clone();
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("dat"))
    {
        queue_project_input(
            path,
            &mut request,
            &mut load_status,
            &mut pending_cnt,
            version.value,
            &mut run_state,
        );
    } else {
        request.request(path);
    }
}

fn queue_project_input(
    ctrl_path: PathBuf,
    request: &mut MeshLoadRequest,
    load_status: &mut MeshLoadStatus,
    pending_cnt: &mut fem_core::PendingCntLoad,
    version: u64,
    run_state: &mut FrontistrRunState,
) {
    let mut content = match hecmw::load_hecmw_ctrl(&ctrl_path) {
        Ok(content) => content,
        Err(error) => {
            load_status.failed(
                ctrl_path,
                format!("Could not read project control: {error}"),
            );
            return;
        }
    };
    let (Some(mesh_path), Some(cnt_path)) = hecmw::resolve_paths(&ctrl_path, &content) else {
        load_status.failed(
            ctrl_path,
            "Project mesh or analysis control file was not found",
        );
        return;
    };
    let partition = if content.has_partition_input {
        let path = ctrl_path.with_file_name("hecmw_part_ctrl.dat");
        match hecmw::partition_control::load(&path) {
            Ok(partition) => {
                run_state.use_imported_ranks(partition.domains.get());
                Some(partition)
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && content.distributed_mesh_path.is_none() =>
            {
                None
            }
            Err(error) => {
                content.import_issues.push(fem_core::SetupImportIssue {
                    source: path.display().to_string(),
                    line: 1,
                    keyword: "!PARTITION".into(),
                    message: error.to_string(),
                });
                None
            }
        }
    } else {
        None
    };
    run_state.clear_export_target();
    load_status.loading(mesh_path.clone());
    request.request(mesh_path);
    pending_cnt.request(cnt_path, 0, version);
    pending_cnt.project_control = Some(fem_core::FrontistrControlImport {
        control: content.settings,
        partition,
        issues: content.import_issues,
    });
}

/// Opens `hecmw_ctrl.dat` and queues its mesh and control files together.
pub(crate) fn open_project_button_system(
    mut request: ResMut<MeshLoadRequest>,
    mut load_status: ResMut<MeshLoadStatus>,
    mut pending_cnt: ResMut<fem_core::PendingCntLoad>,
    version: Res<FemModelVersion>,
    mut run_state: ResMut<FrontistrRunState>,
    mut buttons: Query<
        (
            Ref<ButtonInteraction>,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<OpenProjectButton>,
    >,
) {
    for (interaction, mut bg, mut border) in &mut buttons {
        if *interaction == ButtonInteraction::Pressed && interaction.is_changed() {
            let Some(ctrl_path) = rfd::FileDialog::new()
                .set_title("Open FrontISTR project (hecmw_ctrl.dat)")
                .add_filter("FrontISTR project", &["dat"])
                .add_filter("All files", &["*"])
                .pick_file()
            else {
                continue;
            };

            queue_project_input(
                ctrl_path,
                &mut request,
                &mut load_status,
                &mut pending_cnt,
                version.value,
                &mut run_state,
            );
        }

        let color = match *interaction {
            ButtonInteraction::Pressed => Color::srgb(0.12, 0.44, 0.22),
            ButtonInteraction::Hovered => Color::srgb(0.14, 0.52, 0.26),
            ButtonInteraction::None => Color::srgb(0.10, 0.30, 0.18),
        };
        *bg = BackgroundColor(color);
        *border = BorderColor::all(Color::srgb(0.15, 0.46, 0.26));
    }
}

/// Applies the `.cnt` queued with an Open Project request after mesh load.
pub(crate) fn apply_pending_cnt_system(
    mut model: Option<ResMut<FemModel>>,
    version: Res<FemModelVersion>,
    mut pending_cnt: ResMut<fem_core::PendingCntLoad>,
    mut setup: ResMut<fem_core::AnalysisSetup>,
) {
    if pending_cnt.path.is_none() {
        return;
    }

    let Some((path, mesh_index)) = pending_cnt.take_if_ready(version.value) else {
        return;
    };
    let Some(model) = model.as_deref_mut() else {
        return;
    };
    let Some(mesh) = model.meshes.get(mesh_index) else {
        return;
    };

    if let Some(import) = pending_cnt.project_control.take() {
        setup.frontistr_control = import.control;
        setup.frontistr_partition = import.partition;
        setup.import_issues.extend(import.issues);
    }
    match hecmw::load_cnt_file(&path, mesh, mesh_index) {
        Ok(data) => {
            let counts = (
                data.boundary_conditions.len(),
                data.boundary_conditions
                    .iter()
                    .map(|condition| condition.nodes.len())
                    .sum::<usize>(),
                data.nodal_loads.len(),
                data.distributed_loads.len(),
                data.materials.len(),
                data.sections.len(),
                data.contact_settings.len(),
            );
            let applied_contacts = merge_analysis_control(&path, data, model, &mut setup, true);
            bevy::log::info!(
                "Loaded analysis setup from {:?}: {} BCs / {} constrained nodes, {} nodal loads, {} distributed loads, {} materials, {} sections, {applied_contacts}/{} contacts",
                path.file_name(),
                counts.0,
                counts.1,
                counts.2,
                counts.3,
                counts.4,
                counts.5,
                counts.6,
            );
        }
        Err(error) => {
            let issue = fem_core::SetupImportIssue {
                source: path.display().to_string(),
                line: 1,
                keyword: "CNT".into(),
                message: format!("Cannot load the project's analysis control file: {error}"),
            };
            bevy::log::warn!("Setup import: {issue}; Export/Run blocked");
            setup.import_issues.push(issue);
        }
    }
}

/// Loads a standalone FrontISTR `.cnt` into the current model.
pub(crate) fn open_setup_button_system(
    mut pending_path: Local<Option<std::path::PathBuf>>,
    mut buttons: Query<
        (
            Ref<ButtonInteraction>,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<OpenSetupButton>,
    >,
    mut model: Option<ResMut<FemModel>>,
    mut setup: ResMut<fem_core::AnalysisSetup>,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == ButtonInteraction::Pressed && interaction.is_changed() {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Open analysis control file")
                .add_filter("FrontISTR control (.cnt)", &["cnt"])
                .pick_file()
            {
                *pending_path = Some(path);
            }
        }

        let color = match *interaction {
            ButtonInteraction::Pressed => BUTTON_PRESSED,
            ButtonInteraction::Hovered => BUTTON_HOVERED,
            ButtonInteraction::None => BUTTON_NORMAL,
        };
        *background = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }

    if let Some(path) = pending_path.take() {
        let Some(model) = model.as_deref_mut() else {
            return;
        };
        let Some(mesh) = model.meshes.first() else {
            return;
        };

        match hecmw::load_cnt_file(&path, mesh, 0) {
            Ok(data) => {
                let applied_contacts =
                    merge_analysis_control(&path, data, model, &mut setup, false);
                bevy::log::info!(
                    "Loaded analysis setup from {:?}; updated {applied_contacts} contact pair(s)",
                    path.file_name()
                );
            }
            Err(err) => bevy::log::warn!("Failed to load .cnt file: {err}"),
        }
    }
}

fn merge_analysis_control(
    path: &Path,
    mut data: hecmw::CntData,
    model: &mut FemModel,
    setup: &mut AnalysisSetup,
    require_all_contacts: bool,
) -> usize {
    data.check_contact_references(&model.contacts, path, require_all_contacts);
    for issue in &data.import_issues {
        bevy::log::warn!("Setup import: {issue}; Export/Run blocked (viewing is available)");
    }
    let applied_contacts = data.apply_contact_settings(&mut model.contacts);
    data.merge_into(setup);
    applied_contacts
}

/// Loads mesh files and runs STEP meshing off the UI thread.
pub(crate) fn mesh_load_system(
    mut model: ResMut<FemModel>,
    mut request: ResMut<MeshLoadRequest>,
    mut status: ResMut<MeshLoadStatus>,
    mut version: ResMut<FemModelVersion>,
    mut camera_fit: ResMut<CameraFitRequest>,
    mut setup: ResMut<fem_core::AnalysisSetup>,
    step_settings: Option<Res<StepMeshSettings>>,
    mut run_state: Option<ResMut<FrontistrRunState>>,
    mut step: ResMut<StepImport>,
    pending_cnt: Option<ResMut<fem_core::PendingCntLoad>>,
) {
    let options = step_settings
        .as_deref()
        .map(|settings| settings.options)
        .unwrap_or_default();

    let Some((path, import)) = request.take() else {
        if step.has_action()
            && let Some((source, mesh)) = step.apply_action(options)
        {
            let size = mesh.bounds().map(|(min, max)| max - min);
            if !source.add_part
                && let Some(run) = run_state.as_deref_mut()
            {
                run.clear_export_target();
            }
            apply_mesh(
                mesh,
                &source.path,
                source.add_part,
                &mut model,
                &mut status,
                &mut version,
                &mut camera_fit,
                &mut setup,
            );
            if let Some(size) = size {
                status.message = format!(
                    "STEP meshed | {:.6} x {:.6} x {:.6} mm (X x Y x Z)",
                    size.x, size.y, size.z
                );
            }
        }
        if step.is_meshing() {
            step.poll();
        }
        return;
    };

    // A newer load owns the viewport. A previously queued STEP result cannot
    // become the preview or change the new model once its task is dropped.
    step.clear();
    if is_step(&path) {
        if let Some(mut pending_cnt) = pending_cnt {
            // A control file from a superseded project must not attach to the accepted STEP.
            *pending_cnt = default();
        }
        step.start(
            StepSource {
                path,
                add_part: import,
            },
            options,
        );
        return;
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "geo" => match gmsh::run_gmsh(&path, None) {
            Ok(mesh) => apply_mesh(
                mesh,
                &path,
                import,
                &mut model,
                &mut status,
                &mut version,
                &mut camera_fit,
                &mut setup,
            ),
            Err(e) => status.failed(path, e.to_string()),
        },
        "inp" => match hecmw::load_inp_file(&path) {
            Ok(mesh) => apply_mesh(
                mesh,
                &path,
                import,
                &mut model,
                &mut status,
                &mut version,
                &mut camera_fit,
                &mut setup,
            ),
            Err(e) => status.failed(path, e.to_string()),
        },
        _ => match hecmw::load_mesh_project(&path) {
            Ok(hecmw::HecmwMeshProject {
                mesh,
                materials,
                sections,
                contacts: contact_pairs,
                mut equations,
                import_issues,
            }) => {
                let mesh_index = if import { model.meshes.len() } else { 0 };
                apply_mesh(
                    mesh,
                    &path,
                    import,
                    &mut model,
                    &mut status,
                    &mut version,
                    &mut camera_fit,
                    &mut setup,
                );
                let loaded_contacts =
                    merge_mesh_contact_pairs(&mut model, mesh_index, contact_pairs);
                if loaded_contacts > 0 {
                    bevy::log::info!(
                        "Loaded {loaded_contacts} contact pair(s) from {:?}",
                        path.file_name()
                    );
                }

                let mut changed = false;
                for material in materials {
                    if setup.material_by_name(&material.name).is_none() {
                        setup.materials.push(material);
                        changed = true;
                    }
                }
                for section in sections {
                    setup.sections.push(section);
                    changed = true;
                }
                for equation in &mut equations {
                    for term in &mut equation.terms {
                        term.mesh_index = mesh_index;
                    }
                }
                if !equations.is_empty() {
                    setup.mpc_equations.extend(equations);
                    changed = true;
                }
                if !import_issues.is_empty() {
                    setup.import_issues.extend(import_issues);
                    changed = true;
                }
                if changed {
                    setup.set_changed();
                }
            }
            Err(_) => match gmsh::load_msh_file(&path) {
                Ok(mesh) => apply_mesh(
                    mesh,
                    &path,
                    import,
                    &mut model,
                    &mut status,
                    &mut version,
                    &mut camera_fit,
                    &mut setup,
                ),
                Err(e) => status.failed(path, e.to_string()),
            },
        },
    }
}

pub(crate) fn merge_mesh_contact_pairs(
    model: &mut FemModel,
    mesh_index: usize,
    definitions: Vec<hecmw::HecmwContactPairDefinition>,
) -> usize {
    let Some(mesh) = model.meshes.get(mesh_index) else {
        return 0;
    };
    let mut resolved = Vec::new();

    for definition in definitions {
        let master = mesh.surface_sets.iter().position(|set| {
            set.name
                .eq_ignore_ascii_case(&definition.master_surface_name)
        });
        let Some(master) = master else {
            bevy::log::warn!(
                "Contact pair '{}' refers to missing master surface '{}'",
                definition.name,
                definition.master_surface_name
            );
            continue;
        };

        let master = fem_core::SurfaceSetRef::new(mesh_index, master);
        let contact =
            match definition.pair_type {
                hecmw::HecmwContactPairType::NodeSurface => {
                    let Some(slave) = mesh.node_sets.iter().position(|set| {
                        set.name.eq_ignore_ascii_case(&definition.slave_group_name)
                    }) else {
                        bevy::log::warn!(
                            "Contact pair '{}' refers to missing slave node group '{}'",
                            definition.name,
                            definition.slave_group_name
                        );
                        continue;
                    };
                    fem_core::ContactPair::new_node_surface(
                        definition.name,
                        master,
                        fem_core::NodeSetRef::new(mesh_index, slave),
                        ContactType::SmallSliding,
                    )
                }
                hecmw::HecmwContactPairType::SurfaceSurface => {
                    let Some(slave) = mesh.surface_sets.iter().position(|set| {
                        set.name.eq_ignore_ascii_case(&definition.slave_group_name)
                    }) else {
                        bevy::log::warn!(
                            "Contact pair '{}' refers to missing slave surface '{}'",
                            definition.name,
                            definition.slave_group_name
                        );
                        continue;
                    };
                    fem_core::ContactPair::new(
                        definition.name,
                        master,
                        fem_core::SurfaceSetRef::new(mesh_index, slave),
                        ContactType::SmallSliding,
                    )
                }
            };
        resolved.push(contact);
    }

    let count = resolved.len();
    model.contacts.extend(resolved);
    count
}

pub(crate) fn apply_mesh(
    mesh: fem_core::FemMesh,
    path: &std::path::PathBuf,
    import: bool,
    model: &mut FemModel,
    status: &mut MeshLoadStatus,
    version: &mut FemModelVersion,
    camera_fit: &mut CameraFitRequest,
    setup: &mut fem_core::AnalysisSetup,
) {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("mesh")
        .to_string();
    if import {
        model.add_mesh(name, mesh);
    } else {
        setup.clear();
        // Output controls (including imported GROUP references) are project-local.
        setup.output = fem_core::OutputSettings::default();
        *model = FemModel::single_mesh(name, mesh);
    }
    status.loaded(path.clone());
    version.bump();
    camera_fit.request();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_project_keeps_global_control_and_requires_parallel_input_settings() {
        let directory = tempfile::tempdir().unwrap();
        let ctrl = directory.path().join("parallel.dat");
        std::fs::write(directory.path().join("mesh.msh"), "").unwrap();
        std::fs::write(directory.path().join("solver.cnt"), "").unwrap();
        std::fs::write(
            &ctrl,
            "!MESH,NAME=part_in,TYPE=HECMW-ENTIRE\nmesh.msh\n\
             !MESH,NAME=fstrMSH,TYPE=HECMW-DIST\nmesh_dist\n\
             !CONTROL,NAME=fstrCNT\nsolver.cnt\n\
             !RESULT,NAME=fstrRES,IO=OUT,TYPE=BINARY\nsolver.res\n",
        )
        .unwrap();
        let mut app = App::new();
        app.insert_resource(StartupInput(ctrl))
            .init_resource::<MeshLoadRequest>()
            .init_resource::<MeshLoadStatus>()
            .init_resource::<fem_core::PendingCntLoad>()
            .init_resource::<FemModelVersion>()
            .init_resource::<FrontistrRunState>()
            .add_systems(Startup, queue_startup_input);
        app.update();
        let request = app.world().resource::<MeshLoadRequest>();
        assert_eq!(request.path, Some(directory.path().join("mesh.msh")));
        let pending = app.world().resource::<fem_core::PendingCntLoad>();
        let import = pending.project_control.as_ref().unwrap();
        assert_eq!(import.control.binary_result, Some(true));
        assert_eq!(import.issues.len(), 1);
        assert!(import.issues[0].source.ends_with("hecmw_part_ctrl.dat"));
    }
}
