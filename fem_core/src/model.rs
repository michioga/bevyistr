use bevy::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

use crate::spatial::{Aabb, Bvh};
use crate::SelectionLevel;

#[derive(Resource, Default, Debug, Clone)]
pub struct FemModel {
    pub parts: Vec<Part>,

    pub meshes: Vec<FemMesh>,

    pub contacts: Vec<ContactPair>,
}

#[derive(Debug, Clone)]
pub struct Part {
    pub name: String,

    pub mesh_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactPair {
    pub name: String,

    pub master: SurfaceSetRef,

    pub slave: SurfaceSetRef,

    pub contact_type: ContactType,
}

impl ContactPair {
    pub fn new(
        name: impl Into<String>,
        master: SurfaceSetRef,
        slave: SurfaceSetRef,
        contact_type: ContactType,
    ) -> Self {
        Self {
            name: name.into(),
            master,
            slave,
            contact_type,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SurfaceSetRef {
    pub mesh_index: usize,

    pub surface_set_index: usize,
}

impl SurfaceSetRef {
    pub const fn new(mesh_index: usize, surface_set_index: usize) -> Self {
        Self {
            mesh_index,
            surface_set_index,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ContactType {
    #[default]
    Tied,

    Frictionless,
}

#[derive(Debug, Clone, Default)]
pub struct FemMesh {
    pub nodes: Vec<FemNode>,

    pub elements: Vec<FemElement>,

    pub node_sets: Vec<FemNodeSet>,

    pub element_sets: Vec<FemElementSet>,

    pub surface_sets: Vec<FemSurfaceSet>,

    pub topology: TopologyCache,
}

impl FemMesh {
    pub fn demo_hex8() -> Self {
        let nodes = vec![
            FemNode::new(NodeId(0), Vec3::new(-1.0, -0.5, -0.5)),
            FemNode::new(NodeId(1), Vec3::new(1.0, -0.5, -0.5)),
            FemNode::new(NodeId(2), Vec3::new(1.0, 0.5, -0.5)),
            FemNode::new(NodeId(3), Vec3::new(-1.0, 0.5, -0.5)),
            FemNode::new(NodeId(4), Vec3::new(-1.0, -0.5, 0.5)),
            FemNode::new(NodeId(5), Vec3::new(1.0, -0.5, 0.5)),
            FemNode::new(NodeId(6), Vec3::new(1.0, 0.5, 0.5)),
            FemNode::new(NodeId(7), Vec3::new(-1.0, 0.5, 0.5)),
        ];

        let elements = vec![FemElement::new_hex8(
            ElementId(0),
            [
                NodeId(0),
                NodeId(1),
                NodeId(2),
                NodeId(3),
                NodeId(4),
                NodeId(5),
                NodeId(6),
                NodeId(7),
            ],
        )];

        Self::new(nodes, elements)
    }

    pub fn new(nodes: Vec<FemNode>, elements: Vec<FemElement>) -> Self {
        let mut mesh = Self {
            nodes,
            elements,
            node_sets: Vec::new(),
            element_sets: Vec::new(),
            surface_sets: Vec::new(),
            topology: TopologyCache::default(),
        };

        mesh.rebuild_topology_cache();

        mesh
    }

    pub fn rebuild_topology_cache(&mut self) {
        self.topology = TopologyCache::build(&self.nodes, &self.elements);
    }

    pub fn cached_edges(&self) -> &[FemEdge] {
        &self.topology.edges
    }

    pub fn cached_faces(&self) -> &[FemFace] {
        &self.topology.faces
    }

    pub fn cached_boundary_faces(&self) -> &[FemFace] {
        &self.topology.boundary_faces
    }

    pub fn cached_boundary_edges(&self) -> &[FemEdge] {
        &self.topology.boundary_edges
    }

    /// World-space AABB of each boundary face, parallel to
    /// [`FemMesh::cached_boundary_faces`].
    pub fn cached_boundary_face_bounds(&self) -> &[Aabb] {
        &self.topology.boundary_face_bounds
    }

    /// Indices into [`FemMesh::cached_boundary_faces`] whose AABB
    /// intersects the given ray.
    ///
    /// This is a broad-phase filter via [`Bvh::query_ray`]: callers
    /// performing an exact ray-triangle test should still do so on each
    /// returned candidate.
    pub fn boundary_face_indices_along_ray(&self, origin: Vec3, direction: Vec3) -> Vec<usize> {
        self.topology
            .boundary_face_bvh
            .query_ray(origin, direction)
            .into_iter()
            .map(|index| index as usize)
            .collect()
    }

    /// Indices into [`FemMesh::cached_boundary_faces`] whose AABB overlaps
    /// `aabb`. Intended for box-select and clipping-region queries.
    pub fn boundary_face_indices_in_aabb(&self, aabb: Aabb) -> Vec<usize> {
        self.topology
            .boundary_face_bvh
            .query_aabb(aabb)
            .into_iter()
            .map(|index| index as usize)
            .collect()
    }

    /// Indices into [`FemMesh::cached_boundary_faces`] whose AABB is within
    /// `radius` of `point`. Used by [`crate::FemModel::find_contact_candidates`]
    /// to narrow the proximity search.
    pub fn boundary_face_indices_near(&self, point: Vec3, radius: f32) -> Vec<usize> {
        self.topology
            .boundary_face_bvh
            .query_radius(point, radius)
            .into_iter()
            .map(|index| index as usize)
            .collect()
    }

    /// Indices into [`FemMesh::nodes`] within `radius` of the given ray.
    ///
    /// Node AABBs are degenerate points, so this uses
    /// [`Bvh::query_ray_with_radius`] (a cylinder test) rather than
    /// [`Bvh::query_ray`], which would essentially never hit a point.
    pub fn node_indices_along_ray(&self, origin: Vec3, direction: Vec3, radius: f32) -> Vec<usize> {
        self.topology
            .node_bvh
            .query_ray_with_radius(origin, direction, radius)
            .into_iter()
            .map(|index| index as usize)
            .collect()
    }

    /// Indices into [`FemMesh::cached_boundary_edges`] within `radius` of
    /// the given ray, via [`Bvh::query_ray_with_radius`].
    pub fn boundary_edge_indices_along_ray(
        &self,
        origin: Vec3,
        direction: Vec3,
        radius: f32,
    ) -> Vec<usize> {
        self.topology
            .boundary_edge_bvh
            .query_ray_with_radius(origin, direction, radius)
            .into_iter()
            .map(|index| index as usize)
            .collect()
    }

    /// Indices into [`FemMesh::nodes`] whose (degenerate) AABB overlaps
    /// `aabb`. Intended for box-select.
    pub fn node_indices_in_aabb(&self, aabb: Aabb) -> Vec<usize> {
        self.topology
            .node_bvh
            .query_aabb(aabb)
            .into_iter()
            .map(|index| index as usize)
            .collect()
    }

    /// Indices into [`FemMesh::cached_boundary_edges`] whose AABB overlaps
    /// `aabb`. Intended for box-select.
    pub fn boundary_edge_indices_in_aabb(&self, aabb: Aabb) -> Vec<usize> {
        self.topology
            .boundary_edge_bvh
            .query_aabb(aabb)
            .into_iter()
            .map(|index| index as usize)
            .collect()
    }

    pub fn node_position(&self, id: NodeId) -> Option<Vec3> {
        if let Some(index) = self.topology.node_indices.get(&id) {
            return self.nodes.get(*index).map(|node| node.position);
        }

        self.nodes
            .iter()
            .find(|node| node.id == id)
            .map(|node| node.position)
    }

    pub fn node_positions(&self, ids: &[NodeId]) -> Option<Vec<Vec3>> {
        ids.iter()
            .map(|id| self.node_position(*id))
            .collect::<Option<Vec<_>>>()
    }

    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut iter = self.nodes.iter();
        let first = iter.next()?.position;
        let mut min = first;
        let mut max = first;

        for node in iter {
            min = min.min(node.position);
            max = max.max(node.position);
        }

        Some((min, max))
    }

    pub fn derived_edges(&self) -> Vec<FemEdge> {
        derive_edges(&self.elements)
    }

    pub fn derived_faces(&self) -> Vec<FemFace> {
        derive_faces(&self.elements)
    }

    pub fn boundary_faces(&self) -> Vec<FemFace> {
        derive_boundary_faces(&self.elements)
    }

    pub fn boundary_edges(&self) -> Vec<FemEdge> {
        derive_boundary_edges(&self.elements)
    }

    pub fn surface_refs_from_targets(&self, targets: &[FemEntityId]) -> Vec<ElementFaceRef> {
        let mut refs = BTreeSet::new();

        for target in targets {
            match *target {
                FemEntityId::Face(id) => {
                    if let Some(surface) = self
                        .cached_boundary_faces()
                        .iter()
                        .find(|face| face.id == id)
                        .and_then(FemFace::element_face_ref)
                    {
                        refs.insert(surface);
                    }
                }
                FemEntityId::Element(id) => {
                    for surface in self
                        .cached_boundary_faces()
                        .iter()
                        .filter(|face| face.element == Some(id))
                        .filter_map(FemFace::element_face_ref)
                    {
                        refs.insert(surface);
                    }
                }
                FemEntityId::Node(_) | FemEntityId::Edge(_) => {}
            }
        }

        refs.into_iter().collect()
    }

    /// Converts a [`FemNodeSet`] into the corresponding [`FemEntityId`]s,
    /// for highlighting/selecting the set in the UI.
    pub fn node_set_targets(&self, set: &FemNodeSet) -> Vec<FemEntityId> {
        set.nodes.iter().map(|&id| FemEntityId::Node(id)).collect()
    }

    /// Converts a [`FemElementSet`] into the corresponding [`FemEntityId`]s.
    pub fn element_set_targets(&self, set: &FemElementSet) -> Vec<FemEntityId> {
        set.elements
            .iter()
            .map(|&id| FemEntityId::Element(id))
            .collect()
    }

    /// Converts a [`FemSurfaceSet`] into the corresponding boundary
    /// [`FemEntityId::Face`]s.
    ///
    /// A surface set stores `ElementFaceRef`s (element id + local face
    /// index), which is the canonical, solver-portable representation, but
    /// the UI/selection layer works in terms of boundary [`FaceId`]s. This
    /// resolves each `ElementFaceRef` back to the matching cached boundary
    /// face. Surfaces with no matching boundary face (e.g. an internal face
    /// accidentally added to the set) are silently skipped.
    pub fn surface_set_targets(&self, set: &FemSurfaceSet) -> Vec<FemEntityId> {
        let surfaces: std::collections::HashSet<ElementFaceRef> =
            set.surfaces.iter().copied().collect();

        self.cached_boundary_faces()
            .iter()
            .filter_map(|face| {
                let face_ref = face.element_face_ref()?;
                surfaces
                    .contains(&face_ref)
                    .then_some(FemEntityId::Face(face.id))
            })
            .collect()
    }

    pub fn push_surface_set_from_targets(
        &mut self,
        name: impl Into<String>,
        targets: &[FemEntityId],
    ) -> usize {
        let surfaces = self.surface_refs_from_targets(targets);
        let count = surfaces.len();

        if count > 0 {
            self.surface_sets.push(FemSurfaceSet {
                name: name.into(),
                surfaces,
            });
        }

        count
    }
}

#[derive(Debug, Clone, Default)]
pub struct TopologyCache {
    pub node_indices: BTreeMap<NodeId, usize>,

    pub edges: Vec<FemEdge>,

    pub faces: Vec<FemFace>,

    pub boundary_faces: Vec<FemFace>,

    pub boundary_edges: Vec<FemEdge>,

    /// World-space AABB of each entry in `boundary_faces`, in the same
    /// order.
    pub boundary_face_bounds: Vec<Aabb>,

    /// BVH over `boundary_face_bounds`, accelerating ray (picking), box
    /// (box-select/clipping), and radius (contact candidate search) queries
    /// against the boundary surface.
    pub boundary_face_bvh: Bvh,

    /// Degenerate (point) AABB of each mesh node, in the same order as the
    /// `nodes` slice passed to [`TopologyCache::build`].
    pub node_bounds: Vec<Aabb>,

    /// BVH over `node_bounds`, accelerating node picking via
    /// [`Bvh::query_ray_with_radius`].
    pub node_bvh: Bvh,

    /// World-space AABB of each entry in `boundary_edges`, in the same
    /// order.
    pub boundary_edge_bounds: Vec<Aabb>,

    /// BVH over `boundary_edge_bounds`, accelerating edge picking via
    /// [`Bvh::query_ray_with_radius`].
    pub boundary_edge_bvh: Bvh,
}

/// World-space positions of the nodes referenced by `ids`, looked up via
/// `node_indices`. Nodes that can't be resolved are silently skipped, so the
/// result may be shorter than `ids`.
fn resolve_node_positions(
    node_indices: &BTreeMap<NodeId, usize>,
    nodes: &[FemNode],
    ids: &[NodeId],
) -> Vec<Vec3> {
    ids.iter()
        .filter_map(|id| node_indices.get(id))
        .filter_map(|&index| nodes.get(index))
        .map(|node| node.position)
        .collect()
}

impl TopologyCache {
    pub fn build(nodes: &[FemNode], elements: &[FemElement]) -> Self {
        let node_indices: BTreeMap<NodeId, usize> = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.id, index))
            .collect();

        let boundary_faces = derive_boundary_faces(elements);
        let boundary_edges = derive_boundary_edges_from_faces(&boundary_faces);

        let boundary_face_bounds: Vec<Aabb> = boundary_faces
            .iter()
            .map(|face| {
                let points = resolve_node_positions(&node_indices, nodes, &face.nodes);

                Aabb::from_points(&points).unwrap_or_else(|| Aabb::from_point(Vec3::ZERO))
            })
            .collect();

        let boundary_face_bvh = Bvh::build(&boundary_face_bounds);

        let node_bounds: Vec<Aabb> = nodes
            .iter()
            .map(|node| Aabb::from_point(node.position))
            .collect();

        let node_bvh = Bvh::build(&node_bounds);

        let boundary_edge_bounds: Vec<Aabb> = boundary_edges
            .iter()
            .map(|edge| {
                let points = resolve_node_positions(&node_indices, nodes, &edge.nodes);

                Aabb::from_points(&points).unwrap_or_else(|| Aabb::from_point(Vec3::ZERO))
            })
            .collect();

        let boundary_edge_bvh = Bvh::build(&boundary_edge_bounds);

        Self {
            node_indices,
            edges: derive_edges(elements),
            faces: derive_faces(elements),
            boundary_faces,
            boundary_edges,
            boundary_face_bounds,
            boundary_face_bvh,
            node_bounds,
            node_bvh,
            boundary_edge_bounds,
            boundary_edge_bvh,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FemNode {
    pub id: NodeId,

    pub position: Vec3,
}

impl FemNode {
    pub const fn new(id: NodeId, position: Vec3) -> Self {
        Self { id, position }
    }

    pub fn from_xyz(id: NodeId, x: f32, y: f32, z: f32) -> Self {
        Self {
            id,
            position: Vec3::new(x, y, z),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FemEdge {
    pub id: EdgeId,

    pub nodes: [NodeId; 2],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FemFace {
    pub id: FaceId,

    pub nodes: Vec<NodeId>,

    pub element: Option<ElementId>,

    pub local_face: Option<LocalFaceId>,
}

impl FemFace {
    pub fn element_face_ref(&self) -> Option<ElementFaceRef> {
        let Some(element) = self.element else {
            return None;
        };
        let Some(local_face) = self.local_face else {
            return None;
        };

        Some(ElementFaceRef::new(element, local_face))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FemElement {
    pub id: ElementId,

    pub element_type: ElementType,

    pub nodes: Vec<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FemNodeSet {
    pub name: String,

    pub nodes: Vec<NodeId>,
}

impl FemNodeSet {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            nodes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FemElementSet {
    pub name: String,

    pub elements: Vec<ElementId>,
}

impl FemElementSet {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            elements: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FemSurfaceSet {
    pub name: String,

    pub surfaces: Vec<ElementFaceRef>,
}

impl FemSurfaceSet {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            surfaces: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ElementFaceRef {
    pub element: ElementId,

    pub local_face: LocalFaceId,
}

impl ElementFaceRef {
    pub const fn new(element: ElementId, local_face: LocalFaceId) -> Self {
        Self {
            element,
            local_face,
        }
    }
}

impl FemElement {
    pub fn new(id: ElementId, element_type: ElementType, nodes: Vec<NodeId>) -> Self {
        Self {
            id,
            element_type,
            nodes,
        }
    }

    pub fn new_hex8(id: ElementId, nodes: [NodeId; 8]) -> Self {
        Self {
            id,
            element_type: ElementType::Hex8,
            nodes: nodes.to_vec(),
        }
    }

    pub fn edge_node_ids(&self) -> Vec<[NodeId; 2]> {
        match &self.element_type {
            ElementType::Rod2 => self.corner_edges(&[(0, 1)]),
            ElementType::Tri3 | ElementType::Tri6 => self.corner_edges(&[(0, 1), (1, 2), (2, 0)]),
            ElementType::Quad4 | ElementType::Quad8 => {
                self.corner_edges(&[(0, 1), (1, 2), (2, 3), (3, 0)])
            }
            ElementType::Tet4 | ElementType::Tet10 => {
                self.corner_edges(&[(0, 1), (1, 2), (2, 0), (0, 3), (1, 3), (2, 3)])
            }
            ElementType::Prism6 | ElementType::Prism15 => self.corner_edges(&[
                (0, 1),
                (1, 2),
                (2, 0),
                (3, 4),
                (4, 5),
                (5, 3),
                (0, 3),
                (1, 4),
                (2, 5),
            ]),
            ElementType::Hex8 | ElementType::Hex20 => {
                if self.nodes.len() < 8 {
                    return Vec::new();
                }

                const EDGES: [(usize, usize); 12] = [
                    (0, 1),
                    (1, 2),
                    (2, 3),
                    (3, 0),
                    (4, 5),
                    (5, 6),
                    (6, 7),
                    (7, 4),
                    (0, 4),
                    (1, 5),
                    (2, 6),
                    (3, 7),
                ];

                EDGES
                    .iter()
                    .map(|(a, b)| [self.nodes[*a], self.nodes[*b]])
                    .collect()
            }
            ElementType::Unsupported(_) => Vec::new(),
            // Beam elements: one edge between node 0 and node 1.
            ElementType::Beam611 | ElementType::Beam641 => self.corner_edges(&[(0, 1)]),
        }
    }

    pub fn face_node_ids(&self) -> Vec<Vec<NodeId>> {
        match &self.element_type {
            ElementType::Rod2 => Vec::new(),
            ElementType::Tri3 | ElementType::Tri6 => self.corner_faces(&[&[0, 1, 2]]),
            ElementType::Quad4 | ElementType::Quad8 => self.corner_faces(&[&[0, 1, 2, 3]]),
            ElementType::Tet4 | ElementType::Tet10 => {
                self.corner_faces(&[&[0, 2, 1], &[0, 1, 3], &[1, 2, 3], &[2, 0, 3]])
            }
            ElementType::Prism6 | ElementType::Prism15 => self.corner_faces(&[
                &[0, 2, 1],
                &[3, 4, 5],
                &[0, 1, 4, 3],
                &[1, 2, 5, 4],
                &[2, 0, 3, 5],
            ]),
            ElementType::Hex8 | ElementType::Hex20 => {
                if self.nodes.len() < 8 {
                    return Vec::new();
                }

                const FACES: [[usize; 4]; 6] = [
                    [0, 1, 2, 3],
                    [4, 7, 6, 5],
                    [0, 4, 5, 1],
                    [1, 5, 6, 2],
                    [2, 6, 7, 3],
                    [3, 7, 4, 0],
                ];

                FACES
                    .iter()
                    .map(|face| face.iter().map(|index| self.nodes[*index]).collect())
                    .collect()
            }
            // Beam elements and unsupported types have no surface faces.
            ElementType::Beam611 | ElementType::Beam641 => Vec::new(),
            ElementType::Unsupported(_) => Vec::new(),
        }
    }

    fn corner_edges(&self, edge_indices: &[(usize, usize)]) -> Vec<[NodeId; 2]> {
        edge_indices
            .iter()
            .filter_map(|(a, b)| Some([*self.nodes.get(*a)?, *self.nodes.get(*b)?]))
            .collect()
    }

    fn corner_faces(&self, face_indices: &[&[usize]]) -> Vec<Vec<NodeId>> {
        face_indices
            .iter()
            .filter_map(|indices| {
                indices
                    .iter()
                    .map(|index| self.nodes.get(*index).copied())
                    .collect::<Option<Vec<_>>>()
            })
            .collect()
    }
}

impl FemModel {
    pub fn single_mesh(name: impl Into<String>, mesh: FemMesh) -> Self {
        let mut mesh = mesh;
        mesh.rebuild_topology_cache();

        Self {
            parts: vec![Part {
                name: name.into(),
                mesh_index: 0,
            }],
            meshes: vec![mesh],
            contacts: Vec::new(),
        }
    }

    /// Adds `mesh` as a new [`Part`] alongside the model's existing meshes,
    /// rather than replacing them.
    ///
    /// This is the "assembly" counterpart to [`FemModel::single_mesh`]: it
    /// supports building up a mixed-dimensional assembly (e.g. a shell body
    /// plus a separately meshed bracket) by importing files one at a time,
    /// per CLAUDE.md's "solid-shell-beam hybrid assembly" goal. Returns the
    /// new mesh's index into [`FemModel::meshes`].
    pub fn add_mesh(&mut self, name: impl Into<String>, mesh: FemMesh) -> usize {
        let mut mesh = mesh;
        mesh.rebuild_topology_cache();

        let mesh_index = self.meshes.len();

        self.meshes.push(mesh);
        self.parts.push(Part {
            name: name.into(),
            mesh_index,
        });

        mesh_index
    }

    pub fn demo_hex8() -> Self {
        Self::single_mesh("Demo Hex8", FemMesh::demo_hex8())
    }

    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut bounds = self.meshes.iter().filter_map(FemMesh::bounds);
        let (mut min, mut max) = bounds.next()?;

        for (mesh_min, mesh_max) in bounds {
            min = min.min(mesh_min);
            max = max.max(mesh_max);
        }

        Some((min, max))
    }

    pub fn create_surface_set_from_targets(
        &mut self,
        name: impl Into<String>,
        targets: &[FemEntityId],
    ) -> usize {
        let name = name.into();

        self.meshes
            .iter_mut()
            .map(|mesh| mesh.push_surface_set_from_targets(name.clone(), targets))
            .sum()
    }

    pub fn surface_set_refs(&self) -> Vec<SurfaceSetRef> {
        self.meshes
            .iter()
            .enumerate()
            .flat_map(|(mesh_index, mesh)| {
                (0..mesh.surface_sets.len())
                    .map(move |surface_set_index| SurfaceSetRef::new(mesh_index, surface_set_index))
            })
            .collect()
    }

    pub fn surface_set_name(&self, surface_set_ref: SurfaceSetRef) -> Option<&str> {
        self.meshes
            .get(surface_set_ref.mesh_index)?
            .surface_sets
            .get(surface_set_ref.surface_set_index)
            .map(|surface_set| surface_set.name.as_str())
    }

    pub fn create_contact_pair_from_recent_surface_sets(
        &mut self,
        name: impl Into<String>,
        contact_type: ContactType,
    ) -> Option<usize> {
        let surface_sets = self.surface_set_refs();

        if surface_sets.len() < 2 {
            return None;
        }

        let master = surface_sets[surface_sets.len() - 2];
        let slave = surface_sets[surface_sets.len() - 1];
        self.contacts
            .push(ContactPair::new(name, master, slave, contact_type));

        Some(self.contacts.len() - 1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ElementType {
    /// 2-node truss/rod (111)
    Rod2,

    /// Flat tri3 shell (711) or solid plane tri3 (231)
    Tri3,
    /// Curved tri6 shell (761) or solid tri6 (232)
    Tri6,
    /// Flat quad4 shell (741) or solid plane quad4 (241)
    Quad4,
    /// Curved quad8 shell (781) or solid quad8 (242)
    Quad8,

    Tet4,
    Tet10,
    Prism6,
    Prism15,
    Hex8,
    Hex20,

    /// 3-DOF/node Euler-Bernoulli beam (611) — 3 translational DOFs only.
    /// Cannot be mixed with solid elements (FrontISTR limitation).
    Beam611,

    /// 6-DOF/node Timoshenko beam (641) — 3 translation + 3 rotation DOFs.
    /// Can be mixed with solid elements.
    Beam641,

    Unsupported(String),
}

impl ElementType {
    /// `true` for beam/truss line elements (Rod2, Beam611, Beam641), rendered
    /// as cylinders along their axis.
    pub const fn is_beam(&self) -> bool {
        matches!(self, Self::Rod2 | Self::Beam611 | Self::Beam641)
    }

    /// `true` for 2-D shell elements (Tri3/Tri6/Quad4/Quad8, including the
    /// HECMW flat-shell 711/741 codes that map to Tri3/Quad4 and the
    /// curved-shell 761/781 codes that map to Tri6/Quad8), rendered as thin
    /// extruded plates using the assigned [`crate::Section`] thickness.
    pub const fn is_shell(&self) -> bool {
        matches!(self, Self::Tri3 | Self::Tri6 | Self::Quad4 | Self::Quad8)
    }

    /// `true` for 3-D solid elements (tets, hexes, prisms/wedges), for which
    /// a bounding-box cuboid is already a reasonable representative shape.
    pub const fn is_solid(&self) -> bool {
        matches!(
            self,
            Self::Tet4 | Self::Tet10 | Self::Prism6 | Self::Prism15 | Self::Hex8 | Self::Hex20
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdgeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FaceId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ElementId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalFaceId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FemEntityId {
    Node(NodeId),

    Edge(EdgeId),

    Face(FaceId),

    Element(ElementId),
}

impl FemEntityId {
    pub const fn level(self) -> SelectionLevel {
        match self {
            Self::Node(_) => SelectionLevel::Node,
            Self::Edge(_) => SelectionLevel::Edge,
            Self::Face(_) => SelectionLevel::Face,
            Self::Element(_) => SelectionLevel::Element,
        }
    }
}

fn ordered_pair(a: NodeId, b: NodeId) -> (NodeId, NodeId) {
    if a <= b { (a, b) } else { (b, a) }
}

fn derive_edges(elements: &[FemElement]) -> Vec<FemEdge> {
    let mut seen = BTreeSet::new();
    let mut edges = Vec::new();

    for element in elements {
        for nodes in element.edge_node_ids() {
            let key = ordered_pair(nodes[0], nodes[1]);

            if seen.insert(key) {
                edges.push(FemEdge {
                    id: EdgeId(edges.len() as u32),
                    nodes,
                });
            }
        }
    }

    edges
}

fn derive_faces(elements: &[FemElement]) -> Vec<FemFace> {
    let mut seen = BTreeSet::new();
    let mut faces = Vec::new();

    for element in elements {
        for (local_index, nodes) in element.face_node_ids().into_iter().enumerate() {
            let mut key = nodes.clone();
            key.sort();

            if seen.insert(key) {
                faces.push(FemFace {
                    id: FaceId(faces.len() as u32),
                    nodes,
                    element: Some(element.id),
                    local_face: Some(LocalFaceId((local_index + 1) as u32)),
                });
            }
        }
    }

    faces
}

fn derive_boundary_faces(elements: &[FemElement]) -> Vec<FemFace> {
    let mut face_counts =
        BTreeMap::<Vec<NodeId>, (Vec<NodeId>, ElementId, LocalFaceId, usize)>::new();

    for element in elements {
        for (local_index, nodes) in element.face_node_ids().into_iter().enumerate() {
            let mut key = nodes.clone();
            key.sort();

            let entry = face_counts.entry(key).or_insert((
                nodes,
                element.id,
                LocalFaceId((local_index + 1) as u32),
                0,
            ));
            entry.3 += 1;
        }
    }

    face_counts
        .into_values()
        .filter_map(|(nodes, element, local_face, count)| {
            if count == 1 {
                Some(FemFace {
                    id: FaceId(0),
                    nodes,
                    element: Some(element),
                    local_face: Some(local_face),
                })
            } else {
                None
            }
        })
        .enumerate()
        .map(|(index, mut face)| {
            face.id = FaceId(index as u32);
            face
        })
        .collect()
}

fn derive_boundary_edges(elements: &[FemElement]) -> Vec<FemEdge> {
    let boundary_faces = derive_boundary_faces(elements);

    derive_boundary_edges_from_faces(&boundary_faces)
}

fn derive_boundary_edges_from_faces(faces: &[FemFace]) -> Vec<FemEdge> {
    let mut seen = BTreeSet::new();
    let mut edges = Vec::new();

    for face in faces {
        if face.nodes.len() < 2 {
            continue;
        }

        for index in 0..face.nodes.len() {
            let a = face.nodes[index];
            let b = face.nodes[(index + 1) % face.nodes.len()];
            let key = ordered_pair(a, b);

            if seen.insert(key) {
                edges.push(FemEdge {
                    id: EdgeId(edges.len() as u32),
                    nodes: [a, b],
                });
            }
        }
    }

    edges
}
