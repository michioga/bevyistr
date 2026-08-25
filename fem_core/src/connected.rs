//! Deterministic topology traversal used by multi-click selection gestures.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use bevy::math::Vec3;

use crate::{EdgeId, ElementId, FaceId, FemEdge, FemElement, FemFace, FemMesh, FemNode, NodeId};

/// Default crease and polyline-turn threshold for CAD-like feature edges.
pub const DEFAULT_FEATURE_EDGE_ANGLE_DEG: f32 = 30.0;

/// Returns the connected boundary-surface component containing `seed_face`.
/// Faces are adjacent when they share a complete corner edge.
pub fn expand_connected_boundary_faces(mesh: &FemMesh, seed_face: FaceId) -> Vec<FaceId> {
    let faces = mesh.cached_boundary_faces();
    let face_by_id: HashMap<_, _> = faces.iter().map(|face| (face.id, face)).collect();
    if !face_by_id.contains_key(&seed_face) {
        return vec![seed_face];
    }

    let mut edge_to_faces = HashMap::<(NodeId, NodeId), Vec<FaceId>>::new();
    for face in faces {
        for index in 0..face.nodes.len() {
            let edge = ordered_pair(face.nodes[index], face.nodes[(index + 1) % face.nodes.len()]);
            edge_to_faces.entry(edge).or_default().push(face.id);
        }
    }

    traverse(seed_face, |face_id| {
        let Some(face) = face_by_id.get(&face_id) else {
            return Vec::new();
        };
        face.nodes
            .iter()
            .copied()
            .zip(face.nodes.iter().copied().cycle().skip(1))
            .take(face.nodes.len())
            .flat_map(|(start, end)| {
                edge_to_faces
                    .get(&ordered_pair(start, end))
                    .into_iter()
                    .flatten()
                    .copied()
            })
            .collect()
    })
}

/// Returns the connected boundary-edge component containing `seed_edge`.
/// Edges are adjacent when they share an endpoint.
pub fn expand_connected_boundary_edges(mesh: &FemMesh, seed_edge: EdgeId) -> Vec<EdgeId> {
    let edges = mesh.cached_boundary_edges();
    let edge_by_id: HashMap<_, _> = edges.iter().map(|edge| (edge.id, edge)).collect();
    if !edge_by_id.contains_key(&seed_edge) {
        return vec![seed_edge];
    }

    let mut node_to_edges = HashMap::<NodeId, Vec<EdgeId>>::new();
    for edge in edges {
        for node in edge.nodes {
            node_to_edges.entry(node).or_default().push(edge.id);
        }
    }

    traverse(seed_edge, |edge_id| {
        let Some(edge) = edge_by_id.get(&edge_id) else {
            return Vec::new();
        };
        edge.nodes
            .iter()
            .flat_map(|node| node_to_edges.get(node).into_iter().flatten().copied())
            .collect()
    })
}

/// Returns the smooth, non-branching feature-edge chain containing
/// `seed_edge`.
///
/// Flat triangulation edges are excluded. Free shell boundaries and creases
/// whose adjacent face normals differ by at least `threshold_deg` are feature
/// edges. Traversal follows the only continuation at each node and stops at
/// branches or turns sharper than the same threshold, so a finely faceted
/// circle is selected as one chain while a rectangular corner remains a
/// boundary between chains.
pub fn expand_continuous_feature_edges(
    mesh: &FemMesh,
    seed_edge: EdgeId,
    threshold_deg: f32,
) -> Vec<EdgeId> {
    let Some(seed) = edge_by_id(mesh, seed_edge) else {
        return vec![seed_edge];
    };
    let feature_edges = feature_edge_ids(mesh, threshold_deg);
    if !feature_edges.contains(&seed_edge) {
        return vec![seed_edge];
    }

    let mut node_to_edges = HashMap::<NodeId, Vec<EdgeId>>::new();
    for edge_id in &feature_edges {
        let Some(edge) = edge_by_id(mesh, *edge_id) else {
            continue;
        };
        for node in edge.nodes {
            node_to_edges.entry(node).or_default().push(edge.id);
        }
    }

    let mut visited = HashSet::from([seed_edge]);
    walk_feature_chain(
        mesh,
        &node_to_edges,
        &mut visited,
        seed_edge,
        seed.nodes[0],
        seed.nodes[1],
        threshold_deg,
    );
    walk_feature_chain(
        mesh,
        &node_to_edges,
        &mut visited,
        seed_edge,
        seed.nodes[1],
        seed.nodes[0],
        threshold_deg,
    );

    let mut result: Vec<_> = visited.into_iter().collect();
    result.sort_unstable();
    result
}

/// Returns every feature edge connected to `seed_edge`, including branches.
pub fn expand_connected_feature_edges(
    mesh: &FemMesh,
    seed_edge: EdgeId,
    threshold_deg: f32,
) -> Vec<EdgeId> {
    let feature_edges = feature_edge_ids(mesh, threshold_deg);
    if edge_by_id(mesh, seed_edge).is_none() || !feature_edges.contains(&seed_edge) {
        return vec![seed_edge];
    }

    let mut node_to_edges = HashMap::<NodeId, Vec<EdgeId>>::new();
    for edge_id in &feature_edges {
        let Some(edge) = edge_by_id(mesh, *edge_id) else {
            continue;
        };
        for node in edge.nodes {
            node_to_edges.entry(node).or_default().push(edge.id);
        }
    }

    traverse(seed_edge, |edge_id| {
        let Some(edge) = edge_by_id(mesh, edge_id) else {
            return Vec::new();
        };
        edge.nodes
            .iter()
            .flat_map(|node| node_to_edges.get(node).into_iter().flatten().copied())
            .collect()
    })
}

fn walk_feature_chain(
    mesh: &FemMesh,
    node_to_edges: &HashMap<NodeId, Vec<EdgeId>>,
    visited: &mut HashSet<EdgeId>,
    mut current_edge: EdgeId,
    mut previous_node: NodeId,
    mut current_node: NodeId,
    threshold_deg: f32,
) {
    loop {
        let candidates: Vec<_> = node_to_edges
            .get(&current_node)
            .into_iter()
            .flatten()
            .copied()
            .filter(|edge| *edge != current_edge && !visited.contains(edge))
            .collect();
        let [next_edge] = candidates.as_slice() else {
            break;
        };
        let Some(edge) = edge_by_id(mesh, *next_edge) else {
            break;
        };
        let next_node = if edge.nodes[0] == current_node {
            edge.nodes[1]
        } else if edge.nodes[1] == current_node {
            edge.nodes[0]
        } else {
            break;
        };
        if edge_turn_degrees(mesh, previous_node, current_node, next_node)
            .is_none_or(|turn| turn > threshold_deg)
        {
            break;
        }

        visited.insert(*next_edge);
        current_edge = *next_edge;
        previous_node = current_node;
        current_node = next_node;
    }
}

fn feature_edge_ids(mesh: &FemMesh, threshold_deg: f32) -> HashSet<EdgeId> {
    if (threshold_deg - DEFAULT_FEATURE_EDGE_ANGLE_DEG).abs() <= f32::EPSILON {
        return mesh.cached_feature_edge_ids().iter().copied().collect();
    }

    derive_feature_edge_ids(
        &mesh.nodes,
        &mesh.topology.node_indices,
        mesh.cached_boundary_faces(),
        mesh.cached_boundary_edges(),
        threshold_deg,
    )
    .into_iter()
    .collect()
}

pub(crate) fn derive_feature_edge_ids(
    nodes: &[FemNode],
    node_indices: &BTreeMap<NodeId, usize>,
    faces: &[FemFace],
    edges: &[FemEdge],
    threshold_deg: f32,
) -> Vec<EdgeId> {
    let node_position = |id: NodeId| {
        node_indices
            .get(&id)
            .and_then(|index| nodes.get(*index))
            .map(|node| node.position)
    };
    let normals: HashMap<_, _> = faces
        .iter()
        .filter_map(|face| {
            let points: Vec<_> = face
                .nodes
                .iter()
                .filter_map(|node| node_position(*node))
                .collect();
            face_normal(&points).map(|normal| (face.id, normal))
        })
        .collect();
    let mut edge_faces = HashMap::<(NodeId, NodeId), Vec<FaceId>>::new();
    for face in faces {
        for index in 0..face.nodes.len() {
            edge_faces
                .entry(ordered_pair(
                    face.nodes[index],
                    face.nodes[(index + 1) % face.nodes.len()],
                ))
                .or_default()
                .push(face.id);
        }
    }
    let cosine = threshold_deg.max(0.0).to_radians().cos();

    edges
        .iter()
        .filter(|edge| {
            let adjacent = edge_faces
                .get(&ordered_pair(edge.nodes[0], edge.nodes[1]))
                .map(Vec::as_slice)
                .unwrap_or_default();
            if adjacent.len() <= 1 {
                return true;
            }

            for first in 0..adjacent.len() {
                for second in (first + 1)..adjacent.len() {
                    let (Some(a), Some(b)) =
                        (normals.get(&adjacent[first]), normals.get(&adjacent[second]))
                    else {
                        return true;
                    };
                    if a.dot(*b).abs() <= cosine {
                        return true;
                    }
                }
            }
            false
        })
        .map(|edge| edge.id)
        .collect()
}

fn edge_by_id(mesh: &FemMesh, edge_id: EdgeId) -> Option<&FemEdge> {
    mesh.cached_edges()
        .get(edge_id.0 as usize)
        .filter(|edge| edge.id == edge_id)
}

fn edge_turn_degrees(
    mesh: &FemMesh,
    previous: NodeId,
    current: NodeId,
    next: NodeId,
) -> Option<f32> {
    let previous = mesh.node_position(previous)?;
    let current = mesh.node_position(current)?;
    let next = mesh.node_position(next)?;
    let incoming = (current - previous).try_normalize()?;
    let outgoing = (next - current).try_normalize()?;
    Some(incoming.dot(outgoing).clamp(-1.0, 1.0).acos().to_degrees())
}

fn face_normal(points: &[Vec3]) -> Option<Vec3> {
    let origin = *points.first()?;
    for first in 1..points.len() {
        let edge = points[first] - origin;
        for second in (first + 1)..points.len() {
            if let Some(normal) = edge.cross(points[second] - origin).try_normalize() {
                return Some(normal);
            }
        }
    }
    None
}

/// Returns the finite-element component containing `seed_element`.
///
/// Adjacency follows the element's intrinsic dimension: solids share a face,
/// shells/plane elements share an edge, and beams/lines share a node. This
/// avoids joining two solid bodies that merely touch at one node.
pub fn expand_connected_elements(mesh: &FemMesh, seed_element: ElementId) -> Vec<ElementId> {
    let element_by_id: HashMap<_, _> = mesh
        .elements
        .iter()
        .map(|element| (element.id, element))
        .collect();
    if !element_by_id.contains_key(&seed_element) {
        return vec![seed_element];
    }

    let mut key_to_elements = HashMap::<ConnectivityKey, Vec<ElementId>>::new();
    let mut element_keys = HashMap::<ElementId, Vec<ConnectivityKey>>::new();

    for element in &mesh.elements {
        let keys = connectivity_keys(element);
        for key in &keys {
            key_to_elements
                .entry(key.clone())
                .or_default()
                .push(element.id);
        }
        element_keys.insert(element.id, keys);
    }

    traverse(seed_element, |element_id| {
        element_keys
            .get(&element_id)
            .into_iter()
            .flatten()
            .flat_map(|key| key_to_elements.get(key).into_iter().flatten().copied())
            .collect()
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ConnectivityKey {
    Face(Vec<NodeId>),
    Edge(NodeId, NodeId),
    Node(NodeId),
}

fn connectivity_keys(element: &FemElement) -> Vec<ConnectivityKey> {
    if element.element_type.is_solid() || element.element_type.is_interface() {
        return element
            .face_node_ids()
            .into_iter()
            .map(|mut nodes| {
                nodes.sort_unstable();
                ConnectivityKey::Face(nodes)
            })
            .collect();
    }

    if element.element_type.is_shell() {
        return element
            .edge_node_ids()
            .into_iter()
            .map(|[start, end]| {
                let (start, end) = ordered_pair(start, end);
                ConnectivityKey::Edge(start, end)
            })
            .collect();
    }

    element
        .nodes
        .iter()
        .copied()
        .map(ConnectivityKey::Node)
        .collect()
}

fn ordered_pair(a: NodeId, b: NodeId) -> (NodeId, NodeId) {
    if a <= b { (a, b) } else { (b, a) }
}

fn traverse<Id, Neighbours>(seed: Id, mut neighbours: Neighbours) -> Vec<Id>
where
    Id: Copy + Eq + std::hash::Hash + Ord,
    Neighbours: FnMut(Id) -> Vec<Id>,
{
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    visited.insert(seed);
    queue.push_back(seed);

    while let Some(current) = queue.pop_front() {
        for neighbour in neighbours(current) {
            if visited.insert(neighbour) {
                queue.push_back(neighbour);
            }
        }
    }

    let mut result: Vec<_> = visited.into_iter().collect();
    result.sort_unstable();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Vec3;
    use crate::{ElementType, FemNode};

    #[test]
    fn all_hex_boundary_faces_form_one_surface_component() {
        let mesh = FemMesh::demo_hex8();
        let seed = mesh.cached_boundary_faces()[0].id;

        assert_eq!(expand_connected_boundary_faces(&mesh, seed).len(), 6);
    }

    #[test]
    fn shell_element_traversal_stops_at_a_disconnected_component() {
        let nodes = vec![
            FemNode::new(NodeId(0), Vec3::new(0.0, 0.0, 0.0)),
            FemNode::new(NodeId(1), Vec3::new(1.0, 0.0, 0.0)),
            FemNode::new(NodeId(2), Vec3::new(1.0, 1.0, 0.0)),
            FemNode::new(NodeId(3), Vec3::new(0.0, 1.0, 0.0)),
            FemNode::new(NodeId(4), Vec3::new(2.0, 0.0, 0.0)),
            FemNode::new(NodeId(5), Vec3::new(2.0, 1.0, 0.0)),
            FemNode::new(NodeId(6), Vec3::new(4.0, 0.0, 0.0)),
            FemNode::new(NodeId(7), Vec3::new(5.0, 0.0, 0.0)),
            FemNode::new(NodeId(8), Vec3::new(5.0, 1.0, 0.0)),
            FemNode::new(NodeId(9), Vec3::new(4.0, 1.0, 0.0)),
        ];
        let elements = vec![
            FemElement::new(
                ElementId(0),
                ElementType::ShellQuad4,
                vec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
            ),
            FemElement::new(
                ElementId(1),
                ElementType::ShellQuad4,
                vec![NodeId(1), NodeId(4), NodeId(5), NodeId(2)],
            ),
            FemElement::new(
                ElementId(2),
                ElementType::ShellQuad4,
                vec![NodeId(6), NodeId(7), NodeId(8), NodeId(9)],
            ),
        ];
        let mesh = FemMesh::new(nodes, elements);

        assert_eq!(
            expand_connected_elements(&mesh, ElementId(0)),
            vec![ElementId(0), ElementId(1)]
        );
        assert_eq!(
            expand_connected_elements(&mesh, ElementId(2)),
            vec![ElementId(2)]
        );
    }

    #[test]
    fn feature_chain_follows_a_straight_free_boundary_but_stops_at_corners() {
        let mesh = two_coplanar_shell_quads();
        let seed = edge_id_between(&mesh, NodeId(0), NodeId(1));
        let continuation = edge_id_between(&mesh, NodeId(1), NodeId(4));

        assert_eq!(
            expand_continuous_feature_edges(
                &mesh,
                seed,
                DEFAULT_FEATURE_EDGE_ANGLE_DEG,
            ),
            sorted(vec![seed, continuation])
        );
    }

    #[test]
    fn connected_feature_edges_exclude_the_coplanar_internal_mesh_edge() {
        let mesh = two_coplanar_shell_quads();
        let seed = edge_id_between(&mesh, NodeId(0), NodeId(1));
        let internal = edge_id_between(&mesh, NodeId(1), NodeId(2));
        let connected = expand_connected_feature_edges(
            &mesh,
            seed,
            DEFAULT_FEATURE_EDGE_ANGLE_DEG,
        );

        assert_eq!(connected.len(), 6);
        assert!(!connected.contains(&internal));
    }

    fn two_coplanar_shell_quads() -> FemMesh {
        let nodes = vec![
            FemNode::new(NodeId(0), Vec3::new(0.0, 0.0, 0.0)),
            FemNode::new(NodeId(1), Vec3::new(1.0, 0.0, 0.0)),
            FemNode::new(NodeId(2), Vec3::new(1.0, 1.0, 0.0)),
            FemNode::new(NodeId(3), Vec3::new(0.0, 1.0, 0.0)),
            FemNode::new(NodeId(4), Vec3::new(2.0, 0.0, 0.0)),
            FemNode::new(NodeId(5), Vec3::new(2.0, 1.0, 0.0)),
        ];
        let elements = vec![
            FemElement::new(
                ElementId(0),
                ElementType::ShellQuad4,
                vec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
            ),
            FemElement::new(
                ElementId(1),
                ElementType::ShellQuad4,
                vec![NodeId(1), NodeId(4), NodeId(5), NodeId(2)],
            ),
        ];
        FemMesh::new(nodes, elements)
    }

    fn edge_id_between(mesh: &FemMesh, first: NodeId, second: NodeId) -> EdgeId {
        let pair = ordered_pair(first, second);
        mesh.cached_boundary_edges()
            .iter()
            .find(|edge| ordered_pair(edge.nodes[0], edge.nodes[1]) == pair)
            .expect("test edge exists")
            .id
    }

    fn sorted(mut edges: Vec<EdgeId>) -> Vec<EdgeId> {
        edges.sort_unstable();
        edges
    }
}
