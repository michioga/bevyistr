use super::{RunResultSource, snapshot};
use fem_core::ResultField;
use std::{hint::black_box, path::PathBuf, time::Instant};

#[test]
#[ignore = "Requires BEVYISTR_TUTORIAL_DIR and saved four-rank hinge output"]
fn real_mpi_res_baseline() {
    let directory = PathBuf::from(
        std::env::var_os("BEVYISTR_TUTORIAL_DIR").expect("Set BEVYISTR_TUTORIAL_DIR"),
    )
    .join("02_elastic_hinge_parallel");
    let mesh = hecmw::load_mesh_file(directory.join("hinge.msh")).unwrap();
    let nodes = [mesh.nodes.iter().map(|node| node.id).collect()];
    let elements = [mesh.elements.iter().map(|element| element.id).collect()];
    let completed = snapshot(&directory, "hinge.res", None).unwrap();
    assert_eq!(completed.len(), 8, "expected two RES frames of four ranks");
    let bytes: u64 = completed.values().map(|(_, stamp)| stamp.bytes).sum();
    // Simulate the result loader receiving these existing files from one run.
    let source = RunResultSource {
        id: 0,
        model_version: 0,
        directory,
        result_header: "hinge.res".into(),
        ranks: 4,
        subdirectory_limit: None,
        before: Default::default(),
        completed: Some(completed),
        partition_prefix: Some("hinge_4".into()),
    };

    let load = || source.load_with_elements(&nodes, &elements).unwrap();
    let first_start = Instant::now();
    let result = load();
    let first_ms = first_start.elapsed().as_secs_f64() * 1000.0;
    let [frames] = result.as_slice() else {
        panic!("expected a single assembled model");
    };
    assert_eq!(
        frames.iter().map(|frame| frame.step).collect::<Vec<_>>(),
        [0, 1]
    );
    let last = &frames[1];
    assert_eq!(last.time, 1.0);
    let Some(ResultField::NodeVector {
        values, max_mag, ..
    }) = last.field_by_name("Displacement")
    else {
        panic!("missing displacement in final frame");
    };
    assert_eq!(values.len(), mesh.nodes.len());
    assert!(values.iter().all(|value| value.is_finite()));
    assert!(max_mag.is_finite() && *max_mag > 0.0);
    let max_displacement = *max_mag;
    drop(result);

    let mut repeat_ms = [0.0; 5];
    for elapsed in &mut repeat_ms {
        let start = Instant::now();
        let result = load();
        *elapsed = start.elapsed().as_secs_f64() * 1000.0;
        black_box(result);
    }
    repeat_ms.sort_by(f64::total_cmp);
    println!(
        "MPI RES ranks={} frames=2 nodes={} elements={} res_bytes={bytes} first_ms={first_ms:.3} repeat_median_ms={:.3} max_displacement={max_displacement:.8e}",
        source.ranks,
        mesh.nodes.len(),
        mesh.elements.len(),
        repeat_ms[2],
    );
}
