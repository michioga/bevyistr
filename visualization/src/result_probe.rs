//! Read-only picking against the same deformed boundary triangles as contours.
use crate::ContourSettings;
use bevy::prelude::*;
use fem_core::{Aabb, Bvh, ElementId, FemMesh, NodeId, ResultField, StepResult};

/// Shared by contour rendering and probing; scale only affects the location,
/// never the reported result value.
pub(crate) fn deformed_position(
    position: Vec3,
    index: usize,
    displacement: Option<&ResultField>,
    scale: f32,
) -> Vec3 {
    if let Some(ResultField::NodeVector { values, .. }) = displacement {
        if let Some(value) = values.get(index) {
            return position + *value * scale;
        }
    }
    position
}

struct Triangle {
    points: [Vec3; 3],
    nodes: [(NodeId, usize); 3],
    element: Option<(ElementId, usize)>,
}

pub struct ProbeSurface {
    triangles: Vec<Triangle>,
    bvh: Bvh,
}

#[derive(Debug, Clone)]
pub struct ProbeHit {
    pub distance: f32,
    /// Sample location: selected vertex for a nodal field, hit point for an element field.
    pub point: Vec3,
    pub element: Option<ElementId>,
    /// Some only for nodal fields. The value is sampled at this triangle vertex,
    /// not interpolated at the cursor and not converted to an element result.
    pub node: Option<NodeId>,
    pub value: Option<f32>,
}

impl ProbeSurface {
    pub fn build(mesh: &FemMesh, step: &StepResult, settings: &ContourSettings) -> Self {
        let mut triangles = Vec::new();
        let Some(field) = step.field_by_name(&settings.field_name) else {
            return Self::empty();
        };
        let valid = match field {
            ResultField::NodeScalar { values, .. } => values.len() == mesh.nodes.len(),
            ResultField::NodeVector { values, .. } => values.len() == mesh.nodes.len(),
            ResultField::ElementScalar { values, .. } => values.len() == mesh.elements.len(),
        };
        if !valid {
            return Self::empty();
        }
        let nodes: std::collections::HashMap<_, _> = mesh
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect();
        let elements: std::collections::HashMap<_, _> = mesh
            .elements
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id, i))
            .collect();
        let displacement = if settings.show_deformation {
            step.field_by_name(&settings.displacement_field)
        } else {
            None
        };
        for face in mesh.cached_boundary_faces() {
            let element = face
                .element
                .and_then(|id| elements.get(&id).map(|&i| (id, i)));
            if matches!(field, ResultField::ElementScalar { .. }) && element.is_none() {
                continue;
            }
            let Some(indices): Option<Vec<_>> =
                face.nodes.iter().map(|id| nodes.get(id).copied()).collect()
            else {
                continue;
            };
            let points: Vec<_> = indices
                .iter()
                .map(|&i| {
                    deformed_position(
                        mesh.nodes[i].position,
                        i,
                        displacement,
                        settings.deformation_scale,
                    )
                })
                .collect();
            if points.len() < 3 || points.iter().any(|p| !p.is_finite()) {
                continue;
            }
            // Exactly the fan triangulation used by build_contour_surface_mesh.
            for i in 1..points.len() - 1 {
                let corners = [0, i, i + 1];
                triangles.push(Triangle {
                    points: corners.map(|j| points[j]),
                    nodes: corners.map(|j| (mesh.nodes[indices[j]].id, indices[j])),
                    element,
                });
            }
        }
        let bounds: Vec<_> = triangles
            .iter()
            .map(|t| {
                let b = Aabb::from_points(&t.points).unwrap();
                // Give flat boxes thickness, avoiding 0 * infinity in slab tests
                // for axis-aligned camera rays exactly on a boundary.
                b.expanded(b.extent().max_element().max(1.0) * 1e-6)
            })
            .collect();
        Self {
            bvh: Bvh::build(&bounds),
            triangles,
        }
    }

    fn empty() -> Self {
        Self {
            triangles: vec![],
            bvh: Bvh::build(&[]),
        }
    }

    pub fn pick(&self, origin: Vec3, direction: Vec3, field: &ResultField) -> Option<ProbeHit> {
        if !origin.is_finite() || !direction.is_finite() {
            return None;
        }
        let mut best: Option<ProbeHit> = None;
        for index in self.bvh.query_ray(origin, direction) {
            let triangle = &self.triangles[index as usize];
            let Some(distance) = intersect(origin, direction, triangle.points) else {
                continue;
            };
            if best.as_ref().is_some_and(|h| h.distance <= distance) {
                continue;
            }
            let point = origin + direction * distance;
            let corner = (0..3)
                .min_by(|&a, &b| {
                    triangle.points[a]
                        .distance_squared(point)
                        .total_cmp(&triangle.points[b].distance_squared(point))
                })
                .unwrap();
            let (id, node_index) = triangle.nodes[corner];
            let (node, value) = match field {
                ResultField::NodeScalar { values, .. } => {
                    (Some(id), values.get(node_index).copied())
                }
                ResultField::NodeVector { values, .. } => {
                    (Some(id), values.get(node_index).map(|v| v.length()))
                }
                ResultField::ElementScalar { values, .. } => (
                    None,
                    triangle.element.and_then(|(_, i)| values.get(i).copied()),
                ),
            };
            best = Some(ProbeHit {
                distance,
                point: if node.is_some() {
                    triangle.points[corner]
                } else {
                    point
                },
                node,
                value: value.filter(|v| v.is_finite()),
                element: triangle.element.map(|(id, _)| id),
            });
        }
        best
    }
}

fn intersect(origin: Vec3, direction: Vec3, points: [Vec3; 3]) -> Option<f32> {
    let [a, b, c] = points.map(|p| p.as_dvec3());
    let e1 = b - a;
    let e2 = c - a;
    let p = direction.as_dvec3().cross(e2);
    let determinant = e1.dot(p);
    if determinant.abs() <= f64::EPSILON * e1.length() * e2.length() * 16.0 {
        return None;
    }
    let t = origin.as_dvec3() - a;
    let u = t.dot(p) / determinant;
    let q = t.cross(e1);
    let v = direction.as_dvec3().dot(q) / determinant;
    if u < -1e-8 || v < -1e-8 || u + v > 1.0 + 1e-8 {
        return None;
    }
    let distance = (e2.dot(q) / determinant) as f32;
    (distance.is_finite() && distance > 0.0).then_some(distance)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settings() -> ContourSettings {
        ContourSettings {
            mesh_index: 0,
            step_index: 0,
            field_name: "E".into(),
            show_deformation: true,
            displacement_field: "Displacement".into(),
            deformation_scale: 10.0,
        }
    }
    #[test]
    fn probe_tracks_deformation_and_preserves_element_value_and_id() {
        let mesh = FemMesh::demo_hex8();
        let step = StepResult {
            fields: vec![
                ResultField::ElementScalar {
                    name: "E".into(),
                    values: vec![42.0],
                    min: 42.0,
                    max: 42.0,
                },
                ResultField::NodeVector {
                    name: "Displacement".into(),
                    values: vec![Vec3::X; 8],
                    min_mag: 1.0,
                    max_mag: 1.0,
                },
            ],
            ..default()
        };
        let center = mesh.nodes.iter().map(|n| n.position).sum::<Vec3>() / mesh.nodes.len() as f32;
        let index = ProbeSurface::build(&mesh, &step, &settings());
        let field = &step.fields[0];
        assert!(
            index
                .pick(center + Vec3::Z * 100.0, Vec3::NEG_Z, field)
                .is_none()
        );
        let hit = index
            .pick(
                center + Vec3::X * 10.0 + Vec3::Z * 100.0,
                Vec3::NEG_Z,
                field,
            )
            .unwrap();
        assert_eq!(hit.element, Some(mesh.elements[0].id));
        assert_eq!(hit.node, None);
        assert_eq!(hit.value, Some(42.0));
        let mut off = settings();
        off.show_deformation = false;
        assert!(
            ProbeSurface::build(&mesh, &step, &off)
                .pick(center + Vec3::Z * 100.0, Vec3::NEG_Z, field)
                .is_some()
        );
    }
    #[test]
    fn nodal_values_are_not_interpolated_or_zero_filled() {
        let mesh = FemMesh::demo_hex8();
        let mut settings = settings();
        settings.show_deformation = false;
        settings.field_name = "N".into();
        let mut field = ResultField::NodeScalar {
            name: "N".into(),
            values: (0..8).map(|i| i as f32 + 10.0).collect(),
            min: 10.0,
            max: 17.0,
        };
        let step = StepResult {
            fields: vec![field.clone()],
            ..default()
        };
        let index = ProbeSurface::build(&mesh, &step, &settings);
        let center = mesh.nodes.iter().map(|n| n.position).sum::<Vec3>() / 8.0;
        let origin = center + Vec3::Z * 100.0;
        let hit = index.pick(origin, Vec3::NEG_Z, &field).unwrap();
        let i = mesh
            .nodes
            .iter()
            .position(|n| Some(n.id) == hit.node)
            .unwrap();
        assert_eq!(hit.value, Some(i as f32 + 10.0));
        if let ResultField::NodeScalar { values, .. } = &mut field {
            values.fill(f32::NAN);
        }
        assert_eq!(index.pick(origin, Vec3::NEG_Z, &field).unwrap().value, None);
    }

    #[test]
    fn probe_triangles_match_rendered_positions_and_pick_front_surface() {
        use bevy::mesh::{Indices, VertexAttributeValues};
        let mesh = FemMesh::demo_hex8();
        let mut settings = settings();
        settings.field_name = "Displacement".into();
        let step = StepResult {
            fields: vec![ResultField::NodeVector {
                name: "Displacement".into(),
                values: vec![Vec3::new(0.3, 0.4, 0.0); mesh.nodes.len()],
                min_mag: 0.5,
                max_mag: 0.5,
            }],
            ..default()
        };
        let rendered =
            crate::demo_mesh::build_contour_surface_mesh(&mesh, &step, &settings, None).unwrap();
        let Some(VertexAttributeValues::Float32x3(vertices)) =
            rendered.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("expected surface positions");
        };
        let indices: Vec<usize> = match rendered.indices() {
            Some(Indices::U16(v)) => v.iter().map(|&i| i as usize).collect(),
            Some(Indices::U32(v)) => v.iter().map(|&i| i as usize).collect(),
            None => (0..vertices.len()).collect(),
        };
        let surface = ProbeSurface::build(&mesh, &step, &settings);
        assert_eq!(surface.triangles.len() * 3, indices.len());
        for (triangle, indices) in surface.triangles.iter().zip(indices.chunks_exact(3)) {
            for (p, &i) in triangle.points.iter().zip(indices) {
                assert_eq!(*p, Vec3::from_array(vertices[i]));
            }
        }
        let center = mesh.nodes.iter().map(|n| n.position).sum::<Vec3>() / mesh.nodes.len() as f32;
        let hit = surface
            .pick(
                center + Vec3::new(3.0, 4.0, 100.0),
                Vec3::NEG_Z,
                &step.fields[0],
            )
            .unwrap();
        let front_z = mesh
            .nodes
            .iter()
            .map(|n| n.position.z)
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!(hit.point.z, front_z);
        assert!((hit.distance - (center.z + 100.0 - front_z)).abs() < 1e-5);
        assert_eq!(hit.value, Some(0.5)); // magnitude, not multiplied by deformation scale
    }
}
