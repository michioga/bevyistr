use super::*;
use fem_core::{ElementId, NodeId, ResultField, StepResult};

fn scene() -> ResultScene {
    ResultScene {
        model: FemModel::demo_hex8(),
        steps: vec![vec![StepResult {
            fields: vec![ResultField::NodeScalar {
                name: "TEMP".into(),
                values: vec![300.0; 8],
                min: 300.0,
                max: 300.0,
            }],
            ..Default::default()
        }]],
    }
}

#[test]
fn task_distinguishes_waiting_ready_error_and_reader_disconnect() {
    let (tx, rx) = mpsc::channel();
    let task = ResultLoadTask::from_receiver(rx);
    assert!(task.poll().is_none());
    tx.send(Ok(scene())).unwrap();
    assert_eq!(task.poll().unwrap().unwrap().steps.len(), 1);
    drop(tx);
    assert!(
        task.poll()
            .unwrap()
            .err()
            .unwrap()
            .contains("stopped unexpectedly")
    );

    let (tx, rx) = mpsc::channel();
    let task = ResultLoadTask::from_receiver(rx);
    tx.send(Err("invalid result".into())).unwrap();
    assert_eq!(task.poll().unwrap().err().unwrap(), "invalid result");
}

#[test]
fn invalid_payload_is_rejected_before_replacing_any_display_state() {
    let mut empty = scene();
    empty.steps[0].clear();
    let mut missing_piece = scene();
    missing_piece.model.add_mesh("second", FemMesh::demo_hex8());
    let mut inconsistent_frames = scene();
    inconsistent_frames
        .model
        .add_mesh("second", FemMesh::demo_hex8());
    inconsistent_frames.steps.push(vec![]);
    let mut no_fields = scene();
    no_fields.steps[0][0].fields.clear();
    for (value, message) in [
        (empty, "No result steps"),
        (missing_piece, "piece count"),
        (inconsistent_frames, "frame counts"),
        (no_fields, "No display fields"),
    ] {
        let (tx, rx) = mpsc::channel();
        tx.send(Ok(value)).unwrap();
        let task = ResultLoadTask::from_receiver(rx);
        assert!(task.poll().unwrap().err().unwrap().contains(message));
    }
}

#[test]
fn discarding_a_task_does_not_deliver_its_late_result() {
    let (tx, rx) = mpsc::channel();
    drop(ResultLoadTask::from_receiver(rx));
    assert!(tx.send(Ok(scene())).is_err());
}

fn native(mesh: &FemMesh, step: u32) -> String {
    let mut text = format!(
        "*fstrresult 2.0\n*comment\nfixture\n*global\n1\n1\nTOTALTIME\n{step}\n*data\n{} 0\n1 0\n1\nTEMP\n",
        mesh.nodes.len()
    );
    for node in mesh.nodes.iter().rev() {
        text.push_str(&format!("{}\n{}\n", node.id.0, node.id.0 + step));
    }
    text
}

#[test]
fn native_file_and_current_mesh_inputs_preserve_sparse_steps_and_id_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut mesh = FemMesh::demo_hex8();
    for (i, node) in mesh.nodes.iter_mut().enumerate() {
        node.id = NodeId(i as u32 + 11);
    }
    for element in &mut mesh.elements {
        element.id = ElementId(99);
        for node in &mut element.nodes {
            node.0 += 11;
        }
    }
    let model = FemModel::single_mesh("original", mesh);
    let mesh = &model.meshes[0];
    let mesh_path = dir.path().join("job.msh");
    hecmw::write_msh_file(&mesh_path, &model, 0).unwrap();
    let path = dir.path().join("job.res.0.2");
    std::fs::write(&path, native(mesh, 2)).unwrap();
    std::fs::write(dir.path().join("job.res.0.10"), native(mesh, 10)).unwrap();
    for input in [MeshInput::File(mesh_path), MeshInput::Current(mesh.clone())] {
        let loaded = ResultInput::WithMesh {
            path: path.clone(),
            mesh: input,
        }
        .load()
        .unwrap();
        assert_eq!(loaded.model.meshes[0].nodes, mesh.nodes);
        assert_eq!(
            loaded.steps[0].iter().map(|s| s.step).collect::<Vec<_>>(),
            [2, 10]
        );
        assert_eq!(loaded.steps[0][1].time, 10.0);
        let ResultField::NodeScalar { values, .. } =
            loaded.steps[0][0].field_by_name("TEMP").unwrap()
        else {
            panic!("missing temperature")
        };
        assert_eq!(*values, (13..21).map(|n| n as f32).collect::<Vec<_>>());
    }
    // A bad later frame must fail the whole request, not return a partial series.
    std::fs::write(dir.path().join("job.res.0.10"), "truncated").unwrap();
    assert!(
        ResultInput::WithMesh {
            path: path.clone(),
            mesh: MeshInput::Current(mesh.clone())
        }
        .load()
        .is_err()
    );
    assert!(
        ResultInput::WithMesh {
            path,
            mesh: MeshInput::File(dir.path().join("missing.msh"))
        }
        .load()
        .is_err()
    );
}

#[test]
fn vtk_worker_loads_geometry_without_a_pre_model_and_reports_bad_files() {
    let dir = tempfile::tempdir().unwrap();
    let source = "<VTKFile><UnstructuredGrid><Piece NumberOfPoints=\"4\" NumberOfCells=\"1\"><Points><DataArray NumberOfComponents=\"3\">0 0 0 1 0 0 0 1 0 0 0 1</DataArray></Points><Cells><DataArray Name=\"connectivity\">0 1 2 3</DataArray><DataArray Name=\"offsets\">4</DataArray><DataArray Name=\"types\">10</DataArray></Cells><PointData><DataArray Name=\"TEMP\">10 20 30 40</DataArray></PointData></Piece></UnstructuredGrid></VTKFile>";
    let path = dir.path().join("scene.0002.vtu");
    std::fs::write(&path, source).unwrap();
    std::fs::write(dir.path().join("scene.0010.vtu"), source).unwrap();
    let task = ResultLoadTask::spawn(ResultInput::Vtk(path.clone())).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let loaded = loop {
        if let Some(loaded) = task.poll() {
            break loaded.unwrap();
        }
        assert!(std::time::Instant::now() < deadline, "reader timed out");
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    assert_eq!(loaded.model.meshes[0].nodes.len(), 4);
    assert_eq!(loaded.model.meshes[0].cached_boundary_faces().len(), 4);
    assert_eq!(
        loaded.steps[0].iter().map(|s| s.step).collect::<Vec<_>>(),
        [2, 10]
    );
    std::fs::write(&path, "broken VTU").unwrap();
    assert!(ResultInput::Vtk(path).load().is_err());
}
