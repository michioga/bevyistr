//! Results get their own surface. Base geometry/material colors are never
//! overwritten, so clearing a contour restores the current assignments.
use crate::demo_mesh::{
    ContourSettings, FemMeshVisual, FemPartVisual, VisualLayer, VisualizationMode,
    VisualizationSettings, build_contour_surface_mesh,
};
use bevy::{pbr::wireframe::Wireframe, prelude::*};
use fem_core::{FemModel, FemModelVersion, FemResultSet};

#[derive(Resource, Default)]
pub(crate) struct ContourSurface(Option<RenderedSurface>);
struct RenderedSurface {
    entity: Entity,
    mesh_index: usize,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}
#[derive(Component)]
pub(crate) struct ContourSuppressed;

pub(crate) fn update_contour_surface(
    mut commands: Commands,
    model: Option<Res<FemModel>>,
    version: Res<FemModelVersion>,
    results: Res<FemResultSet>,
    settings: Res<VisualizationSettings>,
    mut surface: ResMut<ContourSurface>,
    mut last_contour: Local<Option<ContourSettings>>,
    mut last_version: Local<Option<u64>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let rebuild = *last_version != Some(version.value)
        || *last_contour != settings.contour
        || results.is_changed();
    *last_version = Some(version.value);
    if rebuild {
        *last_contour = settings.contour.clone();
        let built = settings.contour.as_ref().and_then(|contour| {
            let mesh = model.as_ref()?.meshes.get(contour.mesh_index)?;
            let step = results
                .by_mesh
                .get(contour.mesh_index)?
                .get(contour.step_index)?;
            Some((
                contour.mesh_index,
                build_contour_surface_mesh(mesh, step, contour)?,
            ))
        });
        if let Some((mesh_index, mesh)) = built {
            if let Some(current) = &mut surface.0 {
                // Reuse the GPU asset identity while scrubbing timesteps.
                if let Some(mut asset) = meshes.get_mut(&current.mesh) {
                    *asset = mesh;
                } else {
                    current.mesh = meshes.add(mesh);
                }
                current.mesh_index = mesh_index;
                commands
                    .entity(current.entity)
                    .insert((Mesh3d(current.mesh.clone()), FemPartVisual { mesh_index }));
            } else {
                let mesh = meshes.add(mesh);
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
                surface.0 = Some(RenderedSurface {
                    entity,
                    mesh_index,
                    mesh,
                    material,
                });
            }
        } else if let Some(current) = surface.0.take() {
            commands.entity(current.entity).despawn();
        }
    }
    if !rebuild && !settings.is_changed() {
        return;
    }
    if let Some(current) = &surface.0 {
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

/// Run after regular/contact visibility. Never replace edges, nodes or the
/// surfaces of other parts with the currently selected result mesh.
pub(crate) fn apply_contour_visibility(
    mut commands: Commands,
    surface: Res<ContourSurface>,
    settings: Res<VisualizationSettings>,
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
        if *layer == VisualLayer::Shaded
            && surface
                .0
                .as_ref()
                .is_some_and(|s| s.mesh_index == part.mesh_index)
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
            Some(&Visibility::Visible)
        );
        assert_eq!(
            app.world().get::<Visibility>(other),
            Some(&Visibility::Visible)
        );
        let handle = app
            .world()
            .resource::<ContourSurface>()
            .0
            .as_ref()
            .unwrap()
            .mesh
            .clone();
        // Recoloring the base or another UI frame does not rebuild the contour.
        app.update();
        assert_eq!(
            app.world()
                .resource::<ContourSurface>()
                .0
                .as_ref()
                .unwrap()
                .mesh,
            handle
        );
        app.world_mut()
            .resource_mut::<VisualizationSettings>()
            .contour = None;
        app.update();
        assert!(app.world().resource::<ContourSurface>().0.is_none());
        assert_eq!(
            app.world().get::<Visibility>(base),
            Some(&Visibility::Visible)
        );
    }
}
