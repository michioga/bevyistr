use std::collections::{BTreeMap, BTreeSet};

use bevy::prelude::*;

use crate::{
    Aabb, ContactPair, ContactSlaveRef, ContactType, ElementFaceRef, FaceId, FemFace, FemMesh,
    FemModel, FemSurfaceSet, MpcEquation, MpcTerm, NodeId, SurfaceSetRef,
};

/// Centroid and approximate outward normal of a boundary face.
///
/// Used as the basic geometric primitive for proximity-based contact
/// candidate search.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceGeometry {
    pub centroid: Vec3,

    pub normal: Vec3,
}

/// Tuning parameters for [`FemModel::find_contact_candidates`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContactSearchParams {
    /// Maximum surface-to-surface distance for two boundary faces to be
    /// considered a potential contact pair, in model units.
    pub max_gap: f32,

    /// Maximum angular deviation, in degrees, from perfectly opposing
    /// (180°, typical solid-solid contact) or perfectly aligned (0°,
    /// coincident shell surfaces) face normals for a pair to be accepted.
    pub normal_tolerance_deg: f32,
}

impl Default for ContactSearchParams {
    fn default() -> Self {
        Self {
            max_gap: 0.05,
            normal_tolerance_deg: 20.0,
        }
    }
}

/// A proposed contact region between two meshes (or two disjoint regions of
/// the same mesh), found by [`FemModel::find_contact_candidates`].
///
/// `faces_a` becomes the master surface and `faces_b` the slave surface if
/// the candidate is accepted via [`FemModel::accept_contact_candidate`].
#[derive(Debug, Clone, PartialEq)]
pub struct ContactCandidate {
    pub mesh_a: usize,

    pub mesh_b: usize,

    pub faces_a: Vec<FaceId>,

    pub faces_b: Vec<FaceId>,

    /// Number of individual face-to-face matches that contributed to this
    /// candidate. Two matches can reference the same face on one side
    /// while pairing with different faces on the other side, so this can
    /// exceed `faces_a.len().max(faces_b.len())`.
    pub pair_count: usize,

    /// Mean surface-to-surface distance across all matched face pairs.
    pub average_gap: f32,
}

impl ContactCandidate {
    /// `true` if both sides of the candidate lie within the same mesh
    /// (self-contact, e.g. a folded shell).
    pub fn is_self_contact(&self) -> bool {
        self.mesh_a == self.mesh_b
    }
}

/// Holds the results of the most recent contact candidate search together
/// with the search parameters and which candidate is currently selected for
/// review/acceptance in the UI.
///
/// This is the state behind the "近接surfaceを自動検出 → 接触候補を提案 →
/// ユーザーが数クリックで承認" workflow.
#[derive(Resource, Debug, Clone, Default)]
pub struct ContactCandidateState {
    pub params: ContactSearchParams,

    pub candidates: Vec<ContactCandidate>,

    pub selected: Option<usize>,
}

impl ContactCandidateState {
    pub fn selected_candidate(&self) -> Option<&ContactCandidate> {
        self.selected.and_then(|index| self.candidates.get(index))
    }

    /// Re-runs the search against `model` and selects the first result, if
    /// any. Interfaces that already have an accepted surface-to-surface
    /// contact are omitted from the review queue.
    pub fn refresh(&mut self, model: &FemModel) {
        self.candidates = model
            .find_contact_candidates(&self.params)
            .into_iter()
            .filter(|candidate| !model.contact_candidate_is_defined(candidate))
            .collect();
        self.selected = if self.candidates.is_empty() {
            None
        } else {
            Some(0)
        };
    }

    /// Selects the next candidate, wrapping from the last candidate back to
    /// the first. Does nothing when there are no candidates.
    pub fn select_next(&mut self) {
        if self.candidates.is_empty() {
            self.selected = None;
            return;
        }

        let current = self.selected.unwrap_or(0).min(self.candidates.len() - 1);
        self.selected = Some((current + 1) % self.candidates.len());
    }

    /// Selects the previous candidate, wrapping from the first candidate to
    /// the last. Does nothing when there are no candidates.
    pub fn select_previous(&mut self) {
        if self.candidates.is_empty() {
            self.selected = None;
            return;
        }

        let current = self.selected.unwrap_or(0).min(self.candidates.len() - 1);
        self.selected = Some((current + self.candidates.len() - 1) % self.candidates.len());
    }

    /// Removes the currently selected candidate (e.g. after it has been
    /// accepted) and selects the next one, if any remain.
    pub fn remove_selected(&mut self) {
        let Some(index) = self.selected else {
            return;
        };

        if index < self.candidates.len() {
            self.candidates.remove(index);
        }

        self.selected = if self.candidates.is_empty() {
            None
        } else {
            Some(index.min(self.candidates.len() - 1))
        };
    }
}

/// Kinematic capability available at an automatically discovered spider
/// center. An isolated/reference node can safely provide translations only;
/// a node belonging to a FrontISTR beam can also transfer rotations through
/// offset-dependent rigid-body equations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RigidSpiderMode {
    TranslationOnly,
    RigidBody,
}

/// One ConSeek-style proximity result: a non-solid center node and nearby
/// nodes on one solid boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RigidSpiderCandidate {
    pub master_mesh: usize,
    pub master_node: NodeId,
    pub slave_mesh: usize,
    pub slave_nodes: Vec<NodeId>,
    pub mode: RigidSpiderMode,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RigidSpiderSearchParams {
    pub radius: f32,
    pub minimum_slave_nodes: usize,
}

impl Default for RigidSpiderSearchParams {
    fn default() -> Self {
        Self {
            radius: 1.0,
            minimum_slave_nodes: 3,
        }
    }
}

#[derive(Resource, Debug, Clone, Default)]
pub struct RigidSpiderCandidateState {
    pub params: RigidSpiderSearchParams,
    pub candidates: Vec<RigidSpiderCandidate>,
    pub selected: Option<usize>,
}

impl RigidSpiderCandidateState {
    pub fn selected_candidate(&self) -> Option<&RigidSpiderCandidate> {
        self.selected.and_then(|index| self.candidates.get(index))
    }

    pub fn refresh(&mut self, model: &FemModel) {
        self.candidates = model.find_rigid_spider_candidates(&self.params);
        self.selected = (!self.candidates.is_empty()).then_some(0);
    }

    pub fn select_next(&mut self) {
        if self.candidates.is_empty() {
            self.selected = None;
            return;
        }
        let current = self.selected.unwrap_or(0).min(self.candidates.len() - 1);
        self.selected = Some((current + 1) % self.candidates.len());
    }

    pub fn select_previous(&mut self) {
        if self.candidates.is_empty() {
            self.selected = None;
            return;
        }
        let current = self.selected.unwrap_or(0).min(self.candidates.len() - 1);
        self.selected = Some((current + self.candidates.len() - 1) % self.candidates.len());
    }

    pub fn remove_selected(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
        if index < self.candidates.len() {
            self.candidates.remove(index);
        }
        self.selected = if self.candidates.is_empty() {
            None
        } else {
            Some(index.min(self.candidates.len() - 1))
        };
    }
}

#[cfg(test)]
mod candidate_state_tests {
    use super::*;

    fn candidate(mesh_a: usize, mesh_b: usize) -> ContactCandidate {
        ContactCandidate {
            mesh_a,
            mesh_b,
            faces_a: Vec::new(),
            faces_b: Vec::new(),
            pair_count: 0,
            average_gap: 0.0,
        }
    }

    #[test]
    fn candidate_navigation_wraps_in_both_directions() {
        let mut state = ContactCandidateState {
            candidates: vec![candidate(0, 1), candidate(1, 2), candidate(2, 3)],
            selected: Some(0),
            ..default()
        };

        state.select_previous();
        assert_eq!(state.selected, Some(2));

        state.select_next();
        assert_eq!(state.selected, Some(0));
    }

    #[test]
    fn removing_a_candidate_keeps_a_valid_selection() {
        let mut state = ContactCandidateState {
            candidates: vec![candidate(0, 1), candidate(1, 2)],
            selected: Some(1),
            ..default()
        };

        state.remove_selected();
        assert_eq!(state.candidates.len(), 1);
        assert_eq!(state.selected, Some(0));

        state.remove_selected();
        assert!(state.candidates.is_empty());
        assert_eq!(state.selected, None);
    }
}

#[cfg(test)]
mod contact_search_tests {
    use super::*;
    use crate::{ElementId, ElementType, FemElement, FemNode, NodeId};

    fn shell_quad_mesh(min: Vec2, max: Vec2, z: f32) -> FemMesh {
        let nodes = vec![
            FemNode::new(NodeId(0), Vec3::new(min.x, min.y, z)),
            FemNode::new(NodeId(1), Vec3::new(max.x, min.y, z)),
            FemNode::new(NodeId(2), Vec3::new(max.x, max.y, z)),
            FemNode::new(NodeId(3), Vec3::new(min.x, max.y, z)),
        ];
        let element = FemElement::new(
            ElementId(0),
            ElementType::ShellQuad4,
            vec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
        );

        FemMesh::new(nodes, vec![element])
    }

    fn subdivided_shell_quad_mesh(min: Vec2, max: Vec2, z: f32) -> FemMesh {
        let mut nodes = Vec::new();

        for y_index in 0..=2 {
            for x_index in 0..=2 {
                let fraction = Vec2::new(x_index as f32 * 0.5, y_index as f32 * 0.5);
                let xy = min + (max - min) * fraction;
                nodes.push(FemNode::new(
                    NodeId((y_index * 3 + x_index) as u32),
                    Vec3::new(xy.x, xy.y, z),
                ));
            }
        }

        let mut elements = Vec::new();

        for y_index in 0..2 {
            for x_index in 0..2 {
                let lower_left = (y_index * 3 + x_index) as u32;
                elements.push(FemElement::new(
                    ElementId((y_index * 2 + x_index) as u32),
                    ElementType::ShellQuad4,
                    vec![
                        NodeId(lower_left),
                        NodeId(lower_left + 1),
                        NodeId(lower_left + 4),
                        NodeId(lower_left + 3),
                    ],
                ));
            }
        }

        FemMesh::new(nodes, elements)
    }

    fn disconnected_shell_quads(z: f32) -> FemMesh {
        let patches = [
            (Vec2::new(-4.0, -1.0), Vec2::new(-2.0, 1.0)),
            (Vec2::new(2.0, -1.0), Vec2::new(4.0, 1.0)),
        ];
        let mut nodes = Vec::new();
        let mut elements = Vec::new();

        for (patch_index, (min, max)) in patches.into_iter().enumerate() {
            let first_node = (patch_index * 4) as u32;
            nodes.extend([
                FemNode::new(NodeId(first_node), Vec3::new(min.x, min.y, z)),
                FemNode::new(NodeId(first_node + 1), Vec3::new(max.x, min.y, z)),
                FemNode::new(NodeId(first_node + 2), Vec3::new(max.x, max.y, z)),
                FemNode::new(NodeId(first_node + 3), Vec3::new(min.x, max.y, z)),
            ]);
            elements.push(FemElement::new(
                ElementId(patch_index as u32),
                ElementType::ShellQuad4,
                vec![
                    NodeId(first_node),
                    NodeId(first_node + 1),
                    NodeId(first_node + 2),
                    NodeId(first_node + 3),
                ],
            ));
        }

        FemMesh::new(nodes, elements)
    }

    #[test]
    fn detects_overlapping_faces_even_when_their_centroids_are_far_apart() {
        let mut model = FemModel::single_mesh(
            "Coarse",
            shell_quad_mesh(Vec2::splat(-1.0), Vec2::splat(1.0), 0.0),
        );
        model.add_mesh(
            "Small patch",
            shell_quad_mesh(Vec2::new(0.6, -0.2), Vec2::new(1.0, 0.2), 0.01),
        );

        let candidates = model.find_contact_candidates(&ContactSearchParams {
            max_gap: 0.02,
            normal_tolerance_deg: 5.0,
        });

        assert_eq!(candidates.len(), 1);
        assert!((candidates[0].average_gap - 0.01).abs() < 1.0e-5);
    }

    #[test]
    fn assigns_the_finer_contact_mesh_to_the_slave_side() {
        let mut model = FemModel::single_mesh(
            "Fine",
            subdivided_shell_quad_mesh(Vec2::splat(-1.0), Vec2::splat(1.0), 0.01),
        );
        model.add_mesh(
            "Coarse",
            shell_quad_mesh(Vec2::splat(-1.0), Vec2::splat(1.0), 0.0),
        );

        let candidates = model.find_contact_candidates(&ContactSearchParams {
            max_gap: 0.02,
            normal_tolerance_deg: 5.0,
        });

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].mesh_a, 1, "coarse mesh must be master");
        assert_eq!(candidates[0].mesh_b, 0, "fine mesh must be slave");
        assert_eq!(candidates[0].faces_a.len(), 1);
        assert_eq!(candidates[0].faces_b.len(), 4);
    }

    #[test]
    fn keeps_disconnected_contact_patches_as_separate_candidates() {
        let mut model = FemModel::single_mesh("Lower patches", disconnected_shell_quads(0.0));
        model.add_mesh("Upper patches", disconnected_shell_quads(0.01));

        let candidates = model.find_contact_candidates(&ContactSearchParams {
            max_gap: 0.02,
            normal_tolerance_deg: 5.0,
        });

        assert_eq!(candidates.len(), 2);

        for candidate in candidates {
            assert_eq!(candidate.mesh_a, 0);
            assert_eq!(candidate.mesh_b, 1);
            assert_eq!(candidate.faces_a.len(), 1);
            assert_eq!(candidate.faces_b.len(), 1);
            assert_eq!(candidate.pair_count, 1);
            assert!((candidate.average_gap - 0.01).abs() < 1.0e-5);
        }
    }

    #[test]
    fn accepted_contact_is_not_offered_or_created_twice() {
        let mut model = FemModel::single_mesh(
            "Lower",
            shell_quad_mesh(Vec2::splat(-1.0), Vec2::splat(1.0), 0.0),
        );
        model.add_mesh(
            "Upper",
            shell_quad_mesh(Vec2::splat(-1.0), Vec2::splat(1.0), 0.01),
        );
        let params = ContactSearchParams {
            max_gap: 0.02,
            normal_tolerance_deg: 5.0,
        };
        let candidate = model.find_contact_candidates(&params).remove(0);

        assert!(
            model
                .accept_contact_candidate(&candidate, "CONTACT_1", ContactType::SmallSliding)
                .is_some()
        );
        assert!(model.contact_candidate_is_defined(&candidate));
        assert!(
            model
                .accept_contact_candidate(&candidate, "CONTACT_2", ContactType::SmallSliding)
                .is_none()
        );
        assert_eq!(model.contacts.len(), 1);

        let mut state = ContactCandidateState {
            params,
            ..default()
        };
        state.refresh(&model);
        assert!(state.candidates.is_empty());
        assert_eq!(state.selected, None);
    }

    #[test]
    fn finds_isolated_center_nodes_for_translational_spiders() {
        let mut solid = FemMesh::demo_hex8();
        solid
            .nodes
            .push(FemNode::new(NodeId(99), Vec3::new(0.0, 0.0, 0.5)));
        solid.rebuild_topology_cache();
        let model = FemModel::single_mesh("Solid", solid);

        let candidates = model.find_rigid_spider_candidates(&RigidSpiderSearchParams {
            radius: 1.2,
            minimum_slave_nodes: 3,
        });

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].master_node, NodeId(99));
        assert_eq!(candidates[0].slave_nodes.len(), 4);
        assert_eq!(candidates[0].mode, RigidSpiderMode::TranslationOnly);
    }

    #[test]
    fn creates_offset_rigid_body_equations_for_a_beam_center() {
        let beam = FemMesh::new(
            vec![
                FemNode::new(NodeId(20), Vec3::new(0.0, 0.0, 0.5)),
                FemNode::new(NodeId(21), Vec3::new(0.0, 0.0, 2.0)),
            ],
            vec![FemElement::new(
                ElementId(0),
                ElementType::Beam611,
                vec![NodeId(20), NodeId(21)],
            )],
        );
        let mut model = FemModel::single_mesh("Beam", beam);
        model.add_mesh("Solid", FemMesh::demo_hex8());
        let candidate = model
            .find_rigid_spider_candidates(&RigidSpiderSearchParams {
                radius: 1.2,
                minimum_slave_nodes: 3,
            })
            .into_iter()
            .find(|candidate| candidate.master_node == NodeId(20))
            .unwrap();

        assert_eq!(candidate.mode, RigidSpiderMode::RigidBody);
        let equations = model
            .rigid_spider_equations(&candidate, "SPIDER_1")
            .unwrap();

        assert_eq!(equations.len(), candidate.slave_nodes.len() * 3);
        assert!(equations.iter().all(MpcEquation::is_valid));
        assert!(
            equations
                .iter()
                .all(|equation| equation.group.as_deref() == Some("SPIDER_1"))
        );
        assert!(equations.iter().any(|equation| {
            equation
                .terms
                .iter()
                .any(|term| term.mesh_index == 0 && term.dof >= 4)
        }));
    }
}

fn shares_node(a: &FemFace, b: &FemFace) -> bool {
    a.nodes.iter().any(|node| b.nodes.contains(node))
}

#[derive(Debug, Clone)]
struct ContactFaceGeometry {
    face: FaceGeometry,
    positions: Vec<Vec3>,
    area: f32,
    bounds: Aabb,
}

fn contact_face_geometry(mesh: &FemMesh, face: &FemFace) -> Option<ContactFaceGeometry> {
    let positions = mesh.node_positions(&face.nodes)?;
    let face_geometry = mesh.face_geometry(face)?;
    let bounds = Aabb::from_points(&positions)?;

    let area = positions[1..positions.len() - 1]
        .iter()
        .zip(&positions[2..])
        .map(|(b, c)| (*b - positions[0]).cross(*c - positions[0]).length() * 0.5)
        .sum();

    Some(ContactFaceGeometry {
        face: face_geometry,
        positions,
        area,
        bounds,
    })
}

fn point_triangle_distance_squared(point: Vec3, a: Vec3, b: Vec3, c: Vec3) -> f32 {
    // Closest-point regions from Real-Time Collision Detection, Christer Ericson.
    let ab = b - a;
    let ac = c - a;
    let ap = point - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);

    if d1 <= 0.0 && d2 <= 0.0 {
        return ap.length_squared();
    }

    let bp = point - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);

    if d3 >= 0.0 && d4 <= d3 {
        return bp.length_squared();
    }

    let vc = d1 * d4 - d3 * d2;

    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);

        return (point - (a + v * ab)).length_squared();
    }

    let cp = point - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);

    if d6 >= 0.0 && d5 <= d6 {
        return cp.length_squared();
    }

    let vb = d5 * d2 - d1 * d6;

    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);

        return (point - (a + w * ac)).length_squared();
    }

    let va = d3 * d6 - d5 * d4;

    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));

        return (point - (b + w * (c - b))).length_squared();
    }

    let denominator = 1.0 / (va + vb + vc);
    let v = vb * denominator;
    let w = vc * denominator;

    (point - (a + ab * v + ac * w)).length_squared()
}

fn segment_segment_distance_squared(p1: Vec3, q1: Vec3, p2: Vec3, q2: Vec3) -> f32 {
    const EPSILON: f32 = 1.0e-12;

    let d1 = q1 - p1;
    let d2 = q2 - p2;
    let r = p1 - p2;
    let a = d1.length_squared();
    let e = d2.length_squared();
    let f = d2.dot(r);

    let (mut s, mut t);

    if a <= EPSILON && e <= EPSILON {
        return r.length_squared();
    }

    if a <= EPSILON {
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = d1.dot(r);

        if e <= EPSILON {
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            let b = d1.dot(d2);
            let denominator = a * e - b * b;

            s = if denominator.abs() > EPSILON {
                ((b * f - c * e) / denominator).clamp(0.0, 1.0)
            } else {
                0.0
            };

            t = (b * s + f) / e;

            if t < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            }
        }
    }

    ((p1 + d1 * s) - (p2 + d2 * t)).length_squared()
}

fn triangle_distance_squared(a: [Vec3; 3], b: [Vec3; 3]) -> f32 {
    let mut minimum = f32::MAX;

    for point in a {
        minimum = minimum.min(point_triangle_distance_squared(point, b[0], b[1], b[2]));
    }

    for point in b {
        minimum = minimum.min(point_triangle_distance_squared(point, a[0], a[1], a[2]));
    }

    for edge_a in 0..3 {
        for edge_b in 0..3 {
            minimum = minimum.min(segment_segment_distance_squared(
                a[edge_a],
                a[(edge_a + 1) % 3],
                b[edge_b],
                b[(edge_b + 1) % 3],
            ));
        }
    }

    minimum
}

fn face_distance(a: &[Vec3], b: &[Vec3]) -> f32 {
    let mut minimum_squared = f32::MAX;

    for index_a in 1..a.len() - 1 {
        let triangle_a = [a[0], a[index_a], a[index_a + 1]];

        for index_b in 1..b.len() - 1 {
            let triangle_b = [b[0], b[index_b], b[index_b + 1]];

            minimum_squared =
                minimum_squared.min(triangle_distance_squared(triangle_a, triangle_b));
        }
    }

    minimum_squared.sqrt()
}

fn average_face_area(
    faces: &[FemFace],
    geometry: &[Option<ContactFaceGeometry>],
    matched: &BTreeSet<FaceId>,
) -> f32 {
    let (area, count) = faces
        .iter()
        .zip(geometry)
        .filter(|(face, _)| matched.contains(&face.id))
        .filter_map(|(_, geometry)| geometry.as_ref())
        .fold((0.0, 0usize), |(area, count), geometry| {
            (area + geometry.area, count + 1)
        });

    if count == 0 {
        f32::MAX
    } else {
        area / count as f32
    }
}

#[derive(Debug, Clone, Copy)]
struct ContactFaceMatch {
    face_a: FaceId,
    face_b: FaceId,
    gap: f32,
}

#[derive(Debug, Default)]
struct ContactMatchComponent {
    faces_a: BTreeSet<FaceId>,
    faces_b: BTreeSet<FaceId>,
    gap_sum: f32,
    pair_count: usize,
}

#[derive(Debug, Default)]
struct DisjointSet {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl DisjointSet {
    fn make_set(&mut self) -> usize {
        let index = self.parent.len();
        self.parent.push(index);
        self.rank.push(0);
        index
    }

    fn find(&mut self, index: usize) -> usize {
        let parent = self.parent[index];

        if parent != index {
            self.parent[index] = self.find(parent);
        }

        self.parent[index]
    }

    fn union(&mut self, left: usize, right: usize) {
        let mut left_root = self.find(left);
        let mut right_root = self.find(right);

        if left_root == right_root {
            return;
        }

        if self.rank[left_root] < self.rank[right_root] {
            std::mem::swap(&mut left_root, &mut right_root);
        }

        self.parent[right_root] = left_root;

        if self.rank[left_root] == self.rank[right_root] {
            self.rank[left_root] += 1;
        }
    }
}

fn contact_graph_node(
    nodes: &mut BTreeMap<(bool, FaceId), usize>,
    groups: &mut DisjointSet,
    side_b: bool,
    face: FaceId,
) -> usize {
    if let Some(&index) = nodes.get(&(side_b, face)) {
        return index;
    }

    let index = groups.make_set();
    nodes.insert((side_b, face), index);
    index
}

fn union_adjacent_contact_faces(
    faces: &[FemFace],
    matched: &BTreeSet<FaceId>,
    side_b: bool,
    graph_nodes: &BTreeMap<(bool, FaceId), usize>,
    groups: &mut DisjointSet,
) {
    let mut faces_at_node = BTreeMap::<NodeId, Vec<FaceId>>::new();

    for face in faces.iter().filter(|face| matched.contains(&face.id)) {
        for &node in &face.nodes {
            faces_at_node.entry(node).or_default().push(face.id);
        }
    }

    // Two boundary faces belong to the same surface patch when they share
    // an edge (at least two nodes). Faces that only meet at a vertex remain
    // separate, which is usually the useful distinction for contact setup.
    let mut shared_node_counts = BTreeMap::<(FaceId, FaceId), u8>::new();

    for node_faces in faces_at_node.values() {
        for left_index in 0..node_faces.len() {
            for right_index in left_index + 1..node_faces.len() {
                let left = node_faces[left_index].min(node_faces[right_index]);
                let right = node_faces[left_index].max(node_faces[right_index]);
                let count = shared_node_counts.entry((left, right)).or_default();
                *count = count.saturating_add(1);

                if *count == 2 {
                    groups.union(graph_nodes[&(side_b, left)], graph_nodes[&(side_b, right)]);
                }
            }
        }
    }
}

impl FemMesh {
    /// Centroid and approximate outward normal of a boundary face.
    ///
    /// The normal is computed by fan-triangulating the face from its
    /// centroid and summing the triangle cross products, which gives a
    /// reasonable area-weighted normal even for non-planar quads. Returns
    /// `None` if any referenced node is missing or the face is degenerate
    /// (fewer than 3 nodes, or zero area).
    pub fn face_geometry(&self, face: &FemFace) -> Option<FaceGeometry> {
        let positions = self.node_positions(&face.nodes)?;

        if positions.len() < 3 {
            return None;
        }

        let mut centroid = Vec3::ZERO;

        for position in &positions {
            centroid += *position;
        }

        centroid /= positions.len() as f32;

        let mut normal = Vec3::ZERO;

        for index in 0..positions.len() {
            let current = positions[index] - centroid;
            let next = positions[(index + 1) % positions.len()] - centroid;

            normal += current.cross(next);
        }

        let normal = normal.normalize_or_zero();

        if normal == Vec3::ZERO {
            return None;
        }

        Some(FaceGeometry { centroid, normal })
    }

    /// Creates a surface set from explicit boundary face ids, used to
    /// materialize accepted contact candidates.
    ///
    /// Unlike [`FemMesh::push_surface_set_from_targets`], which resolves
    /// arbitrary selection targets, this takes [`FaceId`]s directly. Returns
    /// the number of faces added; if zero, no surface set is pushed.
    pub(crate) fn push_surface_set_from_face_ids(
        &mut self,
        name: impl Into<String>,
        face_ids: &[FaceId],
    ) -> usize {
        let surfaces: BTreeSet<ElementFaceRef> = face_ids
            .iter()
            .filter_map(|id| {
                self.cached_boundary_faces()
                    .iter()
                    .find(|face| face.id == *id)
            })
            .filter_map(FemFace::element_face_ref)
            .collect();

        let count = surfaces.len();

        if count > 0 {
            self.surface_sets.push(FemSurfaceSet {
                name: name.into(),
                surfaces: surfaces.into_iter().collect(),
            });
        }

        count
    }
}

impl FemModel {
    /// Finds ConSeek-style rigid-spider candidates. Center nodes are either
    /// unreferenced reference points (translation-only coupling) or nodes of
    /// FrontISTR beam elements (full rigid-body coupling); nearby slave
    /// nodes are restricted to the exterior boundary of solid elements.
    pub fn find_rigid_spider_candidates(
        &self,
        params: &RigidSpiderSearchParams,
    ) -> Vec<RigidSpiderCandidate> {
        let radius = params.radius.max(0.0);
        let minimum_slave_nodes = params.minimum_slave_nodes.max(1);
        let mut solid_boundary_nodes = Vec::with_capacity(self.meshes.len());

        for mesh in &self.meshes {
            let solid_elements: BTreeSet<_> = mesh
                .elements
                .iter()
                .filter(|element| element.element_type.is_solid())
                .map(|element| element.id)
                .collect();
            let nodes = mesh
                .cached_boundary_faces()
                .iter()
                .filter(|face| face.element.is_some_and(|id| solid_elements.contains(&id)))
                .flat_map(|face| face.nodes.iter().copied())
                .collect::<BTreeSet<_>>();
            solid_boundary_nodes.push(nodes);
        }

        let mut candidates = Vec::new();

        for (master_mesh, mesh) in self.meshes.iter().enumerate() {
            let solid_nodes: BTreeSet<_> = mesh
                .elements
                .iter()
                .filter(|element| element.element_type.is_solid())
                .flat_map(|element| element.nodes.iter().copied())
                .collect();
            let referenced_nodes: BTreeSet<_> = mesh
                .elements
                .iter()
                .flat_map(|element| element.nodes.iter().copied())
                .collect();
            let rotational_beam_nodes: BTreeSet<_> = mesh
                .elements
                .iter()
                .filter(|element| {
                    matches!(
                        element.element_type,
                        crate::ElementType::Beam611 | crate::ElementType::Beam641
                    )
                })
                .flat_map(|element| element.nodes.iter().copied())
                .collect();

            for master in mesh.nodes.iter().filter(|node| {
                !solid_nodes.contains(&node.id)
                    && (rotational_beam_nodes.contains(&node.id)
                        || !referenced_nodes.contains(&node.id))
            }) {
                let mode = if rotational_beam_nodes.contains(&master.id) {
                    RigidSpiderMode::RigidBody
                } else {
                    RigidSpiderMode::TranslationOnly
                };

                for (slave_mesh, slave) in self.meshes.iter().enumerate() {
                    if solid_boundary_nodes[slave_mesh].is_empty() {
                        continue;
                    }

                    let slave_nodes = slave
                        .node_indices_near(master.position, radius)
                        .into_iter()
                        .filter_map(|index| slave.nodes.get(index))
                        .filter(|node| solid_boundary_nodes[slave_mesh].contains(&node.id))
                        .filter(|node| !(master_mesh == slave_mesh && node.id == master.id))
                        .filter(|node| node.position.distance(master.position) <= radius)
                        .map(|node| node.id)
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect::<Vec<_>>();

                    if slave_nodes.len() >= minimum_slave_nodes {
                        candidates.push(RigidSpiderCandidate {
                            master_mesh,
                            master_node: master.id,
                            slave_mesh,
                            slave_nodes,
                            mode,
                        });
                    }
                }
            }
        }

        candidates
    }

    /// Converts a reviewed spider candidate into official FrontISTR
    /// `!EQUATION` entries. Translation-only centers produce three equal-DOF
    /// constraints per slave node. Beam centers additionally include the
    /// rigid-body rotation terms `theta × radius`.
    pub fn rigid_spider_equations(
        &self,
        candidate: &RigidSpiderCandidate,
        name: &str,
    ) -> Option<Vec<MpcEquation>> {
        let master = self
            .meshes
            .get(candidate.master_mesh)?
            .node_position(candidate.master_node)?;
        let slave_mesh = self.meshes.get(candidate.slave_mesh)?;
        let mut equations = Vec::with_capacity(candidate.slave_nodes.len() * 3);

        for &slave_node in &candidate.slave_nodes {
            let slave = slave_mesh.node_position(slave_node)?;
            let radius = slave - master;
            let base_name = format!("{name}_N{}", slave_node.0);

            for (dof, rotation_terms) in [
                (1, [(5, -radius.z), (6, radius.y)]),
                (2, [(4, radius.z), (6, -radius.x)]),
                (3, [(4, -radius.y), (5, radius.x)]),
            ] {
                let mut terms = vec![
                    MpcTerm::new(candidate.slave_mesh, slave_node, dof, 1.0),
                    MpcTerm::new(candidate.master_mesh, candidate.master_node, dof, -1.0),
                ];

                if candidate.mode == RigidSpiderMode::RigidBody {
                    terms.extend(
                        rotation_terms
                            .into_iter()
                            .filter(|(_, coefficient)| coefficient.abs() > 1.0e-12)
                            .map(|(rotation_dof, coefficient)| {
                                MpcTerm::new(
                                    candidate.master_mesh,
                                    candidate.master_node,
                                    rotation_dof,
                                    coefficient,
                                )
                            }),
                    );
                }

                equations.push(
                    MpcEquation::new(format!("{base_name}_U{dof}"), 0.0, terms).with_group(name),
                );
            }
        }

        Some(equations)
    }

    /// Searches for nearby, opposing-or-coincident boundary face groups
    /// across (and within) the model's meshes, returning one
    /// [`ContactCandidate`] per connected matching region. Disconnected
    /// contact patches between the same pair of meshes remain separate so
    /// they can be reviewed and accepted independently.
    ///
    /// Two boundary faces are considered a match when their surfaces are
    /// within `params.max_gap` of each other, their normals are roughly
    /// opposing or roughly aligned (within `params.normal_tolerance_deg`),
    /// and they do not already share a node (which would make them ordinary
    /// adjacent mesh topology rather than a contact interface). The side
    /// with the larger average boundary-face area is returned as master;
    /// the finer side is returned as slave, following the node-to-surface
    /// strategy used by ConSeek and FrontISTR contact pairs.
    pub fn find_contact_candidates(&self, params: &ContactSearchParams) -> Vec<ContactCandidate> {
        let max_gap = params.max_gap.max(0.0);
        let cos_alignment = params.normal_tolerance_deg.to_radians().cos();

        let geometry: Vec<Vec<Option<ContactFaceGeometry>>> = self
            .meshes
            .iter()
            .map(|mesh| {
                mesh.cached_boundary_faces()
                    .iter()
                    .map(|face| contact_face_geometry(mesh, face))
                    .collect()
            })
            .collect();

        let mut candidates = Vec::new();

        for mesh_a in 0..self.meshes.len() {
            for mesh_b in mesh_a..self.meshes.len() {
                candidates.extend(self.find_candidates_between(
                    &geometry,
                    mesh_a,
                    mesh_b,
                    max_gap,
                    cos_alignment,
                ));
            }
        }

        candidates
    }

    fn find_candidates_between(
        &self,
        geometry: &[Vec<Option<ContactFaceGeometry>>],
        mesh_a: usize,
        mesh_b: usize,
        max_gap: f32,
        cos_alignment: f32,
    ) -> Vec<ContactCandidate> {
        let faces_a = self.meshes[mesh_a].cached_boundary_faces();
        let faces_b = self.meshes[mesh_b].cached_boundary_faces();

        if faces_a.is_empty() || faces_b.is_empty() {
            return Vec::new();
        }

        let mut matches = Vec::new();

        for (index_a, face_a) in faces_a.iter().enumerate() {
            let Some(geom_a) = geometry[mesh_a][index_a].as_ref() else {
                continue;
            };

            // Broad phase: query with the complete face bounds rather than
            // its centroid. This keeps a small face inside a much larger
            // opposing face discoverable even when their centroids are far
            // apart (a common coarse-master/fine-slave contact layout).
            for index_b in
                self.meshes[mesh_b].boundary_face_indices_in_aabb(geom_a.bounds.expanded(max_gap))
            {
                // When searching a mesh against itself, only consider each
                // unordered pair once and never pair a face with itself.
                if mesh_a == mesh_b && index_b <= index_a {
                    continue;
                }

                let face_b = &faces_b[index_b];

                let Some(geom_b) = geometry[mesh_b][index_b].as_ref() else {
                    continue;
                };

                // Node IDs are scoped to a mesh. Different assembly parts
                // commonly reuse the same IDs, so shared-node rejection is
                // meaningful only while testing self-contact.
                if mesh_a == mesh_b && shares_node(face_a, face_b) {
                    continue;
                }

                let gap = face_distance(&geom_a.positions, &geom_b.positions);

                if gap > max_gap {
                    continue;
                }

                let alignment = geom_a.face.normal.dot(geom_b.face.normal);
                let opposing = alignment <= -cos_alignment;
                let coincident = alignment >= cos_alignment;

                if !opposing && !coincident {
                    continue;
                }

                matches.push(ContactFaceMatch {
                    face_a: face_a.id,
                    face_b: face_b.id,
                    gap,
                });
            }
        }

        if matches.is_empty() {
            return Vec::new();
        }

        // Treat accepted face pairs as edges in a bipartite graph. Every
        // connected component is one physical contact patch. This avoids
        // combining distant interfaces merely because they belong to the
        // same two assembly parts.
        let mut graph_nodes = BTreeMap::new();
        let mut groups = DisjointSet::default();

        for face_match in &matches {
            let node_a =
                contact_graph_node(&mut graph_nodes, &mut groups, false, face_match.face_a);
            let node_b = contact_graph_node(&mut graph_nodes, &mut groups, true, face_match.face_b);
            groups.union(node_a, node_b);
        }

        let matched_a = matches
            .iter()
            .map(|face_match| face_match.face_a)
            .collect::<BTreeSet<_>>();
        let matched_b = matches
            .iter()
            .map(|face_match| face_match.face_b)
            .collect::<BTreeSet<_>>();
        union_adjacent_contact_faces(faces_a, &matched_a, false, &graph_nodes, &mut groups);
        union_adjacent_contact_faces(faces_b, &matched_b, true, &graph_nodes, &mut groups);

        let mut components = BTreeMap::<usize, ContactMatchComponent>::new();

        for face_match in matches {
            let node_a = graph_nodes[&(false, face_match.face_a)];
            let root = groups.find(node_a);
            let component = components.entry(root).or_default();
            component.faces_a.insert(face_match.face_a);
            component.faces_b.insert(face_match.face_b);
            component.gap_sum += face_match.gap;
            component.pair_count += 1;
        }

        components
            .into_values()
            .map(|component| {
                let average_area_a =
                    average_face_area(faces_a, &geometry[mesh_a], &component.faces_a);
                let average_area_b =
                    average_face_area(faces_b, &geometry[mesh_b], &component.faces_b);
                let a_is_finer = average_area_a < average_area_b * (1.0 - 1.0e-4);
                let pair_count = component.pair_count;

                let (master_mesh, master_faces, slave_mesh, slave_faces) = if a_is_finer {
                    (mesh_b, component.faces_b, mesh_a, component.faces_a)
                } else {
                    (mesh_a, component.faces_a, mesh_b, component.faces_b)
                };

                ContactCandidate {
                    mesh_a: master_mesh,
                    mesh_b: slave_mesh,
                    faces_a: master_faces.into_iter().collect(),
                    faces_b: slave_faces.into_iter().collect(),
                    pair_count,
                    average_gap: component.gap_sum / pair_count as f32,
                }
            })
            .collect()
    }

    /// Materializes a [`ContactCandidate`] into a [`ContactPair`] by
    /// creating master/slave surface sets from its matched boundary faces.
    ///
    /// Returns `None` (without modifying the model) if either mesh index is
    /// out of range or either side resolves to zero faces; in the latter
    /// case any surface set already pushed for the other side is rolled
    /// back.
    pub fn accept_contact_candidate(
        &mut self,
        candidate: &ContactCandidate,
        name: impl Into<String>,
        contact_type: ContactType,
    ) -> Option<usize> {
        if self.contact_candidate_is_defined(candidate) {
            return None;
        }

        let name = name.into();

        let master_count = self
            .meshes
            .get_mut(candidate.mesh_a)?
            .push_surface_set_from_face_ids(format!("{name}_MASTER"), &candidate.faces_a);

        if master_count == 0 {
            return None;
        }

        let master = SurfaceSetRef::new(
            candidate.mesh_a,
            self.meshes[candidate.mesh_a].surface_sets.len() - 1,
        );

        let slave_count = self
            .meshes
            .get_mut(candidate.mesh_b)?
            .push_surface_set_from_face_ids(format!("{name}_SLAVE"), &candidate.faces_b);

        if slave_count == 0 {
            self.meshes[candidate.mesh_a].surface_sets.pop();

            return None;
        }

        let slave = SurfaceSetRef::new(
            candidate.mesh_b,
            self.meshes[candidate.mesh_b].surface_sets.len() - 1,
        );

        self.contacts
            .push(ContactPair::new(name, master, slave, contact_type));

        Some(self.contacts.len() - 1)
    }

    /// Returns `true` when the candidate's two complete face sets already
    /// form a defined surface-to-surface contact. Master/slave reversal is
    /// also considered the same interface; node-to-surface contacts are not
    /// comparable to an automatically detected face pair.
    pub fn contact_candidate_is_defined(&self, candidate: &ContactCandidate) -> bool {
        let Some(candidate_master) =
            self.contact_candidate_element_faces(candidate.mesh_a, &candidate.faces_a)
        else {
            return false;
        };
        let Some(candidate_slave) =
            self.contact_candidate_element_faces(candidate.mesh_b, &candidate.faces_b)
        else {
            return false;
        };

        self.contacts.iter().any(|contact| {
            let ContactSlaveRef::Surface(slave) = contact.slave else {
                return false;
            };
            let Some(master_set) = self
                .meshes
                .get(contact.master.mesh_index)
                .and_then(|mesh| mesh.surface_sets.get(contact.master.surface_set_index))
            else {
                return false;
            };
            let Some(slave_set) = self
                .meshes
                .get(slave.mesh_index)
                .and_then(|mesh| mesh.surface_sets.get(slave.surface_set_index))
            else {
                return false;
            };
            let existing_master = master_set.surfaces.iter().copied().collect::<BTreeSet<_>>();
            let existing_slave = slave_set.surfaces.iter().copied().collect::<BTreeSet<_>>();
            let direct = contact.master.mesh_index == candidate.mesh_a
                && slave.mesh_index == candidate.mesh_b
                && existing_master == candidate_master
                && existing_slave == candidate_slave;
            let reversed = contact.master.mesh_index == candidate.mesh_b
                && slave.mesh_index == candidate.mesh_a
                && existing_master == candidate_slave
                && existing_slave == candidate_master;

            direct || reversed
        })
    }

    fn contact_candidate_element_faces(
        &self,
        mesh_index: usize,
        face_ids: &[FaceId],
    ) -> Option<BTreeSet<ElementFaceRef>> {
        let mesh = self.meshes.get(mesh_index)?;
        let faces = face_ids
            .iter()
            .filter_map(|id| {
                mesh.cached_boundary_faces()
                    .iter()
                    .find(|face| face.id == *id)
            })
            .filter_map(FemFace::element_face_ref)
            .collect::<BTreeSet<_>>();

        (faces.len() == face_ids.len()).then_some(faces)
    }
}
