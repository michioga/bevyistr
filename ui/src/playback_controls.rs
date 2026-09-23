//! Explicit first/previous/play/next/last controls. Widget activation avoids
//! treating a press passing through a closed popup as a timeline operation.
use crate::{
    results_ui::{PlaybackState, result_frame_count},
    slider::{SliderId, SliderState, SliderTrack},
};
use bevy::{
    prelude::*,
    ui::InteractionDisabled,
    ui_widgets::{Activate, Button as WidgetButton},
};
use fem_core::FemResultSet;

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Action {
    First,
    Previous,
    PlayPause,
    Next,
    Last,
}

#[derive(Component)]
pub(crate) struct PlayLabel;

pub(crate) fn spawn(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn(Node {
            width: percent(100),
            column_gap: px(4),
            margin: UiRect::top(px(6)),
            ..default()
        })
        .with_children(|row| {
            for (action, label) in [
                (Action::First, "|<"),
                (Action::Previous, "<"),
                (Action::PlayPause, "Play"),
                (Action::Next, ">"),
                (Action::Last, ">|"),
            ] {
                row.spawn((
                    Button,
                    WidgetButton,
                    action,
                    bevy::input_focus::tab_navigation::TabIndex(0),
                    Node {
                        width: if action == Action::PlayPause {
                            auto()
                        } else {
                            px(36)
                        },
                        flex_grow: if action == Action::PlayPause {
                            1.0
                        } else {
                            0.0
                        },
                        height: px(28),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(5)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.10, 0.12, 0.14)),
                    BorderColor::all(Color::srgb(0.34, 0.40, 0.44)),
                ))
                .observe(activate)
                .with_children(|button| {
                    let mut label_entity = button.spawn((
                        Text::new(label),
                        Pickable::IGNORE,
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));
                    if action == Action::PlayPause {
                        label_entity.insert(PlayLabel);
                    }
                });
            }
        });
    parent.spawn((
        Text::new(
            "|< First   < Previous   > Next   >| Last\nPrevious / Next: pause and move one frame",
        ),
        Pickable::IGNORE,
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(Color::srgb(0.6, 0.72, 0.78)),
    ));
}

fn enabled(action: Action, count: usize, frame: usize, playing: bool) -> bool {
    if count == 0 {
        return false;
    }
    match action {
        Action::PlayPause => count > 1 || playing,
        Action::First | Action::Previous => frame > 0 || playing,
        Action::Last | Action::Next => frame < count - 1 || playing,
    }
}

fn activate(
    event: On<Activate>,
    buttons: Query<&Action>,
    results: Res<FemResultSet>,
    mut playback: ResMut<PlaybackState>,
    mut sliders: Query<&mut SliderState, With<SliderTrack>>,
) {
    let Ok(&action) = buttons.get(event.entity) else {
        return;
    };
    let count = result_frame_count(&results);
    let Some(mut slider) = sliders.iter_mut().find(|s| s.id == SliderId::ResultStep) else {
        return;
    };
    let frame = (slider.value.round() as usize).min(count.saturating_sub(1));
    if !enabled(action, count, frame, playback.playing) {
        return;
    }
    playback.elapsed = 0.0;
    if action == Action::PlayPause {
        playback.playing = !playback.playing;
        return;
    }
    playback.playing = false;
    let target = match action {
        Action::First => 0,
        Action::Previous => frame.saturating_sub(1),
        Action::Next => (frame + 1).min(count - 1),
        Action::Last => count - 1,
        Action::PlayPause => unreachable!(),
    };
    // Do not clamp to the initial slider max (1) or a previous result's range.
    slider.min = 0.0;
    slider.max = (count - 1) as f32;
    slider.value = target as f32;
}

pub(crate) fn sync(
    mut commands: Commands,
    playback: Res<PlaybackState>,
    results: Option<Res<FemResultSet>>,
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut buttons: Query<(Entity, &Action, &Interaction, &mut BackgroundColor)>,
    mut labels: Query<&mut Text, With<PlayLabel>>,
) {
    let count = results.as_deref().map_or(0, result_frame_count);
    let frame = sliders
        .iter()
        .find(|s| s.id == SliderId::ResultStep)
        .map_or(0, |s| s.value.round() as usize);
    for (entity, &action, interaction, mut color) in &mut buttons {
        let available = enabled(action, count, frame, playback.playing);
        if available {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else {
            commands.entity(entity).insert(InteractionDisabled);
        }
        color.set_if_neq(BackgroundColor(if !available {
            Color::srgb(0.065, 0.075, 0.085)
        } else if *interaction == Interaction::Pressed {
            Color::srgb(0.22, 0.55, 0.66)
        } else if action == Action::PlayPause && playback.playing {
            Color::srgb(0.18, 0.45, 0.55)
        } else if *interaction == Interaction::Hovered {
            Color::srgb(0.18, 0.22, 0.24)
        } else {
            Color::srgb(0.10, 0.12, 0.14)
        }));
    }
    for mut label in &mut labels {
        label.set_if_neq(Text::new(if playback.playing { "Pause" } else { "Play" }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::results_ui::apply_slider_to_results;

    fn setup(count: usize) -> (App, Entity) {
        let mut app = App::new();
        let mut results = FemResultSet::default();
        results.by_mesh = vec![
            (0..count)
                .map(|i| fem_core::StepResult {
                    eigenvalue: None,
                    step: (i * 5000) as u32,
                    time: i as f32 * 0.25,
                    fields: vec![fem_core::ResultField::NodeScalar {
                        name: "S".into(),
                        values: vec![i as f32],
                        min: i as f32,
                        max: i as f32,
                    }],
                })
                .collect(),
        ];
        results.activate_first();
        app.add_plugins(bevy::ui_widgets::ButtonPlugin)
            .insert_resource(results)
            .init_resource::<PlaybackState>()
            .init_resource::<visualization::VisualizationSettings>()
            .add_systems(Startup, |mut commands: Commands| {
                commands.spawn(Node::default()).with_children(spawn);
            })
            .add_systems(Update, (apply_slider_to_results, sync).chain());
        let slider = app
            .world_mut()
            .spawn((
                SliderTrack,
                SliderState {
                    id: SliderId::ResultStep,
                    value: 0.,
                    min: 0.,
                    max: 1.,
                    dragging: false,
                },
            ))
            .id();
        app.update();
        (app, slider)
    }

    fn click(app: &mut App, action: Action) {
        let entity = app
            .world_mut()
            .query::<(Entity, &Action)>()
            .iter(app.world())
            .find(|(_, a)| **a == action)
            .unwrap()
            .0;
        crate::widget_test_input::click(app.world_mut(), entity);
        app.update();
    }

    #[test]
    fn next_previous_walk_all_six_sparse_frames_and_end_reaches_last() {
        let (mut app, slider) = setup(6);
        for expected in 1..6 {
            click(&mut app, Action::Next);
            let results = app.world().resource::<FemResultSet>();
            assert_eq!(results.active.as_ref().unwrap().step_index, expected);
            assert_eq!(results.by_mesh[0][expected].step, expected as u32 * 5000);
        }
        click(&mut app, Action::Next);
        assert_eq!(app.world().get::<SliderState>(slider).unwrap().value, 5.);
        for expected in (0..5).rev() {
            click(&mut app, Action::Previous);
            assert_eq!(
                app.world().get::<SliderState>(slider).unwrap().value,
                expected as f32
            );
        }
        app.world_mut().get_mut::<SliderState>(slider).unwrap().max = 1.;
        click(&mut app, Action::Last);
        assert_eq!(app.world().get::<SliderState>(slider).unwrap().value, 5.);
        click(&mut app, Action::First);
        assert_eq!(app.world().get::<SliderState>(slider).unwrap().value, 0.);
    }

    #[test]
    fn play_uses_the_same_active_timeline_as_manual_navigation() {
        let (mut app, slider) = setup(6);
        // An unrelated longer series must not change this timeline's limits.
        app.world_mut()
            .resource_mut::<FemResultSet>()
            .by_mesh
            .push(vec![fem_core::StepResult::default(); 10]);
        app.init_resource::<Time>().add_systems(
            Update,
            crate::results_ui::playback_advance_system.before(apply_slider_to_results),
        );
        click(&mut app, Action::PlayPause);
        for expected in [1., 2., 3., 4., 5., 0.] {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_millis(500));
            app.update();
            assert_eq!(
                app.world().get::<SliderState>(slider).unwrap().value,
                expected
            );
            assert_eq!(
                app.world()
                    .resource::<FemResultSet>()
                    .active
                    .as_ref()
                    .unwrap()
                    .step_index,
                expected as usize
            );
        }
        click(&mut app, Action::Last);
        assert_eq!(app.world().get::<SliderState>(slider).unwrap().value, 5.);
    }

    #[test]
    fn manual_step_stops_playback_and_empty_or_single_frame_cannot_play() {
        let (mut app, slider) = setup(6);
        click(&mut app, Action::PlayPause);
        assert!(app.world().resource::<PlaybackState>().playing);
        app.world_mut().resource_mut::<PlaybackState>().elapsed = 0.4;
        click(&mut app, Action::Next);
        assert!(!app.world().resource::<PlaybackState>().playing);
        assert_eq!(app.world().resource::<PlaybackState>().elapsed, 0.);
        assert_eq!(app.world().get::<SliderState>(slider).unwrap().value, 1.);
        for count in [0, 1] {
            let (mut app, slider) = setup(count);
            click(&mut app, Action::PlayPause);
            click(&mut app, Action::Next);
            click(&mut app, Action::Last);
            assert!(!app.world().resource::<PlaybackState>().playing);
            assert_eq!(app.world().get::<SliderState>(slider).unwrap().value, 0.);
        }
    }
}
