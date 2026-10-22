//! Seek position is an index in the loaded series, never a solver step number.
use crate::{
    results_ui::{PlaybackState, result_frame_count},
    slider::{SliderId, SliderLabelText, SliderState, SliderTrack, SliderValueText},
};
use bevy::prelude::*;
use fem_core::FemResultSet;

#[cfg(test)]
#[path = "result_seek_tests.rs"]
mod tests;

#[derive(Component)]
pub(crate) struct SourceLabel;

#[derive(Component)]
pub(crate) struct SpeedHint;

pub(crate) fn spawn_speed_hint(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        SpeedHint,
        Text::new("Frames/sec; not physical-time playback"),
        Pickable::IGNORE,
        TextFont {
            font_size: FontSize::Px(10.),
            ..default()
        },
        TextColor(Color::srgb(0.6, 0.72, 0.78)),
    ));
}

pub(crate) fn spawn_details(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        SourceLabel,
        Text::new("Click / drag to seek and pause"),
        Pickable::IGNORE,
        TextFont {
            font_size: FontSize::Px(10.),
            ..default()
        },
        TextColor(Color::srgb(0.6, 0.72, 0.78)),
    ));
}

pub(crate) fn pause_while_seeking(
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut playback: ResMut<PlaybackState>,
) {
    if sliders
        .iter()
        .any(|s| s.id == SliderId::ResultStep && s.dragging)
    {
        if playback.playing {
            playback.playing = false;
        }
        if playback.elapsed != 0. {
            playback.elapsed = 0.;
        }
    }
}

pub(crate) fn sync(
    results: Res<FemResultSet>,
    mut texts: Query<
        (
            &mut Text,
            Option<&SliderLabelText>,
            Option<&SliderValueText>,
            Has<SourceLabel>,
            Has<SpeedHint>,
        ),
        Or<(
            With<SliderLabelText>,
            With<SliderValueText>,
            With<SourceLabel>,
            With<SpeedHint>,
        )>,
    >,
) {
    let count = result_frame_count(&results);
    let active = results.active.as_ref();
    let index = active
        .map_or(0, |a| a.step_index)
        .min(count.saturating_sub(1));
    let step = results
        .by_mesh
        .get(active.map_or(0, |a| a.mesh_index))
        .and_then(|s| s.get(index));
    let modal = step.is_some_and(|s| s.eigenvalue.is_some());
    let position = if count == 0 { 0 } else { index + 1 };
    for (mut text, label, value, source, speed) in &mut texts {
        if label.is_some_and(|l| l.0 == SliderId::ResultStep) {
            text.set_if_neq(Text::new(if modal { "Mode (order)" } else { "Frame" }));
        } else if value.is_some_and(|v| v.0 == SliderId::ResultStep) {
            text.set_if_neq(Text::new(format!("{position} / {count}")));
        } else if label.is_some_and(|l| l.0 == SliderId::PlaybackSpeed) {
            text.set_if_neq(Text::new(if modal {
                "Speed (modes/sec)"
            } else {
                "Speed (frames/sec)"
            }));
        } else if speed {
            text.set_if_neq(Text::new(if modal {
                "Modes/sec; switches modes, not vibration"
            } else {
                "Frames/sec; not physical-time playback"
            }));
        } else if source {
            let detail = step.map_or_else(
                || "No result frames".into(),
                |s| {
                    format!(
                        "{}{}\n{}",
                        if modal {
                            "Order above; original mode below:\n"
                        } else {
                            ""
                        },
                        s.frame_description(),
                        if count == 1 && modal {
                            "Single mode"
                        } else if count == 1 {
                            "Single frame"
                        } else {
                            "Click / drag to seek and pause"
                        }
                    )
                },
            );
            text.set_if_neq(Text::new(detail));
        }
    }
}
