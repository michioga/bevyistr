use super::*;
use fem_core::{FemMesh, ResultField, StepResult};

fn settings() -> ContourSettings {
    ContourSettings {
        mesh_index: 0,
        step_index: 0,
        field_name: "E".into(),
        show_deformation: true,
        displacement_field: "U".into(),
        deformation_scale: 1.0,
    }
}

#[test]
fn geometry_field_scale_and_scene_replacement_invalidate_before_next_query() {
    let mut cache = ProbeCache::default();
    let mut model = FemModel::single_mesh("sample", FemMesh::demo_hex8());
    let mut results = FemResultSet {
        by_mesh: vec![vec![StepResult {
            fields: vec![
                ResultField::ElementScalar {
                    name: "E".into(),
                    values: vec![42.0],
                    min: 42.0,
                    max: 42.0,
                },
                ResultField::NodeVector {
                    name: "U".into(),
                    values: vec![Vec3::X; 8],
                    min_mag: 1.0,
                    max_mag: 1.0,
                },
            ],
            ..default()
        }]],
        ..default()
    };
    let mut contour = settings();
    let sample = |cache: &mut ProbeCache,
                  model: &FemModel,
                  results: &FemResultSet,
                  contour: &ContourSettings,
                  x: f32| {
        let field = results.by_mesh[0][0]
            .field_by_name(&contour.field_name)
            .unwrap();
        cache.surfaces(model, results, contour)[0].1.pick(
            Vec3::new(x, 0.0, 10.0),
            Vec3::NEG_Z,
            field,
        )
    };
    cache.sync(Some(&contour), true);
    assert_eq!(
        sample(&mut cache, &model, &results, &contour, 1.0)
            .unwrap()
            .value,
        Some(42.0)
    );
    contour.deformation_scale = 5.0;
    cache.sync(Some(&contour), false);
    assert_eq!(cache.builds, 1); // invalidation is not construction
    assert!(sample(&mut cache, &model, &results, &contour, 1.0).is_none());
    assert_eq!(
        sample(&mut cache, &model, &results, &contour, 5.0)
            .unwrap()
            .value,
        Some(42.0)
    );
    contour.field_name = "U".into();
    cache.sync(Some(&contour), false);
    let hit = sample(&mut cache, &model, &results, &contour, 5.0).unwrap();
    assert!(hit.node.is_some());
    assert_eq!(hit.value, Some(1.0));
    contour.show_deformation = false;
    cache.sync(Some(&contour), false);
    assert!(sample(&mut cache, &model, &results, &contour, 5.0).is_none());
    assert!(sample(&mut cache, &model, &results, &contour, 0.0).is_some());
    // Loading another scene with the same IDs and settings cannot reuse its BVH.
    for node in &mut model.meshes[0].nodes {
        node.position += Vec3::X * 20.0;
    }
    cache.sync(Some(&contour), true);
    assert!(sample(&mut cache, &model, &results, &contour, 0.0).is_none());
    assert!(sample(&mut cache, &model, &results, &contour, 20.0).is_some());
    results.by_mesh[0][0].fields.clear();
    cache.sync(Some(&contour), true);
    assert!(
        cache.surfaces(&model, &results, &contour)[0]
            .1
            .pick(
                Vec3::new(20.0, 0.0, 10.0),
                Vec3::NEG_Z,
                &ResultField::ElementScalar {
                    name: "E".into(),
                    values: vec![42.0],
                    min: 42.0,
                    max: 42.0
                }
            )
            .is_none()
    );
    cache.sync(None, false);
    assert!(cache.surfaces.is_none());
}
