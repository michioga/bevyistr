//! Opt-in, read-only checks against the maintainer's existing tutorial outputs.
//! CI uses small synthetic cases; never requires a machine-specific path.
use fem_core::{ResultField, StepResult};
use hecmw::{
    result_series::{FrameSelection, load_mesh_result_frames, load_mesh_result_series},
    vtk_scene::{load_scene, load_scene_frames},
};
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
    assert!(a.same_eigenmode(b));
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
            assert!(a.same_eigenmode(&scene.steps[0][i]));
            if stem == "spring" {
                assert!(a.eigenvalue.is_some());
                println!(
                    "mode {}: RES eigenvalue={:.15e}, VTK eigenvalue={:.15e}",
                    a.step,
                    a.eigenvalue.unwrap(),
                    b.eigenvalue.unwrap()
                );
            } else {
                assert!(a.eigenvalue.is_none());
            }
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
            assert_eq!(native[0].eigenvalue, Some(7.8306921036862833e6));
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
#[ignore = "Requires BEVYISTR_TUTORIAL_DIR; checks the original cavity-flow MSH and CNT"]
fn tutorial_cavity_flow_material_and_time_integration() {
    use fem_core::{AnalysisSetup, FemModel, StepGroup};

    let folder = root().join("18_cavity_flow");
    let control_path = folder.join("hecmw_ctrl.dat");
    let control = hecmw::load_hecmw_ctrl(&control_path).unwrap();
    assert!(
        control.import_issues.is_empty(),
        "{:?}",
        control.import_issues
    );
    let (mesh_path, cnt_path) = hecmw::resolve_paths(&control_path, &control);
    let project = hecmw::load_mesh_project(mesh_path.unwrap()).unwrap();
    let partition = hecmw::partition_control::load(folder.join("hecmw_part_ctrl.dat")).unwrap();
    let mut setup = AnalysisSetup {
        frontistr_control: control.settings,
        frontistr_partition: Some(partition.clone()),
        materials: project.materials,
        sections: project.sections,
        import_issues: project.import_issues,
        ..Default::default()
    };
    assert!(setup.import_issues.is_empty(), "{:?}", setup.import_issues);
    let placeholder = setup.material_by_name("FLUID").unwrap().clone();
    assert!(placeholder.mesh_fluid_placeholder);
    assert_eq!(
        (
            placeholder.young_modulus,
            placeholder.poisson_ratio,
            placeholder.density,
            placeholder.dynamic_viscosity,
        ),
        (None, None, None, None)
    );
    let [section] = setup.sections.as_slice() else {
        panic!("Expected one fluid section");
    };
    assert_eq!(section.element_set_name.as_deref(), Some("ELEMENT_ALL"));
    assert_eq!(section.material_name, "FLUID");
    assert_eq!(section.kind, fem_core::SectionKind::Solid);
    let data = hecmw::load_cnt_file(cnt_path.unwrap(), &project.mesh, 0).unwrap();
    assert!(
        !data.import_issues.iter().any(|issue| {
            ["!FLUID", "!DYNAMIC", "!STEP", "!BOUNDARY"]
                .iter()
                .any(|keyword| issue.keyword.starts_with(keyword))
        }),
        "{:?}",
        data.import_issues
    );
    data.merge_into(&mut setup);
    let dynamic = setup.solver.dynamic.as_ref().unwrap();
    assert_eq!((dynamic.start_time, dynamic.end_time), (0., 1e5));
    assert_eq!((dynamic.fallback_substeps, dynamic.fallback_increment), (10000, 10.));
    assert_eq!((dynamic.gamma, dynamic.beta), (0.5, 0.25));
    assert_eq!(dynamic.monitor_node_or_group, "1");
    assert_eq!(dynamic.monitor_interval, 50);
    assert_eq!(dynamic.output_flags, [1, 0, 0, 0, 0, 0]);
    let step = setup.solver.dynamic_step.as_ref().unwrap();
    assert_eq!((step.increment, step.duration), (10., 1e5));
    assert_eq!(
        (step.substeps, step.max_iterations, step.convergence),
        (Some(10000), Some(100), Some(1e-20))
    );
    assert_eq!(
        step.groups,
        [StepGroup::Boundary(1), StepGroup::Boundary(2)]
    );
    let boundaries: Vec<_> = setup
        .boundary_conditions
        .iter()
        .map(|bc| (bc.name.as_str(), bc.group_id))
        .collect();
    assert_eq!(boundaries, [("N_VELO", 1), ("N_VELO", 1), ("N_FIX", 2)]);
    let fluid = setup.material_by_name("FLUID").unwrap();
    assert_eq!(fluid.dynamic_viscosity, Some(0.001));
    assert_eq!(fluid.density, Some(1000.0));
    assert_eq!((fluid.young_modulus, fluid.poisson_ratio), (None, None));
    assert!(fluid.mesh_fluid_placeholder);
    let model = FemModel::single_mesh("Cavity flow", project.mesh);
    let report = hecmw::validate_frontistr_project(&model, &setup);
    assert!(!report.has_errors(), "{}", report.summary(20));

    let mut invalid = setup.clone();
    invalid.solver.dynamic.as_mut().unwrap().fallback_increment *= 2.0;
    assert!(
        hecmw::validate_frontistr_project(&model, &invalid)
            .summary(20)
            .contains("STEP DTIME")
    );
    invalid = setup.clone();
    invalid.boundary_conditions.retain(|bc| bc.group_id != 2);
    assert!(
        hecmw::validate_frontistr_project(&model, &invalid)
            .summary(20)
            .contains("distinct BOUNDARY groups")
    );
    invalid = setup.clone();
    invalid.sections.clear();
    assert!(
        hecmw::validate_frontistr_project(&model, &invalid)
            .summary(20)
            .contains("Every fluid element needs a SOLID section")
    );

    let dir = tempfile::tempdir().unwrap();
    hecmw::write_parallel_frontistr_project(
        dir.path(),
        "roundtrip",
        &model,
        &setup,
        "new_rank_mesh",
        partition.domains.get(),
    )
    .unwrap();
    let control_path = dir.path().join("hecmw_ctrl.dat");
    let reloaded_ctrl = hecmw::load_hecmw_ctrl(&control_path).unwrap();
    assert!(
        reloaded_ctrl.import_issues.is_empty(),
        "{:?}",
        reloaded_ctrl.import_issues
    );
    assert_eq!(
        reloaded_ctrl.distributed_mesh_path.as_deref(),
        Some("new_rank_mesh")
    );
    assert_eq!(reloaded_ctrl.settings, setup.frontistr_control);
    assert_eq!(
        hecmw::partition_control::load(dir.path().join("hecmw_part_ctrl.dat")).unwrap(),
        partition
    );
    let (mesh_path, cnt_path) = hecmw::resolve_paths(&control_path, &reloaded_ctrl);
    let reloaded = hecmw::load_mesh_project(mesh_path.unwrap()).unwrap();
    assert!(
        reloaded.import_issues.is_empty(),
        "{:?}",
        reloaded.import_issues
    );
    assert_eq!(reloaded.materials, [placeholder]);
    assert_eq!(reloaded.sections, setup.sections);
    assert_eq!(reloaded.mesh.elements, model.meshes[0].elements);
    assert_eq!(reloaded.mesh.node_sets, model.meshes[0].node_sets);
    let cnt = hecmw::load_cnt_file(cnt_path.unwrap(), &reloaded.mesh, 0).unwrap();
    assert!(cnt.import_issues.is_empty(), "{:?}", cnt.import_issues);
    let mut roundtrip = AnalysisSetup {
        materials: reloaded.materials,
        sections: reloaded.sections,
        ..Default::default()
    };
    cnt.merge_into(&mut roundtrip);
    assert_eq!(roundtrip.solver, setup.solver);
    assert_eq!(roundtrip.materials, setup.materials);
    for (before, after) in setup
        .boundary_conditions
        .iter()
        .zip(&roundtrip.boundary_conditions)
    {
        assert_eq!(
            (
                &before.name,
                before.group_id,
                before.dof_start,
                before.dof_end,
                before.value
            ),
            (
                &after.name,
                after.group_id,
                after.dof_start,
                after.dof_end,
                after.value
            )
        );
        assert_eq!(before.nodes, after.nodes);
    }
    assert_eq!(
        setup.boundary_conditions.len(),
        roundtrip.boundary_conditions.len()
    );
    for target in [fem_core::OutputTarget::Res, fem_core::OutputTarget::Vis] {
        assert_eq!(setup.output.write(target), roundtrip.output.write(target));
    }
}

#[test]
#[ignore = "Requires BEVYISTR_TUTORIAL_DIR; complete eight-rank cavity-flow RES and PVTU output"]
fn tutorial_flow_components_and_constant_fields() {
    let folder = root().join("18_cavity_flow");
    let initial_pvtu = folder.join("cavityflow_vis_psf.0000.pvtu");
    let start = std::time::Instant::now();
    let preview = load_scene_frames(&initial_pvtu, FrameSelection::Selected, |_, _| {}).unwrap();
    assert_eq!(preview.model.meshes.len(), 8);
    assert!(preview.steps.iter().all(|frames| frames.len() == 1 && frames[0].step == 0));
    println!("Selected PVTU frame: {:?}", start.elapsed());

    let scene = load_scene(&initial_pvtu).unwrap();
    assert_eq!(scene.model.meshes.len(), 8);
    assert_eq!(scene.steps.len(), 8);
    let timeline: Vec<_> = (0..=100)
        .map(|frame| (frame * 100, (frame * 1000) as f32))
        .collect();
    let field_names = ["VELOCITY[1]", "VELOCITY[2]", "VELOCITY[3]", "PRESSURE"];
    let mut first_local_values = std::collections::HashMap::new();
    let mut different_value_for_shared_id = None;
    for (part, (mesh, steps)) in scene.model.meshes.iter().zip(&scene.steps).enumerate() {
        assert_eq!(steps.len(), timeline.len());
        for (step, &(number, time)) in steps.iter().zip(&timeline) {
            assert_eq!((step.step, step.time), (number, time));
            assert!(step.field_by_name("Displacement").is_none());
            assert!(matches!(
                step.field_by_name("VELOCITY"),
                Some(ResultField::NodeVector { .. })
            ));
            assert!(
                field_names
                    .iter()
                    .all(|name| step.field_by_name(name).is_some())
            );
        }
        let step = &steps[0];
        for field in field_names {
            assert_eq!(scalar_range(&preview.steps[part][0], field), scalar_range(step, field));
        }
        assert_eq!(step.step, 0);
        assert_eq!(step.time, 0.);
        assert!(step.eigenvalue.is_none());
        assert_eq!(step.frame_description(), "Step 0 | Time 0.000000e0");
        assert_eq!(step.fields.len(), 13);
        assert!(step.field_by_name("Displacement").is_none());
        for name in ["VELOCITY[2]", "VELOCITY[3]", "PRESSURE"] {
            assert_eq!(step.field_by_name(name).unwrap().constant_value(), Some(0.));
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
        let ResultField::NodeScalar {
            values: vx,
            min,
            max,
            ..
        } = step.field_by_name("VELOCITY[1]").unwrap()
        else {
            panic!()
        };
        assert_eq!(*min, 0.);
        assert_eq!(*max, 0.001_f32);
        assert_eq!(
            step.field_by_name("VELOCITY[1]").unwrap().constant_value(),
            None
        );
        let ResultField::NodeVector {
            values: velocity,
            min_mag,
            max_mag,
            ..
        } = step.field_by_name("VELOCITY").unwrap()
        else {
            panic!("VELOCITY must remain a vector alongside its scalar components");
        };
        assert_eq!((*min_mag, *max_mag), (0., 0.001_f32));
        for (index, ((node, vector), x)) in mesh.nodes.iter().zip(velocity).zip(vx).enumerate() {
            assert_eq!(node.id.0, index as u32); // Local VTK point index, not a global mesh ID.
            assert_eq!(vector.to_array(), [*x, 0., 0.]);
            assert!((vector.length() - x.abs()).abs() <= 1e-9);
            if let Some(&(other_part, other_value)) = first_local_values.get(&node.id.0) {
                if *x != other_value && different_value_for_shared_id.is_none() {
                    different_value_for_shared_id =
                        Some((node.id.0, other_part, other_value, part + 1, *x));
                }
            } else {
                first_local_values.insert(node.id.0, (part + 1, *x));
            }
        }
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
    let (id, a, av, b, bv) = different_value_for_shared_id
        .expect("Fixture must exercise equal local IDs with distinct part values");
    println!(
        "Local node {id}: Part {a} VX={av}, Part {b} VX={bv}; values must not be merged by node ID"
    );
    let final_ranges = field_names.map(|name| {
        scene
            .steps
            .iter()
            .map(|steps| scalar_range(steps.last().unwrap(), name))
            .fold(
                (f32::INFINITY, f32::NEG_INFINITY),
                |(lowest, highest), (min, max)| (lowest.min(min), highest.max(max)),
            )
    });
    drop(scene);
    drop(preview);

    let mesh = hecmw::load_mesh_file(folder.join("cavityflow.msh")).unwrap();
    assert!(
        mesh.elements
            .iter()
            .all(|element| element.element_type == fem_core::ElementType::FluidTet4)
    );
    assert!(!mesh.cached_boundary_faces().is_empty());
    let native_path = folder.join("cavityflow.res.0.10000");
    let start = std::time::Instant::now();
    let selected = load_mesh_result_frames(&native_path, &mesh, FrameSelection::Selected, |_, _| {})
        .unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!((selected[0].step, selected[0].time), (10000, 100000.0));
    println!("Selected MPI RES frame: {:?}", start.elapsed());

    let start = std::time::Instant::now();
    let native = load_mesh_result_series(&native_path, &mesh).unwrap();
    println!("Complete MPI RES series: {:?}", start.elapsed());
    assert_eq!(native.len(), timeline.len());
    for (step, &(number, time)) in native.iter().zip(&timeline) {
        assert_eq!((step.step, step.time), (number, time));
        assert!(matches!(
            step.field_by_name("VELOCITY"),
            Some(ResultField::NodeVector { .. })
        ));
        assert!(
            field_names
                .iter()
                .all(|name| step.field_by_name(name).is_some())
        );
    }
    let last = native.last().unwrap();
    let (min_pressure, max_pressure) = scalar_range(last, "PRESSURE");
    assert!(min_pressure < 0.0 && max_pressure > 0.0);
    for (field, (vtk_min, vtk_max)) in field_names.into_iter().zip(final_ranges) {
        let (res_min, res_max) = scalar_range(last, field);
        assert_eq!(scalar_range(&selected[0], field), (res_min, res_max));
        for (vtk, res) in [(vtk_min, res_min), (vtk_max, res_max)] {
            // VTK rounds the ASCII data relative to RES. Compare the two
            // assembled ranges without treating piece-local IDs as global IDs.
            assert!(
                (vtk - res).abs() <= 2e-6 * vtk.abs().max(res.abs()).max(1e-4),
                "{field}: PVTU {vtk:e}, RES {res:e}"
            );
        }
    }
    println!(
        "18_cavity_flow: 8 pieces, {} frames; final PRESSURE=[{min_pressure:.5e},{max_pressure:.5e}] at time {}",
        native.len(),
        last.time
    );
}

fn scalar_range(step: &StepResult, name: &str) -> (f32, f32) {
    let Some(ResultField::NodeScalar { min, max, .. }) = step.field_by_name(name) else {
        panic!("Expected nodal scalar {name}");
    };
    (*min, *max)
}
