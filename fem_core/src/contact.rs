use std::collections::BTreeSet;

use bevy::prelude::*;

use crate::{
    ContactPair, ContactType, ElementFaceRef, FaceId, FemFace, FemMesh, FemModel, FemSurfaceSet,
    SurfaceSetRef,
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
    /// Maximum centroid-to-centroid distance for two boundary faces to be
    /// considered a potential contact pair.
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

    /// Mean centroid-to-centroid distance across all matched face pairs.
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
    /// any.
    pub fn refresh(&mut self, model: &FemModel) {
        self.candidates = model.find_contact_candidates(&self.params);
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

fn shares_node(a: &FemFace, b: &FemFace) -> bool {
    a.nodes.iter().any(|node| b.nodes.contains(node))
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
    /// Searches for nearby, opposing-or-coincident boundary face groups
    /// across (and within) the model's meshes, returning one
    /// [`ContactCandidate`] per mesh pair that has at least one matching
    /// face pair.
    ///
    /// Two boundary faces are considered a match when their centroids are
    /// within `params.max_gap` of each other, their normals are roughly
    /// opposing or roughly aligned (within `params.normal_tolerance_deg`),
    /// and they do not already share a node (which would make them ordinary
    /// adjacent mesh topology rather than a contact interface).
    pub fn find_contact_candidates(&self, params: &ContactSearchParams) -> Vec<ContactCandidate> {
        let max_gap = params.max_gap.max(0.0);
        let cos_alignment = params.normal_tolerance_deg.to_radians().cos();

        let geometry: Vec<Vec<Option<FaceGeometry>>> = self
            .meshes
            .iter()
            .map(|mesh| {
                mesh.cached_boundary_faces()
                    .iter()
                    .map(|face| mesh.face_geometry(face))
                    .collect()
            })
            .collect();

        let mut candidates = Vec::new();

        for mesh_a in 0..self.meshes.len() {
            for mesh_b in mesh_a..self.meshes.len() {
                if let Some(candidate) =
                    self.find_candidate_between(&geometry, mesh_a, mesh_b, max_gap, cos_alignment)
                {
                    candidates.push(candidate);
                }
            }
        }

        candidates
    }

    fn find_candidate_between(
        &self,
        geometry: &[Vec<Option<FaceGeometry>>],
        mesh_a: usize,
        mesh_b: usize,
        max_gap: f32,
        cos_alignment: f32,
    ) -> Option<ContactCandidate> {
        let faces_a = self.meshes[mesh_a].cached_boundary_faces();
        let faces_b = self.meshes[mesh_b].cached_boundary_faces();

        if faces_a.is_empty() || faces_b.is_empty() {
            return None;
        }

        let mut matched_a = BTreeSet::new();
        let mut matched_b = BTreeSet::new();
        let mut gap_sum = 0.0f32;
        let mut pair_count = 0usize;

        for (index_a, face_a) in faces_a.iter().enumerate() {
            let Some(geom_a) = geometry[mesh_a][index_a] else {
                continue;
            };

            // Broad-phase: only consider mesh_b faces whose AABB is within
            // `max_gap` of this face's centroid, via the shared BVH.
            for index_b in self.meshes[mesh_b].boundary_face_indices_near(geom_a.centroid, max_gap) {
                // When searching a mesh against itself, only consider each
                // unordered pair once and never pair a face with itself.
                if mesh_a == mesh_b && index_b <= index_a {
                    continue;
                }

                let face_b = &faces_b[index_b];

                let Some(geom_b) = geometry[mesh_b][index_b] else {
                    continue;
                };

                if shares_node(face_a, face_b) {
                    continue;
                }

                let gap = geom_a.centroid.distance(geom_b.centroid);

                if gap > max_gap {
                    continue;
                }

                let alignment = geom_a.normal.dot(geom_b.normal);
                let opposing = alignment <= -cos_alignment;
                let coincident = alignment >= cos_alignment;

                if !opposing && !coincident {
                    continue;
                }

                matched_a.insert(face_a.id);
                matched_b.insert(face_b.id);
                gap_sum += gap;
                pair_count += 1;
            }
        }

        if pair_count == 0 {
            return None;
        }

        Some(ContactCandidate {
            mesh_a,
            mesh_b,
            faces_a: matched_a.into_iter().collect(),
            faces_b: matched_b.into_iter().collect(),
            pair_count,
            average_gap: gap_sum / pair_count as f32,
        })
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
}
