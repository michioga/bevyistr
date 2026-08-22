use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use fem_core::InteractionMode;

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

pub fn orbit_camera_system(
    buttons: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut motion_evr: MessageReader<MouseMotion>,
    mut wheel_evr: MessageReader<MouseWheel>,
    mut query: Query<(&mut Transform, &mut OrbitCamera)>,
    mut mode: ResMut<InteractionMode>,
) {
    let Ok((mut transform, mut orbit)) = query.single_mut() else {
        return;
    };

    let mut rotation_move = Vec2::ZERO;

    for ev in motion_evr.read() {
        rotation_move += ev.delta;
    }

    if buttons.pressed(MouseButton::Middle) {
        let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);

        if shift {
            // Pan: slide the focus (and target_focus) along the camera's
            // local right/up plane. Speed is proportional to `radius` so
            // the scene moves at roughly the same rate as the cursor
            // regardless of zoom level.
            let pan_scale = orbit.radius * 0.0012;

            let right = transform.rotation * Vec3::X;
            let up    = transform.rotation * Vec3::Y;

            let delta = -right * rotation_move.x * pan_scale
                         + up  * rotation_move.y * pan_scale;

            orbit.focus        += delta;
            orbit.target_focus += delta;
        } else {
            // Orbit
            let yaw   = -rotation_move.x * 0.005;
            let pitch = -rotation_move.y * 0.005;

            let rot = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch);
            let offset = transform.translation - orbit.focus;

            transform.translation = orbit.focus + rot * offset;
            transform.look_at(orbit.focus, Vec3::Y);
        }
    }

    if buttons.just_pressed(MouseButton::Right) {
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
        let factor = (1.0 - ev.y * ZOOM_SPEED).max(0.05);

        orbit.radius = (orbit.radius * factor).clamp(orbit.min_radius, orbit.max_radius);

        let dir = (transform.translation - orbit.focus).normalize();

        transform.translation = orbit.focus + dir * orbit.radius;
    }

    orbit.focus = orbit.focus.lerp(orbit.target_focus, 0.15);

    let dir = (transform.translation - orbit.focus).normalize();

    transform.translation = orbit.focus + dir * orbit.radius;

    transform.look_at(orbit.focus, Vec3::Y);
}
