use crate::{ButtonInteraction, InteractionButton};
use bevy::{
    feathers::controls::{
        FeathersNumberInput, HardLimit, NumberInputPrecision, NumberInputRange, NumberInputStep,
        NumberInputValue,
    },
    prelude::*,
    tasks::{IoTaskPool, Task, futures::check_ready},
    ui::InteractionDisabled,
    ui_widgets::ValueChange,
};
use fem_core::{FaceId, FemMesh};
use gmsh::{StepContactRefinement, StepMeshOptions, StepMeshProgress, step_face_tag};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

#[derive(Clone)]
pub(crate) struct StepSource {
    pub path: PathBuf,
    pub add_part: bool,
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StepImportAction {
    Accept,
    Regenerate,
    Discard,
}

pub(crate) struct StepPreviewMeshes {
    pub surface: Option<Mesh>,
    pub edges: Option<Mesh>,
}

struct GeneratedStepMesh {
    mesh: FemMesh,
    render: Option<StepPreviewMeshes>,
    bounds: Option<(Vec3, Vec3)>,
    contact_face_tags: BTreeMap<FaceId, u32>,
    snapshot: Arc<tempfile::TempPath>,
}

struct StepJob {
    task: Task<Result<GeneratedStepMesh, gmsh::GmshError>>,
    progress: Arc<Mutex<StepMeshProgress>>,
    started_at: Instant,
    options: StepMeshOptions,
    refinement: Option<StepContactRefinement>,
}

#[derive(Default)]
enum StepState {
    #[default]
    Idle,
    Meshing(StepJob),
    Preview {
        generated: GeneratedStepMesh,
        options: StepMeshOptions,
        refinement: Option<StepContactRefinement>,
    },
    Failed(String),
}

#[derive(Resource, Default)]
pub(crate) struct StepImport {
    source: Option<StepSource>,
    state: StepState,
    revision: u64,
    action: Option<StepImportAction>,
    snapshot: Option<Arc<tempfile::TempPath>>,
    contact_surface_tags: BTreeSet<u32>,
    contact_size_mm: Option<f32>,
    contact_revision: u64,
}

impl StepImport {
    pub(crate) fn clear(&mut self) {
        self.source = None;
        self.state = StepState::Idle;
        self.action = None;
        self.snapshot = None;
        self.contact_surface_tags.clear();
        self.contact_revision = self.contact_revision.wrapping_add(1);
    }

    pub(crate) fn start(&mut self, source: StepSource, options: StepMeshOptions) {
        let path = source.path.clone();
        self.source = Some(source);
        self.revision = self.revision.wrapping_add(1);
        let Some(pool) = IoTaskPool::try_get() else {
            self.state = StepState::Failed("Gmsh background task pool is unavailable".into());
            return;
        };

        let progress = Arc::new(Mutex::new(StepMeshProgress::ReadingStep));
        let latest = Arc::clone(&progress);
        let started_at = Instant::now();
        let refinement = self.requested_refinement();
        let requested = refinement.clone();
        let snapshot = self.snapshot.clone();
        let task = pool.spawn(async move {
            let snapshot = match snapshot {
                Some(snapshot) => snapshot,
                None => {
                    let file = tempfile::NamedTempFile::new().map_err(gmsh::GmshError::Io)?;
                    std::fs::copy(path, file.path()).map_err(gmsh::GmshError::Io)?;
                    Arc::new(file.into_temp_path())
                }
            };
            let mesh = gmsh::mesh_step_with_contact_refinement(
                snapshot.as_ref().as_ref(),
                options,
                requested.as_ref(),
                |step| *latest.lock().unwrap_or_else(|error| error.into_inner()) = step,
            )?;
            let contact_face_tags = contact_face_tags(&mesh);
            let render = StepPreviewMeshes {
                surface: visualization::build_part_surface_mesh(&mesh),
                edges: visualization::build_part_edge_mesh(&mesh),
            };
            let bounds = mesh.bounds();
            Ok(GeneratedStepMesh {
                mesh,
                render: Some(render),
                bounds,
                contact_face_tags,
                snapshot,
            })
        });
        self.state = StepState::Meshing(StepJob {
            task,
            progress,
            started_at,
            options,
            refinement,
        });
    }

    pub(crate) fn is_active(&self) -> bool {
        self.source.is_some()
    }

    pub(crate) fn is_meshing(&self) -> bool {
        matches!(&self.state, StepState::Meshing(_))
    }

    pub(crate) fn has_action(&self) -> bool {
        self.action.is_some()
    }

    pub(crate) fn poll(&mut self) {
        let StepState::Meshing(job) = &mut self.state else {
            return;
        };
        if let Some(result) = check_ready(&mut job.task) {
            self.state = match result {
                Ok(generated) => {
                    self.snapshot = Some(Arc::clone(&generated.snapshot));
                    StepState::Preview {
                        generated,
                        options: job.options,
                        refinement: job.refinement.take(),
                    }
                }
                Err(error) => StepState::Failed(error.to_string()),
            };
        }
    }

    pub(crate) fn preview(&self) -> Option<(u64, Option<(Vec3, Vec3)>)> {
        match &self.state {
            StepState::Preview { generated, .. } => Some((self.revision, generated.bounds)),
            _ => None,
        }
    }

    pub(crate) fn take_preview_meshes(&mut self) -> Option<StepPreviewMeshes> {
        match &mut self.state {
            StepState::Preview { generated, .. } => generated.render.take(),
            _ => None,
        }
    }

    fn requested_refinement(&self) -> Option<StepContactRefinement> {
        let target_size_mm = self.contact_size_mm.unwrap_or(DEFAULT_CONTACT_SIZE_MM);
        (!self.contact_surface_tags.is_empty() && target_size_mm > 0.0).then(|| {
            StepContactRefinement {
                surface_tags: self.contact_surface_tags.iter().copied().collect(),
                target_size_mm,
            }
        })
    }

    pub(crate) fn toggle_contact_face(&mut self, origin: Vec3, direction: Vec3) {
        let StepState::Preview { generated, .. } = &self.state else {
            return;
        };
        let Some(tag) = picking::pick_mesh_boundary_face(&generated.mesh, origin, direction)
            .and_then(|face| generated.contact_face_tags.get(&face).copied())
        else {
            return;
        };
        if !self.contact_surface_tags.remove(&tag) {
            self.contact_surface_tags.insert(tag);
        }
        self.contact_revision = self.contact_revision.wrapping_add(1);
    }

    pub(crate) fn contact_highlight_revision(&self) -> Option<(u64, u64)> {
        self.preview()
            .filter(|_| !self.contact_surface_tags.is_empty())
            .map(|(revision, _)| (revision, self.contact_revision))
    }

    pub(crate) fn build_contact_highlight(&self) -> Option<Mesh> {
        let StepState::Preview { generated, .. } = &self.state else {
            return None;
        };
        visualization::build_boundary_surface_subset(&generated.mesh, |face| {
            generated
                .contact_face_tags
                .get(&face.id)
                .is_some_and(|tag| self.contact_surface_tags.contains(tag))
        })
    }

    pub(crate) fn queue_action(&mut self, action: StepImportAction) {
        self.action = Some(action);
    }

    pub(crate) fn apply_action(
        &mut self,
        options: StepMeshOptions,
    ) -> Option<(StepSource, FemMesh)> {
        let action = self.action.take()?;
        if !self.action_available(action, options) {
            return None;
        }
        match action {
            StepImportAction::Accept => {
                let source = self.source.take()?;
                match std::mem::take(&mut self.state) {
                    StepState::Preview { generated, .. } => {
                        self.clear();
                        Some((source, generated.mesh))
                    }
                    _ => unreachable!("only a ready preview can be accepted"),
                }
            }
            StepImportAction::Regenerate => {
                if let Some(source) = self.source.clone() {
                    self.start(source, options);
                }
                None
            }
            StepImportAction::Discard => {
                self.clear();
                None
            }
        }
    }

    fn action_available(&self, action: StepImportAction, options: StepMeshOptions) -> bool {
        match action {
            StepImportAction::Accept => {
                matches!(&self.state, StepState::Preview { options: generated, refinement, .. }
                    if *generated == options && *refinement == self.requested_refinement())
            }
            StepImportAction::Regenerate | StepImportAction::Discard => {
                matches!(self.state, StepState::Preview { .. } | StepState::Failed(_))
            }
        }
    }
}

fn contact_face_tags(mesh: &FemMesh) -> BTreeMap<FaceId, u32> {
    let by_element_face = mesh
        .surface_sets
        .iter()
        .filter_map(|group| step_face_tag(&group.name).map(|tag| (tag, group)))
        .flat_map(|(tag, group)| group.surfaces.iter().map(move |face| (*face, tag)))
        .collect::<BTreeMap<_, _>>();
    mesh.cached_boundary_faces()
        .iter()
        .filter_map(|face| {
            let reference = face.element_face_ref()?;
            Some((face.id, *by_element_face.get(&reference)?))
        })
        .collect()
}

#[derive(Component)]
struct StepContactSize;

fn update_step_contact_size(
    change: On<ValueChange<f32>>,
    inputs: Query<&NumberInputValue, With<StepContactSize>>,
    mut step: ResMut<StepImport>,
    mut commands: Commands,
) {
    let Ok(displayed) = inputs.get(change.source) else {
        return;
    };
    if change.value.is_finite() && change.value >= 0.0 {
        step.contact_size_mm = Some(change.value);
        let value = NumberInputValue::F32(change.value);
        if *displayed != value {
            commands.entity(change.source).insert(value);
        }
    }
}

#[derive(Component)]
pub(crate) enum StepWidget {
    Panel,
    Track,
    Fill,
    Actions,
    ContactControls,
    CommittedStats,
}

#[derive(Component)]
pub(crate) struct StepStatus;

const DEFAULT_CONTACT_SIZE_MM: f32 = 1.0;

const ACCENT: Color = Color::srgb(0.33, 0.72, 0.83);
const NORMAL: Color = Color::srgb(0.12, 0.17, 0.21);
const HOVER: Color = Color::srgb(0.21, 0.33, 0.39);
const DISABLED: Color = Color::srgb(0.09, 0.11, 0.14);

pub(crate) fn spawn_step_import_ui(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            StepWidget::Panel,
            Node {
                display: Display::None,
                flex_direction: FlexDirection::Column,
                row_gap: px(5),
                padding: UiRect::all(px(6)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.06, 0.10, 0.12, 0.94)),
            BorderColor::all(Color::srgb(0.28, 0.44, 0.50)),
        ))
        .with_children(|panel| {
            spawn_step_status_and_progress(panel);
            panel
                .spawn((
                    StepWidget::ContactControls,
                    Node {
                        align_items: AlignItems::Center,
                        column_gap: px(6),
                        ..default()
                    },
                ))
                .with_children(|row| {
                    row.spawn((
                        Text::new("Contact size (mm)\n0 = off"),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(Color::srgb(0.87, 0.94, 0.95)),
                    ));
                    let parent = row.target_entity();
                    row.commands()
                        .queue_spawn_scene(bsn! {
                            @FeathersNumberInput
                            NumberInputValue::F32(DEFAULT_CONTACT_SIZE_MM)
                            HardLimit(NumberInputRange::F32(0.0..=f32::MAX))
                            NumberInputPrecision(4)
                            NumberInputStep(0.1)
                            Node { width: px(86) }
                        })
                        .insert((
                            ChildOf(parent),
                            StepContactSize,
                            Name::new("STEP contact mesh size in mm"),
                        ))
                        .observe(update_step_contact_size);
                });
            panel
                .spawn((
                    StepWidget::Actions,
                    Node {
                        column_gap: px(4),
                        ..default()
                    },
                ))
                .with_children(|row| {
                    for (action, label) in [
                        (StepImportAction::Accept, "Use Mesh"),
                        (StepImportAction::Regenerate, "Regenerate Preview"),
                        (StepImportAction::Discard, "Discard"),
                    ] {
                        row.spawn((
                            action,
                            InteractionButton,
                            Node {
                                padding: UiRect::axes(px(5), px(4)),
                                border_radius: BorderRadius::all(px(3)),
                                ..default()
                            },
                            BackgroundColor(NORMAL),
                        ))
                        .with_child((
                            Text::new(label),
                            TextFont {
                                font_size: FontSize::Px(10.0),
                                ..default()
                            },
                            TextColor(Color::srgb(0.90, 0.94, 0.96)),
                        ));
                    }
                });
        });
}

fn spawn_step_status_and_progress(panel: &mut ChildSpawnerCommands) {
    panel.spawn((
        StepStatus,
        Text::default(),
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        TextColor(Color::srgb(0.87, 0.94, 0.95)),
    ));
    panel
        .spawn((
            StepWidget::Track,
            Node {
                height: px(7),
                border_radius: BorderRadius::all(px(2)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgb(0.19, 0.25, 0.29)),
        ))
        .with_child((
            StepWidget::Fill,
            Node {
                position_type: PositionType::Absolute,
                height: percent(100),
                ..default()
            },
            BackgroundColor(ACCENT),
        ));
}

pub(crate) fn step_action_buttons(
    mut step: ResMut<StepImport>,
    buttons: Query<(&StepImportAction, Ref<ButtonInteraction>)>,
) {
    for (action, input) in &buttons {
        if input.is_changed() && *input == ButtonInteraction::Pressed {
            step.queue_action(*action);
        }
    }
}

pub(crate) fn update_step_import_ui(
    mut commands: Commands,
    step: Res<StepImport>,
    settings: Res<crate::project_io::StepMeshSettings>,
    mut panel: Query<(&StepWidget, &mut Node)>,
    mut status_text: Query<&mut Text, With<StepStatus>>,
    mut buttons: Query<(
        Entity,
        &StepImportAction,
        Ref<ButtonInteraction>,
        &mut BackgroundColor,
        Has<InteractionDisabled>,
    )>,
) {
    let options = settings.options;
    let busy = step.is_meshing();
    if !busy
        && !step.is_changed()
        && !settings.is_changed()
        && !buttons.iter().any(|(_, _, input, _, _)| input.is_changed())
    {
        return;
    }
    let active = step.is_active();
    let preview = step.preview().is_some();
    let (label, progress) = if let Some(source) = &step.source {
        let filename = source
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let intent = if source.add_part {
            "Add part"
        } else {
            "Replace model"
        };
        match &step.state {
            StepState::Meshing(job) => {
                let stage = *job
                    .progress
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                let elapsed = job.started_at.elapsed();
                let (detail, fraction) = progress_label(stage);
                let bar = match fraction {
                    Some(percent) => (0.0, f32::from(percent)),
                    None => {
                        let cycle = elapsed.as_secs_f32().rem_euclid(2.0);
                        ((1.0 - (cycle - 1.0).abs()) * 74.0, 26.0)
                    }
                };
                (
                    format!("{filename} | {intent}\n{detail} | {} s", elapsed.as_secs()),
                    bar,
                )
            }
            StepState::Preview { generated, .. } => {
                let mesh = &generated.mesh;
                let hint = if step.action_available(StepImportAction::Accept, options) {
                    "Use Mesh to confirm."
                } else {
                    "Settings changed. Regenerate to apply."
                };
                let dimensions = generated.bounds.map_or_else(String::new, |(min, max)| {
                    let size = max - min;
                    format!(
                        "\n{:.4} x {:.4} x {:.4} mm (X x Y x Z)",
                        size.x, size.y, size.z
                    )
                });
                let contact_faces = match step.contact_surface_tags.len() {
                    0 => "Click faces to set contact refinement.".into(),
                    count => {
                        let tags = step
                            .contact_surface_tags
                            .iter()
                            .take(6)
                            .map(u32::to_string)
                            .collect::<Vec<_>>()
                            .join(", ");
                        let more = if count > 6 { ", ..." } else { "" };
                        format!("{count} marked STEP faces: {tags}{more}")
                    }
                };
                (
                    format!(
                        "STEP mesh preview | {intent}\n{filename} | {} nodes / {} tetrahedra{dimensions}\n{contact_faces}\n{hint}",
                        mesh.nodes.len(),
                        mesh.elements.len()
                    ),
                    (0.0, 0.0),
                )
            }
            StepState::Failed(error) => {
                (format!("{filename} | Meshing failed\n{error}"), (0.0, 0.0))
            }
            StepState::Idle => (String::new(), (0.0, 0.0)),
        }
    } else {
        (String::new(), (0.0, 0.0))
    };

    for (widget, mut node) in &mut panel {
        let shown = match widget {
            StepWidget::Panel => active,
            StepWidget::Track => busy,
            StepWidget::Actions => active && !busy,
            StepWidget::ContactControls => {
                preview
                    || matches!(step.state, StepState::Failed(_))
                        && !step.contact_surface_tags.is_empty()
            }
            StepWidget::CommittedStats => !preview,
            StepWidget::Fill => {
                if busy {
                    let (left, width) = (percent(progress.0), percent(progress.1));
                    if (node.left, node.width) != (left, width) {
                        node.left = left;
                        node.width = width;
                    }
                }
                continue;
            }
        };
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    for mut text in &mut status_text {
        if **text != label {
            **text = label.clone();
        }
    }
    for (entity, action, input, mut background, disabled) in &mut buttons {
        let available = step.action_available(*action, options);
        if available && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else if !available && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        }
        background.set_if_neq(BackgroundColor(match (*input, available, *action) {
            (_, false, _) => DISABLED,
            (ButtonInteraction::Hovered | ButtonInteraction::Pressed, true, _) => HOVER,
            (_, true, StepImportAction::Accept) => Color::srgb(0.13, 0.35, 0.38),
            _ => NORMAL,
        }));
    }
}

fn progress_label(stage: StepMeshProgress) -> (String, Option<u8>) {
    match stage {
        StepMeshProgress::ReadingStep => ("Reading STEP".into(), None),
        StepMeshProgress::MeshingCurves(percent) => {
            (phase_percent("Curves (1D)", percent), percent)
        }
        StepMeshProgress::MeshingSurfaces(percent) => {
            (phase_percent("Surfaces (2D)", percent), percent)
        }
        StepMeshProgress::MeshingVolume(Some(nodes)) => {
            (format!("Volume (3D) | {nodes} nodes added"), None)
        }
        StepMeshProgress::MeshingVolume(None) => ("Volume (3D)".into(), None),
        StepMeshProgress::Optimizing => ("Optimizing mesh".into(), None),
        StepMeshProgress::WritingMesh => ("Writing Gmsh mesh".into(), None),
        StepMeshProgress::ImportingMesh => ("Preparing preview".into(), None),
    }
}

fn phase_percent(name: &str, percent: Option<u8>) -> String {
    match percent {
        Some(percent) => format!("{name} {percent}%"),
        None => name.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_io::StepMeshSettings;

    #[test]
    fn progress_bar_shows_phase_fraction_and_indeterminate_activity() {
        let pool = IoTaskPool::get_or_init(bevy::tasks::TaskPool::new);
        let progress = Arc::new(Mutex::new(StepMeshProgress::MeshingSurfaces(Some(60))));
        let mut app = App::new();
        app.insert_resource(StepImport {
            source: Some(StepSource {
                path: "bracket.step".into(),
                add_part: false,
            }),
            state: StepState::Meshing(StepJob {
                task: pool.spawn(std::future::pending()),
                progress: Arc::clone(&progress),
                started_at: Instant::now(),
                options: StepMeshOptions::default(),
                refinement: None,
            }),
            ..default()
        })
        .init_resource::<StepMeshSettings>()
        .add_systems(Startup, |mut commands: Commands| {
            commands
                .spawn((StepWidget::Panel, Node::default()))
                .with_children(|panel| {
                    spawn_step_status_and_progress(panel);
                    panel.spawn((StepWidget::Actions, Node::default()));
                });
        })
        .add_systems(Update, update_step_import_ui);

        app.update();
        let world = app.world_mut();
        let mut nodes = world.query::<(&StepWidget, &Node)>();
        let find_fill = |nodes: &mut QueryState<(&StepWidget, &Node)>, world: &World| {
            nodes
                .iter(world)
                .find_map(|(marker, node)| {
                    matches!(marker, StepWidget::Fill).then_some((node.left, node.width))
                })
                .unwrap()
        };
        assert_eq!(find_fill(&mut nodes, world), (percent(0), percent(60)));
        assert!(nodes.iter(world).any(|(marker, node)| {
            matches!(marker, StepWidget::Track) && node.display == Display::Flex
        }));
        assert!(nodes.iter(world).any(|(marker, node)| {
            matches!(marker, StepWidget::Actions) && node.display == Display::None
        }));

        *progress.lock().unwrap() = StepMeshProgress::MeshingVolume(Some(6500));
        app.update();
        assert_eq!(find_fill(&mut nodes, app.world()).1, percent(26));
        let world = app.world_mut();
        let mut statuses = world.query_filtered::<&Text, With<StepStatus>>();
        assert!(statuses.single(world).unwrap().contains("6500 nodes added"));

        world.resource_mut::<StepImport>().clear();
        app.update();
        assert!(nodes.iter(app.world()).any(|(marker, node)| {
            matches!(marker, StepWidget::Track) && node.display == Display::None
        }));
    }
}
