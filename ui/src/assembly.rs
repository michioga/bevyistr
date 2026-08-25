//! Direct 3-D assembly manipulation.
//!
//! The panel controls remain useful for exact nudges, but this module owns
//! the viewport-first workflow: hover a part, click it, then drag one of the
//! world-axis arrows. During a drag only render transforms are updated; FEM
//! coordinates and topology are rebuilt once, when the gesture is committed.

use crate::slider::{SliderId, SliderState, SliderTrack};
use bevy::math::primitives::{Cone, Cuboid, Cylinder};
use bevy::mesh::Mesh3d;
use bevy::pbr::MeshMaterial3d;
use bevy::prelude::*;
use fem_core::{
    ContactCandidateState, FemModel, FemModelVersion, InteractionMode, UiPointerState,
    ViewportTool,
};
use visualization::{FemPartVisual, build_part_edge_mesh, build_part_surface_mesh};

const GIZMO_PICK_RADIUS_PX: f32 = 11.0;
const GIZMO_LENGTH_FACTOR: f32 = 0.24;

#[derive(Resource, Debug, Clone)]
pub(crate) struct AssemblyEditorState {
    pub selected_part: Option<usize>,
    pub hovered_part: Option<usize>,
    pub hovered_axis: Option<Vec3>,
    drag: Option<AssemblyDrag>,
}

impl Default for AssemblyEditorState {
    fn default() -> Self {
        Self {
            selected_part: Some(0),
            hovered_part: None,
            hovered_axis: None,
            drag: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct AssemblyDrag {
    part_index: usize,
    mesh_index: usize,
    axis: Vec3,
    last_cursor: Vec2,
    screen_axis: Vec2,
    world_per_pixel: f32,
    accumulated_scalar: f32,
    preview_delta: Vec3,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum PartOverlayKind {
    Hover,
    Selected,
}

#[derive(Component)]
pub(crate) struct PartOverlay {
    kind: PartOverlayKind,
    rendered: Option<(usize, u64)>,
}

#[derive(Component, Debug, Clone)]
pub(crate) struct AssemblyGizmoPiece {
    axis: Option<Vec3>,
    offset: f32,
    rotation: Quat,
    normal_material: Handle<StandardMaterial>,
    active_material: Handle<StandardMaterial>,
}

pub(crate) fn reference_size(model: &FemModel, part_index: usize) -> f32 {
    let part_size = model
        .part_bounds(part_index)
        .map(|(min, max)| min.distance(max))
        .filter(|size| size.is_finite() && *size > 1.0e-9);
    let model_size = model
        .bounds()
        .map(|(min, max)| min.distance(max))
        .filter(|size| size.is_finite() && *size > 1.0e-9);

    part_size.or(model_size).unwrap_or(1.0)
}

pub(crate) fn spawn_assembly_viewport_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let hover_material = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.78, 0.08, 0.30),
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        double_sided: true,
        unlit: true,
        depth_bias: 3.0,
        ..default()
    });
    let selected_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.10, 0.78, 1.0, 0.26),
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        double_sided: true,
        unlit: true,
        depth_bias: 2.0,
        ..default()
    });

    for (kind, material, name) in [
        (PartOverlayKind::Hover, hover_material, "Assembly part hover"),
        (
            PartOverlayKind::Selected,
            selected_material,
            "Assembly part selected",
        ),
    ] {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(0.01, 0.01, 0.01))),
            MeshMaterial3d(material),
            Transform::default(),
            Visibility::Hidden,
            PartOverlay {
                kind,
                rendered: None,
            },
            Name::new(name),
        ));
    }

    let active_material = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.95, 0.55),
        emissive: LinearRgba::rgb(1.4, 1.2, 0.25),
        unlit: true,
        ..default()
    });
    let root_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.90, 0.94, 0.98),
        unlit: true,
        ..default()
    });

    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.10, 0.10, 0.10))),
        MeshMaterial3d(root_material.clone()),
        Transform::default(),
        Visibility::Hidden,
        AssemblyGizmoPiece {
            axis: None,
            offset: 0.0,
            rotation: Quat::IDENTITY,
            normal_material: root_material.clone(),
            active_material: root_material,
        },
        Name::new("Assembly gizmo origin"),
    ));

    for (axis, color, label) in [
        (Vec3::X, Color::srgb(0.92, 0.16, 0.18), "X"),
        (Vec3::Y, Color::srgb(0.20, 0.82, 0.30), "Y"),
        (Vec3::Z, Color::srgb(0.18, 0.42, 1.0), "Z"),
    ] {
        let normal_material = materials.add(StandardMaterial {
            base_color: color,
            unlit: true,
            ..default()
        });
        let rotation = Quat::from_rotation_arc(Vec3::Y, axis);

        commands.spawn((
            Mesh3d(meshes.add(Cylinder {
                radius: 0.035,
                half_height: 0.32,
            })),
            MeshMaterial3d(normal_material.clone()),
            Transform::default(),
            Visibility::Hidden,
            AssemblyGizmoPiece {
                axis: Some(axis),
                offset: 0.32,
                rotation,
                normal_material: normal_material.clone(),
                active_material: active_material.clone(),
            },
            Name::new(format!("Assembly gizmo {label} shaft")),
        ));

        commands.spawn((
            Mesh3d(meshes.add(Cone {
                radius: 0.10,
                height: 0.20,
            })),
            MeshMaterial3d(normal_material.clone()),
            Transform::default(),
            Visibility::Hidden,
            AssemblyGizmoPiece {
                axis: Some(axis),
                offset: 0.74,
                rotation,
                normal_material,
                active_material: active_material.clone(),
            },
            Name::new(format!("Assembly gizmo {label} head")),
        ));
    }
}

pub(crate) fn assembly_viewport_hover_system(
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    model: Res<FemModel>,
    tool: Res<ViewportTool>,
    ui_pointer: Res<UiPointerState>,
    mut state: ResMut<AssemblyEditorState>,
) {
    if *tool != ViewportTool::Assembly || ui_pointer.over_ui {
        state.hovered_part = None;
        state.hovered_axis = None;
        return;
    }
    if let Some(drag) = state.drag {
        state.hovered_part = None;
        state.hovered_axis = Some(drag.axis);
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        state.hovered_part = None;
        state.hovered_axis = None;
        return;
    };
    let Ok((camera, camera_transform)) = camera_query.single() else {
        return;
    };

    state.hovered_axis = state.selected_part.and_then(|part_index| {
        let center = model.part_centroid(part_index)?;
        let size = reference_size(&model, part_index) * GIZMO_LENGTH_FACTOR;
        pick_gizmo_axis(camera, camera_transform, cursor, center, size)
    });

    if state.hovered_axis.is_some() {
        state.hovered_part = None;
        return;
    }

    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        state.hovered_part = None;
        return;
    };
    state.hovered_part = picking::pick_part(&model, ray.origin, *ray.direction)
        .and_then(|hit| part_index_for_mesh(&model, hit.target.mesh_index));
}

pub(crate) fn assembly_viewport_input_system(
    buttons: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    tool: Res<ViewportTool>,
    ui_pointer: Res<UiPointerState>,
    mut mode: ResMut<InteractionMode>,
    mut model: ResMut<FemModel>,
    mut version: ResMut<FemModelVersion>,
    mut contact_candidates: ResMut<ContactCandidateState>,
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut state: ResMut<AssemblyEditorState>,
    mut part_visuals: Query<(&FemPartVisual, &mut Transform)>,
) {
    if *tool != ViewportTool::Assembly {
        cancel_drag(&mut state, &mut part_visuals, &mut mode);
        return;
    }

    if keyboard.just_pressed(KeyCode::Escape) {
        cancel_drag(&mut state, &mut part_visuals, &mut mode);
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };

    if buttons.just_pressed(MouseButton::Left) && !ui_pointer.over_ui {
        if let (Some(part_index), Some(axis)) = (state.selected_part, state.hovered_axis) {
            let Some(mesh_index) = model.parts.get(part_index).map(|part| part.mesh_index) else {
                return;
            };
            let Some(center) = model.part_centroid(part_index) else {
                return;
            };
            let size = reference_size(&model, part_index) * GIZMO_LENGTH_FACTOR;
            let Ok((camera, camera_transform)) = camera_query.single() else {
                return;
            };
            let Some((screen_axis, world_per_pixel)) =
                drag_projection(camera, camera_transform, center, axis, size)
            else {
                return;
            };

            state.drag = Some(AssemblyDrag {
                part_index,
                mesh_index,
                axis,
                last_cursor: cursor,
                screen_axis,
                world_per_pixel,
                accumulated_scalar: 0.0,
                preview_delta: Vec3::ZERO,
            });
            *mode = InteractionMode::AssemblyDrag;
        } else {
            state.selected_part = state.hovered_part;
        }
    }

    if state.drag.is_some()
        && (buttons.pressed(MouseButton::Left) || buttons.just_released(MouseButton::Left))
    {
        let drag = state.drag.unwrap();
        let mut incremental_scalar =
            (cursor - drag.last_cursor).dot(drag.screen_axis) * drag.world_per_pixel;

        if keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight) {
            incremental_scalar *= 0.1;
        }
        let accumulated_scalar = drag.accumulated_scalar + incremental_scalar;
        let mut scalar = accumulated_scalar;
        if keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight) {
            let percent = sliders
                .iter()
                .find(|slider| slider.id == SliderId::AssemblyMovePercent)
                .map(|slider| slider.value)
                .unwrap_or(1.0);
            let step = reference_size(&model, drag.part_index) * percent / 100.0;
            if step > 1.0e-9 {
                scalar = (scalar / step).round() * step;
            }
        }

        let preview_delta = drag.axis * scalar;
        let incremental = preview_delta - drag.preview_delta;
        apply_visual_delta(drag.mesh_index, incremental, &mut part_visuals);
        if let Some(active) = state.drag.as_mut() {
            active.last_cursor = cursor;
            active.accumulated_scalar = accumulated_scalar;
            active.preview_delta = preview_delta;
        }
    }

    if buttons.just_released(MouseButton::Left) {
        let Some(drag) = state.drag.take() else {
            return;
        };
        *mode = InteractionMode::Idle;

        if drag.preview_delta.length_squared() <= 1.0e-18 {
            return;
        }
        if model.translate_part(drag.part_index, drag.preview_delta) {
            contact_candidates.candidates.clear();
            contact_candidates.selected = None;
            version.bump();
        } else {
            apply_visual_delta(drag.mesh_index, -drag.preview_delta, &mut part_visuals);
        }
    }
}

pub(crate) fn update_assembly_part_overlays(
    model: Res<FemModel>,
    version: Res<FemModelVersion>,
    tool: Res<ViewportTool>,
    state: Res<AssemblyEditorState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut overlays: Query<(
        &mut PartOverlay,
        &mut Mesh3d,
        &mut Transform,
        &mut Visibility,
    )>,
) {
    for (mut overlay, mut mesh_handle, mut transform, mut visibility) in &mut overlays {
        let part_index = match overlay.kind {
            PartOverlayKind::Selected => state.selected_part,
            PartOverlayKind::Hover => state
                .hovered_part
                .filter(|hovered| Some(*hovered) != state.selected_part),
        };

        let Some(part_index) = part_index.filter(|_| *tool == ViewportTool::Assembly) else {
            *visibility = Visibility::Hidden;
            overlay.rendered = None;
            continue;
        };
        let Some(part) = model.parts.get(part_index) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let key = (part.mesh_index, version.value);
        if overlay.rendered != Some(key) {
            let Some(fem_mesh) = model.meshes.get(part.mesh_index) else {
                *visibility = Visibility::Hidden;
                continue;
            };
            let Some(new_mesh) = build_part_surface_mesh(fem_mesh)
                .or_else(|| build_part_edge_mesh(fem_mesh))
            else {
                *visibility = Visibility::Hidden;
                continue;
            };
            if let Some(mut existing) = meshes.get_mut(&mesh_handle.0) {
                *existing = new_mesh;
            } else {
                mesh_handle.0 = meshes.add(new_mesh);
            }
            overlay.rendered = Some(key);
        }

        transform.translation = state
            .drag
            .filter(|drag| drag.part_index == part_index)
            .map(|drag| drag.preview_delta)
            .unwrap_or(Vec3::ZERO);
        *visibility = Visibility::Visible;
    }
}

pub(crate) fn update_assembly_gizmo_visuals(
    model: Res<FemModel>,
    tool: Res<ViewportTool>,
    state: Res<AssemblyEditorState>,
    mut pieces: Query<(
        &AssemblyGizmoPiece,
        &mut Transform,
        &mut Visibility,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let Some(part_index) = state
        .selected_part
        .filter(|_| *tool == ViewportTool::Assembly)
    else {
        for (_, _, mut visibility, _) in &mut pieces {
            *visibility = Visibility::Hidden;
        }
        return;
    };
    let Some(mut center) = model.part_centroid(part_index) else {
        return;
    };
    if let Some(drag) = state.drag.filter(|drag| drag.part_index == part_index) {
        center += drag.preview_delta;
    }
    let size = reference_size(&model, part_index) * GIZMO_LENGTH_FACTOR;
    let active_axis = state.drag.map(|drag| drag.axis).or(state.hovered_axis);

    for (piece, mut transform, mut visibility, mut material) in &mut pieces {
        let axis = piece.axis.unwrap_or(Vec3::ZERO);
        transform.translation = center + axis * piece.offset * size;
        transform.rotation = piece.rotation;
        transform.scale = Vec3::splat(size);
        material.0 = if piece.axis.is_some() && piece.axis == active_axis {
            piece.active_material.clone()
        } else {
            piece.normal_material.clone()
        };
        *visibility = Visibility::Visible;
    }
}

fn part_index_for_mesh(model: &FemModel, mesh_index: usize) -> Option<usize> {
    model
        .parts
        .iter()
        .position(|part| part.mesh_index == mesh_index)
}

fn pick_gizmo_axis(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    cursor: Vec2,
    center: Vec3,
    size: f32,
) -> Option<Vec3> {
    [Vec3::X, Vec3::Y, Vec3::Z]
        .into_iter()
        .filter_map(|axis| {
            let start = camera
                .world_to_viewport(camera_transform, center + axis * size * 0.12)
                .ok()?;
            let end = camera
                .world_to_viewport(camera_transform, center + axis * size * 0.90)
                .ok()?;
            if start.distance_squared(end) < 36.0 {
                return None;
            }
            let distance = point_segment_distance(cursor, start, end);
            (distance <= GIZMO_PICK_RADIUS_PX).then_some((axis, distance))
        })
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(axis, _)| axis)
}

fn drag_projection(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    center: Vec3,
    axis: Vec3,
    size: f32,
) -> Option<(Vec2, f32)> {
    let start = camera.world_to_viewport(camera_transform, center).ok()?;
    let end = camera
        .world_to_viewport(camera_transform, center + axis * size)
        .ok()?;
    let screen_delta = end - start;
    let pixels = screen_delta.length();
    (pixels > 1.0).then_some((screen_delta / pixels, size / pixels))
}

fn point_segment_distance(point: Vec2, start: Vec2, end: Vec2) -> f32 {
    let segment = end - start;
    let length_squared = segment.length_squared();
    if length_squared <= f32::EPSILON {
        return point.distance(start);
    }
    let t = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    point.distance(start + segment * t)
}

fn apply_visual_delta(
    mesh_index: usize,
    delta: Vec3,
    visuals: &mut Query<(&FemPartVisual, &mut Transform)>,
) {
    if delta.length_squared() <= 1.0e-20 {
        return;
    }
    for (part, mut transform) in visuals.iter_mut() {
        if part.mesh_index == mesh_index {
            transform.translation += delta;
        }
    }
}

fn cancel_drag(
    state: &mut AssemblyEditorState,
    visuals: &mut Query<(&FemPartVisual, &mut Transform)>,
    mode: &mut InteractionMode,
) {
    if let Some(drag) = state.drag.take() {
        apply_visual_delta(drag.mesh_index, -drag.preview_delta, visuals);
    }
    state.hovered_axis = None;
    state.hovered_part = None;
    if *mode == InteractionMode::AssemblyDrag {
        *mode = InteractionMode::Idle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_distance_hits_the_middle_of_an_axis_handle() {
        assert_eq!(
            point_segment_distance(
                Vec2::new(5.0, 2.0),
                Vec2::ZERO,
                Vec2::new(10.0, 0.0),
            ),
            2.0
        );
    }

    #[test]
    fn reference_size_uses_the_selected_part_diagonal() {
        let model = FemModel::demo_hex8();
        let expected = Vec3::new(2.0, 1.0, 1.0).length();

        assert!((reference_size(&model, 0) - expected).abs() < 1.0e-6);
    }
}
