use crate::{GmshError, StepMeshProgress, load_msh_file, step_process::run_step_mesher};
use fem_core::{ElementType, FemMesh};
use std::{fs, path::Path};

/// Target element sizes for meshing one STEP solid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepMeshOptions {
    /// Upper bound on the target mesh size in millimetres.
    pub max_size_mm: Option<f32>,
    /// Target number of elements per full turn on curved features. Zero disables this constraint.
    pub elements_per_turn: u32,
}

impl StepMeshOptions {
    pub const DEFAULT_ELEMENTS_PER_TURN: u32 = 20;
}

impl Default for StepMeshOptions {
    fn default() -> Self {
        Self {
            max_size_mm: None,
            elements_per_turn: Self::DEFAULT_ELEMENTS_PER_TURN,
        }
    }
}

/// Finer mesh sizing on selected CAD faces of the same STEP file.
#[derive(Debug, Clone, PartialEq)]
pub struct StepContactRefinement {
    pub surface_tags: Vec<u32>,
    pub target_size_mm: f32,
}

/// Gets the CAD surface tag from a generated STEP group name.
pub fn step_face_tag(group: &str) -> Option<u32> {
    group
        .strip_prefix("STEP_FACE_")?
        .parse::<u32>()
        .ok()
        .filter(|tag| *tag > 0)
}

/// Meshes one solid from a STEP file with first-order tetrahedra.
/// Refines the mesh around curved CAD features and grades it into the interior.
/// Coordinates and the optional upper bound on the target mesh size are in millimetres.
pub fn mesh_step(input: &Path, max_size_mm: Option<f32>) -> Result<FemMesh, GmshError> {
    mesh_step_with_progress(
        input,
        StepMeshOptions {
            max_size_mm,
            ..StepMeshOptions::default()
        },
        |_| {},
    )
}

/// Meshes one STEP solid and reports Gmsh's stages as they happen.
pub fn mesh_step_with_progress(
    input: &Path,
    options: StepMeshOptions,
    progress: impl FnMut(StepMeshProgress),
) -> Result<FemMesh, GmshError> {
    mesh_step_with_contact_refinement(input, options, None, progress)
}

/// Meshes one STEP solid, optionally refining its designated contact faces.
pub fn mesh_step_with_contact_refinement(
    input: &Path,
    options: StepMeshOptions,
    refinement: Option<&StepContactRefinement>,
    mut progress: impl FnMut(StepMeshProgress),
) -> Result<FemMesh, GmshError> {
    if let Some(refinement) = refinement
        && (!refinement.target_size_mm.is_finite()
            || refinement.target_size_mm <= 0.0
            || refinement.surface_tags.is_empty()
            || refinement.surface_tags.contains(&0))
    {
        return Err(GmshError::StepMeshing(
            "Contact refinement needs CAD faces and a finite positive target size".into(),
        ));
    }
    if options
        .max_size_mm
        .is_some_and(|size| !size.is_finite() || size <= 0.0)
    {
        return Err(GmshError::StepMeshing(
            "Maximum element size must be finite and positive".into(),
        ));
    }
    progress(StepMeshProgress::ReadingStep);
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
Mesh.MeshSizeExtendFromBoundary = 1;
"#,
    );
    script.push_str(&format!(
        "Mesh.MeshSizeFromCurvature = {};\n",
        options.elements_per_turn
    ));
    if let Some(size) = options.max_size_mm {
        script.push_str(&format!("Mesh.MeshSizeMax = {size};\n"));
    }
    if let Some(refinement) = refinement {
        let tags = refinement
            .surface_tags
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let size = refinement.target_size_mm;
        script.push_str(&format!(
            r#"contactFaces() = {{{tags}}};
missingFaces() = contactFaces();
missingFaces() -= faces();
If (#missingFaces() != 0)
  Error("A selected contact face is not on this STEP solid");
  Exit;
EndIf
Field[1] = Constant;
Field[1].SurfacesList = {{contactFaces()}};
Field[1].IncludeBoundary = 1;
Field[1].VIn = {size};
Field[1].VOut = 1e22;
Background Field = 1;
"#,
        ));
    }
    fs::write(directory.path().join("mesh.geo"), script).map_err(GmshError::Io)?;
    let mesh_path = directory.path().join("mesh.msh");
    run_step_mesher(directory.path(), &mesh_path, &mut progress)?;

    progress(StepMeshProgress::ImportingMesh);
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
    use crate::gmsh_command;

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
    fn step_contact_refinement_preserves_face_groups_and_coarse_far_surface() {
        let directory = tempfile::tempdir().unwrap();
        let input = write_step_fixture(directory.path(), "Box(1) = {0, 0, 0, 80, 40, 16};");
        let options = StepMeshOptions {
            max_size_mm: Some(8.0),
            elements_per_turn: 0,
        };
        let baseline = mesh_step_with_progress(&input, options, |_| {}).unwrap();
        let face_group_at = |x: f32| {
            baseline
                .node_sets
                .iter()
                .find(|group| {
                    !group.nodes.is_empty()
                        && group.nodes.iter().all(|node| {
                            (baseline.node_position(*node).unwrap().x - x).abs() < 1.0e-5
                        })
                })
                .unwrap()
                .name
                .as_str()
        };
        let contact = face_group_at(0.0);
        let distant = face_group_at(80.0);
        let tag = step_face_tag(contact).unwrap();
        let generate = |tag| {
            mesh_step_with_contact_refinement(
                &input,
                options,
                Some(&StepContactRefinement {
                    surface_tags: vec![tag],
                    target_size_mm: 2.0,
                }),
                |_| {},
            )
        };
        let refined = generate(tag).unwrap();
        let triangle_count = |mesh: &FemMesh, name| {
            mesh.surface_sets
                .iter()
                .find(|group| group.name == name)
                .unwrap()
                .surfaces
                .len()
        };
        let local_before = triangle_count(&baseline, contact);
        let local_after = triangle_count(&refined, contact);
        let far_before = triangle_count(&baseline, distant);
        let far_after = triangle_count(&refined, distant);
        eprintln!(
            "Surface triangles: contact {local_before} -> {local_after}, far {far_before} -> {far_after}"
        );
        assert!(local_after > 3 * local_before);
        assert!(far_after > 0 && far_after < 2 * far_before);
        let refined_nodes = refined
            .node_sets
            .iter()
            .find(|group| group.name == contact)
            .unwrap();
        assert!(!refined_nodes.nodes.is_empty());
        assert!(
            refined_nodes
                .nodes
                .iter()
                .all(|node| { refined.node_position(*node).unwrap().x.abs() < 1.0e-5 })
        );
        let invalid = generate(999_999).unwrap_err().to_string();
        assert!(
            invalid.contains("A selected contact face is not on this STEP solid"),
            "{invalid}"
        );
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
        let rim_nodes = |mesh: &FemMesh| {
            mesh.nodes
                .iter()
                .filter(|node| {
                    let point = node.position;
                    point.z.abs() < 1.0e-4
                        && ((point.x - 18.0).hypot(point.y - 20.0) - 2.0).abs() < 1.0e-4
                })
                .count()
        };
        let default_rim_nodes = rim_nodes(&auto);
        assert!(default_rim_nodes >= 16);
        let with_resolution = |elements_per_turn| StepMeshOptions {
            elements_per_turn,
            ..StepMeshOptions::default()
        };
        let unrefined = mesh_step_with_progress(&input, with_resolution(0), |_| {}).unwrap();
        let fine = mesh_step_with_progress(&input, with_resolution(40), |_| {}).unwrap();
        assert!(rim_nodes(&unrefined) < default_rim_nodes);
        assert!(rim_nodes(&fine) > default_rim_nodes);

        let mut progress = Vec::new();
        let limited = mesh_step_with_progress(
            &input,
            StepMeshOptions {
                max_size_mm: Some(4.0),
                ..StepMeshOptions::default()
            },
            |event| progress.push(event),
        )
        .unwrap();
        assert_eq!(progress.first(), Some(&StepMeshProgress::ReadingStep));
        assert!(
            progress
                .iter()
                .any(|p| matches!(p, StepMeshProgress::MeshingCurves(Some(_))))
        );
        assert!(
            progress
                .iter()
                .any(|p| matches!(p, StepMeshProgress::MeshingSurfaces(Some(_))))
        );
        assert!(progress.contains(&StepMeshProgress::MeshingVolume(None)));
        assert!(progress.contains(&StepMeshProgress::Optimizing));
        assert!(progress.contains(&StepMeshProgress::WritingMesh));
        assert_eq!(progress.last(), Some(&StepMeshProgress::ImportingMesh));
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
