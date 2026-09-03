//! Direct 3-D assembly manipulation.
//!
//! The panel controls remain useful for exact nudges, but this module owns
//! the viewport-first workflow: hover a part, click it, then drag one of the
//! world-axis arrows. During a drag only render transforms are updated; FEM
//! coordinates and topology are rebuilt once, when the gesture is committed.

use crate::measurement::MeasurementBoxState;
use crate::slider::{SliderId, SliderState, SliderTrack};
use bevy::camera::visibility::RenderLayers;
use bevy::math::primitives::{Cone, Cuboid, Cylinder, Torus};
use bevy::mesh::Mesh3d;
use bevy::pbr::MeshMaterial3d;
use bevy::prelude::*;
use fem_core::{
    ContactCandidateState, FemModel, FemModelVersion, InteractionMode, MainViewportCamera,
    UiKeyboardState, UiPointerState, ViewportTool,
};
use visualization::{FemPartVisual, build_part_edge_mesh, build_part_surface_mesh};

const GIZMO_PICK_RADIUS_PX: f32 = 11.0;
const GIZMO_LENGTH_FACTOR: f32 = 0.24;
const ROTATION_RING_FACTOR: f32 = 0.36;
const ROTATION_RING_RADIUS: f32 = 0.92;
const ROTATION_PICK_TOLERANCE: f32 = 0.12;
pub(crate) const VIEWPORT_GIZMO_RENDER_LAYER: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum AssemblyGizmoMode {
    #[default]
    Move,
    Rotate,
}

impl AssemblyGizmoMode {
    pub(crate) const ALL: [Self; 2] = [Self::Move, Self::Rotate];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Move => "Move",
            Self::Rotate => "Rotate",
        }
    }
}

#[derive(Resource, Debug, Clone)]
pub(crate) struct AssemblyEditorState {
    pub selected_part: Option<usize>,
    pub hovered_part: Option<usize>,
    pub hovered_axis: Option<Vec3>,
    pub gizmo_mode: AssemblyGizmoMode,
    drag: Option<AssemblyDrag>,
}

impl Default for AssemblyEditorState {
    fn default() -> Self {
        Self {
            selected_part: Some(0),
            hovered_part: None,
            hovered_axis: None,
            gizmo_mode: AssemblyGizmoMode::Move,
            drag: None,
        }
    }
}

impl AssemblyEditorState {
    /// Drag previews change render transforms before FEM coordinates are committed.
    pub(crate) fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }
}

#[derive(Debug, Clone, Copy)]
struct AssemblyDrag {
    part_index: usize,
    mesh_index: usize,
    axis: Vec3,
    kind: AssemblyDragKind,
}

#[derive(Debug, Clone, Copy)]
enum AssemblyDragKind {
    Translation {
        last_cursor: Vec2,
        screen_axis: Vec2,
        world_per_pixel: f32,
        accumulated_scalar: f32,
        preview_delta: Vec3,
    },
    Rotation {
        center: Vec3,
        last_direction: Vec3,
        accumulated_radians: f32,
        preview_radians: f32,
    },
}

impl AssemblyDrag {
    fn preview_transform(self) -> Transform {
        match self.kind {
            AssemblyDragKind::Translation { preview_delta, .. } => {
                Transform::from_translation(preview_delta)
            }
            AssemblyDragKind::Rotation {
                center,
                preview_radians,
                ..
            } => rotation_about(center, self.axis, preview_radians),
        }
    }
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
    mode: Option<AssemblyGizmoMode>,
    offset: f32,
    rotation: Quat,
    normal_material: Handle<StandardMaterial>,
    active_material: Handle<StandardMaterial>,
}

#[derive(Component)]
pub(crate) struct AssemblyGizmoOverlayCamera;

type MainAssemblyCameraFilter = (
    With<MainViewportCamera>,
    Without<AssemblyGizmoOverlayCamera>,
);
type AssemblyOverlayCameraFilter = (
    With<AssemblyGizmoOverlayCamera>,
    Without<MainViewportCamera>,
);

pub(crate) fn reference_size(model: &FemModel, part_index: usize) -> f32 {
    if let Some(part_size) = model
        .part_bounds(part_index)
        .map(|(min, max)| min.distance(max))
        .filter(|size| size.is_finite() && *size > 1.0e-9)
    {
        return part_size;
    }

    model
        .bounds()
        .map(|(min, max)| min.distance(max))
        .filter(|size| size.is_finite() && *size > 1.0e-9)
        .unwrap_or(1.0)
}

/// Canonical viewport axis colours, shared by the manipulation gizmo and
/// every corresponding numeric-step control in the sidebar.
pub(crate) fn axis_color(axis: Vec3) -> Color {
    let axis = axis.abs();
    if axis.x >= axis.y && axis.x >= axis.z {
        Color::srgba(0.92, 0.16, 0.18, 0.86)
    } else if axis.y >= axis.z {
        Color::srgba(0.20, 0.82, 0.30, 0.86)
    } else {
        Color::srgba(0.18, 0.42, 1.0, 0.86)
    }
}

pub(crate) fn spawn_assembly_viewport_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 10,
            is_active: false,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        RenderLayers::layer(VIEWPORT_GIZMO_RENDER_LAYER),
        AssemblyGizmoOverlayCamera,
        Name::new("Assembly gizmo X-ray camera"),
    ));

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
        (
            PartOverlayKind::Hover,
            hover_material,
            "Assembly part hover",
        ),
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
        base_color: Color::srgba(1.0, 0.95, 0.55, 0.90),
        emissive: LinearRgba::rgb(1.4, 1.2, 0.25),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let root_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.90, 0.94, 0.98, 0.86),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });

    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.10, 0.10, 0.10))),
        MeshMaterial3d(root_material.clone()),
        RenderLayers::layer(VIEWPORT_GIZMO_RENDER_LAYER),
        Transform::default(),
        Visibility::Hidden,
        AssemblyGizmoPiece {
            axis: None,
            mode: None,
            offset: 0.0,
            rotation: Quat::IDENTITY,
            normal_material: root_material.clone(),
            active_material: root_material,
        },
        Name::new("Assembly gizmo origin"),
    ));

    for (axis, label) in [(Vec3::X, "X"), (Vec3::Y, "Y"), (Vec3::Z, "Z")] {
        let color = axis_color(axis);
        let normal_material = materials.add(StandardMaterial {
            base_color: color,
            alpha_mode: AlphaMode::Blend,
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
            RenderLayers::layer(VIEWPORT_GIZMO_RENDER_LAYER),
            Transform::default(),
            Visibility::Hidden,
            AssemblyGizmoPiece {
                axis: Some(axis),
                mode: Some(AssemblyGizmoMode::Move),
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
            RenderLayers::layer(VIEWPORT_GIZMO_RENDER_LAYER),
            Transform::default(),
            Visibility::Hidden,
            AssemblyGizmoPiece {
                axis: Some(axis),
                mode: Some(AssemblyGizmoMode::Move),
                offset: 0.74,
                rotation,
                normal_material,
                active_material: active_material.clone(),
            },
            Name::new(format!("Assembly gizmo {label} head")),
        ));

        let ring_material = materials.add(StandardMaterial {
            base_color: color,
            cull_mode: None,
            double_sided: true,
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        });
        commands.spawn((
            Mesh3d(meshes.add(Torus::new(0.84, 1.0))),
            MeshMaterial3d(ring_material.clone()),
            RenderLayers::layer(VIEWPORT_GIZMO_RENDER_LAYER),
            Transform::default(),
            Visibility::Hidden,
            AssemblyGizmoPiece {
                axis: Some(axis),
                mode: Some(AssemblyGizmoMode::Rotate),
                offset: 0.0,
                rotation,
                normal_material: ring_material,
                active_material: active_material.clone(),
            },
            Name::new(format!("Assembly gizmo {label} rotation ring")),
        ));
    }
}

/// Mirrors the engineering camera into a second render pass that contains
/// only the manipulation gizmo. Its independent depth buffer makes the
/// semi-transparent handles visible even when the selected part surrounds
/// its centroid.
pub(crate) fn sync_assembly_overlay_camera(
    tool: Res<ViewportTool>,
    main_camera: Query<(&Transform, &Projection), MainAssemblyCameraFilter>,
    mut overlay_camera: Query<
        (&mut Camera, &mut Transform, &mut Projection),
        AssemblyOverlayCameraFilter,
    >,
) {
    let Ok((mut camera, mut transform, mut projection)) = overlay_camera.single_mut() else {
        return;
    };
    camera.is_active = matches!(*tool, ViewportTool::Assembly | ViewportTool::LoadDirection);
    if !camera.is_active {
        return;
    }

    let Ok((main_transform, main_projection)) = main_camera.single() else {
        camera.is_active = false;
        return;
    };
    *transform = *main_transform;
    *projection = main_projection.clone();
}

pub(crate) fn assembly_viewport_hover_system(
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<MainViewportCamera>>,
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

    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        state.hovered_part = None;
        state.hovered_axis = None;
        return;
    };

    let gizmo_mode = state.gizmo_mode;
    state.hovered_axis = state.selected_part.and_then(|part_index| {
        let center = model.part_centroid(part_index)?;
        match gizmo_mode {
            AssemblyGizmoMode::Move => {
                let size = reference_size(&model, part_index) * GIZMO_LENGTH_FACTOR;
                pick_gizmo_axis(camera, camera_transform, cursor, center, size)
            }
            AssemblyGizmoMode::Rotate => {
                let size = reference_size(&model, part_index) * ROTATION_RING_FACTOR;
                pick_rotation_axis(
                    ray.origin,
                    *ray.direction,
                    center,
                    size * ROTATION_RING_RADIUS,
                    size * ROTATION_PICK_TOLERANCE,
                )
            }
        }
    });

    if state.hovered_axis.is_some() {
        state.hovered_part = None;
        return;
    }

    state.hovered_part = picking::pick_part(&model, ray.origin, *ray.direction)
        .and_then(|hit| part_index_for_mesh(&model, hit.target.mesh_index));
}

pub(crate) fn assembly_viewport_input_system(
    buttons: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    keyboard_state: Res<UiKeyboardState>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<MainViewportCamera>>,
    tool: Res<ViewportTool>,
    ui_pointer: Res<UiPointerState>,
    mut mode: ResMut<InteractionMode>,
    mut model: ResMut<FemModel>,
    mut version: ResMut<FemModelVersion>,
    mut contact_candidates: ResMut<ContactCandidateState>,
    sliders: Query<&SliderState, With<SliderTrack>>,
    mut state: ResMut<AssemblyEditorState>,
    mut measurement: ResMut<MeasurementBoxState>,
    mut part_visuals: Query<(&FemPartVisual, &mut Transform)>,
) {
    if *tool != ViewportTool::Assembly {
        cancel_drag(&mut state, &mut part_visuals, &mut mode);
        return;
    }

    if !keyboard_state.text_editing && keyboard.just_pressed(KeyCode::Escape) {
        cancel_drag(&mut state, &mut part_visuals, &mut mode);
        measurement.clear();
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
            let Ok((camera, camera_transform)) = camera_query.single() else {
                return;
            };
            let kind = match state.gizmo_mode {
                AssemblyGizmoMode::Move => {
                    let size = reference_size(&model, part_index) * GIZMO_LENGTH_FACTOR;
                    let Some((screen_axis, world_per_pixel)) =
                        drag_projection(camera, camera_transform, center, axis, size)
                    else {
                        return;
                    };
                    measurement.begin_assembly_translation(part_index, axis);
                    AssemblyDragKind::Translation {
                        last_cursor: cursor,
                        screen_axis,
                        world_per_pixel,
                        accumulated_scalar: 0.0,
                        preview_delta: Vec3::ZERO,
                    }
                }
                AssemblyGizmoMode::Rotate => {
                    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
                        return;
                    };
                    let Some(last_direction) =
                        ray_plane_direction(ray.origin, *ray.direction, center, axis)
                    else {
                        return;
                    };
                    measurement.begin_assembly_rotation(part_index, axis);
                    AssemblyDragKind::Rotation {
                        center,
                        last_direction,
                        accumulated_radians: 0.0,
                        preview_radians: 0.0,
                    }
                }
            };

            state.drag = Some(AssemblyDrag {
                part_index,
                mesh_index,
                axis,
                kind,
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
        match drag.kind {
            AssemblyDragKind::Translation {
                last_cursor,
                screen_axis,
                world_per_pixel,
                accumulated_scalar,
                ..
            } => {
                let mut incremental_scalar =
                    (cursor - last_cursor).dot(screen_axis) * world_per_pixel;
                if shift_pressed(&keyboard) {
                    incremental_scalar *= 0.1;
                }
                let accumulated_scalar = accumulated_scalar + incremental_scalar;
                let mut scalar = accumulated_scalar;
                if control_pressed(&keyboard) {
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
                set_visual_preview(
                    drag.mesh_index,
                    Transform::from_translation(preview_delta),
                    &mut part_visuals,
                );
                if let Some(active) = state.drag.as_mut() {
                    active.kind = AssemblyDragKind::Translation {
                        last_cursor: cursor,
                        screen_axis,
                        world_per_pixel,
                        accumulated_scalar,
                        preview_delta,
                    };
                }
                measurement.preview_translation(scalar);
            }
            AssemblyDragKind::Rotation {
                center,
                last_direction,
                accumulated_radians,
                ..
            } => {
                let Ok((camera, camera_transform)) = camera_query.single() else {
                    return;
                };
                let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
                    return;
                };
                let Some(current_direction) =
                    ray_plane_direction(ray.origin, *ray.direction, center, drag.axis)
                else {
                    return;
                };
                let mut incremental_radians =
                    signed_angle(last_direction, current_direction, drag.axis);
                if shift_pressed(&keyboard) {
                    incremental_radians *= 0.1;
                }
                let accumulated_radians = accumulated_radians + incremental_radians;
                let mut preview_radians = accumulated_radians;
                if control_pressed(&keyboard) {
                    let step_degrees = sliders
                        .iter()
                        .find(|slider| slider.id == SliderId::AssemblyRotationDegrees)
                        .map(|slider| slider.value)
                        .unwrap_or(5.0);
                    let step_radians = step_degrees.to_radians();
                    if step_radians > 1.0e-9 {
                        preview_radians = (preview_radians / step_radians).round() * step_radians;
                    }
                }

                set_visual_preview(
                    drag.mesh_index,
                    rotation_about(center, drag.axis, preview_radians),
                    &mut part_visuals,
                );
                if let Some(active) = state.drag.as_mut() {
                    active.kind = AssemblyDragKind::Rotation {
                        center,
                        last_direction: current_direction,
                        accumulated_radians,
                        preview_radians,
                    };
                }
                measurement.preview_rotation(preview_radians.to_degrees());
            }
        }
    }

    if buttons.just_released(MouseButton::Left) {
        let Some(drag) = state.drag.take() else {
            return;
        };
        *mode = InteractionMode::Idle;

        let changed = match drag.kind {
            AssemblyDragKind::Translation { preview_delta, .. }
                if preview_delta.length_squared() > 1.0e-18 =>
            {
                let changed = model.translate_part(drag.part_index, preview_delta);
                if changed {
                    measurement.commit_translation(preview_delta.dot(drag.axis));
                }
                changed
            }
            AssemblyDragKind::Rotation {
                preview_radians, ..
            } if preview_radians.abs() > 1.0e-9 => {
                let changed = model.rotate_part_about_centroid(
                    drag.part_index,
                    Quat::from_axis_angle(drag.axis, preview_radians),
                );
                if changed {
                    measurement.commit_rotation(preview_radians.to_degrees());
                }
                changed
            }
            _ => false,
        };

        if changed {
            contact_candidates.candidates.clear();
            contact_candidates.selected = None;
            version.bump();
        } else {
            set_visual_preview(drag.mesh_index, Transform::default(), &mut part_visuals);
            measurement.clear();
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
            let Some(new_mesh) =
                build_part_surface_mesh(fem_mesh).or_else(|| build_part_edge_mesh(fem_mesh))
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

        *transform = state
            .drag
            .filter(|drag| drag.part_index == part_index)
            .map(AssemblyDrag::preview_transform)
            .unwrap_or_default();
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
    if let Some(AssemblyDrag {
        kind: AssemblyDragKind::Translation { preview_delta, .. },
        ..
    }) = state.drag.filter(|drag| drag.part_index == part_index)
    {
        center += preview_delta;
    }
    let part_reference_size = reference_size(&model, part_index);
    let move_size = part_reference_size * GIZMO_LENGTH_FACTOR;
    let rotation_size = part_reference_size * ROTATION_RING_FACTOR;
    let active_axis = state.drag.map(|drag| drag.axis).or(state.hovered_axis);

    for (piece, mut transform, mut visibility, mut material) in &mut pieces {
        if piece.mode.is_some_and(|mode| mode != state.gizmo_mode) {
            *visibility = Visibility::Hidden;
            continue;
        }
        let size = match piece.mode {
            Some(AssemblyGizmoMode::Rotate) => rotation_size,
            _ => move_size,
        };
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

fn pick_rotation_axis(
    ray_origin: Vec3,
    ray_direction: Vec3,
    center: Vec3,
    ring_radius: f32,
    tolerance: f32,
) -> Option<Vec3> {
    [Vec3::X, Vec3::Y, Vec3::Z]
        .into_iter()
        .filter_map(|axis| {
            let radial = ray_plane_vector(ray_origin, ray_direction, center, axis)?;
            let error = (radial.length() - ring_radius).abs();
            (error <= tolerance).then_some((axis, error))
        })
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(axis, _)| axis)
}

fn ray_plane_vector(
    ray_origin: Vec3,
    ray_direction: Vec3,
    center: Vec3,
    normal: Vec3,
) -> Option<Vec3> {
    let denominator = ray_direction.dot(normal);
    if denominator.abs() <= 1.0e-6 {
        return None;
    }
    let distance = (center - ray_origin).dot(normal) / denominator;
    if distance <= 0.0 {
        return None;
    }
    Some(ray_origin + ray_direction * distance - center)
}

fn ray_plane_direction(
    ray_origin: Vec3,
    ray_direction: Vec3,
    center: Vec3,
    normal: Vec3,
) -> Option<Vec3> {
    ray_plane_vector(ray_origin, ray_direction, center, normal)?.try_normalize()
}

fn signed_angle(from: Vec3, to: Vec3, axis: Vec3) -> f32 {
    axis.dot(from.cross(to)).atan2(from.dot(to))
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

fn rotation_about(center: Vec3, axis: Vec3, radians: f32) -> Transform {
    let rotation = Quat::from_axis_angle(axis, radians);
    Transform {
        translation: center - rotation * center,
        rotation,
        ..default()
    }
}

fn set_visual_preview(
    mesh_index: usize,
    preview: Transform,
    visuals: &mut Query<(&FemPartVisual, &mut Transform)>,
) {
    for (part, mut transform) in visuals.iter_mut() {
        if part.mesh_index == mesh_index {
            *transform = preview;
        }
    }
}

fn shift_pressed(keyboard: &ButtonInput<KeyCode>) -> bool {
    keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight)
}

fn control_pressed(keyboard: &ButtonInput<KeyCode>) -> bool {
    keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight)
}

fn cancel_drag(
    state: &mut AssemblyEditorState,
    visuals: &mut Query<(&FemPartVisual, &mut Transform)>,
    mode: &mut InteractionMode,
) {
    if let Some(drag) = state.drag.take() {
        set_visual_preview(drag.mesh_index, Transform::default(), visuals);
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
            point_segment_distance(Vec2::new(5.0, 2.0), Vec2::ZERO, Vec2::new(10.0, 0.0),),
            2.0
        );
    }

    #[test]
    fn reference_size_uses_the_selected_part_diagonal() {
        let model = FemModel::demo_hex8();
        let expected = Vec3::new(2.0, 1.0, 1.0).length();

        assert!((reference_size(&model, 0) - expected).abs() < 1.0e-6);
    }

    #[test]
    fn signed_rotation_angle_uses_the_requested_world_axis() {
        let angle = signed_angle(Vec3::X, Vec3::Y, Vec3::Z);
        assert!((angle.to_degrees() - 90.0).abs() < 1.0e-5);
        let reverse = signed_angle(Vec3::Y, Vec3::X, Vec3::Z);
        assert!((reverse.to_degrees() + 90.0).abs() < 1.0e-5);
    }

    #[test]
    fn rotation_preview_keeps_the_part_centroid_fixed() {
        let center = Vec3::new(4.0, -2.0, 1.0);
        let transform = rotation_about(center, Vec3::Z, 45.0_f32.to_radians());
        assert!(transform.transform_point(center).distance(center) < 1.0e-5);
    }

    #[test]
    fn rotation_ring_pick_chooses_the_plane_with_the_closest_radius() {
        let picked =
            pick_rotation_axis(Vec3::new(0.92, 0.0, 5.0), -Vec3::Z, Vec3::ZERO, 0.92, 0.05);
        assert_eq!(picked, Some(Vec3::Z));
    }
}
