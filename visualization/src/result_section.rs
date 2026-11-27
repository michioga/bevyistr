//! Planar cuts through first-order tetrahedra in the result's reference geometry.
use bevy::{asset::RenderAssetUsages, math::DVec3, mesh::PrimitiveTopology, prelude::*};
use fem_core::{
    ElementType, FemModel, FemModelVersion, FemResultSet, ResultField, ResultGeometry,
    rainbow_color,
};
use std::collections::{HashMap, HashSet};

use crate::{ContourRangeMode, VisualizationSettings, contour_range};

#[derive(Resource, Clone, Copy, PartialEq)]
pub struct ResultSectionSettings {
    pub enabled: bool,
    pub normal: Vec3,
    /// Position from the minimum to maximum projection onto the plane normal.
    pub fraction: f32,
    pub exterior: bool,
}

impl Default for ResultSectionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            normal: Vec3::Z,
            fraction: 0.5,
            exterior: false,
        }
    }
}

#[derive(Resource, Default, Clone, Copy, PartialEq)]
pub struct ResultSectionDisplay {
    pub status: SectionStatus,
    pub exterior: bool,
    pub range: Option<(f32, f32)>,
}

impl ResultSectionDisplay {
    pub fn is_cut(self) -> bool {
        matches!(self.status, SectionStatus::Visible | SectionStatus::Empty)
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum SectionStatus {
    #[default]
    Inactive,
    NeedsTetrahedra,
    NeedsNodalField,
    Empty,
    Visible,
}

#[derive(Clone, Copy)]
struct Vertex {
    point: Vec3,
    from: usize,
    to: usize,
    weight: f32,
}

type Triangle = [Vertex; 3];

struct Cut {
    normal: Vec3,
    pieces: Vec<Vec<Triangle>>,
}

fn is_tet(element_type: &ElementType) -> bool {
    matches!(element_type, ElementType::Tet4 | ElementType::FluidTet4)
}

impl Cut {
    fn new(model: &FemModel, normal: Vec3, fraction: f32) -> Option<Self> {
        let normal = normal.try_normalize()?;
        let axis = normal.as_dvec3();
        let project = |point: Vec3| axis.dot(point.as_dvec3());
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        let mut supports_cut = false;
        for mesh in &model.meshes {
            if mesh
                .elements
                .iter()
                .any(|element| is_tet(&element.element_type))
            {
                supports_cut = true;
                for node in &mesh.nodes {
                    let distance = project(node.position);
                    if distance.is_finite() {
                        min = min.min(distance);
                        max = max.max(distance);
                    }
                }
            }
        }
        if !supports_cut {
            return None;
        }
        let coordinate = min + (max - min) * f64::from(fraction.clamp(0.0, 1.0));
        let tangent = axis.any_orthonormal_vector();
        let bitangent = axis.cross(tangent);
        let epsilon = (max - min) * 1e-9;
        let pieces = model
            .meshes
            .iter()
            .map(|mesh| {
                let indices: HashMap<_, _> = mesh
                    .nodes
                    .iter()
                    .enumerate()
                    .map(|(i, node)| (node.id, i))
                    .collect();
                let distances: Vec<_> = mesh
                    .nodes
                    .iter()
                    .map(|node| {
                        let distance = project(node.position) - coordinate;
                        if distance.abs() <= epsilon {
                            0.0
                        } else {
                            distance
                        }
                    })
                    .collect();
                let mut triangles = Vec::new();
                for element in &mesh.elements {
                    if !is_tet(&element.element_type) {
                        continue;
                    }
                    let [a, b, c, d] = element.nodes.as_slice() else {
                        continue;
                    };
                    let [Some(a), Some(b), Some(c), Some(d)] =
                        [a, b, c, d].map(|id| indices.get(id).copied())
                    else {
                        continue;
                    };
                    let ids = [a, b, c, d];
                    let points = ids.map(|i| mesh.nodes[i].position);
                    cut_tetrahedron(
                        ids,
                        points,
                        ids.map(|i| distances[i]),
                        tangent,
                        bitangent,
                        &mut triangles,
                    );
                }
                triangles
            })
            .collect();
        Some(Self { normal, pieces })
    }
}

const TET_EDGES: [(usize, usize); 6] = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];

fn point_key(point: Vec3) -> [u32; 3] {
    point
        .to_array()
        .map(|value| if value == 0.0 { 0 } else { value.to_bits() })
}

fn cut_tetrahedron(
    ids: [usize; 4],
    points: [Vec3; 4],
    distances: [f64; 4],
    tangent: DVec3,
    bitangent: DVec3,
    triangles: &mut Vec<Triangle>,
) {
    if distances.iter().any(|d| !d.is_finite())
        || distances.iter().all(|&d| d > 0.0)
        || distances.iter().all(|&d| d < 0.0)
        || distances.iter().all(|&d| d == 0.0)
    {
        return;
    }
    let mut polygon = [Vertex {
        point: Vec3::ZERO,
        from: 0,
        to: 0,
        weight: 0.0,
    }; 4];
    let mut count = 0;
    for (i, distance) in distances.into_iter().enumerate() {
        if distance == 0.0 {
            polygon[count] = Vertex {
                point: points[i],
                from: ids[i],
                to: ids[i],
                weight: 0.0,
            };
            count += 1;
        }
    }
    for (mut a, mut b) in TET_EDGES {
        let crosses = (distances[a] < 0.0 && distances[b] > 0.0)
            || (distances[a] > 0.0 && distances[b] < 0.0);
        if !crosses {
            continue;
        }
        // A shared edge must produce exactly the same position in either order,
        // including in separate VTU pieces with independent node numbering.
        if point_key(points[a]) > point_key(points[b]) {
            std::mem::swap(&mut a, &mut b);
        }
        let weight = distances[a] / (distances[a] - distances[b]);
        let point = points[a]
            .as_dvec3()
            .lerp(points[b].as_dvec3(), weight)
            .as_vec3();
        polygon[count] = Vertex {
            point,
            from: ids[a],
            to: ids[b],
            weight: weight as f32,
        };
        count += 1;
    }
    if count < 3 {
        return;
    }
    let polygon = &mut polygon[..count];
    let center = polygon.iter().map(|v| v.point.as_dvec3()).sum::<DVec3>() / count as f64;
    let angle = |v: &Vertex| {
        let offset = v.point.as_dvec3() - center;
        bitangent.dot(offset).atan2(tangent.dot(offset))
    };
    polygon.sort_unstable_by(|a, b| angle(a).total_cmp(&angle(b)));
    let first = (0..count)
        .min_by_key(|&i| point_key(polygon[i].point))
        .unwrap();
    polygon.rotate_left(first);
    for i in 1..(count - 1) {
        let triangle = [polygon[0], polygon[i], polygon[i + 1]];
        let [a, b, c] = triangle.map(|v| v.point.as_dvec3());
        if (b - a).cross(c - a).length_squared() > 0.0 {
            triangles.push(triangle);
        }
    }
}

impl Vertex {
    fn sample(self, field: &ResultField) -> Option<f32> {
        let value = match field {
            ResultField::NodeScalar { values, .. } => {
                let (a, b) = (*values.get(self.from)?, *values.get(self.to)?);
                (f64::from(a) * f64::from(1.0 - self.weight)
                    + f64::from(b) * f64::from(self.weight)) as f32
            }
            ResultField::NodeVector { values, .. } => values
                .get(self.from)?
                .as_dvec3()
                .lerp(values.get(self.to)?.as_dvec3(), f64::from(self.weight))
                .length() as f32,
            ResultField::ElementScalar { .. } => return None,
        };
        value.is_finite().then_some(value)
    }
}

fn build_surface(
    cut: &Cut,
    results: &FemResultSet,
    mode: ContourRangeMode,
) -> Option<(Mesh, (f32, f32))> {
    let active = results.active.as_ref()?;
    let selected = results.active_field()?;
    let mut seen = HashSet::new();
    let mut positions = Vec::new();
    let mut values = Vec::new();
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for (triangles, steps) in cut.pieces.iter().zip(&results.by_mesh) {
        let Some(field) = steps
            .get(active.step_index)
            .and_then(|step| step.field_by_name(&active.field_name))
            .filter(|field| contour_range::same_kind(field, selected))
        else {
            continue;
        };
        for triangle in triangles {
            let [Some(a), Some(b), Some(c)] = triangle.map(|vertex| vertex.sample(field)) else {
                continue;
            };
            let mut key = triangle.map(|vertex| point_key(vertex.point));
            key.sort_unstable();
            if !seen.insert(key) {
                continue;
            }
            positions.extend(triangle.map(|v| v.point.to_array()));
            values.extend([a, b, c]);
            lo = lo.min(a).min(b).min(c);
            hi = hi.max(a).max(b).max(c);
        }
    }
    if positions.is_empty() {
        return None;
    }
    let range = if mode == ContourRangeMode::AllFrames {
        contour_range::resolve(results, mode)
            .map(|(min, max)| {
                // Interpolated vectors can cancel even if nodal magnitudes never reach zero.
                (
                    if matches!(selected, ResultField::NodeVector { .. }) {
                        0.0
                    } else {
                        min
                    },
                    max,
                )
            })
            .unwrap_or((lo, hi))
    } else {
        (lo, hi)
    };
    let colors: Vec<_> = values
        .into_iter()
        .map(|value| {
            let color = rainbow_color(contour_range::normalize(value, range));
            [color.red, color.green, color.blue, color.alpha]
        })
        .collect();
    let normals = vec![cut.normal.to_array(); positions.len()];
    Some((
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors),
        range,
    ))
}

#[derive(Default)]
pub(crate) struct SectionCache {
    plane: Option<(Vec3, f32)>,
    cut: Option<Cut>,
    entity: Option<(Entity, Handle<Mesh>)>,
}

pub(crate) fn update_result_section(
    mut commands: Commands,
    setup: Res<ResultSectionSettings>,
    mut display: ResMut<ResultSectionDisplay>,
    model: Option<Res<FemModel>>,
    geometry: Option<Res<ResultGeometry>>,
    version: Res<FemModelVersion>,
    results: Res<FemResultSet>,
    range_mode: Res<ContourRangeMode>,
    settings: Res<VisualizationSettings>,
    mut cache: Local<SectionCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let geometry_changed = version.is_changed()
        || geometry.as_ref().is_some_and(|g| g.is_changed())
        || model.as_ref().is_some_and(|model| model.is_changed());
    if !geometry_changed
        && !setup.is_changed()
        && !results.is_changed()
        && !range_mode.is_changed()
        && !settings.is_changed()
    {
        return;
    }
    if geometry_changed {
        cache.plane = None;
        cache.cut = None;
    }
    let mut next = ResultSectionDisplay::default();
    let model = geometry
        .as_deref()
        .and_then(|g| g.model.as_ref())
        .or(model.as_deref());
    let mut surface = None;
    if let (true, Some(model), Some(field)) = (
        setup.enabled && settings.contour.is_some() && geometry.as_ref().is_none_or(|g| g.visible),
        model,
        results.active_field(),
    ) {
        let plane = (setup.normal, setup.fraction);
        if cache.plane != Some(plane) {
            cache.cut = Cut::new(model, setup.normal, setup.fraction);
            cache.plane = Some(plane);
        }
        if let Some(cut) = &cache.cut {
            if matches!(field, ResultField::ElementScalar { .. }) {
                next.status = SectionStatus::NeedsNodalField;
            } else {
                next.exterior = setup.exterior;
                surface = build_surface(cut, &results, *range_mode);
                next.status = if surface.is_some() {
                    SectionStatus::Visible
                } else {
                    SectionStatus::Empty
                };
            }
        } else {
            next.status = SectionStatus::NeedsTetrahedra;
        }
    }
    if let Some((mesh, range)) = surface {
        next.range = Some(range);
        if let Some((entity, handle)) = &mut cache.entity {
            if let Some(mut asset) = meshes.get_mut(&*handle) {
                *asset = mesh;
            } else {
                *handle = meshes.add(mesh);
                commands.entity(*entity).insert(Mesh3d(handle.clone()));
            }
        } else {
            let handle = meshes.add(mesh);
            let material = materials.add(StandardMaterial {
                unlit: true,
                cull_mode: None,
                double_sided: true,
                ..default()
            });
            let entity = commands
                .spawn((
                    Mesh3d(handle.clone()),
                    MeshMaterial3d(material),
                    Transform::default(),
                    Name::new("Result section"),
                ))
                .id();
            cache.entity = Some((entity, handle));
        }
    } else if let Some((entity, _)) = cache.entity.take() {
        commands.entity(entity).despawn();
    }
    display.set_if_neq(next);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;
    use fem_core::{ActiveResult, ElementId, FemElement, FemMesh, FemNode, NodeId, StepResult};

    fn tet(points: [Vec3; 4], connectivity: [u32; 4]) -> FemMesh {
        FemMesh::new(
            points
                .into_iter()
                .enumerate()
                .map(|(i, p)| FemNode::new(NodeId(i as u32 + 1), p))
                .collect(),
            vec![FemElement::new(
                ElementId(1),
                ElementType::FluidTet4,
                connectivity.map(NodeId).to_vec(),
            )],
        )
    }

    fn positions(mesh: &Mesh) -> &[[f32; 3]] {
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("Section has no positions");
        };
        positions
    }

    fn results(by_mesh: Vec<Vec<StepResult>>, name: &str) -> FemResultSet {
        FemResultSet {
            by_mesh,
            active: Some(ActiveResult {
                mesh_index: 0,
                step_index: 0,
                field_name: name.to_string(),
            }),
        }
    }

    #[test]
    fn oblique_cut_interpolates_linear_nodal_values_and_deduplicates_shared_faces() {
        let first = tet([Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z], [1, 2, 3, 4]);
        let neighbor = tet([Vec3::ZERO, Vec3::X, Vec3::Y, -Vec3::Z], [4, 3, 2, 1]);
        let duplicate = tet([Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z], [4, 3, 2, 1]);
        let mut model = FemModel::single_mesh("first", first);
        model.meshes.extend([neighbor, duplicate]);
        let values = |p: Vec3| 2.0 + p.dot(Vec3::new(1.0, 3.0, 4.0));
        let frames = model
            .meshes
            .iter()
            .map(|mesh| {
                vec![StepResult {
                    fields: vec![ResultField::NodeScalar {
                        name: "Pressure".into(),
                        min: -2.0,
                        max: 6.0,
                        values: mesh
                            .nodes
                            .iter()
                            .map(|node| values(node.position))
                            .collect(),
                    }],
                    ..default()
                }]
            })
            .collect();
        let results = results(frames, "Pressure");
        let normal = Vec3::new(1.0, 1.0, 0.0);
        let cut = Cut::new(&model, normal, 0.5).unwrap();
        for (piece, steps) in cut.pieces.iter().zip(&results.by_mesh) {
            assert!(!piece.is_empty());
            for vertex in piece.iter().flatten() {
                assert!((vertex.point.x + vertex.point.y - 0.5).abs() < 1e-6);
                assert!(
                    (vertex.sample(&steps[0].fields[0]).unwrap() - values(vertex.point)).abs()
                        < 1e-6
                );
            }
        }
        let (slice, _) = build_surface(&cut, &results, ContourRangeMode::CurrentFrame).unwrap();
        assert_eq!(
            positions(&slice).len(),
            3 * (cut.pieces[0].len() + cut.pieces[1].len())
        );

        let coplanar = Cut::new(&model, Vec3::Z, 0.5).unwrap();
        let (face, _) = build_surface(&coplanar, &results, ContourRangeMode::CurrentFrame).unwrap();
        assert_eq!(positions(&face).len(), 3);
    }

    #[test]
    fn vector_magnitude_is_measured_after_interpolation_and_missing_values_stay_missing() {
        let mesh = tet([Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z], [1, 2, 3, 4]);
        let model = FemModel::single_mesh("unit tet", mesh);
        let cut = Cut::new(&model, Vec3::X, 0.5).unwrap();
        let field = ResultField::NodeVector {
            name: "Velocity".into(),
            values: vec![-Vec3::X, Vec3::X, -Vec3::X, -Vec3::X],
            min_mag: 1.0,
            max_mag: 1.0,
        };
        let mut results = results(
            vec![vec![StepResult {
                fields: vec![field],
                ..default()
            }]],
            "Velocity",
        );
        let (_, range) = build_surface(&cut, &results, ContourRangeMode::CurrentFrame).unwrap();
        assert_eq!(range, (0.0, 0.0));
        let (_, range) = build_surface(&cut, &results, ContourRangeMode::AllFrames).unwrap();
        assert_eq!(range, (0.0, 1.0));
        let ResultField::NodeVector { values, .. } = &mut results.by_mesh[0][0].fields[0] else {
            unreachable!()
        };
        values[1] = Vec3::splat(f32::NAN);
        assert!(build_surface(&cut, &results, ContourRangeMode::CurrentFrame).is_none());
    }

    #[test]
    fn section_keeps_its_geometry_across_frames_and_restores_the_exterior() {
        use crate::ContourSettings;
        use crate::contour::{ContourSurface, update_contour_surface};
        let mesh = tet([Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z], [1, 2, 3, 4]);
        let frames = [0.0, 7.0].map(|value| StepResult {
            fields: vec![ResultField::NodeScalar {
                name: "Pressure".into(),
                values: vec![value; 4],
                min: value,
                max: value,
            }],
            ..default()
        });
        let mut app = App::new();
        app.insert_resource(FemModel::single_mesh("unit tet", mesh))
            .insert_resource(results(vec![frames.into()], "Pressure"))
            .insert_resource(VisualizationSettings {
                contour: Some(ContourSettings {
                    mesh_index: 0,
                    step_index: 0,
                    field_name: "Pressure".into(),
                    show_deformation: false,
                    displacement_field: String::new(),
                    deformation_scale: 1.0,
                }),
                ..default()
            })
            .insert_resource(ResultSectionSettings {
                enabled: true,
                ..default()
            })
            .init_resource::<ResultSectionDisplay>()
            .init_resource::<ResultGeometry>()
            .init_resource::<FemModelVersion>()
            .init_resource::<ContourRangeMode>()
            .init_resource::<ContourSurface>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(
                Update,
                (update_result_section, update_contour_surface).chain(),
            );
        let find = |app: &mut App, label: &str| {
            app.world_mut()
                .query::<(Entity, &Name, &Mesh3d)>()
                .iter(app.world())
                .find_map(|(entity, name, mesh)| {
                    (name.as_str() == label).then(|| (entity, mesh.0.clone()))
                })
        };
        app.update();
        let original = find(&mut app, "Result section").unwrap();
        let points = positions(
            app.world()
                .resource::<Assets<Mesh>>()
                .get(&original.1)
                .unwrap(),
        )
        .to_vec();
        assert_eq!(
            app.world().resource::<ResultSectionDisplay>().range,
            Some((0.0, 0.0))
        );
        assert!(find(&mut app, "Result contour surface").is_none());

        app.world_mut()
            .resource_mut::<FemResultSet>()
            .active
            .as_mut()
            .unwrap()
            .step_index = 1;
        app.world_mut()
            .resource_mut::<VisualizationSettings>()
            .contour
            .as_mut()
            .unwrap()
            .step_index = 1;
        app.update();
        assert_eq!(find(&mut app, "Result section").unwrap(), original);
        assert_eq!(
            positions(
                app.world()
                    .resource::<Assets<Mesh>>()
                    .get(&original.1)
                    .unwrap()
            ),
            points
        );
        assert_eq!(
            app.world().resource::<ResultSectionDisplay>().range,
            Some((7.0, 7.0))
        );

        app.world_mut()
            .resource_mut::<ResultSectionSettings>()
            .enabled = false;
        app.update();
        assert!(find(&mut app, "Result section").is_none());
        assert!(find(&mut app, "Result contour surface").is_some());
    }

    #[test]
    #[ignore = "Requires the actual FrontISTR cavity-flow PVTU and MPI RES outputs"]
    fn cavity_pressure_sections_cover_the_same_area_in_pvtu_and_mpi_res() {
        use hecmw::{
            load_mesh_file,
            result_series::{FrameSelection, load_mesh_result_frames},
            vtk_scene::load_scene_frames,
        };
        let root = std::path::PathBuf::from(std::env::var_os("BEVYISTR_TUTORIAL_DIR").unwrap())
            .join("18_cavity_flow");
        let mesh = load_mesh_file(root.join("cavityflow.msh")).unwrap();
        let steps = load_mesh_result_frames(
            &root.join("cavityflow.res.0.10000"),
            &mesh,
            FrameSelection::Selected,
            |_, _| {},
        )
        .unwrap();
        let vtk = load_scene_frames(
            &root.join("cavityflow_vis_psf.10000.pvtu"),
            FrameSelection::Selected,
            |_, _| {},
        )
        .unwrap();
        let native = FemModel::single_mesh("Cavity", mesh);
        let (min, max) = native.bounds().unwrap();
        let native_results = results(vec![steps], "PRESSURE");
        let vtk_results = results(vtk.steps, "PRESSURE");
        for normal in [Vec3::Z, Vec3::new(1.0, 1.0, 0.25)] {
            let area = |model, results| {
                let cut = Cut::new(model, normal, 0.5).unwrap();
                let (mesh, range) =
                    build_surface(&cut, results, ContourRangeMode::CurrentFrame).unwrap();
                assert!(
                    range.0.is_finite() && range.1.is_finite() && range.0 < range.1,
                    "Pressure range: {range:?}"
                );
                positions(&mesh)
                    .chunks_exact(3)
                    .map(|triangle| {
                        let [a, b, c] = [triangle[0], triangle[1], triangle[2]]
                            .map(|p| Vec3::from_array(p).as_dvec3());
                        (b - a).cross(c - a).length() * 0.5
                    })
                    .sum::<f64>()
            };
            let native_area = area(&native, &native_results);
            let vtk_area = area(&vtk.model, &vtk_results);
            println!("{normal:?}: MPI RES area {native_area:.8}, PVTU area {vtk_area:.8}");
            assert!((native_area - vtk_area).abs() < native_area * 1e-4);
            if normal == Vec3::Z {
                let expected = f64::from((max.x - min.x) * (max.y - min.y));
                assert!((native_area - expected).abs() < expected * 1e-4);
            }
        }
    }
}
