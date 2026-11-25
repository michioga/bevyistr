//! Guided, transactional result opening. File parsing runs off the UI thread.
use crate::ButtonInteraction;
use crate::{
    layout::SidebarPage,
    result_loading::{MeshInput, ResultInput, ResultLoadTask},
    result_requests::{ResultRequestId, ResultRequests},
    results_ui::{OpenResultButton, PlaybackState, ResultLoadError},
    slider::{SliderId, SliderState, SliderTrack},
};
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use fem_core::{FemModel, FemResultSet, ResultGeometry};
use hecmw::result_series::{FrameSelection, detect_result_series, selected_step_number};
use std::path::{Path, PathBuf};

pub(crate) const LOAD_FULL_RES_SERIES_LABEL: &str = "Load full RES series";

#[derive(Resource, Default)]
pub(crate) struct ResultOpenState {
    pending: Option<ResultLoadTask>,
    request: Option<ResultRequestId>,
    path: PathBuf,
    frames: Option<FrameSelection>,
    preview_path: Option<PathBuf>,
    expanding_preview: bool,
    pub status: String,
}

impl ResultOpenState {
    pub(crate) fn begin(
        &mut self,
        path: PathBuf,
        task: ResultLoadTask,
        frames: Option<FrameSelection>,
        requests: &mut ResultRequests,
    ) {
        self.request = Some(requests.begin());
        self.status = format!(
            "Loading {} ...\nYour editable model is preserved.",
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        self.path = path;
        self.frames = frames;
        self.expanding_preview = false;
        self.pending = Some(task);
    }

    fn expandable_preview(&self, requests: &ResultRequests) -> Option<&Path> {
        if self.pending.is_some() || !self.request.is_some_and(|id| requests.is_current(id)) {
            return None;
        }
        self.preview_path.as_deref()
    }
}

#[derive(Component)]
pub(crate) struct OpenResultRequested;

#[derive(Component)]
pub(crate) struct LoadFullResSeriesButton;

pub(crate) fn sync_load_full_res_series(
    state: Res<ResultOpenState>,
    requests: Res<ResultRequests>,
    mut buttons: Query<&mut Node, With<LoadFullResSeriesButton>>,
) {
    let display = if state.expandable_preview(&requests).is_some() {
        Display::Flex
    } else {
        Display::None
    };
    for mut button in &mut buttons {
        if button.display != display {
            button.display = display;
        }
    }
}

pub(crate) fn load_full_res_series(
    _: On<Activate>,
    page: Res<SidebarPage>,
    geometry: Res<ResultGeometry>,
    mut state: ResMut<ResultOpenState>,
    mut requests: ResMut<ResultRequests>,
    mut playback: ResMut<PlaybackState>,
    mut error: ResMut<ResultLoadError>,
) {
    if *page != SidebarPage::Results {
        return;
    }
    let Some(path) = state.expandable_preview(&requests).map(Path::to_path_buf) else {
        return;
    };
    let Some([mesh]) = geometry.model.as_ref().map(|model| model.meshes.as_slice()) else {
        return;
    };
    let input = ResultInput::WithMesh {
        path: path.clone(),
        mesh: MeshInput::Current(mesh.clone()),
        frames: FrameSelection::All,
    };
    match ResultLoadTask::spawn(input) {
        Ok(task) => {
            playback.playing = false;
            error.0 = None;
            state.begin(path, task, Some(FrameSelection::All), &mut requests);
            state.expanding_preview = true;
        }
        Err(message) => error.0 = Some(message),
    }
}

/// Completed click or keyboard activation opens the dialog.
/// Menu items are separate from this button and must never open the dialog.
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

fn choose_native_frames(path: &Path) -> Option<FrameSelection> {
    let Some(step) = selected_step_number(path) else {
        return Some(FrameSelection::All);
    };
    let count = detect_result_series(path).len();
    if count <= 1 {
        return Some(FrameSelection::All);
    }

    let selected = "Selected frame";
    let all = LOAD_FULL_RES_SERIES_LABEL;
    let choice = rfd::MessageDialog::new()
        .set_title("Open RES results")
        .set_description(format!(
            "Selected frame: view Step {step} only.\n{all}: load {count} frames to use Play and the seek bar."
        ))
        .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
            selected.into(),
            all.into(),
            "Cancel".into(),
        ))
        .show();
    match choice {
        rfd::MessageDialogResult::Yes => Some(FrameSelection::Selected),
        rfd::MessageDialogResult::No => Some(FrameSelection::All),
        rfd::MessageDialogResult::Custom(name) if name == selected => Some(FrameSelection::Selected),
        rfd::MessageDialogResult::Custom(name) if name == all => Some(FrameSelection::All),
        _ => None,
    }
}

pub(crate) fn open_result_button_system(
    mut buttons: Query<
        (
            Entity,
            &ButtonInteraction,
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
    mut requests: ResMut<ResultRequests>,
) {
    for (entity, interaction, mut background, requested) in &mut buttons {
        *background = BackgroundColor(if state.pending.is_some() {
            Color::srgb(0.13, 0.15, 0.16)
        } else if *interaction == ButtonInteraction::None {
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
        if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("inp")) {
            error.0 = Some("MicroAVS .inp results cannot be displayed. Choose a .res.rank.step file from the same run, or a VTU/PVTU result.".into());
            state.status = "Result not opened; previous display retained.".into();
            continue;
        }
        let vtk = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("vtu") || e.eq_ignore_ascii_case("pvtu"));
        let (input, frames) = if vtk {
            (ResultInput::Vtk(path.clone()), None)
        } else {
            let Some(frames) = choose_native_frames(&path) else {
                continue;
            };
            let Some(mesh) = choose_mesh(&path, model.as_deref()) else {
                continue;
            };
            (
                ResultInput::WithMesh {
                    path: path.clone(),
                    mesh,
                    frames,
                },
                Some(frames),
            )
        };
        let pending = match ResultLoadTask::spawn(input) {
            Ok(pending) => pending,
            Err(message) => {
                error.0 = Some(message);
                state.status = "Result not opened; previous display retained.".into();
                continue;
            }
        };
        playback.playing = false;
        error.0 = None;
        state.begin(path, pending, frames, &mut requests);
    }
}

pub(crate) fn poll_result_open(
    mut state: ResMut<ResultOpenState>,
    requests: Res<ResultRequests>,
    mut error: ResMut<ResultLoadError>,
    mut geometry: ResMut<ResultGeometry>,
    mut results: ResMut<FemResultSet>,
    mut settings: ResMut<visualization::VisualizationSettings>,
    mut page: ResMut<SidebarPage>,
    mut playback: ResMut<PlaybackState>,
    mut sliders: Query<&mut SliderState, With<SliderTrack>>,
    mut fit: ResMut<crate::project_io::CameraFitRequest>,
) {
    if state
        .request
        .is_some_and(|request| !requests.is_current(request))
    {
        state.pending = None;
        state.request = None;
        state.frames = None;
        state.preview_path = None;
        // Discard the old request's status too, so an analysis handoff cannot
        // retain the previous manual filename in the result description.
        state.status.clear();
        return;
    }
    let Some(task) = &state.pending else {
        return;
    };
    let Some(loaded) = task.poll() else {
        if let Some(progress) = task.progress() {
            let status = format!(
                "Loading {} ...\nRES frames: {} / {}",
                state.path.file_name().unwrap_or_default().to_string_lossy(),
                progress.completed,
                progress.total
            );
            if state.status != status {
                state.status = status;
            }
        }
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
    let requested = selected_step_number(&state.path);
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
    let previous_contour = settings.contour.as_ref().filter(|contour| {
        state.expanding_preview && step.field_by_name(&contour.field_name).is_some()
    });
    let name = previous_contour
        .map(|contour| contour.field_name.clone())
        .or_else(|| {
            step.fields
                .iter()
                .find(|field| field.name().eq_ignore_ascii_case("NodalMISES"))
                .or_else(|| step.fields.first())
                .map(|field| field.name().to_string())
        });
    let deformation = step.field_by_name("Displacement").is_some();
    if let (Some(name), Some(active)) = (name, &mut results.active) {
        active.field_name = name.clone();
        let mut contour = previous_contour.cloned().unwrap_or(visualization::ContourSettings {
            mesh_index: 0,
            step_index: index,
            field_name: name,
            show_deformation: deformation,
            displacement_field: "Displacement".into(),
            deformation_scale: 1.,
        });
        contour.step_index = index;
        settings.contour = Some(contour);
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
            SliderId::DeformScale if !state.expanding_preview => s.value = 1.,
            _ => {}
        }
    }
    error.0 = None;
    if !state.expanding_preview {
        fit.request();
    }
    state.preview_path = (state.frames == Some(FrameSelection::Selected))
        .then(|| state.path.clone());
    state.status = format!(
        "{} | {count} frames\nResult geometry only; Model returns to your editable model.",
        state.path.file_name().unwrap_or_default().to_string_lossy()
    );
    if state.preview_path.is_some() {
        state.status.push_str("\nSelected frame only.");
    }
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
