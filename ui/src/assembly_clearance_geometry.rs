//! Triangle surfaces for the on-demand assembly clearance query.
use bevy::prelude::Vec3;
use fem_core::{Aabb, Bvh, FemMesh};
use parry3d::math::Pose;
use parry3d::query::{ClosestPoints, Ray, RayCast, closest_points, contact};
use parry3d::shape::Triangle;
use std::collections::HashSet;

#[derive(Clone, Debug)]
pub(super) struct BoundarySurface {
    triangles: Vec<Triangle>,
    solid_triangles: Vec<bool>,
    tree: Bvh,
}

pub(super) struct SurfaceDistance {
    pub squared: f32,
    pub points: Option<(Vec3, Vec3)>,
}

impl BoundarySurface {
    pub(super) fn from_mesh(mesh: &FemMesh) -> Option<Self> {
        let solid_elements: HashSet<_> = mesh
            .elements
            .iter()
            .filter(|element| element.element_type.is_solid())
            .map(|element| element.id)
            .collect();
        let mut triangles = Vec::new();
        let mut solid_triangles = Vec::new();
        let mut bounds = Vec::new();
        for face in mesh.cached_boundary_faces() {
            if face.nodes.len() < 3 {
                continue;
            }
            let Some(points) = mesh.node_positions(&face.nodes) else {
                continue;
            };
            if points.iter().any(|point| !point.is_finite()) {
                continue;
            }
            let solid = face.element.is_some_and(|id| solid_elements.contains(&id));
            let first = points[0];
            for edge in points[1..].windows(2) {
                let [b, c] = [edge[0], edge[1]];
                let area_squared = (b - first).cross(c - first).length_squared();
                if area_squared.is_finite() && area_squared > 1.0e-20 {
                    triangles.push(Triangle::new(first, b, c));
                    solid_triangles.push(solid);
                    bounds.push(Aabb::from_point(first).expand(b).expand(c));
                }
            }
        }
        if triangles.is_empty() {
            return None;
        }
        Some(Self {
            triangles,
            solid_triangles,
            tree: Bvh::build(&bounds),
        })
    }

    pub(super) fn distance_to(&self, other: &Self, tolerance: f32) -> Option<SurfaceDistance> {
        self.tree.closest_pair(&other.tree, |a, b| {
            let distance = triangle_distance(
                self.triangles[a as usize],
                other.triangles[b as usize],
                tolerance,
            )?;
            Some((distance.squared, distance))
        })
    }

    /// Odd/even crossings of the solid boundary, independent of face winding.
    /// Shell and beam elements do not enclose a solid volume.
    pub(super) fn contains_solid_point(&self, point: Vec3) -> bool {
        let ray = Ray::new(point, Vec3::new(1.0, 0.371, 0.694).normalize());
        let mut hits: Vec<_> = self
            .tree
            .query_ray(ray.origin, ray.dir)
            .into_iter()
            .filter_map(|index| {
                let index = index as usize;
                self.solid_triangles[index]
                    .then(|| self.triangles[index].cast_local_ray(&ray, f32::MAX, false))
                    .flatten()
                    .filter(|distance| *distance > 0.0)
            })
            .collect();
        hits.sort_unstable_by(f32::total_cmp);
        // Coincident hits at a shared edge, vertex or duplicated face count once.
        hits.dedup_by(|a, b| (*a - *b).abs() <= 8.0 * f32::EPSILON * a.abs().max(b.abs()));
        hits.len() % 2 == 1
    }
}

fn triangle_distance(a: Triangle, b: Triangle, tolerance: f32) -> Option<SurfaceDistance> {
    // Keep the GJK computation close to the origin for meshes with an offset.
    let origin = a.a;
    let a = Triangle::new(Vec3::ZERO, a.b - origin, a.c - origin);
    let b = Triangle::new(b.a - origin, b.b - origin, b.c - origin);
    let pose = Pose::IDENTITY;
    match closest_points(&pose, &a, &pose, &b, f32::MAX).ok()? {
        ClosestPoints::WithinMargin(a, b) => Some(SurfaceDistance {
            squared: a.distance_squared(b),
            points: Some((a + origin, b + origin)),
        }),
        ClosestPoints::Intersecting => Some(SurfaceDistance {
            squared: 0.0,
            points: contact(&pose, &a, &pose, &b, tolerance)
                .ok()
                .flatten()
                .filter(|hit| hit.point1.is_finite() && hit.point2.is_finite())
                .map(|hit| (hit.point1 + origin, hit.point2 + origin)),
        }),
        ClosestPoints::Disjoint => None,
    }
}
