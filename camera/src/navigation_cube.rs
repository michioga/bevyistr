//! Always-available camera orientation control for the engineering viewport.
//!
//! The cube is rendered by a small camera in the upper-right corner. Its six
//! faces, twelve edges, and eight corners map to the 26 axis-aligned viewing
//! directions, while the main orbit focus and distance remain unchanged.

use bevy::camera::{ScalingMode, Viewport, visibility::RenderLayers};
use bevy::math::primitives::Cuboid;
use bevy::mesh::Mesh3d;
use bevy::pbr::MeshMaterial3d;
use bevy::prelude::*;
use fem_core::MainViewportCamera;

use crate::{OrbitCamera, orbit_camera_up};

const NAVIGATION_CUBE_RENDER_LAYER: usize = 29;
const CUBE_HALF_EXTENT: f32 = 0.86;
const CUBE_CAMERA_DISTANCE: f32 = 4.0;
const CUBE_VIEW_HEIGHT: f32 = 4.0;
const CUBE_SIZE_LOGICAL: f32 = 116.0;
const CUBE_RIGHT_LOGICAL: f32 = 20.0;
const CUBE_TOP_LOGICAL: f32 = 18.0;
const SNAP_ANIMATION_SECONDS: f32 = 0.22;
const EDGE_ZONE_THRESHOLD: f32 = 0.58;
type MainCameraFilter = (With<MainViewportCamera>, Without<NavigationCubeCamera>);
type NavigationCameraFilter = (With<NavigationCubeCamera>, Without<MainViewportCamera>);

#[derive(Component)]
pub(crate) struct NavigationCubeCamera;

#[derive(Component)]
pub(crate) struct NavigationCubeHitArea;

#[derive(Component)]
pub(crate) struct NavigationCubeLabel;

#[derive(Component)]
pub(crate) struct NavigationCubePiece {
    direction: IVec3,
    normal_material: Handle<StandardMaterial>,
    hover_material: Handle<StandardMaterial>,
}

#[derive(Debug, Clone, Copy)]
struct ViewAnimation {
    start_direction: Vec3,
    target_direction: Vec3,
    elapsed: f32,
}

#[derive(Resource, Debug, Default)]
pub(crate) struct NavigationCubeState {
    hovered: Option<IVec3>,
    animation: Option<ViewAnimation>,
}

pub(crate) fn spawn_navigation_cube(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 20,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: CUBE_VIEW_HEIGHT,
            },
            near: 0.1,
            far: 20.0,
            ..OrthographicProjection::default_3d()
        }),
        RenderLayers::layer(NAVIGATION_CUBE_RENDER_LAYER),
        Transform::from_xyz(2.5, 2.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
        NavigationCubeCamera,
        Name::new("Navigation Cube camera"),
    ));

    let base_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.14, 0.17, 0.19),
        metallic: 0.05,
        perceptual_roughness: 0.82,
        unlit: true,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.54, 1.54, 1.54))),
        MeshMaterial3d(base_material),
        RenderLayers::layer(NAVIGATION_CUBE_RENDER_LAYER),
        Name::new("Navigation Cube body"),
    ));

    let hover_material = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.78, 0.18),
        emissive: LinearRgba::rgb(0.55, 0.32, 0.04),
        unlit: true,
        ..default()
    });

    let face_x = meshes.add(Cuboid::new(0.055, 1.34, 1.34));
    let face_y = meshes.add(Cuboid::new(1.34, 0.055, 1.34));
    let face_z = meshes.add(Cuboid::new(1.34, 1.34, 0.055));
    for (direction, mesh, color) in [
        (IVec3::X, face_x.clone(), Color::srgb(0.86, 0.18, 0.20)),
        (-IVec3::X, face_x, Color::srgb(0.48, 0.11, 0.13)),
        (IVec3::Y, face_y.clone(), Color::srgb(0.20, 0.76, 0.30)),
        (-IVec3::Y, face_y, Color::srgb(0.11, 0.42, 0.18)),
        (IVec3::Z, face_z.clone(), Color::srgb(0.18, 0.43, 0.92)),
        (-IVec3::Z, face_z, Color::srgb(0.10, 0.23, 0.52)),
    ] {
        let material = materials.add(StandardMaterial {
            base_color: color,
            unlit: true,
            ..default()
        });
        spawn_cube_piece(
            &mut commands,
            mesh,
            Transform::from_translation(direction.as_vec3() * 0.79),
            direction,
            material,
            hover_material.clone(),
            "face",
        );
    }

    let edge_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.64, 0.69, 0.72),
        emissive: LinearRgba::rgb(0.06, 0.07, 0.08),
        unlit: true,
        ..default()
    });
    let edge_x = meshes.add(Cuboid::new(1.42, 0.10, 0.10));
    let edge_y = meshes.add(Cuboid::new(0.10, 1.42, 0.10));
    let edge_z = meshes.add(Cuboid::new(0.10, 0.10, 1.42));
    for a in [-1, 1] {
        for b in [-1, 1] {
            for (direction, mesh) in [
                (IVec3::new(0, a, b), edge_x.clone()),
                (IVec3::new(a, 0, b), edge_y.clone()),
                (IVec3::new(a, b, 0), edge_z.clone()),
            ] {
                spawn_cube_piece(
                    &mut commands,
                    mesh,
                    Transform::from_translation(direction.as_vec3() * 0.79),
                    direction,
                    edge_material.clone(),
                    hover_material.clone(),
                    "edge",
                );
            }
        }
    }

    let corner_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.82, 0.85, 0.87),
        emissive: LinearRgba::rgb(0.10, 0.11, 0.12),
        unlit: true,
        ..default()
    });
    let corner_mesh = meshes.add(Cuboid::new(0.16, 0.16, 0.16));
    for x in [-1, 1] {
        for y in [-1, 1] {
            for z in [-1, 1] {
                let direction = IVec3::new(x, y, z);
                spawn_cube_piece(
                    &mut commands,
                    corner_mesh.clone(),
                    Transform::from_translation(direction.as_vec3() * 0.79),
                    direction,
                    corner_material.clone(),
                    hover_material.clone(),
                    "corner",
                );
            }
        }
    }

    commands
        .spawn((
            Button,
            Node {
                position_type: PositionType::Absolute,
                right: px(12.0),
                top: px(12.0),
                width: px(132.0),
                height: px(146.0),
                padding: UiRect::bottom(px(5.0)),
                justify_content: JustifyContent::FlexEnd,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(8.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.025, 0.035, 0.045, 0.28)),
            BorderColor::all(Color::srgba(0.40, 0.55, 0.64, 0.72)),
            GlobalZIndex(80),
            NavigationCubeHitArea,
            Name::new("Navigation Cube input area"),
        ))
        .with_child((
            Text::new("VIEW • face / edge / corner"),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(Color::srgb(0.72, 0.82, 0.88)),
            NavigationCubeLabel,
        ));
}

fn spawn_cube_piece(
    commands: &mut Commands,
    mesh: Handle<Mesh>,
    transform: Transform,
    direction: IVec3,
    normal_material: Handle<StandardMaterial>,
    hover_material: Handle<StandardMaterial>,
    kind: &str,
) {
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(normal_material.clone()),
        RenderLayers::layer(NAVIGATION_CUBE_RENDER_LAYER),
        transform,
        NavigationCubePiece {
            direction,
            normal_material,
            hover_material,
        },
        Name::new(format!(
            "Navigation Cube {kind} {}",
            direction_label(direction)
        )),
    ));
}

pub(crate) fn update_navigation_cube_viewport(
    windows: Query<&Window>,
    mut cameras: Query<&mut Camera, With<NavigationCubeCamera>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok(mut camera) = cameras.single_mut() else {
        return;
    };

    let scale = window.scale_factor();
    let logical_x = (window.width() - CUBE_RIGHT_LOGICAL - CUBE_SIZE_LOGICAL).max(0.0);
    let logical_y = CUBE_TOP_LOGICAL.max(0.0);
    let physical_position = UVec2::new(
        (logical_x * scale).round() as u32,
        (logical_y * scale).round() as u32,
    );
    let physical_size = UVec2::splat((CUBE_SIZE_LOGICAL * scale).round().max(1.0) as u32);
    let viewport = Viewport {
        physical_position,
        physical_size,
        ..default()
    };
    if camera.viewport.as_ref().is_none_or(|current| {
        current.physical_position != viewport.physical_position
            || current.physical_size != viewport.physical_size
    }) {
        camera.viewport = Some(viewport);
    }
}

pub(crate) fn navigation_cube_input_system(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    hit_areas: Query<&Interaction, With<NavigationCubeHitArea>>,
    nav_camera: Query<(&Camera, &GlobalTransform), With<NavigationCubeCamera>>,
    main_camera: Query<(&Transform, &OrbitCamera), With<MainViewportCamera>>,
    mut state: ResMut<NavigationCubeState>,
) {
    let pointer_is_over = hit_areas
        .single()
        .is_ok_and(|interaction| *interaction != Interaction::None);
    if !pointer_is_over {
        state.hovered = None;
        return;
    }

    let Some(cursor) = windows.single().ok().and_then(Window::cursor_position) else {
        state.hovered = None;
        return;
    };
    let Ok((camera, camera_transform)) = nav_camera.single() else {
        state.hovered = None;
        return;
    };
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        state.hovered = None;
        return;
    };
    let Some(hit) = ray_box_hit(ray.origin, *ray.direction, CUBE_HALF_EXTENT) else {
        state.hovered = None;
        return;
    };
    let direction = snap_direction_from_cube_hit(hit, CUBE_HALF_EXTENT);
    state.hovered = Some(direction);

    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Ok((transform, orbit)) = main_camera.single() else {
        return;
    };
    let start_direction = normalized_or(
        transform.translation - orbit.focus,
        direction.as_vec3().normalize(),
    );
    state.animation = Some(ViewAnimation {
        start_direction,
        target_direction: direction.as_vec3().normalize(),
        elapsed: 0.0,
    });
}

pub(crate) fn animate_navigation_cube_view(
    time: Res<Time>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut state: ResMut<NavigationCubeState>,
    mut main_camera: Query<(&mut Transform, &OrbitCamera), With<MainViewportCamera>>,
) {
    if buttons.pressed(MouseButton::Middle) {
        state.animation = None;
        return;
    }
    let Some(mut animation) = state.animation else {
        return;
    };
    let Ok((mut transform, orbit)) = main_camera.single_mut() else {
        state.animation = None;
        return;
    };

    animation.elapsed += time.delta_secs();
    let linear_t = (animation.elapsed / SNAP_ANIMATION_SECONDS).clamp(0.0, 1.0);
    let eased_t = linear_t * linear_t * (3.0 - 2.0 * linear_t);
    let direction = slerp_direction(
        animation.start_direction,
        animation.target_direction,
        eased_t,
    );
    transform.translation = orbit.focus + direction * orbit.radius;
    transform.look_at(orbit.focus, orbit_camera_up(direction));

    state.animation = (linear_t < 1.0).then_some(animation);
}

pub(crate) fn sync_navigation_cube_camera(
    main_camera: Query<(&Transform, &OrbitCamera), MainCameraFilter>,
    mut nav_camera: Query<&mut Transform, NavigationCameraFilter>,
) {
    let Ok((main_transform, orbit)) = main_camera.single() else {
        return;
    };
    let Ok(mut nav_transform) = nav_camera.single_mut() else {
        return;
    };
    let direction = normalized_or(main_transform.translation - orbit.focus, Vec3::Z);
    *nav_transform = Transform::from_translation(direction * CUBE_CAMERA_DISTANCE)
        .looking_at(Vec3::ZERO, orbit_camera_up(direction));
}

pub(crate) fn update_navigation_cube_highlight(
    state: Res<NavigationCubeState>,
    mut pieces: Query<(&NavigationCubePiece, &mut MeshMaterial3d<StandardMaterial>)>,
    mut labels: Query<&mut Text, With<NavigationCubeLabel>>,
) {
    if !state.is_changed() {
        return;
    }
    for (piece, mut material) in &mut pieces {
        material.0 = if state.hovered == Some(piece.direction) {
            piece.hover_material.clone()
        } else {
            piece.normal_material.clone()
        };
    }
    for mut label in &mut labels {
        **label = state.hovered.map_or_else(
            || "VIEW • face / edge / corner".to_string(),
            |direction| format!("VIEW • {}", direction_label(direction)),
        );
    }
}

fn ray_box_hit(origin: Vec3, direction: Vec3, half_extent: f32) -> Option<Vec3> {
    let mut near = f32::NEG_INFINITY;
    let mut far = f32::INFINITY;
    for axis in 0..3 {
        let origin_axis = origin[axis];
        let direction_axis = direction[axis];
        if direction_axis.abs() < 1.0e-7 {
            if origin_axis.abs() > half_extent {
                return None;
            }
            continue;
        }
        let first = (-half_extent - origin_axis) / direction_axis;
        let second = (half_extent - origin_axis) / direction_axis;
        near = near.max(first.min(second));
        far = far.min(first.max(second));
        if near > far {
            return None;
        }
    }
    let distance = if near >= 0.0 { near } else { far };
    (distance >= 0.0).then_some(origin + direction * distance)
}

fn snap_direction_from_cube_hit(hit: Vec3, half_extent: f32) -> IVec3 {
    let normalized = hit / half_extent;
    let component = |value: f32| {
        if value.abs() >= EDGE_ZONE_THRESHOLD {
            value.signum() as i32
        } else {
            0
        }
    };
    let direction = IVec3::new(
        component(normalized.x),
        component(normalized.y),
        component(normalized.z),
    );
    if direction == IVec3::ZERO {
        let absolute = normalized.abs();
        let axis = if absolute.x >= absolute.y && absolute.x >= absolute.z {
            0
        } else if absolute.y >= absolute.z {
            1
        } else {
            2
        };
        let mut fallback = IVec3::ZERO;
        fallback[axis] = normalized[axis].signum() as i32;
        fallback
    } else {
        direction
    }
}

fn normalized_or(value: Vec3, fallback: Vec3) -> Vec3 {
    if value.is_finite() && value.length_squared() > 1.0e-12 {
        value.normalize()
    } else {
        fallback
    }
}

fn slerp_direction(start: Vec3, target: Vec3, t: f32) -> Vec3 {
    let start = normalized_or(start, Vec3::Z);
    let target = normalized_or(target, Vec3::Z);
    let rotation = Quat::from_rotation_arc(start, target);
    normalized_or(Quat::IDENTITY.slerp(rotation, t) * start, target)
}

fn direction_label(direction: IVec3) -> String {
    let mut labels = Vec::with_capacity(3);
    if direction.y > 0 {
        labels.push("TOP +Y");
    } else if direction.y < 0 {
        labels.push("BOTTOM -Y");
    }
    if direction.x > 0 {
        labels.push("RIGHT +X");
    } else if direction.x < 0 {
        labels.push("LEFT -X");
    }
    if direction.z > 0 {
        labels.push("FRONT +Z");
    } else if direction.z < 0 {
        labels.push("BACK -Z");
    }
    labels.join(" / ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_hit_zones_distinguish_face_edge_and_corner() {
        assert_eq!(
            snap_direction_from_cube_hit(Vec3::new(0.0, 0.0, CUBE_HALF_EXTENT), CUBE_HALF_EXTENT),
            IVec3::Z
        );
        assert_eq!(
            snap_direction_from_cube_hit(
                Vec3::new(CUBE_HALF_EXTENT, 0.0, CUBE_HALF_EXTENT),
                CUBE_HALF_EXTENT,
            ),
            IVec3::new(1, 0, 1)
        );
        assert_eq!(
            snap_direction_from_cube_hit(Vec3::splat(CUBE_HALF_EXTENT), CUBE_HALF_EXTENT),
            IVec3::ONE
        );
    }

    #[test]
    fn ray_box_hit_returns_the_front_surface() {
        let hit =
            ray_box_hit(Vec3::new(0.0, 0.0, 4.0), -Vec3::Z, CUBE_HALF_EXTENT).expect("front hit");
        assert!((hit.z - CUBE_HALF_EXTENT).abs() < 1.0e-6);
    }

    #[test]
    fn direction_slerp_stays_normalized() {
        let middle = slerp_direction(Vec3::Z, Vec3::X, 0.5);
        assert!((middle.length() - 1.0).abs() < 1.0e-6);
        assert!(middle.x > 0.0 && middle.z > 0.0);
    }
}
