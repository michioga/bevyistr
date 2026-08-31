//! File-dialog and FrontISTR project loading/export systems.
//!
//! Keeping these systems outside `layout` prevents the sidebar tree builder
//! from also becoming the owner of mesh parsing, project transactions, and
//! export validation.

use bevy::prelude::*;
use camera::OrbitCamera;
use fem_core::{ContactType, FemModel, FemModelVersion, MeshLoadRequest, MeshLoadStatus};

const PANEL_BORDER: Color = Color::srgba(0.34, 0.40, 0.44, 0.72);
const BUTTON_NORMAL: Color = Color::srgba(0.10, 0.12, 0.14, 0.94);
const BUTTON_HOVERED: Color = Color::srgba(0.18, 0.22, 0.24, 0.96);
const BUTTON_PRESSED: Color = Color::srgb(0.22, 0.55, 0.66);

#[derive(Component)]
pub(crate) struct OpenMeshButton;

#[derive(Component)]
pub(crate) struct ImportMeshButton;

/// Opens `hecmw_ctrl.dat`, reads mesh/cnt stems, and loads both files in
/// one click — the "Open Project" shortcut.
#[derive(Component)]
pub(crate) struct OpenProjectButton;

#[derive(Component)]
pub(crate) struct OpenSetupButton;

#[derive(Component)]
pub(crate) struct ExportButton;

#[derive(Component)]
pub(crate) struct ExportStatusText;

/// Explicit camera-fit request, kept separate from [`FemModelVersion`] so
/// assembly edits can rebuild geometry without disrupting the current view.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub(crate) struct CameraFitRequest {
    pub(crate) revision: u64,
}

impl CameraFitRequest {
    fn request(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
}

pub(crate) fn open_mesh_button_system(
    mut request: ResMut<MeshLoadRequest>,
    mut status: ResMut<MeshLoadStatus>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<OpenMeshButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Open mesh")
                .add_filter("All supported meshes", &["msh", "geo", "inp"])
                .add_filter("HECMW / FrontISTR (.msh)", &["msh"])
                .add_filter("Gmsh geometry (.geo)", &["geo"])
                .add_filter("Abaqus / CalculiX (.inp)", &["inp"])
                .pick_file()
            {
                status.loading(path.clone());
                request.request(path);
            }
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Adds a mesh as a new part instead of replacing the current model.
pub(crate) fn import_mesh_button_system(
    mut request: ResMut<MeshLoadRequest>,
    mut status: ResMut<MeshLoadStatus>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<ImportMeshButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Import mesh as new part")
                .add_filter("All supported meshes", &["msh", "geo", "inp"])
                .add_filter("HECMW / FrontISTR (.msh)", &["msh"])
                .add_filter("Gmsh geometry (.geo)", &["geo"])
                .add_filter("Abaqus / CalculiX (.inp)", &["inp"])
                .pick_file()
            {
                status.loading(path.clone());
                request.request_import(path);
            }
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Loads `.geo`, `.inp`, HEC-MW `.msh`, or Gmsh `.msh` files.
pub(crate) fn mesh_load_system(
    mut model: ResMut<FemModel>,
    mut request: ResMut<MeshLoadRequest>,
    mut status: ResMut<MeshLoadStatus>,
    mut version: ResMut<FemModelVersion>,
    mut camera_fit: ResMut<CameraFitRequest>,
    mut setup: ResMut<fem_core::AnalysisSetup>,
) {
    let Some((path, import)) = request.take() else {
        return;
    };

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
        _ => match hecmw::load_mesh_file_with_setup_and_contacts(&path) {
            Ok((mesh, materials, sections, contact_pairs, mut equations)) => {
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
        *model = FemModel::single_mesh(name, mesh);
    }
    status.loaded(path.clone());
    version.bump();
    camera_fit.request();
}

/// Recenters and re-scales the orbit camera after a mesh file is loaded.
pub(crate) fn camera_refit_on_reload(
    model: Option<Res<FemModel>>,
    request: Res<CameraFitRequest>,
    mut last_version: Local<Option<u64>>,
    mut camera_query: Query<(&mut Transform, &mut OrbitCamera)>,
) {
    let current = request.revision;

    if *last_version == Some(current) {
        return;
    }

    let first_run = last_version.is_none();
    *last_version = Some(current);
    if first_run {
        return;
    }

    let Some((min, max)) = model.as_deref().and_then(FemModel::bounds) else {
        return;
    };
    let (focus, radius) = camera::fit_bounds(min, max);
    let (min_radius, max_radius) = camera::radius_limits(radius);

    let Ok((mut transform, mut orbit)) = camera_query.single_mut() else {
        return;
    };
    orbit.focus = focus;
    orbit.target_focus = focus;
    orbit.radius = radius;
    orbit.min_radius = min_radius;
    orbit.max_radius = max_radius;

    let camera_position = focus + Vec3::new(radius * 0.45, radius * 0.45, radius);
    *transform = Transform::from_translation(camera_position).looking_at(focus, Vec3::Y);
}

/// Opens `hecmw_ctrl.dat` and queues its mesh and control files together.
pub(crate) fn open_project_button_system(
    mut request: ResMut<MeshLoadRequest>,
    mut load_status: ResMut<MeshLoadStatus>,
    mut pending_cnt: ResMut<fem_core::PendingCntLoad>,
    version: Res<FemModelVersion>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<OpenProjectButton>,
    >,
) {
    for (interaction, mut bg, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let Some(ctrl_path) = rfd::FileDialog::new()
                .set_title("Open FrontISTR project (hecmw_ctrl.dat)")
                .add_filter("FrontISTR project", &["dat"])
                .add_filter("All files", &["*"])
                .pick_file()
            else {
                continue;
            };

            match hecmw::load_hecmw_ctrl(&ctrl_path) {
                Ok(content) => {
                    let (mesh_path, cnt_path) = hecmw::resolve_paths(&ctrl_path, &content);
                    if let Some(mesh_path) = mesh_path {
                        load_status.loading(mesh_path.clone());
                        request.request(mesh_path);
                    } else {
                        bevy::log::warn!("hecmw_ctrl.dat: mesh file not found");
                    }
                    if let Some(cnt_path) = cnt_path {
                        pending_cnt.request(cnt_path, 0, version.value);
                    }
                }
                Err(e) => bevy::log::warn!("Failed to parse hecmw_ctrl.dat: {e}"),
            }
        }

        let color = match *interaction {
            Interaction::Pressed => Color::srgb(0.12, 0.44, 0.22),
            Interaction::Hovered => Color::srgb(0.14, 0.52, 0.26),
            Interaction::None => Color::srgb(0.10, 0.30, 0.18),
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
            let applied_contacts = data.apply_contact_settings(&mut model.contacts);
            data.merge_into(&mut setup);
            setup.set_changed();
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
        Err(e) => bevy::log::warn!("Failed to parse cnt file {:?}: {e}", path),
    }
}

pub(crate) fn export_button_system(
    model: Option<Res<FemModel>>,
    status: Res<MeshLoadStatus>,
    setup: Res<fem_core::AnalysisSetup>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<ExportButton>,
    >,
    mut status_query: Query<&mut Text, With<ExportStatusText>>,
) {
    for (interaction, mut bg, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            let Some(model) = model.as_deref() else {
                set_export_status(&mut status_query, "Error: no mesh loaded");
                continue;
            };
            let validation = hecmw::validate_frontistr_project(model, &setup);
            if validation.has_errors() {
                set_export_status(&mut status_query, &validation.summary(5));
                continue;
            }

            let stem = status
                .last_path
                .as_deref()
                .and_then(|p| p.file_stem())
                .and_then(|s| s.to_str())
                .unwrap_or("mesh")
                .to_string();

            let Some(dir) = rfd::FileDialog::new()
                .set_title("Export FrontISTR files to folder")
                .pick_folder()
            else {
                continue;
            };

            match hecmw::write_frontistr_project(&dir, &stem, model, &setup) {
                Ok(summary) => {
                    let part_note = if summary.part_count > 1 {
                        format!("  ({} parts merged)", summary.part_count)
                    } else {
                        String::new()
                    };
                    let message = format!(
                        "OK {stem}.*{part_note}\n{}N/{}E  BC:{} Ld:{} MPC:{} Mat:{} Sec:{} Ctc:{}{}",
                        summary.node_count,
                        summary.element_count,
                        summary.boundary_condition_count,
                        summary.load_count,
                        summary.mpc_equation_count,
                        summary.material_count,
                        summary.section_count,
                        summary.contact_count,
                        if validation.warning_count() > 0 {
                            format!("\n{}", validation.summary(2))
                        } else {
                            String::new()
                        },
                    );
                    set_export_status(&mut status_query, &message);
                }
                Err(e) => set_export_status(&mut status_query, &format!("Error: {e}")),
            }
        }

        let color = match *interaction {
            Interaction::Pressed => Color::srgb(0.12, 0.44, 0.22),
            Interaction::Hovered => Color::srgb(0.14, 0.52, 0.26),
            Interaction::None => Color::srgb(0.10, 0.32, 0.18),
        };
        *bg = BackgroundColor(color);
        *border = BorderColor::all(Color::srgb(0.15, 0.50, 0.28));
    }
}

fn set_export_status(query: &mut Query<&mut Text, With<ExportStatusText>>, msg: &str) {
    if let Ok(mut text) = query.single_mut() {
        **text = msg.to_string();
    }
}

/// Loads a standalone FrontISTR `.cnt` into the current model.
pub(crate) fn open_setup_button_system(
    mut pending_path: Local<Option<std::path::PathBuf>>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<OpenSetupButton>,
    >,
    mut model: Option<ResMut<FemModel>>,
    mut setup: ResMut<fem_core::AnalysisSetup>,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Open analysis control file")
                .add_filter("FrontISTR control (.cnt)", &["cnt"])
                .pick_file()
            {
                *pending_path = Some(path);
            }
        }

        let color = match *interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVERED,
            Interaction::None => BUTTON_NORMAL,
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
                let applied_contacts = data.apply_contact_settings(&mut model.contacts);
                data.merge_into(&mut setup);
                setup.set_changed();
                bevy::log::info!(
                    "Loaded analysis setup from {:?}; updated {applied_contacts} contact pair(s)",
                    path.file_name()
                );
            }
            Err(err) => bevy::log::warn!("Failed to load .cnt file: {err}"),
        }
    }
}
