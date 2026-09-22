//! Opt-in, read-only checks against the maintainer's existing tutorial outputs.
//! CI uses small synthetic cases; never requires a machine-specific path.
use fem_core::{ResultField, StepResult};
use hecmw::{result_series::load_mesh_result_series, vtk_scene::load_scene};
use std::path::PathBuf;

fn root() -> PathBuf {
    std::env::var_os("BEVYISTR_TUTORIAL_DIR")
        .map(PathBuf::from)
        .expect("Set BEVYISTR_TUTORIAL_DIR to existing FrontISTR tutorial outputs")
}

fn close(a: f32, b: f32) -> bool {
    (a.is_nan() && b.is_nan()) || (a - b).abs() <= 2e-5 * a.abs().max(1.)
}

fn compare(a: &StepResult, b: &StepResult) {
    assert_eq!(a.step, b.step);
    assert!(close(a.time, b.time));
    for field in &a.fields {
        let other = b.field_by_name(field.name()).expect(field.name());
        match (field, other) {
            (
                ResultField::NodeScalar { values: a, .. },
                ResultField::NodeScalar { values: b, .. },
            ) => {
                assert_eq!(a.len(), b.len());
                for (i, (&a, &b)) in a.iter().zip(b).enumerate() {
                    assert!(close(a, b), "{} node index {i}: {a} / {b}", field.name());
                }
            }
            (
                ResultField::NodeVector { values: a, .. },
                ResultField::NodeVector { values: b, .. },
            ) => {
                assert_eq!(a.len(), b.len());
                for (i, (a, b)) in a.iter().zip(b).enumerate() {
                    assert!(
                        a.to_array()
                            .into_iter()
                            .zip(b.to_array())
                            .all(|(a, b)| close(a, b)),
                        "{} node index {i}: {a} / {b}",
                        field.name()
                    );
                }
            }
            _ => panic!("Unexpected field association: {}", field.name()),
        }
    }
}

#[test]
#[ignore = "Requires BEVYISTR_TUTORIAL_DIR; checks every eigen mode and heat value"]
fn tutorial_all_eigen_modes_and_heat_match_native_and_standalone_vtk() {
    for (dir, stem, frames, field) in [
        ("15_eigen_spring", "spring", 5, "Displacement"),
        ("16_heat_block", "block", 1, "TEMPERATURE"),
    ] {
        let folder = root().join(dir);
        let mesh = hecmw::load_mesh_file(folder.join(format!("{stem}.msh"))).unwrap();
        let native =
            load_mesh_result_series(&folder.join(format!("{stem}.res.0.1")), &mesh).unwrap();
        let vtk_path = folder.join(format!("{stem}_vis_psf.0001.pvtu"));
        let vtk = load_mesh_result_series(&vtk_path, &mesh).unwrap();
        let scene = load_scene(&vtk_path).unwrap();
        assert_eq!(native.len(), frames);
        assert_eq!(vtk.len(), frames);
        assert_eq!(scene.model.meshes.len(), 1);
        assert_eq!(scene.steps[0].len(), frames);
        for (i, (a, b)) in native.iter().zip(&vtk).enumerate() {
            assert_eq!(a.step, i as u32 + 1);
            assert_eq!(a.time, 0.); // No physical timeline may be invented from mode numbers.
            compare(a, b);
            assert!(scene.steps[0][i].field_by_name(field).is_some());
            let (min, max) = match a.field_by_name(field).unwrap() {
                ResultField::NodeScalar { min, max, .. } => (*min, *max),
                ResultField::NodeVector {
                    min_mag, max_mag, ..
                } => (*min_mag, *max_mag),
                _ => unreachable!(),
            };
            println!(
                "{dir}: frame={} step={} time={} {field} range=[{min:.7e},{max:.7e}]",
                i + 1,
                a.step,
                a.time
            );
        }
        if stem == "block" {
            assert!(scene.steps[0][0].field_by_name("Displacement").is_none());
            // Independent reference: first native record is original Node ID 2.
            let index = mesh.nodes.iter().position(|n| n.id.0 == 2).unwrap();
            let ResultField::NodeScalar { values, .. } =
                native[0].field_by_name("TEMPERATURE").unwrap()
            else {
                panic!()
            };
            assert!(close(values[index], 28.502_321));
        } else {
            let index = mesh.nodes.iter().position(|n| n.id.0 == 1).unwrap();
            let ResultField::NodeVector { values, .. } =
                native[0].field_by_name("Displacement").unwrap()
            else {
                panic!()
            };
            for (a, b) in
                values[index]
                    .to_array()
                    .into_iter()
                    .zip([0.918_969_7, 0.101_047_55, -0.662_340_5])
            {
                assert!(close(a, b));
            }
        }
        println!(
            "{dir}: {} points, {} cells, {frames} frames; fields={:?}",
            scene.model.meshes[0].nodes.len(),
            scene.model.meshes[0].elements.len(),
            scene.steps[0][0]
                .fields
                .iter()
                .map(ResultField::name)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
#[ignore = "Requires BEVYISTR_TUTORIAL_DIR; eight-piece constant/nonconstant flow fields"]
fn tutorial_flow_components_and_constant_fields() {
    let scene = load_scene(&root().join("18_cavity_flow/cavityflow_vis_psf.0000.pvtu")).unwrap();
    assert_eq!(scene.model.meshes.len(), 8);
    for (mesh, steps) in scene.model.meshes.iter().zip(&scene.steps) {
        assert_eq!(steps.len(), 1);
        let step = &steps[0];
        assert_eq!(step.step, 0);
        assert_eq!(step.time, 0.);
        assert_eq!(step.fields.len(), 13);
        assert!(step.field_by_name("Displacement").is_none());
        for name in ["VELOCITY[2]", "VELOCITY[3]", "PRESSURE"] {
            let ResultField::NodeScalar {
                values, min, max, ..
            } = step.field_by_name(name).unwrap()
            else {
                panic!()
            };
            assert_eq!(values.len(), mesh.nodes.len());
            assert!(values.iter().all(|v| *v == 0.));
            assert_eq!((*min, *max), (0., 0.));
        }
        let ResultField::NodeScalar { min, max, .. } = step.field_by_name("VELOCITY[1]").unwrap()
        else {
            panic!()
        };
        assert_eq!(*min, 0.);
        assert!(close(*max, 0.001));
        for field in &step.fields {
            match field {
                ResultField::NodeScalar { values, .. } => {
                    assert_eq!(values.len(), mesh.nodes.len())
                }
                ResultField::NodeVector { values, .. } => {
                    assert_eq!(values.len(), mesh.nodes.len())
                }
                ResultField::ElementScalar { values, .. } => {
                    assert_eq!(values.len(), mesh.elements.len())
                }
            }
        }
    }
    println!(
        "18_cavity_flow: 8 parts, 1 frame, 13 fields; VX range=[0,0.001], VY/VZ/P=0; no displacement"
    );
}
