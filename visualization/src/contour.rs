//! Results get their own surface. Base geometry/material colors are never
//! overwritten, so clearing a contour restores the current assignments.
use crate::demo_mesh::{
    ContourSettings, FemMeshVisual, FemPartVisual, VisualLayer, VisualizationMode,
    VisualizationSettings, build_contour_edge_mesh, build_contour_surface_mesh,
};
use bevy::{pbr::wireframe::Wireframe, prelude::*};
use fem_core::{FemModel, FemModelVersion, FemResultSet};

#[derive(Resource, Default)]
pub(crate) struct ContourSurface(Vec<RenderedSurface>);
struct RenderedSurface {
    entity: Entity,
    mesh_index: usize,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    edge_entity: Entity,
    edge_mesh: Handle<Mesh>,
}
#[derive(Component)]
pub(crate) struct ContourSuppressed;

#[derive(Component)]
pub(crate) struct ResultHiddenOverlay(Visibility);

pub(crate) fn hide_pre_overlays_in_result_view(
    mut commands: Commands,
    geometry: Option<Res<fem_core::ResultGeometry>>,
    mut overlays: Query<
        (Entity, &mut Visibility, Option<&ResultHiddenOverlay>),
        Or<(
            With<crate::demo_mesh::TopologyHighlight>,
            With<crate::boundary_viz::BoundaryVisual>,
        )>,
    >,
) {
    let hidden = geometry
        .as_ref()
        .is_some_and(|g| g.visible && g.model.is_some());
    for (entity, mut visibility, previous) in &mut overlays {
        if hidden {
            if previous.is_none() {
                commands
                    .entity(entity)
                    .insert(ResultHiddenOverlay(*visibility));
            }
            *visibility = Visibility::Hidden;
        } else if let Some(previous) = previous {
            *visibility = previous.0;
            commands.entity(entity).remove::<ResultHiddenOverlay>();
        }
    }
}

pub(crate) fn update_contour_surface(
    mut commands: Commands,
    model: Option<Res<FemModel>>,
    geometry: Option<Res<fem_core::ResultGeometry>>,
    version: Res<FemModelVersion>,
    results: Res<FemResultSet>,
    settings: Res<VisualizationSettings>,
    range_mode: Option<Res<crate::ContourRangeMode>>,
    mut surface: ResMut<ContourSurface>,
    mut last_contour: Local<Option<ContourSettings>>,
    mut last_version: Local<Option<u64>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let rebuild = range_mode.as_ref().is_some_and(|r| r.is_changed()) || *last_version != Some(version.value)
        || *last_contour != settings.contour
        || results.is_changed()
        || geometry.as_ref().is_some_and(|g| g.is_changed());
    *last_version = Some(version.value);
    if rebuild {
        *last_contour = settings.contour.clone();
        let mut previous = std::mem::take(&mut surface.0);
        let visible = geometry.as_ref().is_none_or(|g| g.visible);
        let model = geometry
            .as_deref()
            .and_then(|g| g.model.as_ref())
            .or(model.as_deref());
        if let (true, Some(contour), Some(model)) = (visible, &settings.contour, model) {
            let range = crate::contour_range::resolve(&results, range_mode.as_deref().copied().unwrap_or_default());
            for (mesh_index, mesh) in model.meshes.iter().enumerate() {
                let Some(step) = results
                    .by_mesh
                    .get(mesh_index)
                    .and_then(|steps| steps.get(contour.step_index))
                else {
                    continue;
                };
                if let (Some(selected), Some(field)) = (results.active_field(), step.field_by_name(&contour.field_name)) {
                    if !crate::contour_range::same_kind(selected, field) { continue; }
                }
                let Some(built) = build_contour_surface_mesh(mesh, step, contour, range) else {
                    continue;
                };
                let Some(edges) = build_contour_edge_mesh(mesh, step, contour) else {
                    continue;
                };
                let current =
                    if let Some(index) = previous.iter().position(|s| s.mesh_index == mesh_index) {
                        let mut current = previous.swap_remove(index);
                        if let Some(mut asset) = meshes.get_mut(&current.mesh) {
                            *asset = built;
                        } else {
                            current.mesh = meshes.add(built);
                        }
                        commands
                            .entity(current.entity)
                            .insert(Mesh3d(current.mesh.clone()));
                        if let Some(mut asset) = meshes.get_mut(&current.edge_mesh) {
                            *asset = edges;
                        } else {
                            current.edge_mesh = meshes.add(edges);
                        }
                        commands
                            .entity(current.edge_entity)
                            .insert(Mesh3d(current.edge_mesh.clone()));
                        current
                    } else {
                        let mesh = meshes.add(built);
                        let material = materials.add(StandardMaterial {
                            unlit: true,
                            cull_mode: None,
                            double_sided: true,
                            ..default()
                        });
                        let entity = commands
                            .spawn((
                                Mesh3d(mesh.clone()),
                                MeshMaterial3d(material.clone()),
                                Transform::default(),
                                FemPartVisual { mesh_index },
                                Name::new("Result contour surface"),
                            ))
                            .id();
                        let edge_mesh = meshes.add(edges);
                        let edge_material = materials.add(StandardMaterial {
                            base_color: Color::srgb(0.04, 0.05, 0.055),
                            unlit: true,
                            ..default()
                        });
                        let edge_entity = commands
                            .spawn((
                                Mesh3d(edge_mesh.clone()),
                                MeshMaterial3d(edge_material),
                                Transform::default(),
                                FemPartVisual { mesh_index },
                                Name::new("Result contour edges"),
                            ))
                            .id();
                        RenderedSurface {
                            entity,
                            mesh_index,
                            mesh,
                            material,
                            edge_entity,
                            edge_mesh,
                        }
                    };
                surface.0.push(current);
            }
        }
        for old in previous {
            commands.entity(old.entity).despawn();
            commands.entity(old.edge_entity).despawn();
        }
    }
    if !rebuild && !settings.is_changed() {
        return;
    }
    for current in &surface.0 {
        commands.entity(current.edge_entity).insert(
            if VisualLayer::Edge.visible_in(settings.mode) {
                Visibility::Visible
            } else {
                Visibility::Hidden
            },
        );
        let mut entity = commands.entity(current.entity);
        entity.insert(if VisualLayer::Shaded.visible_in(settings.mode) {
            Visibility::Visible
        } else {
            Visibility::Hidden
        });
        if settings.mode == VisualizationMode::Wireframe {
            entity.insert(Wireframe);
        } else {
            entity.remove::<Wireframe>();
        }
        if let Some(mut material) = materials.get_mut(&current.material) {
            let xray = settings.mode == VisualizationMode::Transparent;
            material.base_color = Color::srgba(1.0, 1.0, 1.0, if xray { 0.18 } else { 1.0 });
            material.alpha_mode = if xray {
                AlphaMode::Blend
            } else {
                AlphaMode::Opaque
            };
        }
    }
}

/// Hide undeformed surfaces/edges/node markers only on parts with results.
/// Result surfaces and edges share the current displacement and step.
pub(crate) fn apply_contour_visibility(
    mut commands: Commands,
    surface: Res<ContourSurface>,
    settings: Res<VisualizationSettings>,
    geometry: Option<Res<fem_core::ResultGeometry>>,
    mut visuals: Query<
        (
            Entity,
            &FemPartVisual,
            &VisualLayer,
            &mut Visibility,
            Option<&ContourSuppressed>,
        ),
        With<FemMeshVisual>,
    >,
) {
    for (entity, part, layer, mut visibility, suppressed) in &mut visuals {
        if geometry
            .as_ref()
            .is_some_and(|g| g.visible && g.model.is_some())
            || surface.0.iter().any(|s| s.mesh_index == part.mesh_index)
        {
            *visibility = Visibility::Hidden;
            if suppressed.is_none() {
                commands.entity(entity).insert(ContourSuppressed);
            }
        } else if suppressed.is_some() {
            *visibility = if layer.visible_in(settings.mode) {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
            commands.entity(entity).remove::<ContourSuppressed>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fem_core::{FemMesh, ResultField, StepResult};

    #[test]
    fn changing_display_field_recolors_surface_without_changing_deformation() {
        use bevy::mesh::VertexAttributeValues;
        let model = FemMesh::demo_hex8();
        let step = StepResult {
            fields: vec![
                ResultField::NodeVector {
                    name: "Displacement".into(), values: vec![Vec3::X; 8], min_mag: 1.0, max_mag: 1.0,
                },
                ResultField::NodeScalar {
                    name: "Nodal stress".into(), values: (0..8).map(|i| i as f32).collect(), min: 0.0, max: 7.0,
                },
                ResultField::ElementScalar {
                    name: "Custom element field".into(), values: vec![0.0], min: 0.0, max: 1.0,
                },
            ],
            ..default()
        };
        let mut settings = ContourSettings {
            mesh_index: 0, step_index: 0, field_name: "Nodal stress".into(),
            show_deformation: true, displacement_field: "Displacement".into(), deformation_scale: 3.0,
        };
        let before = build_contour_surface_mesh(&model, &step, &settings, None).unwrap();
        settings.field_name = "Custom element field".into();
        let after = build_contour_surface_mesh(&model, &step, &settings, None).unwrap();
        let Some(VertexAttributeValues::Float32x3(before_positions)) = before.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!("positions missing"); };
        let Some(VertexAttributeValues::Float32x3(after_positions)) = after.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!("positions missing"); };
        assert_eq!(before_positions, after_positions);
        assert!(!after_positions.is_empty());
        assert!(after_positions.iter().all(|p| model.nodes.iter().any(|n| n.position + 3.0 * Vec3::X == Vec3::from_array(*p))));
        let Some(VertexAttributeValues::Float32x4(before_colors)) = before.attribute(Mesh::ATTRIBUTE_COLOR) else { panic!("colors missing"); };
        let Some(VertexAttributeValues::Float32x4(after_colors)) = after.attribute(Mesh::ATTRIBUTE_COLOR) else { panic!("colors missing"); };
        assert_ne!(before_colors, after_colors);
    }

    #[test]
    fn result_edges_follow_surface_deformation_scale_and_step() {
        let mesh = FemMesh::demo_hex8();
        let mut settings = ContourSettings {
            mesh_index: 0,
            step_index: 0,
            field_name: "Displacement".into(),
            show_deformation: true,
            displacement_field: "Displacement".into(),
            deformation_scale: 20.,
        };
        for displacement in [Vec3::X, Vec3::new(0., 2., 1.)] {
            let step = StepResult {
                fields: vec![ResultField::NodeVector {
                    name: "Displacement".into(),
                    values: vec![displacement; 8],
                    min_mag: displacement.length(),
                    max_mag: displacement.length(),
                }],
                ..default()
            };
            for enabled in [true, false] {
                settings.show_deformation = enabled;
                let edges = build_contour_edge_mesh(&mesh, &step, &settings).unwrap();
                let surface = build_contour_surface_mesh(&mesh, &step, &settings, None).unwrap();
                let Some(bevy::mesh::VertexAttributeValues::Float32x3(edge_points)) =
                    edges.attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    panic!()
                };
                let Some(bevy::mesh::VertexAttributeValues::Float32x3(surface_points)) =
                    surface.attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    panic!()
                };
                let offset = if enabled {
                    displacement * 20.
                } else {
                    Vec3::ZERO
                };
                for p in edge_points {
                    assert!(surface_points.contains(p));
                    assert!(
                        mesh.nodes
                            .iter()
                            .any(|n| n.position + offset == Vec3::from_array(*p))
                    );
                }
            }
        }
    }

    #[test]
    fn contour_is_scoped_to_one_part_and_restores_base_surfaces() {
        let mut app = App::new();
        let mut model = FemModel::demo_hex8();
        model.add_mesh("second", FemMesh::demo_hex8());
        app.insert_resource(model)
            .init_resource::<FemModelVersion>()
            .init_resource::<VisualizationSettings>()
            .init_resource::<ContourSurface>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .insert_resource(FemResultSet {
                by_mesh: vec![vec![StepResult {
                    fields: vec![ResultField::NodeScalar {
                        name: "S".into(),
                        values: vec![1.0; 8],
                        min: 0.0,
                        max: 1.0,
                    }],
                    ..default()
                }]],
                ..default()
            })
            .add_systems(
                Update,
                (update_contour_surface, apply_contour_visibility).chain(),
            );
        let base = app
            .world_mut()
            .spawn((
                FemMeshVisual,
                FemPartVisual { mesh_index: 0 },
                VisualLayer::Shaded,
                Visibility::Visible,
            ))
            .id();
        let edge = app
            .world_mut()
            .spawn((
                FemMeshVisual,
                FemPartVisual { mesh_index: 0 },
                VisualLayer::Edge,
                Visibility::Visible,
            ))
            .id();
        let other = app
            .world_mut()
            .spawn((
                FemMeshVisual,
                FemPartVisual { mesh_index: 1 },
                VisualLayer::Shaded,
                Visibility::Visible,
            ))
            .id();
        let contour = ContourSettings {
            mesh_index: 0,
            step_index: 0,
            field_name: "S".into(),
            show_deformation: false,
            displacement_field: String::new(),
            deformation_scale: 1.0,
        };
        app.world_mut()
            .resource_mut::<VisualizationSettings>()
            .contour = Some(contour);
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(base),
            Some(&Visibility::Hidden)
        );
        assert_eq!(
            app.world().get::<Visibility>(edge),
            Some(&Visibility::Hidden)
        );
        assert_eq!(
            app.world().get::<Visibility>(other),
            Some(&Visibility::Visible)
        );
        let handle = app
            .world()
            .resource::<ContourSurface>()
            .0
            .first()
            .unwrap()
            .mesh
            .clone();
        // Recoloring the base or another UI frame does not rebuild the contour.
        app.update();
        assert_eq!(
            app.world()
                .resource::<ContourSurface>()
                .0
                .first()
                .unwrap()
                .mesh,
            handle
        );
        // A second part's results create a second surface, reusing the first.
        let step = app.world().resource::<FemResultSet>().by_mesh[0][0].clone();
        app.world_mut()
            .resource_mut::<FemResultSet>()
            .by_mesh
            .push(vec![step]);
        app.update();
        assert_eq!(app.world().resource::<ContourSurface>().0.len(), 2);
        assert_eq!(
            app.world().get::<Visibility>(other),
            Some(&Visibility::Hidden)
        );
        assert_eq!(app.world().resource::<ContourSurface>().0[0].mesh, handle);
        app.world_mut()
            .resource_mut::<VisualizationSettings>()
            .contour = None;
        app.update();
        assert!(app.world().resource::<ContourSurface>().0.is_empty());
        assert_eq!(
            app.world().get::<Visibility>(edge),
            Some(&Visibility::Visible)
        );
        assert_eq!(
            app.world().get::<Visibility>(base),
            Some(&Visibility::Visible)
        );
        assert_eq!(
            app.world().get::<Visibility>(other),
            Some(&Visibility::Visible)
        );
    }
}
