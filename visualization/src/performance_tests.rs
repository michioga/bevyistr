//! Opt-in measurements against external, read-only ASCII VTU/PVTU results.
//! Not a performance threshold and not part of the default test run.
use crate::{ContourSettings, demo_mesh, result_probe::ProbeSurface};
use bevy::prelude::*;
use fem_core::ResultField;
use std::{hint::black_box, path::PathBuf, time::Instant};

fn median_ms<T>(mut run: impl FnMut() -> T) -> f64 {
    let mut times = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        let value = run();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        black_box(value); // destruction is outside the measured interval
    }
    times.sort_by(f64::total_cmp);
    times[2]
}

#[test]
#[ignore = "Read-only CPU measurement; set BEVYISTR_BENCH_RESULT to an ASCII VTU/PVTU"]
fn real_result_pipeline_baseline() {
    let path = PathBuf::from(
        std::env::var_os("BEVYISTR_BENCH_RESULT")
            .expect("Set BEVYISTR_BENCH_RESULT to an existing VTU/PVTU"),
    );
    let requested_field = std::env::var("BEVYISTR_BENCH_FIELD").ok();
    let start = Instant::now();
    let scene = hecmw::vtk_scene::load_scene(&path).expect("Result loading failed");
    let first_load_ms = start.elapsed().as_secs_f64() * 1000.0;
    let reload_ms = median_ms(|| hecmw::vtk_scene::load_scene(&path).expect("Reload failed"));
    println!("INPUT {}", path.display());
    println!(
        "LOAD first_ms={first_load_ms:.3} repeat_median_ms={reload_ms:.3} pieces={} frames={}",
        scene.model.meshes.len(),
        scene.steps.first().map_or(0, Vec::len)
    );
    println!("# Each construction: median of 5, destruction excluded; no GPU upload/rendering");
    println!("# Selection measures only undeformed boundary BVH candidates, not full UI picking");

    for (part, mesh) in scene.model.meshes.iter().enumerate() {
        let steps = &scene.steps[part];
        let mut frames = vec![0];
        if steps.len() > 1 {
            frames.push(steps.len() - 1);
        }
        println!(
            "PART {} nodes={} elements={} boundary_faces={}",
            part + 1,
            mesh.nodes.len(),
            mesh.elements.len(),
            mesh.cached_boundary_faces().len()
        );
        for frame in frames {
            let step = &steps[frame];
            let field = requested_field
                .as_deref()
                .and_then(|name| step.field_by_name(name))
                .or_else(|| {
                    if requested_field.is_none() {
                        step.fields
                            .iter()
                            .find(|f| matches!(f, ResultField::NodeScalar { .. }))
                            .or_else(|| step.fields.first())
                    } else {
                        None
                    }
                })
                .expect("Requested field missing or result has no fields");
            let displacement = step.fields.iter().find(|f| {
                matches!(f,
                ResultField::NodeVector { name, .. } if name.eq_ignore_ascii_case("DISPLACEMENT"))
            });
            for scale in [0.0, 1.0, 5.0] {
                let settings = ContourSettings {
                    mesh_index: part,
                    step_index: frame,
                    field_name: field.name().to_owned(),
                    show_deformation: scale != 0.0 && displacement.is_some(),
                    displacement_field: displacement.map_or("", ResultField::name).to_owned(),
                    deformation_scale: scale,
                };
                let surface_ms = median_ms(|| {
                    demo_mesh::build_contour_surface_mesh(mesh, step, &settings, None)
                        .expect("No contour surface")
                });
                let edge_ms = median_ms(|| {
                    demo_mesh::build_contour_edge_mesh(mesh, step, &settings)
                        .expect("No contour edges")
                });
                let probe_ms = median_ms(|| ProbeSurface::build(mesh, step, &settings));
                // Validate geometry, not just elapsed time: every wire vertex
                // belongs to the rendered triangle surface for these solid fixtures.
                let surface =
                    demo_mesh::build_contour_surface_mesh(mesh, step, &settings, None).unwrap();
                let edges = demo_mesh::build_contour_edge_mesh(mesh, step, &settings).unwrap();
                let Some(bevy::mesh::VertexAttributeValues::Float32x3(points)) =
                    surface.attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    panic!("Missing surface positions")
                };
                let Some(bevy::mesh::VertexAttributeValues::Float32x3(edge_points)) =
                    edges.attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    panic!("Missing edge positions")
                };
                let positions: std::collections::HashSet<_> =
                    points.iter().map(|p| p.map(f32::to_bits)).collect();
                assert!(
                    edge_points
                        .iter()
                        .all(|p| positions.contains(&p.map(f32::to_bits)))
                );
                println!(
                    "FRAME {} field={:?} deformation={} scale={scale} surface_ms={surface_ms:.3} edges_ms={edge_ms:.3} probe_ms={probe_ms:.3} vertices={} edge_vertices={}",
                    frame + 1,
                    field.name(),
                    settings.show_deformation,
                    points.len(),
                    edge_points.len()
                );
            }
        }
        let (min, max) = mesh.bounds().expect("Empty mesh");
        let extent = max - min;
        let origins: Vec<_> = (0..256)
            .map(|i| {
                Vec3::new(
                    min.x + (i % 16) as f32 / 16.0 * extent.x + extent.x / 64.0,
                    min.y + (i / 16) as f32 / 16.0 * extent.y + extent.y / 64.0,
                    max.z + extent.max_element().max(1.0),
                )
            })
            .collect();
        let candidates_ms = median_ms(|| {
            origins
                .iter()
                .map(|origin| {
                    black_box(mesh.boundary_face_indices_along_ray(*origin, Vec3::NEG_Z)).len()
                })
                .sum::<usize>()
        });
        println!(
            "SELECTION part={} rays=256 boundary_candidates_batch_ms={candidates_ms:.3}",
            part + 1
        );
    }
}
