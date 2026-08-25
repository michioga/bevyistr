use bevy::prelude::*;
use std::path::PathBuf;

pub mod boundary;
pub mod contact;
pub mod connected;
pub mod model;
pub mod planar;
pub mod result;
pub mod spatial;

pub use boundary::*;
pub use contact::*;
pub use connected::{
    DEFAULT_FEATURE_EDGE_ANGLE_DEG, expand_connected_boundary_edges,
    expand_connected_boundary_faces, expand_connected_elements, expand_connected_feature_edges,
    expand_continuous_feature_edges,
};
pub use model::*;
pub use planar::{
    expand_coplanar_from_element, expand_coplanar_from_face, expand_smooth_from_element,
    expand_smooth_from_face,
};
pub use result::*;
pub use spatial::*;

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InteractionMode {
    #[default]
    Idle,

    Orbit,

    BoxSelect,

    Pan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SelectionLevel {
    Node,

    Edge,

    Face,

    #[default]
    Element,
}

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SelectionFilter {
    pub level: SelectionLevel,
}

impl SelectionFilter {
    pub const fn new(level: SelectionLevel) -> Self {
        Self { level }
    }

    pub fn accepts(&self, level: SelectionLevel) -> bool {
        self.level == level
    }
}

#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct UiPointerState {
    pub over_ui: bool,
}

#[derive(Resource, Debug, Clone, Default)]
pub struct MeshLoadRequest {
    pub path: Option<PathBuf>,

    /// `true` to add the loaded mesh as a new [`Part`] alongside the
    /// existing model (see [`FemModel::add_mesh`]); `false` to replace the
    /// whole model (see [`FemModel::single_mesh`]).
    pub import: bool,
}

impl MeshLoadRequest {
    /// Requests loading `path`, replacing the current model.
    pub fn request(&mut self, path: PathBuf) {
        self.path = Some(path);
        self.import = false;
    }

    /// Requests loading `path` and adding it as a new part of the current
    /// model, rather than replacing it.
    pub fn request_import(&mut self, path: PathBuf) {
        self.path = Some(path);
        self.import = true;
    }

    /// Takes the pending path along with whether it should be imported
    /// (added) or used to replace the model.
    pub fn take(&mut self) -> Option<(PathBuf, bool)> {
        self.path.take().map(|path| (path, self.import))
    }
}

#[derive(Resource, Debug, Clone, Default)]
pub struct MeshLoadStatus {
    pub last_path: Option<PathBuf>,

    pub message: String,

    pub error: Option<String>,
}

impl MeshLoadStatus {
    pub fn loading(&mut self, path: PathBuf) {
        self.last_path = Some(path);
        self.message = "Loading mesh".to_string();
        self.error = None;
    }

    pub fn loaded(&mut self, path: PathBuf) {
        self.last_path = Some(path);
        self.message = "Mesh loaded".to_string();
        self.error = None;
    }

    pub fn failed(&mut self, path: PathBuf, error: impl Into<String>) {
        self.last_path = Some(path);
        self.message = "Mesh load failed".to_string();
        self.error = Some(error.into());
    }
}

#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct FemModelVersion {
    pub value: u64,
}

impl FemModelVersion {
    pub fn bump(&mut self) {
        self.value = self.value.saturating_add(1);
    }
}

/// The entities that would be added to the selection if the person
/// clicked on the current hover target right now.
///
/// Usually just the hovered entity itself, but expanded to a connected
/// Coplanar or Smooth surface group when surface growth is active. It is
/// computed each frame by a system in the `ui` crate (which owns the growth
/// mode and angle slider) and consumed by `visualization`'s hover highlight, so the
/// preview accurately reflects what a single click would select rather
/// than just the single facet under the cursor. Multi-click gestures may
/// expand this group further. Living in `fem_core`
/// rather than `ui` or `visualization` lets both depend on it without
/// `visualization` needing a (backwards) dependency on `ui`.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct HoverPreviewTargets {
    /// FEM entities that will be committed when the pointer is clicked.
    pub targets: Vec<FemEntityRef>,

    /// Geometry used only for the hover overlay. This normally matches
    /// `targets`, but an Element surface-growth preview stores element IDs in
    /// `targets` while drawing the boundary Face patch here. That prevents
    /// internal tetrahedron faces from looking like accidentally selected
    /// edges around an otherwise flat patch.
    pub highlight_targets: Vec<FemEntityRef>,
}

/// A `.cnt` file queued to be merged into [`AnalysisSetup`] once a mesh
/// that's loading *concurrently* (via [`MeshLoadRequest`]) has actually
/// finished — needed because node/element/surface group names in the
/// `.cnt` file can only be resolved once that mesh exists.
///
/// [`MeshLoadRequest`] is consumed by a system that may run on a later
/// frame than the one that queued it (mesh loading is a distinct
/// request/poll step, not an immediate call), so a naive "read the current
/// model" approach when queueing the `.cnt` load would race the mesh load
/// and either resolve groups against the *previous* model or find no model
/// at all. Recording [`FemModelVersion::value`] at queue time and only
/// applying once the version has advanced past it guarantees the mesh is
/// actually ready, regardless of system ordering.
#[derive(Resource, Debug, Clone, Default)]
pub struct PendingCntLoad {
    pub path: Option<PathBuf>,

    /// Index into [`FemModel::meshes`] the `.cnt` file's groups should be
    /// resolved against once it's ready to load.
    pub mesh_index: usize,

    /// [`FemModelVersion::value`] at the time this request was queued; the
    /// load is applied once the live version is greater than this.
    pub after_version: u64,
}

impl PendingCntLoad {
    /// Queues `path` to be merged into `AnalysisSetup` for `mesh_index`
    /// once [`FemModelVersion`] advances past `current_version` (the
    /// version read at the moment the concurrent mesh load was requested).
    pub fn request(&mut self, path: PathBuf, mesh_index: usize, current_version: u64) {
        self.path = Some(path);
        self.mesh_index = mesh_index;
        self.after_version = current_version;
    }

    /// Takes the pending `(path, mesh_index)` if the mesh it depends on has
    /// finished loading (`current_version > after_version`); leaves the
    /// request in place otherwise so it can be checked again next frame.
    pub fn take_if_ready(&mut self, current_version: u64) -> Option<(PathBuf, usize)> {
        if self.path.is_some() && current_version > self.after_version {
            let mesh_index = self.mesh_index;
            self.path.take().map(|path| (path, mesh_index))
        } else {
            None
        }
    }
}
