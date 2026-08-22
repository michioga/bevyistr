use bevy::prelude::*;
use fem_core::{Aabb, FemEntityId, FemModel, InteractionMode, SelectionFilter, SelectionLevel, UiPointerState};
use selection::{Selectable, Selected, SelectionState};

use crate::BoxSelectState;

/// Minimum drag distance, in pixels, for a mouse release to be treated as a
/// box-select gesture rather than a plain click.
///
/// Without this, every click (which still produces a zero-area "box" at the
/// cursor position) would clear the selection that `click_selection_system`
/// just applied on the preceding press, immediately undoing it.
const MIN_DRAG_PX: f32 = 4.0;

/// Multiplier applied to the model's bounding diagonal when unprojecting the
/// selection rectangle into a world-space broad-phase AABB (see
/// [`screen_rect_world_bounds`]). Larger values only widen the BVH
/// broad-phase, never affect the exact screen-space check that follows.
const SCREEN_RECT_DEPTH_MARGIN: f32 = 3.0;

#[derive(Component)]
pub struct SelectionRect;

pub fn spawn_selection_rect(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,

            left: px(0.0),
            top: px(0.0),

            width: px(0.0),
            height: px(0.0),

            border: UiRect::all(px(1.0)),

            ..default()
        },
        BackgroundColor(Color::srgba(0.2, 0.5, 1.0, 0.15)),
        BorderColor::all(Color::srgb(0.4, 0.7, 1.0)),
        Visibility::Hidden,
        SelectionRect,
    ));
}

pub fn begin_box_select(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    mut state: ResMut<BoxSelectState>,
    mut mode: ResMut<InteractionMode>,
    ui_pointer: Res<UiPointerState>,
) {
    if !buttons.just_pressed(MouseButton::Left) || ui_pointer.over_ui {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };

    let Some(cursor) = window.cursor_position() else {
        return;
    };

    state.active = true;
    state.start = cursor;
    state.current = cursor;
    *mode = InteractionMode::BoxSelect;
}

pub fn update_box_select(windows: Query<&Window>, mut state: ResMut<BoxSelectState>) {
    if !state.active {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };

    let Some(cursor) = window.cursor_position() else {
        return;
    };

    state.current = cursor;
}

pub fn end_box_select(
    buttons: Res<ButtonInput<MouseButton>>,
    mut state: ResMut<BoxSelectState>,
    mut mode: ResMut<InteractionMode>,
) {
    if buttons.just_released(MouseButton::Left) {
        state.active = false;

        if *mode == InteractionMode::BoxSelect {
            *mode = InteractionMode::Idle;
        }
    }
}

pub fn update_rect_visual(
    state: Res<BoxSelectState>,
    mut query: Query<(&mut Node, &mut Visibility), With<SelectionRect>>,
) {
    let Ok((mut node, mut visibility)) = query.single_mut() else {
        return;
    };

    if !state.active {
        *visibility = Visibility::Hidden;

        return;
    }

    *visibility = Visibility::Inherited;

    let min = state.start.min(state.current);

    let max = state.start.max(state.current);

    node.left = px(min.x);
    node.top = px(min.y);

    node.width = px(max.x - min.x);
    node.height = px(max.y - min.y);
}

pub fn perform_box_selection(
    mut commands: Commands,

    buttons: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,

    state: Res<BoxSelectState>,
    filter: Res<SelectionFilter>,
    ui_pointer: Res<UiPointerState>,
    mut selection: ResMut<SelectionState>,

    camera_query: Query<(&Camera, &GlobalTransform)>,

    selectable_query: Query<(Entity, &GlobalTransform, &Selectable)>,
    selected_query: Query<Entity, With<Selected>>,

    model: Option<Res<FemModel>>,
) {
    if !buttons.just_released(MouseButton::Left) || ui_pointer.over_ui {
        return;
    }

    let min = state.start.min(state.current);
    let max = state.start.max(state.current);

    if (max - min).length_squared() < MIN_DRAG_PX * MIN_DRAG_PX {
        // Too small to be a deliberate drag; leave whatever
        // `click_selection_system` did on the preceding press untouched.
        return;
    }

    let Ok((camera, camera_transform)) = camera_query.single() else {
        return;
    };

    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);

    if !ctrl {
        for entity in selected_query.iter() {
            commands.entity(entity).remove::<Selected>();
        }

        selection.clear();
    }

    for (entity, transform, selectable) in selectable_query.iter() {
        if !filter.accepts(selectable.level()) {
            continue;
        }

        let world_pos = transform.translation();

        let Ok(screen_pos) = camera.world_to_viewport(camera_transform, world_pos) else {
            continue;
        };

        if screen_pos.x >= min.x
            && screen_pos.x <= max.x
            && screen_pos.y >= min.y
            && screen_pos.y <= max.y
        {
            commands.entity(entity).insert(Selected);

            if !selection.entities.contains(&entity) {
                selection.entities.push(entity);
            }

            if !selection.targets.contains(&selectable.target) {
                selection.targets.push(selectable.target);
            }
        }
    }

    // Meshes large enough to use aggregate rendering have no per-entity
    // `Selectable`s for the loop above to find, so fall back to querying
    // the model's BVHs directly -- the box-select counterpart to
    // `pick_model`'s ray-based fallback in the `picking` crate.
    if let Some(model) = model.as_deref() {
        select_model_targets_in_rect(model, camera, camera_transform, min, max, filter.level, &mut selection);
    }
}

/// Adds [`FemEntityId`] targets whose representative point projects inside
/// the `[min, max]` screen rect to `selection.targets`.
///
/// For each mesh, [`screen_rect_world_bounds`] gives a broad-phase
/// world-space AABB that is queried via the appropriate BVH
/// (`node_indices_in_aabb`, `boundary_edge_indices_in_aabb`, or
/// `boundary_face_indices_in_aabb`, depending on `level`); each candidate is
/// then checked exactly by projecting its representative point (the node
/// position, edge midpoint, or face centroid) back to screen space.
fn select_model_targets_in_rect(
    model: &FemModel,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    min: Vec2,
    max: Vec2,
    level: SelectionLevel,
    selection: &mut SelectionState,
) {
    let Some(world_bounds) = screen_rect_world_bounds(camera, camera_transform, min, max, model) else {
        return;
    };

    let in_rect = |world_pos: Vec3| -> bool {
        let Ok(screen_pos) = camera.world_to_viewport(camera_transform, world_pos) else {
            return false;
        };

        screen_pos.x >= min.x && screen_pos.x <= max.x && screen_pos.y >= min.y && screen_pos.y <= max.y
    };

    for mesh in &model.meshes {
        match level {
            SelectionLevel::Node => {
                for index in mesh.node_indices_in_aabb(world_bounds) {
                    let Some(node) = mesh.nodes.get(index) else {
                        continue;
                    };

                    if in_rect(node.position) {
                        push_target(selection, FemEntityId::Node(node.id));
                    }
                }
            }
            SelectionLevel::Edge => {
                let edges = mesh.cached_boundary_edges();

                for index in mesh.boundary_edge_indices_in_aabb(world_bounds) {
                    let edge = &edges[index];

                    let (Some(start), Some(end)) =
                        (mesh.node_position(edge.nodes[0]), mesh.node_position(edge.nodes[1]))
                    else {
                        continue;
                    };

                    if in_rect((start + end) * 0.5) {
                        push_target(selection, FemEntityId::Edge(edge.id));
                    }
                }
            }
            SelectionLevel::Face | SelectionLevel::Element => {
                let faces = mesh.cached_boundary_faces();

                for index in mesh.boundary_face_indices_in_aabb(world_bounds) {
                    let face = &faces[index];

                    let Some(geometry) = mesh.face_geometry(face) else {
                        continue;
                    };

                    if !in_rect(geometry.centroid) {
                        continue;
                    }

                    let target = if level == SelectionLevel::Element {
                        let Some(element) = face.element else {
                            continue;
                        };

                        FemEntityId::Element(element)
                    } else {
                        FemEntityId::Face(face.id)
                    };

                    push_target(selection, target);
                }
            }
        }
    }
}

fn push_target(selection: &mut SelectionState, target: FemEntityId) {
    if !selection.targets.contains(&target) {
        selection.targets.push(target);
    }
}

/// Computes a generous world-space AABB covering the view frustum slice
/// behind the screen rect `[min, max]`, for use as a [`fem_core::Bvh`]
/// broad-phase filter via `*_indices_in_aabb`.
///
/// Each corner of the rect is unprojected to a ray and extended past the
/// model's center by [`SCREEN_RECT_DEPTH_MARGIN`] times its bounding
/// diagonal, so the result covers the model regardless of camera distance.
/// Returns `None` if the model has no nodes or a corner can't be
/// unprojected (e.g. a degenerate camera transform).
fn screen_rect_world_bounds(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    min: Vec2,
    max: Vec2,
    model: &FemModel,
) -> Option<Aabb> {
    let (model_min, model_max) = model.bounds()?;
    let diagonal = (model_max - model_min).length().max(1.0);
    let center = (model_min + model_max) * 0.5;
    let far = camera_transform.translation().distance(center) + diagonal * SCREEN_RECT_DEPTH_MARGIN;

    let corners = [min, Vec2::new(max.x, min.y), Vec2::new(min.x, max.y), max];
    let mut points = Vec::with_capacity(corners.len() * 2);

    for corner in corners {
        let ray = camera.viewport_to_world(camera_transform, corner).ok()?;

        points.push(ray.origin);
        points.push(ray.origin + *ray.direction * far);
    }

    Aabb::from_points(&points)
}
