//! Post-processing result loading, timeline navigation, and animation UI.

use crate::layout::SidebarPage;
use crate::slider::{SliderId, SliderState, SliderTrack};
use bevy::prelude::*;
use fem_core::{FemModel, FemResultSet};
use visualization::ContourSettings;

const PANEL_BORDER: Color = Color::srgba(0.34, 0.40, 0.44, 0.72);
const BUTTON_NORMAL: Color = Color::srgba(0.10, 0.12, 0.14, 0.94);
const BUTTON_HOVERED: Color = Color::srgba(0.18, 0.22, 0.24, 0.96);
const BUTTON_ACTIVE: Color = Color::srgb(0.18, 0.45, 0.55);
const BUTTON_PRESSED: Color = Color::srgb(0.22, 0.55, 0.66);

#[derive(Component)]
pub(crate) struct OpenResultButton;

#[derive(Component)]
pub(crate) struct ResultStatsText;

#[derive(Component)]
pub(crate) struct ResultSliderSection;

/// Animation playback state for automatic result step advancement.
#[derive(Resource, Debug, Clone)]
pub(crate) struct PlaybackState {
    pub playing: bool,
    /// Seconds per step (0.1 = 10fps, 0.5 = 2fps).
    pub interval: f32,
    /// Elapsed time since the last step advance.
    pub elapsed: f32,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            playing: false,
            interval: 0.2,
            elapsed: 0.0,
        }
    }
}

#[derive(Component)]
pub(crate) struct PlaybackRewindButton;

#[derive(Component)]
pub(crate) struct PlaybackPlayPauseButton;

#[derive(Component)]
pub(crate) struct PlaybackEndButton;

#[derive(Component)]
pub(crate) struct PlaybackPlayPauseLabel;

pub(crate) fn open_result_button_system(
    mut pending_path: Local<Option<std::path::PathBuf>>,
    mut buttons: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        With<OpenResultButton>,
    >,
    model: Option<Res<FemModel>>,
    mut results: ResMut<FemResultSet>,
    mut settings: ResMut<visualization::VisualizationSettings>,
    mut page: ResMut<SidebarPage>,
) {
    for (interaction, mut background, mut border) in &mut buttons {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Open result file")
                .add_filter("All result files", &["res", "frd", "vtu", "pvtu"])
                .add_filter("FrontISTR result (.res.0.*)", &["res"])
                .add_filter("CalculiX result (.frd)", &["frd"])
                .add_filter("VTK XML (.vtu / .pvtu)", &["vtu", "pvtu"])
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

    // Load on a separate branch to avoid holding rfd dialog open
    // while mutating FemResultSet.
    if let Some(path) = pending_path.take() {
        let Some(model) = model.as_deref() else {
            return;
        };
        let Some(fem_mesh) = model.meshes.first() else {
            return;
        };

        let node_ids: Vec<fem_core::NodeId> = fem_mesh.nodes.iter().map(|n| n.id).collect();

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        // Ensure by_mesh slot exists for mesh 0.
        if results.by_mesh.is_empty() {
            results.by_mesh.push(Vec::new());
        }

        let loaded_steps: Vec<fem_core::StepResult> = match ext.as_str() {
            "frd" => match hecmw::load_frd_file(&path, &node_ids) {
                Ok(steps) => steps,
                Err(err) => {
                    bevy::log::warn!("FRD load failed: {err}");
                    return;
                }
            },
            "vtu" | "pvtu" => match hecmw::load_vtu_file(&path, &node_ids) {
                Ok(step) => vec![step],
                Err(err) => {
                    bevy::log::warn!("VTU load failed: {err}");
                    return;
                }
            },
            _ => {
                // .res.0.N — auto-detect series siblings and load all steps.
                match hecmw::load_series(&path, &node_ids) {
                    Ok(steps) => steps,
                    Err(err) => {
                        bevy::log::warn!("Result series load failed: {err}");
                        return;
                    }
                }
            }
        };

        if loaded_steps.is_empty() {
            bevy::log::warn!("Result file contained no steps: {:?}", path.file_name());
            return;
        }

        let step_count = loaded_steps.len();
        results.by_mesh[0].extend(loaded_steps);
        results.activate_first();

        // Auto-activate contour.
        if let Some(active) = &results.active {
            let has_disp = results
                .by_mesh
                .get(active.mesh_index)
                .and_then(|s| s.get(active.step_index))
                .map(|s| s.field_by_name("Displacement").is_some())
                .unwrap_or(false);

            settings.contour = Some(ContourSettings {
                mesh_index: active.mesh_index,
                step_index: active.step_index,
                field_name: active.field_name.clone(),
                show_deformation: has_disp,
                displacement_field: "Displacement".to_string(),
                deformation_scale: 1.0,
            });
        }

        bevy::log::info!(
            "Loaded {step_count} result step(s) from {:?}",
            path.file_name()
        );
        // A newly loaded result is immediately visible without another
        // navigation click.
        *page = SidebarPage::Results;
    }
}

pub(crate) fn update_result_stats_text(
    results: Res<FemResultSet>,
    mut query: Query<&mut Text, With<ResultStatsText>>,
) {
    if !results.is_changed() {
        return;
    }

    let Ok(mut text) = query.single_mut() else {
        return;
    };

    **text = if !results.has_results() {
        "Result: none loaded".to_string()
    } else if let Some(field) = results.active_field() {
        match field {
            fem_core::ResultField::NodeScalar { name, min, max, .. } => {
                format!("Result: {name}\nMin: {min:.4e}  Max: {max:.4e}")
            }
            fem_core::ResultField::NodeVector {
                name,
                min_mag,
                max_mag,
                ..
            } => {
                format!("Result: {name} (magnitude)\nMin: {min_mag:.4e}  Max: {max_mag:.4e}")
            }
            fem_core::ResultField::ElementScalar { name, min, max, .. } => {
                format!("Result: {name}\nMin: {min:.4e}  Max: {max:.4e}")
            }
        }
    } else {
        let total_steps: usize = results.by_mesh.iter().map(|s| s.len()).sum();
        format!("Result: {total_steps} step(s) loaded")
    };
}

// ── animation playback ────────────────────────────────────────────────────────

pub(crate) fn playback_button_system(
    mut playback: ResMut<PlaybackState>,
    results: Option<Res<FemResultSet>>,
    mut play_btns: Query<
        (
            Ref<Interaction>,
            &mut BackgroundColor,
            &mut BorderColor,
            &Children,
        ),
        (
            With<PlaybackPlayPauseButton>,
            Without<PlaybackRewindButton>,
            Without<PlaybackEndButton>,
        ),
    >,
    mut rewind_btns: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        (
            With<PlaybackRewindButton>,
            Without<PlaybackPlayPauseButton>,
            Without<PlaybackEndButton>,
        ),
    >,
    mut end_btns: Query<
        (Ref<Interaction>, &mut BackgroundColor, &mut BorderColor),
        (
            With<PlaybackEndButton>,
            Without<PlaybackPlayPauseButton>,
            Without<PlaybackRewindButton>,
        ),
    >,
    mut labels: Query<&mut Text, With<PlaybackPlayPauseLabel>>,
    mut sliders: Query<&mut SliderState, With<SliderTrack>>,
) {
    let step_count = results
        .as_deref()
        .map(|r| r.by_mesh.iter().map(|s| s.len()).max().unwrap_or(0))
        .unwrap_or(0);

    for (interaction, mut bg, mut border, children) in &mut play_btns {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            playback.playing = !playback.playing;
            playback.elapsed = 0.0;
        }
        let active = playback.playing;
        let color = match (*interaction, active) {
            (Interaction::Pressed, _) => BUTTON_PRESSED,
            (Interaction::Hovered, true) | (Interaction::None, true) => BUTTON_ACTIVE,
            (Interaction::Hovered, false) => BUTTON_HOVERED,
            (Interaction::None, false) => BUTTON_NORMAL,
        };
        *bg = BackgroundColor(color);
        *border = BorderColor::all(PANEL_BORDER);

        for &child in children {
            if let Ok(mut t) = labels.get_mut(child) {
                **t = if playback.playing {
                    "Pause".to_string()
                } else {
                    "Play".to_string()
                };
            }
        }
    }

    for (interaction, mut bg, mut border) in &mut rewind_btns {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            playback.playing = false;
            for mut s in &mut sliders {
                if s.id == SliderId::ResultStep {
                    s.value = 0.0;
                    s.clamp_value();
                }
            }
        }
        *bg = BackgroundColor(if *interaction != Interaction::None {
            BUTTON_HOVERED
        } else {
            BUTTON_NORMAL
        });
        *border = BorderColor::all(PANEL_BORDER);
    }

    for (interaction, mut bg, mut border) in &mut end_btns {
        if *interaction == Interaction::Pressed && interaction.is_changed() {
            playback.playing = false;
            let last = (step_count.saturating_sub(1)) as f32;
            for mut s in &mut sliders {
                if s.id == SliderId::ResultStep {
                    s.value = last;
                    s.clamp_value();
                }
            }
        }
        *bg = BackgroundColor(if *interaction != Interaction::None {
            BUTTON_HOVERED
        } else {
            BUTTON_NORMAL
        });
        *border = BorderColor::all(PANEL_BORDER);
    }
}

/// Advances the result step automatically when [`PlaybackState::playing`]
/// is true, using [`PlaybackState::interval`] as the seconds-per-step.
/// Wraps back to step 0 when the last step is reached (loop mode).
pub(crate) fn playback_advance_system(
    time: Res<Time>,
    mut playback: ResMut<PlaybackState>,
    results: Option<Res<FemResultSet>>,
    mut sliders: Query<&mut SliderState, With<SliderTrack>>,
) {
    if !playback.playing {
        return;
    }

    let step_count = results
        .as_deref()
        .map(|r| r.by_mesh.iter().map(|s| s.len()).max().unwrap_or(0))
        .unwrap_or(0);
    if step_count == 0 {
        playback.playing = false;
        return;
    }

    // Read speed from slider
    let speed = sliders
        .iter()
        .find(|s| s.id == SliderId::PlaybackSpeed)
        .map(|s| s.value)
        .unwrap_or(2.0);
    playback.interval = 1.0 / speed.max(0.1);

    playback.elapsed += time.delta_secs();
    if playback.elapsed < playback.interval {
        return;
    }
    playback.elapsed = 0.0;

    for mut s in &mut sliders {
        if s.id != SliderId::ResultStep {
            continue;
        }
        let next = (s.value + 1.0) % step_count as f32;
        s.value = next;
        s.clamp_value();
    }
}

/// Moves the active result one step with the Left/Right arrow keys.

pub(crate) fn step_keyboard_navigation(
    keyboard: Res<ButtonInput<KeyCode>>,
    keyboard_state: Res<fem_core::UiKeyboardState>,
    results: Res<FemResultSet>,
    mut slider_query: Query<&mut SliderState, With<SliderTrack>>,
) {
    if keyboard_state.text_editing || !results.has_results() {
        return;
    }

    let delta = if keyboard.just_pressed(KeyCode::ArrowRight) {
        1.0
    } else if keyboard.just_pressed(KeyCode::ArrowLeft) {
        -1.0
    } else {
        return;
    };

    for mut state in &mut slider_query {
        if state.id != SliderId::ResultStep {
            continue;
        }

        let new_value = (state.value + delta).clamp(state.min, state.max);

        if (new_value - state.value).abs() > f32::EPSILON {
            state.value = new_value;
        }
    }
}

/// Reads the step slider and deform-scale slider each frame and, when either
/// has changed, updates [`FemResultSet::active`] and
/// [`VisualizationSettings::contour`] so [`update_contour_surface`] re-renders.
///
/// Also shows/hides the slider section and adjusts the step slider's max
/// to match the number of loaded steps.
pub(crate) fn apply_slider_to_results(
    mut results: ResMut<FemResultSet>,
    mut settings: ResMut<visualization::VisualizationSettings>,
    mut section_query: Query<&mut Visibility, With<ResultSliderSection>>,
    mut slider_query: Query<&mut SliderState, With<SliderTrack>>,
) {
    if !results.has_results() {
        if let Ok(mut vis) = section_query.single_mut() {
            *vis = Visibility::Hidden;
        }
        return;
    }

    // Show sliders when results are present.
    if let Ok(mut vis) = section_query.single_mut() {
        *vis = Visibility::Visible;
    }

    let mesh_index = results.active.as_ref().map(|a| a.mesh_index).unwrap_or(0);
    let step_count = results.by_mesh.get(mesh_index).map_or(0, |s| s.len());

    // Read slider values.
    let mut step_value: Option<f32> = None;
    let mut scale_value: Option<f32> = None;

    for mut state in &mut slider_query {
        match state.id {
            SliderId::ResultStep => {
                // Keep max in sync with step count.
                let new_max = (step_count.saturating_sub(1)) as f32;
                if (state.max - new_max).abs() > 0.5 {
                    state.max = new_max;
                    state.clamp_value();
                }
                step_value = Some(state.value);
            }
            SliderId::DeformScale => {
                scale_value = Some(state.value);
            }
            // These sliders are read by dedicated systems; result display doesn't need them.
            SliderId::LoadMagnitude
            | SliderId::SectionThickness
            | SliderId::SurfaceAngle
            | SliderId::DloadMagnitude
            | SliderId::PlaybackSpeed
            | SliderId::AssemblyMovePercent
            | SliderId::AssemblyRotationDegrees
            | SliderId::ContactFriction
            | SliderId::ContactPenaltyFactor
            | SliderId::ContactReviewSeparation
            | SliderId::ContactSearchGap
            | SliderId::ContactSearchAngle
            | SliderId::RigidSpiderRadius => {}
        }
    }

    let step_index = step_value.map(|v| v.round() as usize).unwrap_or(0);

    // Update active step.
    if let Some(active) = results.active.as_mut() {
        if active.step_index != step_index {
            active.step_index = step_index;
            // Signal changed so update_contour_surface re-renders.
            results.set_changed();
        }
    }

    // Update deformation scale in contour settings.
    if let Some(scale) = scale_value {
        if let Some(contour) = settings.contour.as_mut() {
            if (contour.deformation_scale - scale).abs() > 1.0e-4 {
                contour.deformation_scale = scale;
                contour.step_index = step_index;
            }
        }
    }
}
