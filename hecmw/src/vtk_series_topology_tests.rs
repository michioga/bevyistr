use super::*;

#[test]
fn later_frames_still_reject_connectivity_and_piece_count_changes() {
    let f = Fixture::new();
    f.file("a.vtu", &piece(1.0));
    let path = f.file("series.0001.pvtu", &wrapper(&["a.vtu"], 0.0));
    f.file(
        "b.vtu",
        &piece(2.0).replace(">0 1 2 3</DataArray>", ">0 2 1 3</DataArray>"),
    );
    f.file("series.0002.pvtu", &wrapper(&["b.vtu"], 1.0));
    assert!(
        load_scene(&path)
            .err()
            .unwrap()
            .contains("geometry/topology")
    );

    f.file("b.vtu", &piece(2.0));
    f.file("c.vtu", &piece(3.0));
    f.file("series.0002.pvtu", &wrapper(&["b.vtu", "c.vtu"], 1.0));
    assert!(load_scene(&path).err().unwrap().contains("piece count"));
}

fn ghost_piece(value: f32, masks: &str) -> String {
    piece(value)
        .replace("NumberOfCells=\"1\"", "NumberOfCells=\"2\"")
        .replace(">0 1 2 3</DataArray>", ">0 1 2 3 0 1 2 3</DataArray>")
        .replace(">4</DataArray>", ">4 8</DataArray>")
        .replace(">10</DataArray>", ">10 10</DataArray>")
        .replace(&format!("Name=\"Damage\">{value}</DataArray>"),
            &format!("Name=\"Damage\">999 {value}</DataArray><DataArray Name=\"vtkGhostType\">{masks}</DataArray>"))
        .replace("</PointData>",
            "<DataArray Name=\"vtkGhostType\">1 0 0 0</DataArray></PointData>")
}

#[test]
fn every_frame_is_ghost_filtered_and_checked_before_reusing_first_topology() {
    let f = Fixture::new();
    f.file("a.vtu", &ghost_piece(2.0, "1 0"));
    let path = f.file("ghost.0001.pvtu", &wrapper(&["a.vtu"], 0.0));
    f.file("b.vtu", &ghost_piece(3.0, "32 0"));
    f.file("ghost.0002.pvtu", &wrapper(&["b.vtu"], 1.0));
    let scene = load_scene(&path).unwrap();
    let mesh = &scene.model.meshes[0];
    assert_eq!(mesh.nodes.len(), 4); // duplicate point retained for owned cell
    assert_eq!(mesh.elements.len(), 1);
    assert_eq!(mesh.elements[0].id, ElementId(1)); // original local cell ID
    assert_eq!(mesh.cached_boundary_faces().len(), 4);
    let (min, max) = mesh.bounds().unwrap();
    assert_eq!(
        mesh.boundary_face_indices_in_aabb(fem_core::Aabb { min, max })
            .len(),
        4
    );
    assert_eq!(
        scalar(&scene.steps[0][0], "Damage (element)"),
        (vec![2.0], 2.0, 2.0)
    );
    assert_eq!(
        scalar(&scene.steps[0][1], "Damage (element)"),
        (vec![3.0], 3.0, 3.0)
    );
    assert_eq!(scalar(&scene.steps[0][1], "PRESSURE").0, vec![3.0; 4]);
    assert!(scene.steps[0].iter().all(|step| {
        step.fields
            .iter()
            .all(|field| !field.name().contains("vtkGhostType"))
    }));

    // Same coordinates/counts, but a different owned local cell is not the
    // same topology and must not be associated with the first frame's IDs.
    f.file("b.vtu", &ghost_piece(3.0, "0 1"));
    assert!(
        load_scene(&path)
            .err()
            .unwrap()
            .contains("geometry/topology")
    );
    // Unsupported flags in later frames must not be bypassed by reuse.
    f.file("b.vtu", &ghost_piece(3.0, "8 0"));
    assert!(load_scene(&path).err().unwrap().contains("vtkGhostType"));
    f.file(
        "b.vtu",
        &ghost_piece(3.0, "32 0").replace(">1 0 0 0</DataArray>", ">2 0 0 0</DataArray>"),
    );
    assert!(
        load_scene(&path)
            .err()
            .unwrap()
            .contains("No visible VTK cells")
    );
}
