//! Surface selection growth from a seed face (or the boundary face hit on a
//! seed element).
//!
//! Two deliberately different operations are provided:
//!
//! - **Coplanar** compares every candidate with the original seed normal, so
//!   normal drift cannot walk around a cylinder or fillet.
//! - **Smooth** compares each candidate with its immediate predecessor, so a
//!   gently faceted curved surface can be followed step by step.

use std::collections::{HashMap, HashSet, VecDeque};

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
    expand_faces(
        mesh,
        seed_face,
        threshold_deg,
        NormalReference::Seed,
    )
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
    expand_faces(
        mesh,
        seed_face,
        threshold_deg,
        NormalReference::Predecessor,
    )
}

fn expand_faces(
    mesh: &FemMesh,
    seed_face: FaceId,
    threshold_deg: f32,
    normal_reference: NormalReference,
) -> (Vec<FaceId>, Vec<ElementId>) {
    if threshold_deg < 0.0 {
        return (vec![seed_face], vec![]);
    }

    let cos_threshold = threshold_deg.to_radians().cos().abs();

    let faces = mesh.cached_boundary_faces();
    if faces.is_empty() {
        return (vec![seed_face], vec![]);
    }

    // ── Build face-lookup and edge→faces adjacency ──────────────────────────
    let face_by_id: HashMap<FaceId, &_> = faces.iter().map(|f| (f.id, f)).collect();

    // Normals cached per face (unit, or None for degenerate faces)
    let normals: HashMap<FaceId, Vec3> = faces.iter().filter_map(|f| {
        let pts: Vec<Vec3> = f.nodes.iter().filter_map(|&n| mesh.node_position(n)).collect();
        face_normal(&pts).map(|n| (f.id, n))
    }).collect();

    // Bail out early if the seed itself has no usable normal (degenerate
    // face) — nothing sensible to compare against.
    let Some(&seed_normal) = normals.get(&seed_face) else {
        return (vec![seed_face], vec![]);
    };

    // Edge (pair of sorted NodeIds) → list of face IDs sharing that edge
    let mut edge_to_faces: HashMap<(NodeId, NodeId), Vec<FaceId>> = HashMap::new();
    for face in faces {
        let nodes = &face.nodes;
        for i in 0..nodes.len() {
            let a = nodes[i];
            let b = nodes[(i + 1) % nodes.len()];
            let edge = sort_pair(a, b);
            edge_to_faces.entry(edge).or_default().push(face.id);
        }
    }

    // ── BFS from seed face ──────────────────────────────────────────────────
    let mut visited: HashSet<FaceId> = HashSet::new();
    let mut queue: VecDeque<FaceId> = VecDeque::new();
    visited.insert(seed_face);
    queue.push_back(seed_face);

    while let Some(current_id) = queue.pop_front() {
        let Some(current_face) = face_by_id.get(&current_id) else { continue; };
        // Every visited face has a normal (guaranteed by the check below,
        // before it was ever inserted into `visited`), so this always hits.
        let Some(&current_normal) = normals.get(&current_id) else { continue; };

        // Collect neighbour faces via shared edges
        let nodes = &current_face.nodes;
        let mut neighbours: HashSet<FaceId> = HashSet::new();
        for i in 0..nodes.len() {
            let edge = sort_pair(nodes[i], nodes[(i + 1) % nodes.len()]);
            if let Some(sharing) = edge_to_faces.get(&edge) {
                for &fid in sharing {
                    if fid != current_id { neighbours.insert(fid); }
                }
            }
        }

        for neighbour_id in neighbours {
            if visited.contains(&neighbour_id) { continue; }

            let reference_normal = match normal_reference {
                NormalReference::Seed => seed_normal,
                NormalReference::Predecessor => current_normal,
            };
            let cos = normals.get(&neighbour_id)
                .map(|normal| reference_normal.dot(*normal).abs())
                .unwrap_or(0.0);

            if cos >= cos_threshold {
                visited.insert(neighbour_id);
                queue.push_back(neighbour_id);
            }
        }
    }

    // Collect element IDs from the selected faces.
    let mut face_ids: Vec<FaceId> = visited.into_iter().collect();
    face_ids.sort_unstable();

    let mut element_ids: Vec<ElementId> = face_ids.iter()
        .filter_map(|fid| face_by_id.get(fid)?.element)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    element_ids.sort_unstable();

    (face_ids, element_ids)
}

/// Same as [`expand_coplanar_from_face`] but starts from the first
/// boundary face that belongs to `seed_element`, useful when the user
/// is in Element filter mode and clicks an element rather than a face.
pub fn expand_coplanar_from_element(
    mesh:          &FemMesh,
    seed_element:  ElementId,
    threshold_deg: f32,
) -> (Vec<FaceId>, Vec<ElementId>) {
    let seed_face = mesh.cached_boundary_faces()
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
    let seed_face = mesh.cached_boundary_faces()
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
    if pts.len() < 3 { return None; }
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
}
