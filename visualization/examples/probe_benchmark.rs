//! CPU-only baseline; no window, renderer, solver or external data required.
use bevy::prelude::*;
use fem_core::{ElementId, FemElement, FemMesh, FemNode, NodeId, ResultField, StepResult};
use std::{hint::black_box, time::Instant};
use visualization::{ContourSettings, result_probe::ProbeSurface};

fn grid(n: usize) -> FemMesh {
    let width = n + 1;
    let id = |x: usize, y: usize, z: usize| NodeId((x + width * (y + width * z)) as u32);
    let mut nodes = Vec::with_capacity(width.pow(3));
    let mut elements = Vec::with_capacity(n.pow(3));
    for z in 0..=n {
        for y in 0..=n {
            for x in 0..=n {
                nodes.push(FemNode::new(
                    id(x, y, z),
                    Vec3::new(x as f32, y as f32, z as f32),
                ));
            }
        }
    }
    for z in 0..n {
        for y in 0..n {
            for x in 0..n {
                elements.push(FemElement::new_hex8(
                    ElementId(elements.len() as u32),
                    [
                        id(x, y, z),
                        id(x + 1, y, z),
                        id(x + 1, y + 1, z),
                        id(x, y + 1, z),
                        id(x, y, z + 1),
                        id(x + 1, y, z + 1),
                        id(x + 1, y + 1, z + 1),
                        id(x, y + 1, z + 1),
                    ],
                ));
            }
        }
    }
    FemMesh::new(nodes, elements)
}

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let sizes = if args.is_empty() {
        vec![8, 16, 32]
    } else {
        args.iter()
            .map(|s| s.parse::<usize>().map_err(|_| format!("Invalid size: {s}")))
            .collect::<Result<Vec<_>, _>>()?
    };
    if sizes.iter().any(|n| !(1..=128).contains(n)) {
        return Err("Grid sizes must be between 1 and 128 (memory grows cubically).".into());
    }
    const BUILDS: usize = 5;
    const RAYS: usize = 4096;
    println!(
        "# CPU-only synthetic Hex8 grid; 1 untimed warmup, {BUILDS} builds, {RAYS} cached picks"
    );
    println!(
        "# Mesh preparation includes topology; BVH build excludes mesh preparation and result allocation"
    );
    println!(
        "grid,nodes,elements,boundary_faces,mesh_ms,build_median_ms,build_min_ms,build_max_ms,pick_mean_us,hits"
    );
    for n in sizes {
        let start = Instant::now();
        let mesh = grid(n);
        let mesh_ms = start.elapsed().as_secs_f64() * 1000.0;
        let step = StepResult {
            fields: vec![ResultField::NodeVector {
                name: "U".into(),
                values: vec![Vec3::X * 0.25; mesh.nodes.len()],
                min_mag: 0.25,
                max_mag: 0.25,
            }],
            ..default()
        };
        let contour = ContourSettings {
            mesh_index: 0,
            step_index: 0,
            field_name: "U".into(),
            show_deformation: true,
            displacement_field: "U".into(),
            deformation_scale: 2.0,
        };
        // Warm up allocator/code paths; time repeated construction separately
        // from cached ray queries. Dropping each surface is outside build time.
        black_box(ProbeSurface::build(&mesh, &step, &contour));
        let mut timings = Vec::new();
        for _ in 0..BUILDS {
            let start = Instant::now();
            let surface = ProbeSurface::build(&mesh, &step, &contour);
            timings.push(start.elapsed().as_secs_f64() * 1000.0);
            black_box(surface);
        }
        timings.sort_by(f64::total_cmp);
        let surface = ProbeSurface::build(&mesh, &step, &contour);
        let field = &step.fields[0];
        // Deterministic rays inside the deformed top face, avoiding cell edges.
        let origins: Vec<_> = (0..RAYS)
            .map(|i| {
                Vec3::new(
                    0.5 + (i % 64) as f32 / 64.0 * n as f32 + n as f32 / 256.0,
                    (i / 64) as f32 / 64.0 * n as f32 + n as f32 / 256.0,
                    n as f32 + 10.0,
                )
            })
            .collect();
        let start = Instant::now();
        let mut hits = 0;
        for origin in origins {
            if let Some(hit) = black_box(surface.pick(origin, Vec3::NEG_Z, field)) {
                assert_eq!(hit.value, Some(0.25));
                assert!(hit.node.is_some());
                assert!((hit.distance - 10.0).abs() < 1e-4);
                hits += 1;
            }
        }
        let pick_us = start.elapsed().as_secs_f64() * 1_000_000.0 / RAYS as f64;
        assert_eq!(hits, RAYS, "every ray must hit the deformed boundary");
        println!(
            "{n},{},{},{},{mesh_ms:.3},{:.3},{:.3},{:.3},{pick_us:.3},{hits}",
            mesh.nodes.len(),
            mesh.elements.len(),
            mesh.cached_boundary_faces().len(),
            timings[BUILDS / 2],
            timings[0],
            timings[BUILDS - 1]
        );
    }
    Ok(())
}
