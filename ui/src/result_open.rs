//! Guided, transactional result opening. File parsing runs off the UI thread.
use crate::{
    layout::SidebarPage,
    results_ui::{OpenResultButton, PlaybackState, ResultLoadError},
    slider::{SliderId, SliderState, SliderTrack},
};
use bevy::prelude::*;
use fem_core::{FemModel, FemResultSet, ResultGeometry};
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, mpsc},
};

#[derive(Resource, Default)]
pub(crate) struct ResultOpenState {
    pending: Option<Mutex<mpsc::Receiver<Result<hecmw::vtk_scene::ResultScene, String>>>>,
    path: PathBuf,
    pub status: String,
}

fn candidate_mesh(path: &Path) -> Option<PathBuf> {
    let stem = path.file_name()?.to_str()?.split(".res").next()?;
    for dir in path.parent()?.ancestors().take(4) {
        let mesh = dir.join(format!("{stem}.msh"));
        if mesh.is_file() {
            return Some(mesh);
        }
        let ctrl = dir.join("hecmw_ctrl.dat");
        if let Ok(content) = hecmw::load_hecmw_ctrl(&ctrl) {
            if let (Some(mesh), _) = hecmw::resolve_paths(&ctrl, &content) {
                if mesh.is_file() {
                    return Some(mesh);
                }
            }
        }
    }
    None
}

enum GeometryInput {
    File(PathBuf),
    Current(fem_core::FemMesh),
}
fn choose_mesh(path: &Path, model: Option<&FemModel>) -> Option<GeometryInput> {
    let candidate = candidate_mesh(path);
    let current = model.and_then(|m| {
        if m.meshes.len() == 1 && m.meshes[0].nodes != fem_core::FemMesh::demo_hex8().nodes {
            m.meshes.first()
        } else {
            None
        }
    });
    if candidate.is_some() || current.is_some() {
        let proposal = candidate
            .as_ref()
            .map_or("the currently loaded mesh".into(), |p| {
                p.display().to_string()
            });
        let choice=rfd::MessageDialog::new().set_title("Open result with its mesh")
            .set_description(format!("RES contains values but no geometry.\nUse {proposal}?\n\nYes: open together\nNo: choose another MSH\nCancel: keep current display\n\nYour editable model will not be replaced."))
            .set_buttons(rfd::MessageButtons::YesNoCancel).show();
        match choice {
            rfd::MessageDialogResult::Yes => {
                return candidate
                    .map(GeometryInput::File)
                    .or_else(|| current.cloned().map(GeometryInput::Current));
            }
            rfd::MessageDialogResult::No => {}
            _ => return None,
        }
    }
    rfd::FileDialog::new()
        .set_title("Select the matching MSH for this RES (editable model is preserved)")
        .add_filter("HEC-MW mesh", &["msh"])
        .pick_file()
        .map(GeometryInput::File)
}

pub(crate) fn open_result_button_system(
    mut buttons: Query<(Ref<Interaction>, &mut BackgroundColor), With<OpenResultButton>>,
    model: Option<Res<FemModel>>,
    mut state: ResMut<ResultOpenState>,
    mut error: ResMut<ResultLoadError>,
    mut playback: ResMut<PlaybackState>,
    mut run_results: ResMut<crate::solve_results_ui::SolveResultsState>,
) {
    for (interaction, mut background) in &mut buttons {
        *background = BackgroundColor(if state.pending.is_some() {
            Color::srgb(0.13, 0.15, 0.16)
        } else if *interaction == Interaction::None {
            Color::srgb(0.10, 0.12, 0.14)
        } else {
            Color::srgb(0.18, 0.45, 0.55)
        });
        if *interaction != Interaction::Pressed
            || !interaction.is_changed()
            || state.pending.is_some()
        {
            continue;
        }
        let Some(path) = rfd::FileDialog::new()
            .set_title("Open results: VTU/PVTU includes geometry; RES will ask for its mesh")
            .add_filter("All results (including .res.rank.step)", &["*"])
            .add_filter("VTK results", &["vtu", "pvtu"])
            .pick_file()
        else {
            continue;
        };
        let vtk = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("vtu") || e.eq_ignore_ascii_case("pvtu"));
        let input = if vtk {
            None
        } else {
            match choose_mesh(&path, model.as_deref()) {
                Some(input) => Some(input),
                None => continue,
            }
        };
        run_results.cancel_pending();
        playback.playing = false;
        error.0 = None;
        state.path = path.clone();
        state.status = format!(
            "Loading {} ...\nYour editable model is preserved.",
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        let (tx, rx) = mpsc::channel();
        state.pending = Some(Mutex::new(rx));
        std::thread::spawn(move || {
            let loaded = (|| {
                if vtk {
                    return hecmw::vtk_scene::load_scene(&path);
                }
                let mesh = match input.unwrap() {
                    GeometryInput::File(file) => {
                        hecmw::load_mesh_file(file).map_err(|e| e.to_string())?
                    }
                    GeometryInput::Current(mesh) => mesh,
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
                Ok(hecmw::vtk_scene::ResultScene {
                    model: FemModel::single_mesh("Result mesh", mesh),
                    steps: vec![steps],
                })
            })();
            let _ = tx.send(loaded);
        });
    }
}

pub(crate) fn poll_result_open(
    mut state: ResMut<ResultOpenState>,
    mut error: ResMut<ResultLoadError>,
    mut geometry: ResMut<ResultGeometry>,
    mut results: ResMut<FemResultSet>,
    mut settings: ResMut<visualization::VisualizationSettings>,
    mut page: ResMut<SidebarPage>,
    mut playback: ResMut<PlaybackState>,
    mut sliders: Query<&mut SliderState, With<SliderTrack>>,
    mut fit: ResMut<crate::project_io::CameraFitRequest>,
) {
    let Some(rx) = &state.pending else {
        return;
    };
    let reply = rx.lock().unwrap().try_recv();
    let loaded = match reply {
        Ok(v) => v,
        Err(mpsc::TryRecvError::Empty) => return,
        Err(_) => Err("Result reader stopped unexpectedly".into()),
    };
    state.pending = None;
    let scene = match loaded {
        Ok(s) if s.steps.first().is_some_and(|s| !s.is_empty()) => s,
        Ok(_) => {
            error.0 = Some("No result steps".into());
            return;
        }
        Err(e) => {
            error.0 = Some(e);
            state.status = "Result not opened; previous display retained.".into();
            return;
        }
    };
    let requested = hecmw::result_series::selected_step_number(&state.path);
    let count = scene.steps[0].len();
    let index = scene.steps[0]
        .iter()
        .position(|s| Some(s.step) == requested)
        .unwrap_or(0);
    *results = FemResultSet {
        by_mesh: scene.steps,
        active: None,
    };
    results.activate_first();
    if let Some(active) = &mut results.active {
        active.step_index = index;
    }
    let step = &results.by_mesh[0][index];
    let name = step
        .fields
        .iter()
        .find(|f| f.name().eq_ignore_ascii_case("NodalMISES"))
        .or_else(|| step.fields.first())
        .map(|f| f.name().to_string());
    let deformation = step.field_by_name("Displacement").is_some();
    if let (Some(name), Some(active)) = (name, &mut results.active) {
        active.field_name = name.clone();
        settings.contour = Some(visualization::ContourSettings {
            mesh_index: 0,
            step_index: index,
            field_name: name,
            show_deformation: deformation,
            displacement_field: "Displacement".into(),
            deformation_scale: 1.,
        });
    }
    geometry.model = Some(scene.model);
    geometry.visible = true;
    *page = SidebarPage::Results;
    playback.playing = false;
    playback.elapsed = 0.;
    for mut s in &mut sliders {
        match s.id {
            SliderId::ResultStep => {
                s.max = (count - 1) as f32;
                s.value = index as f32;
            }
            SliderId::DeformScale => s.value = 1.,
            _ => {}
        }
    }
    error.0 = None;
    fit.request();
    state.status = format!(
        "{} | {count} frames\nResult geometry only; Model returns to your editable model.",
        state.path.file_name().unwrap_or_default().to_string_lossy()
    );
}

pub(crate) fn sync_result_page(
    page: Res<SidebarPage>,
    mut geometry: ResMut<ResultGeometry>,
    mut fit: ResMut<crate::project_io::CameraFitRequest>,
    mut playback: ResMut<PlaybackState>,
) {
    let visible = *page == SidebarPage::Results;
    if visible != geometry.visible {
        if !visible {
            playback.playing = false;
        }
        geometry.visible = visible;
        if geometry.model.is_some() {
            fit.request();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opening_is_transactional_and_model_page_preserves_pre_geometry() {
        let mut app = App::new();
        let pre = FemModel::demo_hex8();
        let mut result_mesh = fem_core::FemMesh::demo_hex8();
        result_mesh.nodes[0].position.x = 10.;
        let (tx, rx) = mpsc::channel();
        tx.send(Ok(hecmw::vtk_scene::ResultScene {
            model: FemModel::single_mesh("Result", result_mesh),
            steps: vec![vec![fem_core::StepResult {
                step: 5,
                time: 1.,
                fields: vec![fem_core::ResultField::NodeScalar {
                    name: "NodalMISES".into(),
                    values: vec![1.; 8],
                    min: 1.,
                    max: 1.,
                }],
            }]],
        }))
        .unwrap();
        app.insert_resource(pre.clone())
            .insert_resource(ResultOpenState {
                pending: Some(Mutex::new(rx)),
                path: "job.0005.pvtu".into(),
                status: String::new(),
            })
            .init_resource::<ResultLoadError>()
            .init_resource::<ResultGeometry>()
            .init_resource::<FemResultSet>()
            .init_resource::<visualization::VisualizationSettings>()
            .init_resource::<PlaybackState>()
            .init_resource::<crate::project_io::CameraFitRequest>()
            .insert_resource(SidebarPage::Model)
            .add_systems(Update, (poll_result_open, sync_result_page).chain());
        app.update();
        assert_eq!(
            app.world().resource::<FemModel>().meshes[0].nodes,
            pre.meshes[0].nodes
        );
        assert_eq!(*app.world().resource::<SidebarPage>(), SidebarPage::Results);
        assert!(app.world().resource::<ResultGeometry>().visible);
        *app.world_mut().resource_mut::<SidebarPage>() = SidebarPage::Model;
        app.update();
        assert!(!app.world().resource::<ResultGeometry>().visible);
        assert_eq!(
            app.world().resource::<FemModel>().meshes[0].nodes,
            pre.meshes[0].nodes
        );
        let (tx, rx) = mpsc::channel();
        tx.send(Err("invalid data".into())).unwrap();
        app.world_mut().resource_mut::<ResultOpenState>().pending = Some(Mutex::new(rx));
        app.update();
        assert_eq!(app.world().resource::<FemResultSet>().by_mesh[0][0].step, 5);
        assert_eq!(*app.world().resource::<SidebarPage>(), SidebarPage::Model);
    }
    #[test]
    fn finds_res_mesh_in_project_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("job.msh"), "").unwrap();
        let res = dir.path().join("fstrRES/STEP1/job.res.0.1");
        assert_eq!(candidate_mesh(&res), Some(dir.path().join("job.msh")));
        assert!(candidate_mesh(&dir.path().join("unknown.res.0.1")).is_none());
    }
}
