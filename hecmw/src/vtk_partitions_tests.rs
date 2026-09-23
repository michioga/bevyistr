use super::*;
use std::path::PathBuf;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "bevyistr-pvtu-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn piece(value: f32) -> String {
    format!("<VTKFile><UnstructuredGrid><Piece NumberOfPoints=\"4\" NumberOfCells=\"1\">
        <Points><DataArray NumberOfComponents=\"3\">0 0 0 1 0 0 0 1 0 0 0 1</DataArray></Points>
        <Cells><DataArray Name=\"connectivity\">0 1 2 3</DataArray><DataArray Name=\"offsets\">4</DataArray><DataArray Name=\"types\">10</DataArray></Cells>
        <PointData><DataArray Name=\"PRESSURE\">{value} {value} {value} {value}</DataArray>
        <DataArray Name=\"VELOCITY\" NumberOfComponents=\"3\">0 {value} 0 0 {value} 0 0 {value} 0 0 {value} 0</DataArray></PointData>
        <CellData><DataArray Name=\"Damage\">{value}</DataArray></CellData></Piece></UnstructuredGrid></VTKFile>")
}
fn wrapper(files: &[&str], time: f32) -> String {
    format!(
        "<VTKFile><PUnstructuredGrid><FieldData><DataArray Name=\"TimeValue\">{time}</DataArray></FieldData>{}</PUnstructuredGrid></VTKFile>",
        files
            .iter()
            .map(|f| format!("<Piece Source=\"{f}\"/>"))
            .collect::<String>()
    )
}
fn scalar(step: &StepResult, name: &str) -> (Vec<f32>, f32, f32) {
    match step.field_by_name(name).unwrap() {
        ResultField::NodeScalar {
            values, min, max, ..
        }
        | ResultField::ElementScalar {
            values, min, max, ..
        } => (values.clone(), *min, *max),
        _ => panic!("not scalar"),
    }
}

#[test]
fn wrapper_eigenvalues_reach_all_pieces_and_conflicts_are_rejected() {
    let f = Fixture::new();
    f.file("a.vtu", &piece(1.));
    f.file("b.vtu", &piece(2.));
    let modal = wrapper(&["a.vtu","b.vtu"],0.).replace("Name=\"TimeValue\">0", "Name=\"EIGENVALUE\">7.8306921036862833e6");
    let path = f.file("mode.0003.pvtu", &modal);
    let scene = load_scene(&path).unwrap();
    for steps in &scene.steps {
        assert_eq!(steps[0].eigenvalue,Some(7.8306921036862833e6));
        assert_eq!(steps[0].step,3);
        assert_eq!(steps[0].time,0.);
    }
    let single = f.file("single.pvtu", &modal.replace("<Piece Source=\"b.vtu\"/>",""));
    let ids = [NodeId(0),NodeId(1),NodeId(2),NodeId(3)];
    assert_eq!(crate::load_vtu_file(&single,&ids).unwrap().eigenvalue,Some(7.8306921036862833e6));
    f.file("b.vtu", &piece(2.).replace("<UnstructuredGrid>", "<UnstructuredGrid><FieldData><DataArray Name=\"EIGENVALUE\">42</DataArray></FieldData>"));
    assert!(load_scene(&path).err().unwrap().contains("EIGENVALUE"));
    f.file("mode.0003.pvtu", &wrapper(&["a.vtu","b.vtu"],0.));
    assert!(load_scene(&path).is_err()); // Metadata in one piece only is not silently shared.
}

#[test]
fn partitions_preserve_local_values_and_share_ranges_across_sparse_frames() {
    let f = Fixture::new();
    f.file("job.0002/job.0002.0.vtu", &piece(2.0));
    f.file("job.0002/job.0002.1.vtu", &piece(10.0));
    let path = f.file(
        "job.0002.pvtu",
        &wrapper(
            &["job.0002/job.0002.0.vtu", "job.0002/job.0002.1.vtu"],
            0.25,
        ),
    );
    f.file("job.0010/job.0010.0.vtu", &piece(3.0));
    f.file("job.0010/job.0010.1.vtu", &piece(20.0));
    f.file(
        "job.0010.pvtu",
        &wrapper(&["job.0010/job.0010.0.vtu", "job.0010/job.0010.1.vtu"], 2.0),
    );
    let scene = load_scene(&path).unwrap();
    assert_eq!(scene.model.meshes.len(), 2);
    assert_eq!(
        scene
            .model
            .meshes
            .iter()
            .map(|m| m.nodes.len())
            .sum::<usize>(),
        8
    ); // no coordinate welding
    assert_eq!(
        scene.steps[0].iter().map(|s| s.step).collect::<Vec<_>>(),
        [2, 10]
    );
    assert_eq!(scene.steps[1][1].time, 2.0);
    assert_eq!(
        scalar(&scene.steps[0][0], "PRESSURE"),
        (vec![2.0; 4], 2.0, 10.0)
    );
    assert_eq!(
        scalar(&scene.steps[1][0], "Damage (element)"),
        (vec![10.0], 2.0, 10.0)
    );
    match scene.steps[0][1].field_by_name("VELOCITY").unwrap() {
        ResultField::NodeVector {
            values,
            min_mag,
            max_mag,
            ..
        } => {
            assert_eq!(values[0].y, 3.0);
            assert_eq!((*min_mag, *max_mag), (3.0, 20.0));
        }
        _ => panic!("missing vector"),
    }
    let from_rank = load_scene(&f.0.join("job.0002/job.0002.1.vtu")).unwrap();
    assert_eq!(from_rank.model.meshes.len(), 2);
    assert_eq!(from_rank.steps[0].len(), 2);
}

#[test]
fn broken_references_schema_time_and_topology_are_rejected() {
    let f = Fixture::new();
    f.file("a.vtu", &piece(1.0));
    let path = f.file("job.pvtu", &wrapper(&["a.vtu", "missing.vtu"], 0.0));
    assert!(load_scene(&path).err().unwrap().contains("missing.vtu"));
    f.file("job.pvtu", &wrapper(&["a.vtu", "./a.vtu"], 0.0));
    assert!(load_scene(&path).err().unwrap().contains("Duplicate"));
    f.file("b.vtu", &piece(2.0).replace("PRESSURE", "Other"));
    f.file("job.pvtu", &wrapper(&["a.vtu", "b.vtu"], 0.0));
    assert!(load_scene(&path).err().unwrap().contains("inconsistent"));
    f.file(
        "b.vtu",
        &piece(2.0).replace(
            "<UnstructuredGrid>",
            "<UnstructuredGrid><FieldData><DataArray Name=\"TimeValue\">9</DataArray></FieldData>",
        ),
    );
    assert!(load_scene(&path).err().unwrap().contains("TimeValue"));
    assert!(
        parse_scene_piece(&piece(2.0).replace("NumberOfPoints=\"4\"", "NumberOfPoints=\"5\""))
            .is_err()
    );
    let path = f.file("changing.0001.pvtu", &wrapper(&["a.vtu"], 0.0));
    f.file("b.vtu", &piece(2.0).replace("0 0 0 1 0 0", "0 0 0 2 0 0"));
    f.file("changing.0002.pvtu", &wrapper(&["b.vtu"], 0.0));
    assert!(
        load_scene(&path)
            .err()
            .unwrap()
            .contains("geometry/topology")
    );
}

#[test]
fn duplicate_field_names_and_nonfinite_times_are_rejected() {
    let a = parse_scene_piece(&piece(1.0)).unwrap();
    let mut b = parse_scene_piece(&piece(2.0)).unwrap();
    b.1.fields[1] = b.1.fields[0].clone();
    assert!(crate::vtk_partitions::validate_and_share_ranges(&mut [a, b]).is_err());
    let mut a = parse_scene_piece(&piece(1.0)).unwrap();
    a.1.time = f32::NAN;
    assert!(crate::vtk_partitions::validate_and_share_ranges(&mut [a]).is_err());
}

#[test]
fn ghost_cells_are_removed_without_averaging_and_metadata_is_not_a_field() {
    let source = piece(2.0)
        .replace("NumberOfCells=\"1\"", "NumberOfCells=\"2\"")
        .replace(">0 1 2 3</DataArray>", ">0 1 2 3 0 1 2 3</DataArray>")
        .replace(">4</DataArray>", ">4 8</DataArray>")
        .replace(">10</DataArray>", ">10 10</DataArray>")
        .replace(
            "Name=\"Damage\">2</DataArray>",
            "Name=\"Damage\">999 2</DataArray><DataArray Name=\"vtkGhostType\">1 0</DataArray>",
        )
        .replace(
            "</PointData>",
            "<DataArray Name=\"vtkGhostType\">1 0 0 0</DataArray></PointData>",
        );
    let (mesh, step) = parse_scene_piece(&source).unwrap();
    assert_eq!(mesh.nodes.len(), 4); // duplicate point remains for an owned cell
    assert_eq!(mesh.elements.len(), 1);
    assert_eq!(mesh.elements[0].id, ElementId(1)); // original local ID preserved
    let mut parts = vec![(mesh, step)];
    crate::vtk_partitions::validate_and_share_ranges(&mut parts).unwrap();
    assert_eq!(
        scalar(&parts[0].1, "Damage (element)"),
        (vec![2.0], 2.0, 2.0)
    );
    assert!(
        parts[0]
            .1
            .fields
            .iter()
            .all(|f| !f.name().contains("vtkGhostType"))
    );
    let (mesh, _) =
        parse_scene_piece(&source.replace(">1 0</DataArray>", ">32 0</DataArray>")).unwrap();
    assert_eq!(mesh.elements.len(), 1);
    let (mesh, _) =
        parse_scene_piece(&source.replace(">1 0 0 0</DataArray>", ">2 0 0 0</DataArray>")).unwrap();
    assert!(mesh.elements.is_empty());
    assert!(parse_scene_piece(&source.replace(">1 0</DataArray>", ">8 0</DataArray>")).is_err());
    assert!(parse_scene_piece(&source.replace(">1 0</DataArray>", ">0.5 0</DataArray>")).is_err());
}

#[test]
#[ignore = "Requires BEVYISTR_TUTORIAL_DIR; eight-piece FrontISTR flow output"]
fn tutorial_multipart_flow_scene() {
    let root = PathBuf::from(std::env::var_os("BEVYISTR_TUTORIAL_DIR").unwrap());
    let scene = load_scene(&root.join("18_cavity_flow/cavityflow_vis_psf.0000.pvtu")).unwrap();
    assert_eq!(scene.model.meshes.len(), 8);
    assert_eq!(
        scene
            .model
            .meshes
            .iter()
            .map(|m| m.nodes.len())
            .sum::<usize>(),
        35863
    );
    assert_eq!(
        scene
            .model
            .meshes
            .iter()
            .map(|m| m.elements.len())
            .sum::<usize>(),
        178142
    );
    for steps in &scene.steps {
        assert_eq!(steps.len(), 1);
        assert!(steps[0].field_by_name("VELOCITY").is_some());
        assert!(steps[0].field_by_name("PRESSURE").is_some());
        let (_, lo, hi) = scalar(&steps[0], "PRESSURE");
        let (_, first_lo, first_hi) = scalar(&scene.steps[0][0], "PRESSURE");
        assert_eq!((lo, hi), (first_lo, first_hi));
    }
}
