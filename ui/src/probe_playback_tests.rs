//! Headless integration of pointer activation, playback, comparison and history.
//! This exercises UI state, not GPU rendering or native window interaction.
use super::*;
use crate::{
    playback_controls::{self, Action as PlaybackAction},
    probe_comparison::{self, Action as CompareAction, Comparison},
    results_ui::{PlaybackState, apply_slider_to_results, playback_advance_system},
    slider::{SliderId, SliderState, SliderTrack},
};
use fem_core::{FemModelVersion, NodeId, ResultField, ResultGeometry};
use std::time::Duration;

fn target(index: usize) -> Target {
    Target::Node {
        index,
        id: NodeId(100 + index as u32),
    }
}

fn setup() -> App {
    let mut results = FemResultSet::default();
    results.by_mesh = vec![
        [0., 0.25, 1.]
            .into_iter()
            .enumerate()
            .map(|(frame, time)| StepResult {
                step: frame as u32 * 5000,
                time,
                fields: vec![
                    ResultField::NodeScalar {
                        name: "P".into(),
                        values: vec![frame as f32, 10. + frame as f32],
                        min: frame as f32,
                        max: 10. + frame as f32,
                    },
                    ResultField::NodeScalar {
                        name: "Q".into(),
                        values: vec![20. + frame as f32, 30. + frame as f32],
                        min: 20. + frame as f32,
                        max: 30. + frame as f32,
                    },
                ],
                ..default()
            })
            .collect(),
    ];
    results.activate_first();
    let mut app = App::new();
    app.add_plugins(bevy::ui_widgets::ButtonPlugin)
        .insert_resource(SidebarPage::Results)
        .insert_resource(results)
        .insert_resource(VisualizationSettings {
            contour: Some(visualization::ContourSettings {
                field_name: "P".into(),
                mesh_index: 0,
                step_index: 0,
                show_deformation: false,
                displacement_field: "U".into(),
                deformation_scale: 1.,
            }),
            ..default()
        })
        .init_resource::<Time>()
        .init_resource::<PlaybackState>()
        .init_resource::<ProbePin>()
        .init_resource::<Comparison>()
        .init_resource::<ResultGeometry>()
        .init_resource::<FemModelVersion>()
        .init_resource::<HistoryCache>()
        .init_resource::<Assets<Image>>()
        .add_systems(Startup, |mut commands: Commands| {
            commands.spawn(Node::default()).with_children(|parent| {
                probe_comparison::spawn(parent);
                spawn(parent);
                playback_controls::spawn(parent);
            });
            commands.spawn((
                SliderTrack,
                SliderState {
                    id: SliderId::ResultStep,
                    value: 0.,
                    min: 0.,
                    max: 1.,
                    dragging: false,
                },
            ));
        })
        .add_systems(
            Update,
            (
                playback_advance_system,
                apply_slider_to_results,
                playback_controls::sync,
                probe_comparison::update,
                update,
            )
                .chain(),
        );
    app.update();
    app
}

fn click<T: Component>(app: &mut App, matches: impl Fn(&T) -> bool) {
    let entity = app
        .world_mut()
        .query::<(Entity, &T)>()
        .iter(app.world())
        .find(|(_, action)| matches(action))
        .unwrap()
        .0;
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::ZERO);
    crate::widget_test_input::click(app.world_mut(), entity);
    app.update();
}

fn add(app: &mut App, index: usize) {
    app.insert_resource(ProbePin::for_test(0, target(index)));
    // Pinning enables the Add widget on the next UI update, before its click.
    app.update();
    click::<CompareAction>(app, |a| matches!(a, CompareAction::Add));
}

fn assert_frame(app: &mut App, frame: usize) {
    assert_eq!(
        app.world()
            .resource::<FemResultSet>()
            .active
            .as_ref()
            .unwrap()
            .step_index,
        frame
    );
    let cache = app.world().resource::<HistoryCache>();
    let expected = marker_position(&Marker::Line, cache.history.as_ref().unwrap(), frame).unwrap();
    let mut query = app.world_mut().query::<(&Marker, &Node)>();
    let (_, line) = query
        .iter(app.world())
        .find(|(m, _)| matches!(m, Marker::Line))
        .unwrap();
    assert_eq!(line.display, Display::Flex);
    assert_eq!(line.left, percent(expected.x * 100.));
    assert!(expected.x > 0. && expected.x < 1.);
    // The comparison list must show this frame's values, not the previous frame.
    let value = format!("{:.6e}", 10. + frame as f32);
    assert!(
        app.world_mut()
            .query_filtered::<&Text, With<probe_comparison::RowText>>()
            .iter(app.world())
            .any(|text| text.contains("Node 101") && text.ends_with(&value))
    );
}

#[test]
fn playback_and_manual_navigation_keep_comparison_values_marker_and_cache_in_sync() {
    let mut app = setup();
    add(&mut app, 0);
    add(&mut app, 1);
    let entries = app.world().resource::<Comparison>().entries.clone();
    let builds = app.world().resource::<HistoryCache>().builds;
    assert_eq!(
        app.world()
            .resource::<HistoryCache>()
            .history
            .as_ref()
            .unwrap()
            .range,
        Some((0., 12.))
    );

    click::<PlaybackAction>(&mut app, |a| *a == PlaybackAction::PlayPause);
    for frame in [1, 2, 0] {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(500));
        app.update();
        assert_frame(&mut app, frame);
        assert_eq!(app.world().resource::<Comparison>().entries, entries);
        assert_eq!(app.world().resource::<HistoryCache>().builds, builds);
    }
    click::<PlaybackAction>(&mut app, |a| *a == PlaybackAction::Last);
    assert!(!app.world().resource::<PlaybackState>().playing);
    assert_frame(&mut app, 2);
    click::<PlaybackAction>(&mut app, |a| *a == PlaybackAction::Previous);
    assert_frame(&mut app, 1);
    assert_eq!(app.world().resource::<HistoryCache>().builds, builds);

    click::<CompareAction>(&mut app, |a| matches!(a, CompareAction::Remove(0)));
    assert_eq!(app.world().resource::<Comparison>().entries, entries[1..]);
    assert_eq!(
        app.world()
            .resource::<HistoryCache>()
            .history
            .as_ref()
            .unwrap()
            .range,
        Some((10., 12.))
    );
    assert_eq!(
        app.world().resource::<ProbePin>().selection().unwrap().2,
        target(1)
    );
    let cache = app.world().resource::<HistoryCache>();
    let pixels = app
        .world()
        .resource::<Assets<Image>>()
        .get(cache.image.as_ref().unwrap())
        .unwrap()
        .data
        .as_ref()
        .unwrap();
    assert!(
        pixels
            .chunks_exact(4)
            .any(|p| p == probe_comparison::COLORS[1])
    );
    assert!(
        !pixels
            .chunks_exact(4)
            .any(|p| p == probe_comparison::COLORS[0])
    );
    assert_eq!(app.world().resource::<Assets<Image>>().len(), 1);
}

#[test]
fn field_switch_and_clear_return_to_current_pin_without_mixing_histories() {
    let mut app = setup();
    add(&mut app, 0);
    add(&mut app, 1);
    let entries = app.world().resource::<Comparison>().entries.clone();
    app.world_mut()
        .resource_mut::<VisualizationSettings>()
        .contour
        .as_mut()
        .unwrap()
        .field_name = "Q".into();
    app.update();
    assert_eq!(app.world().resource::<Comparison>().entries, entries);
    assert_eq!(
        app.world()
            .resource::<HistoryCache>()
            .history
            .as_ref()
            .unwrap()
            .range,
        Some((20., 32.))
    );
    app.world_mut().resource_mut::<ProbePin>().clear();
    app.update();
    assert_eq!(app.world().resource::<Comparison>().entries, entries);
    app.insert_resource(ProbePin::for_test(0, target(1)));
    click::<CompareAction>(&mut app, |a| matches!(a, CompareAction::Clear));
    assert!(app.world().resource::<Comparison>().entries.is_empty());
    let cache = app.world().resource::<HistoryCache>();
    assert_eq!(
        cache.history.as_ref().unwrap().values,
        vec![Some(30.), Some(31.), Some(32.)]
    );
    assert!(cache.others.is_empty());
    assert_eq!(app.world().resource::<Assets<Image>>().len(), 1);
    app.insert_resource(SidebarPage::Model);
    app.update();
    assert!(app.world().resource::<HistoryCache>().history.is_none());
    assert_eq!(
        app.world_mut()
            .query_filtered::<&Node, With<HistoryRoot>>()
            .single(app.world())
            .unwrap()
            .display,
        Display::None
    );
}
