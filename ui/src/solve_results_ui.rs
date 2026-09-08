//! Automatic, asynchronous Solve -> Results handoff. A completed run starts
//! loading in the background; the Solve button remains available to reopen.
use crate::{
    layout::SidebarPage,
    results_ui::PlaybackState,
    slider::{SliderId, SliderState, SliderTrack},
    solver_runner::FrontistrRunState,
};
use bevy::prelude::*;
use fem_core::{FemModel, FemModelVersion, FemResultSet, NodeId, StepResult};
use std::sync::{
    Mutex,
    mpsc::{self, Receiver, TryRecvError},
};
use visualization::{ContourSettings, VisualizationSettings};

type Loaded = Result<Vec<Vec<StepResult>>, String>;
struct Pending {
    run_id: u64,
    version: u64,
    receiver: Mutex<Receiver<Loaded>>,
}
#[derive(Resource, Default)]
pub(crate) struct SolveResultsState {
    pending: Option<Pending>,
    pub(crate) status: String,
    last_run: Option<u64>,
}
#[derive(Component)]
pub(crate) struct OpenRunResultsButton;
#[derive(Component)]
pub(crate) struct RunResultsText;

pub(crate) fn spawn_results_handoff(panel: &mut ChildSpawnerCommands) {
    panel
        .spawn((
            Button,
            OpenRunResultsButton,
            Node {
                height: px(28.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(4.0)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.10, 0.12, 0.14)),
            BorderColor::all(Color::srgb(0.2, 0.5, 0.4)),
        ))
        .with_child((
            Text::new("Open analysis results again"),
            TextFont {
                font_size: FontSize::Px(11.0),
                ..default()
            },
            TextColor(Color::WHITE),
        ));
    panel.spawn((
        Text::new("Results open automatically after a successful analysis; this reopens them."),
        TextFont {
            font_size: FontSize::Px(9.5),
            ..default()
        },
        TextColor(Color::srgb(0.65, 0.75, 0.70)),
        RunResultsText,
    ));
}

impl SolveResultsState {
    pub(crate) fn cancel_pending(&mut self) {
        self.pending = None;
    }
}

fn queue_results_load(
    source: &crate::run_results::RunResultSource,
    model: &FemModel,
    version: u64,
    state: &mut SolveResultsState,
) {
    let offsets = hecmw::assembly_id_offsets(model);
    let parts: Vec<Vec<NodeId>> = model
        .meshes
        .iter()
        .enumerate()
        .map(|(mi, mesh)| {
            mesh.nodes
                .iter()
                .map(|node| hecmw::remap_node(&offsets, mi, node.id))
                .collect()
        })
        .collect();
    let source = source.clone();
    let run_id = source.id;
    let (tx, rx) = mpsc::channel();
    state.pending = Some(Pending {
        run_id,
        version,
        receiver: Mutex::new(rx),
    });
    state.status = "Loading this run's results...".into();
    std::thread::spawn(move || {
        let _ = tx.send(source.load(&parts));
    });
}

pub(crate) fn open_run_results_system(
    run: Res<FrontistrRunState>,
    model: Option<Res<FemModel>>,
    version: Res<FemModelVersion>,
    mut state: ResMut<SolveResultsState>,
    mut buttons: Query<(Ref<Interaction>, &mut BackgroundColor), With<OpenRunResultsButton>>,
    mut labels: Query<&mut Text, With<RunResultsText>>,
) {
    let source = run.completed_results();
    let id = source.map(|s| s.id);
    let new_run = state.last_run != id;
    if new_run {
        state.pending = None;
        state.status.clear();
        state.last_run = id;
    }
    let compatible = source.is_some_and(|s| s.model_version == version.value);
    if new_run {
        if let (Some(source), Some(model)) = (source, model.as_deref()) {
            if compatible {
                queue_results_load(source, model, version.value, &mut state);
            }
        }
    }
    let enabled = compatible && model.is_some() && state.pending.is_none();
    for (interaction, mut background) in &mut buttons {
        if enabled && *interaction == Interaction::Pressed && interaction.is_changed() {
            if let (Some(model), Some(source)) = (model.as_deref(), source) {
                queue_results_load(source, model, version.value, &mut state);
            }
        }
        background.set_if_neq(BackgroundColor(if enabled {
            if *interaction == Interaction::Hovered {
                Color::srgb(0.16, 0.5, 0.3)
            } else {
                Color::srgb(0.10, 0.32, 0.20)
            }
        } else {
            Color::srgb(0.08, 0.09, 0.10)
        }));
    }
    let status = if source.is_some() && !compatible {
        "Model changed since this run. Run again before opening its results."
    } else if !state.status.is_empty() {
        &state.status
    } else if enabled {
        "Completed. Results open automatically; use the button to reopen them."
    } else {
        "Results open automatically after a successful analysis."
    };
    for mut text in &mut labels {
        text.set_if_neq(Text::new(status));
    }
}

pub(crate) fn poll_run_results_system(
    run: Res<FrontistrRunState>,
    version: Res<FemModelVersion>,
    mut state: ResMut<SolveResultsState>,
    mut results: ResMut<FemResultSet>,
    mut settings: ResMut<VisualizationSettings>,
    mut page: ResMut<SidebarPage>,
    mut playback: ResMut<PlaybackState>,
    mut sliders: Query<&mut SliderState, With<SliderTrack>>,
) {
    let Some(pending) = state.pending.as_ref() else {
        return;
    };
    if version.value != pending.version
        || run.completed_results().map(|s| s.id) != Some(pending.run_id)
    {
        state.pending = None;
        state.status = "Result loading cancelled: model or run changed.".into();
        return;
    }
    let reply = pending.receiver.lock().unwrap().try_recv();
    let loaded = match reply {
        Ok(reply) => reply,
        Err(TryRecvError::Empty) => return,
        Err(TryRecvError::Disconnected) => Err("Result loading worker ended unexpectedly".into()),
    };
    state.pending = None;
    match loaded {
        Err(error) => state.status = format!("Could not open results: {error}"),
        Ok(by_mesh) => {
            let steps = by_mesh.first().map_or(0, Vec::len);
            install_results(
                by_mesh,
                &mut results,
                &mut settings,
                &mut playback,
                &mut sliders,
            );
            *page = SidebarPage::Results;
            state.status =
                format!("Loaded {steps} step(s) from this run. Reopening replaces these results.");
        }
    }
}

fn install_results(
    by_mesh: Vec<Vec<StepResult>>,
    results: &mut FemResultSet,
    settings: &mut VisualizationSettings,
    playback: &mut PlaybackState,
    sliders: &mut Query<&mut SliderState, With<SliderTrack>>,
) {
    // Replace, do not append stale steps or reactivate an earlier run's field.
    *results = FemResultSet {
        by_mesh,
        active: None,
    };
    results.activate_first();
    if let Some(active) = &mut results.active {
        // Open the solved state, not the undeformed time-zero output. Rewind
        // remains available for playing the complete series from the start.
        active.step_index = results.by_mesh[active.mesh_index].len() - 1;
        let step = &results.by_mesh[active.mesh_index][active.step_index];
        active.field_name = step.fields[0].name().to_string();
        if step.field_by_name("Displacement").is_some() {
            active.field_name = "Displacement".into();
        }
        settings.contour = Some(ContourSettings {
            mesh_index: active.mesh_index,
            step_index: active.step_index,
            field_name: active.field_name.clone(),
            show_deformation: step.field_by_name("Displacement").is_some(),
            displacement_field: "Displacement".into(),
            deformation_scale: 1.0,
        });
    }
    playback.playing = false;
    playback.elapsed = 0.0;
    for mut slider in sliders.iter_mut() {
        if slider.id == SliderId::ResultStep {
            slider.max = results
                .by_mesh
                .first()
                .map_or(0, Vec::len)
                .saturating_sub(1) as f32;
            slider.value = slider.max;
        } else if slider.id == SliderId::DeformScale {
            slider.value = 1.0;
        }
    }
}
