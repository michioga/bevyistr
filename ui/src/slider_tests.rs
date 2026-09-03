use super::*;

fn slider_app() -> (App, Entity, Entity, Entity) {
    let mut app = App::new();
    app.init_resource::<ButtonInput<MouseButton>>();
    app.add_systems(Update, update_sliders);
    let track = app
        .world_mut()
        .spawn((
            Node::default(),
            SliderTrack,
            SliderState {
                id: SliderId::AssemblyRotationDegrees,
                min: 1.0,
                max: 45.0,
                value: 5.0,
                dragging: false,
            },
            ComputedNode {
                size: Vec2::ZERO,
                inverse_scale_factor: 1.0,
                ..default()
            },
            UiGlobalTransform::default(),
        ))
        .id();
    let thumb = app
        .world_mut()
        .spawn((Node::default(), SliderThumb { track }))
        .id();
    let fill = app
        .world_mut()
        .spawn((
            Node::default(),
            SliderFill(SliderId::AssemblyRotationDegrees),
        ))
        .id();
    (app, track, thumb, fill)
}

#[test]
fn showing_or_resizing_a_slider_restores_its_thumb_without_changing_value() {
    let (mut app, track, thumb, fill) = slider_app();
    app.update();
    for width in [272.0, 0.0, 136.0, 272.0] {
        app.world_mut().get_mut::<ComputedNode>(track).unwrap().size = Vec2::new(width, TRACK_H);
        app.update();
        if width > 0.0 {
            let expected = (width - THUMB_W) * (5.0 - 1.0) / (45.0 - 1.0);
            for actual in [
                app.world().get::<Node>(thumb).unwrap().left,
                app.world().get::<Node>(fill).unwrap().width,
            ] {
                let Val::Px(actual) = actual else {
                    panic!("slider offset must be in pixels")
                };
                assert!((actual - expected).abs() < 1.0e-5);
            }
        }
        assert_eq!(app.world().get::<SliderState>(track).unwrap().value, 5.0);
    }
}

#[test]
fn hiding_a_dragged_slider_stops_it_without_altering_its_value() {
    let (mut app, track, _, _) = slider_app();
    let mut window = Window::default();
    window.set_cursor_position(Some(Vec2::new(50.0, 50.0)));
    app.world_mut().spawn(window);
    app.world_mut()
        .get_mut::<SliderState>(track)
        .unwrap()
        .dragging = true;
    app.update();
    let slider = app.world().get::<SliderState>(track).unwrap();
    assert!(!slider.dragging);
    assert_eq!(slider.value, 5.0);
}
