//! Surface selection growth from a seed face (or the boundary face hit on a
//! seed element).
//!
//! Two deliberately different operations are provided:
//!
//! - **Coplanar** compares every candidate with the original seed normal, so
//!   normal drift cannot walk around a cylinder or fillet.
//! - **Smooth** compares each candidate with its immediate predecessor, so a
//!   gently faceted curved surface can be followed step by step.

use std::{collections::HashMap, sync::Arc};

use bevy::math::Vec3;

use crate::{ElementId, FaceId, FemMesh, NodeId};

// ─── public API ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NormalReference {
    Seed,
    Predecessor,
}

/// Selects a connected, nearly planar surface patch.
///
/// Every candidate normal is compared with the original seed normal. This is
/// the operation to use for flat CAD faces split into many FEM facets.
pub fn expand_coplanar_from_face(
    mesh: &FemMesh,
    seed_face: FaceId,
    threshold_deg: f32,
) -> (Vec<FaceId>, Vec<ElementId>) {
    expand_faces(mesh, seed_face, threshold_deg, NormalReference::Seed)
}

/// Selects a connected smooth surface patch.
///
/// Each candidate normal is compared with the face from which the traversal
/// reached it. This allows gradual normal drift around cylinders and fillets.
pub fn expand_smooth_from_face(
    mesh: &FemMesh,
    seed_face: FaceId,
    threshold_deg: f32,
) -> (Vec<FaceId>, Vec<ElementId>) {
    expand_faces(mesh, seed_face, threshold_deg, NormalReference::Predecessor)
}

/// Data shared between successive Coplanar and Smooth queries on a mesh.
#[derive(Debug)]
pub(crate) struct SurfaceGrowthTopology {
    face_indices: HashMap<FaceId, usize>,
    normals: Vec<Option<Vec3>>,
    face_edges: Vec<Vec<usize>>,
    edge_faces: Vec<Vec<usize>>,
}

impl SurfaceGrowthTopology {
    fn build(mesh: &FemMesh) -> Self {
        let faces = mesh.cached_boundary_faces();
        let mut edge_indices = HashMap::new();
        let mut edge_faces = Vec::<Vec<usize>>::new();
        let mut face_edges = Vec::with_capacity(faces.len());
        let mut normals = Vec::with_capacity(faces.len());

        for (face_index, face) in faces.iter().enumerate() {
            let points: Vec<_> = face
                .nodes
                .iter()
                .filter_map(|&node| mesh.node_position(node))
                .collect();
            normals.push(face_normal(&points));

            let mut edges = Vec::with_capacity(face.nodes.len());
            for i in 0..face.nodes.len() {
                let edge = sort_pair(face.nodes[i], face.nodes[(i + 1) % face.nodes.len()]);
                let index = *edge_indices.entry(edge).or_insert_with(|| {
                    edge_faces.push(Vec::new());
                    edge_faces.len() - 1
                });
                edge_faces[index].push(face_index);
                edges.push(index);
            }
            face_edges.push(edges);
        }

        Self {
            face_indices: faces
                .iter()
                .enumerate()
                .map(|(i, face)| (face.id, i))
                .collect(),
            normals,
            face_edges,
            edge_faces,
        }
    }
}

fn expand_faces(
    mesh: &FemMesh,
    seed_face: FaceId,
    threshold_deg: f32,
    normal_reference: NormalReference,
) -> (Vec<FaceId>, Vec<ElementId>) {
    let faces = mesh.cached_boundary_faces();
    if threshold_deg < 0.0 || faces.is_empty() {
        return (vec![seed_face], vec![]);
    }

    let cos_threshold = threshold_deg.to_radians().cos().abs();
    let topology = mesh
        .topology
        .surface_growth
        .get_or_init(|| Arc::new(SurfaceGrowthTopology::build(mesh)));
    let Some(&seed) = topology.face_indices.get(&seed_face) else {
        return (vec![seed_face], vec![]);
    };
    let Some(seed_normal) = topology.normals[seed] else {
        return (vec![seed_face], vec![]);
    };

    let mut visited = vec![false; faces.len()];
    let mut selected = vec![seed];
    visited[seed] = true;
    let mut next = 0;

    while let Some(&current) = selected.get(next) {
        next += 1;
        let Some(current_normal) = topology.normals[current] else {
            continue;
        };
        let reference_normal = match normal_reference {
            NormalReference::Seed => seed_normal,
            NormalReference::Predecessor => current_normal,
        };

        for &edge in &topology.face_edges[current] {
            for &neighbour in &topology.edge_faces[edge] {
                if visited[neighbour] {
                    continue;
                }
                let cos = topology.normals[neighbour]
                    .map(|normal| reference_normal.dot(normal).abs())
                    .unwrap_or(0.0);
                if cos >= cos_threshold {
                    visited[neighbour] = true;
                    selected.push(neighbour);
                }
            }
        }
    }

    let mut face_ids: Vec<_> = selected.iter().map(|&i| faces[i].id).collect();
    face_ids.sort_unstable();
    let mut element_ids: Vec<_> = selected.iter().filter_map(|&i| faces[i].element).collect();
    element_ids.sort_unstable();
    element_ids.dedup();

    (face_ids, element_ids)
}
/// Same as [`expand_coplanar_from_face`] but starts from the first
/// boundary face that belongs to `seed_element`, useful when the user
/// is in Element filter mode and clicks an element rather than a face.
pub fn expand_coplanar_from_element(
    mesh: &FemMesh,
    seed_element: ElementId,
    threshold_deg: f32,
) -> (Vec<FaceId>, Vec<ElementId>) {
    let seed_face = mesh
        .cached_boundary_faces()
        .iter()
        .find(|f| f.element == Some(seed_element))
        .map(|f| f.id);

    match seed_face {
        Some(fid) => expand_coplanar_from_face(mesh, fid, threshold_deg),
        None => (vec![], vec![seed_element]),
    }
}

/// Element counterpart to [`expand_smooth_from_face`].
pub fn expand_smooth_from_element(
    mesh: &FemMesh,
    seed_element: ElementId,
    threshold_deg: f32,
) -> (Vec<FaceId>, Vec<ElementId>) {
    let seed_face = mesh
        .cached_boundary_faces()
        .iter()
        .find(|face| face.element == Some(seed_element))
        .map(|face| face.id);

    match seed_face {
        Some(face_id) => expand_smooth_from_face(mesh, face_id, threshold_deg),
        None => (vec![], vec![seed_element]),
    }
}

// ─── helpers ─────────────────────────────────────────────────────────────────

fn sort_pair(a: NodeId, b: NodeId) -> (NodeId, NodeId) {
    if a.0 <= b.0 { (a, b) } else { (b, a) }
}

/// Newell's method for a polygon normal (handles non-planar polys gracefully).
/// Returns `None` for degenerate (zero-area) faces.
fn face_normal(pts: &[Vec3]) -> Option<Vec3> {
    if pts.len() < 3 {
        return None;
    }
    let mut n = Vec3::ZERO;
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        n.x += (a.y - b.y) * (a.z + b.z);
        n.y += (a.z - b.z) * (a.x + b.x);
        n.z += (a.x - b.x) * (a.y + b.y);
    }
    n.try_normalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ElementType, FemElement, FemNode};

    fn three_patch_strip() -> FemMesh {
        let slope_10 = 10.0_f32.to_radians().tan();
        let slope_20 = 20.0_f32.to_radians().tan();
        let heights = [0.0, 0.0, slope_10, slope_10 + slope_20];

        let mut nodes = Vec::new();
        for (station, &height) in heights.iter().enumerate() {
            nodes.push(FemNode::new(
                NodeId((station * 2) as u32),
                Vec3::new(station as f32, 0.0, height),
            ));
            nodes.push(FemNode::new(
                NodeId((station * 2 + 1) as u32),
                Vec3::new(station as f32, 1.0, height),
            ));
        }

        let elements = (0..3)
            .map(|index| {
                FemElement::new(
                    ElementId(index as u32),
                    ElementType::ShellQuad4,
                    vec![
                        NodeId((index * 2) as u32),
                        NodeId((index * 2 + 2) as u32),
                        NodeId((index * 2 + 3) as u32),
                        NodeId((index * 2 + 1) as u32),
                    ],
                )
            })
            .collect();

        FemMesh::new(nodes, elements)
    }

    #[test]
    fn coplanar_does_not_accumulate_normal_drift_but_smooth_does() {
        let mesh = three_patch_strip();
        let seed = mesh
            .cached_boundary_faces()
            .iter()
            .find(|face| face.element == Some(ElementId(0)))
            .expect("the first shell element has one boundary face")
            .id;

        let (coplanar_faces, _) = expand_coplanar_from_face(&mesh, seed, 12.0);
        let (strict_coplanar_faces, _) = expand_coplanar_from_face(&mesh, seed, 0.5);
        let (smooth_faces, _) = expand_smooth_from_face(&mesh, seed, 12.0);

        assert_eq!(coplanar_faces.len(), 2);
        assert_eq!(strict_coplanar_faces.len(), 1);
        assert_eq!(smooth_faces.len(), 3);
    }

    #[test]
    fn surface_growth_rebuilds_after_geometry_and_connectivity_edits() {
        let mesh = three_patch_strip();
        let seed = mesh
            .cached_boundary_faces()
            .iter()
            .find(|face| face.element == Some(ElementId(0)))
            .unwrap()
            .id;
        assert!(mesh.topology.surface_growth.get().is_none());
        assert_eq!(
            expand_coplanar_from_face(&mesh, seed, 0.5).1,
            [ElementId(0)]
        );
        let initial_cache = mesh.topology.surface_growth.get().unwrap();
        assert_eq!(
            expand_smooth_from_face(&mesh, seed, 12.0).1,
            [ElementId(0), ElementId(1), ElementId(2)]
        );
        assert!(Arc::ptr_eq(
            initial_cache,
            mesh.topology.surface_growth.get().unwrap()
        ));

        let mut edited = mesh.clone();
        for node in &mut edited.nodes {
            node.position.z = 0.0;
        }
        edited.rebuild_topology_cache();
        assert!(edited.topology.surface_growth.get().is_none());
        assert_eq!(
            expand_coplanar_from_element(&edited, ElementId(0), 0.5).1,
            [ElementId(0), ElementId(1), ElementId(2)]
        );

        edited.elements.remove(1);
        edited.rebuild_topology_cache();
        assert_eq!(
            expand_smooth_from_element(&edited, ElementId(0), 12.0).1,
            [ElementId(0)]
        );
        assert_eq!(
            expand_smooth_from_face(&mesh, seed, 12.0).1,
            [ElementId(0), ElementId(1), ElementId(2)]
        );
    }
}
