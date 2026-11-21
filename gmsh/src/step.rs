use crate::{GmshError, gmsh_command, load_msh_file};
use fem_core::{ElementType, FemMesh};
use std::{fs, io, path::Path};

/// Meshes one solid from a STEP file with first-order tetrahedra.
/// Refines the mesh around curved CAD features and grades it into the interior.
/// Coordinates and the optional upper bound on the target mesh size are in millimetres.
pub fn mesh_step(input: &Path, max_size_mm: Option<f32>) -> Result<FemMesh, GmshError> {
    if max_size_mm.is_some_and(|size| !size.is_finite() || size <= 0.0) {
        return Err(GmshError::StepMeshing(
            "Maximum element size must be finite and positive".into(),
        ));
    }
    let directory = tempfile::tempdir().map_err(GmshError::Io)?;
    // Keep the import script independent of user filenames and invoke Gmsh
    // only on this job's files, including when an older mesh already exists.
    fs::copy(input, directory.path().join("input.step")).map_err(GmshError::Io)?;
    let mut script = String::from(
        r#"SetFactory("OpenCASCADE");
Geometry.OCCTargetUnit = "MM";
imported() = ShapeFromFile("input.step");
volumes() = Volume{:};
If (#volumes() != 1)
  Error("STEP file must contain exactly one solid volume");
  Exit;
EndIf
Physical Volume("STEP_SOLID") = {volumes()};
boundary() = Boundary{ Volume{volumes()}; };
faces() = Unique(Abs(boundary()));
For index In {0:#faces()-1}
  face = faces(index);
  Physical Surface(Sprintf("STEP_FACE_%g", face)) = {face};
EndFor
Mesh.ElementOrder = 1;
Mesh.Binary = 0;
// Target 20 elements per full turn of a curved feature. Extend the resulting
// sizes from the boundary to grade the mesh toward the interior.
Mesh.MeshSizeFromCurvature = 20;
Mesh.MeshSizeExtendFromBoundary = 1;
"#,
    );
    if let Some(size) = max_size_mm {
        script.push_str(&format!("Mesh.MeshSizeMax = {size};\n"));
    }
    fs::write(directory.path().join("mesh.geo"), script).map_err(GmshError::Io)?;
    let output = gmsh_command()
        .current_dir(directory.path())
        .args([
            "mesh.geo", "-3", "-nopopup", "-format", "msh41", "-o", "mesh.msh",
        ])
        .output()
        .map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                GmshError::StepMeshing("Gmsh is required for STEP files. Add gmsh to PATH.".into())
            } else {
                GmshError::Io(error)
            }
        })?;
    let diagnostics = String::from_utf8_lossy(&output.stderr);
    let errors: Vec<_> = diagnostics
        .lines()
        .filter_map(|line| {
            let (level, message) = line.split_once(':')?;
            (level.trim() == "Error").then_some(message.trim())
        })
        .filter(|message| !message.is_empty())
        .take(3)
        .collect();
    let mesh_path = directory.path().join("mesh.msh");
    if !errors.is_empty() {
        return Err(GmshError::StepMeshing(format!(
            "Gmsh: {}",
            errors.join(" / ")
        )));
    }
    if !output.status.success() || !mesh_path.is_file() {
        return Err(GmshError::StepMeshing(format!(
            "Gmsh did not produce a STEP volume mesh ({})",
            output.status
        )));
    }

    let mesh = load_msh_file(&mesh_path)?;
    if mesh.elements.is_empty()
        || mesh
            .elements
            .iter()
            .any(|element| element.element_type != ElementType::Tet4)
    {
        return Err(GmshError::StepMeshing(
            "STEP meshing did not produce a first-order tetrahedral volume".into(),
        ));
    }
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_sizes_before_running_gmsh() {
        for size in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let result = mesh_step(Path::new("missing.step"), Some(size));
            assert!(matches!(result, Err(GmshError::StepMeshing(_))));
        }
    }

    fn write_step_fixture(directory: &Path, geometry: &str) -> std::path::PathBuf {
        fs::write(
            directory.join("fixture.geo"),
            format!("SetFactory(\"OpenCASCADE\");\n{geometry}\nSave \"fixture.step\";\n"),
        )
        .unwrap();
        let output = gmsh_command()
            .current_dir(directory)
            .args(["fixture.geo", "-parse_and_exit", "-nopopup"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        directory.join("fixture.step")
    }

    #[test]
    #[ignore = "requires Gmsh on PATH"]
    fn step_mesh_refines_a_small_hole_with_or_without_a_size_cap() {
        use bevy::prelude::Vec3;
        let directory = tempfile::tempdir().unwrap();
        let input = write_step_fixture(
            directory.path(),
            r#"Box(1) = {0, 0, 0, 80, 40, 10};
Cylinder(2) = {18, 20, -1, 0, 0, 12, 2};
BooleanDifference(3) = { Volume{1}; Delete; }{ Volume{2}; Delete; };"#,
        );

        let element_size_by_region = |mesh: &FemMesh| {
            let mut nearby = Vec::new();
            let mut faraway = Vec::new();
            for element in &mesh.elements {
                let [a, b, c, d] = <[_; 4]>::try_from(element.nodes.as_slice())
                    .unwrap()
                    .map(|id| mesh.node_position(id).unwrap());
                let center: Vec3 = (a + b + c + d) / 4.0;
                let longest_edge = a
                    .distance(b)
                    .max(a.distance(c))
                    .max(a.distance(d))
                    .max(b.distance(c))
                    .max(b.distance(d))
                    .max(c.distance(d));
                if (center.x - 18.0).hypot(center.y - 20.0) < 4.0 {
                    nearby.push(longest_edge);
                }
                if center.x > 55.0 {
                    faraway.push(longest_edge);
                }
            }
            let mean = |lengths: Vec<f32>| {
                assert!(!lengths.is_empty());
                lengths.iter().sum::<f32>() / lengths.len() as f32
            };
            (mean(nearby), mean(faraway))
        };
        let auto = mesh_step(&input, None).unwrap();
        let rim_nodes = auto
            .nodes
            .iter()
            .filter(|node| {
                let point = node.position;
                point.z.abs() < 1.0e-4
                    && ((point.x - 18.0).hypot(point.y - 20.0) - 2.0).abs() < 1.0e-4
            })
            .count();
        assert!(
            rim_nodes >= 16,
            "only {rim_nodes} nodes around the hole rim"
        );
        let limited = mesh_step(&input, Some(4.0)).unwrap();
        let (near_auto, far_auto) = element_size_by_region(&auto);
        let (near_limited, far_limited) = element_size_by_region(&limited);
        eprintln!(
            "Mean longest edge in mm: auto {near_auto:.2} near / {far_auto:.2} far, capped {near_limited:.2} near / {far_limited:.2} far"
        );
        assert!(far_auto > 2.0 * near_auto);
        assert!(far_limited > 1.5 * near_limited);
        assert!(far_limited < far_auto);
    }

    #[test]
    #[ignore = "requires Gmsh on PATH"]
    fn meshes_a_step_box_and_bracket_and_refuses_multiple_solids() {
        use bevy::prelude::Vec3;
        use std::collections::BTreeSet;
        for (name, geometry, size) in [
            (
                "box",
                "Box(1) = {0, 0, 0, 20, 10, 6};",
                Some(Vec3::new(20.0, 10.0, 6.0)),
            ),
            (
                "bracket",
                "Box(1) = {0, 0, 0, 30, 20, 5}; Box(2) = {0, 0, 0, 6, 20, 22}; BooleanUnion(3) = { Volume{1}; Delete; }{ Volume{2}; Delete; }; Cylinder(4) = {18, 10, -1, 0, 0, 7, 3}; BooleanDifference(5) = { Volume{3}; Delete; }{ Volume{4}; Delete; };",
                Some(Vec3::new(30.0, 20.0, 22.0)),
            ),
            (
                "two boxes",
                "Box(1) = {0, 0, 0, 5, 5, 5}; Box(2) = {10, 0, 0, 5, 5, 5};",
                None,
            ),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let input = write_step_fixture(directory.path(), geometry);
            let existing_mesh = input.with_extension("msh");
            fs::write(&existing_mesh, "existing mesh").unwrap();
            let result = mesh_step(&input, Some(2.5));
            if let Some(dimensions) = size {
                let mesh = result.unwrap_or_else(|error| panic!("{name}: {error}"));
                let (min, max) = mesh.bounds().unwrap();
                assert!((max - min).abs_diff_eq(dimensions, 1.0e-3), "{name}");
                let boundary = mesh.cached_boundary_faces();
                assert!(!boundary.is_empty(), "{name}");
                let solid = mesh
                    .element_sets
                    .iter()
                    .find(|group| group.name == "STEP_SOLID")
                    .unwrap();
                assert_eq!(solid.elements.len(), mesh.elements.len(), "{name}");
                let grouped_faces = mesh
                    .surface_sets
                    .iter()
                    .flat_map(|group| group.surfaces.iter().copied())
                    .collect::<BTreeSet<_>>();
                let exterior = boundary
                    .iter()
                    .map(|face| face.element_face_ref().unwrap())
                    .collect::<BTreeSet<_>>();
                assert_eq!(grouped_faces, exterior, "{name}");
                assert_eq!(
                    mesh.surface_sets
                        .iter()
                        .map(|group| group.surfaces.len())
                        .sum::<usize>(),
                    boundary.len(),
                    "{name}: exterior faces must belong to exactly one CAD face"
                );
                for group in &mesh.surface_sets {
                    assert!(group.name.starts_with("STEP_FACE_"), "{name}");
                    assert!(
                        mesh.node_sets.iter().any(|nodes| nodes.name == group.name),
                        "{name}"
                    );
                }
                if name == "box" {
                    let metres = directory.path().join("metres.stp");
                    let source = fs::read_to_string(&input).unwrap();
                    assert!(source.contains("SI_UNIT(.MILLI.,.METRE.)"));
                    fs::write(
                        &metres,
                        source.replace("SI_UNIT(.MILLI.,.METRE.)", "SI_UNIT($,.METRE.)"),
                    )
                    .unwrap();
                    let mesh = mesh_step(&metres, Some(2500.0)).unwrap();
                    let (min, max) = mesh.bounds().unwrap();
                    assert!((max - min).abs_diff_eq(dimensions * 1000.0, 0.01));
                }
                if name == "bracket" {
                    assert!(mesh.cached_boundary_faces().iter().any(|face| {
                        face.nodes.iter().all(|node| {
                            let point = mesh.node_position(*node).unwrap();
                            ((point.x - 18.0).hypot(point.y - 10.0) - 3.0).abs() < 1.0e-3
                        })
                    }));
                }
                eprintln!(
                    "{name}: {} nodes / {} tetrahedra",
                    mesh.nodes.len(),
                    mesh.elements.len()
                );
            } else {
                let error = result.unwrap_err().to_string();
                assert!(error.contains("exactly one solid"), "{error}");
            }
            assert_eq!(fs::read_to_string(existing_mesh).unwrap(), "existing mesh");
        }
    }
}
