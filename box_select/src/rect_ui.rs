use bevy::prelude::*;
use fem_core::{
    Aabb, FemEntityId, FemEntityRef, FemMesh, FemModel, InteractionMode, NodeId, SelectionFilter,
    SelectionLevel, UiPointerState,
};
use selection::{Selectable, Selected, SelectionState};
use std::collections::HashMap;

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
    mut query: Query<
        (
            &mut Node,
            &mut Visibility,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<SelectionRect>,
    >,
) {
    let Ok((mut node, mut visibility, mut background, mut border)) = query.single_mut() else {
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

    if state.current.x < state.start.x {
        // Right-to-left: crossing selection, matching the familiar CAD /
        // SketchUp convention. Green makes the broader gesture visible
        // before release even though Bevy UI has no dashed border style.
        *background = BackgroundColor(Color::srgba(0.15, 0.75, 0.45, 0.14));
        *border = BorderColor::all(Color::srgb(0.25, 0.9, 0.55));
    } else {
        // Left-to-right: window selection (complete containment only).
        *background = BackgroundColor(Color::srgba(0.2, 0.5, 1.0, 0.15));
        *border = BorderColor::all(Color::srgb(0.4, 0.7, 1.0));
    }
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
    let crossing = state.current.x < state.start.x;

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

    // Query the model geometry directly, including aggregate-rendered meshes
    // that have no per-topology Selectable entities.
    if let Some(model) = model.as_deref() {
        select_model_targets_in_rect(
            model,
            camera,
            camera_transform,
            min,
            max,
            crossing,
            filter.level,
            &mut selection,
        );
    }

    // Keep ECS marker state synchronized for small meshes that still have
    // one Selectable entity per topology item. Exact geometry selection is
    // performed from FemModel above; the entity transform is only a fallback
    // for standalone Selectables that are not backed by a model.
    for (entity, transform, selectable) in selectable_query.iter() {
        if !filter.accepts(selectable.level()) {
            continue;
        }

        let selected = if model.is_some() {
            selection.targets.contains(&selectable.target)
        } else {
            camera
                .world_to_viewport(camera_transform, transform.translation())
                .is_ok_and(|screen_pos| point_in_rect(screen_pos, min, max))
        };

        if !selected {
            continue;
        }

        commands.entity(entity).insert(Selected);

        if !selection.entities.contains(&entity) {
            selection.entities.push(entity);
        }

        push_target(&mut selection, selectable.target);
    }
}

/// Adds mesh-scoped [`FemEntityRef`] targets matched by the screen rectangle.
///
/// For each mesh, [`screen_rect_world_bounds`] gives a broad-phase
/// world-space AABB that is queried via the appropriate BVH
/// (`node_indices_in_aabb`, `boundary_edge_indices_in_aabb`, or
/// `boundary_face_indices_in_aabb`, depending on `level`). Each candidate is
/// then checked in screen space using its actual point, segment, polygon, or
/// element nodes.
fn select_model_targets_in_rect(
    model: &FemModel,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    min: Vec2,
    max: Vec2,
    crossing: bool,
    level: SelectionLevel,
    selection: &mut SelectionState,
) {
    let Some(world_bounds) = screen_rect_world_bounds(camera, camera_transform, min, max, model) else {
        return;
    };

    for (mesh_index, mesh) in model.meshes.iter().enumerate() {
        match level {
            SelectionLevel::Node => {
                for index in mesh.node_indices_in_aabb(world_bounds) {
                    let Some(node) = mesh.nodes.get(index) else {
                        continue;
                    };

                    if project_node(mesh, node.id, camera, camera_transform)
                        .is_some_and(|point| point_in_rect(point, min, max))
                    {
                        push_target(selection, FemEntityRef::node(mesh_index, node.id));
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

                    let Ok(start) = camera.world_to_viewport(camera_transform, start) else {
                        continue;
                    };
                    let Ok(end) = camera.world_to_viewport(camera_transform, end) else {
                        continue;
                    };

                    let matches = if crossing {
                        segment_intersects_rect(start, end, min, max)
                    } else {
                        point_in_rect(start, min, max) && point_in_rect(end, min, max)
                    };

                    if matches {
                        push_target(selection, FemEntityRef::edge(mesh_index, edge.id));
                    }
                }
            }
            SelectionLevel::Face | SelectionLevel::Element => {
                let faces = mesh.cached_boundary_faces();
                let elements_by_id = (level == SelectionLevel::Element).then(|| {
                    mesh.elements
                        .iter()
                        .map(|element| (element.id, element))
                        .collect::<HashMap<_, _>>()
                });

                for index in mesh.boundary_face_indices_in_aabb(world_bounds) {
                    let face = &faces[index];

                    let Some(points) = project_nodes(mesh, &face.nodes, camera, camera_transform)
                    else {
                        continue;
                    };

                    if !polygon_matches_rect(&points, min, max, crossing) {
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

                    let target = FemEntityRef::new(mesh_index, target);

                    if level == SelectionLevel::Element && !crossing {
                        let FemEntityId::Element(element_id) = target.entity else {
                            continue;
                        };
                        let Some(element) = elements_by_id
                            .as_ref()
                            .and_then(|elements| elements.get(&element_id))
                        else {
                            continue;
                        };
                        let Some(element_points) =
                            project_nodes(mesh, &element.nodes, camera, camera_transform)
                        else {
                            continue;
                        };

                        if !element_points
                            .iter()
                            .all(|&point| point_in_rect(point, min, max))
                        {
                            continue;
                        }
                    }

                    push_target(selection, target);
                }

                if level == SelectionLevel::Element {
                    // Line/truss/beam/connector elements have no faces and
                    // therefore never appear in the boundary-face BVH.
                    for element in mesh
                        .elements
                        .iter()
                        .filter(|element| element.face_node_ids().is_empty())
                    {
                        let Some(points) =
                            project_nodes(mesh, &element.nodes, camera, camera_transform)
                        else {
                            continue;
                        };

                        let matches = if crossing {
                            let edges = element.edge_node_ids();
                            if edges.is_empty() {
                                points
                                    .iter()
                                    .any(|&point| point_in_rect(point, min, max))
                            } else {
                                edges.into_iter().any(|edge| {
                                    let Some(edge_points) =
                                        project_nodes(mesh, &edge, camera, camera_transform)
                                    else {
                                        return false;
                                    };
                                    segment_intersects_rect(
                                        edge_points[0],
                                        edge_points[1],
                                        min,
                                        max,
                                    )
                                })
                            }
                        } else {
                            points
                                .iter()
                                .all(|&point| point_in_rect(point, min, max))
                        };

                        if matches {
                            push_target(
                                selection,
                                FemEntityRef::element(mesh_index, element.id),
                            );
                        }
                    }
                }
            }
        }
    }
}

fn push_target(selection: &mut SelectionState, target: FemEntityRef) {
    if !selection.targets.contains(&target) {
        selection.targets.push(target);
    }
    if !selection.highlight_targets.contains(&target) {
        selection.highlight_targets.push(target);
    }
}

fn project_node(
    mesh: &FemMesh,
    node_id: NodeId,
    camera: &Camera,
    camera_transform: &GlobalTransform,
) -> Option<Vec2> {
    let position = mesh.node_position(node_id)?;
    camera
        .world_to_viewport(camera_transform, position)
        .ok()
}

fn project_nodes(
    mesh: &FemMesh,
    node_ids: &[NodeId],
    camera: &Camera,
    camera_transform: &GlobalTransform,
) -> Option<Vec<Vec2>> {
    node_ids
        .iter()
        .map(|&node_id| project_node(mesh, node_id, camera, camera_transform))
        .collect()
}

fn point_in_rect(point: Vec2, min: Vec2, max: Vec2) -> bool {
    point.x >= min.x && point.x <= max.x && point.y >= min.y && point.y <= max.y
}

fn polygon_matches_rect(points: &[Vec2], min: Vec2, max: Vec2, crossing: bool) -> bool {
    if points.is_empty() {
        return false;
    }

    if !crossing {
        return points
            .iter()
            .all(|&point| point_in_rect(point, min, max));
    }

    if points.iter().any(|&point| point_in_rect(point, min, max)) {
        return true;
    }

    if points.len() == 1 {
        return false;
    }

    if points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .any(|(&start, &end)| segment_intersects_rect(start, end, min, max))
    {
        return true;
    }

    if points.len() < 3 {
        return false;
    }

    let rect_corners = [
        min,
        Vec2::new(max.x, min.y),
        max,
        Vec2::new(min.x, max.y),
    ];
    rect_corners
        .into_iter()
        .any(|corner| point_in_polygon(corner, points))
}

fn segment_intersects_rect(start: Vec2, end: Vec2, min: Vec2, max: Vec2) -> bool {
    if point_in_rect(start, min, max) || point_in_rect(end, min, max) {
        return true;
    }

    let corners = [
        min,
        Vec2::new(max.x, min.y),
        max,
        Vec2::new(min.x, max.y),
    ];

    corners
        .into_iter()
        .zip(corners.into_iter().cycle().skip(1))
        .take(corners.len())
        .any(|(rect_start, rect_end)| segments_intersect(start, end, rect_start, rect_end))
}

fn segments_intersect(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    const EPSILON: f32 = 1.0e-5;

    fn cross(a: Vec2, b: Vec2, c: Vec2) -> f32 {
        (b - a).perp_dot(c - a)
    }

    fn on_segment(a: Vec2, b: Vec2, point: Vec2, epsilon: f32) -> bool {
        cross(a, b, point).abs() <= epsilon
            && point.x >= a.x.min(b.x) - epsilon
            && point.x <= a.x.max(b.x) + epsilon
            && point.y >= a.y.min(b.y) - epsilon
            && point.y <= a.y.max(b.y) + epsilon
    }

    let ab_c = cross(a, b, c);
    let ab_d = cross(a, b, d);
    let cd_a = cross(c, d, a);
    let cd_b = cross(c, d, b);

    if ((ab_c > EPSILON && ab_d < -EPSILON) || (ab_c < -EPSILON && ab_d > EPSILON))
        && ((cd_a > EPSILON && cd_b < -EPSILON) || (cd_a < -EPSILON && cd_b > EPSILON))
    {
        return true;
    }

    on_segment(a, b, c, EPSILON)
        || on_segment(a, b, d, EPSILON)
        || on_segment(c, d, a, EPSILON)
        || on_segment(c, d, b, EPSILON)
}

fn point_in_polygon(point: Vec2, polygon: &[Vec2]) -> bool {
    let mut inside = false;
    let mut previous = polygon[polygon.len() - 1];

    for &current in polygon {
        let crosses = (current.y > point.y) != (previous.y > point.y);
        if crosses {
            let x = (previous.x - current.x) * (point.y - current.y)
                / (previous.y - current.y)
                + current.x;
            if point.x < x {
                inside = !inside;
            }
        }
        previous = current;
    }

    inside
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_selection_requires_complete_containment() {
        let min = Vec2::ZERO;
        let max = Vec2::splat(10.0);
        let partly_outside = [
            Vec2::new(2.0, 2.0),
            Vec2::new(12.0, 2.0),
            Vec2::new(12.0, 8.0),
            Vec2::new(2.0, 8.0),
        ];

        assert!(!polygon_matches_rect(&partly_outside, min, max, false));
        assert!(polygon_matches_rect(&partly_outside, min, max, true));
    }

    #[test]
    fn crossing_selection_detects_an_edge_through_the_window() {
        let min = Vec2::ZERO;
        let max = Vec2::splat(10.0);

        assert!(segment_intersects_rect(
            Vec2::new(-2.0, 5.0),
            Vec2::new(12.0, 5.0),
            min,
            max,
        ));
        assert!(!segment_intersects_rect(
            Vec2::new(-2.0, 12.0),
            Vec2::new(12.0, 12.0),
            min,
            max,
        ));
    }

    #[test]
    fn crossing_selection_detects_a_polygon_around_the_window() {
        let min = Vec2::new(4.0, 4.0);
        let max = Vec2::new(6.0, 6.0);
        let surrounding_polygon = [
            Vec2::ZERO,
            Vec2::new(10.0, 0.0),
            Vec2::splat(10.0),
            Vec2::new(0.0, 10.0),
        ];

        assert!(polygon_matches_rect(
            &surrounding_polygon,
            min,
            max,
            true,
        ));
    }
}
