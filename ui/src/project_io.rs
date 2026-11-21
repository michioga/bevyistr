//! File-dialog and FrontISTR project loading/export systems.
//!
//! Keeping these systems outside `layout` prevents the sidebar tree builder
//! from also becoming the owner of mesh parsing, project transactions, and
//! export validation.

use crate::{ButtonInteraction, InteractionButton};
use bevy::{
    feathers::controls::{
        FeathersNumberInput, HardLimit, NumberInputPrecision, NumberInputRange, NumberInputStep,
        NumberInputValue,
    },
    prelude::*,
    tasks::{IoTaskPool, Task, futures::check_ready},
    ui_widgets::ValueChange,
};
use camera::OrbitCamera;
use fem_core::{ContactType, FemModel, FemModelVersion, MeshLoadRequest, MeshLoadStatus};
use gmsh::{StepMeshOptions, StepMeshProgress};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};
use visualization::VisualizationSettings;

use crate::{assembly::AssemblyEditorState, solver_runner::FrontistrRunState};

const DEFAULT_STEP_CURVE_RESOLUTION: i32 = StepMeshOptions::DEFAULT_ELEMENTS_PER_TURN as i32;
const MAX_STEP_CURVE_RESOLUTION: i32 = 100;

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

#[derive(Component)]
pub(crate) struct MeshStatsText;

#[derive(Resource, Default)]
pub(crate) struct StepMeshSettings {
    options: StepMeshOptions,
}

#[derive(Component)]
struct StepMeshSize;

#[derive(Component)]
struct StepCurveResolution;

#[derive(Component)]
pub(crate) struct SaveStepMeshButton;

#[derive(Component)]
pub(crate) struct SaveStepMeshControls;

#[derive(Component)]
pub(crate) struct SaveStepMeshStatus;

pub(crate) struct StepMeshJob {
    path: PathBuf,
    import: bool,
    task: Task<Result<fem_core::FemMesh, gmsh::GmshError>>,
    progress: Arc<Mutex<StepMeshProgress>>,
    started_at: Instant,
    displayed: Option<(StepMeshProgress, u64)>,
}

impl StepMeshJob {
    fn update_status(&mut self, status: &mut ResMut<MeshLoadStatus>) {
        let step = *self.progress.lock().unwrap_or_else(|error| error.into_inner());
        let seconds = self.started_at.elapsed().as_secs();
        if self.displayed == Some((step, seconds)) {
            return;
        }
        self.displayed = Some((step, seconds));
        status.message = format_step_progress(step, seconds);
    }
}

fn format_step_progress(step: StepMeshProgress, elapsed: u64) -> String {
    let (phase, percent) = match step {
        StepMeshProgress::ReadingStep => ("Reading STEP solid", None),
        StepMeshProgress::MeshingCurves(percent) => ("Meshing curves (1D)", percent),
        StepMeshProgress::MeshingSurfaces(percent) => ("Meshing surfaces (2D)", percent),
        StepMeshProgress::MeshingVolume(Some(nodes)) => {
            return format!("Meshing volume (3D)\nAdded nodes: {nodes}\nElapsed: {elapsed} s");
        }
        StepMeshProgress::MeshingVolume(None) => ("Meshing volume (3D)", None),
        StepMeshProgress::Optimizing => ("Optimizing mesh", None),
        StepMeshProgress::WritingMesh => ("Writing Gmsh mesh", None),
        StepMeshProgress::ImportingMesh => ("Importing generated mesh", None),
    };
    match percent {
        Some(percent) => format!("{phase}: {percent}%\nElapsed: {elapsed} s"),
        None => format!("{phase}\nElapsed: {elapsed} s"),
    }
}

fn is_step(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| {
        extension.eq_ignore_ascii_case("step") || extension.eq_ignore_ascii_case("stp")
    })
}

fn update_step_mesh_size(
    change: On<ValueChange<f32>>,
    inputs: Query<&NumberInputValue, With<StepMeshSize>>,
    mut settings: ResMut<StepMeshSettings>,
    mut commands: Commands,
) {
    let Ok(displayed) = inputs.get(change.source) else {
        return;
    };
    if change.value.is_finite() && change.value >= 0.0 {
        settings.options.max_size_mm = (change.value > 0.0).then_some(change.value);
        let value = NumberInputValue::F32(change.value);
        if *displayed != value {
            commands.entity(change.source).insert(value);
        }
    }
}

fn update_step_curve_resolution(
    change: On<ValueChange<i32>>,
    inputs: Query<&NumberInputValue, With<StepCurveResolution>>,
    mut settings: ResMut<StepMeshSettings>,
    mut commands: Commands,
) {
    let Ok(displayed) = inputs.get(change.source) else {
        return;
    };
    let resolution = change.value.clamp(0, MAX_STEP_CURVE_RESOLUTION);
    settings.options.elements_per_turn = resolution as u32;
    let value = NumberInputValue::I32(resolution);
    if *displayed != value {
        commands.entity(change.source).insert(value);
    }
}

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

    parent
        .spawn(Node {
            align_items: AlignItems::Center,
            column_gap: px(6),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new("STEP max size (mm)\n0 = automatic limit"),
                TextFont { font_size: FontSize::Px(10.0), ..default() },
                TextColor(TEXT_MUTED),
            ));
            let row_entity = row.target_entity();
            row.commands()
                .queue_spawn_scene(bsn! {
                    @FeathersNumberInput
                    NumberInputValue::F32(0.0)
                    HardLimit(NumberInputRange::F32(0.0..=f32::MAX))
                    NumberInputPrecision(4)
                    NumberInputStep(0.1)
                    Node { width: px(86) }
                })
                .insert((ChildOf(row_entity), StepMeshSize, Name::new("STEP maximum element size in mm")))
                .observe(update_step_mesh_size);
        });

    parent
        .spawn(Node {
            align_items: AlignItems::Center,
            column_gap: px(6),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new("STEP curve resolution\nElements per full turn\n0 = off / 20 = default"),
                TextFont { font_size: FontSize::Px(10.0), ..default() },
                TextColor(TEXT_MUTED),
            ));
            let row_entity = row.target_entity();
            row.commands()
                .queue_spawn_scene(bsn! {
                    @FeathersNumberInput
                    NumberInputValue::I32(DEFAULT_STEP_CURVE_RESOLUTION)
                    HardLimit(NumberInputRange::I32(0..=MAX_STEP_CURVE_RESOLUTION))
                    NumberInputStep(1.0)
                    Node { width: px(86) }
                })
                .insert((ChildOf(row_entity), StepCurveResolution, Name::new("STEP elements per turn")))
                .observe(update_step_curve_resolution);
        });

    parent
        .spawn((
            SaveStepMeshControls,
            Node {
                display: Display::None,
                flex_direction: FlexDirection::Column,
                row_gap: px(3),
                ..default()
            },
        ))
        .with_children(|controls| {
            controls
                .spawn((
                    InteractionButton,
                    SaveStepMeshButton,
                    Node {
                        padding: UiRect::axes(px(8), px(4)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(5)),
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(BUTTON_NORMAL),
                    BorderColor::all(PANEL_BORDER),
                ))
                .with_child((
                    Text::new("Save FrontISTR Mesh (.msh)..."),
                    TextFont { font_size: FontSize::Px(10.5), ..default() },
                    TextColor(TEXT_MAIN),
                ));
            controls.spawn((
                SaveStepMeshStatus,
                Text::default(),
                TextFont { font_size: FontSize::Px(10.0), ..default() },
                TextColor(TEXT_MUTED),
            ));
        });

    parent.spawn((
        Text::new("No mesh loaded"),
        TextFont {
            font_size: FontSize::Px(11.5),
            ..default()
        },
        TextColor(TEXT_MUTED),
        MeshStatsText,
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

pub(crate) fn open_mesh_button_system(
    mut request: ResMut<MeshLoadRequest>,
    mut status: ResMut<MeshLoadStatus>,
    mut run_state: ResMut<FrontistrRunState>,
    mut buttons: Query<
        (Ref<ButtonInteraction>, &mut BackgroundColor, &mut BorderColor),
        With<OpenMeshButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == ButtonInteraction::Pressed && interaction.is_changed() {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Open Model")
                .add_filter("All supported meshes and CAD", &["msh", "geo", "inp", "step", "stp"])
                .add_filter("STEP solid (.step, .stp)", &["step", "stp"])
                .add_filter("HECMW / FrontISTR (.msh)", &["msh"])
                .add_filter("Gmsh geometry (.geo)", &["geo"])
                .add_filter("Abaqus / CalculiX (.inp)", &["inp"])
                .pick_file()
            {
                if !is_step(&path) {
                    run_state.clear_export_target();
                }
                status.loading(path.clone());
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
        (Ref<ButtonInteraction>, &mut BackgroundColor, &mut BorderColor),
        With<AddMeshButton>,
    >,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == ButtonInteraction::Pressed && interaction.is_changed() {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Add Part")
                .add_filter("All supported meshes and CAD", &["msh", "geo", "inp", "step", "stp"])
                .add_filter("STEP solid (.step, .stp)", &["step", "stp"])
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
            ButtonInteraction::Pressed => BUTTON_PRESSED,
            ButtonInteraction::Hovered => BUTTON_HOVERED,
            ButtonInteraction::None => BUTTON_NORMAL,
        };

        *background = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);
    }
}

pub(crate) fn save_step_mesh_button_system(
    model: Option<Res<FemModel>>,
    editor: Res<AssemblyEditorState>,
    mut controls: Query<&mut Node, With<SaveStepMeshControls>>,
    mut buttons: Query<(Ref<ButtonInteraction>, &mut BackgroundColor), With<SaveStepMeshButton>>,
    mut status: Query<&mut Text, With<SaveStepMeshStatus>>,
) {
    let Some((part, mesh)) = model.as_deref().and_then(|model| {
        let part = model.parts.get(editor.selected_part?)?;
        let mesh = model.meshes.get(part.mesh_index)?;
        is_step(Path::new(&part.name)).then_some((part, mesh))
    }) else {
        for mut control in &mut controls {
            if control.display != Display::None {
                control.display = Display::None;
            }
        }
        for mut text in &mut status {
            if !text.is_empty() {
                text.clear();
            }
        }
        return;
    };
    for mut control in &mut controls {
        if control.display != Display::Flex {
            control.display = Display::Flex;
        }
    }
    for (interaction, mut background) in &mut buttons {
        background.set_if_neq(BackgroundColor(match *interaction {
            ButtonInteraction::Pressed => BUTTON_PRESSED,
            ButtonInteraction::Hovered => BUTTON_HOVERED,
            ButtonInteraction::None => BUTTON_NORMAL,
        }));
        if *interaction != ButtonInteraction::Pressed || !interaction.is_changed() {
            continue;
        }
        let name = Path::new(&part.name).with_extension("msh");
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save FrontISTR mesh geometry and groups")
            .add_filter("FrontISTR mesh (.msh)", &["msh"])
            .set_file_name(name.to_string_lossy())
            .save_file()
        else {
            continue;
        };
        let message = if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("msh"))
        {
            match hecmw::write_msh_geometry_file(&path, mesh) {
                Ok((nodes, elements)) => {
                    let file = path.file_name().unwrap_or_default().to_string_lossy();
                    format!("Saved {file} ({nodes} nodes, {elements} elements)")
                }
                Err(error) => format!("Could not save mesh: {error}"),
            }
        } else {
            "Enter a filename ending in .msh.".into()
        };
        for mut text in &mut status {
            **text = message.clone();
        }
    }
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
    mut step_job: Local<Option<StepMeshJob>>,
) {
    let Some((path, import)) = request.take() else {
        let Some(job) = step_job.as_mut() else {
            return;
        };
        let Some(result) = check_ready(&mut job.task) else {
            job.update_status(&mut status);
            return;
        };
        let Some(StepMeshJob { path, import, .. }) = step_job.take() else {
            return;
        };
        match result {
            Ok(mesh) => {
                let size = mesh.bounds().map(|(min, max)| max - min);
                if !import && let Some(run) = run_state.as_deref_mut() {
                    run.clear_export_target();
                }
                apply_mesh(
                    mesh, &path, import, &mut model, &mut status, &mut version,
                    &mut camera_fit, &mut setup,
                );
                if let Some(size) = size {
                    status.message = format!(
                        "STEP meshed | {:.6} x {:.6} x {:.6} mm (X x Y x Z)",
                        size.x, size.y, size.z
                    );
                }
            }
            Err(error) => status.failed(path, error.to_string()),
        }
        return;
    };

    // A newer Open/Add request owns the viewport, even if Gmsh is still
    // writing the superseded STEP job's temporary files.
    *step_job = None;
    if is_step(&path) {
        let Some(pool) = IoTaskPool::try_get() else {
            status.failed(path, "Gmsh background task pool is unavailable");
            return;
        };
        status.loading(path.clone());
        let input = path.clone();
        let options = step_settings
            .as_deref()
            .map(|settings| settings.options)
            .unwrap_or_default();
        let progress = Arc::new(Mutex::new(StepMeshProgress::ReadingStep));
        let latest = Arc::clone(&progress);
        let task = pool.spawn(async move {
            gmsh::mesh_step_with_progress(&input, options, |step| {
                *latest.lock().unwrap_or_else(|error| error.into_inner()) = step;
            })
        });
        let mut job = StepMeshJob {
            path,
            import,
            task,
            progress,
            started_at: Instant::now(),
            displayed: None,
        };
        job.update_status(&mut status);
        *step_job = Some(job);
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
                mesh, materials, sections, contacts: contact_pairs, mut equations, import_issues,
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

    let display_model=result_geometry.as_deref().filter(|g|g.visible).and_then(|g|g.model.as_ref()).or(model.as_deref());
    let Some((min, max)) = display_model.and_then(FemModel::bounds) else {
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
    mut run_state: ResMut<FrontistrRunState>,
    mut buttons: Query<
        (Ref<ButtonInteraction>, &mut BackgroundColor, &mut BorderColor),
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

            run_state.clear_export_target();

            match hecmw::load_hecmw_ctrl(&ctrl_path) {
                Ok(mut content) => {
                    let (mesh_path, cnt_path) = hecmw::resolve_paths(&ctrl_path, &content);
                    if let (Some(mesh_path), Some(cnt_path)) = (mesh_path, cnt_path) {
                        load_status.loading(mesh_path.clone());
                        request.request(mesh_path);
                        pending_cnt.request(cnt_path, 0, version.value);
                        let partition = if content.has_partition_input {
                            let path = ctrl_path.with_file_name("hecmw_part_ctrl.dat");
                            match hecmw::partition_control::load(&path) {
                                Ok(partition) => {
                                    run_state.use_imported_ranks(partition.domains.get());
                                    Some(partition)
                                }
                                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
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
                        pending_cnt.project_control = Some(fem_core::FrontistrControlImport {
                            control: content.settings,
                            partition,
                            issues: content.import_issues,
                        });
                    } else {
                        bevy::log::warn!("hecmw_ctrl.dat: mesh or control file not found");
                    }
                }
                Err(e) => bevy::log::warn!("Failed to parse hecmw_ctrl.dat: {e}"),
            }
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
            for issue in &data.import_issues {
                bevy::log::warn!("Setup import: {issue}; Export/Run blocked (viewing is available)");
            }
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
    mut run_state: ResMut<FrontistrRunState>,
    mut buttons: Query<
        (Ref<ButtonInteraction>, &mut BackgroundColor, &mut BorderColor),
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
                &stem, run_state.last_output_directory(),
            )
            else {
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

/// Loads a standalone FrontISTR `.cnt` into the current model.
pub(crate) fn open_setup_button_system(
    mut pending_path: Local<Option<std::path::PathBuf>>,
    mut buttons: Query<
        (Ref<ButtonInteraction>, &mut BackgroundColor, &mut BorderColor),
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
                for issue in &data.import_issues {
                    bevy::log::warn!("Setup import: {issue}; Export/Run blocked (viewing is available)");
                }
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_controls_update_job_options_and_displayed_values() {
        let mut app = App::new();
        app.init_resource::<StepMeshSettings>();
        let world = app.world_mut();
        let max_size = world
            .spawn((StepMeshSize, NumberInputValue::F32(0.0)))
            .observe(update_step_mesh_size)
            .id();
        let resolution = world
            .spawn((
                StepCurveResolution,
                NumberInputValue::I32(DEFAULT_STEP_CURVE_RESOLUTION),
            ))
            .observe(update_step_curve_resolution)
            .id();

        world.trigger(ValueChange {
            source: max_size,
            value: 4.0_f32,
            is_final: true,
        });
        world.trigger(ValueChange {
            source: resolution,
            value: 40_i32,
            is_final: true,
        });
        world.flush();
        assert_eq!(world.get(max_size), Some(&NumberInputValue::F32(4.0)));
        assert_eq!(world.get(resolution), Some(&NumberInputValue::I32(40)));
        assert_eq!(
            world.resource::<StepMeshSettings>().options,
            StepMeshOptions {
                max_size_mm: Some(4.0),
                elements_per_turn: 40,
            }
        );

        world.trigger(ValueChange {
            source: max_size,
            value: 0.0_f32,
            is_final: true,
        });
        world.trigger(ValueChange {
            source: resolution,
            value: 0_i32,
            is_final: true,
        });
        world.flush();
        assert_eq!(
            world.resource::<StepMeshSettings>().options,
            StepMeshOptions {
                max_size_mm: None,
                elements_per_turn: 0,
            }
        );
    }
}
