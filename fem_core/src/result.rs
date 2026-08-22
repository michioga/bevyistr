use std::collections::HashMap;

use bevy::prelude::*;

use crate::{ElementId, NodeId};

// ─── scalar/vector field data ─────────────────────────────────────────────────

/// A named scalar or vector result field on the nodes or elements of one mesh.
///
/// Both variants store one value per primitive in the same order as the
/// corresponding `FemMesh::nodes` or `FemMesh::elements` slices.
#[derive(Debug, Clone, PartialEq)]
pub enum ResultField {
    /// One scalar value per node, in `FemMesh::nodes` order.
    NodeScalar {
        name: String,
        values: Vec<f32>,
        /// Min value in `values` (cached for colormap normalization).
        min: f32,
        /// Max value in `values` (cached for colormap normalization).
        max: f32,
    },

    /// One 3-component vector per node (e.g. displacement), in
    /// `FemMesh::nodes` order.
    NodeVector {
        name: String,
        values: Vec<Vec3>,
        /// Magnitude range, cached for colormap normalization.
        min_mag: f32,
        max_mag: f32,
    },

    /// One scalar value per element, in `FemMesh::elements` order.
    ElementScalar {
        name: String,
        values: Vec<f32>,
        min: f32,
        max: f32,
    },
}

impl ResultField {
    pub fn name(&self) -> &str {
        match self {
            Self::NodeScalar { name, .. } => name,
            Self::NodeVector { name, .. } => name,
            Self::ElementScalar { name, .. } => name,
        }
    }

    /// Builds a `NodeScalar` field from a parallel (node_id → value) map,
    /// ordered according to `node_ids` (the `nodes` slice of a `FemMesh`).
    /// Nodes missing from the map receive `0.0`.
    pub fn node_scalar(
        name: impl Into<String>,
        node_ids: &[NodeId],
        map: &HashMap<NodeId, f32>,
    ) -> Self {
        let values: Vec<f32> = node_ids
            .iter()
            .map(|id| *map.get(id).unwrap_or(&0.0))
            .collect();

        let min = values.iter().cloned().fold(f32::MAX, f32::min);
        let max = values.iter().cloned().fold(f32::MIN, f32::max);

        Self::NodeScalar {
            name: name.into(),
            values,
            min: if min == f32::MAX { 0.0 } else { min },
            max: if max == f32::MIN { 0.0 } else { max },
        }
    }

    /// Builds a `NodeVector` field (e.g. displacement) from a
    /// (node_id → Vec3) map, ordered according to `node_ids`.
    pub fn node_vector(
        name: impl Into<String>,
        node_ids: &[NodeId],
        map: &HashMap<NodeId, Vec3>,
    ) -> Self {
        let values: Vec<Vec3> = node_ids
            .iter()
            .map(|id| *map.get(id).unwrap_or(&Vec3::ZERO))
            .collect();

        let mut min_mag = f32::MAX;
        let mut max_mag = f32::MIN;

        for v in &values {
            let mag = v.length();
            min_mag = min_mag.min(mag);
            max_mag = max_mag.max(mag);
        }

        Self::NodeVector {
            name: name.into(),
            values,
            min_mag: if min_mag == f32::MAX { 0.0 } else { min_mag },
            max_mag: if max_mag == f32::MIN { 0.0 } else { max_mag },
        }
    }

    /// Builds an `ElementScalar` field from a (element_id → value) map.
    pub fn element_scalar(
        name: impl Into<String>,
        element_ids: &[ElementId],
        map: &HashMap<ElementId, f32>,
    ) -> Self {
        let values: Vec<f32> = element_ids
            .iter()
            .map(|id| *map.get(id).unwrap_or(&0.0))
            .collect();

        let min = values.iter().cloned().fold(f32::MAX, f32::min);
        let max = values.iter().cloned().fold(f32::MIN, f32::max);

        Self::ElementScalar {
            name: name.into(),
            values,
            min: if min == f32::MAX { 0.0 } else { min },
            max: if max == f32::MIN { 0.0 } else { max },
        }
    }

    /// Maps a value to a `[0, 1]` parameter for colormap lookup.
    ///
    /// Returns the midpoint (`0.5`) when the field's min == max.
    pub fn normalize_node_scalar(&self, index: usize) -> f32 {
        match self {
            Self::NodeScalar { values, min, max, .. } => {
                let range = max - min;
                if range < 1.0e-12 {
                    0.5
                } else {
                    ((values[index] - min) / range).clamp(0.0, 1.0)
                }
            }
            _ => 0.5,
        }
    }

    /// Maps the *magnitude* of a `NodeVector` value at `index` to `[0, 1]`.
    pub fn normalize_node_vector_mag(&self, index: usize) -> f32 {
        match self {
            Self::NodeVector { values, min_mag, max_mag, .. } => {
                let range = max_mag - min_mag;
                if range < 1.0e-12 {
                    0.5
                } else {
                    ((values[index].length() - min_mag) / range).clamp(0.0, 1.0)
                }
            }
            _ => 0.5,
        }
    }
}

// ─── step result ─────────────────────────────────────────────────────────────

/// Result fields for one analysis step/increment, for one mesh.
#[derive(Debug, Clone, Default)]
pub struct StepResult {
    /// Step identifier (e.g. FrontISTR step number, VTU series index).
    pub step: u32,

    /// Physical time or load factor associated with this step.
    pub time: f32,

    pub fields: Vec<ResultField>,
}

impl StepResult {
    pub fn field_names(&self) -> Vec<&str> {
        self.fields.iter().map(|f| f.name()).collect()
    }

    pub fn field_by_name(&self, name: &str) -> Option<&ResultField> {
        self.fields.iter().find(|f| f.name() == name)
    }
}

// ─── fem result collection ───────────────────────────────────────────────────

/// All result data attached to one `FemMesh` (potentially multi-step).
///
/// This maps to the `results` field planned in CLAUDE.md's `FemModel` struct.
/// It is stored separately rather than inline in `FemMesh` so that a large
/// model can be loaded without results and results can be streamed/replaced
/// without rebuilding topology caches.
#[derive(Resource, Debug, Clone, Default)]
pub struct FemResultSet {
    /// Results keyed by mesh index (into `FemModel::meshes`).
    pub by_mesh: Vec<Vec<StepResult>>,

    /// Currently displayed step and field.
    pub active: Option<ActiveResult>,
}

/// The step/field combination currently shown in the contour view.
#[derive(Debug, Clone)]
pub struct ActiveResult {
    /// Index into `FemResultSet::by_mesh`.
    pub mesh_index: usize,

    /// Index into `FemResultSet::by_mesh[mesh_index]`.
    pub step_index: usize,

    /// Name of the `ResultField` to display.
    pub field_name: String,
}

impl FemResultSet {
    /// Returns true if any results have been loaded.
    pub fn has_results(&self) -> bool {
        self.by_mesh.iter().any(|steps| !steps.is_empty())
    }

    /// Active `ResultField`, if any is selected and the indices are valid.
    pub fn active_field(&self) -> Option<&ResultField> {
        let active = self.active.as_ref()?;
        let step = self.by_mesh
            .get(active.mesh_index)?
            .get(active.step_index)?;

        step.field_by_name(&active.field_name)
    }

    /// Selects the first available field as the active result.
    pub fn activate_first(&mut self) {
        for (mi, steps) in self.by_mesh.iter().enumerate() {
            if let Some(step) = steps.first() {
                if let Some(field) = step.fields.first() {
                    self.active = Some(ActiveResult {
                        mesh_index: mi,
                        step_index: 0,
                        field_name: field.name().to_string(),
                    });

                    return;
                }
            }
        }
    }
}

// ─── rainbow colormap ────────────────────────────────────────────────────────

/// Maps `t ∈ [0, 1]` to a rainbow `LinearRgba` colour (blue → cyan →
/// green → yellow → red), matching the de-facto standard FEM contour palette.
///
/// Used by both the aggregate surface renderer and the per-face renderer to
/// colour vertices according to a normalised result value.
pub fn rainbow_color(t: f32) -> LinearRgba {
    let t = t.clamp(0.0, 1.0);

    // Piecewise linear through blue→cyan→green→yellow→red
    // (4 segments of 0.25 each)
    let (r, g, b) = if t < 0.25 {
        let s = t / 0.25;
        (0.0, s, 1.0)
    } else if t < 0.5 {
        let s = (t - 0.25) / 0.25;
        (0.0, 1.0, 1.0 - s)
    } else if t < 0.75 {
        let s = (t - 0.5) / 0.25;
        (s, 1.0, 0.0)
    } else {
        let s = (t - 0.75) / 0.25;
        (1.0, 1.0 - s, 0.0)
    };

    LinearRgba::new(r, g, b, 1.0)
}
