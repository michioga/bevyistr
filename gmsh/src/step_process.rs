use crate::{GmshError, gmsh_command};
use std::{
    io::{self, BufRead, BufReader, Read},
    path::Path,
    process::Stdio,
    thread,
};

/// Stages reported while Gmsh builds a STEP mesh and bevyistr reads it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepMeshProgress {
    ReadingStep,
    /// Gmsh's progress within curve meshing, if available.
    MeshingCurves(Option<u8>),
    /// Gmsh's progress within surface meshing, if available.
    MeshingSurfaces(Option<u8>),
    /// Number of new nodes reported during tetrahedral refinement, if available.
    MeshingVolume(Option<u64>),
    Optimizing,
    WritingMesh,
    ImportingMesh,
}

pub(crate) fn run_step_mesher(
    directory: &Path,
    mesh_path: &Path,
    report: &mut impl FnMut(StepMeshProgress),
) -> Result<(), GmshError> {
    let mut child = gmsh_command()
        .current_dir(directory)
        .args([
            "mesh.geo", "-3", "-nopopup", "-format", "msh41", "-o", "mesh.msh", "-v", "4",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                GmshError::StepMeshing("Gmsh is required for STEP files. Add gmsh to PATH.".into())
            } else {
                GmshError::Io(error)
            }
        })?;
    let stdout = child.stdout.take().expect("Gmsh stdout is piped");
    let stderr = child.stderr.take().expect("Gmsh stderr is piped");
    let (status, errors) = thread::scope(|scope| -> io::Result<_> {
        // Drain both pipes so a long diagnostic stream cannot stall meshing.
        let diagnostics = scope.spawn(move || read_log(stderr, |_| {}));
        let mut previous = StepMeshProgress::ReadingStep;
        let stdout_errors = read_log(stdout, |line| {
            if let Some(stage) = progress_from_log(line, previous)
                && stage != previous
            {
                previous = stage;
                report(stage);
            }
        });
        let status = child.wait();
        let stderr_errors = diagnostics
            .join()
            .map_err(|_| io::Error::other("could not read Gmsh diagnostics"))??;
        let errors = stderr_errors
            .into_iter()
            .chain(stdout_errors?)
            .take(3)
            .collect::<Vec<_>>();
        Ok((status?, errors))
    })
    .map_err(GmshError::Io)?;

    if !errors.is_empty() {
        return Err(GmshError::StepMeshing(format!(
            "Gmsh: {}",
            errors.join(" / ")
        )));
    }
    if !status.success() || !mesh_path.is_file() {
        return Err(GmshError::StepMeshing(format!(
            "Gmsh did not produce a STEP volume mesh ({status})",
        )));
    }
    Ok(())
}

fn read_log(reader: impl Read, mut info: impl FnMut(&str)) -> io::Result<Vec<String>> {
    let mut errors = Vec::new();
    for line in BufReader::new(reader).split(b'\n') {
        let bytes = line?;
        let line = String::from_utf8_lossy(&bytes);
        let Some((level, message)) = line.split_once(':') else {
            continue;
        };
        let message = message.trim();
        match level.trim() {
            "Info" => info(message),
            "Error" if !message.is_empty() && errors.len() < 3 => errors.push(message.into()),
            _ => {}
        }
    }
    Ok(errors)
}

fn progress_from_log(message: &str, stage: StepMeshProgress) -> Option<StepMeshProgress> {
    use StepMeshProgress::*;
    if message.starts_with("Meshing 1D") {
        return Some(MeshingCurves(None));
    }
    if message.starts_with("Meshing 2D") {
        return Some(MeshingSurfaces(None));
    }
    if message.starts_with("Meshing 3D") {
        return Some(MeshingVolume(None));
    }
    if message.starts_with("Optimizing mesh") {
        return Some(Optimizing);
    }
    if message.starts_with("Writing ") {
        return Some(WritingMesh);
    }
    if let MeshingVolume(previous) = stage
        && let Some((_, nodes)) = message.strip_prefix("It. ")?.split_once(" - ")
    {
        let (nodes, _) = nodes.split_once(" nodes created")?;
        let nodes = nodes.trim().parse::<u64>().ok()?;
        return Some(MeshingVolume(Some(previous.unwrap_or(0).max(nodes))));
    }
    let (percent, operation) = message.strip_prefix('[')?.split_once("%]")?;
    let percent = percent.trim().parse::<u8>().ok().filter(|p| *p <= 100)?;
    match stage {
        MeshingCurves(previous) if operation.trim_start().starts_with("Meshing curve ") => {
            Some(MeshingCurves(Some(previous.unwrap_or(0).max(percent))))
        }
        MeshingSurfaces(previous) if operation.trim_start().starts_with("Meshing surface ") => {
            Some(MeshingSurfaces(Some(previous.unwrap_or(0).max(percent))))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_meshing_progress_reports_only_metrics_for_the_current_stage() {
        use StepMeshProgress::*;

        let iteration = "It. 1500 - 1475 nodes created - worst tet radius 2.1 (nodes removed 2 0)";
        assert_eq!(
            progress_from_log(iteration, MeshingVolume(None)),
            Some(MeshingVolume(Some(1475)))
        );
        assert_eq!(
            progress_from_log(iteration, MeshingSurfaces(Some(90))),
            None
        );
        assert_eq!(
            progress_from_log("Done meshing 3D", MeshingVolume(Some(1475))),
            None
        );

        let curve = "[ 10%] Meshing curve 3 (Line)";
        assert_eq!(
            progress_from_log(curve, MeshingCurves(Some(20))),
            Some(MeshingCurves(Some(20)))
        );
        assert_eq!(progress_from_log(curve, MeshingSurfaces(None)), None);
    }
}
