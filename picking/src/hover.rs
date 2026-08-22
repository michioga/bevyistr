use bevy::prelude::*;
use fem_core::{FemEntityId, FemMesh, FemModel, SelectionFilter, SelectionLevel, UiPointerState};

use interaction::HoverResult;

use selection::{Hovered, Selectable};

pub fn hover_system(
    mut commands: Commands,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    selectable_query: Query<(Entity, &GlobalTransform, &Selectable)>,
    hovered_query: Query<Entity, With<Hovered>>,
    filter: Res<SelectionFilter>,
    ui_pointer: Res<UiPointerState>,
    model: Option<Res<FemModel>>,
    mut hover_result: ResMut<HoverResult>,
) {
    if ui_pointer.over_ui {
        clear_hovered(&mut commands, &hovered_query);
        hover_result.clear();

        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };

    let Some(cursor_pos) = window.cursor_position() else {
        clear_hovered(&mut commands, &hovered_query);
        hover_result.clear();

        return;
    };

    let Ok((camera, camera_transform)) = camera_query.single() else {
        clear_hovered(&mut commands, &hovered_query);
        hover_result.clear();

        return;
    };

    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else {
        clear_hovered(&mut commands, &hovered_query);
        hover_result.clear();

        return;
    };

    clear_hovered(&mut commands, &hovered_query);

    let mut best_entity = None;
    let mut best_target = None;
    let mut best_position = None;
    let mut best_level = None;
    let mut best_distance = f32::MAX;

    for (entity, transform, selectable) in selectable_query.iter() {
        let level = selectable.level();

        if !filter.accepts(level) {
            continue;
        }

        let pos = transform.translation();

        let to_center = pos - ray.origin;

        let projected = to_center.dot(*ray.direction);

        if projected < 0.0 {
            continue;
        }

        let closest = ray.origin + *ray.direction * projected;

        let dist = pos.distance(closest);

        if dist < 1.0 && projected < best_distance {
            best_distance = projected;
            best_entity = Some(entity);
            best_target = Some(selectable.target);
            best_position = Some(pos);
            best_level = Some(level);
        }
    }

    if let (Some(entity), Some(target), Some(position), Some(_level)) =
        (best_entity, best_target, best_position, best_level)
    {
        commands.entity(entity).insert(Hovered);
        hover_result.set_entity(entity, target, position);
    } else if let Some((target, point)) =
        model.and_then(|model| pick_model(&model, ray.origin, *ray.direction, filter.level))
    {
        hover_result.set_target(target, point);
    } else {
        hover_result.clear();
    }
}

fn clear_hovered(commands: &mut Commands, hovered_query: &Query<Entity, With<Hovered>>) {
    for entity in hovered_query.iter() {
        commands.entity(entity).remove::<Hovered>();
    }
}

fn pick_model(
    model: &FemModel,
    origin: Vec3,
    direction: Vec3,
    level: SelectionLevel,
) -> Option<(FemEntityId, Vec3)> {
    let threshold = model.bounds().map(selection_threshold).unwrap_or(0.05);
    let mut best = None;

    for mesh in &model.meshes {
        let candidate = match level {
            SelectionLevel::Node => pick_node(mesh, origin, direction, threshold),
            SelectionLevel::Edge => pick_edge(mesh, origin, direction, threshold),
            SelectionLevel::Face => pick_boundary_face(mesh, origin, direction, false),
            SelectionLevel::Element => pick_boundary_face(mesh, origin, direction, true),
        };

        let Some(candidate) = candidate else {
            continue;
        };

        if best
            .as_ref()
            .is_none_or(|(_, _, distance)| candidate.2 < *distance)
        {
            best = Some(candidate);
        }
    }

    best.map(|(target, point, _)| (target, point))
}

/// Picks the mesh node closest to the ray's origin among nodes within
/// `threshold` of the ray.
///
/// [`FemMesh::node_indices_along_ray`] narrows the candidates via the node
/// BVH (expanded by `threshold` into a cylinder around the ray) before the
/// exact per-node distance check below.
fn pick_node(
    mesh: &FemMesh,
    origin: Vec3,
    direction: Vec3,
    threshold: f32,
) -> Option<(FemEntityId, Vec3, f32)> {
    let mut best = None;

    for node_index in mesh.node_indices_along_ray(origin, direction, threshold) {
        let Some(node) = mesh.nodes.get(node_index) else {
            continue;
        };

        let to_node = node.position - origin;
        let projected = to_node.dot(direction);

        if projected < 0.0 {
            continue;
        }

        let closest = origin + direction * projected;
        let distance_to_ray = node.position.distance(closest);

        if distance_to_ray <= threshold
            && best
                .as_ref()
                .is_none_or(|(_, _, distance)| projected < *distance)
        {
            best = Some((FemEntityId::Node(node.id), node.position, projected));
        }
    }

    best
}

/// Picks the boundary edge closest to the ray's origin among edges within
/// `threshold` of the ray.
///
/// [`FemMesh::boundary_edge_indices_along_ray`] narrows the candidates via
/// the boundary edge BVH (expanded by `threshold`) before the exact
/// [`ray_segment_distance`] check below.
fn pick_edge(
    mesh: &FemMesh,
    origin: Vec3,
    direction: Vec3,
    threshold: f32,
) -> Option<(FemEntityId, Vec3, f32)> {
    let mut best = None;
    let edges = mesh.cached_boundary_edges();

    for edge_index in mesh.boundary_edge_indices_along_ray(origin, direction, threshold) {
        let edge = &edges[edge_index];

        let Some(start) = mesh.node_position(edge.nodes[0]) else {
            continue;
        };
        let Some(end) = mesh.node_position(edge.nodes[1]) else {
            continue;
        };
        let Some((distance_to_ray, projected, point)) =
            ray_segment_distance(origin, direction, start, end)
        else {
            continue;
        };

        if distance_to_ray <= threshold
            && best
                .as_ref()
                .is_none_or(|(_, _, distance)| projected < *distance)
        {
            best = Some((FemEntityId::Edge(edge.id), point, projected));
        }
    }

    best
}

/// Picks the boundary face (or its owning element, if `select_element`)
/// closest to the camera along the given ray.
///
/// [`FemMesh::boundary_face_indices_along_ray`] uses the mesh's cached BVH
/// to narrow the search to faces whose AABB the ray actually crosses,
/// before the exact per-triangle [`ray_triangle_intersection`] test below —
/// this is the difference between a linear scan over every boundary face
/// and a roughly O(log n + k) query on large meshes.
fn pick_boundary_face(
    mesh: &FemMesh,
    origin: Vec3,
    direction: Vec3,
    select_element: bool,
) -> Option<(FemEntityId, Vec3, f32)> {
    let mut best = None;
    let faces = mesh.cached_boundary_faces();

    for face_index in mesh.boundary_face_indices_along_ray(origin, direction) {
        let face = &faces[face_index];

        let Some(points) = mesh.node_positions(&face.nodes) else {
            continue;
        };

        if points.len() < 3 {
            continue;
        }

        let target = if select_element {
            let Some(element) = face.element else {
                continue;
            };

            FemEntityId::Element(element)
        } else {
            FemEntityId::Face(face.id)
        };

        for triangle_index in 1..(points.len() - 1) {
            let triangle = [points[0], points[triangle_index], points[triangle_index + 1]];
            let Some(distance) = ray_triangle_intersection(origin, direction, triangle) else {
                continue;
            };

            if best
                .as_ref()
                .is_none_or(|(_, _, best_distance)| distance < *best_distance)
            {
                best = Some((target, origin + direction * distance, distance));
            }
        }
    }

    best
}

fn ray_triangle_intersection(origin: Vec3, direction: Vec3, triangle: [Vec3; 3]) -> Option<f32> {
    let edge_a = triangle[1] - triangle[0];
    let edge_b = triangle[2] - triangle[0];
    let p = direction.cross(edge_b);
    let determinant = edge_a.dot(p);

    if determinant.abs() <= f32::EPSILON {
        return None;
    }

    let inverse_determinant = determinant.recip();
    let t = origin - triangle[0];
    let u = t.dot(p) * inverse_determinant;

    if !(0.0..=1.0).contains(&u) {
        return None;
    }

    let q = t.cross(edge_a);
    let v = direction.dot(q) * inverse_determinant;

    if v < 0.0 || u + v > 1.0 {
        return None;
    }

    let distance = edge_b.dot(q) * inverse_determinant;

    (distance > 0.0).then_some(distance)
}

fn ray_segment_distance(
    origin: Vec3,
    direction: Vec3,
    start: Vec3,
    end: Vec3,
) -> Option<(f32, f32, Vec3)> {
    let segment = end - start;
    let segment_length_sq = segment.length_squared();

    if segment_length_sq <= f32::EPSILON {
        return None;
    }

    let w0 = origin - start;
    let ray_dot_segment = direction.dot(segment);
    let ray_dot_w0 = direction.dot(w0);
    let segment_dot_w0 = segment.dot(w0);
    let denominator = segment_length_sq - ray_dot_segment * ray_dot_segment;

    let (mut ray_t, mut segment_t) = if denominator.abs() <= f32::EPSILON {
        (0.0, (segment_dot_w0 / segment_length_sq).clamp(0.0, 1.0))
    } else {
        (
            (ray_dot_segment * segment_dot_w0 - segment_length_sq * ray_dot_w0) / denominator,
            (segment_dot_w0 - ray_dot_segment * ray_dot_w0) / denominator,
        )
    };

    if ray_t < 0.0 {
        ray_t = 0.0;
        segment_t = (segment_dot_w0 / segment_length_sq).clamp(0.0, 1.0);
    } else if segment_t < 0.0 {
        segment_t = 0.0;
        ray_t = (-ray_dot_w0).max(0.0);
    } else if segment_t > 1.0 {
        segment_t = 1.0;
        ray_t = (ray_dot_segment - ray_dot_w0).max(0.0);
    }

    let ray_point = origin + direction * ray_t;
    let segment_point = start + segment * segment_t;

    Some((ray_point.distance(segment_point), ray_t, segment_point))
}

fn selection_threshold((min, max): (Vec3, Vec3)) -> f32 {
    (max - min).length().max(1.0) * 0.003
}
