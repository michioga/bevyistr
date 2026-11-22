//! FrontISTR file controls, export status, and camera updates after loading.

mod import;
mod step_mesh;

pub use import::StartupInput;
pub(crate) use import::{
    OpenSetupButton, apply_pending_cnt_system, mesh_load_system, open_project_button_system,
    open_setup_button_system, queue_startup_input,
};
#[cfg(test)]
pub(crate) use import::{apply_mesh, merge_mesh_contact_pairs};
#[cfg(test)]
pub(crate) use step_mesh::SaveStepMeshControls;
pub(crate) use step_mesh::{StepMeshSettings, save_step_mesh_button_system};

use crate::{
    ButtonInteraction, InteractionButton, solver_runner::FrontistrRunState, step_import::StepWidget,
};
use bevy::prelude::*;
use camera::OrbitCamera;
use fem_core::{FemModel, FemModelVersion, MeshLoadRequest, MeshLoadStatus};
use import::OpenProjectButton;
use std::path::{Path, PathBuf};
use step_mesh::{is_step, spawn_step_mesh_ui};
use visualization::VisualizationSettings;

const PANEL_BORDER: Color = Color::srgba(0.34, 0.40, 0.44, 0.72);
const TEXT_MAIN: Color = Color::srgb(0.88, 0.92, 0.94);
const TEXT_MUTED: Color = Color::srgb(0.58, 0.66, 0.70);
const BUTTON_NORMAL: Color = Color::srgba(0.10, 0.12, 0.14, 0.94);
const BUTTON_HOVERED: Color = Color::srgba(0.18, 0.22, 0.24, 0.96);
const BUTTON_PRESSED: Color = Color::srgb(0.22, 0.55, 0.66);

#[derive(Component)]
pub(crate) struct OpenMeshButton;

#[derive(Component)]
pub(crate) struct AddMeshButton;

#[derive(Component)]
pub(crate) struct ExportButton;

#[derive(Component)]
pub(crate) struct ExportStatusText;

#[derive(Component)]
pub(crate) struct MeshStatsText;

pub(crate) fn spawn_model_file_ui(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(8.0),
            ..default()
        },))
        .with_children(|row| {
            row.spawn((
                InteractionButton,
                Node {
                    padding: UiRect::axes(px(14.0), px(5.0)),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(5.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(BUTTON_NORMAL),
                BorderColor::all(PANEL_BORDER),
                OpenMeshButton,
                Name::new("OpenMeshButton"),
            ))
            .with_child((
                Text::new("Open Model..."),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(TEXT_MAIN),
            ));

            row.spawn((
                InteractionButton,
                Node {
                    padding: UiRect::axes(px(14.0), px(5.0)),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(5.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(BUTTON_NORMAL),
                BorderColor::all(PANEL_BORDER),
                AddMeshButton,
                Name::new("AddMeshButton"),
            ))
            .with_child((
                Text::new("Add Part..."),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(TEXT_MAIN),
            ));
        });

    spawn_step_mesh_ui(parent);

    parent.spawn((
        Text::new("No mesh loaded"),
        TextFont {
            font_size: FontSize::Px(11.5),
            ..default()
        },
        TextColor(TEXT_MUTED),
        MeshStatsText,
        StepWidget::CommittedStats,
        Node::default(),
    ));

    parent
        .spawn((Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(8.0),
            ..default()
        },))
        .with_children(|row| {
            row.spawn((
                InteractionButton,
                Node {
                    flex_grow: 1.0,
                    padding: UiRect::axes(px(8.0), px(4.0)),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(5.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.10, 0.30, 0.18)),
                BorderColor::all(Color::srgb(0.15, 0.46, 0.26)),
                OpenProjectButton,
                Name::new("OpenProjectButton"),
            ))
            .with_child((
                Text::new("Open Project (hecmw_ctrl.dat)"),
                TextFont {
                    font_size: FontSize::Px(10.5),
                    ..default()
                },
                TextColor(Color::srgb(0.75, 0.97, 0.80)),
            ));
        });
    parent.spawn((
        Text::new(concat!(
            "Open Model replaces the current model.\n",
            "Add Part adds to the assembly.\n",
            "Both accept STEP and mesh files.\n",
            "STEP meshing requires Gmsh.\n",
            "Open Project loads .msh + .cnt.",
        )),
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(Color::srgba(0.45, 0.54, 0.60, 0.80)),
    ));
}

/// Explicit camera-fit request, kept separate from [`FemModelVersion`] so
/// assembly edits can rebuild geometry without disrupting the current view.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub(crate) struct CameraFitRequest {
    pub(crate) revision: u64,
}

impl CameraFitRequest {
    pub(crate) fn request(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
}

fn pick_mesh_file(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(title)
        .add_filter(
            "All supported meshes and CAD",
            &["msh", "geo", "inp", "step", "stp"],
        )
        .add_filter("STEP solid (.step, .stp)", &["step", "stp"])
        .add_filter("HECMW / FrontISTR (.msh)", &["msh"])
        .add_filter("Gmsh geometry (.geo)", &["geo"])
        .add_filter("Abaqus / CalculiX (.inp)", &["inp"])
        .pick_file()
}

pub(crate) fn open_mesh_button_system(
    mut request: ResMut<MeshLoadRequest>,
    mut status: ResMut<MeshLoadStatus>,
    mut run_state: ResMut<FrontistrRunState>,
    mut buttons: Query<
        (
            Ref<ButtonInteraction>,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<OpenMeshButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == ButtonInteraction::Pressed && interaction.is_changed() {
            if let Some(path) = pick_mesh_file("Open Model") {
                if !is_step(&path) {
                    run_state.clear_export_target();
                    status.loading(path.clone());
                }
                request.request(path);
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
}

/// Adds a mesh as a new part instead of replacing the current model.
pub(crate) fn add_mesh_button_system(
    mut request: ResMut<MeshLoadRequest>,
    mut status: ResMut<MeshLoadStatus>,
    mut buttons: Query<
        (
            Ref<ButtonInteraction>,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<AddMeshButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == ButtonInteraction::Pressed && interaction.is_changed() {
            if let Some(path) = pick_mesh_file("Add Part") {
                if !is_step(&path) {
                    status.loading(path.clone());
                }
                request.request_import(path);
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
}

/// Recenters and re-scales the orbit camera after a mesh file is loaded.
pub(crate) fn camera_refit_on_reload(
    model: Option<Res<FemModel>>,
    result_geometry: Option<Res<fem_core::ResultGeometry>>,
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

    let display_model = result_geometry
        .as_deref()
        .filter(|g| g.visible)
        .and_then(|g| g.model.as_ref())
        .or(model.as_deref());
    let Some((min, max)) = display_model.and_then(FemModel::bounds) else {
        return;
    };
    let Ok((mut transform, mut orbit)) = camera_query.single_mut() else {
        return;
    };
    fit_orbit_camera(&mut transform, &mut orbit, min, max);
}

pub(crate) fn fit_orbit_camera(
    transform: &mut Transform,
    orbit: &mut OrbitCamera,
    min: Vec3,
    max: Vec3,
) {
    let (focus, radius) = camera::fit_bounds(min, max);
    let (min_radius, max_radius) = camera::radius_limits(radius);
    orbit.focus = focus;
    orbit.target_focus = focus;
    orbit.radius = radius;
    orbit.min_radius = min_radius;
    orbit.max_radius = max_radius;

    let camera_position = focus + Vec3::new(radius * 0.45, radius * 0.45, radius);
    *transform = Transform::from_translation(camera_position).looking_at(focus, Vec3::Y);
}

pub(crate) fn export_button_system(
    model: Option<Res<FemModel>>,
    status: Res<MeshLoadStatus>,
    setup: Res<fem_core::AnalysisSetup>,
    mut run_state: ResMut<FrontistrRunState>,
    mut buttons: Query<
        (
            Ref<ButtonInteraction>,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<ExportButton>,
    >,
    mut status_query: Query<&mut Text, With<ExportStatusText>>,
) {
    for (interaction, mut bg, mut border) in &mut buttons {
        let enabled = !run_state.is_running();
        if enabled && *interaction == ButtonInteraction::Pressed && interaction.is_changed() {
            let Some(model) = model.as_deref() else {
                set_export_status(&mut status_query, "Error: no mesh loaded");
                continue;
            };
            let validation = hecmw::validate_frontistr_project(model, &setup);
            if validation.has_errors() {
                set_export_status(&mut status_query, &validation.summary(5));
                continue;
            }

            let stem = crate::run_output::project_stem(status.last_path.as_deref());
            let Some(dir) = crate::run_output::choose_output_directory(
                &stem,
                run_state.last_output_directory(),
            ) else {
                continue;
            };

            match hecmw::write_frontistr_project(&dir, &stem, model, &setup) {
                Ok(summary) => {
                    run_state.register_export(dir.clone(), stem.clone());
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

        let color = match (*interaction, enabled) {
            (_, false) => Color::srgba(0.08, 0.09, 0.10, 0.72),
            (ButtonInteraction::Pressed, true) => Color::srgb(0.12, 0.44, 0.22),
            (ButtonInteraction::Hovered, true) => Color::srgb(0.14, 0.52, 0.26),
            (ButtonInteraction::None, true) => Color::srgb(0.10, 0.32, 0.18),
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

pub(crate) fn update_mesh_stats_text(
    model: Option<Res<FemModel>>,
    version: Res<FemModelVersion>,
    status: Res<MeshLoadStatus>,
    settings: Res<VisualizationSettings>,
    mut last_version: Local<Option<u64>>,
    mut query: Query<&mut Text, With<MeshStatsText>>,
) {
    let model_changed = model.as_ref().is_some_and(|model| model.is_changed());
    if *last_version == Some(version.value)
        && !model_changed
        && !status.is_changed()
        && !settings.is_changed()
    {
        return;
    }
    *last_version = Some(version.value);

    let Ok(mut text) = query.single_mut() else {
        return;
    };

    let Some(model) = model else {
        **text = "Mesh: demo pending".to_string();
        return;
    };

    let mesh_count = model.meshes.len();
    let node_count: usize = model.meshes.iter().map(|mesh| mesh.nodes.len()).sum();
    let element_count: usize = model.meshes.iter().map(|mesh| mesh.elements.len()).sum();
    let edge_count: usize = model
        .meshes
        .iter()
        .map(|mesh| mesh.cached_edges().len())
        .sum();
    let face_count: usize = model
        .meshes
        .iter()
        .map(|mesh| mesh.cached_boundary_faces().len())
        .sum();
    let boundary_edge_count: usize = model
        .meshes
        .iter()
        .map(|mesh| mesh.cached_boundary_edges().len())
        .sum();
    let node_set_count: usize = model.meshes.iter().map(|mesh| mesh.node_sets.len()).sum();
    let element_set_count: usize = model
        .meshes
        .iter()
        .map(|mesh| mesh.element_sets.len())
        .sum();
    let surface_set_count: usize = model
        .meshes
        .iter()
        .map(|mesh| mesh.surface_sets.len())
        .sum();
    let contact_count = model.contacts.len();
    let load_status = load_status_line(&status);

    **text = format!(
        "{load_status}\nMeshes: {mesh_count}  Nodes: {node_count}  Elements: {element_count}\nEdges: {edge_count}  Boundary edges: {boundary_edge_count}\nBoundary faces: {face_count}  Render: {}\nSets: N {node_set_count}  E {element_set_count}  S {surface_set_count}  Contacts: {contact_count}",
        settings.mode.label()
    );
}

fn load_status_line(status: &MeshLoadStatus) -> String {
    let path = status
        .last_path
        .as_deref()
        .map(compact_path)
        .unwrap_or_else(|| "No file".to_string());

    if let Some(error) = &status.error {
        format!("File: {path}  Error: {error}")
    } else if status.message.is_empty() {
        format!("File: {path}")
    } else {
        format!("File: {path}  {}", status.message)
    }
}

fn compact_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.display().to_string())
}
