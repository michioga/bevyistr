use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use fem_core::{InteractionMode, UiPointerState};

#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum NavigationScheme {
    #[default]
    Mouse,
    TouchpadAlt,
    TouchpadSpace,
}

impl NavigationScheme {
    fn modifier_pressed(self, keyboard: &ButtonInput<KeyCode>) -> bool {
        match self {
            Self::Mouse => false,
            Self::TouchpadAlt => keyboard.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]),
            Self::TouchpadSpace => keyboard.pressed(KeyCode::Space),
        }
    }
}

#[derive(Component)]
pub struct OrbitCamera {
    pub focus: Vec3,
    pub radius: f32,

    pub target_focus: Vec3,

    /// Smallest allowed `radius`, in world units.
    pub min_radius: f32,

    /// Largest allowed `radius`, in world units.
    pub max_radius: f32,
}

/// Computes a focus point and orbit radius that frame the given axis-aligned
/// bounding box.
///
/// Used both for the initial camera placement at startup and to recenter the
/// camera after a new mesh is loaded.
pub fn fit_bounds(min: Vec3, max: Vec3) -> (Vec3, f32) {
    let focus = (min + max) * 0.5;
    let radius = (max - min).length().max(1.0) * 1.35;

    (focus, radius)
}

/// Min/max zoom radius, scaled from the "fit" radius returned by
/// [`fit_bounds`].
///
/// Deriving the limits from `fit_radius` (rather than fixed absolute
/// numbers) keeps zooming sensible across models of very different physical
/// scale -- a small bracket modeled in meters and a car body modeled in
/// millimeters should both allow zooming in to fine mesh detail and out to
/// roughly the same multiple of their own overview distance.
pub fn radius_limits(fit_radius: f32) -> (f32, f32) {
    let min_radius = (fit_radius * 0.01).max(1.0e-3);
    let max_radius = fit_radius * 20.0;

    (min_radius, max_radius)
}

/// Chooses a stable screen-up vector for an orbit direction. Global Y is the
/// normal up direction, but top and bottom views need Z to avoid a collinear
/// `look_at` basis.
pub fn orbit_camera_up(direction_from_focus: Vec3) -> Vec3 {
    let direction = if direction_from_focus.length_squared() > 1.0e-12 {
        direction_from_focus.normalize()
    } else {
        Vec3::Z
    };
    if direction.dot(Vec3::Y).abs() > 0.98 {
        if direction.y >= 0.0 {
            Vec3::Z
        } else {
            -Vec3::Z
        }
    } else {
        Vec3::Y
    }
}

fn orbit_rotation(rotation: Quat, drag: Vec2) -> Quat {
    const RADIANS_PER_PIXEL: f32 = 0.005;

    // World Y may point into the screen. A horizontal drag must still
    // turn around screen up so the visible base of a part stays below it.
    let screen_rotation = Vec3::new(-drag.y, -drag.x, 0.0) * RADIANS_PER_PIXEL;
    (rotation * Quat::from_scaled_axis(screen_rotation)).normalize()
}

pub fn orbit_camera_system(
    buttons: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut motion_evr: MessageReader<MouseMotion>,
    mut wheel_evr: MessageReader<MouseWheel>,
    mut query: Query<(&mut Transform, &mut OrbitCamera)>,
    mut mode: ResMut<InteractionMode>,
    mut ui_pointer: ResMut<UiPointerState>,
    scheme: Res<NavigationScheme>,
    mut emulating_middle: Local<bool>,
) {
    let over_ui = ui_pointer.over_ui;
    let was_emulating = *emulating_middle;
    let starts_emulation =
        buttons.just_pressed(MouseButton::Left) && !over_ui && scheme.modifier_pressed(&keyboard);
    *emulating_middle = (starts_emulation || was_emulating) && buttons.pressed(MouseButton::Left);
    let navigating = starts_emulation || was_emulating;
    if ui_pointer.navigating != navigating {
        ui_pointer.navigating = navigating;
    }

    let Ok((mut transform, mut orbit)) = query.single_mut() else {
        return;
    };

    let mut rotation_move = Vec2::ZERO;

    for ev in motion_evr.read() {
        rotation_move += ev.delta;
    }

    if !over_ui && (buttons.pressed(MouseButton::Middle) || *emulating_middle) {
        let shift = keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        let ctrl = keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);

        if *emulating_middle && ctrl {
            let factor = (-rotation_move.y * 0.01).exp();
            orbit.radius = (orbit.radius * factor).clamp(orbit.min_radius, orbit.max_radius);
        } else if shift {
            // Pan: slide the focus (and target_focus) along the camera's
            // local right/up plane. Speed is proportional to `radius` so
            // the scene moves at roughly the same rate as the cursor
            // regardless of zoom level.
            let pan_scale = orbit.radius * 0.0012;

            let right = transform.rotation * Vec3::X;
            let up = transform.rotation * Vec3::Y;

            let delta = -right * rotation_move.x * pan_scale + up * rotation_move.y * pan_scale;

            orbit.focus += delta;
            orbit.target_focus += delta;
        } else if rotation_move != Vec2::ZERO {
            transform.rotation = orbit_rotation(transform.rotation, rotation_move);
        }
    }

    if !over_ui && buttons.just_pressed(MouseButton::Right) {
        *mode = InteractionMode::Orbit;
    }

    if buttons.just_released(MouseButton::Right) {
        *mode = InteractionMode::Idle;
    }

    // Each wheel notch changes the radius by a fixed *fraction* rather than
    // a fixed amount, so zooming feels consistent regardless of how close
    // or far the camera currently is -- and, crucially, zooming back out
    // takes roughly the same number of notches as zooming in took, instead
    // of the fixed step becoming negligible once `radius` has shrunk.
    const ZOOM_SPEED: f32 = 0.15;

    for ev in wheel_evr.read() {
        if over_ui {
            continue;
        }

        let factor = (1.0 - ev.y * ZOOM_SPEED).max(0.05);

        orbit.radius = (orbit.radius * factor).clamp(orbit.min_radius, orbit.max_radius);
    }

    orbit.focus = orbit.focus.lerp(orbit.target_focus, 0.15);
    transform.translation = orbit.focus + transform.rotation * Vec3::Z * orbit.radius;
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::mouse::MouseScrollUnit;
    use bevy::input::touch::TouchPhase;

    fn test_app(pointer_over_ui: bool) -> (App, Entity, Entity) {
        let mut app = App::new();
        app.init_resource::<ButtonInput<MouseButton>>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<InteractionMode>();
        app.init_resource::<NavigationScheme>();
        app.insert_resource(UiPointerState {
            over_ui: pointer_over_ui,
            ..default()
        });
        app.add_message::<MouseMotion>();
        app.add_message::<MouseWheel>();
        app.add_systems(Update, orbit_camera_system);

        let window = app.world_mut().spawn_empty().id();
        let camera = app
            .world_mut()
            .spawn((
                Transform::from_xyz(0.0, 0.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
                OrbitCamera {
                    focus: Vec3::ZERO,
                    radius: 10.0,
                    target_focus: Vec3::ZERO,
                    min_radius: 1.0,
                    max_radius: 100.0,
                },
            ))
            .id();

        (app, window, camera)
    }

    fn scroll(app: &mut App, window: Entity, y: f32) {
        app.world_mut()
            .resource_mut::<Messages<MouseWheel>>()
            .write(MouseWheel {
                unit: MouseScrollUnit::Line,
                x: 0.0,
                y,
                window,
                phase: TouchPhase::Moved,
            });
    }

    fn drag(app: &mut App, delta: Vec2) {
        app.world_mut()
            .write_message(MouseMotion { delta })
            .unwrap();
        app.update();
    }

    #[test]
    fn touchpad_modes_orbit_pan_and_zoom_and_own_the_left_button_release() {
        for (scheme, key) in [
            (NavigationScheme::TouchpadAlt, KeyCode::AltLeft),
            (NavigationScheme::TouchpadSpace, KeyCode::Space),
        ] {
            let (mut app, _, camera) = test_app(false);
            app.insert_resource(scheme);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Left);
            drag(&mut app, Vec2::new(30.0, 0.0));
            assert!(
                app.world()
                    .resource::<UiPointerState>()
                    .blocks_viewport_tools()
            );
            let orbit_position = app.world().get::<Transform>(camera).unwrap().translation;
            assert_ne!(orbit_position, Vec3::Z * 10.0);

            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .clear_just_pressed(MouseButton::Left);
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(key);
            keys.press(KeyCode::ShiftLeft);
            drag(&mut app, Vec2::new(30.0, 0.0));
            let orbit = app.world().get::<OrbitCamera>(camera).unwrap();
            assert!(orbit.focus.length() > 0.1);
            let pan_position = app.world().get::<Transform>(camera).unwrap().translation;
            assert!((pan_position - orbit.focus).abs_diff_eq(orbit_position, 1.0e-5));

            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::ShiftLeft);
            keys.press(KeyCode::ControlLeft);
            drag(&mut app, Vec2::new(0.0, 20.0));
            assert!(app.world().get::<OrbitCamera>(camera).unwrap().radius < 10.0);
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .release(MouseButton::Left);
            app.update();
            assert!(app.world().resource::<UiPointerState>().navigating);
            app.update();
            assert!(!app.world().resource::<UiPointerState>().navigating);
        }
    }

    #[test]
    fn mouse_mode_and_ui_origin_do_not_claim_alt_left_drag() {
        for (scheme, over_ui) in [
            (NavigationScheme::Mouse, false),
            (NavigationScheme::TouchpadAlt, true),
        ] {
            let (mut app, _, camera) = test_app(over_ui);
            app.insert_resource(scheme);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::AltLeft);
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Left);
            drag(&mut app, Vec2::new(30.0, 0.0));
            assert!(!app.world().resource::<UiPointerState>().navigating);
            app.world_mut().resource_mut::<UiPointerState>().over_ui = false;
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .clear_just_pressed(MouseButton::Left);
            drag(&mut app, Vec2::new(30.0, 0.0));
            assert_eq!(
                app.world().get::<Transform>(camera).unwrap().translation,
                Vec3::Z * 10.0
            );
        }
    }

    #[test]
    fn wheel_over_ui_is_discarded_instead_of_zooming_later() {
        let (mut app, window, camera) = test_app(true);

        scroll(&mut app, window, 1.0);
        app.update();
        assert_eq!(app.world().get::<OrbitCamera>(camera).unwrap().radius, 10.0);

        app.world_mut().resource_mut::<UiPointerState>().over_ui = false;
        app.update();
        assert_eq!(app.world().get::<OrbitCamera>(camera).unwrap().radius, 10.0);

        scroll(&mut app, window, 1.0);
        app.update();
        assert_eq!(app.world().get::<OrbitCamera>(camera).unwrap().radius, 8.5);
    }

    #[test]
    fn top_and_bottom_views_use_a_non_collinear_up_axis() {
        assert!(orbit_camera_up(Vec3::Y).dot(Vec3::Y).abs() < 1.0e-6);
        assert!(orbit_camera_up(-Vec3::Y).dot(Vec3::Y).abs() < 1.0e-6);
        assert_eq!(orbit_camera_up(Vec3::Z), Vec3::Y);
    }

    #[test]
    fn horizontal_drag_from_top_view_faces_the_side_and_preserves_screen_up() {
        let (mut app, _, camera) = test_app(false);
        let top_view = Transform::from_translation(Vec3::Y * 10.0).looking_at(Vec3::ZERO, Vec3::Z);
        *app.world_mut().get_mut::<Transform>(camera).unwrap() = top_view;
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Middle);

        let quarter_turn_pixels = std::f32::consts::FRAC_PI_2 / 0.005;
        drag(&mut app, Vec2::new(quarter_turn_pixels, 0.0));
        let turned = app.world().get::<Transform>(camera).unwrap();
        assert!((turned.rotation * Vec3::Y).abs_diff_eq(Vec3::Z, 1.0e-5));
        assert!(turned.translation.abs_diff_eq(Vec3::X * 10.0, 1.0e-5));
    }

    #[test]
    fn vertical_drag_crosses_world_poles_without_flipping_the_screen() {
        for sign in [-1.0, 1.0] {
            let near_pole = orbit_rotation(Quat::IDENTITY, Vec2::new(0.0, sign * 280.0));
            let past_pole = orbit_rotation(near_pole, Vec2::new(0.0, sign * 70.0));
            assert!((near_pole * Vec3::Z).z > 0.0);
            assert!((past_pole * Vec3::Z).z < 0.0);
            assert!((near_pole * Vec3::Y).dot(past_pole * Vec3::Y) > 0.9);
            assert!((past_pole * Vec3::X).abs_diff_eq(Vec3::X, 1.0e-5));
        }
    }
}
