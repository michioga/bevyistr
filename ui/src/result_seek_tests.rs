use super::*;
use crate::{results_ui, slider};
use fem_core::{ResultField, StepResult};
use std::time::Duration;

fn setup(count: usize, modal: bool, scale: f32) -> (App, Entity, Entity) {
    let mut results = FemResultSet::default();
    results.by_mesh = vec![
        (0..count)
            .map(|i| StepResult {
                step: if modal { [1, 3, 7][i] } else { i as u32 * 5000 },
                time: i as f32 * 0.25,
                eigenvalue: modal.then_some(100. + i as f64),
                fields: vec![ResultField::NodeScalar {
                    name: "P".into(),
                    values: vec![i as f32],
                    min: i as f32,
                    max: i as f32,
                }],
            })
            .collect(),
    ];
    results.activate_first();
    let mut app = App::new();
    app.insert_resource(results)
        .init_resource::<PlaybackState>()
        .init_resource::<Time>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<visualization::VisualizationSettings>()
        .add_systems(Startup, |mut commands: Commands| {
            commands.spawn(Node::default()).with_children(|parent| {
                slider::spawn_slider(
                    parent,
                    slider::SliderConfig {
                        id: SliderId::ResultStep,
                        label: "Frame",
                        width: 272.,
                        min: 0.,
                        max: 0.,
                        value: 0.,
                    },
                );
                spawn_details(parent);
                spawn_speed_hint(parent);
            });
        })
        .add_systems(
            Update,
            (
                slider::update_sliders,
                pause_while_seeking,
                results_ui::playback_advance_system,
                results_ui::apply_slider_to_results,
                sync,
            )
                .chain(),
        )
        .add_systems(PostUpdate, slider::sync_slider_visuals);
    let window = app.world_mut().spawn(Window::default()).id();
    app.update();
    let track = app
        .world_mut()
        .query_filtered::<Entity, With<SliderTrack>>()
        .single(app.world())
        .unwrap();
    app.world_mut().entity_mut(track).insert((
        ComputedNode {
            size: Vec2::new(272., 6.) / scale,
            inverse_scale_factor: scale,
            ..default()
        },
        UiGlobalTransform::from(bevy::math::Affine2::from_translation(
            Vec2::new(150., 100.) / scale,
        )),
    ));
    app.update();
    (app, track, window)
}

fn value(app: &mut App) -> String {
    app.world_mut()
        .query_filtered::<&Text, With<SliderValueText>>()
        .single(app.world())
        .unwrap()
        .0
        .clone()
}

fn source(app: &mut App) -> String {
    app.world_mut()
        .query_filtered::<&Text, With<SourceLabel>>()
        .single(app.world())
        .unwrap()
        .0
        .clone()
}

fn title(app: &mut App) -> String {
    app.world_mut()
        .query_filtered::<&Text, With<SliderLabelText>>()
        .single(app.world())
        .unwrap()
        .0
        .clone()
}

fn assert_thumb(app: &mut App, expected: f32) {
    let node = app
        .world_mut()
        .query_filtered::<&Node, With<slider::SliderThumb>>()
        .single(app.world())
        .unwrap();
    assert_eq!(node.left, px(expected));
    let fill = app
        .world_mut()
        .query_filtered::<&Node, With<slider::SliderFill>>()
        .single(app.world())
        .unwrap();
    assert_eq!(fill.width, px(expected));
}

#[test]
fn count_and_source_step_are_distinct_for_sparse_empty_and_single_series() {
    for count in [0, 1, 3] {
        let (mut app, track, _) = setup(count, false, 1.);
        assert_eq!(title(&mut app), "Frame");
        assert_eq!(
            value(&mut app),
            if count == 0 {
                "0 / 0"
            } else if count == 1 {
                "1 / 1"
            } else {
                "1 / 3"
            }
        );
        if count > 0 {
            app.world_mut().get_mut::<SliderState>(track).unwrap().value = (count - 1) as f32;
            app.update();
            assert_eq!(value(&mut app), format!("{count} / {count}"));
            assert!(source(&mut app).contains(&format!("Step {} | Time", (count - 1) * 5000)));
        }
        if count == 1 {
            assert!(source(&mut app).contains("Single frame"));
        }
    }
}

#[test]
fn modal_seek_shows_order_and_preserves_the_original_mode_number() {
    let (mut app, track, _) = setup(3, true, 1.);
    app.world_mut().get_mut::<SliderState>(track).unwrap().value = 2.;
    app.update();
    assert_eq!(title(&mut app), "Mode (order)");
    assert_eq!(value(&mut app), "3 / 3");
    assert!(source(&mut app).contains("Mode 7 | Eigenvalue"));
    assert!(source(&mut app).contains("Order above; original mode below"));
    assert!(
        app.world_mut()
            .query_filtered::<&Text, With<SpeedHint>>()
            .single(app.world())
            .unwrap()
            .0
            .contains("not vibration")
    );
    assert!(!source(&mut app).contains("Time"));
    assert_thumb(&mut app, 260.);
    // A shorter replacement series must reset both count and position.
    {
        let mut results = app.world_mut().resource_mut::<FemResultSet>();
        results.by_mesh[0].truncate(1);
        results.by_mesh[0][0].eigenvalue = None;
        results.activate_first();
    }
    app.update();
    assert_eq!(title(&mut app), "Frame");
    assert_eq!(value(&mut app), "1 / 1");
    assert!(
        app.world_mut()
            .query_filtered::<&Text, With<SpeedHint>>()
            .single(app.world())
            .unwrap()
            .0
            .contains("not physical-time")
    );
    assert_thumb(&mut app, 0.);
}

#[test]
fn dragging_pauses_snaps_and_syncs_the_thumb_on_the_same_playback_frame() {
    for scale in [1., 0.5] {
        let (mut app, track, window) = setup(3, false, scale);
        app.world_mut().resource_mut::<PlaybackState>().playing = true;
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs(1));
        // y=108 is within the thumb height, but outside the thin fill strip.
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_cursor_position(Some(Vec2::new(180., 108.)));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        assert!(!app.world().resource::<PlaybackState>().playing);
        assert_eq!(app.world().resource::<PlaybackState>().elapsed, 0.);
        assert_eq!(app.world().get::<SliderState>(track).unwrap().value, 1.);
        assert_eq!(value(&mut app), "2 / 3");
        assert_thumb(&mut app, 130.);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_cursor_position(Some(Vec2::new(280., 108.)));
        app.update();
        assert_eq!(value(&mut app), "3 / 3");
        assert_thumb(&mut app, 260.);
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_cursor_position(None);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.update();
        assert!(!app.world().get::<SliderState>(track).unwrap().dragging);
        app.world_mut().resource_mut::<PlaybackState>().playing = true;
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(500));
        app.update();
        assert_eq!(value(&mut app), "1 / 3");
        assert_thumb(&mut app, 0.);
    }
}

#[test]
fn popup_click_does_not_seek_the_track_behind_it() {
    let (mut app, track, window) = setup(3, false, 1.);
    app.world_mut()
        .spawn(bevy::ui_widgets::MenuPopup::default());
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(Vec2::new(280., 100.)));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert!(!app.world().get::<SliderState>(track).unwrap().dragging);
    assert_eq!(value(&mut app), "1 / 3");
}
