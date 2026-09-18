//! Post-processing result loading, timeline navigation, and animation UI.

use crate::slider::{SliderId, SliderState, SliderTrack};
use bevy::prelude::*;
use fem_core::FemResultSet;
#[cfg(test)]
use visualization::ContourSettings;

#[derive(Component)]
pub(crate) struct OpenResultButton;

#[derive(Component)]
pub(crate) struct ResultStatsText;

#[derive(Resource, Default)]
pub(crate) struct ResultLoadError(pub Option<String>);

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

pub(crate) use crate::playback_controls::sync as playback_button_system;

pub(crate) use crate::result_open::open_result_button_system;

pub(crate) fn update_result_stats_text(
    results: Res<FemResultSet>,
    opening: Option<Res<crate::result_open::ResultOpenState>>,
    mut load_error: ResMut<ResultLoadError>,
    mut query: Query<&mut Text, With<ResultStatsText>>,
) {
    if !results.is_changed() && !load_error.is_changed() && !opening.as_ref().is_some_and(|s|s.is_changed()) {
        return;
    }

    let Ok(mut text) = query.single_mut() else {
        return;
    };

    if results.is_changed() && !load_error.is_changed() {
        load_error.0 = None;
    }
    if let Some(error) = &load_error.0 {
        **text = format!("{error}\nPrevious results, if any, remain displayed.");
        return;
    }

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
    if let Some(value) = results.active_field().and_then(|field| field.constant_value()) {
        text.push_str(&format!("\nConstant field: {value:.4e} (uniform color)"));
    }
    if let Some(active) = &results.active {
        if let Some(steps) = results.by_mesh.get(active.mesh_index) {
            if let Some(step) = steps.get(active.step_index) {
                text.push_str(&format!("\nFrame {}/{} | Step {} | Time {:.6e}", active.step_index+1, steps.len(), step.step, step.time));
            }
        }
    }
    if let Some(opening)=opening { if !opening.status.is_empty() {text.push_str(&format!("\n{}",opening.status));} }
}

// ── animation playback ────────────────────────────────────────────────────────

/// Number of frames in the active timeline, shared by playback and navigation.
pub(crate) fn result_frame_count(results: &FemResultSet) -> usize {
    let mesh = results.active.as_ref().map_or(0, |a| a.mesh_index);
    results.by_mesh.get(mesh).map_or(0, Vec::len)
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
        .map(result_frame_count)
        .unwrap_or(0);
    if step_count < 2 {
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
        s.min = 0.0;
        s.max = step_count.saturating_sub(1) as f32;
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
    if results.active.as_ref().is_some_and(|a| a.step_index != step_index) {
        results.active.as_mut().unwrap().step_index = step_index;
    }

    // Do not mark results/settings changed on idle frames: this otherwise
    // rebuilds every part's GPU surface even when playback is stopped.
    if settings.contour.as_ref().is_some_and(|c| {
        c.step_index != step_index
            || scale_value.is_some_and(|scale| c.deformation_scale != scale)
    }) {
        let contour = settings.contour.as_mut().unwrap();
        contour.step_index = step_index;
        if let Some(scale) = scale_value {
            contour.deformation_scale = scale;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_timeline_updates_contour_and_visible_step_metadata() {
        let mut app=App::new();
        let mut results=FemResultSet::default();
        results.by_mesh=vec![vec![
            fem_core::StepResult {step:0,time:0.,fields:vec![fem_core::ResultField::NodeScalar{name:"S".into(),values:vec![0.],min:0.,max:0.}]},
            fem_core::StepResult {step:5000,time:0.005,fields:vec![fem_core::ResultField::NodeScalar{name:"S".into(),values:vec![10.],min:10.,max:10.}]},
        ]];
        results.activate_first();
        let mut settings=visualization::VisualizationSettings::default();
        settings.contour=Some(ContourSettings {mesh_index:0,step_index:0,field_name:"S".into(),show_deformation:false,displacement_field:"Displacement".into(),deformation_scale:1.});
        app.insert_resource(results).insert_resource(settings).init_resource::<ResultLoadError>()
            .add_systems(Update,(apply_slider_to_results,update_result_stats_text).chain());
        let slider=app.world_mut().spawn((SliderTrack,SliderState{id:SliderId::ResultStep,min:0.,max:1.,value:1.,dragging:false})).id();
        let label=app.world_mut().spawn((ResultStatsText,Text::default())).id();
        app.update();
        assert_eq!(app.world().resource::<FemResultSet>().active.as_ref().unwrap().step_index,1);
        assert_eq!(app.world().resource::<visualization::VisualizationSettings>().contour.as_ref().unwrap().step_index,1);
        assert!(app.world().get::<Text>(label).unwrap().contains("Frame 2/2 | Step 5000"));
        app.world_mut().get_mut::<SliderState>(slider).unwrap().value=0.;
        app.update();
        assert!(app.world().get::<Text>(label).unwrap().contains("Frame 1/2 | Step 0"));
    }
}
