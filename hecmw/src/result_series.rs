//! Discover temporal files without confusing a VTK/MPI rank with a step.
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq)]
enum Pattern {
    Native { base: String },
    Vtk { base: String, extension: String },
}

fn pattern(name: &str) -> Option<(Pattern, u32)> {
    let (stem, extension) = name.rsplit_once('.')?;
    if extension.eq_ignore_ascii_case("pvtu") || extension.eq_ignore_ascii_case("vtu") {
        let (base, step) = stem.rsplit_once('.')?;
        return Some((
            Pattern::Vtk {
                base: base.into(),
                extension: extension.to_ascii_lowercase(),
            },
            step.parse().ok()?,
        ));
    }
    let (_, rank) = stem.rsplit_once(".res.")?;
    rank.parse::<u32>().ok()?;
    Some((
        Pattern::Native { base: stem.into() },
        extension.parse().ok()?,
    ))
}

/// The requested output number, not its ordinal in the sparse timeline.
pub fn selected_step_number(path: &Path) -> Option<u32> {
    let name = path.file_name()?.to_str()?;
    if path
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("vtu"))
    {
        let folder = path.parent()?.file_name()?.to_str()?;
        if name
            .strip_prefix(&format!("{folder}."))
            .and_then(|s| s.strip_suffix(".vtu"))
            .is_some_and(|s| s.parse::<u32>().is_ok())
        {
            return folder.rsplit('.').next()?.parse().ok();
        }
    }
    pattern(name).map(|(_, step)| step)
}

/// Sparse output numbers are sorted numerically, never assumed contiguous.
/// FrontISTR piece VTUs prefer their PVTU wrapper when it references this
/// exact piece. Rank files must never be mistaken for time steps.
pub fn detect_result_series(path: &Path) -> Vec<(u32, PathBuf)> {
    if path
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("vtu"))
    {
        if let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|s| s.to_str()))
        {
            if let Some(folder) = dir.file_name().and_then(|s| s.to_str()) {
                if name
                    .strip_prefix(&format!("{folder}."))
                    .and_then(|s| s.strip_suffix(".vtu"))
                    .is_some_and(|s| s.parse::<u32>().is_ok())
                {
                    let wrapper = dir.with_file_name(format!("{folder}.pvtu"));
                    if let Ok(text) = std::fs::read_to_string(&wrapper) {
                        let points_to_piece = text
                            .split("Source=\"")
                            .skip(1)
                            .filter_map(|s| s.split('"').next())
                            .any(|s| {
                                wrapper
                                    .parent()
                                    .unwrap_or(Path::new("."))
                                    .join(s)
                                    .canonicalize()
                                    .ok()
                                    .zip(path.canonicalize().ok())
                                    .is_some_and(|(a, b)| a == b)
                            });
                        if points_to_piece {
                            return detect_result_series(&wrapper);
                        }
                    }
                    return vec![(
                        folder
                            .rsplit('.')
                            .next()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0),
                        path.into(),
                    )];
                }
            }
        }
    }
    let Some((expected, selected_step)) =
        path.file_name().and_then(|s| s.to_str()).and_then(pattern)
    else {
        return vec![(0, path.into())];
    };
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut candidates = Vec::new();
    let mut directories = vec![parent.to_owned()];
    if matches!(expected, Pattern::Native { .. })
        && parent.file_name().and_then(|s| s.to_str())
            == Some(format!("STEP{selected_step}").as_str())
    {
        if let Some(root) = parent.parent() {
            if let Ok(entries) = root.read_dir() {
                directories = entries
                    .flatten()
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                    .filter(|e| {
                        e.file_name()
                            .to_str()
                            .and_then(|s| s.strip_prefix("STEP"))
                            .is_some_and(|s| s.parse::<u32>().is_ok())
                    })
                    .map(|e| e.path())
                    .collect();
            }
        }
    }
    for dir in directories {
        if let Ok(entries) = dir.read_dir() {
            for entry in entries
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            {
                if let Some((found, step)) = entry.file_name().to_str().and_then(pattern) {
                    if found == expected {
                        candidates.push((step, entry.path()));
                    }
                }
            }
        }
    }
    candidates.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    if candidates.is_empty() {
        vec![(selected_step, path.into())]
    } else {
        candidates
    }
}

/// Do not install a partially loaded series after a failure.
pub fn load_mesh_result_series(
    path: &Path,
    mesh: &fem_core::FemMesh,
) -> Result<Vec<fem_core::StepResult>, String> {
    let files = detect_result_series(path);
    if files.windows(2).any(|p| p[0].0 == p[1].0) {
        return Err("Ambiguous duplicate result steps".into());
    }
    files
        .iter()
        .map(|(number, file)| {
            let result = if file
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("vtu") || s.eq_ignore_ascii_case("pvtu"))
            {
                crate::vtu::load_vtu_for_mesh(file, mesh)
                    .map(|mut step| {
                        step.step = *number;
                        step
                    })
                    .map_err(|e| e.to_string())
            } else {
                std::fs::read_to_string(file)
                    .map_err(|e| e.to_string())
                    .and_then(|source| {
                        if source.trim_start().starts_with("*fstrresult") {
                            crate::native_result::NativeResult::parse(source.trim_start())?
                                .step_for_mesh(mesh, *number)
                        } else {
                            let nodes: Vec<_> = mesh.nodes.iter().map(|n| n.id).collect();
                            crate::load_result_file(file, &nodes).map_err(|e| e.to_string())
                        }
                    })
            };
            result.map_err(|e| format!("{}: {e}", file.display()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::Vec3;
    use fem_core::{FemMesh, FemNode, NodeId, ResultField};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "bevyistr-series-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir(&dir).unwrap();
            Self(dir)
        }
        fn file(&self, name: &str, content: &str) -> PathBuf {
            let path = self.0.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, content).unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn discovery_sorts_sparse_steps_and_keeps_ranks_separate() {
        let f = Fixture::new();
        let path = f.file("job.res.0.10", "");
        f.file("job.res.0.2", "");
        f.file("job.res.1.1", "");
        f.file("other.res.0.0", "");
        assert_eq!(
            detect_result_series(&path)
                .iter()
                .map(|(n, _)| *n)
                .collect::<Vec<_>>(),
            vec![2, 10]
        );
        let piece = f.file("job_psf.0010/job_psf.0010.0.vtu", "");
        f.file("job_psf.0010/job_psf.0010.1.vtu", "");
        assert_eq!(detect_result_series(&piece), vec![(10, piece.clone())]);
        f.file(
            "job_psf.0010.pvtu",
            "<Piece Source=\"./job_psf.0010/job_psf.0010.0.vtu\"/>",
        );
        f.file("job_psf.0002.pvtu", "");
        assert_eq!(
            detect_result_series(&piece)
                .iter()
                .map(|(n, _)| *n)
                .collect::<Vec<_>>(),
            vec![2, 10]
        );
        let path = f.file("res/STEP10/job.res.0.10", "");
        f.file("res/STEP0/job.res.0.0", "");
        assert_eq!(
            detect_result_series(&path)
                .iter()
                .map(|(n, _)| *n)
                .collect::<Vec<_>>(),
            vec![0, 10]
        );
    }

    #[test]
    fn native_ids_are_not_file_order_and_only_unused_nodes_may_be_missing() {
        let mut mesh = FemMesh::demo_hex8();
        mesh.nodes.insert(1, FemNode::new(NodeId(999), Vec3::ZERO));
        let records = (0..8)
            .rev()
            .map(|i| format!("{i} {}", i + 5))
            .collect::<Vec<_>>()
            .join("\n");
        let source = format!("*fstrresult\n8 1\n1 0\n1\nTEMP\n{records}\n");
        let native = crate::native_result::NativeResult::parse(&source).unwrap();
        let step = native.step_for_mesh(&mesh, 10).unwrap();
        let ResultField::NodeScalar {
            values, min, max, ..
        } = &step.fields[0]
        else {
            panic!()
        };
        assert_eq!((*min, *max), (5., 12.));
        assert_eq!(values[0], 5.);
        assert!(values[1].is_nan());
        assert_eq!(values[2], 6.);
        mesh.elements[0].nodes.push(NodeId(999));
        assert!(native.step_for_mesh(&mesh, 10).is_err());
    }

    #[test]
    #[ignore = "Requires BEVYISTR_TUTORIAL_DIR; read-only actual outputs"]
    fn tutorial_result_series_native_matches_vtk() {
        let root = PathBuf::from(std::env::var_os("BEVYISTR_TUTORIAL_DIR").unwrap());
        for (dir, stem, native, vtk) in [
            (
                "01_elastic_hinge",
                "hinge",
                "hinge.res.0.1",
                "hinge_vis_psf.0001.pvtu",
            ),
            (
                "19_conrod",
                "conrod",
                "fstrRES/STEP1/conrod.res.0.1",
                "vis_out/conrod_psf.0001.pvtu",
            ),
        ] {
            let mesh = crate::load_mesh_file(root.join(dir).join(format!("{stem}.msh"))).unwrap();
            let a = load_mesh_result_series(&root.join(dir).join(native), &mesh).unwrap();
            let b = load_mesh_result_series(&root.join(dir).join(vtk), &mesh).unwrap();
            assert_eq!(a.len(), 2);
            assert_eq!(b.len(), 2);
            for (a, b) in a.iter().zip(&b) {
                assert_eq!(a.step, b.step);
                assert!((a.time - b.time).abs() < 1e-6);
                for field in &a.fields {
                    if let ResultField::NodeScalar { name, values, .. } = field {
                        let Some(ResultField::NodeScalar { values: other, .. }) =
                            b.field_by_name(name)
                        else {
                            panic!("Missing {name}")
                        };
                        assert_eq!(values.len(), other.len());
                        for (i, (a, b)) in values.iter().zip(other).enumerate() {
                            assert!(
                                (a.is_nan() && b.is_nan())
                                    || (a - b).abs() <= 2e-5 * a.abs().max(1.),
                                "{dir} {name} node index {i}: {a}/{b}"
                            );
                        }
                    }
                }
            }
        }
        let paths = detect_result_series(&root.join("12_dynamic_beam/beam.res.0.5000"));
        assert!(paths.len() > 2);
        assert!(paths.windows(2).all(|p| p[0].0 < p[1].0));
        let mesh = crate::load_mesh_file(root.join("12_dynamic_beam/beam.msh")).unwrap();
        let series = load_mesh_result_series(&paths[0].1, &mesh).unwrap();
        assert_eq!(series.len(), paths.len());
        assert!(series.windows(2).all(|p| p[0].time < p[1].time));
    }
}
