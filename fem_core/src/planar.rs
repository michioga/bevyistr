//! Planar selection: expand a seed face (or the boundary-face of a seed
//! element) to all connected coplanar faces within an angle threshold.
//!
//! This is the core geometry computation behind the "Select Coplanar" UI
//! feature. The result is a `Vec<FaceId>` and a parallel `Vec<ElementId>`
//! that the UI can push into [`SelectionState`].

use std::collections::{HashMap, HashSet, VecDeque};

use bevy::math::Vec3;

use crate::{ElementId, FaceId, FemMesh, NodeId};

// ─── public API ──────────────────────────────────────────────────────────────

/// Expands outward from `seed_face` through the boundary-face adjacency
/// graph of `mesh`, adding each face whose normal makes an angle ≤
/// `threshold_deg` with its **immediate BFS predecessor's** normal (not
/// the seed face's normal — see below).
///
/// Returns `(face_ids, element_ids)` — the caller inserts both into
/// [`SelectionState`] (faces for the Face filter, elements for the Element
/// filter).
///
/// The angle comparison uses **absolute** dot product so that reversed
/// normals (opposite winding on neighbouring faces) still match — in
/// practice HECMW meshes sometimes have inconsistent winding on surface
/// patches exported from CAD.
///
/// Comparing each face to its immediate predecessor (step-by-step) rather
/// than to the fixed seed is deliberate, not incidental: a smoothly curved
/// surface — a cylindrical bore, a fillet — is faceted into many small
/// triangles whose normal changes by only a few degrees from one to the
/// next, but whose normal *accumulates* around a full sweep (a bore's
/// normal is perpendicular to the seed's a quarter of the way around, and
/// anti-parallel halfway around). Comparing against a fixed seed normal
/// means the threshold would have to account for that entire accumulated
/// sweep, so no single-digit or even moderate threshold could ever walk
/// all the way around a full bore — the walk stalls a small arc away from
/// the seed regardless of how the threshold is tuned. Comparing each step
/// to its own predecessor only ever measures the *local* facet-to-facet
/// angle change, so a curved surface with gentle enough faceting can be
/// walked all the way around with a small threshold, exactly matching
/// tools like Blender's "Select Similar > Coplanar."
pub fn expand_coplanar_from_face(
    mesh:          &FemMesh,
    seed_face:     FaceId,
    threshold_deg: f32,
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
    if !normals.contains_key(&seed_face) {
        return (vec![seed_face], vec![]);
    }

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

            // Compare to *this* face's normal (the BFS predecessor), not
            // the seed's — see the function doc comment for why.
            let cos = normals.get(&neighbour_id)
                .map(|n| current_normal.dot(*n).abs())
                .unwrap_or(0.0);

            if cos >= cos_threshold {
                visited.insert(neighbour_id);
                queue.push_back(neighbour_id);
            }
        }
    }

    // Collect element IDs from the selected faces.
    let element_ids: Vec<ElementId> = visited.iter()
        .filter_map(|fid| face_by_id.get(fid)?.element)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    (visited.into_iter().collect(), element_ids)
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
