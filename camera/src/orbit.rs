use bevy::camera_controller::pan_orbit_camera::controller::{
    component::{OrbitConstraint, PanOrbitCamera},
    motion::CurrentMotion,
};
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::math::DVec3;
use bevy::prelude::*;
use bevy::window::RequestRedraw;
use fem_core::UiPointerState;
use std::time::Duration;

#[derive(Component, Clone)]
#[require(PanOrbitCamera = screen_space_orbit())]
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

fn screen_space_orbit() -> PanOrbitCamera {
    let mut controller = PanOrbitCamera {
        // A horizontal drag turns about the screen's up axis even in the exact top view.
        orbit_constraint: OrbitConstraint::Free,
        ..default()
    };
    controller.smoothing.orbit = Duration::ZERO;
    controller.smoothing.pan = Duration::ZERO;
    controller
}

fn snap_view(rotation: Quat) -> Quat {
    let view = rotation * Vec3::Z;
    let mut best_dot = f32::NEG_INFINITY;
    let mut best = Vec3::Z;
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                let direction = IVec3::new(x, y, z);
                if direction != IVec3::ZERO {
                    let direction = direction.as_vec3().normalize();
                    let dot = direction.dot(view);
                    if dot > best_dot {
                        best = direction;
                        best_dot = dot;
                    }
                }
            }
        }
    }

    let heading = Transform::IDENTITY
        .looking_to(-best, orbit_camera_up(best))
        .rotation;
    if best.y.abs() < 0.999 {
        return heading;
    }
    let screen_up = rotation * Vec3::Y;
    (0..4)
        .map(|turn| heading * Quat::from_rotation_z(turn as f32 * std::f32::consts::FRAC_PI_2))
        .max_by(|a, b| {
            (*a * Vec3::Y)
                .dot(screen_up)
                .total_cmp(&(*b * Vec3::Y).dot(screen_up))
        })
        .unwrap_or(heading)
}

pub fn orbit_camera_system(
    buttons: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut motion_evr: MessageReader<MouseMotion>,
    mut wheel_evr: MessageReader<MouseWheel>,
    windows: Query<&Window>,
    mut query: Query<(
        &mut Transform,
        &mut OrbitCamera,
        &mut PanOrbitCamera,
        &Camera,
        &mut Projection,
    )>,
    mut redraw: MessageWriter<RequestRedraw>,
    mut ui_pointer: ResMut<UiPointerState>,
    mut mouse_drag: Local<Option<MouseButton>>,
    mut unsnapped_rotation: Local<Option<Quat>>,
) {
    let over_ui = ui_pointer.over_ui;
    let had_mouse_drag = mouse_drag.is_some();
    if mouse_drag.is_some_and(|button| !buttons.pressed(button)) {
        *mouse_drag = None;
    }
    if mouse_drag.is_none() && !over_ui && !buttons.pressed(MouseButton::Left) {
        *mouse_drag = [MouseButton::Right, MouseButton::Middle]
            .into_iter()
            .find(|button| buttons.just_pressed(*button) && buttons.pressed(*button));
    }
    let navigating = had_mouse_drag || mouse_drag.is_some();
    if ui_pointer.navigating != navigating {
        ui_pointer.navigating = navigating;
    }

    let Ok((mut transform, mut orbit, mut controller, camera, projection)) = query.single_mut()
    else {
        return;
    };

    let mut rotation_move = Vec2::ZERO;

    for ev in motion_evr.read() {
        rotation_move += ev.delta;
    }

    let shift = keyboard.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let pan = *mouse_drag == Some(MouseButton::Middle) && !shift;
    let snap = mouse_drag.is_some() && shift;
    if !snap {
        *unsnapped_rotation = None;
    }

    if mouse_drag.is_none() {
        if controller.current_motion.is_moving() {
            controller.current_motion = CurrentMotion::Stationary;
        }
    } else if rotation_move != Vec2::ZERO && camera.logical_viewport_size().is_some() {
        let anchor = Some(-DVec3::Z * f64::from(orbit.radius));
        if pan && !controller.current_motion.is_panning() {
            controller.start_pan(anchor);
        } else if !pan && !controller.current_motion.is_orbiting() {
            controller.start_orbit(anchor);
        }

        controller.send_screenspace_input(rotation_move);
        let rotation = unsnapped_rotation.unwrap_or(transform.rotation).as_dquat();
        let translation = orbit.focus.as_dvec3() + rotation * DVec3::Z * f64::from(orbit.radius);
        if let Some((delta, turn)) = controller.update_transform_and_projection(
            camera,
            &translation,
            &rotation,
            projection,
            &mut redraw,
            Duration::ZERO,
        ) {
            if pan {
                let delta = (rotation * delta).as_vec3();
                orbit.focus += delta;
                orbit.target_focus += delta;
            } else {
                let turned = (rotation * turn).as_quat().normalize();
                transform.rotation = if snap {
                    *unsnapped_rotation = Some(turned);
                    snap_view(turned)
                } else {
                    turned
                };
            }
        }
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

        let previous_radius = orbit.radius;
        let radius = (previous_radius * factor).clamp(orbit.min_radius, orbit.max_radius);
        let focus_normal = transform.rotation * Vec3::Z;
        let current = Transform {
            rotation: transform.rotation,
            translation: orbit.focus + focus_normal * previous_radius,
            ..default()
        };
        let cursor = windows
            .get(ev.window)
            .ok()
            .and_then(Window::cursor_position);
        if let Some(ray) = cursor.and_then(|cursor| {
            camera
                .viewport_to_world(&GlobalTransform::from(current), cursor)
                .ok()
        }) {
            // Zoom about the cursor on the plane through the current view centre.
            let distance =
                (orbit.focus - ray.origin).dot(focus_normal) / ray.direction.dot(focus_normal);
            if distance.is_finite() {
                let anchor = ray.get_point(distance);
                let delta = (anchor - orbit.focus) * (1.0 - radius / previous_radius);
                orbit.focus += delta;
                orbit.target_focus += delta;
            }
        }
        orbit.radius = radius;
    }

    orbit.focus = orbit.focus.lerp(orbit.target_focus, 0.15);
    transform.translation = orbit.focus + transform.rotation * Vec3::Z * orbit.radius;
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
    use bevy::input::mouse::MouseScrollUnit;
    use bevy::input::touch::TouchPhase;

    fn test_app(pointer_over_ui: bool) -> (App, Entity, Entity) {
        let mut app = App::new();
        app.init_resource::<ButtonInput<MouseButton>>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.insert_resource(UiPointerState {
            over_ui: pointer_over_ui,
            ..default()
        });
        app.add_message::<MouseMotion>();
        app.add_message::<MouseWheel>();
        app.add_message::<RequestRedraw>();
        app.add_systems(Update, orbit_camera_system);

        let window = app.world_mut().spawn_empty().id();
        let camera = app
            .world_mut()
            .spawn((
                Camera {
                    computed: ComputedCameraValues {
                        target_info: Some(RenderTargetInfo {
                            physical_size: UVec2::new(1280, 720),
                            scale_factor: 1.0,
                        }),
                        ..default()
                    },
                    ..default()
                },
                Projection::Perspective(PerspectiveProjection {
                    aspect_ratio: 1280.0 / 720.0,
                    ..default()
                }),
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
    fn mouse_drag_owns_input_through_release() {
        for button in [MouseButton::Right, MouseButton::Middle] {
            let (mut app, _, _) = test_app(false);
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(button);
            drag(&mut app, Vec2::new(30.0, 0.0));
            assert!(
                app.world()
                    .resource::<UiPointerState>()
                    .blocks_viewport_tools()
            );
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .release(button);
            app.update();
            assert!(
                app.world()
                    .resource::<UiPointerState>()
                    .blocks_viewport_tools()
            );
            app.update();
            assert!(
                !app.world()
                    .resource::<UiPointerState>()
                    .blocks_viewport_tools()
            );
        }
    }

    #[test]
    fn modified_left_drags_remain_available_to_selection() {
        for key in [
            KeyCode::AltLeft,
            KeyCode::Space,
            KeyCode::ShiftLeft,
            KeyCode::ControlLeft,
        ] {
            let (mut app, _, camera) = test_app(false);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Left);
            drag(&mut app, Vec2::new(30.0, 20.0));
            assert!(
                !app.world()
                    .resource::<UiPointerState>()
                    .blocks_viewport_tools()
            );
            assert_eq!(
                app.world().get::<Transform>(camera).unwrap().translation,
                Vec3::Z * 10.0
            );
        }
    }

    #[test]
    fn mouse_middle_pans_right_orbits_and_shift_snaps() {
        for (button, shift, over_ui) in [
            (MouseButton::Middle, false, false),
            (MouseButton::Right, false, false),
            (MouseButton::Right, true, false),
            (MouseButton::Middle, true, false),
            (MouseButton::Right, false, true),
        ] {
            let (mut app, _, camera) = test_app(over_ui);
            if shift {
                app.world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(KeyCode::ShiftLeft);
            }
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(button);
            drag(&mut app, Vec2::new(150.0, 0.0));
            let orbit = app.world().get::<OrbitCamera>(camera).unwrap();
            let transform = app.world().get::<Transform>(camera).unwrap();
            assert_eq!(
                app.world().resource::<UiPointerState>().navigating,
                !over_ui
            );
            if over_ui {
                assert_eq!(transform.rotation, Quat::IDENTITY);
                assert_eq!(orbit.focus, Vec3::ZERO);
            } else if button == MouseButton::Middle && !shift {
                assert_eq!(transform.rotation, Quat::IDENTITY);
                assert!(orbit.focus.length() > 0.1);
            } else {
                assert_ne!(transform.rotation, Quat::IDENTITY);
                assert_eq!(orbit.focus, Vec3::ZERO);
                if shift {
                    let direction = transform.rotation * Vec3::Z;
                    assert!(
                        Vec3::new(direction.x.abs(), direction.y, direction.z)
                            .abs_diff_eq(Vec3::new(1.0, 0.0, 1.0).normalize(), 1.0e-5,)
                    );
                }
            }
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
            .press(MouseButton::Right);

        let quarter_turn_pixels = std::f32::consts::FRAC_PI_2 / 0.005;
        drag(&mut app, Vec2::new(quarter_turn_pixels, 0.0));
        let turned = app.world().get::<Transform>(camera).unwrap();
        assert!((turned.rotation * Vec3::Y).abs_diff_eq(Vec3::Z, 1.0e-5));
        assert!(turned.translation.abs_diff_eq(Vec3::X * 10.0, 1.0e-5));
    }

    #[test]
    fn vertical_drag_crosses_world_poles_without_flipping_the_screen() {
        for sign in [-1.0, 1.0] {
            let (mut app, _, camera) = test_app(false);
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Right);
            drag(&mut app, Vec2::new(0.0, sign * 280.0));
            let near_pole = app.world().get::<Transform>(camera).unwrap().rotation;
            drag(&mut app, Vec2::new(0.0, sign * 70.0));
            let past_pole = app.world().get::<Transform>(camera).unwrap().rotation;
            assert!((near_pole * Vec3::Z).z > 0.0);
            assert!((past_pole * Vec3::Z).z < 0.0);
            assert!((near_pole * Vec3::Y).dot(past_pole * Vec3::Y) > 0.9);
            assert!((past_pole * Vec3::X).abs_diff_eq(Vec3::X, 1.0e-5));
        }
    }
}
