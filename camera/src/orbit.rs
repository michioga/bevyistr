use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use fem_core::{InteractionMode, UiPointerState};

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

pub fn orbit_camera_system(
    buttons: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut motion_evr: MessageReader<MouseMotion>,
    mut wheel_evr: MessageReader<MouseWheel>,
    mut query: Query<(&mut Transform, &mut OrbitCamera)>,
    mut mode: ResMut<InteractionMode>,
    ui_pointer: Res<UiPointerState>,
) {
    let Ok((mut transform, mut orbit)) = query.single_mut() else {
        return;
    };

    let mut rotation_move = Vec2::ZERO;

    for ev in motion_evr.read() {
        rotation_move += ev.delta;
    }

    if !ui_pointer.over_ui && buttons.pressed(MouseButton::Middle) {
        let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);

        if shift {
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
        } else {
            // Orbit
            let yaw = -rotation_move.x * 0.005;
            let pitch = -rotation_move.y * 0.005;

            let rot = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch);
            let offset = transform.translation - orbit.focus;
            let rotated_offset = rot * offset;

            transform.translation = orbit.focus + rotated_offset;
            transform.look_at(orbit.focus, orbit_camera_up(rotated_offset));
        }
    }

    if !ui_pointer.over_ui && buttons.just_pressed(MouseButton::Right) {
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
        if ui_pointer.over_ui {
            continue;
        }

        let factor = (1.0 - ev.y * ZOOM_SPEED).max(0.05);

        orbit.radius = (orbit.radius * factor).clamp(orbit.min_radius, orbit.max_radius);

        let dir = (transform.translation - orbit.focus).normalize();

        transform.translation = orbit.focus + dir * orbit.radius;
    }

    orbit.focus = orbit.focus.lerp(orbit.target_focus, 0.15);

    let dir = (transform.translation - orbit.focus).normalize();

    transform.translation = orbit.focus + dir * orbit.radius;

    transform.look_at(orbit.focus, orbit_camera_up(dir));
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
        app.insert_resource(UiPointerState {
            over_ui: pointer_over_ui,
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
}
