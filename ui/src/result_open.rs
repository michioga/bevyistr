//! Guided, transactional result opening. File parsing runs off the UI thread.
use crate::{
    layout::SidebarPage,
    result_loading::{MeshInput, ResultInput, ResultLoadTask},
    results_ui::{OpenResultButton, PlaybackState, ResultLoadError},
    slider::{SliderId, SliderState, SliderTrack},
};
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use fem_core::{FemModel, FemResultSet, ResultGeometry};
use std::path::{Path, PathBuf};

#[derive(Resource, Default)]
pub(crate) struct ResultOpenState {
    pending: Option<ResultLoadTask>,
    path: PathBuf,
    pub status: String,
}

#[derive(Component)]
pub(crate) struct OpenResultRequested;

/// Legacy UI Interaction can report Pressed on a control exposed by a closing
/// popup. Only the picking widget's completed click/keyboard activation may
/// request a dialog; menu items are not descendants of this button.
pub(crate) fn request_result_open(
    event: On<Activate>,
    buttons: Query<(), With<OpenResultButton>>,
    page: Res<SidebarPage>,
    state: Res<ResultOpenState>,
    mut commands: Commands,
) {
    if *page == SidebarPage::Results && state.pending.is_none() && buttons.contains(event.entity) {
        commands.entity(event.entity).insert(OpenResultRequested);
    }
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

fn choose_mesh(path: &Path, model: Option<&FemModel>) -> Option<MeshInput> {
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
                    .map(MeshInput::File)
                    .or_else(|| current.cloned().map(MeshInput::Current));
            }
            rfd::MessageDialogResult::No => {}
            _ => return None,
        }
    }
    rfd::FileDialog::new()
        .set_title("Select the matching MSH for this RES (editable model is preserved)")
        .add_filter("HEC-MW mesh", &["msh"])
        .pick_file()
        .map(MeshInput::File)
}

pub(crate) fn open_result_button_system(
    mut buttons: Query<
        (
            Entity,
            &Interaction,
            &mut BackgroundColor,
            Has<OpenResultRequested>,
        ),
        With<OpenResultButton>,
    >,
    mut commands: Commands,
    page: Res<SidebarPage>,
    model: Option<Res<FemModel>>,
    mut state: ResMut<ResultOpenState>,
    mut error: ResMut<ResultLoadError>,
    mut playback: ResMut<PlaybackState>,
    mut run_results: ResMut<crate::solve_results_ui::SolveResultsState>,
) {
    for (entity, interaction, mut background, requested) in &mut buttons {
        *background = BackgroundColor(if state.pending.is_some() {
            Color::srgb(0.13, 0.15, 0.16)
        } else if *interaction == Interaction::None {
            Color::srgb(0.10, 0.12, 0.14)
        } else {
            Color::srgb(0.18, 0.45, 0.55)
        });
        if !requested {
            continue;
        }
        commands.entity(entity).remove::<OpenResultRequested>();
        if state.pending.is_some() || *page != SidebarPage::Results {
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
            ResultInput::Vtk(path.clone())
        } else {
            match choose_mesh(&path, model.as_deref()) {
                Some(mesh) => ResultInput::WithMesh {
                    path: path.clone(),
                    mesh,
                },
                None => continue,
            }
        };
        let pending = match ResultLoadTask::spawn(input) {
            Ok(pending) => pending,
            Err(message) => {
                error.0 = Some(message);
                state.status = "Result not opened; previous display retained.".into();
                continue;
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
        state.pending = Some(pending);
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
    let Some(task) = &state.pending else {
        return;
    };
    let Some(loaded) = task.poll() else {
        return;
    };
    state.pending = None;
    let scene = match loaded {
        Ok(s) => s,
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
    let piece_count = scene.model.meshes.len();
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
    if piece_count > 1 {
        state.status.push_str(&format!("\n{piece_count} VTK pieces | local IDs; common color range.\nPartitions are not welded. Unmarked overlaps / internal boundaries remain."));
    }
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
#[path = "result_open_tests.rs"]
mod tests;
