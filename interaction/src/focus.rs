use bevy::prelude::*;

use camera::OrbitCamera;

pub fn focus_selected_system(
    keyboard: Res<ButtonInput<KeyCode>>,

    selected_query: Query<(&GlobalTransform, Option<&Name>)>,

    mut camera_query: Query<&mut OrbitCamera>,
) {
    if !keyboard.just_pressed(KeyCode::KeyF) {
        return;
    }

    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    let mut count = 0usize;

    for (transform, name) in selected_query.iter() {
        if name.is_none() {
            continue;
        }

        let pos = transform.translation();

        min = min.min(pos);
        max = max.max(pos);
        count += 1;
    }

    if count == 0 {
        return;
    }

    let Ok(mut orbit) = camera_query.single_mut() else {
        return;
    };

    let (focus, radius) = camera::fit_bounds(min, max);
    let (min_radius, max_radius) = camera::radius_limits(radius);

    orbit.target_focus = focus;

    // Only shrink the radius if it currently lies outside the newly-computed
    // window (i.e. keep the zoom level if already within sensible range so
    // a quick re-focus doesn't jolt the camera).
    if orbit.radius > max_radius || orbit.radius < min_radius {
        orbit.radius = radius;
    }

    orbit.min_radius = min_radius;
    orbit.max_radius = max_radius;
}
