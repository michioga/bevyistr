//! Result I/O and worker completion, independent of dialogs and ECS/UI updates.
use fem_core::{FemMesh, FemModel};
use hecmw::vtk_scene::ResultScene;
use std::{
    path::PathBuf,
    sync::{Mutex, mpsc},
};

pub(crate) type Loaded = Result<ResultScene, String>;

pub(crate) enum MeshInput {
    File(PathBuf),
    Current(FemMesh),
}

pub(crate) enum ResultInput {
    Vtk(PathBuf),
    WithMesh { path: PathBuf, mesh: MeshInput },
}

impl ResultInput {
    pub fn load(self) -> Loaded {
        match self {
            Self::Vtk(path) => hecmw::vtk_scene::load_scene(&path),
            Self::WithMesh { path, mesh } => {
                let mesh = match mesh {
                    MeshInput::File(path) => {
                        hecmw::load_mesh_file(path).map_err(|e| e.to_string())?
                    }
                    MeshInput::Current(mesh) => mesh,
                };
                let steps = if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("frd"))
                {
                    hecmw::load_frd_file(
                        &path,
                        &mesh.nodes.iter().map(|n| n.id).collect::<Vec<_>>(),
                    )
                    .map_err(|e| e.to_string())?
                } else {
                    hecmw::result_series::load_mesh_result_series(&path, &mesh)?
                };
                Ok(ResultScene {
                    model: FemModel::single_mesh("Result mesh", mesh),
                    steps: vec![steps],
                })
            }
        }
    }
}

pub(crate) struct ResultLoadTask {
    receiver: Mutex<mpsc::Receiver<Loaded>>,
}

impl ResultLoadTask {
    pub fn spawn(input: ResultInput) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("bevyistr-result-reader".into())
            .spawn(move || {
                let _ = tx.send(input.load());
            })
            .map_err(|e| format!("Could not start result reader: {e}"))?;
        Ok(Self {
            receiver: Mutex::new(rx),
        })
    }

    /// None means still loading. Every terminal reply is validated before the
    /// UI can replace its current geometry, values or timeline.
    pub fn poll(&self) -> Option<Loaded> {
        let receiver = match self.receiver.lock() {
            Ok(receiver) => receiver,
            Err(_) => return Some(Err("Result reader state unavailable".into())),
        };
        match receiver.try_recv() {
            Ok(loaded) => Some(loaded.and_then(validate_scene)),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("Result reader stopped unexpectedly".into()))
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn from_receiver(receiver: mpsc::Receiver<Loaded>) -> Self {
        Self {
            receiver: Mutex::new(receiver),
        }
    }
}

fn validate_scene(scene: ResultScene) -> Loaded {
    let count = scene
        .steps
        .first()
        .filter(|steps| !steps.is_empty())
        .ok_or("No result steps")?
        .len();
    if scene.model.meshes.len() != scene.steps.len() {
        return Err("Result geometry and piece count disagree".into());
    }
    if scene.steps.iter().any(|steps| steps.len() != count) {
        return Err("Result pieces have different frame counts".into());
    }
    if scene
        .steps
        .iter()
        .flatten()
        .any(|step| step.fields.is_empty())
    {
        return Err("No display fields in a result frame".into());
    }
    Ok(scene)
}

#[cfg(test)]
#[path = "result_loading_tests.rs"]
mod tests;
